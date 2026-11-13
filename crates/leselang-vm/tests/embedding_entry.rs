use std::cell::Cell;

use leselang_vm::embedding::{
    CompileError, Error, Fuel, Host, HostBuilder, Limits, Poll, Program, ScalarType, ScalarValue,
    diagnostics::{self, DiagnosticKind},
    parameter,
};

fn value(host: &Host<'_>, program: &Program) -> ScalarValue {
    let mut session = host.start(program, &[], Fuel::new(1000)).unwrap();
    let Poll::Value(value) = session.poll().unwrap() else {
        panic!("expected a pure scalar result");
    };
    value
}

#[test]
fn default_and_explicit_main_keep_the_same_compilation_contract() {
    let host = HostBuilder::new().build();
    let source = "fn main() = add(left: 19, right: 23)";
    for program in [
        host.compile(source, &[]).unwrap(),
        host.compile_entry(source, "main", &[]).unwrap(),
    ] {
        assert_eq!(program.entry_name(), "main");
        assert_eq!(program.result_type(), ScalarType::Integer);
        assert_eq!(value(&host, &program), ScalarValue::Integer(42));
    }
    assert!(host.compile("fn task() = 42", &[]).is_err());
}

#[test]
fn named_entry_owns_its_inputs_and_can_be_reused_only_with_its_host() {
    let host = HostBuilder::new().build();
    let program = {
        let source = String::from("fn task() = 42");
        let entry = String::from("task");
        host.compile_entry(&source, &entry, &[]).unwrap()
    };
    assert_eq!(program.entry_name(), "task");
    assert_eq!(program.result_type(), ScalarType::Integer);
    assert_eq!(value(&host, &program), ScalarValue::Integer(42));
    assert_eq!(value(&host, &program), ScalarValue::Integer(42));
    let other = HostBuilder::new().build();
    assert!(matches!(
        other.start(&program, &[], Fuel::new(1000)),
        Err(Error::ForeignProgram)
    ));
}

#[test]
fn unselected_main_can_be_a_parameterized_helper_in_either_source_order() {
    let host = HostBuilder::new().build();
    for source in [
        "fn main(n: integer) = add(left: n, right: 23)\nfn task() = main(n: 19)",
        "fn task() = main(n: 19)\nfn main(n: integer) = add(left: n, right: 23)",
    ] {
        let program = host.compile_entry(source, "task", &[]).unwrap();
        assert_eq!(program.entry_name(), "task");
        assert_eq!(program.result_type(), ScalarType::Integer);
        assert_eq!(value(&host, &program), ScalarValue::Integer(42));
        assert!(host.compile(source, &[]).is_err());
    }
}

#[test]
fn metadata_preserves_each_inferred_scalar_type_through_helpers() {
    let host = HostBuilder::new().build();
    for (body, expected) in [
        ("42", ScalarType::Integer),
        ("true", ScalarType::Boolean),
        ("\"ready\"", ScalarType::String),
        ("none", ScalarType::None),
        ("optional_string(value: none)", ScalarType::OptionalString),
        ("strings(first: \"ready\")", ScalarType::StringList),
    ] {
        let source = format!("fn helper() = {body}\nfn task() = helper()");
        let program = host.compile_entry(&source, "task", &[]).unwrap();
        assert_eq!(program.result_type(), expected);
        assert_eq!(value(&host, &program).scalar_type(), expected);
    }
}

#[test]
fn named_entry_still_drives_two_host_effects_explicitly() {
    let calls = Cell::new(0);
    let mut builder = HostBuilder::new();
    builder
        .register(
            "counter.next",
            vec![parameter("value", ScalarType::Integer)],
            ScalarType::Integer,
            "counter.write",
            |arguments, _| {
                let Some(ScalarValue::Integer(value)) = arguments.get("value") else {
                    panic!("argument contract was not checked");
                };
                calls.set(calls.get() + 1);
                Ok(ScalarValue::Integer(value + 1))
            },
        )
        .unwrap();
    let host = builder.build();
    let grants = &["counter.write"];
    let program = host
        .compile_entry(
            "fn main(n: integer) = counter.next(value: n)\n\
             fn task() = bind(first: main(n: 40), body: main(n: first))",
            "task",
            grants,
        )
        .unwrap();
    assert_eq!(program.result_type(), ScalarType::Integer);
    assert_eq!(calls.get(), 0);
    let mut session = host.start(&program, grants, Fuel::new(1000)).unwrap();
    for expected_calls in 1..=2 {
        let Poll::Request(request) = session.poll().unwrap() else {
            panic!("expected a host effect");
        };
        assert_eq!(calls.get(), expected_calls - 1);
        let authorized = session.authorize(request).unwrap();
        assert_eq!(calls.get(), expected_calls - 1);
        let receipt = session.dispatch(authorized).unwrap();
        assert_eq!(calls.get(), expected_calls);
        session.accept(receipt).unwrap();
        assert_eq!(calls.get(), expected_calls);
    }
    assert!(matches!(
        session.poll().unwrap(),
        Poll::Value(ScalarValue::Integer(42))
    ));
    let mut denied = host.start(&program, &[], Fuel::new(1000)).unwrap();
    assert!(denied.poll().is_err());
    assert_eq!(calls.get(), 2);
}

