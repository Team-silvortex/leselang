//! Explicit macOS desktop acceptance: a real process and real WebView are
//! necessary to catch event loops that exit before Rust main can return.
#![cfg(all(target_os = "macos", feature = "desktop"))]

use serde_json::Value;
use std::{
    ffi::OsStr,
    fs::{File, OpenOptions},
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct SourceFile(PathBuf);

impl SourceFile {
    fn create(source: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "leselang-tauri-exit-{}-{}.lese",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut file: File = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .unwrap();
        file.write_all(source.as_bytes()).unwrap();
        Self(path)
    }
}

impl Drop for SourceFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn run(args: &[&OsStr]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_leselang-tauri-host"))
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("launch the real Tauri host");
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let output = child.wait_with_output().unwrap();
            panic!(
                "native host exceeded its startup/execution deadlines: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn report(output: &Output, text: &str, expected: bool) {
    let stdout = std::str::from_utf8(&output.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 1, "one complete JSON record");
    assert!(stdout.ends_with('\n'));
    let report: Value = serde_json::from_str(stdout).unwrap();
    assert_eq!(report["result"]["value"], expected);
    assert_eq!(report["verified"], expected);
    assert_eq!(report["effects"], 9);
    let after = report["native_after"].as_array().unwrap();
    let saved = after
        .iter()
        .find(|item| item["identifier"] == "save")
        .unwrap();
    assert_eq!(saved["value"], text);
    assert_eq!(
        saved["clicks"], 1,
        "exactly one real Save, including on failure"
    );
    let other = after
        .iter()
        .find(|item| item["identifier"] == "other")
        .unwrap();
    assert_eq!(other["clicks"], 0);
}

#[test]
#[ignore = "opens two real WebViews; requires an unlocked macOS GUI session"]
fn native_cli_exit_status_tracks_verified_result_after_actual_side_effects() {
    let text = "Native exit regression ✓";
    let success = run(&["--text".as_ref(), text.as_ref(), "--report-json".as_ref()]);
    report(&success, text, true);
    assert!(
        success.status.success(),
        "{}",
        String::from_utf8_lossy(&success.stderr)
    );

    // Execute and verify the complete reference interaction first, then make
    // the program's final predicate false without undoing its actual Save.
    let source = include_str!("../examples/form.lese");
    assert_eq!(source.matches("fn main(text: string)").count(), 1);
    let source = source.replace("fn main(text: string)", "fn verified_form(text: string)")
        + "\nfn main(text: string) = bind(done: verified_form(text: text), body: false)\n";
    let source = SourceFile::create(&source);
    let failure = run(&[
        "--run".as_ref(),
        source.0.as_os_str(),
        "--text".as_ref(),
        text.as_ref(),
        "--report-json".as_ref(),
    ]);
    report(&failure, text, false);
    assert_eq!(
        failure.status.code(),
        Some(1),
        "false verification must fail the process"
    );
    assert!(
        String::from_utf8_lossy(&failure.stderr).contains("did not finish with a verified result")
    );
}
