//! Draft promotion + suppression passes, block 2 (same-id promotions and the
//! descendant/parent/move-noise suppressors), extracted from lib.rs verbatim
//! (issue #29 monolith split, phase B).

use crate::*;

pub(crate) fn promote_same_id_named_line_moves_drafts<'a>(
    changes: &mut Vec<ChangeDraft<'a>>,
    matching: &[MatchPair<'a>],
) {
    let existing_pairs: HashSet<(&str, &str, &str)> = changes
        .iter()
        .filter_map(|change| {
            Some((
                change.change_type,
                change.old_node?.id.as_str(),
                change.new_node?.id.as_str(),
            ))
        })
        .collect();
    // Insertion-shift discrimination (issue #32): within each parent, same-identity entities
    // whose RELATIVE line order is preserved (LIS of new start lines in old order) merely
    // shifted because content above them grew/shrank — not moves. Only order-inverted
    // entities promote to MOVE drafts.
    let mut group_members: HashMap<String, Vec<(&MatchPair<'a>, u32, u32)>> = HashMap::new();
    for pair in matching {
        if !is_named_entity_type(pair.old_node.node_type.as_str()) {
            continue;
        }
        if pair.old_node.id != pair.new_node.id
            || pair.old_node.label != pair.new_node.label
            || pair.old_node.position.start_line == pair.new_node.position.start_line
        {
            continue;
        }
        if existing_pairs.contains(&("MOVE", pair.old_node.id.as_str(), pair.new_node.id.as_str()))
        {
            continue;
        }
        let parent = pair
            .old_node
            .id
            .rsplit_once('.')
            .map(|(prefix, _)| prefix.to_owned())
            .unwrap_or_default();
        group_members.entry(parent).or_default().push((
            pair,
            pair.old_node.position.start_line,
            pair.new_node.position.start_line,
        ));
    }
    let mut promoted = Vec::new();
    for members in group_members.values_mut() {
        members.sort_by_key(|(_, old_line, _)| *old_line);
        let sequence: Vec<usize> = members
            .iter()
            .map(|(_, _, new_line)| *new_line as usize)
            .collect();
        let stationary = longest_increasing_subsequence_positions(&sequence);
        for (position, (pair, _, _)) in members.iter().enumerate() {
            if stationary.contains(&position) {
                continue;
            }
            promoted.push(edit_op_to_draft(EditOp {
                kind: "MOVE",
                old_node: Some(pair.old_node),
                new_node: Some(pair.new_node),
                old_index: None,
                new_index: None,
            }));
        }
    }
    changes.extend(promoted);
}

pub(crate) fn promote_same_id_named_renames_from_add_delete_drafts<'a>(changes: &mut Vec<ChangeDraft<'a>>) {
    let deleted_entities: Vec<&SemanticNode> = changes
        .iter()
        .filter(|change| change.change_type == "DELETION")
        .filter_map(|change| change.old_node)
        .filter(|node| is_named_entity_type(node.node_type.as_str()))
        .collect();
    if deleted_entities.is_empty() {
        return;
    }
    let added_entities_by_key: HashMap<(&str, &str), &SemanticNode> = changes
        .iter()
        .filter(|change| change.change_type == "ADDITION")
        .filter_map(|change| change.new_node)
        .filter(|node| is_named_entity_type(node.node_type.as_str()))
        .map(|node| ((node.node_type.as_str(), node.id.as_str()), node))
        .collect();
    if added_entities_by_key.is_empty() {
        return;
    }
    let existing_move_pairs: HashSet<(&str, &str)> = changes
        .iter()
        .filter(|change| change.change_type == "MOVE")
        .filter_map(|change| Some((change.old_node?.id.as_str(), change.new_node?.id.as_str())))
        .collect();
    let mut promoted_keys = HashSet::new();
    let mut promoted = Vec::new();
    for old_node in deleted_entities {
        if old_node.id.is_empty() || old_node.node_type.is_empty() || old_node.label.is_empty() {
            continue;
        }
        let Some(new_node) =
            added_entities_by_key.get(&(old_node.node_type.as_str(), old_node.id.as_str()))
        else {
            continue;
        };
        if new_node.label.is_empty()
            || new_node.label == old_node.label
            || existing_move_pairs.contains(&(old_node.id.as_str(), old_node.id.as_str()))
            || !same_id_named_rename_looks_compatible_node(old_node, new_node)
        {
            continue;
        }
        promoted_keys.insert((old_node.node_type.clone(), old_node.id.clone()));
        // Same structural id + same type + different label = a RENAME (the entity stays put; only
        // its name changed), not a MOVE. Emitting MOVE here contradicted this function's own name
        // and surfaced a clean function rename (greet -> welcome) as MOVE + a redundant identifier
        // modification; the Python oracle collapses it to one change. Emit a REFACTORING rename.
        promoted.push(ChangeDraft {
            change_type: "REFACTORING",
            old_node: Some(old_node),
            new_node: Some(new_node),
            old_index: None,
            new_index: None,
            confidence: 1.0,
            description: format!(
                "Rename {}('{}') -> ('{}')",
                old_node.node_type, old_node.label, new_node.label
            ),
            refactoring_kind: Some("RENAME_SYMBOL"),
            text_diff: None,
        });
    }
    if promoted_keys.is_empty() {
        return;
    }
    changes.retain(|change| {
        if !matches!(change.change_type, "DELETION" | "ADDITION") {
            return true;
        }
        let node = change.old_node.or(change.new_node);
        !node.is_some_and(|node| promoted_keys.contains(&(node.node_type.clone(), node.id.clone())))
    });
    changes.extend(promoted);
}

