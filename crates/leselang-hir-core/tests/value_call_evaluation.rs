use std::{
    cell::RefCell,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

use leselang_hir_core::{
    call_evaluation::*,
    ir::{Computation, ComputedArgument},
    pure_evaluation::{
        PureEvaluationEnvironment, PureEvaluationFault, PureEvaluationLimits, PureValue,
    },
};
use leselang_runtime_core::{
    host_protocol::{LocalObjectOwner, ObjectError, ObjectRef},
    *,
};

type Node = Computation<&'static str, (), (), ()>;
const LIMITS: CallEvaluationLimits = CallEvaluationLimits {
    pure: PureEvaluationLimits {
        max_nodes: 64,
        max_depth: 8,
        max_bindings: 8,
    },
    max_arguments: 4,
};

// Deliberately borrowed and without Debug/serde: preparation must retain native
// aliases, rather than requiring opaque payload formatting or deep copies.
struct Native<'a> {
    object: ObjectRef,
    text: &'a str,
}
struct PrivateError(Rc<()>);
struct Environment<'a> {
    events: &'a RefCell<Vec<&'static str>>,
    marker: Rc<()>,
}
impl<'a> PureEvaluationEnvironment<&'static str, ()> for Environment<'a> {
    type Result = Rc<Native<'a>>;
    type Error = PrivateError;
    fn field(
        &self,
        value: &Self::Result,
        field: &&'static str,
    ) -> Result<ScalarValue, PrivateError> {
        self.events.borrow_mut().push(field);
        match *field {
            "text" => Ok(ScalarValue::String(value.text.to_owned())),
            "oversized" => Ok(ScalarValue::String("x".repeat(MAX_SCALAR_STRING_BYTES + 1))),
            _ => Err(PrivateError(self.marker.clone())),
        }
    }
    fn member(&self, group: &Self::Result, _: &str, _: &()) -> Result<Self::Result, PrivateError> {
        self.events.borrow_mut().push("member");
        Ok(group.clone())
    }
}

#[derive(Clone, Copy)]
enum Domain {
    Object,
    Text,
}
fn local(name: &str) -> Node {
    Node::Local { name: name.into() }
}
fn field(name: &'static str) -> Node {
    Node::Field {
        value: Box::new(local("target")),
        field: name,
    }
}
fn literal(value: ScalarValue) -> Node {
    Node::Literal { value }
}
fn argument(name: &str, value: Node) -> ComputedArgument<Node> {
    ComputedArgument {
        name: name.into(),
        value,
    }
}
fn parameters() -> [NamedParameter<&'static str, Domain>; 3] {
    [
        NamedParameter::required("object", Domain::Object),
        NamedParameter::optional("omitted", Domain::Text),
        NamedParameter::required("text", Domain::Text),
    ]
}

