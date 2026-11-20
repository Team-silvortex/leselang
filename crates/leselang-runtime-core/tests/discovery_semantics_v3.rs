use leselang_runtime_core::{
    ScalarType, ScalarValue,
    discovery::{semantics::*, *},
};
use serde_json::{Value, json};

fn fixture() -> (CatalogDescription, v3::SemanticDescription) {
    let string = TypeDescription::Scalar(ScalarType::String);
    let catalog = CatalogDescription {
        profile: "named.example".into(),
        catalog_version: 1,
        native_types: vec![],
        operations: vec![OperationDescription {
            name: "sample.pair".into(),
            required_capability: "sample.write".into(),
            result: string.clone(),
            parameters: ["title", "message"]
                .into_iter()
                .map(|name| ParameterDescription {
                    name: name.into(),
                    required: true,
                    types: vec![string.clone()],
                })
                .collect(),
        }],
    };
    let description = v3::SemanticDescription {
        version: 3,
        operations: vec![OperationSemantics {
            operation: "sample.pair".into(),
            summary: "Write the named pair.".into(),
            effect: EffectClass::Write,
            completion: CompletionKind::Value,
        }],
        tasks: vec![v3::TaskDescription {
            id: "write_pair".into(),
            summary: "Write and independently verify both named strings.".into(),
            inputs: ["title", "message"]
                .into_iter()
                .map(|name| TaskInput {
                    name: name.into(),
                    scalar_type: ScalarType::String,
                })
                .collect(),
            execution: v3::ExecutionStyle::NamedStrings,
            steps: vec![TaskStep::Call {
                binding: "written".into(),
                operation: "sample.pair".into(),
                arguments: ["title", "message"]
                    .into_iter()
                    .map(|name| TaskArgument {
                        name: name.into(),
                        value: Term::Input { name: name.into() },
                    })
                    .collect(),
            }],
            result: Term::Binding {
                name: "written".into(),
            },
            verification: v2::ReportContract {
                kind: "pair_report".into(),
                max_fuel: 1000,
                result: v2::ResultContract::Exact(ScalarValue::String("ok".into())),
                require_verified: false,
                predicates: ["title", "message"]
                    .into_iter()
                    .map(|name| ReportPredicate {
                        id: format!("{name}_matches"),
                        left: ReportTerm::Input { name: name.into() },
                        relation: ReportRelation::Equal,
                        right: ReportTerm::Path {
                            path: vec![ReportSegment::Field { name: name.into() }],
                        },
                    })
                    .collect(),
            },
        }],
    };
    (catalog, description)
}

#[test]
fn named_inputs_round_trip_without_changing_base_or_legacy_public_enums() {
    let (catalog, description) = fixture();
    let base = catalog.to_json().unwrap();
    let raw = catalog.to_json_with_semantics_v3(&description).unwrap();
    let value: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["protocol_version"], 1);
    assert_eq!(
        value["semantics"],
        serde_json::to_value(&description).unwrap()
    );
    assert_eq!(
        serde_json::from_value::<v3::SemanticDescription>(value["semantics"].clone()).unwrap(),
        description
    );
    assert_eq!(catalog.to_json().unwrap(), base);
    // An exhaustive match remains source-compatible after adding v3.
    let legacy = match ExecutionStyle::Text {
        ExecutionStyle::Text => "text",
        ExecutionStyle::NamedText => "named_text",
        ExecutionStyle::NoText => "no_text",
    };
    assert_eq!(legacy, "text");
    assert!(serde_json::from_value::<v2::SemanticDescription>(value["semantics"].clone()).is_err());
    assert!(serde_json::from_value::<SemanticDescription>(value["semantics"].clone()).is_err());
}

#[test]
fn earlier_versions_still_refuse_multiple_inputs_with_their_original_execution_styles() {
    let (catalog, description) = fixture();
    let mut value = serde_json::to_value(description).unwrap();
    value["tasks"][0]["execution"] = json!("text");
    value["version"] = json!(2);
    let v2: v2::SemanticDescription = serde_json::from_value(value.clone()).unwrap();
    assert!(catalog.to_json_with_semantics_v2(&v2).is_err());
    value["version"] = json!(1);
    let v1: SemanticDescription = serde_json::from_value(value).unwrap();
    assert!(catalog.to_json_with_semantics(&v1).is_err());
}

