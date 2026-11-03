//! Operator tokens are behaviour, not trivia (#229).
//!
//! tree-sitter keeps operators (`+`, `<`, `and`, `+=`, ...) as *anonymous* tokens. Both the
//! native Python serializer (`named_children`) and the Wasm parser components dropped them, so
//! `a + b` and `a - b` produced identical trees, identical structural hashes, and the
//! style-only shortcut reported a logic change as `IGNORED_STYLE`.
//!
//! This module is the one place that decides which node types carry an operator, which tokens
//! count, and how an operator enters the tree: as an `operator` leaf child whose label is the
//! token. Parent labels are never changed - they are identity for rename maps, profile labels
//! and scaffold suppression (intentumdiff-python#290). It has two feeds:
//!
//! * the native Python CST, where tree-sitter hands us the real anonymous tokens
//!   (`serialize_ts_node` fills `CstNode::operator`), and
//! * Wasm component trees, which arrive as finished `SemanticNode`s without the token. Every
//!   node carries a source span, so the operator is recovered from the source text between
//!   the node's children (`recover_operator_tokens`).
//!
//! The source recovery is the 0.0.2 stop-gap (option A). Core #230 tracks making the
//! components emit the tokens themselves (option B) and retiring the recovery.

use sha2::{Digest, Sha256};

/// Node type of the leaf that carries one operator token (`label` is the token: `+`, `and`).
pub(crate) const OPERATOR_NODE_TYPE: &str = "operator";

/// Node types whose anonymous tokens are operators that change behaviour.
///
/// Derived from what the shipped components actually emit for `a + b`, `a < b && !c`,
/// `x += 1` across javascript/typescript, go, rust, java, c/cpp, csharp, ruby, php, scala,
/// elixir, dart, zig, r and python. Declarations (`variable_declarator`, `let_declaration`,
/// ...) are deliberately excluded: their `=` never changes on its own and would only add noise.
pub(crate) fn is_operator_bearing(node_type: &str) -> bool {
    const PARTS: &[&str] = &[
        "binary",
        "unary",
        "infix",
        "prefix",
        "postfix",
        "operator",
        "assignment",
        "comparison",
        "boolean",
        "update_expression",
        "additive",
        "multiplicative",
        "equality",
        "relational",
        "logical",
        "bitwise",
        "shift",
        "_op_expression",
        "compound_assignment",
    ];
    let t = node_type.to_ascii_lowercase();
    // `operator_assignment` (ruby) and friends contain "assignment"; type-level syntax such as
    // zig's `error_union_type` contains none of these parts.
    PARTS.iter().any(|part| t.contains(part)) && !t.contains("declarat") && !t.contains("definition")
}

const WORD_OPERATORS: &[&str] = &[
    "and", "or", "not", "is", "in", "instanceof", "typeof", "xor", "mod", "div", "rem", "band",
    "bor", "bxor", "bnot", "bsl", "bsr", "andalso", "orelse", "notin", "isnot",
];

const SYMBOL_CHARS: &[char] = &['+', '-', '*', '/', '%', '<', '>', '=', '!', '&', '|', '^', '~', '?'];

/// Is this whole token an operator (used for tree-sitter anonymous tokens)?
pub(crate) fn is_operator_token(token: &str) -> bool {
    let token = token.trim();
    if token.is_empty() {
        return false;
    }
    if token.chars().all(|c| SYMBOL_CHARS.contains(&c)) {
        return true;
    }
    // `is not` / `not in` arrive as one anonymous token in some grammars.
    token
        .split_whitespace()
        .all(|word| WORD_OPERATORS.contains(&word))
}

#[derive(Clone, Copy)]
enum CommentStyle {
    /// `// ...` and `/* ... */`
    Slash,
    /// `# ...`
    Hash,
    /// `-- ...`
    DashDash,
}

fn comment_styles(language: &str) -> &'static [CommentStyle] {
    use CommentStyle::*;
    match language.to_ascii_lowercase().as_str() {
        "python" | "ruby" | "r" | "elixir" | "perl" | "bash" | "shell" | "sh" | "powershell"
        | "nim" | "julia" | "crystal" | "coffeescript" | "tcl" | "cmake" | "makefile"
        | "yaml" | "toml" | "dockerfile" => &[Hash],
        "sql" | "tsql" | "plsql" | "lua" | "haskell" | "elm" | "ada" | "purescript" => &[DashDash],
        "php" => &[Slash, Hash],
        _ => &[Slash],
    }
}

/// Replace every comment in *gap* with spaces of the same byte length (offsets preserved).
fn blank_comments(gap: &str, language: &str) -> String {
    let stripped_spans = comment_spans(gap, language);
    let mut out = String::with_capacity(gap.len());
    let mut i = 0;
    for (s, e) in stripped_spans {
        out.push_str(&gap[i..s]);
        out.extend(std::iter::repeat(' ').take(e - s));
        i = e;
    }
    out.push_str(&gap[i..]);
    out
}

