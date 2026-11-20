//! Tauri report identities around the shared registered task execution.
use crate::{AgentApplication, Scope, Transport};
use serde_json::{Value, json};

pub use leselang_registered_host::{
    PreparedTask, prepare_named_task, prepare_task, run_prepared_registered_task,
};

/// Run one admitted task through the original explicitly driven VM. The report
/// records typed results and observations; it makes no business-verification
/// claim. The application checks its own catalog predicates and fresh state.
pub fn run_prepared_task(
    transport: &mut dyn Transport,
    scope: Scope,
    task: PreparedTask,
) -> Result<Value, String> {
    run_prepared_task_for_application(transport, scope, task, AgentApplication::Form)
}

/// Run through the same VM while binding report identity to the host's selected
/// application. This enum comes from native startup, never page/model input.
/// It does not add authority; the transport still owns its live registrations.
pub fn run_prepared_task_for_application(
    transport: &mut dyn Transport,
    scope: Scope,
    task: PreparedTask,
    application: AgentApplication,
) -> Result<Value, String> {
    let mut report = run_prepared_registered_task(transport, scope, task)?;
    let fields = report
        .as_object_mut()
        .ok_or("invalid registered task report")?;
    for (from, to) in [
        ("objects_before", "native_before"),
        ("objects_after", "native_after"),
    ] {
        let value = fields
            .remove(from)
            .ok_or("missing registered task observations")?;
        fields.insert(to.into(), value);
    }
    fields.insert("profile".into(), json!(application.profile()));
    fields.insert("kind".into(), json!("tauri_task_run"));
    Ok(report)
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
