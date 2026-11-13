use leselang_runtime_core::{ScalarType, discovery::*};
use serde_json::{Value, json};

fn scalar(ty: ScalarType) -> TypeDescription {
    TypeDescription::Scalar(ty)
}

fn operation(name: &str) -> OperationDescription {
    OperationDescription {
        name: name.into(),
        parameters: vec![],
        result: scalar(ScalarType::None),
        required_capability: "read".into(),
    }
}

fn parameter(name: &str) -> ParameterDescription {
    ParameterDescription {
        name: name.into(),
        required: true,
        types: vec![scalar(ScalarType::String)],
    }
}

fn native(name: &str) -> NativeTypeDescription {
    NativeTypeDescription {
        name: name.into(),
        fields: vec![],
    }
}

fn catalog() -> CatalogDescription {
    CatalogDescription {
        profile: "test.gui-1".into(),
        catalog_version: 7,
        operations: vec![operation("gui.observe")],
        native_types: vec![native("object"), native("snapshot")],
    }
}

fn record(description: &CatalogDescription) -> Value {
    serde_json::from_str(&description.to_json().unwrap()).unwrap()
}

#[test]
fn exact_versioned_schema_retains_types_order_and_required_capabilities() {
    let mut description = catalog();
    description.operations[0].parameters = vec![
        ParameterDescription {
            name: "target".into(),
            required: true,
            types: vec![TypeDescription::Native("object".into())],
        },
        ParameterDescription {
            name: "fallback".into(),
            required: false,
            types: vec![scalar(ScalarType::String), scalar(ScalarType::None)],
        },
    ];
    description.operations[0].result = TypeDescription::Native("snapshot".into());
    description.native_types[1].fields = vec![
        FieldDescription {
            name: "value".into(),
            scalar_type: ScalarType::OptionalString,
        },
        FieldDescription {
            name: "revision".into(),
            scalar_type: ScalarType::Integer,
        },
    ];
    let before = description.clone();
    let encoded = description.to_json().unwrap();
    assert!(encoded.ends_with('\n'));
    assert_eq!(encoded.bytes().filter(|byte| *byte == b'\n').count(), 1);
    assert_eq!(encoded, description.to_json().unwrap());
    assert_eq!(description, before);
    assert_eq!(
        record(&description),
        json!({
            "schema_version": 1, "protocol_version": 1, "catalog_version": 7, "profile": "test.gui-1",
            "operations": [{
                "name": "gui.observe", "parameters": [
                    { "name": "target", "required": true, "types": [{ "kind": "native", "name": "object" }] },
                    { "name": "fallback", "required": false, "types": [{ "kind": "scalar", "name": "string" }, { "kind": "scalar", "name": "none" }] }
                ], "result": { "kind": "native", "name": "snapshot" }, "required_capability": "read"
            }],
            "native_types": [
                { "name": "object", "fields": [] },
                { "name": "snapshot", "fields": [{ "name": "value", "scalar_type": "optional_string" }, { "name": "revision", "scalar_type": "integer" }] }
            ]
        })
    );
}

#[test]
fn all_scalar_names_optional_and_deny_all_domains_are_exact() {
    let mut description = catalog();
    let types = [
        ScalarType::Integer,
        ScalarType::Boolean,
        ScalarType::String,
        ScalarType::None,
        ScalarType::OptionalString,
        ScalarType::StringList,
    ];
    description.operations[0].parameters = vec![
        ParameterDescription {
            name: "body".into(),
            required: true,
            types: types.into_iter().map(scalar).collect(),
        },
        ParameterDescription {
            name: "blocked".into(),
            required: false,
            types: vec![],
        },
    ];
    let json = record(&description);
    let names: Vec<_> = json["operations"][0]["parameters"][0]["types"]
        .as_array()
        .unwrap()
        .iter()
        .map(|ty| ty["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "integer",
            "boolean",
            "string",
            "none",
            "optional_string",
            "string_list"
        ]
    );
    assert_eq!(json["operations"][0]["parameters"][1]["types"], json!([]));
    assert_eq!(json["operations"][0]["parameters"][1]["required"], false);
    description.operations.clear();
    description.native_types.clear();
    assert_eq!(record(&description)["operations"], json!([]));
}

