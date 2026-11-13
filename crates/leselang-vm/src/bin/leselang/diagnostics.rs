//! CLI presentation of the public, host-neutral diagnostic view.

use leselang_vm::embedding::{
    CompileError,
    diagnostics::{self as shared, Diagnostic as StructuredDiagnostic, DiagnosticKind},
};
#[cfg(test)]
use leselang_vm::{embedding::Error, native::syntax::Span};

const MAX_MESSAGE: usize = 512;
const EXCERPT_CHARS: usize = 120;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Format {
    #[default]
    Text,
    Json,
}

impl Format {
    pub fn compile(self, path: &str, source: &str, error: &CompileError) -> String {
        if self == Self::Text {
            return render_compile(path, source, error);
        }
        shared::compile(error)
            .iter()
            .map(|diagnostic| {
                let phase = if diagnostic.kind == DiagnosticKind::Syntax {
                    "syntax"
                } else {
                    "compile"
                };
                json_diagnostic(Some(path), source, phase, diagnostic, None)
            })
            .collect()
    }

    pub fn runtime(
        self,
        path: &str,
        diagnostic: &StructuredDiagnostic,
        fuel: Option<u64>,
    ) -> String {
        if self == Self::Json {
            return json_diagnostic(Some(path), "", "runtime", diagnostic, fuel);
        }
        let mut result = render_note(path, "", diagnostic);
        if let Some(fuel) = fuel {
            result.push_str(&format!("note: execution stopped with {fuel} fuel remaining; completed output is not rolled back\n"));
        }
        result
    }

    pub fn other(self, path: Option<&str>, phase: &str, code: &str, message: &str) -> String {
        let diagnostic = StructuredDiagnostic {
            code: code.into(),
            kind: DiagnosticKind::Runtime,
            message: message.into(),
            span: None,
        };
        if self == Self::Json {
            return json_record(path, "", phase, phase, &diagnostic, None);
        }
        let prefix = path
            .map(|path| format!("{}: ", display_path(path)))
            .unwrap_or_default();
        format!("{prefix}error: {}\n", bounded(message, MAX_MESSAGE))
    }
}

fn hint(diagnostic: &StructuredDiagnostic) -> Option<&'static str> {
    match diagnostic.kind {
        DiagnosticKind::CapabilityDenied => Some("std.print requires --allow stdio"),
        DiagnosticKind::UnknownOperation => Some("this CLI provides std.print(message: string)"),
        DiagnosticKind::HostUncertain => {
            Some("output may be partial; completed output is not rolled back")
        }
        _ => None,
    }
}

fn text_message(diagnostic: &StructuredDiagnostic) -> String {
    match hint(diagnostic) {
        Some(hint) => format!("{}; {hint}", bounded(&diagnostic.message, MAX_MESSAGE)),
        None => bounded(&diagnostic.message, MAX_MESSAGE),
    }
}

fn render_compile(path: &str, source: &str, error: &CompileError) -> String {
    shared::compile(error)
        .iter()
        .map(|diagnostic| render_note(path, source, diagnostic))
        .collect()
}

#[cfg(test)]
fn render_runtime(error: &Error) -> String {
    Format::Text.runtime("test.lese", &shared::runtime(error), None)
}

// JSON Lines is a diagnostic presentation contract, not a VM checkpoint or
// Effect wire format. No serde dependency is added to the executable/library.
fn json_diagnostic(
    path: Option<&str>,
    source: &str,
    phase: &str,
    diagnostic: &StructuredDiagnostic,
    fuel: Option<u64>,
) -> String {
    json_record(
        path,
        source,
        phase,
        diagnostic.kind.as_str(),
        diagnostic,
        fuel,
    )
}

