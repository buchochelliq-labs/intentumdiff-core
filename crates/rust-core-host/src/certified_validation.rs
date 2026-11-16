// Extracted verbatim from lib.rs (issue #85, lib.rs split round 2).
use crate::*;

pub(crate) fn validate_certified_semantic_diff(diff: &Value) -> Result<(), String> {
    validate_certified_semantic_diff_with_sources(diff, None)
}

/// The certification guarantee (#254). Beyond the envelope, a certified diff must have:
/// - every change's REQUIRED sides (ADDITION: new only; DELETION: old only; MODIFICATION /
///   MOVE / REORDER: both; REFACTORING / STYLE_ONLY: at least one);
/// - on every node, recursively through `children`: non-empty id/node_type, a string label and
///   a valid position (end >= start); when the exact old/new buffers are supplied, every
///   position lies inside its side's source (0-based line, UTF-8 byte column <= line length);
/// - every `change_groups[].raw_change_indices` entry an in-range integer;
/// - explicit boolean flags with `has_semantic_changes == !changes.is_empty()` and
///   `is_style_only` implying no changes.
///
/// It certifies evidence integrity, not that the semantics are right.
pub(crate) fn validate_certified_semantic_diff_with_sources(
    diff: &Value,
    sources: Option<(&str, &str)>,
) -> Result<(), String> {
    validate_certified_semantic_diff_envelope(diff)?;
    let changes = diff
        .get("changes")
        .and_then(Value::as_array)
        .ok_or_else(|| "SemanticDiff changes must be an array".to_owned())?;
    let bounds = sources.map(|(old, new)| (line_lengths(old), line_lengths(new)));
    for change in changes {
        validate_certified_change(change)?;
        if let Some((old_lines, new_lines)) = &bounds {
            for (side, lines, name) in [("old_node", old_lines, "old"), ("new_node", new_lines, "new")] {
                if let Some(node) = change.get(side).filter(|value| !value.is_null()) {
                    validate_node_within_source(node, lines, name, 0)?;
                }
            }
        }
    }
    let flag = |key: &str| diff.get(key).and_then(Value::as_bool)
        .ok_or_else(|| format!("SemanticDiff {key} must be a boolean"));
    let (semantic, style_only) = (flag("has_semantic_changes")?, flag("is_style_only")?);
    if semantic != !changes.is_empty() {
        return Err("SemanticDiff has_semantic_changes must equal whether changes exist".to_owned());
    }
    if style_only && !changes.is_empty() {
        return Err("SemanticDiff is_style_only cannot carry changes".to_owned());
    }
    if let Some(groups) = diff.get("change_groups").filter(|value| !value.is_null()) {
        let groups = groups.as_array().ok_or_else(|| "SemanticDiff change_groups must be an array".to_owned())?;
        for group in groups {
            let indices = group.get("raw_change_indices").and_then(Value::as_array)
                .ok_or_else(|| "change group raw_change_indices must be an array".to_owned())?;
            if !indices.iter().all(|index| index.as_u64().is_some_and(|i| (i as usize) < changes.len())) {
                return Err("change group raw_change_indices must be in-range change indices".to_owned());
            }
        }
    }
    Ok(())
}

fn line_lengths(source: &str) -> Vec<u64> {
    source.split('\n').map(|line| line.len() as u64).collect()
}

