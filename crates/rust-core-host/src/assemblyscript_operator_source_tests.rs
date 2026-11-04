//! Independent source-judged Rust integration controls for AssemblyScript #191.
//! These use the *real*, checksum-verified parser Wasm and fixed expected byte spans.
use crate::*;
use serde_json::{json, Value};

fn review(old: &str, new: &str) -> Value {
    let staging = std::env::var("INTENTUMDIFF_TEST_WASM_DIR")
        .expect("full CI must stage registry-pinned AssemblyScript Wasm");
    let source = std::path::Path::new(&staging).join("assemblyscript_parser.wasm");
    assert!(source.is_file(), "verified AssemblyScript component missing: {}", source.display());
    let dir = tempfile::tempdir().expect("isolated parser manifest");
    std::fs::copy(source, dir.path().join("assemblyscript_parser.wasm"))
        .expect("copy exact checksum-verified component bytes");
    std::fs::write(dir.path().join("parser_manifest.json"), json!({
        "schema_version": 1,
        "parsers": {
            "assemblyscript": {
                "plugin_id": "assemblyscript",
                "wasm": "assemblyscript_parser.wasm",
                "extensions": [".as"]
            }
        },
        "extension_index": {".as": "assemblyscript"}
    }).to_string()).expect("test-only verified-component manifest");
    let raw = crate::live_server::live_diff_contents_impl(
        env!("CARGO_MANIFEST_DIR"), "code.as", old, new, "{}",
        dir.path().to_str().expect("UTF-8 staging path")
    ).expect("native source review");
    let response: Value = serde_json::from_str(&raw).expect("native JSON");
    let diff = response.get("diff").unwrap_or_else(|| panic!("no native diff: {response:#}"));
    assert_eq!(diff["language"], "assemblyscript");
    assert_eq!(diff["is_fallback"], false);
    assert_eq!(diff["parse_errors"], json!([]));
    assert_eq!(diff["is_style_only"], false);
    diff.clone()
}

#[test]
fn square_operator_and_operand_rewrite_is_one_expression_modification_next_to_cube() {
    let old = "export function square(n: i32): i32 {\n  return n * n;\n}\n";
    let new = "export function square(n: i32): i32 {\n  return n ** 2;\n}\n\nexport function cube(n: i32): i32 {\n  return n ** 3;\n}\n";
    let diff = review(old, new);
    assert_eq!(diff["has_semantic_changes"], true);
    let changes = diff["changes"].as_array().unwrap();
    assert_eq!(changes.len(), 2, "one continuing expression change and one cube addition: {diff:#}");
    let expression = changes.iter().find(|c| c["change_type"] == "MODIFICATION")
        .unwrap_or_else(|| panic!("missing square operator change: {diff:#}"));
    assert_eq!(expression["old_node"]["node_type"], "binary_expression");
    assert_eq!(expression["new_node"]["node_type"], "binary_expression");
    assert_eq!(expression["old_node"]["position"], json!({
        "start_line": 1, "start_col": 9, "end_line": 1, "end_col": 14
    }));
    assert_eq!(expression["new_node"]["position"], json!({
        "start_line": 1, "start_col": 9, "end_line": 1, "end_col": 15
    }));
    assert!(expression["old_node"].to_string().contains("\"*\""), "{diff:#}");
    assert!(expression["new_node"].to_string().contains("\"**\""), "{diff:#}");
    assert!(expression["new_node"].to_string().contains("\"2\""), "{diff:#}");
    let cube = changes.iter().filter(|c| {
        c["change_type"] == "ADDITION"
            && c["new_node"]["node_type"] == "function_declaration"
            && c["new_node"]["label"] == "cube"
    }).count();
    assert_eq!(cube, 1, "cube must be a single new named callable: {diff:#}");
    assert!(diff["change_groups"].as_array().unwrap().iter()
        .all(|g| g["kind"] != "IGNORED_STYLE"), "no false formatting equivalence: {diff:#}");
}

#[test]
fn operator_only_edit_is_not_a_delete_add_pair_or_style() {
    let old = "export function square(n: i32): i32 {\n  return n * n;\n}\n";
    let new = "export function square(n: i32): i32 {\n  return n ** n;\n}\n";
    let diff = review(old, new);
    let changes = diff["changes"].as_array().unwrap();
    assert_eq!(changes.len(), 1, "operator-only edit must be one modification: {diff:#}");
    assert_eq!(changes[0]["change_type"], "MODIFICATION");
    assert_eq!(changes[0]["old_node"]["position"], json!({
        "start_line": 1, "start_col": 9, "end_line": 1, "end_col": 14
    }));
    assert_eq!(changes[0]["new_node"]["position"], json!({
        "start_line": 1, "start_col": 9, "end_line": 1, "end_col": 15
    }));
    assert!(diff["change_groups"].as_array().unwrap().iter()
        .all(|g| g["kind"] != "IGNORED_STYLE"), "{diff:#}");
}

#[test]
fn same_operator_operand_edit_must_not_be_falsely_suppressed() {
    let old = "export function square(n: i32): i32 {\n  return n * n;\n}\n";
    let new = "export function square(n: i32): i32 {\n  return n * 2;\n}\n";
    let diff = review(old, new);
    assert_eq!(diff["has_semantic_changes"], true, "{diff:#}");
    assert!(!diff["changes"].as_array().unwrap().is_empty(), "{diff:#}");
    assert!(diff["change_groups"].as_array().unwrap().iter()
        .all(|g| g["kind"] != "IGNORED_STYLE"), "{diff:#}");
}
