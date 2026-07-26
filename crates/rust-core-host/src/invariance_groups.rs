//! Invariance equivalence groups (integer/string canonical values, quote and
//! formatting equivalence) and the apply-invariances entry values, extracted
//! from lib.rs verbatim (issue #29 monolith split, phase B).

use crate::*;

pub(crate) fn rust_apply_invariances_value(request: &Value) -> Result<Value, String> {
    match request
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("apply")
    {
        "apply" => rust_apply_review_invariances(request),
        "style_only" => rust_build_style_only_evidence(request),
        "zero_change_literal" => rust_build_zero_change_literal_evidence(request),
        other => Err(format!("unknown invariance mode: {other}")),
    }
}

pub(crate) fn rust_apply_review_invariances(request: &Value) -> Result<Value, String> {
    let language = request
        .get("language")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let old_source = request
        .get("old_source")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let new_source = request
        .get("new_source")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let old_tree: SemanticNode = serde_json::from_value(
        request
            .get("old_tree")
            .cloned()
            .ok_or_else(|| "old_tree is required".to_owned())?,
    )
    .map_err(|exc| format!("old_tree: {exc}"))?;
    let new_tree: SemanticNode = serde_json::from_value(
        request
            .get("new_tree")
            .cloned()
            .ok_or_else(|| "new_tree is required".to_owned())?,
    )
    .map_err(|exc| format!("new_tree: {exc}"))?;
    let mut changes = request
        .get("changes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut change_groups = Vec::new();
    let mut ignored_style_changes = Vec::new();

    if language == "css" {
        if let Some((groups, ignored)) =
            rust_css_color_equivalence(&changes, &old_tree, &new_tree, old_source, new_source)
        {
            changes.clear();
            change_groups.extend(groups);
            ignored_style_changes.extend(ignored);
        }
    }

    if !changes.is_empty() && rust_literal_invariance_language(language) {
        let (kept, groups, ignored) =
            rust_apply_literal_equivalences(changes, old_source, new_source, language);
        changes = kept;
        change_groups.extend(groups);
        ignored_style_changes.extend(ignored);
    }

    Ok(json!({
        "changes": changes,
        "change_groups": change_groups,
        "ignored_style_changes": ignored_style_changes,
    }))
}

pub(crate) fn rust_build_style_only_evidence(request: &Value) -> Result<Value, String> {
    let language = request
        .get("language")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let old_source = request
        .get("old_source")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let new_source = request
        .get("new_source")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if old_source == new_source {
        return Ok(rust_empty_invariance_result());
    }

    let mut groups = Vec::new();
    let mut ignored = Vec::new();
    let mut literal_spans = Vec::new();
    if rust_literal_invariance_language(language) {
        for (occurrence, item) in
            rust_source_string_literal_evidence(old_source, new_source, language)
                .into_iter()
                .enumerate()
        {
            literal_spans.push((item.old_span, item.new_span));
            let (group, ignored_item) = rust_source_literal_group(
                &item,
                "core.string_literal.decoded_value.safe",
                "Both string literal spellings decode to the same plain string value.",
                "canonical_value_equivalence",
                "amber",
                language,
                occurrence,
                "style_only_shortcut",
                0.95,
            );
            groups.push(group);
            ignored.push(ignored_item);
        }
    }

    for (occurrence, evidence) in rust_changed_source_evidence(old_source, new_source)
        .into_iter()
        .filter(|item| !rust_evidence_covered_by_literal(item, &literal_spans))
        .enumerate()
    {
        let (group, ignored_item) = rust_source_group(
            &evidence,
            "generic.style_only_shortcut.source_equivalence",
            "The parser-normalized tree is unchanged after trivia filtering, so source-only layout or comment edits were ignored while preserving source-span evidence.",
            "syntactic_trivia",
            "amber",
            language,
            occurrence,
            "style_only_shortcut",
            0.75,
            None,
        );
        groups.push(group);
        ignored.push(ignored_item);
    }

    Ok(json!({
        "changes": [],
        "change_groups": groups,
        "ignored_style_changes": ignored,
    }))
}

pub(crate) fn rust_build_zero_change_literal_evidence(request: &Value) -> Result<Value, String> {
    let language = request
        .get("language")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !rust_literal_invariance_language(language) {
        return Ok(rust_empty_invariance_result());
    }
    let old_source = request
        .get("old_source")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let new_source = request
        .get("new_source")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if old_source == new_source {
        return Ok(rust_empty_invariance_result());
    }
    let old_tree: SemanticNode = serde_json::from_value(
        request
            .get("old_tree")
            .cloned()
            .ok_or_else(|| "old_tree is required".to_owned())?,
    )
    .map_err(|exc| format!("old_tree: {exc}"))?;
    let new_tree: SemanticNode = serde_json::from_value(
        request
            .get("new_tree")
            .cloned()
            .ok_or_else(|| "new_tree is required".to_owned())?,
    )
    .map_err(|exc| format!("new_tree: {exc}"))?;
    let mut groups = Vec::new();
    let mut ignored = Vec::new();
    for (occurrence, item) in
        rust_tree_string_literal_evidence(&old_tree, &new_tree, old_source, new_source, language)
            .into_iter()
            .enumerate()
    {
        let (group, ignored_item) = rust_source_literal_group(
            &item,
            "core.string_literal.decoded_value.safe",
            "Both string literal spellings decode to the same plain string value.",
            "canonical_value_equivalence",
            "amber",
            language,
            occurrence,
            "zero_change_source_equivalence",
            0.95,
        );
        groups.push(group);
        ignored.push(ignored_item);
    }
    Ok(json!({
        "changes": [],
        "change_groups": groups,
        "ignored_style_changes": ignored,
    }))
}

pub(crate) fn rust_empty_invariance_result() -> Value {
    json!({
        "changes": [],
        "change_groups": [],
        "ignored_style_changes": [],
    })
}

pub(crate) fn rust_literal_invariance_language(language: &str) -> bool {
    matches!(
        language,
        "python" | "javascript" | "typescript" | "tsx" | "csharp"
    )
}

pub(crate) fn rust_apply_literal_equivalences(
    changes: Vec<Value>,
    old_source: &str,
    new_source: &str,
    language: &str,
) -> (Vec<Value>, Vec<Value>, Vec<Value>) {
    let mut kept = Vec::new();
    let mut groups = Vec::new();
    let mut ignored = Vec::new();
    for (idx, change) in changes.into_iter().enumerate() {
        if let Some((group, item)) = rust_integer_literal_equivalence_group(idx, &change, language)
        {
            groups.push(group);
            ignored.push(item);
            continue;
        }
        if let Some((group, item)) =
            rust_string_literal_equivalence_group(idx, &change, old_source, new_source, language)
        {
            groups.push(group);
            ignored.push(item);
            continue;
        }
        kept.push(change);
    }
    (kept, groups, ignored)
}

pub(crate) fn rust_integer_literal_equivalence_group(
    raw_index: usize,
    change: &Value,
    language: &str,
) -> Option<(Value, Value)> {
    if rust_change_type(change) != "MODIFICATION" {
        return None;
    }
    let (old_node, new_node) = rust_first_literal_pair(change, "integer")
        .or_else(|| rust_first_literal_pair(change, "integer_literal"))
        .or_else(|| rust_first_literal_pair(change, "numeric_literal"))?;
    if !(old_node.is_leaf() && new_node.is_leaf()) {
        return None;
    }
    let old_canonical = canonical_integer_literal_for_invariance(&old_node.label)?;
    let new_canonical = canonical_integer_literal_for_invariance(&new_node.label)?;
    if old_canonical != new_canonical || old_node.label == new_node.label {
        return None;
    }
    let metadata = json!({
        "index_space": "invariance_input",
        "reason": "Both integer literal spellings parse to the same exact integer value.",
        "equivalence_kind": "canonical_value_equivalence",
        "canonical_old": format!("int({old_canonical})"),
        "canonical_new": format!("int({new_canonical})"),
        "old_label": old_node.label,
        "new_label": new_node.label,
        "old_node_type": old_node.node_type,
        "new_node_type": new_node.node_type,
        "risk": "green",
        "language": language,
    });
    let group = json!({
        "kind": "IGNORED_STYLE",
        "raw_change_indices": [raw_index],
        "old_labels": [old_node.label],
        "new_labels": [new_node.label],
        "old_node_ids": [old_node.id],
        "new_node_ids": [new_node.id],
        "confidence": 1.0,
        "rule_id": "core.integer_literal.canonical_value.safe",
        "metadata": metadata.clone(),
    });
    let mut ignored = metadata;
    ignored["rule_id"] = json!("core.integer_literal.canonical_value.safe");
    Some((group, ignored))
}

pub(crate) fn rust_string_literal_equivalence_group(
    raw_index: usize,
    change: &Value,
    old_source: &str,
    new_source: &str,
    language: &str,
) -> Option<(Value, Value)> {
    if rust_change_type(change) != "MODIFICATION" {
        return None;
    }
    let (old_node, new_node) = rust_first_literal_pair(change, "string")
        .or_else(|| rust_first_literal_pair(change, "string_literal"))
        .or_else(|| rust_first_literal_pair(change, "character_literal"))?;
    if !(old_node.is_leaf() && new_node.is_leaf()) {
        return None;
    }
    let (old_start, old_end) = rust_source_span_offsets(old_source, &old_node.position)?;
    let (new_start, new_end) = rust_source_span_offsets(new_source, &new_node.position)?;
    let old_raw = old_source.get(old_start..old_end)?.trim();
    let new_raw = new_source.get(new_start..new_end)?.trim();
    if old_raw == new_raw {
        return None;
    }
    let old_decoded = decode_invariance_string_literal(old_raw, language)?;
    let new_decoded = decode_invariance_string_literal(new_raw, language)?;
    if old_decoded != new_decoded {
        return None;
    }
    let evidence = RustSourceLiteralEvidence {
        old_label: old_raw.to_owned(),
        new_label: new_raw.to_owned(),
        canonical: format!("string({old_decoded})"),
        old_span: (old_start, old_end),
        new_span: (new_start, new_end),
    };
    let (mut group, ignored) = rust_source_literal_group(
        &evidence,
        "core.string_literal.decoded_value.safe",
        "Both string literal spellings decode to the same plain string value.",
        "canonical_value_equivalence",
        "amber",
        language,
        0,
        "invariance_input",
        0.95,
    );
    group["raw_change_indices"] = json!([raw_index]);
    group["old_node_ids"] = json!([old_node.id]);
    group["new_node_ids"] = json!([new_node.id]);
    group["old_labels"] = json!([old_raw]);
    group["new_labels"] = json!([new_raw]);
    group["metadata"]["old_node_type"] = json!(old_node.node_type);
    group["metadata"]["new_node_type"] = json!(new_node.node_type);
    Some((group, ignored))
}

pub(crate) fn rust_first_literal_pair(
    change: &Value,
    node_type: &str,
) -> Option<(SemanticNode, SemanticNode)> {
    let old_node = rust_change_node(change, "old_node")?;
    let new_node = rust_change_node(change, "new_node")?;
    if rust_node_matches_literal_type(&old_node.node_type, node_type)
        && rust_node_matches_literal_type(&new_node.node_type, node_type)
    {
        return Some((old_node, new_node));
    }
    let new_by_id = rust_owned_nodes_by_id(&new_node);
    for old_descendant in old_node.descendants() {
        if !rust_node_matches_literal_type(&old_descendant.node_type, node_type) {
            continue;
        }
        if let Some(new_descendant) = new_by_id.get(old_descendant.id.as_str()) {
            if rust_node_matches_literal_type(&new_descendant.node_type, node_type) {
                return Some((old_descendant.clone(), new_descendant.clone()));
            }
        }
    }
    None
}

pub(crate) fn rust_node_matches_literal_type(actual: &str, expected: &str) -> bool {
    actual == expected
        || (expected == "integer" && (actual.contains("integer") || actual == "numeric_literal"))
        || (expected == "string"
            && (actual.contains("string")
                || actual == "character_literal"
                || actual == "char_literal"))
}

pub(crate) fn rust_change_type(change: &Value) -> &str {
    change
        .get("change_type")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

pub(crate) fn rust_change_node(change: &Value, key: &str) -> Option<SemanticNode> {
    serde_json::from_value(change.get(key)?.clone()).ok()
}

pub(crate) fn rust_owned_nodes_by_id(root: &SemanticNode) -> HashMap<String, SemanticNode> {
    let mut result = HashMap::new();
    result.insert(root.id.clone(), root.clone());
    for node in root.descendants() {
        result.insert(node.id.clone(), node.clone());
    }
    result
}

pub(crate) fn canonical_integer_literal_for_invariance(label: &str) -> Option<String> {
    let mut stripped = label.trim();
    let mut sign = "";
    if let Some(rest) = stripped.strip_prefix('+') {
        stripped = rest;
    } else if let Some(rest) = stripped.strip_prefix('-') {
        sign = "-";
        stripped = rest;
    }
    let normalized = stripped.replace('_', "");
    if normalized.is_empty() {
        return None;
    }
    let (digits, radix) = if let Some(rest) = normalized
        .strip_prefix("0x")
        .or_else(|| normalized.strip_prefix("0X"))
    {
        (rest, 16)
    } else if let Some(rest) = normalized
        .strip_prefix("0o")
        .or_else(|| normalized.strip_prefix("0O"))
    {
        (rest, 8)
    } else if let Some(rest) = normalized
        .strip_prefix("0b")
        .or_else(|| normalized.strip_prefix("0B"))
    {
        (rest, 2)
    } else {
        (normalized.as_str(), 10)
    };
    if digits.is_empty() {
        return None;
    }
    let value = i128::from_str_radix(digits, radix).ok()?;
    Some(format!("{sign}{value}"))
}

pub(crate) fn decode_invariance_string_literal(raw: &str, language: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.len() < 2 || trimmed.contains('`') {
        return None;
    }
    let quote_start = trimmed
        .char_indices()
        .find_map(|(idx, ch)| matches!(ch, '\'' | '"').then_some(idx))?;
    let prefix = &trimmed[..quote_start];
    if language == "python" {
        if prefix
            .chars()
            .any(|ch| matches!(ch, 'f' | 'F' | 'b' | 'B' | 'r' | 'R'))
        {
            return None;
        }
    } else if matches!(language, "javascript" | "typescript" | "tsx" | "csharp") {
        if !prefix.is_empty() || trimmed.starts_with(['@', '$']) {
            return None;
        }
    } else {
        return None;
    }
    let literal = &trimmed[quote_start..];
    let first = literal.chars().next()?;
    let last = literal.chars().last()?;
    if !matches!(first, '\'' | '"') || first != last {
        return None;
    }
    let inner = &literal[first.len_utf8()..literal.len() - last.len_utf8()];
    decode_basic_string_escapes(inner, first)
}

pub(crate) fn decode_basic_string_escapes(inner: &str, quote: char) -> Option<String> {
    let mut result = String::new();
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch == quote {
            return None;
        }
        if ch != '\\' {
            result.push(ch);
            continue;
        }
        let escaped = chars.next()?;
        match escaped {
            '\\' => result.push('\\'),
            '\'' => result.push('\''),
            '"' => result.push('"'),
            'n' => result.push('\n'),
            'r' => result.push('\r'),
            't' => result.push('\t'),
            '0' => result.push('\0'),
            _ => return None,
        }
    }
    Some(result)
}

pub(crate) fn rust_tree_string_literal_evidence(
    old_tree: &SemanticNode,
    new_tree: &SemanticNode,
    old_source: &str,
    new_source: &str,
    language: &str,
) -> Vec<RustSourceLiteralEvidence> {
    let new_by_id = rust_owned_nodes_by_id(new_tree);
    let mut result = Vec::new();
    for old_node in std::iter::once(old_tree).chain(old_tree.descendants().into_iter()) {
        if !rust_node_matches_literal_type(&old_node.node_type, "string") {
            continue;
        }
        let Some(new_node) = new_by_id.get(old_node.id.as_str()) else {
            continue;
        };
        if !rust_node_matches_literal_type(&new_node.node_type, "string") {
            continue;
        }
        let Some((old_start, old_end)) = rust_source_span_offsets(old_source, &old_node.position)
        else {
            continue;
        };
        let Some((new_start, new_end)) = rust_source_span_offsets(new_source, &new_node.position)
        else {
            continue;
        };
        let Some(old_raw) = old_source.get(old_start..old_end) else {
            continue;
        };
        let Some(new_raw) = new_source.get(new_start..new_end) else {
            continue;
        };
        if old_raw == new_raw {
            continue;
        }
        let Some(old_decoded) = decode_invariance_string_literal(old_raw, language) else {
            continue;
        };
        let Some(new_decoded) = decode_invariance_string_literal(new_raw, language) else {
            continue;
        };
        if old_decoded != new_decoded {
            continue;
        }
        result.push(RustSourceLiteralEvidence {
            old_label: old_raw.to_owned(),
            new_label: new_raw.to_owned(),
            canonical: format!("string({old_decoded})"),
            old_span: (old_start, old_end),
            new_span: (new_start, new_end),
        });
    }
    result
}

pub(crate) fn rust_source_string_literal_evidence(
    old_source: &str,
    new_source: &str,
    language: &str,
) -> Vec<RustSourceLiteralEvidence> {
    let old_spans = rust_scan_string_literals(old_source, language);
    let new_spans = rust_scan_string_literals(new_source, language);
    if old_spans.is_empty() || old_spans.len() != new_spans.len() {
        return Vec::new();
    }
    old_spans
        .into_iter()
        .zip(new_spans)
        .filter_map(
            |((old_label, old_span, old_decoded), (new_label, new_span, new_decoded))| {
                if old_label == new_label || old_decoded != new_decoded {
                    return None;
                }
                Some(RustSourceLiteralEvidence {
                    old_label,
                    new_label,
                    canonical: format!("string({old_decoded})"),
                    old_span,
                    new_span,
                })
            },
        )
        .collect()
}

pub(crate) fn rust_scan_string_literals(
    source: &str,
    language: &str,
) -> Vec<(String, (usize, usize), String)> {
    let bytes = source.as_bytes();
    let mut result = Vec::new();
    let mut idx = 0usize;
    while idx < bytes.len() {
        let ch = bytes[idx] as char;
        if !matches!(ch, '\'' | '"') {
            idx += 1;
            continue;
        }
        let start = idx;
        idx += 1;
        let mut escaped = false;
        while idx < bytes.len() {
            let current = bytes[idx] as char;
            idx += 1;
            if escaped {
                escaped = false;
                continue;
            }
            if current == '\\' {
                escaped = true;
                continue;
            }
            if current == ch {
                let end = idx;
                if let Some(raw) = source.get(start..end) {
                    if let Some(decoded) = decode_invariance_string_literal(raw, language) {
                        result.push((raw.to_owned(), (start, end), decoded));
                    }
                }
                break;
            }
        }
    }
    result
}

pub(crate) fn rust_changed_source_evidence(old_source: &str, new_source: &str) -> Vec<RustSourceEvidence> {
    let old_lines = rust_line_ranges(old_source);
    let new_lines = rust_line_ranges(new_source);
    if old_lines.len() > 512 || new_lines.len() > 512 {
        return rust_single_changed_source_evidence(old_source, new_source);
    }
    let n = old_lines.len();
    let m = new_lines.len();
    let mut dp = vec![0usize; (n + 1) * (m + 1)];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            let index = i * (m + 1) + j;
            dp[index] = if old_lines[i].0 == new_lines[j].0 {
                1 + dp[(i + 1) * (m + 1) + j + 1]
            } else {
                dp[(i + 1) * (m + 1) + j].max(dp[i * (m + 1) + j + 1])
            };
        }
    }
    let mut evidence = Vec::new();
    let mut i = 0usize;
    let mut j = 0usize;
    while i < n || j < m {
        if i < n && j < m && old_lines[i].0 == new_lines[j].0 {
            i += 1;
            j += 1;
            continue;
        }
        let old_start_index = i;
        let new_start_index = j;
        while i < n || j < m {
            if i < n && j < m && old_lines[i].0 == new_lines[j].0 {
                break;
            }
            if j < m && (i == n || dp[i * (m + 1) + j + 1] >= dp[(i + 1) * (m + 1) + j]) {
                j += 1;
            } else if i < n {
                i += 1;
            } else {
                break;
            }
        }
        evidence.push(RustSourceEvidence {
            old_label: rust_source_label(rust_span_text(
                old_source,
                &old_lines,
                old_start_index,
                i,
            )),
            new_label: rust_source_label(rust_span_text(
                new_source,
                &new_lines,
                new_start_index,
                j,
            )),
            old_span: rust_line_span(old_source, &old_lines, old_start_index, i),
            new_span: rust_line_span(new_source, &new_lines, new_start_index, j),
        });
    }
    evidence
}

