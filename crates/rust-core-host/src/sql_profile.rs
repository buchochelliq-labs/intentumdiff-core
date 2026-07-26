// Extracted verbatim from lib.rs (issue #85, lib.rs split round 2).
use crate::*;

pub(crate) fn sql_clause_role(node_type: &str) -> Option<&'static str> {
    Some(match node_type {
        "from" | "from_clause" => "from",
        "group_by" | "group_by_clause" => "group_by",
        "having" | "having_clause" => "having",
        "join" | "join_clause" => "join",
        "limit" | "limit_clause" => "limit",
        "order_by" | "order_by_clause" => "order_by",
        "select" | "select_clause" => "select",
        "where" | "where_clause" => "where",
        _ => return None,
    })
}

/// python query_profiles._is_generic_label (query variant).
pub(crate) fn sql_is_generic_label(label: &str, node_type: &str) -> bool {
    let text = label.trim();
    if text.is_empty() {
        return true;
    }
    let lowered = text.to_lowercase();
    lowered == node_type.to_lowercase()
        || matches!(
            lowered.as_str(),
            "binary_expression" | "dax_file" | "field" | "from" | "join" | "order_by"
                | "program" | "relation" | "select" | "select_expression" | "select_statement"
                | "statement" | "term" | "where"
        )
}

/// python `_normalize_sql_identifier`: strip quotes/brackets, take after the last dot, collapse
/// whitespace, lowercase.
pub(crate) fn sql_normalize_identifier(label: &str) -> String {
    let mut text = label.trim();
    text = text.trim_matches('"').trim_matches('\'');
    text = text.trim_matches(|c| c == '[' || c == ']');
    let text = match text.rsplit_once('.') {
        Some((_, tail)) => tail,
        None => text,
    };
    text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// python `_normalize_sql_term_identity`: a simple (optionally dotted) identifier reduces to its
/// last segment; anything else lowercases whole.
pub(crate) fn sql_normalize_term_identity(label: &str) -> String {
    let text = label.split_whitespace().collect::<Vec<_>>().join(" ");
    let text = text.trim_matches('"').trim_matches('\'');
    let is_ident = |s: &str| {
        let mut chars = s.chars();
        matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
    };
    let segments: Vec<&str> = text.split('.').collect();
    match segments.as_slice() {
        [single] if is_ident(single) => single.to_lowercase(),
        [qualifier, name] if is_ident(qualifier) && is_ident(name) => name.to_lowercase(),
        _ => text.to_lowercase(),
    }
}

pub(crate) fn sql_first_descendant_of_type<'a>(
    node: &'a SemanticNode,
    types: &[&str],
) -> Option<&'a SemanticNode> {
    node.descendants()
        .into_iter()
        .find(|n| types.contains(&n.node_type.to_lowercase().as_str()))
}

/// python `_sql_relation_base`.
pub(crate) fn sql_relation_base(node: &SemanticNode) -> String {
    if let Some(obj) = sql_first_descendant_of_type(node, &["object_reference"]) {
        if !obj.label.is_empty() {
            return sql_normalize_identifier(&obj.label);
        }
    }
    if !node.label.is_empty() && !sql_is_generic_label(&node.label, &node.node_type) {
        if let Some(first) = node.label.split_whitespace().next() {
            return sql_normalize_identifier(first);
        }
    }
    String::new()
}

pub(crate) fn sql_pos_sorted<'a>(mut nodes: Vec<&'a SemanticNode>) -> Vec<&'a SemanticNode> {
    nodes.sort_by(|a, b| resource_node_sort_key(a).cmp(&resource_node_sort_key(b)));
    nodes
}

pub(crate) fn sql_same_type_ordinal_in(node: &SemanticNode, scope_nodes: Vec<&SemanticNode>) -> usize {
    let mut ordinal = 0;
    for candidate in sql_pos_sorted(scope_nodes) {
        if candidate.id == node.id {
            return ordinal;
        }
        if candidate.node_type == node.node_type {
            ordinal += 1;
        }
    }
    ordinal
}

/// python `_sql_statement_kind`.
pub(crate) fn sql_statement_kind(statement: &SemanticNode) -> String {
    for child in &statement.children {
        let ct = child.node_type.to_lowercase();
        if matches!(ct.as_str(), "select" | "select_clause" | "select_statement") {
            return "select".to_string();
        }
        if let Some(kind) = ct.strip_suffix("_statement") {
            return kind.to_string();
        }
    }
    let own = statement.node_type.to_lowercase();
    if own == "select_statement" {
        "select".to_string()
    } else {
        own
    }
}

