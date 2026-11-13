//! Report ingress failures are testable without connecting to any application.
use std::{fs, path::PathBuf, process::Command};

const BIN: &str = env!("CARGO_BIN_EXE_leselang-macos-ax-host");

struct SourceFile(PathBuf);
impl SourceFile {
    fn new(bytes: &[u8], suffix: &str) -> Self {
        use std::io::Write;
        let path = std::env::temp_dir().join(format!(
            "leselang-calculator-report-{}-{}-{suffix}.lese",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap()
            .write_all(bytes)
            .unwrap();
        Self(path)
    }
}
impl Drop for SourceFile {
    fn drop(&mut self) {
        fs::remove_file(&self.0).unwrap();
    }
}

#[test]
fn json_reporting_preserves_source_bounds_before_native_access() {
    for (bytes, reason, suffix) in [
        (
            vec![b'a'; 16385],
            "source exceeds 16384 UTF-8 bytes",
            "oversized",
        ),
        (vec![0xff], "source is not valid UTF-8", "invalid-utf8"),
    ] {
        let source = SourceFile::new(&bytes, suffix);
        let output = Command::new(BIN)
            .args(["--calculator", "--report-json", "--run"])
            .arg(&source.0)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(error.contains(reason), "{error}");
        assert!(!error.contains("Accessibility access"), "{error}");
    }
}

#[test]
fn report_flag_rejects_non_calculator_modes_before_opening_source_or_app() {
    for flags in [
        vec!["--report-json"],
        vec!["--report-json", "--document", "/nonexistent.txt"],
        vec![
            "--report-json",
            "--calculator",
            "--controls",
            "--run",
            "/nonexistent.lese",
        ],
        vec!["--report-json", "--describe", "--calculator"],
        vec![
            "--report-json",
            "--app",
            "/Applications/Nonexistent.app",
            "--windows",
        ],
    ] {
        let output = Command::new(BIN).args(&flags).output().unwrap();
        assert!(!output.status.success(), "{flags:?}");
        assert!(output.stdout.is_empty(), "{flags:?}");
        assert!(!output.stderr.is_empty(), "{flags:?}");
    }
}
