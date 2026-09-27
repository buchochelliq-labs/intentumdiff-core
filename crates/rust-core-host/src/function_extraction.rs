//! Conservative Python expression extraction: an added undecorated helper returns
//! the exact expression replaced by its call in a continuing callable. This is a
//! refactoring classification, not a claim that arbitrary helper additions are safe.
use crate::*;

pub(crate) fn promote_expression_extractions<'a>(
    changes: &mut Vec<ChangeDraft<'a>>,
    old: &'a SemanticNode,
    new: &'a SemanticNode,
    old_source: &str,
    new_source: &str,
) {
    let oi = TreeIndex::new(old);
    let ni = TreeIndex::new(new);
    let ol: Vec<_> = old_source.lines().collect();
    let nl: Vec<_> = new_source.lines().collect();
    let snippet =
        |n: &SemanticNode, lines: &[&str]| slice_source_text(lines, &n.position).trim().to_owned();
    let renames: HashSet<_> = changes
        .iter()
        .filter(|c| c.refactoring_kind == Some("RENAME_SYMBOL"))
        .filter_map(|c| Some((c.old_node?.id.as_str(), c.new_node?.id.as_str())))
        .collect();
    let mut candidates = Vec::new();
    for change in changes.iter() {
        if change.change_type != "ADDITION" {
            continue;
        }
        let Some(helper) = change
            .new_node
            .filter(|n| n.node_type == "function_definition")
        else {
            continue;
        };
        let Some(hp) = ni.parent.get(helper.id.as_str()) else {
            continue;
        };
        // Restrict to module-level helpers: methods/closures have additional binding semantics.
        if !ni.by_id.get(hp).is_some_and(|p| p.node_type == "module") {
            continue;
        }
        if oi.named_entities.iter().any(|n| n.label == helper.label)
            || ni
                .named_entities
                .iter()
                .filter(|n| n.label == helper.label)
                .count()
                != 1
            || !snippet(helper, &nl).starts_with("def ")
        {
            continue;
        }
        // Binding changes can make a syntactically identical call invoke something
        // else. Decline any other use/binding of this name beyond direct calls and
        // this declaration. Also decline annotations (which may execute at definition).
        if helper
            .children
            .iter()
            .any(|c| !matches!(c.node_type.as_str(), "identifier" | "parameters" | "block"))
            || ni.nodes.iter().any(|n| {
                n.node_type == "identifier"
                    && n.label == helper.label
                    && !ni
                        .parent
                        .get(n.id.as_str())
                        .and_then(|p| ni.by_id.get(p))
                        .is_some_and(|p| {
                            (p.id == helper.id
                                && helper.children.first().is_some_and(|c| c.id == n.id))
                                || (p.node_type == "call"
                                    && p.label == helper.label
                                    && p.children.first().is_some_and(|c| c.id == n.id))
                        })
            })
        {
            continue;
        }
        let Some(params) = helper.children.iter().find(|c| c.node_type == "parameters") else {
            continue;
        };
        if params.children.iter().any(|c| c.node_type != "identifier") {
            continue;
        }
        let names: Vec<_> = params.children.iter().map(|c| c.label.as_str()).collect();
        if names.is_empty() {
            continue;
        }
        let Some(block) = helper.children.iter().find(|c| c.node_type == "block") else {
            continue;
        };
        if block.children.len() != 1
            || block.children[0].node_type != "return_statement"
            || block.children[0].children.len() != 1
        {
            continue;
        }
        let expr = &block.children[0].children[0];
        fn supported(n: &SemanticNode, names: &[&str], property: bool) -> bool {
            match n.node_type.as_str() {
                "identifier" => property || names.contains(&n.label.as_str()),
                "integer" | "float" => true,
                "attribute" => {
                    n.children.len() == 2
                        && supported(&n.children[0], names, false)
                        && supported(&n.children[1], names, true)
                }
                "binary_operator" | "parenthesized_expression" => {
                    n.children.iter().all(|c| supported(c, names, false))
                }
                _ => false,
            }
        }
        if !supported(expr, &names, false) {
            continue;
        }
        let expression_text = snippet(expr, &nl);
        if expression_text.is_empty() {
            continue;
        }
        for caller in ni.named_entities.iter().copied().filter(|n| {
            n.node_type == "function_definition"
                && n.id != helper.id
                && ni.parent.get(n.id.as_str()) == Some(hp)
                && helper.position.start_line < n.position.start_line
        }) {
            let old_callers: Vec<_> = oi
                .named_entities
                .iter()
                .copied()
                .filter(|o| {
                    o.node_type == caller.node_type
                        && (o.label == caller.label
                            || renames.contains(&(o.id.as_str(), caller.id.as_str())))
                        && callable_renames::scope_path(&oi, o)
                            == callable_renames::scope_path(&ni, caller)
                })
                .collect();
            if old_callers.len() != 1 {
                continue;
            }
            let old_caller = old_callers[0];
            for call in caller
                .descendants()
                .into_iter()
                .filter(|n| n.node_type == "call" && n.label == helper.label)
            {
                let Some(args) = call
                    .children
                    .iter()
                    .find(|n| n.node_type == "argument_list")
                else {
                    continue;
                };
                if args.children.len() != names.len()
                    || args
                        .children
                        .iter()
                        .zip(&names)
                        .any(|(arg, name)| arg.node_type != "identifier" || arg.label != *name)
                {
                    continue;
                }
                // The call replaces this expression at the same structural location
                // within the established caller. Require exact source AND its context.
                let Some(relative) = call.id.strip_prefix(&caller.id) else {
                    continue;
                };
                let old_id = format!("{}{relative}", old_caller.id);
                let Some(old_expr) = oi.by_id.get(old_id.as_str()).copied() else {
                    continue;
                };
                if snippet(old_expr, &ol) != expression_text {
                    continue;
                }
                let (Some(op), Some(np)) = (
                    oi.parent.get(old_expr.id.as_str()),
                    ni.parent.get(call.id.as_str()),
                ) else {
                    continue;
                };
                let (Some(op), Some(np)) = (oi.by_id.get(op), ni.by_id.get(np)) else {
                    continue;
                };
                let call_text = snippet(call, &nl);
                let new_context = snippet(np, &nl);
                if call_text.is_empty()
                    || new_context.matches(&call_text).count() != 1
                    || snippet(op, &ol) != new_context.replacen(&call_text, &expression_text, 1)
                {
                    continue;
                }
                candidates.push((helper, old_expr, call));
            }
        }
    }
    // One occurrence per helper for now; multiple competing evidence locations stay explicit.
    let chosen: Vec<_> = candidates
        .iter()
        .copied()
        .filter(|(h, _, _)| {
            candidates
                .iter()
                .filter(|(other, _, _)| other.id == h.id)
                .count()
                == 1
        })
        .collect();
    for (helper, expression, call) in chosen {
        let within = |id: &str, root: &str| id == root || id.starts_with(&format!("{root}."));
        changes.retain(|c| {
            !(c.new_node
                .is_some_and(|n| within(&n.id, &helper.id) || within(&n.id, &call.id))
                || c.old_node.is_some_and(|n| within(&n.id, &expression.id)))
        });
        changes.push(ChangeDraft {
            change_type: "REFACTORING",
            refactoring_kind: Some("EXTRACT_FUNCTION"),
            old_node: Some(expression),
            new_node: Some(helper),
            old_index: None,
            new_index: None,
            confidence: 0.95,
            description: format!("Extract expression into function '{}'", helper.label),
            text_diff: None,
        });
    }
}
