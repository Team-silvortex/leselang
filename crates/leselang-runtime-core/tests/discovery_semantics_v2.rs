use leselang_runtime_core::{
    ScalarType, ScalarValue,
    discovery::{
        semantics::{self, *},
        *,
    },
};
use serde_json::{Value, json};

fn field(name: &str) -> ReportSegment {
    ReportSegment::Field { name: name.into() }
}
fn path() -> Vec<ReportSegment> {
    vec![
        field("native_after"),
        ReportSegment::Match {
            field: "identifier".into(),
            value: "readout".into(),
        },
        field("value"),
    ]
}
fn fixture(value: ScalarValue) -> (CatalogDescription, SemanticDescription) {
    let ty = value.scalar_type();
    let catalog = CatalogDescription {
        profile: "sample.observation".into(),
        catalog_version: 1,
        operations: vec![OperationDescription {
            name: "sample.observe".into(),
            parameters: vec![],
            result: TypeDescription::Scalar(ty),
            required_capability: "sample.read".into(),
        }],
        native_types: vec![],
    };
    let semantics = SemanticDescription {
        version: 1,
        operations: vec![OperationSemantics {
            operation: "sample.observe".into(),
            summary: "Read a scalar value.".into(),
            effect: EffectClass::Read,
            completion: CompletionKind::Value,
        }],
        tasks: vec![TaskDescription {
            id: "read_value".into(),
            summary: "Read and verify the observed scalar.".into(),
            inputs: vec![],
            steps: vec![TaskStep::Call {
                binding: "observed".into(),
                operation: "sample.observe".into(),
                arguments: vec![],
            }],
            result: Term::Binding {
                name: "observed".into(),
            },
            execution: ExecutionStyle::NoText,
            verification: ReportContract {
                kind: "sample_read".into(),
                max_fuel: 100,
                result: value,
                require_verified: false,
                predicates: vec![ReportPredicate {
                    id: "unchanged".into(),
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
    };
    (catalog, semantics)
}
fn observed(value: ScalarValue) -> (CatalogDescription, v2::SemanticDescription) {
    let ty = value.scalar_type();
    let (catalog, semantics) = fixture(value);
    let mut semantics = v2::SemanticDescription::from(semantics);
    semantics.tasks[0].verification.result = v2::ResultContract::Observed {
        scalar_type: ty,
        path: path(),
    };
    (catalog, semantics)
}

#[test]
fn v1_sources_and_bytes_stay_unchanged_and_exact_v2_values_keep_their_shape() {
    // Empty vectors still infer the original public structs without annotations.
    let empty = SemanticDescription {
        version: 1,
        operations: vec![],
        tasks: vec![],
    };
    assert_eq!(empty.version, 1);
    for value in [
        ScalarValue::String("quote \"\\\n✓".into()),
        ScalarValue::Integer(u64::MAX),
        ScalarValue::Boolean(false),
        ScalarValue::None,
    ] {
        let (catalog, old) = fixture(value.clone());
        let original = catalog.to_json_with_semantics(&old).unwrap();
        let upgraded = v2::SemanticDescription::from(old.clone());
        assert_eq!(
            upgraded.tasks[0].verification.result,
            v2::ResultContract::Exact(value)
        );
        let wire = catalog.to_json_with_semantics_v2(&upgraded).unwrap();
        assert_eq!(
            wire,
            original.replace(
                "\"semantics\":{\"version\":1,",
                "\"semantics\":{\"version\":2,"
            )
        );
        assert_eq!(catalog.to_json_with_semantics(&old).unwrap(), original);
        let decoded: Value = serde_json::from_str(&wire).unwrap();
        assert_eq!(decoded["schema_version"], 1);
        assert_eq!(decoded["protocol_version"], 1);
        assert_eq!(
            decoded["semantics"],
            serde_json::to_value(&upgraded).unwrap()
        );
        assert_eq!(
            upgraded,
            serde_json::from_value(decoded["semantics"].clone()).unwrap()
        );
        assert!(
            serde_json::from_value::<semantics::SemanticDescription>(decoded["semantics"].clone())
                .unwrap()
                .validate(&catalog)
                .is_err()
        );
    }
}

#[test]
fn observed_scalars_round_trip_and_the_encoder_matches_serde() {
    for value in [
        ScalarValue::String("anything".into()),
        ScalarValue::Integer(0),
        ScalarValue::Boolean(true),
    ] {
        let (catalog, semantics) = observed(value);
        semantics.validate(&catalog).unwrap();
        let wire = catalog.to_json_with_semantics_v2(&semantics).unwrap();
        let decoded: Value = serde_json::from_str(&wire).unwrap();
        assert_eq!(
            decoded["semantics"],
            serde_json::to_value(&semantics).unwrap()
        );
        assert_eq!(
            decoded["semantics"]["tasks"][0]["verification"]["result"]["kind"],
            "observed"
        );
        assert_eq!(
            semantics,
            serde_json::from_value(decoded["semantics"].clone()).unwrap()
        );
        assert!(
            serde_json::from_value::<SemanticDescription>(decoded["semantics"].clone()).is_err()
        );
    }
}

#[test]
fn observation_paths_are_bounded_nonempty_typed_and_cannot_select_the_result() {
    let (catalog, valid) = observed(ScalarValue::String(String::new()));
    let paths = [
        vec![],
        vec![field("result")],
        vec![field("result"), field("value")],
        vec![ReportSegment::Match {
            field: "identifier".into(),
            value: "x".into(),
        }],
        vec![field("")],
        vec![field(&"x".repeat(129))],
        vec![field("evidence"); 9],
        vec![
            field("evidence"),
            ReportSegment::Match {
                field: "".into(),
                value: "x".into(),
            },
        ],
        vec![
            field("evidence"),
            ReportSegment::Match {
                field: "id".into(),
                value: "x".repeat(4097),
            },
        ],
    ];
    for path in paths {
        let mut invalid = valid.clone();
        invalid.tasks[0].verification.result = v2::ResultContract::Observed {
            scalar_type: ScalarType::String,
            path,
        };
        assert!(invalid.validate(&catalog).is_err());
        assert!(catalog.to_json_with_semantics_v2(&invalid).is_err());
    }
    for scalar_type in [
        ScalarType::None,
        ScalarType::OptionalString,
        ScalarType::StringList,
        ScalarType::Integer,
    ] {
        let mut invalid = valid.clone();
        invalid.tasks[0].verification.result = v2::ResultContract::Observed {
            scalar_type,
            path: path(),
        };
        assert!(invalid.validate(&catalog).is_err());
    }
    let mut boundary = valid.clone();
    boundary.tasks[0].verification.result = v2::ResultContract::Observed {
        scalar_type: ScalarType::String,
        path: std::iter::once(field(&"a".repeat(128)))
            .chain((0..7).map(|_| ReportSegment::Match {
                field: "b".repeat(128),
                value: "x".repeat(4096),
            }))
            .collect(),
    };
    assert!(boundary.validate(&catalog).is_ok());
    assert!(catalog.to_json_with_semantics_v2(&boundary).is_ok());
    for version in [0, 1, 3, u32::MAX] {
        let mut invalid = valid.clone();
        invalid.version = version;
        assert!(invalid.validate(&catalog).is_err());
    }
}

#[test]
fn result_codec_rejects_unknown_missing_and_duplicate_fields() {
    for wire in [
        r#"{"kind":"observed","scalar_type":"string"}"#,
        r#"{"kind":"observed","scalar_type":"string","path":[],"fallback":"x"}"#,
        r#"{"kind":"observed","scalar_type":"string","scalar_type":"integer","path":[]}"#,
        r#"{"kind":"observed","kind":"string","value":"x","scalar_type":"string","path":[]}"#,
        r#"{"kind":"observed","scalar_type":"native","path":[]}"#,
        r#"{"kind":"none","value":null}"#,
        r#"{"kind":"string","value":"a","value":"b"}"#,
        r#"{"kind":"integer","value":18446744073709551616}"#,
        r#"{"kind":"integer","value":1.0}"#,
        r#"{"kind":"boolean","value":"true"}"#,
    ] {
        assert!(
            serde_json::from_str::<v2::ResultContract>(wire).is_err(),
            "{wire}"
        );
    }
    let (_, semantics) = observed(ScalarValue::String(String::new()));
    let original = serde_json::to_value(semantics).unwrap();
    for path in [
        vec!["unknown"],
        vec!["tasks", "0", "unknown"],
        vec!["tasks", "0", "verification", "unknown"],
    ] {
        let mut value = original.clone();
        let mut selected = &mut value;
        for key in &path[..path.len() - 1] {
            selected = if let Ok(index) = key.parse::<usize>() {
                &mut selected[index]
            } else {
                &mut selected[*key]
            };
        }
        selected[*path.last().unwrap()] = json!(true);
        assert!(serde_json::from_value::<v2::SemanticDescription>(value).is_err());
    }
}

#[test]
fn v2_retains_the_full_v1_task_validation_and_aggregate_encoder_bound() {
    let (catalog, valid) = observed(ScalarValue::String(String::new()));
    for mutation in [
        |s: &mut v2::SemanticDescription| s.tasks[0].verification.predicates.clear(),
        |s: &mut v2::SemanticDescription| s.tasks[0].verification.max_fuel = 1_000_001,
        |s: &mut v2::SemanticDescription| {
            s.tasks[0].result = Term::Literal {
                value: ScalarValue::Boolean(true),
            }
        },
        |s: &mut v2::SemanticDescription| {
            s.tasks[0].inputs.push(TaskInput {
                name: "body".into(),
                scalar_type: ScalarType::String,
            })
        },
        |s: &mut v2::SemanticDescription| {
            s.tasks[0].steps.push(TaskStep::Guard {
                conditions: vec![],
                failure: ScalarValue::String(String::new()),
            })
        },
    ] {
        let mut invalid = valid.clone();
        mutation(&mut invalid);
        assert!(invalid.validate(&catalog).is_err());
    }
    let mut large = valid;
    let template = large.tasks[0].clone();
    large.tasks = (0..8)
        .map(|index| {
            let mut task = template.clone();
            task.id = format!("task_{index}");
            task.verification.result = v2::ResultContract::Observed {
                scalar_type: ScalarType::String,
                path: std::iter::once(field("evidence"))
                    .chain((0..7).map(|_| ReportSegment::Match {
                        field: "id".into(),
                        value: "x".repeat(4096),
                    }))
                    .collect(),
            };
            task.verification.predicates = (0..2)
                .map(|index| ReportPredicate {
                    id: format!("proof_{index}"),
                    left: ReportTerm::Literal {
                        value: ScalarValue::String("x".repeat(4096)),
                    },
                    relation: ReportRelation::Equal,
                    right: ReportTerm::Literal {
                        value: ScalarValue::String("x".repeat(4096)),
                    },
                })
                .collect();
            task
        })
        .collect();
    large.validate(&catalog).unwrap();
    assert_eq!(
        catalog.to_json_with_semantics_v2(&large),
        Err(DescriptionError::OutputLimit)
    );
}
