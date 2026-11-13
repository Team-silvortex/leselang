use std::cell::{Cell, RefCell};

use leselang_vm::embedding::{Fuel, Host, HostBuilder, Poll, ScalarType, ScalarValue, parameter};

const OPERATION: &str = "counter.state.add";
const GRANT: &str = "counter.write";
const TWO_EFFECTS: &str = r#"
fn increment(n: integer) = counter.state.add(amount: n)
fn main() = bind(first: increment(n: 19), body: increment(n: add(left: first, right: 4)))
"#;

fn counter_host<'a>(total: &'a Cell<u64>, calls: &'a RefCell<Vec<u64>>) -> Host<'a> {
    let mut builder = HostBuilder::new();
    builder
        .register(
            OPERATION,
            vec![parameter("amount", ScalarType::Integer)],
            ScalarType::Integer,
            GRANT,
            |arguments, _| {
                let Some(ScalarValue::Integer(amount)) = arguments.get("amount") else {
                    panic!("expected the admitted integer argument");
                };
                calls.borrow_mut().push(*amount);
                total.set(total.get() + amount);
                Ok(ScalarValue::Integer(total.get()))
            },
        )
        .unwrap();
    builder.build()
}

#[test]
fn registered_multi_segment_operation_drives_two_explicit_effects() {
    let total = Cell::new(0);
    let calls = RefCell::new(Vec::new());
    let host = counter_host(&total, &calls);
    assert_eq!(host.describe().operations()[0].name(), OPERATION);
    let program = host.compile(TWO_EFFECTS, &[GRANT]).unwrap();
    assert!(calls.borrow().is_empty());
    let mut session = host.start(&program, &[GRANT], Fuel::new(1000)).unwrap();
    for expected_calls in 1..=2 {
        let Poll::Request(work) = session.poll().unwrap() else {
            panic!("expected a host operation");
        };
        assert_eq!(work.operation(), OPERATION);
        assert_eq!(calls.borrow().len(), expected_calls - 1);
        let authorized = session.authorize(work).unwrap();
        let receipt = session.dispatch(authorized).unwrap();
        assert_eq!(calls.borrow().len(), expected_calls);
        session.accept(receipt).unwrap();
        assert_eq!(calls.borrow().len(), expected_calls);
    }
    assert!(matches!(
        session.poll().unwrap(),
        Poll::Value(ScalarValue::Integer(42))
    ));
    assert_eq!(*calls.borrow(), [19, 23]);
    assert_eq!(total.get(), 42);
    assert!(session.fuel_remaining() < 1000);
}

#[test]
fn cold_multi_segment_calls_keep_capability_and_signature_admission() {
    let total = Cell::new(0);
    let calls = RefCell::new(Vec::new());
    let host = counter_host(&total, &calls);
    for source in [
        "fn main() = counter.state.add(amount: 1)",
        "fn main() = choose(when: true, then: 0, otherwise: counter.state.add(amount: 1))",
        "fn unused() = counter.state.add(amount: 1)\nfn main() = 0",
    ] {
        assert!(host.compile(source, &[]).is_err(), "{source}");
        assert!(host.compile(source, &[GRANT]).is_ok(), "{source}");
    }
    for source in [
        "fn main() = counter.state.missing(amount: 1)",
        "fn main() = choose(when: true, then: 0, otherwise: counter.state.add(amount: true))",
        "fn unused() = counter.state.add(wrong: 1)\nfn main() = 0",
    ] {
        assert!(host.compile(source, &[GRANT]).is_err(), "{source}");
    }
    assert!(calls.borrow().is_empty());
    assert_eq!(total.get(), 0);
}

#[test]
fn multi_segment_names_do_not_change_live_grant_checks() {
    let total = Cell::new(0);
    let calls = RefCell::new(Vec::new());
    let host = counter_host(&total, &calls);
    let program = host.compile(TWO_EFFECTS, &[GRANT]).unwrap();
    let mut session = host.start(&program, &[GRANT], Fuel::new(1000)).unwrap();
    let Poll::Request(work) = session.poll().unwrap() else {
        panic!("expected a host operation");
    };
    session.set_grants(&[]);
    assert!(session.authorize(work).is_err());
    assert!(calls.borrow().is_empty());
    assert_eq!(total.get(), 0);
}
