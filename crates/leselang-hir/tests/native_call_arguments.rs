use std::cell::RefCell;

use leselang_hir::{
    call_typing::{CallTypeError, CallTypeHost, CallTypeLimits, check_call_arguments},
    catalog_source::{CatalogSourceEnvironment, CatalogSourceLimits, compile_catalog_source},
    control_source::ControlSourceLimits,
    flow_typing::{CallFlowEnvironment, CallFlowTypeLimits, infer_call_flow_type},
    ir::{Computation, ComputedArgument},
    pure_typing::{PureType, PureTypeEnvironment, TypeInferenceLimits},
    source_call::{SourceCallError, SourceCallHost, SourceCallLimits, bind_source_arguments},
};
use leselang_runtime_core::*;
use leselang_syntax::{Expression, parse};

type Node = Computation<(), &'static str, (), ()>;
#[derive(Clone, Copy, Debug, PartialEq)]
enum Native {
    Object,
    Snapshot,
}
type Declaration = PureType<Native>;
type Host<'a, 's> = SourceCallHost<'a, 's, &'static str, ScalarTypeSet, Declaration, u8>;
const SOURCE: SourceCallLimits = SourceCallLimits {
    max_source_nodes: 128,
    max_source_depth: 16,
    max_lowered_nodes: 128,
    max_lowered_depth: 16,
    max_arguments: 8,
};
const CALL: CallTypeLimits = CallTypeLimits {
    pure: TypeInferenceLimits {
        max_nodes: 128,
        max_depth: 16,
        max_bindings: 16,
    },
    max_arguments: 8,
};
const FLOW: CallFlowTypeLimits = CallFlowTypeLimits {
    call: CALL,
    max_calls: 8,
};
const COMPILE: CatalogSourceLimits = CatalogSourceLimits {
    control: ControlSourceLimits {
        source: SOURCE,
        max_bindings: 16,
    },
    max_calls: 8,
};

#[derive(Default)]
struct Environment {
    mapped: RefCell<Vec<&'static str>>,
    native: RefCell<Vec<String>>,
}
impl PureTypeEnvironment<(), &'static str> for Environment {
    type Result = Native;
    fn field_type(&self, result: &Native, _: &()) -> Option<ScalarType> {
        (*result == Native::Snapshot).then_some(ScalarType::Integer)
    }
    fn member_result(&self, _: &Native, _: &str, _: &&'static str) -> Option<Native> {
        None
    }
}
impl<'s> CallFlowEnvironment<'s, (), &'static str, Declaration> for Environment {
    fn call_result_type(
        &self,
        _: &&'static str,
        declaration: &'s Declaration,
    ) -> Option<Declaration> {
        Some(declaration.clone())
    }
    fn call_native_argument_type(
        &self,
        operation: &&'static str,
        name: &str,
        result: &Native,
    ) -> bool {
        self.native.borrow_mut().push(format!("{operation}.{name}"));
        matches!(*operation, "gui.observe" | "gui.apply")
            && name == "target"
            && *result == Native::Object
    }
}
impl<'source, 's> CatalogSourceEnvironment<'source, 's, (), &'static str, Declaration>
    for Environment
{
    type Error = ();
    fn source_operation(&self, original: &&'static str) -> Result<&'static str, ()> {
        self.mapped.borrow_mut().push(original);
        Ok(original)
    }
    fn source_field(
        &self,
        result: &Native,
        name: &'source str,
    ) -> Result<Option<((), ScalarType)>, ()> {
        Ok(
            (*result == Native::Snapshot && name == "revision")
                .then_some(((), ScalarType::Integer)),
        )
    }
    fn source_member(
        &self,
        _: &Native,
        _: &'source str,
        _: &'source str,
    ) -> Result<Option<(&'static str, Native)>, ()> {
        Ok(None)
    }
}

struct Legacy;
impl PureTypeEnvironment<(), &'static str> for Legacy {
    type Result = Native;
    fn field_type(&self, _: &Native, _: &()) -> Option<ScalarType> {
        None
    }
    fn member_result(&self, _: &Native, _: &str, _: &&'static str) -> Option<Native> {
        None
    }
}
impl<'s> CallFlowEnvironment<'s, (), &'static str, Declaration> for Legacy {
    fn call_result_type(
        &self,
        _: &&'static str,
        declaration: &'s Declaration,
    ) -> Option<Declaration> {
        Some(declaration.clone())
    }
}
impl<'source, 's> CatalogSourceEnvironment<'source, 's, (), &'static str, Declaration> for Legacy {
    type Error = ();
    fn source_operation(&self, original: &&'static str) -> Result<&'static str, ()> {
        Ok(original)
    }
    fn source_field(&self, _: &Native, _: &'source str) -> Result<Option<((), ScalarType)>, ()> {
        Ok(None)
    }
    fn source_member(
        &self,
        _: &Native,
        _: &'source str,
        _: &'source str,
    ) -> Result<Option<(&'static str, Native)>, ()> {
        Ok(None)
    }
}