#[test]
fn mixed_operands_preserve_schema_order_native_aliases_borrowed_payloads_and_original_fuel() {
    let owner = LocalObjectOwner::new();
    let scope_owner = LocalExecutionScope::new();
    let lease = scope_owner.lease();
    let text = String::from("ready");
    let native = Rc::new(Native {
        object: owner.reference(&lease).unwrap(),
        text: &text,
    });
    let events = RefCell::new(Vec::new());
    let environment = Environment {
        events: &events,
        marker: Rc::new(()),
    };
    let parameters = parameters();
    let schema = OperationSchema {
        key: (),
        parameters: &parameters,
        result: (),
        required_capability: (),
    };
    let arguments = [
        argument("text", field("text")),
        argument("object", local("target")),
    ];
    let mut bindings = vec![("target", PureValue::Result(native.clone()))];
    let mut scope = ScopeFrame::new(&mut bindings);
    let mut fuel = Fuel::new(100);
    let original_meter = &fuel as *const Fuel;
    let prepared = evaluate_value_call_arguments_in_scope(
        &arguments,
        &schema,
        &mut scope,
        &environment,
        &mut fuel,
        LIMITS,
        |domain, value, fuel| {
            assert_eq!(fuel as *const Fuel, original_meter);
            fuel.charge(3).unwrap();
            match (domain, value) {
                (Domain::Object, PureValue::Result(value)) => {
                    events.borrow_mut().push("object-domain");
                    owner.validate(&value.object, &lease).unwrap();
                }
                (Domain::Text, PureValue::Scalar(ScalarValue::String(value))) => {
                    events.borrow_mut().push("text-domain");
                    assert_eq!(value, "ready");
                }
                _ => panic!("wrong typed operand"),
            }
            Ok::<_, ()>(())
        },
    )
    .unwrap();
    assert!(std::ptr::eq(prepared.schema(), &schema));
    assert_eq!(prepared.arguments().len(), 2);
    let first = &prepared.arguments()[0];
    assert_eq!(
        (first.parameter_index, first.argument_index, first.name),
        (0, 1, "object")
    );
    assert_eq!(first.name.as_ptr(), parameters[0].name.as_ptr());
    let PureValue::Result(value) = &first.value else {
        panic!()
    };
    assert!(Rc::ptr_eq(value, &native));
    assert_eq!(value.text.as_ptr(), text.as_ptr());
    assert_eq!(
        (
            prepared.arguments()[1].parameter_index,
            prepared.arguments()[1].argument_index
        ),
        (2, 0)
    );
    assert_eq!(*events.borrow(), ["text", "object-domain", "text-domain"]);
    assert_eq!(fuel.remaining(), 89); // five pure/copy units and six explicit native validation units.
    assert_eq!(scope.len(), 1);
    assert_eq!(Rc::strong_count(&native), 3);
    assert!(!format!("{:?}", first).contains("ready"));
    let (same_schema, values) = prepared.into_parts();
    assert!(std::ptr::eq(same_schema, &schema));
    assert_eq!(Rc::strong_count(&native), 3);
    drop(values);
    assert_eq!(Rc::strong_count(&native), 2);
}

#[test]
fn complete_signature_and_cold_structure_preflight_precede_native_access() {
    let owner = LocalObjectOwner::new();
    let execution = LocalExecutionScope::new();
    let native = Rc::new(Native {
        object: owner.reference(&execution.lease()).unwrap(),
        text: "ready",
    });
    let events = RefCell::new(Vec::new());
    let environment = Environment {
        events: &events,
        marker: Rc::new(()),
    };
    let parameters = parameters();
    let schema = OperationSchema {
        key: (),
        parameters: &parameters,
        result: (),
        required_capability: (),
    };
    let cases = [
        vec![argument("object", local("target"))],
        vec![
            argument("object", local("target")),
            argument("text", field("text")),
            argument("text", field("text")),
        ],
        vec![
            argument("object", local("target")),
            argument("unknown", field("text")),
        ],
        vec![
            argument("object", local("target")),
            argument("bad-name", field("text")),
        ],
        vec![
            argument("object", local("target")),
            argument(
                "text",
                Node::Choose {
                    when: Box::new(literal(ScalarValue::Boolean(true))),
                    then: Box::new(field("text")),
                    otherwise: Box::new(Node::Call {
                        operation: (),
                        arguments: vec![],
                    }),
                },
            ),
        ],
    ];
    for arguments in cases {
        let mut bindings = vec![("target", PureValue::Result(native.clone()))];
        let mut scope = ScopeFrame::new(&mut bindings);
        let mut fuel = Fuel::new(100);
        let error = evaluate_value_call_arguments_in_scope(
            &arguments,
            &schema,
            &mut scope,
            &environment,
            &mut fuel,
            LIMITS,
            |_, _, _| -> Result<(), ()> { panic!("invalid forest/signature reached validation") },
        )
        .err()
        .unwrap();
        assert!(matches!(
            error,
            ValueCallEvaluationError::Preparation(CallEvaluationError::Preflight(_))
        ));
        assert!(events.borrow().is_empty());
        assert_eq!(fuel.remaining(), 100);
        assert_eq!(Rc::strong_count(&native), 2);
        assert_eq!(scope.len(), 1);
    }
}

