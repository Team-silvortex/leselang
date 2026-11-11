//! Shared atomic-call preparation. This module never dispatches host effects.

use std::fmt;

use leselang_runtime_core::{
    ArgumentTypeError, Fuel, OperationCatalog, OperationSchema, ScalarArgumentDomain,
    ScalarArgumentType, ScalarValue, ScopeFrame, StructureBudget, check_argument_type,
};

use crate::call_typing::{
    CallTypeError, MAX_CALL_ARGUMENTS, preflight_arguments_with_budget, preflight_signature,
    valid_argument_name,
};
use crate::ir::{Computation, ComputedArgument};
use crate::pure_evaluation::{
    PureEvaluationEnvironment, PureEvaluationFailure, PureEvaluationFault, PureEvaluationLimits,
    PureValue, evaluate_preflighted_in_scope, preflight_value_scope, scalar_copy_cost,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallEvaluationLimits {
    /// One physical call root plus the entire argument forest, not per-argument limits.
    pub pure: PureEvaluationLimits,
    pub max_arguments: usize,
}

/// Trusted native catalog/version/grants and actual-value projection adapter.
/// Counts/native work are host-owned, not metered by language fuel.
pub struct CallEvaluationHost<'a, 'schema, Operation, Domain, ResultTag, Capability, Environment> {
    pub catalog:
        &'a OperationCatalog<'schema, Operation, &'schema str, Domain, ResultTag, Capability>,
    pub version: u32,
    pub granted: &'a [Capability],
    pub environment: &'a Environment,
}

impl<Operation, Domain, ResultTag, Capability, Environment> fmt::Debug
    for CallEvaluationHost<'_, '_, Operation, Domain, ResultTag, Capability, Environment>
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CallEvaluationHost")
            .field("version", &self.version)
            .field("grants", &self.granted.len())
            .finish()
    }
}

/// An owned evaluated value with original submission/declaration positions.
/// Names and values are data, not safe log fields or an immutable certificate.
pub struct PreparedArgument<'schema> {
    pub parameter_index: usize,
    pub argument_index: usize,
    pub name: &'schema str,
    pub value: ScalarValue,
}

impl fmt::Debug for PreparedArgument<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedArgument")
            .field("parameter_index", &self.parameter_index)
            .field("argument_index", &self.argument_index)
            .field("scalar_type", &self.value.scalar_type())
            .finish()
    }
}

/// Borrowed original schema and move-only, declaration-ordered scalar values.
/// No dispatch, receipt, revision, deadline or durable-replay authority is granted.
/// Native metadata may use interior mutability; hosts recheck execution policy
/// before effects. Parts can be extracted/mutated, so this is no lasting certificate.
///
/// ```compile_fail
/// use leselang_hir_core::call_evaluation::PreparedCall;
/// use leselang_runtime_core::ScalarTypeSet;
/// fn duplicate(call: PreparedCall<'_, u32, ScalarTypeSet, (), u8>) {
///     let other = call.clone();
/// }
/// ```
///
/// ```compile_fail
/// use leselang_hir_core::call_evaluation::PreparedCall;
/// use leselang_runtime_core::ScalarTypeSet;
/// fn wire(call: PreparedCall<'_, u32, ScalarTypeSet, (), u8>) {
///     let serialized = serde_json::to_string(&call).unwrap();
/// }
/// ```
#[must_use = "prepared call data must be passed to the owning host, not silently discarded"]
pub struct PreparedCall<'schema, Operation, Domain, ResultTag, Capability> {
    schema:
        &'schema OperationSchema<'schema, Operation, &'schema str, Domain, ResultTag, Capability>,
    arguments: Vec<PreparedArgument<'schema>>,
}

