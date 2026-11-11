//! CLI-only, bounded rendering of the public compiler and runtime errors.

use leselang_vm::{
    embedding::{CompileError, Error, HostFailure},
    native::{
        core::{CalculationFailure, OperationCatalogError, ReplyAcceptanceError},
        hir::{
            bound_projection_source::BoundProjectionSourceError,
            catalog_group_source::CatalogGroupSourceError, catalog_program::CatalogProgramError,
            catalog_source::CatalogSourceError, choice_source::ChoiceSourceError,
            control_source::ControlSourceError, effect_evaluation::EffectEvaluationFault,
            effect_session::EffectSessionReplyError, effect_stack::EffectStackFault,
            helper_declarations::HelperDeclarationError,
            helper_dependencies::HelperDependencyError, helper_hygiene::HelperHygieneError,
            helper_source::HelperSourceError, projection_source::ProjectionSourceError,
            pure_evaluation::PureEvaluationFault, scalar_source::ScalarSourceError,
            source_call::SourceCallError,
        },
        syntax::Span,
    },
};

const MAX_DEPTH: usize = 64;
const MAX_MESSAGE: usize = 512;
const EXCERPT_CHARS: usize = 120;

struct Note {
    message: String,
    span: Option<Span>,
}

impl Note {
    fn new(message: impl ToString, span: Option<Span>) -> Self {
        Self {
            message: message.to_string(),
            span,
        }
    }
}

/// Syntax codes come from the parser; LSC0001 covers compiler admission failures.
pub fn render_compile(path: &str, source: &str, error: &CompileError) -> String {
    match error {
        CompileError::Syntax(diagnostics) => {
            let mut rendered = String::new();
            for diagnostic in diagnostics {
                rendered.push_str(&render_note(
                    path,
                    source,
                    &diagnostic.code,
                    Note::new(&diagnostic.message, Some(diagnostic.span)),
                ));
            }
            rendered
        }
        CompileError::Source(error) => render_note(path, source, "LSC0001", catalog(error, 0)),
        CompileError::SourceLimit => render_note(path, source, "LSC0001", Note::new(error, None)),
    }
}

fn catalog(error: &CatalogSourceError<Error>, depth: usize) -> Note {
    if depth >= MAX_DEPTH {
        return Note::new(error, None);
    }
    match error {
        CatalogSourceError::Control(error) => control(error, catalog, depth + 1),
        CatalogSourceError::Scalar { span, error } => {
            scalar(error, Some(*span), catalog, depth + 1)
        }
        CatalogSourceError::Call { span, error } => call(error, Some(*span), catalog, depth + 1),
        CatalogSourceError::Projection(error) => match error.as_ref() {
            BoundProjectionSourceError::Projection(error) => {
                projection(error, runtime_note, depth + 1)
            }
            BoundProjectionSourceError::Scope { span, error } => Note::new(error, Some(*span)),
            BoundProjectionSourceError::BoundReference { span }
            | BoundProjectionSourceError::Unbound { span } => Note::new(error, Some(*span)),
        },
        CatalogSourceError::ComputedProjection(error) => projection(error, catalog, depth + 1),
        CatalogSourceError::Mapping { span, error } => {
            Note::new(runtime(error, depth + 1), Some(*span))
        }
        CatalogSourceError::OperationIdentity { span }
        | CatalogSourceError::UnknownLocal { span }
        | CatalogSourceError::NonScalar { span }
        | CatalogSourceError::ResultType { span }
        | CatalogSourceError::UnsupportedSource { span } => Note::new(error, Some(*span)),
        CatalogSourceError::Type(error) => Note::new(error, None),
        CatalogSourceError::Program(error) => program(error, depth + 1),
        CatalogSourceError::Group(error) => match error.as_ref() {
            CatalogGroupSourceError::Source(error) => catalog(error, depth + 1),
            CatalogGroupSourceError::GroupLimit { span }
            | CatalogGroupSourceError::NestedGroup { span }
            | CatalogGroupSourceError::NonAtomic { span, .. }
            | CatalogGroupSourceError::PreparedMember { span, .. }
            | CatalogGroupSourceError::BranchMapping { span, .. }
            | CatalogGroupSourceError::BranchType { span, .. } => Note::new(error, Some(*span)),
            CatalogGroupSourceError::Catalog { span, error } => catalog_error(error, Some(*span)),
            CatalogGroupSourceError::Type(error) => Note::new(error, None),
            _ => Note::new(error, None),
        },
        CatalogSourceError::InconsistentType => Note::new(error, None),
    }
}

