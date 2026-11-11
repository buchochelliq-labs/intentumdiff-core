//! CommonMark code blocks are compared by what they render, not by their source form (#274).
//!
//! The Markdown component emits an indented code block as `indented_code_block` labelled with
//! its raw source text (later lines keep their indentation), and a fenced block as
//! `fenced_code_block` labelled `code` / `code:<info>` whose `code_fence_content` child carries
//! no content at all. So an equivalent form change (indented <-> fenced, ``` <-> ~~~, a wider
//! fence) was reported as a modification, while a real edit to fenced code was invisible and
//! certified as style-only.
//!
//! Core re-derives each block from its source span (CommonMark 0.31.2 §4.4, §4.5) and replaces
//! it with one canonical node:
//!
//! * `code_block`, labelled `code` or `code:<info string>` (info string trimmed, backslash
//!   escapes resolved; an indented block has none), so a language change stays an update;
//! * one `code_content` leaf whose label is the rendered code text (without the final line
//!   break), omitted for an empty block. Indented blocks lose up to four columns of indentation
//!   per line and their trailing blank lines; fenced blocks lose up to the opening fence's
//!   indentation per line. Every other byte of the code, whitespace included, is content.
//!
//! The node's structural hash is a pure function of that canonical form, so equivalent blocks
//! hash identically. Container prefixes (list indentation, block quote markers) are taken from
//! the parser's `block_continuation` nodes; a block whose lines cannot be resolved exactly is
//! left untouched (conservative: the parser's tree stands).

use crate::{NodePosition, SemanticNode};
use std::collections::HashMap;

const CODE_BLOCK: &str = "code_block";
const CODE_CONTENT: &str = "code_content";

pub(crate) fn normalize_code_blocks(tree: &mut SemanticNode, source: &str, language: &str) {
    if language != "markdown" {
        return;
    }
    let lines: Vec<&str> = source
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect();
    normalize_node(tree, &lines);
}

fn normalize_node(node: &mut SemanticNode, lines: &[&str]) -> bool {
    if matches!(node.node_type.as_str(), "indented_code_block" | "fenced_code_block") {
        return normalize_block(node, lines);
    }
    let mut changed = false;
    for child in &mut node.children {
        changed |= normalize_node(child, lines);
    }
    if changed {
        node.structural_hash = crate::enriched_structural_hash(node, &node.label, &node.children);
    }
    changed
}

/// Visual column of byte `end` in `line` (tabs advance to the next multiple of four).
fn visual_col(line: &str, end: usize) -> usize {
    line[..end].chars().fold(0, |col, ch| if ch == '\t' { col + 4 - col % 4 } else { col + 1 })
}

fn is_blank(text: &str) -> bool {
    text.chars().all(|c| c == ' ' || c == '\t')
}

/// Remove up to `n` columns of leading whitespace from `text`, which starts at visual column
/// `col`. A tab only partly consumed leaves its remaining columns as spaces (CommonMark §2.2).
/// Returns the remainder and the number of columns removed.
fn strip_columns(text: &str, col: usize, n: usize, tabs: bool) -> (String, usize) {
    let mut removed = 0;
    for (index, ch) in text.char_indices() {
        if removed >= n {
            return (text[index..].to_string(), removed);
        }
        match ch {
            ' ' => removed += 1,
            '\t' if tabs => {
                let width = 4 - (col + removed) % 4;
                if removed + width > n {
                    let rest = " ".repeat(removed + width - n);
                    return (rest + &text[index + 1..], n);
                }
                removed += width;
            }
            _ => return (text[index..].to_string(), removed),
        }
    }
    (String::new(), removed)
}

/// One source line of the block after its container prefix: (text, visual column, row).
struct BlockLine<'a> {
    text: &'a str,
    col: usize,
    row: usize,
    prefix_end: usize,
}

fn continuation_ends(node: &SemanticNode, ends: &mut HashMap<usize, usize>) {
    for child in &node.children {
        if child.node_type == "block_continuation" && child.position.start_line == child.position.end_line {
            let entry = ends.entry(child.position.start_line as usize).or_insert(0);
            *entry = (*entry).max(child.position.end_col as usize);
        }
        continuation_ends(child, ends);
    }
}

