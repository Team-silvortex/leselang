use leselang_runtime_core::{
    ScalarType, ScalarValue,
    discovery::{semantics::*, *},
};
use serde_json::{Value, json};

fn scalar(ty: ScalarType) -> TypeDescription {
    TypeDescription::Scalar(ty)
}
fn native(name: &str) -> TypeDescription {
    TypeDescription::Native(name.into())
}
fn lit(value: ScalarValue) -> Term {
    Term::Literal { value }
}
fn text(value: &str) -> Term {
    lit(ScalarValue::String(value.into()))
}
fn binding(name: &str) -> Term {
    Term::Binding { name: name.into() }
}
fn field(binding: &str, name: &str) -> Term {
    Term::Field {
        binding: binding.into(),
        name: name.into(),
    }
}
fn argument(name: &str, value: Term) -> TaskArgument {
    TaskArgument {
        name: name.into(),
        value,
    }
}
fn path(field: &str) -> Vec<ReportSegment> {
    vec![
        ReportSegment::Field {
            name: "native_after".into(),
        },
        ReportSegment::Match {
            field: "identifier".into(),
            value: "input".into(),
        },
        ReportSegment::Field { name: field.into() },
    ]
}
fn predicate(id: &str, left: ReportTerm, right: ReportTerm) -> ReportPredicate {
    ReportPredicate {
        id: id.into(),
        left,
        relation: ReportRelation::Equal,
        right,
    }
}

