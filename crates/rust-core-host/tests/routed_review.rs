use intentumdiff_rust_core::routed_review::complete;
use serde_json::json;

#[test]
fn generic_review_discards_parser_groups_and_keeps_real_source_change() {
    let out = complete(&json!({"language":"generic", "old_source":"old\n", "new_source":"new\n",
        "old_filename":"a.txt", "new_filename":"a.txt", "finalized":{"changes":[],
        "change_groups":[{"kind":"MEANINGFUL_CHANGE", "raw_change_indices":[42]}], "is_style_only":true}})).unwrap();
    assert_eq!(out["has_semantic_changes"], true);
    assert_eq!(out["is_style_only"], false);
    assert!(!out["changes"].as_array().unwrap().is_empty());
    for group in out["change_groups"].as_array().unwrap() {
        for index in group["raw_change_indices"].as_array().unwrap() {
            assert!(index.as_u64().unwrap() < out["changes"].as_array().unwrap().len() as u64);
        }
    }
}

#[test]
fn suppressed_non_equivalent_sources_never_get_style_evidence() {
    let tree = |label| json!({"id":label,"node_type":"identifier", "label":label,"children":[],"structural_hash":label,"position":{"start_line":0,"start_col":0,"end_line":0,"end_col":1}});
    let out = complete(&json!({"language":"python", "old_source":"pass", "new_source":"print(1)",
        "old_filename":"a.py", "new_filename":"a.py", "old_tree":tree("pass"), "new_tree":tree("print"),
        "finalized":{"changes":[], "change_groups":[], "is_style_only":false}})).unwrap();
    assert_eq!(out["is_style_only"], false);
    assert!(out["metadata"].get("ignored_style_changes").is_none());
    assert_eq!(out["metadata"]["no_surviving_changes"], true);
}

#[test]
fn shared_source_judged_corpus() {
    let cases: serde_json::Value = serde_json::from_str(include_str!("fixtures/routed_review.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let out = complete(&case["request"]).unwrap();
        assert_eq!(out["has_semantic_changes"], case["semantic"]);
        assert_eq!(out["is_style_only"], case["style"]);
        assert_eq!(out["changes"].as_array().unwrap().len() as u64, case["changes"].as_u64().unwrap());
        if let Some(indices) = case.get("meaningful_indices") {
            let group = out["change_groups"].as_array().unwrap().iter().find(|g| g["kind"] == "MEANINGFUL_CHANGE").expect("surviving evidence");
            assert_eq!(&group["raw_change_indices"], indices);
            assert_eq!(group["old_labels"], json!(["2"]));
            assert_eq!(group["new_labels"], json!(["3"]));
        }
        if case["no_style_evidence"] == true { assert!(out["metadata"].get("ignored_style_changes").is_none()); }
    }
}