fn validate_node_within_source(node: &Value, lines: &[u64], side: &str, depth: usize) -> Result<(), String> {
    if depth > 4096 {
        return Err("semantic node nesting too deep".to_owned());
    }
    let position = &node["position"];
    for (line_key, col_key) in [("start_line", "start_col"), ("end_line", "end_col")] {
        let (line, col) = (position_u64(position, line_key)?, position_u64(position, col_key)?);
        let fits = usize::try_from(line).ok().and_then(|line| lines.get(line)).is_some_and(|len| col <= *len);
        if !fits {
            return Err(format!(
                "semantic node {} position {line}:{col} is outside the {side} source",
                node.get("id").and_then(Value::as_str).unwrap_or_default()
            ));
        }
    }
    if let Some(children) = node.get("children").and_then(Value::as_array) {
        for child in children {
            validate_node_within_source(child, lines, side, depth + 1)?;
        }
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
    if details.get("certification").and_then(Value::as_str) == Some("rust_source_fallback_v1") {
        // This certifies source evidence delivery, never semantic interpretation.
        // Keep parser certification separate and reject incomplete/forged envelopes.
        if diff.get("is_fallback").and_then(Value::as_bool) != Some(true)
            || diff.get("is_style_only").and_then(Value::as_bool) != Some(false)
            || metadata.get("engine_owner").and_then(Value::as_str) != Some("rust")
            || metadata.get("semantic_contract").and_then(Value::as_str) != Some("rust_source_fallback_v1")
            || details.get("engine").and_then(Value::as_str) != Some("rust_source_fallback_v1")
            || details.get("python_parser_backend").and_then(Value::as_str) != Some("source_comparison")
            || details.get("wasm_boundary").and_then(Value::as_str) != Some("not_applicable")
        {
            return Err("invalid Rust source fallback certification".to_owned());
        }
        if !metadata.get("fallback_reason").and_then(Value::as_str).is_some_and(|reason| !reason.is_empty()) {
            return Err("source fallback requires a reason".to_owned());
        }
        if metadata.get("fallback_reason").and_then(Value::as_str) == Some("parse_errors")
            && !diff.get("parse_errors").and_then(Value::as_array).is_some_and(|warnings| !warnings.is_empty())
        {
            return Err("parse-error fallback requires warnings".to_owned());
        }
        let changes = diff.get("changes").and_then(Value::as_array)
            .ok_or_else(|| "source fallback requires changes array".to_owned())?;
        if changes.len() > 1 || diff.get("has_semantic_changes").and_then(Value::as_bool) != Some(!changes.is_empty()) {
            return Err("invalid source fallback change state".to_owned());
        }
        if let Some(change) = changes.first() {
            if !matches!(change.get("change_type").and_then(Value::as_str), Some("ADDITION" | "DELETION" | "MODIFICATION")) {
                return Err("source fallback cannot claim semantic classifications".to_owned());
            }
            let old_present = change.get("old_node").is_some_and(|node| node.is_object());
            let new_present = change.get("new_node").is_some_and(|node| node.is_object());
            if !matches!((change["change_type"].as_str(), old_present, new_present),
                (Some("ADDITION"), false, true) | (Some("DELETION"), true, false) | (Some("MODIFICATION"), true, true)) {
                return Err("source fallback requires evidence for each changed side".to_owned());
            }
            for side in ["old_node", "new_node"] {
                if let Some(node) = change.get(side).filter(|node| !node.is_null()) {
                    if node.get("node_type").and_then(Value::as_str) != Some("unparsed_source")
                        || !node.get("position").is_some_and(Value::is_object)
                        || !node.get("label").is_some_and(Value::is_string)
                    {
                        return Err("source fallback requires unparsed source nodes".to_owned());
                    }
                }
            }
            let groups = diff.get("change_groups").and_then(Value::as_array)
                .ok_or_else(|| "source fallback requires unknown-semantics group".to_owned())?;
            if groups.len() != 1 || groups[0]["metadata"]["semantic_equivalence"] != "unknown"
                || groups[0]["raw_change_indices"] != json!([0])
                || groups[0]["kind"] != "MEANINGFUL_CHANGE"
                || groups[0]["rule_id"] != "source.incomplete_review" {
                return Err("source fallback must preserve semantic uncertainty".to_owned());
            }
        }
        if changes.is_empty() && diff.get("change_groups") != Some(&json!([])) {
            return Err("unchanged source fallback must not fabricate groups".to_owned());
        }
        let ranges = metadata.get("source_ranges")
            .ok_or_else(|| "source fallback requires source ranges".to_owned())?;
        for side in ["old", "new"] {
            let start = ranges.get(format!("{side}_start_byte")).and_then(Value::as_u64);
            let end = ranges.get(format!("{side}_end_byte")).and_then(Value::as_u64);
            if !matches!((start, end), (Some(start), Some(end)) if start <= end) {
                return Err("invalid source fallback byte range".to_owned());
            }
            let has_span = start != end;
            let has_node = changes.first().and_then(|change| change.get(format!("{side}_node")))
                .is_some_and(Value::is_object);
            if has_span != has_node {
                return Err("source fallback range must match side evidence".to_owned());
            }
        }
        if ranges["old_start_byte"] != ranges["new_start_byte"] {
            return Err("source fallback ranges must share their unchanged prefix".to_owned());
        }
    } else if diff.get("is_fallback").and_then(Value::as_bool) == Some(true)
        || details.get("certification").and_then(Value::as_str)
        != Some(PYTHON_NATIVE_V4KB_CERTIFICATION)
    {
        return Err("SemanticDiff certification is not python_native_v4kb or rust_source_fallback_v1".to_owned());
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
    let present = |side: &str| change.get(side).is_some_and(|value| !value.is_null());
    match (change_type, present("old_node"), present("new_node")) {
        ("ADDITION", _, false) => return Err("ADDITION requires a new_node".to_owned()),
        ("DELETION", false, _) => return Err("DELETION requires an old_node".to_owned()),
        ("MODIFICATION" | "MOVE" | "REORDER", old, new) if !(old && new) => {
            return Err(format!("{change_type} requires both old_node and new_node"));
        }
        ("REFACTORING" | "STYLE_ONLY", false, false) => {
            return Err(format!("{change_type} requires an old_node or a new_node"));
        }
        _ => {}
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
    validate_certified_node_at(node, 0)
}

fn validate_certified_node_at(node: &Value, depth: usize) -> Result<(), String> {
    if depth > 4096 {
        return Err("semantic node nesting too deep".to_owned());
    }
    if !node.is_object() {
        return Err("semantic node must be an object".to_owned());
    }
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
    if !node.get("label").is_some_and(Value::is_string) {
        return Err("semantic node label must be a string".to_owned());
    }
    let position = node
        .get("position")
        .filter(|value| value.is_object())
        .ok_or_else(|| "semantic node position must be an object".to_owned())?;
    validate_certified_position(position)?;
    match node.get("children") {
        None | Some(Value::Null) => {}
        Some(Value::Array(children)) => {
            for child in children {
                validate_certified_node_at(child, depth + 1)?;
            }
        }
        Some(_) => return Err("semantic node children must be an array".to_owned()),
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

#[cfg(test)]
#[path = "certified_validation_tests.rs"]
mod certified_validation_tests;
