//! Public diagnostic contracts exercised from an independent Cargo workspace.
use std::cell::Cell;

use leselang_vm::{
    embedding::{
        CompileError, EffectSessionEnd, Error, Fuel, Host, HostBuilder, HostFailure, Poll,
        ScalarType, ScalarValue,
        diagnostics::{self, Diagnostic, DiagnosticKind, SourcePosition},
        parameter,
    },
    native::syntax::Span,
};

const GRANT: &[&str] = &["device.control"];
const ONE: &str = "fn main() = device.write(amount: 19)";
const TWO: &str = "fn main() = bind(first: device.write(amount: 19), body: device.write(amount: add(left: first, right: 1)))";

fn host(calls: &Cell<u64>) -> Host<'_> {
    let mut builder = HostBuilder::new();
    builder
        .register(
            "device.write",
            vec![parameter("amount", ScalarType::Integer)],
            ScalarType::Integer,
            "device.control",
            |arguments, _| {
                calls.set(calls.get() + 1);
                let Some(ScalarValue::Integer(value)) = arguments.get("amount") else {
                    panic!("checked operation lost its integer argument");
                };
                Ok(ScalarValue::Integer(value + 1))
            },
        )
        .unwrap();
    builder.build()
}

fn compile_failure(host: &Host<'_>, source: &str, grants: &[&str]) -> Vec<Diagnostic> {
    let error = host
        .compile(source, grants)
        .err()
        .expect("source must fail");
    let diagnostics = diagnostics::compile(&error);
    assert!(!diagnostics.is_empty());
    for diagnostic in &diagnostics {
        assert!(!diagnostic.message.is_empty());
        assert!(diagnostic.message.chars().count() <= 512);
        assert!(!diagnostic.message.contains("std.print"));
        assert!(!diagnostic.message.contains("--allow"));
    }
    diagnostics
}

#[test]
fn custom_host_compilation_reports_structured_causes_without_dispatch_or_authority_changes() {
    let calls = Cell::new(0);
    let host = host(&calls);
    for (source, grants, kind) in [
        (
            "fn main() = device.absent()",
            GRANT,
            DiagnosticKind::UnknownOperation,
        ),
        (ONE, &[][..], DiagnosticKind::CapabilityDenied),
        (
            "fn main() = device.write(amount: true)",
            GRANT,
            DiagnosticKind::Compilation,
        ),
        (
            "fn main() = device.write(wrong: 1)",
            GRANT,
            DiagnosticKind::Compilation,
        ),
        (
            "fn main() = device.write()",
            GRANT,
            DiagnosticKind::Compilation,
        ),
    ] {
        let reported = compile_failure(&host, source, grants);
        assert_eq!(reported.len(), 1, "{source}");
        assert_eq!(reported[0].kind, kind, "{source}");
        assert_eq!(reported[0].code, "LSC0001");
        assert!(reported[0].location(source).is_some(), "{source}");
    }
    assert_eq!(calls.get(), 0);
    assert!(host.compile(ONE, &[]).is_err());
    assert!(host.compile(ONE, GRANT).is_ok());
    assert_eq!(calls.get(), 0);
}

#[test]
fn cold_branches_and_unused_helpers_keep_the_original_operation_location() {
    let calls = Cell::new(0);
    let host = host(&calls);
    for source in [
        "fn main() = choose(when: true, then: 0, otherwise: device.absent())",
        "fn unused() = device.absent()\nfn main() = 0",
        "fn unused() = choose(when: false, then: device.absent(), otherwise: 0)\nfn main() = 0",
    ] {
        let reported = compile_failure(&host, source, GRANT);
        assert_eq!(reported[0].kind, DiagnosticKind::UnknownOperation);
        let span = reported[0].span.expect("the source error has a call site");
        assert_eq!(&source[span.start..span.end], "device.absent()");
    }
    let source = "fn unused() = device.write(amount: 1)\nfn main() = 0";
    let denied = compile_failure(&host, source, &[]);
    assert_eq!(denied[0].kind, DiagnosticKind::CapabilityDenied);
    assert_eq!(calls.get(), 0);
}

#[test]
fn syntax_diagnostics_preserve_codes_and_unicode_byte_spans() {
    let calls = Cell::new(0);
    let host = host(&calls);
    let source = "// 汉字🦀\r\nfn main() = @";
    let reported = compile_failure(&host, source, &[]);
    let unexpected = reported.iter().find(|d| d.code == "LSE0002").unwrap();
    assert_eq!(unexpected.kind, DiagnosticKind::Syntax);
    let span = unexpected.span.unwrap();
    assert_eq!(&source[span.start..span.end], "@");
    let location = unexpected.location(source).unwrap();
    assert_eq!(
        location.start,
        SourcePosition {
            line: 2,
            column: 13
        }
    );
    assert_eq!(
        location.end,
        SourcePosition {
            line: 2,
            column: 14
        }
    );
    assert_eq!(calls.get(), 0);
}

