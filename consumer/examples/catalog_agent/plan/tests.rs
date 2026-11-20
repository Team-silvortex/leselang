use super::legacy::native;
use super::*;
use leselang_runtime_core::{
    ScalarType,
    discovery::{CatalogDescription, TypeDescription},
};
use serde_json::{Value, json};

const CATALOG: &[u8] = include_bytes!("../../../../fixtures/host-semantics/appkit-v1.json");

fn catalog() -> Value {
    serde_json::from_slice(CATALOG).unwrap()
}
fn bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}
fn change_catalog(change: impl FnOnce(&mut Value)) -> String {
    let mut value = catalog();
    change(&mut value);
    generate(&bytes(&value)).unwrap_err()
}
fn operation<'a>(catalog: &'a mut Value, name: &str) -> &'a mut Value {
    catalog["operations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|operation| operation["name"] == name)
        .unwrap()
}
fn report(text: &str) -> Value {
    let before = json!([
        {"identifier":"save","role":"button","title":"draft","value":"", "editable":false,"enabled":true,"clicks":0},
        {"identifier":"other","role":"button","title":"draft","value":"", "editable":false,"enabled":true,"clicks":0},
        {"identifier":"input","role":"text_field","title":"Message to save","value":"", "editable":true,"enabled":true,"clicks":0}
    ]);
    let mut after = before.clone();
    after[0]["title"] = json!("ready");
    after[0]["value"] = json!(text);
    after[0]["clicks"] = json!(1);
    after[2]["value"] = json!(text);
    json!({
        "schema_version":1,"kind":"appkit_run","profile":"appkit",
        "result":{"type":"boolean","value":true},"effects":9,"fuel_remaining":71,
        "native_before":before,"native_after":after
    })
}

#[test]
fn typescript_profile_reuses_source_but_requires_its_own_verified_report() {
    let mut description = catalog();
    description["profile"] = json!("tauri.typescript");
    assert_eq!(profile(&bytes(&description)).unwrap(), "tauri.typescript");
    assert_eq!(
        generate(&bytes(&description)).unwrap(),
        generate(CATALOG).unwrap()
    );
    let mut result = report("TypeScript ✓");
    result["kind"] = json!("tauri_run");
    result["profile"] = json!("tauri.typescript");
    result["fuel_remaining"] = json!(9000);
    result["verified"] = json!(true);
    result["native_before"][0]["title"] = json!("Save");
    result["native_after"][0]["title"] = json!("Save");
    assert!(verify_for("tauri.typescript", &bytes(&result), "TypeScript ✓").is_ok());
    assert!(verify(&bytes(&result), "TypeScript ✓").is_err());
    for invalid in [Value::Bool(false), Value::Null, json!("true")] {
        result["verified"] = invalid;
        assert!(verify_for("tauri.typescript", &bytes(&result), "TypeScript ✓").is_err());
    }
    result["verified"] = json!(true);
    result["native_after"][0]["value"] = json!("different");
    assert!(verify_for("tauri.typescript", &bytes(&result), "TypeScript ✓").is_err());
}

fn assert_generated_program_types(
    catalog_bytes: &[u8],
    expected_result: ScalarType,
    expected_parameters: &[&str],
) {
    assert_source_program_types(
        catalog_bytes,
        &generate(catalog_bytes).unwrap(),
        expected_result,
        expected_parameters,
    );
}