fn fixture() -> (CatalogDescription, SemanticDescription) {
    let parameters = |items: &[(&str, TypeDescription)]| {
        items
            .iter()
            .map(|(name, ty)| ParameterDescription {
                name: (*name).into(),
                required: true,
                types: vec![ty.clone()],
            })
            .collect()
    };
    let catalog = CatalogDescription {
        profile: "reference.custom".into(),
        catalog_version: 1,
        operations: vec![
            OperationDescription {
                name: "screen.find".into(),
                parameters: parameters(&[("name", scalar(ScalarType::String))]),
                result: native("object"),
                required_capability: "read".into(),
            },
            OperationDescription {
                name: "screen.observe".into(),
                parameters: parameters(&[("target", native("object"))]),
                result: native("snapshot"),
                required_capability: "read".into(),
            },
            OperationDescription {
                name: "screen.write".into(),
                parameters: parameters(&[
                    ("target", native("object")),
                    ("revision", scalar(ScalarType::Integer)),
                    ("text", scalar(ScalarType::String)),
                ]),
                result: scalar(ScalarType::Boolean),
                required_capability: "write".into(),
            },
        ],
        native_types: vec![
            NativeTypeDescription {
                name: "object".into(),
                fields: vec![],
            },
            NativeTypeDescription {
                name: "snapshot".into(),
                fields: [
                    ("text", ScalarType::String),
                    ("revision", ScalarType::Integer),
                    ("labels", ScalarType::StringList),
                    ("last", ScalarType::OptionalString),
                ]
                .iter()
                .map(|(name, scalar_type)| FieldDescription {
                    name: (*name).into(),
                    scalar_type: *scalar_type,
                })
                .collect(),
            },
        ],
    };
    let semantics = SemanticDescription {
        version: 1,
        operations: catalog
            .operations
            .iter()
            .enumerate()
            .map(|(index, operation)| OperationSemantics {
                operation: operation.name.clone(),
                summary: "Human-readable meaning, not a permission grant".into(),
                effect: if index == 2 {
                    EffectClass::Write
                } else {
                    EffectClass::Read
                },
                completion: if index == 2 {
                    CompletionKind::Submission
                } else {
                    CompletionKind::Value
                },
            })
            .collect(),
        tasks: vec![TaskDescription {
            id: "fill".into(),
            summary: "Fill an explicitly selected object".into(),
            inputs: vec![TaskInput {
                name: "text".into(),
                scalar_type: ScalarType::String,
            }],
            execution: ExecutionStyle::NamedText,
            steps: vec![
                TaskStep::Call {
                    binding: "target".into(),
                    operation: "screen.find".into(),
                    arguments: vec![argument("name", text("input"))],
                },
                TaskStep::Call {
                    binding: "before".into(),
                    operation: "screen.observe".into(),
                    arguments: vec![argument("target", binding("target"))],
                },
                TaskStep::Guard {
                    conditions: vec![
                        Condition::Equal {
                            left: field("before", "text"),
                            right: text(""),
                        },
                        Condition::Contains {
                            list: field("before", "labels"),
                            value: text("ready"),
                        },
                        Condition::OptionalEquals {
                            optional: field("before", "last"),
                            value: text("draft"),
                        },
                        Condition::IncreasedBy {
                            before: field("before", "revision"),
                            after: field("before", "revision"),
                            amount: 0,
                        },
                    ],
                    failure: ScalarValue::Boolean(false),
                },
                TaskStep::Call {
                    binding: "submitted".into(),
                    operation: "screen.write".into(),
                    arguments: vec![
                        argument("target", binding("target")),
                        argument("revision", field("before", "revision")),
                        argument(
                            "text",
                            Term::Input {
                                name: "text".into(),
                            },
                        ),
                    ],
                },
                TaskStep::Guard {
                    conditions: vec![Condition::Equal {
                        left: binding("submitted"),
                        right: lit(ScalarValue::Boolean(true)),
                    }],
                    failure: ScalarValue::Boolean(false),
                },
            ],
            result: lit(ScalarValue::Boolean(true)),
            verification: ReportContract {
                kind: "custom_run".into(),
                max_fuel: 1000,
                result: ScalarValue::Boolean(true),
                require_verified: true,
                predicates: vec![
                    predicate(
                        "saved",
                        ReportTerm::Scalar {
                            path: path("text"),
                            scalar_type: ScalarType::String,
                        },
                        ReportTerm::Input {
                            name: "text".into(),
                        },
                    ),
                    predicate(
                        "rows",
                        ReportTerm::Length {
                            path: vec![ReportSegment::Field {
                                name: "native_after".into(),
                            }],
                        },
                        ReportTerm::Literal {
                            value: ScalarValue::Integer(1),
                        },
                    ),
                    predicate(
                        "stable",
                        ReportTerm::Path {
                            path: path("labels"),
                        },
                        ReportTerm::Path {
                            path: path("labels"),
                        },
                    ),
                    ReportPredicate {
                        id: "increment".into(),
                        left: ReportTerm::Scalar {
                            path: path("revision"),
                            scalar_type: ScalarType::Integer,
                        },
                        relation: ReportRelation::IncreasedBy { amount: 1 },
                        right: ReportTerm::Literal {
                            value: ScalarValue::Integer(1),
                        },
                    },
                ],
            },
        }],
    };
    (catalog, semantics)
}

#[test]
fn extension_is_optional_and_codec_matches_serde_without_changing_legacy_bytes() {
    let (catalog, semantics) = fixture();
    let original = catalog.to_json().unwrap();
    let mut encoded: Value =
        serde_json::from_str(&catalog.to_json_with_semantics(&semantics).unwrap()).unwrap();
    assert_eq!(
        encoded
            .as_object_mut()
            .unwrap()
            .remove("semantics")
            .unwrap(),
        serde_json::to_value(&semantics).unwrap()
    );
    assert_eq!(encoded, serde_json::from_str::<Value>(&original).unwrap());
    assert_eq!(catalog.to_json().unwrap(), original);
    assert_eq!(
        semantics,
        serde_json::from_value(serde_json::to_value(&semantics).unwrap()).unwrap()
    );
    assert!(!original.contains("semantics"));
    assert!(
        catalog
            .to_json_with_semantics(&semantics)
            .unwrap()
            .ends_with('\n')
    );
}