pub(crate) fn same_id_named_rename_looks_compatible_node(
    old_node: &SemanticNode,
    new_node: &SemanticNode,
) -> bool {
    if old_node.position.start_line == new_node.position.start_line {
        return true;
    }
    let mut new_descendants = HashMap::new();
    collect_descendant_identity_by_id_node(new_node, &mut new_descendants);
    has_matching_descendant_identity_node(old_node, &new_descendants)
}

pub(crate) fn collect_descendant_identity_by_id_node<'a>(
    node: &'a SemanticNode,
    result: &mut HashMap<&'a str, (&'a str, &'a str)>,
) {
    for child in &node.children {
        result.insert(
            child.id.as_str(),
            (child.node_type.as_str(), child.label.as_str()),
        );
        collect_descendant_identity_by_id_node(child, result);
    }
}

pub(crate) fn has_matching_descendant_identity_node(
    node: &SemanticNode,
    candidates: &HashMap<&str, (&str, &str)>,
) -> bool {
    node.children.iter().any(|child| {
        candidates
            .get(child.id.as_str())
            .is_some_and(|(candidate_type, candidate_label)| {
                *candidate_type == child.node_type && *candidate_label == child.label
            })
            || has_matching_descendant_identity_node(child, candidates)
    })
}

pub(crate) fn promote_imported_function_variable_renames_drafts<'a>(changes: &mut Vec<ChangeDraft<'a>>) {
    let deleted_functions: Vec<&SemanticNode> = changes
        .iter()
        .filter(|change| change.change_type == "DELETION")
        .filter_map(|change| change.old_node)
        .filter(|node| node.node_type == "function_definition")
        .collect();
    let added_imports: Vec<&SemanticNode> = changes
        .iter()
        .filter(|change| change.change_type == "ADDITION")
        .filter_map(|change| change.new_node)
        .filter(|node| node.node_type == "import_from_statement")
        .collect();
    if deleted_functions.is_empty() || added_imports.is_empty() {
        return;
    }

    let mut additions = Vec::new();
    for old_function in deleted_functions {
        let Some(old_identifier) = first_parameter_identifier_node(old_function) else {
            continue;
        };
        let Some(new_identifier) = added_imports
            .iter()
            .filter_map(|import_node| best_import_identifier_node(import_node, &old_function.label))
            .max_by_key(|node| node.label.len())
        else {
            continue;
        };
        if old_identifier.label.is_empty()
            || new_identifier.label.is_empty()
            || old_identifier.label == new_identifier.label
            || change_pair_exists_drafts(
                changes,
                "REFACTORING",
                Some(old_identifier.id.as_str()),
                Some(new_identifier.id.as_str()),
            )
        {
            continue;
        }
        additions.push(ChangeDraft {
            change_type: "REFACTORING",
            old_node: Some(old_identifier),
            new_node: Some(new_identifier),
            old_index: None,
            new_index: None,
            confidence: 1.0,
            description: format!(
                "Rename variable '{}' -> '{}'",
                old_identifier.label, new_identifier.label
            ),
            refactoring_kind: Some("RENAME_VARIABLE"),
            text_diff: None,
        });
    }
    changes.extend(additions);
}

pub(crate) fn collect_leaf_refs<'a>(node: &'a SemanticNode, out: &mut Vec<&'a SemanticNode>) {
    if node.children.is_empty() {
        out.push(node);
        return;
    }
    for child in &node.children {
        collect_leaf_refs(child, out);
    }
}

/// Root-to-node reference path inside `root`'s subtree (excluding `root` itself).
pub(crate) fn path_to_node<'a>(root: &'a SemanticNode, id: &str) -> Option<Vec<&'a SemanticNode>> {
    for child in &root.children {
        if child.id == id {
            return Some(vec![child]);
        }
        if let Some(mut path) = path_to_node(child, id) {
            path.insert(0, child);
            return Some(path);
        }
    }
    None
}

