//! Bridge from the engine's `CstNode` to the canonical AST (#9).
//!
//! # Why the core normalises, not the plugins
//!
//! The obvious design is for parser plugins to emit UAST directly. It is also the expensive
//! one: what a plugin emits is fixed by the WIT contract, so changing it means rebuilding
//! and re-certifying all 69 components and re-pinning every registry checksum. The
//! 2026-08-04 rebrand did that accidentally and produced 637 failures whose symptom named
//! the wrong layer entirely.
//!
//! Normalising here gets the same canonical tree with none of that blast radius: the plugin
//! ABI is untouched, and a component certified last month still works.
//!
//! # What this buys
//!
//! `derive_node_facts` and friends currently ask "is this node type one of these forty
//! strings?" in a dozen places. Those predicates ARE a canonical vocabulary — just an
//! implicit one, re-derived per question. Going through UAST makes it explicit and shared,
//! and gives roles (`Negated`, `EarlyExit`) that no amount of node-type matching can reach.

use intentumdiff_ast::{Span, SourceTree};

use crate::CstNode;

impl SourceTree for CstNode {
    fn node_type(&self) -> &str {
        &self.node_type
    }

    fn span(&self) -> Span {
        // CstNode carries LINE/COLUMN, not byte offsets, so the byte fields stay zero.
        // Populating them with line numbers would be worse than leaving them empty: a
        // consumer slicing source by `start_byte` would silently read the wrong region.
        Span {
            start_byte: 0,
            end_byte: 0,
            start_row: self.start_line,
            end_row: self.end_line,
        }
    }

    fn children(&self) -> Vec<&Self> {
        self.children.iter().collect()
    }