#[test]
fn codec_covers_all_literal_execution_and_report_variants_with_exact_escaping() {
    let (catalog, mut semantics) = fixture();
    semantics.operations[0].summary = "meaning \"\\\n\t\u{202e} GUI ✓".into();
    let template = semantics.tasks[0].clone();
    let mut text_task = template.clone();
    text_task.id = "plain_text".into();
    text_task.execution = ExecutionStyle::Text;
    semantics.tasks.push(text_task);
    semantics.tasks[0]
        .verification
        .predicates
        .push(ReportPredicate {
            id: "integer_maximum".into(),
            left: ReportTerm::Scalar {
                path: path("revision"),
                scalar_type: ScalarType::Integer,
            },
            relation: ReportRelation::AtMost,
            right: ReportTerm::Literal {
                value: ScalarValue::Integer(i64::MAX as u64),
            },
        });
    for (index, value) in [
        ScalarValue::String("\"\\\n\t\u{2028}\u{2066} ✓".into()),
        ScalarValue::Integer(u64::MAX),
        ScalarValue::None,
    ]
    .into_iter()
    .enumerate()
    {
        let mut task = template.clone();
        task.id = format!("no_text_{index}");
        task.inputs.clear();
        task.execution = ExecutionStyle::NoText;
        task.steps = vec![TaskStep::Guard {
            conditions: vec![Condition::Equal {
                left: lit(value.clone()),
                right: lit(value.clone()),
            }],
            failure: value.clone(),
        }];
        task.result = lit(value.clone());
        task.verification.result = value.clone();
        task.verification.require_verified = false;
        task.verification.predicates = vec![predicate(
            "same",
            ReportTerm::Literal {
                value: value.clone(),
            },
            ReportTerm::Literal { value },
        )];
        semantics.tasks.push(task);
    }
    let encoded = catalog.to_json_with_semantics(&semantics).unwrap();
    assert_eq!(encoded.bytes().filter(|byte| *byte == b'\n').count(), 1);
    assert!(!encoded.contains('\u{202e}'));
    let decoded: Value = serde_json::from_str(&encoded).unwrap();
    assert_eq!(
        decoded["semantics"],
        serde_json::to_value(semantics).unwrap()
    );
}

#[test]
fn versions_missing_claims_unknown_operations_and_duplicates_are_rejected() {
    let (catalog, semantics) = fixture();
    let mutations: &[fn(&mut SemanticDescription)] = &[
        |s| s.version = 2,
        |s| s.tasks.clear(),
        |s| s.operations[0].operation = "other.find".into(),
        |s| s.operations.push(s.operations[0].clone()),
        |s| {
            s.operations.remove(0);
        },
        |s| s.tasks.push(s.tasks[0].clone()),
        |s| s.tasks[0].summary.clear(),
        |s| s.operations[0].summary.clear(),
        |s| s.tasks[0].execution = ExecutionStyle::NoText,
        |s| s.tasks[0].inputs.clear(),
        |s| s.tasks[0].inputs[0].scalar_type = ScalarType::Integer,
    ];
    for mutate in mutations {
        let mut invalid = semantics.clone();
        mutate(&mut invalid);
        assert_eq!(
            invalid.validate(&catalog),
            Err(DescriptionError::InvalidSemantics)
        );
    }
}

#[test]
fn names_bindings_forward_references_and_input_collisions_are_rejected() {
    let (catalog, semantics) = fixture();
    for invalid_name in [
        "",
        "body",
        "fn",
        "none",
        "true",
        "false",
        "_lese_helper",
        "two.words",
        "0first",
        "汉字",
        &"x".repeat(65),
    ] {
        let mut invalid = semantics.clone();
        invalid.tasks[0].id = invalid_name.into();
        assert_eq!(
            invalid.validate(&catalog),
            Err(DescriptionError::InvalidSemantics)
        );
        let mut invalid = semantics.clone();
        if let TaskStep::Call { binding, .. } = &mut invalid.tasks[0].steps[0] {
            *binding = invalid_name.into();
        }
        assert_eq!(
            invalid.validate(&catalog),
            Err(DescriptionError::InvalidSemantics)
        );
    }
    for replacement in [
        binding("later"),
        Term::Input {
            name: "missing".into(),
        },
        field("target", "text"),
    ] {
        let mut invalid = semantics.clone();
        if let TaskStep::Call { arguments, .. } = &mut invalid.tasks[0].steps[0] {
            arguments[0].value = replacement;
        }
        assert_eq!(
            invalid.validate(&catalog),
            Err(DescriptionError::InvalidSemantics)
        );
    }
    for name in ["target", "text"] {
        let mut invalid = semantics.clone();
        if let TaskStep::Call { binding, .. } = &mut invalid.tasks[0].steps[1] {
            *binding = name.into();
        }
        assert_eq!(
            invalid.validate(&catalog),
            Err(DescriptionError::InvalidSemantics)
        );
    }
}

