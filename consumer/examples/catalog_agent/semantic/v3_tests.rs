use super::super::prepare;
use leselang_runtime_core::ScalarType;
use serde_json::{Value, json};

pub(super) fn catalog() -> Vec<u8> {
    serde_json::to_vec(&json!({
        "schema_version": 1, "protocol_version": 1, "catalog_version": 1,
        "profile": "named.pair", "native_types": [],
        "operations": [{ "name": "sample.pair", "required_capability": "sample.write",
            "parameters": [
                { "name": "title", "required": true, "types": [{ "kind": "scalar", "name": "string" }] },
                { "name": "message", "required": true, "types": [{ "kind": "scalar", "name": "string" }] }
            ], "result": { "kind": "scalar", "name": "string" } }],
        "semantics": { "version": 3,
            "operations": [{ "operation": "sample.pair", "summary": "Write two named strings.", "effect": "write", "completion": "value" }],
            "tasks": [{ "id": "write_pair", "summary": "Write and independently verify both fields.",
                "inputs": [{ "name": "title", "scalar_type": "string" }, { "name": "message", "scalar_type": "string" }],
                "execution": "named_strings",
                "steps": [{ "kind": "call", "binding": "written", "operation": "sample.pair", "arguments": [
                    { "name": "title", "value": { "kind": "input", "name": "title" } },
                    { "name": "message", "value": { "kind": "input", "name": "message" } }
                ] }],
                "result": { "kind": "binding", "name": "written" },
                "verification": { "kind": "pair_report", "max_fuel": 1000, "require_verified": false,
                    "result": { "kind": "string", "value": "ok" },
                    "predicates": [
                        { "id": "title_matches", "left": { "kind": "input", "name": "title" }, "relation": { "kind": "equal" }, "right": { "kind": "path", "path": [{ "kind": "field", "name": "title" }] } },
                        { "id": "message_matches", "left": { "kind": "input", "name": "message" }, "relation": { "kind": "equal" }, "right": { "kind": "path", "path": [{ "kind": "field", "name": "message" }] } }
                    ] }
            }] }
    })).unwrap()
}
fn report() -> Vec<u8> {
    serde_json::to_vec(
        &json!({ "schema_version": 1, "profile": "named.pair", "kind": "pair_report",
        "effects": 1, "fuel_remaining": 999, "result": { "type": "string", "value": "ok" },
        "title": "Two named fields", "message": "Literal \"\\\n✓" }),
    )
    .unwrap()
}
fn arguments() -> Vec<(String, String)> {
    vec![
        ("message".into(), "Literal \"\\\n✓".into()),
        ("title".into(), "Two named fields".into()),
    ]
}

#[test]
fn named_plans_generate_sorted_entry_parameters_and_verify_every_input_independently() {
    let raw = catalog();
    let plan = prepare(&raw, "write_pair").unwrap();
    assert_eq!(plan.semantic_version, Some(3));
    assert!(plan.named_strings);
    assert!(!plan.takes_text);
    assert_eq!(plan.input_name, None);
    assert_eq!(plan.input_names, ["title", "message"]);
    assert!(
        plan.source
            .contains("fn main(message: string, title: string)")
    );
    assert_eq!(plan.expected_effects, 1);
    super::super::tests::assert_source_program_types(
        &raw,
        &plan.source,
        ScalarType::String,
        &["message", "title"],
    );
    assert_eq!(
        plan.verify_named(&report(), &arguments()).unwrap().effects,
        1
    );
    assert!(plan.verify(&report(), "ignored").is_err());
    let mut changed: Value = serde_json::from_slice(&report()).unwrap();
    for name in ["title", "message"] {
        let original = changed[name].clone();
        changed[name] = json!("wrong field");
        assert!(
            plan.verify_named(&serde_json::to_vec(&changed).unwrap(), &arguments())
                .is_err()
        );
        changed[name] = original;
    }
}

#[test]
fn named_argument_binding_rejects_missing_extra_duplicate_and_oversized_values() {
    let plan = prepare(&catalog(), "write_pair").unwrap();
    for invalid in [
        vec![],
        vec![("title".into(), "only one".into())],
        vec![
            ("title".into(), "one".into()),
            ("unknown".into(), "two".into()),
        ],
        vec![
            ("title".into(), "one".into()),
            ("title".into(), "two".into()),
        ],
        vec![
            ("title".into(), "\0".into()),
            ("message".into(), "two".into()),
        ],
        vec![
            ("title".into(), "x".repeat(4097)),
            ("message".into(), "two".into()),
        ],
    ] {
        assert!(plan.verify_named(&report(), &invalid).is_err());
    }
}

