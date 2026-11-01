//! SemanticDiff → LSP shape mappings (#100 S3 slice 1), ported from
//! `src/intentumdiff/lsp_server/_codelens.py` and `_diagnostics.py`.
//!
//! These are the intentumdiff-HANDLER half of the future native LSP server (the generic
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
            "source": "intentumdiff",
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
            "source": "intentumdiff",
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


/// Convert a Rust parser byte-column into an LSP UTF-16 character offset.
///
/// `NodePosition` columns are UTF-8 byte offsets within their own
/// source lines, not Unicode scalar or UTF-16 offsets. The source is mandatory;
/// guessing from a node label would silently misplace non-ASCII annotations.
fn utf16_column_from_byte(source: &str, line: usize, column: usize) -> Result<usize, String> {
    // Preserve the empty final line after a trailing LF. CRLF's CR is a line
    // terminator, not a positionable LSP character.
    let raw_line = source.split('\n').nth(line)
        .ok_or_else(|| format!("LSP source line {line} is out of range"))?;
    let line_text = raw_line.strip_suffix('\r').unwrap_or(raw_line);
    if column > line_text.len() || !line_text.is_char_boundary(column) {
        return Err(format!(
            "invalid UTF-8 byte column {column} on source line {line}"
        ));
    }
    Ok(line_text[..column].encode_utf16().count())
}

fn required_source_coordinate(position: &Value, field: &str) -> Result<usize, String> {
    let raw = position.get(field).and_then(Value::as_u64)
        .ok_or_else(|| format!("missing/nonnegative LSP source position {field}"))?;
    usize::try_from(raw)
        .map_err(|_| format!("LSP source position {field} exceeds addressable size"))
}

/// Source-judged LSP range conversion owned by Rust. Refuse malformed spans
/// instead of allowing Python/VSIX to interpret parser bytes as UTF-16.
pub(crate) fn node_to_utf16_lsp_range_value(
    node: &Value,
    source: &str,
) -> Result<Value, String> {
    let pos = node.get("position").and_then(Value::as_object)
        .ok_or("semantic source node requires a position object")?;
    let position = Value::Object(pos.clone());
    let start_line = required_source_coordinate(&position, "start_line")?;
    let start_col = required_source_coordinate(&position, "start_col")?;
    let end_line = required_source_coordinate(&position, "end_line")?;
    let end_col = required_source_coordinate(&position, "end_col")?;
    if (end_line, end_col) < (start_line, start_col) {
        return Err("LSP source span ends before it starts".to_owned());
    }
    let start_character = utf16_column_from_byte(source, start_line, start_col)?;
    let end_character = utf16_column_from_byte(source, end_line, end_col)?;
    Ok(json!({
        "start": {"line": start_line, "character": start_character},
        "end": {"line": end_line, "character": end_character},
    }))
}

/// Stable Rust C ABI boundary for source-aware UTF-8 byte to UTF-16 LSP ranges.
pub(crate) fn utf16_lsp_range_json_impl(
    node_json: &str,
    source: &str,
) -> Result<String, String> {
    let node: Value = serde_json::from_str(node_json)
        .map_err(|e| format!("invalid LSP semantic node JSON: {e}"))?;
    node_to_utf16_lsp_range_value(&node, source).map(|range| range.to_string())
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
        assert_eq!(diags[0]["source"], "intentumdiff");
        assert_eq!(diags[1]["severity"], 2);
        assert_eq!(diags[1]["message"], "Parse error: unexpected token");
        assert_eq!(diags[1]["range"]["end"]["line"], 2_147_483_647i64);
    }


    #[test]
    fn utf16_ranges_are_source_judged_not_byte_or_codepoint_guesses() {
        let cases = [
            ("été\n", 0, 5, 3),
            ("Alïcë\n", 0, 7, 5),
            ("🦣\n", 0, 4, 2),
            ("e\u{301}\n", 0, 3, 2),
        ];
        for (source, line, byte_end, utf16_end) in cases {
            let result = node_to_utf16_lsp_range_value(
                &node(line, 0, line, byte_end), source,
            ).unwrap();
            assert_eq!(result["start"], json!({"line": line, "character": 0}));
            assert_eq!(result["end"], json!({"line": line, "character": utf16_end}));
        }
        // Different source lines, with CRLF and a surrogate pair in the first.
        let range = node_to_utf16_lsp_range_value(
            &node(0, 1, 1, 5), "a🦣\r\nété\r\n",
        ).unwrap();
        assert_eq!(range["start"], json!({"line": 0, "character": 1}));
        assert_eq!(range["end"], json!({"line": 1, "character": 3}));
    }

    #[test]
    fn utf16_ranges_fail_closed_on_interior_utf8_and_out_of_bounds_positions() {
        let source = "été\r\n";
        for invalid in [
            node(0, 1, 0, 5), // interior byte of U+00E9
            node(0, 0, 0, 6), // past logical line end / into CR
            node(2, 0, 2, 0), // beyond final LF
            node(0, 5, 0, 0), // backwards
            node(0, 0, 1, 1), // past trailing empty line
        ] {
            assert!(node_to_utf16_lsp_range_value(&invalid, source).is_err());
        }
        let bad = json!({"position": {"start_line": 0, "start_col": -1,
            "end_line": 0, "end_col": 1}});
        assert!(node_to_utf16_lsp_range_value(&bad, source).is_err());
        assert!(utf16_lsp_range_json_impl("not-json", source).is_err());
    }

    #[test]
    fn empty_diff_yields_empty_lists() {
        let diff = json!({"changes": [], "parse_errors": []});
        assert!(semantic_diff_to_codelens_value(&diff).is_empty());
        assert!(semantic_diff_to_diagnostics_value(&diff).is_empty());
    }
}
