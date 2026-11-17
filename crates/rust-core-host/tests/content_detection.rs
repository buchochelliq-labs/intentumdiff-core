use intentumdiff_rust_core::content_detection::{finish, plan, Request};
use serde_json::{json, Value};

#[test]
fn source_expected_content_corpus() {
    let cases: Vec<Value> = serde_json::from_str(include_str!("fixtures/content_detection.json")).unwrap();
    for case in cases {
        let request = serde_json::from_value(case["request"].clone()).unwrap();
        let observations: Vec<_> = serde_json::from_value(case["observations"].clone()).unwrap();
        let actual = finish(&request, &observations);
        if let Some(error) = case["error"].as_str() {
            assert!(actual.unwrap_err().contains(error), "{}", case["name"]);
        } else {
            assert_eq!(serde_json::to_value(actual.unwrap()).unwrap(), case["expected"], "{}", case["name"]);
        }
    }
}

#[test]
fn plans_filter_and_bound_utf8() {
    for suffix in ["é", "😀"] {
        let request: Request = serde_json::from_value(json!({"entries":[],"content":format!("{}{suffix}tail", "a".repeat(4095))})).unwrap();
        assert_eq!(plan(&request).unwrap().sample, "a".repeat(4095));
    }
    let request: Request = serde_json::from_value(json!({"entries":[
        {"plugin_id":"py","grammar_id":"py-grammar","languages":["python"],"priority":1},
        {"plugin_id":"rb","grammar_id":"rb-grammar","languages":["ruby"],"priority":100}
    ],"allowed_plugins":["py-grammar"],"candidates":["python"],"plugin_id":"py"})).unwrap();
    assert_eq!(plan(&request).unwrap().indices, vec![0]);
}

/// Allowlist identifiers (python #311): grammar, plugin and language ids all select a parser;
/// `None` = every parser, `[]` = none; unknown ids select nothing.
#[test]
fn allowlist_accepts_grammar_plugin_and_language_identifiers() {
    let entries = json!([
        {"plugin_id":"acme/py-plugin","grammar_id":"py-grammar","languages":["python"],"priority":1},
        {"plugin_id":"rb","grammar_id":"rb-grammar","languages":["ruby"],"priority":100}
    ]);
    for (allow, expected) in [
        (json!(null), vec![1, 0]), (json!([]), vec![]), (json!(["py-grammar"]), vec![0]),
        (json!(["acme/py-plugin"]), vec![0]), (json!(["python"]), vec![0]),
        (json!(["ruby", "py-grammar"]), vec![1, 0]), (json!(["unknown"]), vec![]), (json!(["Python"]), vec![]),
    ] {
        let request: Request = serde_json::from_value(json!({"entries":entries,"allowed_plugins":allow})).unwrap();
        assert_eq!(plan(&request).unwrap().indices, expected, "{allow}");
    }
}