fn catalog_error(error: &OperationCatalogError, span: Option<Span>) -> Note {
    match error {
        OperationCatalogError::CapabilityDenied => {
            Note::new("capability denied; std.print requires --allow stdio", span)
        }
        OperationCatalogError::UnknownOperation => Note::new(
            "unknown operation; this CLI provides std.print(message: string)",
            span,
        ),
        _ => Note::new(error, span),
    }
}

fn call<E>(
    error: &SourceCallError<E>,
    span: Option<Span>,
    inner: fn(&E, usize) -> Note,
    depth: usize,
) -> Note {
    if depth >= MAX_DEPTH {
        return Note::new(error, span);
    }
    match error {
        SourceCallError::Catalog(error) => catalog_error(error, span),
        SourceCallError::Names(error) => Note::new(error, span),
        SourceCallError::Shape { span, error } => {
            Note::new(format!("invalid source shape: {error:?}"), Some(*span))
        }
        SourceCallError::Lowering { error, .. } => inner(error, depth + 1),
        SourceCallError::Output { error, .. } => Note::new(error, None),
        SourceCallError::Argument { error, .. } => Note::new(error, span),
        _ => Note::new(error, span),
    }
}

fn control<E>(error: &ControlSourceError<E>, inner: fn(&E, usize) -> Note, depth: usize) -> Note {
    if depth >= MAX_DEPTH {
        return Note::new(error, None);
    }
    match error {
        ControlSourceError::Source(error) => call(error, None, inner, depth + 1),
        ControlSourceError::Lexical(error) => scalar(error, None, inner, depth + 1),
        ControlSourceError::Choice(error) => match error {
            ChoiceSourceError::Source(error) => call(error, None, inner, depth + 1),
            ChoiceSourceError::Native { error, .. } => inner(error, depth + 1),
            ChoiceSourceError::Names { span }
            | ChoiceSourceError::Condition { span }
            | ChoiceSourceError::BranchTypes { span }
            | ChoiceSourceError::ProducedWhen { span, .. } => Note::new(error, Some(*span)),
            ChoiceSourceError::Output { error, .. } => Note::new(error, None),
        },
        ControlSourceError::Projection(error) => projection(error, inner, depth + 1),
        ControlSourceError::Native { error, .. } => inner(error, depth + 1),
        ControlSourceError::Output { span, error } => Note::new(error, Some(*span)),
        ControlSourceError::ProducedCondition { span, error } => Note::new(error, Some(*span)),
        ControlSourceError::Condition { span } | ControlSourceError::BranchTypes { span } => {
            Note::new(error, Some(*span))
        }
    }
}

fn scalar<E>(
    error: &ScalarSourceError<E>,
    context: Option<Span>,
    inner: fn(&E, usize) -> Note,
    depth: usize,
) -> Note {
    if depth >= MAX_DEPTH {
        return Note::new(error, context);
    }
    match error {
        ScalarSourceError::Source(error) => call(error, context, inner, depth + 1),
        ScalarSourceError::Native { error, .. } => inner(error, depth + 1),
        ScalarSourceError::Names { span, expected } => Note::new(
            format!("invalid named operands; expected {}", expected.join(", ")),
            Some(*span),
        ),
        ScalarSourceError::Binary {
            span,
            operator,
            left,
            right,
        } => Note::new(
            format!(
                "{} does not accept {left:?} and {right:?} operands",
                operator.name()
            ),
            Some(*span),
        ),
        ScalarSourceError::Unary { span, operator } => Note::new(
            format!("{} expects {}", operator.name(), operator.expected_input()),
            Some(*span),
        ),
        ScalarSourceError::Bindings { span, error }
        | ScalarSourceError::FoldBindings { span, error }
        | ScalarSourceError::Produced { span, error } => Note::new(error, Some(*span)),
        ScalarSourceError::Generation { span, error } => Note::new(error, Some(*span)),
        ScalarSourceError::LiteralLimit { span }
        | ScalarSourceError::NonScalar { span }
        | ScalarSourceError::InconsistentLiteral { span }
        | ScalarSourceError::Condition { span }
        | ScalarSourceError::BranchTypes { span }
        | ScalarSourceError::RecoveryTypes { span }
        | ScalarSourceError::StringCount { span }
        | ScalarSourceError::StringLabels { span }
        | ScalarSourceError::StringItem { span }
        | ScalarSourceError::StringBytes { span }
        | ScalarSourceError::BindingShape { span }
        | ScalarSourceError::LoopShape { span, .. }
        | ScalarSourceError::LoopLimit { span, .. }
        | ScalarSourceError::LoopCondition { span }
        | ScalarSourceError::LoopState { span }
        | ScalarSourceError::FoldShape { span, .. }
        | ScalarSourceError::FoldItem { span }
        | ScalarSourceError::FoldLimit { span, .. }
        | ScalarSourceError::FoldItems { span }
        | ScalarSourceError::FoldState { span } => Note::new(error, Some(*span)),
    }
}

