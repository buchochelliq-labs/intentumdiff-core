// Extracted verbatim from lib.rs (issue #85, lib.rs split round 2).
use crate::*;

pub(crate) fn keyed_generic_label(label: &str, node_type: &str) -> bool {
    let text = label.trim();
    if text.is_empty() {
        return true;
    }
    let lowered = text.to_lowercase();
    lowered == node_type.to_lowercase()
        || matches!(
            lowered.as_str(),
            "array" | "block_mapping" | "block_node" | "block_sequence" | "document"
                | "flow_mapping" | "flow_node" | "flow_sequence" | "job" | "object"
                | "pipeline" | "stream"
        )
}

/// python keyed_profiles.normalize_keyed_identity: strip matching quotes, drop a trailing
/// parenthesised suffix, drop everything from the first `:`, lowercase.
pub(crate) fn normalize_keyed_identity(label: &str) -> String {
    let mut text = label.trim();
    if text.len() >= 2
        && ((text.starts_with('"') && text.ends_with('"'))
            || (text.starts_with('\'') && text.ends_with('\'')))
    {
        text = text[1..text.len() - 1].trim();
    }
    let mut owned = text.to_string();
    if owned.ends_with(')') {
        if let Some(open) = owned.rfind('(') {
            owned.truncate(open);
            owned = owned.trim_end().to_string();
        }
    }
    if let Some(colon) = owned.find(':') {
        owned.truncate(colon);
        owned = owned.trim_end().to_string();
    }
    owned.to_lowercase()
}

pub(crate) fn keyed_parent_type<'a>(node: &SemanticNode, by_id: &HashMap<&str, &'a SemanticNode>) -> String {
    node.id
        .rsplit_once('.')
        .and_then(|(parent_id, _)| by_id.get(parent_id).copied())
        .map(|p| p.node_type.to_lowercase())
        .unwrap_or_default()
}

/// python `_json_ancestor_path` / `_yaml_ancestor_path` (per-language pair/item vocabularies).
pub(crate) fn keyed_ancestor_path(
    node: &SemanticNode,
    by_id: &HashMap<&str, &SemanticNode>,
    language: &str,
) -> Vec<String> {
    let mut labels: Vec<String> = Vec::new();
    let mut current = node.id.clone();
    while let Some((parent_id, _)) = current.rsplit_once('.') {
        if let Some(ancestor) = by_id.get(parent_id).copied() {
            let at = ancestor.node_type.to_lowercase();
            if language == "json" {
                if at == "pair" && !ancestor.label.is_empty() {
                    labels.push(normalize_keyed_identity(&ancestor.label));
                } else if at == "object" && keyed_parent_type(ancestor, by_id) == "array" {
                    labels.push(format!("[{}]", json_array_item_identity(ancestor)));
                }
            } else {
                if matches!(at.as_str(), "block_mapping_pair" | "flow_pair")
                    && !ancestor.label.is_empty()
                {
                    labels.push(normalize_keyed_identity(&ancestor.label));
                } else if matches!(at.as_str(), "block_sequence_item" | "flow_sequence_item")
                    && !ancestor.label.is_empty()
                {
                    let identity = normalize_keyed_identity(&ancestor.label);
                    if !identity.is_empty()
                        && !keyed_generic_label(&ancestor.label, &ancestor.node_type)
                    {
                        labels.push(format!("[{identity}]"));
                    }
                }
            }
        }
        current = parent_id.to_string();
    }
    labels.retain(|l| !l.is_empty());
    labels.reverse();
    labels
}


/// Identity for a json array ITEM: a real (non-generic, non-positional) label when present,
/// else the CONTENT (structural-hash prefix) — `[0]`/`[1]` position labels are not identities,
/// and keying by them makes an inserted element re-identify every later sibling (package.json
/// commands insert read as 4 bogus string MODIFICATIONs).
pub(crate) fn json_array_item_identity(node: &SemanticNode) -> String {
    let label = node.label.trim();
    let positional = label.len() >= 3
        && label.starts_with('[')
        && label.ends_with(']')
        && label[1..label.len() - 1].chars().all(|c| c.is_ascii_digit());
    if !label.is_empty() && !positional && !keyed_generic_label(label, &node.node_type) {
        return normalize_keyed_identity(label);
    }
    format!("#{}", &node.structural_hash[..node.structural_hash.len().min(12)])
}

