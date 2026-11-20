//! Verify exact or observed results and every declared report predicate.
use super::scalar_name;
use crate::plan::json::decode;
use crate::plan::{MAX_REPORT_BYTES, Verified};
use leselang_runtime_core::{ScalarType, ScalarValue, discovery::semantics::*};
use serde_json::Value;

fn scalar_json(value: &ScalarValue) -> Value {
    match value {
        ScalarValue::Integer(value) => Value::from(*value),
        ScalarValue::Boolean(value) => Value::Bool(*value),
        ScalarValue::String(value) => Value::String(value.clone()),
        ScalarValue::None => Value::Null,
        _ => unreachable!("validated report scalar"),
    }
}

fn bounded(value: &Value) -> bool {
    match value {
        Value::String(value) => value.len() <= 4096,
        Value::Array(values) => values.iter().all(bounded),
        Value::Object(values) => values
            .iter()
            .all(|(name, value)| name.len() <= 4096 && bounded(value)),
        _ => true,
    }
}

fn path<'a>(report: &'a Value, path: &[ReportSegment]) -> Result<&'a Value, String> {
    let mut value = report;
    for segment in path {
        value = match segment {
            ReportSegment::Field { name } => value
                .as_object()
                .and_then(|object| object.get(name))
                .ok_or("semantic report path has a missing field or wrong container type")?,
            ReportSegment::Match {
                field,
                value: wanted,
            } => {
                let values = value
                    .as_array()
                    .ok_or("semantic report selection requires an array")?;
                let mut matched = values
                    .iter()
                    .filter(|value| value.get(field).and_then(Value::as_str) == Some(wanted));
                let selected = matched
                    .next()
                    .ok_or("semantic report selection is missing")?;
                if matched.next().is_some() {
                    return Err("semantic report selection is ambiguous".into());
                }
                selected
            }
        };
    }
    Ok(value)
}

fn report_term(
    report: &Value,
    term: &ReportTerm,
    input: Option<&str>,
    text: &str,
) -> Result<Value, String> {
    match term {
        ReportTerm::Literal { value } => Ok(scalar_json(value)),
        ReportTerm::Input { name } if Some(name.as_str()) == input => {
            Ok(Value::String(text.into()))
        }
        ReportTerm::Input { .. } => Err("semantic report input is not declared".into()),
        ReportTerm::Path { path: segments } => Ok(path(report, segments)?.clone()),
        ReportTerm::Scalar {
            path: segments,
            scalar_type,
        } => {
            let value = path(report, segments)?;
            let valid = match scalar_type {
                ScalarType::Integer => value.as_u64().is_some(),
                ScalarType::Boolean => value.is_boolean(),
                ScalarType::String => value.is_string(),
                ScalarType::None => value.is_null(),
                ScalarType::OptionalString => value.is_null() || value.is_string(),
                ScalarType::StringList => value.as_array().is_some_and(|values| {
                    values.len() <= 64
                        && values
                            .iter()
                            .try_fold(4096usize, |remaining, item| {
                                remaining.checked_sub(item.as_str()?.len())
                            })
                            .is_some()
                }),
            };
            if !valid {
                return Err("semantic report scalar has the wrong type".into());
            }
            Ok(value.clone())
        }
        ReportTerm::Length { path: segments } => Ok(Value::from(
            path(report, segments)?
                .as_array()
                .ok_or("semantic report length requires an array")?
                .len(),
        )),
    }
}

pub(in crate::plan) fn verify(
    profile: &str,
    effects: usize,
    contract: &v2::ReportContract,
    input: Option<&str>,
    report_bytes: &[u8],
    text: &str,
) -> Result<Verified, String> {
    if text.len() > 4096 {
        return Err("expected input exceeds scalar bounds".into());
    }
    let report: Value = decode(report_bytes, MAX_REPORT_BYTES, "semantic run report")?;
    if !bounded(&report) {
        return Err("semantic report exceeds scalar text bounds".into());
    }
    let number = |name: &str| report.get(name).and_then(Value::as_u64);
    if number("schema_version") != Some(1)
        || report.get("profile").and_then(Value::as_str) != Some(profile)
        || report.get("kind").and_then(Value::as_str) != Some(contract.kind.as_str())
        || number("effects") != Some(effects as u64)
        || number("fuel_remaining").is_none_or(|fuel| fuel > contract.max_fuel)
        || (contract.require_verified && report.get("verified") != Some(&Value::Bool(true)))
    {
        return Err("semantic report header does not match its catalog task".into());
    }
    let result = report
        .get("result")
        .ok_or("semantic report is missing its result")?;
    let expected = match &contract.result {
        v2::ResultContract::Exact(value) => scalar_json(value),
        v2::ResultContract::Observed { scalar_type, path } => report_term(
            &report,
            &ReportTerm::Scalar {
                scalar_type: *scalar_type,
                path: path.clone(),
            },
            input,
            text,
        )?,
    };
    if result.get("type").and_then(Value::as_str)
        != Some(scalar_name(contract.result.scalar_type()))
        || result.get("value") != Some(&expected)
    {
        return Err("semantic report result does not establish the declared postcondition".into());
    }
    for predicate in &contract.predicates {
        let left = report_term(&report, &predicate.left, input, text)?;
        let right = report_term(&report, &predicate.right, input, text)?;
        let satisfied = match predicate.relation {
            ReportRelation::Equal => left == right,
            ReportRelation::AtMost => left
                .as_u64()
                .zip(right.as_u64())
                .is_some_and(|(left, right)| left <= right),
            ReportRelation::IncreasedBy { amount } => left
                .as_u64()
                .and_then(|left| left.checked_add(amount))
                .zip(right.as_u64())
                .is_some_and(|(left, right)| left == right),
        };
        if !satisfied {
            return Err(format!("semantic report predicate {} failed", predicate.id));
        }
    }
    Ok(Verified {
        effects,
        fuel_remaining: number("fuel_remaining").expect("validated report fuel"),
    })
}