fn projection<E>(
    error: &ProjectionSourceError<E>,
    inner: fn(&E, usize) -> Note,
    depth: usize,
) -> Note {
    if depth >= MAX_DEPTH {
        return Note::new(error, None);
    }
    match error {
        ProjectionSourceError::Source(error) => call(error, None, inner, depth + 1),
        ProjectionSourceError::Native { error, .. } => inner(error, depth + 1),
        ProjectionSourceError::Generation { span, error } => Note::new(error, Some(*span)),
        ProjectionSourceError::Produced { span, error } => Note::new(error, Some(*span)),
        ProjectionSourceError::NotProjection { span }
        | ProjectionSourceError::Names { span }
        | ProjectionSourceError::FieldName { span }
        | ProjectionSourceError::MemberShape { span }
        | ProjectionSourceError::MemberNames { span }
        | ProjectionSourceError::NonResult { span }
        | ProjectionSourceError::FieldNotExported { span }
        | ProjectionSourceError::MemberNotExported { span } => Note::new(error, Some(*span)),
    }
}

fn program(error: &CatalogProgramError<CatalogSourceError<Error>>, depth: usize) -> Note {
    if depth >= MAX_DEPTH {
        return Note::new(error, None);
    }
    match error {
        CatalogProgramError::Declarations(error) => declaration(error),
        CatalogProgramError::Dependencies(error) => match error {
            HelperDependencyError::InvalidName { span, .. }
            | HelperDependencyError::DuplicateName { span, .. }
            | HelperDependencyError::EntryConflict { span, .. }
            | HelperDependencyError::Source { span, .. }
            | HelperDependencyError::EntryCall { span, .. } => Note::new(error, Some(*span)),
            _ => Note::new(error, None),
        },
        CatalogProgramError::EntryCall { span } => Note::new(error, Some(*span)),
        CatalogProgramError::Names { span, error } => Note::new(error, Some(*span)),
        CatalogProgramError::Arguments { span, error } => match error.as_ref() {
            HelperSourceError::Lowering { error, .. } => catalog(error, depth + 1),
            HelperSourceError::Names(error) => Note::new(error, Some(*span)),
            HelperSourceError::Shape { span, error } => {
                Note::new(format!("invalid helper call shape: {error:?}"), Some(*span))
            }
            HelperSourceError::Argument { error, .. } => Note::new(error, Some(*span)),
            HelperSourceError::Output { error, .. } => Note::new(error, None),
            _ => Note::new(error, Some(*span)),
        },
        CatalogProgramError::Hygiene { span, error } => match error.as_ref() {
            HelperHygieneError::Native(error) => catalog(error, depth + 1),
            _ => Note::new(error, Some(*span)),
        },
        CatalogProgramError::Bindings { span, error } => Note::new(error, Some(*span)),
        _ => Note::new(error, None),
    }
}

fn declaration<E>(error: &HelperDeclarationError<E>) -> Note {
    match error {
        HelperDeclarationError::Parameters { span, error, .. } => Note::new(error, Some(*span)),
        HelperDeclarationError::Source { span, error, .. } => Note::new(error, Some(*span)),
        HelperDeclarationError::InvalidName { span, .. }
        | HelperDeclarationError::ReservedName { span, .. }
        | HelperDeclarationError::DuplicateName { span, .. }
        | HelperDeclarationError::Policy { span, .. }
        | HelperDeclarationError::MissingEntry { span }
        | HelperDeclarationError::EntryParameters { span, .. } => Note::new(error, Some(*span)),
        _ => Note::new(error, None),
    }
}

