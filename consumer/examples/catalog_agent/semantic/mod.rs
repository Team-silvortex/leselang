//! Finite, host-declared tasks compiled and verified independently of GUI hosts.
mod report;
mod source;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod v2_tests;
#[cfg(test)]
mod v3_tests;

use leselang_runtime_core::ScalarType;
pub(super) use report::{verify, verify_named};
pub(super) use source::{TaskSource, generate};

fn scalar_name(ty: ScalarType) -> &'static str {
    match ty {
        ScalarType::Integer => "integer",
        ScalarType::Boolean => "boolean",
        ScalarType::String => "string",
        ScalarType::None => "none",
        ScalarType::OptionalString => "optional_string",
        ScalarType::StringList => "string_list",
    }
}
