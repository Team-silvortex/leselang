//! Task declarations for the interactive application, separate from its legacy runner.
use leselang_vm::native::core::{
    ScalarType, ScalarValue,
    discovery::semantics::{
        Condition, ExecutionStyle, ReportContract, ReportPredicate, ReportRelation, ReportSegment,
        ReportTerm, SemanticDescription, TaskArgument, TaskDescription, TaskStep, Term,
    },
};

pub(crate) fn describe() -> SemanticDescription {
    let mut description = crate::reference_semantics::describe("tauri.typescript")
        .expect("the reference application has a form contract");
    let save = &mut description.tasks[0];
    save.verification.kind = "tauri_task_run".into();
    save.verification.require_verified = false;

    let mut message = save.clone();
    message.id = "save_message".into();
    message.summary = "Save the supplied message once, return saved, and verify its value while preserving unrelated controls.".into();
    message.inputs[0].name = "message".into();
    message.result = literal(ScalarValue::String("saved".into()));
    message.verification.result = ScalarValue::String("saved".into());
    for step in &mut message.steps {
        match step {
            TaskStep::Call { arguments, .. } => {
                for argument in arguments {
                    rename_input(&mut argument.value);
                }
            }
            TaskStep::Guard {
                conditions,
                failure,
            } => {
                *failure = ScalarValue::String("failed".into());
                for condition in conditions {
                    let (left, right) = match condition {
                        Condition::Equal { left, right } => (left, right),
                        Condition::Contains { list, value } => (list, value),
                        Condition::IncreasedBy { before, after, .. } => (before, after),
                        Condition::OptionalEquals { optional, value } => (optional, value),
                    };
                    rename_input(left);
                    rename_input(right);
                }
            }
        }
    }
    for predicate in &mut message.verification.predicates {
        for term in [&mut predicate.left, &mut predicate.right] {
            if let ReportTerm::Input { name } = term {
                *name = "message".into();
            }
        }
    }
    description.tasks.push(message);
    description.tasks.push(inspect(
        "inspect_controls",
        "Inspect the three registered controls without changing application state; return the verified control count.",
        ScalarValue::Integer(3),
        ScalarValue::Integer(0),
    ));
    description.tasks.push(inspect(
        "check_controls",
        "Check the registered controls and preserve all application state; complete with no result value.",
        ScalarValue::None,
        ScalarValue::None,
    ));
    description
}

fn rename_input(term: &mut Term) {
    if let Term::Input { name } = term {
        *name = "message".into();
    }
}

fn literal(value: ScalarValue) -> Term {
    Term::Literal { value }
}

fn binding(name: &str) -> Term {
    Term::Binding { name: name.into() }
}

fn text(value: &str) -> Term {
    literal(ScalarValue::String(value.into()))
}

fn call(binding: &str, operation: &str, arguments: Vec<(&str, Term)>) -> TaskStep {
    TaskStep::Call {
        binding: binding.into(),
        operation: operation.into(),
        arguments: arguments
            .into_iter()
            .map(|(name, value)| TaskArgument {
                name: name.into(),
                value,
            })
            .collect(),
    }
}

fn segment(name: &str) -> ReportSegment {
    ReportSegment::Field { name: name.into() }
}

fn object(array: &str, identifier: &str) -> Vec<ReportSegment> {
    vec![
        segment(array),
        ReportSegment::Match {
            field: "identifier".into(),
            value: identifier.into(),
        },
    ]
}

fn equal(id: &str, left: ReportTerm, right: ReportTerm) -> ReportPredicate {
    ReportPredicate {
        id: id.into(),
        left,
        relation: ReportRelation::Equal,
        right,
    }
}

fn unchanged_controls() -> Vec<ReportPredicate> {
    let mut predicates = Vec::new();
    for array in ["native_before", "native_after"] {
        predicates.push(equal(
            &format!("{array}_count"),
            ReportTerm::Length {
                path: vec![segment(array)],
            },
            ReportTerm::Literal {
                value: ScalarValue::Integer(3),
            },
        ));
    }
    for (identifier, role, editable) in [
        ("input", "text_field", true),
        ("save", "button", false),
        ("other", "button", false),
    ] {
        predicates.push(equal(
            &format!("{identifier}_unchanged"),
            ReportTerm::Path {
                path: object("native_before", identifier),
            },
            ReportTerm::Path {
                path: object("native_after", identifier),
            },
        ));
        for (name, scalar_type, expected) in [
            (
                "role",
                ScalarType::String,
                Some(ScalarValue::String(role.into())),
            ),
            (
                "editable",
                ScalarType::Boolean,
                Some(ScalarValue::Boolean(editable)),
            ),
            (
                "enabled",
                ScalarType::Boolean,
                Some(ScalarValue::Boolean(true)),
            ),
            ("title", ScalarType::String, None),
            ("value", ScalarType::String, None),
            ("clicks", ScalarType::Integer, None),
        ] {
            let mut path = object("native_before", identifier);
            path.push(segment(name));
            let value = ReportTerm::Scalar { path, scalar_type };
            predicates.push(equal(
                &format!("{identifier}_{name}"),
                value.clone(),
                expected.map_or(value, |value| ReportTerm::Literal { value }),
            ));
        }
    }
    predicates
}

fn inspect(id: &str, summary: &str, result: ScalarValue, failure: ScalarValue) -> TaskDescription {
    TaskDescription {
        id: id.into(),
        summary: summary.into(),
        inputs: Vec::new(),
        steps: vec![
            call("ids", "gui.list", vec![]),
            TaskStep::Guard {
                conditions: ["input", "save", "other"]
                    .into_iter()
                    .map(|identifier| Condition::Contains {
                        list: binding("ids"),
                        value: text(identifier),
                    })
                    .collect(),
                failure: failure.clone(),
            },
            call("save_object", "gui.find", vec![("name", text("save"))]),
            call(
                "saved",
                "gui.observe",
                vec![("target", binding("save_object"))],
            ),
            TaskStep::Guard {
                conditions: vec![Condition::Equal {
                    left: Term::Field {
                        binding: "saved".into(),
                        name: "role".into(),
                    },
                    right: text("button"),
                }],
                failure,
            },
        ],
        result: literal(result.clone()),
        execution: ExecutionStyle::NoText,
        verification: ReportContract {
            kind: "tauri_task_run".into(),
            max_fuel: 10_000,
            result,
            require_verified: false,
            predicates: unchanged_controls(),
        },
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn agent_catalog_matches_the_public_fixture_without_replacing_legacy_description() {
        let catalog = crate::catalog::describe().unwrap();
        let description = super::describe();
        description.validate(&catalog).unwrap();
        assert_eq!(
            catalog.to_json_with_semantics(&description).unwrap(),
            include_str!(
                "../../../consumer/examples/catalog_agent/fixtures/tauri-agent-semantics-v1.json"
            )
        );
        assert_eq!(
            crate::describe().unwrap(),
            include_str!(
                "../../../consumer/examples/catalog_agent/fixtures/tauri-semantics-v1.json"
            )
        );
    }
}
