//! The Calculator strategy shares discovery and process driving with the form
//! strategies. Its application semantics are deliberately explicit: 1 + 2 = 3.
use super::{
    CatalogDescription, MAX_REPORT_BYTES, ScalarType, Verified, call, decode, native,
    require_operation, typed,
};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

pub(super) const PROFILE: &str = "macos_ax.calculator";
const BUTTONS: [&str; 4] = ["one", "add", "two", "equals"];

pub(super) fn supported(catalog: &CatalogDescription) -> Result<(), String> {
    if catalog.profile != PROFILE
        || catalog.operations.len() != 3
        || catalog.native_types.len() != 2
    {
        return Err("this strategy requires the three-operation Calculator profile".into());
    }
    require_operation(
        catalog,
        "gui.find",
        &[("name", typed(ScalarType::String))],
        native("object"),
        "gui.read",
    )?;
    require_operation(
        catalog,
        "gui.observe",
        &[("target", native("object"))],
        native("snapshot"),
        "gui.read",
    )?;
    require_operation(
        catalog,
        "gui.press",
        &[
            ("target", native("object")),
            ("expected_revision", typed(ScalarType::Integer)),
            ("button", typed(ScalarType::String)),
        ],
        typed(ScalarType::Boolean),
        "gui.write",
    )?;
    let object = catalog.native_types.iter().find(|ty| ty.name == "object");
    let snapshot = catalog.native_types.iter().find(|ty| ty.name == "snapshot");
    let fields = [
        ("role", ScalarType::String),
        ("revision", ScalarType::Integer),
        ("display", ScalarType::String),
        ("buttons", ScalarType::StringList),
        ("last_expression", ScalarType::OptionalString),
    ];
    if object.is_none_or(|object| !object.fields.is_empty())
        || snapshot.is_none_or(|snapshot| {
            snapshot.fields.len() != fields.len()
                || fields.iter().any(|(name, ty)| {
                    !snapshot
                        .fields
                        .iter()
                        .any(|field| field.name == *name && field.scalar_type == *ty)
                })
        })
    {
        return Err("native types do not match the supported Calculator fields".into());
    }
    Ok(())
}

pub(super) fn generate(catalog: &CatalogDescription) -> Result<String, String> {
    let find = call(catalog, "gui.find", &[("name", "\"calculator\"")]);
    let observe = call(catalog, "gui.observe", &[("target", "target")]);
    let press = |observed: &str, button: &str| {
        call(
            catalog,
            "gui.press",
            &[
                ("target", "target"),
                (
                    "expected_revision",
                    &format!("field(value: {observed}, name: \"revision\")"),
                ),
                ("button", &format!("\"{button}\"")),
            ],
        )
    };
    let one = press("before", "one");
    let add = press("after_one", "add");
    let two = press("after_add", "two");
    let equals = press("after_two", "equals");
    let source = format!(
        r#"// Generated from the validated Calculator catalog. No clear or retry operation.
fn calculation_matches(observed: snapshot) = and(
  left: eq(left: field(value: observed, name: "display"), right: "3"),
  right: eq(left: value_or(left: field(value: observed, name: "last_expression"), right: ""), right: "1 + 2")
)
fn main() = bind(
  target: {find}, body: bind(
    before: {observe}, body: choose(
      when: eq(left: field(value: before, name: "display"), right: "0"),
      then: bind(one: {one}, body: bind(
        after_one: {observe}, body: bind(add: {add}, body: bind(
          after_add: {observe}, body: bind(two: {two}, body: bind(
            after_two: {observe}, body: bind(equals: {equals}, body: bind(
              after: {observe}, body: choose(
                when: and(
                  left: and(left: and(left: one, right: add), right: and(left: two, right: equals)),
                  right: calculation_matches(observed: after)
                ),
                then: field(value: after, name: "display"), otherwise: "verification failed"
              )
            ))
          ))
        ))
      )), otherwise: "initial display mismatch"
    )
  )
)
"#
    );
    if source.len() > 16 * 1024 {
        return Err("generated source exceeds host source bound".into());
    }
    Ok(source)
}

#[derive(Deserialize)]
struct Report {
    schema_version: u32,
    kind: String,
    profile: String,
    result: super::ResultValue,
    effects: usize,
    fuel_remaining: u64,
    verified: bool,
    native_before: Vec<Snapshot>,
    native_after: Vec<Snapshot>,
}
#[derive(Deserialize)]
struct Snapshot {
    identifier: String,
    role: String,
    display: String,
    // Value, instead of Option, requires the nullable field to be present.
    last_expression: Value,
    buttons: Vec<Button>,
}
#[derive(Deserialize)]
struct Button {
    name: String,
    enabled: bool,
}

