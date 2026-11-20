//! Tauri owns delivery and the event loop; one dedicated thread owns the VM.
//! Nothing here serializes a LocalVm, ObjectRef, permission grant or receipt.
use super::wire::*;
use leselang_tauri_host::Scope;
#[cfg(test)]
use leselang_tauri_host::Transport;
use serde_json::{Value, json};
use std::{
    io::Write,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI32, Ordering},
        mpsc::{SyncSender, sync_channel},
    },
    time::{Duration, Instant},
};
use tauri::{Manager, State, WebviewUrl, WebviewWindow, ipc::Channel};

struct Task {
    source: String,
    text: String,
    hold: bool,
    report_json: bool,
}

struct Shared {
    claimed: AtomicBool,
    retired: AtomicBool,
    finished: AtomicBool,
    exit_code: Arc<AtomicI32>,
    live: Arc<AtomicBool>,
    replies: Mutex<Option<SyncSender<Value>>>,
    pending: Arc<Mutex<Option<Correlation>>>,
    task: Mutex<Option<Task>>,
}

impl Shared {
    fn retire(&self) {
        self.retired.store(true, Ordering::Release);
        if let Ok(mut replies) = self.replies.lock() {
            self.live.store(false, Ordering::Release);
            replies.take();
        } else {
            self.live.store(false, Ordering::Release);
        }
        if let Ok(mut pending) = self.pending.lock() {
            pending.take();
        }
    }
}

fn local_asset(url: &tauri::Url) -> bool {
    ((url.scheme() == "tauri" && url.host_str() == Some("localhost"))
        || (url.scheme() == "http" && url.host_str() == Some("tauri.localhost")))
        && matches!(url.path(), "" | "/" | "/index.html")
        && url.query().is_none()
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
}

fn caller(webview: &WebviewWindow) -> Result<(), String> {
    if webview.label() != "main" || !webview.url().is_ok_and(|url| local_asset(&url)) {
        return Err("bridge requires the bundled main WebView".into());
    }
    Ok(())
}

#[tauri::command]
fn bridge_ready(
    webview: WebviewWindow,
    shared: State<'_, Shared>,
    on_request: Channel<Value>,
    epoch: String,
    session: String,
) -> Result<(), String> {
    caller(&webview)?;
    if !valid_nonce(&epoch) || !valid_nonce(&session) {
        return Err("invalid bridge epoch or session label".into());
    }
    if shared
        .claimed
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("this host accepts only one bridge lifetime".into());
    }
    let task = shared
        .task
        .lock()
        .map_err(|_| "bridge task lock failed")?
        .take()
        .ok_or("bridge task is no longer available")?;
    let (sender, receiver) = sync_channel(1);
    {
        let mut replies = shared
            .replies
            .lock()
            .map_err(|_| "bridge reply lock failed")?;
        if shared.retired.load(Ordering::Acquire) {
            return Err("bridge lifetime has been retired".into());
        }
        *replies = Some(sender);
        shared.live.store(true, Ordering::Release);
    }
    let endpoint = Endpoint {
        scope: Scope {
            window: "main".into(),
            epoch,
            session,
        },
        outgoing: on_request,
        incoming: receiver,
        live: shared.live.clone(),
        pending: shared.pending.clone(),
        started: Instant::now(),
    };
    let app = webview.app_handle().clone();
    let deadline_app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(EXECUTION_TIMEOUT);
        let shared = deadline_app.state::<Shared>();
        if !shared.finished.load(Ordering::Acquire) {
            shared.retire();
            eprintln!("Tauri host: execution deadline exceeded; outcome may be partial; no retry");
            deadline_app.exit(1);
        }
    });
    std::thread::spawn(move || {
        let mut endpoint = endpoint;
        let scope = endpoint.scope.clone();
        let (report, mut exit_code) = match leselang_tauri_host::run(
            &mut endpoint,
            scope,
            &task.source,
            &task.text,
        ) {
            Ok(report) => {
                let exit_code = if let Err(error) = output(&report, task.report_json) {
                    eprintln!("Tauri host: {error}");
                    1
                } else if report["verified"].as_bool() == Some(true) {
                    0
                } else {
                    1
                };
                (report, exit_code)
            }
            Err(error) => {
                eprintln!("Tauri host: {error}; outcome may be partial; no retry");
                (
                    json!({"schema_version":1,"kind":"tauri_run","status":"failed", "error":error,"possible_partial_effects":true}),
                    1,
                )
            }
        };
        // The status message is display-only. It cannot create a VM reply.
        if endpoint
            .outgoing
            .send(json!({"kind":"finished","report":report}))
            .is_err()
            && task.hold
        {
            exit_code = 1;
        }
        let shared = app.state::<Shared>();
        shared.exit_code.store(exit_code, Ordering::Release);
        shared.finished.store(true, Ordering::Release);
        shared.retire();
        if !task.hold {
            app.exit(exit_code);
        }
    });
    Ok(())
}

