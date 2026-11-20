use leselang_tauri_host::{
    AgentApplication, ExecutionFailureCode, Scope, Transport, prepare_named_task, prepare_task,
    run_prepared_registered_task, run_prepared_task, run_prepared_task_for_application,
    run_prepared_task_for_application_detailed,
};
use serde_json::{Value, json};

struct EmptyRegistry {
    live: bool,
    requests: usize,
}

impl Transport for EmptyRegistry {
    fn exchange(&mut self, request: Value) -> Result<Value, String> {
        self.requests += 1;
        assert_eq!(request["operation"], "gui.list");
        Ok(json!({
            "transport_version":1, "window":request["window"], "epoch":request["epoch"],
            "session":request["session"], "request_id":request["request_id"],
            "status":"ok", "value":[], "error":null,
        }))
    }

    fn is_live(&self) -> bool {
        self.live
    }
}

fn scope() -> Scope {
    Scope {
        window: "main".into(),
        epoch: "test-page".into(),
        session: "test-task".into(),
    }
}

#[test]
fn typed_host_configuration_changes_only_report_identity_not_vm_execution() {
    let mut reports = Vec::new();
    for application in [
        AgentApplication::Form,
        AgentApplication::TaskBoard,
        AgentApplication::Article,
    ] {
        let task = prepare_task("fn main() = 42", Vec::new()).unwrap();
        let mut transport = EmptyRegistry {
            live: true,
            requests: 0,
        };
        let report =
            run_prepared_task_for_application(&mut transport, scope(), task, application).unwrap();
        assert_eq!(report["profile"], application.profile());
        assert_eq!(report["kind"], "tauri_task_run");
        assert_eq!(report["result"], json!({"type":"integer", "value":42}));
        assert_eq!(report["effects"], 0);
        assert_eq!(report["native_before"], json!([]));
        assert_eq!(report["native_after"], json!([]));
        assert_eq!(transport.requests, 1);
        reports.push(report);
    }
    assert_eq!(reports[0]["fuel_remaining"], reports[1]["fuel_remaining"]);
    let task = prepare_task("fn main() = 42", Vec::new()).unwrap();
    let mut transport = EmptyRegistry {
        live: true,
        requests: 0,
    };
    assert_eq!(
        run_prepared_task(&mut transport, scope(), task).unwrap(),
        reports[0]
    );
    let task = prepare_task("fn main() = 42", Vec::new()).unwrap();
    let mut transport = EmptyRegistry {
        live: true,
        requests: 0,
    };
    let mut registered = run_prepared_registered_task(&mut transport, scope(), task).unwrap();
    assert_eq!(registered["profile"], "javascript.registered");
    assert_eq!(registered["kind"], "javascript_task_run");
    let fields = registered.as_object_mut().unwrap();
    let before = fields.remove("objects_before").unwrap();
    let after = fields.remove("objects_after").unwrap();
    fields.insert("native_before".into(), before);
    fields.insert("native_after".into(), after);
    fields.insert("profile".into(), json!("tauri.typescript"));
    fields.insert("kind".into(), json!("tauri_task_run"));
    assert_eq!(
        registered, reports[0],
        "only report identity and observation field names change"
    );
    assert_eq!(transport.requests, 1);
}

#[test]
fn an_application_identity_never_reopens_a_closed_transport() {
    for application in [
        AgentApplication::Form,
        AgentApplication::TaskBoard,
        AgentApplication::Article,
    ] {
        let task = prepare_task("fn main() = 42", Vec::new()).unwrap();
        let mut transport = EmptyRegistry {
            live: false,
            requests: 0,
        };
        assert!(
            run_prepared_task_for_application(&mut transport, scope(), task, application).is_err()
        );
        assert_eq!(transport.requests, 0);
    }
}

#[test]
fn article_named_arguments_use_the_shared_vm_and_native_report_fields() {
    let title = "Title: \"quoted\" ✓";
    let content = "Content\n正文";
    let task = prepare_named_task(
        "fn main(title: string, content: string) = concat(left: title, right: content)",
        vec![
            ("content".into(), content.into()),
            ("title".into(), title.into()),
        ],
    )
    .unwrap();
    let mut transport = EmptyRegistry {
        live: true,
        requests: 0,
    };
    let report =
        run_prepared_task_for_application(&mut transport, scope(), task, AgentApplication::Article)
            .unwrap();
    assert_eq!(report["profile"], "tauri.article");
    assert_eq!(report["kind"], "tauri_task_run");
    assert_eq!(
        report["result"],
        json!({"type":"string", "value":format!("{title}{content}")})
    );
    assert_eq!(report["native_before"], json!([]));
    assert_eq!(report["native_after"], json!([]));
    assert!(report.get("objects_before").is_none());
    assert!(report.get("objects_after").is_none());
    assert_eq!(report["effects"], 0);
    assert_eq!(transport.requests, 1);
}

#[test]
fn detailed_application_entry_preserves_success_and_typed_missing_object_failure() {
    for application in [
        AgentApplication::Form,
        AgentApplication::TaskBoard,
        AgentApplication::Article,
    ] {
        let source = "fn main() = bind(first: gui.list(), body: gui.list())";
        let mut transport = EmptyRegistry {
            live: true,
            requests: 0,
        };
        let expected = run_prepared_task_for_application(
            &mut transport,
            scope(),
            prepare_task(source, vec![]).unwrap(),
            application,
        )
        .unwrap();
        let report = run_prepared_task_for_application_detailed(
            &mut transport,
            scope(),
            prepare_task(source, vec![]).unwrap(),
            application,
        )
        .unwrap();
        assert_eq!(report, expected);
        assert!(report.get("execution_failure").is_none());
        let missing = "fn main() = bind(target: gui.find(name: \"missing\"), body: true)";
        let failure = run_prepared_task_for_application_detailed(
            &mut transport,
            scope(),
            prepare_task(missing, vec![]).unwrap(),
            application,
        )
        .unwrap_err();
        assert_eq!(failure.code(), ExecutionFailureCode::ObjectUnavailable);
        assert_eq!(failure.to_string(), "unknown registered object");
        let old = run_prepared_task_for_application(
            &mut transport,
            scope(),
            prepare_task(missing, vec![]).unwrap(),
            application,
        )
        .unwrap_err();
        assert_eq!(old, failure.to_string());
    }
}
