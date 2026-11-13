//! Finite, host-declared tasks compiled into ordinary LeseLang source.
//! Metadata selects neither a process nor permissions and cannot supply source.
use super::{CatalogDescription, MAX_REPORT_BYTES, Verified, decode};
use leselang_runtime_core::{
    ScalarType, ScalarValue,
    discovery::{TypeDescription, semantics::*},
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

fn quoted(value: &str) -> String {
    let mut output = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character => output.push(character),
        }
    }
    output.push('"');
    output
}

fn literal(value: &ScalarValue) -> String {
    match value {
        ScalarValue::Integer(value) => value.to_string(),
        ScalarValue::Boolean(value) => value.to_string(),
        ScalarValue::String(value) => quoted(value),
        ScalarValue::None => "none".into(),
        _ => unreachable!("semantic literal types were validated"),
    }
}

fn term(value: &Term) -> String {
    match value {
        Term::Literal { value } => literal(value),
        Term::Input { name } | Term::Binding { name } => name.clone(),
        Term::Field { binding, name } => {
            format!("field(value: {binding}, name: {})", quoted(name))
        }
    }
}

fn condition(value: &Condition) -> String {
    match value {
        Condition::Equal { left, right } => {
            format!("eq(left: {}, right: {})", term(left), term(right))
        }
        Condition::Contains { list, value } => {
            format!(
                "_lese_contains(items: {}, wanted: {})",
                term(list),
                term(value)
            )
        }
        Condition::IncreasedBy {
            before,
            after,
            amount,
        } => {
            format!(
                "eq(left: {}, right: add(left: {}, right: {amount}))",
                term(after),
                term(before)
            )
        }
        Condition::OptionalEquals { optional, value } => {
            let optional = term(optional);
            format!(
                "and(left: has_value(value: {optional}), right: eq(left: value_or(left: {optional}, right: \"\"), right: {}))",
                term(value)
            )
        }
    }
}

fn all(values: &[String]) -> String {
    if values.len() == 1 {
        values[0].clone()
    } else {
        let middle = values.len() / 2;
        format!(
            "and(left: {}, right: {})",
            all(&values[..middle]),
            all(&values[middle..])
        )
    }
}

fn uses_term(value: &Term, names: &mut BTreeSet<String>) {
    match value {
        Term::Input { name } | Term::Binding { name } => {
            names.insert(name.clone());
        }
        Term::Field { binding, .. } => {
            names.insert(binding.clone());
        }
        Term::Literal { .. } => {}
    }
}

fn uses_condition(condition: &Condition, names: &mut BTreeSet<String>) {
    let (left, right) = match condition {
        Condition::Equal { left, right } => (left, right),
        Condition::Contains { list, value } => (list, value),
        Condition::IncreasedBy { before, after, .. } => (before, after),
        Condition::OptionalEquals { optional, value } => (optional, value),
    };
    uses_term(left, names);
    uses_term(right, names);
}

fn live_names(task: &TaskDescription, start: usize) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for step in &task.steps[start..] {
        match step {
            TaskStep::Call { arguments, .. } => {
                for argument in arguments {
                    uses_term(&argument.value, &mut names);
                }
            }
            TaskStep::Guard { conditions, .. } => {
                for condition in conditions {
                    uses_condition(condition, &mut names);
                }
            }
        }
    }
    uses_term(&task.result, &mut names);
    names
}

fn type_name(ty: &TypeDescription) -> &str {
    match ty {
        TypeDescription::Native(name) => name,
        TypeDescription::Scalar(ty) => scalar_name(*ty),
    }
}

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

