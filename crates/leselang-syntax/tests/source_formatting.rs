use leselang_syntax::{MAX_SOURCE_BYTES, TokenKind, format, format_source, parse};

fn token_texts(source: &str) -> Vec<(TokenKind, String)> {
    let tree = parse(source);
    assert!(
        tree.diagnostics.is_empty(),
        "{source}\n{:?}",
        tree.diagnostics
    );
    tree.tokens
        .iter()
        .filter(|token| !matches!(token.kind, TokenKind::Whitespace | TokenKind::Eof))
        .map(|token| (token.kind, tree.token_text(token).unwrap().to_owned()))
        .collect()
}

fn check(source: &str) -> String {
    let result = format_source(source).unwrap_or_else(|errors| panic!("{source}\n{errors:?}"));
    assert_eq!(token_texts(&result), token_texts(source), "{source}");
    assert_eq!(format_source(&result).unwrap(), result, "{source}");
    assert!(result.ends_with('\n'));
    result
}

#[test]
fn source_formatter_retains_comments_with_a_deterministic_layout() {
    let source = "// inventory\nfn main ( ) = runtime.list( environment: \"production\", // local\n cluster: none, role: \"edge\", ) // done\n// trailing";
    let expected = "// inventory\nfn main() = runtime.list(\n  environment: \"production\", // local\n  cluster: none,\n  role: \"edge\",\n) // done\n\n// trailing\n";
    assert_eq!(check(source), expected);
    // The earlier AST formatter remains available with its original contract.
    assert_eq!(
        format(&parse(source)).unwrap(),
        "fn main() = runtime.list(\n  environment: \"production\",\n  cluster: none,\n  role: \"edge\",\n)\n"
    );
}

#[test]
fn comments_in_every_token_gap_keep_their_exact_anchor_and_are_idempotent() {
    let original = "fn helper(n: integer, flag: boolean) = gui.window.read(value: n, mode: flag)\nfn main() = helper(n: 42, flag: true)";
    let tree = parse(original);
    let mut boundaries = vec![0];
    boundaries.extend(
        tree.tokens
            .iter()
            .filter(|token| !matches!(token.kind, TokenKind::Whitespace | TokenKind::Eof))
            .map(|token| token.span.end),
    );
    for (index, boundary) in boundaries.into_iter().enumerate() {
        for prefix in [" ", "\n", "\r\n"] {
            let source = format!(
                "{}{prefix}// gap {index} 中文 🦀  \n{}",
                &original[..boundary],
                &original[boundary..]
            );
            check(&source);
        }
    }
}

#[test]
fn comments_literals_and_crlf_keep_their_raw_token_bytes() {
    for source in [
        "// first\r\nfn main() = \"https://example.test/ // literal\\n\" // last\r\n",
        "fn main() = \"literal\r\nnew line\" // spaces  \r\n",
        "fn main() = gui // namespace\r\n . window // component\n . read(// inside\r\n) // end",
        "// repeated carriage returns\r\r\nfn main() = 0 // EOF carriage return\r",
        "//\r\n//\r\nfn main() = false\r\n// blank\r\r\r\n",
        "fn main() = strings(first: \"// quote\\\" and \\\\ slash\", second: \"你好\")",
    ] {
        check(source);
    }
}

#[test]
fn declaration_order_and_optional_commas_are_retained() {
    for source in [
        "fn first(a: integer,) = a\n// second\nfn main() = first(a: 1,)",
        "fn first(a: integer) = a // first\n\n// main\nfn main()=first(a:1)",
        "fn first() = 1 fn second() = 2 fn main() = 3",
        "fn main() = call()",
        "fn main() = call(// trailing opener\n)",
        "fn main() = call(\n// standalone inner\n)",
        "fn helper(// parameter\n a // label\n : // type\n integer // close\n) // equal\n = a\nfn main() = helper(a: 1 // comma\n ,)",
    ] {
        check(source);
    }
}

#[test]
fn invalid_source_returns_the_original_parser_diagnostics() {
    for source in [
        "",
        "// comment only",
        "fn main() = @ // bad",
        "fn main() = call(value:)",
        "fn main() = \"unterminated",
        "fn main() = gui..find()",
        "fn main() = 0001",
    ] {
        assert_eq!(
            format_source(source).unwrap_err(),
            parse(source).diagnostics
        );
    }
    let oversized = format!("fn main() = 0\n//{}", "x".repeat(MAX_SOURCE_BYTES));
    assert_eq!(
        format_source(&oversized).unwrap_err(),
        parse(&oversized).diagnostics
    );
}

#[test]
fn output_growth_is_bounded_and_reports_an_original_source_span() {
    let source = format!("fn main()=strings({})", "x:\"\",".repeat(40_000));
    assert!(source.len() < MAX_SOURCE_BYTES);
    let tree = parse(&source);
    assert!(tree.diagnostics.is_empty());
    let errors = format_source(&source).unwrap_err();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "LSE2002");
    assert_eq!(errors[0].span, tree.function.unwrap().span);

    // Exact input limits also apply when adding just the final LF would exceed
    // the output limit. Comment text itself is never silently truncated.
    let prefix = "fn main() = 0 //";
    let exact = format!("{prefix}{}", "x".repeat(MAX_SOURCE_BYTES - prefix.len()));
    assert_eq!(exact.len(), MAX_SOURCE_BYTES);
    assert_eq!(format_source(&exact).unwrap_err()[0].code, "LSE2002");
}

#[test]
fn source_formatter_uses_source_as_authority_instead_of_a_mutated_tree() {
    let source = "// retain\nfn main() = 42";
    let mut tree = parse(source);
    tree.function.as_mut().unwrap().name = "changed".into();
    tree.tokens.clear();
    // This API does not accept the mutable tree or serialize a forged token
    // stream; it parses the supplied source itself.
    assert_eq!(
        format_source(tree.source()).unwrap(),
        "// retain\nfn main() = 42\n"
    );
}