fn comment_spans(gap: &str, language: &str) -> Vec<(usize, usize)> {
    let styles = comment_styles(language);
    let mut spans = Vec::new();
    let mut i = 0;
    while i < gap.len() {
        let rest = &gap[i..];
        let mut end = None;
        for style in styles {
            end = match style {
                CommentStyle::Slash if rest.starts_with("//") => Some(rest.find('\n').unwrap_or(rest.len())),
                CommentStyle::Slash if rest.starts_with("/*") => {
                    Some(rest.find("*/").map(|e| e + 2).unwrap_or(rest.len()))
                }
                CommentStyle::Hash if rest.starts_with('#') => Some(rest.find('\n').unwrap_or(rest.len())),
                CommentStyle::DashDash if rest.starts_with("--") => {
                    Some(rest.find('\n').unwrap_or(rest.len()))
                }
                _ => None,
            };
            if end.is_some() {
                break;
            }
        }
        match end {
            Some(len) => {
                spans.push((i, i + len));
                i += len.max(1);
            }
            None => i += rest.chars().next().map(char::len_utf8).unwrap_or(1),
        }
    }
    spans
}

/// Extract operator tokens from a gap of source text (whitespace, comments, punctuation and
/// identifiers are dropped; maximal symbol runs and operator words are kept).
#[cfg(test)]
pub(crate) fn operator_tokens_in_gap(gap: &str, language: &str, out: &mut Vec<String>) {
    let mut located = Vec::new();
    located_operator_tokens_in_gap(gap, language, &mut located);
    out.extend(located.into_iter().map(|(_, token)| token));
}

/// As `operator_tokens_in_gap`, with each token's byte offset within *gap*. Comments are
/// blanked (same length) rather than removed, so offsets stay true to the source.
fn located_operator_tokens_in_gap(gap: &str, language: &str, out: &mut Vec<(usize, String)>) {
    let cleaned = blank_comments(gap, language);
    let mut chars = cleaned.char_indices().peekable();
    while let Some((start, ch)) = chars.next() {
        if SYMBOL_CHARS.contains(&ch) {
            let mut end = start + ch.len_utf8();
            while let Some(&(idx, next)) = chars.peek() {
                if !SYMBOL_CHARS.contains(&next) {
                    break;
                }
                end = idx + next.len_utf8();
                chars.next();
            }
            out.push((start, cleaned[start..end].to_owned()));
        } else if ch.is_alphabetic() || ch == '_' {
            let mut end = start + ch.len_utf8();
            while let Some(&(idx, next)) = chars.peek() {
                if !(next.is_alphanumeric() || next == '_') {
                    break;
                }
                end = idx + next.len_utf8();
                chars.next();
            }
            let word = &cleaned[start..end];
            if WORD_OPERATORS.contains(&word) {
                out.push((start, word.to_owned()));
            }
        }
    }
}

/// The label a node carries once its operator is known. Deterministic and free of layout, so a
/// whitespace-only edit never changes it.
pub(crate) fn label_with_operator(label: &str, operator: &str) -> String {
    if operator.is_empty() {
        label.to_owned()
    } else if label.is_empty() {
        operator.to_owned()
    } else {
        format!("{label} {operator}")
    }
}

/// Mix an operator into an existing structural hash.
pub(crate) fn hash_with_operator(hash: &str, operator: &str, child_hashes: &[&str]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(hash.as_bytes());
    hasher.update(b"|op:");
    hasher.update(operator.as_bytes());
    for child in child_hashes {
        hasher.update(b"|");
        hasher.update(child.as_bytes());
    }
    hex::encode(hasher.finalize())
}

fn line_starts(source: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (i, byte) in source.bytes().enumerate() {
        if byte == b'\n' {
            starts.push(i + 1);
        }
    }
    starts
}

fn byte_offset(starts: &[usize], source: &str, line: u32, col: u32) -> Option<usize> {
    let offset = starts.get(line as usize)? + col as usize;
    (offset <= source.len() && source.is_char_boundary(offset)).then_some(offset)
}

/// Recover operator tokens from source for a component-built semantic tree (option A, #229).
///
/// Runs post-order. An operator-bearing node whose source gaps hold operator tokens gets them
/// appended to its label; its structural hash and every ancestor's hash are re-derived so that
/// two trees differing only by an operator no longer hash equal. Subtrees without an operator
/// keep their original hash, so unrelated matching and moves are unaffected. Idempotent: a node
/// whose label already ends with its recovered operator is left alone.
pub(crate) fn recover_operator_tokens(tree: &mut crate::SemanticNode, source: &str, language: &str) {
    let starts = line_starts(source);
    recover_node(tree, source, &starts, language);
}

