use super::scalar_name;
use leselang_runtime_core::{
    ScalarType, ScalarValue,
    discovery::{CatalogDescription, TypeDescription, semantics::*},
};
use serde_json::Value;
use serde_json::json;

fn field(name: &str) -> ReportSegment {
    ReportSegment::Field { name: name.into() }
}
fn evidence_path() -> Vec<ReportSegment> {
    vec![
        field("native_after"),
        ReportSegment::Match {
            field: "identifier".into(),
            value: "readout".into(),
        },
        field("value"),
    ]
}
fn semantics(ty: ScalarType) -> v2::SemanticDescription {
    v2::SemanticDescription {
        version: 2,
        operations: vec![OperationSemantics {
            operation: "sample.observe".into(),
            summary: "Read the current scalar.".into(),
            effect: EffectClass::Read,
            completion: CompletionKind::Value,
        }],
        tasks: vec![v2::TaskDescription {
            id: "read_value".into(),
            summary: "Observe the value and preserve application state.".into(),
            inputs: vec![],
            execution: ExecutionStyle::NoText,
            steps: vec![TaskStep::Call {
                binding: "observed".into(),
                operation: "sample.observe".into(),
                arguments: vec![],
            }],
            result: Term::Binding {
                name: "observed".into(),
            },
            verification: v2::ReportContract {
                kind: "sample_read".into(),
                max_fuel: 1000,
                result: v2::ResultContract::Observed {
                    scalar_type: ty,
                    path: evidence_path(),
                },
                require_verified: false,
                predicates: vec![ReportPredicate {
                    id: "preserved".into(),
                    left: ReportTerm::Path {
                        path: vec![field("native_before")],
                    },
                    relation: ReportRelation::Equal,
                    right: ReportTerm::Path {
                        path: vec![field("native_after")],
                    },
                }],
            },
        }],
    }
}
fn catalog(ty: ScalarType) -> Vec<u8> {
    CatalogDescription {
        profile: "sample.observation".into(),
        catalog_version: 1,
        operations: vec![leselang_runtime_core::discovery::OperationDescription {
            name: "sample.observe".into(),
            parameters: vec![],
            result: TypeDescription::Scalar(ty),
            required_capability: "sample.read".into(),
        }],
        native_types: vec![],
    }
    .to_json_with_semantics_v2(&semantics(ty))
    .unwrap()
    .into_bytes()
}
fn report(ty: ScalarType, value: Value) -> Value {
    json!({ "schema_version":1, "profile":"sample.observation", "kind":"sample_read", "effects":1,"fuel_remaining":900,
        "result":{"type":scalar_name(ty),"value":value},
        "native_before":[{"identifier":"readout","value":value}], "native_after":[{"identifier":"readout","value":value}] })
}
fn bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}
fn prepare(catalog: &[u8]) -> Result<super::super::PreparedPlan, String> {
    super::super::prepare(catalog, "read_value")
}

#[test]
fn exact_v2_tasks_keep_the_original_source_and_report_rules() {
    for (raw, task) in [
        (
            include_bytes!("../../../../fixtures/host-semantics/appkit-semantics-v1.json")
                .as_slice(),
            "fill_and_save",
        ),
        (
            include_bytes!("../../../../fixtures/host-semantics/tauri-semantics-v1.json")
                .as_slice(),
            "fill_and_save",
        ),
        (
            include_bytes!("../../../../fixtures/host-semantics/calculator-semantics-v1.json")
                .as_slice(),
            "calculate_one_plus_two",
        ),
    ] {
        let old = super::super::prepare(raw, task).unwrap();
        let mut upgraded: Value = serde_json::from_slice(raw).unwrap();
        upgraded["semantics"]["version"] = json!(2);
        let current = super::super::prepare(&bytes(&upgraded), task).unwrap();
        assert_eq!(current.source, old.source);
        assert_eq!(current.expected_effects, old.expected_effects);
        assert_eq!(current.required_capabilities, old.required_capabilities);
        assert_eq!(old.semantic_version, Some(1));
        assert_eq!(current.semantic_version, Some(2));
        assert!(current.verify(b"{}", "anything").is_err());
    }
}

#[test]
fn one_catalog_accepts_different_actual_scalars_without_changing_generated_source() {
    for (ty, values) in [
        (
            ScalarType::String,
            vec![
                json!(""),
                json!("first"),
                json!("second \"\\\n✓"),
                json!("x".repeat(4096)),
            ],
        ),
        (
            ScalarType::Integer,
            vec![
                json!(0),
                json!(3),
                json!(9_007_199_254_740_993u64),
                json!(u64::MAX),
            ],
        ),
        (ScalarType::Boolean, vec![json!(false), json!(true)]),
    ] {
        let raw = catalog(ty);
        let plan = prepare(&raw).unwrap();
        super::super::tests::assert_source_program_types(&raw, &plan.source, ty, &[]);
        for value in values {
            assert_eq!(
                plan.verify(&bytes(&report(ty, value)), "").unwrap().effects,
                1
            );
            assert_eq!(prepare(&raw).unwrap().source, plan.source);
        }
    }
}

