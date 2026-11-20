//! Reference application task contracts belong to hosts, not the language VM.
//! These descriptions grant no authority and contain no source or executable argv.
use leselang_vm::native::core::{
    ScalarType, ScalarValue,
    discovery::semantics::{
        CompletionKind, Condition, EffectClass, ExecutionStyle, OperationSemantics, ReportContract,
        ReportPredicate, ReportRelation, ReportSegment, ReportTerm, SemanticDescription,
        TaskArgument, TaskDescription, TaskInput, TaskStep, Term,
    },
};

pub(crate) fn describe(profile: &str) -> Option<SemanticDescription> {
    match profile {
        "appkit" => Some(form("appkit_run", 1000, "ready", ExecutionStyle::NamedText)),
        "tauri.typescript" => Some(form("tauri_run", 10_000, "Save", ExecutionStyle::Text)),
        "macos_ax.calculator" => Some(calculator()),
        _ => None,
    }
}

fn string(value: &str) -> ScalarValue {
    ScalarValue::String(value.into())
}
fn literal(value: ScalarValue) -> Term {
    Term::Literal { value }
}
fn text(value: &str) -> Term {
    literal(string(value))
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
fn input() -> Term {
    Term::Input {
        name: "text".into(),
    }
}
fn equal(left: Term, right: Term) -> Condition {
    Condition::Equal { left, right }
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
fn guard(conditions: Vec<Condition>, failure: ScalarValue) -> TaskStep {
    TaskStep::Guard {
        conditions,
        failure,
    }
}
fn form_guard(conditions: Vec<Condition>) -> TaskStep {
    guard(conditions, ScalarValue::Boolean(false))
}
fn acknowledged(name: &str) -> Condition {
    equal(binding(name), literal(ScalarValue::Boolean(true)))
}
fn input_ready(name: &str) -> Vec<Condition> {
    vec![
        equal(field(name, "role"), text("text_field")),
        equal(field(name, "editable"), literal(ScalarValue::Boolean(true))),
        equal(field(name, "enabled"), literal(ScalarValue::Boolean(true))),
    ]
}
fn operation(name: &str, summary: &str, write: bool) -> OperationSemantics {
    OperationSemantics {
        operation: name.into(),
        summary: summary.into(),
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
fn property(array: &str, identifier: &str, name: &str, scalar_type: ScalarType) -> ReportTerm {
    let mut path = object(array, identifier);
    path.push(segment(name));
    ReportTerm::Scalar { path, scalar_type }
}
fn report_literal(value: ScalarValue) -> ReportTerm {
    ReportTerm::Literal { value }
}
fn report_text(value: &str) -> ReportTerm {
    report_literal(string(value))
}
fn report_equal(id: &str, left: ReportTerm, right: ReportTerm) -> ReportPredicate {
    ReportPredicate {
        id: id.into(),
        left,
        relation: ReportRelation::Equal,
        right,
    }
}
fn array_length(id: &str, path: Vec<ReportSegment>, length: u64) -> ReportPredicate {
    report_equal(
        id,
        ReportTerm::Length { path },
        report_literal(ScalarValue::Integer(length)),
    )
}

fn form(
    kind: &str,
    max_fuel: u64,
    saved_title: &str,
    execution: ExecutionStyle,
) -> SemanticDescription {
    let mut after_input = input_ready("input_after");
    after_input.push(equal(field("input_after", "value"), input()));
    let steps = vec![
        call("ids", "gui.list", vec![]),
        form_guard(vec![
            Condition::Contains {
                list: binding("ids"),
                value: text("input"),
            },
            Condition::Contains {
                list: binding("ids"),
                value: text("save"),
            },
        ]),
        call("input_object", "gui.find", vec![("name", text("input"))]),
        call(
            "input_before",
            "gui.observe",
            vec![("target", binding("input_object"))],
        ),
        form_guard(input_ready("input_before")),
        call(
            "typed",
            "gui.set_text",
            vec![
                ("target", binding("input_object")),
                ("expected_revision", field("input_before", "revision")),
                ("text", input()),
            ],
        ),
        form_guard(vec![acknowledged("typed")]),
        call(
            "input_after",
            "gui.observe",
            vec![("target", binding("input_object"))],
        ),
        form_guard(after_input),
        call("save_object", "gui.find", vec![("name", text("save"))]),
        call(
            "save_before",
            "gui.observe",
            vec![("target", binding("save_object"))],
        ),
        form_guard(vec![
            equal(field("save_before", "role"), text("button")),
            equal(
                field("save_before", "enabled"),
                literal(ScalarValue::Boolean(true)),
            ),
        ]),
        call(
            "clicked",
            "gui.click",
            vec![
                ("target", binding("save_object")),
                ("expected_revision", field("save_before", "revision")),
            ],
        ),
        form_guard(vec![acknowledged("clicked")]),
        call(
            "save_after",
            "gui.observe",
            vec![("target", binding("save_object"))],
        ),
        form_guard(vec![
            equal(field("save_after", "value"), input()),
            Condition::IncreasedBy {
                before: field("save_before", "clicks"),
                after: field("save_after", "clicks"),
                amount: 1,
            },
        ]),
    ];
    SemanticDescription {
        version: 1,
        operations: vec![
            operation("gui.list", "List the identifiers registered by this form; a listed name is not an execution capability.", false),
            operation("gui.find", "Resolve one registered identifier into an execution-local opaque object reference.", false),
            operation("gui.observe", "Read the selected control's scalar state and revision for later guarded actions.", false),
            operation("gui.set_text", "Queue text replacement for an editable control at the observed revision; observe afterward to verify the value.", true),
            operation("gui.click", "Queue the selected button's native action at the observed revision; acceptance alone does not verify saved application state.", true),
        ],
        tasks: vec![TaskDescription {
            id: "fill_and_save".into(),
            summary: "Fill the registered input, click Save once, and verify saved text and unchanged unrelated control state.".into(),
            inputs: vec![TaskInput { name: "text".into(), scalar_type: ScalarType::String }],
            steps,
            result: literal(ScalarValue::Boolean(true)),
            execution,
            verification: ReportContract {
                kind: kind.into(), max_fuel,
                result: ScalarValue::Boolean(true),
                require_verified: kind == "tauri_run",
                predicates: form_predicates(saved_title),
            },
        }],
    }
}

fn form_predicates(saved_title: &str) -> Vec<ReportPredicate> {
    let mut predicates = vec![
        array_length("before_count", vec![segment("native_before")], 3),
        array_length("after_count", vec![segment("native_after")], 3),
    ];
    for (phase, array) in [("before", "native_before"), ("after", "native_after")] {
        for identifier in ["input", "save", "other"] {
            predicates.push(ReportPredicate {
                id: format!("{identifier}_{phase}_clicks_bound"),
                left: property(array, identifier, "clicks", ScalarType::Integer),
                relation: ReportRelation::AtMost,
                right: report_literal(ScalarValue::Integer(i64::MAX as u64)),
            });
        }
        for (identifier, role, editable) in
            [("input", "text_field", true), ("save", "button", false)]
        {
            for (name, scalar_type, expected) in [
                ("role", ScalarType::String, string(role)),
                (
                    "editable",
                    ScalarType::Boolean,
                    ScalarValue::Boolean(editable),
                ),
                ("enabled", ScalarType::Boolean, ScalarValue::Boolean(true)),
            ] {
                predicates.push(report_equal(
                    &format!("{identifier}_{phase}_{name}"),
                    property(array, identifier, name, scalar_type),
                    report_literal(expected),
                ));
            }
        }
    }
    for identifier in ["input", "save"] {
        predicates.push(report_equal(
            &format!("{identifier}_saved_value"),
            property("native_after", identifier, "value", ScalarType::String),
            ReportTerm::Input {
                name: "text".into(),
            },
        ));
        // Prior text need not be empty, but it must still be present and typed.
        let before = property("native_before", identifier, "value", ScalarType::String);
        predicates.push(report_equal(
            &format!("{identifier}_before_value_type"),
            before.clone(),
            before,
        ));
    }
    predicates.push(report_equal(
        "input_title_unchanged",
        property("native_before", "input", "title", ScalarType::String),
        property("native_after", "input", "title", ScalarType::String),
    ));
    predicates.push(ReportPredicate {
        id: "input_clicks_unchanged".into(),
        left: property("native_before", "input", "clicks", ScalarType::Integer),
        relation: ReportRelation::IncreasedBy { amount: 0 },
        right: property("native_after", "input", "clicks", ScalarType::Integer),
    });
    predicates.push(report_equal(
        "save_title",
        property("native_after", "save", "title", ScalarType::String),
        report_text(saved_title),
    ));
    let before_title = property("native_before", "save", "title", ScalarType::String);
    predicates.push(report_equal(
        "save_before_title_type",
        before_title.clone(),
        before_title,
    ));
    predicates.push(ReportPredicate {
        id: "save_once".into(),
        left: property("native_before", "save", "clicks", ScalarType::Integer),
        relation: ReportRelation::IncreasedBy { amount: 1 },
        right: property("native_after", "save", "clicks", ScalarType::Integer),
    });
    predicates.push(report_equal(
        "other_unchanged",
        ReportTerm::Path {
            path: object("native_before", "other"),
        },
        ReportTerm::Path {
            path: object("native_after", "other"),
        },
    ));
    predicates.push(report_equal(
        "other_role",
        property("native_before", "other", "role", ScalarType::String),
        report_text("button"),
    ));
    for (name, scalar_type) in [
        ("title", ScalarType::String),
        ("value", ScalarType::String),
        ("editable", ScalarType::Boolean),
        ("enabled", ScalarType::Boolean),
        ("clicks", ScalarType::Integer),
    ] {
        let value = property("native_before", "other", name, scalar_type);
        predicates.push(report_equal(
            &format!("other_{name}_type"),
            value.clone(),
            value,
        ));
    }
    predicates
}

fn calculator() -> SemanticDescription {
    let mut steps = vec![
        call("target", "gui.find", vec![("name", text("calculator"))]),
        call("before", "gui.observe", vec![("target", binding("target"))]),
        guard(
            vec![equal(field("before", "display"), text("0"))],
            string("initial display mismatch"),
        ),
    ];
    let mut observed = "before";
    for (button, after) in [
        ("one", "after_one"),
        ("add", "after_add"),
        ("two", "after_two"),
        ("equals", "after"),
    ] {
        steps.push(call(
            button,
            "gui.press",
            vec![
                ("target", binding("target")),
                ("expected_revision", field(observed, "revision")),
                ("button", text(button)),
            ],
        ));
        steps.push(guard(
            vec![acknowledged(button)],
            string("verification failed"),
        ));
        steps.push(call(
            after,
            "gui.observe",
            vec![("target", binding("target"))],
        ));
        observed = after;
    }
    steps.push(guard(
        vec![
            equal(field("after", "display"), text("3")),
            Condition::OptionalEquals {
                optional: field("after", "last_expression"),
                value: text("1 + 2"),
            },
        ],
        string("verification failed"),
    ));
    let mut predicates = vec![
        array_length("before_count", vec![segment("native_before")], 1),
        array_length("after_count", vec![segment("native_after")], 1),
        report_equal(
            "initial_zero",
            property("native_before", "calculator", "display", ScalarType::String),
            report_text("0"),
        ),
        report_equal(
            "display_value",
            property("native_after", "calculator", "display", ScalarType::String),
            report_text("3"),
        ),
        report_equal(
            "last_expression",
            property(
                "native_after",
                "calculator",
                "last_expression",
                ScalarType::String,
            ),
            report_text("1 + 2"),
        ),
    ];
    let before_expression = property(
        "native_before",
        "calculator",
        "last_expression",
        ScalarType::OptionalString,
    );
    predicates.push(report_equal(
        "before_expression_type",
        before_expression.clone(),
        before_expression,
    ));
    for (phase, array) in [("before", "native_before"), ("after", "native_after")] {
        predicates.push(report_equal(
            &format!("{phase}_role"),
            property(array, "calculator", "role", ScalarType::String),
            report_text("AXWindow"),
        ));
        let mut buttons = object(array, "calculator");
        buttons.push(segment("buttons"));
        predicates.push(array_length(
            &format!("{phase}_button_count"),
            buttons.clone(),
            4,
        ));
        for button in ["one", "add", "two", "equals"] {
            let mut path = buttons.clone();
            path.push(ReportSegment::Match {
                field: "name".into(),
                value: button.into(),
            });
            path.push(segment("enabled"));
            predicates.push(report_equal(
                &format!("{phase}_{button}_enabled"),
                ReportTerm::Scalar {
                    path,
                    scalar_type: ScalarType::Boolean,
                },
                report_literal(ScalarValue::Boolean(true)),
            ));
        }
    }
    SemanticDescription {
        version: 1,
        operations: vec![
            operation("gui.find", "Resolve the scoped Calculator window; discovery does not launch or reset the application.", false),
            operation("gui.observe", "Read Calculator display, optional last expression, allowed buttons and current revision.", false),
            operation("gui.press", "Queue one admitted Calculator button at the observed revision; fresh observation must verify its result.", true),
        ],
        tasks: vec![TaskDescription {
            id: "calculate_one_plus_two".into(),
            summary: "Require an initial display of zero, press 1 + 2 = once, and verify both result 3 and expression 1 + 2 without clearing or retrying.".into(),
            inputs: vec![], steps, result: field("after", "display"), execution: ExecutionStyle::NoText,
            verification: ReportContract { kind: "macos_ax_calculator_run".into(), max_fuel: 2000, result: string("3"), require_verified: true, predicates },
        }],
    }
}