/// python keyed_profiles._json_key / _yaml_key.
pub(crate) fn keyed_data_key(
    node: &SemanticNode,
    by_id: &HashMap<&str, &SemanticNode>,
    language: &str,
) -> Option<Vec<String>> {
    let node_type = node.node_type.to_lowercase();
    let (pair_types, item_types): (&[&str], &[&str]) = if language == "json" {
        (&["pair"], &[])
    } else {
        (
            &["block_mapping_pair", "flow_pair"],
            &["block_sequence_item", "flow_sequence_item"],
        )
    };

    if pair_types.contains(&node_type.as_str()) && !node.label.is_empty() {
        let mut key = vec![language.to_string(), "pair".to_string()];
        key.extend(keyed_ancestor_path(node, by_id, language));
        key.push(normalize_keyed_identity(&node.label));
        return Some(key);
    }

    if language == "json" && node_type == "object" {
        if keyed_parent_type(node, by_id) == "array" {
            let array_parent = nearest_ancestor_of_types(node.id.as_str(), by_id, &["array"]);
            if let Some(array_parent) = array_parent {
                let mut key = vec!["json".to_string(), "array_item".to_string()];
                key.extend(keyed_ancestor_path(array_parent, by_id, language));
                key.push(json_array_item_identity(node));
                return Some(key);
            }
        }
    }

    if item_types.contains(&node_type.as_str()) && !node.label.is_empty() {
        let identity = normalize_keyed_identity(&node.label);
        if !identity.is_empty() && !keyed_generic_label(&node.label, &node.node_type) {
            let mut key = vec![language.to_string(), "sequence_item".to_string()];
            key.extend(keyed_ancestor_path(node, by_id, language));
            key.push(identity);
            return Some(key);
        }
    }

    // Array/sequence SCALARS pair by CONTENT (label + same-label ordinal), not position — an
    // inserted element must not cross-pair later unchanged values into bogus MODIFICATIONs
    // (package.json activationEvents insert). Python reaches the same end shape via the
    // presentation repair (_repair_shifted_array_scalar_insertions); keying by identity is the
    // routed equivalent at the true source. A single pair VALUE still pairs by role below, so
    // an edited scalar value stays a MODIFICATION.
    if node.children.is_empty() && !node.label.is_empty() {
        let container_types: &[&str] = if language == "json" {
            &["array"]
        } else {
            &["block_sequence", "flow_sequence"]
        };
        let parent_type = keyed_parent_type(node, by_id);
        if container_types.contains(&parent_type.as_str()) {
            if let Some((parent_id, _)) = node.id.rsplit_once('.') {
                if let Some(container) = by_id.get(parent_id).copied() {
                    let mut ordinal = 0usize;
                    for sibling in &container.children {
                        if sibling.id == node.id {
                            break;
                        }
                        if sibling.node_type == node.node_type && sibling.label == node.label {
                            ordinal += 1;
                        }
                    }
                    // Content-faithful identity: normalize_keyed_identity strips from the first
                    // ':' (built for `key: value` labels), which collapses every `onCommand:*`
                    // scalar to the same identity — use the quote-stripped label verbatim.
                    let mut identity = node.label.trim();
                    if identity.len() >= 2
                        && ((identity.starts_with('"') && identity.ends_with('"'))
                            || (identity.starts_with('\'') && identity.ends_with('\'')))
                    {
                        identity = identity[1..identity.len() - 1].trim();
                    }
                    let mut key = vec![language.to_string(), "array_scalar".to_string()];
                    key.extend(keyed_ancestor_path(node, by_id, language));
                    key.push(node.node_type.to_lowercase());
                    key.push(identity.to_string());
                    key.push(ordinal.to_string());
                    return Some(key);
                }
            }
        }
    }

    if let Some(nearest_pair) = nearest_ancestor_of_types(node.id.as_str(), by_id, pair_types) {
        let branch = direct_child_under(nearest_pair.id.as_str(), node, by_id)?;
        let mut path = vec![language.to_string()];
        let role_is_key = nearest_pair
            .children
            .first()
            .is_some_and(|first| first.id == branch.id);
        let anc = keyed_ancestor_path(nearest_pair, by_id, language);
        let pair_identity = normalize_keyed_identity(&nearest_pair.label);
        let is_leaf = node.children.is_empty();
        let value_container = if language == "json" {
            matches!(node_type.as_str(), "object" | "array")
        } else {
            matches!(
                node_type.as_str(),
                "block_node" | "flow_node" | "block_mapping" | "flow_mapping"
            )
        };
        if role_is_key {
            if node.id == branch.id || is_leaf {
                path.push("key".to_string());
                path.extend(anc);
                path.push(pair_identity);
                path.push(node_type);
                return Some(path);
            }
            return None;
        }
        if node.id == branch.id || is_leaf || value_container {
            path.push("value".to_string());
            path.extend(anc);
            path.push(pair_identity);
            path.push(node_type);
            return Some(path);
        }
    }

    None
}

