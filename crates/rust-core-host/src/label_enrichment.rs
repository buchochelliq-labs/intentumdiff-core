//! Profile label enrichment (statement/markup/mdx/path/keyed/query families),
//! extracted from lib.rs verbatim (issue #29 monolith split, phase B).

use crate::*;

/// python statement_profiles.enrich_statement_profile_labels (issue #57 profile-enrichment
/// port): improve statement-level labels from source spans when plugins emit weak labels.
/// Bottom-up; recomputes structural hashes on changed nodes (python parity).
pub(crate) fn enrich_statement_profile_labels_node(
    node: &SemanticNode,
    source_lines: &[&str],
    language: &str,
) -> SemanticNode {
    let mut children_changed = false;
    let children: Vec<SemanticNode> = node
        .children
        .iter()
        .map(|child| {
            let enriched = enrich_statement_profile_labels_node(child, source_lines, language);
            if enriched.structural_hash != child.structural_hash
                || enriched.label != child.label
            {
                children_changed = true;
            }
            enriched
        })
        .collect();
    let node_type = node.node_type.to_lowercase();
    let snippet = profile_source_snippet(source_lines, node);
    let mut label = node.label.clone();

    if language == "asm" && node_type == "instruction" {
        if !snippet.is_empty() {
            label = snippet;
        }
    } else if language == "bash" {
        if matches!(node_type.as_str(), "command" | "declaration_command") {
            if !snippet.is_empty() {
                label = snippet;
            } else {
                let from_children = bash_command_label_from_children(&children);
                if !from_children.is_empty() {
                    label = from_children;
                }
            }
        } else if node_type == "variable_assignment" && !snippet.is_empty() {
            label = snippet;
        }
    } else if language == "delphi"
        && matches!(
            node_type.as_str(),
            "assignment" | "assignment_statement" | "exprcall" | "procedure_call" | "statement"
        )
    {
        let compact = snippet.trim_end_matches(';').trim().to_string();
        if !compact.is_empty() {
            label = compact;
        }
    }

    if label == node.label && !children_changed {
        return node.clone();
    }
    let structural_hash = synthetic_structural_hash(&node.node_type, &label, &children);
    let mut updated = node.clone();
    updated.label = label;
    updated.children = children;
    updated.structural_hash = structural_hash;
    updated
}

// ── Path-family profile enrichment (issue #57 profile-enrichment port, unit 2) ──
// python analysis/path_profiles.py enrich_path_profile_labels + helpers. Hand-rolled
// scanners replace the python regexes (repo norm: no regex crate in 22k lines).

/// python path_profiles._source_slice (0-based lines, unlike statement's asm/delphi).
pub(crate) fn path_source_slice(source_lines: &[&str], node: &SemanticNode) -> String {
    let pos = &node.position;
    let start_line = pos.start_line as usize;
    if start_line >= source_lines.len() {
        return String::new();
    }
    // Tree-sitter columns are UTF-8 BYTE offsets, not Unicode scalar indexes.
    // Slicing by chars shifted every nested markup node after a non-ASCII
    // character and silently lost meaningful XML text modifications.
    let clip = |line: &str, from: Option<usize>, to: Option<usize>| -> String {
        let from = from.unwrap_or(0).min(line.len());
        let to = to.unwrap_or(line.len()).min(line.len());
        if from >= to {
            return String::new();
        }
        // Invalid byte boundaries are not recoverable by char-index guessing.
        line.get(from..to).unwrap_or("").to_string()
    };
    let end_line = pos.end_line as usize;
    if start_line == end_line {
        return clip(
            source_lines[start_line],
            Some(pos.start_col as usize),
            Some(pos.end_col as usize),
        );
    }
    let mut parts = vec![clip(source_lines[start_line], Some(pos.start_col as usize), None)];
    for line in &source_lines[start_line + 1..end_line.min(source_lines.len())] {
        parts.push((*line).to_string());
    }
    if end_line < source_lines.len() {
        parts.push(clip(source_lines[end_line], None, Some(pos.end_col as usize)));
    }
    parts.join("\n")
}

pub(crate) fn compact_ws(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn path_normalize_text(label: &str) -> String {
    let text = label.trim();
    let quoted = (text.starts_with('"') && text.ends_with('"') && text.len() >= 2)
        || (text.starts_with('\'') && text.ends_with('\'') && text.len() >= 2);
    let text = if quoted { text[1..text.len() - 1].trim() } else { text };
    text.to_lowercase()
}

pub(crate) fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

pub(crate) fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | ':' | '.' | '-')
}

/// python path_profiles._tag_name_from_source (regex search for the first tag name).
pub(crate) fn tag_name_from_source(snippet: &str) -> String {
    let chars: Vec<char> = snippet.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<' {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if j < chars.len() && chars[j] == '/' {
                j += 1;
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
            }
            if j < chars.len() && is_ident_start(chars[j]) {
                let start = j;
                while j < chars.len() && is_ident_char(chars[j]) {
                    j += 1;
                }
                return chars[start..j].iter().collect();
            }
        }
        i += 1;
    }
    String::new()
}

/// python path_profiles._attribute_label_from_source: anchored `name(= value)?` parse
/// with quote stripping and boolean-attribute normalization (disabled="disabled" -> disabled).
pub(crate) fn attribute_label_from_source(snippet: &str) -> String {
    let text = snippet.trim();
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() || !is_ident_start(chars[0]) {
        return String::new();
    }
    let mut i = 0;
    while i < chars.len() && is_ident_char(chars[i]) {
        i += 1;
    }
    let name: String = chars[..i].iter().collect();
    let rest: String = chars[i..].iter().collect();
    let rest = rest.trim();
    if rest.is_empty() {
        return name;
    }
    let Some(value) = rest.strip_prefix('=') else {
        return String::new();
    };
    let value = value
        .trim()
        .trim_matches(|c| c == '"' || c == '\'')
        .to_string();
    if !value.is_empty() && value != name {
        format!("{name}={value}")
    } else {
        name
    }
}

