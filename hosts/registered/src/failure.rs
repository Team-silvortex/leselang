//! Optional execution diagnostics, separate from successful reports and wire authority.
use crate::Error;

/// Closed, host-selected failure categories. These never grant execution authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionFailureCode {
    Failed,
    ObjectUnavailable,
    FuelExhausted,
    DeadlineExceeded,
    ScopeClosed,
    Cancelled,
}

impl ExecutionFailureCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Failed => "execution_failed",
            Self::ObjectUnavailable => "execution_object_unavailable",
            Self::FuelExhausted => "execution_fuel_exhausted",
            Self::DeadlineExceeded => "execution_deadline_exceeded",
            Self::ScopeClosed => "execution_scope_closed",
            Self::Cancelled => "execution_cancelled",
        }
    }
}

/// A typed execution failure retaining the original legacy error text.
#[derive(Debug)]
pub struct ExecutionFailure {
    code: ExecutionFailureCode,
    message: String,
}

impl ExecutionFailure {
    /// For trusted host lifecycle branches with an independently known cause.
    pub fn new(code: ExecutionFailureCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn code(&self) -> ExecutionFailureCode {
        self.code
    }
}

impl std::fmt::Display for ExecutionFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ExecutionFailure {}

impl From<Error> for ExecutionFailure {
    fn from(error: Error) -> Self {
        // Classify while the native variant is still available. Transport text
        // may resemble a native message and must never select its category.
        let code = match &error {
            Error::Native("unknown registered object") => ExecutionFailureCode::ObjectUnavailable,
            Error::Fuel => ExecutionFailureCode::FuelExhausted,
            _ => ExecutionFailureCode::Failed,
        };
        Self::new(code, error.to_string())
    }
}

impl From<String> for ExecutionFailure {
    fn from(message: String) -> Self {
        Self::new(ExecutionFailureCode::Failed, message)
    }
}

impl From<&str> for ExecutionFailure {
    fn from(message: &str) -> Self {
        Self::new(ExecutionFailureCode::Failed, message)
    }
}
