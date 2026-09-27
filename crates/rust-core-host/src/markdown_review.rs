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
    let mut heading_indices = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start_matches(' ');
        let indent = line.len() - trimmed.len();
        if indent > 3 {
            continue;
        }
        let first = trimmed.chars().next().unwrap_or(' ');
        let count = trimmed.chars().take_while(|c| *c == first).count();
        if let Some((marker, width)) = fence {
            if first == marker && count >= width && trimmed[count..].trim().is_empty() {
                fence = None;
            }
            continue;
        }
        if (first == '`' || first == '~') && count >= 3 {
            fence = Some((first, count));
            continue;
        }
        if first == '#' && (1..=6).contains(&count) && trimmed[count..].starts_with(' ') {
            heading_indices.push(index);
        }
    }
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
            lines[end - 1].len()
        } else {
            lines[start].len()
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

/// Complete presentation payload shared by Rust callers and language adapters.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Presentation {
    pub changes: Vec<Value>,
    pub change_groups: Vec<Value>,
    #[serde(default)]
    pub ignored_style_changes: Vec<Value>,
}
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReviewPhase {
    Moves,
    Renames,
    All,
}

fn md_filename(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with(".md") || lower.ends_with(".markdown")
}
fn contained(node: &Value, section: &Value, heading_only: bool) -> bool {
    let pos = &node["position"];
    let section_pos = &section["position"];
    let Some(start) = pos["start_line"].as_u64() else {
        return false;
    };
    let Some(end) = pos["end_line"].as_u64() else {
        return false;
    };
    let Some(section_start) = section_pos["start_line"].as_u64() else {
        return false;
    };
    if heading_only {
        return start == section_start && end == section_start;
    }
    let Some(section_end) = section_pos["end_line"].as_u64() else {
        return false;
    };
    start >= section_start && end <= section_end
}

// Source-span style evidence is superseded only when every nonempty side
// belongs to a proved move/rename. Unrelated style evidence remains intact.
fn evidence_owned(evidence: &Value, old: &str, new: &str, owned: &[&Value]) -> bool {
    let mut seen = false;
    for (side, source) in [("old", old), ("new", new)] {
        let Some(span) = evidence
            .get(format!("{side}_span"))
            .and_then(Value::as_array)
        else {
            return false;
        };
        let (Some(start), Some(end)) = (
            span.first().and_then(Value::as_u64),
            span.get(1).and_then(Value::as_u64),
        ) else {
            return false;
        };
        if start == end {
            continue;
        }
        seen = true;
        let offsets: Vec<usize> = std::iter::once(0)
            .chain(source.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        let covered = owned.iter().any(|change| {
            let pos = &change[format!("{side}_node")]["position"];
            let Some(first) = pos["start_line"]
                .as_u64()
                .and_then(|i| offsets.get(i as usize))
                .copied()
            else {
                return false;
            };
            let Some(last) = pos["end_line"].as_u64() else {
                return false;
            };
            let limit = offsets
                .get(last as usize + 1)
                .copied()
                .unwrap_or(source.len());
            start as usize >= first && end as usize <= limit
        });
        if !covered {
            return false;
        }
    }
    seen
}

/// Reconcile by source evidence, never by a heading label shared by unrelated sections.
pub fn reconcile(
    mut presentation: Presentation,
    old_source: &str,
    new_source: &str,
    old_filename: &str,
    new_filename: &str,
    phase: ReviewPhase,
) -> Result<Presentation, String> {
    if !md_filename(old_filename) && !md_filename(new_filename) {
        return Ok(presentation);
    }
    let review: Value =
        serde_json::from_str(&crate::markdown_section_review_impl(old_source, new_source))
            .map_err(|e| e.to_string())?;
    let empty = Vec::new();
    let moves = review["moves"].as_array().unwrap_or(&empty);
    let renames = review["renames"].as_array().unwrap_or(&empty);
    let use_moves = matches!(phase, ReviewPhase::Moves | ReviewPhase::All);
    let use_renames = matches!(phase, ReviewPhase::Renames | ReviewPhase::All);
    let owned: Vec<&Value> = moves
        .iter()
        .filter(|_| use_moves)
        .chain(renames.iter().filter(|_| use_renames))
        .collect();
    presentation
        .ignored_style_changes
        .retain(|e| !evidence_owned(e, old_source, new_source, &owned));
    presentation.change_groups.retain(|g| {
        g["kind"] != "IGNORED_STYLE"
            || !evidence_owned(&g["metadata"], old_source, new_source, &owned)
    });
    let mut remap = HashMap::new();
    let mut retained = Vec::new();
    for (index, change) in presentation.changes.into_iter().enumerate() {
        let kind = change["change_type"].as_str().unwrap_or("");
        let move_owned = use_moves
            && moves.iter().any(|m| match kind {
                "DELETION" => contained(&change["old_node"], &m["old_node"], false),
                "ADDITION" => contained(&change["new_node"], &m["new_node"], false),
                _ => false,
            });
        let rename_owned = use_renames
            && renames.iter().any(|r| match kind {
                "MODIFICATION" => {
                    contained(&change["old_node"], &r["old_node"], true)
                        && contained(&change["new_node"], &r["new_node"], true)
                }
                "DELETION" => contained(&change["old_node"], &r["old_node"], true),
                "ADDITION" => contained(&change["new_node"], &r["new_node"], true),
                _ => false,
            });
        if !move_owned && !rename_owned {
            remap.insert(index, retained.len());
            retained.push(change);
        }
    }
    presentation.change_groups.retain(|g| {
        let Some(indices) = g["raw_change_indices"].as_array() else {
            return true;
        };
        indices.is_empty()
            || indices.iter().any(|i| {
                i.as_u64()
                    .is_some_and(|i| remap.contains_key(&(i as usize)))
            })
    });
    for group in &mut presentation.change_groups {
        if let Some(indices) = group
            .get_mut("raw_change_indices")
            .and_then(Value::as_array_mut)
        {
            *indices = indices
                .iter()
                .filter_map(|i| {
                    i.as_u64()
                        .and_then(|i| remap.get(&(i as usize)))
                        .map(|i| json!(i))
                })
                .collect();
        }
    }
    for (enabled, changes, group_key) in [
        (use_moves, moves, "move_group"),
        (use_renames, renames, "rename_group"),
    ] {
        if !enabled || changes.is_empty() {
            continue;
        }
        let start = retained.len();
        retained.extend(changes.iter().cloned());
        let mut group = review[group_key].clone();
        if group.is_object() {
            group["raw_change_indices"] = json!((start..retained.len()).collect::<Vec<_>>());
            presentation.change_groups.push(group);
        }
    }
    presentation.changes = retained;
    Ok(presentation)
}
