//! Deterministic transport fixtures exercise the real Rust compiler/VM and bridge.
//! Actual JavaScript and WebView acceptance are separate tests.
use leselang_tauri_host::{
    Scope, Transport,
    gateway::{Backend, RemoteGui},
    host,
};
use serde_json::{Value, json};
use std::{cell::RefCell, rc::Rc};

const SOURCE: &str = include_str!("../examples/form.lese");

fn scope() -> Scope {
    Scope {
        window: "main".into(),
        epoch: "document-1".into(),
        session: "session-1".into(),
    }
}

#[derive(Clone, Copy)]
enum Corrupt {
    Window,
    Epoch,
    Session,
    Serial,
    Version,
    Status,
    SuccessWithError,
    ExtraField,
    ValueType,
    Oversized,
    CloseBeforeReply,
    Rejected,
    Uncertain,
}

struct Model {
    values: Vec<Value>,
    names: Vec<String>,
    requests: Vec<Value>,
    live: bool,
    corrupt: Option<Corrupt>,
    corrupt_write: Option<Corrupt>,
    writes: usize,
}

impl Model {
    fn new() -> Self {
        let control = |identifier: &str, role: &str, revision: u32| {
            json!({
                "identifier":identifier,"generation":1,"revision":revision,
                "role":role,"title":"draft","value":"","enabled":true,
                "editable":role=="text_field","clicks":0,
            })
        };
        Self {
            values: vec![
                control("save", "button", 80),
                control("other", "button", 20),
                control("input", "text_field", 37),
            ],
            names: vec!["input".into(), "save".into(), "other".into()],
            requests: Vec::new(),
            live: true,
            corrupt: None,
            corrupt_write: None,
            writes: 0,
        }
    }
    fn advance(&mut self, slot: usize, count: u64) {
        self.values[slot]["revision"] =
            json!(self.values[slot]["revision"].as_u64().unwrap() + count);
    }
}

struct Fake(Rc<RefCell<Model>>);
impl Fake {
    fn new() -> (Self, Rc<RefCell<Model>>) {
        let model = Rc::new(RefCell::new(Model::new()));
        (Self(model.clone()), model)
    }
}

impl Transport for Fake {
    fn exchange(&mut self, request: Value) -> Result<Value, String> {
        let mut model = self.0.borrow_mut();
        if !model.live {
            return Err("fixture is closed".into());
        }
        model.requests.push(request.clone());
        let mut error = None;
        let args = &request["arguments"];
        let value = match request["operation"].as_str().unwrap() {
            "gui.list" => json!(model.names),
            "gui.find" => {
                let observed = model
                    .values
                    .iter()
                    .find(|value| value["identifier"] == args["name"])
                    .ok_or("unknown fixture object")?;
                json!({"name":observed["identifier"],"generation":observed["generation"]})
            }
            operation => {
                let slot = model
                    .values
                    .iter()
                    .position(|value| value["identifier"] == args["target"]["name"])
                    .ok_or("unknown fixture target")?;
                if model.values[slot]["generation"] != args["target"]["generation"] {
                    error = Some("stale_object");
                    Value::Null
                } else if operation == "gui.observe" {
                    model.values[slot].clone()
                } else if model.values[slot]["revision"] != args["expected_revision"] {
                    error = Some("stale_revision");
                    Value::Null
                } else if operation == "gui.set_text" {
                    model.values[slot]["value"] = args["text"].clone();
                    model.advance(slot, 7);
                    model.advance(0, 11);
                    model.writes += 1;
                    json!(true)
                } else if operation == "gui.click" {
                    model.values[slot]["value"] = model.values[2]["value"].clone();
                    model.values[slot]["title"] = json!("ready");
                    model.values[slot]["clicks"] =
                        json!(model.values[slot]["clicks"].as_u64().unwrap() + 1);
                    model.advance(slot, 13);
                    model.writes += 1;
                    json!(true)
                } else {
                    return Err("unexpected fixture operation".into());
                }
            }
        };
        let mut reply = json!({
            "transport_version":1,"window":request["window"],"epoch":request["epoch"],
            "session":request["session"],"request_id":request["request_id"],
            "status":if error.is_some(){"rejected"}else{"ok"},"value":value,"error":error,
        });
        if matches!(
            request["operation"].as_str(),
            Some("gui.set_text" | "gui.click")
        ) && let Some(corrupt) = model.corrupt_write.take()
        {
            model.corrupt = Some(corrupt);
        }
        if let Some(corrupt) = model.corrupt.take() {
            match corrupt {
                Corrupt::Window => reply["window"] = json!("other"),
                Corrupt::Epoch => reply["epoch"] = json!("previous-document"),
                Corrupt::Session => reply["session"] = json!("previous-session"),
                Corrupt::Serial => {
                    reply["request_id"] = json!(request["request_id"].as_u64().unwrap() - 1)
                }
                Corrupt::Version => reply["transport_version"] = json!(2),
                Corrupt::Status => reply["status"] = json!("submitted"),
                Corrupt::SuccessWithError => reply["error"] = json!("contradictory"),
                Corrupt::ExtraField => reply["extra"] = json!(true),
                Corrupt::ValueType => reply["value"] = json!(123),
                Corrupt::Oversized => reply["value"] = json!("x".repeat(32 * 1024)),
                Corrupt::CloseBeforeReply => model.live = false,
                Corrupt::Rejected | Corrupt::Uncertain => {
                    reply["status"] = json!(if matches!(corrupt, Corrupt::Rejected) {
                        "rejected"
                    } else {
                        "uncertain"
                    });
                    reply["value"] = Value::Null;
                    reply["error"] = json!("fixture_failure");
                }
            }
        }
        Ok(reply)
    }
    fn is_live(&self) -> bool {
        self.0.borrow().live
    }
}

