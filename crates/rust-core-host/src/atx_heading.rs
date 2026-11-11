//! CommonMark ATX headings are owned by core, not the parser label (#275).
//!
//! The Markdown and MDX components emit a heading's text as its label and drop the opening
//! sequence, so `# Guide` and `## Guide` produced identical labels and the style-only shortcut
//! certified a document-outline change as `IGNORED_STYLE`. They also keep the optional closing
//! sequence (`# Guide #`) in the label, so adding one was reported as a section rename.
//!
//! Every heading node carries its source span, so core re-derives both facts from the heading
//! line (CommonMark 0.31 §4.2):
//!
//! * the level enters the tree as a `heading_level` leaf child whose label is the level
//!   (`"1"`..`"6"`), so a level change is an ordinary leaf update, and
//! * a valid optional closing sequence (a run of `#` preceded by a space or tab, or the entire
//!   content, followed only by spaces/tabs) is stripped from the heading label. `# foo#` keeps
//!   `foo#` and `# foo \#` keeps its escaped hash: those are heading content.
//!
//! Lines that are not well-formed ATX headings, setext headings and nodes whose label does not
//! match the heading line are left untouched (conservative: the parser's label stands).

use crate::SemanticNode;

/// Leaf node type that carries a heading's level.
pub(crate) const HEADING_LEVEL_NODE_TYPE: &str = "heading_level";

/// One parsed ATX heading line.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct AtxHeading<'a> {
    pub(crate) level: usize,
    /// Byte column of the first `#` of the opening sequence.
    pub(crate) open_col: usize,
    /// Heading content before closing-sequence removal (spaces/tabs trimmed).
    pub(crate) raw_content: &'a str,
    /// Heading content per CommonMark (closing sequence removed).
    pub(crate) content: &'a str,
}

impl AtxHeading<'_> {
    /// The heading line with the closing sequence and incidental spacing normalised away.
    pub(crate) fn canonical(&self) -> String {
        let hashes = "#".repeat(self.level);
        if self.content.is_empty() {
            hashes
        } else {
            format!("{hashes} {}", self.content)
        }
    }
}

fn is_blank(c: char) -> bool {
    c == ' ' || c == '\t'
}

/// Parse one source line as a CommonMark ATX heading.
pub(crate) fn parse_atx_heading(line: &str) -> Option<AtxHeading<'_>> {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let rest = line.trim_start_matches(' ');
    let open_col = line.len() - rest.len();
    if open_col > 3 {
        return None;
    }
    let level = rest.bytes().take_while(|b| *b == b'#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let after = &rest[level..];
    if !(after.is_empty() || after.starts_with(is_blank)) {
        return None;
    }
    let raw_content = after.trim_matches(is_blank);
    Some(AtxHeading {
        level,
        open_col,
        raw_content,
        content: strip_closing_sequence(raw_content),
    })
}

/// Remove a valid optional closing sequence from spaces/tabs-trimmed heading content.
fn strip_closing_sequence(raw: &str) -> &str {
    let without = raw.trim_end_matches('#');
    if without.len() == raw.len() {
        return raw;
    }
    if without.is_empty() {
        // `### ###`: the closing sequence is the whole content.
        return "";
    }
    if without.ends_with(is_blank) {
        return without.trim_end_matches(is_blank);
    }
    // `foo#` / `foo \#`: the hashes are content.
    raw
}

fn heading_language(language: &str) -> Option<bool> {
    match language.to_ascii_lowercase().as_str() {
        "markdown" | "md" => Some(false),
        "mdx" => Some(true),
        _ => None,
    }
}

/// Normalise every ATX heading node of a Markdown/MDX tree (no-op for other languages).
/// Idempotent: a heading that already carries a `heading_level` child is left alone.
pub(crate) fn normalize_atx_headings(tree: &mut SemanticNode, source: &str, language: &str) {
    let Some(mdx) = heading_language(language) else {
        return;
    };
    let lines: Vec<&str> = source.split('\n').collect();
    normalize_node(tree, &lines, mdx);
}

fn is_heading_node(node: &SemanticNode, mdx: bool) -> bool {
    // markdown: `section > atx_heading`; mdx: the heading is the (leaf) `section` itself.
    if mdx {
        node.node_type == "section"
    } else {
        node.node_type == "atx_heading"
    }
}

fn normalize_node(node: &mut SemanticNode, lines: &[&str], mdx: bool) -> bool {
    let heading_child_label = node
        .children
        .first()
        .filter(|child| child.node_type == "atx_heading")
        .map(|child| child.label.clone());
    let mut changed = false;
    for child in &mut node.children {
        changed |= normalize_node(child, lines, mdx);
    }
    // markdown sections are labelled by their heading: keep that identity in step.
    if let (false, Some(before), Some(after)) = (
        mdx,
        heading_child_label,
        node.children.first().map(|child| child.label.clone()),
    ) {
        if node.node_type == "section" && node.label == before && before != after {
            node.label = after;
            changed = true;
        }
    }
    if is_heading_node(node, mdx)
        && !node.children.iter().any(|c| c.node_type == HEADING_LEVEL_NODE_TYPE)
    {
        changed |= normalize_heading(node, lines);
    }
    if changed {
        node.structural_hash = crate::enriched_structural_hash(node, &node.label, &node.children);
    }
    changed
}