/// Runtime IR has no retained source spans; do not invent a line number.
pub fn render_runtime(error: &Error) -> String {
    format!(
        "error[LSR0001]: {}\n",
        bounded(&runtime(error, 0), MAX_MESSAGE)
    )
}

fn runtime_note(error: &Error, depth: usize) -> Note {
    Note::new(runtime(error, depth), None)
}

fn runtime(error: &Error, depth: usize) -> String {
    if depth >= MAX_DEPTH {
        return "nested runtime failure".into();
    }
    match error {
        Error::Evaluation(failure) => match failure.as_ref() {
            CalculationFailure::Scalar(error) => error.to_string(),
            CalculationFailure::External(EffectStackFault::Control(error)) => match error {
                EffectEvaluationFault::Native(error) => runtime(error, depth + 1),
                EffectEvaluationFault::Pure(PureEvaluationFault::Native(error)) => {
                    runtime(error, depth + 1)
                }
                EffectEvaluationFault::Pure(PureEvaluationFault::Loop(error)) => error.to_string(),
                EffectEvaluationFault::Pure(PureEvaluationFault::Fold(error)) => error.to_string(),
                EffectEvaluationFault::Pure(PureEvaluationFault::Preflight(error)) => {
                    error.to_string()
                }
                EffectEvaluationFault::Pure(error) => error.to_string(),
                EffectEvaluationFault::CallShape(error) => error.to_string(),
                _ => error.to_string(),
            },
            CalculationFailure::External(error) => error.to_string(),
        },
        Error::Acceptance(error) => match error.as_ref() {
            EffectSessionReplyError::Reply(ReplyAcceptanceError::Authority(error)) => {
                runtime(error, depth + 1)
            }
            _ => error.to_string(),
        },
        Error::Host(HostFailure::Failed(message)) => {
            format!("host operation failed: {}", bounded(message, MAX_MESSAGE))
        }
        Error::Host(HostFailure::Uncertain(message)) => format!(
            "host outcome uncertain (output may be partial): {}",
            bounded(message, MAX_MESSAGE)
        ),
        Error::Host(HostFailure::Cancelled) => "host cancelled execution".into(),
        Error::Denied => "capability denied; std.print requires --allow stdio".into(),
        Error::Capture => "capture failed (capture bounds or fuel exhausted)".into(),
        Error::Execution(message) => bounded(message, MAX_MESSAGE),
        _ => error.to_string(),
    }
}

// Escape terminal controls, and cap user-controlled path/message text by characters.
fn bounded(text: &str, limit: usize) -> String {
    let mut result = String::new();
    let mut chars = text.chars();
    for ch in chars.by_ref().take(limit) {
        push_visible(&mut result, ch);
    }
    if chars.next().is_some() {
        result.push_str("...");
    }
    result
}

fn push_visible(output: &mut String, ch: char) {
    if ch == '\t' {
        output.push_str("    ");
    } else if ch.is_control() || matches!(ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}') {
        output.extend(ch.escape_default());
    } else {
        output.push(ch);
    }
}

/// Present user-controlled paths without unbounded output or terminal controls.
pub fn display_path(path: &str) -> String {
    bounded(path, 256)
}

