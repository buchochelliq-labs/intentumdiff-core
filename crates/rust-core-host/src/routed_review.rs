//! Complete routed review semantics for native callers and language bindings.
use serde_json::{json, Value};

/// Reconcile finalized tree changes into the complete public review DTO.
/// Hosts supply source, trees, filenames and optional schema/compile metadata.
pub fn complete(request: &Value) -> Result<Value, String> {
    let text = |key: &str| request.get(key).and_then(Value::as_str).ok_or_else(|| format!("missing {key}"));
    let language = text("language")?;
    let old_source = text("old_source")?;
    let new_source = text("new_source")?;
    let old_filename = text("old_filename")?;
    let new_filename = text("new_filename")?;
    let fin = request.get("finalized").filter(|v| v.is_object()).ok_or("missing finalized review")?;
    let mut diff = if language.eq_ignore_ascii_case("generic") {
        crate::native_generic_text_diff(language, old_source, new_source, old_filename, new_filename)?
    } else {
        let mut changes = fin.get("changes").and_then(Value::as_array).cloned().ok_or("finalized changes must be an array")?;
        let mut groups = fin.get("change_groups").and_then(Value::as_array).cloned().unwrap_or_default();
        let mut ignored = fin.get("ignored_style_changes").and_then(Value::as_array).cloned().unwrap_or_default();
        let mut style = fin.get("is_style_only").and_then(Value::as_bool).unwrap_or(false);
        if !changes.is_empty() {
            let inv = crate::invariance_groups::rust_apply_invariances_value(&json!({
                "changes":changes,"old_tree":request["old_tree"],"new_tree":request["new_tree"],
                "old_source":old_source,"new_source":new_source,"language":language
            }))?;
            let kept = inv["changes"].as_array().ok_or("invalid invariance changes")?.clone();
            // Existing indices refer to the pre-invariance list. Remap retained groups;
            // discard groups whose evidence was removed by equivalence.
            if kept != changes {
                let mut matched = vec![false; kept.len()];
                let mapping: Vec<Option<usize>> = changes.iter().map(|change| {
                    let found = kept.iter().enumerate().position(|(i, candidate)| !matched[i] && candidate == change);
                    if let Some(i) = found { matched[i] = true; }
                    found
                }).collect();
                groups.retain_mut(|group| {
                    if group["metadata"]["index_space"].as_str().is_some_and(|s| s != "final_changes") {
                        return true;
                    }
                    let Some(indices) = group.get("raw_change_indices").and_then(Value::as_array) else { return true; };
                    if indices.is_empty() { return true; }
                    let mapped: Vec<usize> = indices.iter().filter_map(|index| index.as_u64()
                        .and_then(|i| mapping.get(i as usize).copied().flatten())).collect();
                    if mapped.is_empty() { return false; }
                    if mapped.len() != indices.len() {
                        // Ordinary evidence survives partially; a relationship classification
                        // cannot retain its claim after part of its premise was removed.
                        if group["kind"] != "MEANINGFUL_CHANGE" { return false; }
                        for (field, side, attr) in [("old_labels", "old_node", "label"),
                            ("new_labels", "new_node", "label"), ("old_node_ids", "old_node", "id"),
                            ("new_node_ids", "new_node", "id")] {
                            let mut values: Vec<String> = Vec::new();
                            if attr == "label" {
                                let key = if side == "old_node" { "old_entity_label" } else { "new_entity_label" };
                                if let Some(label) = group["metadata"][key].as_str() { values.push(label.to_owned()); }
                            }
                            for value in mapped.iter().filter_map(|i| kept[*i][side][attr].as_str()) {
                                if !values.iter().any(|v| v == value) { values.push(value.to_owned()); }
                            }
                            group[field] = json!(values);
                        }
                    }
                    group["raw_change_indices"] = json!(mapped);
                    true
                });
                if kept.is_empty() && old_source != new_source { style = true; }
            }
            changes = kept;
            groups.extend(inv["change_groups"].as_array().cloned().unwrap_or_default());
            let mut specific = inv["ignored_style_changes"].as_array().cloned().unwrap_or_default();
            specific.extend(ignored);
            ignored = specific;
        }
        if changes.is_empty() && !style {
            style = old_source == new_source || crate::review_trees_equivalent_impl(
                &request["old_tree"].to_string(), &request["new_tree"].to_string())? == "true";
        }
        // Suppression alone is not proof of equivalence. Never fabricate style evidence
        // for different source trees merely because no changes survived refinement.
        if changes.is_empty() && style && old_source != new_source && ignored.is_empty() {
            let evidence = crate::invariance_groups::rust_build_style_only_evidence(&json!({
                "old_source":old_source,"new_source":new_source,"language":language
            }))?;
            groups.extend(evidence["change_groups"].as_array().cloned().unwrap_or_default());
            ignored.extend(evidence["ignored_style_changes"].as_array().cloned().unwrap_or_default());
        }
        let mut metadata = json!({});
        if !ignored.is_empty() { metadata["ignored_style_changes"] = json!(ignored); }
        if fin["no_surviving_changes"] == true || (changes.is_empty() && !style && old_source != new_source) {
            metadata["no_surviving_changes"] = json!(true);
        }
        json!({"old_filename":old_filename,"new_filename":new_filename,"language":language,
            "has_semantic_changes":!changes.is_empty() && !style,"is_style_only":style,
            "changes":changes,"change_groups":groups,"metadata":metadata,
            "parse_errors":[],"llm_summary":"","gitignore_excluded":false,"is_fallback":false,"guardrail_violations":[]})
    };
    diff["metadata"]["engine_owner"] = json!("rust");
    diff["metadata"]["semantic_contract"] = json!("rust_finalize_review_v1");
    diff["metadata"]["rust_core"] = json!({"engine":"rust_finalize_review_v1", "stage":"per_stage_finalize_routing", "used":true});
    for (input, output) in [("schema_metadata", "schema"), ("compile_metadata", "compile_commands")] {
        if let Some(value) = request.get(input).filter(|v| !v.is_null()) { diff["metadata"][output] = value.clone(); }
    }
    Ok(diff)
}
