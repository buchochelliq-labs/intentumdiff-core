//! Markdown section derivation for the review path, extracted from lib.rs
//! verbatim (issue #29 monolith split, phase B). The pyfunction wrapper stays
//! in lib.rs beside the pymodule registration.

use crate::*;

pub(crate) struct MarkdownSection {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) start_line: usize,
    pub(crate) end_line: usize,
    pub(crate) end_col: usize,
    pub(crate) section_hash: String,
    pub(crate) body_hash: String,
}

pub(crate) fn markdown_sections(source: &str, side: &str) -> Vec<MarkdownSection> {
    let lines: Vec<&str> = source.lines().collect();
    let heading_indices: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| {
            line.starts_with('#') && line.trim_start_matches('#').starts_with(' ')
        })
        .map(|(idx, _)| idx)
        .collect();
    let mut sections = Vec::new();
    for (order, &start) in heading_indices.iter().enumerate() {
        let end = heading_indices
            .get(order + 1)
            .copied()
            .unwrap_or(lines.len());
        let text = lines[start..end].join("\n");
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        let mut hasher = Sha256::new();
        hasher.update(text.as_bytes());
        let section_hash = format!("{:x}", hasher.finalize());
        let body = lines[(start + 1).min(end)..end].join("\n");
        let mut body_hasher = Sha256::new();
        body_hasher.update(body.trim().as_bytes());
        let body_hash = format!("{:x}", body_hasher.finalize());
        let end_line = end.saturating_sub(1).max(start);
        let end_col = if end > start {
            lines[end - 1].chars().count()
        } else {
            lines[start].chars().count()
        };
        sections.push(MarkdownSection {
            id: format!("markdown-{side}-{order}"),
            label: lines[start].trim().to_string(),
            start_line: start,
            end_line,
            end_col,
            section_hash,
            body_hash,
        });
    }
    sections
}

pub(crate) fn markdown_section_node_json(section: &MarkdownSection) -> Value {
    serde_json::json!({
        "id": section.id,
        "node_type": "markdown_section",
        "label": section.label,
        "position": {
            "start_line": section.start_line,
            "start_col": 0,
            "end_line": section.end_line,
            "end_col": section.end_col,
        },
        "structural_hash": section.section_hash,
        "children": [],
    })
}