pub(super) fn assert_source_program_types(
    catalog_bytes: &[u8],
    source: &str,
    expected_result: ScalarType,
    expected_parameters: &[&str],
) {
    use leselang_hir_core::{
        catalog_program::{CatalogProgramLimits, compile_catalog_typed_program},
        catalog_source::{CatalogSourceEnvironment, CatalogSourceLimits},
        control_source::ControlSourceLimits,
        flow_typing::CallFlowEnvironment,
        pure_typing::{PureType, PureTypeEnvironment},
        source_call::{SourceCallHost, SourceCallLimits},
    };
    use leselang_runtime_core::{
        NamedParameter, OperationCatalog, OperationCatalogLimits, OperationSchema, ScalarTypeSet,
    };
    struct Environment<'a>(&'a CatalogDescription);
    impl PureTypeEnvironment<String, String> for Environment<'_> {
        type Result = String;
        fn field_type(&self, result: &String, field: &String) -> Option<ScalarType> {
            self.0
                .native_types
                .iter()
                .find(|native| native.name == *result)?
                .fields
                .iter()
                .find(|candidate| candidate.name == *field)
                .map(|field| field.scalar_type)
        }
        fn member_result(&self, _: &String, _: &str, _: &String) -> Option<String> {
            None
        }
    }
    impl<'s> CallFlowEnvironment<'s, String, String, TypeDescription> for Environment<'_> {
        fn call_native_argument_type(
            &self,
            operation: &String,
            name: &str,
            result: &String,
        ) -> bool {
            self.0
                .operations
                .iter()
                .find(|candidate| candidate.name == *operation)
                .and_then(|operation| {
                    operation
                        .parameters
                        .iter()
                        .find(|parameter| parameter.name == name)
                })
                .is_some_and(|parameter| parameter.types.contains(&native(result)))
        }
        fn call_result_type(
            &self,
            _: &String,
            result: &'s TypeDescription,
        ) -> Option<PureType<String>> {
            Some(match result {
                TypeDescription::Scalar(ty) => PureType::Scalar(*ty),
                TypeDescription::Native(name) => PureType::Result(name.clone()),
            })
        }
    }
    impl<'source, 's> CatalogSourceEnvironment<'source, 's, String, String, TypeDescription>
        for Environment<'_>
    {
        type Error = ();
        fn source_type_name(&self, name: &str) -> Option<String> {
            self.0
                .native_types
                .iter()
                .find(|native| native.name == name)
                .map(|native| native.name.clone())
        }
        fn source_operation(&self, operation: &String) -> Result<String, ()> {
            Ok(operation.clone())
        }
        fn source_field(
            &self,
            result: &String,
            name: &'source str,
        ) -> Result<Option<(String, ScalarType)>, ()> {
            Ok(self
                .field_type(result, &name.to_owned())
                .map(|ty| (name.to_owned(), ty)))
        }
        fn source_member(
            &self,
            _: &String,
            _: &'source str,
            _: &'source str,
        ) -> Result<Option<(String, String)>, ()> {
            Ok(None)
        }
    }
    let description = decode::<Catalog>(catalog_bytes, MAX_DESCRIPTION_BYTES, "catalog")
        .unwrap()
        .into_description()
        .unwrap();
    let parameters: Vec<Vec<_>> = description
        .operations
        .iter()
        .map(|operation| {
            operation
                .parameters
                .iter()
                .map(|parameter| {
                    let domain = parameter.types.iter().fold(
                        ScalarTypeSet::empty(),
                        |domain, ty| match ty {
                            TypeDescription::Scalar(ty) => domain.with(*ty),
                            TypeDescription::Native(_) => domain,
                        },
                    );
                    NamedParameter::required(parameter.name.as_str(), domain)
                })
                .collect()
        })
        .collect();
    let schemas: Vec<_> = description
        .operations
        .iter()
        .zip(&parameters)
        .map(|(operation, parameters)| OperationSchema {
            key: operation.name.clone(),
            parameters,
            result: operation.result.clone(),
            required_capability: operation.required_capability.clone(),
        })
        .collect();
    let catalog = OperationCatalog::new(
        1,
        &schemas,
        OperationCatalogLimits {
            max_operations: 5,
            max_parameters_per_operation: 3,
        },
    )
    .unwrap();
    let granted = description
        .operations
        .iter()
        .map(|operation| operation.required_capability.clone())
        .collect::<Vec<_>>();
    let host = SourceCallHost {
        catalog: &catalog,
        version: 1,
        granted: &granted,
    };
    let limits = CatalogProgramLimits {
        expression: CatalogSourceLimits {
            control: ControlSourceLimits {
                source: SourceCallLimits {
                    max_source_nodes: 512,
                    max_source_depth: 64,
                    max_lowered_nodes: 512,
                    max_lowered_depth: 64,
                    max_arguments: 8,
                },
                max_bindings: 16,
            },
            max_calls: 16,
        },
        max_functions: 8,
        max_parameters: 8,
        max_compilation_nodes: 1024,
        max_expansion_depth: 16,
    };
    let parsed = leselang_syntax::parse(source);
    let declarations: Vec<_> = parsed.function.iter().chain(&parsed.helpers).collect();
    let result = compile_catalog_typed_program::<String, _, (), (), _, _, _, _>(
        &declarations,
        "main",
        limits,
        &host,
        &Environment(&description),
    );
    let compiled = result.unwrap_or_else(|error| panic!("{error:?}\n{source}"));
    assert_eq!(compiled.result_type, PureType::Scalar(expected_result));
    assert_eq!(
        compiled
            .entry_parameters
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect::<Vec<_>>(),
        expected_parameters
    );
}

