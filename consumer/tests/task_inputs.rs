//! Typed task inputs through the public facade and an independent dependency graph.
use std::cell::{Cell, RefCell};

use leselang_runtime_core::{
    NamedArgumentError, OptionalStringValue, ScalarContractError, StringListValue,
};
use leselang_vm::embedding::{
    EffectSessionEnd, EffectSessionStatus, EntryArgumentError, EntryParameter, Error, Fuel, Host,
    HostBuilder, HostFailure, Limits, Poll, Program, Request, ScalarType, ScalarValue, StartError,
    bind_entry_arguments,
    diagnostics::{self, DiagnosticKind},
    parameter,
};

const GRANTS: &[&str] = &["counter.write"];
const TWO_EFFECTS: &str = r#"
fn increment(amount: integer) = counter.add(amount: amount)
fn task(amount: integer, extra: integer) = bind(
    first: increment(amount: amount),
    body: increment(amount: add(left: first, right: extra)),
)
"#;

fn supplied(name: &str, value: ScalarValue) -> (String, ScalarValue) {
    (name.into(), value)
}

fn counter_host<'a>(
    total: &'a Cell<u64>,
    calls: &'a RefCell<Vec<u64>>,
    uncertain_at: Option<u64>,
) -> Host<'a> {
    let mut builder = HostBuilder::new();
    builder
        .register(
            "counter.add",
            vec![parameter("amount", ScalarType::Integer)],
            ScalarType::Integer,
            "counter.write",
            move |arguments, fuel| {
                let Some(ScalarValue::Integer(amount)) = arguments.get("amount") else {
                    panic!("expected the admitted integer operand");
                };
                fuel.charge(amount + 1)
                    .map_err(|_| HostFailure::Failed("insufficient host fuel".into()))?;
                calls.borrow_mut().push(*amount);
                total.set(total.get() + amount);
                if uncertain_at == Some(*amount) {
                    return Err(HostFailure::Uncertain("acknowledgement lost".into()));
                }
                Ok(ScalarValue::Integer(total.get()))
            },
        )
        .unwrap();
    builder.build()
}

fn request(poll: Poll) -> Request {
    let Poll::Request(request) = poll else {
        panic!("expected a host request");
    };
    request
}

fn inputs(amount: u64, extra: u64) -> Vec<(String, ScalarValue)> {
    vec![
        supplied("amount", ScalarValue::Integer(amount)),
        supplied("extra", ScalarValue::Integer(extra)),
    ]
}

#[test]
fn typed_tasks_are_opt_in_and_expose_owned_required_parameter_metadata() {
    let host = HostBuilder::new().build();
    let source = "fn task(left: integer, right: integer) = sub(left: left, right: right)";
    assert!(host.compile_entry(source, "task", &[]).is_err());
    assert!(host.compile(&source.replace("task", "main"), &[]).is_err());
    let program = {
        let source = source.to_owned();
        let entry = "task".to_owned();
        host.compile_task(&source, &entry, &[]).unwrap()
    };
    assert_eq!(program.entry_name(), "task");
    assert_eq!(program.result_type(), ScalarType::Integer);
    assert_eq!(program.parameters().len(), 2);
    for (parameter, name) in program.parameters().iter().zip(["left", "right"]) {
        assert_eq!(parameter.name, name);
        assert_eq!(parameter.domain, ScalarType::Integer);
        assert!(parameter.required);
    }
    assert!(host.start(&program, &[], Fuel::new(100)).is_err());

    for (left, right, expected) in [(42, 19, 23), (100, 7, 93)] {
        let mut session = host
            .start_with_arguments(
                &program,
                vec![
                    supplied("right", ScalarValue::Integer(right)),
                    supplied("left", ScalarValue::Integer(left)),
                ],
                &[],
                Fuel::new(100),
            )
            .unwrap();
        assert_eq!(session.fuel_remaining(), 100);
        assert!(
            matches!(session.poll().unwrap(), Poll::Value(ScalarValue::Integer(value)) if value == expected)
        );
    }

    for program in [
        host.compile("fn main() = 42", &[]).unwrap(),
        host.compile_task("fn task() = 42", "task", &[]).unwrap(),
    ] {
        assert!(program.parameters().is_empty());
        let mut original = host.start(&program, &[], Fuel::new(100)).unwrap();
        let mut explicit = host
            .start_with_arguments(&program, vec![], &[], Fuel::new(100))
            .unwrap();
        assert!(matches!(
            original.poll().unwrap(),
            Poll::Value(ScalarValue::Integer(42))
        ));
        assert!(matches!(
            explicit.poll().unwrap(),
            Poll::Value(ScalarValue::Integer(42))
        ));
        assert_eq!(original.fuel_remaining(), explicit.fuel_remaining());
        assert!(
            host.start_with_arguments(
                &program,
                vec![supplied("extra", ScalarValue::None)],
                &[],
                Fuel::new(100)
            )
            .is_err()
        );
    }
}

