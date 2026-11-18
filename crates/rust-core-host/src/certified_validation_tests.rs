//! #254: certification requires required sides, recursive positions inside the exact source
//! buffers, in-range group indices and flag consistency — not just a valid envelope.
use crate::*;

fn node(id: &str, line: u64, start: u64, end: u64, children: Vec<Value>) -> Value {
    json!({"id": id, "node_type": "identifier", "label": "x", "structural_hash": format!("h{id}"),
           "position": {"start_line": line, "start_col": start, "end_line": line, "end_col": end},
           "children": children})
}

fn certified(changes: Vec<Value>, groups: Value) -> Value {
    let has = !changes.is_empty();
    let mut diff = semantic_diff_payload("a.py", "a.py", changes, has, COMPLETE, json!({
        "certification": PYTHON_NATIVE_V4KB_CERTIFICATION, "trust_tier": "first_party_core_builder"}));
    diff["metadata"]["rust_core"]["details"]["certification"] = json!(PYTHON_NATIVE_V4KB_CERTIFICATION);
    diff["metadata"]["rust_core"]["details"]["trust_tier"] = json!("first_party_core_builder");
    diff["change_groups"] = groups;
    diff
}

const OLD: &str = "x = 1\ny = 2\n";
const NEW: &str = "x = 1\ny = 3\nzz = 4\n";

fn check(diff: &Value) -> Result<(), String> {
    validate_certified_semantic_diff_with_sources(diff, Some((OLD, NEW)))
}

#[test]
fn valid_additions_deletions_pairs_and_style_results_pass() {
    let add = json!({"change_type": "ADDITION", "new_node": node("0.2", 2, 0, 6, vec![node("0.2.0", 2, 0, 2, vec![])])});
    let del = json!({"change_type": "DELETION", "old_node": node("0.1", 1, 0, 5, vec![])});
    let modi = json!({"change_type": "MODIFICATION", "old_node": node("0.1.2", 1, 4, 5, vec![]), "new_node": node("0.1.2", 1, 4, 5, vec![])});
    let diff = certified(vec![add, del, modi], json!([{"kind": "MEANINGFUL_CHANGE", "raw_change_indices": [0, 1, 2]}]));
    check(&diff).unwrap();
    validate_certified_semantic_diff(&diff).unwrap();
    // End-of-buffer positions (after the trailing newline) are in bounds.
    let eof = certified(vec![json!({"change_type": "ADDITION", "new_node": node("0.3", 3, 0, 0, vec![])})], json!([]));
    check(&eof).unwrap();
    let mut style = certified(vec![], json!([{"kind": "IGNORED_STYLE", "raw_change_indices": []}]));
    style["is_style_only"] = json!(true);
    check(&style).unwrap();
}

#[test]
fn every_malformed_class_is_rejected() {
    let ok_node = node("0.1", 1, 0, 5, vec![]);
    let cases: Vec<(&str, Value, &str)> = vec![
        ("addition without new_node", certified(vec![json!({"change_type": "ADDITION"})], json!([])), "ADDITION requires a new_node"),
        ("deletion without old_node", certified(vec![json!({"change_type": "DELETION", "old_node": null})], json!([])), "DELETION requires an old_node"),
        ("pair missing a side", certified(vec![json!({"change_type": "MODIFICATION", "old_node": ok_node})], json!([])), "MODIFICATION requires both"),
        ("move missing a side", certified(vec![json!({"change_type": "MOVE", "new_node": ok_node})], json!([])), "MOVE requires both"),
        ("missing position", certified(vec![json!({"change_type": "DELETION", "old_node": {"id": "0.1", "node_type": "identifier", "label": "x"}})], json!([])), "position"),
        ("child with inverted span", certified(vec![json!({"change_type": "DELETION", "old_node": node("0.1", 1, 0, 5, vec![node("0.1.0", 1, 4, 2, vec![])])})], json!([])), "end must be >= start"),
        ("child missing span", certified(vec![json!({"change_type": "DELETION", "old_node": node("0.1", 1, 0, 5, vec![json!({"id": "0.1.0", "node_type": "x", "label": ""})])})], json!([])), "position"),
        ("non-array children", certified(vec![json!({"change_type": "DELETION", "old_node": {"id": "0.1", "node_type": "x", "label": "", "position": {"start_line": 0, "start_col": 0, "end_line": 0, "end_col": 1}, "children": {}}})], json!([])), "children"),
        ("line out of source", certified(vec![json!({"change_type": "DELETION", "old_node": node("0.9", 7, 0, 1, vec![])})], json!([])), "outside the old source"),
        ("column out of source", certified(vec![json!({"change_type": "DELETION", "old_node": node("0.1", 1, 0, 40, vec![])})], json!([])), "outside the old source"),
        ("nested child out of new source", certified(vec![json!({"change_type": "ADDITION", "new_node": node("0.2", 2, 0, 6, vec![node("0.2.0", 2, 0, 99, vec![])])})], json!([])), "outside the new source"),
        ("group index out of range", certified(vec![json!({"change_type": "DELETION", "old_node": ok_node})], json!([{"kind": "MEANINGFUL_CHANGE", "raw_change_indices": [1]}])), "raw_change_indices"),
        ("group index not an integer", certified(vec![json!({"change_type": "DELETION", "old_node": ok_node})], json!([{"kind": "MEANINGFUL_CHANGE", "raw_change_indices": ["0"]}])), "raw_change_indices"),
    ];
    for (name, diff, needle) in cases {
        let error = check(&diff).expect_err(name);
        assert!(error.contains(needle), "{name}: {error}");
    }
    let mut semantic_without_changes = certified(vec![], json!([]));
    semantic_without_changes["has_semantic_changes"] = json!(true);
    assert!(check(&semantic_without_changes).unwrap_err().contains("has_semantic_changes"));
    let mut silent_changes = certified(vec![json!({"change_type": "DELETION", "old_node": ok_node})], json!([]));
    silent_changes["has_semantic_changes"] = json!(false);
    assert!(check(&silent_changes).unwrap_err().contains("has_semantic_changes"));
    let mut style_with_changes = certified(vec![json!({"change_type": "DELETION", "old_node": ok_node})], json!([]));
    style_with_changes["is_style_only"] = json!(true);
    assert!(check(&style_with_changes).unwrap_err().contains("is_style_only"));
    let mut missing_flag = certified(vec![], json!([]));
    missing_flag.as_object_mut().unwrap().remove("is_style_only");
    assert!(check(&missing_flag).unwrap_err().contains("is_style_only"));
}