#[test]
fn invalid_prefix_and_zero_fuel_do_not_clone_or_validate_native_operands() {
    let owner = LocalObjectOwner::new();
    let execution = LocalExecutionScope::new();
    let native = Rc::new(Native {
        object: owner.reference(&execution.lease()).unwrap(),
        text: "ready",
    });
    let events = RefCell::new(Vec::new());
    let environment = Environment {
        events: &events,
        marker: Rc::new(()),
    };
    let parameters = [NamedParameter::required("object", Domain::Object)];
    let schema = OperationSchema {
        key: (),
        parameters: &parameters,
        result: (),
        required_capability: (),
    };
    let arguments = [argument("object", local("target"))];
    for invalid_prefix in [false, true] {
        let mut bindings = vec![("target", PureValue::Result(native.clone()))];
        if invalid_prefix {
            bindings.push((
                "oversized",
                PureValue::Scalar(ScalarValue::String("x".repeat(MAX_SCALAR_STRING_BYTES + 1))),
            ));
        }
        let mut scope = ScopeFrame::new(&mut bindings);
        let mut fuel = Fuel::new(if invalid_prefix { 100 } else { 0 });
        let remaining = fuel.remaining();
        let error = evaluate_value_call_arguments_in_scope(
            &arguments,
            &schema,
            &mut scope,
            &environment,
            &mut fuel,
            LIMITS,
            |_, _, _| -> Result<(), ()> { panic!("refused execution reached native admission") },
        )
        .err()
        .unwrap();
        if invalid_prefix {
            assert!(matches!(
                error,
                ValueCallEvaluationError::Preparation(CallEvaluationError::Scope(_))
            ));
        } else {
            assert!(matches!(
                error,
                ValueCallEvaluationError::Preparation(CallEvaluationError::Evaluation {
                    failure: CalculationFailure::External(PureEvaluationFault::FuelExhausted),
                    ..
                })
            ));
        }
        assert_eq!(fuel.remaining(), remaining);
        assert_eq!(Rc::strong_count(&native), 2);
        assert!(events.borrow().is_empty());
    }
}

#[test]
fn later_projection_failure_returns_original_native_error_and_drops_partial_aliases() {
    let owner = LocalObjectOwner::new();
    let execution = LocalExecutionScope::new();
    let native = Rc::new(Native {
        object: owner.reference(&execution.lease()).unwrap(),
        text: "ready",
    });
    let events = RefCell::new(Vec::new());
    let marker = Rc::new(());
    let environment = Environment {
        events: &events,
        marker: marker.clone(),
    };
    let parameters = parameters();
    let schema = OperationSchema {
        key: (),
        parameters: &parameters,
        result: (),
        required_capability: (),
    };
    let arguments = [
        argument("text", field("failed")),
        argument("object", local("target")),
    ];
    let mut bindings = vec![("target", PureValue::Result(native.clone()))];
    let mut scope = ScopeFrame::new(&mut bindings);
    let mut fuel = Fuel::new(100);
    let error = evaluate_value_call_arguments_in_scope(
        &arguments,
        &schema,
        &mut scope,
        &environment,
        &mut fuel,
        LIMITS,
        |_, _, _| -> Result<(), ()> { panic!("partial operands reached native admission") },
    )
    .err()
    .unwrap();
    let ValueCallEvaluationError::Preparation(CallEvaluationError::Evaluation {
        parameter_index: 2,
        argument_index: 0,
        failure: CalculationFailure::External(PureEvaluationFault::Native(PrivateError(original))),
    }) = error
    else {
        panic!("original evaluation failure was lost")
    };
    assert!(Rc::ptr_eq(&original, &marker));
    assert_eq!(Rc::strong_count(&native), 2);
    assert_eq!(scope.len(), 1);
    assert_eq!(*events.borrow(), ["failed"]);
    assert_eq!(fuel.remaining(), 97);
}