fn located(span: Option<Span>) -> Diagnostic {
    Diagnostic {
        code: "custom".into(),
        kind: DiagnosticKind::Compilation,
        message: "source position".into(),
        span,
    }
}

#[test]
fn locations_are_half_open_scalar_columns_with_crlf_and_eof_support() {
    let source = "汉🦀e\u{301}\r\nx";
    let end_of_first = source.find('\r').unwrap();
    let diagnostic = located(Some(Span {
        start: "汉".len(),
        end: end_of_first,
    }));
    let location = diagnostic.location(source).unwrap();
    assert_eq!(location.start, SourcePosition { line: 1, column: 2 });
    assert_eq!(location.end, SourcePosition { line: 1, column: 5 });
    let diagnostic = located(Some(Span {
        start: "汉".len(),
        end: source.len(),
    }));
    let location = diagnostic.location(source).unwrap();
    assert_eq!(location.end, SourcePosition { line: 2, column: 2 });
    let source = "fn main() =\r\n";
    let location = located(Some(Span {
        start: source.len(),
        end: source.len(),
    }))
    .location(source)
    .unwrap();
    assert_eq!(location.start, SourcePosition { line: 2, column: 1 });
    assert_eq!(location.start, location.end);
}

#[test]
fn missing_and_invalid_utf8_ranges_do_not_guess_locations_or_panic() {
    let source = "é🦀";
    for span in [
        None,
        Some(Span { start: 1, end: 2 }),
        Some(Span { start: 0, end: 1 }),
        Some(Span { start: 2, end: 1 }),
        Some(Span {
            start: 0,
            end: usize::MAX,
        }),
        Some(Span {
            start: usize::MAX,
            end: usize::MAX,
        }),
    ] {
        assert!(located(span).location(source).is_none(), "{span:?}");
    }
    let source_limit = diagnostics::compile(&CompileError::SourceLimit);
    assert_eq!(source_limit[0].kind, DiagnosticKind::Compilation);
    assert!(source_limit[0].span.is_none());
    assert!(source_limit[0].location(source).is_none());
}

#[test]
fn program_reuse_and_two_explicit_effects_are_unchanged_after_diagnostic_queries() {
    let calls = Cell::new(0);
    let host = host(&calls);
    let program = host.compile(TWO, GRANT).unwrap();
    let error = host
        .compile("fn main() = device.absent()", GRANT)
        .err()
        .unwrap();
    for expected_calls in [2, 4] {
        let mut session = host.start(&program, GRANT, Fuel::new(1000)).unwrap();
        for _ in 0..2 {
            let Poll::Request(request) = session.poll().unwrap() else {
                panic!("expected request")
            };
            let before = session.fuel_remaining();
            let reported = diagnostics::compile(&error);
            assert_eq!(reported[0].kind, DiagnosticKind::UnknownOperation);
            assert_eq!(session.fuel_remaining(), before);
            assert!(matches!(session.poll().unwrap(), Poll::Awaiting));
            let request = session.authorize(request).unwrap();
            let receipt = session.dispatch(request).unwrap();
            session.accept(receipt).unwrap();
        }
        assert!(matches!(
            session.poll().unwrap(),
            Poll::Value(ScalarValue::Integer(22))
        ));
        assert_eq!(calls.get(), expected_calls);
    }
}

#[test]
fn runtime_diagnostics_preserve_committed_effects_and_do_not_refill_or_resume() {
    let calls = Cell::new(0);
    let host = host(&calls);
    let source =
        "fn main() = bind(first: device.write(amount: 1), body: parse_integer(value: \"bad\"))";
    let program = host.compile(source, GRANT).unwrap();
    let mut session = host.start(&program, GRANT, Fuel::new(1000)).unwrap();
    let Poll::Request(request) = session.poll().unwrap() else {
        panic!("expected request")
    };
    let request = session.authorize(request).unwrap();
    let receipt = session.dispatch(request).unwrap();
    session.accept(receipt).unwrap();
    let error = session.poll().unwrap_err();
    let before = session.fuel_remaining();
    for _ in 0..3 {
        let diagnostic = diagnostics::runtime(&error);
        assert_eq!(diagnostic.kind, DiagnosticKind::Runtime);
        assert_eq!(diagnostic.code, "LSR0001");
        assert!(diagnostic.message.contains("integer"));
        assert!(diagnostic.span.is_none());
        assert!(diagnostic.location(source).is_none());
    }
    assert_eq!(session.fuel_remaining(), before);
    assert_eq!(calls.get(), 1);
    assert!(matches!(
        session.poll().unwrap(),
        Poll::Terminal(EffectSessionEnd::Failed)
    ));
    assert_eq!(calls.get(), 1);
}