#[test]
fn real_source_crosses_the_wire_and_compares_distinct_local_and_remote_revisions() {
    let (mut transport, model) = Fake::new();
    let text = "保存 \"✓\" \\\n";
    let report = leselang_tauri_host::run(&mut transport, scope(), SOURCE, text).unwrap();
    assert_eq!(report["verified"], true);
    assert_eq!(report["result"], json!({"type":"boolean","value":true}));
    assert_eq!(report["effects"], 9);
    assert!(
        report["fuel_remaining"]
            .as_u64()
            .is_some_and(|fuel| fuel > 0 && fuel < 10_000)
    );
    assert_eq!(report["native_after"][0]["value"], text);
    assert_eq!(report["native_after"][2]["value"], text);
    assert_eq!(report["native_after"][1], report["native_before"][1]);
    let model = model.borrow();
    assert_eq!(model.writes, 2);
    let actions: Vec<_> = model
        .requests
        .iter()
        .filter(|request| {
            matches!(
                request["operation"].as_str(),
                Some("gui.set_text" | "gui.click")
            )
        })
        .collect();
    assert_eq!(actions.len(), 2);
    assert_eq!(actions[0]["arguments"]["expected_revision"], 37);
    assert_eq!(actions[1]["arguments"]["expected_revision"], 91);
    for (index, request) in model.requests.iter().enumerate() {
        assert_eq!(request["request_id"], index + 1);
        assert_eq!(request["budget_ms"], 5000);
        assert_eq!(request["transport_version"], 1);
    }
}

#[test]
fn metadata_planned_form_runs_the_real_vm_and_refuses_disabled_input() {
    let source =
        include_str!("../../../consumer/examples/catalog_agent/fixtures/form-semantics.lese");
    let (mut transport, model) = Fake::new();
    let text = "语义 ✓";
    let report = leselang_tauri_host::run(&mut transport, scope(), source, text).unwrap();
    assert_eq!(report["verified"], true);
    assert_eq!(report["result"], json!({"type":"boolean","value":true}));
    assert_eq!(report["effects"], 9);
    assert!(
        report["fuel_remaining"]
            .as_u64()
            .is_some_and(|fuel| fuel > 0 && fuel < 10_000)
    );
    assert_eq!(report["native_after"][0]["value"], text);
    assert_eq!(report["native_after"][2]["value"], text);
    assert_eq!(report["native_after"][1], report["native_before"][1]);
    assert_eq!(model.borrow().writes, 2);

    let (mut transport, model) = Fake::new();
    model.borrow_mut().values[2]["enabled"] = json!(false);
    let report = leselang_tauri_host::run(&mut transport, scope(), source, text).unwrap();
    assert_eq!(report["verified"], false);
    assert_eq!(report["result"]["value"], false);
    assert_eq!(report["effects"], 3);
    assert_eq!(model.borrow().writes, 0);
    assert_eq!(report["native_after"], report["native_before"]);
}

