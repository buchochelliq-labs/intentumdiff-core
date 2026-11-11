//! #299: the surviving sub-expression of a modified statement is part of that modification,
//! not a separate MOVED_CODE group. Real moves stay moves.
use crate::*;
use serde_json::Value;

fn native(old: &str, new: &str) -> Value {
    let request = json!({"schema_version": 1, "python_parser_backend": "native",
        "files": [{"old_source": old, "new_source": new, "old_filename": "pagination.py",
            "new_filename": "pagination.py", "language": "python"}]});
    let payload: Value = serde_json::from_str(&diff_batch(&request.to_string()).unwrap()).unwrap();
    assert_eq!(payload["status"], COMPLETE, "{payload}");
    payload["diffs"][0]["diff"].clone()
}

fn kinds(diff: &Value) -> Vec<String> {
    diff["change_groups"].as_array().unwrap().iter().map(|g| g["kind"].as_str().unwrap().to_owned()).collect()
}

fn assert_single_meaningful(old: &str, new: &str, what: &str) {
    let diff = native(old, new);
    assert_eq!(kinds(&diff), ["MEANINGFUL_CHANGE"], "{what}: {diff:#}");
    assert!(diff["changes"].as_array().unwrap().iter().all(|c| c["change_type"] != "MOVE"), "{what}: {diff:#}");
    assert_eq!(diff["has_semantic_changes"], true, "{what}");
}

const TAIL: &str = "    return items[start:start + per_page]\n";

#[test]
fn removed_operand_is_one_meaningful_change() {
    assert_single_meaningful(
        &format!("def paginate(items, page, per_page):\n    start = page * per_page + 1\n{TAIL}"),
        &format!("def paginate(items, page, per_page):\n    start = page * per_page\n{TAIL}"),
        "removed operand");
}

#[test]
fn added_operand_is_one_meaningful_change() {
    assert_single_meaningful(
        &format!("def paginate(items, page, per_page):\n    start = page * per_page\n{TAIL}"),
        &format!("def paginate(items, page, per_page):\n    start = page * per_page + 1\n{TAIL}"),
        "added operand");
}

#[test]
fn wrapped_expression_is_one_meaningful_change() {
    assert_single_meaningful(
        "def f(x):\n    y = x + 1\n    return y\n",
        "def f(x):\n    y = g(x + 1)\n    return y\n",
        "wrapped expression");
}

#[test]
fn reordered_statements_are_still_moves() {
    let diff = native(
        "def f():\n    a = compute_alpha(1)\n    b = compute_beta(2)\n    c = compute_gamma(3)\n    return a, b, c\n\ndef g():\n    pass\n",
        "def g():\n    pass\n\ndef f():\n    a = compute_alpha(1)\n    b = compute_beta(2)\n    c = compute_gamma(3)\n    return a, b, c\n");
    assert!(kinds(&diff).iter().any(|k| k == "MOVED_CODE"), "{diff:#}");
}

/// #300: a statement modification says WHAT changed (bounded source text), and a deletion
/// of an unlabelled node names the deleted text instead of its node type twice.
#[test]
fn statement_modifications_carry_bounded_source_text() {
    let diff = native(
        &format!("def paginate(items, page, per_page):\n    start = page * per_page + 1\n{TAIL}"),
        &format!("def paginate(items, page, per_page):\n    start = page * per_page\n{TAIL}"));
    let changes = diff["changes"].as_array().unwrap();
    let modification = changes.iter().find(|c| c["change_type"] == "MODIFICATION").unwrap_or_else(|| panic!("{diff:#}"));
    assert_eq!(modification["text_diff"], "start = page * per_page + 1 -> start = page * per_page", "{modification:#}");
    let deletion = changes.iter().find(|c| c["change_type"] == "DELETION").unwrap_or_else(|| panic!("{diff:#}"));
    assert_eq!(deletion["description"], "Delete binary_operator('page * per_page + 1')", "{deletion:#}");
}

