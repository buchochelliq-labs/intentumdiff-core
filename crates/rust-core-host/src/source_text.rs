//! Bounded source text for change evidence (#300): what a changed node's source says, so a
//! consumer can tell WHAT changed without re-slicing the files. Whitespace is collapsed and
//! the text is capped; nodes whose span is larger than `MAX_SPAN_BYTES` carry none.
use crate::{ChangeDraft, SemanticNode};

/// Longest emitted text, in characters (a trailing `…` included).
pub(crate) const MAX_TEXT_CHARS: usize = 120;
/// Nodes spanning more source than this get no text at all.
pub(crate) const MAX_SPAN_BYTES: usize = 4096;

fn line_starts(source: &str) -> Vec<usize> {
    let mut starts = vec![0];
    starts.extend(source.bytes().enumerate().filter(|(_, b)| *b == b'\n').map(|(i, _)| i + 1));
    starts
}

/// The node's bounded, whitespace-collapsed source text.
pub(crate) fn node_text(source: &str, starts: &[usize], node: &SemanticNode) -> Option<String> {
    node_text_full(source, starts, node).map(|text| bound(&text, 0))
}

/// `text` from character `skip` on (marked with a leading `…`), capped at MAX_TEXT_CHARS.
fn bound(text: &str, skip: usize) -> String {
    let mut out = String::new();
    if skip > 0 {
        out.push('…');
    }
    let rest: Vec<char> = text.chars().skip(skip).collect();
    let room = MAX_TEXT_CHARS - out.chars().count();
    if rest.len() <= room {
        out.extend(rest);
    } else {
        out.extend(&rest[..room - 1]);
        out.push('…');
    }
    out
}

/// Bounded old/new texts that keep the first difference in view: when either side is too long,
/// both windows start a little before the first differing character. None when equal.
fn bounded_pair(old: &str, new: &str) -> Option<(String, String)> {
    if old == new {
        return None;
    }
    let long = old.chars().count() > MAX_TEXT_CHARS || new.chars().count() > MAX_TEXT_CHARS;
    let prefix = old.chars().zip(new.chars()).take_while(|(a, b)| a == b).count();
    let skip = if long { prefix.saturating_sub(MAX_TEXT_CHARS / 4) } else { 0 };
    Some((bound(old, skip), bound(new, skip)))
}

fn node_text_full(source: &str, starts: &[usize], node: &SemanticNode) -> Option<String> {
    let at = |line: u32, col: u32| -> Option<usize> {
        let offset = starts.get(line as usize)? + col as usize;
        (offset <= source.len() && source.is_char_boundary(offset)).then_some(offset)
    };
    let start = at(node.position.start_line, node.position.start_col)?;
    let end = at(node.position.end_line, node.position.end_col)?;
    if end <= start || end - start > MAX_SPAN_BYTES {
        return None;
    }
    let collapsed = source[start..end].split_whitespace().collect::<Vec<_>>().join(" ");
    (!collapsed.is_empty()).then_some(collapsed)
}

/// Fill `text_diff` for modifications that have none (non-leaf statements) with the bounded
/// old/new source, and make additions/deletions of unlabelled nodes name their text.
pub(crate) fn annotate_source_text_drafts(changes: &mut [ChangeDraft<'_>], old_source: &str, new_source: &str) {
    if old_source.is_empty() && new_source.is_empty() {
        return;
    }
    let (old_starts, new_starts) = (line_starts(old_source), line_starts(new_source));
    for change in changes {
        match change.change_type {
            "MODIFICATION" if change.text_diff.is_none() => {
                let (Some(old), Some(new)) = (change.old_node, change.new_node) else { continue };
                if let (Some(a), Some(b)) = (node_text_full(old_source, &old_starts, old), node_text_full(new_source, &new_starts, new)) {
                    if let Some((a, b)) = bounded_pair(&a, &b) {
                        change.text_diff = Some(format!("{a} -> {b}"));
                    }
                }
            }
            "DELETION" | "ADDITION" => {
                let (node, source, starts) = if change.change_type == "DELETION" {
                    (change.old_node, old_source, &old_starts)
                } else {
                    (change.new_node, new_source, &new_starts)
                };
                let Some(node) = node else { continue };
                // Only unlabelled nodes: a real label already names the thing.
                if !(node.label.is_empty() || node.label == node.node_type) {
                    continue;
                }
                let Some(text) = node_text(source, starts, node) else { continue };
                let generic = format!("{}('{}')", node.node_type, node.label);
                if change.description.contains(&generic) {
                    change.description = change.description.replace(&generic, &format!("{}('{}')", node.node_type, text));
                }
            }
            _ => {}
        }
    }
}