fn scalar_samples() -> Vec<(&'static str, ScalarValue)> {
    vec![
        ("integer", ScalarValue::Integer(u64::MAX)),
        ("boolean", ScalarValue::Boolean(true)),
        (
            "string",
            ScalarValue::String("🦀\"\\\n// input is data".into()),
        ),
        ("none", ScalarValue::None),
        (
            "optional_string",
            ScalarValue::OptionalString(OptionalStringValue(None)),
        ),
        (
            "optional_string",
            ScalarValue::OptionalString(OptionalStringValue(Some(String::new()))),
        ),
        (
            "string_list",
            ScalarValue::StringList(StringListValue(vec!["a".into(), "🦀".into()])),
        ),
    ]
}

#[test]
fn every_scalar_kind_is_supplied_as_data_and_can_reuse_a_compiled_task() {
    let host = HostBuilder::new().build();
    for (kind, value) in scalar_samples() {
        let source = format!("fn task(value: {kind}) = value");
        let program = host.compile_task(&source, "task", &[]).unwrap();
        assert_eq!(program.parameters()[0].domain, value.scalar_type());
        assert_eq!(program.result_type(), value.scalar_type());
        for _ in 0..2 {
            let mut session = host
                .start_with_arguments(
                    &program,
                    vec![supplied("value", value.clone())],
                    &[],
                    Fuel::new(10_000),
                )
                .unwrap();
            assert_eq!(session.fuel_remaining(), 10_000);
            let Poll::Value(actual) = session.poll().unwrap() else {
                panic!("input identity must complete without a host Effect");
            };
            assert_eq!(actual, value);
        }
    }
}

#[test]
fn invalid_argument_shapes_and_types_fail_before_any_host_work() {
    let total = Cell::new(0);
    let calls = RefCell::new(Vec::new());
    let host = counter_host(&total, &calls, None);
    let program = host.compile_task(TWO_EFFECTS, "task", GRANTS).unwrap();
    for arguments in [
        vec![],
        vec![supplied("amount", ScalarValue::Integer(19))],
        vec![
            supplied("amount", ScalarValue::Integer(19)),
            supplied("amount", ScalarValue::Integer(4)),
        ],
        vec![
            supplied("amount", ScalarValue::Integer(19)),
            supplied("wrong", ScalarValue::Integer(4)),
        ],
        vec![
            supplied("amount", ScalarValue::Integer(19)),
            supplied("extra", ScalarValue::Integer(4)),
            supplied("unknown", ScalarValue::None),
        ],
        vec![
            supplied("amount", ScalarValue::Boolean(true)),
            supplied("extra", ScalarValue::Integer(4)),
        ],
        vec![
            supplied("amount", ScalarValue::Integer(19)),
            supplied("extra", ScalarValue::String("4".into())),
        ],
    ] {
        for budget in [0, 1000] {
            assert!(
                host.start_with_arguments(&program, arguments.clone(), GRANTS, Fuel::new(budget))
                    .is_err()
            );
            assert_eq!(total.get(), 0);
            assert!(calls.borrow().is_empty());
        }
    }
    let other = counter_host(&total, &calls, None);
    assert!(
        other
            .start_with_arguments(&program, inputs(19, 4), GRANTS, Fuel::new(1000))
            .is_err()
    );
    assert_eq!(total.get(), 0);
    assert!(calls.borrow().is_empty());
}