#[test]
fn invalid_missing_and_parameterized_entries_keep_structured_diagnostics() {
    let host = HostBuilder::new().build();
    for (source, entry, expected_message, has_span) in [
        (
            "fn task() = 0",
            "bad.name",
            "designated entry name is invalid",
            false,
        ),
        (
            "fn task() = 0",
            "absent",
            "designated entry declaration is missing",
            true,
        ),
        (
            "fn task(n: integer) = n",
            "task",
            "designated entry cannot have parameters",
            true,
        ),
    ] {
        let Err(error) = host.compile_entry(source, entry, &[]) else {
            panic!("invalid entry was accepted");
        };
        let notes = diagnostics::compile(&error);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].code, "LSC0001");
        assert_eq!(notes[0].kind, DiagnosticKind::Compilation);
        assert_eq!(notes[0].message, expected_message);
        assert_eq!(notes[0].span.is_some(), has_span);
        if has_span {
            assert!(notes[0].location(source).is_some());
        }
    }
}

#[test]
fn selected_entry_cannot_be_called_even_from_an_unused_declaration() {
    let host = HostBuilder::new().build();
    for source in [
        "fn task() = task()",
        "fn task() = 0\nfn main() = task()",
        "fn unused() = task()\nfn task() = 0",
    ] {
        let Err(error) = host.compile_entry(source, "task", &[]) else {
            panic!("entry call was accepted");
        };
        let notes = diagnostics::compile(&error);
        assert_eq!(notes[0].code, "LSC0001");
        assert_eq!(notes[0].kind, DiagnosticKind::Compilation);
        assert!(notes[0].message.contains("entry"));
        assert!(notes[0].location(source).is_some());
    }
}

#[test]
fn named_entry_preserves_cold_checks_for_all_declarations_and_branches() {
    let mut builder = HostBuilder::new();
    builder
        .register("read", vec![], ScalarType::Integer, "read", |_, _| {
            panic!("compilation must not call the host");
        })
        .unwrap();
    let host = builder.build();
    for (source, kind) in [
        (
            "fn main() = missing()\nfn task() = 0",
            DiagnosticKind::UnknownOperation,
        ),
        (
            "fn unused() = add(left: true, right: 1)\nfn task() = 0",
            DiagnosticKind::Compilation,
        ),
        (
            "fn main() = read()\nfn task() = 0",
            DiagnosticKind::CapabilityDenied,
        ),
        (
            "fn task() = choose(when: true, then: 0, otherwise: read())",
            DiagnosticKind::CapabilityDenied,
        ),
    ] {
        let Err(error) = host.compile_entry(source, "task", &[]) else {
            panic!("invalid cold source was accepted");
        };
        let notes = diagnostics::compile(&error);
        assert_eq!(notes[0].code, "LSC0001");
        assert_eq!(notes[0].kind, kind);
    }
    let program = host
        .compile_entry("fn main() = read()\nfn task() = 0", "task", &["read"])
        .unwrap();
    assert_eq!(value(&host, &program), ScalarValue::Integer(0));
}

#[test]
fn named_entry_preserves_source_limits_and_syntax_errors() {
    let host = HostBuilder::with_limits(Limits {
        max_source_bytes: 8,
        ..Limits::default()
    })
    .build();
    assert!(matches!(
        host.compile_entry("fn task() = 42", "task", &[]),
        Err(CompileError::SourceLimit)
    ));
    let host = HostBuilder::new().build();
    assert!(matches!(
        host.compile_entry("fn task() = @", "task", &[]),
        Err(CompileError::Syntax(_))
    ));
}
