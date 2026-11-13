//! AppKit integration belongs to this independent host, not the language crates.
//! The native view bridge and local VM are both owned by the macOS main thread.

mod catalog;
#[cfg(target_os = "macos")]
pub mod gui;
#[cfg(target_os = "macos")]
pub mod host;
#[path = "../../reference_semantics.rs"]
mod reference_semantics;

/// Inspect the real host catalog without creating a GUI, querying permissions,
/// compiling source or starting a session. Available on every target platform.
pub fn describe() -> Result<leselang_vm::native::core::discovery::CatalogDescription, Error> {
    catalog::describe()
}

/// Offline JSON discovery with the reference application's optional task contract.
/// The original structural `describe()` API and its `to_json()` output are unchanged.
pub fn describe_json() -> Result<String, String> {
    let description = describe().map_err(|error| error.to_string())?;
    let semantics = reference_semantics::describe(&description.profile)
        .expect("the reference AppKit profile has a task contract");
    description
        .to_json_with_semantics(&semantics)
        .map_err(|error| error.to_string())
}

use leselang_vm::native::core::host_protocol::ObjectError;

#[derive(Debug)]
pub enum Error {
    Gui(&'static str),
    Native(&'static str),
    Object(ObjectError),
    Fuel,
    Uncertain(&'static str),
}
impl From<ObjectError> for Error {
    fn from(error: ObjectError) -> Self {
        Self::Object(error)
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Gui(message) | Self::Native(message) | Self::Uncertain(message) => {
                formatter.write_str(message)
            }
            Self::Object(error) => error.fmt(formatter),
            Self::Fuel => formatter.write_str("host fuel exhausted"),
        }
    }
}
impl std::error::Error for Error {}
