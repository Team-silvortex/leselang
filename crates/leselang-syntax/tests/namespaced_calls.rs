use leselang_syntax::{Expression, Span, SyntaxTree, format, parse};

#[test]
fn multi_segment_calls_preserve_identity_spans_and_roundtrips() {
    for source in [
        "fn main() = gui.window.find()",
        "// 窗口\r\nfn main() = gui.window.controls.observe(target: \"名称\")",
        "fn main() = gui // namespace\n . window . find()",
        "fn main() = gui.window.apply(value: gui.window.read())",
    ] {
        let tree = parse(source);
        assert!(tree.diagnostics.is_empty(), "{:?}", tree.diagnostics);
        assert_eq!(tree.reconstruct().as_deref(), Some(source));
        let Expression::Call { callee, span, .. } = &tree.function.as_ref().unwrap().body else {
            panic!("expected a call");
        };
        assert!(callee.starts_with("gui.window."), "{callee}");
        assert_eq!(span.start, source.find("gui").unwrap());
        assert_eq!(span.end, source.len());
        assert!(source.is_char_boundary(span.start));
        assert!(source.is_char_boundary(span.end));
        assert_eq!(
            serde_json::from_slice::<SyntaxTree>(&serde_json::to_vec(&tree).unwrap()).unwrap(),
            tree
        );
        let canonical = format(&tree).unwrap();
        let reparsed = parse(&canonical);
        assert!(reparsed.diagnostics.is_empty());
        assert_eq!(format(&reparsed).unwrap(), canonical);
        let Expression::Call { callee: again, .. } = &reparsed.function.unwrap().body else {
            panic!("expected a formatted call");
        };
        assert_eq!(again, callee);
    }
}

#[test]
fn incomplete_namespaces_report_the_unexpected_token_without_partial_success() {
    for (source, code, offending) in [
        ("fn main() = .gui.window.find()", "LSE1109", "."),
        ("fn main() = gui..find()", "LSE1103", "."),
        ("fn main() = gui.window..find()", "LSE1103", "."),
        ("fn main() = gui.window.()", "LSE1103", "("),
        ("fn main() = gui.window.", "LSE1103", ""),
        ("fn main() = gui.window.find", "LSE1104", ""),
        ("fn main() = gui.window.fn()", "LSE1103", "fn"),
        ("fn main() = gui.window.none()", "LSE1103", "none"),
    ] {
        let tree = parse(source);
        let diagnostic = tree
            .diagnostics
            .iter()
            .find(|error| error.code == code)
            .unwrap_or_else(|| panic!("{source}: {:?}", tree.diagnostics));
        let Span { start, end } = diagnostic.span;
        assert_eq!(&source[start..end], offending, "{source}");
        assert!(format(&tree).is_err());
        assert_eq!(tree.reconstruct().as_deref(), Some(source));
    }
}

#[test]
fn namespace_depth_does_not_inherit_the_embedding_name_limit() {
    // Native syntax accepts the source within its own budget. Individual hosts
    // still decide their catalog's name limits at registration or compilation.
    let name = std::iter::repeat_n("namespace", 32)
        .chain(std::iter::once("operation"))
        .collect::<Vec<_>>()
        .join(".");
    assert!(name.len() > 128);
    let source = format!("fn main() = {name}()");
    let tree = parse(&source);
    assert!(tree.diagnostics.is_empty(), "{:?}", tree.diagnostics);
    let Expression::Call { callee, .. } = &tree.function.as_ref().unwrap().body else {
        panic!("expected a call");
    };
    assert_eq!(callee, &name);
    assert_eq!(format(&tree).unwrap(), source + "\n");
}