#[test]
fn required_unknown_duplicate_and_typed_arguments_use_actual_catalog_domains() {
    let (catalog, semantics) = fixture();
    let mutations: &[fn(&mut Vec<TaskArgument>)] = &[
        Vec::clear,
        |args| args.push(args[0].clone()),
        |args| args[0].name = "unknown".into(),
        |args| args[0].value = text("not a native object"),
        |args| args[0].value = binding("before"),
        |args| args[1].value = field("before", "unknown"),
        |args| args[1].value = field("before", "text"),
        |args| args[2].value = lit(ScalarValue::None),
    ];
    for mutate in mutations {
        let mut invalid = semantics.clone();
        if let TaskStep::Call { arguments, .. } = &mut invalid.tasks[0].steps[3] {
            mutate(arguments);
        }
        assert_eq!(
            invalid.validate(&catalog),
            Err(DescriptionError::InvalidSemantics)
        );
    }
    let mut deny = catalog.clone();
    deny.operations[2].parameters[2].types.clear();
    assert_eq!(
        semantics.validate(&deny),
        Err(DescriptionError::InvalidSemantics)
    );
    let mut optional = catalog.clone();
    optional.operations[2].parameters[2].required = false;
    let mut missing = semantics.clone();
    if let TaskStep::Call { arguments, .. } = &mut missing.tasks[0].steps[3] {
        arguments.pop();
    }
    assert!(missing.validate(&optional).is_ok());
}

#[test]
fn guard_types_native_equality_result_and_failure_types_remain_strict() {
    let (catalog, semantics) = fixture();
    for condition in [
        Condition::Equal {
            left: binding("target"),
            right: binding("target"),
        },
        Condition::Equal {
            left: field("before", "text"),
            right: lit(ScalarValue::Boolean(true)),
        },
        Condition::Contains {
            list: field("before", "text"),
            value: text("a"),
        },
        Condition::IncreasedBy {
            before: field("before", "text"),
            after: field("before", "revision"),
            amount: 1,
        },
        Condition::OptionalEquals {
            optional: field("before", "text"),
            value: text("a"),
        },
    ] {
        let mut invalid = semantics.clone();
        invalid.tasks[0].steps[2] = TaskStep::Guard {
            conditions: vec![condition],
            failure: ScalarValue::Boolean(false),
        };
        assert_eq!(
            invalid.validate(&catalog),
            Err(DescriptionError::InvalidSemantics)
        );
    }
    let mut invalid = semantics.clone();
    invalid.tasks[0].result = binding("target");
    assert_eq!(
        invalid.validate(&catalog),
        Err(DescriptionError::InvalidSemantics)
    );
    let mut invalid = semantics.clone();
    invalid.tasks[0].result = text("different type");
    assert_eq!(
        invalid.validate(&catalog),
        Err(DescriptionError::InvalidSemantics)
    );
    let mut invalid = semantics.clone();
    if let TaskStep::Guard { failure, .. } = &mut invalid.tasks[0].steps[2] {
        *failure = ScalarValue::None;
    }
    assert_eq!(
        invalid.validate(&catalog),
        Err(DescriptionError::InvalidSemantics)
    );
}

