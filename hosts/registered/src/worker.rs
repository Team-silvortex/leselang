//! One bounded Rust VM execution owned by an independent JavaScript parent.
//! stdout is a carrier only; no Tauri runtime, child application or evaluator.
mod json_lines;
mod operations;

use crate::{
    ExecutionFailure, ExecutionFailureCode, PreparedTask, Scope, Transport, describe_registered,
    prepare_named_task, prepare_task, run_prepared_registered_task_detailed,
};
use json_lines::{Correlation, challenge, read_record};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{BufReader, Write},
    process::ExitCode,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, SyncSender, sync_channel},
    },
    thread,
    time::{Duration, Instant},
};

const START_LIMIT: usize = 128 * 1024;
const WIRE_LIMIT: usize = 64 * 1024;
const REPORT_LIMIT: usize = 128 * 1024;
const TERMINAL_LIMIT: usize = 512 * 1024;
const TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartScope {
    window: String,
    epoch: String,
    session: String,
}

impl StartScope {
    fn admit(self) -> Result<Scope, String> {
        if [&self.window, &self.epoch, &self.session]
            .iter()
            .any(|name| !crate::gateway::valid_name(name))
        {
            return Err("invalid worker scope".into());
        }
        Ok(Scope {
            window: self.window,
            epoch: self.epoch,
            session: self.session,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Start {
    agent_worker_version: u32,
    kind: String,
    scope: StartScope,
    source: String,
    arguments: BTreeMap<String, String>,
}

fn admit(mut value: Value) -> Result<(Scope, PreparedTask), String> {
    // Version three opts into terminal execution reports without conflating
    // that choice with the independent single/named-input admission rules.
    // Legacy versions retain their exact field set, including rejecting this
    // new field even when its value is null.
    let named_inputs = if value["agent_worker_version"].as_u64() == Some(3) {
        match value
            .as_object_mut()
            .and_then(|fields| fields.remove("input_mode"))
        {
            Some(Value::String(mode)) if mode == "single_string" => false,
            Some(Value::String(mode)) if mode == "named_strings" => true,
            _ => return Err("invalid worker input mode".into()),
        }
    } else {
        value["agent_worker_version"].as_u64() == Some(2)
    };
    let start: Start = serde_json::from_value(value).map_err(|_| "invalid worker start record")?;
    if !matches!(start.agent_worker_version, 1..=3) || start.kind != "start" {
        return Err("unsupported worker start record".into());
    }
    let scope = start.scope.admit()?;
    // Complete syntax, type and argument admission before any Bridge request.
    let arguments = start.arguments.into_iter().collect();
    let prepared = if named_inputs {
        prepare_named_task(&start.source, arguments)?
    } else {
        prepare_task(&start.source, arguments)?
    };
    Ok((scope, prepared))
}

#[derive(Clone)]
struct Pending {
    version: u32,
    correlation: Correlation,
    challenge: String,
}

fn deliver(pending: &Mutex<Option<Pending>>, value: Value) -> Result<Value, String> {
    let fields = value.as_object().ok_or("invalid worker reply envelope")?;
    if fields.len() != 4
        || !matches!(value["agent_worker_version"].as_u64(), Some(1..=4))
        || value["kind"] != "reply"
        || !fields.contains_key("challenge")
        || !fields.contains_key("reply")
    {
        return Err("invalid worker reply envelope".into());
    }
    let mut current = pending
        .lock()
        .map_err(|_| "pending worker request lock failed")?;
    if current.as_ref().is_some_and(|expected| {
        value["agent_worker_version"] == expected.version
            && value["challenge"].as_str() == Some(expected.challenge.as_str())
            && Correlation::read(&value["reply"]).as_ref() == Some(&expected.correlation)
    }) {
        current.take();
        Ok(value["reply"].clone())
    } else {
        Err("unsolicited, duplicate or foreign worker reply".into())
    }
}

struct Outgoing {
    bytes: Vec<u8>,
    receipt: SyncSender<Result<(), String>>,
}

/// Dedicated bounded readers/writers keep both empty and blocked pipes inside
/// the main thread's deadline. They are deliberately not joined on retirement:
/// returning from main ends the one-task process even if an OS read is blocked.
struct StdioTransport {
    version: u32,
    outgoing: SyncSender<Outgoing>,
    incoming: Receiver<Result<Value, String>>,
    live: Arc<AtomicBool>,
    pending: Arc<Mutex<Option<Pending>>>,
}

impl StdioTransport {
    fn open() -> Self {
        let (outgoing, writes) = sync_channel::<Outgoing>(1);
        let (replies, incoming) = sync_channel(1);
        let live = Arc::new(AtomicBool::new(true));
        let pending = Arc::new(Mutex::new(None));
        thread::spawn(move || {
            let stdout = std::io::stdout();
            let mut writer = stdout.lock();
            while let Ok(outgoing) = writes.recv() {
                let result = writer
                    .write_all(&outgoing.bytes)
                    .and_then(|()| writer.flush())
                    .map_err(|_| "worker stdout write failed".to_owned());
                let failed = result.is_err();
                let _ = outgoing.receipt.try_send(result);
                if failed {
                    break;
                }
            }
        });
        let reader_live = live.clone();
        let reader_pending = pending.clone();
        thread::spawn(move || {
            let mut reader = BufReader::new(std::io::stdin());
            let initial = read_record(&mut reader, START_LIMIT);
            if initial.is_err() {
                reader_live.store(false, Ordering::Release);
            }
            let failed = initial.is_err();
            if replies.try_send(initial).is_err() || failed {
                reader_live.store(false, Ordering::Release);
                return;
            }
            while reader_live.load(Ordering::Acquire) {
                let reply = read_record(&mut reader, WIRE_LIMIT)
                    .and_then(|value| deliver(&reader_pending, value));
                let failed = reply.is_err();
                if failed {
                    reader_live.store(false, Ordering::Release);
                }
                if replies.try_send(reply).is_err() || failed {
                    reader_live.store(false, Ordering::Release);
                    break;
                }
            }
        });
        Self {
            version: 1,
            outgoing,
            incoming,
            live,
            pending,
        }
    }

    fn write(&self, value: Value, limit: usize) -> Result<(), String> {
        let mut bytes = serde_json::to_vec(&value).map_err(|_| "worker encoding failed")?;
        bytes.push(b'\n');
        if bytes.len() > limit {
            return Err("worker output exceeds wire limit".into());
        }
        let (receipt, done) = sync_channel(1);
        self.outgoing
            .try_send(Outgoing { bytes, receipt })
            .map_err(|_| "worker writer unavailable")?;
        done.recv_timeout(TIMEOUT)
            .map_err(|_| "worker write deadline exceeded")?
    }

    fn initial(&self) -> Result<Value, String> {
        let value = self
            .incoming
            .recv_timeout(TIMEOUT)
            .map_err(|_| "worker start deadline exceeded")??;
        if !self.is_live() {
            return Err("worker connection closed before admission".into());
        }
        Ok(value)
    }

    fn retire(&self) {
        self.live.store(false, Ordering::Release);
        if let Ok(mut pending) = self.pending.lock() {
            pending.take();
        }
    }

    fn serve(&mut self) -> Result<Value, String> {
        let initial = self.initial()?;
        if let Some(version @ 1..=4) = initial["agent_worker_version"].as_u64() {
            self.version = version as u32;
        }
        if self.version == 4 {
            return operations::execute(self, initial);
        }
        let (scope, prepared) = admit(initial)?;
        if !self.is_live() {
            return Err("worker connection closed before execution".into());
        }
        let version = self.version;
        execute_admitted(self, scope, prepared, version)
    }
}

impl Transport for StdioTransport {
    fn exchange(&mut self, request: Value) -> Result<Value, String> {
        let result = (|| {
            let began = Instant::now();
            if !self.is_live() {
                return Err("worker connection is retired".into());
            }
            let challenge = challenge()?;
            let correlation =
                Correlation::read(&request).ok_or("worker request lacks correlation")?;
            {
                let mut pending = self
                    .pending
                    .lock()
                    .map_err(|_| "pending worker request lock failed")?;
                if pending.is_some() {
                    return Err("a worker request is already pending".into());
                }
                *pending = Some(Pending {
                    version: self.version,
                    correlation,
                    challenge: challenge.clone(),
                });
            }
            self.write(json!({"agent_worker_version":self.version,"kind":"request","challenge":challenge,"request":request}), WIRE_LIMIT)?;
            let remaining = TIMEOUT
                .checked_sub(began.elapsed())
                .ok_or("worker request deadline exceeded")?;
            let reply = self
                .incoming
                .recv_timeout(remaining)
                .map_err(|_| "worker reply deadline exceeded; outcome may be partial")??;
            if !self.is_live() {
                return Err("worker connection retired before acceptance".into());
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

fn execute_admitted(
    transport: &mut impl Transport,
    scope: Scope,
    prepared: PreparedTask,
    version: u32,
) -> Result<Value, String> {
    match run_prepared_registered_task_detailed(transport, scope, prepared) {
        Ok(report) => Ok(report),
        Err(error) if version == 3 && transport.is_live() => Ok(execution_failure_report(error)),
        Err(error) => Err(error.to_string()),
    }
}

fn execution_failure_report(error: ExecutionFailure) -> Value {
    let code = error.code();
    let message = error.to_string();
    let message = if message.is_empty() {
        "workflow execution failed".to_owned()
    } else if message.len() <= 4096 {
        message
    } else {
        let suffix = "… [truncated]";
        let mut end = 4096 - suffix.len();
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}{suffix}", &message[..end])
    };
    json!({
        "schema_version": 1,
        "kind": "javascript_task_run",
        "profile": "javascript.registered",
        "status": if code == ExecutionFailureCode::Cancelled { "cancelled" } else { "failed" },
        "error": message,
        "possible_partial_effects": true,
        "execution_failure": { "code": code.as_str() },
    })
}

fn error_message(error: &str) -> String {
    let mut end = error.len().min(1024);
    while !error.is_char_boundary(end) {
        end -= 1;
    }
    error[..end].to_owned()
}

fn serve() -> Result<(), String> {
    let mut transport = StdioTransport::open();
    let mut terminal_started = false;
    let result = transport.serve().and_then(|report| {
        let report_json =
            serde_json::to_string(&report).map_err(|_| "worker report encoding failed")?;
        if report_json.len() > REPORT_LIMIT {
            return Err("worker report exceeds 128 KiB".into());
        }
        if !transport.is_live() {
            return Err("worker connection closed before report delivery".into());
        }
        terminal_started = true;
        transport.write(
            json!({"agent_worker_version":transport.version,"kind":"report","report_json":report_json}),
            TERMINAL_LIMIT,
        )?;
        if !transport.is_live() {
            return Err("worker connection closed during report delivery".into());
        }
        Ok(())
    });
    if let Err(error) = &result {
        transport.retire();
        // Error delivery is best effort and bounded, including an unread stdout.
        // Once report output begins, report failure through the exit status;
        // never follow a partial or complete report with a second terminal.
        if !terminal_started {
            let _ = transport.write(
                json!({"agent_worker_version":transport.version,"kind":"error","error":error_message(error)}),
                TERMINAL_LIMIT,
            );
        }
    }
    transport.retire();
    result
}

pub fn main() -> ExitCode {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let result = if args.len() == 1 && args[0] == "--describe" {
        describe_registered().and_then(|catalog| {
            std::io::stdout()
                .write_all(catalog.as_bytes())
                .map_err(|_| "cannot write worker catalog".into())
        })
    } else if args.len() == 1 && args[0] == "--serve" {
        serve()
    } else {
        Err("usage: leselang-agent-worker --describe | --serve".into())
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!(
                "Agent worker: {}; outcome may be partial; no retry",
                error_message(&error)
            );
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run_prepared_registered_task;

    fn start(source: &str, arguments: Value) -> Value {
        json!({"agent_worker_version":1,"kind":"start","scope":{"window":"node","epoch":"instance","session":"run_1"},"source":source,"arguments":arguments})
    }

    #[test]
    fn complete_admission_rejects_bad_source_input_scope_and_fields_before_transport() {
        for value in [
            start("this is not source", json!({})),
            start("fn main(text: string) = text", json!({})),
            start("fn main() = true", json!({"text":"extra"})),
            start("fn main() = true", json!({"x":true})),
            start(
                "fn main(text: string) = text",
                json!({"text":"x".repeat(4097)}),
            ),
            start(&" ".repeat(16 * 1024 + 1), json!({})),
            {
                let mut value = start("fn main() = true", json!({}));
                value["scope"]["window"] = json!("invalid.window");
                value
            },
            {
                let mut value = start("fn main() = true", json!({}));
                value["profile"] = json!("tauri.typescript");
                value
            },
            {
                let mut value = start("fn main() = true", json!({}));
                value["kind"] = json!("reply");
                value
            },
        ] {
            assert!(admit(value).is_err());
        }
    }

    #[test]
    fn named_start_requires_explicit_version_two_and_reply_versions_cannot_cross() {
        let source = "fn main(title: string, content: string) = title";
        let mut value = start(source, json!({"title":"Title", "content":"Content"}));
        assert!(
            admit(value.clone()).is_err(),
            "legacy startup keeps its one-input limit"
        );
        value["agent_worker_version"] = json!(2);
        assert!(admit(value.clone()).is_ok());
        value["arguments"]["content"] = json!(true);
        assert!(admit(value.clone()).is_err());
        // Keep every other field valid so an argument failure cannot mask a
        // regression in version admission.
        for version in [
            json!(0),
            json!(4),
            json!(-1),
            json!("2"),
            json!(null),
            json!(2.0),
        ] {
            let mut value = start("fn main() = true", json!({}));
            value["agent_worker_version"] = version;
            assert!(admit(value).is_err());
        }

        let reply = json!({"transport_version":1,"window":"node","epoch":"instance","session":"run_1","request_id":1});
        let pending = Mutex::new(Some(Pending {
            version: 2,
            correlation: Correlation::read(&reply).unwrap(),
            challenge: "a".repeat(64),
        }));
        let mut envelope = json!({"agent_worker_version":1,"kind":"reply","challenge":"a".repeat(64),"reply":reply});
        assert!(deliver(&pending, envelope.clone()).is_err());
        envelope["agent_worker_version"] = json!(2);
        assert_eq!(deliver(&pending, envelope).unwrap(), reply);
    }

    #[test]
    fn execution_reports_require_version_three_with_an_explicit_bounded_input_mode() {
        let mut single = start(
            "fn main(content: string) = content",
            json!({"content":"Text"}),
        );
        single["agent_worker_version"] = json!(3);
        for invalid in [
            None,
            Some(json!(null)),
            Some(json!(true)),
            Some(json!("other")),
        ] {
            let mut value = single.clone();
            if let Some(mode) = invalid {
                value["input_mode"] = mode;
            }
            assert!(admit(value).is_err());
        }
        single["input_mode"] = json!("single_string");
        assert!(admit(single.clone()).is_ok());
        single["input_mode"] = json!("named_strings");
        assert!(admit(single).is_ok());

        let mut named = start(
            "fn main(title: string, content: string) = title",
            json!({"title":"Title", "content":"Content"}),
        );
        named["agent_worker_version"] = json!(3);
        named["input_mode"] = json!("single_string");
        assert!(admit(named.clone()).is_err());
        named["input_mode"] = json!("named_strings");
        assert!(admit(named.clone()).is_ok());
        named["arguments"]["content"] = json!("文".repeat(1366));
        assert!(admit(named).is_err(), "the UTF-8 input bound is unchanged");

        for version in [1, 2] {
            for mode in [json!(null), json!("single_string"), json!("named_strings")] {
                let mut legacy = start("fn main() = true", json!({}));
                legacy["agent_worker_version"] = json!(version);
                legacy["input_mode"] = mode;
                assert!(admit(legacy).is_err(), "legacy frames remain exact");
            }
        }
        let mut extra = start("fn main() = true", json!({}));
        extra["agent_worker_version"] = json!(3);
        extra["input_mode"] = json!("single_string");
        extra["execution_reports"] = json!("v1");
        assert!(admit(extra).is_err());
    }

    #[test]
    fn reply_gate_pins_all_versions_without_consuming_foreign_replies() {
        let reply = json!({"transport_version":1,"window":"node","epoch":"instance","session":"run_1","request_id":1});
        for version in [1, 2, 3, 4] {
            let pending = Mutex::new(Some(Pending {
                version,
                correlation: Correlation::read(&reply).unwrap(),
                challenge: "a".repeat(64),
            }));
            for foreign in [0, 1, 2, 3, 4, 5] {
                if foreign != version {
                    let envelope = json!({"agent_worker_version":foreign,"kind":"reply","challenge":"a".repeat(64),"reply":reply});
                    assert!(deliver(&pending, envelope).is_err());
                    assert!(pending.lock().unwrap().is_some());
                }
            }
            let envelope = json!({"agent_worker_version":version,"kind":"reply","challenge":"a".repeat(64),"reply":reply});
            assert_eq!(deliver(&pending, envelope.clone()).unwrap(), reply);
            assert!(deliver(&pending, envelope).is_err());
        }
    }

    #[test]
    fn framed_json_rejects_duplicate_truncated_oversized_and_invalid_utf8_data() {
        for bytes in [
            b"{\"x\":1,\"x\":2}\n".as_slice(),
            b"{\"x\":{\"a\":1,\"a\":2}}\n",
            b"{}",
            b"\xff\n",
            b"true false\n",
        ] {
            assert!(read_record(&mut std::io::Cursor::new(bytes), WIRE_LIMIT).is_err());
        }
        assert!(
            read_record(
                &mut std::io::Cursor::new(vec![b' '; START_LIMIT + 1]),
                START_LIMIT
            )
            .is_err()
        );
        assert_eq!(
            read_record(
                &mut std::io::Cursor::new(b"{\"a\":[true,null,1]}\n"),
                WIRE_LIMIT
            )
            .unwrap(),
            json!({"a":[true,null,1]})
        );
    }

    #[test]
    fn reply_gate_refuses_preplay_foreign_scope_challenge_extra_fields_and_duplicate() {
        let reply = json!({"transport_version":1,"window":"node","epoch":"instance","session":"run_1","request_id":1});
        let value = json!({"agent_worker_version":1,"kind":"reply","challenge":"a".repeat(64),"reply":reply});
        let pending = Mutex::new(None);
        assert!(deliver(&pending, value.clone()).is_err());
        *pending.lock().unwrap() = Some(Pending {
            version: 1,
            correlation: Correlation::read(&reply).unwrap(),
            challenge: "a".repeat(64),
        });
        for field in [
            "window",
            "epoch",
            "session",
            "request_id",
            "transport_version",
        ] {
            let mut foreign = value.clone();
            foreign["reply"][field] = json!("foreign");
            assert!(deliver(&pending, foreign).is_err());
        }
        for (field, invalid) in [
            ("agent_worker_version", json!(2)),
            ("kind", json!("start")),
            ("challenge", json!("b".repeat(64))),
            ("extra", json!(true)),
        ] {
            let mut foreign = value.clone();
            foreign[field] = invalid;
            assert!(deliver(&pending, foreign).is_err());
        }
        assert_eq!(deliver(&pending, value.clone()).unwrap(), reply);
        assert!(deliver(&pending, value).is_err());
    }

    struct EmptyRegistry {
        requests: usize,
        live: bool,
    }
    impl Transport for EmptyRegistry {
        fn exchange(&mut self, request: Value) -> Result<Value, String> {
            self.requests += 1;
            assert_eq!(request["operation"], "gui.list");
            Ok(
                json!({"transport_version":1,"window":request["window"],"epoch":request["epoch"],"session":request["session"],"request_id":request["request_id"],"status":"ok","value":[],"error":null}),
            )
        }
        fn is_live(&self) -> bool {
            self.live
        }
    }

    #[test]
    fn admitted_no_input_and_renamed_input_tasks_run_the_real_vm_with_typed_results() {
        for (source, arguments, result) in [
            (
                "fn main() = 18446744073709551615",
                json!({}),
                json!({"type":"integer","value":u64::MAX}),
            ),
            (
                "fn main() = true",
                json!({}),
                json!({"type":"boolean","value":true}),
            ),
            (
                "fn main() = none",
                json!({}),
                json!({"type":"none","value":null}),
            ),
            (
                "fn main(content: string) = content",
                json!({"content":"quote \" \\ 文本\n✓"}),
                json!({"type":"string","value":"quote \" \\ 文本\n✓"}),
            ),
        ] {
            let (scope, prepared) = admit(start(source, arguments)).unwrap();
            let mut transport = EmptyRegistry {
                requests: 0,
                live: true,
            };
            let report = run_prepared_registered_task(&mut transport, scope, prepared).unwrap();
            assert_eq!(report["kind"], "javascript_task_run");
            assert_eq!(report["profile"], "javascript.registered");
            assert_eq!(report["result"], result);
            assert_eq!(report["effects"], 0);
            assert!(report["fuel_remaining"].as_u64().unwrap() < 10_000);
            assert_eq!(report["objects_before"], json!([]));
            assert_eq!(report["objects_after"], json!([]));
            assert_eq!(transport.requests, 1);
        }
    }

    #[test]
    fn closed_transport_never_starts_a_registered_task() {
        for version in [1, 2, 3] {
            let (scope, prepared) = admit(start("fn main() = true", json!({}))).unwrap();
            let mut transport = EmptyRegistry {
                requests: 0,
                live: false,
            };
            assert!(execute_admitted(&mut transport, scope, prepared, version).is_err());
            assert_eq!(transport.requests, 0);
        }
    }

    #[test]
    fn live_missing_object_failure_is_a_terminal_report_only_with_explicit_version_three() {
        let source = "fn main() = bind(target: gui.find(name: \"missing\"), body: true)";
        for version in [1, 2, 3] {
            let (scope, prepared) = admit(start(source, json!({}))).unwrap();
            let mut transport = EmptyRegistry {
                requests: 0,
                live: true,
            };
            let result = execute_admitted(&mut transport, scope, prepared, version);
            if version == 3 {
                assert_eq!(
                    result.unwrap(),
                    json!({
                        "schema_version":1, "kind":"javascript_task_run", "profile":"javascript.registered",
                        "status":"failed", "error":"unknown registered object", "possible_partial_effects":true,
                        "execution_failure":{"code":"execution_object_unavailable"},
                    })
                );
            } else {
                assert_eq!(result.unwrap_err(), "unknown registered object");
            }
            assert_eq!(
                transport.requests, 1,
                "only the registry bootstrap was sent"
            );
        }
    }

    #[test]
    fn version_three_success_preserves_exact_report_effect_and_fuel_accounting() {
        let source = "fn main() = bind(first: gui.list(), body: gui.list())";
        let mut expected = None;
        for version in [1, 2, 3] {
            let (scope, prepared) = admit(start(source, json!({}))).unwrap();
            let mut transport = EmptyRegistry {
                requests: 0,
                live: true,
            };
            let report = execute_admitted(&mut transport, scope, prepared, version).unwrap();
            assert_eq!(report["effects"], 2);
            assert!(report.get("status").is_none());
            assert!(report.get("execution_failure").is_none());
            if let Some(old) = &expected {
                assert_eq!(&report, old);
            }
            expected = Some(report);
        }
    }

    #[test]
    fn transport_retirement_never_becomes_a_report_and_live_error_text_cannot_select_a_code() {
        struct BrokenRegistry {
            live: bool,
            retire: bool,
        }
        impl Transport for BrokenRegistry {
            fn exchange(&mut self, _: Value) -> Result<Value, String> {
                if self.retire {
                    self.live = false;
                }
                Err("unknown registered object".into())
            }
            fn is_live(&self) -> bool {
                self.live
            }
        }
        for retire in [false, true] {
            let (scope, prepared) = admit(start("fn main() = true", json!({}))).unwrap();
            let mut transport = BrokenRegistry { live: true, retire };
            let result = execute_admitted(&mut transport, scope, prepared, 3);
            if retire {
                assert_eq!(result.unwrap_err(), "unknown registered object");
            } else {
                let report = result.unwrap();
                assert_eq!(report["error"], "unknown registered object");
                assert_eq!(report["execution_failure"]["code"], "execution_failed");
            }
        }
    }

    #[test]
    fn report_error_text_is_bounded_without_interpreting_it_as_a_failure_code() {
        for message in [
            String::new(),
            "x".repeat(4096),
            "x".repeat(4097),
            "文".repeat(1366),
        ] {
            let report = execution_failure_report(ExecutionFailure::from(message.clone()));
            assert_eq!(report["execution_failure"]["code"], "execution_failed");
            let text = report["error"].as_str().unwrap();
            assert!(!text.is_empty());
            assert!(text.len() <= 4096);
            if message.is_empty() {
                assert_eq!(text, "workflow execution failed");
            } else if message.len() <= 4096 {
                assert_eq!(text, message);
            } else {
                assert!(text.ends_with("… [truncated]"));
            }
        }
        let report = execution_failure_report(ExecutionFailure::from("unknown registered object"));
        assert_eq!(report["execution_failure"]["code"], "execution_failed");
        let report = execution_failure_report(ExecutionFailure::from(crate::Error::Fuel));
        assert_eq!(
            report["execution_failure"]["code"],
            "execution_fuel_exhausted"
        );
    }
}
