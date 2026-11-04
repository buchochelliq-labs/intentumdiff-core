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
fn rejects_malformed_filename_filters_without_silently_weakening_guardrails() {
    // A YAML scalar in an array must not be serialized into a filename
    // pattern: a successful but ineffective policy is a fail-open result.
    for source in [
        "protected: [{language: json, path: version, files: [42]}]",
        "protected: [{language: json, path: version, files: [false]}]",
        "protected: [{language: json, path: version, files: [null]}]",
        "protected: [{language: json, path: version, files: ['']}]",
        "protected: [{language: json, path: version, files: ['  ']}]",
        "protected: [{language: json, path: version, files: ''}]",
        "protected: [{language: json, path: version, files: '  '}]",
        "protected: [{language: json, path: version, files: [config.json, 42]}]",
    ] {
        let error = parse_policy(source).unwrap_err();
        assert!(error.contains(".files"), "{source}: {error}");
    }

    let rules = parse_policy(
        "protected: [{language: json, path: version, files: [config.json, '*.yaml']}]"
    ).unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0]["files"], serde_json::json!(["config.json", "*.yaml"]));
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


#[test]
fn file_source_physical_basename_controls_filename_scoped_guardrails() {
    let rules = parse_policy("protected: [{language: json, path: version, files: [config.json]}]").unwrap();
    let mut request = serde_json::json!({
        "diff": {"language": "json", "old_filename": "alias.json",
                 "new_filename": "alias.json", "changes": []},
        "rules": rules,
        "guardrail_old_filename": "/before/config.json",
        "guardrail_new_filename": r"C:\protected\config.json"
    });
    // The rule applies even though the public label does not match. Without
    // trees, a matching rule MUST fail closed rather than return clean.
    assert!(intentumdiff_rust_core::guardrail_policy::apply_policy(&request)
        .unwrap_err().contains("requires parsed trees"));
    request.as_object_mut().unwrap().remove("guardrail_old_filename");
    request.as_object_mut().unwrap().remove("guardrail_new_filename");
    assert!(intentumdiff_rust_core::guardrail_policy::apply_policy(&request).is_ok());
}

#[test]
fn policy_file_alias_is_still_an_immutable_policy_edit() {
    let mut request = serde_json::json!({
        "diff": {"language": "yaml", "old_filename": "alias.yaml",
                 "new_filename": "alias.yaml", "changes": [],
                 "metadata": {}, "guardrail_violations": []},
        "rules": [], "old_source": "before", "new_source": "after",
        "guardrail_old_filename": "/old/other.yaml",
        "guardrail_new_filename": "/protected/intentumdiff.yaml"
    });
    let result = intentumdiff_rust_core::guardrail_policy::apply_policy(&request).unwrap();
    assert_eq!(result["guardrail_violations"][0]["rule_id"], "intentumdiff.policy_file");
    assert_eq!(result["guardrail_violations"][0]["file"], "alias.yaml");
    assert_eq!(result["old_filename"], "alias.yaml");
    assert_eq!(result["new_filename"], "alias.yaml");
    assert_eq!(result["metadata"]["guardrails"]["immutable_count"], 1);

    request.as_object_mut().unwrap().remove("guardrail_old_filename");
    request.as_object_mut().unwrap().remove("guardrail_new_filename");
    assert!(intentumdiff_rust_core::guardrail_policy::apply_policy(&request).unwrap()
        ["guardrail_violations"].as_array().unwrap().is_empty());
}

#[test]
fn invalid_physical_filename_override_is_not_ignored() {
    let mut request = serde_json::json!({
        "diff": {"language": "json", "old_filename": "alias.json",
                 "new_filename": "alias.json", "changes": []},
        "rules": []
    });
    for bad in [
        serde_json::Value::Null,
        serde_json::json!(42),
        serde_json::json!(""),
        serde_json::json!("/trailing/"),
    ] {
        request["guardrail_new_filename"] = bad;
        assert!(intentumdiff_rust_core::guardrail_policy::apply_policy(&request).is_err());
    }
}
