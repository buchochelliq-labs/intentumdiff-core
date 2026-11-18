//! `keyed_data.number_exact_decimal_respelling` through real parser components and the native
//! live review (C ABI `live_diff_contents`), each staged alone with a test-only manifest the
//! way `tests_completeness.rs` stages its components.
//!
//! * The ADF component built from adf-parser PR #11 (`478706fc`, plugin-sdk #36 `01badea`):
//!   `number` labels are the exact source token and the hash is the canonical decimal. It is
//!   not registry-pinned yet, so that test is ignored unless
//!   `INTENTUMDIFF_TEST_ADF_SOURCE_LABEL_WASM` names the built component
//!   (`cargo build --release --target wasm32-wasip2` in that checkout).
//! * The registry-pinned ADF and JSON components from `INTENTUMDIFF_TEST_WASM_DIR`.
use serde_json::{json, Value};

const RULE: &str = crate::keyed_data::NUMBER_RESPELLING_RULE;

fn staged(component: &std::path::Path, language: &str, wasm: &str) -> tempfile::TempDir {
    assert!(component.is_file(), "component absent: {}", component.display());
    let fixture = tempfile::tempdir().expect("isolated parser staging");
    std::fs::copy(component, fixture.path().join(wasm)).expect("copy exact component bytes");
    std::fs::write(
        fixture.path().join("parser_manifest.json"),
        json!({"schema_version": 1,
            "parsers": {language: {"plugin_id": language, "wasm": wasm, "extensions": [".json"]}},
            "extension_index": {".json": language}})
        .to_string(),
    )
    .expect("test-only manifest");
    fixture
}

fn pinned(wasm: &str) -> std::path::PathBuf {
    std::path::Path::new(&std::env::var("INTENTUMDIFF_TEST_WASM_DIR")
        .expect("Tier-C tests need INTENTUMDIFF_TEST_WASM_DIR (checksum-verified parser components)"))
        .join(wasm)
}

fn review(parsers: &tempfile::TempDir, language: &str, old: &str, new: &str) -> Value {
    let config = json!({"language_hint": language, "strict_plugins": true}).to_string();
    let raw = crate::live_server::live_diff_contents_impl(env!("CARGO_MANIFEST_DIR"), "pipeline.json", old, new,
        &config, parsers.path().to_str().expect("UTF-8 fixture path"))
    .expect("native review");
    let response: Value = serde_json::from_str(&raw).expect("valid native response");
    let diff = response.get("diff").unwrap_or_else(|| panic!("review declined: {response:#}")).clone();
    assert_eq!(diff["is_fallback"], false, "{diff:#}");
    assert_eq!(diff["parse_errors"], json!([]), "{diff:#}");
    diff
}

/// An ADF pipeline with two numeric settings: an activity retry count and a parameter default.
fn pipeline(retry: &str, ratio: &str) -> String {
    format!(r#"{{
  "name": "P",
  "properties": {{
    "parameters": {{
      "ratio": {{"type": "float", "defaultValue": {ratio}}}
    }},
    "activities": [
      {{"name": "A", "type": "WebActivity", "policy": {{"retry": {retry}}}}}
    ]
  }}
}}
"#)
}

fn shown(diff: &Value) -> Vec<String> {
    let mut labels = Vec::new();
    for change in diff["changes"].as_array().unwrap() {
        for side in ["old_node", "new_node"] {
            if let Some(label) = change[side]["label"].as_str() { labels.push(label.to_owned()); }
        }
    }
    labels
}

fn rule_groups(diff: &Value) -> Vec<&Value> {
    diff["change_groups"].as_array().unwrap().iter().filter(|g| g["rule_id"] == RULE).collect()
}

fn assert_equivalent(diff: &Value, old: &str, new: &str) {
    assert_eq!(diff["completeness"], "equivalent", "{old}->{new}: {diff:#}");
    assert_eq!(diff["changes"], json!([]), "{diff:#}");
    assert_eq!(diff["has_semantic_changes"], false);
    assert_eq!(diff["is_style_only"], false);
    assert_eq!(diff["metadata"]["equivalence_proof"], RULE, "{diff:#}");
    assert!(diff["incomplete"].is_null(), "{diff:#}");
    let groups = rule_groups(diff);
    assert_eq!(groups.len(), 1, "{diff:#}");
    assert_eq!((&groups[0]["old_labels"], &groups[0]["new_labels"]), (&json!([old]), &json!([new])),
        "evidence shows the spellings as written");
}

fn assert_meaningful(diff: &Value, labels: &[&str]) {
    assert_eq!(diff["completeness"], "complete", "{diff:#}");
    assert_eq!(diff["has_semantic_changes"], true, "{diff:#}");
    let shown = shown(diff);
    for label in labels {
        assert!(shown.iter().any(|s| s == label), "{label} missing from {shown:?}: {diff:#}");
    }
    assert!(diff["metadata"].get("equivalence_proof").is_none(), "{diff:#}");
}

