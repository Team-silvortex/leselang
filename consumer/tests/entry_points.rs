//! Named entry points through the public API and an independent dependency graph.
use std::cell::{Cell, RefCell};

use leselang_vm::embedding::{
    EffectSessionEnd, EffectSessionStatus, Error, Fuel, Host, HostBuilder, HostFailure, Poll,
    Request, ScalarType, ScalarValue,
    diagnostics::{self, DiagnosticKind},
    parameter,
};

const GRANTS: &[&str] = &["counter.write"];
const TWO_EFFECTS: &str = r#"
fn increment(n: integer) = counter.add(amount: n)
fn task() = bind(first: increment(n: 19), body: increment(n: add(left: first, right: 4)))
fn unused() = 0
"#;

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
        panic!("expected a pending host Effect");
    };
    request
}

#[derive(Debug, PartialEq)]
struct Outcome {
    result: Result<ScalarValue, (DiagnosticKind, String)>,
    fuel_remaining: u64,
    calls: Vec<u64>,
    total: u64,
}

fn run(source: &str, entry: Option<&str>, budget: u64, uncertain_at: Option<u64>) -> Outcome {
    let total = Cell::new(0);
    let calls = RefCell::new(Vec::new());
    let host = counter_host(&total, &calls, uncertain_at);
    let program = match entry {
        Some(entry) => host.compile_entry(source, entry, GRANTS),
        None => host.compile(source, GRANTS),
    }
    .unwrap();
    assert_eq!(program.entry_name(), entry.unwrap_or("main"));
    assert_eq!(program.result_type(), ScalarType::Integer);
    assert_eq!(total.get(), 0);
    assert!(calls.borrow().is_empty());
    let mut session = host.start(&program, GRANTS, Fuel::new(budget)).unwrap();
    let result = (|| -> Result<ScalarValue, Error> {
        loop {
            match session.poll()? {
                Poll::Request(request) => {
                    let authorized = session.authorize(request).map_err(|error| error.error)?;
                    let receipt = session.dispatch(authorized).map_err(|error| error.error)?;
                    session.accept(receipt).map_err(|error| error.error)?;
                }
                Poll::Value(value) => return Ok(value),
                unexpected => panic!("unexpected driver boundary: {unexpected:?}"),
            }
        }
    })()
    .map_err(|error| {
        let diagnostic = diagnostics::runtime(&error);
        (diagnostic.kind, diagnostic.message)
    });
    let calls = calls.borrow().clone();
    Outcome {
        result,
        fuel_remaining: session.fuel_remaining(),
        calls,
        total: total.get(),
    }
}

#[test]
fn selection_is_explicit_and_compile_still_defaults_to_main() {
    let host = HostBuilder::new().build();
    let source = "fn first() = 19\nfn second() = 23";
    assert!(host.compile(source, &[]).is_err());
    for (entry, expected) in [("first", 19), ("second", 23)] {
        let program = host.compile_entry(source, entry, &[]).unwrap();
        assert_eq!(program.entry_name(), entry);
        assert_eq!(program.result_type(), ScalarType::Integer);
        let mut session = host.start(&program, &[], Fuel::new(100)).unwrap();
        assert!(
            matches!(session.poll().unwrap(), Poll::Value(ScalarValue::Integer(value)) if value == expected)
        );
    }

    let source = "fn second() = 23\nfn main() = 19";
    for (program, entry, expected) in [
        (host.compile(source, &[]).unwrap(), "main", 19),
        (host.compile_entry(source, "main", &[]).unwrap(), "main", 19),
        (
            host.compile_entry(source, "second", &[]).unwrap(),
            "second",
            23,
        ),
    ] {
        assert_eq!(program.entry_name(), entry);
        let mut session = host.start(&program, &[], Fuel::new(100)).unwrap();
        assert!(
            matches!(session.poll().unwrap(), Poll::Value(ScalarValue::Integer(value)) if value == expected)
        );
    }
}

#[test]
fn unselected_main_can_be_a_parameterized_helper() {
    let host = HostBuilder::new().build();
    let source = "fn main(n: integer) = add(left: n, right: 23)\nfn task() = main(n: 19)";
    assert!(host.compile(source, &[]).is_err());
    let program = host.compile_entry(source, "task", &[]).unwrap();
    let mut session = host.start(&program, &[], Fuel::new(100)).unwrap();
    assert!(matches!(
        session.poll().unwrap(),
        Poll::Value(ScalarValue::Integer(42))
    ));
}