pub(crate) fn rust_line_ranges(source: &str) -> Vec<(&str, usize, usize)> {
    let mut result = Vec::new();
    let mut start = 0usize;
    for part in source.split_inclusive('\n') {
        let end = start + part.len();
        result.push((part, start, end));
        start = end;
    }
    if start < source.len() {
        result.push((&source[start..], start, source.len()));
    }
    result
}

pub(crate) fn rust_line_span(
    source: &str,
    lines: &[(&str, usize, usize)],
    start_index: usize,
    end_index: usize,
) -> (usize, usize) {
    let start = lines
        .get(start_index)
        .map(|line| line.1)
        .unwrap_or(source.len());
    let end = if end_index > start_index {
        lines
            .get(end_index.saturating_sub(1))
            .map(|line| line.2)
            .unwrap_or(start)
    } else {
        start
    };
    (start, end)
}

pub(crate) fn rust_span_text<'a>(
    source: &'a str,
    lines: &[(&str, usize, usize)],
    start_index: usize,
    end_index: usize,
) -> &'a str {
    let (start, end) = rust_line_span(source, lines, start_index, end_index);
    source.get(start..end).unwrap_or_default()
}

pub(crate) fn rust_single_changed_source_evidence(
    old_source: &str,
    new_source: &str,
) -> Vec<RustSourceEvidence> {
    if old_source == new_source {
        return Vec::new();
    }
    let mut prefix = 0usize;
    for (old_ch, new_ch) in old_source.chars().zip(new_source.chars()) {
        if old_ch != new_ch {
            break;
        }
        prefix += old_ch.len_utf8();
    }
    let mut old_suffix = old_source.len();
    let mut new_suffix = new_source.len();
    while old_suffix > prefix && new_suffix > prefix {
        let old_ch = old_source[..old_suffix].chars().next_back().unwrap();
        let new_ch = new_source[..new_suffix].chars().next_back().unwrap();
        if old_ch != new_ch {
            break;
        }
        old_suffix -= old_ch.len_utf8();
        new_suffix -= new_ch.len_utf8();
    }
    vec![RustSourceEvidence {
        old_label: rust_source_label(&old_source[prefix..old_suffix]),
        new_label: rust_source_label(&new_source[prefix..new_suffix]),
        old_span: (prefix, old_suffix),
        new_span: (prefix, new_suffix),
    }]
}

