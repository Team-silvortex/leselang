//! Formatting is source tooling, exercised through an independent dependency graph.
use std::cell::{Cell, RefCell};

use leselang_syntax::{TokenKind, format, format_source, parse};
use leselang_vm::embedding::{
    Error, Fuel, Host, HostBuilder, HostFailure, Poll, ScalarType, ScalarValue,
    diagnostics::{self, DiagnosticKind},
    parameter,
};

const GRANT: &[&str] = &["counter.write"];
const TWO_EFFECTS: &str = r#"// 两次宿主操作 🦀
fn increment(n: // parameter type follows
integer) = counter // namespace remains one operation
.state.add(amount: n) // helper result
fn main() = bind(note: "https://example.invalid/path // literal", body:
bind(first: increment(n: 19), // first response is used exactly once
body: increment(n: add(left: first, right: 4)))) // final value
// Keep this closing comment.
"#;

fn counter_host<'a>(
    total: &'a Cell<u64>,
    calls: &'a RefCell<Vec<u64>>,
    uncertain_at: Option<u64>,
) -> Host<'a> {
    let mut builder = HostBuilder::new();
    builder
        .register(
            "counter.state.add",
            vec![parameter("amount", ScalarType::Integer)],
            ScalarType::Integer,
            "counter.write",
            move |arguments, fuel| {
                let Some(ScalarValue::Integer(amount)) = arguments.get("amount") else {
                    panic!("the admitted operation requires an integer");
                };
                fuel.charge(amount + 1)
                    .map_err(|_| HostFailure::Failed("fuel exhausted before mutation".into()))?;
                calls.borrow_mut().push(*amount);
                total.set(total.get() + amount);
                if uncertain_at == Some(*amount) {
                    return Err(HostFailure::Uncertain(
                        "counter acknowledgement lost".into(),
                    ));
                }
                Ok(ScalarValue::Integer(total.get()))
            },
        )
        .unwrap();
    builder.build()
}

#[derive(Debug, PartialEq)]
struct Outcome {
    result: Result<ScalarValue, (DiagnosticKind, String)>,
    fuel_remaining: u64,
    calls: Vec<u64>,
    total: u64,
}