#[test]
fn metadata_owns_its_entry_name_and_reports_all_six_scalar_results() {
    let host = HostBuilder::new().build();
    for (expression, expected) in [
        ("42", ScalarType::Integer),
        ("true", ScalarType::Boolean),
        ("\"结果 🦀\"", ScalarType::String),
        ("none", ScalarType::None),
        ("optional_string(value: none)", ScalarType::OptionalString),
        (
            "strings(first: \"a\", second: \"b\")",
            ScalarType::StringList,
        ),
    ] {
        let program = {
            let source = format!("fn helper() = {expression}\nfn selected() = helper()");
            let entry = String::from("selected");
            host.compile_entry(&source, &entry, &[]).unwrap()
        };
        assert_eq!(program.entry_name(), "selected");
        assert_eq!(program.result_type(), expected);
        let mut session = host.start(&program, &[], Fuel::new(1000)).unwrap();
        let Poll::Value(value) = session.poll().unwrap() else {
            panic!("pure scalar helper should complete without an Effect");
        };
        assert_eq!(value.scalar_type(), expected);
    }
}

#[test]
fn changing_the_entry_never_skips_cold_validation_or_entry_call_rules() {
    let total = Cell::new(0);
    let calls = RefCell::new(Vec::new());
    let host = counter_host(&total, &calls, None);
    for (source, entry, grants, expected) in [
        (
            "fn task() = 1",
            "missing",
            GRANTS,
            DiagnosticKind::Compilation,
        ),
        ("fn task() = 1", "", GRANTS, DiagnosticKind::Compilation),
        ("fn task() = 1", "a.b", GRANTS, DiagnosticKind::Compilation),
        (
            "fn task(x: integer) = x",
            "task",
            GRANTS,
            DiagnosticKind::Compilation,
        ),
        (
            "fn task() = task()",
            "task",
            GRANTS,
            DiagnosticKind::Compilation,
        ),
        (
            "fn task() = helper()\nfn helper() = task()",
            "task",
            GRANTS,
            DiagnosticKind::Compilation,
        ),
        (
            "fn task() = 1\nfn unused() = task()",
            "task",
            GRANTS,
            DiagnosticKind::Compilation,
        ),
        (
            "fn task() = 1\nfn unused() = unused()",
            "task",
            GRANTS,
            DiagnosticKind::Compilation,
        ),
        (
            "fn task() = choose(when: true, then: 0, otherwise: counter.add(amount: 1))",
            "task",
            &[][..],
            DiagnosticKind::CapabilityDenied,
        ),
        (
            "fn task() = 0\nfn main() = counter.add(amount: 1)",
            "task",
            &[][..],
            DiagnosticKind::CapabilityDenied,
        ),
        (
            "fn task() = 0\nfn unused() = counter.add(amount: true)",
            "task",
            GRANTS,
            DiagnosticKind::Compilation,
        ),
        (
            "fn task() = 0\nfn unused() = counter.missing()",
            "task",
            GRANTS,
            DiagnosticKind::UnknownOperation,
        ),
    ] {
        let error = host.compile_entry(source, entry, grants).err().unwrap();
        assert_eq!(
            diagnostics::compile(&error)[0].kind,
            expected,
            "{source}; entry={entry:?}"
        );
    }
    assert!(
        host.compile_entry("fn task() = 1", &"a".repeat(65), GRANTS)
            .is_err()
    );
    assert_eq!(total.get(), 0);
    assert!(calls.borrow().is_empty());
}

