// Extracted verbatim from lib.rs (issue #85, lib.rs split round 2).
use crate::*;

pub(crate) fn validate_certified_semantic_diff(diff: &Value) -> Result<(), String> {
    validate_certified_semantic_diff_envelope(diff)?;
    let changes = diff
        .get("changes")
        .and_then(Value::as_array)
        .ok_or_else(|| "SemanticDiff changes must be an array".to_owned())?;
    for change in changes {
        validate_certified_change(change)?;
    }
    Ok(())
}

pub(crate) fn validate_certified_semantic_diff_envelope(diff: &Value) -> Result<(), String> {
    let old_filename = diff
        .get("old_filename")
        .and_then(Value::as_str)
        .ok_or_else(|| "SemanticDiff old_filename must be a string".to_owned())?;
    let new_filename = diff
        .get("new_filename")
        .and_then(Value::as_str)
        .ok_or_else(|| "SemanticDiff new_filename must be a string".to_owned())?;
    if old_filename.is_empty() || new_filename.is_empty() {
        return Err("SemanticDiff filenames must not be empty".to_owned());
    }
    if diff.get("language").and_then(Value::as_str) != Some("python") {
        return Err("certified commit JSON only accepts Python diffs".to_owned());
    }
    let metadata = diff
        .get("metadata")
        .and_then(Value::as_object)
        .ok_or_else(|| "SemanticDiff metadata must be an object".to_owned())?;
    let rust_core = metadata
        .get("rust_core")
        .and_then(Value::as_object)
        .ok_or_else(|| "SemanticDiff metadata.rust_core must be an object".to_owned())?;
    if rust_core.get("status").and_then(Value::as_str) != Some(COMPLETE) {
        return Err("SemanticDiff rust_core status must be complete".to_owned());
    }
    let details = rust_core
        .get("details")
        .and_then(Value::as_object)
        .ok_or_else(|| "SemanticDiff rust_core details must be an object".to_owned())?;
    if details.get("certification").and_then(Value::as_str)
        != Some(PYTHON_NATIVE_V4KB_CERTIFICATION)
    {
        return Err("SemanticDiff certification is not python_native_v4kb".to_owned());
    }
    if details.get("trust_tier").and_then(Value::as_str) != Some("first_party_core_builder") {
        return Err("SemanticDiff trust tier is not first_party_core_builder".to_owned());
    }
    let Some(changes) = diff.get("changes").and_then(Value::as_array) else {
        return Err("SemanticDiff changes must be an array".to_owned());
    };
    for change in changes {
        let change_type = change
            .get("change_type")
            .and_then(Value::as_str)
            .ok_or_else(|| "change_type must be a string".to_owned())?;
        if !is_valid_change_type(change_type) {
            return Err(format!("unsupported change_type: {change_type}"));
        }
    }
    Ok(())
}

pub(crate) fn is_valid_change_type(change_type: &str) -> bool {
    matches!(
        change_type,
        "ADDITION"
            | "DELETION"
            | "MODIFICATION"
            | "MOVE"
            | "REORDER"
            | "STYLE_ONLY"
            | "REFACTORING"
    )
}

pub(crate) fn validate_certified_change(change: &Value) -> Result<(), String> {
    let change_type = change
        .get("change_type")
        .and_then(Value::as_str)
        .ok_or_else(|| "change_type must be a string".to_owned())?;
    if !is_valid_change_type(change_type) {
        return Err(format!("unsupported change_type: {change_type}"));
    }
    if change_type == "ADDITION" && change.get("old_node").is_some_and(|value| !value.is_null()) {
        return Err("ADDITION must not have an old_node".to_owned());
    }
    if change_type == "DELETION" && change.get("new_node").is_some_and(|value| !value.is_null()) {
        return Err("DELETION must not have a new_node".to_owned());
    }
    if change_type == "REFACTORING"
        && change
            .get("refactoring_kind")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .is_empty()
    {
        return Err("REFACTORING requires refactoring_kind".to_owned());
    }
    if let Some(node) = change.get("old_node").filter(|value| !value.is_null()) {
        validate_certified_node(node)?;
    }
    if let Some(node) = change.get("new_node").filter(|value| !value.is_null()) {
        validate_certified_node(node)?;
    }
    Ok(())
}

pub(crate) fn validate_certified_node(node: &Value) -> Result<(), String> {
    for field in ["id", "node_type"] {
        if node
            .get(field)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .is_empty()
        {
            return Err(format!("semantic node {field} must not be empty"));
        }
    }
    if let Some(position) = node.get("position").filter(|value| !value.is_null()) {
        validate_certified_position(position)?;
    }
    Ok(())
}

pub(crate) fn validate_certified_position(position: &Value) -> Result<(), String> {
    let start_line = position_u64(position, "start_line")?;
    let start_col = position_u64(position, "start_col")?;
    let end_line = position_u64(position, "end_line")?;
    let end_col = position_u64(position, "end_col")?;
    if (end_line, end_col) < (start_line, start_col) {
        return Err("node position end must be >= start".to_owned());
    }
    Ok(())
}

pub(crate) fn position_u64(position: &Value, field: &str) -> Result<u64, String> {
    position
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("node position {field} must be a non-negative integer"))
}

pub(crate) fn finalize_hex_hash(hasher: Sha256) -> String {
    format!("{:x}", hasher.finalize())
}

pub(crate) fn update_semantic_signature_hash_from_diff(hasher: &mut Sha256, diff: &Value) {
    let old_filename = diff
        .get("old_filename")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let new_filename = diff
        .get("new_filename")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if let Some(changes) = diff.get("changes").and_then(Value::as_array) {
        for change in changes {
            update_signature_field(hasher, old_filename);
            update_signature_field(hasher, new_filename);
            update_signature_field(
                hasher,
                change
                    .get("change_type")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown"),
            );
            update_node_signature_hash(hasher, change.get("old_node"));
            update_node_signature_hash(hasher, change.get("new_node"));
            update_signature_field(
                hasher,
                change
                    .get("refactoring_kind")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            );
            update_signature_field(
                hasher,
                change
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            );
            hasher.update(b"\n");
        }
    }
}

pub(crate) fn update_node_signature_hash(hasher: &mut Sha256, node: Option<&Value>) {
    let Some(node) = node.filter(|value| !value.is_null()) else {
        update_signature_field(hasher, "");
        return;
    };
    update_signature_field(
        hasher,
        node.get("id").and_then(Value::as_str).unwrap_or_default(),
    );
    update_signature_field(
        hasher,
        node.get("node_type")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    update_signature_field(
        hasher,
        node.get("label")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    let position = node.get("position").unwrap_or(&Value::Null);
    for field in ["start_line", "start_col", "end_line", "end_col"] {
        hasher.update(
            position
                .get(field)
                .and_then(Value::as_u64)
                .unwrap_or_default()
                .to_string()
                .as_bytes(),
        );
        hasher.update(b"\0");
    }
}

pub(crate) fn update_signature_field(hasher: &mut Sha256, value: &str) {
    hasher.update(value.as_bytes());
    hasher.update(b"\0");
}
