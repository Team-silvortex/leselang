//! A complete scalar host using the optional high-level embedding API.
use std::cell::Cell;

use leselang_vm::embedding::{
    Fuel, HostBuilder, HostFailure, Poll, ScalarType, ScalarValue, parameter,
};

const SOURCE: &str = r#"
fn increment(n: integer) = bind(receipt: counter.add(amount: n), body: receipt)
fn main() = bind(first: increment(n: 19), body: increment(n: add(left: first, right: 4)))
"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let total = Cell::new(0_u64);
    let calls = Cell::new(0);
    let mut builder = HostBuilder::new();
    builder.register(
        "counter.add",
        vec![parameter("amount", ScalarType::Integer)],
        ScalarType::Integer,
        "counter.write",
        |arguments, fuel| {
            // The callback shares the session's original meter. Charge before work.
            fuel.charge(1)
                .map_err(|_| HostFailure::Failed("host fuel exhausted".into()))?;
            let Some(ScalarValue::Integer(amount)) = arguments.get("amount") else {
                return Err(HostFailure::Failed("missing integer amount".into()));
            };
            let next = total
                .get()
                .checked_add(*amount)
                .filter(|value| *value <= 100)
                .ok_or_else(|| HostFailure::Failed("counter limit exceeded".into()))?;
            total.set(next);
            calls.set(calls.get() + 1);
            Ok(ScalarValue::Integer(next))
        },
    )?;
    let host = builder.build();
    let grants = ["counter.write"];
    let program = host.compile(SOURCE, &grants)?;
    let mut session = host.start(&program, &grants, Fuel::new(1_000))?;
    loop {
        match session.poll()? {
            Poll::Request(request) => {
                let authorized = session.authorize(request)?;
                let receipt = session.dispatch(authorized)?;
                session.accept(receipt)?;
            }
            Poll::Value(ScalarValue::Integer(value)) => {
                assert_eq!((value, total.get(), calls.get()), (42, 42, 2));
                println!(
                    "value={value}, calls={}, fuel_remaining={}",
                    calls.get(),
                    session.fuel_remaining()
                );
                return Ok(());
            }
            Poll::Value(_) => return Err("unexpected result type".into()),
            Poll::Awaiting => return Err("host loop lost its pending request or receipt".into()),
            Poll::Terminal(end) => return Err(format!("execution ended: {end:?}").into()),
        }
    }
}