fn snapshot(values: &[Snapshot]) -> Result<&Snapshot, String> {
    let [value] = values else {
        return Err("Calculator report must describe exactly one window".into());
    };
    if value.identifier != "calculator"
        || value.role != "AXWindow"
        || value.display.len() > 4096
        || !(value.last_expression.is_null()
            || value
                .last_expression
                .as_str()
                .is_some_and(|expression| expression.len() <= 4096))
    {
        return Err("Calculator report has invalid public snapshot fields".into());
    }
    Ok(value)
}

fn buttons(snapshot: &Snapshot) -> Result<BTreeMap<&str, bool>, String> {
    if snapshot.buttons.len() != BUTTONS.len() {
        return Err("Calculator report must identify every admitted button".into());
    }
    let mut selected = BTreeMap::new();
    for button in &snapshot.buttons {
        if !BUTTONS.contains(&button.name.as_str())
            || !button.enabled
            || selected
                .insert(button.name.as_str(), button.enabled)
                .is_some()
        {
            return Err("Calculator report has missing, duplicate or disabled buttons".into());
        }
    }
    Ok(selected)
}

pub(super) fn verify(report_bytes: &[u8]) -> Result<Verified, String> {
    let report: Report = decode(report_bytes, MAX_REPORT_BYTES, "Calculator run report")?;
    if report.schema_version != 1
        || report.kind != "macos_ax_calculator_run"
        || report.profile != PROFILE
        || !report.verified
    {
        return Err(
            "Calculator report does not match the selected catalog and verified host run".into(),
        );
    }
    if report.result.kind != "string" || report.result.value != "3" {
        return Err("Calculator program did not return the expected result".into());
    }
    if report.effects != 10 || report.fuel_remaining > 2000 {
        return Err("Calculator report has an unexpected Effect count or Fuel budget".into());
    }
    let before = snapshot(&report.native_before)?;
    let after = snapshot(&report.native_after)?;
    if before.display != "0" || after.display != "3" || after.last_expression != "1 + 2" {
        return Err(
            "Calculator display and expression do not establish the requested calculation".into(),
        );
    }
    if buttons(before)? != buttons(after)? {
        return Err("Calculator button state changed".into());
    }
    Ok(Verified {
        effects: report.effects,
        fuel_remaining: report.fuel_remaining,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const CATALOG: &[u8] = include_bytes!("fixtures/calculator-v1.json");
    fn bytes(value: &Value) -> Vec<u8> {
        serde_json::to_vec(value).unwrap()
    }
    fn catalog() -> Value {
        serde_json::from_slice(CATALOG).unwrap()
    }
    fn report() -> Value {
        let before = json!([{
            "identifier":"calculator","role":"AXWindow","display":"0","last_expression":null,
            "buttons":BUTTONS.iter().map(|name|json!({"name":name,"enabled":true})).collect::<Vec<_>>()
        }]);
        let mut after = before.clone();
        after[0]["display"] = json!("3");
        after[0]["last_expression"] = json!("1 + 2");
        json!({
            "schema_version":1,"kind":"macos_ax_calculator_run","profile":PROFILE,
            "result":{"type":"string","value":"3"},"effects":10,"fuel_remaining":500,
            "verified":true,"native_before":before,"native_after":after,
        })
    }

    #[test]
    fn actual_calculator_catalog_selects_a_different_ten_effect_strategy() {
        assert_eq!(super::super::profile(CATALOG).unwrap(), PROFILE);
        let task = super::super::task(PROFILE).unwrap();
        assert_eq!(task.name, "calculate_one_plus_two");
        assert!(!task.takes_text);
        assert_eq!(task.expected_effects, 10);
        let source = super::super::generate(CATALOG).unwrap();
        // The host's deterministic backend executes this same artifact, without
        // linking this consumer or a GUI framework into either other's crate.
        assert_eq!(source, include_str!("fixtures/calculator.lese"));
        assert!(source.len() < 16 * 1024);
        let parsed = leselang_syntax::parse(&source);
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        assert!(source.contains("fn main()"));
        assert_eq!(source.matches("gui.find(").count(), 1);
        assert_eq!(source.matches("gui.observe(").count(), 5);
        assert_eq!(source.matches("gui.press(").count(), 4);
        assert!(source.contains("otherwise: \"initial display mismatch\""));
        assert!(!source.contains("gui.clear"));
        let mut reordered = catalog();
        reordered["operations"].as_array_mut().unwrap().reverse();
        reordered["native_types"].as_array_mut().unwrap().reverse();
        for operation in reordered["operations"].as_array_mut().unwrap() {
            operation["parameters"].as_array_mut().unwrap().reverse();
        }
        let source = super::super::generate(&bytes(&reordered)).unwrap();
        assert!(leselang_syntax::parse(&source).diagnostics.is_empty());
        assert!(source.contains("gui.press(button: \"one\", expected_revision:"));
    }

    #[test]
    fn all_calculator_operations_and_fields_are_validated_before_planning() {
        for index in 0..3 {
            for (key, value) in [
                ("required_capability", json!("gui.admin")),
                ("result", json!({"kind":"scalar","name":"none"})),
                ("name", json!("gui.unknown")),
            ] {
                let mut invalid = catalog();
                invalid["operations"][index][key] = value;
                assert!(super::super::generate(&bytes(&invalid)).is_err());
            }
            for (key, value) in [
                ("required", json!(false)),
                ("name", json!("wrong")),
                ("types", json!([{"kind":"scalar","name":"optional_string"}])),
            ] {
                let mut invalid = catalog();
                invalid["operations"][index]["parameters"][0][key] = value;
                assert!(super::super::generate(&bytes(&invalid)).is_err());
            }
        }
        for index in 0..5 {
            let mut invalid = catalog();
            invalid["native_types"][1]["fields"][index]["scalar_type"] = json!("none");
            assert!(super::super::generate(&bytes(&invalid)).is_err());
        }
        let mut invalid = catalog();
        invalid["native_types"][0]["fields"] = json!([{"name":"display","scalar_type":"string"}]);
        assert!(super::super::generate(&bytes(&invalid)).is_err());
        invalid = catalog();
        invalid["operations"].as_array_mut().unwrap().pop();
        assert!(super::super::generate(&bytes(&invalid)).is_err());
        invalid = catalog();
        invalid["native_types"][1]["fields"]
            .as_array_mut()
            .unwrap()
            .pop();
        assert!(super::super::generate(&bytes(&invalid)).is_err());
        invalid = catalog();
        invalid["profile"] = json!("tauri.typescript");
        assert!(super::super::generate(&bytes(&invalid)).is_err());
    }

    #[test]
    fn calculator_verification_requires_result_expression_and_independent_evidence() {
        let mut valid = report();
        valid["native_after"][0]["buttons"]
            .as_array_mut()
            .unwrap()
            .reverse();
        valid["native_before"][0]["last_expression"] = json!("older calculation");
        assert_eq!(
            verify(&bytes(&valid)).unwrap(),
            Verified {
                effects: 10,
                fuel_remaining: 500
            }
        );
        assert!(super::super::verify_for(PROFILE, &bytes(&valid), "").is_ok());
        assert!(super::super::verify_for("tauri.typescript", &bytes(&valid), "").is_err());
        for (key, value) in [
            ("schema_version", json!(2)),
            ("profile", json!("tauri.typescript")),
            ("kind", json!("tauri_run")),
            ("verified", json!(false)),
            ("verified", json!("true")),
            ("effects", json!(9)),
            ("fuel_remaining", json!(2001)),
            ("fuel_remaining", json!(-1)),
            ("result", json!({"type":"boolean","value":true})),
            (
                "result",
                json!({"type":"string","value":"initial display mismatch"}),
            ),
        ] {
            let mut invalid = report();
            invalid[key] = value;
            assert!(verify(&bytes(&invalid)).is_err(), "{key}");
        }
        let mut valid = report();
        valid["fuel_remaining"] = json!(0);
        assert!(verify(&bytes(&valid)).is_ok());
    }

    #[test]
    fn matching_number_with_wrong_expression_or_native_state_is_rejected() {
        for (side, key, value) in [
            ("native_before", "display", json!("42")),
            ("native_after", "display", json!("4")),
            ("native_after", "last_expression", json!("0 + 3")),
            ("native_after", "last_expression", Value::Null),
            ("native_before", "last_expression", json!(false)),
            ("native_before", "role", json!("AXButton")),
            ("native_after", "identifier", json!("other")),
            ("native_after", "display", json!("x".repeat(4097))),
            ("native_before", "last_expression", json!("x".repeat(4097))),
        ] {
            let mut invalid = report();
            invalid[side][0][key] = value;
            assert!(verify(&bytes(&invalid)).is_err(), "{side} {key}");
        }
        for side in ["native_before", "native_after"] {
            for (key, value) in [
                ("enabled", json!(false)),
                ("name", json!("clear")),
                ("name", json!("equals")),
            ] {
                let mut invalid = report();
                invalid[side][0]["buttons"][0][key] = value;
                assert!(verify(&bytes(&invalid)).is_err());
            }
            let mut invalid = report();
            invalid[side][0]
                .as_object_mut()
                .unwrap()
                .remove("last_expression");
            assert!(verify(&bytes(&invalid)).is_err());
            let mut invalid = report();
            invalid[side][0]["buttons"].as_array_mut().unwrap().pop();
            assert!(verify(&bytes(&invalid)).is_err());
            let mut invalid = report();
            let duplicate = invalid[side][0].clone();
            invalid[side].as_array_mut().unwrap().push(duplicate);
            assert!(verify(&bytes(&invalid)).is_err());
        }
        let raw = serde_json::to_string(&report()).unwrap();
        assert!(
            verify(
                raw.replacen("\"effects\":10", "\"effects\":10,\"effects\":10", 1)
                    .as_bytes()
            )
            .is_err()
        );
    }
}
