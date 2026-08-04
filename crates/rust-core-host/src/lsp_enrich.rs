//! LSP hover-target collection (#100 S2 slice 3) — the tree-aware half of
//! `TypeEnricher` (`src/intentumdiff/lsp/enricher.py`), ported walk-for-walk. The generic
//! transport half (batch hover under a concurrency cap) lives in `crates/lsp-client`;
//! this module only decides WHERE to hover and WHICH node id receives each result.
//!
//! Semantics mirrored exactly:
//! - name-type leaves (`_NAME_TYPES`) hover at their own position, result under their id;
//! - declaration/parameter nodes (`_DECL_NODE_TYPES` / `_PARAM_NODE_TYPES`) hover at their
//!   first name-leaf descendant's position but store under the DECLARATION node's id — the
//!   node refactoring rules read `type_info` from;
//! - pre-order walk, result ids deduplicated (first occurrence wins).
//!
//! Works over the tree JSON (`serde_json::Value`) rather than the core's `SemanticNode`
//! struct so it accepts any tree the Python side holds (enriched or bare).

use serde_json::{json, Value};

const NAME_TYPES: [&str; 4] = ["function_name", "class_name", "variable_name", "method_name"];

const DECL_OR_PARAM_TYPES: [&str; 11] = [
    // _DECL_NODE_TYPES
    "assignment",
    "variable_declaration",
    "let_declaration",
    "const_declaration",
    "local_variable_declaration",
    "variable_declarator",
    // _PARAM_NODE_TYPES
    "parameter",
    "typed_parameter",
    "typed_identifier",
    "required_parameter",
    "optional_parameter",
];

/// Leaf name types accepted by the first-name-leaf walk (`_first_name_leaf`): the
/// name types plus the generic identifier spellings.
const LEAF_NAME_TYPES: [&str; 7] = [
    "function_name",
    "class_name",
    "variable_name",
    "method_name",
    "identifier",
    "name",
    "symbol",
];

fn node_type(node: &Value) -> &str {
    node.get("node_type").and_then(Value::as_str).unwrap_or("")
}

fn node_id(node: &Value) -> &str {
    node.get("id").and_then(Value::as_str).unwrap_or("")
}

fn children(node: &Value) -> &[Value] {
    node.get("children")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn is_leaf(node: &Value) -> bool {
    children(node).is_empty()
}

fn start_position(node: &Value) -> (u32, u32) {
    let position = node.get("position");
    let line = position
        .and_then(|p| p.get("start_line"))
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;
    let col = position
        .and_then(|p| p.get("start_col"))
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;
    (line, col)
}

/// `_first_name_leaf`: the first LEAF descendant (children order, depth-first) whose
/// node_type is a name type. NB the walk inspects children only, never the node itself.
fn first_name_leaf<'a>(node: &'a Value) -> Option<&'a Value> {
    for child in children(node) {
        if is_leaf(child) && LEAF_NAME_TYPES.contains(&node_type(child)) {
            return Some(child);
        }
        if let Some(found) = first_name_leaf(child) {
            return Some(found);
        }
    }
    None
}

/// `_collect_hover_targets`, flattened to `(result_id, line, col)` triples in walk order.
pub(crate) fn collect_hover_targets(root: &Value) -> Vec<(String, u32, u32)> {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut targets: Vec<(String, u32, u32)> = Vec::new();
    let mut stack: Vec<&Value> = vec![root];
    // Pre-order walk matching Python's `[root, *root.descendants()]`.
    while let Some(node) = stack.pop() {
        let id = node_id(node);
        if !seen.contains(id) {
            let ty = node_type(node);
            if NAME_TYPES.contains(&ty) && is_leaf(node) {
                seen.insert(id.to_owned());
                let (line, col) = start_position(node);
                targets.push((id.to_owned(), line, col));
            } else if DECL_OR_PARAM_TYPES.contains(&ty) {
                if let Some(leaf) = first_name_leaf(node) {
                    seen.insert(id.to_owned());
                    let (line, col) = start_position(leaf);
                    targets.push((id.to_owned(), line, col));
                }
            }
        }
        for child in children(node).iter().rev() {
            stack.push(child);
        }
    }
    targets
}

pub(crate) fn collect_hover_targets_json_impl(tree_json: &str) -> Result<String, String> {
    let root: Value = serde_json::from_str(tree_json)
        .map_err(|e| format!("invalid semantic tree json: {e}"))?;
    let triples: Vec<Value> = collect_hover_targets(&root)
        .into_iter()
        .map(|(id, line, col)| json!({"id": id, "line": line, "col": col}))
        .collect();
    Ok(Value::Array(triples).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(id: &str, node_type: &str, line: u32, col: u32) -> Value {
        json!({
            "id": id, "node_type": node_type, "label": id,
            "position": {"start_line": line, "start_col": col, "end_line": line, "end_col": col},
            "structural_hash": "h", "children": [],
        })
    }

    fn parent(id: &str, node_type: &str, children: Vec<Value>) -> Value {
        json!({
            "id": id, "node_type": node_type, "label": id,
            "position": {"start_line": 0, "start_col": 0, "end_line": 9, "end_col": 0},
            "structural_hash": "h", "children": children,
        })
    }

    #[test]
    fn name_leaves_hover_at_their_own_position() {
        let root = parent("m", "module", vec![leaf("f", "function_name", 3, 4)]);
        assert_eq!(collect_hover_targets(&root), vec![("f".to_owned(), 3, 4)]);
    }

    #[test]
    fn generic_identifiers_and_non_name_leaves_are_not_targets() {
        let root = parent(
            "m",
            "module",
            vec![leaf("i", "identifier", 1, 0), leaf("s", "string", 2, 0)],
        );
        assert!(collect_hover_targets(&root).is_empty());
    }

    #[test]
    fn declaration_hovers_at_name_leaf_but_stores_under_the_declaration_id() {
        let root = parent(
            "m",
            "module",
            vec![parent(
                "a1",
                "assignment",
                vec![leaf("v", "identifier", 5, 2), leaf("lit", "integer", 5, 6)],
            )],
        );
        assert_eq!(collect_hover_targets(&root), vec![("a1".to_owned(), 5, 2)]);
    }

    #[test]
    fn declaration_without_a_name_leaf_is_skipped() {
        let root = parent(
            "m",
            "module",
            vec![parent("a1", "assignment", vec![leaf("lit", "integer", 5, 6)])],
        );
        assert!(collect_hover_targets(&root).is_empty());
    }

    #[test]
    fn non_leaf_name_types_are_not_own_position_targets() {
        // A function_name with children is not a leaf → not hovered directly.
        let root = parent(
            "m",
            "module",
            vec![parent("fn", "function_name", vec![leaf("x", "noise", 0, 0)])],
        );
        assert!(collect_hover_targets(&root).is_empty());
    }

    #[test]
    fn duplicate_result_ids_are_collected_once_in_preorder() {
        let root = parent(
            "m",
            "module",
            vec![
                leaf("dup", "function_name", 1, 0),
                leaf("dup", "function_name", 2, 0),
                parent(
                    "p1",
                    "typed_parameter",
                    vec![leaf("pn", "name", 4, 8)],
                ),
            ],
        );
        assert_eq!(
            collect_hover_targets(&root),
            vec![("dup".to_owned(), 1, 0), ("p1".to_owned(), 4, 8)]
        );
    }
}
