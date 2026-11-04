use intentumdiff_rust_core::symbol_index::{resolve_references, SymbolDefinition, ReferenceUsage};
use serde_json::json;

#[test]
fn only_one_definition_resolves_and_inputs_remain_unchanged() {
    let def: SymbolDefinition = serde_json::from_value(json!({"qualified_name":"f", "file":"a.py","node_type":"function_definition","node_id":"def", "language":"python","start_line":0,"start_col":0,"end_line":1,"end_col":8})).unwrap();
    let reference: ReferenceUsage = serde_json::from_value(json!({"qualified_name":"f","file":"b.py","node_id":"call","reference_kind":"CALL","position":{"start_line":0,"start_col":0,"end_line":0,"end_col":3},"language":"python","enclosing_scope":null})).unwrap();
    for count in [0,1,2] {
        let definitions = vec![def.clone(); count];
        let result = resolve_references(&definitions, &[reference.clone()]);
        assert_eq!(result[0].resolved_definition.is_some(), count == 1);
        assert!(reference.resolved_definition.is_none());
    }
}

#[test]
fn source_judged_reference_corpus() {
    let cases: serde_json::Value = serde_json::from_str(include_str!("fixtures/reference_resolution.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let definitions: Vec<SymbolDefinition> = serde_json::from_value(case["definitions"].clone()).unwrap();
        let references: Vec<ReferenceUsage> = serde_json::from_value(case["references"].clone()).unwrap();
        let before = serde_json::to_value(&references).unwrap();
        let out = resolve_references(&definitions, &references);
        assert_eq!(serde_json::to_value(&references).unwrap(), before);
        assert_eq!(json!(out[0].resolved_definition.as_ref().map(|d| &d.file)), case["expected_file"]);
    }
}
