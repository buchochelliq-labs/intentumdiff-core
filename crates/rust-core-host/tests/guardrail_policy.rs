use intentumdiff_rust_core::guardrail_policy::parse_policy;

#[test]
fn normalizes_nested_and_root_policy_forms() {
    for source in [
        "guardrails:\n  protected:\n    - language: JSON\n      path: ' .Secrets. Token. '\n",
        "protected:\n  - language: JSON\n    path: ' .Secrets. Token. '\n",
    ] {
        let rules = parse_policy(source).unwrap();
        assert_eq!(rules[0]["language"], "json");
        assert_eq!(rules[0]["path"], "secrets.token");
        assert_eq!(rules[0]["severity"], "important");
        assert_eq!(rules[0]["rule_id"], "guardrail.1");
    }
}

#[test]
fn invalid_policy_never_becomes_no_rules() {
    for source in ["[]", "guardrails: []", "protected: nope", "protected: [42]",
                   "protected: [{language: python, path: key}]",
                   "protected: [{language: json, path: key, severity: ignore}]",
                   "protected: [{language: json, path: key, files: 42}]", "[broken"] {
        assert!(parse_policy(source).is_err(), "{source}");
    }
}

#[test]
fn empty_policy_is_successful_empty_rules() {
    for source in ["", "{}", "protected: null", "guardrails: {protected: []}"] {
        assert!(parse_policy(source).unwrap().is_empty());
    }
}

#[test]
fn policy_file_edit_is_owned_by_rust() {
    let request = serde_json::json!({
        "diff": {"old_filename":"intentumdiff.yaml", "new_filename":"intentumdiff.yaml",
                 "language":"yaml", "changes":[], "metadata":{}, "guardrail_violations":[]},
        "old_source":"old", "new_source":"new", "rules":[]
    });
    let result = intentumdiff_rust_core::guardrail_policy::apply_policy(&request).unwrap();
    assert_eq!(result["guardrail_violations"][0]["rule_id"], "intentumdiff.policy_file");
    assert_eq!(result["metadata"]["guardrails"]["immutable_count"], 1);
}

#[test]
fn missing_trees_cannot_pass_applicable_rules() {
    let rules = parse_policy("protected: [{language: json, path: secret}]").unwrap();
    let request = serde_json::json!({"diff":{"language":"json", "old_filename":"a.json",
        "new_filename":"a.json", "changes":[]}, "rules":rules});
    assert!(intentumdiff_rust_core::guardrail_policy::apply_policy(&request).is_err());
}

#[test]
fn source_judged_policy_corpus() {
    let cases: serde_json::Value = serde_json::from_str(include_str!("fixtures/guardrail_policy.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let result = parse_policy(case["source"].as_str().unwrap());
        if let Some(error) = case.get("error") {
            assert!(result.unwrap_err().contains(error.as_str().unwrap()), "{}", case["name"]);
        } else {
            assert_eq!(serde_json::json!(result.unwrap()), case["expected"], "{}", case["name"]);
        }
    }
}
