//! Offline discovery works from another Rust crate and a normal worker thread.
use leselang_macos_ax_host::discovery::{Profile, describe, describe_json};
use leselang_vm::native::core::{ScalarType, discovery::TypeDescription};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_leselang-macos-ax-host");
const PROFILES: &[(Profile, &[&str], usize)] = &[
    (Profile::TextEdit, &[], 3),
    (Profile::Calculator, &["--calculator"], 3),
    (Profile::Controls, &["--controls"], 6),
    (Profile::Widgets, &["--widgets"], 13),
    (
        Profile::WidgetsWithValueWaits,
        &["--widgets", "--wait-values"],
        14,
    ),
    (
        Profile::WidgetsWithStateWaits,
        &["--widgets", "--wait-states"],
        17,
    ),
];

#[test]
fn all_profiles_are_available_without_a_backend_on_a_worker_thread() {
    let descriptions = std::thread::spawn(|| {
        PROFILES
            .iter()
            .map(|(profile, _, _)| describe(*profile).unwrap())
            .collect::<Vec<_>>()
    })
    .join()
    .unwrap();
    for (description, (_, _, count)) in descriptions.iter().zip(PROFILES) {
        assert_eq!(description.operations.len(), *count);
        assert_eq!(
            description
                .native_types
                .iter()
                .map(|ty| ty.name.as_str())
                .collect::<Vec<_>>(),
            ["object", "snapshot"]
        );
        assert!(description.native_types[0].fields.is_empty());
        let json: serde_json::Value =
            serde_json::from_str(&description.to_json().unwrap()).unwrap();
        assert_eq!(json["schema_version"], 1);
        assert_eq!(json["protocol_version"], 1);
        assert_eq!(json["catalog_version"], 1);
        assert_eq!(json["operations"].as_array().unwrap().len(), *count);
    }
    let field = |index: usize, name: &str| {
        descriptions[index].native_types[1]
            .fields
            .iter()
            .find(|field| field.name == name)
            .map(|field| field.scalar_type)
    };
    assert_eq!(field(0, "value"), Some(ScalarType::String));
    assert_eq!(field(1, "value"), None);
    assert_eq!(
        field(1, "last_expression"),
        Some(ScalarType::OptionalString)
    );
    assert_eq!(field(2, "value"), Some(ScalarType::OptionalString));
    assert_eq!(field(2, "check_state"), None);
    assert_eq!(field(3, "check_state"), Some(ScalarType::String));
    assert_eq!(descriptions[4].operations[..13], descriptions[3].operations);
    assert_eq!(descriptions[5].operations[..14], descriptions[4].operations);
    for (operation, expected) in descriptions[5].operations[13..].iter().zip([
        ScalarType::OptionalString,
        ScalarType::Boolean,
        ScalarType::Boolean,
        ScalarType::Integer,
    ]) {
        assert_eq!(
            operation
                .parameters
                .iter()
                .map(|parameter| parameter.name.as_str())
                .collect::<Vec<_>>(),
            [
                "target",
                "expected",
                "max_attempts",
                "interval_ms",
                "timeout_ms"
            ]
        );
        assert_eq!(
            operation.parameters[0].types,
            [TypeDescription::Native("object".into())]
        );
        assert_eq!(
            operation.parameters[1].types,
            [TypeDescription::Scalar(expected)]
        );
        assert_eq!(operation.result, TypeDescription::Native("snapshot".into()));
        assert_eq!(operation.required_capability, "gui.read");
    }
}

#[test]
fn cli_emits_only_the_same_single_json_record_for_every_profile() {
    for (profile, flags, _) in PROFILES {
        let output = Command::new(BIN)
            .arg("--describe")
            .args(*flags)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        assert_eq!(output.stdout, describe_json(*profile).unwrap().as_bytes());
        let text = std::str::from_utf8(&output.stdout).unwrap();
        assert_eq!(text.lines().count(), 1);
        assert!(text.ends_with('\n'));
        let mut json: serde_json::Value = serde_json::from_str(text).unwrap();
        let semantics = json.as_object_mut().unwrap().remove("semantics");
        if *profile == Profile::Calculator {
            assert_eq!(
                output.stdout,
                include_bytes!(
                    "../../../consumer/examples/catalog_agent/fixtures/calculator-semantics-v1.json"
                )
            );
            let semantics = semantics.unwrap();
            assert_eq!(semantics["version"], 1);
            assert_eq!(semantics["tasks"][0]["id"], "calculate_one_plus_two");
            assert_eq!(semantics["tasks"][0]["execution"], "no_text");
            assert_eq!(semantics["tasks"][0]["verification"]["max_fuel"], 2000);
            assert_eq!(semantics["operations"].as_array().unwrap().len(), 3);
        } else {
            assert!(
                semantics.is_none(),
                "a catalog does not imply an application task"
            );
        }
        assert_eq!(
            json,
            serde_json::from_str::<serde_json::Value>(
                &describe(*profile).unwrap().to_json().unwrap()
            )
            .unwrap()
        );
    }
}

#[test]
fn describe_refuses_scope_execution_and_conflicting_profile_flags() {
    for suffix in [
        vec!["--describe"],
        vec!["--report-json"],
        vec!["--calculator", "--report-json"],
        vec!["--help"],
        vec!["--check-permission"],
        vec!["--prepare", "/not-created-by-discovery.txt"],
        vec!["--document", "/nonexistent.txt"],
        vec!["--run", "/nonexistent.lese"],
        vec!["--entry", "main"],
        vec!["--text", "text", "value"],
        vec!["--discover"],
        vec!["--app", "/Applications/Nonexistent.app"],
        vec!["--pid", "123"],
        vec!["--window-title", "window"],
        vec!["--window-id", "window"],
        vec!["--window-document", "/nonexistent.txt"],
        vec!["--windows"],
        vec!["--auxiliary-processes"],
        vec!["--allow-auxiliary", "123", "1", "2", "/nonexistent"],
        vec!["--calculator", "--controls"],
        vec!["--controls", "--widgets"],
        vec!["--calculator", "--widgets"],
        vec!["--wait-values"],
        vec!["--wait-states"],
        vec!["--widgets", "--wait-values", "--wait-states"],
    ] {
        let output = Command::new(BIN)
            .arg("--describe")
            .args(&suffix)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{suffix:?}");
        assert!(output.stdout.is_empty(), "{suffix:?}");
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
