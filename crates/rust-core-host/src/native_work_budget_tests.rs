//! #255: native refinement / inline diff work is bounded independently of Wasm fuel, and
//! exhausting a budget degrades to coarser honest evidence, never to an empty success.
use crate::*;

#[test]
fn inline_diff_budget_degrades_to_one_honest_replacement() {
    let (old, new) = ("alpha bravo charlie", "alpha brave new charlie");
    let full = inline_char_diff(old, new);
    assert_ne!(full, inline_char_diff_with_budget(old, new, 0));
    assert_eq!(inline_char_diff_with_budget(old, new, 0), format!("[-{old}][+{new}]"));
    // A budget large enough for the whole recursion reproduces the unbounded placement.
    assert_eq!(inline_char_diff_with_budget(old, new, usize::MAX), full);
}

#[test]
fn long_single_line_replacements_stay_bounded_and_identical_to_the_rendered_cut() {
    // n = 4096 previously ran the full quadratic search for a result cut to 200 chars.
    for n in [512usize, 1024, 2048, 4096, 50_000] {
        let (old, new) = ("a".repeat(n), "b".repeat(n));
        let diff = inline_char_diff(&old, &new);
        assert!(diff.chars().count() <= 200, "{n}");
        assert!(diff.starts_with("[-a"), "{n}: never empty");
        // Early stop is output-preserving: a near-identical long line still renders its cut
        // exactly as the unbounded algorithm would.
        let (a, b) = (format!("{}x{}", "c".repeat(n / 2), "d".repeat(n / 2)), format!("{}y{}", "c".repeat(n / 2), "d".repeat(n / 2)));
        let bounded = inline_char_diff(&a, &b);
        if n <= 2048 {
            assert_eq!(bounded, inline_char_diff_with_budget(&a, &b, usize::MAX), "{n}");
        }
        assert!(bounded.ends_with("..."), "{n}");
        if (n + 1) * (n + 1) > INLINE_DIFF_CELL_BUDGET {
            assert!(bounded.starts_with("[-c"), "{n}: past the budget the span is one replacement");
        }
    }
    let review = crate::text_review_generic::generic_text_review_impl(&"a".repeat(4096), &"b".repeat(4096), 1);
    let review: Value = serde_json::from_str(&review).unwrap();
    assert_eq!(review["used"], true);
    assert_eq!(review["changes"].as_array().unwrap().len(), 1, "a budgeted inline diff never empties the review");
}

fn entity(id: &str, label: &str, token: &str) -> SemanticNode {
    let leaf = |id: String, node_type: &str, label: &str| SemanticNode {
        id, node_type: node_type.into(), label: label.into(),
        position: NodePosition { start_line: 0, start_col: 0, end_line: 0, end_col: 1 },
        structural_hash: format!("h:{node_type}:{label}"), children: vec![], parent_type: None, type_info: None, facts: None };
    let mut function = leaf(id.into(), "function_definition", label);
    function.children = vec![leaf(format!("{id}.0"), "identifier", label), leaf(format!("{id}.1"), "parameters", "parameters"),
        leaf(format!("{id}.2"), "string", token)];
    function
}

fn module(children: Vec<SemanticNode>) -> SemanticNode {
    SemanticNode { id: "0".into(), node_type: "module".into(), label: String::new(),
        position: NodePosition { start_line: 0, start_col: 0, end_line: 0, end_col: 1 },
        structural_hash: "m".into(), children, parent_type: None, type_info: None, facts: None }
}

#[test]
fn callable_rename_seeding_has_a_candidate_graph_budget() {
    // Within budget: a unique renamed callable is seeded.
    let (old, new) = (module(vec![entity("0.0", "total", "'body'")]), module(vec![entity("0.0", "calculate_total", "'body'")]));
    let (oi, ni) = (TreeIndex::new(&old), TreeIndex::new(&new));
    let mut pairs = Vec::new();
    callable_renames::seed_renamed_callables(&oi, &ni, &mut pairs, 0.5, None);
    assert_eq!(pairs.len(), 1, "control: the rename is recoverable within budget");

    // Past the budget (open old x open new > CALLABLE_RENAME_PAIR_BUDGET) nothing is inferred.
    let side = (callable_renames::CALLABLE_RENAME_PAIR_BUDGET as f64).sqrt() as usize + 1;
    let old = module((0..side).map(|i| entity(&format!("0.{i}"), &format!("f{i}"), &format!("'b{i}'"))).collect());
    let new = module((0..side).map(|i| entity(&format!("0.{i}"), &format!("g{i}"), &format!("'b{i}'"))).collect());
    let (oi, ni) = (TreeIndex::new(&old), TreeIndex::new(&new));
    let mut pairs = Vec::new();
    callable_renames::seed_renamed_callables(&oi, &ni, &mut pairs, 0.5, None);
    assert!(pairs.is_empty(), "dense candidate graph is not searched");
}
