//! AssemblyScript binary-operator source edits are semantic expression changes (#191).
//! Tree-Sitter exposes an operator as an anonymous leaf whose *node type* changes
//! (for example `*` -> `**`). The generic matcher correctly declines to pair
//! differently typed leaves, but its separate delete/add drafts must not obscure
//! the changed continuing expression or its other edited operands.
use crate::*;

const OPERATORS: &[&str] = &[
    "+", "-", "*", "**", "/", "%", "<<", ">>", ">>>", "&", "|", "^",
];

fn operator(node: &SemanticNode) -> Option<&SemanticNode> {
    let mut operators = node.children.iter().filter(|child| {
        OPERATORS.contains(&child.node_type.as_str()) && child.label == child.node_type
    });
    let only = operators.next()?;
    if operators.next().is_some() {
        return None; // ambiguous grammar: do not manufacture a pairing
    }
    Some(only)
}

fn inside(id: &str, parent: &str) -> bool {
    id == parent || id.strip_prefix(parent).is_some_and(|tail| tail.starts_with('.'))
}

/// Coalesce only a source-proven operator deletion/addition inside a continuing,
/// uniquely named AssemblyScript function. The expression's complete old/new
/// subtrees retain both the changed operator and any changed operands.
/// Do not infer renames, drop independent entities, or change another language.
pub(crate) fn promote_assemblyscript_operator_expression_changes<'a>(
    changes: &mut Vec<ChangeDraft<'a>>,
    old_tree: &'a SemanticNode,
    new_tree: &'a SemanticNode,
    language: &str,
) {
    if language != "assemblyscript" {
        return;
    }
    let old_functions: Vec<_> = old_tree.descendants().into_iter().filter(|node| {
        node.node_type == "function_declaration"
            && !node.label.is_empty() && node.label != "(anonymous)"
    }).collect();
    let new_functions: Vec<_> = new_tree.descendants().into_iter().filter(|node| {
        node.node_type == "function_declaration"
            && !node.label.is_empty() && node.label != "(anonymous)"
    }).collect();

    #[cfg(test)]
    eprintln!("[assemblyscript-diagnostic] functions old={:?} new={:?}; drafts={:?}",
        old_functions.iter().map(|n| (&n.id, &n.label)).collect::<Vec<_>>(),
        new_functions.iter().map(|n| (&n.id, &n.label)).collect::<Vec<_>>(),
        changes.iter().map(|c| (c.change_type,
            c.old_node.map(|n| (&n.id, &n.node_type)),
            c.new_node.map(|n| (&n.id, &n.node_type)))).collect::<Vec<_>>());
    for old_function in &old_functions {
        if old_functions.iter().filter(|f| f.label == old_function.label).count() != 1 {
            continue;
        }
        let counterparts: Vec<_> = new_functions.iter()
            .filter(|f| f.label == old_function.label).collect();
        if counterparts.len() != 1 {
            continue;
        }
        let new_function = *counterparts[0];
        let new_descendants = new_function.descendants();
        for old_expression in old_function.descendants() {
            if old_expression.node_type != "binary_expression" {
                continue;
            }
            let Some(relative_id) = old_expression.id.strip_prefix(&old_function.id) else {
                continue;
            };
            if !relative_id.starts_with('.') {
                continue;
            }
            let corresponding_id = format!("{}{relative_id}", new_function.id);
            let Some(new_expression) = new_descendants.iter().copied().find(|n| {
                n.id == corresponding_id && n.node_type == "binary_expression"
            }) else {
                continue;
            };
            let (Some(old_operator), Some(new_operator)) =
                (operator(old_expression), operator(new_expression))
            else {
                continue;
            };
            if old_operator.node_type == new_operator.node_type {
                continue;
            }
            // Require two actual raw edit-script events. An observed operator
            // pair, not a coincidental structural position, justifies this edit.
            let deleted = changes.iter().any(|change| {
                change.change_type == "DELETION"
                    && change.old_node.is_some_and(|n| n.id == old_operator.id)
            });
            let added = changes.iter().any(|change| {
                change.change_type == "ADDITION"
                    && change.new_node.is_some_and(|n| n.id == new_operator.id)
            });
            #[cfg(test)]
            eprintln!("[assemblyscript-diagnostic] old function={} new function={} oldexpr={} newexpr={} operator={}->{} deleted={} added={}",
                old_function.label, new_function.label,
                old_expression.id, new_expression.id,
                old_operator.node_type, new_operator.node_type, deleted, added);
            if !deleted || !added {
                continue;
            }
            // A nested refactoring/move is independent evidence and must survive;
            // decline this compaction rather than hide it.
            if changes.iter().any(|change| {
                matches!(change.change_type, "REFACTORING" | "MOVE" | "REORDER")
                    && (change.old_node.is_some_and(|n| inside(&n.id, &old_expression.id))
                        || change.new_node.is_some_and(|n| inside(&n.id, &new_expression.id)))
            }) {
                continue;
            }
            changes.retain(|change| match change.change_type {
                "DELETION" => !change.old_node.is_some_and(|n| inside(&n.id, &old_expression.id)),
                "ADDITION" => !change.new_node.is_some_and(|n| inside(&n.id, &new_expression.id)),
                "MODIFICATION" => !(change.old_node.is_some_and(|n| inside(&n.id, &old_expression.id))
                    && change.new_node.is_some_and(|n| inside(&n.id, &new_expression.id))),
                _ => true,
            });
            changes.push(ChangeDraft {
                change_type: "MODIFICATION",
                old_node: Some(old_expression),
                new_node: Some(new_expression),
                old_index: None,
                new_index: None,
                confidence: 1.0,
                description: format!(
                    "Change AssemblyScript binary expression in {}: {} -> {}",
                    old_function.label, old_operator.label, new_operator.label
                ),
                refactoring_kind: None,
                text_diff: None,
            });
        }
    }
}

#[cfg(all(test, feature = "tier-c-wasm"))]
#[path = "assemblyscript_operator_source_tests.rs"]
mod source_tests;