fn source(text: &str) -> Expression {
    let tree = parse(&format!("fn main() = {text}"));
    assert!(tree.diagnostics.is_empty());
    tree.function.unwrap().body
}
fn with_host(test: impl FnOnce(&Host<'_, '_>)) {
    let find = [NamedParameter::required(
        "name",
        ScalarTypeSet::only(ScalarType::String),
    )];
    let target = [NamedParameter::required("target", ScalarTypeSet::empty())];
    let apply = [
        NamedParameter::required("target", ScalarTypeSet::empty()),
        NamedParameter::required(
            "expected_revision",
            ScalarTypeSet::only(ScalarType::Integer),
        ),
        NamedParameter::required("text", ScalarTypeSet::only(ScalarType::String)),
    ];
    let schemas = [
        OperationSchema {
            key: "gui.find",
            parameters: &find,
            result: PureType::Result(Native::Object),
            required_capability: 1,
        },
        OperationSchema {
            key: "gui.observe",
            parameters: &target,
            result: PureType::Result(Native::Snapshot),
            required_capability: 2,
        },
        OperationSchema {
            key: "gui.apply",
            parameters: &apply,
            result: PureType::Scalar(ScalarType::Boolean),
            required_capability: 3,
        },
    ];
    let catalog = OperationCatalog::new(
        1,
        &schemas,
        OperationCatalogLimits {
            max_operations: 3,
            max_parameters_per_operation: 3,
        },
    )
    .unwrap();
    test(&SourceCallHost {
        catalog: &catalog,
        version: 1,
        granted: &[1, 2, 3],
    });
}
fn call(value: Node) -> Node {
    Node::Call {
        operation: "gui.observe",
        arguments: vec![ComputedArgument {
            name: "target".into(),
            value,
        }],
    }
}

#[test]
fn default_flow_and_original_scalar_entry_keep_rejecting_native_arguments() {
    with_host(|host| {
        let node = call(Node::Local {
            name: "target".into(),
        });
        let prefix = [("target", PureType::Result(Native::Object))];
        let legacy = CallTypeHost {
            catalog: host.catalog,
            version: 1,
            granted: host.granted,
            environment: &Legacy,
        };
        assert!(infer_call_flow_type(&node, &prefix, &Legacy, &legacy, FLOW).is_err());
        assert!(
            compile_catalog_source::<(), _, (), (), _, _, _, _>(
                &source("gui.observe(target: target)"),
                &[("target", &prefix[0].1)],
                COMPILE,
                host,
                &Legacy,
            )
            .is_err()
        );
        let env = Environment::default();
        let typed = CallTypeHost {
            catalog: host.catalog,
            version: 1,
            granted: host.granted,
            environment: &env,
        };
        assert_eq!(
            infer_call_flow_type(&node, &prefix, &env, &typed, FLOW).unwrap(),
            PureType::Result(Native::Snapshot)
        );
        let Node::Call { arguments, .. } = node else {
            panic!()
        };
        let schema = host.catalog.lookup("gui.observe", 1).unwrap();
        assert!(matches!(
            check_call_arguments(&arguments, schema, &prefix, &env, CALL),
            Err(CallTypeError::Argument {
                error: ArgumentTypeError::NonScalar,
                ..
            })
        ));
    });
}