#[test]
fn scalar_recovery_provenance_and_argument_positions_survive_mixed_preparation() {
    let events = RefCell::new(Vec::new());
    let environment = Environment {
        events: &events,
        marker: Rc::new(()),
    };
    let parameters = [NamedParameter::required("number", Domain::Text)];
    let schema = OperationSchema {
        key: (),
        parameters: &parameters,
        result: (),
        required_capability: (),
    };
    let arguments = [argument(
        "number",
        Node::Unary {
            operator: UnaryOperator::ParseInteger,
            value: Box::new(literal(ScalarValue::String("bad".into()))),
        },
    )];
    let validation_arguments = [argument("number", literal(ScalarValue::Integer(42)))];
    let mut bindings = vec![];
    let mut scope = ScopeFrame::new(&mut bindings);
    let mut fuel = Fuel::new(100);
    let error = evaluate_value_call_arguments_in_scope(
        &arguments,
        &schema,
        &mut scope,
        &environment,
        &mut fuel,
        LIMITS,
        |_, _, _| -> Result<(), ()> { panic!("failed scalar reached validation") },
    )
    .err()
    .unwrap();
    assert!(matches!(
        error,
        ValueCallEvaluationError::Preparation(CallEvaluationError::Evaluation {
            parameter_index: 0,
            argument_index: 0,
            failure: CalculationFailure::Scalar(ScalarError::InvalidIntegerText),
        })
    ));
    let error = evaluate_value_call_arguments_in_scope(
        &validation_arguments,
        &schema,
        &mut scope,
        &environment,
        &mut fuel,
        LIMITS,
        |_, _, _| Err(ScalarError::InvalidIntegerText),
    )
    .err()
    .unwrap();
    assert!(matches!(
        error,
        ValueCallEvaluationError::Argument {
            parameter_index: 0,
            argument_index: 0,
            error: ScalarError::InvalidIntegerText
        }
    ));
}

#[test]
fn oversized_projected_scalar_never_reaches_native_validation() {
    let owner = LocalObjectOwner::new();
    let execution = LocalExecutionScope::new();
    let native = Rc::new(Native {
        object: owner.reference(&execution.lease()).unwrap(),
        text: "ready",
    });
    let events = RefCell::new(Vec::new());
    let environment = Environment {
        events: &events,
        marker: Rc::new(()),
    };
    let parameters = parameters();
    let schema = OperationSchema {
        key: (),
        parameters: &parameters,
        result: (),
        required_capability: (),
    };
    let arguments = [
        argument("object", local("target")),
        argument("text", field("oversized")),
    ];
    let mut bindings = vec![("target", PureValue::Result(native.clone()))];
    let mut scope = ScopeFrame::new(&mut bindings);
    let error = evaluate_value_call_arguments_in_scope(
        &arguments,
        &schema,
        &mut scope,
        &environment,
        &mut Fuel::new(100),
        LIMITS,
        |_, _, _| -> Result<(), ()> { panic!("unbounded scalar reached native validation") },
    )
    .err()
    .unwrap();
    assert!(matches!(
        error,
        ValueCallEvaluationError::Preparation(CallEvaluationError::Evaluation {
            failure: CalculationFailure::External(PureEvaluationFault::InvalidContract),
            ..
        })
    ));
    assert_eq!(Rc::strong_count(&native), 2);
}

#[test]
fn validation_error_or_unwind_occurs_after_all_values_without_leaking_aliases_or_locals() {
    for unwind in [false, true] {
        let owner = LocalObjectOwner::new();
        let execution = LocalExecutionScope::new();
        let native = Rc::new(Native {
            object: owner.reference(&execution.lease()).unwrap(),
            text: "ready",
        });
        let events = RefCell::new(Vec::new());
        let environment = Environment {
            events: &events,
            marker: Rc::new(()),
        };
        let parameters = parameters();
        let schema = OperationSchema {
            key: (),
            parameters: &parameters,
            result: (),
            required_capability: (),
        };
        let arguments = [
            argument(
                "object",
                Node::Bind {
                    name: "temporary".into(),
                    value: Box::new(local("target")),
                    body: Box::new(local("temporary")),
                },
            ),
            argument("text", field("text")),
        ];
        let mut bindings = vec![("target", PureValue::Result(native.clone()))];
        let mut scope = ScopeFrame::new(&mut bindings);
        let mut fuel = Fuel::new(100);
        let marker = Rc::new(());
        let result = catch_unwind(AssertUnwindSafe(|| {
            evaluate_value_call_arguments_in_scope(
                &arguments,
                &schema,
                &mut scope,
                &environment,
                &mut fuel,
                LIMITS,
                |_, _, fuel| {
                    assert_eq!(*events.borrow(), ["text"]);
                    fuel.charge(2).unwrap();
                    if unwind {
                        panic!("native validator panic");
                    }
                    Err(PrivateError(marker.clone()))
                },
            )
        }));
        if unwind {
            assert!(result.is_err());
        } else {
            let ValueCallEvaluationError::Argument {
                parameter_index: 0,
                argument_index: 0,
                error: PrivateError(original),
            } = result.unwrap().err().unwrap()
            else {
                panic!()
            };
            assert!(Rc::ptr_eq(&original, &marker));
        }
        assert_eq!(scope.len(), 1);
        assert!(scope.get("temporary").is_none());
        assert_eq!(Rc::strong_count(&native), 2);
        assert_eq!(fuel.remaining(), 91);
    }
}

