//! Contract-only tests for the pinned invariance catalogue.
//! These intentionally do not change the production Rust evaluator dispatch.
//! Activating catalogue-driven suppression requires native/Python golden-output gates.
use serde_json::Value;
use std::collections::HashSet;

const CATALOGUE: &str = include_str!("../invariances/rules.yaml");
const SCHEMA: &str = include_str!("../invariances/rules.schema.json");

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
        if status == "catalog" && (enabled || rule.get("evaluator").is_some_and(|v| !v.is_null())) {
            return Err(format!("{id}: catalogue-only rule cannot execute"));
        }
        if status == "implemented" {
            let evaluator = rule.get("evaluator").and_then(Value::as_str)
                .ok_or(format!("{id}: implemented rule missing evaluator"))?;
            const KNOWN: &[&str] = &[
                "css_color_equivalence", "formatting_suppression_evidence",
                "integer_literal_equivalence", "number_literal_equivalence",
                "parser_normalization", "style_only_shortcut_evidence",
                "string_literal_equivalence",
            ];
            if !KNOWN.contains(&evaluator) {
                return Err(format!("{id}: unknown evaluator: {evaluator}"));
            }
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
        "version: 1\nrules: [{id: a, languages: [python], enabled: true, status: catalog}]",
        "version: 1\nrules: [{id: a, languages: [python], enabled: false, status: catalog, evaluator: integer_literal_equivalence}]",
        "version: 1\nrules: [{id: a, languages: [python], enabled: false, status: unknown}]",
        "version: 1\nrules: [{id: a, languages: [], enabled: true, status: implemented, evaluator: x}]",
        "version: 1\nrules: [{id: a, languages: [python], enabled: true, status: implemented, evaluator: unknown}]",
    ] {
        assert!(validate(invalid).is_err(), "incorrectly accepted: {invalid}");
    }
}
 

#[test]
fn duplicate_catalogue_ids_are_rejected_as_duplicates() {
    // Both entries must be otherwise valid: a bad evaluator must not mask
    // a missing duplicate-ID check.
    let raw = "version: 1\nrules: [{id: duplicate, languages: [python], enabled: true, status: implemented, evaluator: integer_literal_equivalence, group_kind: IGNORED_STYLE, risk: green, equivalence_kind: canonical_value_equivalence}, {id: duplicate, languages: [python], enabled: true, status: implemented, evaluator: integer_literal_equivalence, group_kind: IGNORED_STYLE, risk: green, equivalence_kind: canonical_value_equivalence}]";
    assert_eq!(validate(raw).unwrap_err(), "duplicate rule id: duplicate");
}

#[test]
fn pinned_json_schema_matches_catalogue_contracts() {
    let schema: Value = serde_json::from_str(SCHEMA).expect("valid invariance catalogue JSON schema");
    let required = schema["$defs"]["rule"]["required"].as_array().expect("base required fields");
    let implemented_required = schema["$defs"]["rule"]["allOf"][0]["then"]["required"]
        .as_array().expect("implemented required fields");

    for key in ["id", "languages", "status", "enabled", "explanation"] {
        assert!(required.contains(&Value::String(key.into())), "missing base field: {key}");
    }
    for key in ["evaluator", "group_kind", "risk", "equivalence_kind"] {
        assert!(!required.contains(&Value::String(key.into())), "{key} is optional for catalog-only rules");
        assert!(implemented_required.contains(&Value::String(key.into())), "{key} is required for implemented rules");
    }

    let evaluators: HashSet<&str> = schema["$defs"]["evaluator"]["enum"].as_array()
        .expect("evaluator enum").iter().map(|v| v.as_str().expect("evaluator string")).collect();
    let languages: HashSet<&str> = schema["$defs"]["language"]["enum"].as_array()
        .expect("language enum").iter().map(|v| v.as_str().expect("language string")).collect();
    for rule in validate(CATALOGUE).expect("pinned rules validate") {
        let id = rule["id"].as_str().expect("rule ID");
        for language in rule["languages"].as_array().expect("rule languages") {
            assert!(languages.contains(language.as_str().expect("language")), "{id}: language missing from schema");
        }
        if rule["status"] == "implemented" {
            let evaluator = rule["evaluator"].as_str().expect("implemented evaluator");
            assert!(evaluators.contains(evaluator), "{id}: evaluator missing from schema: {evaluator}");
            for key in ["group_kind", "risk", "equivalence_kind"] {
                assert!(rule.get(key).and_then(Value::as_str).is_some(), "{id}: missing {key}");
            }
        } else {
            assert_eq!(rule["status"], "catalog", "{id}: invalid status");
            assert_eq!(rule["enabled"], false, "{id}: catalog-only entry must be disabled");
            assert!(rule.get("evaluator").is_none_or(Value::is_null), "{id}: catalog-only entry has evaluator");
        }
    }
}