fn json_record(
    path: Option<&str>,
    source: &str,
    phase: &str,
    kind: &str,
    diagnostic: &StructuredDiagnostic,
    fuel: Option<u64>,
) -> String {
    let location = diagnostic.location(source);
    let (span, range) = match (diagnostic.span, location) {
        (Some(span), Some(range)) => (
            format!("{{\"start\":{},\"end\":{}}}", span.start, span.end),
            format!(
                "{{\"start\":{{\"line\":{},\"column\":{}}},\"end\":{{\"line\":{},\"column\":{}}}}}",
                range.start.line, range.start.column, range.end.line, range.end.column
            ),
        ),
        _ => ("null".into(), "null".into()),
    };
    let path_truncated = path.is_some_and(|path| path.chars().count() > 256);
    let path = path
        .map(|path| json_string(&path.chars().take(256).collect::<String>()))
        .unwrap_or_else(|| "null".into());
    let help = hint(diagnostic)
        .map(json_string)
        .unwrap_or_else(|| "null".into());
    let fuel = fuel
        .map(|fuel| fuel.to_string())
        .unwrap_or_else(|| "null".into());
    format!(
        "{{\"schema_version\":1,\"severity\":\"error\",\"phase\":{},\"code\":{},\"kind\":{},\"message\":{},\"path\":{path},\"path_truncated\":{path_truncated},\"span\":{span},\"range\":{range},\"help\":{help},\"fuel_remaining\":{fuel}}}\n",
        json_string(phase),
        json_string(&diagnostic.code.chars().take(64).collect::<String>()),
        json_string(kind),
        json_string(
            &diagnostic
                .message
                .chars()
                .take(MAX_MESSAGE)
                .collect::<String>()
        )
    )
}

fn json_string(value: &str) -> String {
    use std::fmt::Write;
    let mut output = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            ch if ch.is_control()
                || matches!(ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}') =>
            {
                write!(output, "\\u{:04x}", ch as u32).expect("writing to String");
            }
            ch => output.push(ch),
        }
    }
    output.push('"');
    output
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

fn render_note(path: &str, source: &str, note: &StructuredDiagnostic) -> String {
    let path = display_path(path);
    let code = bounded(&note.code, 64);
    let message = text_message(note);
    let Some(location) = note.location(source) else {
        return format!("{path}: error[{code}]: {message}\n");
    };
    let span = note.span.expect("validated source location has a span");
    let prefix = &source[..span.start];
    let line = location.start.line;
    let line_start = prefix.rfind('\n').map_or(0, |index| index + 1);
    let line_end = source[span.start..]
        .find('\n')
        .map_or(source.len(), |index| span.start + index);
    let content = source[line_start..line_end]
        .strip_suffix('\r')
        .unwrap_or(&source[line_start..line_end]);
    let column = location.start.column - 1;
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
        embedding::{HostBuilder, HostFailure, ScalarType, ScalarValue, parameter},
        native::{
            core::CalculationFailure,
            hir::{
                catalog_source::CatalogSourceError, effect_evaluation::EffectEvaluationFault,
                effect_stack::EffectStackFault, pure_evaluation::PureEvaluationFault,
            },
            syntax::Diagnostic,
        },
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
    fn json_fields_are_bounded_escaped_and_invalid_locations_remain_absent() {
        let path = "字".repeat(1000);
        let diagnostic = StructuredDiagnostic {
            code: "code".repeat(100),
            kind: DiagnosticKind::Compilation,
            message: format!("\u{1b}\n\t\u{7f}\u{85}\u{202e}{}", "🦀".repeat(1000)),
            span: Some(Span { start: 1, end: 2 }),
        };
        let rendered = json_diagnostic(Some(&path), "é", "compile", &diagnostic, None);
        let record: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(record["path_truncated"], true);
        assert_eq!(record["path"].as_str().unwrap().chars().count(), 256);
        assert_eq!(record["code"].as_str().unwrap().chars().count(), 64);
        assert_eq!(
            record["message"].as_str().unwrap().chars().count(),
            MAX_MESSAGE
        );
        assert!(record["span"].is_null());
        assert!(record["range"].is_null());
        assert_eq!(rendered.lines().count(), 1);
        assert!(!rendered.contains('\u{1b}'));
        assert!(!rendered.contains('\u{202e}'));
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