/// python path_profiles._attributes_from_opening_tag: attrs between the tag name and
/// the first `>` (no angle brackets inside), tokens `name(="v"|'v'|bare)?`.
pub(crate) fn attributes_from_opening_tag(snippet: &str, language: &str) -> Vec<(String, String)> {
    let chars: Vec<char> = snippet.chars().collect();
    let mut i = 0;
    let mut attr_region: Option<String> = None;
    while i < chars.len() {
        if chars[i] == '<' {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if j < chars.len() && is_ident_start(chars[j]) {
                while j < chars.len() && is_ident_char(chars[j]) {
                    j += 1;
                }
                let region_start = j;
                let mut k = j;
                let mut ok = false;
                let mut quote: Option<char> = None;
                while k < chars.len() {
                    match chars[k] {
                        '"' | '\'' if quote.is_none() => quote = Some(chars[k]),
                        c if quote == Some(c) => quote = None,
                        '>' if quote.is_none() => {
                            ok = true;
                            break;
                        }
                        '<' if quote.is_none() => break,
                        _ => {}
                    }
                    k += 1;
                }
                if ok {
                    let mut region: String = chars[region_start..k].iter().collect();
                    if region.ends_with('/') {
                        region.pop();
                    }
                    attr_region = Some(region);
                    break;
                }
            }
        }
        i += 1;
    }
    let Some(region) = attr_region else {
        return Vec::new();
    };
    let chars: Vec<char> = region.chars().collect();
    let mut result = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !is_ident_start(chars[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && is_ident_char(chars[i]) {
            i += 1;
        }
        let raw_name = chars[start..i].iter().collect::<String>();
        let name = if language == "xml" {
            raw_name
        } else {
            path_normalize_text(&raw_name)
        };
        let mut j = i;
        while j < chars.len() && chars[j].is_whitespace() {
            j += 1;
        }
        let mut value = String::new();
        if j < chars.len() && chars[j] == '=' {
            j += 1;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if j < chars.len() && (chars[j] == '"' || chars[j] == '\'') {
                let quote = chars[j];
                j += 1;
                let vstart = j;
                while j < chars.len() && chars[j] != quote {
                    j += 1;
                }
                value = chars[vstart..j].iter().collect();
                if j < chars.len() {
                    j += 1;
                }
            } else {
                let vstart = j;
                while j < chars.len()
                    && !chars[j].is_whitespace()
                    && !matches!(chars[j], '"' | '\'' | '/' | '>')
                {
                    j += 1;
                }
                value = chars[vstart..j].iter().collect();
            }
            i = j;
        }
        if !name.is_empty() {
            result.push((name, value));
        }
    }
    result
}

/// End of a markup opening tag, ignoring > within single or double quoted
/// attributes. Offsets are UTF-8 bytes, matching Tree-sitter coordinates.
fn markup_opening_tag_end(snippet: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (offset, ch) in snippet.char_indices() {
        match ch {
            '"' | '\'' if quote.is_none() => quote = Some(ch),
            c if quote == Some(c) => quote = None,
            '>' if quote.is_none() => return Some(offset + 1),
            '<' if offset > 0 && quote.is_none() => return None,
            _ => {}
        }
    }
    None
}

/// python path_profiles._direct_markup_text: fullmatch `<tag ...> text </tag>` (text has
/// no '<') or the CDATA variant; whitespace-compacted. Case-insensitive tag comparison.
/// Use source-byte offsets throughout: Unicode case folding can change byte length.
pub(crate) fn direct_markup_text(snippet: &str, tag_label: &str) -> String {
    let text = snippet.trim();
    let Some(open_len) = markup_opening_tag_end(text) else {
        return String::new();
    };
    let Some(opening) = text.get(1..open_len - 1) else {
        return String::new();
    };
    let opening = opening.trim_start();
    let name_len = opening.find(char::is_whitespace).unwrap_or(opening.len());
    let name = &opening[..name_len];
    if name.to_lowercase() != tag_label.to_lowercase() || opening[name_len..].contains('<') {
        return String::new();
    }

    let body_and_close = &text[open_len..];
    let Some(close_start) = body_and_close.rfind("</") else {
        return String::new();
    };
    let closing = body_and_close[close_start + 2..].trim_start();
    let Some(closing_name) = closing.strip_suffix('>') else {
        return String::new();
    };
    if closing_name.trim().to_lowercase() != tag_label.to_lowercase() {
        return String::new();
    }

    let content = body_and_close[..close_start].trim();
    if content.is_empty() {
        return String::new();
    }
    if let Some(cdata) = content.strip_prefix("<![CDATA[") {
        if let Some(inner) = cdata.strip_suffix("]]>") {
            return compact_ws(inner.trim());
        }
        return String::new();
    }
    if content.contains('<') {
        return String::new();
    }
    compact_ws(content)
}

/// python path_profiles._decode_xml_entities: CDATA unwrap + XML entities
/// (lt/gt/amp/quot/apos + numeric; python used html.unescape — XML parser output
/// only produces these forms).
pub(crate) fn decode_xml_entities(text: &str) -> String {
    if let Some(start) = text.find("<![CDATA[") {
        if let Some(end) = text.rfind("]]>") {
            if end > start {
                return text[start + 9..end].to_string();
            }
        }
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(idx) = rest.find('&') {
        out.push_str(&rest[..idx]);
        rest = &rest[idx..];
        let Some(semi) = rest.find(';') else {
            out.push_str(rest);
            return out;
        };
        let entity = &rest[1..semi];
        let decoded = match entity {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

pub(crate) fn path_synthetic_position(node: &SemanticNode) -> NodePosition {
    NodePosition {
        start_line: node.position.start_line,
        start_col: node.position.start_col,
        end_line: node.position.start_line,
        end_col: node.position.start_col,
    }
}

pub(crate) fn path_synthetic_node(
    id: String,
    node_type: &str,
    label: String,
    position: NodePosition,
    children: Vec<SemanticNode>,
) -> SemanticNode {
    let structural_hash = synthetic_structural_hash(node_type, &label, &children);
    SemanticNode {
        id,
        node_type: node_type.to_string(),
        label,
        position,
        structural_hash,
        children,
        parent_type: None,
        type_info: None,
        facts: None,
    }
}

pub(crate) fn path_is_generic_label(label: &str, node_type: &str) -> bool {
    let text = label.trim();
    if text.is_empty() {
        return true;
    }
    let lowered = text.to_lowercase();
    lowered == node_type.to_lowercase()
        || matches!(
            lowered.as_str(),
            "class_selector" | "content" | "document" | "element" | "id_selector"
                | "pseudo_class_selector" | "pseudo_element_selector" | "rule_set"
                | "selector" | "selectors" | "stylesheet"
        )
}

// Style enrichment uses parser byte columns. Keep the exact slice and offsets
// while deriving nodes; a parent's start is not a child's source location.
fn style_source_slice(lines: &[&str], node: &SemanticNode) -> String {
    let p = &node.position;
    let Some(first) = lines.get(p.start_line as usize) else { return String::new() };
    if p.start_line == p.end_line {
        return first.get(p.start_col as usize..p.end_col as usize).unwrap_or("").to_string();
    }
    let Some(start) = first.get(p.start_col as usize..) else { return String::new() };
    let mut parts = vec![start];
    for line in p.start_line + 1..p.end_line {
        let Some(text) = lines.get(line as usize) else { return String::new() };
        parts.push(text);
    }
    let Some(end) = lines.get(p.end_line as usize).and_then(|line| line.get(..p.end_col as usize)) else { return String::new() };
    parts.push(end);
    parts.join("\n")
}

fn style_position(node: &SemanticNode, snippet: &str, range: std::ops::Range<usize>) -> NodePosition {
    let point = |offset: usize| {
        let prefix = &snippet[..offset];
        let line = prefix.bytes().filter(|&b| b == b'\n').count() as u32;
        let col = prefix.rfind('\n').map_or(node.position.start_col as usize + offset, |i| offset - i - 1);
        (node.position.start_line + line, col as u32)
    };
    let (start_line, start_col) = point(range.start);
    let (end_line, end_col) = point(range.end);
    NodePosition { start_line, start_col, end_line, end_col }
}

struct StyleItem {
    kind: String,
    label: String,
    value: String,
    span: std::ops::Range<usize>,
    value_span: std::ops::Range<usize>,
}

// Replace lexical comments with spaces without moving source byte offsets. Only
// delimiters outside strings and balanced function/attribute arguments split items.
fn style_lexical_source(snippet: &str, language: &str) -> (String, Vec<(usize, u8)>) {
    let bytes = snippet.as_bytes();
    let mut cleaned = bytes.to_vec();
    let mut delimiters = Vec::new();
    let mut quote = None;
    let mut interpolation = 0usize;
    let mut arguments = Vec::new();
    let mut url_depth = None;
    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        if let Some(q) = quote {
            if byte == b'\\' { i += 2; continue; }
            if byte == q { quote = None; }
        } else if url_depth.is_none() && byte == b'/' && bytes.get(i + 1) == Some(&b'*') {
            let start = i;
            i += 2;
            while i < bytes.len() && !(bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/')) {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            for b in &mut cleaned[start..i] {
                if !matches!(*b, b'\r' | b'\n') { *b = b' '; }
            }
            continue;
        } else if language == "scss" && url_depth.is_none()
            && byte == b'/' && bytes.get(i + 1) == Some(&b'/') {
            while i < bytes.len() && !matches!(bytes[i], b'\r' | b'\n') {
                cleaned[i] = b' ';
                i += 1;
            }
            continue;
        } else {
            if language == "scss" && byte == b'#' && bytes.get(i + 1) == Some(&b'{') {
                interpolation += 1;
                i += 2;
                continue;
            }
            if interpolation > 0 && matches!(byte, b'{' | b'}') {
                if byte == b'{' { interpolation += 1; } else { interpolation -= 1; }
                i += 1;
                continue;
            }
            match byte {
                b'\\' => { i += 2; continue; }
                b'\'' | b'"' => quote = Some(byte),
                b'[' | b']' if url_depth.is_some() => {},
                b'(' | b'[' => {
                    if byte == b'(' && url_depth.is_none() && i >= 3
                        && bytes[i - 3..i].eq_ignore_ascii_case(b"url")
                        && (i == 3 || !matches!(bytes[i - 4], b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'-' | 0x80..=0xff)) {
                        // Sass permits // inside the CSS url() token.
                        url_depth = Some(arguments.len() + 1);
                    }
                    arguments.push(byte);
                }
                b')' if arguments.last() == Some(&b'(') => {
                    if url_depth == Some(arguments.len()) { url_depth = None; }
                    arguments.pop();
                }
                b']' if arguments.last() == Some(&b'[') => { arguments.pop(); }
                b':' | b';' | b'{' | b'}' if arguments.is_empty() && interpolation == 0 => delimiters.push((i, byte)),
                _ => {}
            }
        }
        i += 1;
    }
    // Comment bytes alone were replaced by ASCII spaces, preserving UTF-8.
    (String::from_utf8(cleaned).expect("comment masking preserves UTF-8"), delimiters)
}

fn style_items_from_source(snippet: &str, parent_selector: &str, language: &str) -> Vec<StyleItem> {
    let (cleaned, delimiters) = style_lexical_source(snippet, language);
    let Some(open) = delimiters.iter().position(|(_, byte)| *byte == b'{') else { return vec![] };
    let mut result: Vec<StyleItem> = Vec::new();
    let mut blocks: Vec<Option<usize>> = vec![None];
    let mut start = delimiters[open].0 + 1;
    let mut colon = None;
    let mut value_blocks = 0usize;
    for &(offset, byte) in &delimiters[open + 1..] {
        if value_blocks > 0 {
            if byte == b'{' { value_blocks += 1; }
            if byte == b'}' { value_blocks -= 1; }
            continue;
        }
        if byte == b'{' && colon.is_some_and(|colon| cleaned[start..colon].trim().starts_with("--")) {
            value_blocks = 1;
            continue;
        }
        if byte == b':' { colon.get_or_insert(offset); continue; }
        let text = cleaned[start..offset].trim();
        let text_start = start + cleaned[start..offset].len() - cleaned[start..offset].trim_start().len();
        if byte == b'{' {
            let index = if text.is_empty() { None } else {
                result.push(StyleItem {
                    kind: "rule_set".into(), label: compact_ws(&text.replace('&', parent_selector)),
                    value: String::new(), span: text_start..offset + 1, value_span: 0..0,
                });
                Some(result.len() - 1)
            };
            blocks.push(index);
        } else {
            if let Some(rest) = text.strip_prefix("@include").filter(|rest| rest.starts_with(char::is_whitespace)) {
                let name = rest.trim().split('(').next().unwrap_or("").trim();
                if !name.is_empty() {
                    result.push(StyleItem { kind: "include_statement".into(), label: name.into(), value: String::new(),
                        span: text_start..offset + usize::from(byte == b';'), value_span: 0..0 });
                }
            } else if let Some(rest) = text.strip_prefix("@extend").filter(|rest| rest.starts_with(char::is_whitespace)) {
                let name = rest.trim();
                if !name.is_empty() {
                    result.push(StyleItem { kind: "extend_statement".into(), label: name.into(), value: String::new(),
                        span: text_start..offset + usize::from(byte == b';'), value_span: 0..0 });
                }
            } else if let Some(colon) = colon.filter(|_| !text.starts_with('@')) {
                let name = cleaned[start..colon].trim();
                let value_text = cleaned[colon + 1..offset].trim();
                let value_start = colon + 1 + cleaned[colon + 1..offset].len() - cleaned[colon + 1..offset].trim_start().len();
                let value_span = value_start..value_start + value_text.len();
                if !name.is_empty() {
                    result.push(StyleItem { kind: "declaration".into(), label: name.into(),
                        value: snippet[value_span.clone()].into(), span: text_start..offset + usize::from(byte == b';'), value_span });
                }
            }
            if byte == b'}' {
                if let Some(Some(index)) = blocks.pop() { result[index].span.end = offset + 1; }
                if blocks.is_empty() { break; }
            }
        }
        start = offset + 1;
        colon = None;
    }
    result
}

/// python path_profiles._enrich_style_children.
pub(crate) fn enrich_style_children(
    node: &SemanticNode,
    source_lines: &[&str],
    mut children: Vec<SemanticNode>,
    label: &str,
    language: &str,
) -> Vec<SemanticNode> {
    let node_type = node.node_type.to_lowercase();
    if matches!(node_type.as_str(), "declaration" | "variable_declaration") {
        let snippet = style_source_slice(source_lines, node);
        let value = match snippet.find(':') {
            Some(colon) => snippet[colon + 1..].trim().trim_end_matches(';').trim().to_string(),
            None => String::new(),
        };
        if value.is_empty()
            || children.iter().any(|c| c.node_type == "property_value")
        {
            return children;
        }
        let value_start = snippet.find(':').unwrap() + 1;
        let value_start = value_start + snippet[value_start..].len() - snippet[value_start..].trim_start().len();
        let value_position = style_position(node, &snippet, value_start..value_start + value.len());
        children.push(path_synthetic_node(
            format!("{}.path_value", node.id),
            "property_value",
            value,
            value_position,
            Vec::new(),
        ));
        return children;
    }
    if !matches!(node_type.as_str(), "rule_set" | "mixin_statement" | "function_statement") {
        return children;
    }
    let existing: HashSet<(String, String)> = children
        .iter()
        .map(|c| (c.node_type.clone(), c.label.clone()))
        .collect();
    let snippet = style_source_slice(source_lines, node);
    for (index, item) in
        style_items_from_source(&snippet, label, language).into_iter().enumerate()
    {
        if existing.contains(&(item.kind.clone(), item.label.clone())) {
            continue;
        }
        let item_children = if item.value.is_empty() {
            Vec::new()
        } else {
            vec![path_synthetic_node(
                format!("{}.path_{}.value", node.id, index),
                "property_value",
                item.value,
                style_position(node, &snippet, item.value_span),
                Vec::new(),
            )]
        };
        children.push(path_synthetic_node(
            format!("{}.path_{}", node.id, index),
            &item.kind,
            item.label,
            style_position(node, &snippet, item.span),
            item_children,
        ));
    }
    children
}

/// python path_profiles._attribute_parts / _attribute_name.
fn attribute_name_for_language(label: &str, language: &str) -> String {
    let text = label.trim();
    let name = match text.find('=') {
        Some(eq) => text[..eq].trim(),
        None => text,
    };
    if language == "xml" {
        name.to_string()
    } else {
        path_normalize_text(name)
    }
}

/// python path_profiles._enrich_existing_markup_attributes: rewrite existing attribute
/// labels to name=value (boolean attributes stay name-only); recompute hashes on change.
pub(crate) fn enrich_existing_markup_attributes(
    nodes: Vec<SemanticNode>,
    attr_values: &HashMap<String, String>,
    language: &str,
) -> Vec<SemanticNode> {
    nodes
        .into_iter()
        .map(|node| {
            // Only attributes owned by this opening tag may consume its values.
            // Recursing through element descendants lets an ancestor's `id`,
            // `class`, etc. overwrite the nested element's own source value.
            let label = enriched_markup_attribute_label(&node, attr_values, language);
            let mut children = node.children.clone();
            if matches!(node.node_type.to_lowercase().as_str(), "start_tag" | "self_closing_tag") {
                children = children
                    .into_iter()
                    .map(|child| {
                        if child.node_type.eq_ignore_ascii_case("attribute") {
                            let child_label = enriched_markup_attribute_label(&child, attr_values, language);
                            if child_label != child.label {
                                let structural_hash = synthetic_structural_hash(
                                    &child.node_type,
                                    &child_label,
                                    &child.children,
                                );
                                let mut updated = child;
                                updated.label = child_label;
                                updated.structural_hash = structural_hash;
                                return updated;
                            }
                        }
                        child
                    })
                    .collect();
            }
            let children_changed = children.len() != node.children.len()
                || children
                    .iter()
                    .zip(node.children.iter())
                    .any(|(a, b)| a.structural_hash != b.structural_hash || a.label != b.label);
            if !children_changed && label == node.label {
                return node;
            }
            let structural_hash =
                synthetic_structural_hash(&node.node_type, &label, &children);
            let mut updated = node;
            updated.label = label;
            updated.children = children;
            updated.structural_hash = structural_hash;
            updated
        })
        .collect()
}

fn enriched_markup_attribute_label(
    node: &SemanticNode,
    attr_values: &HashMap<String, String>,
    language: &str,
) -> String {
    if !node.node_type.eq_ignore_ascii_case("attribute") {
        return node.label.clone();
    }
    let name = attribute_name_for_language(&node.label, language);
    match attr_values.get(&name) {
        Some(value) if !value.is_empty() && *value != name => format!("{name}={value}"),
        Some(_) => name,
        None => node.label.clone(),
    }
}

/// python path_profiles._enrich_markup_attributes: re-inject attributes parsed from the
/// opening tag as synthetic `attribute` children (skipping xmlns/xmlns:*), preferring an
/// existing start/self-closing tag child; otherwise a synthetic start_tag is prepended.
pub(crate) fn enrich_markup_attributes(
    node: &SemanticNode,
    source_lines: &[&str],
    children: Vec<SemanticNode>,
    label: &str,
    language: &str,
) -> Vec<SemanticNode> {
    let snippet = path_source_slice(source_lines, node);
    let attrs: Vec<(String, String)> = attributes_from_opening_tag(&snippet, language)
        .into_iter()
        .filter(|(name, _)| name != "xmlns" && !name.starts_with("xmlns:"))
        .collect();
    if attrs.is_empty() {
        return children;
    }
    let attr_values: HashMap<String, String> = attrs.iter().cloned().collect();
    let children = enrich_existing_markup_attributes(children, &attr_values, language);
    let existing_names: HashSet<String> = children
        .iter()
        .flat_map(|child| {
            std::iter::once(child)
                .chain(
                    matches!(child.node_type.to_lowercase().as_str(), "start_tag" | "self_closing_tag")
                        .then_some(child.children.iter())
                        .into_iter()
                        .flatten(),
                )
                .filter(|candidate| candidate.node_type.eq_ignore_ascii_case("attribute"))
                .map(|candidate| attribute_name_for_language(&candidate.label, language))
                .collect::<Vec<_>>()
        })
        .collect();
    let synthetic_attrs: Vec<SemanticNode> = attrs
        .iter()
        .enumerate()
        .filter(|(_, (name, _))| !existing_names.contains(name))
        .map(|(index, (name, value))| {
            let attr_label = if value.is_empty() {
                name.clone()
            } else {
                format!("{name}={value}")
            };
            path_synthetic_node(
                format!("{}.path_attr_{}", node.id, index),
                "attribute",
                attr_label,
                path_synthetic_position(node),
                Vec::new(),
            )
        })
        .collect();
    if synthetic_attrs.is_empty() {
        return children;
    }
    for (index, child) in children.iter().enumerate() {
        if matches!(
            child.node_type.to_lowercase().as_str(),
            "end_tag" | "self_closing_tag" | "start_tag"
        ) {
            let mut tag_children = child.children.clone();
            tag_children.extend(synthetic_attrs.iter().cloned());
            let structural_hash =
                synthetic_structural_hash(&child.node_type, &child.label, &tag_children);
            let mut updated = child.clone();
            updated.children = tag_children;
            updated.structural_hash = structural_hash;
            let mut result = children.clone();
            result[index] = updated;
            return result;
        }
    }
    let start_tag = path_synthetic_node(
        format!("{}.path_start", node.id),
        "start_tag",
        label.to_string(),
        path_synthetic_position(node),
        synthetic_attrs,
    );
    let mut result = vec![start_tag];
    result.extend(children);
    result
}

/// Locate the *source bytes* for a synthetic markup text child, rather than
/// attributing the text mutation to a zero-width span at the element's start.
/// The label may be whitespace-normalized or XML-entity-decoded, so searching
/// for the final label would lose the original evidence range.
fn markup_text_position(node: &SemanticNode, snippet: &str) -> Option<NodePosition> {
    let open_end = markup_opening_tag_end(snippet)?;
    let close_start = snippet.rfind("</")?;
    if close_start <= open_end {
        return None;
    }
    let raw = snippet.get(open_end..close_start)?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut start = open_end + raw.len() - raw.trim_start().len();
    let mut end = start + trimmed.len();
    if trimmed.starts_with("<![CDATA[") && trimmed.ends_with("]]>") {
        // Point at the actual CDATA content, not the <![CDATA[ delimiters.
        start += "<![CDATA[".len();
        end = end.checked_sub("]]>".len())?;
        let inner = snippet.get(start..end)?;
        start += inner.len() - inner.trim_start().len();
        end -= inner.len() - inner.trim_end().len();
    }
    if start >= end {
        return None;
    }
    // style_position is byte-based and translates multiline offsets to
    // parser coordinates. It is appropriate for markup source slices too.
    Some(style_position(node, snippet, start..end))
}

/// python path_profiles._enrich_markup_children: attributes + a synthetic `text` child
/// so text-value changes surface as element-level MODIFICATIONs (xml decodes entities).
pub(crate) fn enrich_markup_children(
    node: &SemanticNode,
    source_lines: &[&str],
    children: Vec<SemanticNode>,
    label: &str,
    language: &str,
) -> Vec<SemanticNode> {
    if !matches!(
        node.node_type.to_lowercase().as_str(),
        "element" | "script_element" | "style_element"
    ) {
        return children;
    }
    let mut children = enrich_markup_attributes(node, source_lines, children, label, language);
    if children.iter().any(|c| {
        matches!(c.node_type.to_lowercase().as_str(), "text" | "chardata" | "content")
    }) {
        return children;
    }
    let snippet = path_source_slice(source_lines, node);
    let mut text = direct_markup_text(&snippet, label);
    if language == "xml" && !text.is_empty() {
        text = decode_xml_entities(&text);
    }
    if text.is_empty() {
        return children;
    }
    children.push(path_synthetic_node(
        format!("{}.path_text", node.id),
        "text",
        text,
        markup_text_position(node, &snippet).unwrap_or(NodePosition {
            start_line: node.position.start_line,
            start_col: node.position.start_col,
            end_line: node.position.end_line,
            end_col: node.position.end_col,
        }),
        Vec::new(),
    ));
    children
}

/// python path_profiles._enrich_mdx_children + _mdx_code_body(+_from_position): the
/// code-fence body becomes a synthetic `code_content` child.
pub(crate) fn enrich_mdx_children(
    node: &SemanticNode,
    source_lines: &[&str],
    mut children: Vec<SemanticNode>,
) -> Vec<SemanticNode> {
    if !node.node_type.eq_ignore_ascii_case("code_block") {
        return children;
    }
    if children.iter().any(|c| c.node_type == "code_content") {
        return children;
    }
    // Rust owns both the MDX code-body label and its source coordinates. A
    // synthetic (opening_fence_row, 0)-(same_row, 0) extent incorrectly claimed
    // the body had no source bytes; route equality cannot certify that (#206).
    let opening = node.position.start_line as usize;
    let closing = node.position.end_line as usize;
    let Some(opening_line) = source_lines.get(opening) else {
        return children;
    };
    let fence = opening_line.trim_start();
    if !(fence.starts_with("```") || fence.starts_with("~~~")) {
        return children;
    }
    // An unterminated fence ends on its final content row. Exclude that
    // row only if it is a matching closing delimiter, not arbitrary code.
    let delimiter = fence.as_bytes()[0];
    let width = fence.bytes().take_while(|byte| *byte == delimiter).count();
    let has_closing_fence = closing > opening && source_lines.get(closing).is_some_and(|line| {
        let candidate = line.trim();
        let closing_width = candidate.bytes().take_while(|byte| *byte == delimiter).count();
        closing_width >= width && candidate[closing_width..].is_empty()
    });
    let body_end = if has_closing_fence { closing } else { closing.saturating_add(1) };
    let Some(inner) = source_lines.get(opening + 1..body_end) else {
        return children;
    };
    let body = inner.iter().map(|line| line.trim_end())
        .collect::<Vec<_>>().join("\n").trim().to_string();
    if body.is_empty() {
        return children;
    }
    let Some(first) = inner.iter().position(|line| !line.trim().is_empty()) else {
        return children;
    };
    let Some(last) = inner.iter().rposition(|line| !line.trim().is_empty()) else {
        return children;
    };
    let position = NodePosition {
        start_line: (opening + 1 + first) as u32,
        start_col: (inner[first].len() - inner[first].trim_start().len()) as u32,
        end_line: (opening + 1 + last) as u32,
        end_col: inner[last].trim_end().len() as u32,
    };
    children.push(path_synthetic_node(
        format!("{}.path_code", node.id),
        "code_content",
        body,
        position,
        Vec::new(),
    ));
    children
}


/// python path_profiles.enrich_path_profile_labels (issue #57 profile-enrichment port):
/// recover css/scss selector + declaration labels, html/xml tag/attribute labels with
/// synthetic attribute/text children, and mdx code-fence bodies; bottom-up with
/// structural-hash recomputation on changed nodes.
pub(crate) fn enrich_path_profile_labels_node(
    node: &SemanticNode,
    source_lines: &[&str],
    language: &str,
) -> SemanticNode {
    let mut children_changed = false;
    let children: Vec<SemanticNode> = node
        .children
        .iter()
        .map(|child| {
            let enriched = enrich_path_profile_labels_node(child, source_lines, language);
            if enriched.structural_hash != child.structural_hash
                || enriched.label != child.label
            {
                children_changed = true;
            }
            enriched
        })
        .collect();
    let node_type = node.node_type.to_lowercase();
    let mut label = node.label.clone();
    let mut children = children;

    if matches!(language, "css" | "scss") {
        let snippet = path_source_slice(source_lines, node);
        if node_type == "rule_set" {
            let selector = compact_ws(snippet.split('{').next().unwrap_or(""));
            if !selector.is_empty() {
                label = selector;
            }
        } else if node_type == "selectors"
            || matches!(
                node_type.as_str(),
                "adjacent_sibling_selector" | "attribute_selector" | "child_selector"
                    | "class_selector" | "descendant_selector" | "id_selector"
                    | "pseudo_class_selector" | "pseudo_element_selector"
                    | "sibling_selector" | "type_selector" | "universal_selector"
            )
        {
            let compact = compact_ws(&snippet);
            if !compact.is_empty() {
                label = compact;
            }
        } else if matches!(node_type.as_str(), "declaration" | "variable_declaration") {
            let name = snippet.split(':').next().unwrap_or("").trim().to_string();
            if !name.is_empty() {
                label = name;
            }
        }
        let before = children.len();
        children = enrich_style_children(node, source_lines, children, &label, language);
        if children.len() != before {
            children_changed = true;
        }
    } else if matches!(language, "html" | "xml") {
        if matches!(
            node_type.as_str(),
            "element" | "script_element" | "style_element" | "end_tag"
                | "self_closing_tag" | "start_tag"
        ) {
            if path_is_generic_label(&label, &node_type) {
                let snippet = path_source_slice(source_lines, node);
                let tag = tag_name_from_source(&snippet);
                if !tag.is_empty() {
                    label = tag;
                }
            }
        } else if node_type == "attribute" {
            let snippet = path_source_slice(source_lines, node);
            let attr = attribute_label_from_source(&snippet);
            if !attr.is_empty() {
                label = attr;
            }
        }
        let before_hashes: Vec<String> =
            children.iter().map(|c| c.structural_hash.clone()).collect();
        children = enrich_markup_children(node, source_lines, children, &label, language);
        if children.len() != before_hashes.len()
            || children
                .iter()
                .zip(before_hashes.iter())
                .any(|(c, h)| &c.structural_hash != h)
        {
            children_changed = true;
        }
    } else if language == "mdx" {
        let before = children.len();
        children = enrich_mdx_children(node, source_lines, children);
        if children.len() != before {
            children_changed = true;
        }
    }

    if label == node.label && !children_changed {
        return node.clone();
    }
    let structural_hash = synthetic_structural_hash(&node.node_type, &label, &children);
    let mut updated = node.clone();
    updated.label = label;
    updated.children = children;
    updated.structural_hash = structural_hash;
    updated
}

// ── Resource profile enrichment (hcl/puppet) — issue #90 ──
// The last profile family ported off Python; the Rust core is now authoritative.
// Fills identity labels for hcl blocks and puppet resource/parameter nodes from
// their (recursively-enriched) children. Mirrors
// analysis/resource_profiles.py::enrich_resource_profile_labels.

/// python resource_profiles._first_concrete_label — first descendant (incl. self,
/// pre-order) whose label is non-empty and non-generic.
fn resource_first_concrete_label(node: &SemanticNode) -> Option<String> {
    if !node.label.is_empty() && !is_generic_resource_label(&node.label, &node.node_type) {
        return Some(node.label.clone());
    }
    for child in &node.children {
        if let Some(found) = resource_first_concrete_label(child) {
            return Some(found);
        }
    }
    None
}

/// python _hcl_block_identity_from_children.
fn hcl_block_identity_from_children(node: &SemanticNode, children: &[SemanticNode]) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    if !node.label.is_empty() && !is_generic_resource_label(&node.label, &node.node_type) {
        if node.label.contains(' ') {
            return node.label.split_whitespace().map(str::to_string).collect();
        }
        parts.push(node.label.clone());
    }
    for child in children {
        if child.node_type.to_lowercase() == "body" {
            break;
        }
        if let Some(label) = resource_first_concrete_label(child) {
            parts.push(label);
        }
    }
    parts
}

/// python _puppet_resource_identity_from_children.
fn puppet_resource_identity_from_children(
    node: &SemanticNode,
    children: &[SemanticNode],
) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    if !node.label.is_empty() && !is_generic_resource_label(&node.label, &node.node_type) {
        if let Some(first) = node.label.split_whitespace().next() {
            parts.push(first.to_string());
        }
    }
    for child in children {
        let ct = child.node_type.to_lowercase();
        if ct == "string" || ct == "title" {
            if !child.label.is_empty() {
                // Titles are IDENTITY, not literals: strip the quotes the
                // content-preserving string capture includes (issue #46).
                parts.push(
                    child
                        .label
                        .trim()
                        .trim_matches(|c| c == '\'' || c == '"')
                        .to_string(),
                );
            }
            break;
        }
        if ct == "resource_body" || ct == "attribute" || ct == "attribute_list" {
            break;
        }
    }
    parts.truncate(2);
    parts
}

/// python _first_descendant_label — first descendant (incl. self, pre-order) of
/// any of `nodes` whose node_type is in `node_types` and has a non-empty label.
fn resource_first_descendant_label(nodes: &[SemanticNode], node_types: &[&str]) -> Option<String> {
    fn walk(node: &SemanticNode, node_types: &[&str]) -> Option<String> {
        if node_types.contains(&node.node_type.to_lowercase().as_str()) && !node.label.is_empty() {
            return Some(node.label.clone());
        }
        for child in &node.children {
            if let Some(found) = walk(child, node_types) {
                return Some(found);
            }
        }
        None
    }
    nodes.iter().find_map(|node| walk(node, node_types))
}

pub(crate) fn enrich_resource_profile_labels_node(node: &SemanticNode, language: &str) -> SemanticNode {
    let children: Vec<SemanticNode> = node
        .children
        .iter()
        .map(|child| enrich_resource_profile_labels_node(child, language))
        .collect();
    let node_type = node.node_type.to_lowercase();
    let mut label = node.label.clone();

    if language == "hcl" && node_type == "block" {
        let parts = hcl_block_identity_from_children(node, &children);
        if !parts.is_empty() {
            label = parts.join(" ");
        }
    } else if language == "puppet" {
        if node_type == "resource_declaration" || node_type == "resource_statement" {
            let parts = puppet_resource_identity_from_children(node, &children);
            if !parts.is_empty() {
                label = parts.join(" ");
            }
        } else if node_type == "parameter" {
            if let Some(found) = resource_first_descendant_label(&children, &["variable"]) {
                label = found;
            }
        }
    }

    let children_changed = children.len() != node.children.len()
        || children
            .iter()
            .zip(node.children.iter())
            .any(|(c, o)| c.structural_hash != o.structural_hash);
    if label == node.label && !children_changed {
        return node.clone();
    }
    let structural_hash = enriched_structural_hash(node, &label, &children);
    let mut updated = node.clone();
    updated.label = label;
    updated.children = children;
    updated.structural_hash = structural_hash;
    updated
}

// ── Keyed + query profile enrichment (issue #57 profile-enrichment port, unit 3) ──

/// python keyed_profiles._is_generic_label (keyed variant, incl. positional [N]).
pub(crate) fn keyed_is_generic_label(label: &str, node_type: &str) -> bool {
    let text = label.trim();
    if text.is_empty() {
        return true;
    }
    let lowered = text.to_lowercase();
    if lowered == node_type.to_lowercase()
        || matches!(
            lowered.as_str(),
            "array" | "block_mapping" | "block_node" | "block_sequence" | "document"
                | "flow_mapping" | "flow_node" | "flow_sequence" | "job" | "object"
                | "pipeline" | "stream"
        )
    {
        return true;
    }
    lowered.len() >= 3
        && lowered.starts_with('[')
        && lowered.ends_with(']')
        && lowered[1..lowered.len() - 1].chars().all(|c| c.is_ascii_digit())
}

/// python keyed_profiles._first_meaningful_label.
pub(crate) fn first_meaningful_label(nodes: &[SemanticNode]) -> String {
    for node in nodes {
        for candidate in std::iter::once(node).chain(node.descendants()) {
            if !candidate.label.is_empty()
                && !keyed_is_generic_label(&candidate.label, &candidate.node_type)
            {
                return candidate.label.clone();
            }
        }
    }
    String::new()
}

/// python keyed_profiles._identity_label_from_direct_pairs (direct children only).
pub(crate) fn identity_label_from_direct_pairs(
    nodes: &[SemanticNode],
    pair_types: &[&str],
    identity_keys: &HashSet<String>,
) -> String {
    for candidate in nodes {
        if !pair_types.contains(&candidate.node_type.to_lowercase().as_str()) {
            continue;
        }
        if !identity_keys.contains(&normalize_keyed_identity(&candidate.label)) {
            continue;
        }
        if candidate.children.len() > 1 {
            let label = first_meaningful_label(&candidate.children[1..]);
            if !label.is_empty() {
                return label;
            }
        }
    }
    String::new()
}

/// python keyed_profiles._identity_label_from_pairs (any descendant pair).
pub(crate) fn identity_label_from_pairs(
    nodes: &[SemanticNode],
    pair_types: &[&str],
    identity_keys: &HashSet<String>,
) -> String {
    for node in nodes {
        for candidate in std::iter::once(node).chain(node.descendants()) {
            if !pair_types.contains(&candidate.node_type.to_lowercase().as_str()) {
                continue;
            }
            if !identity_keys.contains(&normalize_keyed_identity(&candidate.label)) {
                continue;
            }
            if candidate.children.len() > 1 {
                let label = first_meaningful_label(&candidate.children[1..]);
                if !label.is_empty() {
                    return label;
                }
            }
        }
    }
    String::new()
}

/// python keyed_profiles.enrich_keyed_data_labels: fill missing json/yaml pair/object/
/// sequence-item labels from key children. NOTE: python does NOT recompute structural
/// hashes here (model_copy without structural_hash) — parity preserved.
pub(crate) fn enrich_keyed_data_labels_node(
    node: &SemanticNode,
    language: &str,
    identity_keys: &HashSet<String>,
) -> (SemanticNode, bool) {
    // Changed-ness propagates EXPLICITLY: keyed enrichment does not recompute
    // structural hashes (python parity), so a grandchild's label fill is invisible
    // to label/hash comparison one level up — detecting by fields silently DROPPED
    // enriched subtrees under any ancestor whose own label didn't change.
    let mut children_changed = false;
    let children: Vec<SemanticNode> = node
        .children
        .iter()
        .map(|child| {
            let (enriched, changed) =
                enrich_keyed_data_labels_node(child, language, identity_keys);
            if changed {
                children_changed = true;
            }
            enriched
        })
        .collect();
    let node_type = node.node_type.to_lowercase();
    let mut label = node.label.clone();

    if language == "json" && node_type == "pair" && keyed_is_generic_label(&label, &node_type) {
        let candidate = first_meaningful_label(&children[..children.len().min(1)]);
        if !candidate.is_empty() {
            label = candidate;
        }
    } else if language == "json"
        && node_type == "object"
        && keyed_is_generic_label(&label, &node_type)
    {
        let candidate = identity_label_from_direct_pairs(&children, &["pair"], identity_keys);
        if !candidate.is_empty() {
            label = candidate;
        }
    } else if language == "yaml"
        && matches!(node_type.as_str(), "block_mapping_pair" | "flow_pair")
        && keyed_is_generic_label(&label, &node_type)
    {
        let candidate = first_meaningful_label(&children[..children.len().min(1)]);
        if !candidate.is_empty() {
            label = candidate;
        }
    } else if language == "yaml"
        && matches!(node_type.as_str(), "block_sequence_item" | "flow_sequence_item")
        && keyed_is_generic_label(&label, &node_type)
    {
        let candidate = identity_label_from_pairs(
            &children,
            &["block_mapping_pair", "flow_pair"],
            identity_keys,
        );
        if !candidate.is_empty() {
            label = candidate;
        }
    }

    if label == node.label && !children_changed {
        return (node.clone(), false);
    }
    let mut updated = node.clone();
    updated.label = label;
    updated.children = children;
    (updated, true)
}

/// python query_profiles.enrich_query_profile_labels (sql): clause labels from
/// compacted source spans, 120-char cap. No hash recomputation (python parity).
pub(crate) fn enrich_query_profile_labels_node(
    node: &SemanticNode,
    source_lines: &[&str],
) -> (SemanticNode, bool) {
    // Explicit changed-flag propagation — same reason as the keyed visitor above.
    let mut children_changed = false;
    let children: Vec<SemanticNode> = node
        .children
        .iter()
        .map(|child| {
            let (enriched, changed) = enrich_query_profile_labels_node(child, source_lines);
            if changed {
                children_changed = true;
            }
            enriched
        })
        .collect();
    let node_type = node.node_type.to_lowercase();
    let mut label = node.label.clone();
    if matches!(
        node_type.as_str(),
        "binary_expression" | "from" | "join" | "join_clause" | "order_by"
            | "order_by_clause" | "relation" | "term" | "where" | "where_clause"
    ) {
        let compact = compact_ws(&path_source_slice(source_lines, node));
        let compact = if compact.chars().count() > 120 {
            let head: String = compact.chars().take(117).collect();
            format!("{head}...")
        } else {
            compact
        };
        if !compact.is_empty() {
            label = compact;
        }
    }
    if label == node.label && !children_changed {
        return (node.clone(), false);
    }
    let mut updated = node.clone();
    updated.label = label;
    updated.children = children;
    (updated, true)
}

#[cfg(test)]
mod markup_text_span_tests {
    use super::*;

    #[test]
    fn nested_and_sibling_attributes_keep_their_own_values() {
        fn attr(id: &str, value: &str) -> SemanticNode {
            path_synthetic_node(
                id.into(),
                "attribute",
                format!("id={value}"),
                NodePosition { start_line: 0, start_col: 0, end_line: 0, end_col: 1 },
                vec![],
            )
        }
        fn start(id: &str, name: &str, value: &str) -> SemanticNode {
            path_synthetic_node(
                id.into(),
                "start_tag",
                name.into(),
                NodePosition { start_line: 0, start_col: 0, end_line: 0, end_col: 1 },
                vec![attr(&format!("{id}.0"), value)],
            )
        }
        fn element(id: &str, name: &str, value: &str, children: Vec<SemanticNode>) -> SemanticNode {
            path_synthetic_node(
                id.into(),
                "element",
                name.into(),
                NodePosition { start_line: 0, start_col: 0, end_line: 0, end_col: 1 },
                vec![start(&format!("{id}.0"), name, value), children.into_iter().next().unwrap_or_else(|| path_synthetic_node(
                    format!("{id}.1"), "text", "text".into(),
                    NodePosition { start_line: 0, start_col: 0, end_line: 0, end_col: 1 }, vec![],
                ))],
            )
        }
        let source = "<root id=\"outer\"><child id=\"before\"><leaf id=\"deep\"/></child><child id=\"sibling\"/></root>";
        let leaf = element("0.0.0.0", "leaf", "deep", vec![]);
        let nested = element("0.0.0", "child", "before", vec![leaf]);
        let sibling = element("0.0.1", "child", "sibling", vec![]);
        let root = path_synthetic_node(
            "0".into(),
            "element",
            "root".into(),
            NodePosition { start_line: 0, start_col: 0, end_line: 0, end_col: source.len() as u32 },
            vec![],
        );
        let children = enrich_markup_attributes(
            &root,
            &[source],
            vec![start("0.0", "root", "stale"), nested, sibling],
            "root",
            "html",
        );
        let root_start = &children[0];
        assert_eq!(root_start.children.iter().find(|n| n.node_type == "attribute").unwrap().label, "id=outer");
        let nested_id = &children[1].children[0].children[0];
        assert_eq!(nested_id.label, "id=before");
        let leaf_id = &children[1].children[1].children[0].children[0];
        assert_eq!(leaf_id.label, "id=deep");
        let sibling_id = &children[2].children[0].children[0];
        assert_eq!(sibling_id.label, "id=sibling");
    }

    #[test]
    fn xml_attribute_names_remain_case_sensitive() {
        let source = "<root Foo=\"outer\" foo=\"lower\"><child Foo=\"inner\"/></root>";
        assert_eq!(
            attributes_from_opening_tag(source, "xml"),
            vec![("Foo".to_string(), "outer".to_string()), ("foo".to_string(), "lower".to_string())],
        );
        assert_eq!(attribute_name_for_language("Foo=outer", "xml"), "Foo");
        assert_eq!(attribute_name_for_language("Foo=outer", "html"), "foo");
    }

    #[test]
    fn nested_xml_text_points_to_the_value_not_the_opening_tag() {
        let source = "<root><value>1</value></root>";
        let node = path_synthetic_node(
            "0.0.0".into(), "element", "value".into(),
            NodePosition { start_line: 0, start_col: 6, end_line: 0, end_col: 22 },
            vec![],
        );
        let children = enrich_markup_children(&node, &[source], vec![], "value", "xml");
        let text = children.iter().find(|c| c.node_type == "text").expect("text child");
        assert_eq!(text.label, "1");
        assert_eq!(
            (text.position.start_line, text.position.start_col,
             text.position.end_line, text.position.end_col),
            (0, 13, 0, 14),
        );
    }

    #[test]
    fn multiline_xml_text_uses_source_line_and_indentation() {
        let source = "<value>\n    alpha\n</value>";
        let node = path_synthetic_node(
            "0.0".into(), "element", "value".into(),
            NodePosition { start_line: 0, start_col: 0, end_line: 2, end_col: 8 },
            vec![],
        );
        let lines: Vec<&str> = source.lines().collect();
        let children = enrich_markup_children(&node, &lines, vec![], "value", "xml");
        let text = children.iter().find(|c| c.node_type == "text").expect("text child");
        assert_eq!(text.label, "alpha");
        assert_eq!(
            (text.position.start_line, text.position.start_col,
             text.position.end_line, text.position.end_col),
            (1, 4, 1, 9),
        );
    }

    #[test]
    fn markup_after_unicode_prefix_keeps_meaningful_source_span() {
        let source = "<root>é<value>1</value></root>";
        let start = source.find("<value>").expect("nested value");
        let end = start + "<value>1</value>".len();
        let node = path_synthetic_node(
            "0.0.0".into(), "element", "value".into(),
            NodePosition {
                start_line: 0, start_col: start as u32,
                end_line: 0, end_col: end as u32,
            }, vec![],
        );
        assert_eq!(path_source_slice(&[source], &node), "<value>1</value>");
        let children = enrich_markup_children(&node, &[source], vec![], "value", "xml");
        let text = children.iter().find(|c| c.node_type == "text").expect("text child");
        assert_eq!(text.label, "1");
        assert_eq!(
            (text.position.start_line, text.position.start_col,
             text.position.end_line, text.position.end_col),
            (0, 15, 0, 16),
        );
    }

    #[test]
    fn quoted_gt_in_attribute_preserves_text_and_attribute_evidence() {
        // Both are legal XML opening tags; the delimiter is the unquoted '>'.
        // The non-ASCII capital dotted I also expands when lowercased, so
        // calculating offsets from a lowercased copy corrupts UTF-8 spans.
        for (source, value, attribute) in [
            ("<value note=\"x>y\">1</value>", "1", "x>y"),
            ("<value note=\"İ>y\">2</value>", "2", "İ>y"),
            ("<value note='x>y'>3</value>", "3", "x>y"),
        ] {
            assert_eq!(direct_markup_text(source, "value"), value);
            assert_eq!(
                attributes_from_opening_tag(source, "xml"),
                vec![("note".to_string(), attribute.to_string())]
            );
            let node = path_synthetic_node(
                "0.0".into(), "element", "value".into(),
                NodePosition {
                    start_line: 0, start_col: 0,
                    end_line: 0, end_col: source.len() as u32,
                },
                vec![],
            );
            let children = enrich_markup_children(&node, &[source], vec![], "value", "xml");
            let text = children.iter().find(|c| c.node_type == "text").expect("text child");
            let expected_start = source.find(&format!(">{value}<")).unwrap() + 1;
            assert_eq!(text.label, value);
            assert_eq!(
                (text.position.start_line, text.position.start_col,
                 text.position.end_line, text.position.end_col),
                (0, expected_start as u32, 0, (expected_start + value.len()) as u32),
            );
        }
    }

    #[test]
    fn xml_entities_and_cdata_preserve_original_byte_ranges() {
        for (source, label, start, end) in [
            ("<value>A&amp;B</value>", "A&B", 7, 14),
            ("<value><![CDATA[  one  ]]></value>", "one", 18, 21),
            ("<value>café</value>", "café", 7, 12),
        ] {
            let node = path_synthetic_node(
                "0.0".into(), "element", "value".into(),
                NodePosition {
                    start_line: 0, start_col: 0,
                    end_line: 0, end_col: source.len() as u32,
                },
                vec![],
            );
            let children = enrich_markup_children(&node, &[source], vec![], "value", "xml");
            let text = children.iter().find(|c| c.node_type == "text").expect("text child");
            assert_eq!(text.label, label);
            assert_eq!(
                (text.position.start_line, text.position.start_col,
                 text.position.end_line, text.position.end_col),
                (0, start, 0, end),
            );
        }
    }
}

#[cfg(test)]
mod style_span_tests {
    use super::*;

    #[test]
    fn style_source_keeps_complete_escaped_multiline_values() {
        let source = "a {\n  b: 'line1 \\\n      line2';\n}\n\n";
        let items = style_items_from_source(source, "a", "scss");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "b");
        assert_eq!(items[0].value, "'line1 \\\n      line2'");
        assert_eq!(&source[items[0].value_span.clone()], items[0].value);
    }

    #[test]
    fn style_source_skips_comments_and_tracks_lexical_delimiters() {
        let source = ".é { /* : ; { ignored } */ color /* name */: red; content: 'é;}:/*text*/'; transform: fn(1; 2); &:hover { b: blue; c: green; } }";
        let items = style_items_from_source(source, ".é", "css");
        let actual: Vec<_> = items.iter().map(|i| (i.kind.as_str(), i.label.as_str(), i.value.as_str())).collect();
        assert_eq!(actual, vec![
            ("declaration", "color", "red"),
            ("declaration", "content", "'é;}:/*text*/'"),
            ("declaration", "transform", "fn(1; 2)"),
            ("rule_set", ".é:hover", ""),
            ("declaration", "b", "blue"),
            ("declaration", "c", "green"),
        ]);
        for item in items.iter().filter(|i| !i.value.is_empty()) {
            assert_eq!(&source[item.value_span.clone()], item.value);
        }
        assert!(source[items[0].span.clone()].starts_with("color"));
    }

    #[test]
    fn style_source_multiline_value_positions_preserve_crlf_and_utf8() {
        let source = "é a {\r\n  b: 'café \\\r\n    fin'; c: \"x\";\r\n}";
        let node = path_synthetic_node("0.0".into(), "rule_set", "a".into(),
            NodePosition { start_line: 0, start_col: 3, end_line: 3, end_col: 1 }, vec![]);
        let children = enrich_style_children(&node, &source.split('\n').collect::<Vec<_>>(), vec![], "a", "scss");
        let value = &children[0].children[0];
        assert_eq!(value.label, "'café \\\r\n    fin'");
        assert_eq!((value.position.start_line, value.position.start_col, value.position.end_line, value.position.end_col), (1, 5, 2, 8));
        assert_eq!(children[1].label, "c");
    }

    #[test]
    fn style_source_preserves_interpolation_and_custom_property_blocks() {
        for (source, expected) in [
            ("a { color: #{$tone}; margin: 1px; }", vec![("color", "#{$tone}"), ("margin", "1px")]),
            ("a { --theme: { color: red; }; margin: 1px; }", vec![("--theme", "{ color: red; }"), ("margin", "1px")]),
        ] {
            let items = style_items_from_source(source, "a", "scss");
            assert!(items.iter().all(|i| i.kind == "declaration"));
            assert_eq!(items.iter().map(|i| (i.label.as_str(), i.value.as_str())).collect::<Vec<_>>(), expected);
        }
    }

    #[test]
    fn style_source_skips_scss_line_comment_delimiters() {
        let items = style_items_from_source("a { // } ; :\n color: red;\n}", "a", "scss");
        assert_eq!(items.len(), 1);
        assert_eq!((items[0].label.as_str(), items[0].value.as_str()), ("color", "red"));
    }

    #[test]
    fn style_source_keeps_unquoted_scss_url_comment_markers() {
        let source = "a { background: url(http://example/a); color: red; }";
        let items = style_items_from_source(source, "a", "scss");
        assert_eq!(items.len(), 2);
        assert_eq!((items[0].label.as_str(), items[0].value.as_str()), ("background", "url(http://example/a)"));
        assert_eq!(&source[items[0].span.clone()], "background: url(http://example/a);");
        assert_eq!(&source[items[0].value_span.clone()], items[0].value);
    }

    #[test]
    fn style_source_keeps_block_comment_markers_in_unquoted_urls() {
        let source = "a { background: url(https://example/a/*/b); color: red; }";
        for language in ["css", "scss"] {
            let items = style_items_from_source(source, "a", language);
            assert_eq!(items.len(), 2);
            assert_eq!(items[0].value, "url(https://example/a/*/b)");
            assert_eq!(&source[items[0].span.clone()], "background: url(https://example/a/*/b);");
            assert_eq!(&source[items[0].value_span.clone()], items[0].value);
        }
    }

    #[test]
    fn style_source_keeps_unmatched_brackets_in_url_payloads() {
        let source = "a { background: url(https://example/a[b); color: red; }";
        for language in ["css", "scss"] {
            let items = style_items_from_source(source, "a", language);
            assert_eq!(items.len(), 2);
            assert_eq!(items[0].value, "url(https://example/a[b)");
            assert_eq!(&source[items[0].value_span.clone()], items[0].value);
            assert_eq!((items[1].label.as_str(), items[1].value.as_str()), ("color", "red"));
        }
    }

    #[test]
    fn style_children_point_to_declarations_values_and_nested_selectors() {
        let source = ".button {\n  color: white;\n  padding: 8px 16px;\n  @include base;\n  &:hover { color: blue; }\n}";
        let node = path_synthetic_node("0.0".into(), "rule_set", ".button".into(),
            NodePosition { start_line: 0, start_col: 0, end_line: 5, end_col: 1 }, vec![]);
        let children = enrich_style_children(&node, &source.lines().collect::<Vec<_>>(), vec![], ".button", "scss");
        let color = &children[0];
        assert_eq!((color.position.start_line, color.position.start_col), (1, 2));
        assert_eq!((color.position.end_line, color.position.end_col), (1, 15));
        let value = &color.children[0];
        assert_eq!((value.position.start_line, value.position.start_col, value.position.end_col), (1, 9, 14));
        assert_eq!((children[2].position.start_line, children[2].position.start_col), (3, 2));
        assert_eq!((children[3].position.start_line, children[3].position.start_col), (4, 2));
        assert_eq!((children[4].position.start_line, children[4].position.start_col), (4, 12));
    }

    #[test]
    fn style_value_span_uses_utf8_bytes_and_declaration_offset() {
        let source = "é $name: café;";
        let node = path_synthetic_node("0.0".into(), "declaration", "$name".into(),
            NodePosition { start_line: 0, start_col: 3, end_line: 0, end_col: source.len() as u32 }, vec![]);
        let children = enrich_style_children(&node, &[source], vec![], "$name", "scss");
        assert_eq!(children[0].label, "café");
        assert_eq!((children[0].position.start_col, children[0].position.end_col), (10, 15));
    }
}

#[cfg(test)]
mod mdx_code_body_source_span_tests {
    use super::*;

    fn fenced_node(lines: &[&str]) -> SemanticNode {
        path_synthetic_node(
            "0.0".into(), "code_block", "bash".into(),
            NodePosition {
                start_line: 0, start_col: 0,
                end_line: (lines.len() - 1) as u32,
                end_col: lines.last().map_or(0, |line| line.len() as u32),
            },
            vec![],
        )
    }

    #[test]
    fn code_content_is_anchored_to_actual_body_bytes_not_opening_fence() {
        let lines = ["```bash", "npm install my-package", "```"];
        let node = fenced_node(&lines);
        let children = enrich_mdx_children(&node, &lines, vec![]);
        assert_eq!(children.len(), 1);
        let body = &children[0];
        assert_eq!(body.node_type, "code_content");
        assert_eq!(body.label, "npm install my-package");
        assert_eq!(
            (body.position.start_line, body.position.start_col,
             body.position.end_line, body.position.end_col),
            (1, 0, 1, lines[1].len() as u32),
        );
    }

    #[test]
    fn code_content_multiline_utf8_span_tracks_original_indentation() {
        let lines = ["```bash", "  echo café", "    echo 😀", "```"];
        let node = fenced_node(&lines);
        let children = enrich_mdx_children(&node, &lines, vec![]);
        assert_eq!(children.len(), 1);
        let body = &children[0];
        assert_eq!(body.label, "echo café\n    echo 😀");
        assert_eq!(
            (body.position.start_line, body.position.start_col,
             body.position.end_line, body.position.end_col),
            (1, 2, 2, lines[2].len() as u32),
        );
    }

    #[test]
    fn unclosed_code_fences_retain_final_content_and_utf8_span() {
        for lines in [
            vec!["```bash", "echo hello"],
            vec!["~~~bash", "echo first", "  echo café"],
            vec!["````bash", "echo first", "```"],
            vec!["```bash", "echo first", "~~~"],
            vec!["```bash", "echo first", "```not a closing fence"],
        ] {
            let children = enrich_mdx_children(&fenced_node(&lines), &lines, vec![]);
            assert_eq!(children.len(), 1, "{lines:?}");
            let body = &children[0];
            assert_eq!(body.label, lines[1..].join("\n"));
            assert_eq!(body.position.start_line, 1);
            assert_eq!(body.position.start_col, 0);
            assert_eq!(body.position.end_line, (lines.len() - 1) as u32);
            assert_eq!(body.position.end_col, lines.last().unwrap().len() as u32);
        }
    }

    #[test]
    fn matching_longer_closing_fence_is_excluded_from_content() {
        let lines = ["~~~bash", "echo hello", "  ~~~~  "];
        let children = enrich_mdx_children(&fenced_node(&lines), &lines, vec![]);
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].label, "echo hello");
        assert_eq!(children[0].position.end_line, 1);
    }

    #[test]
    fn empty_code_fence_cannot_manufacture_content_evidence() {
        let lines = ["```bash", "   ", "```"];
        let node = fenced_node(&lines);
        assert!(enrich_mdx_children(&node, &lines, vec![]).is_empty());
    }
}

