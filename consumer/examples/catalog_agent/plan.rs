//! Auditable GUI strategies selected only after complete catalog validation.
//! A type signature alone does not tell an agent what an arbitrary operation does.
#[path = "calculator.rs"]
mod calculator;
#[path = "semantic.rs"]
mod semantic;

use std::{collections::BTreeSet, fmt};

use leselang_runtime_core::discovery::semantics::{
    ExecutionStyle, ReportContract, SemanticDescription, TaskStep,
};
use leselang_runtime_core::{
    ScalarType,
    discovery::{
        CatalogDescription, FieldDescription, MAX_DESCRIPTION_BYTES, NativeTypeDescription,
        OperationDescription, ParameterDescription, TypeDescription,
    },
};
use serde::{
    Deserialize,
    de::{self, DeserializeOwned, MapAccess, SeqAccess, Visitor},
};

pub const MAX_REPORT_BYTES: usize = 128 * 1024;

#[derive(Debug, Eq, PartialEq)]
pub struct Verified {
    pub effects: usize,
    pub fuel_remaining: u64,
}

/// A validated plan owns the exact report contract used after execution.
pub struct PreparedPlan {
    pub profile: String,
    pub task: String,
    pub entry: String,
    pub takes_text: bool,
    pub input_name: Option<String>,
    pub execution: ExecutionStyle,
    pub required_capabilities: Vec<String>,
    pub expected_effects: usize,
    pub checks: Vec<String>,
    pub source: String,
    pub semantic: bool,
    verification: Option<ReportContract>,
}

impl PreparedPlan {
    pub fn verify(&self, report_bytes: &[u8], text: &str) -> Result<Verified, String> {
        if let Some(contract) = &self.verification {
            semantic::verify(
                &self.profile,
                self.expected_effects,
                contract,
                self.input_name.as_deref(),
                report_bytes,
                text,
            )
        } else if self.profile == "appkit" {
            verify(report_bytes, text)
        } else {
            verify_for(&self.profile, report_bytes, text)
        }
    }
}

/// Present semantic metadata is authoritative for planning, including errors.
/// Only an absent extension may use the original known-profile strategies.
pub fn prepare(catalog_bytes: &[u8], requested_task: &str) -> Result<PreparedPlan, String> {
    let value: serde_json::Value = decode(catalog_bytes, MAX_DESCRIPTION_BYTES, "catalog")?;
    if let Some(extension) = value.get("semantics") {
        let catalog = decode::<Catalog>(catalog_bytes, MAX_DESCRIPTION_BYTES, "catalog")?
            .into_semantic_description()?;
        let semantics: SemanticDescription = serde_json::from_value(extension.clone())
            .map_err(|_| "catalog semantic metadata has an invalid structure or field type")?;
        catalog
            .to_json_with_semantics(&semantics)
            .map_err(|error| format!("invalid semantic catalog metadata: {error}"))?;
        let selected = semantics
            .tasks
            .iter()
            .find(|task| task.id == requested_task)
            .ok_or("requested task is not declared by the discovered host")?;
        let mut capabilities = BTreeSet::new();
        let mut effects = 0;
        for step in &selected.steps {
            if let TaskStep::Call { operation, .. } = step {
                effects += 1;
                capabilities.insert(
                    catalog
                        .operations
                        .iter()
                        .find(|candidate| candidate.name == *operation)
                        .expect("validated operation")
                        .required_capability
                        .clone(),
                );
            }
        }
        Ok(PreparedPlan {
            profile: catalog.profile.clone(),
            task: selected.id.clone(),
            entry: "main".into(),
            takes_text: !selected.inputs.is_empty(),
            input_name: selected.inputs.first().map(|input| input.name.clone()),
            execution: selected.execution.clone(),
            required_capabilities: capabilities.into_iter().collect(),
            expected_effects: effects,
            checks: selected
                .verification
                .predicates
                .iter()
                .map(|predicate| predicate.id.clone())
                .collect(),
            source: semantic::generate(&catalog, selected)?,
            semantic: true,
            verification: Some(selected.verification.clone()),
        })
    } else {
        let profile = profile(catalog_bytes)?;
        let contract = task(&profile)?;
        if requested_task != contract.name {
            return Err("requested task does not match the discovered host contract".into());
        }
        let execution = if !contract.takes_text {
            ExecutionStyle::NoText
        } else if profile == "tauri.typescript" {
            ExecutionStyle::Text
        } else {
            ExecutionStyle::NamedText
        };
        Ok(PreparedPlan {
            profile,
            task: contract.name.into(),
            entry: contract.entry.into(),
            takes_text: contract.takes_text,
            input_name: contract.takes_text.then(|| "text".into()),
            execution,
            required_capabilities: vec!["gui.read".into(), "gui.write".into()],
            expected_effects: contract.expected_effects,
            checks: contract
                .checks
                .iter()
                .map(|check| (*check).into())
                .collect(),
            source: generate(catalog_bytes)?,
            semantic: false,
            verification: None,
        })
    }
}