#[test]
fn foreign_duplicate_malformed_and_late_wire_replies_cannot_resume_the_bridge() {
    for corruption in [
        Corrupt::Window,
        Corrupt::Epoch,
        Corrupt::Session,
        Corrupt::Serial,
        Corrupt::Version,
        Corrupt::Status,
        Corrupt::SuccessWithError,
        Corrupt::ExtraField,
        Corrupt::Oversized,
        Corrupt::CloseBeforeReply,
        Corrupt::Rejected,
        Corrupt::Uncertain,
    ] {
        let (mut transport, model) = Fake::new();
        let gui = RemoteGui::new(&mut transport, scope()).unwrap();
        model.borrow_mut().corrupt = Some(corruption);
        assert!(gui.list().is_err());
        assert!(!gui.is_open().unwrap());
        let requests = model.borrow().requests.len();
        assert!(gui.list().is_err());
        assert_eq!(
            model.borrow().requests.len(),
            requests,
            "retirement must prevent transport replay"
        );
    }
}

#[test]
fn operation_result_types_are_checked_separately_from_valid_envelopes() {
    let (mut transport, model) = Fake::new();
    let gui = RemoteGui::new(&mut transport, scope()).unwrap();
    model.borrow_mut().corrupt = Some(Corrupt::ValueType);
    assert!(gui.observe(2).is_err());
    assert_eq!(model.borrow().writes, 0);
    assert!(!gui.is_open().unwrap());
    let requests = model.borrow().requests.len();
    assert!(gui.list().is_err());
    assert_eq!(model.borrow().requests.len(), requests);
}

#[test]
fn rollback_and_unversioned_changes_cannot_replace_cached_observations() {
    for rollback in [true, false] {
        let (mut transport, model) = Fake::new();
        let gui = RemoteGui::new(&mut transport, scope()).unwrap();
        let prior = gui.snapshot(2).unwrap();
        if rollback {
            model.borrow_mut().values[2]["revision"] = json!(36);
        } else {
            model.borrow_mut().values[2]["value"] = json!("unversioned change");
        }
        assert!(gui.observe(2).is_err());
        assert_eq!(gui.snapshot(2).unwrap(), prior);
        assert!(!gui.is_open().unwrap());
        let requests = model.borrow().requests.len();
        assert!(gui.list().is_err());
        assert_eq!(model.borrow().requests.len(), requests);
    }
}

#[test]
fn new_find_accepts_replacement_revision_reset_and_old_local_reference_is_retired() {
    let (mut transport, model) = Fake::new();
    let gui = RemoteGui::new(&mut transport, scope()).unwrap();
    host::with_host(&gui, |host, scope| {
        let lease = scope.lease();
        let old = host.owner(2).reference(&lease).unwrap();
        {
            let mut model = model.borrow_mut();
            model.values[2]["generation"] = json!(2);
            model.values[2]["revision"] = json!(0);
            model.values[2]["value"] = json!("replacement");
        }
        assert_eq!(gui.find("input").unwrap(), 2);
        host.refresh(2).unwrap();
        assert!(host.owner(2).validate(&old, &lease).is_err());
        let current = host.owner(2).reference(&lease).unwrap();
        host.owner(2).validate(&current, &lease).unwrap();
        gui.observe(2).unwrap();
        let observed = gui.snapshot(2).unwrap();
        assert_eq!(observed.identity, 2);
        assert_eq!(observed.mutation, 0);
        assert_eq!(observed.value, "replacement");
    })
    .unwrap();
}

#[test]
fn replacement_without_refinding_rejects_old_remote_generation_without_mutation() {
    let (mut transport, model) = Fake::new();
    let gui = RemoteGui::new(&mut transport, scope()).unwrap();
    model.borrow_mut().values[2]["generation"] = json!(2);
    model.borrow_mut().values[2]["revision"] = json!(0);
    assert!(gui.set_text(2, 1, "must not mutate").is_err());
    assert_eq!(model.borrow().writes, 0);
    assert!(!gui.is_open().unwrap());
}