pub(crate) fn promote_label_updates_inside_moved_entities_drafts<'a>(changes: &mut Vec<ChangeDraft<'a>>) {
    let mut additions = Vec::new();
    for change in changes.iter() {
        if change.change_type != "MOVE" {
            continue;
        }
        let Some(old_entity) = change.old_node else {
            continue;
        };
        let Some(new_entity) = change.new_node else {
            continue;
        };
        if !is_entity_container_type(old_entity.node_type.as_str()) {
            continue;
        }
        let mut recovered_any = false;
        let old_descendants = all_descendant_node_refs_by_id(old_entity);
        let new_descendants = all_descendant_node_refs_by_id(new_entity);
        for (old_id, old_node) in old_descendants {
            let Some(new_node) = new_descendants.get(old_id) else {
                continue;
            };
            if old_node.node_type != new_node.node_type
                || !is_moved_child_update_type(old_node.node_type.as_str())
                || old_node.label == new_node.label
                || change_pair_exists_drafts(
                    changes,
                    "MODIFICATION",
                    Some(old_node.id.as_str()),
                    Some(new_node.id.as_str()),
                )
            {
                continue;
            }
            recovered_any = true;
            additions.push(ChangeDraft {
                change_type: "MODIFICATION",
                old_node: Some(old_node),
                new_node: Some(new_node),
                old_index: None,
                new_index: None,
                confidence: 1.0,
                description: format!(
                    "Update {} -> {}",
                    format_node_ref(old_node),
                    format_node_ref(new_node)
                ),
                refactoring_kind: None,
                text_diff: None,
            });
        }

        // Id-based pairing only works when the move kept the subtree numbering. A move
        // across nesting levels (csharp block -> file-scoped namespace) renumbers every
        // descendant, the matcher never pairs the inner leaves (descendant seeding is
        // hash-gated), and the edit inside the moved entity vanished — the python side
        // recovers it by sequence-aligning the leaf (node_type, label) keys
        // (_recover_modifications_inside_moved_entities). Port: when the two subtrees
        // have the same leaf count, zip leaves positionally and surface each same-type
        // label delta at the deepest same-type/same-label ancestor pair (so the change
        // carries the enclosing member's name, e.g. method_declaration 'Bar'), falling
        // back to the leaf pair itself.
        if !recovered_any && old_entity.structural_hash != new_entity.structural_hash {
            let mut old_leaves = Vec::new();
            let mut new_leaves = Vec::new();
            collect_leaf_refs(old_entity, &mut old_leaves);
            collect_leaf_refs(new_entity, &mut new_leaves);
            if old_leaves.len() != new_leaves.len() {
                continue;
            }
            for (old_leaf, new_leaf) in old_leaves.iter().zip(&new_leaves) {
                if old_leaf.node_type != new_leaf.node_type || old_leaf.label == new_leaf.label
                {
                    continue;
                }
                let old_path = path_to_node(old_entity, old_leaf.id.as_str());
                let new_path = path_to_node(new_entity, new_leaf.id.as_str());
                let (mut anchor_old, mut anchor_new): (&SemanticNode, &SemanticNode) =
                    (old_leaf, new_leaf);
                if let (Some(old_path), Some(new_path)) = (old_path, new_path) {
                    for (old_ancestor, new_ancestor) in old_path.iter().zip(&new_path) {
                        if old_ancestor.node_type == new_ancestor.node_type
                            && old_ancestor.label == new_ancestor.label
                            && (is_named_entity_type(old_ancestor.node_type.as_str())
                                || is_moved_child_update_type(
                                    old_ancestor.node_type.as_str(),
                                ))
                        {
                            anchor_old = old_ancestor;
                            anchor_new = new_ancestor;
                        }
                    }
                }
                if change_pair_exists_drafts(
                    changes,
                    "MODIFICATION",
                    Some(anchor_old.id.as_str()),
                    Some(anchor_new.id.as_str()),
                ) || additions.iter().any(|draft| {
                    draft.old_node.is_some_and(|node| node.id == anchor_old.id)
                }) {
                    continue;
                }
                additions.push(ChangeDraft {
                    change_type: "MODIFICATION",
                    old_node: Some(anchor_old),
                    new_node: Some(anchor_new),
                    old_index: None,
                    new_index: None,
                    confidence: 0.85,
                    description: format!(
                        "Update inside moved {} '{}': {} -> {}",
                        old_entity.node_type,
                        old_entity.label,
                        format_node_ref(old_leaf),
                        format_node_ref(new_leaf)
                    ),
                    refactoring_kind: None,
                    text_diff: None,
                });
            }
        }
    }
    changes.extend(additions);
}

pub(crate) fn suppress_same_label_function_delete_for_addition_drafts(changes: &mut Vec<ChangeDraft<'_>>) {
    let added_labels: HashSet<(&str, &str)> = changes
        .iter()
        .filter(|change| change.change_type == "ADDITION")
        .filter_map(|change| change.new_node)
        .filter(|node| is_named_entity_type(node.node_type.as_str()))
        .map(|node| (node.node_type.as_str(), node.label.as_str()))
        .collect();
    if added_labels.is_empty() {
        return;
    }
    changes.retain(|change| {
        if change.change_type != "DELETION" {
            return true;
        }
        let Some(node) = change.old_node else {
            return true;
        };
        !is_named_entity_type(node.node_type.as_str())
            || !added_labels.contains(&(node.node_type.as_str(), node.label.as_str()))
    });
}