#[test]
fn capabilities_and_literal_field_names_preserve_utf8_controls_and_escaping() {
    let mut description = catalog();
    let special = format!(
        "权限\"\\🦀\r\n\t\u{85}\u{2028}\u{202e}\u{2066}{}",
        (0..=31).map(char::from).collect::<String>()
    );
    description.operations[0].required_capability = special.clone();
    description.native_types[1].fields = vec![FieldDescription {
        name: special.clone(),
        scalar_type: ScalarType::String,
    }];
    let encoded = description.to_json().unwrap();
    assert_eq!(encoded.bytes().filter(|byte| *byte == b'\n').count(), 1);
    let value: Value = serde_json::from_str(&encoded).unwrap();
    assert_eq!(value["operations"][0]["required_capability"], special);
    assert_eq!(value["native_types"][1]["fields"][0]["name"], special);
    assert!(!encoded.contains('\u{202e}'));
    assert!(encoded.contains("\\u202e"));
}

#[test]
fn identifier_rules_preserve_scalar_host_labels_and_bound_utf8_bytes() {
    let mut description = catalog();
    description.profile = "p".repeat(128);
    description.operations[0].name = "x".repeat(128);
    description.operations[0].parameters = vec![parameter(&"p".repeat(64)), parameter("body")];
    description.operations[0].required_capability = "界".repeat(42) + "ab";
    description.native_types[0].name = "n".repeat(64);
    description.native_types[1].fields = vec![FieldDescription {
        name: "界".repeat(42) + "ab",
        scalar_type: ScalarType::String,
    }];
    assert!(description.to_json().is_ok());
    description.operations[0].name = "gui.body.find".into();
    assert!(description.to_json().is_ok());
    description.operations[0].name = "body".into();
    assert!(description.to_json().is_ok());

    for name in [
        "",
        "a..b",
        ".a",
        "a.",
        "fn",
        "a.none",
        "wrong-name",
        "2thing",
        "汉字",
    ] {
        let mut invalid = catalog();
        invalid.operations[0].name = name.into();
        assert_eq!(
            invalid.to_json(),
            Err(DescriptionError::InvalidOperationName),
            "{name}"
        );
    }
    let mut invalid = description.clone();
    invalid.operations[0].name = "x".repeat(129);
    assert_eq!(
        invalid.to_json(),
        Err(DescriptionError::InvalidOperationName)
    );
    for name in ["", "a.b", "none", "p-p", "0p"] {
        let mut invalid = catalog();
        invalid.operations[0].parameters.push(parameter(name));
        assert_eq!(
            invalid.to_json(),
            Err(DescriptionError::InvalidParameterName)
        );
    }
    let mut invalid = catalog();
    invalid.operations[0]
        .parameters
        .push(parameter(&"p".repeat(65)));
    assert_eq!(
        invalid.to_json(),
        Err(DescriptionError::InvalidParameterName)
    );
    for alias in [
        "integer",
        "boolean",
        "string",
        "none",
        "optional_string",
        "string_list",
        "body",
        "fn",
        "true",
        "false",
        "native.type",
        "",
    ] {
        let mut invalid = catalog();
        invalid.native_types[0].name = alias.into();
        assert_eq!(
            invalid.to_json(),
            Err(DescriptionError::InvalidNativeTypeName),
            "{alias}"
        );
    }
    let mut invalid = catalog();
    invalid.native_types[0].name = "n".repeat(65);
    assert_eq!(
        invalid.to_json(),
        Err(DescriptionError::InvalidNativeTypeName)
    );
    for label in [String::new(), "界".repeat(43)] {
        let mut invalid = catalog();
        invalid.operations[0].required_capability = label.clone();
        assert_eq!(invalid.to_json(), Err(DescriptionError::InvalidCapability));
        let mut invalid = catalog();
        invalid.native_types[0].fields.push(FieldDescription {
            name: label,
            scalar_type: ScalarType::String,
        });
        assert_eq!(invalid.to_json(), Err(DescriptionError::InvalidFieldName));
    }
}