/// Planning metadata is selected by the validated profile, not a CLI override.
#[derive(Debug, Eq, PartialEq)]
pub struct TaskContract {
    pub name: &'static str,
    pub entry: &'static str,
    pub expected_effects: usize,
    pub takes_text: bool,
    pub checks: &'static [&'static str],
}

pub fn task(profile: &str) -> Result<TaskContract, String> {
    match profile {
        "appkit" | "tauri.typescript" => Ok(TaskContract {
            name: "fill_and_save",
            entry: "main",
            expected_effects: 9,
            takes_text: true,
            checks: &[
                "program_true",
                "input_value",
                "saved_value",
                "save_click_count",
                "other_unchanged",
            ],
        }),
        calculator::PROFILE => Ok(TaskContract {
            name: "calculate_one_plus_two",
            entry: "main",
            expected_effects: 10,
            takes_text: false,
            checks: &[
                "program_result",
                "initial_zero",
                "display_value",
                "last_expression",
                "button_state_unchanged",
            ],
        }),
        _ => Err("no known task contract for this catalog profile".into()),
    }
}

// Check every object, including unknown extension fields, before typed decoding.
// serde's normal recursion limit and the byte bound also apply to this pass.
struct UniqueJson;
impl<'de> Deserialize<'de> for UniqueJson {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct UniqueVisitor;
        impl<'de> Visitor<'de> for UniqueVisitor {
            type Value = UniqueJson;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("JSON with no repeated object keys")
            }
            fn visit_bool<E: de::Error>(self, _: bool) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_i64<E: de::Error>(self, _: i64) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_u64<E: de::Error>(self, _: u64) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_f64<E: de::Error>(self, _: f64) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_str<E: de::Error>(self, _: &str) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_unit<E: de::Error>(self) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut values: A) -> Result<UniqueJson, A::Error> {
                while values.next_element::<UniqueJson>()?.is_some() {}
                Ok(UniqueJson)
            }
            fn visit_map<A: MapAccess<'de>>(self, mut values: A) -> Result<UniqueJson, A::Error> {
                let mut names = BTreeSet::new();
                while let Some(name) = values.next_key::<String>()? {
                    if !names.insert(name) {
                        return Err(de::Error::custom("repeated object key"));
                    }
                    values.next_value::<UniqueJson>()?;
                }
                Ok(UniqueJson)
            }
        }
        deserializer.deserialize_any(UniqueVisitor)
    }
}

fn decode<T: DeserializeOwned>(bytes: &[u8], limit: usize, what: &str) -> Result<T, String> {
    if bytes.is_empty() || bytes.len() > limit {
        return Err(format!("{what} is empty or exceeds its byte limit"));
    }
    serde_json::from_slice::<UniqueJson>(bytes).map_err(|_| {
        format!("{what} must contain one complete JSON value with unique object keys")
    })?;
    serde_json::from_slice(bytes)
        .map_err(|_| format!("{what} has an invalid structure or field type"))
}