#[test]
fn scalar_control_arguments_are_unchanged_by_unrelated_native_bindings() {
    with_host(|host| {
        let object = PureType::Result(Native::Object);
        for operand in [
            "\"save\"",
            "bind(name: \"save\", body: name)",
            "choose(when: false, then: \"cancel\", otherwise: \"save\")",
            "bind(flag: false, body: choose(when: flag, then: \"cancel\", otherwise: concat(left: \"sa\", right: \"ve\")))",
            "loop(name: \"save\", while: false, next: name, limit: 1)",
            "fold(name: \"save\", items: strings(), item: \"part\", next: name, limit: 1)",
        ] {
            let text = source(&format!("gui.find(name: {operand})"));
            let scalar = Environment::default();
            let native = Environment::default();
            let expected: (Node, _) =
                compile_catalog_source(&text, &[], COMPILE, host, &scalar).unwrap();
            let actual: (Node, _) =
                compile_catalog_source(&text, &[("unrelated", &object)], COMPILE, host, &native)
                    .unwrap();
            assert_eq!(actual, expected, "{operand}");
            assert_eq!(*scalar.mapped.borrow(), *native.mapped.borrow());
            assert!(native.native.borrow().is_empty());
        }
        for operand in [
            "choose(when: true, then: \"save\", otherwise: 1)",
            "bind(name: \"save\", body: missing)",
            "choose(when: true, then: \"save\", otherwise: gui.find(name: \"save\"))",
        ] {
            let text = source(&format!("gui.find(name: {operand})"));
            for prefix in [&[][..], &[("unrelated", &object)][..]] {
                let env = Environment::default();
                assert!(
                    compile_catalog_source::<(), _, (), (), _, _, _, _>(
                        &text, prefix, COMPILE, host, &env
                    )
                    .is_err()
                );
                assert!(env.mapped.borrow().is_empty());
            }
        }
    });
}

#[test]
fn source_binds_real_native_results_and_keeps_scalar_and_native_types_separate() {
    with_host(|host| {
        let env = Environment::default();
        let text = source(
            "bind(target: gui.find(name: \"save\"), body: bind(before: gui.observe(target: target), body: gui.apply(text: \"ready\", target: target, expected_revision: field(value: before, name: \"revision\"))))",
        );
        let (node, ty): (Node, _) =
            compile_catalog_source(&text, &[], COMPILE, host, &env).unwrap();
        assert_eq!(ty, PureType::Scalar(ScalarType::Boolean));
        assert_eq!(
            env.mapped.borrow().as_slice(),
            ["gui.find", "gui.observe", "gui.apply"]
        );
        let Node::Bind { body, .. } = node else {
            panic!()
        };
        let Node::Bind { body, .. } = *body else {
            panic!()
        };
        let Node::Call { arguments, .. } = *body else {
            panic!()
        };
        assert_eq!(
            arguments
                .iter()
                .map(|argument| argument.name.as_str())
                .collect::<Vec<_>>(),
            ["target", "expected_revision", "text"]
        );
        for argument in ["1", "\"save\"", "true", "none", "snapshot"] {
            let bad = source(&format!("gui.observe(target: {argument})"));
            let snapshot = PureType::Result(Native::Snapshot);
            assert!(
                compile_catalog_source::<(), _, (), (), _, _, _, _>(
                    &bad,
                    &[("snapshot", &snapshot)],
                    COMPILE,
                    host,
                    &env
                )
                .is_err()
            );
        }
    });
}

#[test]
fn native_selection_and_aliasing_are_pure_and_cold_failures_precede_mapping() {
    with_host(|host| {
        let object = PureType::Result(Native::Object);
        let snapshot = PureType::Result(Native::Snapshot);
        let prefix = [
            ("first", &object),
            ("second", &object),
            ("snapshot", &snapshot),
        ];
        let valid = source(
            "gui.observe(target: bind(alias: choose(when: false, then: first, otherwise: second), body: alias))",
        );
        let env = Environment::default();
        assert!(
            compile_catalog_source::<(), _, (), (), _, _, _, _>(
                &valid, &prefix, COMPILE, host, &env
            )
            .is_ok()
        );
        for text in [
            "gui.observe(target: choose(when: true, then: first, otherwise: snapshot))",
            "gui.observe(target: choose(when: true, then: first, otherwise: missing))",
            "gui.observe(target: choose(when: true, then: first, otherwise: gui.find(name: \"save\")))",
            "gui.observe(target: gui.find(name: \"save\"))",
            "choose(when: true, then: gui.observe(target: first), otherwise: missing(target: first))",
        ] {
            let env = Environment::default();
            assert!(
                compile_catalog_source::<(), _, (), (), _, _, _, _>(
                    &source(text),
                    &prefix,
                    COMPILE,
                    host,
                    &env
                )
                .is_err(),
                "{text}"
            );
            assert!(env.mapped.borrow().is_empty(), "{text}");
        }
    });
}

