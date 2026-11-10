//! Whitespace inside HTML `<pre>`, `<textarea>`, `<listing>` and `<plaintext>` is
//! rendered verbatim (CSS `white-space: pre`; textarea/plaintext are raw text), so an
//! edit to it is a semantic change, through the real HTML parser component. Elsewhere
//! HTML whitespace collapses and stays layout. Assessment case `web-html-pre_whitespace`.
use crate::*;
use serde_json::Value;

fn diff(old: &str, new: &str) -> Value {
    let wasm = std::env::var("INTENTUMDIFF_TEST_WASM_DIR").expect("staged parser components");
    let parser = std::path::Path::new(&wasm).join("html_parser.wasm");
    assert!(parser.is_file(), "html_parser.wasm missing: {}", parser.display());
    native_wasm_single_diff("html", parser.to_str().unwrap(), old, new, "doc.html", "doc.html", "{}", None)
        .expect("wasm diff")
}

#[test]
fn html_preformatted_whitespace_changes_are_semantic() {
    let cases = [
        // The assessment case: the amount of inter-word space inside <pre>.
        ("<pre>a b</pre>\n", "<pre>a  b</pre>\n"),
        // Leading/trailing spaces and line breaks inside <pre> are rendered.
        ("<pre> a</pre>\n", "<pre>a</pre>\n"),
        ("<pre>a\nb</pre>\n", "<pre>a b</pre>\n"),
        ("<pre>a\n\nb</pre>\n", "<pre>a\nb</pre>\n"),
        // Upper-case tag names and attributes do not change significance.
        ("<PRE class=\"c\">x  y</PRE>\n", "<PRE class=\"c\">x y</PRE>\n"),
        // Descendants of <pre> inherit preformatting, and mixed content is kept.
        ("<pre><b>a  b</b></pre>\n", "<pre><b>a b</b></pre>\n"),
        ("<pre>a  <b>x</b></pre>\n", "<pre>a <b>x</b></pre>\n"),
        ("<pre>x<br>  y</pre>\n", "<pre>x<br> y</pre>\n"),
        ("<div>\n  <pre>a  b</pre>\n</div>\n", "<div>\n  <pre>a b</pre>\n</div>\n"),
        // <textarea>, <listing> and <plaintext> content is whitespace-significant too.
        ("<textarea>a  b</textarea>\n", "<textarea>a b</textarea>\n"),
        ("<listing>a  b</listing>\n", "<listing>a b</listing>\n"),
        // A real text edit inside mixed <pre> content (previously no text was synthesised).
        ("<pre>hello <b>x</b></pre>\n", "<pre>bye <b>x</b></pre>\n"),
    ];
    for (old, new) in cases {
        for (a, b) in [(old, new), (new, old)] {
            let d = diff(a, b);
            assert_eq!(d["is_fallback"], false, "{a:?} -> {b:?}: {d}");
            assert_ne!(d["is_style_only"], true, "{a:?} -> {b:?} served as style-only: {d}");
            assert_eq!(d["has_semantic_changes"], true, "{a:?} -> {b:?}: {d}");
            assert!(!d["changes"].as_array().unwrap().is_empty(), "{a:?} -> {b:?}: no changes: {d}");
        }
    }
}

#[test]
fn html_collapsible_whitespace_stays_style_only() {
    let cases = [
        // Outside preformatted elements the amount of whitespace is layout.
        ("<p>a b</p>\n", "<p>a  b</p>\n"),
        ("<div>\n  <p>a b</p>\n</div>\n", "<div>\n    <p>a\n    b</p>\n</div>\n"),
        // HTML drops a single newline immediately after the <pre>/<textarea> start tag.
        ("<pre>\na b</pre>\n", "<pre>a b</pre>\n"),
        ("<textarea>\na</textarea>\n", "<textarea>a</textarea>\n"),
        // Re-indenting a <pre> element itself (outside its content) is layout.
        ("<div>\n<pre>a  b</pre>\n</div>\n", "<div>\n    <pre>a  b</pre>\n</div>\n"),
    ];
    for (old, new) in cases {
        for (a, b) in [(old, new), (new, old)] {
            let d = diff(a, b);
            assert_eq!(d["is_fallback"], false, "{a:?} -> {b:?}: {d}");
            assert_eq!(d["has_semantic_changes"], false, "{a:?} -> {b:?}: {d}");
        }
    }
}