#[test]
fn reports_require_actual_typed_matching_evidence_and_preserve_all_other_checks() {
    let plan = prepare(&catalog(ScalarType::String)).unwrap();
    let valid = report(ScalarType::String, json!("current"));
    let changes: &[fn(&mut Value)] = &[
        |v| v["result"]["value"] = json!("predicted"),
        |v| v["result"]["type"] = json!("integer"),
        |v| v["native_after"][0]["value"] = json!("different"),
        |v| {
            v["native_after"][0]
                .as_object_mut()
                .unwrap()
                .remove("value");
        },
        |v| v["native_after"] = json!([]),
        |v| v["native_after"] = json!([v["native_after"][0].clone(), v["native_after"][0].clone()]),
        |v| v["native_after"] = json!({"value":"current"}),
        |v| v["native_before"][0]["value"] = json!("changed"),
        |v| v["effects"] = json!(0),
        |v| v["effects"] = json!(2),
        |v| v["fuel_remaining"] = json!(1001),
        |v| v["fuel_remaining"] = json!(-1),
        |v| v["profile"] = json!("another"),
        |v| v["kind"] = json!("another"),
        |v| v["schema_version"] = json!(2),
        |v| v["result"]["value"] = json!("x".repeat(4097)),
    ];
    for change in changes {
        let mut invalid = valid.clone();
        change(&mut invalid);
        assert!(plan.verify(&bytes(&invalid), "").is_err(), "{invalid}");
    }
    for (ty, invalids) in [
        (
            ScalarType::String,
            vec![Value::Null, json!(1), json!(true), json!([])],
        ),
        (
            ScalarType::Integer,
            vec![json!("3"), json!(-1), json!(1.0), json!(1.5), Value::Null],
        ),
        (
            ScalarType::Boolean,
            vec![json!("false"), json!(0), json!(1), Value::Null],
        ),
    ] {
        let plan = prepare(&catalog(ty)).unwrap();
        for invalid in invalids {
            assert!(plan.verify(&bytes(&report(ty, invalid)), "").is_err());
        }
    }
    let raw = String::from_utf8(bytes(&valid)).unwrap();
    for invalid in [
        raw.replace(
            "\"value\":\"current\"",
            "\"value\":\"current\",\"value\":\"current\"",
        ),
        raw.replace("\"result\":", "\"result\":{},\"result\":"),
        format!("{raw} {{}}"),
    ] {
        assert!(plan.verify(invalid.as_bytes(), "").is_err());
    }
    let mut declared: Value = serde_json::from_slice(&catalog(ScalarType::String)).unwrap();
    declared["semantics"]["tasks"][0]["verification"]["require_verified"] = json!(true);
    let strict = prepare(&bytes(&declared)).unwrap();
    assert!(strict.verify(&bytes(&valid), "").is_err());
    let mut verified = valid;
    verified["verified"] = json!(true);
    assert!(strict.verify(&bytes(&verified), "").is_ok());
}

#[test]
fn unsupported_or_malformed_semantics_fail_during_planning_without_fallback() {
    let valid: Value = serde_json::from_slice(&catalog(ScalarType::String)).unwrap();
    let changes: &[fn(&mut Value)] = &[
        |v| v["semantics"]["version"] = json!(1),
        |v| v["semantics"]["version"] = json!(3),
        |v| v["semantics"]["version"] = json!(2.0),
        |v| v["semantics"]["version"] = json!("2"),
        |v| v["semantics"]["tasks"][0]["verification"]["result"]["path"] = json!([]),
        |v| {
            v["semantics"]["tasks"][0]["verification"]["result"]["path"] =
                json!([{"kind":"field","name":"result"},{"kind":"field","name":"value"}])
        },
        |v| {
            v["semantics"]["tasks"][0]["verification"]["result"]["path"] =
                json!([{"kind":"match","field":"id","value":"x"}])
        },
        |v| {
            v["semantics"]["tasks"][0]["verification"]["result"]["path"] =
                json!(vec![json!({"kind":"field","name":"evidence"}); 9])
        },
        |v| v["semantics"]["tasks"][0]["verification"]["result"]["scalar_type"] = json!("none"),
        |v| {
            v["semantics"]["tasks"][0]["verification"]["result"]["scalar_type"] =
                json!("optional_string")
        },
        |v| {
            v["semantics"]["tasks"][0]["verification"]["result"]["scalar_type"] =
                json!("string_list")
        },
        |v| v["semantics"]["tasks"][0]["verification"]["result"]["scalar_type"] = json!("boolean"),
        |v| v["semantics"]["tasks"][0]["verification"]["result"]["fallback"] = json!("x"),
        |v| {
            v["semantics"]["tasks"][0]["verification"]["result"]
                .as_object_mut()
                .unwrap()
                .remove("path");
        },
        |v| v["semantics"]["tasks"][0]["source"] = json!("attacker controlled source"),
        |v| v["semantics"]["tasks"][0]["verification"]["predicates"] = json!([]),
    ];
    for change in changes {
        let mut invalid = valid.clone();
        change(&mut invalid);
        assert!(prepare(&bytes(&invalid)).is_err(), "{invalid}");
    }
    let raw = String::from_utf8(bytes(&valid)).unwrap();
    for invalid in [
        raw.replace("\"version\":2", "\"version\":2,\"version\":2"),
        raw.replace(
            "\"scalar_type\":\"string\"",
            "\"scalar_type\":\"string\",\"scalar_type\":\"string\"",
        ),
    ] {
        assert!(prepare(invalid.as_bytes()).is_err());
    }
}

