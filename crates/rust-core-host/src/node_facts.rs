//! NodeFacts derivation predicates and kind maps (issues #69/#70/#72), extracted
//! from lib.rs verbatim (issue #29 monolith split, phase B). Behavior taxonomy,
//! literal-kind mapping, and return-value helpers for the facts pass.

use crate::*;

// Cross-grammar control-flow vocabularies (issue #69-H behavior classification). Node-type only —
// no names or values — so the derived flags are privacy-safe.
pub(crate) fn is_conditional_node_type(node_type: &str) -> bool {
    matches!(
        node_type,
        "if_statement"
            | "if_expression"
            | "conditional_expression"
            | "ternary_expression"
            | "match_statement"
            | "match_expression"
            | "switch_statement"
            | "switch_expression"
            | "when_expression"
            | "case_statement"
            | "cond"
    )
}

pub(crate) fn is_loop_node_type(node_type: &str) -> bool {
    matches!(
        node_type,
        "for_statement"
            | "for_expression"
            | "for_in_statement"
            | "foreach_statement"
            | "while_statement"
            | "while_expression"
            | "do_statement"
            | "loop_expression"
            | "loop_statement"
            | "repeat_statement"
            | "range_clause"
    )
}

pub(crate) fn is_error_handling_node_type(node_type: &str) -> bool {
    matches!(
        node_type,
        "try_statement"
            | "try_expression"
            | "catch_clause"
            | "except_clause"
            | "rescue_clause"
            | "rescue"
            | "finally_clause"
    )
}

pub(crate) fn is_throw_node_type(node_type: &str) -> bool {
    matches!(
        node_type,
        "raise_statement" | "throw_statement" | "throw_expression" | "raise_expression"
    )
}

/// A node that performs actual computation — arithmetic/logical/comparison operators, a
/// comprehension/generator, a ternary, or an in-place arithmetic assignment. The direct antidote
/// to the #68 fabrication (`print()+return 99` became "performs some internal computation"): a
/// substantive body with NONE of these performs no computation, and the explainer can say so.
/// Node KIND only — never operands, operator text, or values (privacy-safe).
pub(crate) fn is_computation_node_type(node_type: &str) -> bool {
    matches!(
        node_type,
        // arithmetic / logical / comparison operators (python + curly-brace grammars)
        "binary_operator"
            | "boolean_operator"
            | "comparison_operator"
            | "unary_operator"
            | "binary_expression"
            | "unary_expression"
            | "logical_expression"
            | "update_expression"
            | "binary_expr"
            | "unary_expr"
            | "arithmetic_expression"
            | "relational_expression"
            // comprehensions / generator (python)
            | "list_comprehension"
            | "set_comprehension"
            | "dictionary_comprehension"
            | "generator_expression"
            // ternary / conditional expression
            | "conditional_expression"
            | "ternary_expression"
            // an augmented assignment is an in-place arithmetic op (`x += 1`)
            | "augmented_assignment"
            | "augmented_assignment_expression"
            | "compound_assignment_expr"
    )
}

/// The rightmost identifier of a Python base-class expression — `Enum` from `enum.Enum`,
/// `ABC` from `ABC`, the value part of `Generic[T]`. Used ONLY to classify the class kind into
/// a boolean (is_enum/is_exception); the name itself is never emitted (privacy-safe).
pub(crate) fn cst_base_name(node: &CstNode) -> Option<&str> {
    match node.node_type.as_str() {
        "identifier" => Some(node.text.as_str()),
        "attribute" => node
            .children
            .iter()
            .rev()
            .find(|c| c.node_type == "identifier")
            .map(|c| c.text.as_str()),
        "subscript" => node.children.first().and_then(cst_base_name),
        _ => None,
    }
}

/// A base class that makes the subclass an enumeration.
pub(crate) fn is_enum_base_name(name: &str) -> bool {
    matches!(
        name,
        "Enum" | "IntEnum" | "StrEnum" | "Flag" | "IntFlag" | "ReprEnum"
    )
}

