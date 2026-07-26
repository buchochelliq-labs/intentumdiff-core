//! SemanticDiff → LSP shape mappings (#100 S3 slice 1), ported from
//! `src/intentdiff/lsp_server/_codelens.py` and `_diagnostics.py`.
//!
//! These are the intentdiff-HANDLER half of the future native LSP server (the generic
//! JSON-RPC dispatch reuses the live-server binary's stdio pattern + the lsp-client
//! codec). Output uses the LSP wire shapes exactly as `lsprotocol` serialises them
//! (camelCase `character`, numeric severities), so a client cannot tell the producer
//! changed. The Python mappings remain the executing shell under pygls until the native
//! server lands; the parity suite pins the twins together.

use serde_json::{json, Value};

/// Change types surfaced as code lenses (`_CODELENS_CHANGE_TYPES`).
const CODELENS_CHANGE_TYPES: [&str; 5] = [
    "REFACTORING",
    "MOVE",
    "MOVE_TO_MODULE",
    "CROSS_FILE_RENAME",
    "REORDER",
];

/// LSP DiagnosticSeverity numeric values.
const SEVERITY_WARNING: i64 = 2;
const SEVERITY_HINT: i64 = 4;

/// `node_to_lsp_range`: a SemanticNode's 0-indexed position as an LSP Range.
fn node_to_lsp_range(node: &Value) -> Value {
    let pos = node.get("position");
    let get = |field: &str| -> i64 {
        pos.and_then(|p| p.get(field)).and_then(Value::as_i64).unwrap_or(0)
    };
    json!({
        "start": {"line": get("start_line"), "character": get("start_col")},
        "end": {"line": get("end_line"), "character": get("end_col")},
    })
}

/// `_codelens_title`: `↻ KIND: description` for refactorings, `⟳ TYPE: description`
/// otherwise. (`ChangeType`/`RefactoringKind` serialise name-as-value, so the JSON string
/// IS the Python `.name`.)
fn codelens_title(change: &Value) -> String {
    let description = change
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("");
    if let Some(kind) = change.get("refactoring_kind").and_then(Value::as_str) {
        return format!("\u{21bb} {kind}: {description}");
    }
    let change_type = change
        .get("change_type")
        .and_then(Value::as_str)
        .unwrap_or("");
    format!("\u{27f3} {change_type}: {description}")
}

/// `semantic_diff_to_codelens`: display-only lenses for the refactoring-family change
/// types, anchored at `new_node` (falling back to `old_node`).
pub fn semantic_diff_to_codelens_value(diff: &Value) -> Vec<Value> {
    let empty: Vec<Value> = Vec::new();
    let changes = diff
        .get("changes")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    changes
        .iter()
        .filter(|change| {
            let ct = change
                .get("change_type")
                .and_then(Value::as_str)
                .unwrap_or("");
            CODELENS_CHANGE_TYPES.contains(&ct)
        })
        .filter_map(|change| {
            let node = match change.get("new_node") {
                Some(n) if !n.is_null() => n,
                _ => match change.get("old_node") {
                    Some(n) if !n.is_null() => n,
                    _ => return None,
                },
            };
            Some(json!({
                "range": node_to_lsp_range(node),
                "command": {"title": codelens_title(change), "command": ""},
            }))
        })
        .collect()
}

/// `semantic_diff_to_diagnostics`: STYLE_ONLY changes → Hint on the new node; parse
/// errors → Warning over the full-document sentinel range.
pub fn semantic_diff_to_diagnostics_value(diff: &Value) -> Vec<Value> {
    let empty: Vec<Value> = Vec::new();
    let mut diags: Vec<Value> = Vec::new();
    for change in diff.get("changes").and_then(Value::as_array).unwrap_or(&empty) {
        if change.get("change_type").and_then(Value::as_str) != Some("STYLE_ONLY") {
            continue;
        }
        let Some(new_node) = change.get("new_node").filter(|n| !n.is_null()) else {
            continue;
        };
        diags.push(json!({
            "range": node_to_lsp_range(new_node),
            "severity": SEVERITY_HINT,
            "message": "Style-only change — no semantic impact",
            "source": "intentdiff",
        }));
    }
    for err in diff.get("parse_errors").and_then(Value::as_array).unwrap_or(&empty) {
        let err_text = match err {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        diags.push(json!({
            "range": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 2_147_483_647i64, "character": 0},
            },
            "severity": SEVERITY_WARNING,
            "message": format!("Parse error: {err_text}"),
            "source": "intentdiff",
        }));
    }
    diags
}

