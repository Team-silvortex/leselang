use std::cell::Cell;

use leselang_hir_core::{
    catalog_program::{
        CatalogProgramLimits, CompiledCatalogProgram, compile_catalog_program,
        compile_catalog_typed_program,
    },
    catalog_source::{CatalogSourceEnvironment, CatalogSourceError, CatalogSourceLimits},
    control_source::ControlSourceLimits,
    flow_typing::CallFlowEnvironment,
    helper_declarations::{HelperDeclarationLimits, accept_helper_declarations},
    ir::Computation,
    pure_evaluation::{
        PureEvaluationEnvironment, PureEvaluationLimits, PureValue, evaluate_pure_in_scope,
    },
    pure_typing::{PureType, PureTypeEnvironment},
    source_call::{SourceCallHost, SourceCallLimits},
};
use leselang_runtime_core::{
    Fuel, NamedParameter, OperationCatalog, OperationCatalogLimits, OperationSchema, ScalarType,
    ScalarTypeSet, ScalarValue, ScopeFrame,
};

// Native metadata deliberately has no Debug, serde or public scalar conversion.
#[derive(Clone, PartialEq)]
enum Meta {
    Object,
    Snapshot,
    Integer,
}
type Node = Computation<String, String, (), ()>;
type Output = CompiledCatalogProgram<Node, Meta>;

struct Environment {
    resolutions: Cell<usize>,
}
impl Environment {
    fn new() -> Self {
        Self {
            resolutions: Cell::new(0),
        }
    }
}
impl PureTypeEnvironment<String, String> for Environment {
    type Result = Meta;
    fn field_type(&self, result: &Meta, field: &String) -> Option<ScalarType> {
        (*result == Meta::Snapshot && field == "text").then_some(ScalarType::String)
    }
    fn member_result(&self, _: &Meta, _: &str, _: &String) -> Option<Meta> {
        None
    }
    fn join_results(&self, _: Meta, _: Meta) -> Option<Meta> {
        panic!("helper arguments require exact identity, never a native join")
    }
}
impl<'schema> CallFlowEnvironment<'schema, String, String, Meta> for Environment {
    fn call_result_type(&self, _: &String, declaration: &'schema Meta) -> Option<PureType<Meta>> {
        Some(match declaration {
            Meta::Integer => PureType::Scalar(ScalarType::Integer),
            other => PureType::Result(other.clone()),
        })
    }
    fn call_native_argument_type(&self, operation: &String, name: &str, ty: &Meta) -> bool {
        matches!(operation.as_str(), "gui.observe" | "gui.write")
            && name == "target"
            && *ty == Meta::Object
    }
}
impl<'source, 'schema> CatalogSourceEnvironment<'source, 'schema, String, String, Meta>
    for Environment
{
    type Error = ();
    fn source_type_name(&self, name: &str) -> Option<Meta> {
        self.resolutions.set(self.resolutions.get() + 1);
        match name {
            "object" => Some(Meta::Object),
            "snapshot" => Some(Meta::Snapshot),
            _ => None,
        }
    }
    fn source_operation(&self, original: &String) -> Result<String, ()> {
        Ok(original.clone())
    }
    fn source_field(
        &self,
        result: &Meta,
        name: &'source str,
    ) -> Result<Option<(String, ScalarType)>, ()> {
        Ok(self
            .field_type(result, &name.to_owned())
            .map(|ty| (name.to_owned(), ty)))
    }
    fn source_member(
        &self,
        _: &Meta,
        _: &'source str,
        _: &'source str,
    ) -> Result<Option<(String, Meta)>, ()> {
        Ok(None)
    }
}
impl PureEvaluationEnvironment<String, String> for Environment {
    type Result = ();
    type Error = ();
    fn field(&self, _: &(), _: &String) -> Result<ScalarValue, ()> {
        Err(())
    }
    fn member(&self, _: &(), _: &str, _: &String) -> Result<(), ()> {
        Err(())
    }
}