/// A base class that makes the subclass an exception — the builtin roots or any name that reads
/// as an error/warning (`class ParseError(ValueError)` -> `ValueError` ends with "Error").
pub(crate) fn is_exception_base_name(name: &str) -> bool {
    name == "Exception"
        || name == "BaseException"
        || name.ends_with("Error")
        || name.ends_with("Exception")
        || name.ends_with("Warning")
}

/// A call/invocation node across grammars — for the fan-out (`call_count`) and `recursive`
/// coupling facts (#69-J). Node kind only; the callee name is read solely to detect self-calls.
pub(crate) fn is_call_node_type(node_type: &str) -> bool {
    matches!(
        node_type,
        "call"
            | "call_expression"
            | "function_call"
            | "function_call_expression"
            | "method_invocation"
            | "method_call_expression"
            | "invocation_expression"
            | "call_expr"
            | "macro_invocation"
    )
}

/// A returned value that is a freshly-built object or collection (factory signal).
pub(crate) fn is_construction_node_type(node_type: &str) -> bool {
    matches!(
        node_type,
        "list"
            | "dictionary"
            | "set"
            | "tuple"
            | "list_literal"
            | "array"
            | "array_expression"
            | "object"
            | "object_expression"
            | "new_expression"
            | "object_creation_expression"
            | "struct_expression"
            | "composite_literal"
            | "record_expression"
    )
}

/// A coarse "what kind of function is this" rollup (#69-H) from the privacy-safe behavior flags,
/// so the explainer can lead with purpose. Conservative — returns None rather than guess.
pub(crate) fn behavior_category(
    returns_value: bool,
    side_effects: bool,
    has_conditional: bool,
    has_loop: bool,
    throws: bool,
    mutates: bool,
    constructs: bool,
) -> Option<&'static str> {
    if throws && has_conditional {
        Some("validator") // guards inputs and raises
    } else if constructs && returns_value {
        Some("factory") // builds and returns a new object/collection
    } else if mutates && !returns_value {
        Some("mutator") // changes state, returns nothing
    } else if side_effects && !returns_value {
        Some("io") // does output/effects, returns nothing
    } else if returns_value && !side_effects && !mutates && !has_loop && !has_conditional && !throws
    {
        Some("accessor") // a pure read / getter
    } else if returns_value && (has_loop || has_conditional) {
        Some("transformer") // computes a result from a branch/loop
    } else {
        None
    }
}

/// A statement that mutates state: an augmented assignment (`x += 1`) or an assignment whose
/// target is an attribute/subscript (`self.x = …`, `a[i] = …`). Target node_type only — no name.
pub(crate) fn is_mutating_statement(node: &SemanticNode) -> bool {
    let nt = node.node_type.as_str();
    if matches!(
        nt,
        "augmented_assignment"
            | "augmented_assignment_expression"
            | "compound_assignment_expr"
    ) {
        return true;
    }
    if matches!(nt, "assignment" | "assignment_expression" | "assignment_statement") {
        if let Some(target) = node.children.first() {
            return matches!(
                target.node_type.as_str(),
                "attribute"
                    | "member_expression"
                    | "field_expression"
                    | "selector_expression"
                    | "subscript"
                    | "subscript_expression"
                    | "index_expression"
            );
        }
    }
    false
}

/// Cross-grammar scalar-literal KIND (never the value). `number` is JS/TS's int-or-float node.
pub(crate) fn semantic_literal_kind(node_type: &str) -> Option<&'static str> {
    match node_type {
        "integer" | "int_literal" | "integer_literal" | "decimal_integer_literal"
        | "hex_integer_literal" | "octal_integer_literal" | "binary_integer_literal" => Some("int"),
        "float" | "float_literal" | "floating_point_literal"
        | "decimal_floating_point_literal" | "hex_floating_point_literal" => Some("float"),
        "number" | "number_literal" => Some("number"),
        "string" | "string_literal" | "interpreted_string_literal" | "raw_string_literal"
        | "concatenated_string" | "template_string" | "char_literal" | "rune_literal"
        | "character_literal" => Some("str"),
        "true" | "false" | "boolean_literal" => Some("bool"),
        "none" | "null" | "nil" | "null_literal" | "undefined" => Some("none"),
        _ => None,
    }
}

