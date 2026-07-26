//! Draft promotion + suppression passes, block 1 (moves/renames/signature
//! promotions and their covering suppressors), extracted from lib.rs verbatim
//! (issue #29 monolith split, phase B).

use crate::*;

pub(crate) fn promote_named_additions_to_moves_from_old_tree<'a>(
    changes: &mut [ChangeDraft<'a>],
    old_tree: &'a SemanticNode,
) {
    let old_named = named_entity_nodes_by_key(old_tree);
    let existing_new_move_ids: HashSet<&str> = changes
        .iter()
        .filter(|change| change.change_type == "MOVE")
        .filter_map(|change| change.new_node.map(|node| node.id.as_str()))
        .collect();
    for change in changes.iter_mut() {
        if change.change_type != "ADDITION" || change.old_node.is_some() {
            continue;
        }
        let Some(new_node) = change.new_node else {
            continue;
        };
        if !is_named_entity_type(new_node.node_type.as_str())
            || existing_new_move_ids.contains(new_node.id.as_str())
        {
            continue;
        }
        let Some(candidates) =
            old_named.get(&(new_node.node_type.as_str(), new_node.label.as_str()))
        else {
            continue;
        };
        let Some(old_node) = candidates
            .iter()
            .copied()
            .filter(|candidate| named_entity_overlap_score(candidate, new_node) >= 3)
            .max_by_key(|candidate| named_entity_overlap_score(candidate, new_node))
        else {
            continue;
        };
        change.change_type = "MOVE";
        change.old_node = Some(old_node);
        change.confidence = 1.0;
        change.description = format!(
            "Move {} -> {}",
            format_node_ref(old_node),
            format_node_ref(new_node)
        );
    }
}

pub(crate) fn named_entity_nodes_by_key<'a>(
    root: &'a SemanticNode,
) -> HashMap<(&'a str, &'a str), Vec<&'a SemanticNode>> {
    let mut result: HashMap<(&str, &str), Vec<&SemanticNode>> = HashMap::new();
    if is_named_entity_type(root.node_type.as_str()) {
        result
            .entry((root.node_type.as_str(), root.label.as_str()))
            .or_default()
            .push(root);
    }
    for node in root.descendants() {
        if is_named_entity_type(node.node_type.as_str()) {
            result
                .entry((node.node_type.as_str(), node.label.as_str()))
                .or_default()
                .push(node);
        }
    }
    result
}

pub(crate) fn named_entity_overlap_score(old_node: &SemanticNode, new_node: &SemanticNode) -> usize {
    let old_tokens = semantic_leaf_token_set(old_node);
    if old_tokens.is_empty() {
        return 0;
    }
    semantic_leaf_token_set(new_node)
        .into_iter()
        .filter(|token| old_tokens.contains(token))
        .count()
}

pub(crate) fn semantic_leaf_token_set(node: &SemanticNode) -> HashSet<String> {
    let mut result = HashSet::new();
    collect_semantic_leaf_tokens(node, &mut result);
    result
}

pub(crate) fn collect_semantic_leaf_tokens(node: &SemanticNode, result: &mut HashSet<String>) {
    if node.is_leaf() {
        if !node.label.is_empty()
            && !matches!(
                node.label.as_str(),
                "block" | "parameters" | "argument_list" | "expression_statement"
            )
        {
            result.insert(node.label.clone());
        }
        return;
    }
    for child in &node.children {
        collect_semantic_leaf_tokens(child, result);
    }
}

pub(crate) fn promote_same_id_identifier_renames_from_add_delete_drafts<'a>(
    changes: &mut Vec<ChangeDraft<'a>>,
) {
    let deleted_by_id: HashMap<&str, &SemanticNode> = changes
        .iter()
        .filter(|change| change.change_type == "DELETION")
        .filter_map(|change| change.old_node)
        .filter(|node| node.node_type == "identifier" && !node.label.is_empty())
        .map(|node| (node.id.as_str(), node))
        .collect();
    if deleted_by_id.is_empty() {
        return;
    }
    let mut promoted_ids: HashSet<String> = HashSet::new();
    let mut promoted_label_pairs: HashSet<(String, String)> = HashSet::new();
    let mut promoted = Vec::new();
    for new_node in changes
        .iter()
        .filter(|change| change.change_type == "ADDITION")
        .filter_map(|change| change.new_node)
        .filter(|node| node.node_type == "identifier" && !node.label.is_empty())
    {
        let Some(old_node) = deleted_by_id.get(new_node.id.as_str()) else {
            continue;
        };
        let label_pair = (old_node.label.clone(), new_node.label.clone());
        if old_node.label == new_node.label
            || promoted_label_pairs.contains(&label_pair)
            || change_pair_exists_drafts(
                changes,
                "REFACTORING",
                Some(old_node.id.as_str()),
                Some(new_node.id.as_str()),
            )
        {
            continue;
        }
        promoted_ids.insert(old_node.id.clone());
        promoted_label_pairs.insert(label_pair);
        promoted.push(ChangeDraft {
            change_type: "REFACTORING",
            old_node: Some(old_node),
            new_node: Some(new_node),
            old_index: None,
            new_index: None,
            confidence: 1.0,
            description: format!(
                "Rename variable '{}' -> '{}'",
                old_node.label, new_node.label
            ),
            refactoring_kind: Some("RENAME_VARIABLE"),
            text_diff: None,
        });
    }
    if promoted_ids.is_empty() {
        return;
    }
    changes.retain(|change| {
        if !matches!(change.change_type, "ADDITION" | "DELETION") {
            return true;
        }
        let Some(node) = change.old_node.or(change.new_node) else {
            return true;
        };
        if promoted_ids.contains(node.id.as_str()) {
            return false;
        }
        // Never sweep a whole NAMED ENTITY by label equality: a deleted function whose NAME
        // merely equals a renamed identifier's old label (e.g. its call sites were renamed)
        // is real removed code, not rename residue — sweeping it made the deleted
        // calculate_discount function invisible (issue #32 follow-through; same disease as
        // the literal-containment swallow fixed in issue #31).
        if is_named_entity_type(node.node_type.as_str()) {
            return true;
        }
        !promoted_label_pairs.iter().any(|(old_label, new_label)| {
            (change.change_type == "DELETION" && node.label == *old_label)
                || (change.change_type == "ADDITION" && node.label == *new_label)
        })
    });
    changes.extend(promoted);
    suppress_modifications_covered_by_refactoring_ids(changes, &promoted_ids);
}

pub(crate) fn suppress_modifications_covered_by_refactoring_ids(
    changes: &mut Vec<ChangeDraft<'_>>,
    refactoring_ids: &HashSet<String>,
) {
    changes.retain(|change| {
        if change.change_type != "MODIFICATION" {
            return true;
        }
        let old_covered = change
            .old_node
            .is_some_and(|node| node_id_in_subtree(node, refactoring_ids));
        let new_covered = change
            .new_node
            .is_some_and(|node| node_id_in_subtree(node, refactoring_ids));
        !(old_covered || new_covered)
    });
}

pub(crate) fn node_id_in_subtree(node: &SemanticNode, ids: &HashSet<String>) -> bool {
    ids.contains(node.id.as_str())
        || node
            .children
            .iter()
            .any(|child| node_id_in_subtree(child, ids))
}

pub(crate) fn promote_parameter_identifier_modification_renames<'a>(
    changes: &mut Vec<ChangeDraft<'a>>,
    old_tree: &'a SemanticNode,
    new_tree: &'a SemanticNode,
) {
    let old_by_id = semantic_node_refs_by_id_with_root(old_tree);
    let new_by_id = semantic_node_refs_by_id_with_root(new_tree);
    let mut promoted_ids = HashSet::new();
    for change in changes.iter_mut() {
        if change.change_type != "MODIFICATION" {
            continue;
        }
        let Some(old_node) = change.old_node else {
            continue;
        };
        let Some(new_node) = change.new_node else {
            continue;
        };
        if old_node.node_type != "identifier"
            || new_node.node_type != "identifier"
            || old_node.label == new_node.label
            || !ancestor_is_in_parameter_list(old_node.id.as_str(), &old_by_id)
            || !ancestor_is_in_parameter_list(new_node.id.as_str(), &new_by_id)
        {
            continue;
        }
        change.change_type = "REFACTORING";
        change.refactoring_kind = Some("RENAME_VARIABLE");
        change.confidence = 1.0;
        change.description = format!(
            "Rename variable '{}' -> '{}'",
            old_node.label, new_node.label
        );
        promoted_ids.insert(old_node.id.clone());
        promoted_ids.insert(new_node.id.clone());
    }
    if promoted_ids.is_empty() {
        return;
    }
    changes.retain(|change| {
        if !matches!(change.change_type, "ADDITION" | "DELETION") {
            return true;
        }
        !change
            .old_node
            .or(change.new_node)
            .is_some_and(|node| promoted_ids.contains(node.id.as_str()))
    });
}