fn normalize_heading(node: &mut SemanticNode, lines: &[&str]) -> bool {
    let line_index = node.position.start_line as usize;
    let Some(heading) = lines.get(line_index).and_then(|line| parse_atx_heading(line)) else {
        return false;
    };
    // The node must start on the heading's opening sequence, not merely on its line.
    let start_col = node.position.start_col as usize;
    if start_col != 0 && start_col != heading.open_col {
        return false;
    }
    if heading.raw_content != heading.content {
        let closing = heading.raw_content[heading.content.len()..].trim_start_matches(is_blank);
        if node.label == heading.raw_content {
            node.label = heading.content.to_owned();
        } else if let Some(stripped) = node.label.strip_suffix(closing) {
            // The parser may render inline content differently; strip only the exact
            // closing run, and only when it is separated from the content by a blank.
            if stripped.ends_with(is_blank) || stripped.is_empty() {
                node.label = stripped.trim_end_matches(is_blank).to_owned();
            }
        }
    }
    let line = line_index as u32;
    let open = heading.open_col as u32;
    let label = heading.level.to_string();
    let leaf = SemanticNode {
        id: format!("{}.heading_level", node.id),
        node_type: HEADING_LEVEL_NODE_TYPE.to_owned(),
        structural_hash: crate::synthetic_structural_hash(HEADING_LEVEL_NODE_TYPE, &label, &[]),
        label,
        position: crate::NodePosition {
            start_line: line,
            start_col: open,
            end_line: line,
            end_col: open + heading.level as u32,
        },
        children: Vec::new(),
        parent_type: None,
        type_info: None,
        facts: None,
    };
    node.children.insert(0, leaf);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content(line: &str) -> Option<(usize, &str)> {
        parse_atx_heading(line).map(|h| (h.level, h.content))
    }

    #[test]
    fn closing_sequence_follows_commonmark() {
        assert_eq!(content("# Guide"), Some((1, "Guide")));
        assert_eq!(content("# Guide #"), Some((1, "Guide")));
        assert_eq!(content("## Guide ###   "), Some((2, "Guide")));
        assert_eq!(content("#\tFoo\t##"), Some((1, "Foo")));
        assert_eq!(content("# foo#"), Some((1, "foo#")));
        assert_eq!(content("# foo \\#"), Some((1, "foo \\#")));
        assert_eq!(content("### ###"), Some((3, "")));
        assert_eq!(content("#"), Some((1, "")));
        assert_eq!(content("   # x"), Some((1, "x")));
        assert_eq!(content("# Guide #\r"), Some((1, "Guide")));
    }

    fn section_renames(old: &str, new: &str) -> usize {
        let review: serde_json::Value =
            serde_json::from_str(&crate::markdown_section_review_impl(old, new)).unwrap();
        review["renames"].as_array().unwrap().len()
    }

    #[test]
    fn source_section_review_ignores_closing_sequence_but_not_level_or_text() {
        assert_eq!(section_renames("# Guide\n\nbody\n", "# Guide #\n\nbody\n"), 0);
        assert_eq!(section_renames("# Guide\n\nbody\n", "#  Guide\t##  \n\nbody\n"), 0);
        assert_eq!(section_renames("# Guide\n\nbody\n", "## Guide\n\nbody\n"), 1);
        assert_eq!(section_renames("#\tFoo\n\nbody\n", "##\tFoo\n\nbody\n"), 1);
        assert_eq!(section_renames("# foo#\n\nbody\n", "# foo\n\nbody\n"), 1);
    }

    #[test]
    fn tree_headings_gain_level_and_lose_closing_sequence() {
        let source = "# Guide #\n";
        let heading = SemanticNode {
            id: "0.0.0".into(),
            node_type: "atx_heading".into(),
            label: "Guide #".into(),
            position: crate::NodePosition { start_line: 0, start_col: 0, end_line: 1, end_col: 0 },
            structural_hash: "h".into(),
            children: vec![],
            parent_type: None,
            type_info: None,
            facts: None,
        };
        let mut section = SemanticNode {
            id: "0.0".into(),
            node_type: "section".into(),
            label: "Guide #".into(),
            structural_hash: "s".into(),
            children: vec![heading.clone()],
            ..heading
        };
        normalize_atx_headings(&mut section, source, "markdown");
        assert_eq!(section.label, "Guide");
        let heading = &section.children[0];
        assert_eq!(heading.label, "Guide");
        assert_eq!(heading.children.len(), 1);
        assert_eq!(heading.children[0].node_type, HEADING_LEVEL_NODE_TYPE);
        assert_eq!(heading.children[0].label, "1");
        // Idempotent: a second pass changes nothing.
        let before = serde_json::to_string(&section).unwrap();
        normalize_atx_headings(&mut section, source, "markdown");
        assert_eq!(serde_json::to_string(&section).unwrap(), before);
    }

    #[test]
    fn non_headings_are_rejected() {
        for line in ["#hashtag", "####### seven", "    # code", "text # not", "", "\\# escaped"] {
            assert_eq!(parse_atx_heading(line), None, "{line:?}");
        }
    }
}
