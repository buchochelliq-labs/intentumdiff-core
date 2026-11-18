//! `keyed_data.number_exact_decimal_respelling` on synthetic trees: the leaf proof, the
//! whole-document proof, and the routed-review verdict (core #252 `equivalent` + proof id).
use super::*;

/// One-line JSON-number source holding `slots` (`(key, value-token)`), and its tree in the
/// configuration-projection shape (plugin-sdk #36): `object > property(key) > number|string`,
/// the number label being the exact source token and its hash the given identity.
fn document(slots: &[(&str, &str)]) -> (String, SemanticNode) {
    let mut source = String::from("{");
    let mut properties = Vec::new();
    for (index, (key, token)) in slots.iter().enumerate() {
        if index > 0 {
            source.push(',');
        }
        source.push_str(&format!("\"{key}\":"));
        let start = source.len();
        source.push_str(token);
        let (node_type, label) = match token.strip_prefix('"') {
            Some(text) => ("string", text.trim_end_matches('"')),
            None => ("number", *token),
        };
        let hash = exact_json_number(label).filter(|_| node_type == "number").unwrap_or_else(|| label.to_owned());
        properties.push(json!({
            "id": format!("0.{index}"), "node_type": "property", "label": key, "structural_hash": format!("p:{key}:{hash}"),
            "position": {"start_line": 0, "start_col": start - key.len() - 3, "end_line": 0, "end_col": start - 1},
            "children": [{
                "id": format!("0.{index}.0"), "node_type": node_type, "label": label,
                "structural_hash": format!("{node_type}:{hash}"),
                "position": {"start_line": 0, "start_col": start, "end_line": 0, "end_col": source.len()},
                "children": [],
            }],
        }));
    }
    source.push('}');
    let tree = serde_json::from_value(json!({
        "id": "0", "node_type": "object", "label": "", "structural_hash": "root",
        "position": {"start_line": 0, "start_col": 0, "end_line": 0, "end_col": source.len()},
        "children": properties,
    }))
    .expect("valid synthetic tree");
    (source, tree)
}

fn leaf<'t>(tree: &'t SemanticNode, index: usize) -> &'t SemanticNode {
    &tree.children[index].children[0]
}

fn modification(old: &SemanticNode, new: &SemanticNode) -> Value {
    json!({"change_type": "MODIFICATION", "old_node": old, "new_node": new, "confidence": 1.0,
        "description": "Update"})
}

/// The leaf MODIFICATIONs the edit script emits for every slot whose label differs.
fn changes(old: &SemanticNode, new: &SemanticNode) -> Vec<Value> {
    (0..old.children.len())
        .filter(|&i| leaf(old, i).label != leaf(new, i).label || leaf(old, i).node_type != leaf(new, i).node_type)
        .map(|i| modification(leaf(old, i), leaf(new, i)))
        .collect()
}

struct Reviewed {
    invariances: Value,
    routed: Value,
}

fn review_with(language: &str, old: &[(&str, &str)], new: &[(&str, &str)],
    pick: impl Fn(&SemanticNode, &SemanticNode) -> Vec<Value>) -> Reviewed {
    let (old_source, old_tree) = document(old);
    let (new_source, new_tree) = document(new);
    let drafts = pick(&old_tree, &new_tree);
    let invariances = crate::invariance_groups::rust_apply_review_invariances(&json!({
        "language": language, "old_source": old_source, "new_source": new_source,
        "old_tree": old_tree, "new_tree": new_tree, "changes": drafts,
    }))
    .expect("invariances");
    let routed = crate::routed_review::complete(&json!({
        "language": language, "old_source": old_source, "new_source": new_source,
        "old_filename": "pipeline.json", "new_filename": "pipeline.json",
        "old_tree": old_tree, "new_tree": new_tree,
        "finalized": {"changes": drafts, "change_groups": [], "is_style_only": false, "no_surviving_changes": false},
    }))
    .expect("routed review");
    Reviewed { invariances, routed }
}

fn review(language: &str, old: &[(&str, &str)], new: &[(&str, &str)]) -> Reviewed {
    review_with(language, old, new, changes)
}

fn rule_groups(diff: &Value) -> Vec<&Value> {
    diff["change_groups"].as_array().unwrap().iter()
        .filter(|g| g["rule_id"] == NUMBER_RESPELLING_RULE).collect()
}