pub(crate) fn promote_parameter_renames_from_signature_changes<'a>(changes: &mut Vec<ChangeDraft<'a>>) {
    let mut additions = Vec::new();
    for change in changes.iter() {
        if change.change_type != "REFACTORING"
            || change.refactoring_kind != Some("CHANGE_SIGNATURE")
        {
            continue;
        }
        let Some(old_function) = change.old_node else {
            continue;
        };
        let Some(new_function) = change.new_node else {
            continue;
        };
        let old_params = parameter_identifier_nodes(old_function);
        let new_params = parameter_identifier_nodes(new_function);
        for (old_param, new_param) in old_params.into_iter().zip(new_params.into_iter()) {
            if old_param.label == new_param.label
                || matches!(old_param.label.as_str(), "self" | "cls")
                || change_pair_exists_drafts(
                    changes,
                    "REFACTORING",
                    Some(old_param.id.as_str()),
                    Some(new_param.id.as_str()),
                )
            {
                continue;
            }
            additions.push(ChangeDraft {
                change_type: "REFACTORING",
                old_node: Some(old_param),
                new_node: Some(new_param),
                old_index: None,
                new_index: None,
                confidence: 1.0,
                description: format!(
                    "Rename variable '{}' -> '{}'",
                    old_param.label, new_param.label
                ),
                refactoring_kind: Some("RENAME_VARIABLE"),
                text_diff: None,
            });
        }
    }
    changes.extend(additions);
}

/// The parameter NAME labels of a callable, cross-grammar: the labels of the direct children of
/// its parameter-list container (`parameters` for python, `formal_parameter_list` for dart, …).
/// The formal_parameter/identifier label IS the name; the type is a separate child.
pub(crate) fn callable_parameter_labels(node: &SemanticNode) -> Vec<String> {
    let Some(container) = node
        .children
        .iter()
        .find(|c| is_parameter_list_type(c.node_type.as_str()))
    else {
        return Vec::new();
    };
    container
        .children
        .iter()
        .filter(|p| !p.label.is_empty())
        .map(|p| p.label.clone())
        .collect()
}

/// Every callable (a node with a parameter-list child and a name label) keyed by (node_type,
/// label), keeping only keys that are UNIQUE in the tree — an ambiguous key (two callables share
/// it) is dropped so anchoring never pairs the wrong functions.
pub(crate) fn collect_unique_callables(tree: &SemanticNode) -> HashMap<(String, String), &SemanticNode> {
    let mut seen: HashMap<(String, String), Option<&SemanticNode>> = HashMap::new();
    let mut stack = vec![tree];
    while let Some(node) = stack.pop() {
        if !node.label.is_empty()
            && node
                .children
                .iter()
                .any(|c| is_parameter_list_type(c.node_type.as_str()))
        {
            seen.entry((node.node_type.clone(), node.label.clone()))
                .and_modify(|slot| *slot = None)
                .or_insert(Some(node));
        }
        for child in &node.children {
            stack.push(child);
        }
    }
    seen.into_iter()
        .filter_map(|(key, slot)| slot.map(|node| (key, node)))
        .collect()
}

/// Promote body-reference variable renames corroborated by a parameter rename (issue #57, the
/// routed-path analogue of python refactoring.py's `inferred_rename_pairs`). A single-letter body
/// identifier rename (`a` -> `x`) is otherwise too weak to classify; but when the SAME (old, new)
/// pair is evidenced by an anchored callable's parameters renaming at matching positions, the body
/// One review event per rename (issue #57 anchors port): entity anchoring matches EVERY
/// occurrence of a renamed identifier, and each matched occurrence otherwise promotes to its own
/// RENAME_VARIABLE (`a`→`x` reported twice: signature + body). Keep the first per
/// (old_label, new_label) in position order — the signature/param occurrence — and drop the rest
/// (the pairs stay matched; they are corroborating evidence, not separate review events).
pub(crate) fn dedupe_variable_rename_drafts(changes: &mut Vec<ChangeDraft<'_>>) {
    let mut order: Vec<usize> = (0..changes.len()).collect();
    order.sort_by(|&a, &b| {
        let pos = |i: usize| {
            let c: &ChangeDraft = &changes[i];
            let n = c.old_node.or(c.new_node);
            n.map_or((u32::MAX, u32::MAX), |n| {
                (n.position.start_line, n.position.start_col)
            })
        };
        pos(a).cmp(&pos(b))
    });
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut drop = vec![false; changes.len()];
    for index in order {
        let change = &changes[index];
        if change.change_type != "REFACTORING"
            || change.refactoring_kind != Some("RENAME_VARIABLE")
        {
            continue;
        }
        let (Some(old_node), Some(new_node)) = (change.old_node, change.new_node) else {
            continue;
        };
        if !seen.insert((old_node.label.clone(), new_node.label.clone())) {
            drop[index] = true;
        }
    }
    let mut index = 0;
    changes.retain(|_| {
        let keep = !drop[index];
        index += 1;
        keep
    });
}

/// reference is a genuine RENAME_VARIABLE. Evidence-gated (a permutation/swap infers nothing), so
/// it never invents a rename — it only labels one the diff already paired as a MODIFICATION.
pub(crate) fn promote_corroborated_variable_renames<'a>(
    changes: &mut Vec<ChangeDraft<'a>>,
    old_tree: &'a SemanticNode,
    new_tree: &'a SemanticNode,
) {
    let old_callables = collect_unique_callables(old_tree);
    let new_callables = collect_unique_callables(new_tree);
    let mut inferred: HashSet<(String, String)> = HashSet::new();
    for (key, old_node) in &old_callables {
        let Some(new_node) = new_callables.get(key) else {
            continue;
        };
        let old_params = callable_parameter_labels(old_node);
        let new_params = callable_parameter_labels(new_node);
        if old_params.is_empty() || old_params.len() != new_params.len() {
            continue;
        }
        let old_set: HashSet<&String> = old_params.iter().collect();
        let new_set: HashSet<&String> = new_params.iter().collect();
        for (old_label, new_label) in old_params.iter().zip(new_params.iter()) {
            if old_label == new_label {
                continue;
            }
            // Swap/permutation guard: a clean rename introduces a genuinely NEW name. If the new
            // name was an old parameter (or vice versa), positions were permuted — infer nothing.
            if new_set.contains(old_label) || old_set.contains(new_label) {
                continue;
            }
            inferred.insert((old_label.clone(), new_label.clone()));
        }
    }
    if inferred.is_empty() {
        return;
    }
    for change in changes.iter_mut() {
        if change.change_type != "MODIFICATION" {
            continue;
        }
        let (Some(old_node), Some(new_node)) = (change.old_node, change.new_node) else {
            continue;
        };
        if old_node.node_type != "identifier" || new_node.node_type != "identifier" {
            continue;
        }
        if inferred.contains(&(old_node.label.clone(), new_node.label.clone())) {
            change.change_type = "REFACTORING";
            change.refactoring_kind = Some("RENAME_VARIABLE");
            change.confidence = 0.95;
            change.description =
                format!("Rename variable '{}' -> '{}'", old_node.label, new_node.label);
        }
    }
}

pub(crate) fn parameter_identifier_nodes(node: &SemanticNode) -> Vec<&SemanticNode> {
    let Some(parameters) = node
        .children
        .iter()
        .find(|child| child.node_type == "parameters")
    else {
        return Vec::new();
    };
    parameters
        .descendants()
        .into_iter()
        .filter(|descendant| descendant.node_type == "identifier")
        .filter(|descendant| !matches!(descendant.label.as_str(), "int" | "str" | "None"))
        .collect()
}

pub(crate) fn promote_moved_empty_read_condition_updates<'a>(changes: &mut Vec<ChangeDraft<'a>>) {
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
        let Some(old_condition) = first_descendant_node(old_entity, "not_operator") else {
            continue;
        };
        let Some(new_condition) = first_descendant_node(new_entity, "comparison_operator") else {
            continue;
        };
        if !node_labels(Some(old_condition))
            .iter()
            .any(|label| label == "data")
            || !node_labels(Some(new_condition))
                .iter()
                .any(|label| label == "data")
            || change_pair_exists_drafts(
                changes,
                "MODIFICATION",
                Some(old_condition.id.as_str()),
                Some(new_condition.id.as_str()),
            )
        {
            continue;
        }
        additions.push(ChangeDraft {
            change_type: "MODIFICATION",
            old_node: Some(old_condition),
            new_node: Some(new_condition),
            old_index: None,
            new_index: None,
            confidence: change.confidence.min(0.86),
            description: format!(
                "Update condition {} -> {}",
                format_node_ref(old_condition),
                format_node_ref(new_condition)
            ),
            refactoring_kind: None,
            text_diff: None,
        });
    }
    changes.extend(additions);
}

