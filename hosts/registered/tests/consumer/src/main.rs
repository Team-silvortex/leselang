//! An independent Cargo workspace consumes only the registered host's public API.
use leselang_registered_host::{
    PreparedTask, Scope, Transport, describe_registered, prepare_task, run_prepared_registered_task,
};
use serde_json::{Value, json};

const TWO_EFFECTS: &str = "fn main() = bind(first: gui.list(), body: gui.list())";

struct EmptyApplication {
    live: bool,
    requests: usize,
}

impl Transport for EmptyApplication {
    fn exchange(&mut self, request: Value) -> Result<Value, String> {
        assert!(self.live);
        assert_eq!(request["operation"], "gui.list");
        self.requests += 1;
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
        window: "independent-rust".into(),
        epoch: "fresh-instance".into(),
        session: "two-effects".into(),
    }
}

fn main() {
    let catalog: Value = serde_json::from_str(&describe_registered().unwrap()).unwrap();
    assert_eq!(catalog["profile"], "javascript.registered");
    assert_eq!(catalog["operations"].as_array().unwrap().len(), 5);
    assert!(catalog.get("semantics").is_none());

    let prepared: PreparedTask = prepare_task(TWO_EFFECTS, vec![]).unwrap();
    let mut application = EmptyApplication {
        live: true,
        requests: 0,
    };
    let report = run_prepared_registered_task(&mut application, scope(), prepared).unwrap();
    assert_eq!(report["profile"], "javascript.registered");
    assert_eq!(report["kind"], "javascript_task_run");
    assert_eq!(report["effects"], 2);
    assert_eq!(report["result"], json!({"type":"string_list", "value":[]}));
    assert_eq!(report["objects_before"], json!([]));
    assert_eq!(report["objects_after"], json!([]));
    assert!(
        report["fuel_remaining"]
            .as_u64()
            .is_some_and(|fuel| fuel > 0 && fuel < 10_000)
    );
    assert_eq!(
        application.requests, 3,
        "bootstrap plus two explicit VM Effects"
    );

    application.live = false;
    let prepared = prepare_task(TWO_EFFECTS, vec![]).unwrap();
    assert!(run_prepared_registered_task(&mut application, scope(), prepared).is_err());
    assert_eq!(
        application.requests, 3,
        "revoked transport receives no new requests"
    );
    assert!(prepare_task("not a LeseLang program", vec![]).is_err());
    println!("{report}");
}
