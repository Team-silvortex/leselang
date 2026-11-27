use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
};

fn next(stdout: &mut impl BufRead) -> Value {
    let mut line = String::new();
    assert!(
        stdout.read_line(&mut line).unwrap() > 0,
        "worker closed early"
    );
    serde_json::from_str(&line).unwrap()
}

fn run(profile: bool, version: u32, reject_at: Option<u32>) -> Value {
    let mut worker = Command::new(env!("CARGO_BIN_EXE_leselang-agent-worker"));
    if profile {
        worker.arg("--profile-gui");
    }
    let mut child = worker
        .arg("--serve")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    if profile {
        assert_eq!(
            next(&mut output),
            json!({"agent_worker_version":3,"kind":"profile_ready"})
        );
    }
    let scope = json!({"window":"test","epoch":"one","session":"benchmark"});
    let start = json!({"agent_worker_version":version,"kind":"start","scope":scope,
        "source":"fn main() = gui.list()","arguments":{},"input_mode":"single_string"});
    writeln!(input, "{start}").unwrap();
    let mut requests = 0;
    let terminal = loop {
        let frame = next(&mut output);
        if frame["kind"] != "request" {
            break frame;
        }
        requests += 1;
        let request = &frame["request"];
        assert_eq!(request["operation"], "gui.list");
        assert_eq!(request["request_id"], requests);
        let deny = Some(requests) == reject_at;
        let reply = json!({"agent_worker_version":3,"kind":"reply","challenge":frame["challenge"],
            "reply":{"transport_version":1,"request_id":requests,"window":scope["window"],"epoch":scope["epoch"],
                     "session":scope["session"],"status":if deny {"rejected"} else {"ok"},
                     "value":if deny {Value::Null} else {json!([])}, "error":deny.then_some("no_read")}});
        writeln!(input, "{reply}").unwrap();
    };
    assert!(
        output.fill_buf().unwrap().is_empty(),
        "output after terminal"
    );
    assert_eq!(
        child.wait().unwrap().success(),
        terminal["kind"] == "report"
    );
    if terminal["kind"] == "report" {
        if profile {
            assert_eq!(terminal["worker_profile"]["exchange_count"], requests);
        } else {
            assert_eq!(terminal.as_object().unwrap().len(), 3);
        }
    }
    terminal
}

#[test]
fn opt_in_profile_counts_bootstrap_and_vm_exchanges_without_changing_the_task_report() {
    for reject_at in [None, Some(1), Some(2)] {
        let ordinary = run(false, 3, reject_at);
        let measured = run(true, 3, reject_at);
        assert_eq!(ordinary["kind"], "report");
        assert_eq!(measured["kind"], "report");
        assert_eq!(measured["report_json"], ordinary["report_json"]);
        let profile = &measured["worker_profile"];
        assert_eq!(profile.as_object().unwrap().len(), 4);
        assert!(profile["compile_ns"].as_u64().unwrap() > 0);
        assert!(profile["exchange_ns"].as_u64().unwrap() > 0);
        assert_eq!(
            profile["vm_local_ns"].as_u64().unwrap() > 0,
            reject_at != Some(1)
        );
        assert_eq!(
            profile["exchange_count"],
            if reject_at == Some(1) { 1 } else { 2 }
        );
    }
}

#[test]
fn profile_mode_rejects_other_worker_versions_before_dispatch() {
    for version in [1, 2, 4] {
        let frame = run(true, version, None);
        assert_eq!(
            frame,
            json!({"agent_worker_version":3,"kind":"error","error":"GUI profiling requires worker version three"})
        );
    }
}
