use super::*;
use std::net::{TcpListener, TcpStream};
use std::thread::{self, JoinHandle};

fn request() -> Request {
    Request {
        system: "Return a JSON task; \"quotes\" and \\ remain data.\n下一行".into(),
        user: "请保存：你好 ✓".into(),
    }
}

fn run() -> Run {
    Run {
        id: "request-1".into(),
        cancelled: AtomicBool::new(false),
    }
}

fn response(content: &str) -> String {
    json!({"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":content}}]})
        .to_string()
}

fn read_request(stream: &mut TcpStream) -> String {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        let length = stream.read(&mut buffer).unwrap();
        if length == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..length]);
        if let Some(end) = bytes.windows(4).position(|value| value == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
            let length: usize = headers
                .lines()
                .find_map(|line| line.strip_prefix("content-length: "))
                .unwrap()
                .parse()
                .unwrap();
            if bytes.len() >= end + 4 + length {
                break;
            }
        }
        assert!(bytes.len() < 5 * 1024 * 1024);
    }
    String::from_utf8(bytes).unwrap()
}

fn server(raw: Vec<u8>, delay: Duration) -> (Config, JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let worker = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(5))
                }
                Err(error) => panic!("local test listener: {error}"),
            }
        };
        // Accept is polled nonblockingly; request parsing uses its own bounded
        // blocking reads and must not depend on the first packet already arriving.
        stream.set_nonblocking(false).unwrap();
        let received = read_request(&mut stream);
        thread::sleep(delay);
        let _ = stream.write_all(&raw);
        received
    });
    let config = Config::parse(
        Some(format!("http://{address}/v1/chat/completions")),
        Some("test-model".into()),
        Some("synthetic-key\\\"data".into()),
    )
    .unwrap()
    .unwrap();
    (config, worker)
}

fn http(status: &str, body: &str) -> Vec<u8> {
    format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).into_bytes()
}

#[test]
fn configuration_is_explicit_and_info_never_contains_credentials_or_endpoint() {
    assert!(Config::parse(None, None, None).unwrap().is_none());
    let config = Config::parse(
        Some("https://example.test/v1/chat/completions".into()),
        Some("chosen-model".into()),
        Some("private-token".into()),
    )
    .unwrap();
    assert_eq!(
        Settings(Ok(config)).info().unwrap(),
        json!({"configured":true,"model":"chosen-model"})
    );
    for (endpoint, model, key) in [
        (None, Some("model"), None),
        (
            Some("http://remote.test/v1/chat/completions"),
            Some("model"),
            Some("key"),
        ),
        (
            Some("https://example.test/v1/chat/completions"),
            Some("model"),
            None,
        ),
        (
            Some("https://user:secret@example.test/v1/chat/completions"),
            Some("model"),
            Some("key"),
        ),
        (
            Some("https://example.test/v1/chat/completions?key=secret"),
            Some("model"),
            Some("key"),
        ),
        (
            Some("https://example.test/v1/chat/completions#fragment"),
            Some("model"),
            Some("key"),
        ),
        (Some("file:///tmp/model"), Some("model"), Some("key")),
        (
            Some("https://example.test/v1/chat/completions"),
            Some("model"),
            Some("bad\nheader"),
        ),
        (
            Some("https://example.test/v1/chat/completions"),
            Some("\n"),
            Some("key"),
        ),
    ] {
        assert_eq!(
            Config::parse(
                endpoint.map(str::to_owned),
                model.map(str::to_owned),
                key.map(str::to_owned)
            )
            .err()
            .unwrap(),
            "model_config_invalid"
        );
    }
    assert!(
        Config::parse(
            Some("http://[::1]:1234/v1/chat/completions".into()),
            Some("local".into()),
            None
        )
        .unwrap()
        .is_some()
    );
}

#[test]
fn local_http_exercises_real_curl_request_and_extracts_only_assistant_content() {
    let content = r#"{"task":"save_message","arguments":{"message":"你好 ✓"}}"#;
    let (config, server) = server(http("200 OK", &response(content)), Duration::ZERO);
    assert_eq!(
        exchange(&config, &request(), &run(), Duration::from_secs(2)).unwrap(),
        content
    );
    let received = server.join().unwrap();
    assert!(received.starts_with("POST /v1/chat/completions HTTP/1.1\r\n"));
    assert!(received.contains("Authorization: Bearer synthetic-key\\\"data\r\n"));
    let (_, body) = received.split_once("\r\n\r\n").unwrap();
    let body: Value = serde_json::from_str(body).unwrap();
    assert_eq!(body["model"], "test-model");
    assert_eq!(body["messages"][0]["content"], request().system);
    assert_eq!(body["messages"][1]["content"], request().user);
    assert_eq!(body["response_format"], json!({"type":"json_object"}));
    assert_eq!(body["stream"], false);
}

