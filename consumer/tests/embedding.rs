use std::{
    cell::Cell,
    panic::{AssertUnwindSafe, catch_unwind},
};

use leselang_vm::embedding::{
    Arguments, CompileError, EffectSessionEnd, EffectSessionStatus, Error, Fuel, Host, HostBuilder,
    HostFailure, Limits, NamedParameter, Poll, Request, ScalarType, ScalarTypeSet, ScalarValue,
    parameter,
};

const GRANTS: &[&str] = &["counter.write"];
const TWO_EFFECTS: &str = r#"
fn increment(n: integer) = bind(receipt: counter.add(amount: n), body: receipt)
fn main() = bind(first: increment(n: 19), body: increment(n: add(left: first, right: 4)))
"#;
const ONE_EFFECT: &str = "fn main() = counter.add(amount: 19)";

#[test]
fn host_discovery_uses_the_registered_contract_without_granting_or_invoking_operations() {
    let calls = Cell::new(0);
    let mut builder = HostBuilder::new();
    builder
        .register(
            "surface.observe",
            vec![],
            ScalarType::String,
            "surface.read",
            |_, _| {
                calls.set(calls.get() + 1);
                Ok(ScalarValue::String("ready".into()))
            },
        )
        .unwrap();
    builder
        .register(
            "surface.apply",
            vec![
                parameter("expected_revision", ScalarType::Integer),
                NamedParameter::optional("text".into(), ScalarTypeSet::only(ScalarType::String)),
            ],
            ScalarType::Boolean,
            "surface.write",
            |_, _| panic!("discovery must not dispatch"),
        )
        .unwrap();
    let host = builder.build();
    let description = host.describe();
    assert_eq!(
        description.protocol_version(),
        leselang_vm::embedding::ProtocolVersion::V1
    );
    assert_eq!(description.operations().len(), 2);
    let observe = &description.operations()[0];
    assert_eq!(observe.name(), "surface.observe");
    assert_eq!(observe.required_capability(), "surface.read");
    assert_eq!(observe.result_type(), ScalarType::String);
    assert!(observe.parameters().is_empty());
    let apply = &description.operations()[1];
    assert_eq!(apply.name(), "surface.apply");
    assert_eq!(apply.required_capability(), "surface.write");
    assert_eq!(apply.parameters()[0].name, "expected_revision");
    assert!(apply.parameters()[0].required);
    assert_eq!(
        apply.parameters()[1].domain,
        ScalarTypeSet::only(ScalarType::String)
    );
    assert!(!apply.parameters()[1].required);
    assert_eq!(calls.get(), 0);
    assert!(host.compile("fn main() = surface.observe()", &[]).is_err());
    let program = host
        .compile("fn main() = surface.observe()", &["surface.read"])
        .unwrap();
    let mut session = host.start(&program, &[], Fuel::new(1000)).unwrap();
    assert!(session.poll().is_err());
    assert_eq!(calls.get(), 0);
}

fn integer(arguments: &Arguments, name: &str) -> u64 {
    let Some(ScalarValue::Integer(value)) = arguments.get(name) else {
        panic!("expected integer argument");
    };
    *value
}