const LIMITS: CatalogProgramLimits = CatalogProgramLimits {
    expression: CatalogSourceLimits {
        control: ControlSourceLimits {
            source: SourceCallLimits {
                max_source_nodes: 256,
                max_source_depth: 32,
                max_lowered_nodes: 256,
                max_lowered_depth: 32,
                max_arguments: 16,
            },
            max_bindings: 16,
        },
        max_calls: 32,
    },
    max_functions: 16,
    max_parameters: 8,
    max_compilation_nodes: 4096,
    max_expansion_depth: 16,
};

fn compile(
    source: &str,
    entry: &str,
    env: &Environment,
    grants: &[u8],
    limits: CatalogProgramLimits,
    typed: bool,
) -> Result<Output, CatalogSourceError<()>> {
    let parsed = leselang_syntax::parse(source);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let declarations: Vec<_> = parsed.function.iter().chain(&parsed.helpers).collect();
    let observe = [NamedParameter::required("target", ScalarTypeSet::empty())];
    let write = [
        NamedParameter::required("target", ScalarTypeSet::empty()),
        NamedParameter::required("text", ScalarTypeSet::only(ScalarType::String)),
    ];
    let schemas = [
        OperationSchema {
            key: "gui.find".to_owned(),
            parameters: &[],
            result: Meta::Object,
            required_capability: 1,
        },
        OperationSchema {
            key: "gui.observe".to_owned(),
            parameters: &observe,
            result: Meta::Snapshot,
            required_capability: 1,
        },
        OperationSchema {
            key: "gui.write".to_owned(),
            parameters: &write,
            result: Meta::Integer,
            required_capability: 2,
        },
        OperationSchema {
            key: "gui.snapshot".to_owned(),
            parameters: &[],
            result: Meta::Snapshot,
            required_capability: 1,
        },
    ];
    let catalog = OperationCatalog::new(
        1,
        &schemas,
        OperationCatalogLimits {
            max_operations: 4,
            max_parameters_per_operation: 2,
        },
    )
    .unwrap();
    let host = SourceCallHost {
        catalog: &catalog,
        version: 1,
        granted: grants,
    };
    if typed {
        compile_catalog_typed_program(&declarations, entry, limits, &host, env)
    } else {
        compile_catalog_program(&declarations, entry, limits, &host, env).map(
            |(node, result_type)| Output {
                node,
                result_type,
                entry_parameters: vec![],
            },
        )
    }
}

fn cause(error: &CatalogSourceError<()>) -> String {
    use leselang_hir_core::{
        catalog_program::CatalogProgramError, control_source::ControlSourceError,
        helper_source::HelperSourceError, projection_source::ProjectionSourceError,
    };
    match error {
        CatalogSourceError::Control(error) => match error.as_ref() {
            ControlSourceError::Native { error, .. } => cause(error),
            _ => format!("control: {error}"),
        },
        CatalogSourceError::ComputedProjection(error) => match error.as_ref() {
            ProjectionSourceError::Native { error, .. } => cause(error),
            _ => format!("projection: {error}"),
        },
        CatalogSourceError::Type(error) => format!("type: {error:?}"),
        CatalogSourceError::Program(error) => match error.as_ref() {
            CatalogProgramError::Arguments { error, .. } => match error.as_ref() {
                HelperSourceError::Lowering { error, .. } => cause(error),
                _ => format!("arguments: {error}"),
            },
            CatalogProgramError::Hygiene { error, .. } => format!("hygiene: {error}"),
            CatalogProgramError::Bindings { error, .. } => format!("bindings: {error:?}"),
            _ => format!("program: {error}"),
        },
        _ => error.to_string(),
    }
}

#[test]
fn scalar_entry_uses_original_locals_and_owned_signature_without_wrappers() {
    let environment = Environment::new();
    let program = compile(
        "fn task(n: integer) = add(left: n, right: 2)",
        "task",
        &environment,
        &[],
        LIMITS,
        true,
    )
    .unwrap();
    assert_eq!(
        program.entry_parameters,
        [NamedParameter::required(
            "n".to_owned(),
            ScalarType::Integer
        )]
    );
    assert!(matches!(
        program.result_type,
        PureType::Scalar(ScalarType::Integer)
    ));
    assert!(!matches!(program.node, Node::Bind { .. }));
    let mut bindings = vec![("n", PureValue::Scalar(ScalarValue::Integer(40)))];
    let mut scope = ScopeFrame::new(&mut bindings);
    let result = evaluate_pure_in_scope(
        &program.node,
        &mut scope,
        &environment,
        &mut Fuel::new(1000),
        PureEvaluationLimits {
            max_nodes: 256,
            max_depth: 32,
            max_bindings: 16,
        },
    )
    .unwrap();
    assert!(matches!(
        result,
        PureValue::Scalar(ScalarValue::Integer(42))
    ));
    assert_eq!(environment.resolutions.get(), 0);
}

