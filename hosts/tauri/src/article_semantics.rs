//! Native v3 article contract over the existing registered-object operations.
//! Its independent JavaScript counterpart uses the same calls and predicates,
//! with only the native report kind and observation field names changed.
use leselang_vm::native::core::{
    ScalarType, ScalarValue,
    discovery::semantics::{
        CompletionKind, Condition, EffectClass, OperationSemantics, ReportPredicate,
        ReportRelation, ReportSegment, ReportTerm, TaskArgument, TaskInput, TaskStep, Term, v2, v3,
    },
};

pub(crate) fn describe() -> v3::SemanticDescription {
    v3::SemanticDescription {
        version: v3::VERSION,
        operations: [
            "gui.list",
            "gui.find",
            "gui.observe",
            "gui.set_text",
            "gui.click",
        ]
        .into_iter()
        .map(|operation| {
            let write = matches!(operation, "gui.set_text" | "gui.click");
            OperationSemantics {
                operation: operation.into(),
                summary: format!("Use the application's registered {operation} operation."),
                effect: if write {
                    EffectClass::Write
                } else {
                    EffectClass::Read
                },
                completion: if write {
                    CompletionKind::Submission
                } else {
                    CompletionKind::Value
                },
            }
        })
        .collect(),
        tasks: vec![read("title"), read("content"), publish(), check()],
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
fn input(name: &str) -> Term {
    Term::Input { name: name.into() }
}
fn field(binding: &str, name: &str) -> Term {
    Term::Field {
        binding: binding.into(),
        name: name.into(),
    }
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
fn equal(left: Term, right: Term) -> Condition {
    Condition::Equal { left, right }
}
fn guard(conditions: Vec<Condition>) -> TaskStep {
    TaskStep::Guard {
        conditions,
        failure: ScalarValue::Boolean(false),
    }
}
fn path(phase: &str, id: Option<&str>, field: Option<&str>) -> Vec<ReportSegment> {
    let mut path = vec![ReportSegment::Field { name: phase.into() }];
    if let Some(id) = id {
        path.push(ReportSegment::Match {
            field: "identifier".into(),
            value: id.into(),
        });
    }
    if let Some(field) = field {
        path.push(ReportSegment::Field { name: field.into() });
    }
    path
}
fn at(phase: &str, id: Option<&str>, field: Option<&str>) -> ReportTerm {
    ReportTerm::Path {
        path: path(phase, id, field),
    }
}
fn exact(id: &str, left: ReportTerm, right: ReportTerm) -> ReportPredicate {
    ReportPredicate {
        id: id.into(),
        left,
        right,
        relation: ReportRelation::Equal,
    }
}
fn unchanged() -> ReportPredicate {
    exact(
        "unchanged",
        at("native_before", None, None),
        at("native_after", None, None),
    )
}
fn contract(result: v2::ResultContract, predicates: Vec<ReportPredicate>) -> v2::ReportContract {
    v2::ReportContract {
        kind: "tauri_task_run".into(),
        max_fuel: 10_000,
        require_verified: false,
        result,
        predicates,
    }
}

fn read(id: &str) -> v3::TaskDescription {
    v3::TaskDescription {
        id: format!("read_{id}"),
        summary: format!("Read the current article {id} without changing state."),
        inputs: vec![],
        execution: v3::ExecutionStyle::NoText,
        steps: vec![
            call("_registered_0", "gui.find", vec![("name", text(id))]),
            call(
                "current",
                "gui.observe",
                vec![("target", binding("_registered_0"))],
            ),
        ],
        result: field("current", "value"),
        verification: contract(
            v2::ResultContract::Observed {
                scalar_type: ScalarType::String,
                path: path("native_after", Some(id), Some("value")),
            },
            vec![unchanged()],
        ),
    }
}

/// Expand the same finite action sequence as defineRegisteredTaskV3. Each write
/// admits against a fresh revision, checks its acknowledgement and observes the
/// accepted state. Input values remain named terms, never generated source.
fn action(id: &str, after: &str, sequence: usize, written: bool) -> Vec<TaskStep> {
    let target = format!("_registered_{sequence}");
    let before = format!("_registered_{}", sequence + 1);
    let ack = format!("_registered_{}", sequence + 2);
    let role = if written { "text_field" } else { "button" };
    let mut conditions = vec![
        equal(field(&before, "role"), text(role)),
        equal(
            field(&before, "enabled"),
            literal(ScalarValue::Boolean(true)),
        ),
    ];
    if written {
        conditions.push(equal(
            field(&before, "editable"),
            literal(ScalarValue::Boolean(true)),
        ));
    }
    let mut arguments = vec![
        ("target", binding(&target)),
        ("expected_revision", field(&before, "revision")),
    ];
    if written {
        arguments.push(("text", input(id)));
    }
    let mut steps = vec![
        call(&target, "gui.find", vec![("name", text(id))]),
        call(&before, "gui.observe", vec![("target", binding(&target))]),
        guard(conditions),
        call(
            &ack,
            if written { "gui.set_text" } else { "gui.click" },
            arguments,
        ),
        guard(vec![equal(
            binding(&ack),
            literal(ScalarValue::Boolean(true)),
        )]),
        call(after, "gui.observe", vec![("target", binding(&target))]),
    ];
    if written {
        steps.push(guard(vec![
            equal(field(after, "role"), text("text_field")),
            equal(field(after, "value"), input(id)),
        ]));
    }
    steps
}

fn publish() -> v3::TaskDescription {
    let mut steps = action("title", "title_after", 0, true);
    steps.extend(action("content", "content_after", 3, true));
    steps.extend(action("publish", "published", 6, false));
    let mut predicates: Vec<_> = [
        ("title_input", "title", "value", "title"),
        ("content_input", "content", "value", "content"),
        ("saved_title", "publish", "title", "title"),
        ("saved_content", "publish", "value", "content"),
    ]
    .into_iter()
    .map(|(id, control, field, input)| {
        exact(
            id,
            at("native_after", Some(control), Some(field)),
            ReportTerm::Input { name: input.into() },
        )
    })
    .collect();
    predicates.push(ReportPredicate {
        id: "published_once".into(),
        left: at("native_before", Some("publish"), Some("clicks")),
        relation: ReportRelation::IncreasedBy { amount: 1 },
        right: at("native_after", Some("publish"), Some("clicks")),
    });
    predicates.push(exact(
        "other_unchanged",
        at("native_before", Some("other"), None),
        at("native_after", Some("other"), None),
    ));
    v3::TaskDescription {
        id: "publish_article".into(),
        summary:
            "Publish the supplied title and content together, and preserve unrelated controls."
                .into(),
        inputs: ["title", "content"]
            .map(|name| TaskInput {
                name: name.into(),
                scalar_type: ScalarType::String,
            })
            .into(),
        execution: v3::ExecutionStyle::NamedStrings,
        steps,
        result: literal(ScalarValue::Boolean(true)),
        verification: contract(ScalarValue::Boolean(true).into(), predicates),
    }
}

fn check() -> v3::TaskDescription {
    v3::TaskDescription {
        id: "check_article".into(),
        summary: "Check that the article's complete registered inventory is preserved.".into(),
        inputs: vec![],
        execution: v3::ExecutionStyle::NoText,
        steps: vec![call("ids", "gui.list", vec![])],
        result: literal(ScalarValue::None),
        verification: contract(ScalarValue::None.into(), vec![unchanged()]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sdk_generated_article_programs_fit_the_unchanged_host_binding_limits() {
        for (name, source) in [
            (
                "read_title",
                include_str!("../../../fixtures/host-semantics/tauri-article-read-title.lese"),
            ),
            (
                "read_content",
                include_str!("../../../fixtures/host-semantics/tauri-article-read-content.lese"),
            ),
            (
                "publish_article",
                include_str!("../../../fixtures/host-semantics/tauri-article-publish.lese"),
            ),
            (
                "check_article",
                include_str!("../../../fixtures/host-semantics/tauri-article-check.lese"),
            ),
        ] {
            let arguments = if name == "publish_article" {
                vec![
                    ("content".into(), "正文\nQuoted: \"literal\" ✓".into()),
                    ("title".into(), "独立标题".into()),
                ]
            } else {
                vec![]
            };
            crate::prepare_named_task(source, arguments).unwrap_or_else(|error| {
                panic!("SDK generated {name} did not compile with existing host limits: {error}")
            });
            assert!(
                crate::prepare_named_task(
                    source,
                    vec![("title".into(), "missing or unexpected input".into())]
                )
                .is_err()
            );
            if name == "publish_article" {
                assert!(
                    crate::prepare_task(
                        source,
                        vec![
                            ("title".into(), "title".into()),
                            ("content".into(), "content".into())
                        ]
                    )
                    .is_err(),
                    "v3 publication must not widen the legacy entry"
                );
            }
        }
    }

    #[test]
    fn article_contract_uses_existing_operations_with_independent_named_inputs() {
        let catalog = crate::catalog::describe_article().unwrap();
        let semantics = describe();
        semantics.validate(&catalog).unwrap();
        assert_eq!(catalog.profile, "tauri.article");
        assert_eq!(
            catalog.operations,
            crate::catalog::describe().unwrap().operations
        );
        assert_eq!(semantics.version, 3);
        assert_eq!(semantics.tasks.len(), 4);
        for (task, effects) in semantics.tasks.iter().zip([2, 2, 12, 1]) {
            assert_eq!(task.verification.kind, "tauri_task_run");
            assert!(!task.verification.require_verified);
            assert_eq!(
                task.steps
                    .iter()
                    .filter(|step| matches!(step, TaskStep::Call { .. }))
                    .count(),
                effects
            );
        }
        let publication = &semantics.tasks[2];
        assert_eq!(
            publication
                .inputs
                .iter()
                .map(|input| input.name.as_str())
                .collect::<Vec<_>>(),
            ["title", "content"]
        );
        assert!(matches!(
            publication.execution,
            v3::ExecutionStyle::NamedStrings
        ));
        assert_eq!(
            crate::describe_agent_article().unwrap(),
            include_str!("../../../fixtures/host-semantics/tauri-article-semantics-v3.json")
        );
    }
}