#[test]
fn source_text_is_bounded_and_whitespace_collapsed() {
    let long = (0..25).map(|i| format!("value_number_{i}")).collect::<Vec<_>>().join(" +\n        ");
    let diff = native(&format!("def f():\n    total = ({long})\n    return total\n"),
        &format!("def f():\n    total = ({long} + 1)\n    return total\n"));
    let texts: Vec<&str> = diff["changes"].as_array().unwrap().iter().filter_map(|c| c["text_diff"].as_str()).collect();
    assert!(texts.iter().any(|t| t.contains('…')), "a long statement is bounded, not dropped: {diff:#}");
    for change in diff["changes"].as_array().unwrap() {
        if let Some(text) = change["text_diff"].as_str() {
            assert!(!text.contains('\n'), "{text}");
            for side in text.split(" -> ") {
                assert!(side.chars().count() <= crate::source_text::MAX_TEXT_CHARS, "{side}");
            }
        }
    }
}

/// An identity-keyed element (schema coordinate key) is an entity boundary for #299: a MOVE
/// whose path from the modified ancestor runs through `book[isbn]` changed identity, so it is
/// kept. The same drafts without the dialect fold as before (non-vacuity).
#[test]
fn moves_through_identity_keyed_elements_are_kept() {
    fn node(id: &str, node_type: &str, label: &str, children: Vec<Value>) -> Value {
        json!({"id": id, "node_type": node_type, "label": label, "structural_hash": format!("{id}:{label}"),
            "position": {"start_line": 0, "start_col": 0, "end_line": 0, "end_col": 0}, "children": children})
    }
    fn catalog(isbn: &str) -> SemanticNode {
        serde_json::from_value(node("0", "document", "", vec![node("0.0", "element", "catalog", vec![
            node("0.0.0", "element", "book", vec![
                node("0.0.0.0", "element", "isbn", vec![node("0.0.0.0.0", "text", isbn, vec![])]),
                node("0.0.0.1", "element", "price", vec![node("0.0.0.1.0", "text", "10.99", vec![])]),
            ]),
        ])])).unwrap()
    }
    fn find<'n>(n: &'n SemanticNode, id: &str) -> &'n SemanticNode {
        fn walk<'n>(n: &'n SemanticNode, id: &str) -> Option<&'n SemanticNode> {
            if n.id == id { return Some(n); }
            n.children.iter().find_map(|c| walk(c, id))
        }
        walk(n, id).unwrap()
    }
    let (old_tree, new_tree) = (catalog("978-0"), catalog("978-9"));
    let run = |dialects: Vec<UserXmlDialect>| {
        let _scope = xml_schema::scoped_xml_dialects(dialects);
        let draft = |change_type: &'static str, id: &str| ChangeDraft {
            change_type, old_node: Some(find(&old_tree, id)), new_node: Some(find(&new_tree, id)),
            old_index: None, new_index: None, confidence: 1.0, description: String::new(),
            refactoring_kind: None, text_diff: None,
        };
        // The catalog root is the (non-keyed) modification; <price> re-parents between books.
        let mut changes = vec![draft("MODIFICATION", "0.0"), draft("MOVE", "0.0.0.1")];
        suppress_subexpression_moves_inside_modified_statement_drafts(&mut changes, &old_tree, &new_tree, "xml");
        changes.iter().map(|c| c.change_type).collect::<Vec<_>>()
    };
    let dialect: UserXmlDialect = serde_json::from_value(json!({
        "language_id": "acme-catalog", "root_element": "catalog", "keyed_elements": {"book": ["isbn"]}})).unwrap();
    assert_eq!(run(vec![dialect]), ["MODIFICATION", "MOVE"], "a move across book identities is kept");
    assert_eq!(run(Vec::new()), ["MODIFICATION"], "unkeyed, the move folds into the modification");
}
