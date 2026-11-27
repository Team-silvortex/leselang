//! Test-only carrier: withhold the real worker's first write request or its
//! correlated successful reply. The application sees EOF in either case.
use serde_json::{Value, json};
use std::{
    env,
    error::Error,
    fs::OpenOptions,
    io::{self, BufRead, BufReader, Read, Write},
    process::{Child, Command, ExitCode, Stdio},
};

type TestResult<T> = Result<T, Box<dyn Error>>;

fn record(reader: &mut impl BufRead, limit: u64) -> TestResult<Value> {
    let mut bytes = Vec::new();
    reader.take(limit).read_until(b'\n', &mut bytes)?;
    if bytes.last() != Some(&b'\n') {
        return Err("incomplete or oversized test frame".into());
    }
    Ok(serde_json::from_slice(&bytes)?)
}

fn send(mut writer: impl Write, frame: &Value) -> TestResult<()> {
    serde_json::to_writer(&mut writer, frame)?;
    writer.write_all(b"\n")?;
    Ok(writer.flush()?)
}

fn inject(worker: &mut Child) -> TestResult<String> {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../fixtures/application-operations/lost-reply.json"
    ))?;
    let mut app = io::stdin().lock();
    let start = record(&mut app, 128 * 1024)?;
    if start["profile"] != "lese.operations" {
        return Err("expected the application operations profile".into());
    }
    let operation = if start["source"] == fixture["write_source"] {
        "ledger.add"
    } else if start["source"] == fixture["idempotent_source"] {
        "ledger.add_once"
    } else {
        return Err("expected a ledger write workflow from the shared fixture".into());
    };
    send(worker.stdin.as_mut().unwrap(), &start)?;
    let request = record(&mut BufReader::new(worker.stdout.as_mut().unwrap()), 65536)?;
    let inner = &request["request"];
    if request["kind"] != "request"
        || request["agent_worker_version"] != 4
        || request["challenge"]
            .as_str()
            .is_none_or(|id| id.len() != 64)
        || inner["operation"] != operation
        || inner["request_id"] != 1
        || inner["arguments"] != start["arguments"]
    {
        return Err("expected a first v4 ledger write request".into());
    }
    let case = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["key"] == inner["arguments"]["key"])
        .ok_or("unknown ledger fixture key")?;
    let cut = case["id"].as_str().ok_or("missing cut")?;
    match cut {
        "request-lost" => {} // Keep the first request on this side of the pipe.
        "reply-lost" => {
            send(io::stdout().lock(), &request)?;
            let reply = record(&mut app, 65536)?;
            let expected = json!({
                "agent_worker_version": 4, "kind": "reply", "challenge": request["challenge"],
                "reply": {
                    "transport_version": inner["transport_version"], "window": inner["window"],
                    "epoch": inner["epoch"], "session": inner["session"], "request_id": inner["request_id"],
                    "status": "ok", "value": case["expected"]["balance"], "error": null
                }
            });
            if reply != expected {
                return Err(
                    "application did not send the correlated successful write reply".into(),
                );
            }
            // Deliberately never forward this valid reply to the waiting worker.
        }
        _ => return Err("unknown fault injection point".into()),
    }
    Ok(cut.to_owned())
}

fn run() -> TestResult<()> {
    if !env::args_os().skip(1).eq(["--serve"]) {
        return Err("expected --serve".into());
    }
    let executable = env::current_exe()?;
    let profile_dir = executable
        .ancestors()
        .nth(2)
        .ok_or("relay must be in the Cargo examples directory")?;
    let mut worker =
        Command::new(profile_dir.join(format!("leselang-agent-worker{}", env::consts::EXE_SUFFIX)))
            .arg("--serve")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
    let cut = inject(&mut worker);
    // Reap the real worker on both injected cuts and fixture errors.
    let killed = worker.kill();
    let status = worker.wait()?;
    let cut = cut?;
    killed?;
    if status.success() {
        return Err("worker exited successfully before the cut".into());
    }
    let marker = format!("lese-test-cut {cut}");
    if let Some(path) = env::var_os("LESE_TEST_CUT_LOG") {
        // The .NET SDK consumes stderr privately. Its test supplies an existing
        // temporary witness file and requires a fresh marker for each cut.
        writeln!(OpenOptions::new().append(true).open(path)?, "{marker}")?;
    }
    eprintln!("{marker}");
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::from(1), // Simulated transport loss.
        Err(error) => {
            eprintln!("relay setup or protocol failure: {error}");
            ExitCode::from(2)
        }
    }
}
