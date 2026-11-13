//! Host-neutral, structured views of scalar embedding failures.
//!
//! These diagnostics supplement the original error types; they do not compile,
//! execute, authorize or retry work. Messages are descriptive text rather than
//! a parsing contract. Use [`DiagnosticKind`] for the classifications exposed
//! here and retain the original error when richer HIR details are needed.
//! Syntax codes are preserved; compilation and runtime views retain the
//! `LSC0001` and `LSR0001` codes used by the reference CLI.
//!
//! Returned messages contain at most 512 Unicode scalar values, including any
//! truncation marker. They are not escaped for a terminal, JSON or HTML; each
//! presentation layer must escape them appropriately. No source or path is
//! retained. Runtime IR does not retain source spans, so runtime locations are
//! deliberately absent.

use crate::{
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

/// A host-neutral category; further categories may be added without changing
/// the original compiler or runtime error types.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DiagnosticKind {
    Syntax,
    Compilation,
    UnknownOperation,
    CapabilityDenied,
    Runtime,
    HostUncertain,
}

impl DiagnosticKind {
    /// Stable snake-case category for machine-readable presentation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Syntax => "syntax",
            Self::Compilation => "compilation",
            Self::UnknownOperation => "unknown_operation",
            Self::CapabilityDenied => "capability_denied",
            Self::Runtime => "runtime",
            Self::HostUncertain => "host_uncertain",
        }
    }
}

/// An owned error view. Byte spans are half-open UTF-8 source intervals.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub code: String,
    pub kind: DiagnosticKind,
    pub message: String,
    pub span: Option<Span>,
}

/// A one-based line and column. Columns count Unicode scalar values, not UTF-8
/// bytes, UTF-16 code units, grapheme clusters or terminal display cells.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourcePosition {
    pub line: usize,
    pub column: usize,
}

/// The start and exclusive end of a source interval. LF separates lines; a
/// CRLF sequence advances to the next line once, at LF.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceRange {
    pub start: SourcePosition,
    pub end: SourcePosition,
}

impl Diagnostic {
    /// Resolve this span against the original source. Missing, reversed,
    /// out-of-bounds or non-UTF-8-boundary spans have no location. The caller
    /// must provide the source that produced the error: this view does not
    /// retain or authenticate a source buffer.
    pub fn location(&self, source: &str) -> Option<SourceRange> {
        let span = self.span?;
        if span.start > span.end
            || span.end > source.len()
            || !source.is_char_boundary(span.start)
            || !source.is_char_boundary(span.end)
        {
            return None;
        }
        let position = |offset| {
            let prefix = &source[..offset];
            let line_start = prefix.rfind('\n').map_or(0, |index| index + 1);
            SourcePosition {
                line: prefix.bytes().filter(|byte| *byte == b'\n').count() + 1,
                column: source[line_start..offset].chars().count() + 1,
            }
        };
        Some(SourceRange {
            start: position(span.start),
            end: position(span.end),
        })
    }
}

struct Note {
    kind: DiagnosticKind,
    message: String,
    span: Option<Span>,
}

impl Note {
    fn new(message: impl ToString, span: Option<Span>) -> Self {
        Self {
            kind: DiagnosticKind::Compilation,
            message: bounded(&message.to_string()),
            span,
        }
    }

    fn kind(mut self, kind: DiagnosticKind) -> Self {
        self.kind = kind;
        self
    }

    fn at(mut self, span: Option<Span>) -> Self {
        self.span = span;
        self
    }

    fn diagnostic(self, code: impl Into<String>) -> Diagnostic {
        Diagnostic {
            code: code.into(),
            kind: self.kind,
            message: self.message,
            span: self.span,
        }
    }
}