impl<'schema, Operation, Domain, ResultTag, Capability>
    PreparedCall<'schema, Operation, Domain, ResultTag, Capability>
{
    pub fn schema(
        &self,
    ) -> &'schema OperationSchema<'schema, Operation, &'schema str, Domain, ResultTag, Capability>
    {
        self.schema
    }

    pub fn arguments(&self) -> &[PreparedArgument<'schema>] {
        &self.arguments
    }

    pub fn into_parts(
        self,
    ) -> (
        &'schema OperationSchema<'schema, Operation, &'schema str, Domain, ResultTag, Capability>,
        Vec<PreparedArgument<'schema>>,
    ) {
        (self.schema, self.arguments)
    }

    /// Restore host-decoded operands without reevaluating their expressions.
    ///
    /// Complete counts, names, declaration order, submission-index permutation
    /// and scalar bounds precede every native callback. Named signature checking
    /// uses the original submission order. Mandatory whole-call host admission
    /// then precedes the existing native domain checks in declaration order.
    /// Values move unchanged; names rebind to the selected original schema, so
    /// the returned call does not borrow decoded names or the input snapshot.
    ///
    /// The host must authenticate the original committed operation/schema and
    /// evaluated operands, resource budget and publication/settlement state.
    /// Well-formed data is not provenance: do not rearm published, cancelled or
    /// settled work. Live authorization is required again before dispatch.
    /// This entry creates no meter, identity, evaluator, codec or durable authority.
    /// Aggregate decode/storage limits and native callback work/unwind are owned
    /// by the host. Refusal consumes the inputs without retry or rollback.
    pub fn restore<Error>(
        schema: &'schema OperationSchema<
            'schema,
            Operation,
            &'schema str,
            Domain,
            ResultTag,
            Capability,
        >,
        arguments: Vec<PreparedArgument<'_>>,
        max_arguments: usize,
        admit: impl FnOnce(
            &OperationSchema<'schema, Operation, &'schema str, Domain, ResultTag, Capability>,
            &[PreparedArgument<'_>],
        ) -> Result<(), Error>,
    ) -> Result<Self, PreparedCallRestoreError<Error>>
    where
        Domain: ScalarArgumentDomain,
    {
        if max_arguments > MAX_CALL_ARGUMENTS {
            return Err(PreparedCallRestoreError::Preflight(
                CallTypeError::InvalidLimits,
            ));
        }
        if arguments.len() > max_arguments {
            return Err(PreparedCallRestoreError::Preflight(
                CallTypeError::ArgumentLimit,
            ));
        }
        if schema.parameters.len() > max_arguments {
            return Err(PreparedCallRestoreError::Preflight(
                CallTypeError::ParameterLimit,
            ));
        }
        let mut names = vec![""; arguments.len()];
        let mut seen = [false; MAX_CALL_ARGUMENTS];
        let mut previous = None;
        for (index, row) in arguments.iter().enumerate() {
            if !valid_argument_name(row.name) {
                return Err(PreparedCallRestoreError::Preflight(
                    CallTypeError::InvalidArgumentName {
                        argument_index: row.argument_index,
                    },
                ));
            }
            let Some(parameter) = schema.parameters.get(row.parameter_index) else {
                return Err(PreparedCallRestoreError::Row { index });
            };
            if row.argument_index >= arguments.len()
                || seen[row.argument_index]
                || previous.is_some_and(|previous| row.parameter_index <= previous)
                || row.name != parameter.name
            {
                return Err(PreparedCallRestoreError::Row { index });
            }
            if !row.value.is_bounded() {
                return Err(PreparedCallRestoreError::Argument {
                    parameter_index: row.parameter_index,
                    argument_index: row.argument_index,
                    error: ArgumentTypeError::UnboundedLiteral,
                });
            }
            names[row.argument_index] = row.name;
            seen[row.argument_index] = true;
            previous = Some(row.parameter_index);
        }
        preflight_signature(&names, schema, max_arguments)
            .map_err(PreparedCallRestoreError::Preflight)?;
        admit(schema, &arguments).map_err(PreparedCallRestoreError::Native)?;
        for row in &arguments {
            check_argument_type(
                &schema.parameters[row.parameter_index].domain,
                ScalarArgumentType::literal(&row.value),
            )
            .map_err(|error| PreparedCallRestoreError::Argument {
                parameter_index: row.parameter_index,
                argument_index: row.argument_index,
                error,
            })?;
        }
        Ok(Self {
            schema,
            arguments: arguments
                .into_iter()
                .map(|row| PreparedArgument {
                    name: schema.parameters[row.parameter_index].name,
                    ..row
                })
                .collect(),
        })
    }
}

/// Closed positions and verbatim native admission errors, never decoded payloads.
pub enum PreparedCallRestoreError<Native> {
    Preflight(CallTypeError),
    Row {
        index: usize,
    },
    Argument {
        parameter_index: usize,
        argument_index: usize,
        error: ArgumentTypeError,
    },
    Native(Native),
}
impl<Native> fmt::Display for PreparedCallRestoreError<Native> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Preflight(_) => "saved call signature rejected",
            Self::Row { .. } => "saved call operand correspondence rejected",
            Self::Argument { .. } => "saved call operand value rejected",
            Self::Native(_) => "saved call native admission rejected",
        })
    }
}
impl<Native> fmt::Debug for PreparedCallRestoreError<Native> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}
impl<Native> std::error::Error for PreparedCallRestoreError<Native> {}

