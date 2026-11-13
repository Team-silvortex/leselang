//! Tauri owns delivery and the event loop; one dedicated thread owns the VM.
//! Nothing here serializes a LocalVm, ObjectRef, permission grant or receipt.
use crate::transport_core::*;
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
mod tests {
    use super::*;
    #[test]
    fn transport_admits_only_bundled_main_document_urls_and_bounded_nonce_labels() {
        for allowed in [
            "tauri://localhost",
            "tauri://localhost/",
            "tauri://localhost/index.html",
            "http://tauri.localhost/index.html",
        ] {
            assert!(local_asset(&allowed.parse().unwrap()));
        }
        for denied in [
            "https://example.com/",
            "tauri://other/index.html",
            "tauri://localhost/other.html",
            "tauri://localhost/?remote=true",
            "tauri://localhost:9000/",
            "tauri://user@localhost/",
        ] {
            assert!(!local_asset(&denied.parse().unwrap()));
        }
        assert!(valid_nonce("123e4567-e89b-12d3-a456-426614174000"));
        assert!(!valid_nonce(""));
        assert!(!valid_nonce(&"x".repeat(65)));
        assert!(!valid_nonce("label\n"));
    }

    #[test]
    fn task_failure_runtime_failure_or_early_exit_cannot_be_success() {
        assert!(completed(0, 0, false).is_ok());
        for (task, runtime, failed_exit) in [
            (1, 0, false), // Wry reports zero despite a failed verification.
            (0, 1, false), // A successful task cannot mask a runtime failure.
            (0, 0, true),  // Worker finished after close or timeout was requested.
            (1, 0, true),  // Close before startup or a readiness timeout.
        ] {
            assert!(completed(task, runtime, failed_exit).is_err());
        }
    }

    fn envelope() -> Value {
        json!({"transport_version":1,"window":"main","epoch":"page-1","session":"session-1","request_id":1})
    }

    #[test]
    fn replies_require_current_pending_scope_and_are_consumed_once() {
        let reply = envelope();
        let pending = Mutex::new(None);
        assert!(
            accept_pending(&pending, &reply).is_err(),
            "reply before request must fail"
        );
        *pending.lock().unwrap() = Correlation::from_value(&reply);
        for (key, wrong) in [
            ("transport_version", json!(2)),
            ("window", json!("other")),
            ("epoch", json!("page-2")),
            ("session", json!("session-2")),
            ("request_id", json!(2)),
        ] {
            let mut altered = reply.clone();
            altered[key] = wrong;
            assert!(accept_pending(&pending, &altered).is_err(), "{key}");
            assert!(pending.lock().unwrap().is_some());
        }
        accept_pending(&pending, &reply).unwrap();
        assert!(
            accept_pending(&pending, &reply).is_err(),
            "duplicate cannot satisfy a future request"
        );
    }

    #[test]
    fn endpoint_refuses_closed_lifetimes_and_rechecks_after_delivery() {
        for retire_on_delivery in [false, true] {
            let live = Arc::new(AtomicBool::new(retire_on_delivery));
            let callback_live = live.clone();
            let (sender, incoming) = sync_channel(1);
            let channel = Channel::new(move |_| {
                callback_live.store(false, Ordering::Release);
                sender.try_send(envelope()).unwrap();
                Ok(())
            });
            let mut endpoint = Endpoint {
                scope: Scope {
                    window: "main".into(),
                    epoch: "page-1".into(),
                    session: "session-1".into(),
                },
                outgoing: channel,
                incoming,
                live,
                pending: Arc::new(Mutex::new(None)),
                started: Instant::now(),
            };
            assert!(endpoint.exchange(envelope()).is_err());
            assert!(!endpoint.is_live());
        }
    }

    #[test]
    fn total_deadline_retires_an_unanswered_exchange_without_retry() {
        let live = Arc::new(AtomicBool::new(true));
        let (_sender, incoming) = sync_channel(1);
        let channel = Channel::new(move |_| Ok(()));
        let mut endpoint = Endpoint {
            scope: Scope {
                window: "main".into(),
                epoch: "page-1".into(),
                session: "session-1".into(),
            },
            outgoing: channel,
            incoming,
            live,
            pending: Arc::new(Mutex::new(None)),
            started: Instant::now() - EXECUTION_TIMEOUT + Duration::from_millis(30),
        };
        let started = Instant::now();
        assert!(endpoint.exchange(envelope()).is_err());
        assert!(!endpoint.is_live());
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(endpoint.exchange(envelope()).is_err());
    }
}