/// The single returned value node, unwrapping a solitary list wrapper (Go `expression_list`).
pub(crate) fn single_return_value(ret: &SemanticNode) -> Option<&SemanticNode> {
    if ret.children.len() != 1 {
        return None;
    }
    let child = &ret.children[0];
    if matches!(child.node_type.as_str(), "expression_list" | "argument_list") {
        return if child.children.len() == 1 {
            Some(&child.children[0])
        } else {
            None
        };
    }
    Some(child)
}

pub(crate) fn semantic_return_literal_kind(ret: &SemanticNode) -> Option<&'static str> {
    semantic_literal_kind(single_return_value(ret)?.node_type.as_str())
}

/// A bare call statement (`foo()` / `console.log(...)` / `println!(...)`) — a side effect.
/// `x = foo()` and `return foo()` nest the call, so this matches only free-standing calls.
pub(crate) fn is_bare_call_statement(node: &SemanticNode) -> bool {
    matches!(node.node_type.as_str(), "expression_statement" | "call_statement")
        && node.children.iter().any(|c| {
            matches!(
                c.node_type.as_str(),
                "call"
                    | "call_expression"
                    | "method_invocation"
                    | "method_call"
                    | "invocation_expression"
                    | "macro_invocation"
                    | "function_call_expression"
            )
        })
}

// ============ Fact deltas (issue #178) ============
// A change's INTENT lives in how the facts moved, not in their final state: `has_loop: true`
// on the new node says nothing if the old node also looped. This diffs two NodeFacts bags
// and reports what moved.
//
// Deliberately STRUCTURED, never English. Wording is presentation and differs per surface —
// CodeLens is terse, release notes are prose, and a future locale needs its own strings — so
// the binding renders, the engine only finds. This lived in the VS Code extension
// (intentExplain.ts::computeChangeDelta) until #178; every other binding got nothing, which
// is exactly the "thin skins do zero functional work" rule in docs/TARGET_ARCHITECTURE.md.

fn fact_str<'a>(facts: &'a Value, key: &str) -> Option<&'a str> {
    facts.get(key).and_then(Value::as_str)
}

fn fact_bool(facts: &Value, key: &str) -> bool {
    facts.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn fact_u64(facts: &Value, key: &str) -> Option<u64> {
    facts.get(key).and_then(Value::as_u64)
}

/// Diff two NodeFacts objects into structured deltas. Empty when nothing moved.
pub(crate) fn compute_fact_delta(before: &Value, after: &Value) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();

    // Arity. Only when BOTH sides carry a count: a missing count means "not derived", not
    // zero, and reporting "removes 2 parameters" because the new parser pruned the list
    // would be a confident lie.
    if let (Some(from), Some(to)) = (fact_u64(before, "param_count"), fact_u64(after, "param_count"))
    {
        if from != to {
            out.push(json!({ "kind": "param_count", "from": from, "to": to }));
        }
    }

    // Asynchrony is only reported when GAINED. Losing `async` is nearly always a revert or a
    // move rather than an intent, and the old renderer never surfaced it.
    if !fact_bool(before, "is_async") && fact_bool(after, "is_async") {
        out.push(json!({ "kind": "became_async" }));
    }

    let (rb, ra) = (fact_str(before, "returns"), fact_str(after, "returns"));
    let (kb, ka) = (fact_str(before, "return_kind"), fact_str(after, "return_kind"));
    if rb != ra || kb != ka {
        let transition = match (rb, ra) {
            (Some("none"), Some(a)) if a != "none" => "gained_value",
            (Some(b), Some("none")) if b != "none" => "lost_value",
            _ => "changed",
        };
        let mut d = serde_json::Map::new();
        d.insert("kind".to_owned(), json!("returns"));
        d.insert("transition".to_owned(), json!(transition));
        if let Some(b) = rb {
            d.insert("from".to_owned(), json!(b));
        }
        if let Some(a) = ra {
            d.insert("to".to_owned(), json!(a));
        }
        out.push(Value::Object(d));
    }

    // Control shape. from/to are always carried, but `transition` is set only for the three
    // shifts that have an agreed meaning. Something like looping -> branching is a genuine
    // change with no honest one-liner, so it is reported as data and left unphrased rather
    // than forced into a misleading sentence.
    let (cb, ca) = (fact_str(before, "control_shape"), fact_str(after, "control_shape"));
    if cb != ca {
        let transition = match (cb, ca) {
            (b, Some("looping")) if b != Some("looping") => Some("added_loop"),
            (Some("linear"), Some("branching")) => Some("added_branch"),
            (b, Some("linear")) if b != Some("linear") => Some("removed_control_flow"),
            _ => None,
        };
        let mut d = serde_json::Map::new();
        d.insert("kind".to_owned(), json!("control_shape"));
        if let Some(t) = transition {
            d.insert("transition".to_owned(), json!(t));
        }
        if let Some(b) = cb {
            d.insert("from".to_owned(), json!(b));
        }
        if let Some(a) = ca {
            d.insert("to".to_owned(), json!(a));
        }
        out.push(Value::Object(d));
    }

    let (eb, ea) = (
        fact_bool(before, "has_error_handling"),
        fact_bool(after, "has_error_handling"),
    );
    if eb != ea {
        out.push(json!({ "kind": "error_handling", "added": ea }));
    }

    // Side effects, gained only — matching asynchrony above.
    if !fact_bool(before, "side_effects") && fact_bool(after, "side_effects") {
        out.push(json!({ "kind": "side_effects", "added": true }));
    }

    out
}