impl<Operation, Domain, ResultTag, Capability> fmt::Debug
    for PreparedCall<'_, Operation, Domain, ResultTag, Capability>
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedCall")
            .field("arguments", &self.arguments.len())
            .finish()
    }
}

/// Closed positions/errors with the original typed evaluation failure retained.
/// Native errors are never cloned, formatted, serialized or made recoverable.
pub enum CallEvaluationError<Native> {
    Preflight(CallTypeError),
    Scope(PureEvaluationFault<Native>),
    FuelExhausted,
    NonScalar {
        parameter_index: usize,
        argument_index: usize,
    },
    Evaluation {
        parameter_index: usize,
        argument_index: usize,
        failure: PureEvaluationFailure<Native>,
    },
    Argument {
        parameter_index: usize,
        argument_index: usize,
        error: ArgumentTypeError,
    },
}

impl<Native> fmt::Display for CallEvaluationError<Native> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Preflight(_) => "call preparation preflight rejected",
            Self::Scope(_) => "call preparation value scope rejected",
            Self::FuelExhausted => "call preparation fuel exhausted",
            Self::NonScalar { .. } => "call preparation requires scalar arguments",
            Self::Evaluation { .. } => "call argument evaluation failed",
            Self::Argument { .. } => "evaluated call argument is outside its declared domain",
        })
    }
}

impl<Native> fmt::Debug for CallEvaluationError<Native> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl<Native> std::error::Error for CallEvaluationError<Native> {}

type Arguments<Field, Operation, HostEffect, IrResult> =
    [ComputedArgument<Computation<Field, Operation, HostEffect, IrResult>>];

fn preflight<'expression, Field, Operation, HostEffect, IrResult, NativeResult, NativeError>(
    arguments: &'expression Arguments<Field, Operation, HostEffect, IrResult>,
    scope: &ScopeFrame<'_, 'expression, PureValue<NativeResult>>,
    limits: CallEvaluationLimits,
) -> Result<Vec<&'expression str>, CallEvaluationError<NativeError>> {
    if limits.max_arguments > MAX_CALL_ARGUMENTS {
        return Err(CallEvaluationError::Preflight(CallTypeError::InvalidLimits));
    }
    preflight_value_scope(scope, limits.pure).map_err(CallEvaluationError::Scope)?;
    let mut budget = StructureBudget::new(limits.pure.max_nodes, limits.pure.max_depth);
    budget
        .visit(0, 0, 0)
        .map_err(|error| CallEvaluationError::Preflight(CallTypeError::Structure(error)))?;
    preflight_arguments_with_budget(arguments, 1, &mut budget, limits.max_arguments)
        .map_err(CallEvaluationError::Preflight)?;
    Ok(arguments
        .iter()
        .map(|argument| argument.name.as_str())
        .collect())
}

pub(crate) fn evaluate_arguments<
    'expression,
    'schema,
    Field,
    Operation,
    HostEffect,
    IrResult,
    Key,
    Domain,
    ResultTag,
    Capability,
    Environment,
>(
    arguments: &'expression Arguments<Field, Operation, HostEffect, IrResult>,
    names: &[&str],
    schema: &'schema OperationSchema<'schema, Key, &'schema str, Domain, ResultTag, Capability>,
    scope: &mut ScopeFrame<'_, 'expression, PureValue<Environment::Result>>,
    environment: &Environment,
    fuel: &mut Fuel,
    limits: CallEvaluationLimits,
) -> Result<
    PreparedCall<'schema, Key, Domain, ResultTag, Capability>,
    CallEvaluationError<Environment::Error>,