#[test]
fn even_unused_inputs_obey_scalar_size_limits_before_starting() {
    let host = HostBuilder::new().build();
    for (kind, valid, invalid) in [
        (
            "string",
            ScalarValue::String("🦀".repeat(1024)),
            ScalarValue::String("🦀".repeat(1024) + "x"),
        ),
        (
            "optional_string",
            ScalarValue::OptionalString(OptionalStringValue(Some("x".repeat(4096)))),
            ScalarValue::OptionalString(OptionalStringValue(Some("x".repeat(4097)))),
        ),
        (
            "string_list",
            ScalarValue::StringList(StringListValue(vec!["".into(); 64])),
            ScalarValue::StringList(StringListValue(vec!["".into(); 65])),
        ),
        (
            "string_list",
            ScalarValue::StringList(StringListValue(vec!["x".repeat(2048); 2])),
            ScalarValue::StringList(StringListValue(vec!["x".repeat(2048), "x".repeat(2049)])),
        ),
    ] {
        let source = format!("fn task(unused: {kind}) = 42");
        let program = host.compile_task(&source, "task", &[]).unwrap();
        assert!(
            host.start_with_arguments(
                &program,
                vec![supplied("unused", invalid)],
                &[],
                Fuel::new(100)
            )
            .is_err()
        );
        let mut session = host
            .start_with_arguments(
                &program,
                vec![supplied("unused", valid)],
                &[],
                Fuel::new(100),
            )
            .unwrap();
        assert_eq!(session.fuel_remaining(), 100);
        assert!(matches!(
            session.poll().unwrap(),
            Poll::Value(ScalarValue::Integer(42))
        ));
    }
}

#[test]
fn startup_errors_preserve_parameter_positions_and_execution_binding_limits() {
    let host = HostBuilder::new().build();
    let program = host
        .compile_task(
            "fn task(first: integer, second: string) = first",
            "task",
            &[],
        )
        .unwrap();
    let error = host
        .start_with_arguments(&program, vec![], &[], Fuel::new(0))
        .err()
        .unwrap();
    assert_eq!(
        error,
        StartError::Arguments(EntryArgumentError::Names(
            NamedArgumentError::MissingArgument { parameter_index: 0 }
        ))
    );
    let error = host
        .start_with_arguments(
            &program,
            vec![
                supplied("second", ScalarValue::Boolean(true)),
                supplied("first", ScalarValue::Integer(1)),
            ],
            &[],
            Fuel::new(0),
        )
        .err()
        .unwrap();
    assert_eq!(
        error,
        StartError::Arguments(EntryArgumentError::Value {
            parameter_index: 1,
            error: ScalarContractError::TypeMismatch
        })
    );
    let error = host
        .start_with_arguments(
            &program,
            vec![
                supplied("second", ScalarValue::String("x".repeat(4097))),
                supplied("first", ScalarValue::Integer(1)),
            ],
            &[],
            Fuel::new(0),
        )
        .err()
        .unwrap();
    assert_eq!(
        error,
        StartError::Arguments(EntryArgumentError::Value {
            parameter_index: 1,
            error: ScalarContractError::UnboundedValue
        })
    );

    let mut limits = Limits::default();
    limits.execution.effects.pure.max_bindings = 1;
    let limited = HostBuilder::with_limits(limits).build();
    let program = limited
        .compile_task(
            "fn task(first: integer, second: string) = first",
            "task",
            &[],
        )
        .unwrap();
    let error = limited
        .start_with_arguments(
            &program,
            vec![
                supplied("first", ScalarValue::Integer(1)),
                supplied("second", ScalarValue::String(String::new())),
            ],
            &[],
            Fuel::new(100),
        )
        .err()
        .unwrap();
    assert_eq!(
        error,
        StartError::Arguments(EntryArgumentError::BindingLimit)
    );
}