#[test]
fn generated_programs_fit_the_existing_host_compiler_limits() {
    assert_generated_program_types(CATALOG, ScalarType::Boolean, &["text"]);
    let mut tauri = catalog();
    tauri["profile"] = json!("tauri.typescript");
    assert_generated_program_types(&bytes(&tauri), ScalarType::Boolean, &["text"]);
    assert_generated_program_types(
        include_bytes!("../../../../fixtures/host-semantics/calculator-v1.json"),
        ScalarType::String,
        &[],
    );
}

#[test]
fn real_catalog_generates_typed_source_and_tolerates_declaration_order_and_extensions() {
    let generated = generate(CATALOG).unwrap();
    assert!(generated.len() < 16 * 1024);
    let parsed = leselang_syntax::parse(&generated);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    assert!(generated.contains("fn main(text: string)"));
    assert!(generated.contains("when: typed"));
    assert!(generated.contains("when: clicked"));
    assert!(generated.contains("input_ready(observed: before)"));
    assert!(generated.contains("input_ready(observed: after)"));
    let mut reordered = catalog();
    reordered["operations"].as_array_mut().unwrap().reverse();
    reordered["native_types"].as_array_mut().unwrap().reverse();
    reordered["extension"] = json!({"future":true});
    for op in reordered["operations"].as_array_mut().unwrap() {
        op["parameters"].as_array_mut().unwrap().reverse();
        op["extension"] = json!("ignored");
    }
    let reordered_source = generate(&bytes(&reordered)).unwrap();
    assert!(
        leselang_syntax::parse(&reordered_source)
            .diagnostics
            .is_empty()
    );
    // Named argument emission follows the supplied declaration order.
    assert!(reordered_source.contains("gui.set_text(text: text, expected_revision:"));
}

#[test]
fn unsupported_versions_profiles_and_semantically_unrecognized_operations_are_rejected() {
    for key in ["schema_version", "protocol_version", "catalog_version"] {
        assert!(change_catalog(|catalog| catalog[key] = json!(2)).contains("version"));
    }
    assert!(
        change_catalog(|catalog| catalog["profile"] = json!("another.appkit")).contains("profile")
    );
    assert!(
        change_catalog(|catalog| operation(catalog, "gui.click")["name"] = json!("gui.delete"))
            .contains("missing")
    );
    change_catalog(|catalog| {
        catalog["operations"].as_array_mut().unwrap().pop();
    });
    change_catalog(|catalog| {
        let another = operation(catalog, "gui.click").clone();
        catalog["operations"].as_array_mut().unwrap().push(another);
    });
}