#[test]
fn report_conditions_require_bounded_paths_unique_ids_and_compatible_known_types() {
    let (catalog, semantics) = fixture();
    let mutations: &[fn(&mut ReportContract)] = &[
        |r| r.kind.clear(),
        |r| r.max_fuel = 0,
        |r| r.predicates.clear(),
        |r| r.predicates.push(r.predicates[0].clone()),
        |r| r.predicates[0].id = "body".into(),
        |r| r.predicates[0].left = ReportTerm::Path { path: vec![] },
        |r| {
            r.predicates[0].left = ReportTerm::Path {
                path: vec![ReportSegment::Field { name: "".into() }],
            }
        },
        |r| {
            r.predicates[0].right = ReportTerm::Input {
                name: "unknown".into(),
            }
        },
        |r| {
            r.predicates[0].right = ReportTerm::Literal {
                value: ScalarValue::Integer(2),
            }
        },
        |r| r.predicates[0].relation = ReportRelation::IncreasedBy { amount: 0 },
        |r| r.predicates[0].relation = ReportRelation::AtMost,
    ];
    for mutate in mutations {
        let mut invalid = semantics.clone();
        mutate(&mut invalid.tasks[0].verification);
        assert_eq!(
            invalid.validate(&catalog),
            Err(DescriptionError::InvalidSemantics)
        );
    }
}

#[test]
fn finite_task_and_collection_ceilings_are_inclusive() {
    let (catalog, semantics) = fixture();
    let mut boundary = semantics.clone();
    boundary.tasks[0].summary = "x".repeat(1024);
    boundary.tasks[0].verification.max_fuel = 1_000_000;
    let guard = boundary.tasks[0].steps[4].clone();
    boundary.tasks[0].steps.resize(21, guard);
    if let TaskStep::Guard { conditions, .. } = &mut boundary.tasks[0].steps[2] {
        conditions.resize(8, conditions[0].clone());
    }
    let mut predicate = boundary.tasks[0].verification.predicates[2].clone();
    let segments = vec![
        ReportSegment::Field {
            name: "f".repeat(128)
        };
        8
    ];
    predicate.left = ReportTerm::Path {
        path: segments.clone(),
    };
    predicate.right = ReportTerm::Path { path: segments };
    boundary.tasks[0].verification.predicates = (0..64)
        .map(|index| {
            let mut predicate = predicate.clone();
            predicate.id = format!("p{index}");
            predicate
        })
        .collect();
    boundary.tasks = (0..8)
        .map(|index| {
            let mut task = boundary.tasks[0].clone();
            task.id = format!("task{index}");
            task
        })
        .collect();
    assert!(boundary.validate(&catalog).is_ok());
    let mutations: &[fn(&mut SemanticDescription)] = &[
        |s| s.tasks.push(s.tasks[0].clone()),
        |s| s.tasks[0].summary.push('x'),
        |s| {
            let step = s.tasks[0].steps[0].clone();
            s.tasks[0].steps.push(step);
        },
        |s| s.tasks[0].verification.max_fuel += 1,
        |s| {
            let predicate = s.tasks[0].verification.predicates[0].clone();
            s.tasks[0].verification.predicates.push(predicate);
        },
        |s| {
            if let TaskStep::Guard { conditions, .. } = &mut s.tasks[0].steps[2] {
                conditions.push(conditions[0].clone());
            }
        },
        |s| {
            if let ReportTerm::Path { path } = &mut s.tasks[0].verification.predicates[0].left {
                path.push(ReportSegment::Field {
                    name: "extra".into(),
                });
            }
        },
    ];
    for mutate in mutations {
        let mut invalid = boundary.clone();
        mutate(&mut invalid);
        assert_eq!(
            invalid.validate(&catalog),
            Err(DescriptionError::SemanticsLimit)
        );
    }
    let mut calls = semantics.clone();
    calls.tasks[0].steps = (0..16)
        .map(|index| TaskStep::Call {
            binding: format!("object{index}"),
            operation: "screen.find".into(),
            arguments: vec![argument("name", text("input"))],
        })
        .collect();
    assert!(calls.validate(&catalog).is_ok());
    calls.tasks[0].steps.push(TaskStep::Call {
        binding: "object16".into(),
        operation: "screen.find".into(),
        arguments: vec![argument("name", text("input"))],
    });
    assert_eq!(
        calls.validate(&catalog),
        Err(DescriptionError::SemanticsLimit)
    );
}

