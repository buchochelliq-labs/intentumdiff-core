//! Production-path regressions for full-source evidence and final-array ownership.
#![cfg(feature = "tier-c-wasm")]
use serde_json::{json, Value};

fn case(family: &str, id: &str) -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/language_assessment").join(format!("{family}.json"));
    let corpus: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    corpus["cases"].as_array().unwrap().iter().find(|c| c["id"] == id).unwrap().clone()
}

fn review(case: &Value) -> Value {
    let staged = std::path::PathBuf::from(std::env::var("INTENTUMDIFF_TEST_WASM_DIR").expect("verified parser components"));
    let language = case["language_id"].as_str().unwrap();
    let filename = case["filename"].as_str().unwrap();
    let extension = format!(".{}", filename.rsplit('.').next().unwrap());
    let wasm = match language { "tsx" => "js_ts_parser.wasm", "css" => "css_parser.wasm", "python" => "python_parser.wasm", _ => panic!("unsupported test route") };
    let parsers = tempfile::tempdir().unwrap();
    std::fs::copy(staged.join(wasm), parsers.path().join(wasm)).unwrap();
    let mut manifest = json!({"parsers":{}, "extension_index":{}});
    manifest["parsers"][language] = json!({"plugin_id":language,"wasm":wasm,"extensions":[extension]});
    manifest["extension_index"][&extension] = json!(language);
    std::fs::write(parsers.path().join("parser_manifest.json"), manifest.to_string()).unwrap();
    let response = intentumdiff_rust_core::live_server::live_diff_contents_impl(
        env!("CARGO_MANIFEST_DIR"), filename, case["old"].as_str().unwrap(), case["new"].as_str().unwrap(),
        "{}", &parsers.path().to_string_lossy()).unwrap();
    let response: Value = serde_json::from_str(&response).unwrap();
    let diff = response.get("diff").expect("native review succeeded").clone();
    assert_eq!(diff["is_fallback"], false, "{diff:#}");
    assert_eq!(diff["parse_errors"], json!([]), "{diff:#}");
    diff
}

fn assert_final_references(diff: &Value) {
    let changes = diff["changes"].as_array().unwrap();
    for group in diff["change_groups"].as_array().unwrap() {
        for index in group["raw_change_indices"].as_array().unwrap() {
            assert!(index.as_u64().is_some_and(|i| i < changes.len() as u64), "invalid final reference: {diff:#}");
        }
    }
}

#[test]
fn assessed_tsx_quote_evidence_uses_full_crlf_source_offsets() {
    let case = case("web", "web-tsx-external-module-quote-spelling");
    let diff = review(&case);
    assert_eq!(diff["has_semantic_changes"], false);
    let group = diff["change_groups"].as_array().unwrap().iter()
        .find(|g| g["rule_id"] == "core.string_literal.decoded_value.safe").unwrap();
    assert_eq!(group["metadata"]["old_span"], json!([144,151]), "{diff:#}");
    assert_eq!(group["metadata"]["new_span"], json!([144,151]), "{diff:#}");
    for side in ["old", "new"] {
        let span = &group["metadata"][format!("{side}_span")];
        let raw = &case[side].as_str().unwrap()[span[0].as_u64().unwrap() as usize..span[1].as_u64().unwrap() as usize];
        assert_eq!(raw, group["metadata"][format!("{side}_label")].as_str().unwrap());
    }
    assert!(!group["old_node_ids"].as_array().unwrap().is_empty());
    assert_final_references(&diff);
}

#[test]
fn assessed_css_keyword_equivalence_has_evidence_without_discarded_change_indices() {
    let diff = review(&case("web", "web-css-external-keyword-case"));
    assert_eq!(diff["has_semantic_changes"], false);
    assert_eq!(diff["changes"], json!([]));
    let group = diff["change_groups"].as_array().unwrap().iter()
        .find(|g| g["rule_id"] == "css.color.canonical_equivalence").unwrap();
    assert_eq!(group["metadata"]["old_span"], json!([17,22]));
    assert_eq!(group["metadata"]["canonical_old"], "srgb(0,0,0,1)");
    assert!(!group["old_node_ids"].as_array().unwrap().is_empty());
    assert_final_references(&diff);
}

#[test]
fn assessed_python_bytes_result_has_no_invalid_final_references() {
    // Semantic str/bytes classification is tracked separately by core#243.
    // This regression only requires honest references for whatever changes survive.
    let diff = review(&case("dynamic", "dynamic/python/language-feature"));
    assert!(!diff["change_groups"].as_array().unwrap().is_empty());
    assert_final_references(&diff);
}

#[test]
fn python_quote_suppression_before_surviving_edit_preserves_final_ownership() {
    let diff = review(&json!({"language_id":"python","filename":"mixed.py",
        "old":"value = 'same'\nanswer = 1\n", "new":"value = \"same\"\nanswer = 2\n"}));
    let changes = diff["changes"].as_array().unwrap();
    assert_eq!(changes.len(), 1, "{diff:#}");
    assert_eq!(changes[0]["old_node"]["label"], "1");
    assert_eq!(changes[0]["new_node"]["label"], "2");
    let groups = diff["change_groups"].as_array().unwrap();
    let ignored = groups.iter().find(|g| g["rule_id"] == "core.string_literal.decoded_value.safe").unwrap();
    assert_eq!(ignored["raw_change_indices"], json!([]));
    assert!(!ignored["old_node_ids"].as_array().unwrap().is_empty());
    let meaningful = groups.iter().find(|g| g["kind"] == "MEANINGFUL_CHANGE").unwrap();
    assert_eq!(meaningful["raw_change_indices"], json!([0]));
    assert!(meaningful["old_node_ids"].as_array().unwrap().contains(&changes[0]["old_node"]["id"]));
    assert!(meaningful["new_node_ids"].as_array().unwrap().contains(&changes[0]["new_node"]["id"]));
    assert_final_references(&diff);
}

#[test]
fn python_multiline_raw_to_normal_literal_remains_semantic() {
    let diff = review(&json!({"language_id":"python","filename":"multiline.py",
        "old":"value = r\"\"\"a\\nb\nnext\"\"\"\n", "new":"value = \"\"\"a\\nb\nnext\"\"\"\n"}));
    assert_eq!(diff["has_semantic_changes"], true, "{diff:#}");
    assert_eq!(diff["is_style_only"], false, "{diff:#}");
    assert!(!diff["changes"].as_array().unwrap().is_empty(), "{diff:#}");
    assert!(!diff["change_groups"].as_array().unwrap().iter()
        .any(|g| g["rule_id"] == "core.string_literal.decoded_value.safe"), "{diff:#}");
    assert_final_references(&diff);
}
