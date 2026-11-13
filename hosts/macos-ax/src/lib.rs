//! Cross-process control discovery inside an explicit application/window scope,
//! with TextEdit and Calculator compatibility profiles sharing one session engine.
//!
//! The native transport has observed-state freshness, not an atomic remote
//! compare-and-set contract. The language's local object/session rules remain
//! unchanged. OS accessibility permission is checked without prompting.
use leselang_vm::native::core::host_protocol::ObjectError;
use std::fmt;

#[cfg(target_os = "macos")]
pub mod access;
pub mod calculator;
#[cfg(target_os = "macos")]
pub mod calculator_access;
pub mod controls;
#[cfg(target_os = "macos")]
pub mod controls_access;
pub mod driver;
pub mod host;
pub mod scope;
#[cfg(target_os = "macos")]
pub mod scope_access;
#[cfg(target_os = "macos")]
mod transport;
pub mod widgets;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Snapshot {
    pub identity: u64,
    pub document: String,
    pub role: String,
    pub value: String,
    pub editable: bool,
}

/// A transport for a single scoped editor. Implementations recheck authority,
/// process, document and object identity before touching the native target.
pub trait Backend {
    fn snapshot(&self) -> Result<Snapshot, Error>;
    /// Compare the latest observed state immediately before entering the setter.
    /// Every failure after entering that setter must be `Error::Uncertain`.
    fn set_text(&self, expected: &Snapshot, text: &str) -> Result<(), Error>;
    fn check_permission(&self) -> Result<(), Error>;
}

#[derive(Debug)]
pub enum Error {
    PermissionDenied,
    Unavailable(&'static str),
    Invalid(&'static str),
    Stale,
    NotEditable,
    Bounds,
    WrongThread,
    Fuel,
    Api(i32),
    Uncertain(i32),
    Object(ObjectError),
}
impl From<ObjectError> for Error {
    fn from(error: ObjectError) -> Self {
        Self::Object(error)
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PermissionDenied => {
                f.write_str("Accessibility permission is not available for this host process")
            }
            Self::Unavailable(message) | Self::Invalid(message) => f.write_str(message),
            Self::Stale => {
                f.write_str("the scoped process, document, object or observed state changed")
            }
            Self::NotEditable => f.write_str("the scoped text area is not editable"),
            Self::Bounds => {
                f.write_str("the target exceeds the adapter's traversal or text bounds")
            }
            Self::WrongThread => {
                f.write_str("the Accessibility adapter must stay on the main thread")
            }
            Self::Fuel => f.write_str("the original session Fuel is exhausted"),
            Self::Api(code) => write!(f, "Accessibility read failed ({code})"),
            Self::Uncertain(code) => write!(
                f,
                "Accessibility mutation outcome is uncertain ({code}); do not automatically retry"
            ),
            Self::Object(error) => write!(f, "native object validation failed: {error:?}"),
        }
    }
}
impl std::error::Error for Error {}

#[cfg(target_os = "macos")]
pub mod widgets_access;
