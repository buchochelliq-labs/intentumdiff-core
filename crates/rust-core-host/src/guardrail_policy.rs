//! Shared policy interpretation for native consumers and C-ABI bindings.
use serde_json::{json, Value};

const GUARDRAIL_RULE_LANGUAGES: [&str; 11] = [
    "adf",
    "databricks",
    "databricks-workflow",
    "dbt-config",
    "dbt-packages",
    "dbt-yaml",
    "dockerfile",
    "hcl",
    "json",
    "puppet",
    "yaml",
];


/// Parse supplied YAML/JSON bytes. Hosts own file discovery and reading.
/// Malformed policy structure is an error, never an empty successful policy.
pub fn parse_policy(source: &str) -> Result<Vec<Value>, String> {
    let doc: Value = serde_yaml::from_str(source).map_err(|e| format!("guardrail policy unparseable: {e}"))?;
    let doc = if doc.is_null() { json!({}) } else { doc };
    if !doc.is_object() { return Err("policy must contain a YAML mapping".into()); }
    let guardrails = doc.get("guardrails").unwrap_or(&doc);
    if !guardrails.is_object() { return Err("guardrails section must be a mapping".into()); }
    let empty = Vec::new();
    let raw_rules = match guardrails.get("protected") {
        None | Some(Value::Null) => &empty,
        Some(Value::Array(items)) => items,
        _ => return Err("guardrails.protected must be a list".into()),
    };
    // Reject malformed rules rather than silently weakening protection.
    let mut rules: Vec<Value> = Vec::with_capacity(raw_rules.len());
    for (index, raw_rule) in raw_rules.iter().enumerate() {
        let Some(map) = raw_rule.as_object() else {
            return Err(format!(
                "guardrails.protected[{index}] must be a mapping"
            ));
        };
        let language = map
            .get("language")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_lowercase();
        if !GUARDRAIL_RULE_LANGUAGES.contains(&language.as_str()) {
            return Err(format!(
                "guardrails.protected[{index}] targets unsupported language '{language}'"
            ));
        }
        let path =
            crate::guardrail_normalise_path(map.get("path").and_then(Value::as_str).unwrap_or(""));
        if path.is_empty() {
            return Err(format!(
                "guardrails.protected[{index}] requires path"
            ));
        }
        let severity = match map.get("severity") {
            None => "important",
            Some(Value::String(value)) => value,
            Some(_) => return Err(format!("guardrails.protected[{index}].severity must be a string")),
        }.to_lowercase();
        if severity != "important" && severity != "immutable" {
            return Err(format!(
                "guardrails.protected[{index}] has unsupported severity '{severity}'"
            ));
        }
        let files: Vec<String> = match map.get("files") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::String(s)) => vec![s.clone()],
            Some(Value::Array(items)) => items
                .iter()
                .map(|item| match item {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect(),
            Some(_) => {
                return Err(format!(
                    "guardrails.protected[{index}].files is invalid"
                ));
            }
        };
        let rule_id = map
            .get("id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("guardrail.{}", index + 1));
        let message = map
            .get("message")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("Protected path {path} changed"));
        rules.push(json!({
            "rule_id": rule_id,
            "severity": severity,
            "language": language,
            "path": path,
            "message": message,
            "files": files,
        }));
    }
    Ok(rules)
}


pub fn policy_file_violation(diff: &Value, old_filename: &str, new_filename: &str) -> Value {
    let file = if !new_filename.is_empty() {
        new_filename
    } else {
        old_filename
    };
    json!({
        "rule_id": "intentumdiff.policy_file",
        "severity": "immutable",
        "file": file,
        "language": diff.get("language").cloned().unwrap_or_else(|| json!("")),
        "semantic_path": "intentumdiff.yaml",
        "node_type": "",
        "old_node_id": Value::Null,
        "new_node_id": Value::Null,
        "position": Value::Null,
        "old_value": "project policy",
        "new_value": "project policy",
        "message": "Project guardrail policy changed",
    })
}

/// Apply shared guardrail meaning to a complete review DTO.
pub fn apply_policy(request: &Value) -> Result<Value, String> {
    let mut diff = request.get("diff").filter(|v| v.is_object()).cloned()
        .ok_or("guardrail application requires a diff object")?;
    if request.get("enabled").and_then(Value::as_bool) == Some(false) { return Ok(diff); }
    let old_filename = diff.get("old_filename").and_then(Value::as_str).unwrap_or("").to_owned();
    let new_filename = diff.get("new_filename").and_then(Value::as_str).unwrap_or("").to_owned();
    let old_source = request.get("old_source").and_then(Value::as_str).unwrap_or("");
    let new_source = request.get("new_source").and_then(Value::as_str).unwrap_or("");
    let rules = request.get("rules").and_then(Value::as_array).ok_or("guardrail rules must be an array")?;
    let mut violations = crate::evaluate_guardrail_rules_for_diff(
        &diff, rules, diff.get("language").and_then(Value::as_str).unwrap_or(""),
        &old_filename, &new_filename,
        &request.get("old_tree").unwrap_or(&Value::Null).to_string(),
        &request.get("new_tree").unwrap_or(&Value::Null).to_string(),
    )?;
    let is_policy = |name: &str| name.rsplit(['/', '\\']).next()
        .is_some_and(|name| name.eq_ignore_ascii_case("intentumdiff.yaml"));
    if old_source != new_source && (is_policy(&old_filename) || is_policy(&new_filename)) {
        violations.insert(0, policy_file_violation(&diff, &old_filename, &new_filename));
    }
    crate::attach_guardrail_violations(&mut diff, violations);
    Ok(diff)
}