#[test]
#[ignore = "needs the adf-parser PR #11 component: set INTENTUMDIFF_TEST_ADF_SOURCE_LABEL_WASM"]
fn adf_source_label_component_respellings_are_equivalent_and_values_stay_meaningful() {
    let component = std::env::var("INTENTUMDIFF_TEST_ADF_SOURCE_LABEL_WASM")
        .expect("INTENTUMDIFF_TEST_ADF_SOURCE_LABEL_WASM names the adf-parser PR #11 build");
    let parsers = staged(std::path::Path::new(&component), "adf", "adf_parser.wasm");
    let base = pipeline("9", "1");
    // Respelling-only: equivalent with the proof id (core #252), never a meaningful change.
    for (old, new, edited) in [
        ("9", "9.0", pipeline("9.0", "1")),
        ("9", "9E0", pipeline("9E0", "1")),
        ("1", "1e0", pipeline("9", "1e0")),
    ] {
        assert_equivalent(&review(&parsers, "adf", &base, &edited), old, new);
    }
    assert_equivalent(&review(&parsers, "adf", &pipeline("1000", "1"), &pipeline("1e3", "1")), "1000", "1e3");
    // Real value changes: meaningful, labelled with the source text.
    for (edited, labels) in [
        (pipeline("10", "1"), &["9", "10"][..]),
        (pipeline("-9", "1"), &["9", "-9"][..]),
        (pipeline("\"9\"", "1"), &["9"][..]),
    ] {
        let diff = review(&parsers, "adf", &base, &edited);
        assert_meaningful(&diff, labels);
        assert!(rule_groups(&diff).is_empty(), "{diff:#}");
    }
    let diff = review(&parsers, "adf", &pipeline("9", "1.0"), &pipeline("9", "1.01"));
    assert_meaningful(&diff, &["1.0", "1.01"]);
    // Mixed: only the real change is shown; the respelling is evidence.
    let diff = review(&parsers, "adf", &base, &pipeline("9.0", "2"));
    assert_meaningful(&diff, &["1", "2"]);
    assert!(!shown(&diff).iter().any(|s| s == "9" || s == "9.0"), "{diff:#}");
    assert_eq!(rule_groups(&diff).len(), 1, "{diff:#}");
    // Spellings the rule cannot prove are never hidden (the component itself rejects
    // non-JSON spellings such as `+1` or `0x10`, so only grammar-valid ones reach the core).
    let diff = review(&parsers, "adf", &base, &pipeline("9", "-0"));
    assert_meaningful(&diff, &["-0"]);
}

/// The registry-pinned ADF component (raw labels and raw hashes, before plugin-sdk #31/#36)
/// keeps today's results: a respelling is not matched, so the rule never fires, and every
/// real change is shown as before.
#[test]
#[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged parser wasm (set INTENTUMDIFF_TEST_WASM_DIR)")]
fn pinned_adf_component_keeps_its_results() {
    let parsers = staged(&pinned("adf_parser.wasm"), "adf", "adf_parser.wasm");
    let base = pipeline("9", "1");
    for (edited, labels) in [
        (pipeline("9.0", "1"), &["9", "9.0"][..]),
        (pipeline("10", "1"), &["9", "10"][..]),
        (pipeline("9.0", "2"), &["9", "9.0", "1", "2"][..]),
    ] {
        let diff = review(&parsers, "adf", &base, &edited);
        assert_meaningful(&diff, labels);
        assert!(rule_groups(&diff).is_empty(), "{diff:#}");
        let kinds: Vec<_> = diff["changes"].as_array().unwrap().iter()
            .map(|c| c["change_type"].as_str().unwrap().to_owned()).collect();
        assert!(kinds.iter().all(|k| k == "DELETION" || k == "ADDITION"), "{kinds:?}: {diff:#}");
    }
    let unchanged = review(&parsers, "adf", &base, &base);
    assert_eq!(unchanged["completeness"], "complete");
    assert_eq!(unchanged["changes"], json!([]));
}

/// The pinned JSON component: same rule, same proof; layout may change with the respelling.
#[test]
#[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged parser wasm (set INTENTUMDIFF_TEST_WASM_DIR)")]
fn pinned_json_component_number_respellings() {
    let parsers = staged(&pinned("json_parser.wasm"), "json", "json_parser.wasm");
    assert_equivalent(&review(&parsers, "json", "{\"a\": 1E-2, \"b\": \"x\"}\n", "{\"a\": 0.01, \"b\": \"x\"}\n"), "1E-2", "0.01");
    assert_equivalent(&review(&parsers, "json", "[20e1]\n", "[\n  200.0\n]\n"), "20e1", "200.0");
    let diff = review(&parsers, "json", "{\"a\": 1E-2, \"b\": \"x\"}\n", "{\"a\": 0.01, \"b\": \"y\"}\n");
    assert_meaningful(&diff, &["x", "y"]);
    assert!(!shown(&diff).iter().any(|s| s == "1E-2"), "{diff:#}");
    for (old, new) in [("[9]\n", "[10]\n"), ("[1.0]\n", "[1.01]\n"), ("[-0]\n", "[0]\n"), ("[1]\n", "[\"1\"]\n")] {
        let diff = review(&parsers, "json", old, new);
        assert_eq!(diff["completeness"], "complete", "{old}->{new}: {diff:#}");
        assert_eq!(diff["has_semantic_changes"], true, "{diff:#}");
        assert!(rule_groups(&diff).is_empty(), "{diff:#}");
    }
}