#[test]
fn generation_rollback_retires_the_bridge_without_publishing_old_object_state() {
    let (mut transport, model) = Fake::new();
    model.borrow_mut().values[2]["generation"] = json!(2);
    let gui = RemoteGui::new(&mut transport, scope()).unwrap();
    let prior = gui.snapshot(2).unwrap();
    model.borrow_mut().values[2]["generation"] = json!(1);
    model.borrow_mut().values[2]["value"] = json!("stale instance");
    assert!(gui.find("input").is_err());
    assert_eq!(gui.snapshot(2).unwrap(), prior);
    assert!(!gui.is_open().unwrap());
    let requests = model.borrow().requests.len();
    assert!(gui.list().is_err());
    assert_eq!(model.borrow().requests.len(), requests);
}

#[test]
fn action_failure_or_reply_loss_preserves_effects_without_retry() {
    for corruption in [
        Corrupt::Uncertain,
        Corrupt::CloseBeforeReply,
        Corrupt::Serial,
    ] {
        let (mut transport, model) = Fake::new();
        let gui = RemoteGui::new(&mut transport, scope()).unwrap();
        model.borrow_mut().corrupt = Some(corruption);
        assert!(gui.set_text(2, 1, "already delivered").is_err());
        assert_eq!(model.borrow().writes, 1);
        assert_eq!(model.borrow().values[2]["value"], "already delivered");
        assert!(gui.set_text(2, 1, "retry").is_err());
        assert_eq!(model.borrow().writes, 1);
    }
}

#[test]
fn authority_loss_prevents_new_requests_and_wrong_scope_never_contacts_transport() {
    let (mut transport, model) = Fake::new();
    let mut wrong = scope();
    wrong.window = "secondary".into();
    assert!(RemoteGui::new(&mut transport, wrong).is_err());
    assert!(model.borrow().requests.is_empty());
    let gui = RemoteGui::new(&mut transport, scope()).unwrap();
    model.borrow_mut().live = false;
    let count = model.borrow().requests.len();
    assert!(gui.list().is_err());
    assert_eq!(model.borrow().requests.len(), count);
}

fn registered_fixture() -> (Fake, Rc<RefCell<Model>>) {
    let (transport, shared) = Fake::new();
    {
        let mut model = shared.borrow_mut();
        for (slot, name) in ["publish", "unrelated", "draft"].iter().enumerate() {
            model.values[slot]["identifier"] = json!(name);
        }
        for name in ["reset", "status"] {
            model.values.push(json!({
                "identifier":name,"generation":1,"revision":0,
                "role":"button","title":name,"value":"","enabled":true,
                "editable":false,"clicks":0,
            }));
        }
        model.names = ["status", "draft", "publish", "unrelated", "reset"]
            .map(str::to_owned)
            .into();
    }
    (transport, shared)
}

#[test]
fn generic_runner_controls_five_application_names_without_form_verification() {
    let (mut transport, shared) = registered_fixture();
    let source = SOURCE
        .replace("\"input\"", "\"draft\"")
        .replace("\"save\"", "\"publish\"");
    let mut scope = scope();
    scope.window = "node-main".into();
    let text = "Independent JavaScript registry ✓";
    let report = leselang_tauri_host::run_registered(&mut transport, scope, &source, text).unwrap();
    assert_eq!(report["kind"], "javascript_run");
    assert_eq!(report["profile"], "javascript.registered");
    assert_eq!(report["result"], json!({"type":"boolean","value":true}));
    assert_eq!(report["effects"], 9);
    assert!(
        report.get("verified").is_none(),
        "business verification belongs to the application"
    );
    let before = report["objects_before"].as_array().unwrap();
    let after = report["objects_after"].as_array().unwrap();
    assert_eq!(after.len(), 5);
    assert_eq!(after[1]["identifier"], "draft");
    assert_eq!(after[1]["value"], text);
    assert_eq!(after[2]["identifier"], "publish");
    assert_eq!(after[2]["value"], text);
    assert_eq!(after[2]["clicks"], 1);
    for slot in [0, 3, 4] {
        assert_eq!(after[slot], before[slot]);
    }
    assert_eq!(shared.borrow().writes, 2);
}