#[test]
fn native_argument_binding_moves_payloads_into_declaration_order() {
    let parameters = [
        EntryParameter::required("first".into(), ScalarType::String),
        EntryParameter::required("second".into(), ScalarType::StringList),
    ];
    let text = "owned scalar payload".to_owned();
    let text_ptr = text.as_ptr();
    let items = vec!["owned list member".to_owned()];
    let list_ptr = items.as_ptr();
    let item_ptr = items[0].as_ptr();
    let aligned = bind_entry_arguments(
        &parameters,
        vec![
            supplied("second", ScalarValue::StringList(StringListValue(items))),
            supplied("first", ScalarValue::String(text)),
        ],
        2,
    )
    .unwrap();
    assert_eq!(aligned[0].0, "first");
    assert_eq!(aligned[1].0, "second");
    let ScalarValue::String(text) = &aligned[0].1 else {
        panic!("string input lost its type")
    };
    assert_eq!(text.as_ptr(), text_ptr);
    let ScalarValue::StringList(list) = &aligned[1].1 else {
        panic!("list input lost its type")
    };
    assert_eq!(list.0.as_ptr(), list_ptr);
    assert_eq!(list.0[0].as_ptr(), item_ptr);

    let optional = [EntryParameter::optional(
        "value".into(),
        ScalarType::Integer,
    )];
    assert_eq!(
        bind_entry_arguments(&optional, vec![], 1),
        Err(EntryArgumentError::InvalidSignature)
    );
    let duplicate = [
        EntryParameter::required("same".into(), ScalarType::Integer),
        EntryParameter::required("same".into(), ScalarType::Integer),
    ];
    assert_eq!(
        bind_entry_arguments(&duplicate, vec![], 2),
        Err(EntryArgumentError::Names(
            NamedArgumentError::DuplicateParameter { index: 1 }
        ))
    );
}

#[test]
fn task_compilation_checks_signatures_cold_branches_and_unused_helpers() {
    let total = Cell::new(0);
    let calls = RefCell::new(Vec::new());
    let host = counter_host(&total, &calls, None);
    for (source, grants) in [
        ("fn task(n: missing_type) = 0", GRANTS),
        ("fn task(n: integer, n: integer) = n", GRANTS),
        ("fn task(n: integer) = bind(n: 1, body: n)", GRANTS),
        ("fn task(n: integer) = task(n: n)", GRANTS),
        ("fn task(n: integer) = n\nfn unused() = task(n: 0)", GRANTS),
        (
            "fn task(n: integer) = choose(when: true, then: n, otherwise: counter.add(amount: n))",
            &[][..],
        ),
        (
            "fn task(n: integer) = n\nfn unused() = counter.add(amount: 1)",
            &[][..],
        ),
        (
            "fn task(n: integer) = n\nfn unused() = counter.add(amount: true)",
            GRANTS,
        ),
        (
            "fn task(n: integer) = n\nfn unused() = counter.missing()",
            GRANTS,
        ),
    ] {
        assert!(
            host.compile_task(source, "task", grants).is_err(),
            "{source}"
        );
    }
    assert!(
        host.compile_task("fn task(n: integer) = n", "missing", GRANTS)
            .is_err()
    );
    assert!(calls.borrow().is_empty());
    assert_eq!(total.get(), 0);
}

#[derive(Debug, PartialEq)]
struct Outcome {
    result: Result<ScalarValue, DiagnosticKind>,
    fuel_remaining: u64,
    calls: Vec<u64>,
    total: u64,
}

fn run(
    host: &Host<'_>,
    program: &Program,
    arguments: Vec<(String, ScalarValue)>,
    budget: u64,
    total: &Cell<u64>,
    calls: &RefCell<Vec<u64>>,
) -> Outcome {
    total.set(0);
    calls.borrow_mut().clear();
    let mut session = host
        .start_with_arguments(program, arguments, GRANTS, Fuel::new(budget))
        .unwrap();
    assert_eq!(session.fuel_remaining(), budget);
    assert!(calls.borrow().is_empty());
    let result = (|| -> Result<ScalarValue, Error> {
        loop {
            match session.poll()? {
                Poll::Request(work) => {
                    let authorized = session.authorize(work).map_err(|e| e.error)?;
                    let receipt = session.dispatch(authorized).map_err(|e| e.error)?;
                    session.accept(receipt).map_err(|e| e.error)?;
                }
                Poll::Value(value) => return Ok(value),
                unexpected => panic!("unexpected explicit boundary {unexpected:?}"),
            }
        }
    })()
    .map_err(|error| diagnostics::runtime(&error).kind);
    Outcome {
        result,
        fuel_remaining: session.fuel_remaining(),
        calls: calls.borrow().clone(),
        total: total.get(),
    }
}

