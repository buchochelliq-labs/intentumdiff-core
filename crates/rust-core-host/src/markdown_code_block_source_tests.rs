//! Wasm-route regressions for issue #274: equivalent CommonMark code-block forms. The
//! markdown component emits an indented block as `indented_code_block` labelled with its
//! raw source and a fenced block as `fenced_code_block` with no content at all, so a form
//! change (indented <-> fenced, ``` <-> ~~~, fence width) was a modification while a real
//! edit to fenced code was served as style-only. Core derives the rendered code content and
//! info string from source (CommonMark 0.31.2 §4.4, §4.5).
use crate::*;
use serde_json::Value;

fn md(old: &str, new: &str) -> Value {
    let wasm = std::env::var("INTENTUMDIFF_TEST_WASM_DIR").expect("staged parser components");
    let parser = std::path::Path::new(&wasm).join("markdown_parser.wasm");
    assert!(parser.is_file(), "parser missing: {}", parser.display());
    native_wasm_single_diff("markdown", parser.to_str().unwrap(), old, new, "doc.md", "doc.md", "{}", None)
        .expect("wasm diff")
}

#[test]
fn equivalent_code_block_forms_are_not_semantic_changes() {
    let cases = [
        // Assessment case web-markdown-external-code-block-form (spec examples 18/19).
        ("    \\[\\]\n", "~~~\n\\[\\]\n~~~\n"),
        // Fence character and fence width.
        ("```\na\n```\n", "~~~\na\n~~~\n"),
        ("```\na\n```\n", "````\na\n````\n"),
        ("```rust\na\n```\n", "~~~rust\na\n~~~\n"),
        // Indented vs fenced, including interior blank lines and extra indentation.
        ("    a\n\n    b\n", "```\na\n\nb\n```\n"),
        ("      a\n", "```\n  a\n```\n"),
        ("\tcode\n", "```\ncode\n```\n"),
        // An unclosed fence runs to the end of the document.
        ("    a\n", "```\na\n"),
        // Up to the opening fence's indentation is removed from content lines.
        ("  ```\n  a\n   b\n  ```\n", "```\na\n b\n```\n"),
        // Info-string surrounding whitespace is not part of the info string.
        ("``` rust\na\n```\n", "```rust\na\n```\n"),
        // Inside a heading section and inside a list item.
        ("# T\n\n    x\n", "# T\n\n```\nx\n```\n"),
        ("- x\n\n      code\n", "- x\n\n  ```\n  code\n  ```\n"),
    ];
    for (old, new) in cases {
        for (a, b) in [(old, new), (new, old)] {
            let d = md(a, b);
            assert_eq!(d["is_fallback"], false, "{a:?} -> {b:?}: {d}");
            assert_eq!(d["has_semantic_changes"], false, "{a:?} -> {b:?}: {d}");
            assert!(d["changes"].as_array().unwrap().is_empty(), "{a:?} -> {b:?}: unexpected changes: {d}");
        }
    }
}

#[test]
fn code_content_and_info_string_edits_stay_semantic() {
    let cases = [
        // Real edits to fenced code (previously served as style-only).
        ("```\na\n```\n", "```\nb\n```\n"),
        ("~~~\nlet x = 1;\n~~~\n", "~~~\nlet x = 2;\n~~~\n"),
        // Whitespace inside code is rendered verbatim in <pre><code>.
        ("```\na  b\n```\n", "```\na b\n```\n"),
        ("```\na\nb\n```\n", "```\na\n\nb\n```\n"),
        ("    a\n", "     a\n"),
        ("```\n  a\n```\n", "    a\n"),
        // Content edits across forms.
        ("    a\n", "    b\n"),
        ("    a\n", "```\nb\n```\n"),
        // The info string (language) is semantic, in either form.
        ("```\na\n```\n", "```rust\na\n```\n"),
        ("```python\na\n```\n", "```rust\na\n```\n"),
        ("    a\n", "```python\na\n```\n"),
        // An empty block vs a block holding one blank line render differently.
        ("```\n```\n", "```\n\n```\n"),
    ];
    for (old, new) in cases {
        for (a, b) in [(old, new), (new, old)] {
            let d = md(a, b);
            assert_eq!(d["is_fallback"], false, "{a:?} -> {b:?}: {d}");
            assert_ne!(d["is_style_only"], true, "{a:?} -> {b:?}: served as style-only: {d}");
            assert_eq!(d["has_semantic_changes"], true, "{a:?} -> {b:?}: {d}");
            assert!(!d["changes"].as_array().unwrap().is_empty(), "{a:?} -> {b:?}: no changes: {d}");
        }
    }
}
