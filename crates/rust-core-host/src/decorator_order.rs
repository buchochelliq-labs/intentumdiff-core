//! Decorator composition is ordered. Include unchanged siblings when determining
//! relative order: an insertion can leave a necessary anchor at the same index.
use crate::*;

pub(crate) fn preserve_decorator_order<'a>(
    changes: &mut Vec<ChangeDraft<'a>>,
    old: &'a SemanticNode,
    new: &'a SemanticNode,
    old_source: &str,
    new_source: &str,
) {
    let oi = TreeIndex::new(old);
    let ni = TreeIndex::new(new);
    let old_lines: Vec<_> = old_source.lines().collect();
    let new_lines: Vec<_> = new_source.lines().collect();
    let renames: HashSet<_> = changes
        .iter()
        .filter(|c| c.refactoring_kind == Some("RENAME_SYMBOL"))
        .filter_map(|c| Some((c.old_node?.id.as_str(), c.new_node?.id.as_str())))
        .collect();
    let containers = |index: &TreeIndex<'a>| {
        index
            .nodes
            .iter()
            .copied()
            .filter(|n| n.children.iter().any(|c| c.node_type == "decorator"))
            .collect::<Vec<_>>()
    };
    let mut pending = Vec::new();
    for o in containers(&oi) {
        let Some(of) = o
            .children
            .iter()
            .find(|c| is_named_entity_type(&c.node_type))
        else {
            continue;
        };
        let candidates: Vec<_> = containers(&ni)
            .into_iter()
            .filter(|n| {
                n.children.iter().any(|nf| {
                    nf.node_type == of.node_type
                        && (nf.label == of.label
                            || renames.contains(&(of.id.as_str(), nf.id.as_str())))
                        && callable_renames::scope_path(&oi, of)
                            == callable_renames::scope_path(&ni, nf)
                })
            })
            .collect();
        if candidates.len() != 1 {
            continue;
        }
        let n = candidates[0];
        let ods: Vec<_> = o
            .children
            .iter()
            .filter(|c| c.node_type == "decorator")
            .collect();
        let nds: Vec<_> = n
            .children
            .iter()
            .filter(|c| c.node_type == "decorator")
            .collect();
        let text = |d: &SemanticNode, lines: &[&str]| {
            normalized_decorator(&slice_source_text(lines, &d.position))
        };
        let mut pairs = Vec::new();
        for (a, od) in ods.iter().enumerate() {
            let key = text(od, &old_lines);
            if key.is_empty() || ods.iter().filter(|d| text(d, &old_lines) == key).count() != 1 {
                continue;
            }
            let hits: Vec<_> = nds
                .iter()
                .enumerate()
                .filter(|(_, d)| text(d, &new_lines) == key)
                .collect();
            if hits.len() == 1 {
                pairs.push((a, hits[0].0));
            }
        }
        let sequence: Vec<_> = pairs.iter().map(|(_, b)| *b).collect();
        let stationary = longest_increasing_subsequence_positions(&sequence);
        for (idx, (a, b)) in pairs.iter().enumerate() {
            if !stationary.contains(&idx) {
                pending.push((ods[*a], nds[*b], *a, *b));
            }
        }
    }
    for (old_node, new_node, a, b) in pending {
        changes.retain(|c| {
            !(c.old_node.is_some_and(|n| n.id == old_node.id)
                && c.new_node.is_some_and(|n| n.id == new_node.id)
                && matches!(c.change_type, "REORDER" | "MOVE" | "MODIFICATION"))
        });
        changes.push(ChangeDraft {
            change_type: "MODIFICATION",
            old_node: Some(old_node),
            new_node: Some(new_node),
            old_index: Some(a),
            new_index: Some(b),
            confidence: 1.0,
            description: format!(
                "Reorder decorator '{}' relative to its siblings",
                old_node.label
            ),
            refactoring_kind: None,
            text_diff: None,
        });
    }
}

// Ignore inter-token formatting, preserving quoted contents, escapes and word
// boundaries. This never equates different operators or decorator argument values.
fn normalized_decorator(source: &str) -> String {
    if source.contains("\"\"\"") || source.contains("'''") || source.contains('#') {
        return source.trim().to_owned();
    }
    let mut out = String::new();
    let mut quote = None;
    let mut escape = false;
    let mut space = false;
    for c in source.chars() {
        if let Some(q) = quote {
            out.push(c);
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        if c.is_whitespace() {
            space = true;
            continue;
        }
        let word = |ch: char| ch.is_alphanumeric() || ch == '_';
        if space && out.chars().last().is_some_and(word) && word(c) {
            out.push(' ');
        }
        space = false;
        if c == '\'' || c == '"' {
            quote = Some(c);
        }
        out.push(c);
    }
    out
}
