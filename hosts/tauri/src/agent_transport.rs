//! An application page can start bounded, sequential workflow sessions.
//! Tauri transports JSON; each blocking worker exclusively owns its LocalVm.
use crate::transport_core::*;
use leselang_tauri_host::Scope;
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::{RecvTimeoutError, SyncSender, sync_channel},
};
use std::time::Instant;
use tauri::{Manager, State, WebviewUrl, WebviewWindow, ipc::Channel};

const MAX_PAGE_SESSIONS: usize = 64;

struct Run {
    scope: Scope,
    report_kind: &'static str,
    live: Arc<AtomicBool>,
    pending: Arc<Mutex<Option<Correlation>>>,
    replies: Mutex<Option<SyncSender<Value>>>,
    cancelled: AtomicBool,
    expired: AtomicBool,
}

impl Run {
    fn retire(&self) {
        self.live.store(false, Ordering::Release);
        if let Ok(mut sender) = self.replies.lock() {
            sender.take();
        }
        if let Ok(mut pending) = self.pending.lock() {
            pending.take();
        }
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.retire();
    }

    fn matches(&self, epoch: &str, session: &str) -> bool {
        self.scope.epoch == epoch && self.scope.session == session
    }

    fn reply(&self, reply: Value) -> Result<(), String> {
        if !self.live.load(Ordering::Acquire) {
            return Err("workflow bridge is no longer live".into());
        }
        let result: Result<(), String> = (|| {
            bounded(&reply, WIRE_LIMIT)?;
            accept_pending(&self.pending, &reply)?;
            self.replies
                .lock()
                .map_err(|_| "workflow reply lock failed")?
                .as_ref()
                .ok_or("workflow has no pending recipient")?
                .try_send(reply)
                .map_err(|_| "duplicate or unavailable workflow reply".to_owned())
        })();
        if result.is_err() {
            self.retire();
        }
        result
    }
}

#[derive(Default)]
struct Registry {
    epoch: Option<String>,
    used: Vec<String>,
    active: Option<Arc<Run>>,
    page_started: bool,
    retired: bool,
}

impl Registry {
    fn start(
        &mut self,
        epoch: String,
        session: String,
        source: &str,
        text: &str,
        outgoing: Channel<Value>,
    ) -> Result<(Arc<Run>, Endpoint), String> {
        if text.len() > 4096 || text.contains('\0') {
            return Err("workflow text exceeds 4096 UTF-8 bytes or contains NUL".into());
        }
        self.admit(epoch, session, source, outgoing, "tauri_run")
    }

    fn start_task(
        &mut self,
        epoch: String,
        session: String,
        source: &str,
        outgoing: Channel<Value>,
    ) -> Result<(Arc<Run>, Endpoint), String> {
        self.admit(epoch, session, source, outgoing, "tauri_task_run")
    }

    fn admit(
        &mut self,
        epoch: String,
        session: String,
        source: &str,
        outgoing: Channel<Value>,
        report_kind: &'static str,
    ) -> Result<(Arc<Run>, Endpoint), String> {
        if !valid_nonce(&epoch) || !valid_nonce(&session) {
            return Err("invalid workflow epoch or session".into());
        }
        if source.len() > 16 * 1024 || source.contains('\0') {
            return Err("workflow source exceeds 16 KiB or contains NUL".into());
        }
        if self.retired || self.epoch.as_ref().is_some_and(|value| value != &epoch) {
            return Err("application page lifetime has been retired".into());
        }
        if self.active.is_some() {
            return Err("a workflow is already active; wait for its final outcome".into());
        }
        if self.used.contains(&session) || self.used.len() == MAX_PAGE_SESSIONS {
            return Err("workflow session was used or the page reached its 64-task limit".into());
        }
        let (sender, incoming) = sync_channel(1);
        let scope = Scope {
            window: "main".into(),
            epoch: epoch.clone(),
            session: session.clone(),
        };
        let run = Arc::new(Run {
            scope: scope.clone(),
            report_kind,
            live: Arc::new(AtomicBool::new(true)),
            pending: Arc::new(Mutex::new(None)),
            replies: Mutex::new(Some(sender)),
            cancelled: AtomicBool::new(false),
            expired: AtomicBool::new(false),
        });
        let endpoint = Endpoint {
            scope,
            outgoing,
            incoming,
            live: run.live.clone(),
            pending: run.pending.clone(),
            started: Instant::now(),
        };
        self.epoch = Some(epoch);
        self.used.push(session);
        self.active = Some(run.clone());
        Ok((run, endpoint))
    }

    fn cancel(&self, epoch: &str, session: &str) -> Result<(), String> {
        if self.epoch.as_deref() != Some(epoch) || !self.used.iter().any(|used| used == session) {
            return Err("cancel does not belong to this page and an admitted task".into());
        }
        if let Some(run) = &self.active
            && run.matches(epoch, session)
        {
            run.cancel();
        }
        // A duplicate or late cancel for an old admitted session is harmless.
        Ok(())
    }

