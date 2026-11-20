//! An application page can start bounded, sequential workflow sessions.
//! Tauri transports JSON; each blocking worker exclusively owns its LocalVm.
use super::model;
use super::wire::*;
use leselang_tauri_host::{AgentApplication, Scope};
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
    application: AgentApplication,
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
    application: AgentApplication,
    epoch: Option<String>,
    used: Vec<String>,
    active: Option<Arc<Run>>,
    page_started: bool,
    retired: bool,
    models: model::Registry,
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
        if self.application != AgentApplication::Form {
            return Err("the legacy form runner is unavailable in this application".into());
        }
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
        if self.active.is_some() || self.models.busy() {
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
            application: self.application,
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
                run.application,
                run.report_kind,
                "cancelled",
                "workflow cancelled; earlier effects may remain",
            )
        } else if run.expired.load(Ordering::Acquire) {
            failure(
                run.application,
                run.report_kind,
                "failed",
                "workflow execution deadline exceeded",
            )
        } else if self.retired || !current {
            failure(
                run.application,
                run.report_kind,
                "failed",
                "application page lifetime has been retired",
            )
        } else if !run.live.load(Ordering::Acquire) {
            failure(
                run.application,
                run.report_kind,
                "failed",
                "workflow bridge retired before final acceptance",
            )
        } else {
            match result {
                Ok(mut report) => {
                    if report
                        .get("profile")
                        .is_some_and(|profile| profile.as_str() != Some(run.application.profile()))
                    {
                        failure(
                            run.application,
                            run.report_kind,
                            "failed",
                            "worker report belongs to another application",
                        )
                    } else if let Some(fields) = report.as_object_mut() {
                        fields.insert("profile".into(), json!(run.application.profile()));
                        report
                    } else {
                        failure(
                            run.application,
                            run.report_kind,
                            "failed",
                            "worker report is not an object",
                        )
                    }
                }
                Err(error) => failure(run.application, run.report_kind, "failed", &error),
            }
        };
        // Retirement precedes admission of a later workflow and IPC completion.
        run.retire();
        if current {
            self.active.take();
        }
        if bounded(&report, REPORT_LIMIT).is_err() {
            failure(
                run.application,
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
        self.models.close();
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

    fn model_epoch(&mut self, epoch: &str, request_id: &str) -> Result<(), String> {
        if !valid_nonce(epoch) || !valid_nonce(request_id) {
            return Err("model_request_invalid".into());
        }
        if self.retired || self.epoch.as_ref().is_some_and(|current| current != epoch) {
            return Err("model_page_retired".into());
        }
        self.epoch = Some(epoch.to_owned());
        Ok(())
    }

    fn start_model(&mut self, epoch: &str, request_id: String) -> Result<Arc<model::Run>, String> {
        self.model_epoch(epoch, &request_id)?;
        if self.active.is_some() {
            return Err("model_request_pending".into());
        }
        self.models.begin(request_id)
    }

    fn cancel_model(&mut self, epoch: &str, request_id: &str) -> Result<(), String> {
        self.model_epoch(epoch, request_id)?;
        self.models.cancel(request_id)
    }
}

#[derive(Default)]
struct Shared(Mutex<Registry>);

fn failure(application: AgentApplication, kind: &str, status: &str, error: &str) -> Value {
    json!({"schema_version":1,"kind":kind,"profile":application.profile(),"status":status,"error":error,
        "possible_partial_effects":true})
}

fn local_asset_for(application: AgentApplication, url: &tauri::Url) -> bool {
    ((url.scheme() == "tauri" && url.host_str() == Some("localhost"))
        || (url.scheme() == "http" && url.host_str() == Some("tauri.localhost")))
        && url.path().strip_prefix('/') == Some(application.asset())
        && url.query().is_none()
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
}

#[cfg(test)]
fn local_asset(url: &tauri::Url) -> bool {
    local_asset_for(AgentApplication::Form, url)
}

fn caller(webview: &WebviewWindow) -> Result<AgentApplication, String> {
    let application = *webview.state::<AgentApplication>();
    if webview.label() != "main"
        || !webview
            .url()
            .is_ok_and(|url| local_asset_for(application, &url))
    {
        return Err("agent commands require the bundled main agent page".into());
    }
    Ok(application)
}

#[tauri::command]
fn agent_model_info(
    webview: WebviewWindow,
    shared: State<'_, Shared>,
    settings: State<'_, model::Settings>,
) -> Result<Value, String> {
    caller(&webview)?;
    if shared
        .0
        .lock()
        .map_err(|_| "model_transport_failed")?
        .retired
    {
        return Err("model_page_retired".into());
    }
    settings.info()
}

#[tauri::command]
async fn agent_model_complete(
    webview: WebviewWindow,
    shared: State<'_, Shared>,
    settings: State<'_, model::Settings>,
    epoch: String,
    request_id: String,
    request: model::Request,
) -> Result<String, String> {
    caller(&webview)?;
    request.validate()?;
    let config = settings.config()?;
    let run = shared
        .0
        .lock()
        .map_err(|_| "model_transport_failed")?
        .start_model(&epoch, request_id)?;
    let worker_run = run.clone();
    let result =
        tauri::async_runtime::spawn_blocking(move || model::complete(config, request, worker_run))
            .await
            .unwrap_or_else(|_| Err("model_transport_failed".into()));
    shared
        .0
        .lock()
        .map_err(|_| "model_transport_failed")?
        .models
        .finish(&run, result)
}

#[tauri::command]
fn agent_model_cancel(
    webview: WebviewWindow,
    shared: State<'_, Shared>,
    epoch: String,
    request_id: String,
) -> Result<(), String> {
    caller(&webview)?;
    shared
        .0
        .lock()
        .map_err(|_| "model_transport_failed")?
        .cancel_model(&epoch, &request_id)
}

#[tauri::command]
fn agent_describe(
    webview: WebviewWindow,
    shared: State<'_, Shared>,
    raw: Option<bool>,
    task_catalog: Option<bool>,
    observed_results: Option<bool>,
) -> Result<Value, String> {
    let application = caller(&webview)?;
    if shared.0.lock().map_err(|_| "page lock failed")?.retired {
        return Err("application page lifetime has been retired".into());
    }
    description_value_for(
        application,
        raw.unwrap_or(false),
        task_catalog.unwrap_or(false),
        observed_results.unwrap_or(false),
    )
}

#[cfg(test)]
fn description_value(raw: bool, task_catalog: bool) -> Result<Value, String> {
    description_value_with_observed(raw, task_catalog, false)
}

#[cfg(test)]
fn description_value_with_observed(
    raw: bool,
    task_catalog: bool,
    observed_results: bool,
) -> Result<Value, String> {
    description_value_for(AgentApplication::Form, raw, task_catalog, observed_results)
}

fn description_value_for(
    application: AgentApplication,
    raw: bool,
    task_catalog: bool,
    observed_results: bool,
) -> Result<Value, String> {
    if observed_results && !task_catalog {
        return Err("observed results require the explicit task catalog".into());
    }
    let description = if application == AgentApplication::Article {
        if !task_catalog || !observed_results {
            return Err("the article requires its explicit observed task catalog".into());
        }
        leselang_tauri_host::describe_agent_article()?
    } else if application == AgentApplication::TaskBoard {
        if !task_catalog || !observed_results {
            return Err("the task board requires its explicit observed task catalog".into());
        }
        leselang_tauri_host::describe_agent_board()?
    } else if observed_results {
        leselang_tauri_host::describe_agent_observed()?
    } else if task_catalog {
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
    leselang_tauri_host::prepare_task(source, task_arguments(arguments, 1)?)
}

fn prepare_named_task_input(
    source: &str,
    arguments: Value,
) -> Result<leselang_tauri_host::PreparedTask, String> {
    leselang_tauri_host::prepare_named_task(source, task_arguments(arguments, 8)?)
}

fn prepare_legacy_task_input(
    application: AgentApplication,
    source: &str,
    arguments: Value,
) -> Result<leselang_tauri_host::PreparedTask, String> {
    if application == AgentApplication::Article {
        return Err("the article requires the explicit named-string task entry".into());
    }
    prepare_task_input(source, arguments)
}

fn task_arguments(arguments: Value, maximum: usize) -> Result<Vec<(String, String)>, String> {
    let Value::Object(arguments) = arguments else {
        return Err("workflow arguments must be a named string map".into());
    };
    if arguments.len() > maximum {
        return Err("workflow argument count exceeds the selected entry limit".into());
    }
    arguments
        .into_iter()
        .map(|(name, value)| {
            let Value::String(value) = value else {
                return Err("workflow argument must be a string".to_owned());
            };
            Ok((name, value))
        })
        .collect::<Result<Vec<_>, String>>()
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
    let application = caller(&webview)?;
    // Compile and bind the complete task before registry admission, transport
    // creation or application reads. Rejected work consumes no page session.
    let task = prepare_legacy_task_input(application, &source, arguments)?;
    start_prepared_task(
        &shared,
        on_request,
        epoch,
        session,
        source,
        task,
        application,
    )
    .await
}

/// Explicit v3 named-string entry; the legacy command keeps its one-input limit.
#[tauri::command]
async fn agent_start_named_task(
    webview: WebviewWindow,
    shared: State<'_, Shared>,
    on_request: Channel<Value>,
    epoch: String,
    session: String,
    source: String,
    arguments: Value,
) -> Result<String, String> {
    let application = caller(&webview)?;
    let task = prepare_named_task_input(&source, arguments)?;
    start_prepared_task(
        &shared,
        on_request,
        epoch,
        session,
        source,
        task,
        application,
    )
    .await
}

async fn start_prepared_task(
    shared: &Shared,
    on_request: Channel<Value>,
    epoch: String,
    session: String,
    source: String,
    task: leselang_tauri_host::PreparedTask,
    application: AgentApplication,
) -> Result<String, String> {
    let (run, mut endpoint) = shared
        .0
        .lock()
        .map_err(|_| "page lock failed")?
        .start_task(epoch, session, &source, on_request)?;
    let report = complete(shared, run, move || {
        let scope = endpoint.scope.clone();
        leselang_tauri_host::run_prepared_task_for_application(
            &mut endpoint,
            scope,
            task,
            application,
        )
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
    launch_application(AgentApplication::Form)
}

pub fn launch_board() -> Result<(), String> {
    launch_application(AgentApplication::TaskBoard)
}

pub fn launch_article() -> Result<(), String> {
    launch_application(AgentApplication::Article)
}

fn launch_application(application: AgentApplication) -> Result<(), String> {
    let app = tauri::Builder::default()
        .manage(application)
        .manage(Shared(Mutex::new(Registry {
            application,
            ..Registry::default()
        })))
        .manage(model::Settings::from_env())
        .invoke_handler(tauri::generate_handler![
            agent_describe,
            agent_start,
            agent_start_task,
            agent_start_named_task,
            agent_reply,
            agent_cancel,
            agent_closed,
            agent_model_info,
            agent_model_complete,
            agent_model_cancel
        ])
        .setup(move |app| {
            tauri::WebviewWindowBuilder::new(
                app,
                "main",
                WebviewUrl::App(application.asset().into()),
            )
            .title(application.title())
            .inner_size(1000.0, 850.0)
            .on_navigation(move |url| local_asset_for(application, url))
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
mod tests;