#[test]
fn reordered_inputs_keep_two_effects_and_every_fuel_cutoff() {
    let total = Cell::new(0);
    let calls = RefCell::new(Vec::new());
    let host = counter_host(&total, &calls, None);
    let program = host.compile_task(TWO_EFFECTS, "task", GRANTS).unwrap();
    let complete = run(&host, &program, inputs(19, 4), 1000, &total, &calls);
    assert_eq!(complete.result, Ok(ScalarValue::Integer(42)));
    assert_eq!(complete.calls, [19, 23]);
    let cost = 1000 - complete.fuel_remaining;
    let mut partial = false;
    for budget in 0..=cost + 1 {
        let original = run(&host, &program, inputs(19, 4), budget, &total, &calls);
        let mut reversed = inputs(19, 4);
        reversed.reverse();
        assert_eq!(
            original,
            run(&host, &program, reversed, budget, &total, &calls),
            "fuel={budget}"
        );
        partial |= original.result.is_err() && original.calls == [19];
    }
    assert!(
        partial,
        "already committed effects must survive later Fuel exhaustion"
    );
}

#[test]
fn supplied_inputs_are_captured_across_effects_and_accepted_without_hidden_dispatch() {
    let total = Cell::new(0);
    let calls = RefCell::new(Vec::new());
    let host = counter_host(&total, &calls, None);
    let program = host.compile_task(TWO_EFFECTS, "task", GRANTS).unwrap();
    let mut session = host
        .start_with_arguments(&program, inputs(19, 4), GRANTS, Fuel::new(1000))
        .unwrap();
    for (amount, value) in [(19, 19), (23, 42)] {
        let work = request(session.poll().unwrap());
        assert_eq!(
            work.arguments().get("amount"),
            Some(&ScalarValue::Integer(amount))
        );
        let count = calls.borrow().len();
        assert!(matches!(session.poll().unwrap(), Poll::Awaiting));
        let authorized = session.authorize(work).unwrap();
        assert_eq!(calls.borrow().len(), count);
        let receipt = session.dispatch(authorized).unwrap();
        assert_eq!(receipt.value(), &ScalarValue::Integer(value));
        assert_eq!(calls.borrow().len(), count + 1);
        assert!(matches!(session.poll().unwrap(), Poll::Awaiting));
        session.accept(receipt).unwrap();
        assert_eq!(session.status(), EffectSessionStatus::Ready);
        assert_eq!(calls.borrow().len(), count + 1);
    }
    assert!(matches!(
        session.poll().unwrap(),
        Poll::Value(ScalarValue::Integer(42))
    ));
}

#[test]
fn different_input_sessions_cannot_exchange_requests_or_receipts() {
    let total = Cell::new(0);
    let calls = RefCell::new(Vec::new());
    let host = counter_host(&total, &calls, None);
    let program = host
        .compile_task(
            "fn task(amount: integer) = counter.add(amount: amount)",
            "task",
            GRANTS,
        )
        .unwrap();
    let mut first = host
        .start_with_arguments(
            &program,
            vec![supplied("amount", ScalarValue::Integer(19))],
            GRANTS,
            Fuel::new(1000),
        )
        .unwrap();
    let mut second = host
        .start_with_arguments(
            &program,
            vec![supplied("amount", ScalarValue::Integer(23))],
            GRANTS,
            Fuel::new(2000),
        )
        .unwrap();
    let first_work = request(first.poll().unwrap());
    let second_work = request(second.poll().unwrap());
    assert_eq!(
        first_work.arguments().get("amount"),
        Some(&ScalarValue::Integer(19))
    );
    assert_eq!(
        second_work.arguments().get("amount"),
        Some(&ScalarValue::Integer(23))
    );
    let second_fuel = second.fuel_remaining();
    let refused = second.authorize(first_work).unwrap_err();
    let authorized = first.authorize(refused.input).unwrap();
    let receipt = first.dispatch(authorized).unwrap();
    let refused = second.accept(receipt).unwrap_err();
    assert_eq!(second.fuel_remaining(), second_fuel);
    first.accept(refused.input).unwrap();
    assert!(matches!(
        first.poll().unwrap(),
        Poll::Value(ScalarValue::Integer(19))
    ));
    let authorized = second.authorize(second_work).unwrap();
    let receipt = second.dispatch(authorized).unwrap();
    second.accept(receipt).unwrap();
    assert!(matches!(
        second.poll().unwrap(),
        Poll::Value(ScalarValue::Integer(42))
    ));
    assert_eq!(*calls.borrow(), [19, 23]);
}

