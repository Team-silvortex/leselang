//! AppKit integration belongs to this independent host, not the language crates.
//! The native view bridge and local VM are both owned by the macOS main thread.

#[cfg(target_os = "macos")]
pub mod gui;
#[cfg(target_os = "macos")]
pub mod host;

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
