//! Source-judged Elixir declaration-visibility regression (#271).
//! `def` / `defp` are declaration macros selecting public vs private visibility;
//! swapping them is an API change, never a variable rename. Driven through the
//! real registry-pinned Elixir parser Wasm via the native review path.
use serde_json::{json, Value};

fn review(old: &str, new: &str) -> Value {
    let staged_dir = std::env::var("INTENTUMDIFF_TEST_WASM_DIR")
        .expect("full CI must provision trusted Elixir Wasm");
    let staged = std::path::Path::new(&staged_dir).join("elixir_parser.wasm");
    assert!(staged.is_file(), "registry-pinned Elixir component absent: {}", staged.display());
    let fixture = tempfile::tempdir().expect("isolated parser staging");
    std::fs::copy(staged, fixture.path().join("elixir_parser.wasm"))
        .expect("copy exact verified parser bytes");
    std::fs::write(
        fixture.path().join("parser_manifest.json"),
        json!({
            "schema_version": 1,
            "parsers": {
                "elixir": {
                    "plugin_id": "elixir",
                    "wasm": "elixir_parser.wasm",
                    "extensions": [".ex"]
                }
            },
            "extension_index": {".ex": "elixir"}
        }).to_string(),
    ).expect("test-only manifest for already-pinned component");
    let raw = crate::live_server::live_diff_contents_impl(
        env!("CARGO_MANIFEST_DIR"), "code.ex", old, new, "{}",
        fixture.path().to_str().expect("UTF-8 fixture path")
    ).expect("native review");
    let response: Value = serde_json::from_str(&raw).expect("valid native response");
    let diff = response.get("diff").unwrap_or_else(||
        panic!("Elixir review unexpectedly declined: {response:#}")
    ).clone();
    assert_eq!(diff["language"], "elixir");
    assert_eq!(diff["is_fallback"], false);
    assert_eq!(diff["parse_errors"], json!([]));
    diff
}

#[test]
fn def_to_defp_is_a_visibility_change_not_a_variable_rename() {
    let old = "defmodule Answer do\n  def value(), do: 1\nend\n";
    let new = "defmodule Answer do\n  defp value(), do: 1\nend\n";
    let diff = review(old, new);
    assert_eq!(diff["has_semantic_changes"], true, "{diff:#}");
    assert_eq!(diff["is_style_only"], false, "{diff:#}");
    let changes = diff["changes"].as_array().expect("changes array");
    assert!(!changes.is_empty(), "visibility change must surface: {diff:#}");
    assert!(changes.iter().all(|c| c["refactoring_kind"] != "RENAME_VARIABLE"
        && !c["description"].as_str().unwrap_or("").starts_with("Rename variable")),
        "def/defp is a declaration macro, not a variable binder: {diff:#}");
    assert!(changes.iter().any(|c| c["change_type"] == "MODIFICATION"),
        "the public->private API change is a modification: {diff:#}");
    assert!(diff["change_groups"].as_array().unwrap().iter()
        .all(|g| g["kind"] != "IGNORED_STYLE"), "visibility is not formatting: {diff:#}");
}

#[test]
fn defp_to_def_is_a_visibility_change_not_a_variable_rename() {
    let old = "defmodule Answer do\n  defp value(x), do: x\nend\n";
    let new = "defmodule Answer do\n  def value(x), do: x\nend\n";
    let diff = review(old, new);
    assert_eq!(diff["has_semantic_changes"], true, "{diff:#}");
    assert_eq!(diff["is_style_only"], false, "{diff:#}");
    let changes = diff["changes"].as_array().expect("changes array");
    assert!(changes.iter().all(|c| c["refactoring_kind"] != "RENAME_VARIABLE"),
        "def/defp is a declaration macro, not a variable binder: {diff:#}");
    assert!(changes.iter().any(|c| c["change_type"] == "MODIFICATION"), "{diff:#}");
}

#[test]
fn def_to_defp_modification_names_the_declaration_keyword_swap() {
    let old = "defmodule Answer do\n  def value(), do: 1\nend\n";
    let new = "defmodule Answer do\n  defp value(), do: 1\nend\n";
    let diff = review(old, new);
    let changes = diff["changes"].as_array().expect("changes array");
    assert_eq!(changes.len(), 1, "exactly the visibility edit: {diff:#}");
    let c = &changes[0];
    assert_eq!(c["change_type"], "MODIFICATION", "{diff:#}");
    assert_eq!(c["old_node"]["label"], "def", "{diff:#}");
    assert_eq!(c["new_node"]["label"], "defp", "{diff:#}");
    assert_eq!(c["old_node"]["position"], json!({
        "start_line": 1, "start_col": 2, "end_line": 1, "end_col": 5
    }), "{diff:#}");
    assert_eq!(c["new_node"]["position"], json!({
        "start_line": 1, "start_col": 2, "end_line": 1, "end_col": 6
    }), "{diff:#}");
    assert!(diff["change_groups"].as_array().unwrap().iter()
        .any(|g| g["kind"] == "MEANINGFUL_CHANGE"), "{diff:#}");
}

#[test]
fn elixir_callee_rename_inside_a_definition_body_is_still_promoted() {
    // Guard: only the declaring keyword of a declaration form is exempt; an ordinary call
    // target in the body (`foo(1)` -> `bar(1)`) keeps its existing rename promotion.
    let old = "defmodule Answer do\n  def value(), do: foo(1)\nend\n";
    let new = "defmodule Answer do\n  def value(), do: bar(1)\nend\n";
    let diff = review(old, new);
    assert_eq!(diff["has_semantic_changes"], true, "{diff:#}");
    let changes = diff["changes"].as_array().expect("changes array");
    assert!(changes.iter().any(|c| c["change_type"] == "REFACTORING"
        && c["refactoring_kind"] == "RENAME_VARIABLE"
        && c["old_node"]["label"] == "foo" && c["new_node"]["label"] == "bar"),
        "callee reference rename unaffected by the declaration-form gate: {diff:#}");
}

#[test]
fn elixir_parameter_rename_is_not_hidden_by_the_declaration_form_gate() {
    let old = "defmodule Answer do\n  def value(a) do\n    a + 1\n  end\nend\n";
    let new = "defmodule Answer do\n  def value(b) do\n    b + 1\n  end\nend\n";
    let diff = review(old, new);
    assert_eq!(diff["has_semantic_changes"], true, "{diff:#}");
    assert_eq!(diff["is_style_only"], false, "{diff:#}");
    let changes = diff["changes"].as_array().expect("changes array");
    // Both the binder and its body reference are still reported a -> b; the unchanged `def`
    // keyword produces no change at all.
    let renamed: Vec<_> = changes.iter().filter(|c|
        c["old_node"]["label"] == "a" && c["new_node"]["label"] == "b").collect();
    assert_eq!(renamed.len(), 2, "{diff:#}");
    assert!(changes.iter().all(|c| c["old_node"]["label"] != "def"
        && c["new_node"]["label"] != "def"), "{diff:#}");
}