    fn token(&self) -> Option<&str> {
        // Empty text means "this node has no token of its own" (an interior node), which is
        // not the same as an empty-string token. Returning Some("") would make every
        // interior node look like it carried a value.
        if self.text.is_empty() {
            None
        } else {
            Some(self.text.as_str())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use intentumdiff_ast::{normalize, Category, Role};

    fn cst(node_type: &str, text: &str, children: Vec<CstNode>) -> CstNode {
        CstNode {
            node_type: node_type.to_owned(),
            named: true,
            text: text.to_owned(),
            start_line: 1,
            start_col: 0,
            end_line: 1,
            end_col: 1,
            children,
        }
    }
    fn leaf(node_type: &str) -> CstNode {
        cst(node_type, "", vec![])
    }

    #[test]
    fn a_cst_normalises_into_canonical_categories() {
        let tree = cst(
            "module",
            "",
            vec![cst(
                "function_definition",
                "",
                vec![cst("block", "", vec![leaf("return_statement")])],
            )],
        );
        let u = normalize(&tree, "python");
        assert_eq!(u.category, Category::File);
        let f = u.find(Category::FunctionDeclaration).expect("a function");
        assert!(f.find(Category::Return).is_some());
    }

    #[test]
    fn a_guard_clause_in_a_real_cst_gets_its_roles() {
        // The payoff: roles that node-type matching cannot reach, derived from the tree the
        // engine already has, with no plugin change.
        //
        //     def f(x):
        //         if not x: return None
        //         work(x)
        let tree = cst(
            "module",
            "",
            vec![cst(
                "function_definition",
                "",
                vec![cst(
                    "block",
                    "",
                    vec![
                        cst(
                            "if_statement",
                            "",
                            vec![
                                cst("not_operator", "not", vec![leaf("identifier")]),
                                cst("block", "", vec![leaf("return_statement")]),
                            ],
                        ),
                        leaf("call"),
                    ],
                )],
            )],
        );
        let u = normalize(&tree, "python");
        let cond = u.find(Category::Conditional).expect("a conditional");
        assert!(cond.has_role(Role::Negated), "roles={:?}", cond.roles);
        let ret = cond.find(Category::Return).expect("a return");
        assert!(ret.has_role(Role::EarlyExit), "roles={:?}", ret.roles);
    }

    #[test]
    fn interior_nodes_carry_no_token() {
        // Empty text is "no token", not an empty token — otherwise every interior node
        // would look like it held a value.
        let interior = cst("block", "", vec![]);
        assert_eq!(interior.token(), None);
        let literal = cst("string", "\"hello\"", vec![]);
        assert_eq!(literal.token(), Some("\"hello\""));
    }

    #[test]
    fn line_positions_survive_the_bridge() {
        let mut node = leaf("identifier");
        node.start_line = 42;
        node.end_line = 43;
        let s = node.span();
        assert_eq!(s.start_row, 42);
        assert_eq!(s.end_row, 43);
    }
}

// ============ UAST-derived structural facts (#9) ============
// The existing fact vocabulary answers "what does this function CONTAIN?" — has_loop,
// has_conditional, call_count. It cannot answer "how is it ARRANGED?", and arrangement is
// usually where the intent is:
//
//     if not x: return None      guard clause: reject and leave
//     if x: do_work()            wrapped body: same flags, different meaning
//
// Both are has_conditional + a call. Roles separate them, and roles come from UAST.
//
// Same privacy contract as every other fact: flags and counts, never a name or a value.

use intentumdiff_ast::{normalize, Category, Role, UastNode};
use serde_json::{json, Value};

/// Count nodes of a category that carry a role.
fn count_with_role(root: &UastNode, category: Category, role: Role) -> usize {
    std::iter::once(root)
        .chain(root.descendants())
        .filter(|n| n.category == category && n.has_role(role))
        .count()
}

/// True when a negated conditional contains an early return — the guard-clause shape.
///
/// Both halves are required. A negated condition alone is just a branch; an early return
/// alone is a plain short-circuit. It is the pair that means "reject this input and leave",
/// which is what an explainer wants to say.
fn has_guard_clause(root: &UastNode) -> bool {
    std::iter::once(root)
        .chain(root.descendants())
        .filter(|n| n.category == Category::Conditional && n.has_role(Role::Negated))
        .any(|cond| {
            std::iter::once(cond)
                .chain(cond.descendants())
                .any(|d| d.category == Category::Return && d.has_role(Role::EarlyExit))
        })
}

/// Structural facts a node-type predicate cannot reach, derived via the canonical AST.
///
/// Returns None when nothing structural was found, so a trivial function does not pay for
/// an all-false fact bag.
pub(crate) fn uast_structural_facts(node: &CstNode, language: &str) -> Option<Value> {
    let uast = normalize(node, language);

    let guard = has_guard_clause(&uast);
    let early_exits = count_with_role(&uast, Category::Return, Role::EarlyExit);
    let negated = count_with_role(&uast, Category::Conditional, Role::Negated);

    if !guard && early_exits == 0 && negated == 0 {
        return None;
    }
    let mut facts = serde_json::Map::new();
    if guard {
        facts.insert("has_guard_clause".to_owned(), json!(true));
    }
    if early_exits > 0 {
        facts.insert("early_exit_count".to_owned(), json!(early_exits));
    }
    if negated > 0 {
        facts.insert("negated_condition_count".to_owned(), json!(negated));
    }
    Some(Value::Object(facts))
}

#[cfg(test)]
mod structural_fact_tests {
    use super::*;

    fn cst2(node_type: &str, text: &str, children: Vec<CstNode>) -> CstNode {
        CstNode {
            node_type: node_type.to_owned(),
            named: true,
            text: text.to_owned(),
            start_line: 1,
            start_col: 0,
            end_line: 1,
            end_col: 1,
            children,
        }
    }
    fn l2(t: &str) -> CstNode {
        cst2(t, "", vec![])
    }

    /// `if not x: return None` followed by real work.
    fn guard_fn() -> CstNode {
        cst2("function_definition", "", vec![cst2("block", "", vec![
            cst2("if_statement", "", vec![
                cst2("not_operator", "not", vec![l2("identifier")]),
                cst2("block", "", vec![l2("return_statement")]),
            ]),
            l2("call"),
        ])])
    }

    /// `if x: do_work()` — same flags under the old vocabulary, different meaning.
    fn wrapped_fn() -> CstNode {
        cst2("function_definition", "", vec![cst2("block", "", vec![
            cst2("if_statement", "", vec![
                l2("identifier"),
                cst2("block", "", vec![l2("call")]),
            ]),
        ])])
    }

    #[test]
    fn a_guard_clause_is_recognised() {
        let f = uast_structural_facts(&guard_fn(), "python").expect("facts");
        assert_eq!(f["has_guard_clause"], true);
        assert_eq!(f["early_exit_count"], 1);
        assert_eq!(f["negated_condition_count"], 1);
    }

    #[test]
    fn a_wrapped_body_is_not_a_guard_clause() {
        // THE distinction the flat fact bag could not make. Both functions are
        // has_conditional + a call; only the arrangement differs.
        assert!(
            uast_structural_facts(&wrapped_fn(), "python").is_none(),
            "a wrapped body has no guard, no early exit and no negation"
        );
    }

    #[test]
    fn both_halves_are_required_for_a_guard() {
        // An early return WITHOUT negation is a short-circuit, not a guard: `if x: return`
        // then more code. It should report the early exit but claim no guard.
        let short_circuit = cst2("function_definition", "", vec![cst2("block", "", vec![
            cst2("if_statement", "", vec![
                l2("identifier"),
                cst2("block", "", vec![l2("return_statement")]),
            ]),
            l2("call"),
        ])]);
        let f = uast_structural_facts(&short_circuit, "python").expect("facts");
        assert_eq!(f["early_exit_count"], 1);
        assert!(f.get("has_guard_clause").is_none(), "no negation, so no guard");
    }

    #[test]
    fn facts_carry_no_names_or_values() {
        // Same privacy contract as every other fact: flags and counts only.
        let mut f = guard_fn();
        f.children[0].children[1].text = "charge_customer_card".to_owned();
        let rendered = uast_structural_facts(&f, "python").expect("facts").to_string();
        assert!(!rendered.contains("charge"), "leaked a token: {rendered}");
    }
}