>
where
    Domain: ScalarArgumentDomain,
    Environment: PureEvaluationEnvironment<Field, Operation>,
{
    let mut values = Vec::with_capacity(arguments.len());
    for (parameter_index, parameter) in schema.parameters.iter().enumerate() {
        let Some(argument_index) = names.iter().position(|name| *name == parameter.name) else {
            continue;
        };
        let value = evaluate_preflighted_in_scope(
            &arguments[argument_index].value,
            scope,
            environment,
            fuel,
            limits.pure.max_bindings,
        )
        .map_err(|failure| CallEvaluationError::Evaluation {
            parameter_index,
            argument_index,
            failure,
        })?;
        let PureValue::Scalar(value) = value else {
            return Err(CallEvaluationError::NonScalar {
                parameter_index,
                argument_index,
            });
        };
        fuel.charge(scalar_copy_cost(&value))
            .map_err(|_| CallEvaluationError::FuelExhausted)?;
        values.push(PreparedArgument {
            parameter_index,
            argument_index,
            name: parameter.name,
            value,
        });
    }
    // All values are evaluated before any native domain callback, matching the
    // reference resolver's failure order and preventing partially prepared output.
    for value in &values {
        check_argument_type(
            &schema.parameters[value.parameter_index].domain,
            ScalarArgumentType::literal(&value.value),
        )
        .map_err(|error| CallEvaluationError::Argument {
            parameter_index: value.parameter_index,
            argument_index: value.argument_index,
            error,
        })?;
    }
    Ok(PreparedCall {
        schema,
        arguments: values,
    })
}

/// Evaluate arguments against an already selected original signature.
///
/// Whole forest/prefix/name shape preflight precedes fuel/native value queries.
/// Values execute once in declaration order, then every evaluated value is checked
/// against the declared scalar type, bounds and native domain. Optional omissions
/// are not filled; names borrow declarations and scalar buffers move into output.
/// One physical call root is counted, but its execution fuel is caller-owned.
/// This entry performs no catalog/version/capability lookup or dispatch.
/// Static cold type checking remains `check_call_arguments`' responsibility.
pub fn evaluate_call_arguments_in_scope<
    'expression,
    'schema,
    Field,
    Operation,
    HostEffect,
    IrResult,
    Key,
    Domain,
    ResultTag,
    Capability,
    Environment,
>(
    arguments: &'expression Arguments<Field, Operation, HostEffect, IrResult>,
    schema: &'schema OperationSchema<'schema, Key, &'schema str, Domain, ResultTag, Capability>,
    scope: &mut ScopeFrame<'_, 'expression, PureValue<Environment::Result>>,
    environment: &Environment,
    fuel: &mut Fuel,
    limits: CallEvaluationLimits,
) -> Result<
    PreparedCall<'schema, Key, Domain, ResultTag, Capability>,
    CallEvaluationError<Environment::Error>,
>
where
    Domain: ScalarArgumentDomain,
    Environment: PureEvaluationEnvironment<Field, Operation>,
{
    let names = preflight(arguments, scope, limits)?;
    preflight_signature(&names, schema, limits.max_arguments)
        .map_err(CallEvaluationError::Preflight)?;
    evaluate_arguments(arguments, &names, schema, scope, environment, fuel, limits)
}

/// One evaluated scalar or opaque native result, in original schema order.
///
/// Native values use the pure environment's existing alias/Clone contract. This
/// row neither serializes an object identity nor certifies live authority.
pub struct PreparedValueArgument<'schema, NativeResult> {
    pub parameter_index: usize,
    pub argument_index: usize,
    pub name: &'schema str,
    pub value: PureValue<NativeResult>,
}

impl<NativeResult> fmt::Debug for PreparedValueArgument<'_, NativeResult> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedValueArgument")
            .field("parameter_index", &self.parameter_index)
            .field("argument_index", &self.argument_index)
            .field("value", &self.value)
            .finish()
    }
}

/// Move-only, declaration-ordered operands using the original borrowed schema.
///
/// The native payload requires no Debug, serde, Send or 'static bound. A successful
/// preparation is not dispatch permission: the host must revalidate references,
/// current grants and operation policy immediately before actual native work.
///
/// ```compile_fail
/// use leselang_hir_core::call_evaluation::PreparedValueCall;
/// fn duplicate(call: PreparedValueCall<'_, u32, (), (), (), ()>) {
///     let other = call.clone();
/// }
/// ```
#[must_use = "prepared operands must be handed to their host or deliberately discarded"]
pub struct PreparedValueCall<'schema, Key, Domain, ResultTag, Capability, NativeResult> {
    schema: &'schema OperationSchema<'schema, Key, &'schema str, Domain, ResultTag, Capability>,
    arguments: Vec<PreparedValueArgument<'schema, NativeResult>>,
}

