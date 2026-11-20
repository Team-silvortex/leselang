//! A second application contract over the same registered-object operations.
//! The finite report vocabulary checks exact completion transitions. Its read
//! contracts check types and preservation; the application additionally owns
//! the pending/enabled/zero versus done/disabled/one state invariant.
use leselang_vm::native::core::{
    ScalarType, ScalarValue,
    discovery::semantics::{
        CompletionKind, Condition, EffectClass, ExecutionStyle, OperationSemantics,
        ReportPredicate, ReportRelation, ReportSegment, ReportTerm, TaskArgument, TaskStep, Term,
        v2,
    },
};

const ITEMS: [(&str, &str); 2] = [("review", "核对示例报告"), ("archive", "整理示例资料")];

pub(crate) fn describe() -> v2::SemanticDescription {
    v2::SemanticDescription {
        version: v2::VERSION,
        operations: [
            ("gui.list", "List the registered task identifiers.", false),
            ("gui.find", "Find a task completion control by its declared identifier.", false),
            ("gui.observe", "Observe a task's current status, availability and completion count.", false),
            ("gui.click", "Submit completion of the pending task; observe its state after the application accepts the action.", true),
        ].into_iter().map(|(operation, summary, write)| OperationSemantics {
            operation: operation.into(), summary: summary.into(),
            effect: if write { EffectClass::Write } else { EffectClass::Read },
            completion: if write { CompletionKind::Submission } else { CompletionKind::Value },
        }).collect(),
        tasks: vec![
            read("review", "Read the review task's actual pending or done status without modifying either task."),
            read("archive", "Read the archive task's actual pending or done status without modifying either task."),
            complete("review", "Complete the pending review task once, verify done status and one completion, and preserve the archive task."),
            complete("archive", "Complete the pending archive task once, verify done status and one completion, and preserve the review task."),
            check(),
        ],
    }
}