#[test]
fn inputs_do_not_preserve_revoked_authority_or_revive_cancelled_sessions() {
    let total = Cell::new(0);
    let calls = RefCell::new(Vec::new());
    let host = counter_host(&total, &calls, None);
    let program = host.compile_task(TWO_EFFECTS, "task", GRANTS).unwrap();
    let mut denied = host
        .start_with_arguments(&program, inputs(19, 4), &[], Fuel::new(1000))
        .unwrap();
    assert!(denied.poll().is_err());
    assert!(calls.borrow().is_empty());

    let mut pending = host
        .start_with_arguments(&program, inputs(19, 4), GRANTS, Fuel::new(1000))
        .unwrap();
    let work = request(pending.poll().unwrap());
    pending.set_grants(&[]);
    let rejected = pending.authorize(work).unwrap_err();
    assert!(matches!(rejected.error, Error::Denied));
    pending.set_grants(GRANTS);
    let authorized = pending.authorize(rejected.input).unwrap();
    pending.set_grants(&[]);
    assert!(pending.dispatch(authorized).is_err());
    assert!(pending.cancel());
    assert!(calls.borrow().is_empty());

    let mut committed = host
        .start_with_arguments(&program, inputs(19, 4), GRANTS, Fuel::new(1000))
        .unwrap();
    let work = request(committed.poll().unwrap());
    let authorized = committed.authorize(work).unwrap();
    let receipt = committed.dispatch(authorized).unwrap();
    committed.set_grants(&[]);
    let rejected = committed.accept(receipt).unwrap_err();
    assert_eq!(
        diagnostics::runtime(&rejected.error).kind,
        DiagnosticKind::CapabilityDenied
    );
    assert_eq!(rejected.input.value(), &ScalarValue::Integer(19));
    assert!(committed.cancel());
    let remaining = committed.fuel_remaining();
    committed.set_grants(GRANTS);
    let late = committed.accept(rejected.input).unwrap_err();
    assert_eq!(late.input.value(), &ScalarValue::Integer(19));
    assert_eq!(committed.fuel_remaining(), remaining);
    assert!(matches!(
        committed.poll().unwrap(),
        Poll::Terminal(EffectSessionEnd::Cancelled)
    ));
    assert_eq!(*calls.borrow(), [19]);
    assert_eq!(total.get(), 19);
}

#[test]
fn a_task_host_uncertain_result_keeps_partial_mutations_and_stops_the_tail() {
    let total = Cell::new(0);
    let calls = RefCell::new(Vec::new());
    let host = counter_host(&total, &calls, Some(23));
    let source = "fn task(first: integer, second: integer) = bind(a: counter.add(amount: first), body: bind(b: counter.add(amount: second), body: counter.add(amount: 100)))";
    let program = host.compile_task(source, "task", GRANTS).unwrap();
    let result = run(
        &host,
        &program,
        vec![
            supplied("first", ScalarValue::Integer(19)),
            supplied("second", ScalarValue::Integer(23)),
        ],
        1000,
        &total,
        &calls,
    );
    assert_eq!(result.result, Err(DiagnosticKind::HostUncertain));
    assert_eq!(result.calls, [19, 23]);
    assert_eq!(result.total, 42);
}

// A pure native VM oracle: every effect hook fails if accidentally reached.
mod native {
    use leselang_vm::native::{
        LocalVm,
        core::{CalculationFailure, LocalExecutionScope, ScalarTypeSet, ScopeFrame},
        hir::{
            effect_evaluation::{EffectEvaluationEnvironment, EffectEvaluationLimits},
            effect_reentry::{EffectReentryEnvironment, RestoredEffectBindings},
            effect_session::{EffectCorrelation, EffectSessionEnvironment, EffectSessionPoll},
            ir::Computation,
            pure_evaluation::{PureEvaluationEnvironment, PureValue},
        },
    };

    use super::*;

    type Node = Computation<(), String, (), ()>;
    struct NoEffects;