fn counter_host<'a>(total: &'a Cell<u64>, calls: &'a Cell<u64>) -> Host<'a> {
    let mut builder = HostBuilder::new();
    builder
        .register(
            "counter.add",
            vec![parameter("amount", ScalarType::Integer)],
            ScalarType::Integer,
            "counter.write",
            move |arguments, _| {
                calls.set(calls.get() + 1);
                total.set(total.get() + integer(arguments, "amount"));
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

#[test]
fn two_effects_require_every_explicit_step_and_reuse_the_original_session() {
    let total = Cell::new(0);
    let calls = Cell::new(0);
    let host = counter_host(&total, &calls);
    let program = host.compile(TWO_EFFECTS, GRANTS).unwrap();
    let mut session = host.start(&program, GRANTS, Fuel::new(1000)).unwrap();
    let lease = session.lease();
    assert_eq!(session.status(), EffectSessionStatus::Ready);
    assert_eq!(calls.get(), 0);

    for (amount, expected_total) in [(19, 19), (23, 42)] {
        let work = request(session.poll().unwrap());
        assert_eq!(integer(work.arguments(), "amount"), amount);
        let fuel = session.fuel_remaining();
        let before_calls = calls.get();
        assert!(matches!(session.poll().unwrap(), Poll::Awaiting));
        assert_eq!(session.fuel_remaining(), fuel);
        assert_eq!(calls.get(), before_calls);

        let authorized = session.authorize(work).unwrap();
        assert_eq!(calls.get(), before_calls);
        let receipt = session.dispatch(authorized).unwrap();
        assert_eq!(receipt.value(), &ScalarValue::Integer(expected_total));
        assert_eq!(calls.get(), before_calls + 1);
        assert_eq!(session.status(), EffectSessionStatus::Awaiting);
        assert!(matches!(session.poll().unwrap(), Poll::Awaiting));

        session.accept(receipt).unwrap();
        assert_eq!(session.status(), EffectSessionStatus::Ready);
        assert_eq!(calls.get(), before_calls + 1);
        assert!(lease.is_open());
    }
    assert!(matches!(
        session.poll().unwrap(),
        Poll::Value(ScalarValue::Integer(42))
    ));
    assert_eq!(total.get(), 42);
    assert_eq!(calls.get(), 2);
    assert!(!lease.is_open());
    let remaining = session.fuel_remaining();
    assert!(remaining < 1000);
    assert!(!session.cancel());
    assert!(matches!(
        session.poll().unwrap(),
        Poll::Terminal(EffectSessionEnd::Completed)
    ));
    assert_eq!(session.fuel_remaining(), remaining);
}

#[test]
fn registration_rejects_duplicates_without_corrupting_existing_operations() {
    let calls = Cell::new(0);
    let mut builder = HostBuilder::new();
    builder
        .register("read", vec![], ScalarType::Integer, "read", |_, _| {
            calls.set(calls.get() + 1);
            Ok(ScalarValue::Integer(7))
        })
        .unwrap();
    assert!(
        builder
            .register("read", vec![], ScalarType::Boolean, "read", |_, _| {
                panic!("duplicate registration replaced original")
            })
            .is_err()
    );
    assert!(
        builder
            .register(
                "invalid",
                vec![
                    parameter("n", ScalarType::Integer),
                    parameter("n", ScalarType::Boolean),
                ],
                ScalarType::Integer,
                "read",
                |_, _| panic!("invalid registration was admitted"),
            )
            .is_err()
    );
    let host = builder.build();
    let program = host.compile("fn main() = read()", &["read"]).unwrap();
    assert!(
        host.compile("fn main() = invalid(n: 1)", &["read"])
            .is_err()
    );
    let mut session = host.start(&program, &["read"], Fuel::new(1000)).unwrap();
    let work = request(session.poll().unwrap());
    let work = session.authorize(work).unwrap();
    let receipt = session.dispatch(work).unwrap();
    session.accept(receipt).unwrap();
    assert!(matches!(
        session.poll().unwrap(),
        Poll::Value(ScalarValue::Integer(7))
    ));
    assert_eq!(calls.get(), 1);
}

#[test]
fn cold_compilation_rejects_invalid_calls_and_denied_capabilities_without_dispatch() {
    let total = Cell::new(0);
    let calls = Cell::new(0);
    let host = counter_host(&total, &calls);
    for source in [
        "fn main() =",
        "fn main() = missing()",
        "fn main() = counter.add(amount: true)",
        "fn main() = counter.add(wrong: 1)",
        "fn main() = counter.add()",
        "fn main() = counter.add(amount: 1, amount: 2)",
        "fn main() = choose(when: true, then: 1, otherwise: missing())",
        "fn main() = 1\nfn unused() = missing()",
    ] {
        assert!(host.compile(source, GRANTS).is_err(), "{source}");
    }
    assert!(host.compile(ONE_EFFECT, &[]).is_err());
    assert_eq!(calls.get(), 0);
    assert_eq!(total.get(), 0);
}

#[test]
fn syntax_errors_preserve_structured_codes_messages_and_source_spans() {
    let host = HostBuilder::new().build();
    let source = "fn main() = @";
    let Err(CompileError::Syntax(diagnostics)) = host.compile(source, &[]) else {
        panic!("expected parser diagnostics");
    };
    let unknown = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "LSE0002")
        .unwrap();
    assert!(!unknown.message.is_empty());
    assert_eq!(&source[unknown.span.start..unknown.span.end], "@");
}

#[test]
fn registration_and_source_limits_are_enforced_without_callbacks() {
    let mut limits = Limits::default();
    limits.catalog.max_operations = 1;
    limits.catalog.max_parameters_per_operation = 1;
    let mut builder = HostBuilder::with_limits(limits);
    for name in [
        "",
        ".bad",
        "bad.",
        "bad..name",
        "has space",
        "9bad",
        "main",
        "add",
        "parse_integer",
        "seq",
        "recover",
        "counter.true",
        "fn.read",
        "true",
    ] {
        assert!(
            builder
                .register(name, vec![], ScalarType::Integer, "write", |_, _| {
                    panic!("invalid operation was invoked")
                })
                .is_err()
        );
    }
    for name in ["bad-name".to_owned(), "none".to_owned(), "a".repeat(65)] {
        assert!(
            builder
                .register(
                    "invalid",
                    vec![parameter(name, ScalarType::Integer)],
                    ScalarType::Integer,
                    "write",
                    |_, _| { panic!("invalid parameter was admitted") }
                )
                .is_err()
        );
    }
    assert!(
        builder
            .register(
                "large",
                vec![
                    parameter("a", ScalarType::Integer),
                    parameter("b", ScalarType::Integer)
                ],
                ScalarType::Integer,
                "write",
                |_, _| { panic!("parameter count limit was bypassed") }
            )
            .is_err()
    );
    builder
        .register("one", vec![], ScalarType::Integer, "write", |_, _| {
            Ok(ScalarValue::Integer(1))
        })
        .unwrap();
    assert!(
        builder
            .register("two", vec![], ScalarType::Integer, "write", |_, _| {
                panic!("operation count limit was bypassed")
            })
            .is_err()
    );
    let host = builder.build();
    assert!(host.compile("fn main() = one()", &["write"]).is_ok());

    let limits = Limits {
        max_source_bytes: 1,
        ..Limits::default()
    };
    let host = HostBuilder::with_limits(limits).build();
    assert!(host.compile("fn main() = 1", &[]).is_err());
    let mut limits = Limits::default();
    limits.compilation.max_compilation_nodes = 0;
    let host = HostBuilder::with_limits(limits).build();
    assert!(host.compile("fn main() = 1", &[]).is_err());
}

#[test]
fn namespaced_language_names_remain_usable_host_operations() {
    let mut builder = HostBuilder::new();
    builder
        .register(
            "counter.main",
            vec![parameter("body", ScalarType::Integer)],
            ScalarType::Integer,
            "write",
            |arguments, _| Ok(ScalarValue::Integer(integer(arguments, "body"))),
        )
        .unwrap();
    let host = builder.build();
    let program = host
        .compile("fn main() = counter.main(body: 42)", &["write"])
        .unwrap();
    let mut session = host.start(&program, &["write"], Fuel::new(1000)).unwrap();
    let work = request(session.poll().unwrap());
    assert_eq!(work.operation(), "counter.main");
    let authorized = session.authorize(work).unwrap();
    let receipt = session.dispatch(authorized).unwrap();
    session.accept(receipt).unwrap();
    assert!(matches!(
        session.poll().unwrap(),
        Poll::Value(ScalarValue::Integer(42))
    ));
}

#[test]
fn execution_limits_refuse_before_dispatch_and_pure_programs_need_no_host_operations() {
    let host = HostBuilder::new().build();
    let program = host
        .compile("fn main() = add(left: 19, right: 23)", &[])
        .unwrap();
    let mut session = host.start(&program, &[], Fuel::new(1000)).unwrap();
    assert!(matches!(
        session.poll().unwrap(),
        Poll::Value(ScalarValue::Integer(42))
    ));

    let mut limits = Limits::default();
    limits.execution.effects.pure.max_nodes = 0;
    let mut builder = HostBuilder::with_limits(limits);
    builder
        .register("refuse", vec![], ScalarType::Integer, "read", |_, _| {
            panic!("execution structural limit was bypassed")
        })
        .unwrap();
    let host = builder.build();
    let program = host.compile("fn main() = refuse()", &["read"]).unwrap();
    let mut session = host.start(&program, &["read"], Fuel::new(1000)).unwrap();
    assert!(session.poll().is_err());
    assert!(matches!(
        session.poll().unwrap(),
        Poll::Terminal(EffectSessionEnd::Failed)
    ));
    assert!(!session.lease().is_open());
}

#[test]
fn named_and_optional_parameters_are_bound_through_the_existing_catalog() {
    let mut builder = HostBuilder::new();
    builder
        .register(
            "math.combine",
            vec![
                parameter("left", ScalarType::Integer),
                parameter("right", ScalarType::Integer),
                NamedParameter::optional("extra".into(), ScalarTypeSet::only(ScalarType::Integer)),
            ],
            ScalarType::Integer,
            "math",
            |arguments, _| {
                assert!(arguments.get("extra").is_none());
                Ok(ScalarValue::Integer(
                    integer(arguments, "left") * 10 + integer(arguments, "right"),
                ))
            },
        )
        .unwrap();
    let host = builder.build();
    let program = host
        .compile("fn main() = math.combine(right: 2, left: 4)", &["math"])
        .unwrap();
    let mut session = host.start(&program, &["math"], Fuel::new(1000)).unwrap();
    let work = request(session.poll().unwrap());
    let authorized = session.authorize(work).unwrap();
    let receipt = session.dispatch(authorized).unwrap();
    session.accept(receipt).unwrap();
    assert!(matches!(
        session.poll().unwrap(),
        Poll::Value(ScalarValue::Integer(42))
    ));
}

#[test]
fn program_cannot_be_started_against_a_different_host() {
    let first_total = Cell::new(0);
    let first_calls = Cell::new(0);
    let second_total = Cell::new(0);
    let second_calls = Cell::new(0);
    let first = counter_host(&first_total, &first_calls);
    let second = counter_host(&second_total, &second_calls);
    let program = first.compile(ONE_EFFECT, GRANTS).unwrap();
    assert!(second.start(&program, GRANTS, Fuel::new(1000)).is_err());
    assert_eq!(second_calls.get(), 0);
}

#[test]
fn denied_poll_is_terminal_and_never_invokes_the_host() {
    let total = Cell::new(0);
    let calls = Cell::new(0);
    let host = counter_host(&total, &calls);
    let program = host.compile(TWO_EFFECTS, GRANTS).unwrap();
    let mut session = host.start(&program, GRANTS, Fuel::new(1000)).unwrap();
    session.set_grants(&[]);
    assert!(session.poll().is_err());
    assert_eq!(
        session.status(),
        EffectSessionStatus::Terminal(EffectSessionEnd::Failed)
    );
    assert!(!session.lease().is_open());
    session.set_grants(GRANTS);
    assert!(matches!(
        session.poll().unwrap(),
        Poll::Terminal(EffectSessionEnd::Failed)
    ));
    assert_eq!(calls.get(), 0);
}

#[test]
fn refused_authorization_returns_the_original_request_without_dispatch() {
    let total = Cell::new(0);
    let calls = Cell::new(0);
    let host = counter_host(&total, &calls);
    let program = host.compile(ONE_EFFECT, GRANTS).unwrap();
    let mut session = host.start(&program, GRANTS, Fuel::new(1000)).unwrap();
    let work = request(session.poll().unwrap());
    let fuel = session.fuel_remaining();
    session.set_grants(&[]);
    let rejected = session.authorize(work).err().unwrap();
    assert_eq!(integer(rejected.input.arguments(), "amount"), 19);
    assert_eq!(session.status(), EffectSessionStatus::Awaiting);
    assert_eq!(session.fuel_remaining(), fuel);
    assert_eq!(calls.get(), 0);
    session.set_grants(GRANTS);
    let authorized = session.authorize(rejected.input).unwrap();
    let receipt = session.dispatch(authorized).unwrap();
    session.accept(receipt).unwrap();
    assert!(matches!(
        session.poll().unwrap(),
        Poll::Value(ScalarValue::Integer(19))
    ));
}

#[test]
fn revocation_after_authorization_is_rechecked_immediately_before_dispatch() {
    let total = Cell::new(0);
    let calls = Cell::new(0);
    let host = counter_host(&total, &calls);
    let program = host.compile(TWO_EFFECTS, GRANTS).unwrap();
    let mut session = host.start(&program, GRANTS, Fuel::new(1000)).unwrap();
    let work = request(session.poll().unwrap());
    let authorized = session.authorize(work).unwrap();
    let fuel = session.fuel_remaining();
    session.set_grants(&[]);
    assert!(session.dispatch(authorized).is_err());
    assert_eq!(calls.get(), 0);
    assert_eq!(total.get(), 0);
    assert_eq!(session.fuel_remaining(), fuel);
    assert!(session.cancel());
}

#[test]
fn receipt_refused_after_revocation_can_be_returned_without_replaying_the_effect() {
    let total = Cell::new(0);
    let calls = Cell::new(0);
    let host = counter_host(&total, &calls);
    let program = host.compile(ONE_EFFECT, GRANTS).unwrap();
    let mut session = host.start(&program, GRANTS, Fuel::new(1000)).unwrap();
    let work = request(session.poll().unwrap());
    let authorized = session.authorize(work).unwrap();
    let receipt = session.dispatch(authorized).unwrap();
    let fuel = session.fuel_remaining();
    session.set_grants(&[]);
    let rejected = session.accept(receipt).err().unwrap();
    assert_eq!(rejected.input.value(), &ScalarValue::Integer(19));
    assert_eq!(session.status(), EffectSessionStatus::Awaiting);
    assert_eq!(session.fuel_remaining(), fuel);
    assert_eq!(calls.get(), 1);
    session.set_grants(GRANTS);
    session.accept(rejected.input).unwrap();
    assert!(matches!(
        session.poll().unwrap(),
        Poll::Value(ScalarValue::Integer(19))
    ));
    assert_eq!(calls.get(), 1);
}

#[test]
fn revocation_after_acceptance_fails_reentry_without_replaying_or_running_the_tail() {
    let total = Cell::new(0);
    let calls = Cell::new(0);
    let host = counter_host(&total, &calls);
    let program = host.compile(TWO_EFFECTS, GRANTS).unwrap();
    let mut session = host.start(&program, GRANTS, Fuel::new(1000)).unwrap();
    let work = request(session.poll().unwrap());
    let authorized = session.authorize(work).unwrap();
    let receipt = session.dispatch(authorized).unwrap();
    session.accept(receipt).unwrap();
    session.set_grants(&[]);
    assert!(session.poll().is_err());
    assert_eq!(
        session.status(),
        EffectSessionStatus::Terminal(EffectSessionEnd::Failed)
    );
    assert_eq!(total.get(), 19);
    assert_eq!(calls.get(), 1);
    session.set_grants(GRANTS);
    assert!(matches!(
        session.poll().unwrap(),
        Poll::Terminal(EffectSessionEnd::Failed)
    ));
    assert_eq!(calls.get(), 1);
}

#[test]
fn request_authorization_and_receipt_are_bound_to_their_own_session() {
    let total = Cell::new(0);
    let calls = Cell::new(0);
    let host = counter_host(&total, &calls);
    let program = host.compile(ONE_EFFECT, GRANTS).unwrap();
    let mut first = host.start(&program, GRANTS, Fuel::new(1000)).unwrap();
    let mut second = host.start(&program, GRANTS, Fuel::new(1000)).unwrap();
    let first_work = request(first.poll().unwrap());
    let second_work = request(second.poll().unwrap());
    let rejected = second.authorize(first_work).err().unwrap();
    assert_eq!(calls.get(), 0);
    let authorized = first.authorize(rejected.input).unwrap();
    let receipt = first.dispatch(authorized).unwrap();
    let rejected = second.accept(receipt).err().unwrap();
    assert_eq!(second.status(), EffectSessionStatus::Awaiting);
    first.accept(rejected.input).unwrap();
    assert!(matches!(
        first.poll().unwrap(),
        Poll::Value(ScalarValue::Integer(19))
    ));
    let authorized = second.authorize(second_work).unwrap();
    let receipt = second.dispatch(authorized).unwrap();
    second.accept(receipt).unwrap();
    assert!(matches!(
        second.poll().unwrap(),
        Poll::Value(ScalarValue::Integer(38))
    ));
    assert_eq!(calls.get(), 2);
}

#[test]
fn dispatch_refuses_an_authorized_request_from_another_session() {
    let total = Cell::new(0);
    let calls = Cell::new(0);
    let host = counter_host(&total, &calls);
    let program = host.compile(ONE_EFFECT, GRANTS).unwrap();
    let mut first = host.start(&program, GRANTS, Fuel::new(1000)).unwrap();
    let mut second = host.start(&program, GRANTS, Fuel::new(1000)).unwrap();
    let first_work = request(first.poll().unwrap());
    let second_work = request(second.poll().unwrap());
    let authorized = first.authorize(first_work).unwrap();
    assert!(second.dispatch(authorized).is_err());
    assert_eq!(calls.get(), 0);
    let authorized = second.authorize(second_work).unwrap();
    let receipt = second.dispatch(authorized).unwrap();
    second.accept(receipt).unwrap();
    assert!(matches!(
        second.poll().unwrap(),
        Poll::Value(ScalarValue::Integer(19))
    ));
    assert!(first.cancel());
}

#[test]
fn sessions_sharing_a_program_can_dispatch_and_finish_in_reverse_order() {
    let total = Cell::new(0);
    let calls = Cell::new(0);
    let host = counter_host(&total, &calls);
    let program = host.compile(ONE_EFFECT, GRANTS).unwrap();
    let mut first = host.start(&program, GRANTS, Fuel::new(1000)).unwrap();
    let mut second = host.start(&program, GRANTS, Fuel::new(1000)).unwrap();
    let first_work = request(first.poll().unwrap());
    let second_work = request(second.poll().unwrap());
    let first_fuel = first.fuel_remaining();
    let authorized = second.authorize(second_work).unwrap();
    let receipt = second.dispatch(authorized).unwrap();
    second.accept(receipt).unwrap();
    assert!(matches!(
        second.poll().unwrap(),
        Poll::Value(ScalarValue::Integer(19))
    ));
    assert_eq!(first.fuel_remaining(), first_fuel);
    let authorized = first.authorize(first_work).unwrap();
    let receipt = first.dispatch(authorized).unwrap();
    first.accept(receipt).unwrap();
    assert!(matches!(
        first.poll().unwrap(),
        Poll::Value(ScalarValue::Integer(38))
    ));
    assert_eq!(calls.get(), 2);
}

#[test]
fn cancellation_at_each_boundary_retains_only_already_committed_effects() {
    for phase in 0..=4 {
        let total = Cell::new(0);
        let calls = Cell::new(0);
        let host = counter_host(&total, &calls);
        let program = host.compile(TWO_EFFECTS, GRANTS).unwrap();
        let mut session = host.start(&program, GRANTS, Fuel::new(1000)).unwrap();
        if phase > 0 {
            let work = request(session.poll().unwrap());
            if phase > 1 {
                let authorized = session.authorize(work).unwrap();
                if phase > 2 {
                    let receipt = session.dispatch(authorized).unwrap();
                    if phase > 3 {
                        session.accept(receipt).unwrap();
                    }
                }
            }
        }
        let fuel = session.fuel_remaining();
        assert!(session.cancel());
        assert!(!session.cancel());
        assert!(!session.lease().is_open());
        assert!(matches!(
            session.poll().unwrap(),
            Poll::Terminal(EffectSessionEnd::Cancelled)
        ));
        assert_eq!(session.fuel_remaining(), fuel);
        assert_eq!(calls.get(), u64::from(phase >= 3));
        assert_eq!(total.get(), if phase >= 3 { 19 } else { 0 });
    }
}

#[test]
fn cancelled_authorization_never_dispatches_and_late_receipt_retains_committed_value() {
    let total = Cell::new(0);
    let calls = Cell::new(0);
    let host = counter_host(&total, &calls);
    let program = host.compile(TWO_EFFECTS, GRANTS).unwrap();
    let mut before = host.start(&program, GRANTS, Fuel::new(1000)).unwrap();
    let work = request(before.poll().unwrap());
    let authorized = before.authorize(work).unwrap();
    before.cancel();
    assert!(before.dispatch(authorized).is_err());
    assert_eq!(calls.get(), 0);

    let mut after = host.start(&program, GRANTS, Fuel::new(1000)).unwrap();
    let work = request(after.poll().unwrap());
    let authorized = after.authorize(work).unwrap();
    let receipt = after.dispatch(authorized).unwrap();
    after.cancel();
    let fuel = after.fuel_remaining();
    let rejected = after.accept(receipt).err().unwrap();
    assert_eq!(rejected.input.value(), &ScalarValue::Integer(19));
    assert_eq!(after.fuel_remaining(), fuel);
    assert_eq!(
        after.status(),
        EffectSessionStatus::Terminal(EffectSessionEnd::Cancelled)
    );
    assert_eq!(total.get(), 19);
    assert_eq!(calls.get(), 1);
}

#[test]
fn dropping_a_pending_session_closes_only_its_lease() {
    let total = Cell::new(0);
    let calls = Cell::new(0);
    let host = counter_host(&total, &calls);
    let program = host.compile(ONE_EFFECT, GRANTS).unwrap();
    let mut first = host.start(&program, GRANTS, Fuel::new(1000)).unwrap();
    let mut second = host.start(&program, GRANTS, Fuel::new(1000)).unwrap();
    let lease = first.lease();
    let first_work = request(first.poll().unwrap());
    drop(first);
    assert!(!lease.is_open());
    assert!(second.lease().is_open());
    assert!(second.authorize(first_work).is_err());
    let work = request(second.poll().unwrap());
    let authorized = second.authorize(work).unwrap();
    let receipt = second.dispatch(authorized).unwrap();
    second.accept(receipt).unwrap();
    assert!(matches!(
        second.poll().unwrap(),
        Poll::Value(ScalarValue::Integer(19))
    ));
    assert_eq!(calls.get(), 1);
}

#[test]
fn exhausted_initial_fuel_is_terminal_without_dispatch_or_refill() {
    let total = Cell::new(0);
    let calls = Cell::new(0);
    let host = counter_host(&total, &calls);
    let program = host.compile(TWO_EFFECTS, GRANTS).unwrap();
    let mut session = host.start(&program, GRANTS, Fuel::new(0)).unwrap();
    assert!(session.poll().is_err());
    assert_eq!(session.fuel_remaining(), 0);
    assert!(matches!(
        session.poll().unwrap(),
        Poll::Terminal(EffectSessionEnd::Failed)
    ));
    assert_eq!(calls.get(), 0);
    assert!(!session.lease().is_open());
}

#[test]
fn host_work_debits_the_same_meter_without_accepting_or_resuming() {
    let mut builder = HostBuilder::new();
    builder
        .register("debit", vec![], ScalarType::Integer, "debit", |_, fuel| {
            fuel.charge(7)
                .map_err(|_| HostFailure::Failed("fuel exhausted".into()))?;
            Ok(ScalarValue::Integer(42))
        })
        .unwrap();
    let host = builder.build();
    let program = host.compile("fn main() = debit()", &["debit"]).unwrap();
    let mut session = host.start(&program, &["debit"], Fuel::new(1000)).unwrap();
    let work = request(session.poll().unwrap());
    let authorized = session.authorize(work).unwrap();
    let fuel = session.fuel_remaining();
    let receipt = session.dispatch(authorized).unwrap();
    assert_eq!(session.fuel_remaining(), fuel - 7);
    assert_eq!(session.status(), EffectSessionStatus::Awaiting);
    session.accept(receipt).unwrap();
    assert_eq!(session.fuel_remaining(), fuel - 7);
    assert!(matches!(
        session.poll().unwrap(),
        Poll::Value(ScalarValue::Integer(42))
    ));
}

#[test]
fn fuel_exhaustion_after_the_first_effect_preserves_its_mutation() {
    let total = Cell::new(0);
    let calls = Cell::new(0);
    let mut builder = HostBuilder::new();
    builder
        .register(
            "counter.add",
            vec![parameter("amount", ScalarType::Integer)],
            ScalarType::Integer,
            "counter.write",
            |arguments, fuel| {
                calls.set(calls.get() + 1);
                total.set(total.get() + integer(arguments, "amount"));
                fuel.charge(fuel.remaining()).unwrap();
                Ok(ScalarValue::Integer(total.get()))
            },
        )
        .unwrap();
    let host = builder.build();
    let program = host.compile(TWO_EFFECTS, GRANTS).unwrap();
    let mut session = host.start(&program, GRANTS, Fuel::new(1000)).unwrap();
    let work = request(session.poll().unwrap());
    let authorized = session.authorize(work).unwrap();
    let receipt = session.dispatch(authorized).unwrap();
    assert_eq!(session.fuel_remaining(), 0);
    session.accept(receipt).unwrap();
    assert!(session.poll().is_err());
    assert_eq!(
        session.status(),
        EffectSessionStatus::Terminal(EffectSessionEnd::Failed)
    );
    assert_eq!(session.fuel_remaining(), 0);
    assert_eq!(total.get(), 19);
    assert_eq!(calls.get(), 1);
}

#[test]
fn scalar_failure_in_the_next_call_preserves_prior_effects_and_its_recovery_class() {
    let total = Cell::new(0);
    let calls = Cell::new(0);
    let host = counter_host(&total, &calls);
    let program = host.compile(
        "fn main() = bind(first: counter.add(amount: 19), body: counter.add(amount: add(left: first, right: 18446744073709551615)))",
        GRANTS,
    ).unwrap();
    let mut session = host.start(&program, GRANTS, Fuel::new(1000)).unwrap();
    let work = request(session.poll().unwrap());
    let authorized = session.authorize(work).unwrap();
    let receipt = session.dispatch(authorized).unwrap();
    session.accept(receipt).unwrap();
    let Err(Error::Evaluation(failure)) = session.poll() else {
        panic!("expected the second call's arithmetic failure");
    };
    assert!(failure.is_recoverable());
    assert_eq!(
        session.status(),
        EffectSessionStatus::Terminal(EffectSessionEnd::Failed)
    );
    assert_eq!(total.get(), 19);
    assert_eq!(calls.get(), 1);
    assert!(!session.lease().is_open());
    assert!(matches!(
        session.poll().unwrap(),
        Poll::Terminal(EffectSessionEnd::Failed)
    ));
    assert_eq!(calls.get(), 1);
}

#[test]
fn recover_handles_scalar_operand_errors_but_never_native_denial_or_fuel_exhaustion() {
    let total = Cell::new(0);
    let calls = Cell::new(0);
    let host = counter_host(&total, &calls);
    let program = host.compile(
        "fn main() = counter.add(amount: recover(value: parse_integer(value: \"bad\"), fallback: 19))",
        GRANTS,
    ).unwrap();
    for (grants, fuel) in [(&[][..], 1000), (GRANTS, 0)] {
        let mut session = host.start(&program, grants, Fuel::new(fuel)).unwrap();
        let Err(Error::Evaluation(failure)) = session.poll() else {
            panic!("expected unrecoverable execution refusal");
        };
        assert!(!failure.is_recoverable());
        assert_eq!(
            session.status(),
            EffectSessionStatus::Terminal(EffectSessionEnd::Failed)
        );
        assert_eq!(calls.get(), 0);
    }
    let mut session = host.start(&program, GRANTS, Fuel::new(1000)).unwrap();
    let work = request(session.poll().unwrap());
    assert_eq!(integer(work.arguments(), "amount"), 19);
    let authorized = session.authorize(work).unwrap();
    let receipt = session.dispatch(authorized).unwrap();
    session.accept(receipt).unwrap();
    assert!(matches!(
        session.poll().unwrap(),
        Poll::Value(ScalarValue::Integer(19))
    ));
    assert_eq!(calls.get(), 1);
}

#[test]
fn classified_host_failures_close_the_session_and_preserve_partial_effects_and_fuel() {
    for end in [
        EffectSessionEnd::Failed,
        EffectSessionEnd::HostUncertain,
        EffectSessionEnd::Cancelled,
    ] {
        let total = Cell::new(0);
        let calls = Cell::new(0);
        let mut builder = HostBuilder::new();
        builder
            .register(
                "counter.add",
                vec![parameter("amount", ScalarType::Integer)],
                ScalarType::Integer,
                "counter.write",
                |arguments, fuel| {
                    fuel.charge(3).unwrap();
                    calls.set(calls.get() + 1);
                    total.set(total.get() + integer(arguments, "amount"));
                    Err(match end {
                        EffectSessionEnd::Failed => HostFailure::Failed("after mutation".into()),
                        EffectSessionEnd::HostUncertain => {
                            HostFailure::Uncertain("delivery unknown".into())
                        }
                        EffectSessionEnd::Cancelled => HostFailure::Cancelled,
                        EffectSessionEnd::Completed => unreachable!(),
                    })
                },
            )
            .unwrap();
        let host = builder.build();
        let program = host.compile(TWO_EFFECTS, GRANTS).unwrap();
        let mut session = host.start(&program, GRANTS, Fuel::new(1000)).unwrap();
        let work = request(session.poll().unwrap());
        let authorized = session.authorize(work).unwrap();
        let fuel = session.fuel_remaining();
        assert!(session.dispatch(authorized).is_err());
        assert_eq!(session.status(), EffectSessionStatus::Terminal(end));
        assert!(!session.lease().is_open());
        assert_eq!(session.fuel_remaining(), fuel - 3);
        assert!(matches!(session.poll().unwrap(), Poll::Terminal(actual) if actual == end));
        assert!(!session.cancel());
        assert_eq!(total.get(), 19);
        assert_eq!(calls.get(), 1);
    }
}

#[test]
fn callback_panic_seals_host_uncertain_even_when_the_caller_retains_the_session() {
    let total = Cell::new(0);
    let mut builder = HostBuilder::new();
    builder
        .register(
            "counter.add",
            vec![parameter("amount", ScalarType::Integer)],
            ScalarType::Integer,
            "counter.write",
            |arguments, fuel| {
                fuel.charge(3).unwrap();
                total.set(total.get() + integer(arguments, "amount"));
                panic!("native callback failed after mutation")
            },
        )
        .unwrap();
    let host = builder.build();
    let program = host.compile(TWO_EFFECTS, GRANTS).unwrap();
    let mut session = host.start(&program, GRANTS, Fuel::new(1000)).unwrap();
    let work = request(session.poll().unwrap());
    let authorized = session.authorize(work).unwrap();
    let fuel = session.fuel_remaining();
    assert!(catch_unwind(AssertUnwindSafe(|| session.dispatch(authorized))).is_err());
    assert_eq!(
        session.status(),
        EffectSessionStatus::Terminal(EffectSessionEnd::HostUncertain)
    );
    assert!(!session.lease().is_open());
    assert_eq!(session.fuel_remaining(), fuel - 3);
    assert_eq!(total.get(), 19);
    assert!(matches!(
        session.poll().unwrap(),
        Poll::Terminal(EffectSessionEnd::HostUncertain)
    ));
}

#[test]
fn wrong_typed_and_unbounded_replies_are_returned_without_acceptance_or_reexecution() {
    for unbounded in [false, true] {
        let calls = Cell::new(0);
        let mut builder = HostBuilder::new();
        builder
            .register(
                "reply",
                vec![],
                if unbounded {
                    ScalarType::String
                } else {
                    ScalarType::Integer
                },
                "reply",
                |_, _| {
                    calls.set(calls.get() + 1);
                    Ok(if unbounded {
                        ScalarValue::String("x".repeat(4097))
                    } else {
                        ScalarValue::Boolean(true)
                    })
                },
            )
            .unwrap();
        let host = builder.build();
        let program = host.compile("fn main() = reply()", &["reply"]).unwrap();
        let mut session = host.start(&program, &["reply"], Fuel::new(1000)).unwrap();
        let work = request(session.poll().unwrap());
        let authorized = session.authorize(work).unwrap();
        let receipt = session.dispatch(authorized).unwrap();
        let pointer = match receipt.value() {
            ScalarValue::String(value) => Some(value.as_ptr()),
            _ => None,
        };
        let fuel = session.fuel_remaining();
        let rejected = session.accept(receipt).err().unwrap();
        match rejected.input.value() {
            ScalarValue::String(value) => {
                assert_eq!(Some(value.as_ptr()), pointer);
                assert_eq!(value.len(), 4097);
            }
            ScalarValue::Boolean(value) => assert!(*value),
            _ => panic!("original callback reply was changed"),
        }
        assert_eq!(session.fuel_remaining(), fuel);
        assert_eq!(session.status(), EffectSessionStatus::Awaiting);
        assert!(session.lease().is_open());
        assert!(matches!(session.poll().unwrap(), Poll::Awaiting));
        assert_eq!(calls.get(), 1);
        assert!(session.cancel());
    }
}