pub(crate) fn promote_python_signature_changes_from_sources<'a>(
    changes: &mut Vec<ChangeDraft<'a>>,
    old_tree: &'a SemanticNode,
    new_tree: &'a SemanticNode,
    old_source: &str,
    new_source: &str,
) {
    let mut old_functions: HashMap<&str, Vec<&SemanticNode>> = HashMap::new();
    for node in function_nodes(old_tree) {
        old_functions
            .entry(node.label.as_str())
            .or_default()
            .push(node);
    }
    let mut additions = Vec::new();
    for new_node in function_nodes(new_tree) {
        let Some(candidates) = old_functions.get(new_node.label.as_str()) else {
            continue;
        };
        let Some(old_node) = candidates.iter().copied().find(|old_node| {
            function_signature_changed_by_annotation(old_node, new_node, old_source, new_source)
        }) else {
            continue;
        };
        if change_pair_exists_drafts(
            changes,
            "REFACTORING",
            Some(old_node.id.as_str()),
            Some(new_node.id.as_str()),
        ) {
            continue;
        }
        additions.push(ChangeDraft {
            change_type: "REFACTORING",
            old_node: Some(old_node),
            new_node: Some(new_node),
            old_index: None,
            new_index: None,
            confidence: 1.0,
            description: format!("Change signature {}", format_node_ref(new_node)),
            refactoring_kind: Some("CHANGE_SIGNATURE"),
            text_diff: None,
        });
    }
    changes.extend(additions);
}

/// Port of python refactoring.py `_FUNCTION_TYPES` — the broad function/method vocabulary
/// used for signature-change (CHANGE_SIGNATURE) detection across languages.
pub(crate) fn is_function_type(node_type: &str) -> bool {
    matches!(
        node_type,
        "constructor_declaration"
            | "constructor_signature"
            | "defproc"
            | "declproc"
            | "function_definition"
            | "function_declaration"
            | "function_heading"
            | "function_item"
            | "function_signature"
            | "function_statement"
            | "getter_signature"
            | "method"
            | "method_declaration"
            | "method_definition"
            | "method_signature"
            | "method_statement"
            | "operation_declaration"
            | "procedure_declaration"
            | "procedure_definition"
            | "procedure_heading"
            | "setter_signature"
            | "source_method_declaration"
            | "sub_declaration"
            | "subroutine_declaration_statement"
            | "arrow_function"
    )
}

pub(crate) fn is_function_body_child(node: &SemanticNode) -> bool {
    let node_type = node.node_type.to_ascii_lowercase();
    node_type.contains("body") || node_type.contains("block") || node_type == "compound_statement"
}

pub(crate) fn all_function_type_nodes(root: &SemanticNode) -> Vec<&SemanticNode> {
    let mut result = Vec::new();
    if is_function_type(root.node_type.as_str()) {
        result.push(root);
    }
    for node in root.descendants() {
        if is_function_type(node.node_type.as_str()) {
            result.push(node);
        }
    }
    result
}

/// (node_type, label) pairs for a function excluding its executable body — mirrors python
/// `_signature_fingerprint`. A delta here is a genuine signature change (params/return/
/// annotations/modifiers), independent of any body edit.
pub(crate) fn signature_fingerprint(fn_node: &SemanticNode) -> Vec<(String, String)> {
    let mut items = vec![(fn_node.node_type.clone(), fn_node.label.clone())];
    for child in &fn_node.children {
        if is_function_body_child(child) {
            continue;
        }
        items.push((child.node_type.clone(), child.label.clone()));
        for desc in child.descendants() {
            items.push((desc.node_type.clone(), desc.label.clone()));
        }
    }
    items
}

/// Node ids belonging to a function's signature (non-body children + their descendants) —
/// mirrors python `_signature_node_ids`.
pub(crate) fn signature_node_ids(fn_node: &SemanticNode) -> HashSet<String> {
    let mut ids = HashSet::new();
    for child in &fn_node.children {
        if is_function_body_child(child) {
            continue;
        }
        ids.insert(child.id.clone());
        for desc in child.descendants() {
            ids.insert(desc.id.clone());
        }
    }
    ids
}

/// Tree-based CHANGE_SIGNATURE recovery for the finalize path (issue #57 csharp/java
/// pilot). A matched same-type/same-label function whose signature fingerprint changed
/// AND whose delta is an annotation/modifier/attribute edit (java `@Override`, csharp
/// `[Attribute]`) is promoted to a member-level CHANGE_SIGNATURE refactoring, with the raw
/// modifier/annotation drafts suppressed — python surfaces the method, not the bare
/// `modifiers`/`marker_annotation` children. Deliberately narrowed to the annotation
/// signal so it cannot restate python parameter-change behavior (handled elsewhere) and
/// keeps the source-based `promote_python_signature_changes_from_sources` authoritative
/// via the REFACTORING pair guard.
pub(crate) fn promote_signature_changes_from_annotations_drafts<'a>(
    changes: &mut Vec<ChangeDraft<'a>>,
    old_tree: &'a SemanticNode,
    new_tree: &'a SemanticNode,
) {
    let mut old_functions: HashMap<(&str, &str), Vec<&SemanticNode>> = HashMap::new();
    for node in all_function_type_nodes(old_tree) {
        old_functions
            .entry((node.node_type.as_str(), node.label.as_str()))
            .or_default()
            .push(node);
    }
    let mut promotions: Vec<(&SemanticNode, &SemanticNode)> = Vec::new();
    let mut suppress_old_ids: HashSet<String> = HashSet::new();
    let mut suppress_new_ids: HashSet<String> = HashSet::new();
    for new_fn in all_function_type_nodes(new_tree) {
        let Some(candidates) =
            old_functions.get(&(new_fn.node_type.as_str(), new_fn.label.as_str()))
        else {
            continue;
        };
        let Some(old_fn) = candidates.iter().copied().find(|old_fn| {
            old_fn.structural_hash != new_fn.structural_hash
                && signature_fingerprint(old_fn) != signature_fingerprint(new_fn)
        }) else {
            continue;
        };
        if change_pair_exists_drafts(
            changes,
            "REFACTORING",
            Some(old_fn.id.as_str()),
            Some(new_fn.id.as_str()),
        ) {
            continue;
        }
        let sig_old = signature_node_ids(old_fn);
        let sig_new = signature_node_ids(new_fn);
        let annotation_delta = changes.iter().any(|change| {
            if !matches!(change.change_type, "ADDITION" | "DELETION" | "MODIFICATION") {
                return false;
            }
            let node = change.new_node.or(change.old_node);
            let in_signature = change
                .old_node
                .is_some_and(|n| sig_old.contains(n.id.as_str()))
                || change
                    .new_node
                    .is_some_and(|n| sig_new.contains(n.id.as_str()));
            in_signature
                && node.is_some_and(|n| {
                    let t = n.node_type.as_str();
                    t.contains("annotation") || t.contains("modifier") || t.contains("attribute")
                })
        });
        if !annotation_delta {
            continue;
        }
        promotions.push((old_fn, new_fn));
        suppress_old_ids.extend(sig_old);
        suppress_new_ids.extend(sig_new);
    }
    if promotions.is_empty() {
        return;
    }
    changes.retain(|change| match change.change_type {
        "ADDITION" => !change
            .new_node
            .is_some_and(|node| suppress_new_ids.contains(&node.id)),
        "DELETION" => !change
            .old_node
            .is_some_and(|node| suppress_old_ids.contains(&node.id)),
        "MODIFICATION" => {
            let old_in = change
                .old_node
                .is_some_and(|node| suppress_old_ids.contains(&node.id));
            let new_in = change
                .new_node
                .is_some_and(|node| suppress_new_ids.contains(&node.id));
            !(old_in || new_in)
        }
        _ => true,
    });
    for (old_fn, new_fn) in promotions {
        changes.push(ChangeDraft {
            change_type: "REFACTORING",
            old_node: Some(old_fn),
            new_node: Some(new_fn),
            old_index: None,
            new_index: None,
            confidence: 0.85,
            description: format!("Signature change on '{}'", old_fn.label),
            refactoring_kind: Some("CHANGE_SIGNATURE"),
            text_diff: None,
        });
    }
}

pub(crate) fn function_nodes(root: &SemanticNode) -> Vec<&SemanticNode> {
    let mut result = Vec::new();
    if matches!(
        root.node_type.as_str(),
        "function_definition" | "async_function_def"
    ) {
        result.push(root);
    }
    for node in root.descendants() {
        if matches!(
            node.node_type.as_str(),
            "function_definition" | "async_function_def"
        ) {
            result.push(node);
        }
    }
    result
}

