//! #301: size limits are configurable, named in the error, and classified as their own kind.
use crate::*;

fn big_python(functions: usize) -> String {
    (0..functions)
        .map(|i| format!("def f{i}(a, b):\n    if a > b:\n        return a * {i} + b\n    return b - {i}\n"))
        .collect()
}

#[test]
fn size_limit_errors_name_the_setting_and_have_their_own_envelope_kind() {
    let cst = json!({"type": "module", "text": "", "children": []}).to_string();
    let env: Value = serde_json::from_str(&crate::c_abi::dispatch("diff_python_cst",
        &[json!(cst), json!(cst), json!("a.py"), json!("a.py"), json!("{\"max_cst_bytes\": 4}")])).unwrap();
    assert_eq!(env["ok"], false, "{env}");
    assert_eq!(env["error_type"], "size_limit", "{env}");
    let message = env["error"].as_str().unwrap();
    assert!(message.contains("max_cst_bytes"), "{message}");
    // An ordinary bad request keeps its kind.
    let env: Value = serde_json::from_str(&crate::c_abi::dispatch("diff_python_cst",
        &[json!("{"), json!(cst), json!("a.py"), json!("a.py"), json!("{}")])).unwrap();
    assert_eq!(env["error_type"], "value_error", "{env}");
}

#[test]
fn parser_output_limit_names_its_setting() {
    let err = check_byte_limit("old parser output", "0123456789", 4).unwrap_err();
    assert!(err.contains("max_plugin_output_bytes"), "{err}");
    assert!(is_size_limit_error(&err), "{err}");
}

fn batch(old: &str, new: &str, limit: usize) -> Value {
    let request = json!({"schema_version": 1, "python_parser_backend": "native",
        "config": {"max_cst_bytes": limit, "max_plugin_output_bytes": limit, "max_nodes": 2_000_000},
        "files": [{"old_source": old, "new_source": new, "old_filename": "big.py",
            "new_filename": "big.py", "language": "python"}]});
    match diff_batch(&request.to_string()) {
        Ok(raw) => serde_json::from_str(&raw).unwrap(),
        Err(message) => json!({"error": message}),
    }
}

#[test]
fn raising_the_limit_reviews_a_file_the_default_rejects() {
    let old = big_python(300);
    let new = old.replacen("return a * 7 + b", "return a * 8 + b", 1);
    // A limit below the file's CST is refused, naming the setting...
    let refused = batch(&old, &new, 8 * 1024);
    let text = refused.to_string();
    assert!(text.contains("max_cst_bytes") && text.contains(SIZE_LIMIT_MARKER), "{}", text.chars().take(1500).collect::<String>());
    assert_ne!(refused["status"], COMPLETE);
    // ...and raising it reviews the same file completely.
    let reviewed = batch(&old, &new, 256 * 1024 * 1024);
    assert_eq!(reviewed["status"], COMPLETE, "{}", reviewed.to_string().chars().take(1500).collect::<String>());
    assert_eq!(reviewed["diffs"][0]["diff"]["has_semantic_changes"], true);
}

/// The issue's 20k-line file (slow in a debug build; run with `--ignored` or `--release`).
#[test]
#[ignore = "large input; run with --ignored in a release build"]
fn twenty_thousand_line_python_file_is_reviewed_with_raised_limits() {
    let old = big_python(5000);
    let new = old.replacen("return a * 7 + b", "return a * 8 + b", 1);
    assert!(old.lines().count() >= 20_000);
    let reviewed = batch(&old, &new, 256 * 1024 * 1024);
    assert_eq!(reviewed["status"], COMPLETE, "{}", reviewed.to_string().chars().take(1500).collect::<String>());
    assert_eq!(reviewed["diffs"][0]["diff"]["has_semantic_changes"], true);
}
