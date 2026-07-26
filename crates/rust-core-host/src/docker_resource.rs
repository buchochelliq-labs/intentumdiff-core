// Extracted verbatim from lib.rs (issue #85, lib.rs split round 2).
use crate::*;

/// python `_docker_shell_identity`: a coarse command identity (first 2-3 tokens) so different RUN
/// commands do not match, capped so re-runs of the SAME command stay distinct by ordinal.
pub(crate) fn docker_shell_identity(label: &str) -> String {
    let text = normalize_docker_label(label);
    if text.is_empty() {
        return String::new();
    }
    let tokens: Vec<&str> = text.split(' ').collect();
    if tokens.len() >= 2 && tokens[0] == "npm" && matches!(tokens[1], "ci" | "install") {
        return "npm install".to_string();
    }
    if tokens.len() >= 3 && (tokens[1].starts_with('-') || tokens[2].starts_with('-')) {
        return tokens[..3].join(" ");
    }
    if tokens.len() >= 2 {
        return tokens[..2].join(" ");
    }
    tokens[0].to_string()
}

pub(crate) fn docker_all_nodes_sorted<'a>(by_id: &HashMap<&str, &'a SemanticNode>) -> Vec<&'a SemanticNode> {
    let mut nodes: Vec<&SemanticNode> = by_id.values().copied().collect();
    nodes.sort_by(|a, b| resource_node_sort_key(a).cmp(&resource_node_sort_key(b)));
    nodes
}

/// python `_same_label_ordinal`: index among same-type nodes with the same normalized label.
pub(crate) fn docker_same_label_ordinal(node: &SemanticNode, by_id: &HashMap<&str, &SemanticNode>) -> usize {
    let mut ordinal = 0;
    for candidate in docker_all_nodes_sorted(by_id) {
        if candidate.id == node.id {
            return ordinal;
        }
        if candidate.node_type == node.node_type
            && normalize_docker_label(&candidate.label) == normalize_docker_label(&node.label)
        {
            ordinal += 1;
        }
    }
    ordinal
}

/// python `_same_type_ordinal_in_root`: index among same-type nodes in the whole tree.
pub(crate) fn docker_same_type_ordinal(node: &SemanticNode, by_id: &HashMap<&str, &SemanticNode>) -> usize {
    let mut ordinal = 0;
    for candidate in docker_all_nodes_sorted(by_id) {
        if candidate.id == node.id {
            return ordinal;
        }
        if candidate.node_type == node.node_type {
            ordinal += 1;
        }
    }
    ordinal
}

/// python `_same_docker_instruction_identity_ordinal`: index among same-kind instructions sharing
/// the shell identity (so two identical `RUN`s stay distinct).
pub(crate) fn docker_same_identity_ordinal(
    node: &SemanticNode,
    by_id: &HashMap<&str, &SemanticNode>,
    kind: &str,
    identity: &str,
) -> usize {
    let mut ordinal = 0;
    for candidate in docker_all_nodes_sorted(by_id) {
        if candidate.id == node.id {
            return ordinal;
        }
        if candidate.node_type != node.node_type {
            continue;
        }
        let lowered = candidate.node_type.to_lowercase();
        let candidate_kind = lowered.strip_suffix("_instruction").unwrap_or(&lowered);
        if candidate_kind != kind {
            continue;
        }
        if docker_shell_identity(&docker_instruction_detail(candidate)) == identity {
            ordinal += 1;
        }
    }
    ordinal
}

/// python `_sibling_ordinal`: index among same-type direct siblings.
pub(crate) fn docker_sibling_ordinal(
    node: &SemanticNode,
    by_id: &HashMap<&str, &SemanticNode>,
    node_type: &str,
) -> usize {
    let Some((parent_id, _)) = node.id.rsplit_once('.') else {
        return 0;
    };
    let Some(parent) = by_id.get(parent_id).copied() else {
        return 0;
    };
    let mut ordinal = 0;
    for sibling in &parent.children {
        if sibling.id == node.id {
            return ordinal;
        }
        if sibling.node_type.to_lowercase() == node_type.to_lowercase() {
            ordinal += 1;
        }
    }
    ordinal
}