/// python keyed_profiles.is_keyed_data_review_container (json/yaml subset).
pub(crate) fn is_keyed_review_container(node: &SemanticNode) -> bool {
    let node_type = node.node_type.to_lowercase();
    if matches!(node_type.as_str(), "pair" | "block_mapping_pair" | "flow_pair") {
        return true;
    }
    if matches!(
        node_type.as_str(),
        "block_sequence_item" | "flow_sequence_item" | "object"
    ) {
        return !node.label.is_empty() && !keyed_generic_label(&node.label, &node.node_type);
    }
    false
}



/// python presentation._style_groups_from_suppression for the routed path (issue #57
/// javascript): the js/ts style rule relabels SUPPRESSION residue as an IGNORED_STYLE group.
/// The routed refine suppresses noise without per-pass evidence groups, so the residue is
/// computed as the DELTA between the raw edit-script drafts and the surviving ones. Provenance
/// is "suppression" (issue #51): relabelled residue, never an equivalence proof.
pub(crate) fn js_style_group_from_suppression(
    before: &[(String, Option<String>, Option<String>, Vec<String>, Vec<String>)],
    changes: &[ChangeDraft<'_>],
    language: &str,
) -> Option<(Value, Value)> {
    if !matches!(language, "javascript" | "typescript" | "tsx") {
        return None;
    }
    let surviving: HashSet<(String, Option<String>, Option<String>)> = changes
        .iter()
        .map(|c| {
            (
                c.change_type.to_string(),
                c.old_node.map(|n| n.id.clone()),
                c.new_node.map(|n| n.id.clone()),
            )
        })
        .collect();
    let survived_endpoint_old: HashSet<&String> = changes
        .iter()
        .filter_map(|c| c.old_node.map(|n| &n.id))
        .collect();
    let survived_endpoint_new: HashSet<&String> = changes
        .iter()
        .filter_map(|c| c.new_node.map(|n| &n.id))
        .collect();
    let mut old_labels: Vec<String> = Vec::new();
    let mut new_labels: Vec<String> = Vec::new();
    let mut old_ids: Vec<String> = Vec::new();
    let mut new_ids: Vec<String> = Vec::new();
    for (change_type, old_id, new_id, olabels, nlabels) in before {
        if surviving.contains(&(change_type.clone(), old_id.clone(), new_id.clone())) {
            continue;
        }
        // An endpoint that still participates in ANY surviving change was transformed
        // (promoted/paired), not suppressed — only fully-vanished drafts are style residue.
        if old_id.as_ref().is_some_and(|id| survived_endpoint_old.contains(id))
            || new_id.as_ref().is_some_and(|id| survived_endpoint_new.contains(id))
        {
            continue;
        }
        old_labels.extend(olabels.iter().cloned());
        new_labels.extend(nlabels.iter().cloned());
        old_ids.extend(old_id.iter().cloned());
        new_ids.extend(new_id.iter().cloned());
    }
    if old_ids.is_empty() && new_ids.is_empty() {
        return None;
    }
    // Only meaningful when at least one precise change survived to represent the edit.
    if !changes.iter().any(|c| c.change_type == "MODIFICATION") {
        return None;
    }
    let reason = "Formatting-only JavaScript call and argument wrapping were ignored because \
                  the review-level semantic changes are already represented by more precise \
                  changes.";
    let rule_id = "javascript.formatting.call_argument_wrapping_equivalence";
    old_labels.dedup();
    new_labels.dedup();
    let group = json!({
        "kind": "IGNORED_STYLE",
        "raw_change_indices": [],
        "old_labels": old_labels,
        "new_labels": new_labels,
        "old_node_ids": old_ids,
        "new_node_ids": new_ids,
        "confidence": 0.7,
        "rule_id": rule_id,
        "metadata": {
            "index_space": "mixed",
            "reason": reason,
            "equivalence_kind": "syntactic_trivia",
            "risk": "amber",
            "language": language,
        },
    });
    let ignored = json!({
        "language": language,
        "rule_id": rule_id,
        "reason": reason,
        "equivalence_kind": "syntactic_trivia",
        "risk": "amber",
        "provenance": "suppression",
    });
    Some((group, ignored))
}

/// python presentation._suppress_yaml_representation_equivalent_modifications (issue #57 yaml):
/// block and flow styles present the SAME representation graph (YAML 1.2 §3), so style-wrapper
/// churn (block_node↔flow_node MODIFICATIONs, scaffold item ADD/DELETEs with generic or
/// positional labels, same-label scalar MOVEs) suppresses with evidence. Anchors/aliases/tags
/// (&, *, !) change identity, not presentation — guarded out.
pub(crate) fn suppress_yaml_representation_equivalent_drafts(
    changes: &mut Vec<ChangeDraft<'_>>,
) -> Option<Value> {
    const SCAFFOLD: &[&str] = &[
        "block_node", "flow_node", "block_sequence", "flow_sequence", "block_mapping",
        "flow_mapping", "block_sequence_item", "flow_sequence_item",
    ];
    const SCALARS: &[&str] = &[
        "plain_scalar", "single_quote_scalar", "double_quote_scalar", "integer_scalar",
        "float_scalar", "boolean_scalar", "null_scalar", "string",
    ];
    fn cross_style(a: &str, b: &str) -> bool {
        matches!(
            (a, b),
            ("block_node", "flow_node") | ("flow_node", "block_node")
                | ("block_sequence", "flow_sequence") | ("flow_sequence", "block_sequence")
                | ("block_mapping", "flow_mapping") | ("flow_mapping", "block_mapping")
        )
    }
    fn has_identity_tokens(node: &SemanticNode) -> bool {
        std::iter::once(node)
            .chain(node.descendants())
            .any(|n| n.label.contains('&') || n.label.contains('*') || n.label.contains('!'))
    }
    let generic_or_equal = |old: &SemanticNode, new: &SemanticNode| {
        old.label == new.label
            || (old.label == old.node_type && new.label == new.node_type)
    };
    let mut suppressed_labels: Vec<String> = Vec::new();
    changes.retain(|change| {
        match change.change_type {
            "MODIFICATION" | "MOVE" => {
                let (Some(old_node), Some(new_node)) = (change.old_node, change.new_node) else {
                    return true;
                };
                if !generic_or_equal(old_node, new_node) {
                    return true;
                }
                if has_identity_tokens(old_node) || has_identity_tokens(new_node) {
                    return true;
                }
                let ot = old_node.node_type.to_lowercase();
                let nt = new_node.node_type.to_lowercase();
                let matched = if change.change_type == "MODIFICATION" {
                    cross_style(&ot, &nt) || (ot == nt && SCAFFOLD.contains(&ot.as_str()))
                } else {
                    // MOVEs: scalar positional shifts AND scaffold wrappers relocated by the
                    // presentation change (a flow_node "moving" when its container restyles).
                    (ot == nt && SCALARS.contains(&ot.as_str()))
                        || (cross_style(&ot, &nt)
                            || (ot == nt && SCAFFOLD.contains(&ot.as_str())))
                };
                if matched {
                    suppressed_labels.push(old_node.label.clone());
                    return false;
                }
                true
            }
            "DELETION" | "ADDITION" => {
                let node = change.old_node.or(change.new_node);
                let Some(node) = node else { return true };
                let nt = node.node_type.to_lowercase();
                if !SCAFFOLD.contains(&nt.as_str()) {
                    return true;
                }
                if has_identity_tokens(node) {
                    return true;
                }
                let positional = node.label.starts_with('[');
                if !(node.label == node.node_type || positional) {
                    return true;
                }
                suppressed_labels.push(node.label.clone());
                false
            }
            _ => true,
        }
    });
    if suppressed_labels.is_empty() {
        return None;
    }
    Some(json!({
        "kind": "NOISE_SUPPRESSED",
        "raw_change_indices": [],
        "old_labels": suppressed_labels,
        "new_labels": [],
        "old_node_ids": [],
        "new_node_ids": [],
        "confidence": 0.9,
        "rule_id": "presentation.suppress_yaml_representation_equivalent_modification",
        "metadata": {"equivalence": "yaml_representation_graph"},
    }))
}

/// python keyed_profiles.augment_keyed_data_matching: drop cross-key positional matches, then
/// pair unmatched keyed nodes by identical key (position order).
pub(crate) fn augment_keyed_data_matching<'a>(
    old_tree: &'a SemanticNode,
    new_tree: &'a SemanticNode,
    matching: Vec<MatchPair<'a>>,
    language: &str,
) -> Vec<MatchPair<'a>> {
    if !matches!(language, "json" | "yaml") {
        return matching;
    }
    let old_by_id = semantic_node_refs_by_id_with_root(old_tree);
    let new_by_id = semantic_node_refs_by_id_with_root(new_tree);
    let old_keys: HashMap<&str, Vec<String>> = old_by_id
        .iter()
        .filter_map(|(id, node)| keyed_data_key(node, &old_by_id, language).map(|k| (*id, k)))
        .collect();
    let new_keys: HashMap<&str, Vec<String>> = new_by_id
        .iter()
        .filter_map(|(id, node)| keyed_data_key(node, &new_by_id, language).map(|k| (*id, k)))
        .collect();

    let mut result: Vec<MatchPair<'a>> = Vec::new();
    let mut matched_old: HashSet<String> = HashSet::new();
    let mut matched_new: HashSet<String> = HashSet::new();
    for pair in matching {
        let ok = old_keys.get(pair.old_node.id.as_str());
        let nk = new_keys.get(pair.new_node.id.as_str());
        if (ok.is_some() || nk.is_some()) && (ok.is_none() || nk.is_none() || ok != nk) {
            continue;
        }
        if matched_old.contains(pair.old_node.id.as_str())
            || matched_new.contains(pair.new_node.id.as_str())
        {
            continue;
        }
        matched_old.insert(pair.old_node.id.clone());
        matched_new.insert(pair.new_node.id.clone());
        result.push(pair);
    }

    let mut new_by_key: HashMap<&Vec<String>, Vec<&SemanticNode>> = HashMap::new();
    for (id, key) in &new_keys {
        if matched_new.contains(*id) {
            continue;
        }
        if let Some(node) = new_by_id.get(id).copied() {
            new_by_key.entry(key).or_default().push(node);
        }
    }
    for nodes in new_by_key.values_mut() {
        nodes.sort_by(|a, b| resource_node_sort_key(a).cmp(&resource_node_sort_key(b)));
    }
    let mut old_nodes: Vec<&SemanticNode> = old_keys
        .keys()
        .filter_map(|id| old_by_id.get(id).copied())
        .collect();
    old_nodes.sort_by(|a, b| resource_node_sort_key(a).cmp(&resource_node_sort_key(b)));
    for old_node in old_nodes {
        if matched_old.contains(old_node.id.as_str()) {
            continue;
        }
        let Some(key) = old_keys.get(old_node.id.as_str()) else {
            continue;
        };
        let Some(candidates) = new_by_key.get_mut(key) else {
            continue;
        };
        if candidates.is_empty() {
            continue;
        }
        let new_node = candidates.remove(0);
        matched_old.insert(old_node.id.clone());
        matched_new.insert(new_node.id.clone());
        result.push(MatchPair { old_node, new_node });
    }
    result
}