#[test]
fn named_input_bounds_names_types_and_execution_conventions_are_strict() {
    let (catalog, valid) = fixture();
    let mut eight = valid.clone();
    for index in 2..8 {
        eight.tasks[0].inputs.push(TaskInput {
            name: format!("extra_{index}"),
            scalar_type: ScalarType::String,
        });
    }
    assert!(catalog.to_json_with_semantics_v3(&eight).is_ok());
    eight.tasks[0].inputs.push(TaskInput {
        name: "ninth".into(),
        scalar_type: ScalarType::String,
    });
    assert!(catalog.to_json_with_semantics_v3(&eight).is_err());
    for bad in ["title", "", "body", "_lese_hidden", "has space", "标题"] {
        let mut invalid = valid.clone();
        invalid.tasks[0].inputs[1].name = bad.into();
        assert!(
            catalog.to_json_with_semantics_v3(&invalid).is_err(),
            "{bad}"
        );
    }
    for ty in [
        ScalarType::Integer,
        ScalarType::Boolean,
        ScalarType::None,
        ScalarType::OptionalString,
        ScalarType::StringList,
    ] {
        let mut invalid = valid.clone();
        invalid.tasks[0].inputs[0].scalar_type = ty;
        assert!(catalog.to_json_with_semantics_v3(&invalid).is_err());
    }
    let mut invalid = valid.clone();
    invalid.tasks[0].execution = v3::ExecutionStyle::NoText;
    assert!(catalog.to_json_with_semantics_v3(&invalid).is_err());
    invalid.tasks[0].inputs.clear();
    invalid.tasks[0].execution = v3::ExecutionStyle::NamedStrings;
    assert!(catalog.to_json_with_semantics_v3(&invalid).is_err());
    for version in [0, 1, 2, 4] {
        let mut invalid = valid.clone();
        invalid.version = version;
        assert!(catalog.to_json_with_semantics_v3(&invalid).is_err());
    }
}

#[test]
fn named_inputs_are_resolved_in_calls_guards_results_and_each_report_predicate() {
    let (catalog, valid) = fixture();
    let mut invalid = valid.clone();
    invalid.tasks[0].verification.predicates[1].left = ReportTerm::Input {
        name: "missing".into(),
    };
    assert!(catalog.to_json_with_semantics_v3(&invalid).is_err());
    invalid = valid.clone();
    invalid.tasks[0].result = Term::Input {
        name: "missing".into(),
    };
    assert!(catalog.to_json_with_semantics_v3(&invalid).is_err());
    invalid = valid.clone();
    if let TaskStep::Call { arguments, .. } = &mut invalid.tasks[0].steps[0] {
        arguments[1].value = Term::Input {
            name: "missing".into(),
        };
    }
    assert!(catalog.to_json_with_semantics_v3(&invalid).is_err());
    invalid = valid;
    if let TaskStep::Call { binding, .. } = &mut invalid.tasks[0].steps[0] {
        *binding = "title".into();
    }
    assert!(catalog.to_json_with_semantics_v3(&invalid).is_err());
}

#[test]
fn v3_no_input_tasks_keep_explicit_no_text_execution() {
    let (catalog, mut description) = fixture();
    let task = &mut description.tasks[0];
    task.inputs.clear();
    task.execution = v3::ExecutionStyle::NoText;
    if let TaskStep::Call { arguments, .. } = &mut task.steps[0] {
        for argument in arguments {
            argument.value = Term::Literal {
                value: ScalarValue::String("fixed".into()),
            };
        }
    }
    for predicate in &mut task.verification.predicates {
        predicate.left = ReportTerm::Literal {
            value: ScalarValue::String("fixed".into()),
        };
    }
    let value: Value =
        serde_json::from_str(&catalog.to_json_with_semantics_v3(&description).unwrap()).unwrap();
    assert_eq!(value["semantics"]["tasks"][0]["execution"], "no_text");
    assert_eq!(value["semantics"]["tasks"][0]["inputs"], json!([]));
    description.tasks[0].execution = v3::ExecutionStyle::NamedStrings;
    assert!(catalog.to_json_with_semantics_v3(&description).is_err());
}