pub(crate) fn rust_source_label(value: &str) -> String {
    if value.is_empty() {
        return "<empty>".to_owned();
    }
    if value.trim().is_empty() {
        return "<whitespace>".to_owned();
    }
    let label = value
        .replace('\r', "\\r")
        .replace('\n', "\\n")
        .trim()
        .to_owned();
    if label.len() <= 80 {
        label
    } else {
        let truncated: String = label.chars().take(77).collect();
        format!("{truncated}...")
    }
}

pub(crate) fn rust_evidence_covered_by_literal(
    evidence: &RustSourceEvidence,
    literal_spans: &[((usize, usize), (usize, usize))],
) -> bool {
    literal_spans.iter().any(|(old_span, new_span)| {
        rust_span_within(evidence.old_span, *old_span)
            && rust_span_within(evidence.new_span, *new_span)
    })
}

pub(crate) fn rust_span_within(inner: (usize, usize), outer: (usize, usize)) -> bool {
    outer.0 <= inner.0 && inner.1 <= outer.1
}

pub(crate) fn rust_source_group(
    evidence: &RustSourceEvidence,
    rule_id: &str,
    reason: &str,
    equivalence_kind: &str,
    risk: &str,
    language: &str,
    occurrence: usize,
    index_space: &str,
    confidence: f64,
    canonical: Option<&str>,
) -> (Value, Value) {
    let mut metadata = json!({
        "index_space": index_space,
        "reason": reason,
        "equivalence_kind": equivalence_kind,
        "old_label": evidence.old_label,
        "new_label": evidence.new_label,
        "risk": risk,
        "language": language,
        "occurrence": occurrence,
        "evidence_depth": "source_span",
        "old_span": [evidence.old_span.0, evidence.old_span.1],
        "new_span": [evidence.new_span.0, evidence.new_span.1],
    });
    if let Some(canonical) = canonical {
        metadata["canonical_old"] = json!(canonical);
        metadata["canonical_new"] = json!(canonical);
    }
    let group = json!({
        "kind": "IGNORED_STYLE",
        "raw_change_indices": [],
        "old_labels": [evidence.old_label],
        "new_labels": [evidence.new_label],
        "old_node_ids": [],
        "new_node_ids": [],
        "confidence": confidence,
        "rule_id": rule_id,
        "metadata": metadata.clone(),
    });
    let mut ignored = metadata.clone();
    ignored["rule_id"] = json!(rule_id);
    (group, ignored)
}