/// python `_docker_instruction_key`.
pub(crate) fn docker_instruction_key(
    node: &SemanticNode,
    by_id: &HashMap<&str, &SemanticNode>,
) -> Option<Vec<String>> {
    let lowered = node.node_type.to_lowercase();
    if !is_docker_instruction_type(&lowered) {
        return None;
    }
    let kind = lowered.strip_suffix("_instruction").unwrap_or(&lowered).to_string();
    if kind == "copy" && !node.label.is_empty() && node.label.to_uppercase() != "COPY" {
        return Some(vec![
            "dockerfile".into(),
            "instruction".into(),
            kind,
            normalize_docker_label(&node.label),
            docker_same_label_ordinal(node, by_id).to_string(),
        ]);
    }
    if kind == "run" || kind == "shell" {
        let identity = docker_shell_identity(&docker_instruction_detail(node));
        if !identity.is_empty() {
            let ordinal = docker_same_identity_ordinal(node, by_id, &kind, &identity);
            return Some(vec![
                "dockerfile".into(),
                "instruction".into(),
                kind,
                identity,
                ordinal.to_string(),
            ]);
        }
    }
    Some(vec![
        "dockerfile".into(),
        "instruction".into(),
        kind,
        docker_same_type_ordinal(node, by_id).to_string(),
    ])
}

/// python `_dockerfile_key`.
pub(crate) fn dockerfile_key(node: &SemanticNode, by_id: &HashMap<&str, &SemanticNode>) -> Option<Vec<String>> {
    let lowered = node.node_type.to_lowercase();
    if is_docker_instruction_type(&lowered) {
        return docker_instruction_key(node, by_id);
    }
    let instruction =
        nearest_ancestor_of_types(node.id.as_str(), by_id, DOCKER_INSTRUCTION_TYPES)?;
    let mut key = docker_instruction_key(instruction, by_id)?;
    key.push("child".into());
    key.push(lowered.clone());
    key.push(docker_sibling_ordinal(node, by_id, &lowered).to_string());
    Some(key)
}

pub(crate) fn resource_profile_key(
    node: &SemanticNode,
    by_id: &HashMap<&str, &SemanticNode>,
    language: &str,
) -> Option<Vec<String>> {
    match language {
        "puppet" => puppet_key(node, by_id),
        "hcl" => hcl_key(node, by_id),
        "dockerfile" => dockerfile_key(node, by_id),
        _ => None,
    }
}

/// python resource_profiles.augment_resource_profile_matching: drop positional matches that
/// straddle resource-profile keys, then pair unmatched keyed nodes by identical key.
pub(crate) fn augment_resource_profile_matching<'a>(
    old_tree: &'a SemanticNode,
    new_tree: &'a SemanticNode,
    matching: Vec<MatchPair<'a>>,
    language: &str,
) -> Vec<MatchPair<'a>> {
    if !resource_profile_language(language) {
        return matching;
    }
    let old_by_id = semantic_node_refs_by_id_with_root(old_tree);
    let new_by_id = semantic_node_refs_by_id_with_root(new_tree);
    let old_keys: HashMap<&str, Vec<String>> = old_by_id
        .iter()
        .filter_map(|(id, node)| resource_profile_key(node, &old_by_id, language).map(|k| (*id, k)))
        .collect();
    let new_keys: HashMap<&str, Vec<String>> = new_by_id
        .iter()
        .filter_map(|(id, node)| resource_profile_key(node, &new_by_id, language).map(|k| (*id, k)))
        .collect();

    let mut result: Vec<MatchPair<'a>> = Vec::new();
    let mut matched_old: HashSet<String> = HashSet::new();
    let mut matched_new: HashSet<String> = HashSet::new();
    for pair in &matching {
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
        result.push(MatchPair {
            old_node: pair.old_node,
            new_node: pair.new_node,
        });
        matched_old.insert(pair.old_node.id.clone());
        matched_new.insert(pair.new_node.id.clone());
    }

    let mut new_by_key: HashMap<Vec<String>, Vec<&SemanticNode>> = HashMap::new();
    for (id, node) in &new_by_id {
        if let Some(key) = new_keys.get(id) {
            if !matched_new.contains(*id) {
                new_by_key.entry(key.clone()).or_default().push(node);
            }
        }
    }
    for nodes in new_by_key.values_mut() {
        nodes.sort_by(|a, b| resource_node_sort_key(a).cmp(&resource_node_sort_key(b)));
    }

    let mut old_sorted: Vec<&SemanticNode> = old_by_id.values().copied().collect();
    old_sorted.sort_by(|a, b| resource_node_sort_key(a).cmp(&resource_node_sort_key(b)));
    for old_node in old_sorted {
        if matched_old.contains(old_node.id.as_str()) {
            continue;
        }
        let Some(key) = old_keys.get(old_node.id.as_str()) else {
            continue;
        };
        if let Some(candidates) = new_by_key.get_mut(key) {
            if candidates.is_empty() {
                continue;
            }
            let new_node = candidates.remove(0);
            result.push(MatchPair { old_node, new_node });
            matched_old.insert(old_node.id.clone());
            matched_new.insert(new_node.id.clone());
        }
    }
    result
}