#[test]
fn named_two_effect_program_keeps_each_explicit_driver_boundary() {
    let total = Cell::new(0);
    let calls = RefCell::new(Vec::new());
    let host = counter_host(&total, &calls, None);
    let program = host.compile_entry(TWO_EFFECTS, "task", GRANTS).unwrap();
    assert_eq!(program.result_type(), ScalarType::Integer);
    let mut session = host.start(&program, GRANTS, Fuel::new(1000)).unwrap();
    assert!(calls.borrow().is_empty());
    for (amount, expected_total) in [(19, 19), (23, 42)] {
        let work = request(session.poll().unwrap());
        assert_eq!(work.operation(), "counter.add");
        assert_eq!(
            work.arguments().get("amount"),
            Some(&ScalarValue::Integer(amount))
        );
        let before = calls.borrow().len();
        let fuel = session.fuel_remaining();
        assert!(matches!(session.poll().unwrap(), Poll::Awaiting));
        assert_eq!(session.fuel_remaining(), fuel);
        let authorized = session.authorize(work).unwrap();
        assert_eq!(calls.borrow().len(), before);
        let receipt = session.dispatch(authorized).unwrap();
        assert_eq!(receipt.value(), &ScalarValue::Integer(expected_total));
        assert_eq!(calls.borrow().len(), before + 1);
        assert!(matches!(session.poll().unwrap(), Poll::Awaiting));
        session.accept(receipt).unwrap();
        assert_eq!(session.status(), EffectSessionStatus::Ready);
        assert_eq!(calls.borrow().len(), before + 1);
    }
    assert!(matches!(
        session.poll().unwrap(),
        Poll::Value(ScalarValue::Integer(42))
    ));
    assert_eq!(*calls.borrow(), [19, 23]);
    assert_eq!(total.get(), 42);
}

#[test]
fn entry_selection_keeps_every_fuel_cutoff_and_partial_effect_boundary() {
    let main_source = TWO_EFFECTS.replace("fn task()", "fn main()");
    let completed = run(TWO_EFFECTS, Some("task"), 1000, None);
    assert_eq!(completed.result, Ok(ScalarValue::Integer(42)));
    assert_eq!(completed.calls, [19, 23]);
    let cost = 1000 - completed.fuel_remaining;
    let mut partial = false;
    for budget in 0..=cost + 1 {
        let named = run(TWO_EFFECTS, Some("task"), budget, None);
        assert_eq!(
            named,
            run(&main_source, None, budget, None),
            "fuel budget {budget}"
        );
        partial |= named.result.is_err() && named.calls == [19];
    }
    assert!(
        partial,
        "Fuel must expose the first committed Effect boundary"
    );
}

#[test]
fn selected_entry_keeps_uncertain_and_scalar_failures_after_mutation() {
    for (source, uncertain_at, expected_calls, expected_kind) in [
        (
            "fn task() = bind(first: counter.add(amount: 19), body: bind(second: counter.add(amount: 23), body: counter.add(amount: 100)))",
            Some(23),
            vec![19, 23],
            DiagnosticKind::HostUncertain,
        ),
        (
            "fn task() = bind(first: counter.add(amount: 19), body: div(left: 1, right: 0))",
            None,
            vec![19],
            DiagnosticKind::Runtime,
        ),
    ] {
        let named = run(source, Some("task"), 1000, uncertain_at);
        let main = run(
            &source.replace("fn task()", "fn main()"),
            None,
            1000,
            uncertain_at,
        );
        assert_eq!(named, main);
        assert_eq!(named.result.as_ref().unwrap_err().0, expected_kind);
        assert_eq!(named.calls, expected_calls);
        assert_eq!(named.total, expected_calls.iter().sum());
    }
}

#[test]
fn program_metadata_does_not_authorize_a_session_or_transfer_host_identity() {
    let total = Cell::new(0);
    let calls = RefCell::new(Vec::new());
    let host = counter_host(&total, &calls, None);
    let foreign = counter_host(&total, &calls, None);
    let program = host.compile_entry(TWO_EFFECTS, "task", GRANTS).unwrap();
    assert!(matches!(
        foreign.start(&program, GRANTS, Fuel::new(1000)),
        Err(Error::ForeignProgram)
    ));
    assert_eq!(program.entry_name(), "task");
    assert_eq!(program.result_type(), ScalarType::Integer);
    let mut session = host.start(&program, &[], Fuel::new(1000)).unwrap();
    assert!(session.poll().is_err());
    assert_eq!(
        session.status(),
        EffectSessionStatus::Terminal(EffectSessionEnd::Failed)
    );
    assert!(calls.borrow().is_empty());
}

