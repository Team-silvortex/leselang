//! Closed multi-function catalog compilation without native template copies.

use std::{
    borrow::Borrow,
    cell::{Cell, RefCell},
    collections::HashSet,
    convert::Infallible,
    fmt,
};

use leselang_runtime_core::{
    NamedParameter, ScalarArgumentDomain, ScalarType, bind_named_arguments,
};
use leselang_syntax::{Expression, Function, MAX_FUNCTION_PARAMETERS, MAX_FUNCTIONS, Span};

use crate::catalog_group_source::{CatalogGroupSourceLimits, cold_groups_with};
use crate::catalog_source::{
    CatalogSourceEnvironment, CatalogSourceError, CatalogSourceLimits, CatalogSourceResult,
    cold_catalog, lower_catalog_expression,
};
use crate::control_source::{preflight_closed_control_source, preflight_control_source};
use crate::flow_typing::CallFlowTypeError;
use crate::helper_bindings::{
    HelperBinding, HelperBindingError, HelperBindingLimits, bind_helper_arguments,
};
use crate::helper_declarations::{
    HelperDeclarationError, HelperDeclarationLimits, accept_helper_declarations_profile,
};
use crate::helper_dependencies::{
    HelperDependencyError, HelperDependencyLimits, helper_dependency_order,
};
use crate::helper_hygiene::{HelperHygieneError, HelperHygieneLimits, hygienic_helper_body};
use crate::helper_source::{
    HelperParameterError, HelperSourceError, HelperSourceLimits, helper_parameters,
    helper_parameters_with, lower_typed_helper_arguments, scalar_type_name,
};
use crate::ir::Computation;
use crate::pure_typing::{
    MAX_TYPE_INFERENCE_DEPTH, MAX_TYPE_INFERENCE_NODES, PureType, valid_local_name,
};
use crate::scalar_source::primitive_name;
use crate::source_call::{SourceCallHost, span};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CatalogProgramLimits {
    pub expression: CatalogSourceLimits,
    pub max_functions: usize,
    pub max_parameters: usize,
    /// Body source occurrences plus parameter wrappers across all compilation,
    /// including unused-helper checks and repeated expansions. Never refunded.
    pub max_compilation_nodes: usize,
    /// Shared expression-entry nesting, distinct from physical AST/IR depth.
    pub max_expansion_depth: usize,
}

/// An admitted typed entry and its owned scalar input signature. The caller must
/// validate input values and provide these exact names to its original VM scope.
/// This result is neither an execution grant nor a native-object wire format.
pub struct CompiledCatalogProgram<Node, ResultType> {
    pub node: Node,
    pub result_type: PureType<ResultType>,
    pub entry_parameters: Vec<NamedParameter<String, ScalarType>>,
}

/// Typed compilation output with the original structured compiler failure.
pub type CatalogTypedProgramResult<Node, ResultType, Error> =
    Result<CompiledCatalogProgram<Node, ResultType>, CatalogSourceError<Error>>;

pub enum CatalogProgramError<Error> {
    InvalidLimits,
    Declarations(HelperDeclarationError<Infallible>),
    Dependencies(HelperDependencyError),
    Parameters,
    EntryCall {
        span: Span,
    },
    Names {
        span: Span,
        error: leselang_runtime_core::NamedArgumentError,
    },
    CompilationLimit,
    ExpansionDepth,
    NameLimit,
    Arguments {
        span: Span,
        error: Box<HelperSourceError<Error>>,
    },
    Hygiene {
        span: Span,
        error: Box<HelperHygieneError<Error>>,
    },
    Bindings {
        span: Span,
        error: HelperBindingError,
    },
}
impl<Error> fmt::Display for CatalogProgramError<Error> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidLimits => "catalog program limits exceed safety ceilings",
            Self::Declarations(_) => "catalog program declarations rejected",
            Self::Dependencies(_) => "catalog program helper dependencies rejected",
            Self::Parameters => "catalog program helper scalar prefix rejected",
            Self::EntryCall { .. } => "catalog program cannot call its entry",
            Self::Names { .. } => "catalog program helper named signature rejected",
            Self::CompilationLimit => "catalog program compilation work limit exceeded",
            Self::ExpansionDepth => "catalog program expansion nesting limit exceeded",
            Self::NameLimit => "catalog program generated name limit exceeded",
            Self::Arguments { .. } => "catalog program pure scalar helper operands rejected",
            Self::Hygiene { .. } => "catalog program helper isolation rejected",
            Self::Bindings { .. } => "catalog program parameter wrapper rejected",
        })
    }
}
impl<Error> fmt::Debug for CatalogProgramError<Error> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl<Error> std::error::Error for CatalogProgramError<Error> {}