fn labels(diff: &Value) -> Vec<(String, String)> {
    diff["changes"].as_array().unwrap().iter().map(|c| (
        c["old_node"]["label"].as_str().unwrap_or_default().to_owned(),
        c["new_node"]["label"].as_str().unwrap_or_default().to_owned(),
    )).collect()
}

#[test]
fn exact_json_number_equates_spellings_of_one_value_only() {
    for group in [
        &["9", "9.0", "9.00", "0.9e1", "90e-1", "9E0", "9e+0", "9e-0"][..],
        &["1000", "1e3", "1E3", "1e+3", "1.000e3", "10e2", "0.001e6"][..],
        &["0.01", "1e-2", "1E-2", "0.0100", "10e-3"][..],
        &["-12.5", "-12.50", "-125e-1", "-1.25E1"][..],
        &["0", "0.0", "0e5", "0.000e-7"][..],
    ] {
        let canonical: Vec<_> = group.iter().map(|t| exact_json_number(t).unwrap_or_else(|| panic!("{t}"))).collect();
        assert!(canonical.windows(2).all(|w| w[0] == w[1]), "{group:?}: {canonical:?}");
    }
    for (a, b) in [("9", "10"), ("9", "-9"), ("1.0", "1.01"), ("1e3", "1e4"), ("-0", "0"), ("0.1", "0.10000000000000001")] {
        assert_ne!(exact_json_number(a), exact_json_number(b), "{a} vs {b}");
    }
    // Spellings the RFC 8259 grammar does not prove, and values outside the projection domain.
    for token in ["1_000", "0x10", "+1", ".5", "1.", "01", "-01", "Infinity", "NaN", "1e", "1e+", "--1",
        "1e1000001", "10e1000000", "1e-1000001", "0.1e-1000000", "1e99999999999999999999", " 1", "1 "] {
        assert_eq!(exact_json_number(token), None, "{token}");
    }
}

#[test]
fn respelling_only_edit_is_equivalent_with_the_proof_id() {
    for language in ["adf", "json", "databricks-workflow", "databricks"] {
        for (old, new) in [("9", "9.0"), ("1000", "1e3"), ("1E22", "1e+22"), ("0.5", "5E-1"), ("-12.5", "-1.250e1")] {
            let r = review(language, &[("retry", old), ("ratio", "1")], &[("retry", new), ("ratio", "1")]);
            assert_eq!(r.invariances["changes"], json!([]), "{language} {old}->{new}: {}", r.invariances);
            assert_eq!(r.invariances["equivalence_proof"], NUMBER_RESPELLING_RULE);
            let groups = rule_groups(&r.invariances);
            assert_eq!(groups.len(), 1, "{}", r.invariances);
            assert_eq!(groups[0]["kind"], "IGNORED_STYLE");
            assert_eq!(groups[0]["old_labels"], json!([old]), "spellings are shown as written");
            assert_eq!(groups[0]["new_labels"], json!([new]));
            assert_eq!(groups[0]["old_node_ids"], json!(["0.0.0"]));
            assert_eq!(groups[0]["raw_change_indices"], json!([]), "owns no final change");

            let d = &r.routed;
            assert_eq!(d["completeness"], "equivalent", "{language} {old}->{new}: {d}");
            assert_eq!(d["is_style_only"], false, "value equivalence is proved, not style: {d}");
            assert_eq!(d["has_semantic_changes"], false);
            assert_eq!(d["changes"], json!([]));
            assert_eq!(d["metadata"]["equivalence_proof"], NUMBER_RESPELLING_RULE, "{d}");
            assert!(d["metadata"].get("no_surviving_changes").is_none(), "{d}");
            assert!(d["incomplete"].is_null(), "{d}");
            assert_eq!(rule_groups(d).len(), 1, "{d}");
        }
    }
}

#[test]
fn real_value_changes_stay_meaningful_with_source_labels() {
    for (old, new) in [("9", "10"), ("9", "-9"), ("1.0", "1.01"), ("9", "\"9\""), ("1e3", "1e4"), ("-0", "0"),
        ("0.1", "0.10000000000000001")] {
        let r = review("adf", &[("retry", old)], &[("retry", new)]);
        let d = &r.routed;
        assert_eq!(d["completeness"], "complete", "{old}->{new}: {d}");
        assert_eq!(d["has_semantic_changes"], true, "{d}");
        let shown = new.trim_matches('"');
        assert_eq!(labels(d), vec![(old.to_owned(), shown.to_owned())], "labelled as written: {d}");
        assert!(rule_groups(d).is_empty(), "{d}");
        assert!(d["metadata"].get("equivalence_proof").is_none(), "{d}");
        assert!(r.invariances["equivalence_proof"].is_null());
    }
}