/// Original schema plus the exact owned prepared operands, without conversion.
pub type PreparedValueCallParts<'schema, Key, Domain, ResultTag, Capability, NativeResult> = (
    &'schema OperationSchema<'schema, Key, &'schema str, Domain, ResultTag, Capability>,
    Vec<PreparedValueArgument<'schema, NativeResult>>,
);

/// Mixed-operand preparation with separate evaluation and native-admission errors.
pub type ValueCallEvaluationResult<
    'schema,
    Key,
    Domain,
    ResultTag,
    Capability,
    NativeResult,
    NativeError,
    Validation,
> = Result<
    PreparedValueCall<'schema, Key, Domain, ResultTag, Capability, NativeResult>,
    ValueCallEvaluationError<NativeError, Validation>,
>;

impl<'schema, Key, Domain, ResultTag, Capability, NativeResult>
    PreparedValueCall<'schema, Key, Domain, ResultTag, Capability, NativeResult>
{
    pub fn schema(
        &self,
    ) -> &'schema OperationSchema<'schema, Key, &'schema str, Domain, ResultTag, Capability> {
        self.schema
    }

    pub fn arguments(&self) -> &[PreparedValueArgument<'schema, NativeResult>] {
        &self.arguments
    }

    pub fn into_parts(
        self,
    ) -> PreparedValueCallParts<'schema, Key, Domain, ResultTag, Capability, NativeResult> {
        (self.schema, self.arguments)
    }
}

impl<Key, Domain, ResultTag, Capability, NativeResult> fmt::Debug
    for PreparedValueCall<'_, Key, Domain, ResultTag, Capability, NativeResult>
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedValueCall")
            .field("arguments", &self.arguments.len())
            .finish()
    }
}

/// Original evaluation failure or opaque native argument admission failure.
/// Native validation errors never acquire scalar recovery provenance.
pub enum ValueCallEvaluationError<Native, Validation> {
    Preparation(CallEvaluationError<Native>),
    Argument {
        parameter_index: usize,
        argument_index: usize,
        error: Validation,
    },
}

impl<Native, Validation> fmt::Display for ValueCallEvaluationError<Native, Validation> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Preparation(error) => error.fmt(formatter),
            Self::Argument { .. } => {
                formatter.write_str("evaluated call argument admission failed")
            }
        }
    }
}
impl<Native, Validation> fmt::Debug for ValueCallEvaluationError<Native, Validation> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}
impl<Native, Validation> std::error::Error for ValueCallEvaluationError<Native, Validation> {}
impl<Native, Validation> From<CallEvaluationError<Native>>
    for ValueCallEvaluationError<Native, Validation>
{
    fn from(error: CallEvaluationError<Native>) -> Self {
        Self::Preparation(error)
    }
}

/// Evaluate mixed scalar/native operands with the existing pure evaluator.
///
/// Complete physical forest, scope and named-signature preflight precede any
/// native value access or cloning. Values then evaluate once in declaration
/// order, including the existing scalar copy charges and bounds. Only after
/// every operand succeeds does `validate` run, also in declaration order. It
/// must check each scalar domain or native type, owner, lease and value bounds;
/// no domain is implicitly accepted. It receives the original Fuel for any
/// additional trusted host accounting. Native cloning/validation is host-owned
/// work, not preempted or measured by inspecting opaque payloads.
///
/// Optional operands remain omitted. Prefix bindings are borrowed and remain
/// unchanged; native Local reads follow `Environment::Result::clone`, so hosts
/// should use bounded aliases such as opaque references or Rc views. Failure or
/// unwind discards partial operands without retry, rollback or fuel refund.
///
/// One call root is structurally counted, but root execution fuel belongs to
/// the caller, as with `evaluate_call_arguments_in_scope`. This performs no
/// catalog/version/grant lookup, cold static typing, correlation or dispatch.
/// The scalar-only API and its early NonScalar refusal remain unchanged.
pub fn evaluate_value_call_arguments_in_scope<
    'expression,
    'schema,
    Field,
    Operation,
    HostEffect,
    IrResult,
    Key,
    Domain,
    ResultTag,
    Capability,
    Environment,
    Validation,
