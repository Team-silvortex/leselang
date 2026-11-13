//! Source formatting that retains comments and literal spellings.

use crate::{Diagnostic, MAX_SOURCE_BYTES, Span, TokenKind, parse};

/// Format valid source without discarding comments or changing token spellings.
///
/// This is an additive source-oriented interface. [`crate::format`] keeps its
/// existing AST-oriented, comment-erasing behavior. This function parses the
/// original source before formatting; it does not trust a caller-mutated tree.
///
/// Function signatures are compact, nonempty call argument lists use two-space
/// indentation, and declarations are separated by a blank line. Existing commas
/// are retained, including optional trailing commas. Comments stay between the
/// same surrounding non-whitespace tokens, in their original order. A comment
/// on the same source line as the preceding token stays a trailing comment;
/// other comments occupy their own line.
///
/// Whitespace-token layout uses LF. Literal and comment token text is preserved
/// byte for byte, including trailing comment spaces and CR bytes that the lexer
/// includes in a comment before CRLF. No universal line-ending conversion is
/// performed inside these tokens. Successful output ends in LF and is
/// idempotent. The parser's input limit and the same output byte limit apply;
/// an error never returns a partially formatted source string.
pub fn format_source(source: &str) -> Result<String, Vec<Diagnostic>> {
    // parse retains its original source in a SyntaxTree, including on oversized
    // rejection. This source-only wrapper can refuse before making that copy.
    if source.len() > MAX_SOURCE_BYTES {
        return Err(vec![Diagnostic {
            code: "LSE0001".into(),
            message: format!("source exceeds {MAX_SOURCE_BYTES} bytes"),
            span: Span {
                start: 0,
                end: source.len(),
            },
        }]);
    }
    let tree = parse(source);
    if !tree.diagnostics.is_empty() {
        return Err(tree.diagnostics);
    }

    let mut declarations: Vec<_> = tree.function.iter().chain(&tree.helpers).collect();
    declarations.sort_by_key(|declaration| declaration.span.start);
    let error_span = declarations
        .first()
        .map_or(Span { start: 0, end: 0 }, |declaration| declaration.span);
    let mut ends = declarations.iter().map(|declaration| declaration.span.end);
    let tokens: Vec<_> = tree
        .tokens
        .iter()
        .filter(|token| token.kind != TokenKind::Whitespace)
        .collect();
    let mut output = Output::default();
    let mut frames = Vec::new();
    let mut previous_kind = None;
    let mut previous_end = None;
    let mut signature_pending = false;
    let mut declaration_end = None;
    let mut declaration_complete = false;
    let mut declaration_count = 0;
    let mut boundary_comments = false;

    let result = (|| {
        for (index, token) in tokens.iter().enumerate() {
            if token.kind == TokenKind::Eof {
                break;
            }
            // The tree was just parsed from this source, so these intervals are
            // valid UTF-8 boundaries and no external AST mutation is involved.
            let text = &source[token.span.start..token.span.end];
            if token.kind == TokenKind::Comment {
                let trailing =
                    previous_end.is_some_and(|end| !source[end..token.span.start].contains('\n'));
                if trailing {
                    output.space()?;
                } else if declaration_complete && !boundary_comments {
                    output.blank_line()?;
                    boundary_comments = true;
                } else {
                    output.newline()?;
                }
                output.indent(frames.len() * 2)?;
                output.push(text)?;
                output.newline()?;
                previous_end = Some(token.span.end);
                continue;
            }

            if token.kind == TokenKind::Fn {
                if declaration_count > 0 && !boundary_comments {
                    output.blank_line()?;
                }
                signature_pending = true;
                declaration_end = ends.next();
                declaration_complete = false;
                declaration_count += 1;
                boundary_comments = false;
            }

            let closing = token.kind == TokenKind::RightParen;
            let close_frame = closing.then(|| frames.pop()).flatten();
            if close_frame.is_some_and(|frame: Frame| frame.multiline) {
                output.newline()?;
            } else if !closing {
                if previous_kind == Some(TokenKind::LeftParen)
                    && frames.last().is_some_and(|frame| frame.multiline)
                {
                    output.newline()?;
                } else if previous_kind == Some(TokenKind::Comma) {
                    if frames.last().is_some_and(|frame| frame.multiline) {
                        output.newline()?;
                    } else {
                        output.space()?;
                    }
                }
            }

            if token.kind == TokenKind::Equal
                || matches!(
                    previous_kind,
                    Some(TokenKind::Fn | TokenKind::Colon | TokenKind::Equal)
                )
            {
                output.space()?;
            }
            output.indent(frames.len() * 2)?;
            output.push(text)?;

            if token.kind == TokenKind::LeftParen {
                // Comments count as content: even an empty call needs a closing
                // line when a comment occupies the space inside its parentheses.
                let multiline = !signature_pending
                    && tokens
                        .get(index + 1)
                        .is_some_and(|next| next.kind != TokenKind::RightParen);
                frames.push(Frame { multiline });
                signature_pending = false;
            }
            declaration_complete |= declaration_end == Some(token.span.end);
            previous_kind = Some(token.kind);
            previous_end = Some(token.span.end);
        }
        output.newline()
    })();

    match result {
        Ok(()) => Ok(output.text),
        Err(()) => Err(vec![Diagnostic {
            code: "LSE2002".into(),
            message: format!("formatted source exceeds {MAX_SOURCE_BYTES} bytes"),
            span: error_span,
        }]),
    }
}

#[derive(Clone, Copy)]
struct Frame {
    multiline: bool,
}

#[derive(Default)]
struct Output {
    text: String,
}

impl Output {
    fn push(&mut self, text: &str) -> Result<(), ()> {
        if text.len() > MAX_SOURCE_BYTES.saturating_sub(self.text.len()) {
            return Err(());
        }
        self.text.push_str(text);
        Ok(())
    }

    fn line_start(&self) -> bool {
        self.text.is_empty() || self.text.ends_with('\n')
    }

    fn indent(&mut self, spaces: usize) -> Result<(), ()> {
        if self.line_start() {
            self.push(&" ".repeat(spaces))?;
        }
        Ok(())
    }

    fn space(&mut self) -> Result<(), ()> {
        if !self.line_start() && !self.text.ends_with(' ') {
            self.push(" ")?;
        }
        Ok(())
    }

    fn newline(&mut self) -> Result<(), ()> {
        if !self.line_start() {
            self.push("\n")?;
        }
        Ok(())
    }

    fn blank_line(&mut self) -> Result<(), ()> {
        self.newline()?;
        if !self.text.is_empty() && !self.text.ends_with("\n\n") {
            self.push("\n")?;
        }
        Ok(())
    }
}