#[test]
fn mixed_edit_shows_only_the_real_change() {
    let r = review("adf", &[("retry", "9"), ("ratio", "1"), ("timeout", "1000")],
        &[("retry", "9.0"), ("ratio", "2"), ("timeout", "1e3")]);
    let d = &r.routed;
    assert_eq!(d["completeness"], "complete", "{d}");
    assert_eq!(labels(d), vec![("1".to_owned(), "2".to_owned())], "{d}");
    assert_eq!(rule_groups(d).len(), 2, "both respellings are recorded as evidence: {d}");
    assert!(d["metadata"].get("equivalence_proof").is_none(), "a real change survives: {d}");
    assert_eq!(d["has_semantic_changes"], true, "{d}");
}

#[test]
fn unprovable_spellings_are_declined() {
    for (old, new) in [("1_000", "1000"), ("0x10", "16"), ("+1", "1"), (".5", "0.5"), ("1.", "1"), ("01", "1"),
        ("1e1000001", "10e1000000"), ("Infinity", "1e400")] {
        let r = review("adf", &[("retry", old)], &[("retry", new)]);
        assert_eq!(r.invariances["changes"].as_array().unwrap().len(), 1, "{old}->{new}: {}", r.invariances);
        assert!(rule_groups(&r.invariances).is_empty(), "{old}->{new}");
        assert_eq!(r.routed["completeness"], "complete", "{}", r.routed);
        assert_eq!(labels(&r.routed), vec![(old.to_owned(), new.to_owned())]);
    }
}

#[test]
fn not_a_blanket_equal_hash_rule() {
    // A label-only update between equal-hash STRING leaves is not a number respelling.
    let (old_source, mut old_tree) = document(&[("name", "\"a\"")]);
    let (new_source, mut new_tree) = document(&[("name", "\"b\"")]);
    old_tree.children[0].children[0].structural_hash = "same".into();
    new_tree.children[0].children[0].structural_hash = "same".into();
    let change = modification(leaf(&old_tree, 0), leaf(&new_tree, 0));
    let out = crate::invariance_groups::rust_apply_review_invariances(&json!({"language": "adf",
        "old_source": old_source, "new_source": new_source, "old_tree": old_tree, "new_tree": new_tree,
        "changes": [change]})).unwrap();
    assert_eq!(out["changes"].as_array().unwrap().len(), 1, "{out}");
    // Languages whose number nodes are not RFC 8259 numbers are outside the rule.
    for language in ["yaml", "toml", "javascript", "python"] {
        let r = review(language, &[("retry", "9")], &[("retry", "9.0")]);
        assert!(rule_groups(&r.invariances).is_empty(), "{language}: {}", r.invariances);
        assert!(r.invariances["equivalence_proof"].is_null(), "{language}");
    }
    // A non-MODIFICATION (a DELETION + ADDITION pair of equal values) is not rewritten.
    let r = review_with("adf", &[("retry", "9")], &[("retry", "9.0")], |old, new| vec![
        json!({"change_type": "DELETION", "old_node": leaf(old, 0), "new_node": null}),
        json!({"change_type": "ADDITION", "old_node": null, "new_node": leaf(new, 0)}),
    ]);
    assert_eq!(r.invariances["changes"].as_array().unwrap().len(), 2, "{}", r.invariances);
}