#[tauri::command]
fn bridge_reply(
    webview: WebviewWindow,
    shared: State<'_, Shared>,
    reply: Value,
) -> Result<(), String> {
    caller(&webview)?;
    if !shared.live.load(Ordering::Acquire) {
        return Err("WebView bridge is no longer live".into());
    }
    if let Err(error) = bounded(&reply, WIRE_LIMIT) {
        shared.retire();
        return Err(error);
    }
    if let Err(error) = accept_pending(&shared.pending, &reply) {
        shared.retire();
        return Err(error);
    }
    let delivery = shared
        .replies
        .lock()
        .map_err(|_| "bridge reply lock failed")?
        .as_ref()
        .ok_or("no pending bridge recipient")?
        .try_send(reply);
    if delivery.is_err() {
        shared.retire();
        return Err("duplicate or unavailable bridge reply; bridge retired".into());
    }
    Ok(())
}

#[tauri::command]
fn bridge_closed(webview: WebviewWindow, shared: State<'_, Shared>) -> Result<(), String> {
    caller(&webview)?;
    shared.retire();
    Ok(())
}

fn output(report: &Value, report_json: bool) -> Result<(), String> {
    let mut bytes = if report_json {
        serde_json::to_vec(report).map_err(|_| "cannot encode run report")?
    } else {
        serde_json::to_vec_pretty(report).map_err(|_| "cannot encode run report")?
    };
    bytes.push(b'\n');
    if bytes.len() > REPORT_LIMIT {
        return Err("run report exceeds 128 KiB".into());
    }
    let mut output = std::io::stdout().lock();
    output
        .write_all(&bytes)
        .and_then(|()| output.flush())
        .map_err(|error| format!("cannot write report: {error}; output may be partial"))
}

pub fn launch(source: String, text: String, hold: bool, report_json: bool) -> Result<(), String> {
    let exit_code = Arc::new(AtomicI32::new(1));
    let app = tauri::Builder::default()
        .manage(Shared {
            claimed: AtomicBool::new(false),
            retired: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            exit_code: exit_code.clone(),
            live: Arc::new(AtomicBool::new(false)),
            replies: Mutex::new(None),
            pending: Arc::new(Mutex::new(None)),
            task: Mutex::new(Some(Task {
                source,
                text,
                hold,
                report_json,
            })),
        })
        .invoke_handler(tauri::generate_handler![
            bridge_ready,
            bridge_reply,
            bridge_closed
        ])
        .setup(|app| {
            tauri::WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("LeseLang — TypeScript controls")
                .inner_size(720.0, 700.0)
                .on_navigation(local_asset)
                .on_page_load(|webview, payload| {
                    let shared = webview.state::<Shared>();
                    if payload.event() == tauri::webview::PageLoadEvent::Started
                        && shared.claimed.load(Ordering::Acquire)
                    {
                        shared.retire();
                    }
                })
                .build()?;
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(15));
                let shared = handle.state::<Shared>();
                if !shared.claimed.load(Ordering::Acquire) {
                    shared.retire();
                    eprintln!("Tauri host: frontend bridge did not become ready before deadline");
                    handle.exit(1);
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let shared = window.state::<Shared>();
                shared.retire();
                api.prevent_close();
                let exit_code = if shared.finished.load(Ordering::Acquire) {
                    shared.exit_code.load(Ordering::Acquire)
                } else {
                    1
                };
                window.app_handle().exit(exit_code);
            }
        })
        .build(context())
        .map_err(|error| format!("cannot start Tauri: {error}"))?;
    // `run` exits the process inside Tao on macOS. Wry's event loop may also
    // return zero after AppHandle::exit(1), so retain both the requested exit
    // outcome and the independently recorded task result before returning to
    // main. Closing or timing out cannot be turned into success by a worker
    // completing concurrently with shutdown.
    let failed_exit = Arc::new(AtomicBool::new(false));
    let observed_exit = failed_exit.clone();
    let runtime_exit_code = app.run_return(move |app, event| {
        if let tauri::RunEvent::ExitRequested { code, .. } = event {
            let shared = app.state::<Shared>();
            if code.is_some_and(|code| code != 0) || !shared.finished.load(Ordering::Acquire) {
                observed_exit.store(true, Ordering::Release);
            }
            shared.retire();
        }
    });
    completed(
        exit_code.load(Ordering::Acquire),
        runtime_exit_code,
        failed_exit.load(Ordering::Acquire),
    )
}

fn completed(task_exit_code: i32, runtime_exit_code: i32, failed_exit: bool) -> Result<(), String> {
    if task_exit_code == 0 && runtime_exit_code == 0 && !failed_exit {
        Ok(())
    } else {
        Err("task did not finish with a verified result; no retry".into())
    }
}

#[cfg(test)]
mod tests;
