//! Portable Rust ownership of registered-object execution, independent of GUI frameworks.
mod catalog;
pub mod gateway;
pub mod host;
mod task;
mod worker;

pub use task::{PreparedTask, prepare_task, run_prepared_registered_task};
pub use worker::main as worker_main;

use leselang_vm::native::{
    LocalVm,
    core::*,
    hir::{
        catalog_program::CompiledCatalogProgram, effect_session::EffectSessionPoll,
        pure_evaluation::PureValue,
    },
};
use serde_json::{Value, json};

#[derive(Clone, Debug)]
pub struct Scope {
    pub window: String,
    pub epoch: String,
    pub session: String,
}
pub trait Transport {
    fn exchange(&mut self, request: Value) -> Result<Value, String>;
    fn is_live(&self) -> bool;
}
#[derive(Debug)]
pub enum Error {
    Native(&'static str),
    Fuel,
    Object(host_protocol::ObjectError),
    Transport(String),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Native(s) => f.write_str(s),
            Self::Fuel => f.write_str("host fuel exhausted"),
            Self::Object(e) => e.fmt(f),
            Self::Transport(e) => f.write_str(e),
        }
    }
}
impl std::error::Error for Error {}
impl From<host_protocol::ObjectError> for Error {
    fn from(e: host_protocol::ObjectError) -> Self {
        Self::Object(e)
    }
}

/// Project the shared operation catalog under a trusted host-selected profile.
/// This metadata creates no backend, object, lease or execution authority.
pub fn describe_catalog(
    profile: &str,
) -> Result<leselang_vm::native::core::discovery::CatalogDescription, Error> {
    catalog::describe_profile(profile)
}

/// Describe the generic registered-object profile without starting a session.
pub fn describe_registered() -> Result<String, String> {
    describe_catalog("javascript.registered")
        .map_err(|error| error.to_string())?
        .to_json()
        .map_err(|error| error.to_string())
}

/// Run the existing gui.* protocol against a bounded application-owned registry.
/// The caller verifies its own business postcondition from the Boolean result
/// and public observations; this function does not imply form semantics.
/// Bootstrap and final observations are extra host reads, outside VM Effect and
/// Fuel accounting. No VM ownership or authority is serialized to JavaScript.
pub fn run_registered(
    transport: &mut dyn Transport,
    scope: Scope,
    source: &str,
    text: &str,
) -> Result<Value, String> {
    if text.len() > 4096 {
        return Err("task text exceeds 4096 UTF-8 bytes".into());
    }
    let gui = gateway::RemoteGui::registered(transport, scope).map_err(|e| e.to_string())?;
    let before = gui.public_snapshots();
    let (value, effects, fuel_remaining) = execute_boolean(&gui, source, text)?;
    let after = gui.verify_snapshots().map_err(|e| e.to_string())?;
    Ok(json!({
        "schema_version":1, "kind":"javascript_run", "profile":"javascript.registered",
        "result":{"type":"boolean","value":value}, "effects":effects,
        "fuel_remaining":fuel_remaining, "objects_before":before, "objects_after":after,
    }))
}

/// Compile and explicitly execute the legacy named-text Boolean entry against
/// a trusted backend. Returns the scalar result, dispatched Effects and Fuel.
/// Registered inventories and report verification remain the caller's concern.
pub fn execute_boolean(
    gui: &dyn gateway::Backend,
    source: &str,
    text: &str,
) -> Result<(bool, u64, u64), String> {
    if text.len() > 4096 {
        return Err("task text exceeds 4096 UTF-8 bytes".into());
    }
    let (value, effects, fuel_remaining) =
        host::with_host(gui, |host, scope| -> Result<_, String> {
            let program = host.compile_task(source, "main")?;
            let arguments = leselang_vm::embedding::bind_entry_arguments(
                &program.entry_parameters,
                vec![("text".into(), ScalarValue::String(text.into()))],
                host::STACK.effects.pure.max_bindings,
            )
            .map_err(|e| e.to_string())?;
            drive(host, scope, &program, arguments)
        })
        .map_err(|e| e.to_string())??;
    match value {
        ScalarValue::Boolean(value) => Ok((value, effects, fuel_remaining)),
        _ => Err("registered task must return boolean".into()),
    }
}

/// Shared by legacy Boolean runners and admitted scalar tasks. Compilation and
/// parameter binding never add a runtime wrapper or refill the existing Fuel.
fn drive(
    host: &host::Host<'_, '_>,
    scope: LocalExecutionScope,
    program: &CompiledCatalogProgram<host::Node, host::NativeType>,
    arguments: Vec<(String, ScalarValue)>,
) -> Result<(ScalarValue, u64, u64), String> {
    let bindings = program
        .entry_parameters
        .iter()
        .zip(arguments)
        .map(|(parameter, (_, value))| (parameter.name.as_str(), PureValue::Scalar(value)))
        .collect();
    let mut vm = LocalVm::new(
        scope,
        &program.node,
        bindings,
        Fuel::new(10_000),
        host::STACK,
    );
    let mut effects = 0_u64;
    loop {
        match vm
            .poll(host)
            .map_err(|e| format!("language execution failed: {e:?}"))?
        {
            EffectSessionPoll::Request(work) => {
                effects += 1;
                eprintln!("poll -> authorize -> dispatch: {}", work.operation());
                host.authorize(work.identity()).map_err(|e| e.to_string())?;
                let receipt = vm
                    .with_metered_host_outcome(work, |work, fuel| host.dispatch(work, fuel))
                    .map_err(|_| "dispatch boundary")?
                    .map_err(|e| e.to_string())?;
                let receipt = if matches!(receipt.reply, host::Reply::Submitted(true)) {
                    eprintln!("explicit pump -> TypeScript action -> Promise reply");
                    vm.with_metered_host_outcome(receipt, |receipt, fuel| host.pump(receipt, fuel))
                        .map_err(|_| "pump boundary")?
                        .map_err(|e| e.to_string())?
                } else {
                    receipt
                };
                vm.try_accept(&receipt.identity, receipt.reply, host)
                    .map_err(|e| e.error.to_string())?;
                eprintln!("accept");
            }
            EffectSessionPoll::Value(PureValue::Scalar(value)) => {
                break Ok((value, effects, vm.fuel_remaining()));
            }
            _ => break Err("unexpected non-scalar session result".into()),
        }
    }
}
