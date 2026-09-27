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
    let mut diff = crate::semantic_diff_payload(old_filename,new_filename,changes,changed,"COMPLETE",json!({"engine":"rust_source_fallback_v1", "python_parser_backend":"source_comparison", "wasm_boundary":"not_applicable"}));
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