/// The caller validates the complete metadata first. Bounded stage functions and
/// live parameter selection preserve the existing parser and host limits.
pub(super) fn generate(
    catalog: &CatalogDescription,
    task: &TaskDescription,
) -> Result<String, String> {
    if task.steps.iter().any(|step| {
        matches!(step, TaskStep::Call { operation, .. }
        if operation == "main" || operation.starts_with("_lese_"))
    }) {
        return Err("semantic operation collides with a generated function name".into());
    }
    let chunks = task.steps.chunks(9).collect::<Vec<_>>();
    let mut known = task
        .inputs
        .iter()
        .map(|input| {
            (
                input.name.clone(),
                TypeDescription::Scalar(input.scalar_type),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut parameters = Vec::new();
    for (index, steps) in chunks.iter().enumerate() {
        let live = live_names(task, index * 9);
        let selected = known
            .iter()
            .filter(|(name, _)| index == 0 || live.contains(*name))
            .map(|(name, ty)| (name.clone(), ty.clone()))
            .collect::<Vec<_>>();
        if selected.len() > 8 {
            return Err("semantic task needs more than eight live helper parameters".into());
        }
        parameters.push(selected);
        for step in *steps {
            if let TaskStep::Call {
                binding, operation, ..
            } = step
            {
                let operation = catalog
                    .operations
                    .iter()
                    .find(|candidate| candidate.name == *operation)
                    .expect("validated task operation");
                known.insert(binding.clone(), operation.result.clone());
            }
        }
    }
    let contains = task.steps.iter().any(|step| {
        matches!(step, TaskStep::Guard { conditions, .. }
        if conditions.iter().any(|condition| matches!(condition, Condition::Contains { .. })))
    });
    if chunks.len() + usize::from(contains) > 8 {
        return Err("semantic task exceeds the host function limit".into());
    }
    let mut source = String::from(
        "// Generated from validated host semantic metadata; no retry or scheduler.\n",
    );
    if contains {
        source.push_str("fn _lese_contains(items: string_list, wanted: string) = fold(\n  _lese_found: false, items: items, item: \"_lese_item\",\n  next: or(left: _lese_found, right: eq(left: _lese_item, right: wanted)), limit: 64\n)\n");
    }
    for (index, steps) in chunks.iter().enumerate() {
        let name = if index == 0 {
            "main".into()
        } else {
            format!("_lese_stage_{index}")
        };
        let mut body = if index + 1 == chunks.len() {
            term(&task.result)
        } else {
            let arguments = parameters[index + 1]
                .iter()
                .map(|(name, _)| format!("{name}: {name}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("_lese_stage_{}({arguments})", index + 1)
        };
        for step in steps.iter().rev() {
            body = match step {
                TaskStep::Call {
                    binding,
                    operation,
                    arguments,
                } => {
                    let declaration = catalog
                        .operations
                        .iter()
                        .find(|candidate| candidate.name == *operation)
                        .expect("validated task operation");
                    let arguments = declaration
                        .parameters
                        .iter()
                        .filter_map(|parameter| {
                            arguments
                                .iter()
                                .find(|argument| argument.name == parameter.name)
                                .map(|argument| {
                                    format!("{}: {}", argument.name, term(&argument.value))
                                })
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("bind({binding}: {operation}({arguments}), body: {body})")
                }
                TaskStep::Guard {
                    conditions,
                    failure,
                } => {
                    let condition = all(&conditions.iter().map(condition).collect::<Vec<_>>());
                    format!(
                        "choose(when: {condition}, then: {body}, otherwise: {})",
                        literal(failure)
                    )
                }
            };
        }
        let arguments = parameters[index]
            .iter()
            .map(|(name, ty)| format!("{name}: {}", type_name(ty)))
            .collect::<Vec<_>>()
            .join(", ");
        source.push_str(&format!("fn {name}({arguments}) = {body}\n"));
    }
    if source.len() > 16 * 1024 {
        return Err("generated semantic source exceeds host source bound".into());
    }
    if !leselang_syntax::parse(&source).diagnostics.is_empty() {
        return Err("semantic task exceeds the existing language parser limits".into());
    }
    Ok(source)
}

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

pub(super) fn verify(
    profile: &str,
    effects: usize,
    contract: &ReportContract,
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
    if result.get("type").and_then(Value::as_str)
        != Some(scalar_name(contract.result.scalar_type()))
        || result.get("value") != Some(&scalar_json(&contract.result))
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

#[cfg(test)]
mod tests {
    use super::*;
    use leselang_runtime_core::discovery::{
        FieldDescription, NativeTypeDescription, OperationDescription, ParameterDescription,
    };
    use serde_json::json;

    fn input() -> Term {
        Term::Input {
            name: "message".into(),
        }
    }
    fn field() -> Term {
        Term::Field {
            binding: "completed".into(),
            name: "contents".into(),
        }
    }
    fn record_term(array: &str, field: &str, scalar_type: ScalarType) -> ReportTerm {
        ReportTerm::Scalar {
            path: vec![
                ReportSegment::Field { name: array.into() },
                ReportSegment::Match {
                    field: "identifier".into(),
                    value: "done-card".into(),
                },
                ReportSegment::Field { name: field.into() },
            ],
            scalar_type,
        }
    }
    fn catalog() -> Vec<u8> {
        let description = CatalogDescription {
            profile: "org.example.workflow_board".into(),
            catalog_version: 1,
            operations: vec![OperationDescription {
                name: "board.complete".into(),
                required_capability: "board.write".into(),
                parameters: vec![ParameterDescription {
                    name: "message".into(),
                    required: true,
                    types: vec![TypeDescription::Scalar(ScalarType::String)],
                }],
                result: TypeDescription::Native("card".into()),
            }],
            native_types: vec![NativeTypeDescription {
                name: "card".into(),
                fields: vec![FieldDescription {
                    name: "contents".into(),
                    scalar_type: ScalarType::String,
                }],
            }],
        };
        let semantics = SemanticDescription {
            version: 1,
            operations: vec![OperationSemantics {
                operation: "board.complete".into(),
                summary: "Complete the scoped card and read its resulting contents.".into(),
                effect: EffectClass::Write,
                completion: CompletionKind::Value,
            }],
            tasks: vec![TaskDescription {
                id: "complete_card".into(),
                summary: "Complete a card and verify its contents and revision.".into(),
                inputs: vec![TaskInput {
                    name: "message".into(),
                    scalar_type: ScalarType::String,
                }],
                steps: vec![
                    TaskStep::Call {
                        binding: "completed".into(),
                        operation: "board.complete".into(),
                        arguments: vec![TaskArgument {
                            name: "message".into(),
                            value: input(),
                        }],
                    },
                    TaskStep::Guard {
                        conditions: vec![Condition::Equal {
                            left: field(),
                            right: input(),
                        }],
                        failure: ScalarValue::Boolean(false),
                    },
                ],
                result: Term::Literal {
                    value: ScalarValue::Boolean(true),
                },
                execution: ExecutionStyle::NamedText,
                verification: ReportContract {
                    kind: "board_completed".into(),
                    max_fuel: 500,
                    result: ScalarValue::Boolean(true),
                    require_verified: true,
                    predicates: vec![
                        ReportPredicate {
                            id: "card_contents".into(),
                            left: record_term("native_after", "contents", ScalarType::String),
                            relation: ReportRelation::Equal,
                            right: ReportTerm::Input {
                                name: "message".into(),
                            },
                        },
                        ReportPredicate {
                            id: "card_revision".into(),
                            left: record_term("native_before", "revision", ScalarType::Integer),
                            relation: ReportRelation::IncreasedBy { amount: 1 },
                            right: record_term("native_after", "revision", ScalarType::Integer),
                        },
                    ],
                },
            }],
        };
        description
            .to_json_with_semantics(&semantics)
            .unwrap()
            .into_bytes()
    }

    fn report(text: &str) -> Value {
        json!({"schema_version":1,"profile":"org.example.workflow_board","kind":"board_completed",
            "result":{"type":"boolean","value":true},"effects":1,"fuel_remaining":400,"verified":true,
            "native_before":[{"identifier":"done-card","contents":"","revision":8}],
            "native_after":[{"identifier":"done-card","contents":text,"revision":9}]})
    }
    fn bytes(value: &Value) -> Vec<u8> {
        serde_json::to_vec(value).unwrap()
    }
    fn changed(change: impl FnOnce(&mut Value)) -> Vec<u8> {
        let mut value: Value = serde_json::from_slice(&catalog()).unwrap();
        change(&mut value);
        bytes(&value)
    }

    #[test]
    fn an_unknown_application_task_and_operation_use_the_generic_pipeline() {
        let bytes = catalog();
        let plan = super::super::prepare(&bytes, "complete_card").unwrap();
        assert!(plan.semantic);
        assert_eq!(plan.profile, "org.example.workflow_board");
        assert_eq!(plan.input_name.as_deref(), Some("message"));
        assert_eq!(plan.execution, ExecutionStyle::NamedText);
        assert_eq!(plan.required_capabilities, ["board.write"]);
        assert_eq!(plan.expected_effects, 1);
        assert!(plan.source.contains("board.complete(message: message)"));
        assert!(!plan.source.contains("gui."));
        super::super::tests::assert_source_program_types(
            &bytes,
            &plan.source,
            ScalarType::Boolean,
            &["message"],
        );
        let text = "quote \" slash \\ newline\n保存 ✓";
        assert_eq!(
            plan.verify(&self::bytes(&report(text)), text)
                .unwrap()
                .effects,
            1
        );
        assert!(!plan.source.contains(text));
        assert!(super::super::prepare(&bytes, "fill_and_save").is_err());
        let updated = changed(|value| value["catalog_version"] = json!(2));
        assert!(
            super::super::prepare(&updated, "complete_card")
                .unwrap()
                .semantic
        );
    }

    #[test]
    fn three_actual_host_semantic_tasks_fit_the_original_typed_compiler_limits() {
        for (catalog, task, result, parameters, effects) in [
            (
                include_bytes!("fixtures/appkit-semantics-v1.json").as_slice(),
                "fill_and_save",
                ScalarType::Boolean,
                &["text"][..],
                9,
            ),
            (
                include_bytes!("fixtures/tauri-semantics-v1.json").as_slice(),
                "fill_and_save",
                ScalarType::Boolean,
                &["text"][..],
                9,
            ),
            (
                include_bytes!("fixtures/calculator-semantics-v1.json").as_slice(),
                "calculate_one_plus_two",
                ScalarType::String,
                &[][..],
                10,
            ),
        ] {
            let plan = super::super::prepare(catalog, task).unwrap();
            assert!(plan.semantic);
            assert_eq!(plan.expected_effects, effects);
            let golden = if parameters.is_empty() {
                include_str!("fixtures/calculator-semantics.lese")
            } else {
                include_str!("fixtures/form-semantics.lese")
            };
            assert_eq!(plan.source, golden);
            super::super::tests::assert_source_program_types(
                catalog,
                &plan.source,
                result,
                parameters,
            );
        }
    }

    #[test]
    fn semantic_metadata_errors_never_fall_back_to_a_known_profile() {
        for invalid in [Value::Null, json!({}), json!({"version":2})] {
            let bytes = changed(|value| {
                value["profile"] = json!("appkit");
                value["semantics"] = invalid;
            });
            assert!(super::super::prepare(&bytes, "complete_card").is_err());
        }
        for bytes in [
            changed(|v| v["semantics"]["version"] = json!(2)),
            changed(|v| v["semantics"]["tasks"][0]["source"] = json!("malicious source")),
            changed(|v| v["semantics"]["tasks"][0]["steps"][0]["operation"] = json!("unknown.op")),
            changed(|v| {
                v["semantics"]["tasks"][0]["steps"][0]["arguments"][0]["value"] =
                    json!({"kind":"binding","name":"future"})
            }),
            changed(|v| {
                v["semantics"]["tasks"][0]["steps"][1]["conditions"][0]["left"]["name"] =
                    json!("unknown_field")
            }),
            changed(|v| v["semantics"]["tasks"][0]["verification"]["predicates"] = json!([])),
            changed(|v| v["semantics"]["tasks"][0]["steps"][0]["binding"] = json!("message")),
            changed(|v| {
                v["semantics"]["tasks"][0]["steps"][1]["failure"] =
                    json!({"kind":"string","value":"wrong result type"})
            }),
        ] {
            assert!(super::super::prepare(&bytes, "complete_card").is_err());
        }
        let duplicated = String::from_utf8(catalog()).unwrap().replacen(
            "\"version\":1",
            "\"version\":1,\"version\":1",
            1,
        );
        assert!(super::super::prepare(duplicated.as_bytes(), "complete_card").is_err());
    }

    #[test]
    fn used_operations_cannot_shadow_generated_functions() {
        for reserved in ["main", "_lese_contains", "_lese_stage_1"] {
            let bytes = changed(|value| {
                value["operations"][0]["name"] = json!(reserved);
                value["semantics"]["operations"][0]["operation"] = json!(reserved);
                value["semantics"]["tasks"][0]["steps"][0]["operation"] = json!(reserved);
            });
            assert!(super::super::prepare(&bytes, "complete_card").is_err());
        }
    }

    #[test]
    fn semantic_report_predicates_reject_missing_ambiguous_wrong_typed_or_overflowed_evidence() {
        let plan = super::super::prepare(&catalog(), "complete_card").unwrap();
        let alter = |change: fn(&mut Value)| {
            let mut value = report("done");
            change(&mut value);
            value
        };
        for invalid in [
            alter(|v| v["profile"] = json!("appkit")),
            alter(|v| v["kind"] = json!("other_kind")),
            alter(|v| v["result"]["value"] = json!(false)),
            alter(|v| v["effects"] = json!(2)),
            alter(|v| v["fuel_remaining"] = json!(501)),
            alter(|v| v["verified"] = json!(false)),
            alter(|v| {
                v["native_after"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("contents");
            }),
            alter(|v| v["native_after"][0]["contents"] = Value::Null),
            alter(|v| v["native_after"][0]["contents"] = json!("wrong")),
            alter(|v| v["native_after"][0]["revision"] = json!("9")),
            alter(|v| v["native_after"][0]["revision"] = json!(10)),
            alter(|v| v["native_before"][0]["revision"] = json!(u64::MAX)),
            alter(|v| {
                let duplicate = v["native_after"][0].clone();
                v["native_after"].as_array_mut().unwrap().push(duplicate);
            }),
            alter(|v| v["extra"] = json!("x".repeat(4097))),
        ] {
            assert!(plan.verify(&bytes(&invalid), "done").is_err(), "{invalid}");
        }
        assert!(
            plan.verify(br#"{"schema_version":1,"schema_version":1}"#, "done")
                .is_err()
        );
        assert!(plan.verify(b"{} {}", "done").is_err());
    }

    #[test]
    fn quoted_literals_use_the_languages_escapes_and_optional_equals_checks_presence() {
        let text = "\"\\\n\t\r中\0";
        let source = format!("fn main() = {}", literal(&ScalarValue::String(text.into())));
        assert!(leselang_syntax::parse(&source).diagnostics.is_empty());
        let generated = condition(&Condition::OptionalEquals {
            optional: Term::Field {
                binding: "observed".into(),
                name: "value".into(),
            },
            value: Term::Literal {
                value: ScalarValue::String(String::new()),
            },
        });
        assert!(generated.contains("has_value(value:"));
        assert!(generated.contains("value_or(left:"));
    }

    #[test]
    fn declared_numeric_upper_bounds_are_inclusive_and_do_not_coerce_values() {
        let catalog = changed(|value| {
            value["semantics"]["tasks"][0]["verification"]["predicates"].as_array_mut().unwrap().push(json!({
                "id":"revision_bound",
                "left":{"kind":"scalar","path":[{"kind":"field","name":"bounded"}],"scalar_type":"integer"},
                "relation":{"kind":"at_most"},
                "right":{"kind":"literal","value":{"kind":"integer","value":i64::MAX}}
            }));
        });
        let plan = super::super::prepare(&catalog, "complete_card").unwrap();
        for (value, accepted) in [
            (json!(0), true),
            (json!(i64::MAX), true),
            (json!(i64::MAX as u64 + 1), false),
            (json!("9"), false),
            (Value::Null, false),
            (json!(-1), false),
        ] {
            let mut report = report("done");
            report["bounded"] = value;
            assert_eq!(plan.verify(&bytes(&report), "done").is_ok(), accepted);
        }
    }

    #[test]
    fn a_new_scalar_host_executes_the_generated_two_effect_task_through_the_real_vm() {
        use leselang_vm::embedding::{Fuel, HostBuilder, Poll, parameter};
        use std::cell::Cell;
        let total = Cell::new(0u64);
        let calls = Cell::new(0);
        let mut builder = HostBuilder::new();
        builder
            .register(
                "meter.add",
                vec![parameter("amount", ScalarType::Integer)],
                ScalarType::Integer,
                "meter.write",
                |arguments, fuel| {
                    fuel.charge(1).unwrap();
                    let Some(ScalarValue::Integer(amount)) = arguments.get("amount") else {
                        unreachable!()
                    };
                    total.set(total.get().checked_add(*amount).unwrap());
                    calls.set(calls.get() + 1);
                    Ok(ScalarValue::Integer(total.get()))
                },
            )
            .unwrap();
        let host = builder.build();
        let number = |value| Term::Literal {
            value: ScalarValue::Integer(value),
        };
        let binding = |name: &str| Term::Binding { name: name.into() };
        let call = |name: &str, value| TaskStep::Call {
            binding: name.into(),
            operation: "meter.add".into(),
            arguments: vec![TaskArgument {
                name: "amount".into(),
                value: number(value),
            }],
        };
        let guard = |name: &str, expected| TaskStep::Guard {
            conditions: vec![Condition::Equal {
                left: binding(name),
                right: number(expected),
            }],
            failure: ScalarValue::Integer(0),
        };
        let semantics = SemanticDescription {
            version: 1,
            operations: vec![OperationSemantics {
                operation: "meter.add".into(),
                summary: "Add to a bounded host counter and return its value.".into(),
                effect: EffectClass::Write,
                completion: CompletionKind::Value,
            }],
            tasks: vec![TaskDescription {
                id: "accumulate".into(),
                summary: "Accumulate and verify 42.".into(),
                inputs: vec![],
                steps: vec![
                    call("first", 19),
                    guard("first", 19),
                    call("second", 23),
                    guard("second", 42),
                ],
                result: binding("second"),
                execution: ExecutionStyle::NoText,
                verification: ReportContract {
                    kind: "meter_run".into(),
                    max_fuel: 1000,
                    result: ScalarValue::Integer(42),
                    require_verified: false,
                    predicates: vec![ReportPredicate {
                        id: "counter_value".into(),
                        left: ReportTerm::Scalar {
                            path: vec![ReportSegment::Field {
                                name: "counter".into(),
                            }],
                            scalar_type: ScalarType::Integer,
                        },
                        relation: ReportRelation::Equal,
                        right: ReportTerm::Literal {
                            value: ScalarValue::Integer(42),
                        },
                    }],
                },
            }],
        };
        let catalog = host
            .catalog_description("new.vendor.meter")
            .to_json_with_semantics(&semantics)
            .unwrap();
        let plan = super::super::prepare(catalog.as_bytes(), "accumulate").unwrap();
        let grants = ["meter.write"];
        let program = host.compile(&plan.source, &grants).unwrap();
        let mut session = host.start(&program, &grants, Fuel::new(1000)).unwrap();
        for expected in [19, 42] {
            let Poll::Request(request) = session.poll().unwrap() else {
                panic!("expected explicit request")
            };
            assert_eq!(calls.get(), usize::from(expected == 42));
            let authorized = session.authorize(request).unwrap();
            let receipt = session.dispatch(authorized).unwrap();
            assert_eq!(total.get(), expected);
            assert!(matches!(session.poll().unwrap(), Poll::Awaiting));
            session.accept(receipt).unwrap();
        }
        assert!(matches!(
            session.poll().unwrap(),
            Poll::Value(ScalarValue::Integer(42))
        ));
        assert_eq!(calls.get(), 2);
        let report = json!({"schema_version":1,"profile":"new.vendor.meter","kind":"meter_run",
            "result":{"type":"integer","value":42},"effects":2,"fuel_remaining":session.fuel_remaining(),"counter":total.get()});
        assert_eq!(plan.verify(&bytes(&report), "").unwrap().effects, 2);
        assert!(session.fuel_remaining() < 1000);
    }
}
