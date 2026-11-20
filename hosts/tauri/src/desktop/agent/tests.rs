use super::*;
use leselang_tauri_host::Transport;

fn channel() -> Channel<Value> {
    Channel::new(|_| Ok(()))
}

#[test]
fn planning_pins_shared_epoch_and_blocks_overlapping_execution_until_settled() {
    let mut registry = Registry::default();
    let model = registry.start_model("epoch", "request".into()).unwrap();
    assert_eq!(registry.epoch.as_deref(), Some("epoch"));
    assert!(
        registry
            .start_task(
                "other".into(),
                "session".into(),
                "fn main() { 1 }",
                channel()
            )
            .is_err()
    );
    assert!(
        registry
            .start_task(
                "epoch".into(),
                "session".into(),
                "fn main() { 1 }",
                channel()
            )
            .is_err()
    );
    assert!(registry.cancel_model("wrong", "request").is_err());
    registry.cancel_model("epoch", "request").unwrap();
    assert!(registry.models.busy());
    assert!(registry.models.finish(&model, Ok("late".into())).is_err());
    registry
        .start_task(
            "epoch".into(),
            "session".into(),
            "fn main() { 1 }",
            channel(),
        )
        .unwrap();
    assert!(registry.start_model("epoch", "another".into()).is_err());
}

#[test]
fn page_close_and_navigation_retire_planning_before_any_vm_session() {
    for navigate in [false, true] {
        let mut registry = Registry::default();
        registry.navigation_started();
        let model = registry.start_model("epoch", "request".into()).unwrap();
        if navigate {
            registry.navigation_started();
        } else {
            registry.close();
        }
        assert!(registry.retired);
        assert!(registry.models.finish(&model, Ok("late".into())).is_err());
        assert!(registry.start_model("epoch", "next".into()).is_err());
    }
}

#[test]
fn cancellation_can_pin_epoch_and_tombstone_before_completion_ipc_arrives() {
    let mut registry = Registry::default();
    registry.cancel_model("epoch", "request").unwrap();
    assert!(registry.start_model("epoch", "request".into()).is_err());
    assert!(
        registry
            .start_model("different", "request-2".into())
            .is_err()
    );
    let model = registry.start_model("epoch", "request-2".into()).unwrap();
    registry.cancel_model("epoch", "request").unwrap();
    assert_eq!(
        registry
            .models
            .finish(&model, Ok("current".into()))
            .unwrap(),
        "current"
    );
}

#[test]
fn raw_catalog_preserves_semantic_u64_tokens_and_the_legacy_object_response() {
    let raw = description_value(true, false).unwrap();
    let text = raw.as_str().unwrap();
    assert!(text.contains("9223372036854775807"));
    assert!(text.ends_with('\n'));
    let parsed: Value = serde_json::from_str(text).unwrap();
    assert_eq!(parsed, description_value(false, false).unwrap());
    assert_eq!(parsed["semantics"]["version"], 1);
}

