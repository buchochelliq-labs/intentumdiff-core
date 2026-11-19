//! Incomplete input mid-edit (core #314): a parser component that cannot read a side returns its
//! in-band `{"error": ...}` envelope. The native review must decline with a typed
//! `fallback_code: "plugin_parse_error"` and the parser's own message, never a missing-field
//! decoding error, and the source fallback must keep the changed source as a parse error.
use serde_json::{json, Value};

fn review(file: &str, wasm: &str, language: &str, extension: &str, old: &str, new: &str) -> Value {
    let staged_dir = std::env::var("INTENTUMDIFF_TEST_WASM_DIR")
        .expect("full CI must provision trusted parser Wasm");
    let staged = std::path::Path::new(&staged_dir).join(wasm);
    assert!(staged.is_file(), "registry-pinned component absent: {}", staged.display());
    let fixture = tempfile::tempdir().expect("isolated parser staging");
    std::fs::copy(staged, fixture.path().join(wasm)).expect("copy exact verified parser bytes");
    std::fs::write(
        fixture.path().join("parser_manifest.json"),
        json!({
            "schema_version": 1,
            "parsers": {language: {"plugin_id": language, "wasm": wasm, "extensions": [extension]}},
            "extension_index": {extension: language}
        }).to_string(),
    ).expect("test-only manifest for an already-pinned component");
    let raw = crate::live_server::live_diff_contents_impl(
        env!("CARGO_MANIFEST_DIR"), file, old, new, "{}",
        fixture.path().to_str().expect("UTF-8 fixture path"),
    ).expect("native review returns an envelope, not an error");
    serde_json::from_str(&raw).expect("valid native response")
}

#[test]
#[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged parser wasm (set INTENTUMDIFF_TEST_WASM_DIR or enable tier-c-wasm)")]
fn half_typed_yaml_and_xml_decline_with_a_typed_parser_reason() {
    for (file, wasm, language, extension, old, new) in [
        ("config.yaml", "yaml_parser.wasm", "yaml", ".yaml", "a:\n  b: 1\n", "a:\n  b: [1, \n"),
        ("data.xml", "xml_parser.wasm", "xml", ".xml", "<a>\n  <b>1</b>\n</a>\n", "<a>\n  <b>1</b\n"),
    ] {
        let response = review(file, wasm, language, extension, old, new);
        if response.get("diff").is_some() {
            // A component that learns to return a partial tree with error nodes is also fine,
            // provided the review is explicit about it.
            let diff = &response["diff"];
            assert!(diff["is_fallback"] == true || !diff["parse_errors"].as_array().unwrap().is_empty(),
                "{file}: a half-typed file must not be a clean review: {response:#}");
            continue;
        }
        assert_eq!(response["fallback_code"], "plugin_parse_error", "{file}: {response:#}");
        let reason = response["fallback"].as_str().unwrap();
        assert!(reason.contains("plugin returned error (new)"), "{file}: {reason}");
        assert!(!reason.contains("missing field"), "{file}: untyped decoding error leaked: {reason}");
    }
}

#[test]
fn only_parser_errors_carry_the_plugin_parse_error_code() {
    let typed: Value = serde_json::from_str(&crate::live_server::live_fallback_response(
        "config.yaml: plugin returned error (new): Empty semantic tree")).unwrap();
    assert_eq!(typed["fallback_code"], "plugin_parse_error");
    let other: Value = serde_json::from_str(&crate::live_server::live_fallback_response(
        "no bundled parser for notes.xyz")).unwrap();
    assert!(other.get("fallback_code").is_none(), "{other}");
}

#[test]
fn plugin_parse_error_source_fallback_keeps_the_change_as_a_parse_error() {
    let diff = crate::source_fallback::source_fallback_diff_impl(
        "a:\n  b: 1\n", "a:\n  b: [1, \n", "config.yaml", "config.yaml", "yaml", "plugin_parse_error",
    ).unwrap();
    assert_eq!(diff["is_fallback"], true);
    assert_eq!(diff["metadata"]["fallback_reason"], "plugin_parse_error");
    assert!(!diff["parse_errors"].as_array().unwrap().is_empty(), "{diff:#}");
    assert!(!diff["changes"].as_array().unwrap().is_empty(), "the edit must be preserved: {diff:#}");
    // `complete` means the change list is the review (core #252); the fallback itself is
    // carried by `is_fallback` and `parse_errors`, not by completeness.
    assert_eq!(diff["completeness"], "complete", "{diff:#}");
}

#[test]
fn core_decides_the_plugin_parse_error_review() {
    let review = |config: &str| crate::source_fallback::plugin_parse_error_review_impl(
        "a:\n  b: 1\n", "a:\n  b: [1, \n", "config.yaml", "config.yaml", "yaml",
        "new", "Empty semantic tree", config,
    ).unwrap();
    // Default: the edit survives as the typed source fallback, with the parser's message.
    let served = review("{}");
    let diff = &served["diff"];
    assert_eq!(diff["is_fallback"], true, "{served:#}");
    assert_eq!(diff["metadata"]["fallback_reason"], "plugin_parse_error");
    assert_eq!(diff["metadata"]["plugin_error"], "Empty semantic tree");
    assert_eq!(diff["metadata"]["plugin_error_side"], "new");
    assert!(!diff["changes"].as_array().unwrap().is_empty());
    // Fallback disabled: a typed decline carrying the parser's own reason.
    let declined = review(r#"{"fallback_to_token_diff": false}"#);
    assert_eq!(declined["declined"], true, "{declined:#}");
    assert_eq!(declined["fallback_code"], "plugin_parse_error");
    assert_eq!(declined["reason"], "plugin returned error (new): Empty semantic tree");
    assert!(crate::source_fallback::plugin_parse_error_review_impl(
        "", "", "a", "a", "yaml", "both", "x", "{}").is_err(), "side is validated");
}

#[test]
fn plugin_parse_error_review_is_exposed_on_the_c_abi() {
    let args = json!(["<a/>", "<a", "d.xml", "d.xml", "xml", "new", "Empty semantic tree", "{}"]);
    let raw = crate::c_abi::dispatch("plugin_parse_error_review", args.as_array().unwrap());
    let envelope: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(envelope["ok"], true, "{envelope:#}");
}