#[derive(Deserialize)]
struct Catalog {
    schema_version: u32,
    protocol_version: u32,
    catalog_version: u32,
    profile: String,
    operations: Vec<Operation>,
    native_types: Vec<NativeType>,
}
#[derive(Deserialize)]
struct Operation {
    name: String,
    parameters: Vec<Parameter>,
    result: Type,
    required_capability: String,
}
#[derive(Deserialize)]
struct Parameter {
    name: String,
    required: bool,
    types: Vec<Type>,
}
#[derive(Deserialize)]
#[serde(tag = "kind")]
enum Type {
    #[serde(rename = "scalar")]
    Scalar { name: String },
    #[serde(rename = "native")]
    Native { name: String },
}
#[derive(Deserialize)]
struct NativeType {
    name: String,
    fields: Vec<Field>,
}
#[derive(Deserialize)]
struct Field {
    name: String,
    scalar_type: String,
}

fn scalar(name: &str) -> Result<ScalarType, String> {
    match name {
        "integer" => Ok(ScalarType::Integer),
        "boolean" => Ok(ScalarType::Boolean),
        "string" => Ok(ScalarType::String),
        "none" => Ok(ScalarType::None),
        "optional_string" => Ok(ScalarType::OptionalString),
        "string_list" => Ok(ScalarType::StringList),
        _ => Err("catalog uses an unsupported scalar type".into()),
    }
}
impl Type {
    fn into_description(self) -> Result<TypeDescription, String> {
        match self {
            Self::Scalar { name } => scalar(&name).map(TypeDescription::Scalar),
            Self::Native { name } => Ok(TypeDescription::Native(name)),
        }
    }
}
impl Catalog {
    fn into_description(self) -> Result<CatalogDescription, String> {
        if self.catalog_version != 1 {
            return Err(
                "this strategy requires catalog schema, protocol and catalog version 1".into(),
            );
        }
        self.into_semantic_description()
    }

    fn into_semantic_description(self) -> Result<CatalogDescription, String> {
        if self.schema_version != 1 || self.protocol_version != 1 {
            return Err("this strategy requires catalog schema and protocol version 1".into());
        }
        let description = CatalogDescription {
            profile: self.profile,
            catalog_version: self.catalog_version,
            operations: self
                .operations
                .into_iter()
                .map(|operation| {
                    Ok(OperationDescription {
                        name: operation.name,
                        parameters: operation
                            .parameters
                            .into_iter()
                            .map(|parameter| {
                                Ok(ParameterDescription {
                                    name: parameter.name,
                                    required: parameter.required,
                                    types: parameter
                                        .types
                                        .into_iter()
                                        .map(Type::into_description)
                                        .collect::<Result<_, String>>()?,
                                })
                            })
                            .collect::<Result<_, String>>()?,
                        result: operation.result.into_description()?,
                        required_capability: operation.required_capability,
                    })
                })
                .collect::<Result<_, String>>()?,
            native_types: self
                .native_types
                .into_iter()
                .map(|native| {
                    Ok(NativeTypeDescription {
                        name: native.name,
                        fields: native
                            .fields
                            .into_iter()
                            .map(|field| {
                                Ok(FieldDescription {
                                    name: field.name,
                                    scalar_type: scalar(&field.scalar_type)?,
                                })
                            })
                            .collect::<Result<_, String>>()?,
                    })
                })
                .collect::<Result<_, String>>()?,
        };
        // The metadata codec is only a validator here. This does not deserialize
        // an OperationCatalog, ObjectRef or anything granting execution authority.
        description
            .to_json()
            .map_err(|error| format!("invalid catalog metadata: {error}"))?;
        Ok(description)
    }
}

fn native(name: &str) -> TypeDescription {
    TypeDescription::Native(name.into())
}
fn typed(ty: ScalarType) -> TypeDescription {
    TypeDescription::Scalar(ty)
}