/// python `_sql_statement_key`.
pub(crate) fn sql_statement_key(
    node: &SemanticNode,
    by_id: &HashMap<&str, &SemanticNode>,
    root: &SemanticNode,
) -> Option<Vec<String>> {
    let lowered = node.node_type.to_lowercase();
    let statement = if matches!(lowered.as_str(), "statement" | "select_statement") {
        Some(node)
    } else {
        nearest_ancestor_of_types(node.id.as_str(), by_id, &["statement", "select_statement"])
    }?;
    let all: Vec<&SemanticNode> = std::iter::once(root).chain(root.descendants()).collect();
    let ordinal = sql_same_type_ordinal_in(statement, all);
    Some(vec![
        "sql".into(),
        "statement".into(),
        sql_statement_kind(statement),
        ordinal.to_string(),
    ])
}

/// python `_sql_field_context`: the enclosing clause role (self included).
pub(crate) fn sql_field_context(node: &SemanticNode, by_id: &HashMap<&str, &SemanticNode>) -> Option<&'static str> {
    if let Some(role) = sql_clause_role(node.node_type.to_lowercase().as_str()) {
        return Some(role);
    }
    let mut current = node.id.clone();
    while let Some((parent_id, _)) = current.rsplit_once('.') {
        if let Some(parent) = by_id.get(parent_id).copied() {
            if let Some(role) = sql_clause_role(parent.node_type.to_lowercase().as_str()) {
                return Some(role);
            }
        }
        current = parent_id.to_string();
    }
    None
}

/// python `_select_output_term`: the outermost `term` on the path up to a select_expression.
pub(crate) fn sql_select_output_term<'a>(
    node: &'a SemanticNode,
    by_id: &HashMap<&str, &'a SemanticNode>,
) -> Option<&'a SemanticNode> {
    let mut output: Option<&SemanticNode> = None;
    let mut current: &SemanticNode = node;
    loop {
        if current.node_type.to_lowercase() == "term" {
            output = Some(current);
        }
        let Some((parent_id, _)) = current.id.rsplit_once('.') else {
            return output;
        };
        let Some(parent) = by_id.get(parent_id).copied() else {
            return output;
        };
        if parent.node_type.to_lowercase() == "select_expression" {
            return output;
        }
        current = parent;
    }
}