/// python keyed_profiles.augment_keyed_data_changes: recover keyed review-container ADD/DELETEs
/// hidden by coarse container matches (unmatched-ancestor guarded).
pub(crate) fn augment_keyed_data_changes_drafts<'a>(
    changes: &mut Vec<ChangeDraft<'a>>,
    old_tree: &'a SemanticNode,
    new_tree: &'a SemanticNode,
    language: &str,
) -> Option<Value> {
    if !matches!(language, "json" | "yaml") {
        return None;
    }
    // Positional-label churn suppression (python suppress_array_index_only_modification):
    // an array element with IDENTICAL content whose positional label shifted after a sibling
    // insertion ([1]->[2]) is not a change — drop the MODIFICATION/REORDER pair noise,
    // carrying the suppression as evidence.
    let mut suppressed_labels: Vec<String> = Vec::new();
    changes.retain(|change| {
        if !matches!(change.change_type, "MODIFICATION" | "REORDER" | "MOVE") {
            return true;
        }
        let (Some(old_node), Some(new_node)) = (change.old_node, change.new_node) else {
            return true;
        };
        if old_node.structural_hash == new_node.structural_hash {
            suppressed_labels.push(old_node.label.clone());
            return false;
        }
        true
    });
    let old_by_id = semantic_node_refs_by_id_with_root(old_tree);
    let new_by_id = semantic_node_refs_by_id_with_root(new_tree);
    let old_keys: HashMap<&str, Vec<String>> = old_by_id
        .iter()
        .filter_map(|(id, node)| keyed_data_key(node, &old_by_id, language).map(|k| (*id, k)))
        .collect();
    let new_keys: HashMap<&str, Vec<String>> = new_by_id
        .iter()
        .filter_map(|(id, node)| keyed_data_key(node, &new_by_id, language).map(|k| (*id, k)))
        .collect();
    let old_key_set: HashSet<&Vec<String>> = old_keys.values().collect();
    let new_key_set: HashSet<&Vec<String>> = new_keys.values().collect();
    let mentioned_old: HashSet<&str> = changes
        .iter()
        .filter_map(|c| c.old_node.map(|n| n.id.as_str()))
        .collect();
    let mentioned_new: HashSet<&str> = changes
        .iter()
        .filter_map(|c| c.new_node.map(|n| n.id.as_str()))
        .collect();
    let has_unmatched_ancestor = |node: &SemanticNode,
                                  by_id: &HashMap<&str, &SemanticNode>,
                                  keys: &HashMap<&str, Vec<String>>,
                                  opposite: &HashSet<&Vec<String>>| {
        let mut current = node.id.clone();
        while let Some((parent_id, _)) = current.rsplit_once('.') {
            if let Some(parent) = by_id.get(parent_id).copied() {
                if let Some(key) = keys.get(parent_id) {
                    if !opposite.contains(key) && is_keyed_review_container(parent) {
                        return true;
                    }
                }
            }
            current = parent_id.to_string();
        }
        false
    };
    let mut recovered: Vec<ChangeDraft<'a>> = Vec::new();
    let mut old_nodes: Vec<&SemanticNode> = old_keys
        .keys()
        .filter_map(|id| old_by_id.get(id).copied())
        .collect();
    old_nodes.sort_by(|a, b| resource_node_sort_key(a).cmp(&resource_node_sort_key(b)));
    for old_node in old_nodes {
        let Some(key) = old_keys.get(old_node.id.as_str()) else {
            continue;
        };
        if new_key_set.contains(key)
            || mentioned_old.contains(old_node.id.as_str())
            || !is_keyed_review_container(old_node)
            || has_unmatched_ancestor(old_node, &old_by_id, &old_keys, &new_key_set)
        {
            continue;
        }
        recovered.push(ChangeDraft {
            change_type: "DELETION",
            old_node: Some(old_node),
            new_node: None,
            old_index: None,
            new_index: None,
            confidence: 0.94,
            description: format!("Delete {}({:?})", old_node.node_type, old_node.label),
            refactoring_kind: None,
            text_diff: None,
        });
    }
    let mut new_nodes: Vec<&SemanticNode> = new_keys
        .keys()
        .filter_map(|id| new_by_id.get(id).copied())
        .collect();
    new_nodes.sort_by(|a, b| resource_node_sort_key(a).cmp(&resource_node_sort_key(b)));
    for new_node in new_nodes {
        let Some(key) = new_keys.get(new_node.id.as_str()) else {
            continue;
        };
        if old_key_set.contains(key)
            || mentioned_new.contains(new_node.id.as_str())
            || !is_keyed_review_container(new_node)
            || has_unmatched_ancestor(new_node, &new_by_id, &new_keys, &old_key_set)
        {
            continue;
        }
        recovered.push(ChangeDraft {
            change_type: "ADDITION",
            old_node: None,
            new_node: Some(new_node),
            old_index: None,
            new_index: None,
            confidence: 0.94,
            description: format!("Insert -> {}({:?})", new_node.node_type, new_node.label),
            refactoring_kind: None,
            text_diff: None,
        });
    }
    changes.extend(recovered);

    // python presentation._describe_keyed_data_additions: an added array OBJECT is described by
    // its identifying pair ("Insert command 'intentumdiff.toggleEditorDiff'"), not "object([1])".
    const SUMMARY_KEYS: &[&str] = &["command", "name", "id", "key", "title", "label"];
    for change in changes.iter_mut() {
        if change.change_type != "ADDITION" {
            continue;
        }
        let Some(node) = change.new_node else { continue };
        if node.node_type.to_lowercase() != "object" {
            continue;
        }
        'keys: for key in SUMMARY_KEYS {
            for child in &node.children {
                if child.node_type.to_lowercase() != "pair" || child.label != *key {
                    continue;
                }
                let value = child
                    .descendants()
                    .into_iter()
                    .rev()
                    .find(|d| d.children.is_empty() && d.label != *key && !d.label.is_empty());
                if let Some(value) = value {
                    change.description = format!("Insert {} '{}'", key, value.label);
                    break 'keys;
                }
            }
        }
    }

    if suppressed_labels.is_empty() {
        return None;
    }
    Some(json!({
        "kind": "NOISE_SUPPRESSED",
        "raw_change_indices": [],
        "old_labels": suppressed_labels,
        "new_labels": [],
        "old_node_ids": [],
        "new_node_ids": [],
        "confidence": 0.9,
        "rule_id": "presentation.suppress_array_index_only_modification",
        "metadata": {"reason": "Structured-data array elements kept identical content but received different positional labels after sibling insertions or deletions."},
    }))
}

// ── SQL query-profile keying (issue #57) — mirrors python query_profiles' SQL half. Clauses,
// relations, and fields key by their role + normalized identity within their statement, so an
// added JOIN doesn't positionally shift the FROM relation into a bogus MOVE, and unchanged
// SELECT fields never churn. The DAX half is deliberately NOT ported: dax is already routed and
// green without it — adding keys would change its behavior for no contract.