pub(crate) fn rust_source_literal_group(
    evidence: &RustSourceLiteralEvidence,
    rule_id: &str,
    reason: &str,
    equivalence_kind: &str,
    risk: &str,
    language: &str,
    occurrence: usize,
    index_space: &str,
    confidence: f64,
) -> (Value, Value) {
    let base = RustSourceEvidence {
        old_label: evidence.old_label.clone(),
        new_label: evidence.new_label.clone(),
        old_span: evidence.old_span,
        new_span: evidence.new_span,
    };
    rust_source_group(
        &base,
        rule_id,
        reason,
        equivalence_kind,
        risk,
        language,
        occurrence,
        index_space,
        confidence,
        Some(&evidence.canonical),
    )
}

pub(crate) fn rust_css_color_equivalence(
    changes: &[Value],
    old_tree: &SemanticNode,
    new_tree: &SemanticNode,
    old_source: &str,
    new_source: &str,
) -> Option<(Vec<Value>, Vec<Value>)> {
    if changes.is_empty() {
        return None;
    }
    let (old_normalized, old_tokens) = rust_canonicalize_css_colors(old_source);
    let (new_normalized, new_tokens) = rust_canonicalize_css_colors(new_source);
    if old_normalized != new_normalized {
        return None;
    }
    let evidence = rust_changed_color_evidence(&old_tokens, &new_tokens);
    if evidence.is_empty() {
        return None;
    }
    let indices: Vec<usize> = (0..changes.len()).collect();
    let old_labels = rust_labels_from_change_values(changes, "old_node");
    let new_labels = rust_labels_from_change_values(changes, "new_node");
    let old_node_ids = rust_node_ids_from_change_values(changes, "old_node")
        .unwrap_or_else(|| rust_all_node_ids_with_root(old_tree));
    let new_node_ids = rust_node_ids_from_change_values(changes, "new_node")
        .unwrap_or_else(|| rust_all_node_ids_with_root(new_tree));
    let mut groups = Vec::new();
    let mut ignored = Vec::new();
    for (occurrence, item) in evidence.into_iter().enumerate() {
        let metadata = json!({
            "index_space": "invariance_input",
            "reason": "Both CSS color spellings resolve to the same canonical sRGB color.",
            "equivalence_kind": "canonical_value_equivalence",
            "canonical_old": item.canonical.clone(),
            "canonical_new": item.canonical.clone(),
            "old_label": item.old_label.clone(),
            "new_label": item.new_label.clone(),
            "risk": "green",
            "language": "css",
            "occurrence": occurrence,
            "old_span": [item.old_span.0, item.old_span.1],
            "new_span": [item.new_span.0, item.new_span.1],
        });
        groups.push(json!({
            "kind": "IGNORED_STYLE",
            "raw_change_indices": indices.clone(),
            "old_labels": rust_unique_strings([old_labels.clone(), vec![item.old_label.clone()]].concat()),
            "new_labels": rust_unique_strings([new_labels.clone(), vec![item.new_label.clone()]].concat()),
            "old_node_ids": old_node_ids.clone(),
            "new_node_ids": new_node_ids.clone(),
            "confidence": 1.0,
            "rule_id": "css.color.canonical_equivalence",
            "metadata": metadata.clone(),
        }));
        let mut ignored_item = metadata;
        ignored_item["rule_id"] = json!("css.color.canonical_equivalence");
        ignored.push(ignored_item);
    }
    Some((groups, ignored))
}

