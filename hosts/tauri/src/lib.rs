//! Rust VM ownership and a separately versioned bridge to registered JS objects.
mod agent_semantics;
mod application;
mod article_semantics;
mod board_semantics;
mod catalog;
pub mod gateway;
pub mod host;
#[path = "../../shared/semantics.rs"]
mod reference_semantics;
mod task;

pub use application::AgentApplication;
pub use task::{
    PreparedTask, prepare_named_task, prepare_task, run_prepared_registered_task,
    run_prepared_registered_task_detailed, run_prepared_task, run_prepared_task_for_application,
    run_prepared_task_for_application_detailed, run_task,
};

pub use leselang_registered_host::{
    Error, ExecutionFailure, ExecutionFailureCode, Scope, Transport, describe_registered,
    run_registered,
};

use serde_json::{Value, json};

pub fn describe() -> Result<String, String> {
    let description = catalog::describe().map_err(|e| e.to_string())?;
    let semantics = reference_semantics::describe(&description.profile)
        .expect("the reference TypeScript form has a task contract");
    description
        .to_json_with_semantics(&semantics)
        .map_err(|e| e.to_string())
}

/// Offline task contracts for the generic in-app execution entry. The original
/// `describe` and one-shot form runner retain their existing report contract.
pub fn describe_agent() -> Result<String, String> {
    catalog::describe()
        .map_err(|e| e.to_string())?
        .to_json_with_semantics(&agent_semantics::describe())
        .map_err(|e| e.to_string())
}

/// Opt-in v2 task catalog with evidence-bound dynamic observations. No runtime
/// authority or native object is created by reading this description.
pub fn describe_agent_observed() -> Result<String, String> {
    catalog::describe()
        .map_err(|e| e.to_string())?
        .to_json_with_semantics_v2(&agent_semantics::describe_observed())
        .map_err(|e| e.to_string())
}

/// The task board has its own host-owned v2 catalog and report profile. Reading
/// it never launches a page, creates objects or grants execution authority.
pub fn describe_agent_board() -> Result<String, String> {
    catalog::describe_board()
        .map_err(|e| e.to_string())?
        .to_json_with_semantics_v2(&board_semantics::describe())
        .map_err(|e| e.to_string())
}

/// Host-owned v3 article tasks bind title and content independently. Discovery
/// uses the same registered operations and creates no application authority.
pub fn describe_agent_article() -> Result<String, String> {
    catalog::describe_article()
        .map_err(|e| e.to_string())?
        .to_json_with_semantics_v3(&article_semantics::describe())
        .map_err(|e| e.to_string())
}

/// This worker owns the LocalVm. The caller's event loop transports data only.
pub fn run(
    transport: &mut dyn Transport,
    scope: Scope,
    source: &str,
    text: &str,
) -> Result<Value, String> {
    if text.len() > 4096 {
        return Err("task text exceeds 4096 UTF-8 bytes".into());
    }
    let gui = gateway::RemoteGui::new(transport, scope).map_err(|e| e.to_string())?;
    let before = gui.public_snapshots();
    let (value, effects, fuel_remaining) =
        leselang_registered_host::execute_boolean(&gui, source, text)?;
    let after = gui.verify_snapshots().map_err(|e| e.to_string())?;
    let verified = value
        && effects == 9
        && after[0]["value"] == text
        && after[2]["value"] == text
        && after[0]["clicks"].as_u64()
            == before[0]["clicks"].as_u64().and_then(|n| n.checked_add(1))
        && after[1] == before[1];
    let value = json!({"type":"boolean","value":value});
    Ok(
        json!({"schema_version":1,"kind":"tauri_run","profile":"tauri.typescript","result":value,
        "effects":effects,"fuel_remaining":fuel_remaining,"verified":verified,"native_before":before,"native_after":after}),
    )
}