fn require_operation(
    catalog: &CatalogDescription,
    name: &str,
    parameters: &[(&str, TypeDescription)],
    result: TypeDescription,
    capability: &str,
) -> Result<(), String> {
    let operation = catalog
        .operations
        .iter()
        .find(|operation| operation.name == name)
        .ok_or_else(|| format!("required operation {name} is missing"))?;
    if operation.result != result
        || operation.required_capability != capability
        || operation.parameters.len() != parameters.len()
        || parameters.iter().any(|(name, ty)| {
            !operation.parameters.iter().any(|parameter| {
                parameter.name == *name
                    && parameter.required
                    && parameter.types.as_slice() == [ty.clone()]
            })
        })
    {
        return Err(format!(
            "operation {name} does not match the supported GUI contract"
        ));
    }
    Ok(())
}

fn supported(catalog: &CatalogDescription) -> Result<(), String> {
    if catalog.profile == calculator::PROFILE {
        return calculator::supported(catalog);
    }
    if !matches!(catalog.profile.as_str(), "appkit" | "tauri.typescript")
        || catalog.operations.len() != 5
        || catalog.native_types.len() != 2
    {
        return Err("this strategy requires a known five-operation form profile".into());
    }
    require_operation(
        catalog,
        "gui.list",
        &[],
        typed(ScalarType::StringList),
        "gui.read",
    )?;
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
        "gui.set_text",
        &[
            ("target", native("object")),
            ("expected_revision", typed(ScalarType::Integer)),
            ("text", typed(ScalarType::String)),
        ],
        typed(ScalarType::Boolean),
        "gui.write",
    )?;
    require_operation(
        catalog,
        "gui.click",
        &[
            ("target", native("object")),
            ("expected_revision", typed(ScalarType::Integer)),
        ],
        typed(ScalarType::Boolean),
        "gui.write",
    )?;
    let object = catalog
        .native_types
        .iter()
        .find(|native| native.name == "object");
    let snapshot = catalog
        .native_types
        .iter()
        .find(|native| native.name == "snapshot");
    let fields = [
        ("identifier", ScalarType::String),
        ("role", ScalarType::String),
        ("value", ScalarType::String),
        ("editable", ScalarType::Boolean),
        ("revision", ScalarType::Integer),
        ("title", ScalarType::String),
        ("clicks", ScalarType::Integer),
        ("enabled", ScalarType::Boolean),
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
        return Err("native types do not match the supported AppKit fields".into());
    }
    Ok(())
}