fn rejected<Error>(
    error: CatalogProgramError<CatalogSourceError<Error>>,
) -> CatalogSourceError<Error> {
    CatalogSourceError::Program(Box::new(error))
}
type Node<Field, Operation, HostEffect, IrResult> =
    Computation<Field, Operation, HostEffect, IrResult>;
type LocalCall<Node, ResultType, Error> = Option<CatalogSourceResult<Node, ResultType, Error>>;
struct PreparedFunction<'source, ResultType> {
    function: &'source Function,
    parameters: Vec<NamedParameter<&'source str, PureType<ResultType>>>,
    nodes: usize,
}
struct State {
    charged: usize,
    names: HashSet<String>,
    next_name: usize,
}
pub(crate) struct CatalogProgramContext<'source, ResultType> {
    pub(crate) limits: CatalogProgramLimits,
    functions: Vec<PreparedFunction<'source, ResultType>>,
    state: RefCell<State>,
    depth: Cell<usize>,
}
#[derive(Clone, Copy)]
pub(crate) struct CatalogProgramGroupProfile {
    pub limits: CatalogGroupSourceLimits,
    pub helper_groups: bool,
    pub prepared_members: bool,
}
#[derive(Clone, Copy, Default)]
pub(crate) struct CatalogProgramProfile {
    pub groups: Option<CatalogProgramGroupProfile>,
    pub typed: bool,
}
struct Depth<'a>(&'a Cell<usize>, usize);
impl Drop for Depth<'_> {
    fn drop(&mut self) {
        self.0.set(self.1);
    }
}
impl<'source, ResultType: Clone + PartialEq> CatalogProgramContext<'source, ResultType> {
    fn function(&self, source: &Expression) -> Option<&PreparedFunction<'source, ResultType>> {
        let Expression::Call { callee, .. } = source else {
            return None;
        };
        self.functions
            .iter()
            .find(|prepared| prepared.function.name == *callee)
    }
    pub(crate) fn is_helper(&self, source: &Expression) -> bool {
        self.function(source).is_some()
    }
    fn charge<Error>(&self, nodes: usize) -> Result<(), CatalogSourceError<Error>> {
        let mut state = self.state.borrow_mut();
        let Some(total) = state
            .charged
            .checked_add(nodes)
            .filter(|total| *total <= self.limits.max_compilation_nodes)
        else {
            return Err(rejected(CatalogProgramError::CompilationLimit));
        };
        state.charged = total;
        Ok(())
    }
    fn fresh<Error>(&self) -> Result<String, CatalogSourceError<Error>> {
        let mut state = self.state.borrow_mut();
        if state.names.len() >= MAX_TYPE_INFERENCE_NODES {
            return Err(rejected(CatalogProgramError::NameLimit));
        }
        loop {
            let name = format!("_lp{}", state.next_name);
            state.next_name = state
                .next_name
                .checked_add(1)
                .ok_or_else(|| rejected(CatalogProgramError::NameLimit))?;
            if state.names.insert(name.clone()) {
                return Ok(name);
            }
        }
    }
    fn compile<
        'schema,
        Field,
        Operation,
        HostEffect,
        IrResult,
        Domain,
        Declaration,
        Capability,
        Environment,
    >(
        &self,
        source: &'source Expression,
        prefix: &[(&'source str, &PureType<Environment::Result>)],
        host: &SourceCallHost<'_, 'schema, Operation, Domain, Declaration, Capability>,
        environment: &Environment,
    ) -> CatalogSourceResult<
        Node<Field, Operation, HostEffect, IrResult>,
        Environment::Result,
        Environment::Error,
    >
    where
        Operation: Borrow<str> + PartialEq,
        Domain: ScalarArgumentDomain,
        Capability: PartialEq,
        Environment: CatalogSourceEnvironment<
                'source,
                'schema,
                Field,
                Operation,
                Declaration,
                Result = ResultType,
            >,
    {
        self.with_expansion(|| lower_catalog_expression(source, prefix, host, environment, self))
    }
    pub(crate) fn with_expansion<Node, Error>(
        &self,
        lower: impl FnOnce() -> CatalogSourceResult<Node, ResultType, Error>,
    ) -> CatalogSourceResult<Node, ResultType, Error> {
        let previous = self.depth.get();
        if previous > self.limits.max_expansion_depth {
            return Err(rejected(CatalogProgramError::ExpansionDepth));
        }
        self.depth.set(previous + 1);
        let _depth = Depth(&self.depth, previous);
        lower()
    }
    fn body<
        'schema,
        Field,
        Operation,
        HostEffect,
        IrResult,
        Domain,
        Declaration,
        Capability,
        Environment,
    >(
        &self,
        prepared: &PreparedFunction<'source, ResultType>,
        host: &SourceCallHost<'_, 'schema, Operation, Domain, Declaration, Capability>,
        environment: &Environment,
    ) -> CatalogSourceResult<
        Node<Field, Operation, HostEffect, IrResult>,
        Environment::Result,
        Environment::Error,
    >
    where
        Operation: Borrow<str> + PartialEq,
        Domain: ScalarArgumentDomain,
        Capability: PartialEq,
        Environment: CatalogSourceEnvironment<
                'source,
                'schema,
                Field,
                Operation,
                Declaration,
                Result = ResultType,
            >,
    {
        self.body_with(prepared, |source, prefix| {
            lower_catalog_expression(source, prefix, host, environment, self)
        })
    }
    fn body_with<Field, Operation, HostEffect, IrResult, Error>(
        &self,
        prepared: &PreparedFunction<'source, ResultType>,
        lower: impl Fn(
            &'source Expression,
            &[(&'source str, &PureType<ResultType>)],
        ) -> CatalogSourceResult<
            Node<Field, Operation, HostEffect, IrResult>,
            ResultType,
            Error,
        >,
    ) -> CatalogSourceResult<Node<Field, Operation, HostEffect, IrResult>, ResultType, Error> {
        self.charge(prepared.nodes)?;
        let types: Vec<_> = prepared
            .parameters
            .iter()
            .map(|parameter| (parameter.name, &parameter.domain))
            .collect();
        self.with_expansion(|| lower(&prepared.function.body, &types))
    }
    pub(crate) fn lower_helper<
        'schema,
        Field,
        Operation,
        HostEffect,
        IrResult,
        Domain,
        Declaration,
        Capability,
        Environment,
    >(
        &self,
        source: &'source Expression,
        prefix: &[(&'source str, &PureType<Environment::Result>)],
        host: &SourceCallHost<'_, 'schema, Operation, Domain, Declaration, Capability>,
        environment: &Environment,
    ) -> LocalCall<
        Node<Field, Operation, HostEffect, IrResult>,
        Environment::Result,
        Environment::Error,
    >
    where
        Operation: Borrow<str> + PartialEq,
        Domain: ScalarArgumentDomain,
        Capability: PartialEq,
        Environment: CatalogSourceEnvironment<
                'source,
                'schema,
                Field,
                Operation,
                Declaration,
                Result = ResultType,
            >,
    {
        self.lower_helper_with(source, prefix, |source, prefix| {
            lower_catalog_expression(source, prefix, host, environment, self)
        })
    }
    pub(crate) fn lower_helper_with<Field, Operation, HostEffect, IrResult, Error>(
        &self,
        source: &'source Expression,
        prefix: &[(&'source str, &PureType<ResultType>)],
        lower: impl Fn(
            &'source Expression,
            &[(&'source str, &PureType<ResultType>)],
        ) -> CatalogSourceResult<
            Node<Field, Operation, HostEffect, IrResult>,
            ResultType,
            Error,
        >,
    ) -> LocalCall<Node<Field, Operation, HostEffect, IrResult>, ResultType, Error> {
        let prepared = self.function(source)?;
        Some((|| {
            self.charge(prepared.parameters.len())?;
            let control = self.limits.expression.control;
            let arguments = lower_typed_helper_arguments(
                source,
                &prepared.function.name,
                &prepared.parameters,
                HelperSourceLimits {
                    source: control.source,
                    max_parameters: self.limits.max_parameters,
                },
                |argument| self.with_expansion(|| lower(&argument.value, prefix)),
            )
            .map_err(|error| {
                rejected(CatalogProgramError::Arguments {
                    span: span(source),
                    error: Box::new(error),
                })
            })?;
            let (body, ty) = self.body_with(prepared, &lower)?;
            let aliases: Vec<String> = prepared
                .parameters
                .iter()
                .map(|_| self.fresh())
                .collect::<Result<_, _>>()?;
            let parameters: Vec<_> = prepared
                .parameters
                .iter()
                .zip(&aliases)
                .map(|(parameter, alias)| (parameter.name, alias.as_str()))
                .collect();
            // Aliases are checked separately by hygiene and cannot also be caller reservations.
            let reserved: Vec<_> = self
                .state
                .borrow()
                .names
                .iter()
                .filter(|name| !aliases.contains(name))
                .cloned()
                .collect();
            let reserved_refs: Vec<_> = reserved.iter().map(String::as_str).collect();
            let body = hygienic_helper_body(
                body,
                &parameters,
                &reserved_refs,
                HelperHygieneLimits {
                    max_nodes: control.source.max_lowered_nodes,
                    max_depth: control.source.max_lowered_depth,
                    max_bindings: control.max_bindings,
                    max_reserved_names: MAX_TYPE_INFERENCE_NODES,
                },
                || self.fresh::<Error>(),
            )
            .map_err(|error| {
                rejected(CatalogProgramError::Hygiene {
                    span: span(source),
                    error: Box::new(error),
                })
            })?;
            let bindings = aliases
                .into_iter()
                .zip(arguments)
                .map(|(name, argument)| HelperBinding {
                    name,
                    value: argument,
                })
                .collect();
            let node = bind_helper_arguments(
                body,
                bindings,
                HelperBindingLimits {
                    max_nodes: control.source.max_lowered_nodes,
                    max_depth: control.source.max_lowered_depth,
                    max_parameters: self.limits.max_parameters,
                },
            )
            .map_err(|error| {
                rejected(CatalogProgramError::Bindings {
                    span: span(source),
                    error,
                })
            })?;
            Ok((node, ty))
        })())
    }
}

/// Compile all original declarations, including unused helpers, then the explicit
/// parameterless entry. Exact six-scalar signatures, complete cold lexical/source
/// shape, all helper named calls and native catalog policy, and the whole acyclic
/// dependency graph pass before native compilation hooks. Helpers cannot shadow
/// intrinsics/catalog operations, call the entry, recurse or capture caller locals.
///
/// Bodies compile in closed parameter scopes and are recompiled per invocation,
/// hygienically moved into declaration-order pure parameter wrappers. No native
/// slot Clone/serde/Send requirement, template-copy hook or source substitution is
/// installed. Source occurrences and wrappers share a nonrefunding work quota;
/// expression-entry nesting has a separate limit. Whole expanded output still
/// passes original call-flow inference and physical/active-binding/cold-call limits.
/// This quota is not native work, fuel, ingress bytes or durable authority.
///
/// Supports the expression profile plus pure/effectful acyclic scalar-parameter
/// helpers. Seq/all/repeat, opaque Host graphs and durable recovery remain outside
/// this entry. No storage, evaluator, dispatch, partial program or fallback global
/// lookup is created. Native metadata/work/Drop/unwind remain trusted adapter policy.
pub fn compile_catalog_program<
    'source,
    'schema,
    Field,
    Operation,
    HostEffect,
    IrResult,
    Domain,
    Declaration,
    Capability,
    Environment,
>(
    declarations: &[&'source Function],
    entry: &str,
    limits: CatalogProgramLimits,
    host: &SourceCallHost<'_, 'schema, Operation, Domain, Declaration, Capability>,
    environment: &Environment,
) -> CatalogSourceResult<
    Node<Field, Operation, HostEffect, IrResult>,
    Environment::Result,
    Environment::Error,
>
where
    Operation: Borrow<str> + PartialEq,
    Domain: ScalarArgumentDomain,
    Capability: PartialEq,
    Environment: CatalogSourceEnvironment<'source, 'schema, Field, Operation, Declaration>,
{
    compile_catalog_program_with_entry(
        declarations,
        entry,
        limits,
        host,
        environment,
        CatalogProgramProfile::default(),
        |program, source, prefix| program.compile(source, prefix, host, environment),
    )
}

/// Opt in to scalar entry inputs and exact host-declared native helper types.
///
/// Scalar annotations retain their six closed meanings. Other helper annotations
/// are resolved once through [`CatalogSourceEnvironment::source_type_name`] after
/// complete physical/header admission, then retained as exact native type metadata.
/// Entry inputs remain scalar; a native value cannot be supplied as entry data.
/// Native helper operands must be pure, previously bound results of the exact type,
/// with no join, coercion, object serialization or implicit effect execution.
///
/// All declarations and cold paths still pass the original dependency, authority,
/// shape, typing, hygiene and nonrefunding expansion budgets. Entry locals use the
/// original declared names; no source rewrite, wrapper entry or evaluator is added.
/// The caller owns runtime input validation, leases, live grants and original Fuel.
pub fn compile_catalog_typed_program<
    'source,
    'schema,
    Field,
    Operation,
    HostEffect,
    IrResult,
    Domain,
    Declaration,
    Capability,
    Environment,
>(
    declarations: &[&'source Function],
    entry: &str,
    limits: CatalogProgramLimits,
    host: &SourceCallHost<'_, 'schema, Operation, Domain, Declaration, Capability>,
    environment: &Environment,
) -> CatalogTypedProgramResult<
    Node<Field, Operation, HostEffect, IrResult>,
    Environment::Result,
    Environment::Error,
>
where
    Operation: Borrow<str> + PartialEq,
    Domain: ScalarArgumentDomain,
    Capability: PartialEq,
    Environment: CatalogSourceEnvironment<'source, 'schema, Field, Operation, Declaration>,
{
    let (node, result_type) = compile_catalog_program_with_entry(
        declarations,
        entry,
        limits,
        host,
        environment,
        CatalogProgramProfile {
            typed: true,
            ..CatalogProgramProfile::default()
        },
        |program, source, prefix| program.compile(source, prefix, host, environment),
    )?;
    let function = declarations
        .iter()
        .find(|function| function.name == entry)
        .expect("admitted typed program retains its entry");
    let entry_parameters = helper_parameters(function, limits.max_parameters)
        .expect("admitted typed entry has a scalar signature")
        .into_iter()
        .map(|parameter| NamedParameter::required(parameter.name.to_owned(), parameter.domain))
        .collect();
    Ok(CompiledCatalogProgram {
        node,
        result_type,
        entry_parameters,
    })
}

// Entry profiles share complete declarations, cold helper policy, body validation,
// hygiene, work accounting and expansion guards. Only the entry lowerer differs.
pub(crate) fn compile_catalog_program_with_entry<
    'source,
    'schema,
    Field,
    Operation,
    HostEffect,
    IrResult,
    Domain,
    Declaration,
    Capability,
    Environment,
>(
    declarations: &[&'source Function],
    entry: &str,
    limits: CatalogProgramLimits,
    host: &SourceCallHost<'_, 'schema, Operation, Domain, Declaration, Capability>,
    environment: &Environment,
    profile: CatalogProgramProfile,
    finish: impl Fn(
        &CatalogProgramContext<'source, Environment::Result>,
        &'source Expression,
        &[(&'source str, &PureType<Environment::Result>)],
    ) -> CatalogSourceResult<
        Node<Field, Operation, HostEffect, IrResult>,
        Environment::Result,
        Environment::Error,
    >,
) -> CatalogSourceResult<
    Node<Field, Operation, HostEffect, IrResult>,
    Environment::Result,
    Environment::Error,
>
where
    Operation: Borrow<str> + PartialEq,
    Domain: ScalarArgumentDomain,
    Capability: PartialEq,
    Environment: CatalogSourceEnvironment<'source, 'schema, Field, Operation, Declaration>,
{
    let CatalogProgramProfile {
        groups: entry_groups,
        typed,
    } = profile;
    if limits.max_functions > MAX_FUNCTIONS
        || limits.max_parameters > MAX_FUNCTION_PARAMETERS
        || limits.max_compilation_nodes > MAX_TYPE_INFERENCE_NODES
        || limits.max_expansion_depth > MAX_TYPE_INFERENCE_DEPTH
        || limits.expression.max_calls > MAX_TYPE_INFERENCE_NODES
    {
        return Err(rejected(CatalogProgramError::InvalidLimits));
    }
    let source = limits.expression.control.source;
    let accepted = accept_helper_declarations_profile(
        declarations,
        entry,
        HelperDeclarationLimits {
            max_functions: limits.max_functions,
            max_parameters: limits.max_parameters,
            max_source_nodes: source.max_source_nodes,
            max_source_depth: source.max_source_depth,
        },
        |name| {
            Ok(primitive_name(name)
                || matches!(
                    name,
                    "bind"
                        | "choose"
                        | "loop"
                        | "fold"
                        | "field"
                        | "member"
                        | "seq"
                        | "all"
                        | "repeat"
                )
                || host.catalog.lookup(name, host.catalog.version()).is_ok())
        },
        typed,
    )
    .map_err(|e| rejected(CatalogProgramError::Declarations(e)))?;
    let order: Vec<_> = helper_dependency_order(
        accepted.helpers(),
        entry,
        HelperDependencyLimits {
            max_helpers: limits.max_functions.saturating_sub(1),
            max_source_nodes: source.max_source_nodes,
            max_source_depth: source.max_source_depth,
        },
    )
    .map_err(|e| rejected(CatalogProgramError::Dependencies(e)))?
    .collect::<Result<_, _>>()
    .map_err(|e| rejected(CatalogProgramError::Dependencies(e)))?;
    let mut functions = Vec::with_capacity(accepted.helpers().len());
    let mut entry_parameters = Vec::new();
    for (declaration_index, function) in declarations.iter().enumerate() {
        let parameters = helper_parameters_with(function, limits.max_parameters, |name| {
            scalar_type_name(name).map(PureType::Scalar).or_else(|| {
                (typed && !std::ptr::eq(*function, accepted.entry()))
                    .then(|| environment.source_type_name(name))
                    .flatten()
                    .map(PureType::Result)
            })
        })
        .map_err(|error| {
            let span = match error {
                HelperParameterError::InvalidName { index }
                | HelperParameterError::DuplicateName { index }
                | HelperParameterError::UnknownType { index } => function.parameters[index].span,
                _ => function.span,
            };
            rejected(CatalogProgramError::Declarations(
                HelperDeclarationError::Parameters {
                    declaration_index,
                    span,
                    error,
                },
            ))
        })?;
        if std::ptr::eq(*function, accepted.entry()) {
            entry_parameters = parameters;
        } else {
            functions.push(PreparedFunction {
                function,
                parameters,
                nodes: 0,
            });
        }
    }
    // Header/physical gates above bound this forest before walking or copying names.
    let mut names = HashSet::new();
    for name in accepted.reserved_names() {
        // Intrinsic operand labels are not legal locals and cannot collide with fresh aliases.
        if !valid_local_name(name) {
            continue;
        }
        if names.len() >= MAX_TYPE_INFERENCE_NODES && !names.contains(name) {
            return Err(rejected(CatalogProgramError::NameLimit));
        }
        names.insert(name.to_owned());
    }
    let mut program = CatalogProgramContext {
        limits,
        functions,
        state: RefCell::new(State {
            charged: 0,
            names,
            next_name: 0,
        }),
        depth: Cell::new(0),
    };
    let mut cold_calls = 0usize;
    let mut cold_groups = 0usize;
    let mut entry_nodes = 0usize;
    for function in declarations {
        let parameters = program
            .functions
            .iter()
            .find(|prepared| std::ptr::eq(prepared.function, *function))
            .map_or(entry_parameters.as_slice(), |prepared| {
                prepared.parameters.as_slice()
            });
        let prefix: Vec<_> = parameters.iter().map(|p| (p.name, &p.domain)).collect();
        if entry_groups.is_some() {
            preflight_closed_control_source(&function.body, &prefix, limits.expression.control)
                .map_err(|e| CatalogSourceError::Control(Box::new(e)))?;
        } else {
            preflight_control_source(&function.body, &prefix, limits.expression.control)
                .map_err(|e| CatalogSourceError::Control(Box::new(e)))?;
        }
        let groups = entry_groups
            .filter(|profile| profile.helper_groups || std::ptr::eq(*function, accepted.entry()));
        if let Some(profile) = groups {
            let count = cold_groups_with(
                &function.body,
                profile.limits,
                profile.prepared_members,
                |node| program.is_helper(node),
            )
            .map_err(|e| CatalogSourceError::Group(Box::new(e)))?;
            cold_groups = cold_groups
                .checked_add(count)
                .filter(|count| *count <= profile.limits.max_groups)
                .ok_or_else(|| {
                    CatalogSourceError::Group(Box::new(
                        crate::catalog_group_source::CatalogGroupSourceError::GroupLimit {
                            span: span(&function.body),
                        },
                    ))
                })?;
        }
        let local_call = |node: &Expression| {
            let Expression::Call {
                callee, arguments, ..
            } = node
            else {
                return Ok(false);
            };
            if callee == entry {
                return Err(rejected(CatalogProgramError::EntryCall {
                    span: span(node),
                }));
            }
            let Some(prepared) = program.function(node) else {
                return Ok(false);
            };
            let names: Vec<_> = arguments.iter().map(|a| a.name.as_str()).collect();
            bind_named_arguments(&names, &prepared.parameters).map_err(|error| {
                rejected(CatalogProgramError::Names {
                    span: span(node),
                    error,
                })
            })?;
            Ok(true)
        };
        let calls = if groups.is_some_and(|profile| profile.prepared_members) {
            crate::catalog_source::cold_catalog_with_repeat(
                &function.body,
                host,
                limits.expression,
                true,
                true,
                local_call,
            )?
        } else if groups.is_some() {
            crate::catalog_source::cold_catalog_profile(
                &function.body,
                host,
                limits.expression,
                true,
                local_call,
            )?
        } else {
            cold_catalog(&function.body, host, limits.expression, local_call)?
        };
        cold_calls = cold_calls
            .checked_add(calls)
            .filter(|n| *n <= limits.expression.max_calls)
            .ok_or(CatalogSourceError::Type(CallFlowTypeError::CallLimit))?;
        let mut pending = vec![&function.body];
        let mut nodes = 0;
        while let Some(node) = pending.pop() {
            nodes += 1;
            if let Expression::Call { arguments, .. } = node {
                pending.extend(arguments.iter().rev().map(|a| &a.value));
            }
        }
        if let Some(prepared) = program
            .functions
            .iter_mut()
            .find(|p| std::ptr::eq(p.function, *function))
        {
            prepared.nodes = nodes;
        } else {
            entry_nodes = nodes;
        }
    }
    if let Some(profile) = entry_groups.filter(|profile| profile.prepared_members) {
        crate::catalog_atomic_source::cold_prepared_members(
            declarations,
            &order,
            profile.limits.max_branches,
        )
        .map_err(|error| CatalogSourceError::Group(Box::new(error)))?;
    }
    // Unused helpers are checked too. Recompilation deliberately avoids copying native slots.
    for function in order {
        if let Some(prepared) = program
            .functions
            .iter()
            .find(|p| std::ptr::eq(p.function, function))
        {
            let _: (
                Node<Field, Operation, HostEffect, IrResult>,
                PureType<Environment::Result>,
            ) = if entry_groups.is_some_and(|profile| profile.helper_groups) {
                program.body_with(prepared, |source, prefix| finish(&program, source, prefix))?
            } else {
                program.body(prepared, host, environment)?
            };
        }
    }
    program.charge(entry_nodes)?;
    let entry_prefix: Vec<_> = entry_parameters
        .iter()
        .map(|p| (p.name, &p.domain))
        .collect();
    if entry_groups.is_some_and(|profile| profile.helper_groups) {
        program.with_expansion(|| finish(&program, &accepted.entry().body, &entry_prefix))
    } else {
        finish(&program, &accepted.entry().body, &entry_prefix)
    }
}
