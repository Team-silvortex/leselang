//! A portable Rust VM owner controlling a separate, application-owned JS process.
//! This example deliberately has no Tauri runtime or JavaScript evaluator.
use leselang_tauri_host::{Scope, Transport, describe_registered, run_registered};
use serde::{
    Deserialize, Deserializer,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Map, Number, Value, json};
use std::{
    fmt,
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Child, Command, ExitCode, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, SyncSender, sync_channel},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const WIRE_LIMIT: usize = 32 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const SOURCE: &str = include_str!("../../../consumer/javascript/publish.lese");
const MAX_SOURCE_BYTES: usize = 16 * 1024;

fn read_source(reader: impl Read) -> Result<String, String> {
    let mut bytes = Vec::new();
    reader
        .take((MAX_SOURCE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read source: {error}"))?;
    if bytes.len() > MAX_SOURCE_BYTES {
        return Err("source exceeds 16384 UTF-8 bytes".into());
    }
    String::from_utf8(bytes).map_err(|_| "source is not valid UTF-8".into())
}

/// JSON objects must not contain duplicate fields, including nested objects.
/// Parsing Value directly would discard that evidence before gateway validation.
struct StrictValue(Value);
impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Json;
        impl<'de> Visitor<'de> for Json {
            type Value = StrictValue;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("unambiguous JSON")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Bool(v)))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Number(v.into())))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Number(v.into())))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
                Number::from_f64(v)
                    .map(|v| StrictValue(Value::Number(v)))
                    .ok_or_else(|| E::custom("invalid number"))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::String(v.into())))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::String(v)))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
                let mut items = Vec::new();
                while let Some(StrictValue(value)) = a.next_element()? {
                    items.push(value);
                }
                Ok(StrictValue(Value::Array(items)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
                let mut fields = Map::new();
                while let Some(key) = a.next_key::<String>()? {
                    if fields.contains_key(&key) {
                        return Err(de::Error::custom("duplicate JSON field"));
                    }
                    fields.insert(key, a.next_value::<StrictValue>()?.0);
                }
                Ok(StrictValue(Value::Object(fields)))
            }
        }
        d.deserialize_any(Json)
    }
}

