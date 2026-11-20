use leselang_registered_host as registered;
use leselang_tauri_host as tauri;
use serde_json::{Value, json};

struct EmptyRegistry {
    requests: usize,
}

// An implementation of the historical trait must also satisfy the new entry.
impl tauri::Transport for EmptyRegistry {
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
        true
    }
}

fn scope() -> registered::Scope {
    tauri::Scope {
        window: "main".into(),
        epoch: "compatible-instance".into(),
        session: "compatible-task".into(),
    }
}

#[test]
fn historical_and_registered_exports_share_types_and_vm_execution() {
    const SOURCE: &str = "fn main() = bind(first: gui.list(), body: gui.list())";
    let old_task: tauri::PreparedTask = registered::prepare_task(SOURCE, vec![]).unwrap();
    let new_task: registered::PreparedTask = tauri::prepare_task(SOURCE, vec![]).unwrap();
    let old_scope: tauri::Scope = scope();
    let old_error: tauri::Error = registered::Error::Fuel;
    assert!(matches!(old_error, registered::Error::Fuel));
    let _: Option<tauri::host::Node> = None::<registered::host::Node>;
    let _: Option<tauri::host::NativeType> = None::<registered::host::NativeType>;
    let _: Option<tauri::gateway::WidgetSnapshot> = None::<registered::gateway::WidgetSnapshot>;

    let mut direct = EmptyRegistry { requests: 0 };
    let direct_report =
        registered::run_prepared_registered_task(&mut direct, old_scope, old_task).unwrap();
    let mut compatibility = EmptyRegistry { requests: 0 };
    let compatibility_report =
        tauri::run_prepared_registered_task(&mut compatibility, scope(), new_task).unwrap();
    assert_eq!(direct_report, compatibility_report);
    assert_eq!(direct_report["effects"], 2);
    assert_eq!(
        direct_report["result"],
        json!({"type":"string_list", "value":[]})
    );
    assert_eq!(direct.requests, 3);
    assert_eq!(compatibility.requests, direct.requests);
    assert_eq!(
        registered::describe_registered().unwrap(),
        tauri::describe_registered().unwrap()
    );
}
