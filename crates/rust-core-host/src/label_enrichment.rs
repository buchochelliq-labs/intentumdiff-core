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
    let clip = |line: &str, from: Option<usize>, to: Option<usize>| -> String {
        let chars: Vec<char> = line.chars().collect();
        let from = from.unwrap_or(0).min(chars.len());
        let to = to.unwrap_or(chars.len()).min(chars.len());
        if from >= to {
            return String::new();
        }
        chars[from..to].iter().collect()
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
pub(crate) fn attributes_from_opening_tag(snippet: &str) -> Vec<(String, String)> {
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
                while k < chars.len() {
                    match chars[k] {
                        '>' => {
                            ok = true;
                            break;
                        }
                        '<' => break,
                        _ => k += 1,
                    }
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
        let name = path_normalize_text(&chars[start..i].iter().collect::<String>());
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

/// python path_profiles._direct_markup_text: fullmatch `<tag ...> text </tag>` (text has
/// no '<') or the CDATA variant; whitespace-compacted. Case-insensitive tag comparison.
pub(crate) fn direct_markup_text(snippet: &str, tag_label: &str) -> String {
    let text = snippet.trim();
    let lower = text.to_lowercase();
    let tag = tag_label.to_lowercase();
    let Some(rest) = lower.strip_prefix('<') else {
        return String::new();
    };
    let ws_trimmed = rest.len() - rest.trim_start().len();
    let rest_trim = rest.trim_start();
    let Some(after_tag) = rest_trim.strip_prefix(tag.as_str()) else {
        return String::new();
    };
    let gt_rel = match after_tag.find('>') {
        Some(idx)
            if idx == 0 || after_tag[..idx].starts_with(|c: char| c.is_whitespace()) =>
        {
            idx
        }
        _ => return String::new(),
    };
    if after_tag[..gt_rel].contains('<') {
        return String::new();
    }
    let open_len = 1 + ws_trimmed + tag.len() + gt_rel + 1;
    let body_and_close = &text[open_len..];
    let lower_body = &lower[open_len..];
    let close_start = match lower_body.rfind("</") {
        Some(idx) => idx,
        None => return String::new(),
    };
    let closing = lower_body[close_start + 2..].trim_start();
    let Some(after_close_tag) = closing.strip_prefix(tag.as_str()) else {
        return String::new();
    };
    if after_close_tag.trim() != ">" {
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

/// python path_profiles._style_items_from_source: line-parse a rule body into
/// (type, label, value) items; `@include`/`@extend`; nested `{`; scss `&` replaced
/// by the parent selector.
pub(crate) fn style_items_from_source(snippet: &str, parent_selector: &str) -> Vec<(String, String, String)> {
    let body = match (snippet.find('{'), snippet.rfind('}')) {
        (Some(open), Some(close)) if close > open => &snippet[open + 1..close],
        _ => return Vec::new(),
    };
    let mut result = Vec::new();
    for raw_line in body.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line == "}" {
            continue;
        }
        if let Some(rest) = line.strip_prefix("@include") {
            let name = rest
                .trim()
                .split('(')
                .next()
                .unwrap_or("")
                .trim_end_matches(';')
                .trim();
            if !name.is_empty() {
                result.push(("include_statement".to_string(), name.to_string(), String::new()));
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("@extend") {
            let name = rest.trim().trim_end_matches(';').trim();
            if !name.is_empty() {
                result.push(("extend_statement".to_string(), name.to_string(), String::new()));
            }
            continue;
        }
        if let Some(brace) = line.find('{') {
            let selector = line[..brace].trim().replace('&', parent_selector);
            if !selector.is_empty() {
                result.push(("rule_set".to_string(), compact_ws(&selector), String::new()));
            }
            let nested_body = &line[brace + 1..];
            if let Some(colon) = nested_body.find(':') {
                let nested_property = nested_body[..colon].trim();
                let nested_value = nested_body[colon + 1..]
                    .split('}')
                    .next()
                    .unwrap_or("")
                    .trim_end_matches(';')
                    .trim();
                if !nested_property.is_empty() {
                    result.push((
                        "declaration".to_string(),
                        nested_property.to_string(),
                        nested_value.to_string(),
                    ));
                }
            }
            continue;
        }
        if !line.starts_with('@') {
            if let Some(colon) = line.find(':') {
                let name = line[..colon].trim();
                let value = line[colon + 1..].trim_end_matches(';').trim();
                if !name.is_empty() {
                    result.push((
                        "declaration".to_string(),
                        name.to_string(),
                        value.to_string(),
                    ));
                }
            }
        }
    }
    result
}

/// python path_profiles._enrich_style_children.
pub(crate) fn enrich_style_children(
    node: &SemanticNode,
    source_lines: &[&str],
    mut children: Vec<SemanticNode>,
    label: &str,
) -> Vec<SemanticNode> {
    let node_type = node.node_type.to_lowercase();
    if matches!(node_type.as_str(), "declaration" | "variable_declaration") {
        let snippet = path_source_slice(source_lines, node);
        let value = match snippet.find(':') {
            Some(colon) => snippet[colon + 1..].trim().trim_end_matches(';').trim().to_string(),
            None => String::new(),
        };
        if value.is_empty()
            || children.iter().any(|c| c.node_type == "property_value")
        {
            return children;
        }
        children.push(path_synthetic_node(
            format!("{}.path_value", node.id),
            "property_value",
            value,
            path_synthetic_position(node),
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
    let snippet = path_source_slice(source_lines, node);
    for (index, (item_type, item_label, item_value)) in
        style_items_from_source(&snippet, label).into_iter().enumerate()
    {
        if existing.contains(&(item_type.clone(), item_label.clone())) {
            continue;
        }
        let item_children = if item_value.is_empty() {
            Vec::new()
        } else {
            vec![path_synthetic_node(
                format!("{}.path_{}.value", node.id, index),
                "property_value",
                item_value,
                path_synthetic_position(node),
                Vec::new(),
            )]
        };
        children.push(path_synthetic_node(
            format!("{}.path_{}", node.id, index),
            &item_type,
            item_label,
            path_synthetic_position(node),
            item_children,
        ));
    }
    children
}

/// python path_profiles._attribute_parts / _attribute_name.
pub(crate) fn attribute_name_of(label: &str) -> String {
    let text = label.trim();
    match text.find('=') {
        Some(eq) => path_normalize_text(&text[..eq]),
        None => path_normalize_text(text),
    }
}

/// python path_profiles._enrich_existing_markup_attributes: rewrite existing attribute
/// labels to name=value (boolean attributes stay name-only); recompute hashes on change.
pub(crate) fn enrich_existing_markup_attributes(
    nodes: Vec<SemanticNode>,
    attr_values: &HashMap<String, String>,
) -> Vec<SemanticNode> {
    nodes
        .into_iter()
        .map(|node| {
            let children = enrich_existing_markup_attributes(node.children.clone(), attr_values);
            let mut label = node.label.clone();
            if node.node_type.eq_ignore_ascii_case("attribute") {
                let name = attribute_name_of(&node.label);
                if let Some(value) = attr_values.get(&name) {
                    if !value.is_empty() && *value != name {
                        label = format!("{name}={value}");
                    } else {
                        label = name;
                    }
                }
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

/// python path_profiles._enrich_markup_attributes: re-inject attributes parsed from the
/// opening tag as synthetic `attribute` children (skipping xmlns/xmlns:*), preferring an
/// existing start/self-closing tag child; otherwise a synthetic start_tag is prepended.
pub(crate) fn enrich_markup_attributes(
    node: &SemanticNode,
    source_lines: &[&str],
    children: Vec<SemanticNode>,
    label: &str,
) -> Vec<SemanticNode> {
    let snippet = path_source_slice(source_lines, node);
    let attrs: Vec<(String, String)> = attributes_from_opening_tag(&snippet)
        .into_iter()
        .filter(|(name, _)| name != "xmlns" && !name.starts_with("xmlns:"))
        .collect();
    if attrs.is_empty() {
        return children;
    }
    let attr_values: HashMap<String, String> = attrs.iter().cloned().collect();
    let children = enrich_existing_markup_attributes(children, &attr_values);
    let existing_names: HashSet<String> = children
        .iter()
        .flat_map(|child| {
            std::iter::once(child)
                .chain(child.descendants())
                .filter(|d| d.node_type.eq_ignore_ascii_case("attribute"))
                .map(|d| attribute_name_of(&d.label))
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
    let mut children = enrich_markup_attributes(node, source_lines, children, label);
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
        path_synthetic_position(node),
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
    let snippet = path_source_slice(source_lines, node);
    let mut body = {
        let lines: Vec<&str> = snippet.lines().collect();
        if lines.len() >= 2 && lines[0].trim_start().starts_with("```") {
            let inner: &[&str] = if lines[lines.len() - 1].trim_start().starts_with("```") {
                &lines[1..lines.len() - 1]
            } else {
                &lines[1..]
            };
            inner
                .iter()
                .map(|l| l.trim_end())
                .collect::<Vec<_>>()
                .join("\n")
                .trim()
                .to_string()
        } else {
            String::new()
        }
    };
    if body.is_empty() {
        let start_line = node.position.start_line as usize;
        if start_line < source_lines.len()
            && source_lines[start_line].trim_start().starts_with("```")
        {
            let mut collected: Vec<String> = Vec::new();
            for line in &source_lines[start_line + 1..] {
                if line.trim_start().starts_with("```") {
                    break;
                }
                collected.push(line.trim_end().to_string());
            }
            body = collected.join("\n").trim().to_string();
        }
    }
    if body.is_empty() {
        return children;
    }
    children.push(path_synthetic_node(
        format!("{}.path_code", node.id),
        "code_content",
        body,
        path_synthetic_position(node),
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
        children = enrich_style_children(node, source_lines, children, &label);
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
    let structural_hash = synthetic_structural_hash(&node.node_type, &label, &children);
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
