//! An independent Cargo workspace consumes only the registered host's public API.
mod article;
mod publication;

use leselang_registered_host::{
    Error, ExecutionFailure, ExecutionFailureCode, PreparedTask, Scope, Transport,
    describe_registered, prepare_task, run_prepared_registered_task,
    run_prepared_registered_task_detailed,
};
use serde_json::{Value, json};
use std::{collections::HashSet, path::Path};

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

fn check_publications(path: &Path, measure: bool) {
    let fixture = publication::Fixture::load(path);
    let mut ids = HashSet::new();
    assert!(!fixture.cases.is_empty(), "empty publication fixture");
    for case in &fixture.cases {
        let id = case["id"].as_str().unwrap();
        assert!(ids.insert(id), "duplicate publication case {id}");
        let (observed, _, timing) = fixture.check(case, measure);
        println!("LESE_GUI {}", json!({"id":id, "observed":observed}));
        if let Some(timing) = timing {
            println!("LESE_GUI_TIMING {timing}");
        }
    }
}

fn main() {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    match arguments.as_slice() {
        [] => check_publications(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../../fixtures/registered-gui/publication.json"),
            false,
        ),
        [option, path]
            if option == "--registered-gui" || option == "--benchmark-registered-gui" =>
        {
            check_publications(Path::new(path), option == "--benchmark-registered-gui");
            return;
        }
        _ => panic!(
            "usage: leselang-registered-host-consumer [--registered-gui FIXTURE | --benchmark-registered-gui FIXTURE]"
        ),
    }
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

    let mut detailed_application = EmptyApplication {
        live: true,
        requests: 0,
    };
    let detailed = run_prepared_registered_task_detailed(
        &mut detailed_application,
        scope(),
        prepare_task(TWO_EFFECTS, vec![]).unwrap(),
    )
    .unwrap();
    assert_eq!(detailed, report);
    let missing = "fn main() = bind(target: gui.find(name: \"missing\"), body: true)";
    let failure: ExecutionFailure = run_prepared_registered_task_detailed(
        &mut detailed_application,
        scope(),
        prepare_task(missing, vec![]).unwrap(),
    )
    .unwrap_err();
    assert_eq!(failure.code(), ExecutionFailureCode::ObjectUnavailable);
    assert_eq!(failure.code().as_str(), "execution_object_unavailable");
    let legacy_error: String = run_prepared_registered_task(
        &mut detailed_application,
        scope(),
        prepare_task(missing, vec![]).unwrap(),
    )
    .unwrap_err();
    assert_eq!(legacy_error, failure.to_string());
    // Existing downstream exhaustive matches remain source-compatible.
    let original = match Error::Fuel {
        Error::Native(message) => message.to_owned(),
        Error::Fuel => "host fuel exhausted".to_owned(),
        Error::Object(error) => error.to_string(),
        Error::Transport(message) => message,
    };
    assert_eq!(original, "host fuel exhausted");

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