fn literal(value: ScalarValue) -> Term {
    Term::Literal { value }
}
fn text(value: &str) -> Term {
    literal(ScalarValue::String(value.into()))
}
fn binding(name: &str) -> Term {
    Term::Binding { name: name.into() }
}
fn field(binding: &str, name: &str) -> Term {
    Term::Field {
        binding: binding.into(),
        name: name.into(),
    }
}
fn call(name: &str, operation: &str, args: Vec<(&str, Term)>) -> TaskStep {
    TaskStep::Call {
        binding: name.into(),
        operation: operation.into(),
        arguments: args
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
fn object(array: &str, item: &str) -> Vec<ReportSegment> {
    vec![
        segment(array),
        ReportSegment::Match {
            field: "identifier".into(),
            value: item.into(),
        },
    ]
}
fn path(array: &str, item: &str, name: &str) -> Vec<ReportSegment> {
    let mut path = object(array, item);
    path.push(segment(name));
    path
}
fn scalar(array: &str, item: &str, name: &str, scalar_type: ScalarType) -> ReportTerm {
    ReportTerm::Scalar {
        path: path(array, item, name),
        scalar_type,
    }
}
fn exact(id: impl Into<String>, left: ReportTerm, right: ReportTerm) -> ReportPredicate {
    ReportPredicate {
        id: id.into(),
        left,
        relation: ReportRelation::Equal,
        right,
    }
}
fn value(value: ScalarValue) -> ReportTerm {
    ReportTerm::Literal { value }
}
fn unchanged(item: &str) -> ReportPredicate {
    exact(
        format!("{item}_unchanged"),
        ReportTerm::Path {
            path: object("native_before", item),
        },
        ReportTerm::Path {
            path: object("native_after", item),
        },
    )
}

fn observations() -> Vec<ReportPredicate> {
    let mut predicates = Vec::new();
    for array in ["native_before", "native_after"] {
        predicates.push(exact(
            format!("{array}_count"),
            ReportTerm::Length {
                path: vec![segment(array)],
            },
            value(ScalarValue::Integer(2)),
        ));
        for (item, title) in ITEMS {
            for (name, scalar_type, expected) in [
                (
                    "role",
                    ScalarType::String,
                    Some(ScalarValue::String("button".into())),
                ),
                (
                    "title",
                    ScalarType::String,
                    Some(ScalarValue::String(title.into())),
                ),
                (
                    "editable",
                    ScalarType::Boolean,
                    Some(ScalarValue::Boolean(false)),
                ),
                ("value", ScalarType::String, None),
                ("enabled", ScalarType::Boolean, None),
            ] {
                let actual = scalar(array, item, name, scalar_type);
                predicates.push(exact(
                    format!("{array}_{item}_{name}"),
                    actual.clone(),
                    expected.map_or(actual, value),
                ));
            }
            predicates.push(ReportPredicate {
                id: format!("{array}_{item}_clicks"),
                left: scalar(array, item, "clicks", ScalarType::Integer),
                relation: ReportRelation::AtMost,
                right: value(ScalarValue::Integer(1)),
            });
        }
    }
    predicates
}

fn contract(result: v2::ResultContract, predicates: Vec<ReportPredicate>) -> v2::ReportContract {
    v2::ReportContract {
        kind: "tauri_task_run".into(),
        max_fuel: 10_000,
        result,
        require_verified: false,
        predicates,
    }
}

fn read(item: &str, summary: &str) -> v2::TaskDescription {
    let mut predicates = observations();
    predicates.extend(ITEMS.map(|(item, _)| unchanged(item)));
    v2::TaskDescription {
        id: format!("read_{item}_status"),
        summary: summary.into(),
        inputs: vec![],
        steps: vec![
            call("task_object", "gui.find", vec![("name", text(item))]),
            call(
                "task_state",
                "gui.observe",
                vec![("target", binding("task_object"))],
            ),
        ],
        result: field("task_state", "value"),
        execution: ExecutionStyle::NoText,
        verification: contract(
            v2::ResultContract::Observed {
                scalar_type: ScalarType::String,
                path: path("native_after", item, "value"),
            },
            predicates,
        ),
    }
}

fn state_conditions(item: &str, name: &str, done: bool) -> Vec<Condition> {
    let title = ITEMS.iter().find(|(id, _)| *id == item).unwrap().1;
    [
        ("identifier", ScalarValue::String(item.into())),
        ("role", ScalarValue::String("button".into())),
        ("title", ScalarValue::String(title.into())),
        ("editable", ScalarValue::Boolean(false)),
        (
            "value",
            ScalarValue::String(if done { "done" } else { "pending" }.into()),
        ),
        ("enabled", ScalarValue::Boolean(!done)),
        ("clicks", ScalarValue::Integer(u64::from(done))),
    ]
    .into_iter()
    .map(|(field_name, expected)| Condition::Equal {
        left: field(name, field_name),
        right: literal(expected),
    })
    .collect()
}

fn complete(item: &str, summary: &str) -> v2::TaskDescription {
    let mut after = state_conditions(item, "after", true);
    after.push(Condition::Equal {
        left: binding("submitted"),
        right: literal(ScalarValue::Boolean(true)),
    });
    let mut predicates = observations();
    for (array, done) in [("native_before", false), ("native_after", true)] {
        for (name, expected) in [
            (
                "value",
                ScalarValue::String(if done { "done" } else { "pending" }.into()),
            ),
            ("enabled", ScalarValue::Boolean(!done)),
            ("clicks", ScalarValue::Integer(u64::from(done))),
        ] {
            predicates.push(exact(
                format!("{array}_{item}_{name}_transition"),
                scalar(array, item, name, expected.scalar_type()),
                value(expected),
            ));
        }
    }
    predicates.push(unchanged(if item == "review" {
        "archive"
    } else {
        "review"
    }));
    v2::TaskDescription {
        id: format!("complete_{item}"),
        summary: summary.into(),
        inputs: vec![],
        steps: vec![
            call("task_object", "gui.find", vec![("name", text(item))]),
            call(
                "before",
                "gui.observe",
                vec![("target", binding("task_object"))],
            ),
            TaskStep::Guard {
                conditions: state_conditions(item, "before", false),
                failure: ScalarValue::String("failed".into()),
            },
            call(
                "submitted",
                "gui.click",
                vec![
                    ("target", binding("task_object")),
                    ("expected_revision", field("before", "revision")),
                ],
            ),
            call(
                "after",
                "gui.observe",
                vec![("target", binding("task_object"))],
            ),
            TaskStep::Guard {
                conditions: after,
                failure: ScalarValue::String("failed".into()),
            },
        ],
        result: text("done"),
        execution: ExecutionStyle::NoText,
        verification: contract(ScalarValue::String("done".into()).into(), predicates),
    }
}

fn check() -> v2::TaskDescription {
    let mut predicates = observations();
    predicates.extend(ITEMS.map(|(item, _)| unchanged(item)));
    v2::TaskDescription {
        id: "check_board".into(),
        summary: "Discover both task controls and verify their observed state remains unchanged."
            .into(),
        inputs: vec![],
        execution: ExecutionStyle::NoText,
        steps: vec![
            call("ids", "gui.list", vec![]),
            TaskStep::Guard {
                conditions: ITEMS
                    .map(|(item, _)| Condition::Contains {
                        list: binding("ids"),
                        value: text(item),
                    })
                    .into(),
                failure: ScalarValue::None,
            },
        ],
        result: literal(ScalarValue::None),
        verification: contract(ScalarValue::None.into(), predicates),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn board_contract_uses_the_live_catalog_without_form_tasks_or_new_operations() {
        let catalog = crate::catalog::describe_board().unwrap();
        let semantics = describe();
        semantics.validate(&catalog).unwrap();
        assert_eq!(catalog.profile, "tauri.taskboard");
        assert_eq!(
            catalog.operations,
            crate::catalog::describe().unwrap().operations
        );
        assert_eq!(semantics.version, 2);
        assert_eq!(semantics.tasks.len(), 5);
        for task in &semantics.tasks {
            assert!(task.inputs.is_empty());
            assert!(matches!(task.execution, ExecutionStyle::NoText));
            assert!(task.verification.predicates.len() <= 64);
            assert_eq!(task.verification.kind, "tauri_task_run");
            assert!(!task.verification.require_verified);
            let calls = task
                .steps
                .iter()
                .filter(|step| matches!(step, TaskStep::Call { .. }))
                .count();
            assert_eq!(
                calls,
                if task.id.starts_with("read_") {
                    2
                } else if task.id.starts_with("complete_") {
                    4
                } else {
                    1
                }
            );
        }
        let encoded = catalog.to_json_with_semantics_v2(&semantics).unwrap();
        assert_eq!(
            encoded,
            include_str!("../../../fixtures/host-semantics/tauri-board-semantics-v2.json")
        );
        assert!(!encoded.contains("fill_and_save"));
        assert!(!encoded.contains("save_message"));
    }

    #[test]
    fn sdk_generated_board_programs_compile_with_the_real_task_carrier_without_inputs() {
        for (name, source) in [
            (
                "read_review_status",
                include_str!("../../../fixtures/host-semantics/tauri-board-read-review.lese"),
            ),
            (
                "read_archive_status",
                include_str!("../../../fixtures/host-semantics/tauri-board-read-archive.lese"),
            ),
            (
                "complete_review",
                include_str!("../../../fixtures/host-semantics/tauri-board-complete-review.lese"),
            ),
            (
                "complete_archive",
                include_str!("../../../fixtures/host-semantics/tauri-board-complete-archive.lese"),
            ),
            (
                "check_board",
                include_str!("../../../fixtures/host-semantics/tauri-board-check.lese"),
            ),
        ] {
            crate::prepare_task(source, vec![])
                .unwrap_or_else(|error| panic!("SDK generated {name} did not compile: {error}"));
            assert!(
                crate::prepare_task(source, vec![("text".into(), "unexpected input".into())])
                    .is_err(),
                "{name} must not acquire a form text parameter"
            );
        }
    }
}
