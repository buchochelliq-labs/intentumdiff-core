//! Contract-only tests for the pinned invariance catalogue.
//! These intentionally do not change the production Rust evaluator dispatch.
//! Activating catalogue-driven suppression requires native/Python golden-output gates.
use serde_json::Value;
use std::collections::HashSet;

const CATALOGUE: &str = include_str!("../invariances/rules.yaml");

fn validate(raw: &str) -> Result<Vec<Value>, String> {
    let doc: Value = serde_yaml::from_str(raw).map_err(|e| e.to_string())?;
    if doc.get("version").and_then(Value::as_u64) != Some(1) {
        return Err("unsupported invariance catalogue version".into());
    }
    let rules = doc.get("rules").and_then(Value::as_array)
        .ok_or("missing rules array")?;
    let mut ids = HashSet::new();
    for rule in rules {
        let id = rule.get("id").and_then(Value::as_str).filter(|s| !s.is_empty())
            .ok_or("missing rule id")?;
        if !ids.insert(id.to_owned()) {
            return Err(format!("duplicate rule id: {id}"));
        }
        let languages = rule.get("languages").and_then(Value::as_array)
            .filter(|x| !x.is_empty()).ok_or(format!("{id}: missing languages"))?;
        if languages.iter().any(|v| v.as_str().is_none_or(str::is_empty)) {
            return Err(format!("{id}: invalid language"));
        }
        let enabled = rule.get("enabled").and_then(Value::as_bool)
            .ok_or(format!("{id}: enabled must be boolean"))?;
        let status = rule.get("status").and_then(Value::as_str)
            .ok_or(format!("{id}: missing status"))?;
        if !matches!(status, "catalog" | "implemented") {
            return Err(format!("{id}: invalid status"));
        }
        if status == "catalog" && enabled {
            return Err(format!("{id}: catalogue-only rule cannot execute"));
        }
        if status == "implemented" && rule.get("evaluator").and_then(Value::as_str).is_none() {
            return Err(format!("{id}: implemented rule missing evaluator"));
        }
    }
    Ok(rules.clone())
}

#[test]
fn pinned_catalogue_has_unique_valid_rule_contracts() {
    let rules = validate(CATALOGUE).expect("versioned Rust invariance catalogue");
    assert_eq!(rules.len(), 10, "investigate rule changes before activating them");
    assert!(rules.iter().any(|v| v["id"] == "generic.style_only_shortcut.source_equivalence"));
    assert!(rules.iter().any(|v| v["id"] == "catalog.parser_only_languages"
        && v["enabled"] == false));
}

#[test]
fn malformed_disabled_and_unknown_catalogue_inputs_fail_closed() {
    for invalid in [
        "version: 2\nrules: []",
        "version: 1\nrules: [",
        "version: 1\nrules: [{id: duplicate, languages: [python], enabled: true, status: implemented, evaluator: x}, {id: duplicate, languages: [python], enabled: true, status: implemented, evaluator: x}]",
        "version: 1\nrules: [{id: a, languages: [python], enabled: true, status: catalog}]",
        "version: 1\nrules: [{id: a, languages: [python], enabled: false, status: unknown}]",
        "version: 1\nrules: [{id: a, languages: [], enabled: true, status: implemented, evaluator: x}]",
    ] {
        assert!(validate(invalid).is_err(), "incorrectly accepted: {invalid}");
    }
}