#[test]
fn http_errors_and_redirects_never_return_service_text_or_follow_location() {
    for (status, expected) in [
        ("401 Unauthorized", "model_auth_failed"),
        ("403 Forbidden", "model_auth_failed"),
        ("429 Too Many Requests", "model_rate_limited"),
        ("500 Server Error", "model_http_failed"),
        ("302 Found", "model_http_failed"),
    ] {
        let raw = http(status, "PRIVATE KEY reflected by remote service");
        let raw = String::from_utf8(raw)
            .unwrap()
            .replacen(
                "Content-Type:",
                "Location: http://127.0.0.1:1/forbidden\r\nContent-Type:",
                1,
            )
            .into_bytes();
        let (config, server) = server(raw, Duration::ZERO);
        assert_eq!(
            exchange(&config, &request(), &run(), Duration::from_secs(2)).unwrap_err(),
            expected
        );
        server.join().unwrap();
    }
}

#[test]
fn model_response_requires_complete_single_text_choice() {
    let good: Value = serde_json::from_str(&response("{}")).unwrap();
    let mut invalid = vec![
        json!({}),
        json!({"choices":[]}),
        json!({"choices":[good["choices"][0].clone(),good["choices"][0].clone()]}),
    ];
    for finish in ["length", "content_filter", "tool_calls", "unknown"] {
        let mut value = good.clone();
        value["choices"][0]["finish_reason"] = json!(finish);
        invalid.push(value);
    }
    for (field, value) in [
        ("refusal", json!("not allowed")),
        ("tool_calls", json!([])),
        ("function_call", json!({})),
        ("content", Value::Null),
        ("role", json!("user")),
    ] {
        let mut changed = good.clone();
        changed["choices"][0]["message"][field] = value;
        invalid.push(changed);
    }
    for value in invalid {
        assert_eq!(
            parse_response(&value.to_string()).unwrap_err(),
            "model_response_invalid"
        );
    }
    assert!(parse_response("{invalid").is_err());
    assert!(parse_response(&response(&"x".repeat(CONTENT_LIMIT + 1))).is_err());
}

#[test]
fn unknown_content_length_cannot_exceed_reader_bound() {
    let body = "x".repeat(RESPONSE_LIMIT + 4096);
    let raw = format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{body}\r\n0\r\n\r\n", body.len()).into_bytes();
    let (config, server) = server(raw, Duration::ZERO);
    assert_eq!(
        exchange(&config, &request(), &run(), Duration::from_secs(2)).unwrap_err(),
        "model_response_too_large"
    );
    server.join().unwrap();
}

#[test]
fn known_oversized_body_is_rejected_before_decoding() {
    let (config, server) = server(
        http("200 OK", &"x".repeat(RESPONSE_LIMIT + 1)),
        Duration::ZERO,
    );
    assert_eq!(
        exchange(&config, &request(), &run(), Duration::from_secs(2)).unwrap_err(),
        "model_response_too_large"
    );
    server.join().unwrap();
}

#[test]
fn timeout_and_cancellation_settle_process_before_releasing_request() {
    for cancel in [false, true] {
        let (config, server) = server(http("200 OK", &response("{}")), Duration::from_millis(200));
        let run = Arc::new(run());
        let cancelling = run.clone();
        let cancel_worker = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            if cancel {
                cancelling.cancelled.store(true, Ordering::Release);
            }
        });
        let timeout = if cancel {
            Duration::from_secs(2)
        } else {
            Duration::from_millis(80)
        };
        assert_eq!(
            exchange(&config, &request(), &run, timeout).unwrap_err(),
            if cancel {
                "model_request_cancelled"
            } else {
                "model_request_timeout"
            }
        );
        cancel_worker.join().unwrap();
        server.join().unwrap();
    }
}

#[test]
fn request_admission_rejects_oversize_or_unknown_fields_before_network() {
    assert!(
        serde_json::from_value::<Request>(json!({"system":"s","user":"u","model":"overridden"}))
            .is_err()
    );
    assert!(
        Request {
            system: "s".into(),
            user: "x".repeat(2 * 1024 * 1024 + 1)
        }
        .validate()
        .is_err()
    );
    assert!(
        Request {
            system: "s\0".into(),
            user: "u".into()
        }
        .validate()
        .is_err()
    );
    assert!(
        Request {
            system: "s".into(),
            user: "x".repeat(2 * 1024 * 1024)
        }
        .validate()
        .is_ok()
    );
}

#[test]
fn cancellation_tombstone_overlap_late_completion_and_capacity_are_fenced() {
    let mut registry = Registry::default();
    registry.cancel("early").unwrap();
    assert!(registry.begin("early".into()).is_err());
    let first = registry.begin("first".into()).unwrap();
    registry.cancel("first").unwrap();
    assert!(registry.busy());
    assert!(registry.begin("second".into()).is_err());
    assert_eq!(
        registry.finish(&first, Ok("late".into())).unwrap_err(),
        "model_request_cancelled"
    );
    let second = registry.begin("second".into()).unwrap();
    registry.cancel("first").unwrap();
    assert_eq!(
        registry.finish(&second, Ok("current".into())).unwrap(),
        "current"
    );
    assert!(registry.finish(&first, Ok("duplicate".into())).is_err());
    for index in registry.used.len()..REQUEST_LIMIT {
        registry.cancel(&format!("cancel-{index}")).unwrap();
    }
    assert_eq!(
        registry.begin("overflow".into()).err().unwrap(),
        "model_request_limit"
    );
    assert_eq!(
        registry.cancel("overflow").unwrap_err(),
        "model_request_limit"
    );
}