#[test]
fn native_admission_checks_current_owner_generation_and_execution_after_evaluation() {
    for reason in 0..4 {
        let owner = LocalObjectOwner::new();
        let foreign = LocalObjectOwner::new();
        let execution = LocalExecutionScope::new();
        let other_execution = LocalExecutionScope::new();
        let lease = execution.lease();
        let native = Rc::new(Native {
            object: owner.reference(&lease).unwrap(),
            text: "ready",
        });
        let events = RefCell::new(Vec::new());
        let environment = Environment {
            events: &events,
            marker: Rc::new(()),
        };
        let parameters = [NamedParameter::required("object", Domain::Object)];
        let schema = OperationSchema {
            key: (),
            parameters: &parameters,
            result: (),
            required_capability: (),
        };
        let arguments = [argument("object", local("target"))];
        let mut bindings = vec![("target", PureValue::Result(native.clone()))];
        let mut scope = ScopeFrame::new(&mut bindings);
        if reason == 2 {
            owner.replace().unwrap();
        }
        if reason == 3 {
            execution.close();
        }
        let error = evaluate_value_call_arguments_in_scope(
            &arguments,
            &schema,
            &mut scope,
            &environment,
            &mut Fuel::new(100),
            LIMITS,
            |_, value, _| {
                let PureValue::Result(value) = value else {
                    panic!()
                };
                match reason {
                    0 => foreign.validate(&value.object, &lease),
                    1 => owner.validate(&value.object, &other_execution.lease()),
                    _ => owner.validate(&value.object, &lease),
                }
            },
        )
        .err()
        .unwrap();
        let ValueCallEvaluationError::Argument { error, .. } = error else {
            panic!()
        };
        assert_eq!(
            error,
            match reason {
                0 => ObjectError::ForeignOwner,
                1 => ObjectError::WrongExecution,
                2 => ObjectError::StaleGeneration,
                _ => ObjectError::ClosedExecution,
            }
        );
        assert_eq!(Rc::strong_count(&native), 2);
    }
}

#[test]
fn native_operands_support_existing_pure_choose_and_member_alias_semantics() {
    let owner = LocalObjectOwner::new();
    let execution = LocalExecutionScope::new();
    let native = Rc::new(Native {
        object: owner.reference(&execution.lease()).unwrap(),
        text: "ready",
    });
    let events = RefCell::new(Vec::new());
    let environment = Environment {
        events: &events,
        marker: Rc::new(()),
    };
    let parameters = [NamedParameter::required("object", Domain::Object)];
    let schema = OperationSchema {
        key: (),
        parameters: &parameters,
        result: (),
        required_capability: (),
    };
    let arguments = [argument(
        "object",
        Node::Choose {
            when: Box::new(literal(ScalarValue::Boolean(false))),
            then: Box::new(local("target")),
            otherwise: Box::new(Node::Member {
                group: "target".into(),
                name: "child".into(),
                operation: (),
            }),
        },
    )];
    let mut bindings = vec![("target", PureValue::Result(native.clone()))];
    let mut scope = ScopeFrame::new(&mut bindings);
    let prepared = evaluate_value_call_arguments_in_scope(
        &arguments,
        &schema,
        &mut scope,
        &environment,
        &mut Fuel::new(100),
        LIMITS,
        |_, _, _| Ok::<_, ()>(()),
    )
    .unwrap();
    let PureValue::Result(value) = &prepared.arguments()[0].value else {
        panic!()
    };
    assert!(Rc::ptr_eq(value, &native));
    assert_eq!(*events.borrow(), ["member"]);
}

