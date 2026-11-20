use super::source::{condition, literal};
use leselang_runtime_core::discovery::{
    FieldDescription, NativeTypeDescription, OperationDescription, ParameterDescription,
};
use leselang_runtime_core::{
    ScalarType, ScalarValue,
    discovery::{CatalogDescription, TypeDescription, semantics::*},
};
use serde_json::Value;
use serde_json::json;

fn input() -> Term {
    Term::Input {
        name: "message".into(),
    }
}
fn field() -> Term {
    Term::Field {
        binding: "completed".into(),
        name: "contents".into(),
    }
}
fn record_term(array: &str, field: &str, scalar_type: ScalarType) -> ReportTerm {
    ReportTerm::Scalar {
        path: vec![
            ReportSegment::Field { name: array.into() },
            ReportSegment::Match {
                field: "identifier".into(),
                value: "done-card".into(),
            },
            ReportSegment::Field { name: field.into() },
        ],
        scalar_type,
    }
}
fn catalog() -> Vec<u8> {
    let description = CatalogDescription {
        profile: "org.example.workflow_board".into(),
        catalog_version: 1,
        operations: vec![OperationDescription {
            name: "board.complete".into(),
            required_capability: "board.write".into(),
            parameters: vec![ParameterDescription {
                name: "message".into(),
                required: true,
                types: vec![TypeDescription::Scalar(ScalarType::String)],
            }],
            result: TypeDescription::Native("card".into()),
        }],
        native_types: vec![NativeTypeDescription {
            name: "card".into(),
            fields: vec![FieldDescription {
                name: "contents".into(),
                scalar_type: ScalarType::String,
            }],
        }],
    };
    let semantics = SemanticDescription {
        version: 1,
        operations: vec![OperationSemantics {
            operation: "board.complete".into(),
            summary: "Complete the scoped card and read its resulting contents.".into(),
            effect: EffectClass::Write,
            completion: CompletionKind::Value,
        }],
        tasks: vec![TaskDescription {
            id: "complete_card".into(),
            summary: "Complete a card and verify its contents and revision.".into(),
            inputs: vec![TaskInput {
                name: "message".into(),
                scalar_type: ScalarType::String,
            }],
            steps: vec![
                TaskStep::Call {
                    binding: "completed".into(),
                    operation: "board.complete".into(),
                    arguments: vec![TaskArgument {
                        name: "message".into(),
                        value: input(),
                    }],
                },
                TaskStep::Guard {
                    conditions: vec![Condition::Equal {
                        left: field(),
                        right: input(),
                    }],
                    failure: ScalarValue::Boolean(false),
                },
            ],
            result: Term::Literal {
                value: ScalarValue::Boolean(true),
            },
            execution: ExecutionStyle::NamedText,
            verification: ReportContract {
                kind: "board_completed".into(),
                max_fuel: 500,
                result: ScalarValue::Boolean(true),
                require_verified: true,
                predicates: vec![
                    ReportPredicate {
                        id: "card_contents".into(),
                        left: record_term("native_after", "contents", ScalarType::String),
                        relation: ReportRelation::Equal,
                        right: ReportTerm::Input {
                            name: "message".into(),
                        },
                    },
                    ReportPredicate {
                        id: "card_revision".into(),
                        left: record_term("native_before", "revision", ScalarType::Integer),
                        relation: ReportRelation::IncreasedBy { amount: 1 },
                        right: record_term("native_after", "revision", ScalarType::Integer),
                    },
                ],
            },
        }],
    };
    description
        .to_json_with_semantics(&semantics)
        .unwrap()
        .into_bytes()
}

fn report(text: &str) -> Value {
    json!({"schema_version":1,"profile":"org.example.workflow_board","kind":"board_completed",
        "result":{"type":"boolean","value":true},"effects":1,"fuel_remaining":400,"verified":true,
        "native_before":[{"identifier":"done-card","contents":"","revision":8}],
        "native_after":[{"identifier":"done-card","contents":text,"revision":9}]})
}
fn bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}
fn changed(change: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut value: Value = serde_json::from_slice(&catalog()).unwrap();
    change(&mut value);
    bytes(&value)
}

