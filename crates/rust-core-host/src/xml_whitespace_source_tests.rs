//! Issue #247: significant XML text whitespace must survive path-profile
//! normalization, through the real XML parser component. Whitespace inside
//! `xml:space="preserve"` scopes is data; in mixed content, inter-word whitespace
//! present vs absent is data; indentation between child elements is layout.
use crate::*;
use serde_json::Value;

fn diff(old: &str, new: &str) -> Value {
    let wasm = std::env::var("INTENTUMDIFF_TEST_WASM_DIR").expect("staged parser components");
    let parser = std::path::Path::new(&wasm).join("xml_parser.wasm");
    assert!(parser.is_file(), "xml_parser.wasm missing: {}", parser.display());
    native_wasm_single_diff("xml", parser.to_str().unwrap(), old, new, "doc.xml", "doc.xml", "{}", None)
        .expect("wasm diff")
}

#[test]
fn xml_significant_text_whitespace_changes_are_semantic() {
    let cases = [
        // Leading space in a text value is character data.
        ("<r>\n  <v> x</v>\n</r>\n", "<r>\n  <v>x</v>\n</r>\n"),
        // Mixed content: the space between the text and the child element.
        ("<r>\n  <p>a <b>b</b></p>\n</r>\n", "<r>\n  <p>a<b>b</b></p>\n</r>\n"),
        // Mixed content: a whitespace-only run between two inline children.
        ("<r>\n  <p><b>a</b> <i>b</i></p>\n</r>\n", "<r>\n  <p><b>a</b><i>b</i></p>\n</r>\n"),
        // Mixed content: a real word change next to a child element.
        ("<r>\n  <p>hello <b>b</b></p>\n</r>\n", "<r>\n  <p>bye <b>b</b></p>\n</r>\n"),
        // xml:space="preserve" on the element itself: exact whitespace is data.
        ("<r>\n  <v xml:space=\"preserve\">a  b</v>\n</r>\n", "<r>\n  <v xml:space=\"preserve\">a b</v>\n</r>\n"),
        // xml:space="preserve" inherited by descendants.
        ("<r xml:space=\"preserve\"><v>a  b</v></r>\n", "<r xml:space=\"preserve\"><v>a b</v></r>\n"),
        ("<r xml:space=\"preserve\"><s><v>  x</v></s></r>\n", "<r xml:space=\"preserve\"><s><v> x</v></s></r>\n"),
        // ... including CDATA content and line breaks inside the value.
        ("<r xml:space=\"preserve\"><v><![CDATA[a  b]]></v></r>\n", "<r xml:space=\"preserve\"><v><![CDATA[a b]]></v></r>\n"),
        ("<r xml:space=\"preserve\"><v>a\nb</v></r>\n", "<r xml:space=\"preserve\"><v>a b</v></r>\n"),
        // A no-break space is not XML whitespace: it never collapses.
        ("<r>\n  <v>a\u{a0}b</v>\n</r>\n", "<r>\n  <v>a b</v>\n</r>\n"),
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
fn xml_formatting_only_whitespace_stays_style_only() {
    let cases = [
        // Indentation and blank lines between child elements (element-only content).
        ("<r>\n  <a>1</a>\n  <b>2</b>\n</r>\n", "<r>\n    <a>1</a>\n\n    <b>2</b>\n</r>\n"),
        // Re-indenting a multi-line text value keeps its boundary whitespace present.
        ("<r>\n  <v>\n    alpha\n  </v>\n</r>\n", "<r>\n    <v>\n        alpha\n    </v>\n</r>\n"),
        // Default space handling: the amount of inter-word whitespace is layout.
        ("<r>\n  <v>a  b</v>\n</r>\n", "<r>\n  <v>a b</v>\n</r>\n"),
        ("<r>\n  <p>a <b>b</b>\n    c</p>\n</r>\n", "<r>\n  <p>a   <b>b</b> c</p>\n</r>\n"),
        // xml:space="default" resets an inherited preserve scope.
        (
            "<r xml:space=\"preserve\"><s xml:space=\"default\"><v>a  b</v></s></r>\n",
            "<r xml:space=\"preserve\"><s xml:space=\"default\"><v>a b</v></s></r>\n",
        ),
        // Whitespace-only content of a leaf element without preserve is layout.
        ("<r>\n  <v>\n  </v>\n</r>\n", "<r>\n  <v>  </v>\n</r>\n"),
    ];
    for (old, new) in cases {
        for (a, b) in [(old, new), (new, old)] {
            let d = diff(a, b);
            assert_eq!(d["is_fallback"], false, "{a:?} -> {b:?}: {d}");
            assert_eq!(d["is_style_only"], true, "{a:?} -> {b:?}: layout-only edit lost style-only: {d}");
            assert_eq!(d["has_semantic_changes"], false, "{a:?} -> {b:?}: {d}");
        }
    }
}