#[test]
fn invalid_versions_profiles_duplicates_and_unknown_native_references_fail_closed() {
    let mut description = catalog();
    description.catalog_version = 0;
    assert_eq!(
        description.to_json(),
        Err(DescriptionError::InvalidCatalogVersion)
    );
    for profile in ["", "gui/profile", "含义", "bad profile", &"p".repeat(129)] {
        let mut description = catalog();
        description.profile = profile.into();
        assert_eq!(description.to_json(), Err(DescriptionError::InvalidProfile));
    }
    let mut description = catalog();
    description
        .operations
        .push(description.operations[0].clone());
    assert_eq!(
        description.to_json(),
        Err(DescriptionError::DuplicateOperation)
    );
    let mut description = catalog();
    description.operations[0].parameters = vec![parameter("same"), parameter("same")];
    assert_eq!(
        description.to_json(),
        Err(DescriptionError::DuplicateParameter)
    );
    let mut description = catalog();
    description.native_types.push(native("object"));
    assert_eq!(
        description.to_json(),
        Err(DescriptionError::DuplicateNativeType)
    );
    let mut description = catalog();
    description.native_types[0].fields = vec![
        FieldDescription {
            name: "same".into(),
            scalar_type: ScalarType::String
        };
        2
    ];
    assert_eq!(description.to_json(), Err(DescriptionError::DuplicateField));
    for duplicate in [
        scalar(ScalarType::String),
        TypeDescription::Native("object".into()),
    ] {
        let mut description = catalog();
        description.operations[0].parameters = vec![ParameterDescription {
            name: "arg".into(),
            required: true,
            types: vec![duplicate.clone(), duplicate],
        }];
        assert_eq!(
            description.to_json(),
            Err(DescriptionError::DuplicateParameterType)
        );
    }
    let mut description = catalog();
    description.operations[0].result = TypeDescription::Native("missing".into());
    assert_eq!(
        description.to_json(),
        Err(DescriptionError::UndeclaredNativeType)
    );
    let mut description = catalog();
    description.operations[0].parameters = vec![ParameterDescription {
        name: "arg".into(),
        required: true,
        types: vec![TypeDescription::Native("missing".into())],
    }];
    assert_eq!(
        description.to_json(),
        Err(DescriptionError::UndeclaredNativeType)
    );
}