#[test]
fn original_scalar_preparation_keeps_its_early_non_scalar_failure_order() {
    let owner = LocalObjectOwner::new();
    let execution = LocalExecutionScope::new();
    let native = Rc::new(Native {
        object: owner.reference(&execution.lease()).unwrap(),
        text: "ready",
    });
    let events = RefCell::new(Vec::new());
    let environment = Environment {
        events: &events,
        marker: Rc::new(()),
    };
    let parameters = [
        NamedParameter::required("object", ScalarTypeSet::only(ScalarType::String)),
        NamedParameter::required("text", ScalarTypeSet::only(ScalarType::String)),
    ];
    let schema = OperationSchema {
        key: (),
        parameters: &parameters,
        result: (),
        required_capability: (),
    };
    let arguments = [
        argument("text", field("text")),
        argument("object", local("target")),
    ];
    let mut bindings = vec![("target", PureValue::Result(native.clone()))];
    let mut scope = ScopeFrame::new(&mut bindings);
    let mut fuel = Fuel::new(100);
    let error = evaluate_call_arguments_in_scope(
        &arguments,
        &schema,
        &mut scope,
        &environment,
        &mut fuel,
        LIMITS,
    )
    .err()
    .unwrap();
    assert!(matches!(
        error,
        CallEvaluationError::NonScalar {
            parameter_index: 0,
            argument_index: 1
        }
    ));
    assert_eq!(fuel.remaining(), 99);
    assert!(events.borrow().is_empty());
    assert_eq!(Rc::strong_count(&native), 2);
}

#[test]
fn all_scalar_operands_retain_the_original_values_indices_and_fuel_charges() {
    let events = RefCell::new(Vec::new());
    let environment = Environment {
        events: &events,
        marker: Rc::new(()),
    };
    let parameters = [
        NamedParameter::required("first", ScalarTypeSet::only(ScalarType::String)),
        NamedParameter::required("second", ScalarTypeSet::only(ScalarType::Integer)),
    ];
    let schema = OperationSchema {
        key: (),
        parameters: &parameters,
        result: (),
        required_capability: (),
    };
    let arguments = [
        argument("second", literal(ScalarValue::Integer(42))),
        argument("first", literal(ScalarValue::String("x".repeat(65)))),
    ];
    let mut bindings = vec![];
    let mut scope = ScopeFrame::new(&mut bindings);
    let mut old_fuel = Fuel::new(100);
    let scalar = evaluate_call_arguments_in_scope(
        &arguments,
        &schema,
        &mut scope,
        &environment,
        &mut old_fuel,
        LIMITS,
    )
    .unwrap();
    let mut fuel = Fuel::new(100);
    let mixed = evaluate_value_call_arguments_in_scope(
        &arguments,
        &schema,
        &mut scope,
        &environment,
        &mut fuel,
        LIMITS,
        |domain, value, _| {
            let PureValue::Scalar(value) = value else {
                panic!()
            };
            check_argument_type(domain, ScalarArgumentType::literal(value))
        },
    )
    .unwrap();
    assert_eq!(fuel.remaining(), old_fuel.remaining());
    for (original, value) in scalar.arguments().iter().zip(mixed.arguments()) {
        assert_eq!(
            (value.parameter_index, value.argument_index, value.name),
            (
                original.parameter_index,
                original.argument_index,
                original.name
            )
        );
        let PureValue::Scalar(value) = &value.value else {
            panic!()
        };
        assert_eq!(value, &original.value);
    }
}

#[test]
fn zero_argument_preparation_runs_no_validator_and_charges_no_call_root() {
    let events = RefCell::new(Vec::new());
    let environment = Environment {
        events: &events,
        marker: Rc::new(()),
    };
    let schema = OperationSchema::<_, _, Domain, _, _> {
        key: (),
        parameters: &[],
        result: (),
        required_capability: (),
    };
    let mut bindings = vec![];
    let mut scope = ScopeFrame::new(&mut bindings);
    let mut fuel = Fuel::new(0);
    let prepared = evaluate_value_call_arguments_in_scope::<_, _, (), (), _, _, _, _, _, ()>(
        &[],
        &schema,
        &mut scope,
        &environment,
        &mut fuel,
        LIMITS,
        |_, _, _| panic!("no operands to validate"),
    )
    .unwrap();
    assert!(prepared.arguments().is_empty());
    assert_eq!(fuel.remaining(), 0);
}
