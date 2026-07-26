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