#[test]
fn inventory_order_is_metadata_and_cannot_rebind_existing_slots() {
    let (mut transport, shared) = registered_fixture();
    let gui = RemoteGui::registered(&mut transport, scope()).unwrap();
    let slot = gui.find("draft").unwrap();
    assert_eq!(slot, 1);
    host::with_host(&gui, |host, scope| {
        let lease = scope.lease();
        let before = host.owner(slot).reference(&lease).unwrap();
        shared.borrow_mut().names.reverse();
        assert_eq!(gui.list().unwrap(), shared.borrow().names);
        assert_eq!(gui.find("draft").unwrap(), slot);
        host.refresh(slot).unwrap();
        host.owner(slot).validate(&before, &lease).unwrap();
        assert_eq!(gui.snapshot(slot).unwrap().identifier, "draft");
    })
    .unwrap();
}

#[test]
fn replacement_outside_the_form_slots_retires_only_its_original_object() {
    let (mut transport, shared) = registered_fixture();
    let gui = RemoteGui::registered(&mut transport, scope()).unwrap();
    let slot = gui.find("reset").unwrap();
    assert_eq!(slot, 4);
    host::with_host(&gui, |host, scope| {
        let lease = scope.lease();
        let old = host.owner(slot).reference(&lease).unwrap();
        let unrelated = host.owner(1).reference(&lease).unwrap();
        {
            let mut model = shared.borrow_mut();
            model.values[3]["generation"] = json!(2);
            model.values[3]["revision"] = json!(0);
            model.values[3]["title"] = json!("replaced reset");
        }
        assert_eq!(gui.find("reset").unwrap(), slot);
        host.refresh(slot).unwrap();
        assert!(host.owner(slot).validate(&old, &lease).is_err());
        host.owner(1).validate(&unrelated, &lease).unwrap();
        let current = host.owner(slot).reference(&lease).unwrap();
        host.owner(slot).validate(&current, &lease).unwrap();
        gui.observe(slot).unwrap();
        assert_eq!(gui.snapshot(slot).unwrap().title, "replaced reset");
        assert_eq!(gui.snapshot(slot).unwrap().mutation, 0);
    })
    .unwrap();
}

#[test]
fn inventory_addition_removal_or_renaming_retires_without_rebinding_slots() {
    for changed in ["addition", "removal", "rename"] {
        let (mut transport, shared) = registered_fixture();
        let gui = RemoteGui::registered(&mut transport, scope()).unwrap();
        let prior = gui.snapshot(1).unwrap();
        {
            let mut model = shared.borrow_mut();
            match changed {
                "addition" => model.names.push("new_control".into()),
                "removal" => {
                    model.names.remove(1);
                }
                "rename" => model.names[1] = "renamed_control".into(),
                _ => unreachable!(),
            }
        }
        assert!(gui.list().is_err(), "{changed}");
        assert!(!gui.is_open().unwrap());
        assert_eq!(
            gui.snapshot(1).unwrap(),
            prior,
            "historical cache stays unchanged"
        );
        let count = shared.borrow().requests.len();
        assert!(gui.find("draft").is_err());
        assert_eq!(shared.borrow().requests.len(), count);
        assert_eq!(shared.borrow().writes, 0);
    }
}

#[test]
fn removal_or_generation_rollback_at_an_arbitrary_slot_refuses_old_work() {
    for removal in [false, true] {
        let (mut transport, shared) = registered_fixture();
        shared.borrow_mut().values[4]["generation"] = json!(2);
        let gui = RemoteGui::registered(&mut transport, scope()).unwrap();
        // "status" is first in discovery, but last in the application's own storage.
        let prior = gui.snapshot(0).unwrap();
        if removal {
            shared.borrow_mut().values.pop();
        } else {
            shared.borrow_mut().values[4]["generation"] = json!(1);
        }
        assert!(gui.find("status").is_err());
        assert!(!gui.is_open().unwrap());
        assert_eq!(gui.snapshot(0).unwrap(), prior);
        assert_eq!(shared.borrow().writes, 0);
    }
}