fn render_note(path: &str, source: &str, code: &str, note: Note) -> String {
    let path = display_path(path);
    let code = bounded(code, 64);
    let message = bounded(&note.message, MAX_MESSAGE);
    let Some(span) = note.span.filter(|span| {
        span.start <= span.end
            && span.end <= source.len()
            && source.is_char_boundary(span.start)
            && source.is_char_boundary(span.end)
    }) else {
        return format!("{path}: error[{code}]: {message}\n");
    };
    let prefix = &source[..span.start];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let line_start = prefix.rfind('\n').map_or(0, |index| index + 1);
    let line_end = source[span.start..]
        .find('\n')
        .map_or(source.len(), |index| span.start + index);
    let content = source[line_start..line_end]
        .strip_suffix('\r')
        .unwrap_or(&source[line_start..line_end]);
    let column = source[line_start..span.start].chars().count();
    let skip = column.saturating_sub(EXCERPT_CHARS / 2);
    let mut excerpt = if skip > 0 {
        "...".into()
    } else {
        String::new()
    };
    let mut caret = excerpt.chars().count();
    let mut width = 0;
    let mut seen = 0;
    let marked_chars = source[span.start..span.end.min(line_end)].chars().count();
    for (index, ch) in content.chars().enumerate().skip(skip).take(EXCERPT_CHARS) {
        let before = excerpt.chars().count();
        push_visible(&mut excerpt, ch);
        let displayed = excerpt.chars().count() - before;
        if index < column {
            caret += displayed;
        } else if index < column + marked_chars {
            width += displayed;
        }
        seen = index + 1;
    }
    if content.chars().nth(seen).is_some() {
        excerpt.push_str("...");
    }
    let mut rendered = format!(
        "{path}:{line}:{}: error[{code}]: {message}\n  | {excerpt}\n",
        column + 1,
    );
    // Columns above count Unicode scalar values. Without a terminal-width
    // dependency, wide characters and combining sequences cannot be aligned
    // reliably; retain their source text without drawing a misleading underline.
    if excerpt.is_ascii() {
        rendered.push_str(&format!(
            "  | {}{}\n",
            " ".repeat(caret),
            "^".repeat(width.clamp(1, 40))
        ));
    }
    rendered
}

#[cfg(test)]
mod tests {
    use super::*;
    use leselang_vm::{
        embedding::{HostBuilder, ScalarType, ScalarValue, parameter},
        native::syntax::Diagnostic,
    };

    #[test]
    fn unicode_columns_crlf_and_eof_are_safe() {
        let source = "// 汉字\r\nfn main() = \"é\" missing";
        let start = source.find("missing").unwrap();
        let error = CompileError::Syntax(vec![Diagnostic {
            code: "LSEtest".into(),
            message: "unexpected name".into(),
            span: Span {
                start,
                end: source.len(),
            },
        }]);
        let rendered = render_compile("unicode.lese", source, &error);
        assert!(
            rendered.contains("unicode.lese:2:17: error[LSEtest]"),
            "{rendered}"
        );
        assert!(rendered.contains("\"é\" missing"));
        assert!(!rendered.contains('\r'));
        let source = "fn main() =\n";
        let error = CompileError::Syntax(vec![Diagnostic {
            code: "LSE1109".into(),
            message: "expected expression".into(),
            span: Span {
                start: source.len(),
                end: source.len(),
            },
        }]);
        assert!(render_compile("eof.lese", source, &error).contains("eof.lese:2:1:"));
    }

    #[test]
    fn syntax_errors_are_all_preserved_and_long_lines_are_bounded() {
        let source = format!("{}!", "x".repeat(10000));
        let error = CompileError::Syntax(vec![
            Diagnostic {
                code: "LSEone".into(),
                message: "first".into(),
                span: Span {
                    start: 10000,
                    end: 10001,
                },
            },
            Diagnostic {
                code: "LSEtwo".into(),
                message: "second".into(),
                span: Span {
                    start: 10001,
                    end: 10001,
                },
            },
        ]);
        let rendered = render_compile("long.lese", &source, &error);
        assert!(rendered.contains("LSEone"));
        assert!(rendered.contains("LSEtwo"));
        assert!(rendered.contains("..."));
        assert!(rendered.len() < 1000);
    }

    #[test]
    fn compile_diagnostics_reach_unknown_denied_and_signature_causes() {
        let mut builder = HostBuilder::new();
        builder
            .register(
                "std.print",
                vec![parameter("message", ScalarType::String)],
                ScalarType::None,
                "stdio",
                |_, _| Ok(ScalarValue::None),
            )
            .unwrap();
        let host = builder.build();
        for (source, grants, expected) in [
            ("fn main() = missing()", &[][..], "unknown operation"),
            (
                "fn main() = std.print(message: \"hello\")",
                &[][..],
                "--allow stdio",
            ),
            (
                "fn main() = std.print(wrong: \"hello\")",
                &["stdio"][..],
                "unknown named argument",
            ),
        ] {
            let error = host.compile(source, grants).err().unwrap();
            let rendered = render_compile("test.lese", source, &error);
            assert!(rendered.contains(expected), "{rendered}");
            assert!(rendered.contains("test.lese:1:13:"), "{rendered}");
        }
    }

