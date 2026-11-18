use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
};
pub type Result<T> = std::result::Result<T, String>;
pub fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
pub fn string<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key]
        .as_str()
        .ok_or_else(|| format!("missing string {key}"))
}
pub fn array<'a>(v: &'a Value, key: &str) -> Result<&'a Vec<Value>> {
    v[key]
        .as_array()
        .ok_or_else(|| format!("missing array {key}"))
}
pub fn local(root: &Path, relative: &str) -> Result<PathBuf> {
    let p = Path::new(relative);
    if p.as_os_str().is_empty() || p.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(format!("unsafe relative path {relative}"));
    }
    let canonical = root
        .join(p)
        .canonicalize()
        .map_err(|e| format!("{relative}: {e}"))?;
    if !canonical.starts_with(root.canonicalize().map_err(|e| e.to_string())?) {
        return Err(format!("path escapes fixture root: {relative}"));
    }
    Ok(canonical)
}
pub fn checked_bytes(root: &Path, path: &str, expected: &str) -> Result<Vec<u8>> {
    let bytes = fs::read(local(root, path)?).map_err(|e| e.to_string())?;
    if expected.len() != 64 || digest(&bytes) != expected {
        return Err(format!("sha256 mismatch: {path}"));
    }
    Ok(bytes)
}
pub fn unique<'a>(items: &'a [Value], key: &str) -> Result<BTreeSet<&'a str>> {
    let mut ids = BTreeSet::new();
    for item in items {
        let id = string(item, key)?;
        if !ids.insert(id) {
            return Err(format!("duplicate {key}: {id}"));
        }
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adversarial_constant_classifiers_fail_independent_oracles() {
        let semantic = json!({"has_semantic_changes":true,"is_style_only":false,"required_changes":[{"change_type":"literal_modified","old_label":"'a'","new_label":"'b'"}],"forbidden_change_types":["renamed"]});
        let style = json!({"has_semantic_changes":false,"is_style_only":true,"required_changes":[],"forbidden_change_types":["literal_modified"]});
        let always_style = json!({"has_semantic_changes":false,"is_style_only":true,"changes":[]});
        let always_semantic = json!({"has_semantic_changes":true,"is_style_only":false,"changes":[{"change_type":"literal_modified"}]});
        assert!(!evaluate(&semantic, &always_style).is_empty());
        assert!(!evaluate(&style, &always_semantic).is_empty());
        let wrong_literal = json!({"has_semantic_changes":true,"is_style_only":false,"changes":[{"change_type":"literal_modified","old_node":{"label":"'x'"},"new_node":{"label":"'y'"}}]});
        assert!(!evaluate(&semantic, &wrong_literal).is_empty());
    }
    #[test]
    fn rejects_traversal_and_duplicate_ids() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(local(tmp.path(), "../secret").is_err());
        assert!(unique(&[json!({"id":"a"}), json!({"id":"a"})], "id").is_err());
    }
    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escape() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("escape")).unwrap();
        assert!(local(root.path(), "escape").is_err());
    }
    #[test]
    fn validates_spans_and_group_indices() {
        let bad = json!({"changes":[{"old_node":{"position":{"start_line":0,"start_col":1,"end_line":0,"end_col":2},"children":[]}}],"change_groups":[{"raw_change_indices":[1]}]});
        assert!(validate_review(&bad, "é", "x").len() >= 2);
    }
}
pub fn evaluate(expected: &Value, review: &Value) -> Vec<String> {
    let mut errors = vec![];
    for key in ["has_semantic_changes", "is_style_only"] {
        if key == "is_style_only" && expected[key].is_null() {
            continue;
        }
        if expected[key] != review[key] {
            errors.push(format!(
                "{key}: expected {}, actual {}",
                expected[key], review[key]
            ));
        }
    }
    let changes = review["changes"].as_array().cloned().unwrap_or_default();
    for fact in expected["required_changes"]
        .as_array()
        .into_iter()
        .flatten()
    {
        if !changes.iter().any(|change| matches_fact(change, fact)) {
            errors.push(format!("missing required fact: {fact}"));
        }
    }
    for forbidden in expected["forbidden_change_types"]
        .as_array()
        .into_iter()
        .flatten()
    {
        if changes.iter().any(|c| c["change_type"] == *forbidden) {
            errors.push(format!("forbidden change: {forbidden}"));
        }
    }
    errors
}
fn matches_fact(change: &Value, fact: &Value) -> bool {
    fn descendant(node: &Value, label: &Value, position: &Value) -> bool {
        (node["label"] == *label && node["position"] == *position)
            || node["children"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|child| descendant(child, label, position))
    }
    if change["change_type"] != fact["change_type"] {
        return false;
    }
    if fact["match"] == "descendant_evidence" {
        descendant(
            &change["old_node"],
            &fact["old_label"],
            &fact["old_position"],
        ) && descendant(
            &change["new_node"],
            &fact["new_label"],
            &fact["new_position"],
        )
    } else {
        [("old_label", "old_node"), ("new_label", "new_node")]
            .iter()
            .all(|(label, node)| {
                fact.get(*label)
                    .is_none_or(|v| change[*node]["label"] == *v)
            })
    }
}
pub fn evaluate_case(case: &Value, review: &Value) -> Vec<String> {
    let mut errors = evaluate(&case["expected"], review);
    if case["old_sha256"].is_string()
        && case["old_sha256"] == case["new_sha256"]
        && review["changes"]
            .as_array()
            .is_some_and(|changes| !changes.is_empty())
    {
        errors.push("byte-identical input must produce no changes".into());
    }
    errors
}
pub fn validate_review(review: &Value, old: &str, new: &str) -> Vec<String> {
    fn node(n: &Value, source: &str, errors: &mut Vec<String>) {
        let p = &n["position"];
        let point = |line: &str, col: &str| -> Option<usize> {
            let line = p[line].as_u64()? as usize;
            let col = p[col].as_u64()? as usize;
            let mut start = 0;
            let mut lines = source.split('\n');
            let text = lines.nth(line)?;
            for text in source.split('\n').take(line) {
                start += text.len() + 1;
            }
            if col > text.len() || !text.is_char_boundary(col) {
                return None;
            }
            Some(start + col)
        };
        match (
            point("start_line", "start_col"),
            point("end_line", "end_col"),
        ) {
            (Some(a), Some(b)) if a <= b => {}
            _ => errors.push(format!("invalid UTF8 source span: {p}")),
        }
        for child in n["children"].as_array().into_iter().flatten() {
            node(child, source, errors);
        }
    }
    let mut errors = vec![];
    let changes = review["changes"].as_array().cloned().unwrap_or_default();
    for c in &changes {
        for (key, source) in [("old_node", old), ("new_node", new)] {
            if !c[key].is_null() {
                node(&c[key], source, &mut errors);
            }
        }
    }
    for g in review["change_groups"].as_array().into_iter().flatten() {
        for index in g["raw_change_indices"].as_array().into_iter().flatten() {
            if index.as_u64().is_none_or(|i| i as usize >= changes.len()) {
                errors.push(format!("invalid change group index: {index}"));
            }
        }
    }
    errors
}
pub fn validate_expectation(case: &Value) -> Result<()> {
    let e = &case["expected"];
    if e.is_null() {
        if case["required"] != false {
            return Err("required case has null expectation".into());
        }
        return Ok(());
    }
    if case["class"] == "external_seed_only" {
        return Err("seed-only case cannot carry admitted expectations".into());
    }
    let unspecified_identity_style = e.get("is_style_only").is_some_and(Value::is_null)
        && e["has_semantic_changes"] == false
        && case["old_sha256"].as_str().is_some_and(|s| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
        && case["old_sha256"] == case["new_sha256"];
    if !e["has_semantic_changes"].is_boolean()
        || (!e["is_style_only"].is_boolean() && !unspecified_identity_style)
    {
        return Err("expectation classification must be boolean".into());
    }
    if e["has_semantic_changes"] == true && e["is_style_only"] == true {
        return Err("contradictory semantic/style expectation".into());
    }
    let required = array(e, "required_changes")?;
    let forbidden = array(e, "forbidden_change_types")?;
    for forbidden_type in forbidden {
        if forbidden_type.as_str().is_none() {
            return Err("forbidden change types must be strings".into());
        }
    }
    for fact in required {
        if let Some(mode) = fact.get("match") {
            if mode != "descendant_evidence" {
                return Err("unknown required fact match mode".into());
            }
            string(fact, "old_label")?;
            string(fact, "new_label")?;
            for key in ["old_position", "new_position"] {
                let position = fact[key]
                    .as_object()
                    .ok_or_else(|| format!("missing {key}"))?;
                if position.len() != 4
                    || ["start_line", "start_col", "end_line", "end_col"]
                        .iter()
                        .any(|k| {
                            position
                                .get(*k)
                                .and_then(Value::as_u64)
                                .is_none_or(|v| v > u32::MAX as u64)
                        })
                {
                    return Err(format!("invalid expected {key}"));
                }
                if (
                    fact[key]["start_line"].as_u64(),
                    fact[key]["start_col"].as_u64(),
                ) > (
                    fact[key]["end_line"].as_u64(),
                    fact[key]["end_col"].as_u64(),
                ) {
                    return Err(format!("reversed expected {key}"));
                }
            }
        }
        for key in ["old_label", "new_label"] {
            if fact.get(key).is_some_and(|v| !v.is_string()) {
                return Err(format!("{key} must be a string"));
            }
        }
        let ty = string(fact, "change_type")?;
        if forbidden.iter().any(|v| v.as_str() == Some(ty)) {
            return Err(format!("required/forbidden contradiction: {ty}"));
        }
    }
    Ok(())
}
pub fn rate(passed: usize, total: usize) -> Value {
    if total == 0 {
        Value::Null
    } else {
        json!(passed as f64 / total as f64)
    }
}
/// The committed, attributed routing snapshot is independent of the supplied ledger/stage.
pub fn validate_inventory(inventory: &Value, ledger: &Value, stage: &Value) -> Result<()> {
    let expected = inventory["parsers"]
        .as_object()
        .ok_or("inventory missing parsers")?;
    let routed = stage["parsers"]
        .as_object()
        .ok_or("stage missing parsers")?;
    let entries = array(ledger, "languages")?;
    let expected_ids: BTreeSet<_> = expected.keys().map(String::as_str).collect();
    if expected_ids.len() != 74
        || unique(entries, "language_id")? != expected_ids
        || routed.keys().map(String::as_str).collect::<BTreeSet<_>>() != expected_ids
    {
        return Err("routed inventory ID set mismatch".into());
    }
    for entry in entries {
        for key in ["source_url", "evidence_class", "admission_status", "reason"] {
            if string(entry, key)?.trim().is_empty() {
                return Err(format!("empty ledger {key}"));
            }
        }
        let id = string(entry, "language_id")?;
        let component = string(entry, "component")?;
        if expected[id]["wasm"] != component
            || routed[id]["wasm"] != component
            || expected[id]["plugin_id"] != routed[id]["plugin_id"]
        {
            return Err(format!("routed inventory component mismatch: {id}"));
        }
    }
    if inventory["extension_index"] != stage["extension_index"] {
        return Err("extension routing differs from committed inventory".into());
    }
    Ok(())
}
/// Accuracy has an observation only when an admitted review completed successfully.
pub fn summarize(rows: &[&Value]) -> Value {
    let count = |status: &str| rows.iter().filter(|r| r["status"] == status).count();
    let passed = count("passed");
    let judged = passed + count("accuracy_failure");
    json!({"executed":rows.len(),"admitted":rows.iter().filter(|r|r["admitted"]==true).count(),"judged":judged,"runtime_errors":count("runtime_error"),"parse_errors":count("parse_error"),"invalid_results":count("invalid_result"),"unadmitted":count("unadmitted"),"accuracy_failures":count("accuracy_failure"),"passed":passed,"accuracy_rate":rate(passed,judged),"rate_reason":if judged==0{json!("no successfully judged admitted cases")}else{Value::Null}})
}
#[cfg(test)]
mod admission_regressions {
    use super::*;
    #[test]
    fn failed_runtime_has_no_accuracy_observation() {
        let row = json!({"admitted":true,"status":"runtime_error"});
        let s = summarize(&[&row]);
        assert_eq!(s["accuracy_rate"], Value::Null);
        assert_eq!(s["admitted"], 1);
        assert_eq!(s["judged"], 0);
        assert_eq!(s["runtime_errors"], 1);
    }
    #[test]
    fn invented_inventory_cannot_pass_mutually_consistent_ledger_and_stage() {
        let parsers: serde_json::Map<_, _> = (0..74)
            .map(|i| {
                (
                    format!("invented{i}"),
                    json!({"wasm":"missing.wasm","plugin_id":"fake"}),
                )
            })
            .collect();
        let stage = json!({"parsers":parsers,"extension_index":{}});
        let entries: Vec<_> = (0..74)
            .map(|i| json!({"language_id":format!("invented{i}"),"component":"missing.wasm"}))
            .collect();
        let ledger = json!({"languages":entries});
        let trusted = json!({"parsers":{"python":{"wasm":"python_parser.wasm","plugin_id":"python"}},"extension_index":{}});
        assert!(validate_inventory(&trusted, &ledger, &stage).is_err());
    }
    #[test]
    fn contradictory_required_and_forbidden_facts_rejected() {
        let c = json!({"class":"handwritten_regression","required":true,"expected":{"has_semantic_changes":true,"is_style_only":false,"required_changes":[{"change_type":"literal_modified"}],"forbidden_change_types":["literal_modified"]}});
        assert!(validate_expectation(&c).is_err());
    }
}
/// Prevent a truncated fixture set from silently shrinking the approved pilot.
pub fn validate_pilot(cases: &[Value]) -> Result<()> {
    for language in ["python", "javascript", "java", "go", "cpp", "rust"] {
        let mut task_sets = vec![];
        for family in ["repair", "identity"] {
            let selected: Vec<_> = cases
                .iter()
                .filter(|c| c["language_id"] == language && c["family"] == family)
                .collect();
            let tasks: BTreeSet<_> = selected
                .iter()
                .map(|c| string(c, "upstream_task_id"))
                .collect::<Result<_>>()?;
            if selected.len() != 12 || tasks.len() != 12 {
                return Err(format!(
                    "missing approved pilot cases: {language}/{family} needs 12 distinct tasks"
                ));
            }
            if family == "identity" && selected.iter().any(|c| c["old_sha256"] != c["new_sha256"]) {
                return Err(format!("identity source bytes differ: {language}"));
            }
            task_sets.push(tasks);
        }
        if task_sets[0] != task_sets[1] {
            return Err(format!("repair/identity task mismatch: {language}"));
        }
    }
    Ok(())
}
#[cfg(test)]
mod pilot_regressions {
    use super::*;
    #[test]
    fn empty_or_truncated_pilot_cannot_pass() {
        assert!(validate_pilot(&[]).is_err());
        assert!(validate_pilot(&[
            json!({"language_id":"python","family":"repair","upstream_task_id":"Python/0"})
        ])
        .is_err());
    }
}
pub fn validate_declared_counts(manifest: &Value) -> Result<()> {
    let cases = array(manifest, "cases")?;
    let counts = &manifest["declared_counts"];
    if counts["total"].as_u64() != Some(cases.len() as u64) {
        return Err("declared total differs from actual case count".into());
    }
    let mut actual = std::collections::BTreeMap::<&str, u64>::new();
    for case in cases {
        *actual.entry(string(case, "family")?).or_default() += 1;
    }
    let declared = counts["families"]
        .as_object()
        .ok_or("missing declared family counts")?;
    if declared.len() != actual.len()
        || actual
            .iter()
            .any(|(family, count)| declared.get(*family).and_then(Value::as_u64) != Some(*count))
    {
        return Err("declared family counts differ from actual cases".into());
    }
    for id in [
        "literal-c-ordinary",
        "literal-c-escape",
        "literal-c-position",
        "literal-cpp-ordinary",
        "literal-cpp-escape",
        "literal-cpp-position",
        "literal-cpp-prefix",
        "hep-python-00-formatting",
        "hep-python-00-rename",
        "literal-c-identity",
        "literal-c-formatting",
        "literal-cpp-identity",
        "literal-cpp-formatting",
    ] {
        if !cases
            .iter()
            .any(|c| c["id"] == id && c["required"] == true && !c["expected"].is_null())
        {
            return Err(format!("missing required admitted control: {id}"));
        }
    }
    Ok(())
}
#[cfg(test)]
mod counts_regressions {
    use super::*;
    #[test]
    fn inconsistent_or_removed_controls_fail() {
        let m = json!({"cases":[],"declared_counts":{"total":1,"families":{}}});
        assert!(validate_declared_counts(&m).is_err());
        let m = json!({"cases":[],"declared_counts":{"total":0,"families":{}}});
        assert!(validate_declared_counts(&m).is_err());
    }
}
/// Raw results are retained; repeated-run comparisons exclude only host measurements.
pub fn deterministic_review(review: &Value) -> Value {
    fn measurements(value: &mut Value) {
        match value {
            Value::Object(map) => {
                for key in [
                    "duration_ms",
                    "elapsed_ms",
                    "wasm_component_cache_hit",
                    "fuel_consumed",
                    "total_fuel_consumed",
                    "max_fuel_used_percent",
                ] {
                    map.remove(key);
                }
                for child in map.values_mut() {
                    measurements(child);
                }
            }
            Value::Array(values) => {
                for child in values {
                    measurements(child);
                }
            }
            _ => {}
        }
    }
    let mut stable = review.clone();
    if let Some(metadata) = stable.get_mut("metadata") {
        measurements(metadata);
    }
    stable
}
#[cfg(test)]
mod determinism_regressions {
    use super::*;
    #[test]
    fn excludes_measurements_but_keeps_semantic_changes() {
        let a = json!({"changes":[{"change_type":"MODIFICATION"}],"metadata":{"duration_ms":1.0,"nested":{"wasm_component_cache_hit":false},"engine":"native"}});
        let mut b = a.clone();
        b["metadata"]["duration_ms"] = json!(2.0);
        b["metadata"]["nested"]["wasm_component_cache_hit"] = json!(true);
        assert_eq!(deterministic_review(&a), deterministic_review(&b));
        b["changes"][0]["change_type"] = json!("RENAME");
        assert_ne!(deterministic_review(&a), deterministic_review(&b));
    }
}
pub fn read_json_hashed(path: &Path) -> Result<(Value, String)> {
    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    Ok((value, digest(&bytes)))
}
/// Character-literal controls require the complete atomic payload, exact bytes and changed hash.
pub fn validate_literal_facts(
    expected: &Value,
    review: &Value,
    old: &str,
    new: &str,
) -> Vec<String> {
    fn slice<'a>(node: &Value, source: &'a str) -> Option<&'a str> {
        let p = &node["position"];
        let offset = |line: &str, col: &str| {
            let l = p[line].as_u64()? as usize;
            let c = p[col].as_u64()? as usize;
            let text = source.split('\n').nth(l)?;
            if c > text.len() || !text.is_char_boundary(c) {
                return None;
            }
            Some(
                source
                    .split('\n')
                    .take(l)
                    .map(|s| s.len() + 1)
                    .sum::<usize>()
                    + c,
            )
        };
        source.get(offset("start_line", "start_col")?..offset("end_line", "end_col")?)
    }
    let mut errors = vec![];
    for fact in expected["required_changes"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let matching = review["changes"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|c| {
                c["change_type"] == fact["change_type"]
                    && c["old_node"]["label"] == fact["old_label"]
                    && c["new_node"]["label"] == fact["new_label"]
            });
        if let Some(change) = matching {
            for (side, source, label) in [
                ("old_node", old, "old_label"),
                ("new_node", new, "new_label"),
            ] {
                if slice(&change[side], source) != fact[label].as_str() {
                    errors.push(format!(
                        "literal {side} span does not retain exact source payload"
                    ));
                }
            }
            let a = change["old_node"]["structural_hash"].as_str();
            let b = change["new_node"]["structural_hash"].as_str();
            if a.is_none_or(str::is_empty) || b.is_none_or(str::is_empty) || a == b {
                errors.push("changed literal payload must change structural hash".into());
            }
        }
    }
    errors
}
#[cfg(test)]
mod literal_regressions {
    use super::*;
    #[test]
    fn label_alone_does_not_prove_literal_retention() {
        let e = json!({"required_changes":[{"change_type":"MODIFICATION","old_label":"'a'","new_label":"'b'"}]});
        let r = json!({"changes":[{"change_type":"MODIFICATION","old_node":{"label":"'a'","structural_hash":"same","position":{"start_line":0,"start_col":1,"end_line":0,"end_col":2}},"new_node":{"label":"'b'","structural_hash":"same","position":{"start_line":0,"start_col":1,"end_line":0,"end_col":2}}}]});
        assert_eq!(validate_literal_facts(&e, &r, "'a'", "'b'").len(), 3);
    }
}
#[cfg(test)]
mod identity_flag_regressions {
    use super::*;
    #[test]
    fn nullable_style_flag_only_for_proven_identity() {
        let mut case = json!({"class":"reviewed_external_derived","required":true,"old_sha256":"a".repeat(64),"new_sha256":"a".repeat(64),"expected":{"has_semantic_changes":false,"is_style_only":null,"required_changes":[],"forbidden_change_types":["MODIFICATION"]}});
        assert!(validate_expectation(&case).is_ok());
        let expected = &case["expected"];
        for style in [false, true] {
            let actual = json!({"has_semantic_changes":false,"is_style_only":style,"changes":[]});
            assert!(evaluate(expected, &actual).is_empty());
        }
        let wrong = json!({"has_semantic_changes":true,"is_style_only":true,"changes":[{"change_type":"MODIFICATION"}]});
        assert_eq!(evaluate(expected, &wrong).len(), 2);
        case["new_sha256"] = json!("b".repeat(64));
        assert!(validate_expectation(&case).is_err());
    }
}
#[cfg(test)]
mod descendant_evidence_regressions {
    use super::*;
    fn position(line: u32, col: u32) -> Value {
        json!({"start_line":line,"start_col":col,"end_line":line,"end_col":col+3})
    }
    fn fixture() -> (Value, Value) {
        let old = position(14, 24);
        let new = old.clone();
        let expected = json!({"has_semantic_changes":true,"is_style_only":false,"required_changes":[{"change_type":"MODIFICATION","match":"descendant_evidence","old_label":"'1'","new_label":"'0'","old_position":old,"new_position":new}],"forbidden_change_types":[]});
        let review = json!({"has_semantic_changes":true,"is_style_only":false,"changes":[{"change_type":"MODIFICATION","old_node":{"label":"if_statement","children":[{"label":"'1'","position":old}]},"new_node":{"label":"if_statement","children":[{"label":"'0'","position":new}]}}]});
        (expected, review)
    }
    #[test]
    fn exact_descendant_source_facts_satisfy_coarse_parent() {
        let (e, r) = fixture();
        assert!(evaluate(&e, &r).is_empty());
    }
    #[test]
    fn same_value_at_wrong_source_position_is_not_evidence() {
        let (e, mut r) = fixture();
        r["changes"][0]["new_node"]["children"][0]["position"] = position(16, 25);
        assert!(!evaluate(&e, &r).is_empty());
    }
    #[test]
    fn facts_in_different_changes_do_not_satisfy_one_parent_claim() {
        let (e, mut r) = fixture();
        let mut second = r["changes"][0].clone();
        r["changes"][0]["new_node"]["children"] = json!([]);
        second["old_node"]["children"] = json!([]);
        r["changes"].as_array_mut().unwrap().push(second);
        assert!(!evaluate(&e, &r).is_empty());
    }
    #[test]
    fn default_match_still_requires_root_labels() {
        let (mut e, r) = fixture();
        e["required_changes"][0]
            .as_object_mut()
            .unwrap()
            .remove("match");
        assert!(!evaluate(&e, &r).is_empty());
    }
    #[test]
    fn any_change_on_exact_identity_is_rejected() {
        let case = json!({"old_sha256":"a".repeat(64),"new_sha256":"a".repeat(64),"expected":{"has_semantic_changes":false,"is_style_only":null,"required_changes":[],"forbidden_change_types":[]}});
        for ty in ["MOVE", "REORDER", "STYLE_ONLY"] {
            let review = json!({"has_semantic_changes":false,"is_style_only":true,"changes":[{"change_type":ty}]});
            assert!(!evaluate_case(&case, &review).is_empty());
        }
    }
}
