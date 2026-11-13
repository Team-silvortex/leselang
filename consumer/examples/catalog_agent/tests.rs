use super::*;

#[test]
fn arguments_are_explicit_and_task_text_remains_data() {
    let parse = |args: &[&str]| arguments(args.iter().map(OsString::from));
    assert_eq!(parse(&["--help"]).unwrap(), None);
    let options = parse(&["--host", "host", "--output", "new", "--text", "--execute"])
        .unwrap()
        .unwrap();
    assert!(!options.execute);
    assert_eq!(options.text.as_deref(), Some("--execute"));
    for args in [
        vec![],
        vec!["--help", "--execute"],
        vec![
            "--host",
            "host",
            "--output",
            "new",
            "--text",
            "x",
            "--execute",
            "--execute",
        ],
        vec![
            "--host", "host", "--host", "other", "--output", "new", "--text", "x",
        ],
        vec!["--host", "host", "--output", "", "--text", "x"],
        vec!["--host", "host", "--output", "new", "--text", "a\0b"],
    ] {
        assert!(parse(&args).is_err(), "{args:?}");
    }
    assert!(
        parse(&[
            "--host",
            "host",
            "--output",
            "new",
            "--text",
            &"a".repeat(4096)
        ])
        .is_ok()
    );
    assert!(
        parse(&[
            "--host",
            "host",
            "--output",
            "new",
            "--text",
            &"a".repeat(4097)
        ])
        .is_err()
    );
}

#[test]
fn calculator_task_and_literal_host_selectors_are_explicit_and_bounded() {
    let parse = |args: &[&str]| arguments(args.iter().map(OsString::from));
    let base = [
        "--host",
        "host",
        "--output",
        "new",
        "--task",
        "calculate_one_plus_two",
    ];
    let mut args = base.to_vec();
    args.extend(["--host-arg", "--calculator", "--execute"]);
    let options = parse(&args).unwrap().unwrap();
    assert_eq!(options.task, "calculate_one_plus_two");
    assert!(options.text.is_none());
    assert_eq!(options.host_args, vec![OsString::from("--calculator")]);
    for suffix in [
        vec!["--task", "fill_and_save"],
        vec!["--host-arg"],
        vec!["--host-arg", ""],
        vec!["--host-arg", "--run"],
        vec!["--host-arg", "--describe"],
        vec!["--host-arg", "--text=hidden"],
        vec!["--host-arg", "a\0b"],
        vec!["--task", "bad.task"],
    ] {
        let mut args = base.to_vec();
        args.extend(suffix);
        assert!(parse(&args).is_err(), "{args:?}");
    }
    let mut args = base.to_vec();
    for _ in 0..8 {
        args.extend(["--host-arg", "$(literal selector)"]);
    }
    assert_eq!(parse(&args).unwrap().unwrap().host_args.len(), 8);
    args.extend(["--host-arg", "ninth"]);
    assert!(parse(&args).is_err());
    let long = "x".repeat(1025);
    assert!(
        parse(&[
            "--host",
            "host",
            "--output",
            "new",
            "--task",
            "calculate_one_plus_two",
            "--host-arg",
            &long
        ])
        .is_err()
    );
}

#[test]
fn task_identifiers_are_open_but_bounded_and_inputs_wait_for_catalog_validation() {
    let parse = |task: &str| {
        arguments(["--host", "host", "--output", "new", "--task", task].map(OsString::from))
    };
    let options = parse("publish_note").unwrap().unwrap();
    assert_eq!(options.task, "publish_note");
    assert_eq!(options.text, None);
    for name in [
        "",
        "fn",
        "body",
        "a.b",
        "a-b",
        "0task",
        "_lese_stage_0",
        "a\0b",
        &"x".repeat(65),
    ] {
        assert!(parse(name).is_err(), "{name:?}");
    }
    assert!(parse(&"x".repeat(64)).is_ok());
}