#[test]
fn registered_names_and_scope_are_strictly_bounded_before_object_queries() {
    let mut inventories = vec![
        vec!["same".into(), "same".into()],
        (0..65).map(|index| format!("control_{index}")).collect(),
    ];
    for invalid in [
        "".into(),
        "two words".into(),
        "中文".into(),
        "slash/name".into(),
        "x".repeat(65),
    ] {
        inventories.push(vec![invalid]);
    }
    for names in inventories {
        let (mut transport, shared) = Fake::new();
        shared.borrow_mut().names = names;
        assert!(RemoteGui::registered(&mut transport, scope()).is_err());
        assert_eq!(shared.borrow().requests.len(), 1);
        assert_eq!(shared.borrow().writes, 0);
    }
    for field in ["window", "epoch", "session"] {
        let (mut transport, shared) = Fake::new();
        let mut scope = scope();
        match field {
            "window" => scope.window = "other/window".into(),
            "epoch" => scope.epoch.clear(),
            "session" => scope.session = "x".repeat(65),
            _ => unreachable!(),
        }
        assert!(RemoteGui::registered(&mut transport, scope).is_err());
        assert!(shared.borrow().requests.is_empty());
    }
}

#[test]
fn empty_and_maximum_registries_work_without_hidden_form_objects() {
    for count in [0, 64] {
        let (mut transport, shared) = Fake::new();
        {
            let mut model = shared.borrow_mut();
            let template = model.values[0].clone();
            model.values.clear();
            model.names.clear();
            for index in 0..count {
                let name = format!("object_{index}");
                let mut value = template.clone();
                value["identifier"] = json!(name);
                model.values.push(value);
                model.names.push(name);
            }
        }
        let source = if count == 0 {
            "fn main(text: string) = eq(left: text, right: text)"
        } else {
            r#"fn main(text: string) = bind(ids: gui.list(), body:
               bind(target: gui.find(name: "object_63"), body:
                 bind(observed: gui.observe(target: target), body:
                   eq(left: field(value: observed, name: "identifier"), right: text))))"#
        };
        let report =
            leselang_tauri_host::run_registered(&mut transport, scope(), source, "object_63")
                .unwrap();
        let effects = if count == 0 { 0 } else { 3 };
        assert_eq!(report["effects"], effects);
        assert_eq!(report["result"]["value"], true);
        assert_eq!(report["objects_before"].as_array().unwrap().len(), count);
        assert_eq!(report["objects_after"].as_array().unwrap().len(), count);
        assert_eq!(shared.borrow().requests.len(), 1 + count * 3 + effects);
        assert_eq!(shared.borrow().writes, 0);
    }
}

#[test]
fn task_runner_admits_no_input_and_every_scalar_result_without_form_verification() {
    fn assert_send<T: Send>() {}
    assert_send::<leselang_tauri_host::PreparedTask>();
    for (expression, expected) in [
        (
            "18446744073709551615",
            json!({"type":"integer","value":u64::MAX}),
        ),
        ("false", json!({"type":"boolean","value":false})),
        (
            r#""已经完成 ✓""#,
            json!({"type":"string","value":"已经完成 ✓"}),
        ),
        ("none", json!({"type":"none","value":null})),
        (
            r#"char_at(left: "ready", right: 0)"#,
            json!({"type":"optional_string","value":"r"}),
        ),
        (
            r#"char_at(left: "ready", right: 99)"#,
            json!({"type":"optional_string","value":null}),
        ),
        (
            r#"strings(first: "ready", second: "✓")"#,
            json!({"type":"string_list","value":["ready","✓"]}),
        ),
    ] {
        let (mut transport, model) = Fake::new();
        // This carrier must admit an application's actual registry, not require
        // the historical three form controls even for a pure task.
        model.borrow_mut().values.clear();
        model.borrow_mut().names.clear();
        let source = format!("fn main() = {expression}");
        let report =
            leselang_tauri_host::run_task(&mut transport, scope(), &source, vec![]).unwrap();
        assert_eq!(report["kind"], "tauri_task_run");
        assert_eq!(report["profile"], "tauri.typescript");
        assert_eq!(report["result"], expected, "{expression}");
        assert_eq!(report["effects"], 0);
        assert_eq!(report["native_before"], json!([]));
        assert_eq!(report["native_after"], json!([]));
        assert!(report.get("verified").is_none());
        assert!(
            report["fuel_remaining"]
                .as_u64()
                .is_some_and(|n| n < 10_000)
        );
        assert_eq!(
            model.borrow().requests.len(),
            1,
            "only inventory bootstrap is outside VM Effects"
        );
        if expression == "18446744073709551615" {
            assert!(
                serde_json::to_string(&report)
                    .unwrap()
                    .contains("18446744073709551615")
            );
        }
    }
}

