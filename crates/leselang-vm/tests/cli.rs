use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

const BIN: &str = env!("CARGO_BIN_EXE_leselang");
const TWO_PRINTS: &str = r#"
fn main() = bind(first: std.print(message: "first"), body: std.print(message: "second"))
"#;

struct Fixture {
    directory: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "leselang-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        Self { directory }
    }

    fn write(&self, name: &str, source: impl AsRef<[u8]>) -> PathBuf {
        let path = self.directory.join(name);
        fs::write(&path, source).unwrap();
        path
    }

    fn command(&self) -> Command {
        let mut command = Command::new(BIN);
        command.current_dir(&self.directory);
        command
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn run_file(mode: &str, source: &str, flags: &[&str]) -> Output {
    let fixture = Fixture::new();
    let path = fixture.write("program.lese", source);
    fixture
        .command()
        .arg(mode)
        .arg(path)
        .args(flags)
        .output()
        .unwrap()
}

fn stdin_source(mode: &str, source: &[u8], flags: &[&str]) -> Output {
    let mut child = Command::new(BIN)
        .arg(mode)
        .arg("-")
        .args(flags)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(source).unwrap();
    child.wait_with_output().unwrap()
}

fn stderr(output: &Output) -> &str {
    std::str::from_utf8(&output.stderr).unwrap()
}

fn assert_code(output: &Output, code: i32) {
    assert_eq!(
        output.status.code(),
        Some(code),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        stderr(output),
    );
}

fn assert_success(output: &Output, stdout: &[u8]) {
    assert_code(output, 0);
    assert_eq!(output.stdout, stdout);
    assert!(output.stderr.is_empty(), "{}", stderr(output));
}

#[test]
fn standalone_run_compiles_pure_helpers_and_control_programs() {
    for source in [
        "fn main() = add(left: 19, right: 23)",
        "fn double(n: integer) = add(left: n, right: n)\nfn main() = double(n: 21)",
        "fn main() = choose(when: true, then: recover(value: parse_integer(value: \"bad\"), fallback: 42), otherwise: 0)",
    ] {
        assert_success(&run_file("run", source, &[]), b"42\n");
    }
}

#[test]
fn documented_examples_check_and_run_through_the_installed_command_contract() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    for (name, expected, grants) in [
        ("arithmetic.lese", "42\n", false),
        ("choice_recovery.lese", "42\n", false),
        ("bounded_loop.lese", "16\n", false),
        ("fold_strings.lese", "8\n", false),
        ("two_effects.lese", "first\nsecond\n", true),
    ] {
        let path = root.join("examples").join(name);
        for mode in ["check", "run"] {
            let mut command = Command::new(BIN);
            command.arg(mode).arg(&path);
            if grants {
                command.args(["--allow", "stdio"]);
            }
            let output = command.output().unwrap();
            assert_success(
                &output,
                if mode == "check" {
                    b""
                } else {
                    expected.as_bytes()
                },
            );
        }
    }
}