#[test]
fn aggregate_encoding_limit_and_scalar_bounds_return_no_partial_record() {
    let (catalog, mut semantics) = fixture();
    let value = ScalarValue::String("x".repeat(4096));
    semantics.tasks[0].verification.predicates = (0..64)
        .map(|index| {
            predicate(
                &format!("p{index}"),
                ReportTerm::Literal {
                    value: value.clone(),
                },
                ReportTerm::Literal {
                    value: value.clone(),
                },
            )
        })
        .collect();
    assert!(semantics.validate(&catalog).is_ok());
    assert_eq!(
        catalog.to_json_with_semantics(&semantics),
        Err(DescriptionError::OutputLimit)
    );
    semantics.tasks[0].verification.predicates[0].left = ReportTerm::Literal {
        value: ScalarValue::String("x".repeat(4097)),
    };
    assert_eq!(
        semantics.validate(&catalog),
        Err(DescriptionError::SemanticsLimit)
    );
    semantics.tasks[0].verification.predicates[0].left = ReportTerm::Literal {
        value: ScalarValue::OptionalString(leselang_runtime_core::OptionalStringValue(None)),
    };
    assert_eq!(
        semantics.validate(&catalog),
        Err(DescriptionError::InvalidSemantics)
    );
}

#[test]
fn remaining_ingress_bounds_precede_traversing_oversized_collections() {
    let (catalog, semantics) = fixture();
    let mutations: &[fn(&mut SemanticDescription)] = &[
        |s| s.operations.resize(65, s.operations[0].clone()),
        |s| s.operations[0].summary = "x".repeat(1025),
        |s| {
            let input = s.tasks[0].inputs[0].clone();
            s.tasks[0].inputs.push(input);
        },
        |s| {
            if let TaskStep::Call { arguments, .. } = &mut s.tasks[0].steps[0] {
                arguments.resize(65, arguments[0].clone());
            }
        },
        |s| s.tasks[0].verification.kind = "x".repeat(129),
        |s| {
            s.tasks[0].verification.predicates[0].left = ReportTerm::Path {
                path: vec![ReportSegment::Field {
                    name: "x".repeat(129),
                }],
            }
        },
        |s| {
            s.tasks[0].verification.predicates[0].left = ReportTerm::Path {
                path: vec![ReportSegment::Match {
                    field: "name".into(),
                    value: "x".repeat(4097),
                }],
            }
        },
    ];
    for mutate in mutations {
        let mut invalid = semantics.clone();
        mutate(&mut invalid);
        assert_eq!(
            invalid.validate(&catalog),
            Err(DescriptionError::SemanticsLimit)
        );
    }
}

#[test]
fn unchecked_wire_metadata_rejects_unknown_variants_and_fields_and_validates_after_decode() {
    let (catalog, semantics) = fixture();
    let mut value = serde_json::to_value(semantics).unwrap();
    value["version"] = json!(999);
    let decoded: SemanticDescription = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(
        decoded.validate(&catalog),
        Err(DescriptionError::InvalidSemantics)
    );
    value["scheduler"] = json!("automatic");
    assert!(serde_json::from_value::<SemanticDescription>(value).is_err());
    for value in [
        json!({"kind":"unknown"}),
        json!({"kind":"equal","extra":true}),
        json!({"kind":"at_most","extra":true}),
    ] {
        assert!(serde_json::from_value::<ReportRelation>(value).is_err());
    }
    assert!(
        serde_json::from_value::<Term>(
            json!({"kind":"literal","value":{"kind":"none"},"script":"anything"})
        )
        .is_err()
    );
    let (mut invalid_catalog, semantics) = fixture();
    invalid_catalog.catalog_version = 0;
    assert_eq!(
        semantics.validate(&invalid_catalog),
        Err(DescriptionError::InvalidCatalogVersion)
    );
}
