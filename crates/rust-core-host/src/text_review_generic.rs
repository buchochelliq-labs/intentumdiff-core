// Extracted verbatim from lib.rs (issue #85, lib.rs split round 2).
use crate::*;

/// LCS-based opcodes over comparable items. Returns None when the quadratic
/// table would be too large (caller falls back to the Python path).
pub(crate) fn lcs_opcodes<T: PartialEq>(
    old: &[T],
    new: &[T],
) -> Option<Vec<(TextOp, usize, usize, usize, usize)>> {
    const MAX_CELLS: usize = 4_000_000;
    if old.len().saturating_mul(new.len()) > MAX_CELLS {
        return None;
    }
    let cols = new.len() + 1;
    let mut table = vec![0u32; (old.len() + 1) * cols];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            table[i * cols + j] = if old[i] == new[j] {
                table[(i + 1) * cols + j + 1] + 1
            } else {
                table[(i + 1) * cols + j].max(table[i * cols + j + 1])
            };
        }
    }
    fn push(
        ops: &mut Vec<(TextOp, usize, usize, usize, usize)>,
        op: TextOp,
        i0: usize,
        i1: usize,
        j0: usize,
        j1: usize,
    ) {
        if let Some(last) = ops.last_mut() {
            if last.0 == op && last.2 == i0 && last.4 == j0 {
                last.2 = i1;
                last.4 = j1;
                return;
            }
        }
        ops.push((op, i0, i1, j0, j1));
    }
    let mut ops: Vec<(TextOp, usize, usize, usize, usize)> = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < old.len() && j < new.len() {
        if old[i] == new[j] {
            push(&mut ops, TextOp::Equal, i, i + 1, j, j + 1);
            i += 1;
            j += 1;
        } else if table[(i + 1) * cols + j] >= table[i * cols + j + 1] {
            push(&mut ops, TextOp::Delete, i, i + 1, j, j);
            i += 1;
        } else {
            push(&mut ops, TextOp::Insert, i, i, j, j + 1);
            j += 1;
        }
    }
    if i < old.len() {
        push(&mut ops, TextOp::Delete, i, old.len(), j, j);
    }
    if j < new.len() {
        push(&mut ops, TextOp::Insert, i, i, j, new.len());
    }
    // Merge adjacent delete/insert runs into replace (difflib-compatible shape).
    let mut merged: Vec<(TextOp, usize, usize, usize, usize)> = Vec::new();
    for op in ops {
        if let Some(last) = merged.last_mut() {
            let mergeable = matches!(
                (last.0, op.0),
                (TextOp::Delete, TextOp::Insert)
                    | (TextOp::Insert, TextOp::Delete)
                    | (TextOp::Replace, TextOp::Insert)
                    | (TextOp::Replace, TextOp::Delete)
            );
            if mergeable {
                last.0 = TextOp::Replace;
                last.1 = last.1.min(op.1);
                last.2 = last.2.max(op.2);
                last.3 = last.3.min(op.3);
                last.4 = last.4.max(op.4);
                continue;
            }
        }
        merged.push(op);
    }
    Some(merged)
}