/// Port of python `presentation.py::_suppress_haskell_signature_function_sibling_churn`
/// (language-gated to haskell by the caller). A Haskell routine is two sibling top-level
/// nodes — a `signature` (`f :: ...`) and a `function` (`f x = ...`) sharing the entity's
/// label. When the whole routine is added (or removed), the matcher emits BOTH a
/// `signature` and a `function` ADDITION/DELETION; the review only needs the `function`
/// one, so the sibling `signature` change is folded away as scaffold churn. Returns the
/// NOISE_SUPPRESSED evidence group when anything was suppressed.
pub(crate) fn suppress_haskell_signature_function_sibling_churn_drafts(
    changes: &mut Vec<ChangeDraft<'_>>,
) -> Option<Value> {
    let added_fn_labels: HashSet<&str> = changes
        .iter()
        .filter(|change| change.change_type == "ADDITION")
        .filter_map(|change| change.new_node)
        .filter(|node| node.node_type == "function" && !node.label.is_empty())
        .map(|node| node.label.as_str())
        .collect();
    let deleted_fn_labels: HashSet<&str> = changes
        .iter()
        .filter(|change| change.change_type == "DELETION")
        .filter_map(|change| change.old_node)
        .filter(|node| node.node_type == "function" && !node.label.is_empty())
        .map(|node| node.label.as_str())
        .collect();
    if added_fn_labels.is_empty() && deleted_fn_labels.is_empty() {
        return None;
    }

    let mut old_labels = Vec::new();
    let mut new_labels = Vec::new();
    let mut old_ids = Vec::new();
    let mut new_ids = Vec::new();
    let mut suppressed = 0usize;
    changes.retain(|change| {
        let suppress = match change.change_type {
            "ADDITION" => change.new_node.is_some_and(|node| {
                node.node_type == "signature" && added_fn_labels.contains(node.label.as_str())
            }),
            "DELETION" => change.old_node.is_some_and(|node| {
                node.node_type == "signature" && deleted_fn_labels.contains(node.label.as_str())
            }),
            _ => false,
        };
        if suppress {
            if let Some(node) = change.old_node {
                collect_subtree_labels_and_ids(node, &mut old_labels, &mut old_ids);
            }
            if let Some(node) = change.new_node {
                collect_subtree_labels_and_ids(node, &mut new_labels, &mut new_ids);
            }
            suppressed += 1;
        }
        !suppress
    });
    if suppressed == 0 {
        return None;
    }
    Some(json!({
        "kind": "NOISE_SUPPRESSED",
        "raw_change_indices": [],
        "old_labels": dedup_preserve(old_labels),
        "new_labels": dedup_preserve(new_labels),
        "old_node_ids": dedup_preserve(old_ids),
        "new_node_ids": dedup_preserve(new_ids),
        "confidence": 0.75,
        "rule_id": "presentation.haskell.suppress_signature_function_sibling_churn",
        "metadata": {"index_space": "presentation_input", "suppressed_count": suppressed},
    }))
}

