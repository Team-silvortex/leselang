//! Bounded scalar task ingress shared by scalar and native GUI hosts.

use std::fmt;

use crate::native::{
    core::{NamedArgumentError, NamedParameter, ScalarContractError, bind_named_arguments},
    hir::pure_typing::{MAX_TYPE_INFERENCE_BINDINGS, valid_local_name},
    syntax::MAX_FUNCTION_PARAMETERS,
};

use super::{ScalarType, ScalarTypeSet, ScalarValue};

/// Required scalar inputs in declaration order, owned by the compiled program.
pub type EntryParameter = NamedParameter<String, ScalarType>;

/// Input refusal before a session or Effect exists. Positions refer to the
/// supplied arguments or declaration; no submitted names or values are exposed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryArgumentError {
    InvalidSignature,
    BindingLimit,
    Names(NamedArgumentError),
    Value {
        parameter_index: usize,
        error: ScalarContractError,
    },
}

impl fmt::Display for EntryArgumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSignature => {
                f.write_str("entry signature is not bounded required scalar parameters")
            }
            Self::BindingLimit => f.write_str("entry inputs exceed the execution binding limit"),
            Self::Names(error) => error.fmt(f),
            Self::Value {
                parameter_index,
                error,
            } => {
                write!(f, "entry parameter {parameter_index}: {error}")
            }
        }
    }
}

impl std::error::Error for EntryArgumentError {}

/// Validate and move named scalar inputs into declaration order.
///
/// This performs no evaluation, copying of scalar payloads, Fuel charge, scope
/// creation or authorization. All values and names are checked before the
/// result is published. Errors consume the supplied inputs without executing
/// them. Callers bound allocation/decoding before constructing the input vector.
/// The resulting bindings are ordinary data, not a program or permission proof.
pub fn bind_entry_arguments(
    parameters: &[EntryParameter],
    arguments: Vec<(String, ScalarValue)>,
    max_bindings: usize,
) -> Result<Vec<(String, ScalarValue)>, EntryArgumentError> {
    if parameters.len() > MAX_FUNCTION_PARAMETERS
        || parameters
            .iter()
            .any(|parameter| !parameter.required || !valid_local_name(&parameter.name))
    {
        return Err(EntryArgumentError::InvalidSignature);
    }
    if max_bindings > MAX_TYPE_INFERENCE_BINDINGS || parameters.len() > max_bindings {
        return Err(EntryArgumentError::BindingLimit);
    }
    // Reject an unbounded caller vector before allocating name/index views.
    if arguments.len() > parameters.len() {
        return Err(EntryArgumentError::Names(
            NamedArgumentError::TooManyArguments,
        ));
    }
    let names: Vec<_> = arguments.iter().map(|(name, _)| name.as_str()).collect();
    let signature: Vec<_> = parameters
        .iter()
        .map(|parameter| NamedParameter::required(parameter.name.as_str(), parameter.domain))
        .collect();
    let aligned = bind_named_arguments(&names, &signature).map_err(EntryArgumentError::Names)?;
    let mut order = Vec::with_capacity(parameters.len());
    for (parameter_index, (parameter, argument_index)) in aligned.iter().enumerate() {
        ScalarTypeSet::only(parameter.domain)
            .validate(&arguments[argument_index].1)
            .map_err(|error| EntryArgumentError::Value {
                parameter_index,
                error,
            })?;
        order.push(argument_index);
    }
    let mut arguments: Vec<_> = arguments.into_iter().map(Some).collect();
    Ok(order
        .into_iter()
        .map(|index| {
            arguments[index]
                .take()
                .expect("named alignment uses each admitted input once")
        })
        .collect())
}