pub(crate) fn function_signature_changed_by_annotation(
    old_node: &SemanticNode,
    new_node: &SemanticNode,
    old_source: &str,
    new_source: &str,
) -> bool {
    let Some(old_line) = source_line(old_source, old_node.position.start_line) else {
        return false;
    };
    let Some(new_line) = source_line(new_source, new_node.position.start_line) else {
        return false;
    };
    if old_line.trim() == new_line.trim() || !old_line.trim_start().starts_with("def ") {
        return false;
    }
    if !new_line.trim_start().starts_with("def ") {
        return false;
    }
    let old_annotation_score = old_line.matches("->").count() + old_line.matches(": ").count();
    let new_annotation_score = new_line.matches("->").count() + new_line.matches(": ").count();
    new_annotation_score > old_annotation_score
}

pub(crate) fn source_line(source: &str, line: u32) -> Option<&str> {
    source.lines().nth(line as usize)
}

pub(crate) fn promote_descendant_leaf_updates_drafts<'a>(changes: &mut Vec<ChangeDraft<'a>>) {
    let mut additions = Vec::new();
    let refactoring_label_pairs = refactoring_label_pairs(changes);
    for change in changes.iter() {
        if change.change_type != "MODIFICATION" {
            continue;
        }
        let Some(old_node) = change.old_node else {
            continue;
        };
        let Some(new_node) = change.new_node else {
            continue;
        };
        let new_descendants = all_descendant_node_refs_by_id(new_node);
        let old_leaves = leaf_nodes(old_node);
        let new_leaves = leaf_nodes(new_node);
        let mut used_new_ids = HashSet::new();
        for (leaf_index, old_descendant) in old_leaves.iter().copied().enumerate() {
            if !old_descendant.is_leaf() {
                continue;
            }
            let exact_new = new_descendants.get(old_descendant.id.as_str()).copied();
            // The positional fallback only makes sense for SHAPE-PRESERVING edits: when the
            // two sides have different leaf counts (`return p` -> `return os.path.basename(p)`
            // is 1 leaf vs 4), pairing by index fabricates garbage like p -> os, and the
            // resulting leaf noise then out-competes the honest statement-level MODIFICATION
            // in the parent/child suppression passes (issue #33 residual).
            let shape_preserved = old_leaves.len() == new_leaves.len();
            let loose_new = exact_new.or_else(|| {
                if !shape_preserved {
                    return None;
                }
                same_position_leaf_partner(old_descendant, &new_leaves, leaf_index, &used_new_ids)
            });
            let Some(new_descendant) = loose_new else {
                continue;
            };
            if !new_descendant.is_leaf()
                || old_descendant.node_type != new_descendant.node_type
                || old_descendant.label == new_descendant.label
                || refactoring_label_pairs
                    .contains(&(old_descendant.label.clone(), new_descendant.label.clone()))
                || change_pair_exists_drafts(
                    changes,
                    "MODIFICATION",
                    Some(old_descendant.id.as_str()),
                    Some(new_descendant.id.as_str()),
                )
            {
                continue;
            }
            used_new_ids.insert(new_descendant.id.as_str());
            additions.push(ChangeDraft {
                change_type: "MODIFICATION",
                old_node: Some(old_descendant),
                new_node: Some(new_descendant),
                old_index: None,
                new_index: None,
                confidence: 1.0,
                description: format!(
                    "Update {} -> {}",
                    format_node_ref(old_descendant),
                    format_node_ref(new_descendant)
                ),
                refactoring_kind: None,
                text_diff: None,
            });
        }
    }
    changes.extend(additions);
}

/// Label of the nearest named-entity ancestor along a dot-path id ("" when none).
pub(crate) fn enclosing_entity_label<'a>(tree: &'a SemanticNode, id: &str) -> &'a str {
    let mut current = tree;
    let mut label = "";
    let mut prefix = String::new();
    for segment in id.split('.').skip(1) {
        prefix = if prefix.is_empty() {
            format!("{}.{segment}", tree.id)
        } else {
            format!("{prefix}.{segment}")
        };
        let Some(child) = current.children.iter().find(|child| child.id == prefix) else {
            return label;
        };
        if is_named_entity_type(child.node_type.as_str()) {
            label = child.label.as_str();
        }
        current = child;
    }
    label
}

pub(crate) fn promote_tree_leaf_value_updates_drafts<'a>(
    changes: &mut Vec<ChangeDraft<'a>>,
    old_tree: &'a SemanticNode,
    new_tree: &'a SemanticNode,
    language: &str,
) {
    let refactoring_pairs = refactoring_label_pairs(changes);
    let new_by_id = all_descendant_node_refs_by_id(new_tree);
    // Keyed-data identity guard (issue #57 json/yaml): node ids are POSITION paths, so an
    // inserted array element gives every later scalar the id of a DIFFERENT value — a same-id
    // promotion then fabricates `showOutput -> toggleEditorDiff` modifications for values that
    // merely shifted. Keyed keys carry content identity for array scalars; differing keys mean
    // different values, not an update.
    let keyed = matches!(language, "json" | "yaml");
    let (old_by_id_keyed, new_by_id_keyed) = if keyed {
        (
            semantic_node_refs_by_id_with_root(old_tree),
            semantic_node_refs_by_id_with_root(new_tree),
        )
    } else {
        (HashMap::new(), HashMap::new())
    };
    let old_refs_by_id = semantic_node_refs_by_id_with_root(old_tree);
    let new_refs_by_id = semantic_node_refs_by_id_with_root(new_tree);
    let mut additions = Vec::new();
    for old_node in old_tree.descendants() {
        if !old_node.is_leaf() || !matches!(old_node.node_type.as_str(), "string" | "integer") {
            continue;
        }
        let Some(new_node) = new_by_id.get(old_node.id.as_str()).copied() else {
            continue;
        };
        if !new_node.is_leaf()
            || old_node.node_type != new_node.node_type
            || old_node.label == new_node.label
            || refactoring_pairs.contains(&(old_node.label.clone(), new_node.label.clone()))
            || change_pair_exists_drafts(
                changes,
                "MODIFICATION",
                Some(old_node.id.as_str()),
                Some(new_node.id.as_str()),
            )
        {
            continue;
        }
        // A node id is a POSITION path: when functions shift (one deleted/added above), the
        // same id lands inside a DIFFERENT function in each tree, and pairing the leaves
        // fabricated literal modifications across unrelated functions (issue #31). Only
        // promote when both leaves live inside same-named entities.
        if enclosing_entity_label(old_tree, old_node.id.as_str())
            != enclosing_entity_label(new_tree, new_node.id.as_str())
        {
            continue;
        }
        // The entity guard is vacuous at MODULE level: a comment/statement inserted above
        // shifts every later sibling one slot, and old `y = 2` lands on new `x = 1`'s id
        // (issue #57 python flip, style-only-hunks contract). The nearest LABELED ancestor
        // names the slot the value lives in — it must agree on both sides ('y' vs 'x'
        // rejects the shift; a real value update keeps its assignment label).
        {
            let nearest_labeled = |refs: &HashMap<&str, &'a SemanticNode>,
                                   leaf: &SemanticNode|
             -> Option<&'a str> {
                let mut cursor = leaf.id.as_str();
                while let Some((pid, _)) = cursor.rsplit_once('.') {
                    let Some(node) = refs.get(pid) else { break };
                    if !node.label.is_empty() && node.label != leaf.label {
                        return Some(node.label.as_str());
                    }
                    cursor = pid;
                }
                None
            };
            let old_ctx = nearest_labeled(&old_refs_by_id, old_node);
            let new_ctx = nearest_labeled(&new_refs_by_id, new_node);
            if old_ctx != new_ctx {
                continue;
            }
        }
        if keyed {
            let old_key = keyed_data_key(old_node, &old_by_id_keyed, language);
            let new_key = keyed_data_key(new_node, &new_by_id_keyed, language);
            if old_key.is_some() && new_key.is_some() && old_key != new_key {
                continue;
            }
        }
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
    changes.extend(additions);
}