fn read_record(reader: &mut impl BufRead) -> Result<Value, String> {
    let mut bytes = Vec::new();
    loop {
        let available = reader
            .fill_buf()
            .map_err(|e| format!("read JavaScript reply: {e}"))?;
        if available.is_empty() {
            return Err("JavaScript reply closed or truncated".into());
        }
        let length = available
            .iter()
            .position(|b| *b == b'\n')
            .map_or(available.len(), |i| i + 1);
        if bytes.len() + length > WIRE_LIMIT {
            return Err("JavaScript reply exceeds 32 KiB".into());
        }
        let complete = available[length - 1] == b'\n';
        bytes.extend_from_slice(&available[..length]);
        reader.consume(length);
        if complete {
            return serde_json::from_slice::<StrictValue>(&bytes)
                .map(|value| value.0)
                .map_err(|_| "malformed JavaScript JSON reply".into());
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Correlation {
    version: u64,
    window: String,
    epoch: String,
    session: String,
    serial: u64,
}
impl Correlation {
    fn read(value: &Value) -> Option<Self> {
        Some(Self {
            version: value["transport_version"].as_u64()?,
            window: value["window"].as_str()?.into(),
            epoch: value["epoch"].as_str()?.into(),
            session: value["session"].as_str()?.into(),
            serial: value["request_id"].as_u64()?,
        })
    }
}

#[derive(Clone, Debug)]
struct Pending {
    correlation: Correlation,
    challenge: String,
}

fn challenge() -> Result<String, String> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let mut bytes = [0_u8; 32];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut random| random.read_exact(&mut bytes))
            .map_err(|_| "cannot obtain operating-system request randomness")?;
        Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    Err("the Node reference transport currently supports Linux and macOS".into())
}

fn deliver(pending: &Mutex<Option<Pending>>, value: Value) -> Result<Value, String> {
    let fields = value.as_object().ok_or("invalid stdio carrier envelope")?;
    if fields.len() != 3
        || value["stdio_version"] != 1
        || !fields.contains_key("challenge")
        || !fields.contains_key("reply")
    {
        return Err("invalid stdio carrier envelope".into());
    }
    let mut current = pending.lock().map_err(|_| "pending request lock failed")?;
    if current.as_ref().is_some_and(|expected| {
        value["challenge"].as_str() == Some(expected.challenge.as_str())
            && Correlation::read(&value["reply"]).as_ref() == Some(&expected.correlation)
    }) {
        current.take();
        Ok(value["reply"].clone())
    } else {
        Err("unsolicited, duplicate or foreign JavaScript reply".into())
    }
}

struct NodeTransport {
    child: Child,
    outgoing: Option<SyncSender<Vec<u8>>>,
    incoming: Receiver<Result<Value, String>>,
    live: Arc<AtomicBool>,
    pending: Arc<Mutex<Option<Pending>>>,
    timeout: Duration,
}

impl NodeTransport {
    fn start(scope: &Scope, fault: bool) -> Result<Self, String> {
        let adapter =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../consumer/javascript/adapter.mjs");
        let mut command = Command::new("node");
        command.arg(adapter);
        if fault {
            command.arg("--fault-after-commit");
        }
        Self::spawn(command, scope, REQUEST_TIMEOUT)
    }

    fn spawn(mut command: Command, scope: &Scope, timeout: Duration) -> Result<Self, String> {
        // Fail unsupported platforms before starting any application process.
        let _ = challenge()?;
        command
            .env(
                "LESELANG_BRIDGE_SCOPE",
                json!({"window":scope.window,"epoch":scope.epoch,"session":scope.session})
                    .to_string(),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        let mut child = command
            .spawn()
            .map_err(|e| format!("cannot start the JavaScript consumer: {e}"))?;
        let mut stdin = child.stdin.take().ok_or("missing child stdin")?;
        let stdout = child.stdout.take().ok_or("missing child stdout")?;
        let (outgoing, requests) = sync_channel::<Vec<u8>>(1);
        let (replies, incoming) = sync_channel(1);
        let live = Arc::new(AtomicBool::new(true));
        let pending = Arc::new(Mutex::new(None));
        let writer_live = live.clone();
        let writer_replies = replies.clone();
        thread::spawn(move || {
            while let Ok(request) = requests.recv() {
                if !writer_live.load(Ordering::Acquire) {
                    break;
                }
                if stdin
                    .write_all(&request)
                    .and_then(|()| stdin.flush())
                    .is_err()
                {
                    writer_live.store(false, Ordering::Release);
                    let _ = writer_replies.try_send(Err(
                        "JavaScript request write failed; outcome may be partial".into(),
                    ));
                    break;
                }
            }
        });
        let reader_live = live.clone();
        let reader_pending = pending.clone();
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            while reader_live.load(Ordering::Acquire) {
                let reply =
                    read_record(&mut reader).and_then(|value| deliver(&reader_pending, value));
                let failed = reply.is_err();
                if failed {
                    reader_live.store(false, Ordering::Release);
                }
                if replies.try_send(reply).is_err() {
                    reader_live.store(false, Ordering::Release);
                    break;
                }
                if failed {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            outgoing: Some(outgoing),
            incoming,
            live,
            pending,
            timeout,
        })
    }
    fn retire(&mut self) {
        self.live.store(false, Ordering::Release);
        self.outgoing.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Transport for NodeTransport {
    fn exchange(&mut self, request: Value) -> Result<Value, String> {
        if !self.is_live() {
            self.retire();
            return Err("JavaScript connection is retired".into());
        }
        let result = (|| {
            let challenge = challenge()?;
            let correlation = Correlation::read(&request).ok_or("request lacks correlation")?;
            let carrier = json!({"stdio_version":1,"challenge":challenge,"request":request});
            let mut bytes =
                serde_json::to_vec(&carrier).map_err(|_| "cannot encode JavaScript request")?;
            bytes.push(b'\n');
            if bytes.len() > WIRE_LIMIT {
                return Err("JavaScript request exceeds 32 KiB".into());
            }
            let expected = Pending {
                correlation,
                challenge,
            };
            {
                let mut pending = self
                    .pending
                    .lock()
                    .map_err(|_| "pending request lock failed")?;
                if pending.is_some() {
                    return Err("a JavaScript request is already pending".into());
                }
                *pending = Some(expected);
            }
            self.outgoing
                .as_ref()
                .ok_or("JavaScript writer is retired")?
                .try_send(bytes)
                .map_err(|_| "JavaScript request writer unavailable")?;
            let reply = self.incoming.recv_timeout(self.timeout).map_err(|_| {
                "JavaScript reply deadline or delivery failed; outcome may be partial".to_owned()
            })??;
            if !self.is_live() {
                return Err("JavaScript connection retired before acceptance".into());
            }
            Ok(reply)
        })();
        if result.is_err() {
            self.retire();
        }
        result
    }
    fn is_live(&self) -> bool {
        self.live.load(Ordering::Acquire)
    }
}
impl Drop for NodeTransport {
    fn drop(&mut self) {
        self.retire();
    }
}

fn verified(report: &Value, text: &str) -> bool {
    let Some(before) = report["objects_before"].as_array() else {
        return false;
    };
    let Some(after) = report["objects_after"].as_array() else {
        return false;
    };
    let get = |items: &Vec<Value>, name: &str| {
        items
            .iter()
            .find(|item| item["identifier"] == name)
            .cloned()
    };
    let (Some(draft), Some(publish), Some(status)) = (
        get(after, "draft"),
        get(after, "publish"),
        get(after, "status"),
    ) else {
        return false;
    };
    report["kind"] == "javascript_run"
        && report["profile"] == "javascript.registered"
        && report["result"] == json!({"type":"boolean","value":true})
        && report["effects"] == 9
        && before.len() == 5
        && after.len() == 5
        && draft["value"] == text
        && publish["value"] == text
        && publish["clicks"] == 1
        && get(before, "publish").is_some_and(|value| value["clicks"] == 0)
        && status["value"] == "published"
        && ["reset", "unrelated"]
            .iter()
            .all(|name| get(before, name).is_some() && get(before, name) == get(after, name))
}

fn execute() -> Result<(), String> {
    let args = std::env::args_os()
        .skip(1)
        .map(|arg| arg.into_string().map_err(|_| "arguments must be UTF-8"))
        .collect::<Result<Vec<_>, _>>()?;
    if args.first().is_some_and(|arg| arg == "--describe") {
        if args.len() != 1 {
            return Err("--describe must be used alone".into());
        }
        std::io::stdout()
            .write_all(describe_registered()?.as_bytes())
            .map_err(|_| "cannot write catalog")?;
        return Ok(());
    }
    let mut text = None;
    let mut source = None;
    let mut fault = false;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--text" if text.is_none() => text = Some(args.next().ok_or("--text requires TEXT")?),
            "--run" if source.is_none() => source = Some(args.next().ok_or("--run requires FILE")?),
            "--fault-after-commit" if !fault => fault = true,
            _ => {
                return Err(
                    "usage: node_host --describe | [--run FILE] [--text TEXT] [--fault-after-commit]".into(),
                );
            }
        }
    }
    let text = text.unwrap_or_else(|| "JavaScript publication ✓".into());
    if text.len() > 4096 || text.contains('\0') {
        return Err("text exceeds 4096 UTF-8 bytes or contains NUL".into());
    }
    // Admit the complete bounded UTF-8 file before creating a scope or spawning
    // the JavaScript application. The VM still owns syntax and type validation.
    let source = source
        .map(|path| {
            let file = std::fs::File::open(path)
                .map_err(|error| format!("cannot open source: {error}"))?;
            read_source(file)
        })
        .transpose()?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "clock before epoch")?
        .as_nanos();
    let scope = Scope {
        window: "node-main".into(),
        epoch: format!("process-{}", std::process::id()),
        session: format!("session-{nonce}"),
    };
    let mut transport = NodeTransport::start(&scope, fault)?;
    let mut report = run_registered(
        &mut transport,
        scope,
        source.as_deref().unwrap_or(SOURCE),
        &text,
    )?;
    report["verified"] = json!(verified(&report, &text));
    let mut bytes = serde_json::to_vec(&report).map_err(|_| "cannot encode report")?;
    bytes.push(b'\n');
    if bytes.len() > 128 * 1024 {
        return Err("publication report exceeds 128 KiB".into());
    }
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(&bytes)
        .and_then(|()| stdout.flush())
        .map_err(|_| "cannot write complete report")?;
    if report["verified"] == true {
        Ok(())
    } else {
        Err("publication postcondition failed; no retry".into())
    }
}

fn main() -> ExitCode {
    match execute() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Node host: {error}; outcome may be partial; no retry");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_ingress_is_bounded_and_keeps_utf8_validation_before_application_start() {
        assert_eq!(
            read_source(std::io::Cursor::new("x".repeat(MAX_SOURCE_BYTES)))
                .unwrap()
                .len(),
            MAX_SOURCE_BYTES
        );
        assert!(
            read_source(std::io::Cursor::new(vec![b'x'; MAX_SOURCE_BYTES + 1]))
                .unwrap_err()
                .contains("16384")
        );
        assert!(
            read_source(std::io::Cursor::new([0xff]))
                .unwrap_err()
                .contains("UTF-8")
        );
        assert_eq!(
            read_source(std::io::Cursor::new("literal\0text")).unwrap(),
            "literal\0text"
        );
        struct Continuous(usize);
        impl Read for Continuous {
            fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
                self.0 += bytes.len();
                bytes.fill(b'x');
                Ok(bytes.len())
            }
        }
        let mut input = Continuous(0);
        assert!(read_source(&mut input).is_err());
        assert_eq!(input.0, MAX_SOURCE_BYTES + 1);
    }
    #[test]
    fn json_records_are_bounded_complete_and_unambiguous() {
        for bytes in [
            b"{\"x\":1,\"x\":2}\n".as_slice(),
            b"{\"x\":{\"y\":1,\"y\":2}}\n",
            b"true false\n",
            b"{}",
            b"\xff\n",
        ] {
            assert!(read_record(&mut std::io::Cursor::new(bytes)).is_err());
        }
        let oversized = vec![b' '; WIRE_LIMIT + 1];
        assert!(read_record(&mut std::io::Cursor::new(oversized)).is_err());
        assert_eq!(
            read_record(&mut std::io::Cursor::new(b"{\"x\":[1,true,null]}\n")).unwrap(),
            json!({"x":[1,true,null]})
        );
    }
    #[test]
    fn reply_gate_rejects_preplay_foreign_and_duplicate_delivery() {
        let reply = json!({"transport_version":1,"window":"node-main","epoch":"e","session":"s","request_id":1});
        let value = json!({"stdio_version":1,"challenge":"a".repeat(64),"reply":reply});
        let pending = Mutex::new(None);
        assert!(deliver(&pending, value.clone()).is_err());
        *pending.lock().unwrap() = Some(Pending {
            correlation: Correlation::read(&reply).unwrap(),
            challenge: "a".repeat(64),
        });
        let mut foreign = value.clone();
        foreign["reply"]["request_id"] = json!(2);
        assert!(deliver(&pending, foreign).is_err());
        for (field, invalid) in [
            ("stdio_version", json!(2)),
            ("challenge", json!("b".repeat(64))),
            ("extra", json!(true)),
        ] {
            let mut malformed = value.clone();
            malformed[field] = invalid;
            assert!(deliver(&pending, malformed).is_err());
        }
        assert_eq!(deliver(&pending, value.clone()).unwrap(), reply);
        assert!(deliver(&pending, value).is_err());
    }

    fn fixture(mode: &str) -> (NodeTransport, Value) {
        let scope = Scope {
            window: "node-main".into(),
            epoch: "test-document".into(),
            session: "test-session".into(),
        };
        let mut command = Command::new("node");
        command
            .arg(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../consumer/javascript/transport-fixture.mjs"),
            )
            .arg(mode);
        let transport = NodeTransport::spawn(command, &scope, Duration::from_millis(250)).unwrap();
        let request = json!({"transport_version":1,"window":scope.window,"epoch":scope.epoch,"session":scope.session,
            "request_id":1,"budget_ms":5000,"operation":"gui.list","arguments":{}});
        (transport, request)
    }

    #[test]
    #[ignore = "starts Node fixture processes; run explicitly with the JavaScript consumer checks"]
    fn stdio_rejects_a_preplayed_first_reply_even_if_the_serial_matches() {
        let (mut transport, request) = fixture("preplay");
        assert!(transport.exchange(request).is_err());
        assert!(!transport.is_live());
        assert!(transport.child.try_wait().unwrap().is_some());
    }

    #[test]
    #[ignore = "starts Node fixture processes; run explicitly with the JavaScript consumer checks"]
    fn stdio_rejects_buffered_future_reply_even_if_a_new_request_is_pending() {
        let (mut transport, request) = fixture("future");
        let _ = transport.exchange(request.clone());
        let mut future = request;
        future["request_id"] = json!(2);
        assert!(transport.exchange(future).is_err());
        assert!(!transport.is_live());
    }

    #[test]
    #[ignore = "starts Node fixture processes; run explicitly with the JavaScript consumer checks"]
    fn stdio_deadline_covers_an_unread_pipe_and_reaps_the_child() {
        let (mut transport, mut request) = fixture("no-read");
        request["arguments"]["large"] = json!("x".repeat(30_000));
        let started = std::time::Instant::now();
        let error = transport.exchange(request).unwrap_err();
        assert!(error.contains("deadline"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(!transport.is_live());
        assert!(transport.child.try_wait().unwrap().is_some());
    }
}