pub(crate) fn codelens_json_impl(diff_json: &str) -> Result<String, String> {
    let diff: Value =
        serde_json::from_str(diff_json).map_err(|e| format!("invalid diff json: {e}"))?;
    Ok(Value::Array(semantic_diff_to_codelens_value(&diff)).to_string())
}

pub(crate) fn diagnostics_json_impl(diff_json: &str) -> Result<String, String> {
    let diff: Value =
        serde_json::from_str(diff_json).map_err(|e| format!("invalid diff json: {e}"))?;
    Ok(Value::Array(semantic_diff_to_diagnostics_value(&diff)).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(line: i64, col: i64, end_line: i64, end_col: i64) -> Value {
        json!({
            "id": "n", "node_type": "function", "label": "f",
            "position": {
                "start_line": line, "start_col": col,
                "end_line": end_line, "end_col": end_col,
            },
            "structural_hash": "h", "children": [],
        })
    }

    #[test]
    fn codelens_covers_the_refactoring_family_and_skips_the_rest() {
        let diff = json!({
            "changes": [
                {"change_type": "REFACTORING", "description": "Rename f to g",
                 "refactoring_kind": "RENAME_SYMBOL", "new_node": node(2, 0, 5, 1)},
                {"change_type": "MOVE", "description": "Move block",
                 "refactoring_kind": null, "new_node": null, "old_node": node(7, 2, 9, 0)},
                {"change_type": "MODIFICATION", "description": "edited",
                 "new_node": node(1, 0, 1, 5)},
                {"change_type": "REORDER", "description": "swapped",
                 "new_node": null, "old_node": null},
            ],
        });
        let lenses = semantic_diff_to_codelens_value(&diff);
        assert_eq!(lenses.len(), 2);
        assert_eq!(
            lenses[0]["command"]["title"],
            "\u{21bb} RENAME_SYMBOL: Rename f to g"
        );
        assert_eq!(lenses[0]["range"]["start"], json!({"line": 2, "character": 0}));
        // No refactoring_kind → the change-type title glyph; old_node fallback anchors it.
        assert_eq!(lenses[1]["command"]["title"], "\u{27f3} MOVE: Move block");
        assert_eq!(lenses[1]["range"]["end"], json!({"line": 9, "character": 0}));
        assert_eq!(lenses[1]["command"]["command"], "");
    }

    #[test]
    fn diagnostics_map_style_only_hints_and_parse_error_warnings() {
        let diff = json!({
            "changes": [
                {"change_type": "STYLE_ONLY", "new_node": node(4, 0, 4, 10)},
                {"change_type": "STYLE_ONLY", "new_node": null},
                {"change_type": "MODIFICATION", "new_node": node(1, 0, 1, 2)},
            ],
            "parse_errors": ["unexpected token"],
        });
        let diags = semantic_diff_to_diagnostics_value(&diff);
        assert_eq!(diags.len(), 2);
        assert_eq!(diags[0]["severity"], 4);
        assert_eq!(diags[0]["message"], "Style-only change — no semantic impact");
        assert_eq!(diags[0]["source"], "intentdiff");
        assert_eq!(diags[1]["severity"], 2);
        assert_eq!(diags[1]["message"], "Parse error: unexpected token");
        assert_eq!(diags[1]["range"]["end"]["line"], 2_147_483_647i64);
    }

    #[test]
    fn empty_diff_yields_empty_lists() {
        let diff = json!({"changes": [], "parse_errors": []});
        assert!(semantic_diff_to_codelens_value(&diff).is_empty());
        assert!(semantic_diff_to_diagnostics_value(&diff).is_empty());
    }
}