#[cfg(test)]
mod fact_delta_tests {
    use super::*;

    fn kinds(deltas: &[Value]) -> Vec<&str> {
        deltas.iter().filter_map(|d| d["kind"].as_str()).collect()
    }

    fn find<'a>(deltas: &'a [Value], kind: &str) -> &'a Value {
        deltas
            .iter()
            .find(|d| d["kind"] == kind)
            .unwrap_or_else(|| panic!("no delta of kind {kind} in {deltas:?}"))
    }

    #[test]
    fn identical_facts_yield_no_delta() {
        let f = json!({ "param_count": 2, "is_async": true, "control_shape": "looping" });
        assert!(compute_fact_delta(&f, &f).is_empty());
    }

    #[test]
    fn param_count_reports_both_endpoints() {
        let d = compute_fact_delta(&json!({ "param_count": 2 }), &json!({ "param_count": 4 }));
        let p = find(&d, "param_count");
        assert_eq!(p["from"], 2);
        assert_eq!(p["to"], 4);
    }

    #[test]
    fn param_count_needs_both_sides() {
        // A missing count means "not derived", not zero. Reporting a removal because the
        // parser pruned the parameter list would be a confident lie.
        assert!(compute_fact_delta(&json!({}), &json!({ "param_count": 3 })).is_empty());
        assert!(compute_fact_delta(&json!({ "param_count": 3 }), &json!({})).is_empty());
    }

    #[test]
    fn async_is_reported_only_when_gained() {
        let gained = compute_fact_delta(&json!({}), &json!({ "is_async": true }));
        assert_eq!(kinds(&gained), vec!["became_async"]);
        let lost = compute_fact_delta(&json!({ "is_async": true }), &json!({}));
        assert!(lost.is_empty(), "losing async is a revert, not an intent");
    }

    #[test]
    fn return_transitions_are_classified() {
        let gained = compute_fact_delta(
            &json!({ "returns": "none" }),
            &json!({ "returns": "value" }),
        );
        assert_eq!(find(&gained, "returns")["transition"], "gained_value");

        let lost = compute_fact_delta(
            &json!({ "returns": "value" }),
            &json!({ "returns": "none" }),
        );
        assert_eq!(find(&lost, "returns")["transition"], "lost_value");

        // Same `returns`, different literal kind: still a change, but neither gain nor loss.
        let changed = compute_fact_delta(
            &json!({ "returns": "value", "return_kind": "int" }),
            &json!({ "returns": "value", "return_kind": "string" }),
        );
        assert_eq!(find(&changed, "returns")["transition"], "changed");
    }

    #[test]
    fn control_shape_transitions_are_named() {
        let loop_added = compute_fact_delta(
            &json!({ "control_shape": "linear" }),
            &json!({ "control_shape": "looping" }),
        );
        assert_eq!(find(&loop_added, "control_shape")["transition"], "added_loop");

        let branch_added = compute_fact_delta(
            &json!({ "control_shape": "linear" }),
            &json!({ "control_shape": "branching" }),
        );
        assert_eq!(find(&branch_added, "control_shape")["transition"], "added_branch");

        let flattened = compute_fact_delta(
            &json!({ "control_shape": "branching" }),
            &json!({ "control_shape": "linear" }),
        );
        assert_eq!(
            find(&flattened, "control_shape")["transition"],
            "removed_control_flow"
        );
    }

    #[test]
    fn unnameable_control_shift_is_data_without_a_phrase() {
        // looping -> branching is a real change with no honest one-liner. It must still be
        // reported (from/to present) but carry no `transition`, so a renderer stays silent
        // rather than inventing a misleading sentence.
        let d = compute_fact_delta(
            &json!({ "control_shape": "looping" }),
            &json!({ "control_shape": "branching" }),
        );
        let c = find(&d, "control_shape");
        assert_eq!(c["from"], "looping");
        assert_eq!(c["to"], "branching");
        assert!(c.get("transition").is_none(), "must not be phrased");
    }

    #[test]
    fn error_handling_reports_both_directions() {
        let added = compute_fact_delta(&json!({}), &json!({ "has_error_handling": true }));
        assert_eq!(find(&added, "error_handling")["added"], true);
        let removed = compute_fact_delta(&json!({ "has_error_handling": true }), &json!({}));
        assert_eq!(find(&removed, "error_handling")["added"], false);
    }

    #[test]
    fn side_effects_reported_only_when_gained() {
        let gained = compute_fact_delta(&json!({}), &json!({ "side_effects": true }));
        assert_eq!(kinds(&gained), vec!["side_effects"]);
        assert!(compute_fact_delta(&json!({ "side_effects": true }), &json!({})).is_empty());
    }

    #[test]
    fn independent_changes_all_surface() {
        let d = compute_fact_delta(
            &json!({ "param_count": 1, "control_shape": "linear" }),
            &json!({ "param_count": 2, "control_shape": "looping",
                     "is_async": true, "has_error_handling": true }),
        );
        let k = kinds(&d);
        for expected in ["param_count", "became_async", "control_shape", "error_handling"] {
            assert!(k.contains(&expected), "missing {expected} in {k:?}");
        }
    }
}