pub(crate) fn rust_canonicalize_css_colors(source: &str) -> (String, Vec<(String, String, (usize, usize))>) {
    let mut tokens = Vec::new();
    let mut output = String::with_capacity(source.len());
    let bytes = source.as_bytes();
    let mut idx = 0usize;
    while idx < bytes.len() {
        if bytes[idx] == b'#' {
            if let Some((label, canonical, end)) = rust_parse_hex_color(source, idx) {
                output.push_str(&canonical);
                tokens.push((label, canonical, (idx, end)));
                idx = end;
                continue;
            }
        }
        if rust_starts_with_ascii_case_insensitive(source, idx, "rgb(") {
            if let Some((label, canonical, end)) = rust_parse_rgb_color(source, idx) {
                output.push_str(&canonical);
                tokens.push((label, canonical, (idx, end)));
                idx = end;
                continue;
            }
        }
        let ch = source[idx..].chars().next().unwrap();
        if ch.is_ascii_alphabetic() {
            let end = rust_ascii_word_end(source, idx);
            let word = &source[idx..end];
            if let Some((red, green, blue)) = rust_css_named_color(word) {
                let canonical = rust_format_srgb(red, green, blue);
                output.push_str(&canonical);
                tokens.push((word.to_owned(), canonical, (idx, end)));
                idx = end;
                continue;
            }
        }
        output.push(ch);
        idx += ch.len_utf8();
    }
    (output, tokens)
}

pub(crate) fn rust_parse_hex_color(source: &str, start: usize) -> Option<(String, String, usize)> {
    let raw = source.get(start + 1..)?;
    let hex_len = raw
        .chars()
        .take_while(|ch| ch.is_ascii_hexdigit())
        .map(char::len_utf8)
        .sum::<usize>();
    if hex_len != 3 && hex_len != 6 {
        return None;
    }
    let end = start + 1 + hex_len;
    if source
        .get(end..)
        .and_then(|tail| tail.chars().next())
        .is_some_and(|ch| ch.is_ascii_hexdigit())
    {
        return None;
    }
    let label = source[start..end].to_owned();
    let mut digits = source[start + 1..end].to_owned();
    if digits.len() == 3 {
        digits = digits.chars().flat_map(|ch| [ch, ch]).collect();
    }
    let red = u8::from_str_radix(&digits[0..2], 16).ok()?;
    let green = u8::from_str_radix(&digits[2..4], 16).ok()?;
    let blue = u8::from_str_radix(&digits[4..6], 16).ok()?;
    Some((label, rust_format_srgb(red, green, blue), end))
}

pub(crate) fn rust_parse_rgb_color(source: &str, start: usize) -> Option<(String, String, usize)> {
    let close = source.get(start..)?.find(')')? + start;
    let label = source[start..=close].to_owned();
    let inner = &source[start + 4..close];
    let parts: Vec<&str> = if inner.contains(',') {
        inner.split(',').map(str::trim).collect()
    } else {
        inner.split_whitespace().collect()
    };
    if parts.len() != 3 {
        return None;
    }
    let red = rust_parse_css_channel(parts[0])?;
    let green = rust_parse_css_channel(parts[1])?;
    let blue = rust_parse_css_channel(parts[2])?;
    Some((label, rust_format_srgb(red, green, blue), close + 1))
}

pub(crate) fn rust_parse_css_channel(value: &str) -> Option<u8> {
    let channel = value.parse::<u16>().ok()?;
    u8::try_from(channel).ok()
}

pub(crate) fn rust_starts_with_ascii_case_insensitive(source: &str, start: usize, needle: &str) -> bool {
    source
        .get(start..start.saturating_add(needle.len()))
        .is_some_and(|value| value.eq_ignore_ascii_case(needle))
}

pub(crate) fn rust_ascii_word_end(source: &str, start: usize) -> usize {
    let mut end = start;
    for ch in source[start..].chars() {
        if !ch.is_ascii_alphabetic() {
            break;
        }
        end += ch.len_utf8();
    }
    end
}

pub(crate) fn rust_css_named_color(name: &str) -> Option<(u8, u8, u8)> {
    match name.to_ascii_lowercase().as_str() {
        "black" => Some((0, 0, 0)),
        "blue" => Some((0, 0, 255)),
        "cyan" => Some((0, 255, 255)),
        "fuchsia" => Some((255, 0, 255)),
        "gray" => Some((128, 128, 128)),
        "green" => Some((0, 128, 0)),
        "grey" => Some((128, 128, 128)),
        "lime" => Some((0, 255, 0)),
        "magenta" => Some((255, 0, 255)),
        "red" => Some((255, 0, 0)),
        "white" => Some((255, 255, 255)),
        "yellow" => Some((255, 255, 0)),
        _ => None,
    }
}

pub(crate) fn rust_format_srgb(red: u8, green: u8, blue: u8) -> String {
    format!("srgb({red},{green},{blue},1)")
}

pub(crate) fn rust_changed_color_evidence(
    old_tokens: &[(String, String, (usize, usize))],
    new_tokens: &[(String, String, (usize, usize))],
) -> Vec<RustColorEvidence> {
    if old_tokens.len() != new_tokens.len() {
        return Vec::new();
    }
    let mut result = Vec::new();
    for (old, new) in old_tokens.iter().zip(new_tokens) {
        if old.1 != new.1 {
            return Vec::new();
        }
        if old.0 != new.0 {
            result.push(RustColorEvidence {
                old_label: old.0.clone(),
                new_label: new.0.clone(),
                canonical: old.1.clone(),
                old_span: old.2,
                new_span: new.2,
            });
        }
    }
    result
}

pub(crate) fn rust_labels_from_change_values(changes: &[Value], key: &str) -> Vec<String> {
    rust_unique_strings(
        changes
            .iter()
            .filter_map(|change| rust_change_node(change, key))
            .flat_map(|node| {
                let mut labels = Vec::new();
                rust_collect_labels(&node, &mut labels);
                labels
            })
            .collect(),
    )
}

pub(crate) fn rust_node_ids_from_change_values(changes: &[Value], key: &str) -> Option<Vec<String>> {
    let ids = rust_unique_strings(
        changes
            .iter()
            .filter_map(|change| rust_change_node(change, key))
            .flat_map(|node| {
                let mut ids = Vec::new();
                rust_collect_node_ids(&node, &mut ids);
                ids
            })
            .collect(),
    );
    (!ids.is_empty()).then_some(ids)
}

pub(crate) fn rust_all_node_ids_with_root(root: &SemanticNode) -> Vec<String> {
    let mut ids = Vec::new();
    rust_collect_node_ids(root, &mut ids);
    ids
}

pub(crate) fn rust_collect_labels(node: &SemanticNode, labels: &mut Vec<String>) {
    if !node.label.is_empty() {
        labels.push(node.label.clone());
    }
    for child in &node.children {
        rust_collect_labels(child, labels);
    }
}

pub(crate) fn rust_collect_node_ids(node: &SemanticNode, ids: &mut Vec<String>) {
    ids.push(node.id.clone());
    for child in &node.children {
        rust_collect_node_ids(child, ids);
    }
}

pub(crate) fn rust_unique_strings(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for value in values {
        if seen.insert(value.clone()) {
            result.push(value);
        }
    }
    result
}

pub(crate) fn rust_source_span_offsets(source: &str, position: &NodePosition) -> Option<(usize, usize)> {
    let offsets = rust_line_offsets(source);
    let start = rust_offset_from_line_col(&offsets, position.start_line, position.start_col)?;
    let end = rust_offset_from_line_col(&offsets, position.end_line, position.end_col)?;
    (start <= end && end <= source.len()).then_some((start, end))
}

pub(crate) fn rust_line_offsets(source: &str) -> Vec<usize> {
    let mut offsets = vec![0usize];
    for (idx, ch) in source.char_indices() {
        if ch == '\n' {
            offsets.push(idx + ch.len_utf8());
        }
    }
    offsets
}

