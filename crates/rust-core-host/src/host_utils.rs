//! Bounded tree utilities shared by native Wasm hosts and language bindings.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashSet;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Limits {
    pub max_bytes: usize,
    pub max_depth: usize,
    pub max_nodes: usize,
    pub max_trivia_types: usize,
    pub max_trivia_type_bytes: usize,
    pub max_trivia_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bytes: 8 * 1024 * 1024,
            max_depth: 256,
            max_nodes: 1_000_000,
            max_trivia_types: 1024,
            max_trivia_type_bytes: 256,
            max_trivia_bytes: 64 * 1024,
        }
    }
}

fn parse(text: &str, limits: &Limits) -> Result<Value, String> {
    // Callers can tighten limits but cannot disable the shared safety envelope.
    let ceiling = Limits::default();
    if text.len() > limits.max_bytes.min(ceiling.max_bytes) {
        return Err("host-utils JSON payload exceeds byte limit".into());
    }
    let max_depth = limits.max_depth.min(ceiling.max_depth);
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for byte in text.bytes() {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
        } else {
            match byte {
                b'"' => in_string = true,
                b'[' | b'{' => {
                    depth += 1;
                    if depth > max_depth {
                        return Err("host-utils JSON nesting depth exceeds limit".into());
                    }
                }
                b']' | b'}' => depth = depth.saturating_sub(1),
                _ => (),
            }
        }
    }
    let mut deserializer = serde_json::Deserializer::from_str(text);
    deserializer.disable_recursion_limit();
    let root = Value::deserialize(&mut deserializer)
        .map_err(|e| format!("host-utils invalid JSON: {e}"))?;
    deserializer
        .end()
        .map_err(|e| format!("host-utils invalid JSON: {e}"))?;
    let mut stack = vec![(&root, 1usize)];
    let mut count = 0;
    while let Some((value, depth)) = stack.pop() {
        count += 1;
        if count > limits.max_nodes.min(ceiling.max_nodes) {
            return Err("host-utils JSON node count exceeds limit".into());
        }
        if depth > max_depth {
            return Err("host-utils JSON nesting depth exceeds limit".into());
        }
        match value {
            Value::Object(m) => stack.extend(m.values().map(|v| (v, depth + 1))),
            Value::Array(a) => stack.extend(a.iter().map(|v| (v, depth + 1))),
            _ => (),
        }
    }
    let mut nodes = vec![&root];
    while let Some(node) = nodes.pop() {
        if !node.is_object() {
            return Err("host-utils tree nodes must be objects".into());
        }
        for field in ["type", "node_type", "text", "label"] {
            if node.get(field).is_some_and(|v| !v.is_string()) {
                return Err(format!("host-utils node {field} must be a string"));
            }
        }
        if let Some(children) = node.get("children") {
            let children = children
                .as_array()
                .ok_or("host-utils children must be an array")?;
            nodes.extend(children);
        }
    }
    Ok(root)
}

fn node_type(node: &Value) -> &str {
    node.get("node_type")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or_else(|| node.get("type").and_then(Value::as_str))
        .unwrap_or("")
}

/// Strip only child-tree nodes. Removing the root yields JSON null, never the unfiltered tree.
pub fn strip_trivia(text: &str, trivia_types: &[String], limits: &Limits) -> Result<Value, String> {
    let ceiling = Limits::default();
    if trivia_types.len() > limits.max_trivia_types.min(ceiling.max_trivia_types) {
        return Err("host-utils trivia type count exceeds limit".into());
    }
    let mut total = 0usize;
    for kind in trivia_types {
        if kind.len()
            > limits
                .max_trivia_type_bytes
                .min(ceiling.max_trivia_type_bytes)
        {
            return Err("host-utils trivia type exceeds byte limit".into());
        }
        total += kind.len();
    }
    if total > limits.max_trivia_bytes.min(ceiling.max_trivia_bytes) {
        return Err("host-utils trivia type payload exceeds byte limit".into());
    }
    let mut root = parse(text, limits)?;
    let trivia: HashSet<&str> = trivia_types.iter().map(String::as_str).collect();
    fn strip(node: &mut Value, trivia: &HashSet<&str>) -> bool {
        if node.is_object() && trivia.contains(node_type(node)) {
            return false;
        }
        if let Some(children) = node.get_mut("children").and_then(Value::as_array_mut) {
            children.retain_mut(|child| strip(child, trivia));
        }
        true
    }
    if !strip(&mut root, &trivia) {
        root = Value::Null;
    }
    Ok(root)
}

/// Hash a CST or SemanticNode using the same type/text vocabulary and ordered children.
pub fn structural_hash(text: &str, limits: &Limits) -> Result<String, String> {
    let root = parse(text, limits)?;
    fn hash(node: &Value) -> Result<String, String> {
        if !node.is_object() {
            return Err("host-utils tree nodes must be objects".into());
        }
        let payload = if let Some(children) = node
            .get("children")
            .and_then(Value::as_array)
            .filter(|c| !c.is_empty())
        {
            let child_hashes = children.iter().map(hash).collect::<Result<Vec<_>, _>>()?;
            format!("{}|{}", node_type(node), child_hashes.join("|"))
        } else {
            let label = node.get("label").or_else(|| node.get("text"));
            let text = match label {
                None => "",
                Some(Value::String(s)) => s,
                _ => return Err("host-utils node text must be a string".into()),
            };
            format!("{}:{text}", node_type(node))
        };
        Ok(hex::encode(Sha256::digest(payload.as_bytes())))
    }
    hash(&root)
}

#[cfg(test)]
mod regression_tests {
    use super::*;
    #[test]
    fn malformed_tree_children_fail_explicitly() {
        for text in [
            r#"{"type":"root","children":42}"#,
            r#"{"type":"root","children":"secret"}"#,
            r#"{"type":"root","children":[42]}"#,
        ] {
            assert!(structural_hash(text, &Limits::default()).is_err(), "{text}");
            assert!(
                strip_trivia(text, &[], &Limits::default()).is_err(),
                "{text}"
            );
        }
    }
}