#[test]
fn every_required_signature_and_capability_is_checked_without_coercions() {
    for name in [
        "gui.list",
        "gui.find",
        "gui.observe",
        "gui.set_text",
        "gui.click",
    ] {
        change_catalog(|catalog| {
            operation(catalog, name)["required_capability"] = json!("gui.admin")
        });
        change_catalog(|catalog| {
            operation(catalog, name)["result"] = json!({"kind":"scalar","name":"none"})
        });
    }
    for name in ["gui.find", "gui.observe", "gui.set_text", "gui.click"] {
        change_catalog(|catalog| {
            operation(catalog, name)["parameters"][0]["required"] = json!(false)
        });
        change_catalog(|catalog| {
            operation(catalog, name)["parameters"][0]["name"] = json!("different")
        });
        change_catalog(|catalog| operation(catalog, name)["parameters"][0]["types"] = json!([]));
        change_catalog(|catalog| {
            operation(catalog, name)["parameters"][0]["types"] = json!([
                {"kind":"scalar","name":"string"},{"kind":"scalar","name":"none"}
            ])
        });
    }
    change_catalog(|catalog| {
        catalog["native_types"][1]["fields"][2]["scalar_type"] = json!("optional_string")
    });
    change_catalog(|catalog| catalog["native_types"][1]["fields"][0]["name"] = json!("different"));
    change_catalog(|catalog| {
        catalog["native_types"][0]["fields"] = json!([{"name":"unsafe","scalar_type":"integer"}])
    });
}

#[test]
fn full_catalog_validation_checks_unused_declarations_and_domains() {
    for change in [
        ("name", json!("bad operation")),
        ("result", json!({"kind":"native","name":"undeclared"})),
    ] {
        let error = change_catalog(|catalog| operation(catalog, "gui.list")[change.0] = change.1);
        assert!(error.contains("invalid catalog metadata"));
    }
    let error = change_catalog(|catalog| {
        let parameter = operation(catalog, "gui.find")["parameters"][0].clone();
        operation(catalog, "gui.find")["parameters"]
            .as_array_mut()
            .unwrap()
            .push(parameter);
    });
    assert!(error.contains("invalid catalog metadata"));
    let error = change_catalog(|catalog| {
        let ty = operation(catalog, "gui.find")["parameters"][0]["types"][0].clone();
        operation(catalog, "gui.find")["parameters"][0]["types"]
            .as_array_mut()
            .unwrap()
            .push(ty);
    });
    assert!(error.contains("invalid catalog metadata"));
    let error = change_catalog(|catalog| {
        let field = catalog["native_types"][1]["fields"][0].clone();
        catalog["native_types"][1]["fields"]
            .as_array_mut()
            .unwrap()
            .push(field);
    });
    assert!(error.contains("invalid catalog metadata"));
    change_catalog(|catalog| {
        operation(catalog, "gui.list")["result"] = json!({"kind":"future","name":"boolean"})
    });
    change_catalog(|catalog| {
        catalog["native_types"][1]["fields"][0]["scalar_type"] = json!("number")
    });
    let error = change_catalog(|catalog| {
        let one = catalog["operations"][0].clone();
        catalog["operations"] = Value::Array(vec![one; 65]);
    });
    assert!(error.contains("too many operations"));
}

#[test]
fn json_ingress_rejects_duplicates_truncation_trailing_records_bad_utf8_and_oversize() {
    for invalid in [b"".as_slice(), b"{", b"{}{}", b"\xff", b"null", b"[]"] {
        assert!(generate(invalid).is_err());
        assert!(verify(invalid, "x").is_err());
    }
    let text = String::from_utf8(CATALOG.to_vec()).unwrap();
    let duplicated = format!("{{\"schema_version\":1,{}", &text[1..]);
    assert!(generate(duplicated.as_bytes()).is_err());
    let unknown_duplicate = format!("{{\"extra\":{{\"key\":1,\"key\":2}},{}", &text[1..]);
    assert!(generate(unknown_duplicate.as_bytes()).is_err());
    assert!(generate(&vec![b' '; MAX_DESCRIPTION_BYTES + 1]).is_err());
    assert!(verify(&vec![b' '; MAX_REPORT_BYTES + 1], "x").is_err());
    let deep = format!("{}0{}", "[".repeat(130), "]".repeat(130));
    assert!(generate(deep.as_bytes()).is_err());
    let mut padded = CATALOG.to_vec();
    padded.resize(MAX_DESCRIPTION_BYTES, b' ');
    assert!(generate(&padded).is_ok());
}

