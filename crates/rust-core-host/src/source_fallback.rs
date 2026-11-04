//! Conservative source evidence when a semantic interpretation is unavailable.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
const MAX_SOURCE: usize = 4 * 1024 * 1024;

pub(crate) fn parse_errors_present_impl(source: &str, tree: &str, language: &str) -> Result<String, String> {
    crate::check_byte_limit("source", source, MAX_SOURCE)?;
    crate::check_byte_limit("tree", tree, 16 * 1024 * 1024)?;
    if language.eq_ignore_ascii_case("python") && crate::parse_python_tree(source)?.root_node().has_error() {
        return Ok("true".into());
    }
    let root: Value = serde_json::from_str(tree).map_err(|e| format!("tree: {e}"))?;
    let mut pending = vec![&root];
    while let Some(node) = pending.pop() {
        if node.get("type").or_else(|| node.get("node_type")).and_then(Value::as_str) == Some("ERROR")
            || node.get("is_error").and_then(Value::as_bool) == Some(true)
            || node.get("is_missing").and_then(Value::as_bool) == Some(true) {
            return Ok("true".into());
        }
        if let Some(children) = node.get("children").and_then(Value::as_array) { pending.extend(children); }
    }
    Ok("false".into())
}

fn point(source: &str, offset: usize) -> (usize, usize) {
    let prefix = &source[..offset];
    (prefix.bytes().filter(|b| *b == b'\n').count(), prefix.rfind('\n').map_or(offset, |i| offset-i-1))
}
fn node(source: &str, start: usize, end: usize, id: &str) -> Value {
    let (start_line, start_col) = point(source, start);
    let (end_line, end_col) = point(source, end);
    json!({"id": id, "node_type": "unparsed_source", "label": source[start..end].chars().take(160).collect::<String>(),
        "position": {"start_line":start_line,"start_col":start_col,"end_line":end_line,"end_col":end_col},
        "structural_hash":hex::encode(Sha256::digest(source[start..end].as_bytes())),"children":[]})
}

