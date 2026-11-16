//! #253: suppressors may only discard a draft when its WHOLE delta is reported by surviving
//! evidence on the correct sides. One positive (legitimate suppression still happens) and one
//! negative (partial proof no longer suffices) regression per audited predicate.
use crate::*;

fn n(id: &str, node_type: &str, label: &str, children: Vec<SemanticNode>) -> SemanticNode {
    SemanticNode {
        id: id.to_owned(),
        node_type: node_type.to_owned(),
        label: label.to_owned(),
        position: NodePosition { start_line: 0, start_col: 0, end_line: 0, end_col: 1 },
        // Unique per node: no predicate may rely on a hash coincidence.
        structural_hash: format!("h:{id}:{node_type}:{label}"),
        children,
        parent_type: None,
        type_info: None,
        facts: None,
    }
}

fn draft<'a>(change_type: &'static str, old: Option<&'a SemanticNode>, new: Option<&'a SemanticNode>) -> ChangeDraft<'a> {
    ChangeDraft { change_type, old_node: old, new_node: new, old_index: None, new_index: None,
        confidence: 1.0, description: String::new(), refactoring_kind: None, text_diff: None }
}

fn kinds(changes: &[ChangeDraft<'_>]) -> Vec<String> {
    changes.iter().map(|c| format!("{} {}", c.change_type,
        c.old_node.or(c.new_node).map(|n| n.label.as_str()).unwrap_or(""))).collect()
}

/// (1) One contained leaf pair plus an unrelated container delta.
#[test]
fn leaf_pair_inside_add_delete_containers_needs_full_container_coverage() {
    for (other_old, other_new, suppressed) in [("x", "x", true), ("x", "y", false)] {
        let old = n("1.0", "call", "warn", vec![n("1.0.0", "string", "'Oh no'", vec![]), n("1.0.1", "identifier", other_old, vec![])]);
        let new = n("2.0", "call", "warn", vec![n("2.0.0", "string", "'Error'", vec![]), n("2.0.1", "identifier", other_new, vec![])]);
        let mut changes = vec![draft("DELETION", Some(&old), None), draft("ADDITION", None, Some(&new)),
            draft("MODIFICATION", Some(&old.children[0]), Some(&new.children[0]))];
        suppress_add_delete_drafts_covered_by_pairings(&mut changes);
        let kept = kinds(&changes);
        assert_eq!(!kept.contains(&"DELETION warn".to_owned()), suppressed, "{other_old}->{other_new}: {kept:?}");
        assert_eq!(!kept.contains(&"ADDITION warn".to_owned()), suppressed, "{other_old}->{other_new}: {kept:?}");
    }
}

/// (2) One aligned unchanged leaf plus unpaired leaves is still a delta.
#[test]
fn an_aligned_unchanged_leaf_does_not_hide_unpaired_leaves() {
    let old = n("0.1", "block", "block", vec![n("0.1.0", "identifier", "keep", vec![])]);
    let same = n("0.1", "block", "block", vec![n("0.1.0", "identifier", "keep", vec![])]);
    let grown = n("0.1", "block", "block", vec![n("0.1.0", "identifier", "keep", vec![]), n("0.1.1", "call", "launch", vec![])]);
    assert!(!subtree_has_leaf_label_delta(&old, &same));
    assert!(subtree_has_leaf_label_delta(&old, &grown));
    assert!(subtree_has_leaf_label_delta(&grown, &old));
}

/// (3) Equal label sets with different multiplicity or order are not the same content.
#[test]
fn same_label_add_delete_pair_needs_positional_content_match() {
    let old = n("1.0", "call", "f", vec![n("1.0.0", "identifier", "a", vec![]), n("1.0.1", "identifier", "b", vec![])]);
    let swapped = n("2.0", "call", "f", vec![n("2.0.0", "identifier", "b", vec![]), n("2.0.1", "identifier", "a", vec![])]);
    let doubled = n("2.0", "call", "f", vec![n("2.0.0", "identifier", "a", vec![]), n("2.0.1", "identifier", "b", vec![]), n("2.0.2", "identifier", "b", vec![])]);
    let same = n("2.0", "call", "f", vec![n("2.0.0", "identifier", "a", vec![]), n("2.0.1", "identifier", "b", vec![])]);
    for (new, suppressed) in [(&swapped, false), (&doubled, false), (&same, true)] {
        let mut changes = vec![draft("DELETION", Some(&old), None), draft("ADDITION", None, Some(new))];
        suppress_same_label_add_delete_pair_drafts(&mut changes, "javascript");
        assert_eq!(changes.is_empty(), suppressed, "{:?}", kinds(&changes));
    }
}

/// (4) Only old-side (or only new-side) ancestor coverage is not coverage.
#[test]
fn a_leaf_folds_into_its_entity_only_when_both_sides_are_inside_it() {
    let old_f = n("0.0", "function_definition", "f", vec![n("0.0.0", "integer", "1", vec![])]);
    let new_f = n("0.0", "function_definition", "f", vec![n("0.0.0", "integer", "2", vec![])]);
    let new_g = n("0.1", "function_definition", "g", vec![n("0.1.0", "integer", "3", vec![])]);
    let mut inside = vec![draft("MODIFICATION", Some(&old_f), Some(&new_f)),
        draft("MODIFICATION", Some(&old_f.children[0]), Some(&new_f.children[0]))];
    suppress_parent_modifications_drafts(&mut inside, "python", None);
    assert_eq!(kinds(&inside), vec!["MODIFICATION f"], "a covered leaf folds into its member");
    let mut escaped = vec![draft("MODIFICATION", Some(&old_f), Some(&new_f)),
        draft("MODIFICATION", Some(&old_f.children[0]), Some(&new_g.children[0]))];
    suppress_parent_modifications_drafts(&mut escaped, "python", None);
    assert!(kinds(&escaped).contains(&"MODIFICATION 1".to_owned()), "{:?}", kinds(&escaped));

    // Preferred-parent folding obeys the same both-sides rule.
    let old_if = n("0.2", "if_statement", "if", vec![n("0.2.0", "comparison", "x > 1", vec![])]);
    let new_if = n("0.2", "if_statement", "if", vec![n("0.2.0", "comparison", "x > 2", vec![])]);
    let elsewhere = n("0.3", "comparison", "y > 2", vec![]);
    let mut folded = vec![draft("MODIFICATION", Some(&old_if), Some(&new_if)),
        draft("MODIFICATION", Some(&old_if.children[0]), Some(&new_if.children[0]))];
    suppress_child_modifications_under_preferred_parent_drafts(&mut folded);
    assert_eq!(kinds(&folded), vec!["MODIFICATION if"]);
    let mut one_sided = vec![draft("MODIFICATION", Some(&old_if), Some(&new_if)),
        draft("MODIFICATION", Some(&old_if.children[0]), Some(&elsewhere))];
    suppress_child_modifications_under_preferred_parent_drafts(&mut one_sided);
    assert_eq!(one_sided.len(), 2, "{:?}", kinds(&one_sided));
}

/// (4b) A parent modification with an extra, unreported delta is not replaced by its child.
#[test]
fn a_parent_modification_survives_unless_its_whole_delta_is_reported() {
    let old = n("0.0", "assignment", "a = b", vec![n("0.0.0", "identifier", "a", vec![]), n("0.0.1", "identifier", "b", vec![])]);
    let renamed_one = n("0.0", "assignment", "c = b", vec![n("0.0.0", "identifier", "c", vec![]), n("0.0.1", "identifier", "b", vec![])]);
    let renamed_two = n("0.0", "assignment", "c = d", vec![n("0.0.0", "identifier", "c", vec![]), n("0.0.1", "identifier", "d", vec![])]);
    let mut covered = vec![draft("MODIFICATION", Some(&old), Some(&renamed_one)),
        draft("MODIFICATION", Some(&old.children[0]), Some(&renamed_one.children[0]))];
    suppress_parent_modifications_drafts(&mut covered, "python", None);
    assert_eq!(kinds(&covered), vec!["MODIFICATION a"], "fully reported: the precise child wins");
    let mut partial = vec![draft("MODIFICATION", Some(&old), Some(&renamed_two)),
        draft("MODIFICATION", Some(&old.children[0]), Some(&renamed_two.children[0]))];
    suppress_parent_modifications_drafts(&mut partial, "python", None);
    assert_eq!(kinds(&partial), vec!["MODIFICATION a = b", "MODIFICATION a"], "b -> d is reported only by the parent");
}

/// (5) A child label merely CONTAINING the renamed name is body, not identity.
#[test]
fn a_child_mentioning_the_renamed_name_is_still_compared() {
    let section = |name: &str, value: &str| n("0.0", "section", name, vec![
        n("0.0.0", "property", &format!("alias = {name} {value}"), vec![])]);
    assert!(rename_body_is_unchanged(&section("server", "x=1"), &section("serverRenamed", "x=1")));
    assert!(!rename_body_is_unchanged(&section("server", "x=1"), &section("serverRenamed", "x=2")));
    // A `_name`-typed child that is not the entity's name is body too.
    let typed = |name: &str, ty: &str| n("0.0", "section", name, vec![n("0.0.0", "type_name", ty, vec![])]);
    assert!(!rename_body_is_unchanged(&typed("server", "int"), &typed("serverRenamed", "str")));
}

/// (6) A same-label addition in a DIFFERENT scope does not account for a deletion.
#[test]
fn a_same_label_entity_addition_only_absorbs_a_deletion_in_the_same_scope() {
    let old_root = n("0", "module", "", vec![
        n("0.0", "class_definition", "A", vec![n("0.0.0", "function_definition", "f", vec![])]),
        n("0.1", "class_definition", "B", vec![])]);
    let new_root = n("0", "module", "", vec![
        n("0.0", "class_definition", "A", vec![n("0.0.0", "function_definition", "f", vec![n("0.0.0.0", "pass_statement", "pass", vec![])])]),
        n("0.1", "class_definition", "B", vec![n("0.1.0", "function_definition", "f", vec![])])]);
    let matching = vec![MatchPair { old_node: &old_root, new_node: &new_root },
        MatchPair { old_node: &old_root.children[0], new_node: &new_root.children[0] },
        MatchPair { old_node: &old_root.children[1], new_node: &new_root.children[1] }];
    let old_f = &old_root.children[0].children[0];
    let mut other_scope = vec![draft("DELETION", Some(old_f), None), draft("ADDITION", None, Some(&new_root.children[1].children[0]))];
    suppress_same_label_function_delete_for_addition_drafts(&mut other_scope, &matching);
    assert_eq!(other_scope.len(), 2, "A.f deleted is not B.f added: {:?}", kinds(&other_scope));
    let mut same_scope = vec![draft("DELETION", Some(old_f), None), draft("ADDITION", None, Some(&new_root.children[0].children[0]))];
    suppress_same_label_function_delete_for_addition_drafts(&mut same_scope, &matching);
    assert_eq!(kinds(&same_scope), vec!["ADDITION f"]);
    // One addition absorbs at most one deletion.
    let twin = n("0.0.9", "function_definition", "f", vec![]);
    let mut twice = vec![draft("DELETION", Some(old_f), None), draft("DELETION", Some(&twin), None),
        draft("ADDITION", None, Some(&new_root.children[0].children[0]))];
    suppress_same_label_function_delete_for_addition_drafts(&mut twice, &matching);
    assert_eq!(twice.len(), 2, "{:?}", kinds(&twice));
}
