//! Generate ordinary LeseLang from a completely admitted finite task.
use super::scalar_name;
use leselang_runtime_core::{
    ScalarValue,
    discovery::{CatalogDescription, TypeDescription, semantics::*},
};
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

pub(super) fn literal(value: &ScalarValue) -> String {
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

pub(super) fn condition(value: &Condition) -> String {
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

fn live_names(task: &v2::TaskDescription, start: usize) -> BTreeSet<String> {
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

/// The caller validates the complete metadata first. Bounded stage functions and
/// live parameter selection preserve the existing parser and host limits.
pub(in crate::plan) fn generate(
    catalog: &CatalogDescription,
    task: &v2::TaskDescription,
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
