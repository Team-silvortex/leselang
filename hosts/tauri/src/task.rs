//! Bounded task admission and typed reports for the application-owned agent entry.
use crate::{Error, Scope, Transport, drive, gateway, host};
use leselang_vm::{
    embedding::bind_entry_arguments,
    native::{
        core::ScalarValue,
        hir::{catalog_program::CompiledCatalogProgram, pure_typing::PureType},
    },
};
use serde_json::{Value, json};

/// Owned compilation and already-bound data, without a VM, lease or object
/// authority. It may move to the worker that will own the execution session.
pub struct PreparedTask {
    program: CompiledCatalogProgram<host::Node, host::NativeType>,
    arguments: Vec<(String, ScalarValue)>,
}

fn input_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name.bytes().enumerate().all(|(index, byte)| {
            byte == b'_' || byte.is_ascii_alphabetic() || (index > 0 && byte.is_ascii_digit())
        })
        && !matches!(name, "fn" | "true" | "false" | "none" | "body")
        && !name.starts_with("_lese_")
}

/// Admit the semantics-v1 carrier shape: no input or one named string. Complete
/// compilation, input names/types/bounds and scalar results are checked before
/// a transport, JavaScript registration, or execution session is contacted.
pub fn prepare_task(
    source: &str,
    arguments: Vec<(String, String)>,
) -> Result<PreparedTask, String> {
    if source.len() > 16 * 1024 || source.contains('\0') {
        return Err("task source exceeds 16 KiB or contains NUL".into());
    }
    if arguments.len() > 1
        || arguments
            .iter()
            .any(|(name, value)| !input_name(name) || value.len() > 4096 || value.contains('\0'))
    {
        return Err("task arguments require zero or one bounded named string".into());
    }
    // The existing compiler uses only catalog/type metadata. An empty backend
    // supplies no native objects and refuses every operation if this changes.
    let program = host::with_host(&DeclarationsOnly, |host, _| {
        host.compile_task(source, "main")
    })
    .map_err(|error| error.to_string())??;
    if !matches!(program.result_type, PureType::Scalar(_)) {
        return Err("task result must be scalar; native identities cannot leave the worker".into());
    }
    if program.entry_parameters.len() > 1
        || program.entry_parameters.iter().any(|parameter| {
            !input_name(&parameter.name)
                || parameter.domain != leselang_vm::native::core::ScalarType::String
        })
    {
        return Err("task entry requires zero or one named string parameter".into());
    }
    let arguments = bind_entry_arguments(
        &program.entry_parameters,
        arguments
            .into_iter()
            .map(|(name, value)| (name, ScalarValue::String(value)))
            .collect(),
        host::STACK.effects.pure.max_bindings,
    )
    .map_err(|error| error.to_string())?;
    Ok(PreparedTask { program, arguments })
}

/// Run one admitted task through the original explicitly driven VM. The report
/// records typed results and observations; it makes no business-verification
/// claim. The application checks its own catalog predicates and fresh state.
pub fn run_prepared_task(
    transport: &mut dyn Transport,
    scope: Scope,
    task: PreparedTask,
) -> Result<Value, String> {
    let gui =
        gateway::RemoteGui::registered(transport, scope).map_err(|error| error.to_string())?;
    let before = gui.public_snapshots();
    let (value, effects, fuel_remaining) = host::with_host(&gui, |host, scope| {
        drive(host, scope, &task.program, task.arguments)
    })
    .map_err(|error| error.to_string())??;
    let result = typed_result(value)?;
    let after = gui.verify_snapshots().map_err(|error| error.to_string())?;
    // Pure tasks and empty registries may perform no final transport read.
    // Cancellation still fences the report even in that zero-Effect case.
    if !gateway::Backend::is_open(&gui).map_err(|error| error.to_string())? {
        return Err("task authority closed before completion".into());
    }
    Ok(json!({
        "schema_version":1, "kind":"tauri_task_run", "profile":"tauri.typescript",
        "result":result, "effects":effects, "fuel_remaining":fuel_remaining,
        "native_before":before, "native_after":after,
    }))
}

/// Convenience entry preserving admission before any transport work.
pub fn run_task(
    transport: &mut dyn Transport,
    scope: Scope,
    source: &str,
    arguments: Vec<(String, String)>,
) -> Result<Value, String> {
    let task = prepare_task(source, arguments)?;
    run_prepared_task(transport, scope, task)
}

fn typed_result(value: ScalarValue) -> Result<Value, String> {
    if !value.is_bounded() {
        return Err("task result exceeds scalar bounds".into());
    }
    let kind = value.scalar_type();
    let value = match value {
        ScalarValue::Integer(value) => json!(value),
        ScalarValue::Boolean(value) => json!(value),
        ScalarValue::String(value) => json!(value),
        ScalarValue::None => Value::Null,
        ScalarValue::OptionalString(value) => json!(value),
        ScalarValue::StringList(value) => json!(value),
    };
    Ok(json!({"type":kind,"value":value}))
}

struct DeclarationsOnly;
impl gateway::Backend for DeclarationsOnly {
    fn object_count(&self) -> usize {
        0
    }
    fn is_open(&self) -> Result<bool, Error> {
        Err(Error::Native("declarations have no execution backend"))
    }
    fn list(&self) -> Result<Vec<String>, Error> {
        Err(Error::Native("declarations have no execution backend"))
    }
    fn find(&self, _: &str) -> Result<usize, Error> {
        Err(Error::Native("declarations have no execution backend"))
    }
    fn snapshot(&self, _: usize) -> Result<gateway::WidgetSnapshot, Error> {
        Err(Error::Native("declarations have no execution backend"))
    }
    fn observe(&self, _: usize) -> Result<(), Error> {
        Err(Error::Native("declarations have no execution backend"))
    }
    fn click(&self, _: usize, _: usize) -> Result<(), Error> {
        Err(Error::Native("declarations have no execution backend"))
    }
    fn set_text(&self, _: usize, _: usize, _: &str) -> Result<(), Error> {
        Err(Error::Native("declarations have no execution backend"))
    }
}