    #[test]
    fn spanless_type_failure_does_not_invent_a_location() {
        let error = CompileError::Source(CatalogSourceError::Type(
            leselang_vm::native::hir::flow_typing::CallFlowTypeError::BranchTypes,
        ));
        let rendered = render_compile("types.lese", "fn main() = 0", &error);
        assert!(rendered.starts_with("types.lese: error[LSC0001]:"));
        assert!(!rendered.contains('^'));
    }

    #[test]
    fn compiler_diagnostics_reach_helper_and_scalar_errors() {
        let host = HostBuilder::new().build();
        for (source, expected) in [
            (
                "fn helper(n: signed) = n\nfn main() = 0",
                "helper parameter type is not a closed scalar type",
            ),
            (
                "fn main() = add(left: true, right: 1)",
                "add does not accept Boolean and Integer",
            ),
            (
                "fn main() = choose(when: true, then: 0, otherwise: missing())",
                "unknown operation",
            ),
        ] {
            let error = host.compile(source, &[]).err().unwrap();
            let rendered = render_compile("nested.lese", source, &error);
            assert!(rendered.contains(expected), "{rendered}");
            assert!(rendered.contains("nested.lese:1:"), "{rendered}");
        }
    }

    #[test]
    fn invalid_spans_and_terminal_controls_cannot_break_rendering() {
        let source = "é\u{1b}boom";
        let error = CompileError::Syntax(vec![Diagnostic {
            code: "LSEtest".into(),
            message: "bad\u{1b}[2J input".into(),
            span: Span {
                start: 1,
                end: source.len(),
            },
        }]);
        let rendered = render_compile("input.lese", source, &error);
        assert!(rendered.starts_with("input.lese: error[LSEtest]"));
        assert!(!rendered.contains('\u{1b}'));
        assert!(!rendered.contains('^'));
        let error = CompileError::Syntax(vec![Diagnostic {
            code: "LSEtest".into(),
            message: "bad escape".into(),
            span: Span { start: 2, end: 3 },
        }]);
        let rendered = render_compile("input.lese", source, &error);
        assert!(rendered.contains("input.lese:1:2:"));
        assert!(!rendered.contains('\u{1b}'));
        assert!(!rendered.contains('^'));
    }

    #[test]
    fn unicode_excerpts_preserve_text_without_misaligned_underlines() {
        for source in [
            "fn main() = \"汉字\" missing",
            "fn main() = \"e\u{301}\" missing",
            "fn main() = \"🦀\" missing",
        ] {
            let start = source.find("missing").unwrap();
            let column = source[..start].chars().count() + 1;
            let error = CompileError::Syntax(vec![Diagnostic {
                code: "LSEtest".into(),
                message: "unexpected name".into(),
                span: Span {
                    start,
                    end: source.len(),
                },
            }]);
            let rendered = render_compile("unicode.lese", source, &error);
            assert!(
                rendered.contains(&format!("unicode.lese:1:{column}:")),
                "{rendered}"
            );
            assert!(rendered.contains(source), "{rendered}");
            assert!(!rendered.contains('^'), "{rendered}");
        }
    }

    #[test]
    fn displayed_paths_are_bounded_and_escape_terminal_controls() {
        let path = format!("\u{1b}[2J{}", "字".repeat(1000));
        let displayed = display_path(&path);
        assert!(!displayed.contains('\u{1b}'));
        assert!(displayed.ends_with("..."));
        assert!(displayed.chars().count() < 300);
    }

    #[test]
    fn runtime_renderer_preserves_fuel_and_partial_host_output_causes() {
        let fuel = Error::Evaluation(Box::new(CalculationFailure::External(
            EffectStackFault::Control(EffectEvaluationFault::Pure(
                PureEvaluationFault::FuelExhausted,
            )),
        )));
        assert!(render_runtime(&fuel).contains("fuel exhausted"));
        let host = Error::Host(HostFailure::Uncertain("broken pipe".into()));
        let rendered = render_runtime(&host);
        assert!(rendered.contains("output may be partial"));
        assert!(rendered.contains("broken pipe"));
    }
}
