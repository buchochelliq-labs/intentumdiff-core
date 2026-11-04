//! Contract-only tests for the pinned invariance catalogue.
//! These intentionally do not change the production Rust evaluator dispatch.
//! Activating catalogue-driven suppression requires native/Python golden-output gates.
use serde_json::Value;
use std::collections::HashSet;

const CATALOGUE: &str = include_str!("../invariances/rules.yaml");
const SCHEMA: &str = include_str!("../invariances/rules.schema.json");

fn validate(raw: &str) -> Result<Vec<Value>, String> {
    let doc: Value = serde_yaml::from_str(raw).map_err(|e| e.to_string())?;
    let schema: Value = serde_json::from_str(SCHEMA).map_err(|e| e.to_string())?;
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
        let mut seen_languages = HashSet::new();
        for language in languages {
            if !schema["$defs"]["language"]["enum"].as_array()
                .ok_or("schema language enum missing")?.contains(language)
                || !seen_languages.insert(language.as_str().unwrap())
            {
                return Err(format!("{id}: unknown or duplicate language: {language}"));
            }
        }
        if rule.get("explanation").and_then(Value::as_str).is_none_or(str::is_empty) {
            return Err(format!("{id}: missing explanation"));
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
            let _evaluator = rule.get("evaluator").and_then(Value::as_str)
                .ok_or(format!("{id}: implemented rule missing evaluator"))?;
            for (key, definition) in [
                ("evaluator", "evaluator"),
                ("group_kind", "changeGroupKind"),
                ("risk", "risk"),
                ("equivalence_kind", "equivalenceKind"),
            ] {
                let value = rule.get(key).ok_or(format!("{id}: missing {key}"))?;
                let allowed = schema["$defs"][definition]["enum"].as_array()
                    .ok_or(format!("schema {definition} enum missing"))?;
                if !allowed.contains(value) {
                    return Err(format!("{id}: invalid {key}: {value}"));
                }
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
        "version: 1\nrules: [{id: a, explanation: valid, languages: [python], enabled: true, status: catalog}]",
        "version: 1\nrules: [{id: a, explanation: valid, languages: [python], enabled: false, status: catalog, evaluator: integer_literal_equivalence}]",
        "version: 1\nrules: [{id: a, explanation: valid, languages: [python], enabled: false, status: unknown}]",
        "version: 1\nrules: [{id: a, explanation: valid, languages: [], enabled: true, status: implemented, evaluator: x}]",
        "version: 1\nrules: [{id: a, explanation: valid, languages: [python], enabled: true, status: implemented, evaluator: unknown}]",
    ] {
        assert!(validate(invalid).is_err(), "incorrectly accepted: {invalid}");
    }
}
 

#[test]
fn duplicate_catalogue_ids_are_rejected_as_duplicates() {
    // Both entries must be otherwise valid: a bad evaluator must not mask
    // a missing duplicate-ID check.
    let raw = "version: 1\nrules: [{id: duplicate, explanation: valid, languages: [python], enabled: true, status: implemented, evaluator: integer_literal_equivalence, group_kind: IGNORED_STYLE, risk: green, equivalence_kind: canonical_value_equivalence}, {id: duplicate, explanation: valid, languages: [python], enabled: true, status: implemented, evaluator: integer_literal_equivalence, group_kind: IGNORED_STYLE, risk: green, equivalence_kind: canonical_value_equivalence}]";
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

#[test]
fn catalogue_rejects_unknown_or_duplicate_languages() {
    for languages in [serde_json::json!(["not-a-language"]), serde_json::json!(["python", "python"])] {
        let mut doc: Value = serde_yaml::from_str(CATALOGUE).unwrap();
        doc["rules"][0]["languages"] = languages;
        assert!(validate(&serde_yaml::to_string(&doc).unwrap()).is_err());
    }
}

#[test]
fn implemented_metadata_is_required_and_constrained() {
    for key in ["evaluator", "group_kind", "risk", "equivalence_kind", "explanation"] {
        for invalid in [None, Some(Value::Null), Some(serde_json::json!(42)), Some(serde_json::json!(""))] {
            let mut doc: Value = serde_yaml::from_str(CATALOGUE).unwrap();
            let rule = doc["rules"][0].as_object_mut().unwrap();
            match invalid {
                Some(value) => { rule.insert(key.into(), value); }
                None => { rule.remove(key); }
            }
            assert!(validate(&serde_yaml::to_string(&doc).unwrap()).is_err(), "accepted malformed {key}");
        }
    }
    for key in ["evaluator", "group_kind", "risk", "equivalence_kind"] {
        let mut doc: Value = serde_yaml::from_str(CATALOGUE).unwrap();
        doc["rules"][0][key] = serde_json::json!("not-in-schema");
        assert!(validate(&serde_yaml::to_string(&doc).unwrap()).is_err(), "accepted unknown {key}");
    }
}

#[test]
fn shared_style_evidence_covers_every_catalogued_language() {
    // rust_build_style_only_evidence emits shared source evidence without a
    // language restriction once the caller has proved tree equivalence.
    let rules = validate(CATALOGUE).unwrap();
    let shared = rules.iter().find(|rule|
        rule["id"] == "generic.style_only_shortcut.source_equivalence").unwrap();
    let schema: Value = serde_json::from_str(SCHEMA).unwrap();
    let actual: HashSet<&str> = shared["languages"].as_array().unwrap().iter()
        .map(|value| value.as_str().unwrap()).collect();
    let expected: HashSet<&str> = schema["$defs"]["language"]["enum"].as_array().unwrap().iter()
        .map(|value| value.as_str().unwrap()).collect();
    assert_eq!(actual, expected);
}

#[test]
fn unimplemented_f64_evaluator_is_not_advertised_as_executable() {
    // Native literal dispatch currently runs integer/string equivalence only.
    // Parser-normalized spellings are not proof of this distinct f64 evaluator.
    let rules = validate(CATALOGUE).unwrap();
    let numeric = rules.iter().find(|rule|
        rule["id"] == "core.number_literal.canonical_value.safe").unwrap();
    assert_eq!(numeric["status"], "catalog");
    assert_eq!(numeric["enabled"], false);
    assert!(numeric.get("evaluator").is_none_or(Value::is_null));
}