// ============ Non-function fact families (issue #179) ============
// derive_node_facts covered only functions and classes, so a changed YAML key, Terraform
// block, TOML table or INI setting produced NO facts at all — and the intent explainer fell
// back to the change type and the label alone. Across 69 parsers that is most of a real
// review: config, IaC and data files.
//
// Same privacy contract as the function facts: node TYPES and COUNTS only, never a key
// name, never a value. These feed the cloud LLM path at the `facts` share level, so a fact
// carrying a config key would break the BYOK invariant in plugins/vscode/PRIVACY.md.

/// A key/value pair in a mapping — YAML, JSON, TOML, INI.
pub(crate) fn is_keyed_pair_node_type(node_type: &str) -> bool {
    matches!(
        node_type,
        "block_mapping_pair" | "flow_pair" | "pair" | "setting" | "table" | "inline_table"
    )
}

/// A container that holds keyed children.
pub(crate) fn is_mapping_node_type(node_type: &str) -> bool {
    matches!(
        node_type,
        "block_mapping" | "flow_mapping" | "object" | "table" | "inline_table" | "section"
    )
}

/// An ordered collection.
pub(crate) fn is_sequence_node_type(node_type: &str) -> bool {
    matches!(
        node_type,
        "block_sequence" | "flow_sequence" | "array" | "block_sequence_item" | "flow_sequence_item"
    )
}