fn run(source: &str, fuel: u64, uncertain_at: Option<u64>) -> Outcome {
    let total = Cell::new(0);
    let calls = RefCell::new(Vec::new());
    let host = counter_host(&total, &calls, uncertain_at);
    let program = host.compile(source, GRANT).unwrap();
    assert_eq!(total.get(), 0);
    assert!(calls.borrow().is_empty());
    let mut session = host.start(&program, GRANT, Fuel::new(fuel)).unwrap();
    let result = (|| -> Result<ScalarValue, Error> {
        loop {
            match session.poll()? {
                Poll::Request(request) => {
                    let request = session.authorize(request).map_err(|error| error.error)?;
                    let receipt = session.dispatch(request).map_err(|error| error.error)?;
                    session.accept(receipt).map_err(|error| error.error)?;
                }
                Poll::Value(value) => return Ok(value),
                unexpected => panic!("unexpected explicit driver boundary: {unexpected:?}"),
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

fn comments(source: &str) -> Vec<String> {
    let tree = parse(source);
    assert!(tree.diagnostics.is_empty(), "{:?}", tree.diagnostics);
    tree.tokens
        .iter()
        .filter(|token| token.kind == TokenKind::Comment)
        .map(|token| tree.token_text(token).unwrap().to_owned())
        .collect()
}

#[test]
fn formatted_multisegment_helpers_keep_comments_and_two_explicit_effects() {
    for source in [TWO_EFFECTS.to_owned(), TWO_EFFECTS.replace('\n', "\r\n")] {
        let formatted = format_source(&source).unwrap();
        assert_eq!(comments(&formatted), comments(&source));
        assert!(formatted.contains("https://example.invalid/path // literal"));
        assert_eq!(format_source(&formatted).unwrap(), formatted);
        let before = run(&source, 1000, None);
        let after = run(&formatted, 1000, None);
        assert_eq!(before, after);
        assert_eq!(after.result, Ok(ScalarValue::Integer(42)));
        assert_eq!(after.calls, [19, 23]);
        assert_eq!(after.total, 42);
    }
}

#[test]
fn formatting_keeps_every_original_fuel_cutoff_and_partial_effect_boundary() {
    let formatted = format_source(TWO_EFFECTS).unwrap();
    let complete = run(TWO_EFFECTS, 1000, None);
    let cost = 1000 - complete.fuel_remaining;
    let mut saw_partial = false;
    for budget in 0..=cost + 1 {
        let before = run(TWO_EFFECTS, budget, None);
        let after = run(&formatted, budget, None);
        assert_eq!(before, after, "fuel budget {budget}");
        saw_partial |= after.result.is_err() && after.calls == [19];
    }
    assert!(
        saw_partial,
        "the first committed Effect has a distinct failure boundary"
    );
}

#[test]
fn formatting_is_not_compilation_and_keeps_cold_admission_failures() {
    let total = Cell::new(0);
    let calls = RefCell::new(Vec::new());
    let host = counter_host(&total, &calls, None);
    for (source, grants, expected) in [
        (
            "fn main() = choose(when: true, then: 0, otherwise: counter.state.add(amount: 1)) // denied even cold",
            &[][..],
            DiagnosticKind::CapabilityDenied,
        ),
        (
            "// unused does not mean unchecked\nfn unused() = counter.state.add(amount: 1)\nfn main() = 0",
            &[][..],
            DiagnosticKind::CapabilityDenied,
        ),
        (
            "fn main() = choose(when: true, then: 0, otherwise: counter.state.add(amount: true)) // wrong type",
            GRANT,
            DiagnosticKind::Compilation,
        ),
        (
            "fn unused() = counter.state.add(wrong: 1) // wrong name\nfn main() = 0",
            GRANT,
            DiagnosticKind::Compilation,
        ),
        (
            "fn unused() = counter.state.unknown() // unregistered\nfn main() = 0",
            GRANT,
            DiagnosticKind::UnknownOperation,
        ),
    ] {
        let formatted = format_source(source).unwrap();
        for text in [source, formatted.as_str()] {
            let error = host.compile(text, grants).err().unwrap();
            let reports = diagnostics::compile(&error);
            assert_eq!(reports[0].kind, expected);
        }
    }
    assert_eq!(total.get(), 0);
    assert!(calls.borrow().is_empty());
}

#[test]
fn formatting_does_not_replay_or_hide_an_uncertain_host_mutation() {
    let source = "// The acknowledgement for the second call can be lost.\nfn main() = bind(first: counter.state.add(amount: 19), body: bind(second: counter.state.add(amount: 23), body: counter.state.add(amount: 100))) // never reach the last call";
    let formatted = format_source(source).unwrap();
    let before = run(source, 1000, Some(23));
    let after = run(&formatted, 1000, Some(23));
    assert_eq!(before, after);
    assert_eq!(
        after.result.as_ref().unwrap_err().0,
        DiagnosticKind::HostUncertain
    );
    assert_eq!(after.calls, [19, 23]);
    assert_eq!(after.total, 42);
}

#[test]
fn formatting_keeps_a_scalar_failure_after_an_already_committed_effect() {
    let source = "fn main() = bind(first: counter.state.add(amount: 19), // already delivered\n body: div(left: 1, right: 0)) // no rollback";
    let formatted = format_source(source).unwrap();
    let before = run(source, 1000, None);
    let after = run(&formatted, 1000, None);
    assert_eq!(before, after);
    assert!(after.result.is_err());
    assert_eq!(after.calls, [19]);
    assert_eq!(after.total, 19);
}

#[test]
fn formatting_source_does_not_change_a_pending_sessions_authority_or_fuel() {
    let total = Cell::new(0);
    let calls = RefCell::new(Vec::new());
    let host = counter_host(&total, &calls, None);
    let program = host.compile(TWO_EFFECTS, GRANT).unwrap();
    let mut session = host.start(&program, GRANT, Fuel::new(1000)).unwrap();
    let Poll::Request(request) = session.poll().unwrap() else {
        panic!("expected a pending Effect");
    };
    session.set_grants(&[]);
    let before = session.fuel_remaining();
    let formatted = format_source(TWO_EFFECTS).unwrap();
    assert!(!formatted.is_empty());
    assert_eq!(session.fuel_remaining(), before);
    assert_eq!(total.get(), 0);
    assert!(calls.borrow().is_empty());
    let rejected = session.authorize(request).unwrap_err();
    assert!(matches!(rejected.error, Error::Denied));
    assert!(session.cancel());
}

#[test]
fn source_formatting_refuses_invalid_input_without_replacing_legacy_ast_formatting() {
    let source = "// Preserve me in source formatting.\nfn main() = 42 // trailing";
    assert_eq!(format(&parse(source)).unwrap(), "fn main() = 42\n");
    assert_eq!(comments(&format_source(source).unwrap()), comments(source));
    for source in ["// invalid\nfn main() = @", "fn main() = counter.state.()"] {
        let errors = format_source(source).unwrap_err();
        assert_eq!(errors, parse(source).diagnostics);
    }
}