pub(crate) fn inline_char_diff(old_line: &str, new_line: &str) -> String {
    let old_chars: Vec<char> = old_line.chars().collect();
    let new_chars: Vec<char> = new_line.chars().collect();
    // difflib-parity placement: Ratcliff-Obershelp longest-matching-block recursion
    // anchors insert/delete highlights at natural (word) boundaries, where a plain LCS
    // backtrack splits mid-word ("alpha brav[+e new brav]o" instead of
    // "alpha[+ brave new] bravo").
    fn longest_match(
        old: &[char],
        new: &[char],
        alo: usize,
        ahi: usize,
        blo: usize,
        bhi: usize,
    ) -> (usize, usize, usize) {
        let (mut best_i, mut best_j, mut best_size) = (alo, blo, 0usize);
        // Rolling common-substring DP: lengths[idx+1] = run ending at (i, j); ties keep the
        // earliest (lowest i, then lowest j) block, difflib's preference.
        let mut lengths = vec![0usize; bhi.saturating_sub(blo) + 1];
        for i in alo..ahi {
            let mut prev = 0usize;
            for j in blo..bhi {
                let idx = j - blo;
                let above = lengths[idx + 1];
                let size = if old[i] == new[j] { prev + 1 } else { 0 };
                lengths[idx + 1] = size;
                prev = above;
                if size > best_size {
                    best_size = size;
                    best_i = i + 1 - size;
                    best_j = j + 1 - size;
                }
            }
        }
        (best_i, best_j, best_size)
    }
    fn walk(
        old: &[char],
        new: &[char],
        alo: usize,
        ahi: usize,
        blo: usize,
        bhi: usize,
        parts: &mut String,
    ) {
        let (i, j, size) = longest_match(old, new, alo, ahi, blo, bhi);
        if size == 0 {
            let old_slice: String = old[alo..ahi].iter().collect();
            let new_slice: String = new[blo..bhi].iter().collect();
            if !old_slice.is_empty() && !new_slice.is_empty() {
                parts.push_str(&format!("[-{old_slice}][+{new_slice}]"));
            } else if !old_slice.is_empty() {
                parts.push_str(&format!("[-{old_slice}]"));
            } else if !new_slice.is_empty() {
                parts.push_str(&format!("[+{new_slice}]"));
            }
            return;
        }
        walk(old, new, alo, i, blo, j, parts);
        let equal: String = old[i..i + size].iter().collect();
        parts.push_str(&equal);
        walk(old, new, i + size, ahi, j + size, bhi, parts);
    }
    let mut parts = String::new();
    walk(
        &old_chars,
        &new_chars,
        0,
        old_chars.len(),
        0,
        new_chars.len(),
        &mut parts,
    );
    let chars: Vec<char> = parts.chars().collect();
    if chars.len() > 200 {
        let head: String = chars[..197].iter().collect();
        format!("{head}...")
    } else {
        parts
    }
}
pub(crate) fn generic_text_node_json(
    node_id: &str,
    label: &str,
    start_line: usize,
    start_col: usize,
    end_line: usize,
    mut end_col: usize,
) -> Value {
    if (end_line, end_col) == (start_line, start_col) {
        end_col += 1;
    }
    let mut hasher = Sha256::new();
    hasher.update(
        format!("text_line\0{label}\0{start_line}:{start_col}:{end_line}:{end_col}").as_bytes(),
    );
    let digest = format!("{:x}", hasher.finalize());
    serde_json::json!({
        "id": node_id,
        "node_type": "text_line",
        "label": label,
        "position": {
            "start_line": start_line,
            "start_col": start_col,
            "end_line": end_line,
            "end_col": end_col,
        },
        "structural_hash": digest,
        "children": [],
    })
}

pub(crate) fn generic_text_deletion_value(line: &str, line_no: usize) -> Option<Value> {
    if line.trim().is_empty() {
        // Symmetric with insertions: blank-line churn is layout, not content (issue #15).
        return None;
    }
    let len = line.chars().count().max(1);
    Some(serde_json::json!({
        "change_type": "DELETION",
        "old_node": generic_text_node_json(
            &format!("generic-old-{line_no}"), line, line_no, 0, line_no, len),
        "confidence": 0.98,
        "description": format!("Delete line {}: {:?}", line_no + 1, line),
    }))
}

pub(crate) fn generic_text_addition_value(line: &str, line_no: usize) -> Option<Value> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    let leading = line.chars().count() - line.trim_start().chars().count();
    let end = line.trim_end().chars().count();
    Some(serde_json::json!({
        "change_type": "ADDITION",
        "new_node": generic_text_node_json(
            &format!("generic-new-{line_no}"), trimmed, line_no, leading, line_no, end),
        "confidence": 0.98,
        "description": format!("Insert line {}: {:?}", line_no + 1, trimmed),
    }))
}

pub(crate) fn generic_text_changes_value(old_source: &str, new_source: &str) -> Option<Vec<Value>> {
    let old_lines: Vec<&str> = old_source.lines().collect();
    let new_lines: Vec<&str> = new_source.lines().collect();
    let ops = lcs_opcodes(&old_lines, &new_lines)?;
    let mut changes: Vec<Value> = Vec::new();
    for (op, old_start, old_end, new_start, new_end) in ops {
        match op {
            TextOp::Equal => {}
            TextOp::Insert => {
                for (offset, line) in new_lines[new_start..new_end].iter().enumerate() {
                    changes.extend(generic_text_addition_value(line, new_start + offset));
                }
            }
            TextOp::Delete => {
                for (offset, line) in old_lines[old_start..old_end].iter().enumerate() {
                    changes.extend(generic_text_deletion_value(line, old_start + offset));
                }
            }
            TextOp::Replace => {
                let old_block = &old_lines[old_start..old_end];
                let new_block = &new_lines[new_start..new_end];
                let paired = old_block.len().min(new_block.len());
                for offset in 0..paired {
                    let old_line = old_block[offset];
                    let new_line = new_block[offset];
                    if old_line == new_line {
                        continue;
                    }
                    // One changed text line = ONE line-level MODIFICATION; the char
                    // detail lives in text_diff for inline rendering.
                    let old_no = old_start + offset;
                    let new_no = new_start + offset;
                    changes.push(serde_json::json!({
                        "change_type": "MODIFICATION",
                        "old_node": generic_text_node_json(
                            &format!("generic-old-mod-{old_no}"), old_line, old_no, 0,
                            old_no, old_line.chars().count().max(1)),
                        "new_node": generic_text_node_json(
                            &format!("generic-new-mod-{new_no}"), new_line, new_no, 0,
                            new_no, new_line.chars().count().max(1)),
                        "confidence": 0.98,
                        "description": format!(
                            "Change line {}: {:?} -> {:?}", new_no + 1, old_line, new_line),
                        "text_diff": inline_char_diff(old_line, new_line),
                    }));
                }
                for (offset, line) in old_block[paired..].iter().enumerate() {
                    changes.extend(generic_text_deletion_value(line, old_start + paired + offset));
                }
                for (offset, line) in new_block[paired..].iter().enumerate() {
                    changes.extend(generic_text_addition_value(line, new_start + paired + offset));
                }
            }
        }
    }
    Some(net_out_relocated_lines(changes))
}