/// Resolve each line of the block to its content after the container prefix, or `None` when
/// a line cannot be attributed exactly.
fn block_lines<'a>(node: &SemanticNode, lines: &[&'a str]) -> Option<Vec<BlockLine<'a>>> {
    let pos = &node.position;
    let first = pos.start_line as usize;
    let last = if pos.end_col == 0 && pos.end_line > pos.start_line {
        pos.end_line as usize - 1
    } else {
        pos.end_line as usize
    };
    if last >= lines.len() || last < first {
        return None;
    }
    let mut ends = HashMap::new();
    continuation_ends(node, &mut ends);
    let start = pos.start_col as usize;
    let head = lines[first];
    if start > head.len() || !head.is_char_boundary(start) {
        return None;
    }
    let content_col = visual_col(head, start);
    let mut out = Vec::new();
    for row in first..=last {
        let mut raw = lines[row];
        if row == last && pos.end_col > 0 {
            let end = (pos.end_col as usize).min(raw.len());
            if !raw.is_char_boundary(end) {
                return None;
            }
            raw = &raw[..end];
        }
        let mut prefix_end = if row == first {
            start
        } else if let Some(end) = ends.get(&row) {
            *end
        } else if content_col == 0 {
            0
        } else if is_blank(raw) {
            raw.len()
        } else {
            return None;
        };
        if prefix_end > raw.len() || !raw.is_char_boundary(prefix_end) {
            return None;
        }
        if row != first {
            let col = visual_col(raw, prefix_end);
            if col > content_col {
                // The continuation swallowed indentation beyond the container: back off over
                // spaces to the container's content column.
                let back = col - content_col;
                let candidate = prefix_end.checked_sub(back)?;
                if !raw.is_char_boundary(candidate)
                    || !raw[candidate..prefix_end].bytes().all(|b| b == b' ')
                    || visual_col(raw, candidate) != content_col
                {
                    return None;
                }
                prefix_end = candidate;
            } else if col < content_col && !is_blank(&raw[prefix_end..]) {
                return None;
            }
        }
        out.push(BlockLine {
            text: &raw[prefix_end..],
            col: visual_col(raw, prefix_end),
            row,
            prefix_end,
        });
    }
    Some(out)
}

/// CommonMark indented code block (§4.4): (info, content lines).
fn indented_content<'a>(block: &'a [BlockLine<'a>]) -> Option<(String, Vec<(&'a BlockLine<'a>, String)>)> {
    let mut content = Vec::new();
    for line in block {
        let (rest, removed) = strip_columns(line.text, line.col, 4, true);
        if removed < 4 && !is_blank(line.text) {
            return None;
        }
        content.push((line, rest));
    }
    while content.last().is_some_and(|(line, _)| is_blank(line.text)) {
        content.pop();
    }
    if content.is_empty() {
        return None;
    }
    Some((String::new(), content))
}

/// A fence run at the start of `text` (after at most three spaces): (indent, char, width).
fn fence_run(text: &str) -> Option<(usize, u8, usize)> {
    let indent = text.bytes().take_while(|b| *b == b' ').count();
    if indent > 3 {
        return None;
    }
    let marker = *text.as_bytes().get(indent)?;
    if marker != b'`' && marker != b'~' {
        return None;
    }
    let width = text[indent..].bytes().take_while(|b| *b == marker).count();
    (width >= 3).then_some((indent, marker, width))
}

/// Resolve backslash escapes of ASCII punctuation (§2.4) in an info string.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' && chars.peek().is_some_and(|next| next.is_ascii_punctuation()) {
            out.push(chars.next().unwrap());
        } else {
            out.push(ch);
        }
    }
    out
}

/// CommonMark fenced code block (§4.5): (info, content lines).
fn fenced_content<'a>(block: &'a [BlockLine<'a>]) -> Option<(String, Vec<(&'a BlockLine<'a>, String)>)> {
    let (opening, body) = block.split_first()?;
    let (indent, marker, width) = fence_run(opening.text)?;
    let info = opening.text[indent + width..].trim_matches(|c| c == ' ' || c == '\t');
    if marker == b'`' && info.contains('`') {
        return None;
    }
    let is_closing = |text: &str| {
        fence_run(text).is_some_and(|(close_indent, close_marker, close_width)| {
            close_marker == marker
                && close_width >= width
                && is_blank(&text[close_indent + close_width..])
        })
    };
    let mut body = body;
    if let Some((last, rest)) = body.split_last() {
        if is_closing(last.text) {
            body = rest;
        }
    }
    // The parser ends the block at its closing fence; an earlier one means a mismatch.
    if body.iter().any(|line| is_closing(line.text)) {
        return None;
    }
    let content = body
        .iter()
        .map(|line| (line, strip_columns(line.text, line.col, indent, false).0))
        .collect();
    Some((unescape(info), content))
}