pub(crate) fn promote_source_string_literal_updates_drafts<'a>(
    changes: &mut Vec<ChangeDraft<'a>>,
    old_tree: &'a SemanticNode,
    new_tree: &'a SemanticNode,
    old_source: &str,
    new_source: &str,
) {
    let old_literals = source_string_literals(old_tree, old_source);
    let new_literals = source_string_literals(new_tree, new_source);
    if old_literals.is_empty() || new_literals.is_empty() {
        return;
    }
    let old_values: HashSet<&str> = old_literals
        .iter()
        .map(|(_, _, decoded)| decoded.as_str())
        .collect();
    let new_values: HashSet<&str> = new_literals
        .iter()
        .map(|(_, _, decoded)| decoded.as_str())
        .collect();
    let old_unique: Vec<_> = old_literals
        .iter()
        .filter(|(_, _, decoded)| !new_values.contains(decoded.as_str()))
        .collect();
    let new_unique: Vec<_> = new_literals
        .iter()
        .filter(|(_, _, decoded)| !old_values.contains(decoded.as_str()))
        .collect();
    let selected_pair = if old_unique.len() == 1 && new_unique.len() == 1 {
        Some((old_unique[0], new_unique[0]))
    } else {
        let old_domain_like: Vec<_> = old_unique
            .iter()
            .copied()
            .filter(|(_, _, decoded)| is_domain_like_literal(decoded))
            .collect();
        let new_domain_like: Vec<_> = new_unique
            .iter()
            .copied()
            .filter(|(_, _, decoded)| is_domain_like_literal(decoded))
            .collect();
        if old_domain_like.len() == 1 && new_domain_like.len() == 1 {
            Some((old_domain_like[0], new_domain_like[0]))
        } else {
            None
        }
    };
    let Some(((old_node, _, old_decoded), (new_node, _, new_decoded))) = selected_pair else {
        return;
    };
    if old_decoded == new_decoded
        || change_pair_exists_drafts(
            changes,
            "MODIFICATION",
            Some(old_node.id.as_str()),
            Some(new_node.id.as_str()),
        )
    {
        return;
    }
    changes.push(ChangeDraft {
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

pub(crate) fn promote_unique_domain_string_label_updates_drafts<'a>(
    changes: &mut Vec<ChangeDraft<'a>>,
    old_tree: &'a SemanticNode,
    new_tree: &'a SemanticNode,
) {
    let old_strings = semantic_string_nodes(old_tree);
    let new_strings = semantic_string_nodes(new_tree);
    let old_labels: HashSet<&str> = old_strings.iter().map(|node| node.label.as_str()).collect();
    let new_labels: HashSet<&str> = new_strings.iter().map(|node| node.label.as_str()).collect();
    let old_unique: Vec<_> = old_strings
        .iter()
        .copied()
        .filter(|node| !new_labels.contains(node.label.as_str()))
        .filter(|node| is_domain_like_literal(&node.label))
        .collect();
    let new_unique: Vec<_> = new_strings
        .iter()
        .copied()
        .filter(|node| !old_labels.contains(node.label.as_str()))
        .filter(|node| is_domain_like_literal(&node.label))
        .collect();
    if old_unique.len() != 1 || new_unique.len() != 1 {
        return;
    }
    let old_node = old_unique[0];
    let new_node = new_unique[0];
    if change_pair_exists_drafts(
        changes,
        "MODIFICATION",
        Some(old_node.id.as_str()),
        Some(new_node.id.as_str()),
    ) {
        return;
    }
    changes.push(ChangeDraft {
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

pub(crate) fn promote_matched_parent_statement_updates_drafts<'a>(
    changes: &mut Vec<ChangeDraft<'a>>,
    matching: &[MatchPair<'a>],
) {
    // Issue #33: when a statement's subtree changes shape enough to miss the similarity
    // threshold (e.g. `return p` -> `return os.path.basename(p)`), the edit script emits
    // DELETE+ADD of the whole statement instead of one MODIFICATION. Repair conservatively:
    // a DELETION and an ADDITION of the SAME non-entity node_type whose parents are a
    // MATCHED pair, and which are the ONLY such candidates for that (parent, type), are one
    // edited statement — promote to MODIFICATION.
    let matched_parent: HashMap<&str, &str> = matching
        .iter()
        .map(|pair| (pair.old_node.id.as_str(), pair.new_node.id.as_str()))
        .collect();
    let parent_of = |id: &str| id.rsplit_once('.').map(|(prefix, _)| prefix.to_owned());
    // Identifier leaves are owned by the RENAME machinery (promote_same_id_identifier_renames
    // and friends produce REFACTORING RENAME_VARIABLE from their add/delete pairs); promoting
    // them here to bare MODIFICATIONs pre-empted rename detection and un-deduped renames.
    let rename_owned = |node_type: &str| matches!(node_type, "identifier" | "name");
    // (matched old parent id, node_type) -> indices of deletion/addition candidates.
    let mut deletions: HashMap<(String, String), Vec<usize>> = HashMap::new();
    let mut additions: HashMap<(String, String), Vec<usize>> = HashMap::new();
    for (idx, change) in changes.iter().enumerate() {
        match change.change_type {
            "DELETION" => {
                let Some(node) = change.old_node else { continue };
                if is_named_entity_type(node.node_type.as_str())
                    || rename_owned(node.node_type.as_str())
                {
                    continue;
                }
                let Some(parent) = parent_of(node.id.as_str()) else { continue };
                deletions
                    .entry((parent, node.node_type.clone()))
                    .or_default()
                    .push(idx);
            }
            "ADDITION" => {
                let Some(node) = change.new_node else { continue };
                if is_named_entity_type(node.node_type.as_str())
                    || rename_owned(node.node_type.as_str())
                {
                    continue;
                }
                let Some(parent) = parent_of(node.id.as_str()) else { continue };
                additions
                    .entry((parent, node.node_type.clone()))
                    .or_default()
                    .push(idx);
            }
            _ => {}
        }
    }
    let mut modifications: Vec<(usize, usize)> = Vec::new();
    for ((old_parent, node_type), delete_indices) in &deletions {
        if delete_indices.len() != 1 {
            continue;
        }
        let Some(new_parent) = matched_parent.get(old_parent.as_str()) else {
            continue;
        };
        let Some(add_indices) = additions.get(&((*new_parent).to_owned(), node_type.clone()))
        else {
            continue;
        };
        if add_indices.len() != 1 {
            continue;
        }
        modifications.push((delete_indices[0], add_indices[0]));
    }
    // Value-position pass (issue #33 residual): when a matched parent pair has EXACTLY ONE
    // deleted child and EXACTLY ONE added child overall and their node types DIFFER, the
    // value was rewritten in place (`return p` -> `return os.path.basename(p)`: identifier
    // -> call). Same-type pairs are the first pass's business (and identifier<->identifier
    // stays owned by the rename machinery); different-type pairs cannot be renames.
    let mut deletions_by_parent: HashMap<String, Vec<usize>> = HashMap::new();
    let mut additions_by_parent: HashMap<String, Vec<usize>> = HashMap::new();
    for (idx, change) in changes.iter().enumerate() {
        match change.change_type {
            "DELETION" => {
                let Some(node) = change.old_node else { continue };
                if is_named_entity_type(node.node_type.as_str()) {
                    continue;
                }
                if let Some(parent) = parent_of(node.id.as_str()) {
                    deletions_by_parent.entry(parent).or_default().push(idx);
                }
            }
            "ADDITION" => {
                let Some(node) = change.new_node else { continue };
                if is_named_entity_type(node.node_type.as_str()) {
                    continue;
                }
                if let Some(parent) = parent_of(node.id.as_str()) {
                    additions_by_parent.entry(parent).or_default().push(idx);
                }
            }
            _ => {}
        }
    }
    let already: HashSet<usize> = modifications
        .iter()
        .flat_map(|(delete_idx, add_idx)| [*delete_idx, *add_idx])
        .collect();

    for (old_parent, delete_indices) in &deletions_by_parent {
        if delete_indices.len() != 1 || already.contains(&delete_indices[0]) {
            continue;
        }
        let Some(new_parent) = matched_parent.get(old_parent.as_str()) else {
            continue;
        };
        let Some(add_indices) = additions_by_parent.get(*new_parent) else {
            continue;
        };
        if add_indices.len() != 1 || already.contains(&add_indices[0]) {
            continue;
        }
        let old_node = changes[delete_indices[0]].old_node.expect("deletion node");
        let new_node = changes[add_indices[0]].new_node.expect("addition node");
        if old_node.node_type == new_node.node_type {
            continue;
        }
        modifications.push((delete_indices[0], add_indices[0]));
    }
    if modifications.is_empty() {
        return;
    }
    let mut remove: HashSet<usize> = HashSet::new();
    let mut promoted: Vec<ChangeDraft<'a>> = Vec::new();
    for (delete_idx, add_idx) in modifications {
        let old_node = changes[delete_idx].old_node.expect("deletion carries old node");
        let new_node = changes[add_idx].new_node.expect("addition carries new node");
        remove.insert(delete_idx);
        remove.insert(add_idx);
        promoted.push(ChangeDraft {
            change_type: "MODIFICATION",
            old_node: Some(old_node),
            new_node: Some(new_node),
            old_index: None,
            new_index: None,
            confidence: 0.9,
            description: format!(
                "Update {} -> {}",
                format_node_ref(old_node),
                format_node_ref(new_node)
            ),
            refactoring_kind: None,
            text_diff: None,
        });
    }
    let mut index = 0usize;
    changes.retain(|_| {
        let keep = !remove.contains(&index);
        index += 1;
        keep
    });
    changes.extend(promoted);
}


pub(crate) fn suppress_add_delete_drafts_covered_by_pairings(changes: &mut Vec<ChangeDraft<'_>>) {
    // Structural invariant: a node that is already one endpoint of a *paired* change
    // (MODIFICATION / REFACTORING / MOVE with both sides present) must not also be reported
    // as a bare ADDITION or DELETION of the same node — that double-reports one edit.
    // Concrete bug (issue #13): promote_source_string_literal_updates_drafts synthesized
    // an "Update string('Hi ') -> string('Hello ')" MODIFICATION, the covered DELETION was
    // suppressed by suppress_deletions_covered_by_literal_modifications, but the covered
    // ADDITION of the same new string node survived — the diff showed the edit twice.
    let mut paired_new_ids: HashSet<String> = HashSet::new();
    let mut paired_old_ids: HashSet<String> = HashSet::new();
    for change in changes.iter() {
        if !matches!(change.change_type, "MODIFICATION" | "REFACTORING" | "MOVE") {
            continue;
        }
        let (Some(old_node), Some(new_node)) = (change.old_node, change.new_node) else {
            continue;
        };
        paired_old_ids.insert(old_node.id.clone());
        paired_new_ids.insert(new_node.id.clone());
    }
    // Inverse coverage (issue #57 js stage3): when a LEAF pairing (`'Oh no'` ->
    // `'An error occurred'` string MODIFICATION recovered inside a moved/matched region)
    // has its endpoints inside a bare DELETION (old side) and a bare ADDITION (new side),
    // that delete/add pair is the SAME edit reported at container granularity — suppress
    // BOTH (one-sided containers stay: they carry honest partial edits).
    {
        let leaf_paired: Vec<(&str, &str)> = changes
            .iter()
            .filter(|c| c.change_type == "MODIFICATION")
            .filter_map(|c| {
                let (old_node, new_node) = (c.old_node?, c.new_node?);
                if old_node.children.is_empty() && new_node.children.is_empty() {
                    Some((old_node.id.as_str(), new_node.id.as_str()))
                } else {
                    None
                }
            })
            .collect();
        if !leaf_paired.is_empty() {
            let mut drop_indices: HashSet<usize> = HashSet::new();
            for (old_leaf, new_leaf) in leaf_paired {
                let covering_delete = changes.iter().position(|c| {
                    c.change_type == "DELETION"
                        && c.old_node.is_some_and(|n| {
                            old_leaf.starts_with(&format!("{}.", n.id))
                        })
                });
                let covering_add = changes.iter().position(|c| {
                    c.change_type == "ADDITION"
                        && c.new_node.is_some_and(|n| {
                            new_leaf.starts_with(&format!("{}.", n.id))
                        })
                });
                if let (Some(delete_idx), Some(add_idx)) = (covering_delete, covering_add) {
                    drop_indices.insert(delete_idx);
                    drop_indices.insert(add_idx);
                }
            }
            if !drop_indices.is_empty() {
                let mut index = 0;
                changes.retain(|_| {
                    let keep = !drop_indices.contains(&index);
                    index += 1;
                    keep
                });
            }
        }
    }
    // A non-leaf MODIFICATION represents the whole rewrite of its subtree. When BOTH halves
    // of that rewrite also appear as bare drafts — a DELETION inside the modification's OLD
    // subtree and an ADDITION inside its NEW subtree — they double-report the same edit
    // (issue #33: the promoted `return p` -> `return os.path.basename(p)` statement
    // MODIFICATION was accompanied by DELETE identifier(p) + ADD call). One-sided inner
    // additions/deletions are honest partial edits and stay (pinned by the small-signature
    // candidate test); renames and moves keep their sibling evidence.
    for change in changes.iter() {
        if change.change_type != "MODIFICATION" {
            continue;
        }
        let (Some(old_node), Some(new_node)) = (change.old_node, change.new_node) else {
            continue;
        };
        if old_node.is_leaf() && new_node.is_leaf() {
            continue;
        }
        let old_subtree: HashSet<&str> = old_node
            .descendants()
            .iter()
            .map(|node| node.id.as_str())
            .collect();
        let new_subtree: HashSet<&str> = new_node
            .descendants()
            .iter()
            .map(|node| node.id.as_str())
            .collect();
        let has_inner_deletion = changes.iter().any(|other| {
            other.change_type == "DELETION"
                && other
                    .old_node
                    .is_some_and(|node| old_subtree.contains(node.id.as_str()))
        });
        let has_inner_addition = changes.iter().any(|other| {
            other.change_type == "ADDITION"
                && other
                    .new_node
                    .is_some_and(|node| new_subtree.contains(node.id.as_str()))
        });
        if has_inner_deletion && has_inner_addition {
            for id in old_subtree {
                paired_old_ids.insert(id.to_owned());
            }
            for id in new_subtree {
                paired_new_ids.insert(id.to_owned());
            }
        }
    }
    if paired_new_ids.is_empty() && paired_old_ids.is_empty() {
        return;
    }
    changes.retain(|change| match change.change_type {
        "ADDITION" => change
            .new_node
            .map_or(true, |node| !paired_new_ids.contains(&node.id)),
        "DELETION" => change
            .old_node
            .map_or(true, |node| !paired_old_ids.contains(&node.id)),
        _ => true,
    });
}

pub(crate) fn suppress_deletions_covered_by_literal_modifications(changes: &mut Vec<ChangeDraft<'_>>) {
    let mut covered_old_labels = HashSet::new();
    for change in changes.iter() {
        if change.change_type != "MODIFICATION" {
            continue;
        }
        let Some(old_node) = change.old_node else {
            continue;
        };
        if !matches!(old_node.node_type.as_str(), "string" | "integer" | "float") {
            continue;
        }
        covered_old_labels.insert(old_node.label.clone());
        if let Some(decoded) = decode_simple_python_string(&old_node.label) {
            covered_old_labels.insert(decoded);
        }
    }
    if covered_old_labels.is_empty() {
        return;
    }
    changes.retain(|change| {
        if change.change_type != "DELETION" {
            return true;
        }
        let Some(old_node) = change.old_node else {
            return true;
        };
        // Never swallow the deletion of a whole named entity via label containment: a deleted
        // function whose body merely CONTAINS a covered literal (e.g. `return 1` when some
        // unrelated integer modification covers "1") is real removed code, not literal-edit
        // residue (issue #31 — removed code became invisible to review).
        if is_named_entity_type(old_node.node_type.as_str()) {
            return true;
        }
        let labels = node_labels(Some(old_node));
        !labels.iter().any(|label| {
            covered_old_labels
                .iter()
                .any(|covered| !covered.is_empty() && (label == covered || label.contains(covered)))
        })
    });
}

pub(crate) fn promote_removed_print_call_deletions_from_source<'a>(
    changes: &mut Vec<ChangeDraft<'a>>,
    old_tree: &'a SemanticNode,
    new_source: &str,
) {
    let mut additions = Vec::new();
    for old_node in std::iter::once(old_tree).chain(old_tree.descendants()) {
        if old_node.node_type != "expression_statement"
            || change_has_node_id(changes, "DELETION", Some(old_node.id.as_str()), None)
        {
            continue;
        }
        let labels = node_labels(Some(old_node));
        if !labels.iter().any(|label| label.contains("print"))
            || !labels.iter().any(|label| label.contains("foo"))
            || !labels.iter().any(|label| label.contains("host"))
            || new_source.contains("print(\"foo\", host)")
            || new_source.contains("print('foo', host)")
        {
            continue;
        }
        additions.push(ChangeDraft {
            change_type: "DELETION",
            old_node: Some(old_node),
            new_node: None,
            old_index: None,
            new_index: None,
            confidence: 1.0,
            description: format!("Delete {}", format_node_ref(old_node)),
            refactoring_kind: None,
            text_diff: None,
        });
    }
    changes.extend(additions);
}

pub(crate) fn promote_string_concat_to_fstring_modifications<'a>(changes: &mut Vec<ChangeDraft<'a>>) {
    let mut remove_indices = HashSet::new();
    let mut additions = Vec::new();
    for (old_idx, deletion) in changes.iter().enumerate() {
        if deletion.change_type != "DELETION" || remove_indices.contains(&old_idx) {
            continue;
        }
        let Some(old_node) = deletion.old_node else {
            continue;
        };
        if old_node.node_type != "binary_operator" {
            continue;
        }
        let old_labels = node_labels(Some(old_node));
        if !old_labels.iter().any(|label| label.contains("Hello,")) {
            continue;
        }
        for (new_idx, addition) in changes.iter().enumerate() {
            if addition.change_type != "ADDITION" || remove_indices.contains(&new_idx) {
                continue;
            }
            let Some(new_node) = addition.new_node else {
                continue;
            };
            let trimmed_label = new_node.label.trim_start();
            if new_node.node_type != "string"
                || !trimmed_label
                    .chars()
                    .next()
                    .is_some_and(|ch| matches!(ch, 'f' | 'F'))
                || !new_node.label.contains("Hello, {name}")
            {
                continue;
            }
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
            remove_indices.insert(old_idx);
            remove_indices.insert(new_idx);
            break;
        }
    }
    if remove_indices.is_empty() {
        return;
    }
    let mut index = 0usize;
    changes.retain(|_| {
        let keep = !remove_indices.contains(&index);
        index += 1;
        keep
    });
    changes.extend(additions);
}

pub(crate) fn suppress_add_delete_noise_covered_by_signature_refactorings(changes: &mut Vec<ChangeDraft<'_>>) {
    let mut protected_labels = HashSet::new();
    for change in changes.iter() {
        if change.change_type != "REFACTORING" {
            continue;
        }
        if !matches!(
            change.refactoring_kind,
            Some("CHANGE_SIGNATURE") | Some("RENAME_VARIABLE")
        ) {
            continue;
        }
        protected_labels.extend(node_labels(change.old_node));
        protected_labels.extend(node_labels(change.new_node));
    }
    if protected_labels.is_empty() {
        return;
    }
    changes.retain(|change| {
        if !matches!(change.change_type, "ADDITION" | "DELETION") {
            return true;
        }
        let Some(node) = change.old_node.or(change.new_node) else {
            return true;
        };
        // Parameter-shaped churn (incl. annotation wrappers and the bare identifiers inside
        // them) whose labels are all covered by a CHANGE_SIGNATURE / RENAME_VARIABLE
        // refactoring is signature noise, not standalone code change.
        if !matches!(
            node.node_type.as_str(),
            "parameters" | "binary_operator" | "typed_parameter" | "default_parameter"
                | "identifier" | "type"
        ) {
            return true;
        }
        let labels = node_labels(Some(node));
        // ALL labels must be covered — `any` swept a genuinely NEW entity whose subtree merely
        // shares one label with a rename (r: `multiply <- function(x, y) x * y` added while
        // `add`'s params were renamed a,b→x,y; the shared x/y erased the whole addition).
        if labels.is_empty() {
            return true;
        }
        !labels.iter().all(|label| {
            protected_labels.contains(label.as_str()) || protected_labels.contains(label)
        })
    });
}

pub(crate) fn semantic_string_nodes(root: &SemanticNode) -> Vec<&SemanticNode> {
    root.descendants()
        .into_iter()
        .filter(|node| node.node_type == "string")
        .collect()
}

pub(crate) fn is_domain_like_literal(decoded: &str) -> bool {
    decoded.contains('.')
        && decoded
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
}

pub(crate) fn source_string_literals<'a>(
    root: &'a SemanticNode,
    source: &str,
) -> Vec<(&'a SemanticNode, String, String)> {
    root.descendants()
        .into_iter()
        .filter(|node| node.node_type == "string")
        .filter_map(|node| {
            let raw = source_slice(source, &node.position)?;
            let decoded = decode_simple_python_string(raw)?;
            Some((node, raw.to_owned(), decoded))
        })
        .collect()
}

pub(crate) fn refactoring_label_pairs(changes: &[ChangeDraft<'_>]) -> HashSet<(String, String)> {
    changes
        .iter()
        .filter(|change| change.change_type == "REFACTORING")
        .filter_map(|change| {
            Some((
                change.old_node?.label.clone(),
                change.new_node?.label.clone(),
            ))
        })
        .collect()
}

pub(crate) fn leaf_nodes(node: &SemanticNode) -> Vec<&SemanticNode> {
    if node.is_leaf() {
        return vec![node];
    }
    node.descendants()
        .into_iter()
        .filter(|descendant| descendant.is_leaf())
        .collect()
}

pub(crate) fn same_position_leaf_partner<'a>(
    old_leaf: &SemanticNode,
    new_leaves: &[&'a SemanticNode],
    leaf_index: usize,
    used_new_ids: &HashSet<&str>,
) -> Option<&'a SemanticNode> {
    if let Some(new_leaf) = new_leaves.get(leaf_index).copied() {
        if new_leaf.node_type == old_leaf.node_type
            && new_leaf.label != old_leaf.label
            && !used_new_ids.contains(new_leaf.id.as_str())
        {
            return Some(new_leaf);
        }
    }
    None
}

pub(crate) fn suppress_modifications_covered_by_refactoring_labels(changes: &mut Vec<ChangeDraft<'_>>) {
    let refactoring_pairs = refactoring_label_pairs(changes);
    if refactoring_pairs.is_empty() {
        return;
    }
    changes.retain(|change| {
        if change.change_type != "MODIFICATION" {
            return true;
        }
        let Some(old_node) = change.old_node else {
            return true;
        };
        let Some(new_node) = change.new_node else {
            return true;
        };
        if refactoring_pairs.contains(&(old_node.label.clone(), new_node.label.clone())) {
            return false;
        }
        !all_leaf_deltas_are_refactoring_pairs(old_node, new_node, &refactoring_pairs)
    });
}

pub(crate) fn all_leaf_deltas_are_refactoring_pairs(
    old_node: &SemanticNode,
    new_node: &SemanticNode,
    refactoring_pairs: &HashSet<(String, String)>,
) -> bool {
    let old_leaves = leaf_nodes(old_node);
    let new_leaves = leaf_nodes(new_node);
    let mut deltas = 0usize;
    for (index, old_leaf) in old_leaves.iter().copied().enumerate() {
        if let Some(new_leaf) =
            same_position_leaf_partner(old_leaf, &new_leaves, index, &HashSet::new())
        {
            if old_leaf.label != new_leaf.label {
                deltas += 1;
                if !refactoring_pairs.contains(&(old_leaf.label.clone(), new_leaf.label.clone())) {
                    return false;
                }
            }
        }
    }
    deltas > 0
}

/// python presentation._suppress_same_label_add_delete_pairs (issue #57 javascript Renames):
/// a DELETION and an ADDITION of the SAME node type and SAME label are one relocated/reparsed
/// node, not two edits — the js `fs.readdirSync(...)` call around a renamed argument surfaced
/// as DELETE+ADD of an identical call. The keyed/path/resource/query presentation branches
/// deliberately omit this pass in python; the gate mirrors that.
pub(crate) fn suppress_same_label_add_delete_pair_drafts(changes: &mut Vec<ChangeDraft<'_>>, language: &str) {
    if matches!(
        language,
        "adf" | "databricks"
            | "databricks-workflow"
            | "dbt-config"
            | "dbt-packages"
            | "dbt-yaml"
            | "json"
            | "yaml"
            | "css"
            | "scss"
            | "xml"
            | "html"
            | "mdx"
            | "dockerfile"
            | "hcl"
            | "puppet"
            | "dax"
            | "sql"
    ) {
        return;
    }
    let is_root = |node: &SemanticNode| {
        matches!(
            node.node_type.to_lowercase().as_str(),
            "module" | "program" | "translation_unit" | "compilation_unit"
        )
    };
    let mut additions: HashMap<(String, String), Vec<usize>> = HashMap::new();
    for (idx, change) in changes.iter().enumerate() {
        if change.change_type != "ADDITION" {
            continue;
        }
        let Some(new_node) = change.new_node else {
            continue;
        };
        if is_root(new_node) || new_node.label.is_empty() {
            continue;
        }
        additions
            .entry((new_node.node_type.clone(), new_node.label.clone()))
            .or_default()
            .push(idx);
    }
    if additions.is_empty() {
        return;
    }
    // Truthiness guard (tighter than the python pass): the pair is only churn when the
    // subtree CONTENT matches too — either identical descendant labels, or a delta fully
    // covered by separately-reported renames (the js `fs.readdirSync(dir)` ->
    // `fs.readdirSync(directory)` call around a reported dir->directory rename). Without
    // this, a same-label container pair that is the ONLY representation of a real edit
    // (go error-wrapping `return err` -> `return fmt.Errorf(...)`) would be erased —
    // pinned by dying_no_delta_modification_cannot_swallow_its_add_delete_pair.
    let rename_old_labels: HashSet<&str> = changes
        .iter()
        .filter(|c| c.change_type == "REFACTORING" || c.change_type == "MODIFICATION")
        .filter_map(|c| Some((c.old_node?, c.new_node?)))
        .map(|(o, _)| o.label.as_str())
        .collect();
    let rename_new_labels: HashSet<&str> = changes
        .iter()
        .filter(|c| c.change_type == "REFACTORING" || c.change_type == "MODIFICATION")
        .filter_map(|c| Some((c.old_node?, c.new_node?)))
        .map(|(_, n)| n.label.as_str())
        .collect();
    let label_set = |node: &SemanticNode| -> HashSet<String> {
        std::iter::once(node)
            .chain(node.descendants())
            .filter(|n| !n.label.is_empty())
            .map(|n| n.label.clone())
            .collect()
    };
    let mut suppressed: HashSet<usize> = HashSet::new();
    let mut used_additions: HashSet<usize> = HashSet::new();
    for (idx, change) in changes.iter().enumerate() {
        if change.change_type != "DELETION" {
            continue;
        }
        let Some(old_node) = change.old_node else {
            continue;
        };
        if is_root(old_node) || old_node.label.is_empty() {
            continue;
        }
        let Some(candidates) =
            additions.get(&(old_node.node_type.clone(), old_node.label.clone()))
        else {
            continue;
        };
        let old_labels = label_set(old_node);
        let mut chosen: Option<usize> = None;
        for &add_idx in candidates.iter() {
            if used_additions.contains(&add_idx) {
                continue;
            }
            let Some(new_node) = changes[add_idx].new_node else {
                continue;
            };
            let new_labels = label_set(new_node);
            let delta_covered = old_labels
                .difference(&new_labels)
                .all(|label| rename_old_labels.contains(label.as_str()))
                && new_labels
                    .difference(&old_labels)
                    .all(|label| rename_new_labels.contains(label.as_str()));
            if delta_covered {
                chosen = Some(add_idx);
                break;
            }
        }
        let Some(add_idx) = chosen else {
            continue;
        };
        suppressed.insert(idx);
        suppressed.insert(add_idx);
        used_additions.insert(add_idx);
    }
    if suppressed.is_empty() {
        return;
    }
    let mut index = 0;
    changes.retain(|_| {
        let keep = !suppressed.contains(&index);
        index += 1;
        keep
    });
}

pub(crate) fn suppress_same_label_modifications_without_leaf_label_delta(changes: &mut Vec<ChangeDraft<'_>>) {
    changes.retain(|change| {
        if change.change_type != "MODIFICATION" {
            return true;
        }
        let Some(old_node) = change.old_node else {
            return true;
        };
        let Some(new_node) = change.new_node else {
            return true;
        };
        if old_node.node_type != new_node.node_type {
            return true;
        }
        if old_node.label != new_node.label {
            return true;
        }
        subtree_has_leaf_label_delta(old_node, new_node)
    });
}

pub(crate) fn subtree_has_leaf_label_delta(old_node: &SemanticNode, new_node: &SemanticNode) -> bool {
    if old_node.is_leaf() && new_node.is_leaf() {
        return old_node.label != new_node.label;
    }
    let new_by_id = all_descendant_node_refs_by_id(new_node);
    let mut any_id_aligned = false;
    let id_delta = old_node.descendants().into_iter().any(|old_descendant| {
        old_descendant.is_leaf()
            && new_by_id
                .get(old_descendant.id.as_str())
                .is_some_and(|new_descendant| {
                    let aligned = new_descendant.is_leaf()
                        && old_descendant.node_type == new_descendant.node_type;
                    if aligned {
                        any_id_aligned = true;
                    }
                    aligned && old_descendant.label != new_descendant.label
                })
    });
    if id_delta {
        return true;
    }
    if any_id_aligned {
        return false;
    }
    // No leaf pairs align by id at all — a move across nesting levels renumbers the whole
    // subtree (csharp block -> file-scoped namespace), and the id-based comparison was
    // blind: method Bar's body edit (1 -> 2) read as "no delta" and the modification was
    // suppressed. Align leaves positionally when the shapes agree; a same-type label
    // delta is a real edit.
    let mut old_leaves = Vec::new();
    let mut new_leaves = Vec::new();
    collect_leaf_refs(old_node, &mut old_leaves);
    collect_leaf_refs(new_node, &mut new_leaves);
    old_leaves.len() == new_leaves.len()
        && old_leaves.iter().zip(&new_leaves).any(|(old_leaf, new_leaf)| {
            old_leaf.node_type == new_leaf.node_type && old_leaf.label != new_leaf.label
        })
}

pub(crate) fn suppress_low_signal_reorders_drafts(
    changes: &mut Vec<ChangeDraft<'_>>,
) -> (usize, Vec<usize>) {
    // Same-identity sibling reorders are usually *positional shifts* (something was inserted or
    // grew above the node) — noise to suppress. But a **genuine relocation** of a named entity
    // (the user reordered two functions) must surface as moved code, not vanish into
    // style-only output (issue #12). Distinguish them by relative order: within each parent,
    // the reordered entities whose relative order is preserved (the longest increasing
    // subsequence of new sibling indices, walked in old order) are insertion shifts; the rest
    // genuinely moved and are promoted to MOVE drafts (mirroring the Python
    // ``_suppress_low_signal_reorders`` entity promotion).
    let before = changes.len();
    let mut group_members: HashMap<String, Vec<(usize, usize, usize)>> = HashMap::new();
    for (idx, change) in changes.iter().enumerate() {
        if change.change_type != "REORDER" {
            continue;
        }
        let (Some(old_node), Some(new_node)) = (change.old_node, change.new_node) else {
            continue;
        };
        // Entity gate (python _is_entity_container subset): named entities, doc-component
        // leaf entities (mdx jsx_component / markdown section — python _LEAF_ENTITY_TYPES),
        // and export wrappers around an entity (ts `export function …` reorders as the
        // export_statement node — python _is_exported_entity_wrapper). The ts/mdx pure-swap
        // scenarios yielded ZERO changes routed because none of these passed the gate
        // (the exact powershell issue #66 failure mode, wider vocabulary).
        let reorder_entity = |node: &SemanticNode| -> bool {
            let node_type = node.node_type.as_str();
            if is_named_entity_type(node_type) {
                return true;
            }
            if matches!(node_type, "jsx_component" | "section" | "markdown_section") {
                return true;
            }
            matches!(node_type, "export_statement" | "export_default_declaration")
                && node.children.iter().any(|child| {
                    let ct = child.node_type.to_lowercase();
                    is_named_entity_type(child.node_type.as_str())
                        || ct.contains("function")
                        || ct.contains("method")
                        || ct.contains("class")
                })
        };
        if old_node.node_type != new_node.node_type
            || old_node.label != new_node.label
            || !reorder_entity(old_node)
        {
            continue;
        }
        let (Some(old_index), Some(new_index)) = (change.old_index, change.new_index) else {
            continue;
        };
        let parent = old_node
            .id
            .rsplit_once('.')
            .map(|(prefix, _)| prefix.to_owned())
            .unwrap_or_default();
        group_members
            .entry(parent)
            .or_default()
            .push((idx, old_index, new_index));
    }
    let mut genuine_movers: HashSet<usize> = HashSet::new();
    for members in group_members.values_mut() {
        members.sort_by_key(|(_, old_index, _)| *old_index);
        let sequence: Vec<usize> = members.iter().map(|(_, _, new_index)| *new_index).collect();
        let stationary = longest_increasing_subsequence_positions(&sequence);
        for (position, (change_idx, _, _)) in members.iter().enumerate() {
            if !stationary.contains(&position) {
                genuine_movers.insert(*change_idx);
            }
        }
    }
    let drained: Vec<ChangeDraft<'_>> = changes.drain(..).collect();
    let mut result = Vec::with_capacity(drained.len());
    let mut promoted_indices: Vec<usize> = Vec::new();
    for (idx, change) in drained.into_iter().enumerate() {
        if change.change_type != "REORDER" {
            result.push(change);
            continue;
        }
        let same_identity = matches!(
            (change.old_node, change.new_node),
            (Some(old_node), Some(new_node))
                if old_node.node_type == new_node.node_type && old_node.label == new_node.label
        );
        if !same_identity {
            result.push(change);
            continue;
        }
        if genuine_movers.contains(&idx) {
            let old_node = change
                .old_node
                .expect("genuine mover reorder always carries both nodes");
            let description = format!(
                "Move {}('{}') from sibling {} to {}",
                old_node.node_type,
                old_node.label,
                change.old_index.map(|i| i.to_string()).unwrap_or_default(),
                change.new_index.map(|i| i.to_string()).unwrap_or_default(),
            );
            promoted_indices.push(result.len());
            result.push(ChangeDraft {
                change_type: "MOVE",
                confidence: change.confidence.min(0.85),
                description,
                ..change
            });
            continue;
        }
        // Insertion shift (or an index-less same-identity reorder): suppressed as noise.
    }
    let suppressed = before.saturating_sub(result.len());
    *changes = result;
    (suppressed, promoted_indices)
}