#[test]
fn renamed_task_arguments_are_literal_and_match_the_legacy_runtime_fuel() {
    for text in [
        "typed \"text\" \\\n文字 ✓".to_owned(),
        "界".repeat(1365),
        "x".repeat(4096),
    ] {
        let (mut transport, _) = Fake::new();
        let report = leselang_tauri_host::run_task(
            &mut transport,
            scope(),
            "fn main(message: string) = message",
            vec![("message".into(), text.clone())],
        )
        .unwrap();
        assert_eq!(report["result"], json!({"type":"string","value":text}));
        assert_eq!(report["effects"], 0);
        assert_eq!(report["native_before"], report["native_after"]);
    }
    let source =
        include_str!("../../../consumer/examples/catalog_agent/fixtures/form-semantics.lese");
    let (mut old_transport, _) = Fake::new();
    let legacy =
        leselang_tauri_host::run(&mut old_transport, scope(), source, "same input ✓").unwrap();
    let (mut new_transport, _) = Fake::new();
    let report = leselang_tauri_host::run_task(
        &mut new_transport,
        scope(),
        source,
        vec![("text".into(), "same input ✓".into())],
    )
    .unwrap();
    for field in ["result", "effects", "fuel_remaining"] {
        assert_eq!(
            report[field], legacy[field],
            "{field} must preserve the original execution"
        );
    }
    assert!(report.get("verified").is_none());
    assert_eq!(legacy["verified"], true);
}