fn call(catalog: &CatalogDescription, name: &str, values: &[(&str, &str)]) -> String {
    let operation = catalog
        .operations
        .iter()
        .find(|operation| operation.name == name)
        .expect("validated operation");
    let arguments = operation
        .parameters
        .iter()
        .map(|parameter| {
            let value = values
                .iter()
                .find(|(name, _)| parameter.name == *name)
                .expect("strategy supplies every validated parameter")
                .1;
            format!("{}: {value}", parameter.name)
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("{}({arguments})", operation.name)
}

/// Identify a known workflow only after validating the complete public catalog.
pub fn profile(catalog_bytes: &[u8]) -> Result<String, String> {
    let catalog =
        decode::<Catalog>(catalog_bytes, MAX_DESCRIPTION_BYTES, "catalog")?.into_description()?;
    supported(&catalog)?;
    Ok(catalog.profile)
}

/// Generate a known GUI workflow from the validated catalog's declarations.
/// Form text is supplied later as a typed scalar argument, never as source text.
pub fn generate(catalog_bytes: &[u8]) -> Result<String, String> {
    let catalog =
        decode::<Catalog>(catalog_bytes, MAX_DESCRIPTION_BYTES, "catalog")?.into_description()?;
    supported(&catalog)?;
    if catalog.profile == calculator::PROFILE {
        return calculator::generate(&catalog);
    }
    let list = call(&catalog, "gui.list", &[]);
    let find_input = call(&catalog, "gui.find", &[("name", "\"input\"")]);
    let find_save = call(&catalog, "gui.find", &[("name", "\"save\"")]);
    let observe = call(&catalog, "gui.observe", &[("target", "target")]);
    let revision = "field(value: before, name: \"revision\")";
    let set_text = call(
        &catalog,
        "gui.set_text",
        &[
            ("target", "target"),
            ("expected_revision", revision),
            ("text", "text"),
        ],
    );
    let click = call(
        &catalog,
        "gui.click",
        &[("target", "target"), ("expected_revision", revision)],
    );
    let source = format!(
        r#"// Generated from the validated form catalog; text remains a task input.
fn has_control(ids: string_list, wanted: string) = fold(
  found: false, items: ids, item: "candidate",
  next: or(left: found, right: eq(left: candidate, right: wanted)), limit: 64
)
fn input_ready(observed: snapshot) = and(
  left: eq(left: field(value: observed, name: "role"), right: "text_field"),
  right: and(left: field(value: observed, name: "editable"), right: field(value: observed, name: "enabled"))
)
fn save(text: string) = bind(
  target: {find_save}, body: bind(
    before: {observe}, body: choose(
      when: and(left: eq(left: field(value: before, name: "role"), right: "button"), right: field(value: before, name: "enabled")),
      then: bind(clicked: {click}, body: choose(
        when: clicked, then: bind(after: {observe}, body: and(
          left: eq(left: field(value: after, name: "value"), right: text),
          right: eq(left: field(value: after, name: "clicks"), right: add(left: field(value: before, name: "clicks"), right: 1))
        )), otherwise: false
      )), otherwise: false
    )
  )
)
fn fill(text: string) = bind(
  target: {find_input}, body: bind(
    before: {observe}, body: choose(
      when: input_ready(observed: before), then: bind(typed: {set_text}, body: choose(
        when: typed, then: bind(after: {observe}, body: choose(
          when: and(left: input_ready(observed: after), right: eq(left: field(value: after, name: "value"), right: text)),
          then: save(text: text), otherwise: false
        )), otherwise: false
      )), otherwise: false
    )
  )
)
fn main(text: string) = bind(
  ids: {list}, body: choose(
    when: and(left: has_control(ids: ids, wanted: "input"), right: has_control(ids: ids, wanted: "save")),
    then: fill(text: text), otherwise: false
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
    result: ResultValue,
    effects: usize,
    fuel_remaining: u64,
    native_before: Vec<Snapshot>,
    native_after: Vec<Snapshot>,
    verified: Option<bool>,
}
#[derive(Deserialize)]
struct ResultValue {
    #[serde(rename = "type")]
    kind: String,
    value: serde_json::Value,
}
#[derive(Debug, Deserialize, Eq, PartialEq)]
struct Snapshot {
    identifier: String,
    role: String,
    title: String,
    value: String,
    editable: bool,
    enabled: bool,
    clicks: u64,
}

fn snapshots(snapshots: &[Snapshot]) -> Result<[&Snapshot; 3], String> {
    if snapshots.len() != 3
        || snapshots.iter().any(|snapshot| {
            [
                &snapshot.identifier,
                &snapshot.role,
                &snapshot.title,
                &snapshot.value,
            ]
            .iter()
            .any(|text| text.len() > 4096)
                || snapshot.clicks > i64::MAX as u64
        })
    {
        return Err("native report has invalid snapshot bounds".into());
    }
    let mut selected = Vec::new();
    for name in ["input", "save", "other"] {
        let matches: Vec<_> = snapshots
            .iter()
            .filter(|snapshot| snapshot.identifier == name)
            .collect();
        if matches.len() != 1 {
            return Err("native report must identify input, save and other exactly once".into());
        }
        selected.push(matches[0]);
    }
    Ok([selected[0], selected[1], selected[2]])
}

/// Verify both the generated program's Boolean and separate host-native snapshots.
/// Native report reads are host checks, outside the reported VM Effect/Fuel count.
pub fn verify(report_bytes: &[u8], expected_text: &str) -> Result<Verified, String> {
    verify_for("appkit", report_bytes, expected_text)
}

pub fn verify_for(
    profile: &str,
    report_bytes: &[u8],
    expected_text: &str,
) -> Result<Verified, String> {
    if profile == calculator::PROFILE {
        return calculator::verify(report_bytes);
    }
    if expected_text.len() > 4096 {
        return Err("expected text exceeds scalar bounds".into());
    }
    let report: Report = decode(report_bytes, MAX_REPORT_BYTES, "run report")?;
    let (kind, max_fuel, save_title) = match profile {
        "appkit" => ("appkit_run", 1000, "ready"),
        "tauri.typescript" => ("tauri_run", 10_000, "Save"),
        _ => return Err("unsupported form report profile".into()),
    };
    if report.schema_version != 1 || report.kind != kind || report.profile != profile {
        return Err("run report does not match the selected catalog profile".into());
    }
    if profile == "tauri.typescript" && report.verified != Some(true) {
        return Err("TypeScript host did not verify its live control state".into());
    }
    if report.result.kind != "boolean" || report.result.value != serde_json::Value::Bool(true) {
        return Err("generated program did not return the Boolean postcondition true".into());
    }
    if report.effects != 9 || report.fuel_remaining > max_fuel {
        return Err("run report has an unexpected Effect count or Fuel budget".into());
    }
    let [input_before, save_before, other_before] = snapshots(&report.native_before)?;
    let [input_after, save_after, other_after] = snapshots(&report.native_after)?;
    if input_before.role != "text_field"
        || input_after.role != "text_field"
        || !input_before.editable
        || !input_after.editable
        || !input_before.enabled
        || !input_after.enabled
        || input_after.value != expected_text
        || input_before.title != input_after.title
        || input_before.clicks != input_after.clicks
    {
        return Err("native input state does not verify the requested text edit".into());
    }
    if save_before.role != "button"
        || save_after.role != "button"
        || save_before.editable
        || save_after.editable
        || !save_before.enabled
        || !save_after.enabled
        || save_after.value != expected_text
        || save_after.title != save_title
        || save_before.clicks.checked_add(1) != Some(save_after.clicks)
    {
        return Err("native Save state does not verify exactly one requested submission".into());
    }
    if other_before != other_after || other_before.role != "button" {
        return Err("the unrelated native control changed".into());
    }
    Ok(Verified {
        effects: report.effects,
        fuel_remaining: report.fuel_remaining,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    const CATALOG: &[u8] = include_bytes!("fixtures/appkit-v1.json");

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
            NamedParameter, OperationCatalog, OperationCatalogLimits, OperationSchema,
            ScalarTypeSet,
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
                        let domain =
                            parameter
                                .types
                                .iter()
                                .fold(ScalarTypeSet::empty(), |domain, ty| match ty {
                                    TypeDescription::Scalar(ty) => domain.with(*ty),
                                    TypeDescription::Native(_) => domain,
                                });
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
            include_bytes!("fixtures/calculator-v1.json"),
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
            change_catalog(|catalog| catalog["profile"] = json!("another.appkit"))
                .contains("profile")
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
            change_catalog(|catalog| {
                operation(catalog, name)["parameters"][0]["types"] = json!([])
            });
            change_catalog(|catalog| {
                operation(catalog, name)["parameters"][0]["types"] = json!([
                    {"kind":"scalar","name":"string"},{"kind":"scalar","name":"none"}
                ])
            });
        }
        change_catalog(|catalog| {
            catalog["native_types"][1]["fields"][2]["scalar_type"] = json!("optional_string")
        });
        change_catalog(|catalog| {
            catalog["native_types"][1]["fields"][0]["name"] = json!("different")
        });
        change_catalog(|catalog| {
            catalog["native_types"][0]["fields"] =
                json!([{"name":"unsafe","scalar_type":"integer"}])
        });
    }

    #[test]
    fn full_catalog_validation_checks_unused_declarations_and_domains() {
        for change in [
            ("name", json!("bad operation")),
            ("result", json!({"kind":"native","name":"undeclared"})),
        ] {
            let error =
                change_catalog(|catalog| operation(catalog, "gui.list")[change.0] = change.1);
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
}