#[test]
fn entry_prefix_counts_toward_active_bindings_and_cannot_be_shadowed() {
    let environment = Environment::new();
    let limits = CatalogProgramLimits {
        expression: CatalogSourceLimits {
            control: ControlSourceLimits {
                max_bindings: 1,
                ..LIMITS.expression.control
            },
            ..LIMITS.expression
        },
        ..LIMITS
    };
    assert!(
        compile(
            "fn task(n: integer) = n",
            "task",
            &environment,
            &[],
            limits,
            true
        )
        .is_ok()
    );
    for source in [
        "fn task(n: integer, other: integer) = n",
        "fn task(n: integer) = bind(other: 2, body: n)",
        "fn task(n: integer) = bind(n: 2, body: n)",
    ] {
        assert!(
            compile(source, "task", &environment, &[], limits, true).is_err(),
            "{source}"
        );
    }
}

#[test]
fn entry_accepts_all_six_closed_scalar_types_in_declaration_order() {
    let environment = Environment::new();
    let program = compile("fn task(a: integer, b: boolean, c: string, d: none, e: optional_string, f: string_list) = c", "task", &environment, &[], LIMITS, true).unwrap();
    let expected = [
        ScalarType::Integer,
        ScalarType::Boolean,
        ScalarType::String,
        ScalarType::None,
        ScalarType::OptionalString,
        ScalarType::StringList,
    ];
    assert_eq!(
        program
            .entry_parameters
            .iter()
            .map(|p| p.domain)
            .collect::<Vec<_>>(),
        expected
    );
    assert!(program.entry_parameters.iter().all(|p| p.required));
    assert_eq!(environment.resolutions.get(), 0);
}

#[test]
fn old_compiler_and_declaration_admission_remain_closed() {
    let environment = Environment::new();
    for source in [
        "fn task(n: integer) = n",
        "fn helper(target: object) = target\nfn task() = 0",
    ] {
        assert!(compile(source, "task", &environment, &[], LIMITS, false).is_err());
        let parsed = leselang_syntax::parse(source);
        let declarations: Vec<_> = parsed.function.iter().chain(&parsed.helpers).collect();
        assert!(
            accept_helper_declarations(
                &declarations,
                "task",
                HelperDeclarationLimits {
                    max_functions: 16,
                    max_parameters: 8,
                    max_source_nodes: 256,
                    max_source_depth: 32
                },
                |_| Ok::<_, ()>(false)
            )
            .is_err()
        );
    }
    assert_eq!(environment.resolutions.get(), 0);
}

#[test]
fn native_helpers_preserve_identity_across_nested_calls_and_hygiene() {
    let environment = Environment::new();
    let source = "fn observe(target: object) = gui.observe(target: target)\nfn read(target: object) = bind(snapshot: observe(target: target), body: field(value: snapshot, name: \"text\"))\nfn task(_lp0: string) = bind(target: gui.find(), body: bind(first: read(target: target), body: gui.write(target: target, text: concat(left: first, right: _lp0))))";
    let program = compile(source, "task", &environment, &[1, 2], LIMITS, true)
        .unwrap_or_else(|error| panic!("{}", cause(&error)));
    assert!(matches!(
        program.result_type,
        PureType::Scalar(ScalarType::Integer)
    ));
    assert_eq!(program.entry_parameters[0].name, "_lp0");
    // One resolution per declaration parameter, even across unused-body checking
    // and multiple hygienic expansions of observe/read.
    assert_eq!(environment.resolutions.get(), 2);
    let mut pending = vec![&program.node];
    let mut operations = Vec::new();
    while let Some(node) = pending.pop() {
        if let Node::Call { operation, .. } = node {
            operations.push(operation.as_str());
        }
        pending.extend(node.children().rev());
    }
    assert_eq!(operations, ["gui.find", "gui.observe", "gui.write"]);
}

