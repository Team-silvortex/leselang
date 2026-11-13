//! Owned discovery data through an independent, platform-free consumer.
use std::cell::{Cell, RefCell};

use leselang_vm::{
    embedding::{
        EffectSessionEnd, Error, Fuel, Host, HostBuilder, HostFailure, Limits, NamedParameter,
        Poll, ScalarType, ScalarTypeSet, ScalarValue, diagnostics, parameter,
    },
    native::core::discovery::{
        CatalogDescription, DescriptionError, FieldDescription, NativeTypeDescription,
        OperationDescription, ParameterDescription, TypeDescription, semantics as semantic,
    },
};
use serde_json::{Value, json};

const TYPES: [(ScalarType, &str); 6] = [
    (ScalarType::Integer, "integer"),
    (ScalarType::Boolean, "boolean"),
    (ScalarType::String, "string"),
    (ScalarType::None, "none"),
    (ScalarType::OptionalString, "optional_string"),
    (ScalarType::StringList, "string_list"),
];
const GRANTS: &[&str] = &["counter.write"];
const SOURCE: &str = "fn main() = bind(first: counter.add(amount: 19), body: counter.add(amount: add(left: first, right: 4)))";

fn counter_semantics() -> semantic::SemanticDescription {
    use semantic::*;
    let integer = |value| Term::Literal {
        value: ScalarValue::Integer(value),
    };
    SemanticDescription {
        version: 1,
        operations: vec![OperationSemantics {
            operation: "counter.add".into(),
            summary: "Add the supplied amount and return the new counter value.".into(),
            effect: EffectClass::Write,
            completion: CompletionKind::Value,
        }],
        tasks: vec![TaskDescription {
            id: "add_twice".into(),
            summary: "Add 19 then 23 and verify the resulting counter.".into(),
            inputs: vec![],
            steps: vec![
                TaskStep::Call {
                    binding: "first".into(),
                    operation: "counter.add".into(),
                    arguments: vec![TaskArgument {
                        name: "amount".into(),
                        value: integer(19),
                    }],
                },
                TaskStep::Call {
                    binding: "second".into(),
                    operation: "counter.add".into(),
                    arguments: vec![TaskArgument {
                        name: "amount".into(),
                        value: integer(23),
                    }],
                },
            ],
            result: Term::Binding {
                name: "second".into(),
            },
            execution: ExecutionStyle::NoText,
            verification: ReportContract {
                kind: "consumer_counter_run".into(),
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
    }
}

fn decode(description: &CatalogDescription) -> Value {
    let text = description.to_json().unwrap();
    assert!(text.ends_with('\n'));
    assert_eq!(text.lines().count(), 1, "one complete JSON record plus LF");
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["protocol_version"], 1);
    if description.profile == "consumer.counter" {
        // Exercise optional semantics at every existing permission/Fuel/cancel
        // boundary below. Serializing claims must not perform host work or alter
        // registration, the active session, or the original JSON projection.
        let encoded = description
            .to_json_with_semantics(&counter_semantics())
            .unwrap();
        let mut extended: Value = serde_json::from_str(&encoded).unwrap();
        let semantics = extended
            .as_object_mut()
            .unwrap()
            .remove("semantics")
            .unwrap();
        assert_eq!(semantics["tasks"][0]["id"], "add_twice");
        assert_eq!(extended, value);
        assert_eq!(description.to_json().unwrap(), text);
    }
    value
}

#[test]
fn scalar_discovery_projects_actual_registration_order_domains_and_six_result_types() {
    let callbacks = Cell::new(0);
    let mut builder = HostBuilder::new();
    let long_name = "x".repeat(100);
    let capability = "编辑 🦀\"\\\n\r\t\0\u{001f}\u{202e}";
    builder
        .register(
            &long_name,
            vec![
                parameter("target", ScalarType::Integer),
                NamedParameter::optional(
                    "body".into(),
                    ScalarTypeSet::only(ScalarType::OptionalString)
                        .with(ScalarType::None)
                        .with(ScalarType::String),
                ),
                NamedParameter::optional("denied".into(), ScalarTypeSet::empty()),
            ],
            ScalarType::Boolean,
            capability,
            |_, _| {
                callbacks.set(callbacks.get() + 1);
                Ok(ScalarValue::Boolean(true))
            },
        )
        .unwrap();
    for (ty, name) in TYPES {
        builder
            .register(
                &format!("value.result_{name}"),
                vec![],
                ty,
                "values.read",
                |_, _| panic!("catalog discovery cannot execute a registered operation"),
            )
            .unwrap();
    }
    let host = builder.build();
    let original = host.describe();
    let description = host.catalog_description("consumer.scalar");
    let wire = decode(&description);
    assert_eq!(wire["profile"], "consumer.scalar");
    assert_eq!(wire["catalog_version"], 1);
    assert_eq!(wire["native_types"], json!([]));
    assert_eq!(description.operations.len(), original.operations().len());
    let operations = wire["operations"].as_array().unwrap();
    assert_eq!(operations.len(), 7);
    for (index, original) in original.operations().iter().enumerate() {
        assert_eq!(operations[index]["name"], original.name());
        assert_eq!(
            operations[index]["required_capability"],
            original.required_capability()
        );
        assert_eq!(
            operations[index]["parameters"].as_array().unwrap().len(),
            original.parameters().len()
        );
    }
    assert_eq!(operations[0]["name"], long_name);
    assert_eq!(operations[0]["required_capability"], capability);
    assert_eq!(
        operations[0]["result"],
        json!({"kind":"scalar","name":"boolean"})
    );
    assert_eq!(
        operations[0]["parameters"],
        json!([
            {"name":"target","required":true,"types":[{"kind":"scalar","name":"integer"}]},
            {"name":"body","required":false,"types":[
                {"kind":"scalar","name":"string"},
                {"kind":"scalar","name":"none"},
                {"kind":"scalar","name":"optional_string"}
            ]},
            {"name":"denied","required":false,"types":[]}
        ])
    );
    for (operation, (_, name)) in operations[1..].iter().zip(TYPES) {
        assert_eq!(operation["result"], json!({"kind":"scalar","name":name}));
    }
    assert_eq!(
        description.to_json().unwrap(),
        description.to_json().unwrap()
    );
    assert_eq!(callbacks.get(), 0);
}

fn native_catalog() -> CatalogDescription {
    CatalogDescription {
        profile: "consumer.gui".into(),
        catalog_version: 7,
        operations: vec![
            OperationDescription {
                name: "gui.find".into(),
                parameters: vec![ParameterDescription {
                    name: "name".into(),
                    required: true,
                    types: vec![TypeDescription::Scalar(ScalarType::String)],
                }],
                result: TypeDescription::Native("object".into()),
                required_capability: "gui.read".into(),
            },
            OperationDescription {
                name: "gui.observe".into(),
                parameters: vec![ParameterDescription {
                    name: "target".into(),
                    required: true,
                    types: vec![TypeDescription::Native("object".into())],
                }],
                result: TypeDescription::Native("snapshot".into()),
                required_capability: "gui.read".into(),
            },
        ],
        native_types: vec![
            NativeTypeDescription {
                name: "object".into(),
                fields: vec![],
            },
            NativeTypeDescription {
                name: "snapshot".into(),
                fields: TYPES
                    .into_iter()
                    .map(|(scalar_type, name)| FieldDescription {
                        name: format!("field_{name}"),
                        scalar_type,
                    })
                    .collect(),
            },
        ],
    }
}

#[test]
fn owned_native_descriptions_encode_exact_references_and_scalar_snapshot_fields() {
    let description = native_catalog();
    let wire = decode(&description);
    assert_eq!(wire["profile"], "consumer.gui");
    assert_eq!(wire["catalog_version"], 7);
    assert_eq!(
        wire["operations"][0]["result"],
        json!({"kind":"native","name":"object"})
    );
    assert_eq!(
        wire["operations"][1]["parameters"][0]["types"],
        json!([{"kind":"native","name":"object"}])
    );
    assert_eq!(
        wire["operations"][1]["result"],
        json!({"kind":"native","name":"snapshot"})
    );
    assert_eq!(
        wire["native_types"][0],
        json!({"name":"object","fields":[]})
    );
    let fields = wire["native_types"][1]["fields"].as_array().unwrap();
    for (field, (_, name)) in fields.iter().zip(TYPES) {
        assert_eq!(field["name"], format!("field_{name}"));
        assert_eq!(field["scalar_type"], name);
    }
    assert_eq!(fields.len(), 6);
    // JSON is an observation of public metadata, with no instance identity or lease.
    for forbidden in [
        "object_id",
        "lease",
        "generation",
        "callback",
        "grant",
        "receipt",
    ] {
        assert!(wire.get(forbidden).is_none());
    }
}

#[test]
fn invalid_native_references_duplicates_and_names_never_produce_a_record() {
    for case in 0..12 {
        let mut description = native_catalog();
        match case {
            0 => description.catalog_version = 0,
            1 => description.operations[0].result = TypeDescription::Native("missing".into()),
            2 => {
                description.operations[1].parameters[0].types =
                    vec![TypeDescription::Native("missing".into())]
            }
            3 => description.native_types.push(NativeTypeDescription {
                name: "object".into(),
                fields: vec![],
            }),
            4 => description.native_types[1].fields.push(FieldDescription {
                name: "field_integer".into(),
                scalar_type: ScalarType::String,
            }),
            5 => description.operations[1].name = "gui.find".into(),
            6 => description.operations[0]
                .parameters
                .push(ParameterDescription {
                    name: "name".into(),
                    required: false,
                    types: vec![],
                }),
            7 => description.operations[0].parameters[0]
                .types
                .push(TypeDescription::Scalar(ScalarType::String)),
            8 => description.operations[0].name = "gui..find".into(),
            9 => description.operations[0].parameters[0].name = "invalid-name".into(),
            10 => description.native_types[0].name = "invalid.name".into(),
            11 => description.native_types[1].fields[0].name = "".into(),
            _ => unreachable!(),
        }
        assert!(description.to_json().is_err(), "case {case}");
    }
    // Required with no accepted type is an explicit deny-all contract, never "any".
    let mut description = native_catalog();
    description.operations[0].parameters[0].types.clear();
    assert_eq!(
        decode(&description)["operations"][0]["parameters"][0]["types"],
        json!([])
    );
}

#[test]
fn description_bounds_reject_whole_records_including_encoded_output_growth() {
    use leselang_vm::native::core::discovery::{
        MAX_FIELDS_PER_NATIVE_TYPE, MAX_NATIVE_TYPES, MAX_OPERATIONS, MAX_PARAMETER_TYPES,
        MAX_PARAMETERS_PER_OPERATION,
    };
    for (case, expected) in [
        (0, DescriptionError::OperationLimit),
        (1, DescriptionError::ParameterLimit),
        (2, DescriptionError::NativeTypeLimit),
        (3, DescriptionError::FieldLimit),
        (4, DescriptionError::TypeLimit),
        (5, DescriptionError::InvalidProfile),
        (6, DescriptionError::InvalidCapability),
        (7, DescriptionError::InvalidFieldName),
        (8, DescriptionError::InvalidNativeTypeName),
    ] {
        let mut description = native_catalog();
        match case {
            0 => {
                description.operations = (0..=MAX_OPERATIONS)
                    .map(|index| OperationDescription {
                        name: format!("operation_{index}"),
                        parameters: vec![],
                        result: TypeDescription::Scalar(ScalarType::None),
                        required_capability: "read".into(),
                    })
                    .collect()
            }
            1 => {
                description.operations[0].parameters = (0..=MAX_PARAMETERS_PER_OPERATION)
                    .map(|index| ParameterDescription {
                        name: format!("parameter_{index}"),
                        required: true,
                        types: vec![],
                    })
                    .collect()
            }
            2 => {
                description.native_types = (0..=MAX_NATIVE_TYPES)
                    .map(|index| NativeTypeDescription {
                        name: format!("native_{index}"),
                        fields: vec![],
                    })
                    .collect()
            }
            3 => {
                description.native_types[0].fields = (0..=MAX_FIELDS_PER_NATIVE_TYPE)
                    .map(|index| FieldDescription {
                        name: format!("field_{index}"),
                        scalar_type: ScalarType::Integer,
                    })
                    .collect()
            }
            4 => {
                description.operations[0].parameters[0].types =
                    vec![TypeDescription::Scalar(ScalarType::Integer); MAX_PARAMETER_TYPES + 1]
            }
            5 => description.profile = "p".repeat(129),
            6 => description.operations[0].required_capability = "🦀".repeat(32) + "x",
            7 => description.native_types[1].fields[0].name = "🦀".repeat(32) + "x",
            8 => description.native_types[0].name = "string".into(),
            _ => unreachable!(),
        }
        assert_eq!(description.to_json().unwrap_err(), expected, "case {case}");
    }
    let mut description = native_catalog();
    description.operations[0].required_capability = "🦀".repeat(32);
    description.native_types[1].fields[0].name = "🦀".repeat(32);
    assert!(
        description.to_json().is_ok(),
        "UTF-8 byte limits are inclusive"
    );

    // Individually valid bounded collections can still exceed the encoded cap.
    description.operations = (0..MAX_OPERATIONS)
        .map(|index| OperationDescription {
            name: format!("operation_{index}"),
            parameters: (0..MAX_PARAMETERS_PER_OPERATION)
                .map(|parameter| ParameterDescription {
                    name: format!("parameter_{parameter:02}_{}", "x".repeat(48)),
                    required: true,
                    types: vec![TypeDescription::Scalar(ScalarType::String)],
                })
                .collect(),
            result: TypeDescription::Scalar(ScalarType::None),
            required_capability: "read".into(),
        })
        .collect();
    assert_eq!(
        description.to_json().unwrap_err(),
        DescriptionError::OutputLimit
    );
}

#[test]
fn discovery_output_limits_do_not_change_native_registration_validity() {
    let mut limits = Limits::default();
    limits.catalog.max_operations = 65;
    let mut builder = HostBuilder::with_limits(limits);
    for index in 0..65 {
        builder
            .register(
                &format!("operation_{index}"),
                vec![],
                ScalarType::Integer,
                "read",
                |_, _| panic!("metadata projection cannot invoke a registered operation"),
            )
            .unwrap();
    }
    let host = builder.build();
    assert_eq!(host.describe().operations().len(), 65);
    let description = host.catalog_description("consumer.large");
    assert_eq!(
        description.operations.len(),
        65,
        "projection must not silently truncate"
    );
    assert_eq!(
        description.to_json().unwrap_err(),
        DescriptionError::OperationLimit
    );
    assert!(
        host.compile("fn main() = operation_64()", &["read"])
            .is_ok()
    );
}

#[test]
fn metadata_survives_its_host_and_mutation_never_reconfigures_registration() {
    let callbacks = Cell::new(0);
    let mut builder = HostBuilder::new();
    builder
        .register(
            "counter.read",
            vec![],
            ScalarType::Integer,
            "counter.read",
            |_, _| {
                callbacks.set(callbacks.get() + 1);
                Ok(ScalarValue::Integer(42))
            },
        )
        .unwrap();
    let host = builder.build();
    let mut description = host.catalog_description("consumer.owned");
    description.operations[0].required_capability = "forged".into();
    description.operations[0].result = TypeDescription::Scalar(ScalarType::Boolean);
    assert_eq!(
        decode(&description)["operations"][0]["required_capability"],
        "forged"
    );
    assert_eq!(
        host.describe().operations()[0].required_capability(),
        "counter.read"
    );
    assert_eq!(
        host.describe().operations()[0].result_type(),
        ScalarType::Integer
    );
    assert!(
        host.compile("fn main() = counter.read()", &["forged"])
            .is_err()
    );
    assert_eq!(callbacks.get(), 0);
    drop(host);
    assert_eq!(decode(&description)["profile"], "consumer.owned");
    assert_eq!(callbacks.get(), 0);
}

fn counter_host<'a>(total: &'a Cell<u64>, calls: &'a RefCell<Vec<u64>>) -> Host<'a> {
    let mut builder = HostBuilder::new();
    builder
        .register(
            "counter.add",
            vec![parameter("amount", ScalarType::Integer)],
            ScalarType::Integer,
            "counter.write",
            move |arguments, fuel| {
                let Some(ScalarValue::Integer(amount)) = arguments.get("amount") else {
                    panic!("integer operand required")
                };
                fuel.charge(amount + 1)
                    .map_err(|_| HostFailure::Failed("insufficient host fuel".into()))?;
                calls.borrow_mut().push(*amount);
                total.set(total.get() + amount);
                Ok(ScalarValue::Integer(total.get()))
            },
        )
        .unwrap();
    builder.build()
}

#[derive(Debug, PartialEq)]
struct Outcome {
    result: Result<ScalarValue, (diagnostics::DiagnosticKind, String)>,
    fuel_remaining: u64,
    calls: Vec<u64>,
    total: u64,
}

fn run(budget: u64, discover: bool) -> Outcome {
    let total = Cell::new(0);
    let calls = RefCell::new(Vec::new());
    let host = counter_host(&total, &calls);
    if discover {
        decode(&host.catalog_description("consumer.counter"));
    }
    let program = host.compile(SOURCE, GRANTS).unwrap();
    let mut session = host.start(&program, GRANTS, Fuel::new(budget)).unwrap();
    let result = (|| -> Result<ScalarValue, Error> {
        loop {
            if discover {
                let before = session.fuel_remaining();
                decode(&host.catalog_description("consumer.counter"));
                assert_eq!(session.fuel_remaining(), before);
            }
            match session.poll()? {
                Poll::Request(work) => {
                    if discover {
                        decode(&host.catalog_description("consumer.counter"));
                    }
                    let authorized = session.authorize(work).map_err(|e| e.error)?;
                    let receipt = session.dispatch(authorized).map_err(|e| e.error)?;
                    if discover {
                        decode(&host.catalog_description("consumer.counter"));
                    }
                    session.accept(receipt).map_err(|e| e.error)?;
                }
                Poll::Value(value) => return Ok(value),
                other => panic!("unexpected explicit driver boundary {other:?}"),
            }
        }
    })()
    .map_err(|error| {
        let diagnostic = diagnostics::runtime(&error);
        (diagnostic.kind, diagnostic.message)
    });
    let calls = calls.borrow().clone();
    Outcome {
        result,
        fuel_remaining: session.fuel_remaining(),
        calls,
        total: total.get(),
    }
}

#[test]
fn discovery_keeps_two_effects_and_every_original_fuel_cutoff() {
    let complete = run(1000, false);
    assert_eq!(complete.result, Ok(ScalarValue::Integer(42)));
    assert_eq!(complete.calls, [19, 23]);
    let cost = 1000 - complete.fuel_remaining;
    let mut partial = false;
    for budget in 0..=cost + 1 {
        let with_discovery = run(budget, true);
        assert_eq!(with_discovery, run(budget, false), "Fuel {budget}");
        partial |= with_discovery.result.is_err() && with_discovery.calls == [19];
    }
    assert!(
        partial,
        "prior host mutations survive subsequent Fuel exhaustion"
    );
}

#[test]
fn discovering_capabilities_does_not_grant_them_or_override_live_revocation() {
    let total = Cell::new(0);
    let calls = RefCell::new(Vec::new());
    let host = counter_host(&total, &calls);
    assert_eq!(
        decode(&host.catalog_description("consumer.counter"))["operations"][0]["required_capability"],
        "counter.write"
    );
    assert!(host.compile(SOURCE, &[]).is_err());
    let program = host.compile(SOURCE, GRANTS).unwrap();
    let mut denied = host.start(&program, &[], Fuel::new(1000)).unwrap();
    assert!(denied.poll().is_err());
    assert!(calls.borrow().is_empty());

    let mut session = host.start(&program, GRANTS, Fuel::new(1000)).unwrap();
    let Poll::Request(work) = session.poll().unwrap() else {
        panic!("expected an Effect")
    };
    session.set_grants(&[]);
    let fuel = session.fuel_remaining();
    decode(&host.catalog_description("consumer.counter"));
    assert!(matches!(
        session.authorize(work).unwrap_err().error,
        Error::Denied
    ));
    assert_eq!(session.fuel_remaining(), fuel);
    assert_eq!(total.get(), 0);
    assert!(calls.borrow().is_empty());
}

#[test]
fn discovery_does_not_accept_cancelled_late_receipts_or_replay_committed_work() {
    let total = Cell::new(0);
    let calls = RefCell::new(Vec::new());
    let host = counter_host(&total, &calls);
    let program = host.compile(SOURCE, GRANTS).unwrap();
    let mut session = host.start(&program, GRANTS, Fuel::new(1000)).unwrap();
    let Poll::Request(work) = session.poll().unwrap() else {
        panic!("expected an Effect")
    };
    let authorized = session.authorize(work).unwrap();
    let receipt = session.dispatch(authorized).unwrap();
    assert_eq!(receipt.value(), &ScalarValue::Integer(19));
    assert!(session.cancel());
    let fuel = session.fuel_remaining();
    decode(&host.catalog_description("consumer.counter"));
    let rejected = session.accept(receipt).unwrap_err();
    assert_eq!(rejected.input.value(), &ScalarValue::Integer(19));
    assert_eq!(session.fuel_remaining(), fuel);
    assert!(matches!(
        session.poll().unwrap(),
        Poll::Terminal(EffectSessionEnd::Cancelled)
    ));
    assert_eq!(*calls.borrow(), [19]);
    assert_eq!(total.get(), 19);
}
