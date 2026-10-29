//! Source-judged Clojure parameter rename regression (#192).
//! This intentionally requires actual registry-verified parser Wasm; source/positions
//! are independently specified, not generated from an already-wrong engine result.
use crate::*;
use serde_json::{json, Value};

fn review(old: &str, new: &str) -> Value {
    let staged_dir = std::env::var("INTENTUMDIFF_TEST_WASM_DIR")
        .expect("full CI must provision trusted Clojure Wasm");
    let staged = std::path::Path::new(&staged_dir).join("clojure_parser.wasm");
    assert!(staged.is_file(), "registry-pinned Clojure component absent: {}", staged.display());
    let fixture = tempfile::tempdir().expect("isolated parser staging");
    std::fs::copy(staged, fixture.path().join("clojure_parser.wasm"))
        .expect("copy exact verified parser bytes");
    std::fs::write(
        fixture.path().join("parser_manifest.json"),
        json!({
            "schema_version": 1,
            "parsers": {
                "clojure": {
                    "plugin_id": "clojure",
                    "wasm": "clojure_parser.wasm",
                    "extensions": [".clj"]
                }
            },
            "extension_index": {".clj": "clojure"}
        }).to_string(),
    ).expect("test-only manifest for already-pinned component");
    let raw = crate::live_server::live_diff_contents_impl(
        env!("CARGO_MANIFEST_DIR"), "code.clj", old, new, "{}",
        fixture.path().to_str().expect("UTF-8 fixture path")
    ).expect("native review");
    let response: Value = serde_json::from_str(&raw).expect("valid native response");
    let diff = response.get("diff").unwrap_or_else(||
        panic!("Clojure review unexpectedly declined: {response:#}")
    ).clone();
    assert_eq!(diff["language"], "clojure");
    assert_eq!(diff["is_fallback"], false);
    assert_eq!(diff["is_style_only"], false);
    assert_eq!(diff["parse_errors"], json!([]));
    diff
}

#[test]
fn continued_add_form_parameter_renames_are_two_scoped_refactorings() {
    let old = "(defn add [a b]\n  (+ a b))\n";
    let new = "(defn add [x y]\n  (+ x y))\n";
    let diff = review(old, new);
    assert_eq!(diff["has_semantic_changes"], true);
    let changes = diff["changes"].as_array().expect("changes array");
    assert_eq!(changes.len(), 2, "no whole-form or body-reference add/delete churn: {diff:#}");
    for (old_label, new_label, old_col, new_col) in
        [("a", "x", 11, 11), ("b", "y", 13, 13)]
    {
        let matches: Vec<&Value> = changes.iter().filter(|change| {
            change["change_type"] == "REFACTORING"
                && change["refactoring_kind"] == "RENAME_VARIABLE"
                && change["old_node"]["label"] == old_label
                && change["new_node"]["label"] == new_label
        }).collect();
        assert_eq!(matches.len(), 1, "expected one source-scoped {old_label}->{new_label}: {diff:#}");
        let c = matches[0];
        for (side, col) in [("old_node", old_col), ("new_node", new_col)] {
            assert_eq!(c[side]["node_type"], "sym_lit");
            assert_eq!(c[side]["position"], json!({
                "start_line": 0, "start_col": col,
                "end_line": 0, "end_col": col + 1
            }));
        }
    }
    assert!(diff["change_groups"].as_array().unwrap().iter()
        .all(|g| g["kind"] != "IGNORED_STYLE"), "no false style-only reduction: {diff:#}");
}

#[test]
fn gallery_add_renames_remain_scoped_next_to_greet_arity_and_multiply_addition() {
    let old = "(defn greet [name]\\n  (println (str \\"Hello, \\" name)))\\n\\n(defn add [a b]\\n  (+ a b))\\n";
    let new = "(defn greet\\n  ([name] (greet name \\"!\\"))\\n  ([name suffix]\\n   (println (str \\"Hello, \\" name suffix))))\\n\\n(defn add [x y] (+ x y))\\n\\n(defn multiply [x y] (* x y))\\n";
    let diff = review(old, new);
    assert_eq!(diff["has_semantic_changes"], true, "{diff:#}");
    let changes = diff["changes"].as_array().expect("changes");
    for (before, after, col) in [("a", "x", 11), ("b", "y", 13)] {
        let matching: Vec<_> = changes.iter().filter(|c| {
            c["change_type"] == "REFACTORING"
                && c["refactoring_kind"] == "RENAME_VARIABLE"
                && c["old_node"]["label"] == before
                && c["new_node"]["label"] == after
                && c["old_node"]["position"] == json!({
                    "start_line": 3, "start_col": col, "end_line": 3, "end_col": col + 1
                })
                && c["new_node"]["position"] == json!({
                    "start_line": 5, "start_col": col, "end_line": 5, "end_col": col + 1
                })
        }).collect();
        assert_eq!(matching.len(), 1, "exact gallery scoped rename {before}->{after}: {diff:#}");
    }
    assert_eq!(changes.iter().filter(|c| c["change_type"] == "ADDITION"
        && c["new_node"]["label"] == "multiply").count(), 1,
        "new multiply must be one named callable addition: {diff:#}");
    assert!(!changes.iter().any(|c| {
        (c["change_type"] == "ADDITION" && c["new_node"]["label"] == "add")
            || (c["change_type"] == "DELETION" && c["old_node"]["label"] == "add")
    }), "continued add cannot be replaced by whole-function deletion/addition: {diff:#}");
    assert!(diff["change_groups"].as_array().unwrap().iter()
        .all(|g| g["kind"] != "IGNORED_STYLE"),
        "renames and new functions are not formatting: {diff:#}");
}

#[test]
fn swapping_parameters_is_not_a_source_proven_rename() {
    let old = "(defn add [a b]\n  (+ a b))\n";
    let new = "(defn add [b a]\n  (+ b a))\n";
    let diff = review(old, new);
    assert_eq!(diff["has_semantic_changes"], true, "{diff:#}");
    let changes = diff["changes"].as_array().expect("changes");
    let vector_edits: Vec<_> = changes.iter().filter(|c| {
        c["change_type"] == "MODIFICATION"
            && c["old_node"]["node_type"] == "vec_lit"
            && c["new_node"]["node_type"] == "vec_lit"
    }).collect();
    assert_eq!(vector_edits.len(), 1,
        "the ordered formal parameter vector must be a real modification: {diff:#}");
    for side in ["old_node", "new_node"] {
        assert_eq!(vector_edits[0][side]["position"], json!({
            "start_line": 0, "start_col": 10, "end_line": 0, "end_col": 15
        }), "exact byte span of [a b]/[b a]: {diff:#}");
    }
    assert!(changes.iter().all(|c| c["refactoring_kind"] != "RENAME_VARIABLE"),
        "a permutation is not a fresh variable rename: {diff:#}");
    assert!(diff["change_groups"].as_array().unwrap().iter()
        .all(|g| g["kind"] != "IGNORED_STYLE"),
        "parameter permutations must not be hidden as style: {diff:#}");
}

#[test]
fn renamed_parameters_with_changed_computation_keep_operator_edit() {
    let old = "(defn add [a b]\n  (+ a b))\n";
    let new = "(defn add [x y]\n  (- x y))\n";
    let diff = review(old, new);
    assert_eq!(diff["has_semantic_changes"], true);
    assert!(diff["changes"].as_array().unwrap().iter().any(|c| {
        c["refactoring_kind"] != "RENAME_VARIABLE"
    }), "a changed + to - calculation must not disappear as a mere rename: {diff:#}");
}
