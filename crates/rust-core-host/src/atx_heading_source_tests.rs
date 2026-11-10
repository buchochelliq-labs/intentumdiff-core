//! Wasm-route regressions for issue #275: Markdown/MDX ATX headings. The parsers emit the
//! heading text as the `atx_heading` label without its level, and keep any optional closing
//! `#` sequence in that label. Core derives both facts from the heading's source span.
use crate::*;
use serde_json::Value;

fn diff(parser: &str, language: &str, filename: &str, old: &str, new: &str) -> Value {
    let wasm = std::env::var("INTENTUMDIFF_TEST_WASM_DIR").expect("staged parser components");
    let parser = std::path::Path::new(&wasm).join(parser);
    assert!(parser.is_file(), "parser missing: {}", parser.display());
    native_wasm_single_diff(language, parser.to_str().unwrap(), old, new, filename, filename, "{}", None)
        .expect("wasm diff")
}

fn md(old: &str, new: &str) -> Value {
    diff("markdown_parser.wasm", "markdown", "doc.md", old, new)
}

fn mdx(old: &str, new: &str) -> Value {
    diff("mdx_parser.wasm", "mdx", "doc.mdx", old, new)
}

fn assert_semantic(d: &Value, what: &str) {
    assert_eq!(d["is_fallback"], false, "{what}: {d}");
    assert_ne!(d["is_style_only"], true, "{what}: served as style-only: {d}");
    assert_eq!(d["has_semantic_changes"], true, "{what}: {d}");
    assert!(!d["changes"].as_array().unwrap().is_empty(), "{what}: no changes: {d}");
}

fn assert_no_semantic(d: &Value, what: &str) {
    assert_eq!(d["is_fallback"], false, "{what}: {d}");
    assert_eq!(d["has_semantic_changes"], false, "{what}: {d}");
    assert!(d["changes"].as_array().unwrap().is_empty(), "{what}: unexpected changes: {d}");
}

#[test]
fn atx_heading_level_change_is_semantic_not_style() {
    for (old, new) in [("# Guide\n", "## Guide\n"), ("#\tFoo\n", "##\tFoo\n")] {
        for (a, b) in [(old, new), (new, old)] {
            assert_semantic(&md(a, b), &format!("markdown {a:?} -> {b:?}"));
        }
    }
}

#[test]
fn mdx_atx_heading_level_change_is_semantic_not_style() {
    for (a, b) in [("# hi!\n", "## hi!\n"), ("## hi!\n", "# hi!\n")] {
        assert_semantic(&mdx(a, b), &format!("mdx {a:?} -> {b:?}"));
    }
}

#[test]
fn atx_heading_optional_closing_sequence_is_not_a_rename() {
    for (a, b) in [
        ("# Guide\n", "# Guide #\n"),
        ("# Guide #\n", "# Guide\n"),
        ("## Guide\n", "## Guide ###   \n"),
        ("# Guide\n", "# Guide\t##\n"),
    ] {
        assert_no_semantic(&md(a, b), &format!("markdown {a:?} -> {b:?}"));
    }
    for (a, b) in [("# hi!", "# hi! #"), ("# hi! #", "# hi!")] {
        assert_no_semantic(&mdx(a, b), &format!("mdx {a:?} -> {b:?}"));
    }
}

#[test]
fn atx_heading_text_changes_stay_semantic() {
    for (a, b) in [
        ("# Guide\n", "# Manual\n"),
        // No space before the hashes: `#` is heading content per CommonMark.
        ("# foo#\n", "# foo\n"),
        // Escaped closing hash is literal heading content.
        ("# foo\n", "# foo \\#\n"),
        // Closing sequence removal combined with a real text edit.
        ("# Guide #\n", "# Manual\n"),
    ] {
        assert_semantic(&md(a, b), &format!("markdown {a:?} -> {b:?}"));
    }
    assert_semantic(&mdx("# hi!", "# bye!"), "mdx text change");
}
