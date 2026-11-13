//! A normal worker thread can discover the catalog without initializing AppKit.
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_leselang-appkit-host");

#[test]
fn public_discovery_and_cli_work_without_a_main_thread_gui() {
    let description = std::thread::spawn(|| leselang_appkit_host::describe().unwrap())
        .join()
        .unwrap();
    assert_eq!(description.operations.len(), 5);
    assert_eq!(
        description
            .operations
            .iter()
            .map(|operation| operation.name.as_str())
            .collect::<Vec<_>>(),
        [
            "gui.list",
            "gui.set_text",
            "gui.find",
            "gui.observe",
            "gui.click"
        ]
    );
    assert_eq!(description.native_types[0].name, "object");
    assert!(description.native_types[0].fields.is_empty());
    assert_eq!(description.native_types[1].name, "snapshot");
    assert_eq!(description.native_types[1].fields.len(), 8);
    let output = Command::new(BIN).arg("--describe").output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        output.stdout,
        include_bytes!(
            "../../../consumer/examples/catalog_agent/fixtures/appkit-semantics-v1.json"
        )
    );
    assert_eq!(
        output.stdout,
        leselang_appkit_host::describe_json().unwrap().as_bytes()
    );
    let text = std::str::from_utf8(&output.stdout).unwrap();
    assert_eq!(text.lines().count(), 1);
    assert!(text.ends_with('\n'));
    let mut json: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["protocol_version"], 1);
    assert_eq!(json["catalog_version"], 1);
    assert_eq!(json["profile"], "appkit");
    assert_eq!(json["operations"][0]["parameters"], serde_json::json!([]));
    assert_eq!(json["operations"][1]["required_capability"], "gui.write");
    let semantics = json.as_object_mut().unwrap().remove("semantics").unwrap();
    assert_eq!(semantics["version"], 1);
    assert_eq!(semantics["tasks"][0]["id"], "fill_and_save");
    assert_eq!(semantics["tasks"][0]["execution"], "named_text");
    assert_eq!(semantics["tasks"][0]["verification"]["max_fuel"], 1000);
    assert_eq!(semantics["operations"].as_array().unwrap().len(), 5);
    assert_eq!(
        json,
        serde_json::from_str::<serde_json::Value>(&description.to_json().unwrap()).unwrap()
    );
}

#[test]
fn describe_refuses_gui_source_and_input_flags() {
    for suffix in [
        vec!["--describe"],
        vec!["--help"],
        vec!["--demo"],
        vec!["--button-demo"],
        vec!["--self-test"],
        vec!["--hold"],
        vec!["--report-json"],
        vec!["--run", "/nonexistent.lese"],
        vec!["--entry", "main"],
        vec!["--text", "text", "value"],
    ] {
        let output = Command::new(BIN)
            .arg("--describe")
            .args(&suffix)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{suffix:?}");
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
}

#[cfg(unix)]
#[test]
fn closed_output_reports_a_possible_partial_record_without_retry() {
    use std::{
        os::{fd::OwnedFd, unix::net::UnixStream},
        process::Stdio,
    };
    let (writer, reader) = UnixStream::pair().unwrap();
    drop(reader);
    let output = Command::new(BIN)
        .arg("--describe")
        .stdout(Stdio::from(OwnedFd::from(writer)))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("output may be partial"));
}