/// Port of python `presentation.py::_suppress_dart_signature_body_scaffold_churn`
/// (language-gated to dart by the caller). Dart's extractor exposes a function's
/// `function_signature` and `function_body` as siblings, so adding/editing a routine
/// leaks body scaffolding (`function_body`/`block`/`return_statement` add/delete, and
/// expression-body churn whose tokens are all rename labels) around the anchored
/// signature. Suppress that scaffold; keep the signature addition and real edits.
pub(crate) fn suppress_dart_signature_body_scaffold_churn_drafts(
    changes: &mut Vec<ChangeDraft<'_>>,
) -> Option<Value> {
    let mut rename_labels: HashSet<String> = HashSet::new();
    for change in changes.iter() {
        if change.change_type == "REFACTORING"
            && change
                .refactoring_kind
                .is_some_and(|kind| kind.contains("RENAME"))
        {
            if let Some(node) = change.old_node {
                if !node.label.is_empty() {
                    rename_labels.insert(node.label.clone());
                }
            }
            if let Some(node) = change.new_node {
                if !node.label.is_empty() {
                    rename_labels.insert(node.label.clone());
                }
            }
        }
    }

    let mut old_labels = Vec::new();
    let mut new_labels = Vec::new();
    let mut old_ids = Vec::new();
    let mut new_ids = Vec::new();
    let mut suppressed = 0usize;
    changes.retain(|change| {
        let is_add = change.change_type == "ADDITION";
        let is_del = change.change_type == "DELETION";
        if !is_add && !is_del {
            return true;
        }
        let Some(node) = (if is_add { change.new_node } else { change.old_node }) else {
            return true;
        };
        let node_type = node.node_type.as_str();
        // The blanket function_body/block/return_statement arm died with the dart parser's
        // signature+body MERGE (#46/#72): whole-definition adds now travel as one
        // function_definition wrapper, so a bare body/block/return add-or-delete is a REAL
        // body edit that must surface (the trivial-body matrix's dart case). Only the
        // rename-label expression-churn arm remains.
        // Body kinds join the RENAME-CONDITIONED arm only (never blanket): a body node whose
        // concrete subtree labels are ALL rename participants is churn from a parameter
        // rename; one with any other content is a real edit and survives (trivial-body case).
        let suppress = if matches!(
            node_type,
            "additive_expression" | "multiplicative_expression" | "function_expression_body"
                | "return_statement" | "function_body" | "block"
        ) && !rename_labels.is_empty()
        {
            let mut labels = Vec::new();
            let mut ids = Vec::new();
            collect_subtree_labels_and_ids(node, &mut labels, &mut ids);
            let concrete: HashSet<&String> =
                labels.iter().filter(|label| **label != node.node_type).collect();
            !concrete.is_empty() && concrete.iter().all(|label| rename_labels.contains(*label))
        } else {
            false
        };
        if suppress {
            if is_add {
                collect_subtree_labels_and_ids(node, &mut new_labels, &mut new_ids);
            } else {
                collect_subtree_labels_and_ids(node, &mut old_labels, &mut old_ids);
            }
            suppressed += 1;
        }
        !suppress
    });
    if suppressed == 0 {
        return None;
    }
    Some(json!({
        "kind": "NOISE_SUPPRESSED",
        "raw_change_indices": [],
        "old_labels": dedup_preserve(old_labels),
        "new_labels": dedup_preserve(new_labels),
        "old_node_ids": dedup_preserve(old_ids),
        "new_node_ids": dedup_preserve(new_ids),
        "confidence": 0.75,
        "rule_id": "presentation.dart.suppress_signature_body_scaffold_churn",
        "metadata": {"index_space": "presentation_input", "suppressed_count": suppressed},
    }))
}

/// Collect a node's own + descendant labels (non-empty) and ids, mirroring python
/// `_labels`/`_node_ids` (self first, then descendants).
pub(crate) fn collect_subtree_labels_and_ids(
    node: &SemanticNode,
    labels: &mut Vec<String>,
    ids: &mut Vec<String>,
) {
    if !node.label.is_empty() {
        labels.push(node.label.clone());
    }
    ids.push(node.id.clone());
    for descendant in node.descendants() {
        if !descendant.label.is_empty() {
            labels.push(descendant.label.clone());
        }
        ids.push(descendant.id.clone());
    }
}

pub(crate) fn suppress_descendant_noise_drafts(changes: &mut Vec<ChangeDraft<'_>>) {
    // Suppression roots are split BY TYPE. A change inside a subtree that was DELETED (or,
    // on the new side, ADDED) is noise covered by that whole-subtree change. A change inside
    // a subtree that MOVED rode along with the move and is likewise noise. The distinction
    // that matters (issue #57 Root A): a MOVE whose OLD site is inside a DELETED container is
    // NOT noise — the node escaped the deletion, which IS the edit (csharp `name = "guest"`
    // leaving a collapsed `if`). So a MOVE/REORDER is suppressed on the old side only by a
    // MOVED ancestor, never by a mere deletion; a DELETION is still suppressed by a deleted
    // OR moved ancestor.
    let mut old_delete_roots: HashSet<String> = HashSet::new();
    let mut old_move_roots: HashSet<String> = HashSet::new();
    let mut new_add_roots: HashSet<String> = HashSet::new();
    let mut new_move_roots: HashSet<String> = HashSet::new();
    for change in changes.iter() {
        match change.change_type {
            "MOVE" => {
                if let Some(node) = change.old_node {
                    collect_suppression_root_id_node(node, &mut old_move_roots);
                }
                if let Some(node) = change.new_node {
                    collect_suppression_root_id_node(node, &mut new_move_roots);
                }
            }
            "DELETION" => {
                if let Some(node) = change.old_node {
                    collect_suppression_root_id_node(node, &mut old_delete_roots);
                }
            }
            "ADDITION" => {
                if let Some(node) = change.new_node {
                    collect_suppression_root_id_node(node, &mut new_add_roots);
                }
            }
            _ => {}
        }
    }
    if old_delete_roots.is_empty()
        && old_move_roots.is_empty()
        && new_add_roots.is_empty()
        && new_move_roots.is_empty()
    {
        return;
    }
    changes.retain(|change| match change.change_type {
        "DELETION" => !change.old_node.is_some_and(|node| {
            has_suppressed_ancestor_id(&node.id, &old_delete_roots)
                || has_suppressed_ancestor_id(&node.id, &old_move_roots)
        }),
        "MOVE" | "REORDER" => {
            let rode_along_old = change
                .old_node
                .is_some_and(|node| has_suppressed_ancestor_id(&node.id, &old_move_roots));
            let inside_new = change.new_node.is_some_and(|node| {
                has_suppressed_ancestor_id(&node.id, &new_move_roots)
                    || has_suppressed_ancestor_id(&node.id, &new_add_roots)
            });
            !(rode_along_old || inside_new)
        }
        "ADDITION" => !change.new_node.is_some_and(|node| {
            has_suppressed_ancestor_id(&node.id, &new_add_roots)
                || has_suppressed_ancestor_id(&node.id, &new_move_roots)
        }),
        _ => true,
    });
}