#[test]
fn a_split_number_token_proves_nothing() {
    // A parser that kept only `22` of `1e+22` (the label matches its span, but the span is
    // not the whole token) must not equate `1e+22` with `22.0`.
    let (old_source, mut old_tree) = document(&[("a", "1e+22")]);
    let (new_source, new_tree) = document(&[("a", "22.0")]);
    let node = &mut old_tree.children[0].children[0];
    node.label = "22".into();
    node.position.start_col += 3;
    let change = modification(leaf(&old_tree, 0), leaf(&new_tree, 0));
    let out = crate::invariance_groups::rust_apply_review_invariances(&json!({"language": "json",
        "old_source": old_source, "new_source": new_source, "old_tree": old_tree, "new_tree": new_tree,
        "changes": [change]})).unwrap();
    assert_eq!(out["changes"].as_array().unwrap().len(), 1, "{out}");
    // A label that is not the span's bytes is not trusted either.
    let (old_source, mut old_tree) = document(&[("a", "9")]);
    let (new_source, new_tree) = document(&[("a", "9.0")]);
    old_tree.children[0].children[0].label = "9e0".into();
    let change = modification(leaf(&old_tree, 0), leaf(&new_tree, 0));
    let out = crate::invariance_groups::rust_apply_review_invariances(&json!({"language": "adf",
        "old_source": old_source, "new_source": new_source, "old_tree": old_tree, "new_tree": new_tree,
        "changes": [change]})).unwrap();
    assert_eq!(out["changes"].as_array().unwrap().len(), 1, "{out}");
}

#[test]
fn an_unexplained_delta_keeps_the_respelling_instead_of_claiming_equivalence() {
    // The only draft is the respelling, but another value changed too (a lost draft): the
    // whole-document proof fails, so nothing is suppressed and no proof is claimed.
    let r = review_with("adf", &[("retry", "9"), ("ratio", "1")], &[("retry", "9.0"), ("ratio", "2")],
        |old, new| vec![modification(leaf(old, 0), leaf(new, 0))]);
    assert_eq!(r.invariances["changes"].as_array().unwrap().len(), 1, "{}", r.invariances);
    assert!(rule_groups(&r.invariances).is_empty());
    assert!(r.invariances["equivalence_proof"].is_null());
    assert_eq!(r.routed["completeness"], "complete", "{}", r.routed);
    assert_eq!(labels(&r.routed), vec![("9".to_owned(), "9.0".to_owned())]);
}

#[test]
fn document_proof_compares_json_tokens_and_other_sources_byte_for_byte() {
    let (_, old_tree) = document(&[("a", "9")]);
    let (_, new_tree) = document(&[("a", "9.0")]);
    let proves = |old: &str, new: &str, language: &str| number_respelling_equivalent(&old_tree, &new_tree, old, new, language);
    // JSON-lexable sources: layout may change around the respelling; nothing else may.
    assert!(proves("{\"a\":9}", "{\n  \"a\": 9.0\n}\n", "json"));
    assert!(proves("[1E22, \"x y\"]", "[ 1e+22 , \"x y\" ]", "adf"));
    assert!(!proves("[1E22, \"x y\"]", "[1e+22, \"xy\"]", "adf"), "string bytes are exact");
    assert!(!proves("[9]", "[9]\n", "json"), "a respelling is required");
    assert!(!proves("[9, 1]", "[9.0, 2]", "json"));
    assert!(!proves("[-0]", "[0]", "json"));
    assert!(!proves("[9]", "[9.0]", "yaml"), "out of scope");
    // Non-JSON sources (Databricks YAML): identical tree, identical bytes outside numbers.
    let yaml = |token: &str, comment: &str| {
        let source = format!("retry: {token}{comment}\n");
        let tree: SemanticNode = serde_json::from_value(json!({"id": "0", "node_type": "object", "label": "",
            "structural_hash": "r", "position": {"start_line": 0, "start_col": 0, "end_line": 1, "end_col": 0},
            "children": [{"id": "0.0", "node_type": "property", "label": "retry", "structural_hash": "p",
                "position": {"start_line": 0, "start_col": 0, "end_line": 0, "end_col": 5},
                "children": [{"id": "0.0.0", "node_type": "number", "label": token, "structural_hash": "n",
                    "position": {"start_line": 0, "start_col": 7, "end_line": 0, "end_col": 7 + token.len()},
                    "children": []}]}]})).unwrap();
        (source, tree)
    };
    let ((a, ta), (b, tb)) = (yaml("9", ""), yaml("9.0", ""));
    assert!(number_respelling_equivalent(&ta, &tb, &a, &b, "databricks-workflow"));
    let ((a, ta), (b, tb)) = (yaml("9", " # x"), yaml("9.0", " # y"));
    assert!(!number_respelling_equivalent(&ta, &tb, &a, &b, "databricks-workflow"), "a comment the tree omits");
    let ((a, ta), (b, tb)) = (yaml("+9", ""), yaml("9.0", ""));
    assert!(!number_respelling_equivalent(&ta, &tb, &a, &b, "databricks-workflow"), "YAML-only spelling");
}
