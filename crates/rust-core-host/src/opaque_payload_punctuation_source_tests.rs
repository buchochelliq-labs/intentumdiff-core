//! Insignificant punctuation in a parent's own source gaps is not parser payload.
//! #284 retains source evidence when a node's hash moves while its exported
//! projection does not; a child edit moves the parent's hash too, and the gap
//! tokens must not then differ by a JS statement-final `;` or a trailing `,`
//! the grammar's own hash already treats as non-semantic. Real parsers via the
//! native live review path.
use serde_json::{json, Value};

fn review(path: &str, old: &str, new: &str) -> Value {
    let wasm = std::env::var("INTENTUMDIFF_TEST_WASM_DIR").expect("staged parser components");
    let raw = crate::live_server::live_diff_contents_impl(
        env!("CARGO_MANIFEST_DIR"), path, old, new, "{}", &wasm,
    )
    .expect("native review");
    let response: Value = serde_json::from_str(&raw).expect("valid native response");
    let diff = response
        .get("diff")
        .unwrap_or_else(|| panic!("review unexpectedly declined: {response:#}"))
        .clone();
    assert_eq!(diff["is_fallback"], false, "{diff:#}");
    assert_eq!(diff["parse_errors"], json!([]), "{diff:#}");
    diff
}

fn payload_changes(diff: &Value) -> Vec<Value> {
    diff["changes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| {
            c["description"]
                .as_str()
                .is_some_and(|d| d.starts_with("Parser payload changed"))
        })
        .cloned()
        .collect()
}

#[test]
fn js_statement_semicolon_beside_a_value_edit_is_not_payload() {
    for (old, new) in [
        ("function f() {\n  return 1\n}\n", "function f() {\n  return 2;\n}\n"),
        ("function f() {\n  return 1;\n}\n", "function f() {\n  return 2\n}\n"),
        ("let x = 1\n", "let x = 2;\n"),
    ] {
        let diff = review("code.js", old, new);
        assert_eq!(diff["has_semantic_changes"], true, "{old:?} -> {new:?}: {diff:#}");
        assert!(
            payload_changes(&diff).is_empty(),
            "{old:?} -> {new:?}: `;` reported as parser payload: {diff:#}"
        );
        let changes = diff["changes"].as_array().unwrap();
        assert_eq!(changes.len(), 1, "{old:?} -> {new:?}: {diff:#}");
        assert_eq!(changes[0]["change_type"], "MODIFICATION", "{diff:#}");
    }
}

#[test]
fn js_statement_semicolon_alone_stays_style_only() {
    let diff = review("code.js", "function f() {\n  return 1\n}\n", "function f() {\n  return 1;\n}\n");
    assert_eq!(diff["is_style_only"], true, "{diff:#}");
    assert_eq!(diff["has_semantic_changes"], false, "{diff:#}");
}

#[test]
fn python_trailing_comma_beside_an_argument_edit_is_not_payload() {
    for (old, new) in [
        ("g(r\"a\", p)\n", "g(\n    r'a',\n    p,\n)\n"),
        ("g(\"a\", p)\n", "g(\n    'a',\n    p,\n)\n"),
        ("g(\"a\", p)\n", "g(\"b\", p,)\n"),
        ("xs = [1, 2]\n", "xs = [1, 3,]\n"),
    ] {
        for (a, b) in [(old, new), (new, old)] {
            let diff = review("code.py", a, b);
            assert!(
                payload_changes(&diff).is_empty(),
                "{a:?} -> {b:?}: trailing `,` reported as parser payload: {diff:#}"
            );
        }
    }
}

#[test]
fn python_one_element_tuple_comma_remains_semantic() {
    // `(x,)` is a tuple and `(x)` a parenthesised expression: never style.
    let diff = review("code.py", "v = (x,)\n", "v = (x)\n");
    assert_eq!(diff["has_semantic_changes"], true, "{diff:#}");
    assert_ne!(diff["is_style_only"], true, "{diff:#}");
}

#[test]
fn operator_change_beside_an_equivalent_operand_keeps_semantic_evidence() {
    // #284's purpose: an operator edit next to a literal-equivalent operand
    // must not be discharged, with or without the punctuation edits above.
    for (path, old, new) in [
        ("code.js", "function f(a) {\n  return a + 1\n}\n", "function f(a) {\n  return a - 0x1;\n}\n"),
        ("code.js", "function f(a) {\n  return a + 1;\n}\n", "function f(a) {\n  return a - 0x1\n}\n"),
        ("code.py", "v = g(a + 1, b)\n", "v = g(\n    a - 0x1,\n    b,\n)\n"),
    ] {
        let diff = review(path, old, new);
        assert_eq!(diff["has_semantic_changes"], true, "{old:?} -> {new:?}: {diff:#}");
        assert_ne!(diff["is_style_only"], true, "{old:?} -> {new:?}: {diff:#}");
        assert!(!diff["changes"].as_array().unwrap().is_empty(), "{old:?} -> {new:?}: {diff:#}");
    }
}

#[test]
fn punctuation_normalisation_keeps_semantic_commas() {
    use crate::SemanticNode;
    let node = |node_type: &str, children: usize| -> SemanticNode {
        let child = json!({
            "id": "0.0", "node_type": "identifier", "label": "x",
            "position": {"start_line":0,"start_col":0,"end_line":0,"end_col":0},
            "structural_hash": "h"
        });
        serde_json::from_value(json!({
            "id": "0", "node_type": node_type, "label": node_type,
            "children": vec![child; children],
            "position": {"start_line":0,"start_col":0,"end_line":0,"end_col":0},
            "structural_hash": "h"
        }))
        .unwrap()
    };
    let last = |gap: &str, node: &SemanticNode, language: &str| {
        let mut tokens = super::tokens(gap, language).unwrap();
        super::drop_insignificant_punctuation(&mut tokens, node, language);
        tokens.join(" ")
    };
    // Trailing separators after the elements are layout.
    assert_eq!(last(",\n)", &node("argument_list", 2), "python"), "token:)");
    assert_eq!(last(",]", &node("list", 1), "python"), "token:]");
    assert_eq!(last(",}", &node("object", 1), "javascript"), "token:}");
    // `(x,)` is a one-element tuple; `x[1,]` subscripts by a tuple.
    assert_eq!(last(",)", &node("tuple", 1), "python"), "token:, token:)");
    assert_eq!(last(",]", &node("subscript", 2), "python"), "token:, token:]");
    // An elision hole and a bare tuple comma are elements, not separators.
    assert_eq!(last(",,]", &node("array", 1), "javascript"), "token:, token:, token:]");
    assert_eq!(last(",", &node("expression_list", 1), "python"), "token:,");
    // No element precedes: nothing to separate.
    assert_eq!(last(",]", &node("list", 0), "python"), "token:, token:]");
    // Only a JS/TS statement-final `;` is dropped; other languages keep theirs.
    assert_eq!(last(";", &node("return_statement", 1), "javascript"), "");
    assert_eq!(last(";", &node("return_statement", 1), "tsx"), "");
    assert_eq!(last(";", &node("return_statement", 1), "java"), "token:;");
}