#[cfg(unix)]
mod subprocess {
    use super::*;
    use std::{
        os::unix::fs::PermissionsExt,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let directory = std::env::temp_dir().join(format!(
                "leselang-catalog-agent-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&directory).unwrap();
            let fixture = Self(directory);
            fixture.script(
                r##"#!/bin/sh
if [ "$1" = "--describe" ]; then
  printf 'describe\n' >> "$0.calls"
  cat "$0.catalog"
  exit 0
fi
printf 'run\n' >> "$0.calls"
printf '%s\0' "$@" > "$0.args"
cat "$0.report"
printf 'fixture trace\n' >&2
exit "$(cat "$0.exit")"
"##,
            );
            fixture.write("host.catalog", include_bytes!("fixtures/appkit-v1.json"));
            fixture.write("host.exit", b"0");
            fixture
        }
        fn script(&self, source: &str) {
            self.write("host", source.as_bytes());
            fs::set_permissions(self.0.join("host"), fs::Permissions::from_mode(0o700)).unwrap();
        }
        fn write(&self, name: &str, bytes: &[u8]) {
            fs::write(self.0.join(name), bytes).unwrap();
        }
        fn report(&self, text: &str) {
            let before = json!([
                {"identifier":"save","role":"button","title":"draft","value":"","editable":false,"enabled":true,"clicks":0},
                {"identifier":"other","role":"button","title":"draft","value":"","editable":false,"enabled":true,"clicks":0},
                {"identifier":"input","role":"text_field","title":"Message to save","value":"","editable":true,"enabled":true,"clicks":0}
            ]);
            let mut after = before.clone();
            after[0]["title"] = json!("ready");
            after[0]["value"] = json!(text);
            after[0]["clicks"] = json!(1);
            after[2]["value"] = json!(text);
            after.as_array_mut().unwrap().reverse();
            let record = json!({"schema_version":1,"kind":"appkit_run","profile":"appkit",
                "result":{"type":"boolean","value":true},"effects":9,"fuel_remaining":100,
                "native_before":before,"native_after":after});
            let mut bytes = serde_json::to_vec(&record).unwrap();
            bytes.push(b'\n');
            self.write("host.report", &bytes);
        }
        fn options(&self, execute: bool, text: &str) -> Options {
            Options {
                host: self.0.join("host"),
                host_args: Vec::new(),
                output: self.0.join("artifacts"),
                task: "fill_and_save".into(),
                text: Some(text.into()),
                execute,
            }
        }
        fn calls(&self) -> String {
            fs::read_to_string(self.0.join("host.calls")).unwrap()
        }
        fn result(&self) -> serde_json::Value {
            serde_json::from_slice(&fs::read(self.0.join("artifacts/verification.json")).unwrap())
                .unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn offline_plan_uses_the_real_catalog_fixture_and_never_executes() {
        let fixture = Fixture::new();
        let path = run(fixture.options(false, "保存 ✓")).unwrap();
        assert_eq!(fixture.calls(), "describe\n");
        assert_eq!(fixture.result()["status"], "planned");
        assert_eq!(fixture.result()["execution_started"], false);
        assert!(path.join("program.lese").is_file());
        assert!(!path.join("run.json").exists());
        let source = fs::read_to_string(path.join("program.lese")).unwrap();
        assert!(!source.contains("保存"));
        assert!(source.contains("text: string"));
        assert!(leselang_syntax::parse(&source).diagnostics.is_empty());
        assert!(
            run(fixture.options(true, "x")).is_err(),
            "existing artifacts cannot be reused"
        );
        assert_eq!(fixture.calls(), "describe\n");
    }

    #[test]
    fn explicit_run_uses_one_source_and_literal_argv_then_verifies_native_evidence() {
        let fixture = Fixture::new();
        let text = "保存 \"quoted\" \\ $(no shell)\n--execute";
        fixture.report(text);
        let path = run(fixture.options(true, text)).unwrap();
        assert_eq!(fixture.calls(), "describe\nrun\n");
        assert_eq!(fixture.result()["status"], "verified");
        assert_eq!(fixture.result()["effects"], 9);
        let bytes = fs::read(fixture.0.join("host.args")).unwrap();
        let args: Vec<_> = bytes
            .split(|byte| *byte == 0)
            .filter(|arg| !arg.is_empty())
            .collect();
        assert_eq!(args.len(), 8);
        assert_eq!(args[0], b"--run");
        assert_eq!(
            args[1],
            path.join("program.lese").as_os_str().as_encoded_bytes()
        );
        assert_eq!(args[6], text.as_bytes());
        assert_eq!(args[7], b"--report-json");
        assert!(
            !fs::read_to_string(path.join("program.lese"))
                .unwrap()
                .contains(text)
        );
        assert_eq!(
            fs::read(path.join("run.stderr")).unwrap(),
            b"fixture trace\n"
        );
    }

    #[test]
    fn typescript_host_uses_the_selected_profile_cli_and_independent_state_checks() {
        let fixture = Fixture::new();
        fixture.report("TypeScript ✓");
        let mut catalog: serde_json::Value =
            serde_json::from_slice(include_bytes!("fixtures/appkit-v1.json")).unwrap();
        catalog["profile"] = json!("tauri.typescript");
        fixture.write("host.catalog", &serde_json::to_vec(&catalog).unwrap());
        let mut report: serde_json::Value =
            serde_json::from_slice(&fs::read(fixture.0.join("host.report")).unwrap()).unwrap();
        report["kind"] = json!("tauri_run");
        report["profile"] = json!("tauri.typescript");
        report["verified"] = json!(true);
        report["fuel_remaining"] = json!(9000);
        for field in ["native_before", "native_after"] {
            for snapshot in report[field].as_array_mut().unwrap() {
                if snapshot["identifier"] == "save" {
                    snapshot["title"] = json!("Save");
                }
            }
        }
        fixture.write("host.report", &serde_json::to_vec(&report).unwrap());
        run(fixture.options(true, "TypeScript ✓")).unwrap();
        assert_eq!(fixture.calls(), "describe\nrun\n");
        assert_eq!(fixture.result()["status"], "verified");
        let bytes = fs::read(fixture.0.join("host.args")).unwrap();
        let args: Vec<_> = bytes
            .split(|byte| *byte == 0)
            .filter(|arg| !arg.is_empty())
            .collect();
        assert_eq!(args.len(), 5);
        assert_eq!(args[2], b"--text");
        assert_eq!(args[3], "TypeScript ✓".as_bytes());
        assert_eq!(args[4], b"--report-json");
    }

    fn calculator_fixture() -> Fixture {
        let fixture = Fixture::new();
        fixture.script(
            r##"#!/bin/sh
selector="$1"
[ "$selector" = "--calculator" ] || exit 20
shift
if [ "$1" = "--describe" ]; then
  printf 'describe\n' >> "$0.calls"
  printf '%s\0' "$selector" "$@" > "$0.describe-args"
  cat "$0.catalog"
  exit 0
fi
printf 'run\n' >> "$0.calls"
printf '%s\0' "$selector" "$@" > "$0.args"
cat "$0.report"
exit "$(cat "$0.exit")"
"##,
        );
        fixture.write(
            "host.catalog",
            include_bytes!("fixtures/calculator-v1.json"),
        );
        let before = json!([{"identifier":"calculator","role":"AXWindow","display":"0","last_expression":null,
            "buttons":[{"name":"one","enabled":true},{"name":"add","enabled":true},{"name":"two","enabled":true},{"name":"equals","enabled":true}]}]);
        let mut after = before.clone();
        after[0]["display"] = json!("3");
        after[0]["last_expression"] = json!("1 + 2");
        fixture.write("host.report", &serde_json::to_vec(&json!({
            "schema_version":1,"kind":"macos_ax_calculator_run","profile":"macos_ax.calculator",
            "result":{"type":"string","value":"3"},"effects":10,"fuel_remaining":500,"verified":true,
            "native_before":before,"native_after":after
        })).unwrap());
        fixture
    }

    #[test]
    fn same_agent_forwards_calculator_selector_for_discovery_and_execution() {
        for execute in [false, true] {
            let fixture = calculator_fixture();
            let mut options = fixture.options(execute, "");
            options.host_args.push("--calculator".into());
            options.task = "calculate_one_plus_two".into();
            options.text = None;
            let directory = run(options).unwrap();
            assert_eq!(
                fixture.calls(),
                if execute {
                    "describe\nrun\n"
                } else {
                    "describe\n"
                }
            );
            assert_eq!(
                fs::read(fixture.0.join("host.describe-args")).unwrap(),
                b"--calculator\0--describe\0"
            );
            let plan: serde_json::Value =
                serde_json::from_slice(&fs::read(directory.join("plan.json")).unwrap()).unwrap();
            assert_eq!(plan["task"], "calculate_one_plus_two");
            assert_eq!(plan["arguments"], json!({}));
            assert_eq!(plan["expected_effects"], 10);
            assert_eq!(plan["host_arguments"], json!(["--calculator"]));
            if execute {
                let bytes = fs::read(fixture.0.join("host.args")).unwrap();
                let args: Vec<_> = bytes
                    .split(|byte| *byte == 0)
                    .filter(|arg| !arg.is_empty())
                    .collect();
                assert_eq!(args.len(), 4);
                assert_eq!(args[0], b"--calculator");
                assert_eq!(args[1], b"--run");
                assert_eq!(
                    args[2],
                    directory
                        .join("program.lese")
                        .as_os_str()
                        .as_encoded_bytes()
                );
                assert_eq!(args[3], b"--report-json");
                assert_eq!(fixture.result()["status"], "verified");
                assert_eq!(fixture.result()["effects"], 10);
            }
        }
    }

    #[test]
    fn a_requested_task_cannot_override_the_discovered_application_contract() {
        for calculator in [false, true] {
            let fixture = if calculator {
                calculator_fixture()
            } else {
                Fixture::new()
            };
            let mut options = fixture.options(true, "x");
            if calculator {
                options.host_args.push("--calculator".into());
            } else {
                options.task = "calculate_one_plus_two".into();
                options.text = None;
            }
            assert!(run(options).is_err());
            assert_eq!(fixture.calls(), "describe\n");
            assert_eq!(fixture.result()["phase"], "planning");
            assert_eq!(fixture.result()["execution_started"], false);
            assert!(!fixture.0.join("artifacts/program.lese").exists());
        }
    }

    #[test]
    fn calculator_nonzero_or_wrong_expression_is_failure_without_replay() {
        for case in 0..2 {
            let fixture = calculator_fixture();
            if case == 0 {
                fixture.write("host.exit", b"1");
            } else {
                let mut report: serde_json::Value =
                    serde_json::from_slice(&fs::read(fixture.0.join("host.report")).unwrap())
                        .unwrap();
                report["native_after"][0]["last_expression"] = json!("0 + 3");
                fixture.write("host.report", &serde_json::to_vec(&report).unwrap());
            }
            let mut options = fixture.options(true, "");
            options.host_args.push("--calculator".into());
            options.task = "calculate_one_plus_two".into();
            options.text = None;
            assert!(run(options).is_err());
            assert_eq!(fixture.calls(), "describe\nrun\n");
            assert_eq!(fixture.result()["status"], "failed");
            assert_eq!(fixture.result()["possible_partial_effects"], true);
        }
    }

    fn rename_note_values(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::String(text) => {
                let renamed = match text.as_str() {
                    "appkit" => "thirdparty.notes",
                    "appkit_run" => "notes_run",
                    "fill_and_save" => "publish_note",
                    "input" => "draft",
                    "save" => "publish",
                    "other" => "archive",
                    "text" => "message",
                    "gui.list" => "notes.enumerate",
                    "gui.find" => "notes.find",
                    "gui.observe" => "notes.read",
                    "gui.set_text" => "notes.replace",
                    "gui.click" => "notes.submit",
                    _ => return,
                };
                *text = renamed.into();
            }
            serde_json::Value::Array(items) => items.iter_mut().for_each(rename_note_values),
            serde_json::Value::Object(fields) => {
                for (key, value) in fields {
                    // "input" is also a semantic vocabulary tag, not an app id.
                    if key != "kind" || value != "input" {
                        rename_note_values(value);
                    }
                }
            }
            _ => {}
        }
    }

    fn declared_note_fixture(text: &str) -> Fixture {
        let fixture = Fixture::new();
        let mut catalog: serde_json::Value =
            serde_json::from_slice(include_bytes!("fixtures/appkit-semantics-v1.json")).unwrap();
        rename_note_values(&mut catalog);
        fixture.write("host.catalog", &serde_json::to_vec(&catalog).unwrap());
        fixture.report(text);
        let mut report: serde_json::Value =
            serde_json::from_slice(&fs::read(fixture.0.join("host.report")).unwrap()).unwrap();
        rename_note_values(&mut report);
        fixture.write("host.report", &serde_json::to_vec(&report).unwrap());
        fixture
    }

    #[test]
    fn an_unrecognized_app_task_operations_and_controls_use_the_same_agent() {
        let input = "quoted \"\\\n保存 ✓";
        let fixture = declared_note_fixture(input);
        let mut options = fixture.options(true, input);
        options.task = "publish_note".into();
        let directory = run(options).unwrap();
        assert_eq!(fixture.calls(), "describe\nrun\n");
        assert_eq!(fixture.result()["status"], "verified");
        let plan: serde_json::Value =
            serde_json::from_slice(&fs::read(directory.join("plan.json")).unwrap()).unwrap();
        assert_eq!(plan["profile"], "thirdparty.notes");
        assert_eq!(plan["task"], "publish_note");
        assert_eq!(plan["planning_contract"], "catalog_semantics_v1");
        assert_eq!(plan["arguments"], json!({"message":input}));
        assert_eq!(plan["expected_effects"], 9);
        let source = fs::read_to_string(directory.join("program.lese")).unwrap();
        assert!(source.contains("notes.replace"));
        assert!(source.contains("notes.submit"));
        assert!(!source.contains("gui."));
        assert!(!source.contains(input));
        let bytes = fs::read(fixture.0.join("host.args")).unwrap();
        let argv: Vec<_> = bytes
            .split(|byte| *byte == 0)
            .filter(|arg| !arg.is_empty())
            .collect();
        assert_eq!(
            argv[2..],
            [
                b"--entry".as_slice(),
                b"main",
                b"--text",
                b"message",
                input.as_bytes(),
                b"--report-json"
            ]
        );
    }

    #[test]
    fn malformed_present_semantics_never_falls_back_to_a_known_profile() {
        for extension in [serde_json::Value::Null, json!({"version":999})] {
            let fixture = Fixture::new();
            let mut catalog: serde_json::Value =
                serde_json::from_slice(include_bytes!("fixtures/appkit-v1.json")).unwrap();
            catalog["semantics"] = extension;
            fixture.write("host.catalog", &serde_json::to_vec(&catalog).unwrap());
            assert!(run(fixture.options(true, "x")).is_err());
            assert_eq!(fixture.calls(), "describe\n");
            assert_eq!(fixture.result()["phase"], "planning");
            assert_eq!(fixture.result()["possible_partial_effects"], false);
            assert!(!fixture.0.join("artifacts/program.lese").exists());
        }
    }

    #[test]
    fn input_presence_is_checked_against_the_selected_task_before_execution() {
        for calculator in [false, true] {
            let fixture = if calculator {
                calculator_fixture()
            } else {
                declared_note_fixture("x")
            };
            let mut options = fixture.options(true, "x");
            if calculator {
                options.host_args.push("--calculator".into());
                options.task = "calculate_one_plus_two".into();
            } else {
                options.task = "publish_note".into();
                options.text = None;
            }
            assert!(run(options).is_err());
            assert_eq!(fixture.calls(), "describe\n");
            assert_eq!(fixture.result()["phase"], "planning");
            assert_eq!(fixture.result()["execution_started"], false);
            assert!(!fixture.0.join("artifacts/program.lese").exists());
        }
    }

    #[test]
    fn a_declared_report_postcondition_is_enforced_without_replay() {
        let fixture = declared_note_fixture("x");
        let mut report: serde_json::Value =
            serde_json::from_slice(&fs::read(fixture.0.join("host.report")).unwrap()).unwrap();
        report["native_after"][0]["clicks"] = json!(2);
        fixture.write("host.report", &serde_json::to_vec(&report).unwrap());
        let mut options = fixture.options(true, "x");
        options.task = "publish_note".into();
        assert!(run(options).is_err());
        assert_eq!(fixture.calls(), "describe\nrun\n");
        assert_eq!(fixture.result()["phase"], "verification");
        assert_eq!(fixture.result()["possible_partial_effects"], true);
    }

    #[test]
    fn incompatible_catalog_stops_before_source_or_execution() {
        let fixture = Fixture::new();
        let mut catalog: serde_json::Value =
            serde_json::from_slice(include_bytes!("fixtures/appkit-v1.json")).unwrap();
        catalog["profile"] = json!("unreviewed.gui");
        fixture.write("host.catalog", &serde_json::to_vec(&catalog).unwrap());
        assert!(run(fixture.options(true, "x")).is_err());
        assert_eq!(fixture.calls(), "describe\n");
        assert_eq!(fixture.result()["phase"], "planning");
        assert_eq!(fixture.result()["possible_partial_effects"], false);
        assert!(!fixture.0.join("artifacts/program.lese").exists());
    }

    #[test]
    fn nonzero_exit_and_false_or_incomplete_evidence_are_terminal_without_retry() {
        for case in 0..5 {
            let fixture = Fixture::new();
            fixture.report("x");
            match case {
                0 => fixture.write("host.exit", b"1"),
                1 => {
                    let mut report: serde_json::Value =
                        serde_json::from_slice(&fs::read(fixture.0.join("host.report")).unwrap())
                            .unwrap();
                    report["result"]["value"] = json!(false);
                    fixture.write("host.report", &serde_json::to_vec(&report).unwrap());
                }
                2 => fixture.write("host.report", b"{\"schema_version\":1"),
                3 => fixture.write("host.report", b"{}\n{}\n"),
                4 => fixture.write("host.report", &vec![b' '; REPORT_LIMIT + 1]),
                _ => unreachable!(),
            }
            assert!(run(fixture.options(true, "x")).is_err(), "case {case}");
            assert_eq!(fixture.calls(), "describe\nrun\n");
            assert_eq!(fixture.result()["status"], "failed");
            assert_eq!(fixture.result()["possible_partial_effects"], true);
            assert!(fixture.0.join("artifacts/run.json").is_file());
        }
    }

    #[test]
    fn direct_child_capture_bounds_both_pipes_and_stops_at_a_deadline() {
        for case in 0..3 {
            let fixture = Fixture::new();
            fixture.script(match case {
                0 => "#!/bin/sh\nprintf prefix\nprintf trace >&2\nexec sleep 5\n",
                1 => "#!/bin/sh\ncat \"$0.catalog\"\n",
                _ => "#!/bin/sh\ncat \"$0.catalog\" >&2\n",
            });
            if case == 2 {
                fixture.write("host.catalog", &vec![b'x'; 128 * 1024 + 1]);
            }
            let before = std::time::Instant::now();
            let captured =
                process::capture(&fixture.0.join("host"), &[], 32, Duration::from_secs(1)).unwrap();
            assert!(captured.require_success().is_err());
            assert!(before.elapsed() < Duration::from_secs(3));
            assert!(captured.stdout.len() <= 33);
            assert!(captured.stderr.len() <= 128 * 1024 + 1);
            if case == 0 {
                assert_eq!(captured.stdout, b"prefix");
                assert_eq!(captured.stderr, b"trace");
            }
        }
    }

    #[test]
    fn evidence_files_are_create_only_and_do_not_overwrite_existing_bytes() {
        let fixture = Fixture::new();
        fixture.write("sentinel", b"original");
        assert!(save(&fixture.0, "sentinel", b"replacement").is_err());
        assert_eq!(fs::read(fixture.0.join("sentinel")).unwrap(), b"original");
    }
}