#[test]
fn task_source_signature_and_argument_failures_precede_every_transport_request() {
    let cases = [
        ("fn main() = true".into(), vec![("unused".into(), "value".into())]),
        ("fn main(message: string) = message".into(), vec![]),
        ("fn main(message: string) = message".into(), vec![("text".into(), "value".into())]),
        ("fn main(message: string) = message".into(), vec![("message".into(), "a".into()), ("message".into(), "b".into())]),
        ("fn main(message: integer) = message".into(), vec![("message".into(), "1".into())]),
        ("fn main(first: string, second: string) = first".into(), vec![]),
        ("fn main(body: string) = body".into(), vec![("body".into(), "value".into())]),
        ("fn main(_lese_name: string) = _lese_name".into(), vec![("_lese_name".into(), "value".into())]),
        ("fn main(message: string) = message".into(), vec![("bad-name".into(), "value".into())]),
        ("fn main(message: string) = message".into(), vec![("message".into(), "x".repeat(4097))]),
        ("fn main(message: string) = message".into(), vec![("message".into(), "x\0y".into())]),
        ("fn main() = true\0".into(), vec![]),
        (" ".repeat(16 * 1024 + 1), vec![]),
        ("fn main() = missing()".into(), vec![]),
        ("fn unused() = gui.unknown()\nfn main() = true".into(), vec![]),
        (r#"fn main() = gui.find(name: "save")"#.into(), vec![]),
        (r#"fn main() = bind(object: gui.find(name: "save"), body: gui.observe(target: object))"#.into(), vec![]),
        ("fn main(object: object) = true".into(), vec![]),
    ];
    for (source, arguments) in cases {
        let (mut transport, model) = Fake::new();
        assert!(
            leselang_tauri_host::run_task(&mut transport, scope(), &source, arguments).is_err(),
            "{source}"
        );
        assert!(
            model.borrow().requests.is_empty(),
            "admission contacted the transport: {source}"
        );
        assert_eq!(model.borrow().writes, 0);
    }
}

#[test]
fn admitted_tasks_preserve_committed_mutation_when_cancelled_or_given_a_late_reply() {
    let source =
        include_str!("../../../consumer/examples/catalog_agent/fixtures/form-semantics.lese");
    for corruption in [
        Corrupt::Uncertain,
        Corrupt::CloseBeforeReply,
        Corrupt::Serial,
    ] {
        let (mut transport, model) = Fake::new();
        model.borrow_mut().corrupt_write = Some(corruption);
        let task = leselang_tauri_host::prepare_task(
            source,
            vec![("text".into(), "already committed ✓".into())],
        )
        .unwrap();
        assert!(leselang_tauri_host::run_prepared_task(&mut transport, scope(), task).is_err());
        let state = model.borrow();
        assert_eq!(state.writes, 1);
        assert_eq!(state.values[2]["value"], "already committed ✓");
        assert_eq!(
            state.values[0]["clicks"], 0,
            "save must not follow the failed text acknowledgement"
        );
        assert_eq!(
            state
                .requests
                .iter()
                .filter(|request| request["operation"] == "gui.set_text")
                .count(),
            1
        );
    }
    let (mut transport, model) = Fake::new();
    let task = leselang_tauri_host::prepare_task("fn main() = true", vec![]).unwrap();
    model.borrow_mut().live = false;
    assert!(leselang_tauri_host::run_prepared_task(&mut transport, scope(), task).is_err());
    assert!(model.borrow().requests.is_empty());
}

#[test]
fn semantic_provider_sources_cross_the_real_vm_with_renamed_and_no_input_tasks() {
    for (source, arguments, result, effects, writes) in [
        (
            include_str!(
                "../../../consumer/examples/catalog_agent/fixtures/tauri-save-message.lese"
            ),
            vec![("message".into(), "语义入口 \"✓\" \\\nsecond line".into())],
            json!({"type":"string","value":"saved"}),
            9,
            2,
        ),
        (
            include_str!(
                "../../../consumer/examples/catalog_agent/fixtures/tauri-inspect-controls.lese"
            ),
            vec![],
            json!({"type":"integer","value":3}),
            3,
            0,
        ),
        (
            include_str!(
                "../../../consumer/examples/catalog_agent/fixtures/tauri-check-controls.lese"
            ),
            vec![],
            json!({"type":"none","value":null}),
            3,
            0,
        ),
    ] {
        let (mut transport, model) = Fake::new();
        let text = arguments
            .first()
            .map(|(_, value): &(String, String)| value.clone());
        let report =
            leselang_tauri_host::run_task(&mut transport, scope(), source, arguments).unwrap();
        assert_eq!(report["result"], result);
        assert_eq!(report["effects"], effects);
        assert_eq!(model.borrow().writes, writes);
        if let Some(text) = text {
            assert_eq!(model.borrow().values[0]["value"], text);
            assert_eq!(model.borrow().values[2]["value"], text);
        } else {
            assert_eq!(report["native_before"], report["native_after"]);
        }
    }
}

#[test]
fn an_empty_registry_does_not_hide_cancellation_after_a_pure_task() {
    struct CancelAfterBootstrap {
        checks: std::cell::Cell<usize>,
        requests: usize,
    }
    impl Transport for CancelAfterBootstrap {
        fn is_live(&self) -> bool {
            let checks = self.checks.get() + 1;
            self.checks.set(checks);
            checks <= 2 // Bootstrap admission and its returned reply are live.
        }
        fn exchange(&mut self, request: Value) -> Result<Value, String> {
            self.requests += 1;
            assert_eq!(request["operation"], "gui.list");
            Ok(json!({
                "transport_version":1, "window":request["window"], "epoch":request["epoch"],
                "session":request["session"], "request_id":request["request_id"],
                "status":"ok", "value":[], "error":null,
            }))
        }
    }
    let mut transport = CancelAfterBootstrap {
        checks: std::cell::Cell::new(0),
        requests: 0,
    };
    assert!(
        leselang_tauri_host::run_task(&mut transport, scope(), "fn main() = true", vec![]).is_err()
    );
    assert_eq!(transport.requests, 1);
}
