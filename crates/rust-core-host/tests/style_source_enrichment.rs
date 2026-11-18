//! Production-path regressions for CSS/SCSS source enrichment (#273, #276).
#![cfg(feature = "tier-c-wasm")]

use serde_json::{json, Value};

fn review(language: &str, old: &str, new: &str) -> Value {
    let wasm = std::env::var("INTENTUMDIFF_TEST_WASM_DIR")
        .expect("checksum-verified CSS/SCSS components are required");
    let component = format!("{language}_parser.wasm");
    let staged = std::path::Path::new(&wasm).join(&component);
    assert!(staged.is_file(), "verified component is missing: {}", staged.display());
    let temp = tempfile::tempdir().unwrap();
    std::fs::copy(&staged, temp.path().join(&component)).unwrap();
    let extension = format!(".{language}");
    let manifest = json!({
        "parsers": {language: {"extensions": [&extension], "plugin_id": language, "wasm": component}},
        "extension_index": {&extension: language}
    });
    std::fs::write(temp.path().join("parser_manifest.json"), manifest.to_string()).unwrap();
    let result = intentumdiff_rust_core::live_server::live_diff_contents_impl(
        env!("CARGO_MANIFEST_DIR"), &format!("assessment.{language}"), old, new,
        &json!({"language_hint": language, "strict_plugins": true}).to_string(),
        &temp.path().to_string_lossy(),
    ).expect("production source review succeeds");
    let response: Value = serde_json::from_str(&result).unwrap();
    let diff = response.get("diff").expect("production parser selected").clone();
    assert_eq!(diff["is_fallback"], false, "{diff:#}");
    assert_eq!(diff["parse_errors"], json!([]), "{diff:#}");
    diff
}

fn assessment_case(id: &str) -> Value {
    let fixtures: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/language_assessment/web.json"
    )).unwrap();
    fixtures["cases"].as_array().unwrap().iter().find(|c| c["id"] == id).unwrap().clone()
}

#[test]
fn css_comment_before_property_preserves_assessed_behavior() {
    let case = assessment_case("web-css-preservation");
    let diff = review("css", case["old"].as_str().unwrap(), case["new"].as_str().unwrap());
    assert_eq!(diff["has_semantic_changes"], false, "{diff:#}");
}

#[test]
fn scss_escaped_multiline_tail_edit_is_meaningful() {
    let case = assessment_case("web-scss-external-escaped-string-value");
    let diff = review("scss", case["old"].as_str().unwrap(), case["new"].as_str().unwrap());
    assert_eq!(diff["has_semantic_changes"], true, "{diff:#}");
    assert_eq!(diff["is_style_only"], false, "{diff:#}");
    let changes = diff["changes"].as_array().unwrap();
    let changed_value = changes.iter().find(|c| c["old_node"]["node_type"] == "property_value")
        .expect("complete property value remains visible");
    assert_eq!(changed_value["old_node"]["label"], "'line1 \\\n      line2'");
    assert_eq!(changed_value["new_node"]["label"], "'line1 \\\n      changed'");
    assert_eq!(changed_value["old_node"]["position"]["end_line"], 2);
}

#[test]
fn scss_escaped_multiline_spelling_preserves_assessed_behavior() {
    let case = assessment_case("web-scss-external-escaped-string-spelling");
    let diff = review("scss", case["old"].as_str().unwrap(), case["new"].as_str().unwrap());
    assert_eq!(diff["has_semantic_changes"], false, "{diff:#}");
}

#[test]
fn scss_crlf_unicode_and_string_delimiters_preserve_tail_edits() {
    let old = "a {\r\n  b: 'café; } /*text*/ \\\r\n      fin'; c: 2;\r\n}\r\n";
    let diff = review("scss", old, &old.replace("fin", "final"));
    assert_eq!(diff["has_semantic_changes"], true, "{diff:#}");
    assert!(diff["changes"].as_array().unwrap().iter().any(|c|
        c["old_node"]["label"] == "'café; } /*text*/ \\\r\n      fin'"), "{diff:#}");
}

#[test]
fn scss_string_equivalence_is_scoped_to_complete_literal_values() {
    for (old_value, new_value, equivalent) in [
        ("'café \\\r\n  fin'", "\"café   fin\"", true),
        ("'one  two'", "\"one two\"", false),
        ("'literal /* text */'", "\"literal\"", false),
        ("'\\n'", "\"\\a\"", false),
        ("'#{1 + 1}'", "\"#{2}\"", false),
        ("'a' + 'b'", "\"ab\"", false),
    ] {
        let old = format!("a {{ b: {old_value}; }}\n");
        let new = format!("a {{ b: {new_value}; }}\n");
        let diff = review("scss", &old, &new);
        assert_eq!(diff["has_semantic_changes"], !equivalent, "{old_value} -> {new_value}: {diff:#}");
    }
}

#[test]
fn style_comments_interpolation_and_custom_property_payloads_remain_scoped() {
    for (language, old, new, meaningful) in [
        ("scss", "a { color: red;\n}", "a { // } ; :\n color: red;\n}", false),
        ("scss", "a { color: #{$tone}; margin: 1px; }", "a { color: #{$other}; margin: 1px; }", true),
        ("css", "a { --theme: { color: red; }; margin: 1px; }", "a { --theme: { color: blue; }; margin: 1px; }", true),
        ("css", "a { --path: http://example/a; }", "a { --path: http://example/b; }", true),
        ("scss", "a { background: url(http://example/a); color: red; }", "a { background: url(http://example/b); color: red; }", true),
        ("css", "a { background: url(https://example/a/*/b); color: red; }", "a { background: url(https://example/a/*/c); color: red; }", true),
        ("css", "a { background: url(https://example/a[b); color: red; }", "a { background: url(https://example/a[c); color: red; }", true),
    ] {
        let diff = review(language, old, new);
        assert_eq!(diff["has_semantic_changes"], meaningful, "{old} -> {new}: {diff:#}");
    }
}