pub(crate) fn collect_suppression_root_id_node(node: &SemanticNode, result: &mut HashSet<String>) {
    if node.node_type == "module" {
        return;
    }
    result.insert(node.id.clone());
}

pub(crate) fn suppress_parent_modifications_drafts(changes: &mut Vec<ChangeDraft<'_>>, language: &str) {
    // Member-level anchoring first: when an entity-container MODIFICATION (e.g.
    // method_declaration 'Bar') covers a bare LEAF modification, the member keeps the
    // review anchor and the leaf drops — MODIFICATION integer_literal('2') alone loses
    // the "which member changed" context (csharp file-scoped-namespace contract; python
    // presents the member level). Non-leaf children remain the more precise draft and
    // the parent drops, as before.
    //
    // Statement-profile anchoring (issue #57 delphi): for asm/bash/delphi the REVIEW UNIT is
    // the keyed statement — a statement-review-container MODIFICATION (`WriteLn(old)` ->
    // `WriteLn(new)`) anchors the review and its covered descendant MODIFICATIONs fold
    // (leaf or not), instead of the generic cascade preferring the innermost leaf.
    let statement_profiled = statement_profile_language(language);
    let mut drop_leaf = vec![false; changes.len()];
    for parent_index in 0..changes.len() {
        let parent = &changes[parent_index];
        if parent.change_type != "MODIFICATION" {
            continue;
        }
        let (Some(parent_old), Some(parent_new)) = (parent.old_node, parent.new_node) else {
            continue;
        };
        let statement_anchor =
            statement_profiled && is_statement_review_container(parent_old, language);
        if !is_entity_container_type(parent_old.node_type.as_str()) && !statement_anchor {
            continue;
        }
        for child_index in 0..changes.len() {
            if child_index == parent_index {
                continue;
            }
            let child = &changes[child_index];
            if child.change_type != "MODIFICATION" {
                continue;
            }
            let child_is_leaf = child
                .old_node
                .map_or(true, |node| node.children.is_empty())
                && child.new_node.map_or(true, |node| node.children.is_empty());
            // Statement anchors fold ALL covered descendant mods; entity anchors only leaves.
            if !child_is_leaf && !statement_anchor {
                continue;
            }
            let covered = child.old_node.is_some_and(|node| {
                let ids: HashSet<&str> = std::iter::once(node.id.as_str()).collect();
                has_descendant_id_node(parent_old, &ids)
            }) || child.new_node.is_some_and(|node| {
                let ids: HashSet<&str> = std::iter::once(node.id.as_str()).collect();
                has_descendant_id_node(parent_new, &ids)
            });
            if covered {
                drop_leaf[child_index] = true;
            }
        }
    }
    let mut index = 0;
    changes.retain(|_| {
        let keep = !drop_leaf[index];
        index += 1;
        keep
    });

    let mut modified_old_ids: HashSet<&str> = HashSet::new();
    let mut modified_new_ids: HashSet<&str> = HashSet::new();
    for change in changes.iter() {
        if change.change_type != "MODIFICATION" {
            continue;
        }
        if let Some(node) = change.old_node {
            modified_old_ids.insert(node.id.as_str());
        }
        if let Some(node) = change.new_node {
            modified_new_ids.insert(node.id.as_str());
        }
    }
    changes.retain(|change| {
        if change.change_type != "MODIFICATION" {
            return true;
        }
        // A statement-review-container MODIFICATION is the anchored review unit — never
        // dropped in favor of a more nested draft (its descendants folded above).
        if statement_profiled
            && change
                .old_node
                .is_some_and(|node| is_statement_review_container(node, language))
        {
            return true;
        }
        let has_modified_old_descendant = change
            .old_node
            .is_some_and(|node| has_descendant_id_node(node, &modified_old_ids));
        let has_modified_new_descendant = change
            .new_node
            .is_some_and(|node| has_descendant_id_node(node, &modified_new_ids));
        !(has_modified_old_descendant || has_modified_new_descendant)
    });
}

pub(crate) fn has_descendant_id_node(node: &SemanticNode, ids: &HashSet<&str>) -> bool {
    node.children
        .iter()
        .any(|child| ids.contains(child.id.as_str()) || has_descendant_id_node(child, ids))
}