#[test]
fn an_unknown_application_task_and_operation_use_the_generic_pipeline() {
    let bytes = catalog();
    let plan = super::super::prepare(&bytes, "complete_card").unwrap();
    assert!(plan.semantic);
    assert_eq!(plan.profile, "org.example.workflow_board");
    assert_eq!(plan.input_name.as_deref(), Some("message"));
    assert_eq!(plan.execution, ExecutionStyle::NamedText);
    assert_eq!(plan.required_capabilities, ["board.write"]);
    assert_eq!(plan.expected_effects, 1);
    assert!(plan.source.contains("board.complete(message: message)"));
    assert!(!plan.source.contains("gui."));
    super::super::tests::assert_source_program_types(
        &bytes,
        &plan.source,
        ScalarType::Boolean,
        &["message"],
    );
    let text = "quote \" slash \\ newline\n保存 ✓";
    assert_eq!(
        plan.verify(&self::bytes(&report(text)), text)
            .unwrap()
            .effects,
        1
    );
    assert!(!plan.source.contains(text));
    assert!(super::super::prepare(&bytes, "fill_and_save").is_err());
    let updated = changed(|value| value["catalog_version"] = json!(2));
    assert!(
        super::super::prepare(&updated, "complete_card")
            .unwrap()
            .semantic
    );
}

#[test]
fn three_actual_host_semantic_tasks_fit_the_original_typed_compiler_limits() {
    for (catalog, task, result, parameters, effects) in [
        (
            include_bytes!("../../../../fixtures/host-semantics/appkit-semantics-v1.json")
                .as_slice(),
            "fill_and_save",
            ScalarType::Boolean,
            &["text"][..],
            9,
        ),
        (
            include_bytes!("../../../../fixtures/host-semantics/tauri-semantics-v1.json")
                .as_slice(),
            "fill_and_save",
            ScalarType::Boolean,
            &["text"][..],
            9,
        ),
        (
            include_bytes!("../../../../fixtures/host-semantics/calculator-semantics-v1.json")
                .as_slice(),
            "calculate_one_plus_two",
            ScalarType::String,
            &[][..],
            10,
        ),
    ] {
        let plan = super::super::prepare(catalog, task).unwrap();
        assert!(plan.semantic);
        assert_eq!(plan.expected_effects, effects);
        let golden = if parameters.is_empty() {
            include_str!("../../../../fixtures/host-semantics/calculator-semantics.lese")
        } else {
            include_str!("../../../../fixtures/host-semantics/form-semantics.lese")
        };
        assert_eq!(plan.source, golden);
        super::super::tests::assert_source_program_types(catalog, &plan.source, result, parameters);
    }
}

#[test]
fn semantic_metadata_errors_never_fall_back_to_a_known_profile() {
    for invalid in [Value::Null, json!({}), json!({"version":2})] {
        let bytes = changed(|value| {
            value["profile"] = json!("appkit");
            value["semantics"] = invalid;
        });
        assert!(super::super::prepare(&bytes, "complete_card").is_err());
    }
    for bytes in [
        changed(|v| v["semantics"]["version"] = json!(3)),
        changed(|v| v["semantics"]["tasks"][0]["source"] = json!("malicious source")),
        changed(|v| v["semantics"]["tasks"][0]["steps"][0]["operation"] = json!("unknown.op")),
        changed(|v| {
            v["semantics"]["tasks"][0]["steps"][0]["arguments"][0]["value"] =
                json!({"kind":"binding","name":"future"})
        }),
        changed(|v| {
            v["semantics"]["tasks"][0]["steps"][1]["conditions"][0]["left"]["name"] =
                json!("unknown_field")
        }),
        changed(|v| v["semantics"]["tasks"][0]["verification"]["predicates"] = json!([])),
        changed(|v| v["semantics"]["tasks"][0]["steps"][0]["binding"] = json!("message")),
        changed(|v| {
            v["semantics"]["tasks"][0]["steps"][1]["failure"] =
                json!({"kind":"string","value":"wrong result type"})
        }),
    ] {
        assert!(super::super::prepare(&bytes, "complete_card").is_err());
    }
    let duplicated = String::from_utf8(catalog()).unwrap().replacen(
        "\"version\":1",
        "\"version\":1,\"version\":1",
        1,
    );
    assert!(super::super::prepare(duplicated.as_bytes(), "complete_card").is_err());
}