#[test]
fn generalized_source_binder_checks_purity_and_shared_budget_before_admission() {
    with_host(|host| {
        let input = source("gui.observe(target: source)");
        let Expression::Call { arguments, .. } = input else {
            panic!()
        };
        let schema = host.catalog.lookup("gui.observe", 1).unwrap();
        for (node, max_nodes) in [
            (
                call(Node::Literal {
                    value: ScalarValue::None,
                }),
                128,
            ),
            (
                Node::Local {
                    name: "source".into(),
                },
                1,
            ),
        ] {
            let bound = bind_source_arguments::<_, _, _, _, ()>(
                &arguments,
                schema,
                SourceCallLimits {
                    max_lowered_nodes: max_nodes,
                    ..SOURCE
                },
            )
            .unwrap();
            let mut original = Some(node);
            let result = bound.lower_with(
                |_| Ok::<_, ()>((original.take().unwrap(), Native::Object)),
                |_, _, _, _| panic!("invalid operand reached native admission"),
            );
            assert!(matches!(
                result,
                Err(SourceCallError::Argument {
                    error: ArgumentTypeError::Impure,
                    ..
                } | SourceCallError::Output { .. })
            ));
        }
    });
}

#[test]
fn rejected_native_local_does_not_clone_private_type_metadata() {
    #[derive(PartialEq)]
    struct Private;
    impl Clone for Private {
        fn clone(&self) -> Self {
            panic!("rejected native metadata must remain borrowed")
        }
    }
    struct Reject;
    impl PureTypeEnvironment<(), &'static str> for Reject {
        type Result = Private;
        fn field_type(&self, _: &Private, _: &()) -> Option<ScalarType> {
            None
        }
        fn member_result(&self, _: &Private, _: &str, _: &&'static str) -> Option<Private> {
            None
        }
    }
    impl<'s> CallFlowEnvironment<'s, (), &'static str, ()> for Reject {
        fn call_result_type(&self, _: &&'static str, _: &'s ()) -> Option<PureType<Private>> {
            None
        }
    }
    impl<'source, 's> CatalogSourceEnvironment<'source, 's, (), &'static str, ()> for Reject {
        type Error = ();
        fn source_operation(&self, _: &&'static str) -> Result<&'static str, ()> {
            panic!("rejected operand must precede operation mapping")
        }
        fn source_field(
            &self,
            _: &Private,
            _: &'source str,
        ) -> Result<Option<((), ScalarType)>, ()> {
            Ok(None)
        }
        fn source_member(
            &self,
            _: &Private,
            _: &'source str,
            _: &'source str,
        ) -> Result<Option<(&'static str, Private)>, ()> {
            Ok(None)
        }
    }
    let parameters = [NamedParameter::required("target", ScalarTypeSet::empty())];
    let schemas = [OperationSchema {
        key: "gui.observe",
        parameters: &parameters,
        result: (),
        required_capability: (),
    }];
    let catalog = OperationCatalog::new(
        1,
        &schemas,
        OperationCatalogLimits {
            max_operations: 1,
            max_parameters_per_operation: 1,
        },
    )
    .unwrap();
    let host = SourceCallHost {
        catalog: &catalog,
        version: 1,
        granted: &[()],
    };
    let private = PureType::Result(Private);
    assert!(
        compile_catalog_source::<(), _, (), (), _, _, _, _>(
            &source("gui.observe(target: value)"),
            &[("value", &private)],
            COMPILE,
            &host,
            &Reject,
        )
        .is_err()
    );
}