#[test]
fn sessions_sharing_a_named_program_reject_each_others_requests_and_receipts() {
    let total = Cell::new(0);
    let calls = RefCell::new(Vec::new());
    let host = counter_host(&total, &calls, None);
    let program = host
        .compile_entry("fn task() = counter.add(amount: 19)", "task", GRANTS)
        .unwrap();
    let mut first = host.start(&program, GRANTS, Fuel::new(1000)).unwrap();
    let mut second = host.start(&program, GRANTS, Fuel::new(1000)).unwrap();
    let first_work = request(first.poll().unwrap());
    let second_work = request(second.poll().unwrap());
    let second_fuel = second.fuel_remaining();
    let refused = second.authorize(first_work).unwrap_err();
    assert!(calls.borrow().is_empty());
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
        Poll::Value(ScalarValue::Integer(38))
    ));
    assert_eq!(*calls.borrow(), [19, 19]);
}

#[test]
fn live_revocation_is_rechecked_at_authorize_dispatch_and_accept() {
    for boundary in 0..=2 {
        let total = Cell::new(0);
        let calls = RefCell::new(Vec::new());
        let host = counter_host(&total, &calls, None);
        let program = host.compile_entry(TWO_EFFECTS, "task", GRANTS).unwrap();
        let mut session = host.start(&program, GRANTS, Fuel::new(1000)).unwrap();
        let work = request(session.poll().unwrap());
        if boundary == 0 {
            session.set_grants(&[]);
            let fuel = session.fuel_remaining();
            assert!(matches!(
                session.authorize(work).unwrap_err().error,
                Error::Denied
            ));
            assert_eq!(session.fuel_remaining(), fuel);
        } else {
            let authorized = session.authorize(work).unwrap();
            if boundary == 1 {
                session.set_grants(&[]);
                let fuel = session.fuel_remaining();
                assert!(matches!(
                    session.dispatch(authorized).unwrap_err().error,
                    Error::Denied
                ));
                assert_eq!(session.fuel_remaining(), fuel);
            } else {
                let receipt = session.dispatch(authorized).unwrap();
                session.set_grants(&[]);
                let fuel = session.fuel_remaining();
                let rejected = session.accept(receipt).unwrap_err();
                assert_eq!(
                    diagnostics::runtime(&rejected.error).kind,
                    DiagnosticKind::CapabilityDenied
                );
                assert_eq!(rejected.input.value(), &ScalarValue::Integer(19));
                assert_eq!(session.fuel_remaining(), fuel);
                session.set_grants(GRANTS);
                session.accept(rejected.input).unwrap();
                session.set_grants(&[]);
                assert!(session.poll().is_err());
                assert_eq!(
                    session.status(),
                    EffectSessionStatus::Terminal(EffectSessionEnd::Failed)
                );
            }
        }
        assert_eq!(total.get(), if boundary == 2 { 19 } else { 0 });
        assert_eq!(calls.borrow().len(), usize::from(boundary == 2));
    }
}

#[test]
fn cancellation_rejects_dispatch_and_late_receipts_without_undoing_effects() {
    let total = Cell::new(0);
    let calls = RefCell::new(Vec::new());
    let host = counter_host(&total, &calls, None);
    let program = host.compile_entry(TWO_EFFECTS, "task", GRANTS).unwrap();
    let mut before = host.start(&program, GRANTS, Fuel::new(1000)).unwrap();
    let work = request(before.poll().unwrap());
    let authorized = before.authorize(work).unwrap();
    assert!(before.cancel());
    let before_fuel = before.fuel_remaining();
    assert!(before.dispatch(authorized).is_err());
    assert_eq!(before.fuel_remaining(), before_fuel);
    assert!(calls.borrow().is_empty());

    let mut after = host.start(&program, GRANTS, Fuel::new(1000)).unwrap();
    let work = request(after.poll().unwrap());
    let authorized = after.authorize(work).unwrap();
    let receipt = after.dispatch(authorized).unwrap();
    assert!(after.cancel());
    let after_fuel = after.fuel_remaining();
    let rejected = after.accept(receipt).unwrap_err();
    assert_eq!(rejected.input.value(), &ScalarValue::Integer(19));
    assert_eq!(after.fuel_remaining(), after_fuel);
    assert_eq!(
        after.status(),
        EffectSessionStatus::Terminal(EffectSessionEnd::Cancelled)
    );
    assert!(matches!(
        after.poll().unwrap(),
        Poll::Terminal(EffectSessionEnd::Cancelled)
    ));
    assert_eq!(*calls.borrow(), [19]);
    assert_eq!(total.get(), 19);
}
