//! Recover callable continuity before descendant scope gates run. A positional ID is
//! not identity: require an unchanged signature, comparable bodies, and a unique
//! candidate in both directions after existing identities have been consumed.
use crate::*;

fn signature(node: &SemanticNode) -> Option<&SemanticNode> {
    node.children.iter().find(|c| {
        matches!(
            c.node_type.as_str(),
            "parameters" | "formal_parameters" | "parameter_list" | "function_parameters"
        )
    })
}

fn body_tokens(node: &SemanticNode) -> HashMap<(String, String), usize> {
    fn collect(n: &SemanticNode, out: &mut HashMap<(String, String), usize>) {
        *out.entry((n.node_type.clone(), n.label.clone()))
            .or_default() += 1;
        for c in &n.children {
            collect(c, out);
        }
    }
    let mut tokens = HashMap::new();
    for child in &node.children {
        if Some(child.id.as_str()) == signature(node).map(|s| s.id.as_str())
            || (anchor_is_name(child) && child.label == node.label)
        {
            continue;
        }
        collect(child, &mut tokens);
    }
    tokens
}

fn similarity(old: &SemanticNode, new: &SemanticNode) -> Option<f64> {
    if old.node_type != new.node_type
        || !anchor_is_function(old)
        || old.label.is_empty()
        || new.label.is_empty()
        || old.label == new.label
    {
        return None;
    }
    let (os, ns) = (signature(old)?, signature(new)?);
    if os.structural_hash != ns.structural_hash {
        return None;
    }
    let (a, b) = (body_tokens(old), body_tokens(new));
    let an: usize = a.values().sum();
    let bn: usize = b.values().sum();
    if an == 0 || bn == 0 || an.max(bn) > 2 * an.min(bn) {
        return None;
    }
    // Shared scaffolding alone (e.g. block/return in two constant getters) is
    // not evidence of continuity. Require an unchanged concrete body token.
    if !a.keys().any(|(kind, label)| {
        !label.is_empty() && label != kind && b.contains_key(&(kind.clone(), label.clone()))
    }) {
        return None;
    }
    let common: usize = a
        .iter()
        .map(|(k, v)| (*v).min(*b.get(k).unwrap_or(&0)))
        .sum();
    Some(common as f64 / (an + bn - common) as f64)
}

fn same_body(old: &SemanticNode, new: &SemanticNode, sources: Option<(&str, &str)>) -> bool {
    // Ordered subtrees, not a bag of tokens: reordering calls is not an exact body.
    let body = |node: &SemanticNode| {
        node.children
            .iter()
            .filter(|c| !(anchor_is_name(c) && c.label == node.label))
            .map(|c| (c.node_type.clone(), c.structural_hash.clone()))
            .collect::<Vec<_>>()
    };
    if body(old) != body(new) {
        return false;
    }
    // Parser trees may omit punctuation such as + versus *. Verify exact source
    // evidence too before letting this candidate outrank another plausible body.
    sources.is_some_and(|(os, ns)| {
        let snippets = |node: &SemanticNode, source: &str| {
            let lines: Vec<_> = source.lines().collect();
            node.children
                .iter()
                .filter(|c| !(anchor_is_name(c) && c.label == node.label))
                .map(|c| slice_source_text(&lines, &c.position).trim().to_owned())
                .collect::<Vec<_>>()
        };
        snippets(old, os) == snippets(new, ns)
    })
}

pub(crate) fn seed_renamed_callables<'a>(
    old: &TreeIndex<'a>,
    new: &TreeIndex<'a>,
    pairs: &mut Vec<MatchPair<'a>>,
    threshold: f64,
    sources: Option<(&str, &str)>,
) {
    let occupied_old: HashSet<&str> = pairs.iter().map(|p| p.old_node.id.as_str()).collect();
    let occupied_new: HashSet<&str> = pairs.iter().map(|p| p.new_node.id.as_str()).collect();
    let mut candidates = Vec::new();
    for o in &old.named_entities {
        if occupied_old.contains(o.id.as_str()) {
            continue;
        }
        for n in &new.named_entities {
            if occupied_new.contains(n.id.as_str()) {
                continue;
            }
            // Enclosing named scope must retain its identity. Do not infer cross-scope renames.
            let same_scope = scope_path(old, o) == scope_path(new, n);
            if same_scope
                && label_match_parent_compatible(o, n, old, new)
                && similarity(o, n).is_some_and(|score| score >= threshold.max(0.5))
            {
                candidates.push((*o, *n));
            }
        }
    }
    // Consume stronger evidence before positional/bottom-up matching can steal it.
    // A weak edge must not compete with an exact body at either end.
    let exact: Vec<_> = candidates
        .iter()
        .copied()
        .filter(|(o, n)| same_body(o, n, sources))
        .collect();
    candidates.retain(|(o, n)| {
        same_body(o, n, sources) || !exact.iter().any(|(eo, en)| eo.id == o.id || en.id == n.id)
    });
    for (o, n) in &candidates {
        if candidates.iter().filter(|(a, _)| a.id == o.id).count() == 1
            && candidates.iter().filter(|(_, b)| b.id == n.id).count() == 1
        {
            pairs.push(MatchPair {
                old_node: o,
                new_node: n,
            });
        }
    }
}

pub(crate) fn promote_rename_updates(changes: &mut Vec<ChangeDraft<'_>>) {
    let mut name_pairs = Vec::new();
    for change in changes.iter_mut() {
        if change.change_type != "MODIFICATION" {
            continue;
        }
        let (Some(old), Some(new)) = (change.old_node, change.new_node) else {
            continue;
        };
        let Some(score) = similarity(old, new).filter(|s| *s >= 0.5) else {
            continue;
        };
        change.change_type = "REFACTORING";
        change.refactoring_kind = Some("RENAME_SYMBOL");
        change.confidence = score;
        change.description = format!(
            "Rename {}('{}') -> ('{}')",
            old.node_type, old.label, new.label
        );
        for o in &old.children {
            for n in &new.children {
                if anchor_is_name(o)
                    && anchor_is_name(n)
                    && o.label == old.label
                    && n.label == new.label
                {
                    name_pairs.push((o.id.clone(), n.id.clone()));
                }
            }
        }
    }
    // Only the declaration's own name duplicates the rename; every body edit stays.
    changes.retain(|c| {
        !name_pairs.iter().any(|(o, n)| {
            c.old_node.is_some_and(|x| &x.id == o) || c.new_node.is_some_and(|x| &x.id == n)
        })
    });
}

pub(crate) fn scope_path<'b>(
    index: &'b TreeIndex<'_>,
    node: &SemanticNode,
) -> Vec<(String, String)> {
    let mut path = Vec::new();
    let mut cursor = node.id.as_str();
    while let Some(parent_id) = index.parent.get(cursor) {
        if let Some(parent) = index.by_id.get(parent_id) {
            if is_named_entity_type(&parent.node_type) {
                path.push((parent.node_type.clone(), parent.label.clone()));
            }
        }
        cursor = parent_id;
    }
    path
}