#[test]
fn task_catalog_is_opt_in_and_matches_the_generic_report_contract() {
    let legacy = description_value(false, false).unwrap();
    let tasks = description_value(false, true).unwrap();
    assert_eq!(legacy["semantics"]["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(
        legacy["semantics"]["tasks"][0]["verification"]["kind"],
        "tauri_run"
    );
    let declared = tasks["semantics"]["tasks"].as_array().unwrap();
    assert_eq!(declared.len(), 4);
    for task in declared {
        assert_eq!(task["verification"]["kind"], "tauri_task_run");
        assert_eq!(task["verification"]["require_verified"], false);
    }
    let raw = description_value(true, true).unwrap();
    assert!(raw.as_str().unwrap().contains("9223372036854775807"));
    assert_eq!(
        serde_json::from_str::<Value>(raw.as_str().unwrap()).unwrap(),
        tasks
    );
}

#[test]
fn typed_command_admission_rejects_malformed_inputs_and_signatures() {
    assert!(prepare_task_input("fn main() = 3", json!({})).is_ok());
    assert!(
        prepare_task_input(
            "fn main(message: string) = message",
            json!({"message":"literal ✓"})
        )
        .is_ok()
    );
    for (source, arguments) in [
        ("fn main() = true", json!(null)),
        ("fn main() = true", json!([])),
        ("fn main() = true", json!({"text": "extra"})),
        ("fn main(message: string) = message", json!({})),
        (
            "fn main(message: string) = message",
            json!({"text":"wrong name"}),
        ),
        ("fn main(message: string) = message", json!({"message":4})),
        (
            "fn main(message: string) = message",
            json!({"message":"one", "extra":"two"}),
        ),
        (
            "fn main(message: string) = message",
            json!({"message":"界".repeat(1366)}),
        ),
        (
            "fn main(message: string) = message",
            json!({"message":"nul\0input"}),
        ),
        ("fn main() = gui.find(name: \"save\")", json!({})),
        ("fn main() = true\nfn unused() = unknown()", json!({})),
    ] {
        assert!(prepare_task_input(source, arguments).is_err(), "{source}");
    }
}

#[test]
fn generic_completion_preserves_exact_results_and_shared_cancellation() {
    let mut registry = Registry::default();
    let (first, endpoint) = registry
        .start_task("page-1".into(), "one".into(), "fn main() = 0", channel())
        .unwrap();
    let report = registry.finish(
        &first,
        Ok(json!({
            "kind":"tauri_task_run", "result":{"type":"integer", "value":u64::MAX}
        })),
    );
    assert_eq!(report["result"]["value"].as_u64(), Some(u64::MAX));
    assert!(
        serde_json::to_string(&report)
            .unwrap()
            .contains("18446744073709551615")
    );
    assert!(report.get("verified").is_none());
    assert!(!endpoint.is_live());

    let (second, second_endpoint) = registry
        .start_task("page-1".into(), "two".into(), "fn main() = none", channel())
        .unwrap();
    registry.cancel("page-1", "one").unwrap();
    assert!(registry.reply(reply("one")).is_err());
    assert!(second_endpoint.is_live());
    registry.cancel("page-1", "two").unwrap();
    assert!(
        registry
            .start_task("page-1".into(), "three".into(), "", channel())
            .is_err()
    );
    let report = registry.finish(&second, Ok(json!({"result":{"type":"none", "value":null}})));
    assert_eq!(report["kind"], "tauri_task_run");
    assert_eq!(report["status"], "cancelled");
    assert_eq!(report["possible_partial_effects"], true);
    assert!(report.get("result").is_none());
    assert!(!second_endpoint.is_live());

    let (third, _) = registry
        .start_task("page-1".into(), "three".into(), "", channel())
        .unwrap();
    let report = registry.finish(&third, Ok(json!({"oversized":"x".repeat(REPORT_LIMIT)})));
    assert_eq!(report["kind"], "tauri_task_run");
    assert_eq!(report["status"], "failed");
    assert!(report.get("result").is_none());
}
fn begin(registry: &mut Registry, id: &str) -> (Arc<Run>, Endpoint) {
    registry
        .start(
            "page-1".into(),
            id.into(),
            "fn main(text: string) = true",
            "task",
            channel(),
        )
        .unwrap()
}
fn reply(id: &str) -> Value {
    json!({"transport_version":1,"window":"main","epoch":"page-1","session":id,"request_id":1})
}

#[test]
fn page_scope_and_sequential_tasks_do_not_reuse_sessions() {
    let mut registry = Registry::default();
    let (first, first_endpoint) = begin(&mut registry, "one");
    assert!(
        registry
            .start("page-1".into(), "two".into(), "", "", channel())
            .is_err()
    );
    assert!(
        registry
            .start("foreign".into(), "two".into(), "", "", channel())
            .is_err()
    );
    assert_eq!(
        registry.finish(&first, Ok(json!({"verified":true})))["verified"],
        true
    );
    assert!(!first_endpoint.is_live());
    assert!(
        registry
            .start("page-1".into(), "one".into(), "", "", channel())
            .is_err()
    );
    let (second, second_endpoint) = begin(&mut registry, "two");
    registry.cancel("page-1", "one").unwrap();
    assert!(
        second_endpoint.is_live(),
        "old cancel cannot retire a newer task"
    );
    assert!(registry.reply(reply("one")).is_err());
    assert!(
        second_endpoint.is_live(),
        "old reply cannot retire a newer task"
    );
    registry.finish(&second, Ok(json!({"verified":true})));
}

#[test]
fn cancelled_worker_cannot_publish_success_or_admit_overlap() {
    let mut registry = Registry::default();
    let (run, mut endpoint) = begin(&mut registry, "one");
    registry.cancel("page-1", "one").unwrap();
    assert!(!endpoint.is_live());
    assert!(endpoint.exchange(reply("one")).is_err());
    assert!(
        registry
            .start("page-1".into(), "two".into(), "", "", channel())
            .is_err()
    );
    let report = registry.finish(&run, Ok(json!({"verified":true})));
    assert_eq!(report["status"], "cancelled");
    assert_eq!(report["possible_partial_effects"], true);
    assert!(report.get("verified").is_none());
    let _ = begin(&mut registry, "two");
}

#[test]
fn cancellation_wakes_a_worker_waiting_for_a_reply() {
    let mut registry = Registry::default();
    let (sent, observed) = sync_channel(1);
    let channel = Channel::new(move |_| {
        sent.try_send(()).unwrap();
        Ok(())
    });
    let (run, mut endpoint) = registry
        .start("page-1".into(), "one".into(), "", "", channel)
        .unwrap();
    let (done, completed) = sync_channel(1);
    std::thread::spawn(move || {
        let result = endpoint.exchange(reply("one"));
        done.send(result).unwrap();
    });
    observed
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    registry.cancel("page-1", "one").unwrap();
    assert!(
        completed
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap()
            .is_err()
    );
    assert_eq!(
        registry.finish(&run, Err("cancelled".into()))["status"],
        "cancelled"
    );
}

#[test]
fn pending_reply_scope_is_consumed_once_and_failure_retires_only_current_task() {
    let mut registry = Registry::default();
    let (run, endpoint) = begin(&mut registry, "one");
    let value = reply("one");
    *run.pending.lock().unwrap() = Correlation::from_value(&value);
    registry.reply(value.clone()).unwrap();
    assert!(registry.reply(value).is_err());
    assert!(!endpoint.is_live());
    let report = registry.finish(&run, Ok(json!({"verified":true})));
    assert_eq!(
        report["status"], "failed",
        "a late invalid reply cannot be masked by a successful worker"
    );
    assert!(begin(&mut registry, "two").1.is_live());
}

#[test]
fn page_reload_and_deadline_cannot_be_overridden_by_a_successful_worker() {
    let mut registry = Registry::default();
    registry.navigation_started();
    let (run, endpoint) = begin(&mut registry, "one");
    registry.navigation_started();
    assert!(!endpoint.is_live());
    assert_eq!(
        registry.finish(&run, Ok(json!({"verified":true})))["status"],
        "failed"
    );
    assert!(
        registry
            .start("page-2".into(), "two".into(), "", "", channel())
            .is_err()
    );
    let mut registry = Registry::default();
    let (run, _) = begin(&mut registry, "one");
    run.expired.store(true, Ordering::Release);
    assert_eq!(
        registry.finish(&run, Ok(json!({"verified":true})))["status"],
        "failed"
    );
}

#[test]
fn command_admission_bounds_source_text_scope_and_lifetime_history() {
    let mut registry = Registry::default();
    for (epoch, session, source, text) in [
        ("".to_owned(), "id".to_owned(), "".to_owned(), "".to_owned()),
        ("page-1".into(), "bad session".into(), "".into(), "".into()),
        (
            "page-1".into(),
            "id".into(),
            "x".repeat(16 * 1024 + 1),
            "".into(),
        ),
        ("page-1".into(), "id".into(), "".into(), "x".repeat(4097)),
    ] {
        assert!(
            registry
                .start(epoch, session, &source, &text, channel())
                .is_err()
        );
    }
    assert!(registry.epoch.is_none());
    for id in 0..MAX_PAGE_SESSIONS {
        let (run, _) = begin(&mut registry, &format!("task-{id}"));
        registry.finish(&run, Ok(json!({"verified":true})));
    }
    assert!(
        registry
            .start("page-1".into(), "overflow".into(), "", "", channel())
            .is_err()
    );
}

#[test]
fn agent_page_url_is_separate_from_legacy_and_remote_pages() {
    for allowed in [
        "tauri://localhost/agent.html",
        "http://tauri.localhost/agent.html",
    ] {
        assert!(local_asset(&allowed.parse().unwrap()));
    }
    for denied in [
        "tauri://localhost",
        "tauri://localhost/index.html",
        "tauri://localhost/agent.html?remote=1",
        "https://example.com/agent.html",
        "tauri://user@localhost/agent.html",
    ] {
        assert!(!local_asset(&denied.parse().unwrap()));
    }
}