#[test]
fn native_inputs_are_exact_pure_and_never_scalar_or_entry_data() {
    let environment = Environment::new();
    for source in [
        "fn task(target: object) = target",
        "fn helper(target: missing_type) = target\nfn task() = 0",
        "fn helper(target: object) = target\nfn task() = helper(target: \"fake\")",
        "fn helper(target: object) = target\nfn task() = helper(target: gui.find())",
        "fn helper(target: object) = target\nfn task() = bind(value: gui.snapshot(), body: helper(target: value))",
        "fn helper(value: snapshot) = value\nfn task() = bind(target: gui.find(), body: helper(value: target))",
        "fn helper(target: object) = target\nfn task() = bind(value: gui.snapshot(), body: choose(when: true, then: 0, otherwise: bind(wrong: helper(target: value), body: 0)))",
    ] {
        assert!(
            compile(source, "task", &environment, &[1, 2], LIMITS, true).is_err(),
            "{source}"
        );
    }
}

#[test]
fn native_snapshot_helper_projects_only_its_declared_fields() {
    let environment = Environment::new();
    let program = compile("fn text(value: snapshot) = field(value: value, name: \"text\")\nfn task() = bind(snapshot: gui.snapshot(), body: text(value: snapshot))", "task", &environment, &[1], LIMITS, true).unwrap();
    assert!(matches!(
        program.result_type,
        PureType::Scalar(ScalarType::String)
    ));
    assert!(
        compile(
            "fn text(value: object) = field(value: value, name: \"text\")\nfn task() = 0",
            "task",
            &environment,
            &[],
            LIMITS,
            true
        )
        .is_err()
    );
}

#[test]
fn complete_cold_forest_authority_and_lexical_checks_still_apply() {
    let environment = Environment::new();
    for source in [
        "fn unused(target: object) = missing()\nfn task() = 0",
        "fn unused(target: object) = gui.write(target: target, text: \"x\")\nfn task() = 0",
        "fn unused(target: object) = choose(when: true, then: 0, otherwise: gui.write(target: target, text: \"x\"))\nfn task() = 0",
        "fn unused(target: object) = outside\nfn task(outside: string) = outside",
        "fn unused(target: object) = task()\nfn task() = 0",
        "fn unused(target: object) = unused(target: target)\nfn task() = 0",
    ] {
        assert!(
            compile(source, "task", &environment, &[1], LIMITS, true).is_err(),
            "{source}"
        );
    }
}

#[test]
fn physical_and_compilation_limits_still_bound_typed_programs() {
    for limits in [
        CatalogProgramLimits {
            max_parameters: 0,
            ..LIMITS
        },
        CatalogProgramLimits {
            max_compilation_nodes: 1,
            ..LIMITS
        },
        CatalogProgramLimits {
            max_expansion_depth: 0,
            ..LIMITS
        },
        CatalogProgramLimits {
            expression: CatalogSourceLimits {
                control: ControlSourceLimits {
                    max_bindings: 0,
                    ..LIMITS.expression.control
                },
                ..LIMITS.expression
            },
            ..LIMITS
        },
    ] {
        let environment = Environment::new();
        assert!(compile("fn helper(target: object) = target\nfn task() = bind(target: gui.find(), body: helper(target: target))", "task", &environment, &[1], limits, true).is_err());
    }
    let environment = Environment::new();
    let limits = CatalogProgramLimits {
        expression: CatalogSourceLimits {
            control: ControlSourceLimits {
                source: SourceCallLimits {
                    max_source_nodes: 0,
                    ..LIMITS.expression.control.source
                },
                ..LIMITS.expression.control
            },
            ..LIMITS.expression
        },
        ..LIMITS
    };
    assert!(
        compile(
            "fn helper(target: object) = target\nfn task() = 0",
            "task",
            &environment,
            &[],
            limits,
            true
        )
        .is_err()
    );
    assert_eq!(environment.resolutions.get(), 0);
}