fn normalize_block(node: &mut SemanticNode, lines: &[&str]) -> bool {
    let Some(block) = block_lines(node, lines) else {
        return false;
    };
    let resolved = if node.node_type == "indented_code_block" {
        indented_content(&block)
    } else {
        fenced_content(&block)
    };
    let Some((info, content)) = resolved else {
        return false;
    };
    let label = if info.is_empty() { "code".to_string() } else { format!("code:{info}") };
    let mut children = Vec::new();
    if let (Some((first, _)), Some((last, _))) = (content.first(), content.last()) {
        let mut text = content.iter().map(|(_, line)| line.as_str()).collect::<Vec<_>>().join("\n");
        if text.is_empty() && content.len() == 1 {
            // A single blank line: distinct from an empty block, which has no content leaf.
            text = String::new();
        }
        let position = NodePosition {
            start_line: first.row as u32,
            start_col: first.prefix_end as u32,
            end_line: last.row as u32,
            end_col: (last.prefix_end + last.text.len()) as u32,
        };
        children.push(SemanticNode {
            id: format!("{}.code_content", node.id),
            node_type: CODE_CONTENT.to_owned(),
            structural_hash: crate::synthetic_structural_hash(CODE_CONTENT, &text, &[]),
            label: text,
            position,
            children: Vec::new(),
            parent_type: None,
            type_info: None,
            facts: None,
        });
    }
    node.node_type = CODE_BLOCK.to_owned();
    node.structural_hash = crate::synthetic_structural_hash(CODE_BLOCK, &label, &children);
    node.label = label;
    node.children = children;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(source: &str, node_type: &str, end_line: u32) -> SemanticNode {
        SemanticNode {
            id: "0.0.0".into(),
            node_type: node_type.into(),
            label: "x".into(),
            position: NodePosition { start_line: 0, start_col: 0, end_line, end_col: 0 },
            structural_hash: format!("parser:{source}"),
            children: Vec::new(),
            parent_type: None,
            type_info: None,
            facts: None,
        }
    }

    fn canonical(source: &str, node_type: &str, end_line: u32) -> (String, Vec<String>) {
        let lines: Vec<&str> = source.split('\n').collect();
        let mut node = block(source, node_type, end_line);
        assert!(normalize_block(&mut node, &lines), "{source:?} not normalised");
        assert_eq!(node.node_type, CODE_BLOCK);
        (node.label, node.children.iter().map(|c| c.label.clone()).collect())
    }

    #[test]
    fn indented_blocks_strip_four_columns_and_trailing_blank_lines() {
        assert_eq!(canonical("    a\n\n      b\n\n", "indented_code_block", 4), ("code".into(), vec!["a\n\n  b".into()]));
        assert_eq!(canonical("\ta\n", "indented_code_block", 1), ("code".into(), vec!["a".into()]));
        assert_eq!(canonical("  \ta\n", "indented_code_block", 1), ("code".into(), vec!["a".into()]));
        assert_eq!(canonical("     \ta\n", "indented_code_block", 1), ("code".into(), vec![" \ta".into()]));
    }

    #[test]
    fn fenced_blocks_keep_info_and_strip_fence_indentation() {
        assert_eq!(canonical("``` rust \na\n```\n", "fenced_code_block", 3), ("code:rust".into(), vec!["a".into()]));
        assert_eq!(canonical("  ~~~\n  a\n   b\nc\n  ~~~\n", "fenced_code_block", 5), ("code".into(), vec!["a\n b\nc".into()]));
        assert_eq!(canonical("```\n```\n", "fenced_code_block", 2), ("code".into(), vec![]));
        assert_eq!(canonical("```\n\n```\n", "fenced_code_block", 3), ("code".into(), vec!["".into()]));
        // Unclosed: content runs to the end of the block; a shorter fence does not close.
        assert_eq!(canonical("````\na\n```\n", "fenced_code_block", 3), ("code".into(), vec!["a\n```".into()]));
        assert_eq!(canonical("```a\\_b\nx\n```\n", "fenced_code_block", 3), ("code:a_b".into(), vec!["x".into()]));
    }

    #[test]
    fn equivalent_forms_hash_identically() {
        let hash = |source: &str, node_type: &str, end_line: u32| {
            let lines: Vec<&str> = source.split('\n').collect();
            let mut node = block(source, node_type, end_line);
            assert!(normalize_block(&mut node, &lines));
            node.structural_hash
        };
        let indented = hash("    \\[\\]\n", "indented_code_block", 1);
        assert_eq!(indented, hash("~~~\n\\[\\]\n~~~\n", "fenced_code_block", 3));
        assert_eq!(indented, hash("```\n\\[\\]\n", "fenced_code_block", 2));
        assert_ne!(indented, hash("```c\n\\[\\]\n```\n", "fenced_code_block", 3));
    }

    #[test]
    fn unresolvable_lines_leave_the_parser_node_untouched() {
        // A later line with neither a continuation nor the container indentation.
        let lines = vec!["- ```", "a", "```", ""];
        let mut node = block("", "fenced_code_block", 3);
        node.position.start_col = 2;
        assert!(!normalize_block(&mut node, &lines));
        assert_eq!(node.node_type, "fenced_code_block");
        // Not a fence at all.
        let lines = vec!["abc", ""];
        let mut node = block("", "fenced_code_block", 1);
        assert!(!normalize_block(&mut node, &lines));
    }
}

#[cfg(all(test, feature = "tier-c-wasm"))]
#[path = "markdown_code_block_source_tests.rs"]
mod markdown_code_block_source_tests;
