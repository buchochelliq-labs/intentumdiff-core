//! HTML path-profile keying (issues #57/#64), extracted from lib.rs verbatim
//! (issue #29 monolith split, phase B). Elements key by their element path with
//! identity attributes beating same-tag ordinals.

use crate::*;

// ── HTML path-profile keying (issue #57/#64) — mirrors python path_profiles' markup half for
// html only (css/scss/xml/mdx route green without it). Elements key by their ELEMENT PATH where
// a segment with an identity attribute (id/name/key/data-testid/aria-label) is `tag#id=hero`
// (beating same-tag ordinals), so a text edit inside an identity-bearing element pairs even when
// siblings are inserted above it.

pub(crate) const HTML_ELEMENT_TYPES: &[&str] = &["element", "script_element", "style_element"];
pub(crate) const HTML_TAG_TYPES: &[&str] = &["end_tag", "self_closing_tag", "start_tag"];
pub(crate) const HTML_IDENTITY_ATTRS: &[&str] = &["id", "name", "key", "data-testid", "aria-label"];

pub(crate) fn html_normalize_text(label: &str) -> String {
    label.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

pub(crate) fn html_attribute_parts(label: &str) -> (String, String) {
    let text = label.trim();
    match text.split_once('=') {
        Some((name, value)) => (
            html_normalize_text(name),
            value.trim().trim_matches(|c| c == '"' || c == '\'').to_string(),
        ),
        None => (html_normalize_text(text), String::new()),
    }
}

pub(crate) fn html_markup_identity(node: &SemanticNode) -> String {
    for child in &node.children {
        if !HTML_TAG_TYPES.contains(&child.node_type.to_lowercase().as_str()) {
            continue;
        }
        let attrs: Vec<(String, String)> = child
            .children
            .iter()
            .filter(|a| a.node_type.to_lowercase() == "attribute")
            .map(|a| html_attribute_parts(&a.label))
            .collect();
        for name in HTML_IDENTITY_ATTRS {
            for (attr_name, attr_value) in &attrs {
                if attr_name == name && !attr_value.is_empty() {
                    return format!("{}={}", name, html_normalize_text(attr_value));
                }
            }
        }
    }
    String::new()
}

pub(crate) fn html_segment(node: &SemanticNode, by_id: &HashMap<&str, &SemanticNode>) -> String {
    let tag = html_normalize_text(&node.label);
    let identity = html_markup_identity(node);
    if !identity.is_empty() {
        return format!("{tag}#{identity}");
    }
    let ordinal = node
        .id
        .rsplit_once('.')
        .and_then(|(parent_id, _)| by_id.get(parent_id).copied())
        .map(|parent| {
            let mut ordinal = 0usize;
            for sibling in &parent.children {
                if sibling.id == node.id {
                    break;
                }
                if sibling.node_type == node.node_type
                    && html_normalize_text(&sibling.label) == tag
                {
                    ordinal += 1;
                }
            }
            ordinal
        })
        .unwrap_or(0);
    format!("{tag}[{ordinal}]")
}

pub(crate) fn html_element_path(node: &SemanticNode, by_id: &HashMap<&str, &SemanticNode>) -> Vec<String> {
    let mut lineage: Vec<&SemanticNode> = Vec::new();
    let mut current = node.id.clone();
    while let Some((parent_id, _)) = current.rsplit_once('.') {
        if let Some(ancestor) = by_id.get(parent_id).copied() {
            if HTML_ELEMENT_TYPES.contains(&ancestor.node_type.to_lowercase().as_str()) {
                lineage.push(ancestor);
            }
        }
        current = parent_id.to_string();
    }
    lineage.reverse();
    lineage.push(node);
    lineage.into_iter().map(|n| html_segment(n, by_id)).collect()
}

/// python path_profiles._markup_key (html slice).
pub(crate) fn html_path_key(node: &SemanticNode, by_id: &HashMap<&str, &SemanticNode>) -> Option<Vec<String>> {
    let node_type = node.node_type.to_lowercase();
    let generic = node.label.is_empty() || node.label == node.node_type;
    if HTML_ELEMENT_TYPES.contains(&node_type.as_str()) && !generic {
        let mut key = vec!["html".to_string(), "element".to_string()];
        key.extend(html_element_path(node, by_id));
        return Some(key);
    }
    if node_type == "attribute" && !generic {
        let owner = nearest_ancestor_of_types(node.id.as_str(), by_id, HTML_ELEMENT_TYPES)?;
        let mut key = vec!["html".to_string(), "attribute".to_string()];
        key.extend(html_element_path(owner, by_id));
        key.push(html_attribute_parts(&node.label).0);
        return Some(key);
    }
    if HTML_TAG_TYPES.contains(&node_type.as_str()) {
        let owner = nearest_ancestor_of_types(node.id.as_str(), by_id, HTML_ELEMENT_TYPES)?;
        let mut key = vec!["html".to_string(), "tag".to_string()];
        key.extend(html_element_path(owner, by_id));
        key.push(node_type);
        return Some(key);
    }
    if node_type == "doctype" {
        return Some(vec!["html".to_string(), "doctype".to_string()]);
    }
    if node_type == "text" {
        let owner = nearest_ancestor_of_types(node.id.as_str(), by_id, HTML_ELEMENT_TYPES)?;
        let mut key = vec!["html".to_string(), "text".to_string()];
        key.extend(html_element_path(owner, by_id));
        return Some(key);
    }
    None
}