>(
    arguments: &'expression Arguments<Field, Operation, HostEffect, IrResult>,
    schema: &'schema OperationSchema<'schema, Key, &'schema str, Domain, ResultTag, Capability>,
    scope: &mut ScopeFrame<'_, 'expression, PureValue<Environment::Result>>,
    environment: &Environment,
    fuel: &mut Fuel,
    limits: CallEvaluationLimits,
    mut validate: impl FnMut(
        &Domain,
        &PureValue<Environment::Result>,
        &mut Fuel,
    ) -> Result<(), Validation>,
) -> ValueCallEvaluationResult<
    'schema,
    Key,
    Domain,
    ResultTag,
    Capability,
    Environment::Result,
    Environment::Error,
    Validation,
>
where
    Environment: PureEvaluationEnvironment<Field, Operation>,
{
    let names = preflight(arguments, scope, limits)?;
    preflight_signature(&names, schema, limits.max_arguments)
        .map_err(CallEvaluationError::Preflight)?;
    let mut values = Vec::with_capacity(arguments.len());
    for (parameter_index, parameter) in schema.parameters.iter().enumerate() {
        let Some(argument_index) = names.iter().position(|name| *name == parameter.name) else {
            continue;
        };
        let value = evaluate_preflighted_in_scope(
            &arguments[argument_index].value,
            scope,
            environment,
            fuel,
            limits.pure.max_bindings,
        )
        .map_err(|failure| CallEvaluationError::Evaluation {
            parameter_index,
            argument_index,
            failure,
        })?;
        if let PureValue::Scalar(value) = &value {
            if !value.is_bounded() {
                return Err(CallEvaluationError::Argument {
                    parameter_index,
                    argument_index,
                    error: ArgumentTypeError::UnboundedLiteral,
                }
                .into());
            }
            fuel.charge(scalar_copy_cost(value))
                .map_err(|_| CallEvaluationError::FuelExhausted)?;
        }
        values.push(PreparedValueArgument {
            parameter_index,
            argument_index,
            name: parameter.name,
            value,
        });
    }
    for value in &values {
        validate(
            &schema.parameters[value.parameter_index].domain,
            &value.value,
            fuel,
        )
        .map_err(|error| ValueCallEvaluationError::Argument {
            parameter_index: value.parameter_index,
            argument_index: value.argument_index,
            error,
        })?;
    }
    Ok(PreparedValueCall {
        schema,
        arguments: values,
    })
}

/// Prepare one real shared Call through an exact-version, trusted-grant catalog.
///
/// Physical/prefix preflight precedes native key comparisons; authorization and
/// named shape precede value evaluation. This entry charges one call-root unit.
/// It returns the original schema and fully prepared data, never invokes the
/// operation or allocates effect identities/journal rows. Native work/unwind is
/// host-owned and never retried, rolled back or preempted by this module.
/// This is dynamic preparation, not cold static typing; hosts separately use
/// `infer_call_type`. Catalog success does not replace live execution authority,
/// pending-effect correlation or actual returned-value validation.
pub fn prepare_call_in_scope<
    'expression,
    'schema,
    Field,
    Operation,
    HostEffect,
    IrResult,
    Domain,
    ResultTag,
    Capability,
    Environment,
>(
    expression: &'expression Computation<Field, Operation, HostEffect, IrResult>,
    scope: &mut ScopeFrame<'_, 'expression, PureValue<Environment::Result>>,
    host: &CallEvaluationHost<'_, 'schema, Operation, Domain, ResultTag, Capability, Environment>,
    fuel: &mut Fuel,
    limits: CallEvaluationLimits,
) -> Result<
    PreparedCall<'schema, Operation, Domain, ResultTag, Capability>,
    CallEvaluationError<Environment::Error>,
>
where
    Operation: PartialEq,
    Domain: ScalarArgumentDomain,
    Capability: PartialEq,
    Environment: PureEvaluationEnvironment<Field, Operation>,
{
    let Computation::Call {
        operation,
        arguments,
    } = expression
    else {
        return Err(CallEvaluationError::Preflight(CallTypeError::NotCall));
    };
    let names = preflight(arguments, scope, limits)?;
    let schema = host
        .catalog
        .authorize(operation, host.version, host.granted)
        .map_err(|error| CallEvaluationError::Preflight(CallTypeError::Catalog(error)))?;
    preflight_signature(&names, schema, limits.max_arguments)
        .map_err(CallEvaluationError::Preflight)?;
    fuel.charge(1)
        .map_err(|_| CallEvaluationError::FuelExhausted)?;
    evaluate_arguments(
        arguments,
        &names,
        schema,
        scope,
        host.environment,
        fuel,
        limits,
    )
}