#[test]
fn unsupported_named_versions_and_legacy_multi_input_claims_never_fall_back() {
    let original: Value = serde_json::from_slice(&catalog()).unwrap();
    for version in [0, 1, 2, 4] {
        let mut invalid = original.clone();
        invalid["semantics"]["version"] = json!(version);
        assert!(prepare(&serde_json::to_vec(&invalid).unwrap(), "write_pair").is_err());
    }
    for version in [1, 2] {
        let mut invalid = original.clone();
        invalid["semantics"]["version"] = json!(version);
        invalid["semantics"]["tasks"][0]["execution"] = json!("text");
        assert!(prepare(&serde_json::to_vec(&invalid).unwrap(), "write_pair").is_err());
    }
}

#[test]
fn shared_registered_host_fixture_has_identical_source_and_typed_named_or_empty_entries() {
    let raw = include_bytes!("../../../../fixtures/host-semantics/multi-input-semantics-v3.json");
    for (task, result, parameters, effects) in [
        ("read_title", ScalarType::String, &[][..], 2),
        ("read_content", ScalarType::String, &[][..], 2),
        (
            "publish_article",
            ScalarType::Boolean,
            &["content", "title"][..],
            12,
        ),
        ("check_article", ScalarType::None, &[][..], 1),
    ] {
        let plan = prepare(raw, task).unwrap();
        assert_eq!(plan.semantic_version, Some(3));
        assert_eq!(plan.expected_effects, effects);
        assert_eq!(plan.named_strings, !parameters.is_empty());
        assert!(!plan.takes_text);
        super::super::tests::assert_source_program_types(raw, &plan.source, result, parameters);
        if plan.named_strings {
            assert_eq!(
                plan.source,
                include_str!("../../../../fixtures/host-semantics/multi-input-save.lese")
            );
            plan.validate_named_arguments(&[
                ("title".into(), "Title".into()),
                ("content".into(), "Content".into()),
            ])
            .unwrap();
        } else {
            assert!(plan.input_names.is_empty());
            plan.validate_named_arguments(&[]).unwrap();
        }
    }
}

#[test]
fn named_argument_binding_counts_total_utf8_bytes_with_an_inclusive_16_kib_limit() {
    let mut raw: Value = serde_json::from_slice(&catalog()).unwrap();
    let inputs = raw["semantics"]["tasks"][0]["inputs"]
        .as_array_mut()
        .unwrap();
    for name in ["third", "fourth", "fifth"] {
        inputs.push(json!({ "name": name, "scalar_type": "string" }));
    }
    let plan = prepare(&serde_json::to_vec(&raw).unwrap(), "write_pair").unwrap();
    let mut arguments: Vec<(String, String)> = ["title", "message", "third", "fourth"]
        .into_iter()
        .map(|name| (name.into(), "é".repeat(2048)))
        .collect();
    arguments.push(("fifth".into(), String::new()));
    assert_eq!(
        arguments
            .iter()
            .map(|(_, value)| value.len())
            .sum::<usize>(),
        16 * 1024
    );
    assert!(plan.validate_named_arguments(&arguments).is_ok());
    arguments[4].1.push('x');
    assert!(plan.validate_named_arguments(&arguments).is_err());
    assert!(plan.verify_named(&report(), &arguments).is_err());
}

#[test]
fn real_tauri_article_catalog_has_matching_sources_and_typed_named_entries() {
    let raw = include_bytes!("../../../../fixtures/host-semantics/tauri-article-semantics-v3.json");
    for (task, result, parameters, effects, golden) in [
        (
            "read_title",
            ScalarType::String,
            &[][..],
            2,
            include_str!("../../../../fixtures/host-semantics/tauri-article-read-title.lese"),
        ),
        (
            "read_content",
            ScalarType::String,
            &[][..],
            2,
            include_str!("../../../../fixtures/host-semantics/tauri-article-read-content.lese"),
        ),
        (
            "publish_article",
            ScalarType::Boolean,
            &["content", "title"][..],
            12,
            include_str!("../../../../fixtures/host-semantics/tauri-article-publish.lese"),
        ),
        (
            "check_article",
            ScalarType::None,
            &[][..],
            1,
            include_str!("../../../../fixtures/host-semantics/tauri-article-check.lese"),
        ),
    ] {
        let plan = prepare(raw, task).unwrap();
        assert_eq!(plan.profile, "tauri.article");
        assert_eq!(plan.semantic_version, Some(3));
        assert_eq!(plan.source, golden);
        assert_eq!(plan.expected_effects, effects);
        super::super::tests::assert_source_program_types(raw, &plan.source, result, parameters);
    }
}