/// A terminal value. Kind only — the VALUE is never read.
pub(crate) fn is_scalar_node_type(node_type: &str) -> bool {
    matches!(
        node_type,
        "boolean_scalar"
            | "float_scalar"
            | "integer_scalar"
            | "string_scalar"
            | "block_scalar"
            | "double_quote_scalar"
            | "single_quote_scalar"
            | "plain_scalar"
            | "null_scalar"
            | "number"
            | "string"
            | "boolean"
            | "integer"
            | "float"
            | "true"
            | "false"
            | "null"
            | "bool_lit"
            | "string_lit"
            | "numeric_lit"
            | "setting_value"
    )
}

/// A declarative resource block — Terraform/HCL, and the block-shaped IaC grammars.
pub(crate) fn is_resource_block_node_type(node_type: &str) -> bool {
    matches!(node_type, "block" | "resource" | "body" | "config_file")
}

/// Classify what a keyed node's VALUE is, without reading it.
fn keyed_value_kind(node: &SemanticNode) -> Option<&'static str> {
    for child in &node.children {
        let t = child.node_type.as_str();
        if is_mapping_node_type(t) {
            return Some("map");
        }
        if is_sequence_node_type(t) {
            return Some("list");
        }
        if is_scalar_node_type(t) {
            return Some("scalar");
        }
    }
    None
}

/// Facts for keyed structures (YAML/JSON/TOML/INI). Shape only.
pub(crate) fn derive_keyed_facts(node: &SemanticNode) -> Option<Value> {
    let node_type = node.node_type.as_str();
    let is_pair = is_keyed_pair_node_type(node_type);
    let is_container = is_mapping_node_type(node_type) || is_sequence_node_type(node_type);
    if !is_pair && !is_container {
        return None;
    }

    let mut facts = serde_json::Map::new();
    facts.insert("shape".to_owned(), json!(if is_pair { "pair" } else { "container" }));
    facts.insert("child_count".to_owned(), json!(node.children.len()));

    if let Some(kind) = keyed_value_kind(node) {
        facts.insert("value_kind".to_owned(), json!(kind));
    }
    if is_sequence_node_type(node_type) {
        facts.insert("value_kind".to_owned(), json!("list"));
        facts.insert("item_count".to_owned(), json!(node.children.len()));
    }

    // Nesting depth of the subtree. A key whose value grew from a scalar to a nested map is
    // a structural change worth surfacing, and depth is how that shows up.
    let depth = node
        .descendants()
        .iter()
        .filter(|d| is_mapping_node_type(d.node_type.as_str()) || is_sequence_node_type(d.node_type.as_str()))
        .count();
    facts.insert("nesting".to_owned(), json!(depth));
    facts.insert("is_leaf".to_owned(), json!(depth == 0));

    Some(Value::Object(facts))
}

/// Facts for declarative resource blocks (Terraform/HCL and friends).
pub(crate) fn derive_resource_facts(node: &SemanticNode) -> Option<Value> {
    if !is_resource_block_node_type(node.node_type.as_str()) {
        return None;
    }
    let mut attribute_count = 0usize;
    let mut nested_blocks = 0usize;
    for child in &node.children {
        let t = child.node_type.as_str();
        if t == "attribute" {
            attribute_count += 1;
        } else if is_resource_block_node_type(t) {
            nested_blocks += 1;
        } else if let Some(inner) = child.children.first() {
            // HCL wraps a block's contents in `body`; count through one level so an
            // attribute is not invisible just because the grammar nests it.
            if inner.node_type == "attribute" {
                attribute_count += child
                    .children
                    .iter()
                    .filter(|c| c.node_type == "attribute")
                    .count();
            }
        }
    }
    let mut facts = serde_json::Map::new();
    facts.insert("attribute_count".to_owned(), json!(attribute_count));
    facts.insert("nested_block_count".to_owned(), json!(nested_blocks));
    facts.insert("has_nested_block".to_owned(), json!(nested_blocks > 0));
    Some(Value::Object(facts))
}

#[cfg(test)]
mod non_function_facts_tests {
    use super::*;

    // SemanticNode has no Default; build it explicitly so the test compiles against the
    // real struct rather than a convenience that does not exist.
    fn node(node_type: &str, children: Vec<SemanticNode>) -> SemanticNode {
        SemanticNode {
            id: String::new(),
            node_type: node_type.to_owned(),
            label: String::new(),
            position: NodePosition { start_line: 1, start_col: 0, end_line: 1, end_col: 1 },
            structural_hash: String::new(),
            children,
            parent_type: None,
            type_info: None,
            facts: None,
        }
    }