/// python query_profiles._sql_key.
pub(crate) fn sql_key(
    node: &SemanticNode,
    by_id: &HashMap<&str, &SemanticNode>,
    root: &SemanticNode,
) -> Option<Vec<String>> {
    let node_type = node.node_type.to_lowercase();
    let statement_key = sql_statement_key(node, by_id, root);

    if matches!(node_type.as_str(), "statement" | "select_statement") {
        return statement_key;
    }

    if let (Some(role), Some(stmt)) = (sql_clause_role(&node_type), statement_key.as_ref()) {
        let mut key = stmt.clone();
        key.push("clause".into());
        key.push(role.into());
        if role == "join" {
            let relation_name = sql_first_descendant_of_type(node, &["relation"])
                .map(sql_relation_base)
                .unwrap_or_default();
            if relation_name.is_empty() {
                let statement = nearest_ancestor_of_types(
                    node.id.as_str(),
                    by_id,
                    &["statement", "select_statement"],
                );
                let scope: Vec<&SemanticNode> = statement
                    .map(|s| s.descendants())
                    .unwrap_or_else(|| std::iter::once(root).chain(root.descendants()).collect());
                key.push(sql_same_type_ordinal_in(node, scope).to_string());
            } else {
                key.push(relation_name);
            }
        }
        return Some(key);
    }

    if node_type == "relation" {
        if let Some(stmt) = statement_key.as_ref() {
            let base = sql_relation_base(node);
            if !base.is_empty() {
                let mut key = stmt.clone();
                key.push("relation".into());
                key.push(base);
                return Some(key);
            }
        }
    }

    if node_type == "object_reference" {
        if let Some(relation) =
            nearest_ancestor_of_types(node.id.as_str(), by_id, &["relation"])
        {
            if direct_child_under(relation.id.as_str(), node, by_id)
                .is_some_and(|child| child.id == node.id)
            {
                if let Some(mut relation_key) = sql_key(relation, by_id, root) {
                    relation_key.push("object".into());
                    return Some(relation_key);
                }
            }
        }
    }

    if node_type == "field" {
        if let Some(stmt) = statement_key.as_ref() {
            let context = sql_field_context(node, by_id)?;
            if context == "select" {
                if let Some(term) = sql_select_output_term(node, by_id) {
                    let term_identity = if sql_is_generic_label(&term.label, &term.node_type) {
                        sql_normalize_identifier(&node.label)
                    } else {
                        sql_normalize_term_identity(&term.label)
                    };
                    let mut key = stmt.clone();
                    key.push("field".into());
                    key.push(context.into());
                    key.push(term_identity);
                    key.push(sql_normalize_identifier(&node.label));
                    return Some(key);
                }
            }
            let mut key = stmt.clone();
            key.push("field".into());
            key.push(context.into());
            key.push(sql_normalize_identifier(&node.label));
            return Some(key);
        }
    }

    if node_type == "term" {
        if let Some(stmt) = statement_key.as_ref() {
            let field = sql_first_descendant_of_type(node, &["field"]);
            let context = sql_field_context(node, by_id);
            if let (Some(field), Some("select")) = (field, context) {
                if let Some(output_term) = sql_select_output_term(node, by_id) {
                    if output_term.id != node.id {
                        return None;
                    }
                }
                let identity = if !sql_is_generic_label(&node.label, &node_type) {
                    sql_normalize_term_identity(&node.label)
                } else {
                    sql_normalize_identifier(&field.label)
                };
                let mut key = stmt.clone();
                key.push("select_term".into());
                key.push(identity);
                return Some(key);
            }
        }
    }

    if matches!(node_type.as_str(), "binary_expression" | "order_target") {
        if let Some(stmt) = statement_key.as_ref() {
            let context = sql_field_context(node, by_id);
            let field = sql_first_descendant_of_type(node, &["field"]);
            if let (Some(context), Some(field)) = (context, field) {
                let clause = {
                    let mut current = node.id.clone();
                    let mut found: Option<&SemanticNode> = None;
                    while let Some((parent_id, _)) = current.rsplit_once('.') {
                        if let Some(parent) = by_id.get(parent_id).copied() {
                            if sql_clause_role(parent.node_type.to_lowercase().as_str()).is_some()
                            {
                                found = Some(parent);
                                break;
                            }
                        }
                        current = parent_id.to_string();
                    }
                    found
                };
                let scope: Vec<&SemanticNode> = match clause {
                    Some(clause) => clause.descendants(),
                    None => std::iter::once(root).chain(root.descendants()).collect(),
                };
                let mut key = stmt.clone();
                key.push(node_type.clone());
                key.push(context.into());
                key.push(sql_normalize_identifier(&field.label));
                key.push(sql_same_type_ordinal_in(node, scope).to_string());
                return Some(key);
            }
        }
    }

    None
}

const SQL_REVIEW_TYPES: &[&str] = &[
    "binary_expression", "field", "from", "group_by", "group_by_clause", "having",
    "having_clause", "join", "join_clause", "limit", "limit_clause", "order_by",
    "order_by_clause", "relation", "select", "select_clause", "term", "where", "where_clause",
];

/// python query_profiles.is_query_profile_review_container (sql).
pub(crate) fn is_sql_review_container(node: &SemanticNode) -> bool {
    let lowered = node.node_type.to_lowercase();
    SQL_REVIEW_TYPES.contains(&lowered.as_str())
        && !node.label.is_empty()
        && !sql_is_generic_label(&node.label, &node.node_type)
}

/// python query_profiles.augment_query_profile_matching (sql): drop cross-key positional
/// matches, then pair unmatched keyed nodes by identical key (position order).
pub(crate) fn augment_sql_query_matching<'a>(
    old_tree: &'a SemanticNode,
    new_tree: &'a SemanticNode,
    matching: Vec<MatchPair<'a>>,
    language: &str,
) -> Vec<MatchPair<'a>> {
    if language != "sql" {
        return matching;
    }
    let old_by_id = semantic_node_refs_by_id_with_root(old_tree);
    let new_by_id = semantic_node_refs_by_id_with_root(new_tree);
    let old_keys: HashMap<&str, Vec<String>> = old_by_id
        .iter()
        .filter_map(|(id, node)| sql_key(node, &old_by_id, old_tree).map(|k| (*id, k)))
        .collect();
    let new_keys: HashMap<&str, Vec<String>> = new_by_id
        .iter()
        .filter_map(|(id, node)| sql_key(node, &new_by_id, new_tree).map(|k| (*id, k)))
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