/// Extract each syntax diagnostic in parser order, or the compiler's single
/// admission failure. This borrows the error and never changes its contents.
/// Nested source causes are followed for at most 64 diagnostic layers.
pub fn compile(error: &CompileError) -> Vec<Diagnostic> {
    match error {
        CompileError::Syntax(diagnostics) => diagnostics
            .iter()
            .map(|diagnostic| {
                Note::new(&diagnostic.message, Some(diagnostic.span))
                    .kind(DiagnosticKind::Syntax)
                    .diagnostic(&diagnostic.code)
            })
            .collect(),
        CompileError::Source(error) => vec![catalog(error, 0).diagnostic("LSC0001")],
        CompileError::SourceLimit => vec![Note::new(error, None).diagnostic("LSC0001")],
    }
}

fn catalog(error: &CatalogSourceError<Error>, depth: usize) -> Note {
    if depth >= MAX_DEPTH {
        return Note::new("nested compilation failure", None);
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
            runtime_note(error, depth + 1).at(Some(*span))
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
            Note::new("capability denied", span).kind(DiagnosticKind::CapabilityDenied)
        }
        OperationCatalogError::UnknownOperation => {
            Note::new("unknown operation", span).kind(DiagnosticKind::UnknownOperation)
        }
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
        return Note::new("nested compilation failure", span);
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
        return Note::new("nested compilation failure", None);
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
        return Note::new("nested compilation failure", context);
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
        return Note::new("nested compilation failure", None);
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
        return Note::new("nested compilation failure", None);
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

/// Extract a runtime cause without inventing a source location. An uncertain
/// host outcome can have committed partial effects; this view never authorizes
/// retry, cancels a session or rolls back work.
pub fn runtime(error: &Error) -> Diagnostic {
    runtime_note(error, 0).diagnostic("LSR0001")
}

fn runtime_leaf(message: impl ToString) -> Note {
    Note::new(message, None).kind(DiagnosticKind::Runtime)
}

fn runtime_note(error: &Error, depth: usize) -> Note {
    if depth >= MAX_DEPTH {
        return runtime_leaf("nested runtime failure");
    }
    match error {
        Error::Evaluation(failure) => match failure.as_ref() {
            CalculationFailure::Scalar(error) => runtime_leaf(error),
            CalculationFailure::External(EffectStackFault::Control(error)) => match error {
                EffectEvaluationFault::Native(error) => runtime_note(error, depth + 1),
                EffectEvaluationFault::Pure(PureEvaluationFault::Native(error)) => {
                    runtime_note(error, depth + 1)
                }
                EffectEvaluationFault::Pure(PureEvaluationFault::Loop(error)) => {
                    runtime_leaf(error)
                }
                EffectEvaluationFault::Pure(PureEvaluationFault::Fold(error)) => {
                    runtime_leaf(error)
                }
                EffectEvaluationFault::Pure(PureEvaluationFault::Preflight(error)) => {
                    runtime_leaf(error)
                }
                EffectEvaluationFault::Pure(error) => runtime_leaf(error),
                EffectEvaluationFault::CallShape(error) => runtime_leaf(error),
                _ => runtime_leaf(error),
            },
            CalculationFailure::External(error) => runtime_leaf(error),
        },
        Error::Acceptance(error) => match error.as_ref() {
            EffectSessionReplyError::Reply(ReplyAcceptanceError::Authority(error)) => {
                runtime_note(error, depth + 1)
            }
            _ => runtime_leaf(error),
        },
        Error::Host(HostFailure::Failed(message)) => {
            runtime_leaf(format!("host operation failed: {}", bounded(message)))
        }
        Error::Host(HostFailure::Uncertain(message)) => runtime_leaf(format!(
            "host outcome uncertain (effects may be partial): {}",
            bounded(message)
        ))
        .kind(DiagnosticKind::HostUncertain),
        Error::Host(HostFailure::Cancelled) => runtime_leaf("host cancelled execution"),
        Error::Denied => runtime_leaf("capability denied").kind(DiagnosticKind::CapabilityDenied),
        Error::Capture => runtime_leaf("capture failed (capture bounds or fuel exhausted)"),
        Error::Execution(message) => runtime_leaf(bounded(message)),
        _ => runtime_leaf(error),
    }
}

fn bounded(text: &str) -> String {
    if text.chars().take(MAX_MESSAGE + 1).count() <= MAX_MESSAGE {
        return text.to_owned();
    }
    let mut message: String = text.chars().take(MAX_MESSAGE - 3).collect();
    message.push_str("...");
    message
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        embedding::{HostBuilder, ScalarType, ScalarValue, parameter},
        native::{
            hir::{control_source::ControlSourcePhase, flow_typing::CallFlowTypeError},
            syntax::Diagnostic as SyntaxDiagnostic,
        },
    };
    use std::cell::Cell;

    fn source_diagnostic(span: Option<Span>) -> Diagnostic {
        Diagnostic {
            code: "LSEtest".into(),
            kind: DiagnosticKind::Syntax,
            message: "source error".into(),
            span,
        }
    }

    #[test]
    fn locations_count_unicode_scalars_and_keep_exclusive_multiline_ends() {
        let source = "// 汉字\r\nfn main() = \"e\u{301}🦀\"\r\n";
        let start = source.find('e').unwrap();
        let end = source.find('🦀').unwrap() + '🦀'.len_utf8();
        assert_eq!(
            source_diagnostic(Some(Span { start, end })).location(source),
            Some(SourceRange {
                start: SourcePosition {
                    line: 2,
                    column: 14
                },
                end: SourcePosition {
                    line: 2,
                    column: 17
                },
            })
        );
        assert_eq!(
            source_diagnostic(Some(Span {
                start: 0,
                end: source.len()
            }))
            .location(source),
            Some(SourceRange {
                start: SourcePosition { line: 1, column: 1 },
                end: SourcePosition { line: 3, column: 1 },
            })
        );
        assert_eq!(
            source_diagnostic(Some(Span {
                start: source.len(),
                end: source.len()
            }))
            .location(source),
            Some(SourceRange {
                start: SourcePosition { line: 3, column: 1 },
                end: SourcePosition { line: 3, column: 1 },
            })
        );
        assert_eq!(
            source_diagnostic(Some(Span { start: 0, end: 0 })).location(""),
            Some(SourceRange {
                start: SourcePosition { line: 1, column: 1 },
                end: SourcePosition { line: 1, column: 1 },
            })
        );
    }

    #[test]
    fn absent_reversed_out_of_bounds_and_split_utf8_spans_have_no_location() {
        for span in [
            None,
            Some(Span { start: 2, end: 0 }),
            Some(Span { start: 0, end: 3 }),
            Some(Span { start: 1, end: 2 }),
            Some(Span { start: 0, end: 1 }),
            Some(Span {
                start: usize::MAX,
                end: usize::MAX,
            }),
        ] {
            assert_eq!(source_diagnostic(span).location("é"), None, "{span:?}");
        }
    }

    #[test]
    fn syntax_views_preserve_order_codes_and_raw_message_characters() {
        let error = CompileError::Syntax(vec![
            SyntaxDiagnostic {
                code: "LSE0002".into(),
                message: "unexpected \u{1b}\t汉字".into(),
                span: Span { start: 0, end: 1 },
            },
            SyntaxDiagnostic {
                code: "LSE1109".into(),
                message: "expected expression".into(),
                span: Span { start: 1, end: 1 },
            },
        ]);
        let views = compile(&error);
        assert_eq!(views.len(), 2);
        assert_eq!(views[0].code, "LSE0002");
        assert_eq!(views[1].code, "LSE1109");
        assert_eq!(views[0].kind, DiagnosticKind::Syntax);
        assert_eq!(views[0].message, "unexpected \u{1b}\t汉字");
        assert_eq!(views[1].span, Some(Span { start: 1, end: 1 }));
        assert!(matches!(&error, CompileError::Syntax(original) if original.len() == 2));
    }

    #[test]
    fn custom_hosts_get_neutral_unknown_denied_and_signature_diagnostics_without_dispatch() {
        let calls = Cell::new(0);
        let mut builder = HostBuilder::new();
        builder
            .register(
                "door.unlock",
                vec![parameter("key", ScalarType::String)],
                ScalarType::None,
                "doors",
                |_, _| {
                    calls.set(calls.get() + 1);
                    Ok(ScalarValue::None)
                },
            )
            .unwrap();
        let host = builder.build();
        for (source, grants, kind, message) in [
            (
                "fn main() = missing()",
                &[][..],
                DiagnosticKind::UnknownOperation,
                "unknown operation",
            ),
            (
                "fn main() = door.unlock(key: \"test\")",
                &[][..],
                DiagnosticKind::CapabilityDenied,
                "capability denied",
            ),
            (
                "fn main() = door.unlock(wrong: \"test\")",
                &["doors"][..],
                DiagnosticKind::Compilation,
                "unknown named argument",
            ),
        ] {
            let error = host.compile(source, grants).err().unwrap();
            let diagnostic = compile(&error).remove(0);
            assert_eq!(diagnostic.kind, kind);
            assert_eq!(diagnostic.code, "LSC0001");
            assert!(diagnostic.message.contains(message), "{diagnostic:?}");
            assert!(!diagnostic.message.contains("std.print"));
            assert!(!diagnostic.message.contains("--allow"));
            assert_eq!(
                diagnostic.location(source).unwrap().start,
                SourcePosition {
                    line: 1,
                    column: 13
                }
            );
        }
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn cold_branch_and_unused_helper_errors_retain_the_original_call_span() {
        let host = HostBuilder::new().build();
        for source in [
            "fn main() = choose(when: true, then: 0, otherwise: missing())",
            "fn unused() = missing()\nfn main() = 0",
            "fn helper(n: integer) = missing()\nfn main() = helper(n: 1)",
        ] {
            let error = host.compile(source, &[]).err().unwrap();
            let diagnostic = compile(&error).remove(0);
            assert_eq!(
                diagnostic.kind,
                DiagnosticKind::UnknownOperation,
                "{diagnostic:?}"
            );
            let span = diagnostic.span.unwrap();
            assert_eq!(&source[span.start..span.end], "missing()", "{diagnostic:?}");
            assert!(diagnostic.location(source).is_some());
        }
    }

    #[test]
    fn helper_and_scalar_causes_are_available_without_changing_native_errors() {
        let host = HostBuilder::new().build();
        for (source, expected) in [
            (
                "fn helper(n: signed) = n\nfn main() = 0",
                "helper parameter type is not a closed scalar type",
            ),
            (
                "fn helper(n: integer) = add(left: n, right: true)\nfn main() = helper(n: 1)",
                "add does not accept Integer and Boolean",
            ),
            (
                "fn main() = add(left: true, right: 1)",
                "add does not accept Boolean and Integer",
            ),
        ] {
            let error = host.compile(source, &[]).err().unwrap();
            let previous = error.to_string();
            let diagnostic = compile(&error).remove(0);
            assert_eq!(diagnostic.kind, DiagnosticKind::Compilation);
            assert!(diagnostic.message.contains(expected), "{diagnostic:?}");
            assert_eq!(diagnostic.location(source).unwrap().start.line, 1);
            assert_eq!(error.to_string(), previous);
        }
    }

    #[test]
    fn source_limits_and_spanless_type_errors_do_not_invent_source_locations() {
        for error in [
            CompileError::SourceLimit,
            CompileError::Source(CatalogSourceError::Type(CallFlowTypeError::BranchTypes)),
        ] {
            let diagnostic = compile(&error).remove(0);
            assert_eq!(diagnostic.code, "LSC0001");
            assert_eq!(diagnostic.kind, DiagnosticKind::Compilation);
            assert!(diagnostic.span.is_none());
            assert!(diagnostic.location("fn main() = 0").is_none());
        }
    }

    fn evaluated(error: Error) -> Error {
        Error::Evaluation(Box::new(CalculationFailure::External(
            EffectStackFault::Control(EffectEvaluationFault::Native(error)),
        )))
    }

    #[test]
    fn nested_runtime_diagnostics_preserve_authority_and_uncertain_effect_classification() {
        for (error, kind, message) in [
            (
                evaluated(Error::Denied),
                DiagnosticKind::CapabilityDenied,
                "capability denied",
            ),
            (
                Error::Acceptance(Box::new(EffectSessionReplyError::Reply(
                    ReplyAcceptanceError::Authority(Error::Denied),
                ))),
                DiagnosticKind::CapabilityDenied,
                "capability denied",
            ),
            (
                evaluated(Error::Host(HostFailure::Uncertain(
                    "connection lost".into(),
                ))),
                DiagnosticKind::HostUncertain,
                "effects may be partial",
            ),
            (
                Error::Host(HostFailure::Cancelled),
                DiagnosticKind::Runtime,
                "cancelled",
            ),
        ] {
            let diagnostic = runtime(&error);
            assert_eq!(diagnostic.code, "LSR0001");
            assert_eq!(diagnostic.kind, kind);
            assert!(diagnostic.message.contains(message), "{diagnostic:?}");
            assert!(diagnostic.span.is_none());
            assert!(diagnostic.location("fn main() = 0").is_none());
            assert!(!diagnostic.message.contains("--allow"));
            assert!(!diagnostic.message.contains("output may be partial"));
        }
        let fuel = Error::Evaluation(Box::new(CalculationFailure::External(
            EffectStackFault::Control(EffectEvaluationFault::Pure(
                PureEvaluationFault::FuelExhausted,
            )),
        )));
        assert!(runtime(&fuel).message.contains("fuel exhausted"));
    }

    #[test]
    fn compilation_and_runtime_traversal_stop_at_the_depth_ceiling() {
        let mut source = CatalogSourceError::Mapping {
            span: Span { start: 0, end: 0 },
            error: Error::Denied,
        };
        for _ in 0..MAX_DEPTH {
            source = CatalogSourceError::Control(Box::new(ControlSourceError::Native {
                span: Span { start: 0, end: 0 },
                phase: ControlSourcePhase::Leaf,
                error: source,
            }));
        }
        let diagnostic = compile(&CompileError::Source(source)).remove(0);
        assert_eq!(diagnostic.message, "nested compilation failure");
        assert_eq!(diagnostic.kind, DiagnosticKind::Compilation);
        let mut error = Error::Denied;
        for _ in 0..MAX_DEPTH {
            error = evaluated(error);
        }
        assert_eq!(runtime(&error).message, "nested runtime failure");
        assert_eq!(runtime(&error).kind, DiagnosticKind::Runtime);
    }

    #[test]
    fn produced_messages_are_bounded_including_the_truncation_marker() {
        for length in [MAX_MESSAGE - 1, MAX_MESSAGE, MAX_MESSAGE + 1, 10_000] {
            let message = "🦀".repeat(length);
            let diagnostic = compile(&CompileError::Syntax(vec![SyntaxDiagnostic {
                code: "LSEtest".into(),
                message: message.clone(),
                span: Span { start: 0, end: 0 },
            }]))
            .remove(0);
            assert!(diagnostic.message.chars().count() <= MAX_MESSAGE);
            assert_eq!(diagnostic.message.ends_with("..."), length > MAX_MESSAGE);
            for error in [
                Error::Execution(message.clone()),
                Error::Host(HostFailure::Uncertain(message)),
            ] {
                assert!(runtime(&error).message.chars().count() <= MAX_MESSAGE);
            }
        }
    }
}