/// A line deleted here and inserted verbatim there is a reorder, not add+delete —
/// prose has no execution order (issue #14). Greedy pairing by line proximity.
pub(crate) fn net_out_relocated_lines(changes: Vec<Value>) -> Vec<Value> {
    fn label_line(change: &Value, side: &str) -> Option<(String, i64)> {
        let node = change.get(side)?;
        let label = node.get("label")?.as_str()?.to_string();
        if label.trim().is_empty() {
            return None;
        }
        let line = node.get("position")?.get("start_line")?.as_i64()?;
        Some((label, line))
    }
    let deletions: Vec<(usize, String, i64)> = changes
        .iter()
        .enumerate()
        .filter(|(_, change)| change.get("change_type").and_then(Value::as_str) == Some("DELETION"))
        .filter_map(|(idx, change)| {
            label_line(change, "old_node").map(|(label, line)| (idx, label, line))
        })
        .collect();
    let additions: Vec<(usize, String, i64)> = changes
        .iter()
        .enumerate()
        .filter(|(_, change)| change.get("change_type").and_then(Value::as_str) == Some("ADDITION"))
        .filter_map(|(idx, change)| {
            label_line(change, "new_node").map(|(label, line)| (idx, label, line))
        })
        .collect();
    let mut netted: HashSet<usize> = HashSet::new();
    for (del_idx, del_label, del_line) in &deletions {
        let mut best: Option<(usize, i64)> = None;
        for (add_idx, add_label, add_line) in &additions {
            if netted.contains(add_idx) || add_label != del_label {
                continue;
            }
            let distance = (add_line - del_line).abs();
            if best.is_none() || distance < best.expect("some").1 {
                best = Some((*add_idx, distance));
            }
        }
        if let Some((add_idx, _)) = best {
            netted.insert(*del_idx);
            netted.insert(add_idx);
        }
    }
    if netted.is_empty() {
        return changes;
    }
    changes
        .into_iter()
        .enumerate()
        .filter_map(|(idx, change)| (!netted.contains(&idx)).then_some(change))
        .collect()
}

/// python presentation.generic_text_diff (issue #35). The C ABI (`intentdiff_call`) calls this
/// directly. Infallible — an oversized input yields a `{"used": false}` envelope, not an error.
pub(crate) fn generic_text_review_impl(
    old_source: &str,
    new_source: &str,
    raw_change_count: usize,
) -> String {
    let Some(changes) = generic_text_changes_value(old_source, new_source) else {
        return serde_json::json!({"used": false, "reason": "input too large for LCS table"})
            .to_string();
    };
    let group = if raw_change_count > 0 {
        // Audit trail, not hidden content: records that raw parser token churn was
        // replaced wholesale by the stable line view. Owns no final change by design.
        serde_json::json!({
            "kind": "NOISE_SUPPRESSED",
            "raw_change_indices": [],
            "confidence": 0.8,
            "rule_id": "presentation.generic_text_diff",
            "metadata": {
                "suppressed_count": raw_change_count,
                "replacement_count": changes.len(),
                "reason": "Generic parser token churn was replaced with stable text line and character spans.",
            },
        })
    } else {
        Value::Null
    };
    serde_json::json!({"used": true, "changes": changes, "group": group}).to_string()
}

// ---------------------------------------------------------------------------
// Markdown section review stage (issue #36) — engine-side port of the two
// markdown post-presentation rules: section MOVES with LIS insertion-shift
// discrimination (a swap is ONE move, issues #12/#32/#15) and heading RENAMES
// by unique body hash. Python keeps only the change-list filtering.
// ---------------------------------------------------------------------------