/// python query_profiles.augment_query_profile_changes (sql): demote same-key MOVEs (a clause
/// merely displaced by an inserted JOIN) to MODIFICATIONs when content changed or drop them,
/// surface per-descendant modifications, and recover keyed review-container add/deletes.
pub(crate) fn augment_sql_query_changes_drafts<'a>(
    changes: &mut Vec<ChangeDraft<'a>>,
    old_tree: &'a SemanticNode,
    new_tree: &'a SemanticNode,
    language: &str,
) {
    if language != "sql" {
        return;
    }
    let old_by_id = semantic_node_refs_by_id_with_root(old_tree);
    let new_by_id = semantic_node_refs_by_id_with_root(new_tree);
    let old_keys: HashMap<&str, Vec<String>> = old_by_id
        .iter()
        .filter_map(|(id, node)| sql_key(node, &old_by_id, old_tree).map(|k| (*id, k)))
        .collect();
    let new_keys: HashMap<&str, Vec<String>> = new_by_id
        .iter()
        .filter_map(|(id, node)| sql_key(node, &new_by_id, new_tree).map(|k| (*id, k)))
        .collect();
    let old_key_set: HashSet<&Vec<String>> = old_keys.values().collect();
    let new_key_set: HashSet<&Vec<String>> = new_keys.values().collect();

    let mut existing_pairs: HashSet<(String, String)> = changes
        .iter()
        .filter(|c| c.change_type == "MODIFICATION")
        .filter_map(|c| Some((c.old_node?.id.clone(), c.new_node?.id.clone())))
        .collect();

    // MOVE demotion + per-descendant modifications.
    let mut extra: Vec<ChangeDraft<'a>> = Vec::new();
    let mut replaced: Vec<ChangeDraft<'a>> = Vec::new();
    changes.retain(|change| {
        if change.change_type != "MOVE" {
            return true;
        }
        let (Some(old_node), Some(new_node)) = (change.old_node, change.new_node) else {
            return true;
        };
        let same_key = old_keys.get(old_node.id.as_str()).is_some()
            && old_keys.get(old_node.id.as_str()) == new_keys.get(new_node.id.as_str());
        if !same_key {
            return true;
        }
        if old_node.structural_hash != new_node.structural_hash {
            replaced.push(ChangeDraft {
                change_type: "MODIFICATION",
                old_node: Some(old_node),
                new_node: Some(new_node),
                old_index: None,
                new_index: None,
                confidence: change.confidence.min(0.88),
                description: format!(
                    "Update {}({:?}) -> {}({:?})",
                    old_node.node_type, old_node.label, new_node.node_type, new_node.label
                ),
                refactoring_kind: None,
                text_diff: None,
            });
            // Per-descendant review-container modifications inside the demoted move.
            let new_desc_by_key: HashMap<&Vec<String>, &SemanticNode> = new_node
                .descendants()
                .into_iter()
                .filter(|d| is_sql_review_container(d))
                .filter_map(|d| new_keys.get(d.id.as_str()).map(|k| (k, d)))
                .collect();
            for old_desc in old_node.descendants() {
                let Some(key) = old_keys.get(old_desc.id.as_str()) else {
                    continue;
                };
                if !is_sql_review_container(old_desc) {
                    continue;
                }
                let Some(new_desc) = new_desc_by_key.get(key).copied() else {
                    continue;
                };
                let pair = (old_desc.id.clone(), new_desc.id.clone());
                if existing_pairs.contains(&pair)
                    || old_desc.structural_hash == new_desc.structural_hash
                {
                    continue;
                }
                existing_pairs.insert(pair);
                extra.push(ChangeDraft {
                    change_type: "MODIFICATION",
                    old_node: Some(old_desc),
                    new_node: Some(new_desc),
                    old_index: None,
                    new_index: None,
                    confidence: change.confidence.min(0.86),
                    description: format!(
                        "Update {}({:?}) -> {}({:?})",
                        old_desc.node_type, old_desc.label, new_desc.node_type, new_desc.label
                    ),
                    refactoring_kind: None,
                    text_diff: None,
                });
            }
        }
        false
    });
    changes.extend(replaced);
    changes.extend(extra);

    // Keyed review-container ADD/DELETE recovery (unmatched-ancestor guarded).
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
                    if !opposite.contains(key) && is_sql_review_container(parent) {
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
            || !is_sql_review_container(old_node)
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
            || !is_sql_review_container(new_node)
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
}

