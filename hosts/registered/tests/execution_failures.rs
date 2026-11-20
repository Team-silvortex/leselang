use leselang_registered_host::{
    Error, ExecutionFailure, ExecutionFailureCode, Scope, Transport, prepare_task,
    run_prepared_registered_task, run_prepared_registered_task_detailed,
};
use serde_json::{Value, json};

struct EmptyApplication {
    requests: usize,
    transport_error: Option<&'static str>,
}

impl Transport for EmptyApplication {
    fn exchange(&mut self, request: Value) -> Result<Value, String> {
        self.requests += 1;
        if let Some(error) = self.transport_error {
            return Err(error.into());
        }
        assert_eq!(request["operation"], "gui.list");
        Ok(json!({
            "transport_version":1, "window":request["window"], "epoch":request["epoch"],
            "session":request["session"], "request_id":request["request_id"],
            "status":"ok", "value":[], "error":null,
        }))
    }

    fn is_live(&self) -> bool {
        true
    }
}

fn scope() -> Scope {
    Scope {
        window: "failure-test".into(),
        epoch: "fresh".into(),
        session: "one".into(),
    }
}

fn empty() -> EmptyApplication {
    EmptyApplication {
        requests: 0,
        transport_error: None,
    }
}

#[test]
fn missing_object_has_typed_failure_and_keeps_the_legacy_error_text() {
    let source = "fn main() = bind(target: gui.find(name: \"missing\"), body: true)";
    let mut detailed = empty();
    let failure = run_prepared_registered_task_detailed(
        &mut detailed,
        scope(),
        prepare_task(source, vec![]).unwrap(),
    )
    .unwrap_err();
    assert_eq!(failure.code(), ExecutionFailureCode::ObjectUnavailable);
    assert_eq!(failure.code().as_str(), "execution_object_unavailable");
    assert_eq!(failure.to_string(), "unknown registered object");
    assert_eq!(
        detailed.requests, 1,
        "only bootstrap; no target or action was sent"
    );
    let mut legacy = empty();
    let old_error =
        run_prepared_registered_task(&mut legacy, scope(), prepare_task(source, vec![]).unwrap())
            .unwrap_err();
    assert_eq!(old_error, failure.to_string());
    assert_eq!(legacy.requests, detailed.requests);
}

#[test]
fn successful_detailed_execution_retains_exact_report_and_fuel() {
    let source = "fn main() = bind(first: gui.list(), body: gui.list())";
    let old =
        run_prepared_registered_task(&mut empty(), scope(), prepare_task(source, vec![]).unwrap())
            .unwrap();
    let detailed = run_prepared_registered_task_detailed(
        &mut empty(),
        scope(),
        prepare_task(source, vec![]).unwrap(),
    )
    .unwrap();
    assert_eq!(detailed, old);
    assert_eq!(detailed["effects"], 2);
    assert!(detailed.get("execution_failure").is_none());
}

#[test]
fn transport_text_cannot_impersonate_a_native_failure_code() {
    for text in [
        "unknown registered object",
        "host fuel exhausted",
        "execution_cancelled",
    ] {
        let mut application = EmptyApplication {
            requests: 0,
            transport_error: Some(text),
        };
        let failure = run_prepared_registered_task_detailed(
            &mut application,
            scope(),
            prepare_task("fn main() = true", vec![]).unwrap(),
        )
        .unwrap_err();
        assert_eq!(failure.code(), ExecutionFailureCode::Failed);
        assert_eq!(failure.to_string(), text);
        assert_eq!(application.requests, 1);
    }
}

#[test]
fn typed_native_fuel_and_closed_failure_vocabulary_are_independent_of_message_text() {
    assert_eq!(
        ExecutionFailure::from(Error::Fuel).code(),
        ExecutionFailureCode::FuelExhausted
    );
    assert_eq!(
        ExecutionFailure::from(Error::Native("another failure")).code(),
        ExecutionFailureCode::Failed
    );
    assert_eq!(
        ExecutionFailure::from("unknown registered object").code(),
        ExecutionFailureCode::Failed
    );
    assert_eq!(
        ExecutionFailure::from(String::from("host fuel exhausted")).code(),
        ExecutionFailureCode::Failed
    );
    let cases = [
        (ExecutionFailureCode::Failed, "execution_failed"),
        (
            ExecutionFailureCode::ObjectUnavailable,
            "execution_object_unavailable",
        ),
        (
            ExecutionFailureCode::FuelExhausted,
            "execution_fuel_exhausted",
        ),
        (
            ExecutionFailureCode::DeadlineExceeded,
            "execution_deadline_exceeded",
        ),
        (ExecutionFailureCode::ScopeClosed, "execution_scope_closed"),
        (ExecutionFailureCode::Cancelled, "execution_cancelled"),
    ];
    for (code, expected) in cases {
        let failure = ExecutionFailure::new(code, "original error text");
        assert_eq!(failure.code().as_str(), expected);
        assert_eq!(failure.to_string(), "original error text");
    }
}