    fn reply(&self, reply: Value) -> Result<(), String> {
        let run = self.active.as_ref().ok_or("no active workflow recipient")?;
        if reply["window"] != "main"
            || reply["epoch"].as_str() != Some(run.scope.epoch.as_str())
            || reply["session"].as_str() != Some(run.scope.session.as_str())
        {
            return Err("reply belongs to another workflow; current task unchanged".into());
        }
        run.reply(reply)
    }

    fn finish(&mut self, run: &Arc<Run>, result: Result<Value, String>) -> Value {
        let current = self
            .active
            .as_ref()
            .is_some_and(|active| Arc::ptr_eq(active, run));
        let report = if run.cancelled.load(Ordering::Acquire) {
            failure(
                run.report_kind,
                "cancelled",
                "workflow cancelled; earlier effects may remain",
            )
        } else if run.expired.load(Ordering::Acquire) {
            failure(
                run.report_kind,
                "failed",
                "workflow execution deadline exceeded",
            )
        } else if self.retired || !current {
            failure(
                run.report_kind,
                "failed",
                "application page lifetime has been retired",
            )
        } else if !run.live.load(Ordering::Acquire) {
            failure(
                run.report_kind,
                "failed",
                "workflow bridge retired before final acceptance",
            )
        } else {
            match result {
                Ok(report) => report,
                Err(error) => failure(run.report_kind, "failed", &error),
            }
        };
        // Retirement precedes admission of a later workflow and IPC completion.
        run.retire();
        if current {
            self.active.take();
        }
        if bounded(&report, REPORT_LIMIT).is_err() {
            failure(
                run.report_kind,
                "failed",
                "workflow report exceeded 128 KiB",
            )
        } else {
            report
        }
    }

    fn close(&mut self) {
        self.retired = true;
        if let Some(run) = &self.active {
            run.retire();
        }
    }

    fn navigation_started(&mut self) {
        if self.page_started {
            self.close();
        }
        self.page_started = true;
    }
}

#[derive(Default)]
struct Shared(Mutex<Registry>);

fn failure(kind: &str, status: &str, error: &str) -> Value {
    json!({"schema_version":1,"kind":kind,"status":status,"error":error,
        "possible_partial_effects":true})
}

fn local_asset(url: &tauri::Url) -> bool {
    ((url.scheme() == "tauri" && url.host_str() == Some("localhost"))
        || (url.scheme() == "http" && url.host_str() == Some("tauri.localhost")))
        && url.path() == "/agent.html"
        && url.query().is_none()
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
}

fn caller(webview: &WebviewWindow) -> Result<(), String> {
    if webview.label() != "main" || !webview.url().is_ok_and(|url| local_asset(&url)) {
        return Err("agent commands require the bundled main agent page".into());
    }
    Ok(())
}

#[tauri::command]
fn agent_describe(
    webview: WebviewWindow,
    shared: State<'_, Shared>,
    raw: Option<bool>,
    task_catalog: Option<bool>,
) -> Result<Value, String> {
    caller(&webview)?;
    if shared.0.lock().map_err(|_| "page lock failed")?.retired {
        return Err("application page lifetime has been retired".into());
    }
    description_value(raw.unwrap_or(false), task_catalog.unwrap_or(false))
}

fn description_value(raw: bool, task_catalog: bool) -> Result<Value, String> {
    let description = if task_catalog {
        leselang_tauri_host::describe_agent()?
    } else {
        leselang_tauri_host::describe()?
    };
    if raw {
        // Preserve u64 literal tokens across IPC. JS Number cannot represent
        // every integer in the host's semantic verification contract exactly.
        Ok(Value::String(description))
    } else {
        // Existing callers retain their object response.
        serde_json::from_str(&description).map_err(|error| error.to_string())
    }
}

#[tauri::command]
async fn agent_start(
    webview: WebviewWindow,
    shared: State<'_, Shared>,
    on_request: Channel<Value>,
    epoch: String,
    session: String,
    source: String,
    text: String,
) -> Result<Value, String> {
    caller(&webview)?;
    let (run, mut endpoint) = shared
        .0
        .lock()
        .map_err(|_| "page lock failed")?
        .start(epoch, session, &source, &text, on_request)?;
    complete(&shared, run, move || {
        let scope = endpoint.scope.clone();
        leselang_tauri_host::run(&mut endpoint, scope, &source, &text)
    })
    .await
}

fn prepare_task_input(
    source: &str,
    arguments: Value,
) -> Result<leselang_tauri_host::PreparedTask, String> {
    let Value::Object(arguments) = arguments else {
        return Err("workflow arguments must be a named string map".into());
    };
    if arguments.len() > 1 {
        return Err("workflow accepts zero or one named string input".into());
    }
    let arguments = arguments
        .into_iter()
        .map(|(name, value)| {
            let Value::String(value) = value else {
                return Err("workflow argument must be a string".to_owned());
            };
            Ok((name, value))
        })
        .collect::<Result<Vec<_>, String>>()?;
    leselang_tauri_host::prepare_task(source, arguments)
}

