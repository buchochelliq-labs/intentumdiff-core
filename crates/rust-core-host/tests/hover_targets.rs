#[test]
fn source_judged_hover_targets() {
    let cases: serde_json::Value = serde_json::from_str(include_str!("fixtures/hover_targets.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let actual = intentumdiff_rust_core::lsp_enrich::collect_hover_targets(&case["tree"]);
        assert_eq!(serde_json::to_value(actual).unwrap(), case["expected"]);
        let protocol = intentumdiff_rust_core::lsp_enrich::collect_utf16_hover_targets(&case["tree"], case["source"].as_str().unwrap()).unwrap();
        assert_eq!(serde_json::to_value(protocol).unwrap(), case["expected_utf16"]);
    }
}

#[test]
fn invalid_positions_cannot_query_a_different_symbol() {
    use serde_json::json;
    for (line, col) in [(0u64,1u64),(0,99),(1,0),(0,4294967296)] {
        let tree = json!({"id":"x","node_type":"variable_name","children":[],"position":{"start_line":line,"start_col":col}});
        assert!(intentumdiff_rust_core::lsp_enrich::collect_utf16_hover_targets(&tree,"é").is_err());
    }
}