#[test]
fn real_vm_reads_changed_state_with_the_same_catalog_and_compiled_program() {
    use leselang_vm::embedding::{Fuel, HostBuilder, Poll};
    use std::cell::Cell;
    let actual = Cell::new(7u64);
    let effects = Cell::new(0);
    let mut builder = HostBuilder::new();
    builder
        .register(
            "sample.observe",
            vec![],
            ScalarType::Integer,
            "sample.read",
            |_, fuel| {
                fuel.charge(1).unwrap();
                effects.set(effects.get() + 1);
                Ok(ScalarValue::Integer(actual.get()))
            },
        )
        .unwrap();
    let host = builder.build();
    let raw = host
        .catalog_description("sample.observation")
        .to_json_with_semantics_v2(&semantics(ScalarType::Integer))
        .unwrap();
    let plan = prepare(raw.as_bytes()).unwrap();
    let grants = ["sample.read"];
    let program = host.compile(&plan.source, &grants).unwrap();
    for (index, current) in [7, 9_007_199_254_740_993, u64::MAX].into_iter().enumerate() {
        actual.set(current);
        let mut session = host.start(&program, &grants, Fuel::new(1000)).unwrap();
        let Poll::Request(request) = session.poll().unwrap() else {
            panic!("expected an explicit request")
        };
        assert_eq!(effects.get(), index);
        let authorized = session.authorize(request).unwrap();
        let receipt = session.dispatch(authorized).unwrap();
        assert!(matches!(session.poll().unwrap(), Poll::Awaiting));
        session.accept(receipt).unwrap();
        let Poll::Value(ScalarValue::Integer(observed)) = session.poll().unwrap() else {
            panic!("expected the current scalar")
        };
        assert_eq!(observed, current);
        assert_eq!(effects.get(), index + 1);
        let mut evidence = report(ScalarType::Integer, json!(observed));
        evidence["fuel_remaining"] = json!(session.fuel_remaining());
        assert_eq!(plan.verify(&bytes(&evidence), "").unwrap().effects, 1);
        evidence["result"]["value"] = json!(current - 1);
        assert!(plan.verify(&bytes(&evidence), "").is_err());
        assert_eq!(prepare(raw.as_bytes()).unwrap().source, plan.source);
    }
}

#[test]
fn real_tauri_v2_catalog_generates_the_same_observation_source_as_the_sdk() {
    let raw = include_bytes!("../../../../fixtures/host-semantics/tauri-agent-semantics-v2.json");
    let plan = super::super::prepare(raw, "read_message").unwrap();
    assert_eq!(plan.semantic_version, Some(2));
    assert_eq!(plan.expected_effects, 2);
    assert_eq!(plan.required_capabilities, ["gui.read"]);
    assert_eq!(
        plan.source,
        include_str!("../../../../fixtures/host-semantics/tauri-read-message.lese")
    );
    super::super::tests::assert_source_program_types(raw, &plan.source, ScalarType::String, &[]);
}

#[test]
fn javascript_registered_task_builder_emits_ordinary_cross_language_semantics() {
    let raw =
        include_bytes!("../../../../fixtures/host-semantics/javascript-agent-semantics-v2.json");
    for (id, source, result, parameters, effects) in [
        (
            "read_draft",
            include_str!("../../../../fixtures/host-semantics/javascript-read-draft.lese"),
            ScalarType::String,
            &[][..],
            2,
        ),
        (
            "publish_article",
            include_str!("../../../../fixtures/host-semantics/javascript-publish-article.lese"),
            ScalarType::Boolean,
            &["text"][..],
            9,
        ),
        (
            "read_publication",
            include_str!("../../../../fixtures/host-semantics/javascript-read-publication.lese"),
            ScalarType::String,
            &[][..],
            2,
        ),
        (
            "check_publication",
            include_str!("../../../../fixtures/host-semantics/javascript-check-publication.lese"),
            ScalarType::None,
            &[][..],
            1,
        ),
    ] {
        let plan = super::super::prepare(raw, id).unwrap();
        assert_eq!(plan.semantic_version, Some(2));
        assert_eq!(plan.expected_effects, effects);
        assert_eq!(plan.source, source);
        super::super::tests::assert_source_program_types(raw, &plan.source, result, parameters);
    }
}