pub(crate) fn source_fallback_diff_impl(old: &str, new: &str, old_filename: &str, new_filename: &str, language: &str, reason: &str) -> Result<Value, String> {
    crate::check_byte_limit("old source", old, MAX_SOURCE)?;
    crate::check_byte_limit("new source", new, MAX_SOURCE)?;
    let prefix: usize = old.chars().zip(new.chars()).take_while(|(a,b)| a==b).map(|(c,_)| c.len_utf8()).sum();
    let suffix: usize = old[prefix..].chars().rev().zip(new[prefix..].chars().rev()).take_while(|(a,b)| a==b).map(|(c,_)| c.len_utf8()).sum();
    let (old_end, new_end) = (old.len()-suffix, new.len()-suffix);
    let changed = old != new;
    let mut changes = vec![];
    if changed {
        let kind = if old_end == prefix { "ADDITION" } else if new_end == prefix { "DELETION" } else { "MODIFICATION" };
        changes.push(json!({"change_type":kind,
            "old_node":if old_end>prefix {node(old,prefix,old_end,"source.old")} else {Value::Null},
            "new_node":if new_end>prefix {node(new,prefix,new_end,"source.new")} else {Value::Null},
            "description":"Source changed; semantic equivalence is unknown (source fallback)","confidence":0.5}));
    }
    let mut diff = crate::semantic_diff_payload(old_filename,new_filename,changes,changed,crate::COMPLETE,json!({"engine":"rust_source_fallback_v1", "certification":"rust_source_fallback_v1", "trust_tier":"first_party_core_builder", "python_parser_backend":"source_comparison", "wasm_boundary":"not_applicable"}));
    diff["language"] = json!(language);
    diff["is_fallback"] = json!(true);
    diff["metadata"]["rust_core"]["supported_language"] = json!(language);
    diff["metadata"]["engine_owner"] = json!("rust");
    diff["metadata"]["semantic_contract"] = json!("rust_source_fallback_v1");
    diff["metadata"]["fallback_reason"] = json!(reason);
    diff["metadata"]["source_ranges"] = json!({"old_start_byte":prefix,"old_end_byte":old_end,"new_start_byte":prefix,"new_end_byte":new_end});
    diff["metadata"]["preview_limit_chars"] = json!(160);
    if reason == "parse_errors" { diff["parse_errors"] = json!(["Incomplete or invalid syntax; source changes preserved without a semantic interpretation"]); }
    if changed { diff["change_groups"] = json!([{"kind":"MEANINGFUL_CHANGE","raw_change_indices":[0],"confidence":0.5,"rule_id":"source.incomplete_review","metadata":{"semantic_equivalence":"unknown"}}]); }
    Ok(diff)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incomplete_python_commit_preserves_unknown_source_evidence() {
        for new in ["def g(", "def f("] {
            let request = json!({"schema_version":1,"mode":"commit_json",
                "old_ref":"HEAD","new_ref":"","python_parser_backend":"native",
                "config":{},"parallel":false,"files":[{"old_source":"def f(","new_source":new,
                "old_filename":"edit.py","new_filename":"edit.py","language":"python",
                "parser_plugin_id":"python","parser_wasm_path":""}]});
            let (control, payload) = crate::diff_batch_commit_json_impl(&request.to_string()).unwrap();
            assert_eq!(control["status"], crate::COMPLETE);
            let commit: Value = serde_json::from_slice(&payload.unwrap()).unwrap();
            let diff = &commit["file_diffs"][0];
            assert_eq!(diff["is_fallback"], true);
            assert_eq!(diff["is_style_only"], false);
            assert_eq!(diff["metadata"]["rust_core"]["status"], crate::COMPLETE);
            assert_eq!(diff["metadata"]["semantic_contract"], "rust_source_fallback_v1");
            assert!(!diff["parse_errors"].as_array().unwrap().is_empty());
            assert_eq!(diff["changes"].as_array().unwrap().len(), usize::from(new != "def f("));
            if new != "def f(" {
                assert_eq!(diff["changes"][0]["old_node"]["label"], "f");
                assert_eq!(diff["changes"][0]["new_node"]["label"], "g");
                assert_eq!(diff["changes"][0]["new_node"]["position"]["start_col"], 4);
                assert_eq!(diff["change_groups"][0]["metadata"]["semantic_equivalence"], "unknown");
            }
        }
    }

    #[test]
    fn source_fallback_certification_remains_fail_closed() {
        for (old, new) in [("", "def f("), ("def f(", "")] {
            let diff = source_fallback_diff_impl(old, new, "edit.py", "edit.py", "python", "parse_errors").unwrap();
            crate::validate_certified_semantic_diff(&diff).unwrap();
        }
        let diff = source_fallback_diff_impl("def f(", "def g(", "edit.py", "edit.py", "python", "parse_errors").unwrap();
        crate::validate_certified_semantic_diff(&diff).unwrap();
        for (pointer, value) in [
            ("/metadata/rust_core/status", json!("COMPLETE")),
            ("/metadata/rust_core/details/certification", json!("unknown")),
            ("/metadata/rust_core/details/certification", json!(crate::PYTHON_NATIVE_V4KB_CERTIFICATION)),
            ("/metadata/rust_core/details/certification", Value::Null),
            ("/metadata/fallback_reason", json!("")),
            ("/changes/0/new_node", Value::Null),
            ("/metadata/rust_core/details/trust_tier", json!("untrusted")),
            ("/metadata/semantic_contract", json!("unknown")),
            ("/is_fallback", json!(false)),
            ("/is_style_only", json!(true)),
            ("/metadata/source_ranges/new_end_byte", json!(0)),
            ("/metadata/source_ranges/new_end_byte", json!(4)),
            ("/metadata/source_ranges/new_start_byte", json!(3)),
            ("/change_groups/0/kind", json!("REFACTORING")),
            ("/change_groups/0/kind", json!("STYLE_ONLY")),
            ("/change_groups/0/rule_id", json!("unknown")),
            ("/parse_errors", json!([])),
            ("/change_groups/0/metadata/semantic_equivalence", json!("equivalent")),
            ("/changes/0/new_node/position/end_col", json!(0)),
            ("/changes/0/new_node/position", Value::Null),
            ("/changes/0/new_node/node_type", json!("function_definition")),
            ("/changes/0/change_type", json!("REFACTORING")),
        ] {
            let mut invalid = diff.clone();
            *invalid.pointer_mut(pointer).unwrap() = value;
            assert!(crate::validate_certified_semantic_diff(&invalid).is_err(), "accepted {pointer}: {invalid}");
        }
        let mut unchanged = source_fallback_diff_impl("def f(", "def f(", "edit.py", "edit.py", "python", "parse_errors").unwrap();
        crate::validate_certified_semantic_diff(&unchanged).unwrap();
        unchanged["change_groups"] = diff["change_groups"].clone();
        assert!(crate::validate_certified_semantic_diff(&unchanged).is_err());
    }
}