    fn leaf(node_type: &str) -> SemanticNode {
        node(node_type, vec![])
    }

    #[test]
    fn yaml_pair_with_a_scalar_value_is_a_leaf() {
        let pair = node("block_mapping_pair", vec![leaf("plain_scalar")]);
        let f = derive_keyed_facts(&pair).expect("keyed facts");
        assert_eq!(f["shape"], "pair");
        assert_eq!(f["value_kind"], "scalar");
        assert_eq!(f["is_leaf"], true);
        assert_eq!(f["nesting"], 0);
    }

    #[test]
    fn yaml_pair_whose_value_is_a_map_reports_nesting() {
        // key: { a: 1 } — the structural change that matters is scalar -> nested map.
        let inner = node("block_mapping", vec![node("block_mapping_pair", vec![leaf("integer")])]);
        let pair = node("block_mapping_pair", vec![inner]);
        let f = derive_keyed_facts(&pair).expect("keyed facts");
        assert_eq!(f["value_kind"], "map");
        assert_eq!(f["is_leaf"], false);
        assert!(f["nesting"].as_u64().unwrap() >= 1);
    }

    #[test]
    fn sequences_report_item_count() {
        let seq = node("block_sequence", vec![leaf("plain_scalar"), leaf("plain_scalar")]);
        let f = derive_keyed_facts(&seq).expect("keyed facts");
        assert_eq!(f["shape"], "container");
        assert_eq!(f["value_kind"], "list");
        assert_eq!(f["item_count"], 2);
    }

    #[test]
    fn json_and_toml_share_the_keyed_vocabulary() {
        // The families are keyed on GRAMMAR SHAPE, not language, so one implementation
        // serves every mapping-shaped format rather than needing a per-language branch.
        let json_obj = node("object", vec![leaf("string"), leaf("number")]);
        assert!(derive_keyed_facts(&json_obj).is_some(), "json object");
        let toml_table = node("inline_table", vec![leaf("integer")]);
        assert!(derive_keyed_facts(&toml_table).is_some(), "toml table");
        let ini_setting = node("setting", vec![leaf("setting_value")]);
        assert!(derive_keyed_facts(&ini_setting).is_some(), "ini setting");
    }

    #[test]
    fn terraform_block_counts_attributes_and_nesting() {
        let nested = node("block", vec![leaf("attribute")]);
        let block = node("block", vec![leaf("attribute"), leaf("attribute"), nested]);
        let f = derive_resource_facts(&block).expect("resource facts");
        assert_eq!(f["attribute_count"], 2);
        assert_eq!(f["nested_block_count"], 1);
        assert_eq!(f["has_nested_block"], true);
    }

    #[test]
    fn a_flat_resource_block_reports_no_nesting() {
        let block = node("block", vec![leaf("attribute")]);
        let f = derive_resource_facts(&block).expect("resource facts");
        assert_eq!(f["has_nested_block"], false);
        assert_eq!(f["nested_block_count"], 0);
    }

    #[test]
    fn unrelated_node_types_still_get_nothing() {
        // The families must not fire on ordinary code, or a function body would pick up
        // container facts and the function facts would be shadowed.
        assert!(derive_keyed_facts(&leaf("identifier")).is_none());
        assert!(derive_resource_facts(&leaf("identifier")).is_none());
    }

    #[test]
    fn facts_never_carry_names_or_values() {
        // The load-bearing privacy claim: these feed the cloud LLM path at the `facts`
        // share level, so a key name or config value here would break the BYOK invariant.
        let mut pair = node("block_mapping_pair", vec![leaf("plain_scalar")]);
        pair.label = "aws_secret_access_key".to_owned();
        let rendered = derive_keyed_facts(&pair).expect("keyed facts").to_string();
        assert!(
            !rendered.contains("aws_secret"),
            "fact payload leaked the node label: {rendered}"
        );
    }
}