pub(crate) fn rust_offset_from_line_col(offsets: &[usize], line: u32, col: u32) -> Option<usize> {
    offsets
        .get(line as usize)
        .map(|line_start| line_start + col as usize)
}

pub(crate) fn python_formatting_equivalence_group(changes: &[ChangeDraft<'_>]) -> Value {
    let old_ids: Vec<String> = changes
        .iter()
        .filter_map(|change| change.old_node.map(|node| node.id.clone()))
        .take(8)
        .collect();
    let new_ids: Vec<String> = changes
        .iter()
        .filter_map(|change| change.new_node.map(|node| node.id.clone()))
        .take(8)
        .collect();
    let old_labels: Vec<String> = changes
        .iter()
        .flat_map(|change| node_labels(change.old_node))
        .filter(|label| !label.is_empty())
        .take(24)
        .collect();
    let new_labels: Vec<String> = changes
        .iter()
        .flat_map(|change| node_labels(change.new_node))
        .filter(|label| !label.is_empty())
        .take(24)
        .collect();
    json!({
        "kind": "IGNORED_STYLE",
        "raw_change_indices": [],
        "old_labels": old_labels,
        "new_labels": new_labels,
        "old_node_ids": old_ids,
        "new_node_ids": new_ids,
        "confidence": 0.85,
        "rule_id": "python.formatting.call_wrapping_equivalence",
        "metadata": {
            "reason": "Python formatting/call wrapping changed without changing review intent."
        },
    })
}

pub(crate) fn add_compact_superseded_group_for_refactorings(
    changes: &[ChangeDraft<'_>],
    finalization: &mut PythonReviewFinalization,
) {
    let refactoring_count = changes
        .iter()
        .filter(|change| change.change_type == "REFACTORING")
        .count();
    if refactoring_count == 0 {
        return;
    }
    let old_labels: Vec<String> = changes
        .iter()
        .filter(|change| change.change_type == "REFACTORING")
        .flat_map(|change| node_labels(change.old_node))
        .take(16)
        .collect();
    let new_labels: Vec<String> = changes
        .iter()
        .filter(|change| change.change_type == "REFACTORING")
        .flat_map(|change| node_labels(change.new_node))
        .take(16)
        .collect();
    finalization.change_groups.push(json!({
        "kind": "NOISE_SUPPRESSED",
        "raw_change_indices": [],
        "old_labels": old_labels,
        "new_labels": new_labels,
        "old_node_ids": [],
        "new_node_ids": [],
        "confidence": 0.82,
        "rule_id": "presentation.compact_superseded_meaningful_group",
        "metadata": {
            "reason": "refactoring_groups_replace_lower_level_meaningful_changes",
            "suppressed_count": refactoring_count,
        },
    }));
}

pub(crate) fn apply_python_literal_invariances(
    changes: &mut Vec<ChangeDraft<'_>>,
    old_tree: &SemanticNode,
    new_tree: &SemanticNode,
    old_source: &str,
    new_source: &str,
    finalization: &mut PythonReviewFinalization,
) {
    let mut remove_indices = HashSet::new();
    let mut covered_literal_labels = HashSet::new();
    for (idx, change) in changes.iter().enumerate() {
        if change.change_type != "MODIFICATION" {
            continue;
        }
        if let Some(group) = integer_literal_equivalence_group(idx, change) {
            collect_literal_pair_labels(change, "integer", &mut covered_literal_labels);
            remove_indices.insert(idx);
            finalization.change_groups.push(group.0);
            finalization.ignored_style_changes.push(group.1);
            continue;
        }
        if let Some(group) = string_literal_equivalence_group(idx, change, old_source, new_source) {
            collect_literal_pair_labels(change, "string", &mut covered_literal_labels);
            remove_indices.insert(idx);
            finalization.change_groups.push(group.0);
            finalization.ignored_style_changes.push(group.1);
        }
    }
    for (old_idx, new_idx, group, ignored) in
        literal_add_delete_equivalence_groups(changes, old_source, new_source)
    {
        remove_indices.insert(old_idx);
        remove_indices.insert(new_idx);
        collect_invariance_group_labels(&group, &mut covered_literal_labels);
        finalization.change_groups.push(group);
        finalization.ignored_style_changes.push(ignored);
    }
    if let Some((group, ignored)) =
        source_string_quote_equivalence_group(old_tree, new_tree, old_source, new_source)
    {
        if !finalization.change_groups.iter().any(|item| {
            item.get("rule_id").and_then(Value::as_str)
                == Some("core.string_literal.decoded_value.safe")
        }) {
            collect_invariance_group_labels(&group, &mut covered_literal_labels);
            finalization.change_groups.push(group);
            finalization.ignored_style_changes.push(ignored);
        }
    }
    if !covered_literal_labels.is_empty() {
        for (idx, change) in changes.iter().enumerate() {
            if !matches!(change.change_type, "ADDITION" | "DELETION") {
                continue;
            }
            let Some(node) = change.old_node.or(change.new_node) else {
                continue;
            };
            if !matches!(node.node_type.as_str(), "integer" | "string") {
                continue;
            }
            let mut labels = vec![node.label.clone()];
            if let Some(decoded) = decode_simple_python_string(&node.label) {
                labels.push(decoded);
            }
            if labels
                .iter()
                .any(|label| covered_literal_labels.contains(label))
            {
                remove_indices.insert(idx);
            }
        }
    }
    if remove_indices.is_empty() {
        return;
    }
    let mut index = 0usize;
    changes.retain(|_| {
        let keep = !remove_indices.contains(&index);
        index += 1;
        keep
    });
    if changes.is_empty() {
        finalization.is_style_only = false;
    }
}

pub(crate) fn collect_literal_pair_labels(
    change: &ChangeDraft<'_>,
    node_type: &str,
    labels: &mut HashSet<String>,
) {
    if let Some((old_node, new_node)) = first_literal_pair(change, node_type) {
        labels.insert(old_node.label.clone());
        labels.insert(new_node.label.clone());
        if let Some(decoded) = decode_simple_python_string(&old_node.label) {
            labels.insert(decoded);
        }
        if let Some(decoded) = decode_simple_python_string(&new_node.label) {
            labels.insert(decoded);
        }
    }
}

pub(crate) fn collect_invariance_group_labels(group: &Value, labels: &mut HashSet<String>) {
    for key in ["old_labels", "new_labels"] {
        if let Some(items) = group.get(key).and_then(Value::as_array) {
            for item in items.iter().filter_map(Value::as_str) {
                labels.insert(item.to_owned());
            }
        }
    }
    if let Some(metadata) = group.get("metadata").and_then(Value::as_object) {
        for key in ["old_label", "new_label"] {
            if let Some(label) = metadata.get(key).and_then(Value::as_str) {
                labels.insert(label.to_owned());
                if let Some(decoded) = decode_simple_python_string(label) {
                    labels.insert(decoded);
                }
            }
        }
    }
}

pub(crate) fn integer_literal_equivalence_group(
    raw_index: usize,
    change: &ChangeDraft<'_>,
) -> Option<(Value, Value)> {
    let (old_node, new_node) = first_literal_pair(change, "integer")?;
    let old_canonical = canonical_integer_literal(&old_node.label)?;
    let new_canonical = canonical_integer_literal(&new_node.label)?;
    if old_canonical != new_canonical || old_node.label == new_node.label {
        return None;
    }
    let group = json!({
        "kind": "IGNORED_STYLE",
        "raw_change_indices": [raw_index],
        "old_labels": [old_node.label],
        "new_labels": [new_node.label],
        "old_node_ids": [old_node.id],
        "new_node_ids": [new_node.id],
        "confidence": 1.0,
        "rule_id": "core.integer_literal.canonical_value.safe",
        "metadata": {
            "canonical_old": format!("int({old_canonical})"),
            "canonical_new": format!("int({new_canonical})"),
            "old_label": old_node.label,
            "new_label": new_node.label,
        },
    });
    let ignored = json!({
        "language": "python",
        "rule_id": "core.integer_literal.canonical_value.safe",
        "reason": "Integer literal spelling changed but canonical value is equal.",
        "old_label": old_node.label,
        "new_label": new_node.label,
    });
    Some((group, ignored))
}

pub(crate) fn string_literal_equivalence_group(
    raw_index: usize,
    change: &ChangeDraft<'_>,
    old_source: &str,
    new_source: &str,
) -> Option<(Value, Value)> {
    let (old_node, new_node) = first_literal_pair(change, "string")?;
    let old_raw = source_slice(old_source, &old_node.position)?;
    let new_raw = source_slice(new_source, &new_node.position)?;
    let old_decoded = decode_simple_python_string(old_raw)?;
    let new_decoded = decode_simple_python_string(new_raw)?;
    if old_decoded != new_decoded || old_raw == new_raw {
        return None;
    }
    let group = json!({
        "kind": "IGNORED_STYLE",
        "raw_change_indices": [raw_index],
        "old_labels": [old_decoded],
        "new_labels": [new_decoded],
        "old_node_ids": [old_node.id],
        "new_node_ids": [new_node.id],
        "confidence": 1.0,
        "rule_id": "core.string_literal.decoded_value.safe",
        "metadata": {
            "canonical_old": format!("string({old_decoded})"),
            "canonical_new": format!("string({new_decoded})"),
            "old_label": old_raw,
            "new_label": new_raw,
            "evidence_depth": "source_span",
            "old_span": [old_node.position.start_col, old_node.position.end_col],
            "new_span": [new_node.position.start_col, new_node.position.end_col],
        },
    });
    let ignored = json!({
        "language": "python",
        "rule_id": "core.string_literal.decoded_value.safe",
        "reason": "String quote spelling changed but decoded value is equal.",
        "old_label": old_raw,
        "new_label": new_raw,
    });
    Some((group, ignored))
}

pub(crate) fn literal_add_delete_equivalence_groups(
    changes: &[ChangeDraft<'_>],
    old_source: &str,
    new_source: &str,
) -> Vec<(usize, usize, Value, Value)> {
    let mut result = Vec::new();
    let mut used_deletions = HashSet::new();
    let mut used_additions = HashSet::new();
    for (old_idx, deletion) in changes.iter().enumerate() {
        if deletion.change_type != "DELETION" || used_deletions.contains(&old_idx) {
            continue;
        }
        let Some(old_node) = deletion.old_node else {
            continue;
        };
        for (new_idx, addition) in changes.iter().enumerate() {
            if addition.change_type != "ADDITION" || used_additions.contains(&new_idx) {
                continue;
            }
            let Some(new_node) = addition.new_node else {
                continue;
            };
            if old_node.node_type != new_node.node_type {
                continue;
            }
            let pair = match old_node.node_type.as_str() {
                "integer" => {
                    integer_literal_equivalence_values(old_idx, new_idx, old_node, new_node)
                }
                "string" => string_literal_equivalence_values(
                    old_idx, new_idx, old_node, new_node, old_source, new_source,
                ),
                _ => None,
            };
            let Some((group, ignored)) = pair else {
                continue;
            };
            used_deletions.insert(old_idx);
            used_additions.insert(new_idx);
            result.push((old_idx, new_idx, group, ignored));
            break;
        }
    }
    result
}

pub(crate) fn integer_literal_equivalence_values(
    old_idx: usize,
    new_idx: usize,
    old_node: &SemanticNode,
    new_node: &SemanticNode,
) -> Option<(Value, Value)> {
    let old_canonical = canonical_integer_literal(&old_node.label)?;
    let new_canonical = canonical_integer_literal(&new_node.label)?;
    if old_canonical != new_canonical || old_node.label == new_node.label {
        return None;
    }
    let group = json!({
        "kind": "IGNORED_STYLE",
        "raw_change_indices": [old_idx, new_idx],
        "old_labels": [old_node.label],
        "new_labels": [new_node.label],
        "old_node_ids": [old_node.id],
        "new_node_ids": [new_node.id],
        "confidence": 1.0,
        "rule_id": "core.integer_literal.canonical_value.safe",
        "metadata": {
            "canonical_old": format!("int({old_canonical})"),
            "canonical_new": format!("int({new_canonical})"),
            "old_label": old_node.label,
            "new_label": new_node.label,
        },
    });
    let ignored = json!({
        "language": "python",
        "rule_id": "core.integer_literal.canonical_value.safe",
        "reason": "Integer literal spelling changed but canonical value is equal.",
        "old_label": old_node.label,
        "new_label": new_node.label,
    });
    Some((group, ignored))
}

pub(crate) fn string_literal_equivalence_values(
    old_idx: usize,
    new_idx: usize,
    old_node: &SemanticNode,
    new_node: &SemanticNode,
    old_source: &str,
    new_source: &str,
) -> Option<(Value, Value)> {
    let old_raw = source_slice(old_source, &old_node.position)?;
    let new_raw = source_slice(new_source, &new_node.position)?;
    let old_decoded = decode_simple_python_string(old_raw)?;
    let new_decoded = decode_simple_python_string(new_raw)?;
    if old_decoded != new_decoded || old_raw == new_raw {
        return None;
    }
    Some(string_literal_equivalence_payload(
        &[old_idx, new_idx],
        old_node,
        new_node,
        old_raw,
        new_raw,
        &old_decoded,
        &new_decoded,
    ))
}

pub(crate) fn source_string_quote_equivalence_group(
    old_tree: &SemanticNode,
    new_tree: &SemanticNode,
    old_source: &str,
    new_source: &str,
) -> Option<(Value, Value)> {
    if old_source == new_source {
        return None;
    }
    let old_literals = source_string_literals(old_tree, old_source);
    let new_by_id: HashMap<&str, (&SemanticNode, String, String)> =
        source_string_literals(new_tree, new_source)
            .into_iter()
            .map(|(node, raw, decoded)| (node.id.as_str(), (node, raw, decoded)))
            .collect();
    for (old_node, old_raw, old_decoded) in old_literals {
        let Some((new_node, new_raw, new_decoded)) = new_by_id.get(old_node.id.as_str()) else {
            continue;
        };
        if old_decoded == new_decoded.as_str() && old_raw != new_raw.as_str() {
            return Some(string_literal_equivalence_payload(
                &[],
                old_node,
                *new_node,
                &old_raw,
                new_raw,
                &old_decoded,
                new_decoded,
            ));
        }
    }
    None
}

pub(crate) fn string_literal_equivalence_payload(
    raw_indices: &[usize],
    old_node: &SemanticNode,
    new_node: &SemanticNode,
    old_raw: &str,
    new_raw: &str,
    old_decoded: &str,
    new_decoded: &str,
) -> (Value, Value) {
    let group = json!({
        "kind": "IGNORED_STYLE",
        "raw_change_indices": raw_indices,
        "old_labels": [old_decoded],
        "new_labels": [new_decoded],
        "old_node_ids": [old_node.id],
        "new_node_ids": [new_node.id],
        "confidence": 1.0,
        "rule_id": "core.string_literal.decoded_value.safe",
        "metadata": {
            "canonical_old": format!("string({old_decoded})"),
            "canonical_new": format!("string({new_decoded})"),
            "old_label": old_raw,
            "new_label": new_raw,
            "evidence_depth": "source_span",
            "old_span": [old_node.position.start_col, old_node.position.end_col],
            "new_span": [new_node.position.start_col, new_node.position.end_col],
        },
    });
    let ignored = json!({
        "language": "python",
        "rule_id": "core.string_literal.decoded_value.safe",
        "reason": "String quote spelling changed but decoded value is equal.",
        "old_label": old_raw,
        "new_label": new_raw,
    });
    (group, ignored)
}

pub(crate) fn first_literal_pair<'a>(
    change: &'a ChangeDraft<'a>,
    node_type: &str,
) -> Option<(&'a SemanticNode, &'a SemanticNode)> {
    let old_node = change.old_node?;
    let new_node = change.new_node?;
    if old_node.node_type == node_type && new_node.node_type == node_type {
        return Some((old_node, new_node));
    }
    let new_by_id = all_descendant_node_refs_by_id(new_node);
    for old_descendant in old_node.descendants() {
        if old_descendant.node_type != node_type {
            continue;
        }
        if let Some(new_descendant) = new_by_id.get(old_descendant.id.as_str()) {
            if new_descendant.node_type == node_type {
                return Some((old_descendant, *new_descendant));
            }
        }
    }
    None
}

pub(crate) fn canonical_integer_literal(label: &str) -> Option<String> {
    let normalized = label.replace('_', "");
    if normalized.chars().all(|ch| ch.is_ascii_digit()) {
        Some(normalized)
    } else {
        None
    }
}

pub(crate) fn source_slice<'a>(source: &'a str, position: &NodePosition) -> Option<&'a str> {
    if position.start_line != position.end_line {
        return None;
    }
    let line = source.lines().nth(position.start_line as usize)?;
    line.get(position.start_col as usize..position.end_col as usize)
}

