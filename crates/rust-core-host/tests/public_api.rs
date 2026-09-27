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