#[test]
fn a_fuel_failure_has_no_invented_source_range_or_host_dispatch() {
    let calls = Cell::new(0);
    let host = host(&calls);
    let program = host.compile(ONE, GRANT).unwrap();
    let mut session = host.start(&program, GRANT, Fuel::new(0)).unwrap();
    let error = session.poll().unwrap_err();
    let diagnostic = diagnostics::runtime(&error);
    assert_eq!(diagnostic.kind, DiagnosticKind::Runtime);
    assert!(diagnostic.message.to_lowercase().contains("fuel"));
    assert!(diagnostic.span.is_none());
    assert_eq!(session.fuel_remaining(), 0);
    assert_eq!(calls.get(), 0);
}

#[test]
fn inspecting_a_denied_request_does_not_restore_authority_or_consume_its_handle() {
    let calls = Cell::new(0);
    let host = host(&calls);
    let program = host.compile(ONE, GRANT).unwrap();
    let mut session = host.start(&program, GRANT, Fuel::new(1000)).unwrap();
    let Poll::Request(request) = session.poll().unwrap() else {
        panic!("expected request")
    };
    session.set_grants(&[]);
    let rejected = session.authorize(request).unwrap_err();
    let before = session.fuel_remaining();
    let diagnostic = diagnostics::runtime(&rejected.error);
    assert_eq!(diagnostic.kind, DiagnosticKind::CapabilityDenied);
    assert!(!diagnostic.message.contains("--allow"));
    assert!(!diagnostic.message.contains("std.print"));
    assert_eq!(session.fuel_remaining(), before);
    let still_denied = session.authorize(rejected.input).unwrap_err();
    assert!(matches!(still_denied.error, Error::Denied));
    assert_eq!(calls.get(), 0);
    assert!(session.cancel());
}

#[test]
fn uncertain_host_diagnostics_keep_the_terminal_boundary_and_original_message() {
    let calls = Cell::new(0);
    let mut builder = HostBuilder::new();
    builder
        .register(
            "device.write",
            vec![],
            ScalarType::None,
            "device.control",
            |_, _| {
                calls.set(calls.get() + 1);
                Err(HostFailure::Uncertain(
                    "device acknowledged only part ✓\nnext line".into(),
                ))
            },
        )
        .unwrap();
    let host = builder.build();
    let program = host.compile("fn main() = device.write()", GRANT).unwrap();
    let mut session = host.start(&program, GRANT, Fuel::new(1000)).unwrap();
    let Poll::Request(request) = session.poll().unwrap() else {
        panic!("expected request")
    };
    let request = session.authorize(request).unwrap();
    let failure = session.dispatch(request).unwrap_err();
    assert!(failure.request.is_none());
    let before = session.fuel_remaining();
    let diagnostic = diagnostics::runtime(&failure.error);
    assert_eq!(diagnostic.kind, DiagnosticKind::HostUncertain);
    assert_eq!(diagnostic.kind.as_str(), "host_uncertain");
    assert!(
        diagnostic
            .message
            .contains("device acknowledged only part ✓\nnext line")
    );
    assert!(!diagnostic.message.contains("output may be partial"));
    assert!(diagnostic.span.is_none());
    assert_eq!(session.fuel_remaining(), before);
    assert!(matches!(
        session.poll().unwrap(),
        Poll::Terminal(EffectSessionEnd::HostUncertain)
    ));
    assert_eq!(calls.get(), 1);
}

#[test]
fn host_messages_are_bounded_on_unicode_boundaries_without_terminal_policy() {
    let message = format!("\u{1b}[2J{}", "汉🦀".repeat(1024));
    let diagnostic = diagnostics::runtime(&Error::Host(HostFailure::Failed(message)));
    assert!(diagnostic.message.chars().count() <= 512);
    assert!(diagnostic.message.contains('\u{1b}'));
    assert!(diagnostic.message.contains("汉🦀"));
    assert!(diagnostic.span.is_none());
}
