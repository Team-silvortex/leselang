//! Invalid report requests are rejected before creating an AppKit GUI.
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_leselang-appkit-host");

#[test]
fn report_conflicts_fail_with_empty_stdout_without_native_resources() {
    for arguments in [
        vec!["--report-json"],
        vec!["--report-json", "--demo"],
        vec!["--report-json", "--button-demo"],
        vec!["--report-json", "--self-test"],
        vec!["--report-json", "--describe"],
        vec!["--report-json", "--help"],
        vec!["--report-json", "--run", "/nonexistent.lese", "--hold"],
        vec![
            "--run",
            "/nonexistent.lese",
            "--report-json",
            "--report-json",
        ],
    ] {
        let output = Command::new(BIN).args(&arguments).output().unwrap();
        assert!(!output.status.success(), "{arguments:?}");
        assert!(output.stdout.is_empty(), "{arguments:?}");
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(error.contains("--report-json"), "{arguments:?}: {error}");
    }
}

#[test]
fn a_missing_source_has_no_success_report() {
    let output = Command::new(BIN)
        .args([
            "--report-json",
            "--run",
            "/nonexistent-leselang-report-source.lese",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
}