#[test]
fn scalar_results_are_unambiguous_and_none_has_no_output() {
    assert_success(&run_file("run", "fn main() = true", &[]), b"true\n");
    assert_success(&run_file("run", "fn main() = none", &[]), b"");
    let string = run_file("run", r#"fn main() = "line\n\"quoted\"\\tab\t""#, &[]);
    assert_code(&string, 0);
    assert!(string.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<String>(&string.stdout).unwrap(),
        "line\n\"quoted\"\\tab\t"
    );
    let list = run_file(
        "run",
        r#"fn main() = strings(first: "one", second: "two")"#,
        &[],
    );
    assert_code(&list, 0);
    assert!(list.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<Vec<String>>(&list.stdout).unwrap(),
        ["one", "two"]
    );
}

#[test]
fn run_performs_two_stdio_effects_only_with_the_explicit_grant() {
    assert_success(
        &run_file("run", TWO_PRINTS, &["--allow", "stdio"]),
        b"first\nsecond\n",
    );
    let denied = run_file("run", TWO_PRINTS, &[]);
    assert_code(&denied, 1);
    assert!(denied.stdout.is_empty());
    assert!(!denied.stderr.is_empty());
}

#[test]
fn check_admits_effects_without_dispatching_or_printing_the_result() {
    assert_success(&run_file("check", "fn main() = 42", &[]), b"");
    assert_success(&run_file("check", TWO_PRINTS, &["--allow", "stdio"]), b"");
    let denied = run_file("check", TWO_PRINTS, &[]);
    assert_code(&denied, 1);
    assert!(denied.stdout.is_empty());
    assert!(!denied.stderr.is_empty());
}

#[test]
fn cold_denial_unknown_operations_and_wrong_types_never_dispatch() {
    for source in [
        "fn main() = missing()",
        "fn main() = std.print(message: 1)",
        "fn main() = std.print(wrong: \"bad\")",
        "fn main() = choose(when: true, then: none, otherwise: std.print(message: \"cold\"))",
        "fn unused() = std.print(message: \"cold\")\nfn main() = 42",
    ] {
        for mode in ["check", "run"] {
            let output = run_file(mode, source, &[]);
            assert_code(&output, 1);
            assert!(output.stdout.is_empty(), "{source}");
            assert!(!output.stderr.is_empty());
        }
    }
    // With grants, rejection of the signature is independent of authority checks.
    for source in [
        "fn main() = std.print(message: 1)",
        "fn main() = std.print(wrong: \"bad\")",
    ] {
        let output = run_file("run", source, &["--allow", "stdio"]);
        assert_code(&output, 1);
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn syntax_diagnostics_identify_the_file_line_and_column() {
    let output = run_file("check", "// first line\nfn main() = @", &[]);
    assert_code(&output, 1);
    assert!(output.stdout.is_empty());
    let diagnostic = stderr(&output);
    assert!(diagnostic.contains("program.lese:2:13"), "{diagnostic}");
    assert!(diagnostic.contains("LSE0002"), "{diagnostic}");
}

#[test]
fn zero_fuel_fails_before_any_effect_and_does_not_affect_check() {
    let output = run_file("run", TWO_PRINTS, &["--allow", "stdio", "--fuel", "0"]);
    assert_code(&output, 1);
    assert!(output.stdout.is_empty());
    assert!(
        stderr(&output).to_lowercase().contains("fuel"),
        "{}",
        stderr(&output)
    );
    assert_success(&run_file("check", TWO_PRINTS, &["--allow", "stdio"]), b"");
}

#[test]
fn fuel_exhaustion_can_leave_the_first_print_committed_without_running_the_second() {
    let mut partial = None;
    // Find the observable boundary without coupling this contract to HIR charge counts.
    for budget in 1..=128 {
        let output = run_file(
            "run",
            TWO_PRINTS,
            &["--allow", "stdio", "--fuel", &budget.to_string()],
        );
        if output.stdout == b"first\n" {
            partial = Some(output);
            break;
        }
        assert!(output.stdout.is_empty() || output.stdout == b"first\nsecond\n");
    }
    let output = partial.expect("the CLI must expose the original per-session fuel boundary");
    assert_code(&output, 1);
    assert!(
        stderr(&output).to_lowercase().contains("fuel"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn runtime_scalar_failure_retains_a_committed_print() {
    let output = run_file(
        "run",
        "fn main() = bind(first: std.print(message: \"committed\"), body: parse_integer(value: \"bad\"))",
        &["--allow", "stdio"],
    );
    assert_code(&output, 1);
    assert_eq!(output.stdout, b"committed\n");
    assert!(!output.stderr.is_empty());
}

#[test]
fn unicode_source_and_stdio_messages_are_preserved() {
    let output = run_file("run", "fn main() = \"你好 🦀\"", &[]);
    assert_code(&output, 0);
    assert!(output.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<String>(&output.stdout).unwrap(),
        "你好 🦀"
    );
    assert_success(
        &run_file(
            "run",
            "fn main() = std.print(message: \"你好 🦀\")",
            &["--allow", "stdio"],
        ),
        "你好 🦀\n".as_bytes(),
    );
}

#[test]
fn input_bytes_are_bounded_and_validated_as_utf8_before_compilation() {
    let fixture = Fixture::new();
    let mut source = String::from("fn main() = 42\n//");
    source.push_str(&" ".repeat(16 * 1024 - source.len()));
    let path = fixture.write("limit.lese", &source);
    assert_success(
        &fixture.command().arg("run").arg(&path).output().unwrap(),
        b"42\n",
    );
    source.push(' ');
    fs::write(&path, source).unwrap();
    let oversized = fixture.command().arg("check").arg(&path).output().unwrap();
    assert_code(&oversized, 1);
    assert!(oversized.stdout.is_empty());
    assert!(!oversized.stderr.is_empty());

    let path = fixture.write("invalid.lese", [b'f', 0xff]);
    let invalid = fixture.command().arg("run").arg(path).output().unwrap();
    assert_code(&invalid, 1);
    assert!(invalid.stdout.is_empty());
    assert!(!invalid.stderr.is_empty());
}

#[test]
fn source_io_errors_are_reported_with_nonzero_status() {
    let fixture = Fixture::new();
    for path in [
        fixture.directory.join("missing.lese"),
        fixture.directory.clone(),
    ] {
        let output = fixture.command().arg("run").arg(&path).output().unwrap();
        assert_code(&output, 1);
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
}

#[cfg(unix)]
#[test]
fn all_diagnostic_paths_escape_filename_newlines_and_terminal_controls() {
    let fixture = Fixture::new();
    let path = fixture.write(
        "control\n\u{1b}[2J.lese",
        "fn main() = div(left: 1, right: 0)",
    );
    let assert_safe_path = |output: Output| {
        assert_code(&output, 1);
        assert!(output.stdout.is_empty());
        let diagnostic = stderr(&output);
        assert!(!diagnostic.contains('\u{1b}'), "{diagnostic:?}");
        assert!(!diagnostic.contains("control\n"), "{diagnostic:?}");
        assert!(
            diagnostic.contains("control\\n\\u{1b}[2J.lese"),
            "{diagnostic:?}"
        );
    };

    // Runtime, input I/O, and compilation must use the same safe path display.
    assert_safe_path(fixture.command().arg("run").arg(&path).output().unwrap());
    fs::remove_file(&path).unwrap();
    assert_safe_path(fixture.command().arg("run").arg(&path).output().unwrap());
    fs::write(&path, "fn main() = @").unwrap();
    assert_safe_path(fixture.command().arg("check").arg(&path).output().unwrap());
}

#[test]
fn run_and_check_accept_bounded_utf8_source_from_standard_input() {
    assert_success(&stdin_source("run", b"fn main() = 42", &[]), b"42\n");
    assert_success(
        &stdin_source("check", TWO_PRINTS.as_bytes(), &["--allow", "stdio"]),
        b"",
    );
    assert_success(
        &stdin_source("run", TWO_PRINTS.as_bytes(), &["--allow", "stdio"]),
        b"first\nsecond\n",
    );
    let invalid = stdin_source("run", &[0xff], &[]);
    assert_code(&invalid, 1);
    assert!(invalid.stdout.is_empty());
    let oversized = stdin_source("check", &vec![b' '; 16 * 1024 + 1], &[]);
    assert_code(&oversized, 1);
    assert!(oversized.stdout.is_empty());
}

#[test]
fn options_can_precede_or_follow_the_single_input_path() {
    let fixture = Fixture::new();
    let path = fixture.write("program.lese", TWO_PRINTS);
    let before = fixture
        .command()
        .args(["run", "--allow", "stdio", "--fuel", "1000"])
        .arg(&path)
        .output()
        .unwrap();
    let after = fixture
        .command()
        .arg("run")
        .arg(&path)
        .args(["--allow", "stdio", "--fuel", "1000"])
        .output()
        .unwrap();
    assert_success(&before, b"first\nsecond\n");
    assert_success(&after, b"first\nsecond\n");
}

#[test]
fn option_terminator_allows_a_path_starting_with_a_hyphen() {
    let fixture = Fixture::new();
    fixture.write("-program.lese", "fn main() = 42");
    let output = fixture
        .command()
        .args(["run", "--", "-program.lese"])
        .output()
        .unwrap();
    assert_success(&output, b"42\n");
}

#[test]
fn malformed_cli_arguments_use_the_usage_exit_code_without_reading_source() {
    let cases: &[&[&str]] = &[
        &[],
        &["unknown"],
        &["run"],
        &["check"],
        &["run", "a.lese", "b.lese"],
        &["run", "a.lese", "--unknown"],
        &["run", "a.lese", "--allow"],
        &["run", "a.lese", "--allow", "network"],
        &["run", "a.lese", "--allow", "stdio", "--allow", "stdio"],
        &["run", "a.lese", "--fuel"],
        &["run", "a.lese", "--fuel", "invalid"],
        &["run", "a.lese", "--fuel", "-1"],
        &["run", "a.lese", "--fuel", "18446744073709551616"],
        &["run", "a.lese", "--fuel", "1", "--fuel", "2"],
        &["check", "a.lese", "--fuel", "1"],
        &["run", "--"],
        &["run", "--", "a.lese", "b.lese"],
    ];
    for arguments in cases {
        let output = Command::new(BIN).args(*arguments).output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(2),
            "arguments={arguments:?}; {}",
            stderr(&output)
        );
        assert!(output.stdout.is_empty(), "arguments={arguments:?}");
        assert!(!output.stderr.is_empty(), "arguments={arguments:?}");
    }
}

#[test]
fn help_and_version_are_successful_without_source_or_effects() {
    for option in ["--help", "-h"] {
        let output = Command::new(BIN).arg(option).output().unwrap();
        assert_code(&output, 0);
        assert!(output.stderr.is_empty());
        let help = std::str::from_utf8(&output.stdout).unwrap();
        for word in ["check", "run", "--allow", "--fuel"] {
            assert!(help.contains(word), "{help}");
        }
    }
    for option in ["--version", "-V"] {
        let output = Command::new(BIN).arg(option).output().unwrap();
        assert_code(&output, 0);
        assert!(output.stderr.is_empty());
        let version = std::str::from_utf8(&output.stdout).unwrap();
        assert!(version.contains("leselang"));
        assert!(version.contains(env!("CARGO_PKG_VERSION")));
    }
}

#[cfg(unix)]
#[test]
fn non_utf8_file_paths_are_supported_without_lossy_reinterpretation() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let fixture = Fixture::new();
    let name = OsStr::from_bytes(b"source-\xff.lese");
    let path = fixture.directory.join(Path::new(name));
    // Unix argument decoding must not panic, including on filesystems such as
    // APFS that reject non-UTF-8 filenames before the runner can read them.
    let missing = fixture.command().arg("run").arg(&path).output().unwrap();
    assert_code(&missing, 1);
    assert!(missing.stdout.is_empty());
    assert!(!missing.stderr.is_empty());

    #[cfg(target_os = "linux")]
    {
        fs::write(&path, "fn main() = 42").unwrap();
        let output = fixture.command().arg("run").arg(&path).output().unwrap();
        assert_success(&output, b"42\n");
    }
}

fn json_diagnostics(output: &Output) -> Vec<serde_json::Value> {
    let records: Vec<serde_json::Value> = stderr(output)
        .lines()
        .map(|line| serde_json::from_str(line).expect("every stderr line is one JSON diagnostic"))
        .collect();
    assert!(!records.is_empty());
    for record in &records {
        assert_eq!(record["schema_version"], 1);
        assert_eq!(record["severity"], "error");
        for field in ["phase", "code", "kind", "message"] {
            assert!(record[field].is_string(), "{record}");
        }
        for field in [
            "path",
            "path_truncated",
            "span",
            "range",
            "help",
            "fuel_remaining",
        ] {
            assert!(record.get(field).is_some(), "{record}");
        }
    }
    records
}

#[test]
fn json_compile_diagnostics_share_precise_unicode_locations_with_the_embedding_api() {
    let source = "// 中文 🦀\r\nfn helper() = bind(note: \"é🦀\", body: missing.operation.deep())\r\nfn main() = 0";
    let output = run_file("check", source, &["--diagnostic-format", "json"]);
    assert_code(&output, 1);
    assert!(output.stdout.is_empty());
    let records = json_diagnostics(&output);
    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert_eq!(record["phase"], "compile");
    assert_eq!(record["kind"], "unknown_operation");
    assert_eq!(record["code"], "LSC0001");
    let host = leselang_vm::embedding::HostBuilder::new().build();
    let error = host.compile(source, &[]).err().unwrap();
    let diagnostics = leselang_vm::embedding::diagnostics::compile(&error);
    let expected = &diagnostics[0];
    let range = expected.location(source).unwrap();
    assert_eq!(record["message"], expected.message);
    assert_eq!(record["span"]["start"], expected.span.unwrap().start);
    assert_eq!(record["span"]["end"], expected.span.unwrap().end);
    assert_eq!(record["range"]["start"]["line"], range.start.line);
    assert_eq!(record["range"]["start"]["column"], range.start.column);
    assert_eq!(record["range"]["end"]["column"], range.end.column);
    assert_eq!(record["range"]["start"]["line"], 2);
    assert!(record["fuel_remaining"].is_null());
    assert!(record["help"].as_str().unwrap().contains("std.print"));
}

#[test]
fn json_syntax_diagnostics_preserve_parser_order_eof_and_byte_spans() {
    for source in ["// 中文\r\nfn main() = @ #", "fn main() =\n"] {
        let output = stdin_source("check", source.as_bytes(), &["--diagnostic-format", "json"]);
        assert_code(&output, 1);
        assert!(output.stdout.is_empty());
        let records = json_diagnostics(&output);
        let parsed = leselang_vm::native::syntax::parse(source);
        assert_eq!(records.len(), parsed.diagnostics.len());
        for (record, expected) in records.iter().zip(parsed.diagnostics) {
            assert_eq!(record["path"], "-");
            assert_eq!(record["phase"], "syntax");
            assert_eq!(record["kind"], "syntax");
            assert_eq!(record["code"], expected.code);
            assert_eq!(record["span"]["start"], expected.span.start);
            assert_eq!(record["span"]["end"], expected.span.end);
        }
        if source.ends_with('\n') {
            assert_eq!(records.last().unwrap()["range"]["start"]["line"], 2);
            assert_eq!(records.last().unwrap()["range"]["start"]["column"], 1);
        }
    }
}

#[test]
fn json_cold_admission_reports_denial_and_types_without_dispatching() {
    for source in [
        "fn main() = choose(when: false, then: std.print(message: \"hidden\"), otherwise: none)",
        "fn unused() = std.print(message: \"hidden\")\nfn main() = none",
    ] {
        for mode in ["check", "run"] {
            let output = run_file(mode, source, &["--diagnostic-format", "json"]);
            assert_code(&output, 1);
            assert!(output.stdout.is_empty());
            let records = json_diagnostics(&output);
            assert_eq!(records[0]["kind"], "capability_denied");
            assert!(
                records[0]["help"]
                    .as_str()
                    .unwrap()
                    .contains("--allow stdio")
            );
        }
    }
    let output = run_file(
        "run",
        "fn main() = std.print(message: 42)",
        &["--diagnostic-format", "json", "--allow", "stdio"],
    );
    assert_code(&output, 1);
    assert!(output.stdout.is_empty());
    let records = json_diagnostics(&output);
    assert_eq!(records[0]["phase"], "compile");
    assert_eq!(records[0]["kind"], "compilation");
    assert!(records[0]["help"].is_null());
}

#[test]
fn diagnostic_format_does_not_change_success_values_effects_or_check_behavior() {
    for format in ["text", "json"] {
        let flags = ["--diagnostic-format", format, "--allow", "stdio"];
        assert_success(&run_file("check", TWO_PRINTS, &flags), b"");
        assert_success(&run_file("run", TWO_PRINTS, &flags), b"first\nsecond\n");
        assert_success(&run_file("run", "fn main() = 42", &flags), b"42\n");
    }
}

#[test]
fn json_runtime_failure_preserves_partial_effects_and_reports_original_fuel() {
    let source =
        "fn main() = bind(first: std.print(message: \"committed\"), body: div(left: 1, right: 0))";
    let output = run_file(
        "run",
        source,
        &["--diagnostic-format", "json", "--allow", "stdio"],
    );
    assert_code(&output, 1);
    assert_eq!(output.stdout, b"committed\n");
    let records = json_diagnostics(&output);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["phase"], "runtime");
    assert_eq!(records[0]["code"], "LSR0001");
    assert!(records[0]["span"].is_null());
    assert!(records[0]["range"].is_null());
    assert!(records[0]["fuel_remaining"].as_u64().unwrap() < 10_000);

    let output = run_file(
        "run",
        TWO_PRINTS,
        &[
            "--diagnostic-format",
            "json",
            "--allow",
            "stdio",
            "--fuel",
            "0",
        ],
    );
    assert_code(&output, 1);
    assert!(output.stdout.is_empty());
    let records = json_diagnostics(&output);
    assert_eq!(records[0]["fuel_remaining"], 0);
    assert!(records[0]["message"].as_str().unwrap().contains("fuel"));
}

#[test]
fn json_input_failures_have_no_invented_source_location() {
    for source in [vec![0xff], vec![b' '; 16 * 1024 + 1]] {
        let output = stdin_source("check", &source, &["--diagnostic-format", "json"]);
        assert_code(&output, 1);
        assert!(output.stdout.is_empty());
        let records = json_diagnostics(&output);
        assert_eq!(records[0]["phase"], "input");
        assert_eq!(records[0]["code"], "LSI0001");
        assert!(records[0]["span"].is_null());
        assert!(records[0]["range"].is_null());
        assert!(records[0]["fuel_remaining"].is_null());
    }
    let fixture = Fixture::new();
    let output = fixture
        .command()
        .args(["check", "--diagnostic-format", "json", "missing.lese"])
        .output()
        .unwrap();
    assert_code(&output, 1);
    assert_eq!(json_diagnostics(&output)[0]["path"], "missing.lese");
}

#[test]
fn json_usage_errors_follow_the_explicitly_parsed_format_and_option_terminator() {
    for tail in [
        vec![],
        vec!["--unknown"],
        vec!["--fuel", "invalid", "unread.lese"],
        vec!["--diagnostic-format", "json", "unread.lese"],
        vec!["--diagnostic-format", "text", "unread.lese"],
        vec!["--help"],
    ] {
        let output = Command::new(BIN)
            .args(["run", "--diagnostic-format", "json"])
            .args(tail)
            .output()
            .unwrap();
        assert_code(&output, 2);
        assert!(output.stdout.is_empty());
        let records = json_diagnostics(&output);
        assert_eq!(records[0]["phase"], "usage");
        assert_eq!(records[0]["code"], "LSU0001");
        assert!(records[0]["path"].is_null());
    }
    for args in [
        vec!["check", "--diagnostic-format"],
        vec!["check", "--diagnostic-format", "xml"],
        vec!["run", "--unknown", "--diagnostic-format", "json"],
    ] {
        let output = Command::new(BIN).args(args).output().unwrap();
        assert_code(&output, 2);
        assert!(stderr(&output).starts_with("error:"));
    }
    let fixture = Fixture::new();
    fixture.write("--diagnostic-format", "fn main() = 42");
    let output = fixture
        .command()
        .args(["run", "--", "--diagnostic-format"])
        .output()
        .unwrap();
    assert_success(&output, b"42\n");
}

#[cfg(unix)]
#[test]
fn json_paths_round_trip_without_literal_terminal_controls_or_extra_lines() {
    let fixture = Fixture::new();
    let path = fixture.write(
        "line\n\u{1b}[2J\u{202e}.lese",
        "fn main() = div(left: 1, right: 0)",
    );
    for (mode, remove) in [("run", false), ("check", true)] {
        if remove {
            fs::remove_file(&path).unwrap();
        }
        let output = fixture
            .command()
            .arg(mode)
            .arg(&path)
            .args(["--diagnostic-format", "json"])
            .output()
            .unwrap();
        assert_code(&output, 1);
        let records = json_diagnostics(&output);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["path"], path.to_string_lossy().as_ref());
        assert_eq!(records[0]["path_truncated"], false);
        assert!(!stderr(&output).contains('\u{1b}'));
        assert!(!stderr(&output).contains('\u{202e}'));
    }
}
