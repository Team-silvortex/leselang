//! The interactive catalog remains available without a desktop or JS process.
use std::process::Command;

#[test]
fn agent_catalog_cli_is_complete_offline_and_matches_the_public_api() {
    let output = Command::new(env!("CARGO_BIN_EXE_leselang-tauri-host"))
        .arg("--describe-agent")
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        output.stdout,
        leselang_tauri_host::describe_agent().unwrap().as_bytes()
    );
    assert_eq!(output.stdout.last(), Some(&b'\n'));
    assert_eq!(
        output.stdout.iter().filter(|byte| **byte == b'\n').count(),
        1
    );
}

#[test]
fn agent_catalog_cli_refuses_execution_options_without_partial_output() {
    for args in [
        vec!["--describe-agent", "--agent"],
        vec!["--describe-agent", "--text", "literal"],
        vec!["--describe-agent", "--run", "missing"],
        vec!["--describe-agent", "--report-json"],
        vec!["--hold", "--describe-agent"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_leselang-tauri-host"))
            .args(args)
            .env("PATH", "")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
}
