//! Compatibility strategies used only when semantic metadata is absent.
use super::catalog::Catalog;
use super::json::decode;
use super::{MAX_REPORT_BYTES, TaskContract, Verified, calculator};
use leselang_runtime_core::{
    ScalarType,
    discovery::{CatalogDescription, MAX_DESCRIPTION_BYTES, TypeDescription},
};
use serde::Deserialize;

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

pub(super) fn native(name: &str) -> TypeDescription {
    TypeDescription::Native(name.into())
}
pub(super) fn typed(ty: ScalarType) -> TypeDescription {
    TypeDescription::Scalar(ty)
}

pub(super) fn require_operation(
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

pub(super) fn call(catalog: &CatalogDescription, name: &str, values: &[(&str, &str)]) -> String {
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
pub(super) struct ResultValue {
    #[serde(rename = "type")]
    pub(super) kind: String,
    pub(super) value: serde_json::Value,
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