pub(crate) fn decode_simple_python_string(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.len() < 2 {
        return None;
    }
    let quote_start = trimmed
        .char_indices()
        .find_map(|(idx, ch)| matches!(ch, '\'' | '"').then_some(idx))?;
    let prefix = &trimmed[..quote_start];
    if prefix.chars().any(|ch| matches!(ch, 'f' | 'F')) {
        return None;
    }
    if !prefix
        .chars()
        .all(|ch| matches!(ch, 'r' | 'R' | 'u' | 'U' | 'b' | 'B'))
    {
        return None;
    }
    let literal = &trimmed[quote_start..];
    let mut chars = literal.chars();
    let first = chars.next()?;
    let last = literal.chars().last()?;
    if !matches!(first, '\'' | '"') || first != last {
        return None;
    }
    Some(literal[1..literal.len() - 1].to_owned())
}

pub(crate) fn dedup_preserve(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    values.into_iter().filter(|v| seen.insert(v.clone())).collect()
}

/// The formatting-equivalence IGNORED_STYLE rule for a language (port of
/// `_STYLE_RULE_BY_LANGUAGE` + `_STYLE_RULE_METADATA`, presentation.py). Returns
/// (rule_id, explanation, equivalence_kind, risk). Only csharp is finalize-routed today;
/// python/javascript share the family but run on other paths.
pub(crate) fn formatting_equivalence_rule(
    language: &str,
) -> Option<(&'static str, &'static str, &'static str, &'static str)> {
    match language {
        "csharp" => Some((
            "csharp.formatting.initializer_query_output_wrapping_equivalence",
            "Formatting-only C# initializer, LINQ query, and output-call wrapper churn was \
             ignored because the review-level semantic changes are already represented by \
             more precise changes.",
            "syntactic_trivia",
            "amber",
        )),
        _ => None,
    }
}

