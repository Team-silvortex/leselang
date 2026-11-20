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