#[test]
fn collection_limits_accept_exact_ceilings_and_reject_the_next_item() {
    let mut description = catalog();
    description.operations = (0..MAX_OPERATIONS)
        .map(|index| operation(&format!("op{index}")))
        .collect();
    assert!(description.to_json().is_ok());
    description.operations.push(operation("overflow"));
    assert_eq!(description.to_json(), Err(DescriptionError::OperationLimit));
    let mut description = catalog();
    description.operations[0].parameters = (0..MAX_PARAMETERS_PER_OPERATION)
        .map(|index| parameter(&format!("p{index}")))
        .collect();
    assert!(description.to_json().is_ok());
    description.operations[0]
        .parameters
        .push(parameter("overflow"));
    assert_eq!(description.to_json(), Err(DescriptionError::ParameterLimit));
    let mut description = catalog();
    description.native_types = (0..MAX_NATIVE_TYPES)
        .map(|index| native(&format!("type{index}")))
        .collect();
    let mut types: Vec<_> = [
        ScalarType::Integer,
        ScalarType::Boolean,
        ScalarType::String,
        ScalarType::None,
        ScalarType::OptionalString,
        ScalarType::StringList,
    ]
    .into_iter()
    .map(scalar)
    .collect();
    types.extend(
        description
            .native_types
            .iter()
            .map(|ty| TypeDescription::Native(ty.name.clone())),
    );
    description.operations[0].parameters = vec![ParameterDescription {
        name: "all".into(),
        required: true,
        types,
    }];
    assert_eq!(
        description.operations[0].parameters[0].types.len(),
        MAX_PARAMETER_TYPES
    );
    assert!(description.to_json().is_ok());
    description.operations[0].parameters[0]
        .types
        .push(scalar(ScalarType::String));
    assert_eq!(description.to_json(), Err(DescriptionError::TypeLimit));
    description.operations[0].parameters.clear();
    description.native_types.push(native("overflow"));
    assert_eq!(
        description.to_json(),
        Err(DescriptionError::NativeTypeLimit)
    );
    let mut description = catalog();
    description.native_types[0].fields = (0..MAX_FIELDS_PER_NATIVE_TYPE)
        .map(|index| FieldDescription {
            name: format!("f{index}"),
            scalar_type: ScalarType::String,
        })
        .collect();
    assert!(description.to_json().is_ok());
    description.native_types[0].fields.push(FieldDescription {
        name: "overflow".into(),
        scalar_type: ScalarType::String,
    });
    assert_eq!(description.to_json(), Err(DescriptionError::FieldLimit));
}

#[test]
fn encoded_output_limit_is_inclusive_and_includes_the_final_lf() {
    let mut description = catalog();
    description.operations.clear();
    description.native_types = (0..MAX_NATIVE_TYPES)
        .map(|index| NativeTypeDescription {
            name: format!("type{index}"),
            fields: (0..MAX_FIELDS_PER_NATIVE_TYPE)
                .map(|index| FieldDescription {
                    name: format!("f{index:02}"),
                    scalar_type: ScalarType::String,
                })
                .collect(),
        })
        .collect();
    let baseline = description.to_json().unwrap().len();
    let fields = MAX_NATIVE_TYPES * MAX_FIELDS_PER_NATIVE_TYPE;
    let extra = MAX_DESCRIPTION_BYTES - baseline;
    for (index, field) in description
        .native_types
        .iter_mut()
        .flat_map(|native| &mut native.fields)
        .enumerate()
    {
        field
            .name
            .push_str(&"x".repeat(extra / fields + usize::from(index < extra % fields)));
        assert!(field.name.len() <= 128);
    }
    let encoded = description.to_json().unwrap();
    assert_eq!(encoded.len(), MAX_DESCRIPTION_BYTES);
    assert!(encoded.ends_with('\n'));
    serde_json::from_str::<Value>(&encoded).unwrap();
    description.native_types[0].fields[0].name.push('x');
    assert_eq!(description.to_json(), Err(DescriptionError::OutputLimit));
}

#[test]
fn escaping_expansion_counts_toward_output_limit_without_truncation() {
    let mut description = catalog();
    description.operations.clear();
    description.native_types = (0..6)
        .map(|index| NativeTypeDescription {
            name: format!("type{index}"),
            fields: (0..MAX_FIELDS_PER_NATIVE_TYPE)
                .map(|index| FieldDescription {
                    name: format!("f{index:02}{}", "\n".repeat(120)),
                    scalar_type: ScalarType::String,
                })
                .collect(),
        })
        .collect();
    assert!(
        description
            .native_types
            .iter()
            .flat_map(|native| &native.fields)
            .map(|field| field.name.len())
            .sum::<usize>()
            < MAX_DESCRIPTION_BYTES
    );
    assert_eq!(description.to_json(), Err(DescriptionError::OutputLimit));
    description.native_types.truncate(1);
    let decoded = record(&description);
    assert_eq!(
        decoded["native_types"][0]["fields"][0]["name"],
        description.native_types[0].fields[0].name
    );
}