    impl PureEvaluationEnvironment<(), String> for NoEffects {
        type Result = ();
        type Error = Error;
        fn field(&self, _: &(), _: &()) -> Result<ScalarValue, Error> {
            panic!("unexpected field")
        }
        fn member(&self, _: &(), _: &str, _: &String) -> Result<(), Error> {
            panic!("unexpected member")
        }
    }
    impl<'ir> EffectEvaluationEnvironment<'ir, (), String, (), ()> for NoEffects {
        type Request = ();
        type Capture = RestoredEffectBindings<'ir, ()>;
        fn preflight_effect(&self, _: &'ir Node) -> Result<(), Error> {
            panic!("unexpected effect")
        }
        fn prepare_effect(
            &self,
            _: &'ir Node,
            _: &mut ScopeFrame<'_, 'ir, PureValue<()>>,
            _: &mut Fuel,
        ) -> Result<(), CalculationFailure<Error>> {
            panic!("unexpected prepare")
        }
        fn capture(
            &self,
            _: &'ir str,
            _: &'ir Node,
            _: &ScopeFrame<'_, 'ir, PureValue<()>>,
            _: &mut Fuel,
        ) -> Result<Self::Capture, Error> {
            panic!("unexpected capture")
        }
    }
    impl<'ir> EffectReentryEnvironment<'ir, (), String, (), (), (), ScalarTypeSet, ScalarValue>
        for NoEffects
    {
        fn restore_capture(
            &self,
            _: &(),
            _: &ScalarTypeSet,
            _: Self::Capture,
            _: &mut Fuel,
            _: EffectEvaluationLimits,
        ) -> Result<RestoredEffectBindings<'ir, ()>, CalculationFailure<Error>> {
            panic!("unexpected restore")
        }
        fn bind_reply(
            &self,
            _: &(),
            _: &ScalarTypeSet,
            _: ScalarValue,
            _: &mut Fuel,
        ) -> Result<PureValue<()>, CalculationFailure<Error>> {
            panic!("unexpected reply")
        }
    }
    impl<'ir, 'schema> EffectSessionEnvironment<'ir, 'schema, (), String, (), ()> for NoEffects {
        type Identity = ();
        type Declaration = ScalarTypeSet;
        type Reply = ScalarValue;
        type Dispatch = ();
        fn correlate_request(
            &self,
            _: (),
            _: &mut Fuel,
        ) -> EffectCorrelation<'schema, (), ScalarTypeSet, (), Error> {
            panic!("unexpected correlate")
        }
    }

    pub fn local(value: ScalarValue, budget: u64) -> (Result<ScalarValue, String>, u64) {
        let node = Node::Local {
            name: "value".into(),
        };
        let mut session = LocalVm::new(
            LocalExecutionScope::new(),
            &node,
            vec![("value", PureValue::Scalar(value))],
            Fuel::new(budget),
            Limits::default().execution,
        );
        assert_eq!(session.fuel_remaining(), budget);
        let result = match session.poll(&NoEffects) {
            Ok(EffectSessionPoll::Value(PureValue::Scalar(value))) => Ok(value),
            Err(error) => Err(diagnostics::runtime(&Error::Evaluation(Box::new(error))).message),
            other => panic!("unexpected native outcome {other:?}"),
        };
        (result, session.fuel_remaining())
    }
}

#[test]
fn input_reads_keep_the_original_native_vm_fuel_cost_at_every_cutoff() {
    let host = HostBuilder::new().build();
    let mut samples = scalar_samples();
    samples.extend([
        ("string", ScalarValue::String("x".repeat(4096))),
        (
            "optional_string",
            ScalarValue::OptionalString(OptionalStringValue(Some("x".repeat(4096)))),
        ),
        (
            "string_list",
            ScalarValue::StringList(StringListValue(vec!["x".repeat(64); 64])),
        ),
    ]);
    for (kind, value) in samples {
        let source = format!("fn task(value: {kind}) = value");
        let program = host.compile_task(&source, "task", &[]).unwrap();
        let (complete, remaining) = native::local(value.clone(), 20_000);
        assert_eq!(complete, Ok(value.clone()));
        let cost = 20_000 - remaining;
        for budget in 0..=cost + 1 {
            let expected = native::local(value.clone(), budget);
            let mut session = host
                .start_with_arguments(
                    &program,
                    vec![supplied("value", value.clone())],
                    &[],
                    Fuel::new(budget),
                )
                .unwrap();
            assert_eq!(session.fuel_remaining(), budget);
            let actual = match session.poll() {
                Ok(Poll::Value(value)) => Ok(value),
                Err(error) => Err(diagnostics::runtime(&error).message),
                other => panic!("unexpected task outcome {other:?}"),
            };
            assert_eq!(
                (actual, session.fuel_remaining()),
                expected,
                "kind={kind}, fuel={budget}"
            );
        }
    }
}