/// Port of `_style_context_groups_from_final_changes` + `_style_groups_from_suppression`
/// (presentation.py) for the finalize-routed path (issue #57 Root B). When the surviving
/// MODIFICATIONs carry a csharp formatting anchor — an `order_by_clause` modification or a
/// format-string label — the formatter's initializer/LINQ/output wrapper churn was
/// compacted away, so emit an IGNORED_STYLE group recording that, alongside the precise
/// MEANINGFUL changes. Returns (group_json, ignored_entry_json). Provenance is
/// "suppression" (issue #51): relabelled suppression residue, never an equivalence PROOF —
/// it must not let a zero-change diff claim style-only.
pub(crate) fn formatting_equivalence_group_drafts(
    changes: &[ChangeDraft<'_>],
    language: &str,
) -> Option<(Value, Value)> {
    let (rule_id, explanation, equivalence_kind, risk) = formatting_equivalence_rule(language)?;
    let mods: Vec<(usize, &ChangeDraft)> = changes
        .iter()
        .enumerate()
        .filter(|(_, change)| {
            change.change_type == "MODIFICATION"
                && change.old_node.is_some()
                && change.new_node.is_some()
        })
        .collect();
    if mods.is_empty() {
        return None;
    }
    let has_anchor = mods.iter().any(|(_, change)| {
        [change.old_node, change.new_node]
            .into_iter()
            .flatten()
            .any(|node| node.node_type == "order_by_clause")
    }) || mods.iter().any(|(_, change)| {
        [change.old_node, change.new_node]
            .into_iter()
            .flatten()
            .any(|node| {
                node_labels(Some(node))
                    .iter()
                    .any(|label| label.contains("{0}") || label.contains("Name:"))
            })
    });
    if !has_anchor {
        return None;
    }
    let raw_indices: Vec<usize> = mods.iter().map(|(idx, _)| *idx).collect();
    let mut old_labels = Vec::new();
    let mut new_labels = Vec::new();
    let mut old_ids = Vec::new();
    let mut new_ids = Vec::new();
    for (_, change) in &mods {
        old_labels.extend(node_labels(change.old_node));
        new_labels.extend(node_labels(change.new_node));
        old_ids.extend(node_ids(change.old_node));
        new_ids.extend(node_ids(change.new_node));
    }
    let old_labels = dedup_preserve(old_labels);
    let new_labels = dedup_preserve(new_labels);
    let old_ids = dedup_preserve(old_ids);
    let new_ids = dedup_preserve(new_ids);
    let source_rule_ids = json!(["presentation.csharp.style_context_anchor"]);
    let group = json!({
        "kind": "IGNORED_STYLE",
        "raw_change_indices": raw_indices,
        "old_labels": old_labels,
        "new_labels": new_labels,
        "old_node_ids": old_ids,
        "new_node_ids": new_ids,
        "confidence": 0.7,
        "rule_id": rule_id,
        "metadata": {
            "index_space": "mixed",
            "source_group_count": 1,
            "source_rule_ids": source_rule_ids,
            "reason": explanation,
            "equivalence_kind": equivalence_kind,
            "risk": risk,
            "language": language,
        },
    });
    let ignored = json!({
        "language": language,
        "rule_id": rule_id,
        "reason": explanation,
        "equivalence_kind": equivalence_kind,
        "risk": risk,
        "provenance": "suppression",
        "source_group_count": 1,
        "source_rule_ids": source_rule_ids,
        "old_labels": old_labels,
        "new_labels": new_labels,
        "old_node_ids": old_ids,
        "new_node_ids": new_ids,
    });
    Some((group, ignored))
}