#[test]
fn used_operations_cannot_shadow_generated_functions() {
    for reserved in ["main", "_lese_contains", "_lese_stage_1"] {
        let bytes = changed(|value| {
            value["operations"][0]["name"] = json!(reserved);
            value["semantics"]["operations"][0]["operation"] = json!(reserved);
            value["semantics"]["tasks"][0]["steps"][0]["operation"] = json!(reserved);
        });
        assert!(super::super::prepare(&bytes, "complete_card").is_err());
    }
}

#[test]
fn semantic_report_predicates_reject_missing_ambiguous_wrong_typed_or_overflowed_evidence() {
    let plan = super::super::prepare(&catalog(), "complete_card").unwrap();
    let alter = |change: fn(&mut Value)| {
        let mut value = report("done");
        change(&mut value);
        value
    };
    for invalid in [
        alter(|v| v["profile"] = json!("appkit")),
        alter(|v| v["kind"] = json!("other_kind")),
        alter(|v| v["result"]["value"] = json!(false)),
        alter(|v| v["effects"] = json!(2)),
        alter(|v| v["fuel_remaining"] = json!(501)),
        alter(|v| v["verified"] = json!(false)),
        alter(|v| {
            v["native_after"][0]
                .as_object_mut()
                .unwrap()
                .remove("contents");
        }),
        alter(|v| v["native_after"][0]["contents"] = Value::Null),
        alter(|v| v["native_after"][0]["contents"] = json!("wrong")),
        alter(|v| v["native_after"][0]["revision"] = json!("9")),
        alter(|v| v["native_after"][0]["revision"] = json!(10)),
        alter(|v| v["native_before"][0]["revision"] = json!(u64::MAX)),
        alter(|v| {
            let duplicate = v["native_after"][0].clone();
            v["native_after"].as_array_mut().unwrap().push(duplicate);
        }),
        alter(|v| v["extra"] = json!("x".repeat(4097))),
    ] {
        assert!(plan.verify(&bytes(&invalid), "done").is_err(), "{invalid}");
    }
    assert!(
        plan.verify(br#"{"schema_version":1,"schema_version":1}"#, "done")
            .is_err()
    );
    assert!(plan.verify(b"{} {}", "done").is_err());
}

#[test]
fn quoted_literals_use_the_languages_escapes_and_optional_equals_checks_presence() {
    let text = "\"\\\n\t\r中\0";
    let source = format!("fn main() = {}", literal(&ScalarValue::String(text.into())));
    assert!(leselang_syntax::parse(&source).diagnostics.is_empty());
    let generated = condition(&Condition::OptionalEquals {
        optional: Term::Field {
            binding: "observed".into(),
            name: "value".into(),
        },
        value: Term::Literal {
            value: ScalarValue::String(String::new()),
        },
    });
    assert!(generated.contains("has_value(value:"));
    assert!(generated.contains("value_or(left:"));
}

#[test]
fn declared_numeric_upper_bounds_are_inclusive_and_do_not_coerce_values() {
    let catalog = changed(|value| {
        value["semantics"]["tasks"][0]["verification"]["predicates"].as_array_mut().unwrap().push(json!({
            "id":"revision_bound",
            "left":{"kind":"scalar","path":[{"kind":"field","name":"bounded"}],"scalar_type":"integer"},
            "relation":{"kind":"at_most"},
            "right":{"kind":"literal","value":{"kind":"integer","value":i64::MAX}}
        }));
    });
    let plan = super::super::prepare(&catalog, "complete_card").unwrap();
    for (value, accepted) in [
        (json!(0), true),
        (json!(i64::MAX), true),
        (json!(i64::MAX as u64 + 1), false),
        (json!("9"), false),
        (Value::Null, false),
        (json!(-1), false),
    ] {
        let mut report = report("done");
        report["bounded"] = value;
        assert_eq!(plan.verify(&bytes(&report), "done").is_ok(), accepted);
    }
}

#[test]
fn a_new_scalar_host_executes_the_generated_two_effect_task_through_the_real_vm() {
    use leselang_vm::embedding::{Fuel, HostBuilder, Poll, parameter};
    use std::cell::Cell;
    let total = Cell::new(0u64);
    let calls = Cell::new(0);
    let mut builder = HostBuilder::new();
    builder
        .register(
            "meter.add",
            vec![parameter("amount", ScalarType::Integer)],
            ScalarType::Integer,
            "meter.write",
            |arguments, fuel| {
                fuel.charge(1).unwrap();
                let Some(ScalarValue::Integer(amount)) = arguments.get("amount") else {
                    unreachable!()
                };
                total.set(total.get().checked_add(*amount).unwrap());
                calls.set(calls.get() + 1);
                Ok(ScalarValue::Integer(total.get()))
            },
        )
        .unwrap();
    let host = builder.build();
    let number = |value| Term::Literal {
        value: ScalarValue::Integer(value),
    };
    let binding = |name: &str| Term::Binding { name: name.into() };
    let call = |name: &str, value| TaskStep::Call {
        binding: name.into(),
        operation: "meter.add".into(),
        arguments: vec![TaskArgument {
            name: "amount".into(),
            value: number(value),
        }],
    };
    let guard = |name: &str, expected| TaskStep::Guard {
        conditions: vec![Condition::Equal {
            left: binding(name),
            right: number(expected),
        }],
        failure: ScalarValue::Integer(0),
    };
    let semantics = SemanticDescription {
        version: 1,
        operations: vec![OperationSemantics {
            operation: "meter.add".into(),
            summary: "Add to a bounded host counter and return its value.".into(),
            effect: EffectClass::Write,
            completion: CompletionKind::Value,
        }],
        tasks: vec![TaskDescription {
            id: "accumulate".into(),
            summary: "Accumulate and verify 42.".into(),
            inputs: vec![],
            steps: vec![
                call("first", 19),
                guard("first", 19),
                call("second", 23),
                guard("second", 42),
            ],
            result: binding("second"),
            execution: ExecutionStyle::NoText,
            verification: ReportContract {
                kind: "meter_run".into(),
                max_fuel: 1000,
                result: ScalarValue::Integer(42),
                require_verified: false,
                predicates: vec![ReportPredicate {
                    id: "counter_value".into(),
                    left: ReportTerm::Scalar {
                        path: vec![ReportSegment::Field {
                            name: "counter".into(),
                        }],
                        scalar_type: ScalarType::Integer,
                    },
                    relation: ReportRelation::Equal,
                    right: ReportTerm::Literal {
                        value: ScalarValue::Integer(42),
                    },
                }],
            },
        }],
    };
    let catalog = host
        .catalog_description("new.vendor.meter")
        .to_json_with_semantics(&semantics)
        .unwrap();
    let plan = super::super::prepare(catalog.as_bytes(), "accumulate").unwrap();
    let grants = ["meter.write"];
    let program = host.compile(&plan.source, &grants).unwrap();
    let mut session = host.start(&program, &grants, Fuel::new(1000)).unwrap();
    for expected in [19, 42] {
        let Poll::Request(request) = session.poll().unwrap() else {
            panic!("expected explicit request")
        };
        assert_eq!(calls.get(), usize::from(expected == 42));
        let authorized = session.authorize(request).unwrap();
        let receipt = session.dispatch(authorized).unwrap();
        assert_eq!(total.get(), expected);
        assert!(matches!(session.poll().unwrap(), Poll::Awaiting));
        session.accept(receipt).unwrap();
    }
    assert!(matches!(
        session.poll().unwrap(),
        Poll::Value(ScalarValue::Integer(42))
    ));
    assert_eq!(calls.get(), 2);
    let report = json!({"schema_version":1,"profile":"new.vendor.meter","kind":"meter_run",
        "result":{"type":"integer","value":42},"effects":2,"fuel_remaining":session.fuel_remaining(),"counter":total.get()});
    assert_eq!(plan.verify(&bytes(&report), "").unwrap().effects, 2);
    assert!(session.fuel_remaining() < 1000);
}