/// True when any hierarchical-id ancestor of `id` (its `a.b.c` prefixes) is in `set`.
/// Node ids are `parent.child_index`, so `0.1` is an ancestor of `0.1.2`.
pub(crate) fn id_ancestor_in_set(id: &str, set: &HashSet<&str>) -> bool {
    let mut cursor = id;
    while let Some((prefix, _)) = cursor.rsplit_once('.') {
        if set.contains(prefix) {
            return true;
        }
        cursor = prefix;
    }
    false
}

pub(crate) fn suppress_candidate_move_noise_drafts(changes: &mut Vec<ChangeDraft<'_>>) {
    let modified_pairs: HashSet<(&str, &str)> = changes
        .iter()
        .filter(|change| change.change_type == "MODIFICATION")
        .filter_map(|change| Some((change.old_node?.id.as_str(), change.new_node?.id.as_str())))
        .collect();
    // A MOVE of a "noise" statement type is usually a within-container REORDER — noise. But
    // when the statement relocated OUT of a container that is itself being DELETED (csharp
    // `name = "guest"` leaving a collapsed `if`, issue #57 Root A), the move IS the edit and
    // must survive. Distinguish by whether the moved node's old container is deleted.
    let deleted_old_ids: HashSet<&str> = changes
        .iter()
        .filter(|change| change.change_type == "DELETION")
        .filter_map(|change| change.old_node.map(|node| node.id.as_str()))
        .collect();

    changes.retain(|change| {
        if change.change_type != "MOVE" {
            return true;
        }
        if let (Some(old_node), Some(new_node)) = (change.old_node, change.new_node) {
            if modified_pairs.contains(&(old_node.id.as_str(), new_node.id.as_str())) {
                return false;
            }
            if id_ancestor_in_set(old_node.id.as_str(), &deleted_old_ids) {
                return true;
            }
            // A NESTING-DEPTH change is a block-membership change (python dedent: `print(x)`
            // leaving the function body; the off-side rule makes that a behavior change) — the
            // move IS the edit. Positional displacement (something inserted above) never changes
            // a node's depth, so this keeps genuine relocations without readmitting shift noise.
            let depth = |id: &str| id.matches('.').count();
            if depth(old_node.id.as_str()) != depth(new_node.id.as_str()) {
                return true;
            }
        }
        let node = change.old_node.or(change.new_node);
        !node.is_some_and(|node| is_move_noise_type(node.node_type.as_str()))
    });
}

/// A renamed container is not a relocation (issue #57 payoff, ts function-rename): the
/// rename pair was promoted from unmatched DELETE+ADD, so its content-matched CHILDREN
/// (formal_parameters, statement_block) carry parent ids from two "different" containers
/// and the edit script flags them as MOVEs. A MOVE whose endpoints are both inside the
/// SAME refactoring pair's subtrees is stationary — the container renamed around it.
pub(crate) fn suppress_child_moves_under_refactoring_pair_drafts(changes: &mut Vec<ChangeDraft<'_>>) {
    let pairs: Vec<(String, String)> = changes
        .iter()
        .filter(|change| change.change_type == "REFACTORING")
        .filter_map(|change| {
            Some((
                format!("{}.", change.old_node?.id),
                format!("{}.", change.new_node?.id),
            ))
        })
        .collect();
    if pairs.is_empty() {
        return;
    }
    changes.retain(|change| {
        if change.change_type != "MOVE" {
            return true;
        }
        let (Some(old_node), Some(new_node)) = (change.old_node, change.new_node) else {
            return true;
        };
        !pairs.iter().any(|(old_prefix, new_prefix)| {
            old_node.id.starts_with(old_prefix.as_str())
                && new_node.id.starts_with(new_prefix.as_str())
        })
    });
}

pub(crate) fn suppress_child_modifications_under_preferred_parent_drafts(changes: &mut Vec<ChangeDraft<'_>>) {
    let mut preferred_old_ids: HashSet<String> = HashSet::new();
    let mut preferred_new_ids: HashSet<String> = HashSet::new();
    for change in changes.iter() {
        if change.change_type != "MODIFICATION" {
            continue;
        }
        let node = change.old_node.or(change.new_node);
        if !node.is_some_and(|node| is_preferred_modification_parent_type(node.node_type.as_str()))
        {
            continue;
        }
        if let Some(old_node) = change.old_node {
            collect_descendant_ids_node(old_node, &mut preferred_old_ids);
        }
        if let Some(new_node) = change.new_node {
            collect_descendant_ids_node(new_node, &mut preferred_new_ids);
        }
    }
    if preferred_old_ids.is_empty() && preferred_new_ids.is_empty() {
        return;
    }
    changes.retain(|change| {
        if change.change_type != "MODIFICATION" {
            return true;
        }
        if is_label_changing_identifier_modification_draft(change) {
            return true;
        }
        let node = change.old_node.or(change.new_node);
        if node.is_some_and(|node| is_preferred_modification_parent_type(node.node_type.as_str())) {
            return true;
        }
        !(change
            .old_node
            .is_some_and(|node| preferred_old_ids.contains(node.id.as_str()))
            || change
                .new_node
                .is_some_and(|node| preferred_new_ids.contains(node.id.as_str())))
    });
}