#[test]
fn verifies_boolean_and_native_evidence_with_text_as_data() {
    for text in [
        "",
        "LeseLang ✓",
        "quoted \" \\ newline\n保存",
        "fn main() = false",
    ] {
        let mut valid = report(text);
        valid["native_after"].as_array_mut().unwrap().reverse();
        valid["future"] = json!({"diagnostic": 1});
        assert_eq!(
            verify(&bytes(&valid), text).unwrap(),
            Verified {
                effects: 9,
                fuel_remaining: 71
            }
        );
        assert!(verify(&bytes(&valid), "wrong expectation").is_err());
    }
    let mut zero_fuel = report("x");
    zero_fuel["fuel_remaining"] = json!(0);
    assert!(verify(&bytes(&zero_fuel), "x").is_ok());
}

#[test]
fn completed_process_or_truthy_result_is_insufficient_for_verification() {
    for (key, value) in [
        ("schema_version", json!(2)),
        ("kind", json!("another_run")),
        ("profile", json!("macos_ax.controls")),
        ("effects", json!(8)),
        ("effects", json!(10)),
        ("fuel_remaining", json!(1001)),
        ("fuel_remaining", json!(-1)),
    ] {
        let mut invalid = report("x");
        invalid[key] = value;
        assert!(verify(&bytes(&invalid), "x").is_err());
    }
    for result in [
        json!({"type":"boolean","value":false}),
        json!({"type":"boolean","value":"true"}),
        json!({"type":"string","value":"true"}),
        json!({"type":"integer","value":1}),
        json!({"type":"none","value":null}),
        json!({"type":"boolean","value":null}),
    ] {
        let mut invalid = report("x");
        invalid["result"] = result;
        assert!(verify(&bytes(&invalid), "x").is_err());
    }
    let valid = serde_json::to_string(&report("x")).unwrap();
    let duplicate = valid.replacen("\"effects\":9", "\"effects\":9,\"effects\":9", 1);
    assert!(verify(duplicate.as_bytes(), "x").is_err());
    assert!(verify(format!("{valid}\n{valid}\n").as_bytes(), "x").is_err());
}

#[test]
fn native_state_mismatches_and_unrelated_mutations_fail_closed() {
    for (index, key, value) in [
        (2, "value", json!("different")),
        (2, "role", json!("button")),
        (2, "enabled", json!(false)),
        (2, "editable", json!(false)),
        (2, "title", json!("changed")),
        (2, "clicks", json!(1)),
        (0, "value", json!("different")),
        (0, "role", json!("text_field")),
        (0, "enabled", json!(false)),
        (0, "editable", json!(true)),
        (0, "title", json!("draft")),
        (0, "clicks", json!(0)),
        (0, "clicks", json!(2)),
        (1, "title", json!("changed")),
    ] {
        let mut invalid = report("x");
        invalid["native_after"][index][key] = value;
        assert!(verify(&bytes(&invalid), "x").is_err(), "{index} {key}");
    }
    for side in ["native_before", "native_after"] {
        let mut invalid = report("x");
        invalid[side][0]["identifier"] = json!("other");
        assert!(verify(&bytes(&invalid), "x").is_err());
        let mut invalid = report("x");
        invalid[side].as_array_mut().unwrap().pop();
        assert!(verify(&bytes(&invalid), "x").is_err());
        let mut invalid = report("x");
        invalid[side][2]["value"] = json!("x".repeat(4097));
        assert!(verify(&bytes(&invalid), "x").is_err());
    }
    let mut overflow = report("x");
    overflow["native_before"][0]["clicks"] = json!(u64::MAX);
    assert!(verify(&bytes(&overflow), "x").is_err());
    assert!(verify(&bytes(&report("x")), &"x".repeat(4097)).is_err());
}