fn recover_node(
    node: &mut crate::SemanticNode,
    source: &str,
    starts: &[usize],
    language: &str,
) -> bool {
    let mut changed = false;
    for child in &mut node.children {
        changed |= recover_node(child, source, starts, language);
    }
    // Idempotent: a node that already carries `operator` children (native Python CST, a
    // guest that emits them, or a second pass) is left alone.
    let has_operator_child = node.children.iter().any(|c| c.node_type == OPERATOR_NODE_TYPE);
    let mut added = false;
    if is_operator_bearing(&node.node_type) && !node.children.is_empty() && !has_operator_child {
        if let Some(leaves) = gap_operator_leaves(node, source, starts, language) {
            for leaf in leaves {
                node.children.push(leaf);
            }
            node.children.sort_by_key(|c| (c.position.start_line, c.position.start_col));
            added = true;
        }
    }
    if added || changed {
        let child_hashes: Vec<&str> = node.children.iter().map(|c| c.structural_hash.as_str()).collect();
        node.structural_hash = hash_with_operator(&node.structural_hash, "", &child_hashes);
        return true;
    }
    false
}

fn offset_to_line_col(starts: &[usize], offset: usize) -> (u32, u32) {
    let line = starts.partition_point(|&s| s <= offset).saturating_sub(1);
    (line as u32, (offset - starts[line]) as u32)
}

fn operator_leaf(parent_id: &str, index: usize, token: &str, start: (u32, u32), end: (u32, u32)) -> crate::SemanticNode {
    crate::SemanticNode {
        id: format!("{parent_id}.op{index}"),
        node_type: OPERATOR_NODE_TYPE.to_owned(),
        label: token.to_owned(),
        position: crate::NodePosition {
            start_line: start.0,
            start_col: start.1,
            end_line: end.0,
            end_col: end.1,
        },
        structural_hash: hash_with_operator("", token, &[]),
        children: Vec::new(),
        parent_type: None,
        type_info: None,
        facts: None,
    }
}

fn gap_operator_leaves(
    node: &crate::SemanticNode,
    source: &str,
    starts: &[usize],
    language: &str,
) -> Option<Vec<crate::SemanticNode>> {
    let pos = &node.position;
    let start = byte_offset(starts, source, pos.start_line, pos.start_col)?;
    let end = byte_offset(starts, source, pos.end_line, pos.end_col)?;
    if end < start {
        return None;
    }
    let mut spans: Vec<(usize, usize)> = Vec::with_capacity(node.children.len());
    for child in &node.children {
        let p = &child.position;
        let s = byte_offset(starts, source, p.start_line, p.start_col)?;
        let e = byte_offset(starts, source, p.end_line, p.end_col)?;
        if e < s {
            return None;
        }
        spans.push((s, e));
    }
    spans.sort_unstable();
    let mut tokens: Vec<(usize, String)> = Vec::new();
    let mut gap = |from: usize, to: usize, tokens: &mut Vec<(usize, String)>| {
        let mut found = Vec::new();
        located_operator_tokens_in_gap(&source[from..to], language, &mut found);
        tokens.extend(found.into_iter().map(|(offset, token)| (from + offset, token)));
    };
    let mut cursor = start;
    for (s, e) in spans {
        if s > cursor && s <= end {
            gap(cursor, s, &mut tokens);
        }
        cursor = cursor.max(e);
    }
    if cursor < end {
        gap(cursor, end, &mut tokens);
    }
    if tokens.is_empty() {
        return None;
    }
    Some(
        tokens
            .into_iter()
            .enumerate()
            .map(|(i, (offset, token))| {
                let start = offset_to_line_col(starts, offset);
                let end = offset_to_line_col(starts, offset + token.len());
                operator_leaf(&node.id, i, &token, start, end)
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gap_tokens_keep_operators_and_drop_layout_comments_and_punctuation() {
        let mut out = Vec::new();
        operator_tokens_in_gap("  +  ", "javascript", &mut out);
        operator_tokens_in_gap(" /* - */ ", "javascript", &mut out);
        operator_tokens_in_gap(" // x - y\n", "javascript", &mut out);
        operator_tokens_in_gap(" is not ", "python", &mut out);
        operator_tokens_in_gap(" # minus - \n", "python", &mut out);
        operator_tokens_in_gap(", ( ) ;", "c", &mut out);
        operator_tokens_in_gap(" >>>= ", "javascript", &mut out);
        assert_eq!(out, ["+", "is", "not", ">>>="]);
    }

    #[test]
    fn operator_bearing_types_match_shipped_grammars_but_not_declarations() {
        for t in [
            "binary_expression", "binary_operator", "binary", "unary_expression",
            "prefix_unary_expression", "augmented_assignment_expression", "operator_assignment",
            "compound_assignment_expr", "comparison_operator", "boolean_operator", "not_operator",
            "infix_expression", "additive_expression", "logical_and_expression",
            "unary_op_expression", "assignment_expression",
        ] {
            assert!(is_operator_bearing(t), "{t}");
        }
        for t in ["variable_declarator", "let_declaration", "function_definition", "identifier",
            "error_union_type", "call_expression", "return_statement"] {
            assert!(!is_operator_bearing(t), "{t}");
        }
    }

    #[test]
    fn anonymous_tokens_classify() {
        for t in ["+", "-=", "&&", "and", "not in", "is not", "<=>"] {
            assert!(is_operator_token(t), "{t}");
        }
        for t in ["(", ",", ":", "def", "return", "async", ""] {
            assert!(!is_operator_token(t), "{t}");
        }
    }
}