pub(crate) fn collect_descendant_ids_node(node: &SemanticNode, result: &mut HashSet<String>) {
    if node.node_type == "module" {
        return;
    }
    for child in &node.children {
        result.insert(child.id.clone());
        collect_descendant_ids_node(child, result);
    }
}

pub(crate) fn is_label_changing_identifier_modification_draft(change: &ChangeDraft<'_>) -> bool {
    let Some(old_node) = change.old_node else {
        return false;
    };
    let Some(new_node) = change.new_node else {
        return false;
    };
    old_node.node_type == "identifier"
        && new_node.node_type == "identifier"
        && old_node.label != new_node.label
}

/// Content leaves are leaves carrying a real token: containers default their label to
/// their node_type, so `label != node_type` separates `identifier(println)` from
/// structural shells like `statement_list(statement_list)`.
pub(crate) fn collect_content_leaf_ids<'a>(node: &'a SemanticNode, out: &mut Vec<&'a str>) {
    if node.children.is_empty() {
        if !node.label.is_empty() && node.label != node.node_type {
            out.push(node.id.as_str());
        }
        return;
    }
    for child in &node.children {
        collect_content_leaf_ids(child, out);
    }
}

pub(crate) fn collect_subtree_ids<'a>(node: &'a SemanticNode, out: &mut HashSet<&'a str>) {
    out.insert(node.id.as_str());
    for child in &node.children {
        collect_subtree_ids(child, out);
    }
}

pub(crate) fn suppress_candidate_container_noise_drafts<'a>(
    changes: &mut Vec<ChangeDraft<'a>>,
    matching: &[MatchPair<'a>],
) {
    // NB: reorder fate is NOT decided here. This function used to also drop every
    // same-identity named-entity REORDER, duplicating (and pre-empting) the discrimination
    // in suppress_low_signal_reorders_drafts — so a genuine swap of two functions lost its
    // evidence in the refine pass and the diff read style-only (issue #12). Reorders flow
    // through to finalize, where insertion shifts are suppressed (with the NOISE_SUPPRESSED
    // group + count) and genuine relocations promote to MOVE.
    //
    // A bare `block`/`module` ADDITION/DELETION is wrapper noise ONLY when its content is
    // accounted for: every content leaf in its subtree is either matched to the other side
    // (re-wrapped unchanged code) or carried by another surviving non-container draft.
    // When the container is the SOLE carrier of unmatched content — an empty body gaining
    // its first statements — the earlier blanket drop erased the diff entirely (go
    // trivial-body -> 0 changes, issue #57 pilot). The first guard attempt ("no other
    // draft covers it", commit e8b5ce8) kept re-wrap containers whose children were
    // matched-but-not-drafted and broke 7 truthiness contracts; the matched-leaf test is
    // what distinguishes those from genuine new content.
    let matched_old: HashSet<&str> = matching
        .iter()
        .map(|pair| pair.old_node.id.as_str())
        .collect();
    let matched_new: HashSet<&str> = matching
        .iter()
        .map(|pair| pair.new_node.id.as_str())
        .collect();

    let is_container_draft = |change: &ChangeDraft<'_>| {
        matches!(change.change_type, "ADDITION" | "DELETION")
            && change
                .new_node
                .or(change.old_node)
                .is_some_and(|node| matches!(node.node_type.as_str(), "module" | "block"))
    };

    let mut drop = vec![false; changes.len()];
    for index in 0..changes.len() {
        if !is_container_draft(&changes[index]) {
            continue;
        }
        let change = &changes[index];
        let is_addition = change.change_type == "ADDITION";
        let node = if is_addition {
            change.new_node
        } else {
            change.old_node
        };
        let Some(node) = node else {
            drop[index] = true;
            continue;
        };
        let matched = if is_addition {
            &matched_new
        } else {
            &matched_old
        };
        // Coverage comes from other NON-container drafts on the same side; counting other
        // containers would let two nested container drafts cover each other and both drop.
        let mut covered: HashSet<&str> = HashSet::new();
        for (other_index, other) in changes.iter().enumerate() {
            if other_index == index || is_container_draft(other) {
                continue;
            }
            let other_node = if is_addition {
                other.new_node
            } else {
                other.old_node
            };
            if let Some(other_node) = other_node {
                collect_subtree_ids(other_node, &mut covered);
            }
        }
        let mut content_leaves: Vec<&str> = Vec::new();
        for child in &node.children {
            collect_content_leaf_ids(child, &mut content_leaves);
        }
        let sole_carrier = content_leaves
            .iter()
            .any(|leaf_id| !matched.contains(leaf_id) && !covered.contains(leaf_id));
        drop[index] = !sole_carrier;
    }
    let mut index = 0;
    changes.retain(|_| {
        let keep = !drop[index];
        index += 1;
        keep
    });
}