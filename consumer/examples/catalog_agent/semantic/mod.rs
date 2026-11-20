//! Finite, host-declared tasks compiled and verified independently of GUI hosts.
mod report;
mod source;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod v2_tests;

use leselang_runtime_core::ScalarType;
pub(super) use report::verify;
pub(super) use source::generate;

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