/// Additive entry: exact named arguments and all scalar results, with a raw
/// JSON response so a u64 never loses precision in the JavaScript IPC decoder.
#[tauri::command]
async fn agent_start_task(
    webview: WebviewWindow,
    shared: State<'_, Shared>,
    on_request: Channel<Value>,
    epoch: String,
    session: String,
    source: String,
    arguments: Value,
) -> Result<String, String> {
    caller(&webview)?;
    // Compile and bind the complete task before registry admission, transport
    // creation or application reads. Rejected work consumes no page session.
    let task = prepare_task_input(&source, arguments)?;
    let (run, mut endpoint) = shared
        .0
        .lock()
        .map_err(|_| "page lock failed")?
        .start_task(epoch, session, &source, on_request)?;
    let report = complete(&shared, run, move || {
        let scope = endpoint.scope.clone();
        leselang_tauri_host::run_prepared_task(&mut endpoint, scope, task)
    })
    .await?;
    serde_json::to_string(&report).map_err(|error| error.to_string())
}

async fn complete(
    shared: &Shared,
    run: Arc<Run>,
    work: impl FnOnce() -> Result<Value, String> + Send + 'static,
) -> Result<Value, String> {
    let (done, completion) = sync_channel(1);
    let deadline_run = run.clone();
    std::thread::spawn(move || {
        if matches!(
            completion.recv_timeout(EXECUTION_TIMEOUT),
            Err(RecvTimeoutError::Timeout)
        ) {
            deadline_run.expired.store(true, Ordering::Release);
            deadline_run.retire();
        }
    });
    let result = tauri::async_runtime::spawn_blocking(move || {
        let result = work();
        let _ = done.send(());
        result
    })
    .await
    .unwrap_or_else(|error| Err(format!("workflow worker failed: {error}")));
    let mut registry = match shared.0.lock() {
        Ok(registry) => registry,
        Err(_) => {
            run.retire();
            return Err("page lock failed after workflow execution".into());
        }
    };
    Ok(registry.finish(&run, result))
}

#[tauri::command]
fn agent_reply(
    webview: WebviewWindow,
    shared: State<'_, Shared>,
    reply: Value,
) -> Result<(), String> {
    caller(&webview)?;
    shared
        .0
        .lock()
        .map_err(|_| "page lock failed")?
        .reply(reply)
}

#[tauri::command]
fn agent_cancel(
    webview: WebviewWindow,
    shared: State<'_, Shared>,
    epoch: String,
    session: String,
) -> Result<(), String> {
    caller(&webview)?;
    shared
        .0
        .lock()
        .map_err(|_| "page lock failed")?
        .cancel(&epoch, &session)
}

#[tauri::command]
fn agent_closed(
    webview: WebviewWindow,
    shared: State<'_, Shared>,
    epoch: String,
) -> Result<(), String> {
    caller(&webview)?;
    let mut registry = shared.0.lock().map_err(|_| "page lock failed")?;
    if !valid_nonce(&epoch)
        || registry
            .epoch
            .as_deref()
            .is_some_and(|current| current != epoch)
    {
        return Err("close belongs to another page epoch".into());
    }
    registry.close();
    Ok(())
}

pub fn launch() -> Result<(), String> {
    let app = tauri::Builder::default()
        .manage(Shared::default())
        .invoke_handler(tauri::generate_handler![
            agent_describe,
            agent_start,
            agent_start_task,
            agent_reply,
            agent_cancel,
            agent_closed
        ])
        .setup(|app| {
            tauri::WebviewWindowBuilder::new(app, "main", WebviewUrl::App("agent.html".into()))
                .title("LeseLang — Agent workflow")
                .inner_size(1000.0, 850.0)
                .on_navigation(local_asset)
                .on_page_load(|webview, payload| {
                    if payload.event() == tauri::webview::PageLoadEvent::Started
                        && let Ok(mut registry) = webview.state::<Shared>().0.lock()
                    {
                        registry.navigation_started();
                    }
                })
                .build()?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main"
                && matches!(event, tauri::WindowEvent::CloseRequested { .. })
                && let Ok(mut registry) = window.state::<Shared>().0.lock()
            {
                registry.close();
            }
        })
        .build(context())
        .map_err(|error| format!("cannot start agent application: {error}"))?;
    let exit = app.run_return(|app, event| {
        if matches!(event, tauri::RunEvent::ExitRequested { .. })
            && let Ok(mut registry) = app.state::<Shared>().0.lock()
        {
            registry.close();
        }
    });
    if exit == 0 {
        Ok(())
    } else {
        Err("agent application runtime failed".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use leselang_tauri_host::Transport;

    fn channel() -> Channel<Value> {
        Channel::new(|_| Ok(()))
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
}
