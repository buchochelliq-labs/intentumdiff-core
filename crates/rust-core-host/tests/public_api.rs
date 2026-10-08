use intentumdiff_rust_core::api::{review_text, ReviewError};

#[test]
fn typed_review_keeps_move_evidence() -> Result<(), ReviewError> {
    let result = review_text(
        "# A\none\n# B\ntwo\n",
        "# B\ntwo\n# A\none\n",
        "a.md",
        "a.md",
    )?;
    assert!(result.has_semantic_changes);
    assert!(!result.is_style_only);
    assert_eq!(result.changes.len(), 1);
    assert_eq!(result.changes[0].change_type, "MOVE");
    assert_eq!(result.changes[0].old_node.as_ref().unwrap().label, "# B");
    assert!(result
        .change_groups
        .iter()
        .all(|g| g.kind != "IGNORED_STYLE"));
    Ok(())
}

#[test]
fn source_review_reports_missing_parser_as_unsupported() {
    use intentumdiff_rust_core::api::{review_sources, ReviewOptions};
    let root = tempfile::tempdir().unwrap();
    let error = review_sources(root.path(), root.path(), "a.ts", "const x = 1;", "const x = 2;", &ReviewOptions::default()).unwrap_err();
    assert!(matches!(error, ReviewError::Unsupported(ref reason) if reason.contains("no bundled parser")), "{error}");
}

#[test]
fn source_review_preserves_manifest_engine_error() {
    use intentumdiff_rust_core::api::{review_sources, ReviewOptions};
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("parser_manifest.json"), "{").unwrap();
    let error = review_sources(root.path(), root.path(), "a.py", "x = 1", "x = 2", &ReviewOptions::default()).unwrap_err();
    assert!(matches!(error, ReviewError::Engine(ref reason) if reason.contains("invalid parser manifest")), "{error}");
}

#[cfg(feature = "tier-c-wasm")]
#[test]
fn source_review_with_options_keeps_literal_edit() -> Result<(), ReviewError> {
    use intentumdiff_rust_core::api::{review_sources, ReviewOptions};
    let root = tempfile::tempdir().unwrap();
    let staged = std::path::PathBuf::from(std::env::var("INTENTUMDIFF_TEST_WASM_DIR").expect("provision parser components"));
    // CI stages raw components, not a distributable parser inventory. Build the
    // inventory explicitly so this test exercises real component discovery.
    let parsers = tempfile::tempdir().unwrap();
    std::fs::copy(staged.join("python_parser.wasm"), parsers.path().join("python_parser.wasm")).unwrap();
    std::fs::write(parsers.path().join("parser_manifest.json"), serde_json::json!({
        "parsers": {"python": {"plugin_id": "python", "wasm": "python_parser.wasm", "extensions": [".py"]}},
        "extension_index": {".py": "python"}
    }).to_string()).unwrap();
    let options = ReviewOptions { detect_refactorings: Some(false), guardrails_enabled: Some(false), ..Default::default() };
    let review = review_sources(root.path(), parsers.path(), "a.py", "def total(x):\n    return x + 1\n", "def total(x):\n    return x + 2\n", &options)?;
    assert!(review.has_semantic_changes);
    assert!(!review.is_style_only);
    assert_eq!(review.changes.len(), 1);
    let change = &review.changes[0];
    assert_eq!(change.change_type, "MODIFICATION");
    assert_eq!(change.old_node.as_ref().unwrap().label, "1");
    assert_eq!(change.new_node.as_ref().unwrap().label, "2");
    Ok(())
}