#[cfg(test)]
mod mdx_unclosed_fence_regression_tests {
    use super::*;

    fn code_content(source: &str, end_line: u32) -> SemanticNode {
        let lines: Vec<&str> = source.lines().collect();
        let node = path_synthetic_node(
            "0.0".into(), "code_block", String::new(),
            NodePosition {
                start_line: 0, start_col: 0,
                end_line, end_col: lines[end_line as usize].len() as u32,
            },
            vec![],
        );
        enrich_mdx_children(&node, &lines, vec![])
            .into_iter().find(|child| child.node_type == "code_content")
            .expect("MDX body must survive enrichment")
    }

    #[test]
    fn unclosed_single_line_body_at_eof_is_not_discarded() {
        let body = code_content("```bash\necho hello", 1);
        assert_eq!(body.label, "echo hello");
        assert_eq!(
            (body.position.start_line, body.position.start_col,
             body.position.end_line, body.position.end_col),
            (1, 0, 1, 10),
        );
    }

    #[test]
    fn unclosed_multiline_body_preserves_final_line_and_utf8_byte_spans() {
        let body = code_content("```sh\n  echo hello\n  echo café", 2);
        assert_eq!(body.label, "echo hello\n  echo café");
        assert_eq!(
            (body.position.start_line, body.position.start_col,
             body.position.end_line, body.position.end_col),
            (1, 2, 2, 12),
        );
    }

    #[test]
    fn closed_fence_still_excludes_matching_delimiter_line() {
        let body = code_content("````sh\n  echo hello\n````", 2);
        assert_eq!(body.label, "echo hello");
        assert_eq!(
            (body.position.start_line, body.position.start_col,
             body.position.end_line, body.position.end_col),
            (1, 2, 1, 12),
        );
    }
}
