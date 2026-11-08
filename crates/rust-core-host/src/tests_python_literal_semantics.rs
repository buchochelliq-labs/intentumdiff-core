use super::*;

fn native_literal_diff(old_literal: &str, new_literal: &str) -> Value {
    let old_source = format!("value = {old_literal}\n");
    let new_source = format!("value = {new_literal}\n");
    native_source_diff(&old_source, &new_source)
}

fn native_source_diff(old_source: &str, new_source: &str) -> Value {
    let request = json!({
        "schema_version": 1,
        "python_parser_backend": "native",
        "files": [{
            "old_source": old_source,
            "new_source": new_source,
            "old_filename": "literal.py",
            "new_filename": "literal.py",
            "language": "python"
        }]
    });
    let payload: Value = serde_json::from_str(&diff_batch(&request.to_string()).unwrap()).unwrap();
    assert_eq!(payload["status"], COMPLETE, "{payload}");
    payload["diffs"][0]["diff"].clone()
}

fn assert_meaningful_literal_change(old_literal: &str, new_literal: &str) {
    let diff = native_literal_diff(old_literal, new_literal);
    assert_eq!(
        diff["has_semantic_changes"], true,
        "{old_literal} -> {new_literal}: {}",
        diff["change_groups"]
    );
    assert_eq!(diff["is_style_only"], false, "{diff}");
    assert!(!diff["changes"].as_array().unwrap().is_empty(), "{diff}");
}

#[test]
fn python_literal_text_to_bytes_is_meaningful() {
    assert_meaningful_literal_change("\"plain\"", "b\"plain\"");
}

#[test]
fn python_literal_raw_prefix_changes_escape_semantics() {
    assert_meaningful_literal_change(r#""line\n""#, r#"r"line\n""#);
}

#[test]
fn python_literal_raw_bytes_prefix_changes_escape_semantics() {
    assert_meaningful_literal_change(r#"b"line\n""#, r#"rb"line\n""#);
}

#[test]
fn python_literal_escaped_quote_is_not_just_quote_style() {
    assert_meaningful_literal_change(r#""\'""#, r#"r"\'""#);
}

#[test]
fn python_literal_interpolation_edits_remain_meaningful() {
    assert_meaningful_literal_change(r#"f"{first}""#, r#"f"{second}""#);
    assert_meaningful_literal_change(r#""{first}""#, r#"f"{first}""#);
}

#[test]
fn python_literal_multiline_raw_content_edits_remain_meaningful() {
    assert_meaningful_literal_change("r'''first\nsecond'''", "r'''first\nthird'''");
}

#[test]
fn python_literal_safe_prefix_spelling_and_quote_controls() {
    for (old, new) in [
        ("'plain'", "\"plain\""),
        ("u'plain'", "'plain'"),
        ("r'plain'", "R\"plain\""),
        ("b'plain'", "B\"plain\""),
        (r#"rb'line\n'"#, r#"Br"line\n""#),
    ] {
        let diff = native_literal_diff(old, new);
        assert_eq!(
            diff["has_semantic_changes"], false,
            "{old} -> {new}: {diff}"
        );
        assert!(
            diff["change_groups"]
                .as_array()
                .unwrap()
                .iter()
                .any(|group| group["kind"] == "IGNORED_STYLE"),
            "{old} -> {new}: {diff}"
        );
    }
}

#[test]
fn python_literal_quote_evidence_does_not_cover_independent_bytes_change() {
    let diff = native_source_diff(
        "a = 'plain'\nb = 'plain'\n",
        "a = \"plain\"\nb = b'plain'\n",
    );
    assert_eq!(
        diff["has_semantic_changes"], true,
        "{}",
        diff["change_groups"]
    );
    assert!(!diff["changes"].as_array().unwrap().is_empty(), "{diff}");
}

#[test]
fn python_literal_compound_quote_proof_keeps_bytes_sibling_change() {
    assert_meaningful_literal_change("('plain', 'plain')", "(\"plain\", b'plain')");
    assert_meaningful_literal_change("call('plain', 'plain')", "call(\"plain\", b'plain')");
}

#[test]
fn python_literal_swapped_kinds_do_not_pair_across_assignments() {
    let diff = native_source_diff(
        "a = 'plain'\nb = b'plain'\n",
        "a = b\"plain\"\nb = \"plain\"\n",
    );
    assert_eq!(
        diff["has_semantic_changes"], true,
        "{}",
        diff["change_groups"]
    );
    assert!(!diff["changes"].as_array().unwrap().is_empty(), "{diff}");
}

#[test]
fn python_literal_escape_equivalents_preserve_type() {
    for (old, new) in [
        (r#"'\x41'"#, "'A'"),
        (r#"b'\101'"#, "b'A'"),
        (r#"'\u96ea'"#, "'雪'"),
        (r#"r'plain'"#, "'plain'"),
    ] {
        let diff = native_literal_diff(old, new);
        assert_eq!(
            diff["has_semantic_changes"], false,
            "{old} -> {new}: {diff}"
        );
    }
}

#[test]
fn python_literal_native_hash_preserves_prefix_before_finalization() {
    let old = "value = \"plain\"\n";
    let new = "value = b\"plain\"\n";
    let old_cst =
        serialize_ts_node(parse_python_tree(old).unwrap().root_node(), old.as_bytes()).unwrap();
    let new_cst =
        serialize_ts_node(parse_python_tree(new).unwrap().root_node(), new.as_bytes()).unwrap();
    assert_ne!(structural_hash_cst(&old_cst), structural_hash_cst(&new_cst));
}
