// Extracted verbatim from lib.rs (issue #85, lib.rs split round 2).
use crate::*;

pub(crate) fn keyed_generic_label(label: &str, node_type: &str) -> bool {
    let text = label.trim();
    if text.is_empty() {
        return true;
    }
    let lowered = text.to_lowercase();
    lowered == node_type.to_lowercase()
        || matches!(
            lowered.as_str(),
            "array" | "block_mapping" | "block_node" | "block_sequence" | "document"
                | "flow_mapping" | "flow_node" | "flow_sequence" | "job" | "object"
                | "pipeline" | "stream"
        )
}

/// python keyed_profiles.normalize_keyed_identity: strip matching quotes, drop a trailing
/// parenthesised suffix, drop everything from the first `:`, lowercase.
pub(crate) fn normalize_keyed_identity(label: &str) -> String {
    let mut text = label.trim();
    if text.len() >= 2
        && ((text.starts_with('"') && text.ends_with('"'))
            || (text.starts_with('\'') && text.ends_with('\'')))
    {
        text = text[1..text.len() - 1].trim();
    }
    let mut owned = text.to_string();
    if owned.ends_with(')') {
        if let Some(open) = owned.rfind('(') {
            owned.truncate(open);
            owned = owned.trim_end().to_string();
        }
    }
    if let Some(colon) = owned.find(':') {
        owned.truncate(colon);
        owned = owned.trim_end().to_string();
    }
    owned.to_lowercase()
}

pub(crate) fn keyed_parent_type<'a>(node: &SemanticNode, by_id: &HashMap<&str, &'a SemanticNode>) -> String {
    node.id
        .rsplit_once('.')
        .and_then(|(parent_id, _)| by_id.get(parent_id).copied())
        .map(|p| p.node_type.to_lowercase())
        .unwrap_or_default()
}

/// python `_json_ancestor_path` / `_yaml_ancestor_path` (per-language pair/item vocabularies).
pub(crate) fn keyed_ancestor_path(
    node: &SemanticNode,
    by_id: &HashMap<&str, &SemanticNode>,
    language: &str,
) -> Vec<String> {
    let mut labels: Vec<String> = Vec::new();
    let mut current = node.id.clone();
    while let Some((parent_id, _)) = current.rsplit_once('.') {
        if let Some(ancestor) = by_id.get(parent_id).copied() {
            let at = ancestor.node_type.to_lowercase();
            if language == "json" {
                if at == "pair" && !ancestor.label.is_empty() {
                    labels.push(normalize_keyed_identity(&ancestor.label));
                } else if at == "object" && keyed_parent_type(ancestor, by_id) == "array" {
                    labels.push(format!("[{}]", json_array_item_identity(ancestor)));
                }
            } else {
                if matches!(at.as_str(), "block_mapping_pair" | "flow_pair")
                    && !ancestor.label.is_empty()
                {
                    labels.push(normalize_keyed_identity(&ancestor.label));
                } else if matches!(at.as_str(), "block_sequence_item" | "flow_sequence_item")
                    && !ancestor.label.is_empty()
                {
                    let identity = normalize_keyed_identity(&ancestor.label);
                    if !identity.is_empty()
                        && !keyed_generic_label(&ancestor.label, &ancestor.node_type)
                    {
                        labels.push(format!("[{identity}]"));
                    }
                }
            }
        }
        current = parent_id.to_string();
    }
    labels.retain(|l| !l.is_empty());
    labels.reverse();
    labels
}


/// Identity for a json array ITEM: a real (non-generic, non-positional) label when present,
/// else the CONTENT (structural-hash prefix) — `[0]`/`[1]` position labels are not identities,
/// and keying by them makes an inserted element re-identify every later sibling (package.json
/// commands insert read as 4 bogus string MODIFICATIONs).
pub(crate) fn json_array_item_identity(node: &SemanticNode) -> String {
    let label = node.label.trim();
    let positional = label.len() >= 3
        && label.starts_with('[')
        && label.ends_with(']')
        && label[1..label.len() - 1].chars().all(|c| c.is_ascii_digit());
    if !label.is_empty() && !positional && !keyed_generic_label(label, &node.node_type) {
        return normalize_keyed_identity(label);
    }
    format!("#{}", &node.structural_hash[..node.structural_hash.len().min(12)])
}

/// python keyed_profiles._json_key / _yaml_key.
pub(crate) fn keyed_data_key(
    node: &SemanticNode,
    by_id: &HashMap<&str, &SemanticNode>,
    language: &str,
) -> Option<Vec<String>> {
    let node_type = node.node_type.to_lowercase();
    let (pair_types, item_types): (&[&str], &[&str]) = if language == "json" {
        (&["pair"], &[])
    } else {
        (
            &["block_mapping_pair", "flow_pair"],
            &["block_sequence_item", "flow_sequence_item"],
        )
    };

    if pair_types.contains(&node_type.as_str()) && !node.label.is_empty() {
        let mut key = vec![language.to_string(), "pair".to_string()];
        key.extend(keyed_ancestor_path(node, by_id, language));
        key.push(normalize_keyed_identity(&node.label));
        return Some(key);
    }

    if language == "json" && node_type == "object" {
        if keyed_parent_type(node, by_id) == "array" {
            let array_parent = nearest_ancestor_of_types(node.id.as_str(), by_id, &["array"]);
            if let Some(array_parent) = array_parent {
                let mut key = vec!["json".to_string(), "array_item".to_string()];
                key.extend(keyed_ancestor_path(array_parent, by_id, language));
                key.push(json_array_item_identity(node));
                return Some(key);
            }
        }
    }

    if item_types.contains(&node_type.as_str()) && !node.label.is_empty() {
        let identity = normalize_keyed_identity(&node.label);
        if !identity.is_empty() && !keyed_generic_label(&node.label, &node.node_type) {
            let mut key = vec![language.to_string(), "sequence_item".to_string()];
            key.extend(keyed_ancestor_path(node, by_id, language));
            key.push(identity);
            return Some(key);
        }
    }

    // Array/sequence SCALARS pair by CONTENT (label + same-label ordinal), not position — an
    // inserted element must not cross-pair later unchanged values into bogus MODIFICATIONs
    // (package.json activationEvents insert). Python reaches the same end shape via the
    // presentation repair (_repair_shifted_array_scalar_insertions); keying by identity is the
    // routed equivalent at the true source. A single pair VALUE still pairs by role below, so
    // an edited scalar value stays a MODIFICATION.
    if node.children.is_empty() && !node.label.is_empty() {
        let container_types: &[&str] = if language == "json" {
            &["array"]
        } else {
            &["block_sequence", "flow_sequence"]
        };
        let parent_type = keyed_parent_type(node, by_id);
        if container_types.contains(&parent_type.as_str()) {
            if let Some((parent_id, _)) = node.id.rsplit_once('.') {
                if let Some(container) = by_id.get(parent_id).copied() {
                    let mut ordinal = 0usize;
                    for sibling in &container.children {
                        if sibling.id == node.id {
                            break;
                        }
                        if sibling.node_type == node.node_type && sibling.label == node.label {
                            ordinal += 1;
                        }
                    }
                    // Content-faithful identity: normalize_keyed_identity strips from the first
                    // ':' (built for `key: value` labels), which collapses every `onCommand:*`
                    // scalar to the same identity — use the quote-stripped label verbatim.
                    let mut identity = node.label.trim();
                    if identity.len() >= 2
                        && ((identity.starts_with('"') && identity.ends_with('"'))
                            || (identity.starts_with('\'') && identity.ends_with('\'')))
                    {
                        identity = identity[1..identity.len() - 1].trim();
                    }
                    let mut key = vec![language.to_string(), "array_scalar".to_string()];
                    key.extend(keyed_ancestor_path(node, by_id, language));
                    key.push(node.node_type.to_lowercase());
                    key.push(identity.to_string());
                    key.push(ordinal.to_string());
                    return Some(key);
                }
            }
        }
    }

    if let Some(nearest_pair) = nearest_ancestor_of_types(node.id.as_str(), by_id, pair_types) {
        let branch = direct_child_under(nearest_pair.id.as_str(), node, by_id)?;
        let mut path = vec![language.to_string()];
        let role_is_key = nearest_pair
            .children
            .first()
            .is_some_and(|first| first.id == branch.id);
        let anc = keyed_ancestor_path(nearest_pair, by_id, language);
        let pair_identity = normalize_keyed_identity(&nearest_pair.label);
        let is_leaf = node.children.is_empty();
        let value_container = if language == "json" {
            matches!(node_type.as_str(), "object" | "array")
        } else {
            matches!(
                node_type.as_str(),
                "block_node" | "flow_node" | "block_mapping" | "flow_mapping"
            )
        };
        if role_is_key {
            if node.id == branch.id || is_leaf {
                path.push("key".to_string());
                path.extend(anc);
                path.push(pair_identity);
                path.push(node_type);
                return Some(path);
            }
            return None;
        }
        if node.id == branch.id || is_leaf || value_container {
            path.push("value".to_string());
            path.extend(anc);
            path.push(pair_identity);
            path.push(node_type);
            return Some(path);
        }
    }

    None
}

/// python keyed_profiles.is_keyed_data_review_container (json/yaml subset).
pub(crate) fn is_keyed_review_container(node: &SemanticNode) -> bool {
    let node_type = node.node_type.to_lowercase();
    if matches!(node_type.as_str(), "pair" | "block_mapping_pair" | "flow_pair") {
        return true;
    }
    if matches!(
        node_type.as_str(),
        "block_sequence_item" | "flow_sequence_item" | "object"
    ) {
        return !node.label.is_empty() && !keyed_generic_label(&node.label, &node.node_type);
    }
    false
}



/// python presentation._style_groups_from_suppression for the routed path (issue #57
/// javascript): the js/ts style rule relabels SUPPRESSION residue as an IGNORED_STYLE group.
/// The routed refine suppresses noise without per-pass evidence groups, so the residue is
/// computed as the DELTA between the raw edit-script drafts and the surviving ones. Provenance
/// is "suppression" (issue #51): relabelled residue, never an equivalence proof.
pub(crate) fn js_style_group_from_suppression(
    before: &[(String, Option<String>, Option<String>, Vec<String>, Vec<String>)],
    changes: &[ChangeDraft<'_>],
    language: &str,
) -> Option<(Value, Value)> {
    if !matches!(language, "javascript" | "typescript" | "tsx") {
        return None;
    }
    let surviving: HashSet<(String, Option<String>, Option<String>)> = changes
        .iter()
        .map(|c| {
            (
                c.change_type.to_string(),
                c.old_node.map(|n| n.id.clone()),
                c.new_node.map(|n| n.id.clone()),
            )
        })
        .collect();
    let survived_endpoint_old: HashSet<&String> = changes
        .iter()
        .filter_map(|c| c.old_node.map(|n| &n.id))
        .collect();
    let survived_endpoint_new: HashSet<&String> = changes
        .iter()
        .filter_map(|c| c.new_node.map(|n| &n.id))
        .collect();
    let mut old_labels: Vec<String> = Vec::new();
    let mut new_labels: Vec<String> = Vec::new();
    let mut old_ids: Vec<String> = Vec::new();
    let mut new_ids: Vec<String> = Vec::new();
    for (change_type, old_id, new_id, olabels, nlabels) in before {
        if surviving.contains(&(change_type.clone(), old_id.clone(), new_id.clone())) {
            continue;
        }
        // An endpoint that still participates in ANY surviving change was transformed
        // (promoted/paired), not suppressed — only fully-vanished drafts are style residue.
        if old_id.as_ref().is_some_and(|id| survived_endpoint_old.contains(id))
            || new_id.as_ref().is_some_and(|id| survived_endpoint_new.contains(id))
        {
            continue;
        }
        // Discarded raw parent edits are *not* evidence of formatting when a
        // surviving paired semantic descendant represents the same change.
        // Rust's raw script may spell a modified string as DELETE old wrapper
        // + ADD new wrapper, as well as a parent MODIFICATION; all three are
        // refinement, not ignored style. The one-sided rule is deliberately
        // restricted to single-label literal wrappers: do not erase a broader
        // removed/added container with independent semantic content.
        let represented_by_precise_edit = changes.iter().any(|change| {
            let (Some(precise_old), Some(precise_new)) =
                (change.old_node, change.new_node) else {
                    return false;
                };
            if change.change_type != "MODIFICATION" {
                return false;
            }
            let descendant_of = |id: &str, parent: &str| {
                id.strip_prefix(parent).is_some_and(|tail| tail.starts_with('.'))
            };
            match (change_type.as_str(), old_id.as_deref(), new_id.as_deref()) {
                ("MODIFICATION", Some(old_parent), Some(new_parent)) => {
                    descendant_of(&precise_old.id, old_parent)
                        && descendant_of(&precise_new.id, new_parent)
                }
                ("DELETION", Some(old_parent), None) => {
                    !olabels.is_empty()
                        && olabels.iter().all(|label| label == &precise_old.label)
                        && descendant_of(&precise_old.id, old_parent)
                }
                ("ADDITION", None, Some(new_parent)) => {
                    !nlabels.is_empty()
                        && nlabels.iter().all(|label| label == &precise_new.label)
                        && descendant_of(&precise_new.id, new_parent)
                }
                _ => false,
            }
        });
        if represented_by_precise_edit {
            continue;
        }
        old_labels.extend(olabels.iter().cloned());
        new_labels.extend(nlabels.iter().cloned());
        old_ids.extend(old_id.iter().cloned());
        new_ids.extend(new_id.iter().cloned());
    }
    if old_ids.is_empty() && new_ids.is_empty() {
        return None;
    }
    // Only meaningful when at least one precise change survived to represent the edit.
    if !changes.iter().any(|c| c.change_type == "MODIFICATION") {
        return None;
    }
    let reason = "Formatting-only JavaScript call and argument wrapping were ignored because \
                  the review-level semantic changes are already represented by more precise \
                  changes.";
    let rule_id = "javascript.formatting.call_argument_wrapping_equivalence";
    old_labels.dedup();
    new_labels.dedup();
    let group = json!({
        "kind": "IGNORED_STYLE",
        "raw_change_indices": [],
        "old_labels": old_labels,
        "new_labels": new_labels,
        "old_node_ids": old_ids,
        "new_node_ids": new_ids,
        "confidence": 0.7,
        "rule_id": rule_id,
        "metadata": {
            "index_space": "mixed",
            "reason": reason,
            "equivalence_kind": "syntactic_trivia",
            "risk": "amber",
            "language": language,
        },
    });
    let ignored = json!({
        "language": language,
        "rule_id": rule_id,
        "reason": reason,
        "equivalence_kind": "syntactic_trivia",
        "risk": "amber",
        "provenance": "suppression",
    });
    Some((group, ignored))
}

/// A full-document proof is required before presentation scaffolding can be removed. Comparing
/// labels (even positional item labels) says nothing about the values below those wrappers.
pub(crate) fn yaml_representation_equivalent_sources(old_source: &str, new_source: &str) -> bool {
    // The value decoder resolves aliases and does not retain anchor identities. Fail closed
    // across the complete sources, including tokens outside the surviving change drafts.
    // Directive/schema changes are also outside this YAML 1.2 proof. This deliberately also
    // declines markers inside strings/comments rather than risking an identity change.
    if [old_source, new_source].iter().any(|source| source.contains(['&', '*', '!', '%'])) {
        return false;
    }
    fn numeric_looking(text: &str) -> bool {
        static NUMBER: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        NUMBER.get_or_init(|| regex::Regex::new(
            r"(?ix)^[+-]?(?:0x[0-9a-f_]+|0o[0-7_]+|0b[01_]+|(?:[0-9][0-9_]*(?:\.[0-9_]*)?|\.[0-9][0-9_]*)(?:e[+-]?[0-9][0-9_]*)?|\.inf|\.nan)$"
        ).expect("valid numeric-looking YAML scalar pattern")).is_match(text)
    }
    fn supported(value: &serde_yaml::Value) -> bool {
        match value {
            // Distinct decimal values and keys can round to the same f64. Equality after that
            // conversion is not proof; leave float-containing documents to the ordinary diff.
            serde_yaml::Value::Number(number) => !number.is_f64(),
            // Unsupported integer ranges and overflowing float exponents can fall back to
            // String in this decoder. Such a value cannot prove that an unquoted numeric
            // scalar has the same YAML type as a quoted spelling (including mapping keys).
            serde_yaml::Value::String(text) => !numeric_looking(text),
            serde_yaml::Value::Sequence(items) => items.iter().all(supported),
            serde_yaml::Value::Mapping(mapping) => mapping.iter()
                .all(|(key, value)| supported(key) && supported(value)),
            serde_yaml::Value::Tagged(_) => false,
            _ => true,
        }
    }
    fn has_float(value: &serde_yaml::Value) -> bool {
        match value {
            serde_yaml::Value::Number(number) => number.is_f64(),
            serde_yaml::Value::Sequence(items) => items.iter().any(has_float),
            serde_yaml::Value::Mapping(mapping) => mapping.iter().any(|(k, v)| has_float(k) || has_float(v)),
            serde_yaml::Value::Tagged(tagged) => has_float(&tagged.value),
            _ => false,
        }
    }
    // Value decoding preserves scalar types, ordered sequence multiplicity, and full mapping
    // payloads. Duplicate keys, invalid input and multi-document streams return errors: none
    // may be treated as an equivalence proof.
    let decode = |source: &str| serde_yaml::from_str::<serde_yaml::Value>(source).ok();
    let (Some(mut old), Some(mut new)) = (decode(old_source), decode(new_source)) else {
        return false;
    };
    // #285: floats are compared EXACTLY. Each plain scalar the decoder itself resolves to a
    // float is replaced, in the source, by a quoted placeholder carrying its exact canonical
    // decimal (sign, significant digits, exponent), then the documents are decoded again.
    // f64 rounding can therefore never equate distinct decimals, while `1.5` / `1.50` /
    // `15e-1` (one YAML 1.2 core value) still compare equal across block/flow restyles. Any
    // float the scanner cannot place (block scalars, multi-line quotes) declines the proof.
    if has_float(&old) || has_float(&new) {
        let (Some(old_exact), Some(new_exact)) = (exact_float_source(old_source), exact_float_source(new_source)) else {
            return false;
        };
        let (Some(o), Some(n)) = (decode(&old_exact), decode(&new_exact)) else {
            return false;
        };
        // A placeholder is only sound as a WHOLE scalar. One that landed inside a longer
        // plain scalar (`apples 1.5`, a multi-line plain continuation) would equate distinct
        // strings (`1.5` vs `1.50`), so any embedded marker declines the proof.
        fn placeholders_whole(value: &serde_yaml::Value) -> bool {
            match value {
                serde_yaml::Value::String(text) => !text.contains(EXACT_FLOAT_MARK)
                    || (text.starts_with(EXACT_FLOAT_MARK) && text.matches(EXACT_FLOAT_MARK).count() == 1
                        && exact_decimal_form(&text[EXACT_FLOAT_MARK.len_utf8()..])),
                serde_yaml::Value::Sequence(items) => items.iter().all(placeholders_whole),
                serde_yaml::Value::Mapping(mapping) => mapping.iter().all(|(k, v)| placeholders_whole(k) && placeholders_whole(v)),
                serde_yaml::Value::Tagged(_) => false,
                _ => true,
            }
        }
        if !placeholders_whole(&o) || !placeholders_whole(&n) {
            return false;
        }
        (old, new) = (o, n);
    }
    // The decoder also maps an empty/comment-only stream (zero documents) to Null. Decline
    // top-level nulls rather than conflating that stream with a document whose root is null.
    !matches!(&old, serde_yaml::Value::Null)
        && !matches!(&new, serde_yaml::Value::Null)
        && supported(&old) && supported(&new) && old == new
}

/// Marker that starts an exact-float placeholder; sources already containing it decline.
const EXACT_FLOAT_MARK: char = '\u{E000}';

/// The exact canonical decimal of a YAML 1.2 core float spelling: `[-]digits e exponent` with
/// leading/trailing zeros removed (negative zero keeps its sign; `.inf`/`.nan` spelled out).
/// `None` for anything that is not a core-schema float spelling.
fn exact_decimal(token: &str) -> Option<String> {
    let lower = token.to_ascii_lowercase();
    let (negative, body) = match lower.as_bytes().first()? {
        b'-' => (true, &lower[1..]),
        b'+' => (false, &lower[1..]),
        _ => (false, lower.as_str()),
    };
    let sign = if negative { "-" } else { "" };
    if body == ".inf" {
        return Some(format!("{sign}inf"));
    }
    if body == ".nan" && sign.is_empty() && !token.starts_with('+') {
        return Some("nan".to_owned());
    }
    let (mantissa, exponent) = match body.split_once('e') {
        Some((m, e)) => (m, e.parse::<i64>().ok()?),
        None => (body, 0),
    };
    let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if (int.is_empty() && frac.is_empty()) || !int.chars().chain(frac.chars()).all(|c| c.is_ascii_digit()) {
        return None;
    }
    let digits = format!("{int}{frac}");
    let leading = digits.len() - digits.trim_start_matches('0').len();
    let significant = digits.trim_start_matches('0').trim_end_matches('0');
    if significant.is_empty() {
        return Some(format!("{sign}0"));
    }
    let trailing = digits.len() - leading - significant.len();
    // value = digits * 10^(exponent - frac.len()); drop trailing zeros into the exponent.
    let exp = exponent.checked_sub(frac.len() as i64)?.checked_add(trailing as i64)?;
    Some(format!("{sign}{significant}e{exp}"))
}

/// Whether `text` is exactly an `exact_decimal` output (`[-]digits e [-]digits`, `[-]0`,
/// `[-]inf`, `nan`).
fn exact_decimal_form(text: &str) -> bool {
    static FORM: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    FORM.get_or_init(|| regex::Regex::new(r"^(?:-?(?:[1-9][0-9]*e-?[0-9]+|0|inf)|nan)$").expect("valid exact decimal form"))
        .is_match(text)
}

/// `source` with every plain scalar that the YAML decoder resolves to a float replaced by a
/// double-quoted exact placeholder. `None` when the scan cannot be certain: block scalars,
/// quotes left open at end of line, or the marker already present.
fn exact_float_source(source: &str) -> Option<String> {
    if source.contains(EXACT_FLOAT_MARK) {
        return None;
    }
    let mut out = String::with_capacity(source.len() + 16);
    for line in source.split_inclusive('\n') {
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        let mut token_start = true;
        while i < chars.len() {
            let c = chars[i];
            if token_start && (c == '"' || c == '\'') {
                // A quoted scalar: copied verbatim, never a float.
                let mut j = i + 1;
                loop {
                    let Some(&q) = chars.get(j) else { return None }; // open at end of line
                    if c == '"' && q == '\\' { j += 2; continue; }
                    if q == c {
                        if c == '\'' && chars.get(j + 1) == Some(&'\'') { j += 2; continue; }
                        break;
                    }
                    j += 1;
                }
                out.extend(&chars[i..=j]);
                i = j + 1;
                token_start = false;
                continue;
            }
            if c == '#' && token_start {
                out.extend(&chars[i..]); // comment to end of line
                break;
            }
            if token_start && (c == '|' || c == '>') {
                return None; // block scalar content is not scanned
            }
            if c.is_whitespace() || matches!(c, ',' | '[' | ']' | '{' | '}') {
                out.push(c);
                i += 1;
                token_start = true;
                continue;
            }
            if !token_start {
                out.push(c);
                i += 1;
                continue;
            }
            // A plain token: up to whitespace / flow indicator; a trailing ':' is a key marker.
            let mut j = i;
            while j < chars.len() && !chars[j].is_whitespace() && !matches!(chars[j], ',' | '[' | ']' | '{' | '}') {
                j += 1;
            }
            let raw: String = chars[i..j].iter().collect();
            let (token, suffix) = match raw.strip_suffix(':') {
                Some(token) => (token, ":"),
                None => (raw.as_str(), ""),
            };
            let decoded_float = serde_yaml::from_str::<serde_yaml::Value>(token)
                .is_ok_and(|v| matches!(v, serde_yaml::Value::Number(ref n) if n.is_f64()));
            match exact_decimal(token).filter(|_| decoded_float) {
                Some(exact) => {
                    out.push('"');
                    out.push(EXACT_FLOAT_MARK);
                    out.push_str(&exact);
                    out.push('"');
                    out.push_str(suffix);
                }
                None => out.push_str(&raw),
            }
            i = j;
            token_start = false;
        }
    }
    Some(out)
}

/// python presentation._suppress_yaml_representation_equivalent_modifications (issue #57 yaml):
/// block and flow styles present the SAME representation graph (YAML 1.2 §3), so style-wrapper
/// churn (block_node↔flow_node MODIFICATIONs, scaffold item ADD/DELETEs with generic or
/// positional labels, same-label scalar MOVEs) suppresses with evidence. Anchors/aliases/tags
/// (&, *, !) change identity, not presentation — guarded out.
pub(crate) fn suppress_yaml_representation_equivalent_drafts(
    changes: &mut Vec<ChangeDraft<'_>>,
    old_source: &str,
    new_source: &str,
) -> Option<Value> {
    if !yaml_representation_equivalent_sources(old_source, new_source) {
        return None;
    }
    const SCAFFOLD: &[&str] = &[
        "block_node", "flow_node", "block_sequence", "flow_sequence", "block_mapping",
        "flow_mapping", "block_sequence_item", "flow_sequence_item",
        "block_mapping_pair", "flow_pair",
    ];
    const SCALARS: &[&str] = &[
        "plain_scalar", "single_quote_scalar", "double_quote_scalar", "integer_scalar",
        "float_scalar", "boolean_scalar", "null_scalar", "string",
    ];
    fn cross_style(a: &str, b: &str) -> bool {
        matches!(
            (a, b),
            ("block_node", "flow_node") | ("flow_node", "block_node")
                | ("block_sequence", "flow_sequence") | ("flow_sequence", "block_sequence")
                | ("block_mapping", "flow_mapping") | ("flow_mapping", "block_mapping")
                | ("block_mapping_pair", "flow_pair") | ("flow_pair", "block_mapping_pair")
        )
    }
    fn has_identity_tokens(node: &SemanticNode) -> bool {
        std::iter::once(node)
            .chain(node.descendants())
            .any(|n| n.label.contains('&') || n.label.contains('*') || n.label.contains('!'))
    }
    let generic_or_equal = |old: &SemanticNode, new: &SemanticNode| {
        old.label == new.label
            || (old.label == old.node_type && new.label == new.node_type)
    };
    let mut suppressed_labels: Vec<String> = Vec::new();
    changes.retain(|change| {
        match change.change_type {
            "MODIFICATION" | "MOVE" => {
                let (Some(old_node), Some(new_node)) = (change.old_node, change.new_node) else {
                    return true;
                };
                if !generic_or_equal(old_node, new_node) {
                    return true;
                }
                if has_identity_tokens(old_node) || has_identity_tokens(new_node) {
                    return true;
                }
                let ot = old_node.node_type.to_lowercase();
                let nt = new_node.node_type.to_lowercase();
                let matched = if change.change_type == "MODIFICATION" {
                    cross_style(&ot, &nt) || (ot == nt && SCAFFOLD.contains(&ot.as_str()))
                } else {
                    // MOVEs: scalar positional shifts AND scaffold wrappers relocated by the
                    // presentation change (a flow_node "moving" when its container restyles).
                    (ot == nt && SCALARS.contains(&ot.as_str()))
                        || (cross_style(&ot, &nt)
                            || (ot == nt && SCAFFOLD.contains(&ot.as_str())))
                };
                if matched {
                    suppressed_labels.push(old_node.label.clone());
                    return false;
                }
                true
            }
            "DELETION" | "ADDITION" => {
                let node = change.old_node.or(change.new_node);
                let Some(node) = node else { return true };
                let nt = node.node_type.to_lowercase();
                if !SCAFFOLD.contains(&nt.as_str()) {
                    return true;
                }
                if has_identity_tokens(node) {
                    return true;
                }
                let positional = node.label.starts_with('[');
                let pair = matches!(nt.as_str(), "block_mapping_pair" | "flow_pair");
                if !(node.label == node.node_type || positional || pair) {
                    return true;
                }
                suppressed_labels.push(node.label.clone());
                false
            }
            _ => true,
        }
    });
    if suppressed_labels.is_empty() {
        return None;
    }
    Some(json!({
        "kind": "NOISE_SUPPRESSED",
        "raw_change_indices": [],
        "old_labels": suppressed_labels,
        "new_labels": [],
        "old_node_ids": [],
        "new_node_ids": [],
        "confidence": 0.9,
        "rule_id": "presentation.suppress_yaml_representation_equivalent_modification",
        "metadata": {"equivalence": "yaml_representation_graph"},
    }))
}

/// python keyed_profiles.augment_keyed_data_matching: drop cross-key positional matches, then
/// pair unmatched keyed nodes by identical key (position order).
pub(crate) fn augment_keyed_data_matching<'a>(
    old_tree: &'a SemanticNode,
    new_tree: &'a SemanticNode,
    matching: Vec<MatchPair<'a>>,
    language: &str,
) -> Vec<MatchPair<'a>> {
    if !matches!(language, "json" | "yaml") {
        return matching;
    }
    let old_by_id = semantic_node_refs_by_id_with_root(old_tree);
    let new_by_id = semantic_node_refs_by_id_with_root(new_tree);
    let old_keys: HashMap<&str, Vec<String>> = old_by_id
        .iter()
        .filter_map(|(id, node)| keyed_data_key(node, &old_by_id, language).map(|k| (*id, k)))
        .collect();
    let new_keys: HashMap<&str, Vec<String>> = new_by_id
        .iter()
        .filter_map(|(id, node)| keyed_data_key(node, &new_by_id, language).map(|k| (*id, k)))
        .collect();

    let mut result: Vec<MatchPair<'a>> = Vec::new();
    let mut matched_old: HashSet<String> = HashSet::new();
    let mut matched_new: HashSet<String> = HashSet::new();
    for pair in matching {
        let ok = old_keys.get(pair.old_node.id.as_str());
        let nk = new_keys.get(pair.new_node.id.as_str());
        if (ok.is_some() || nk.is_some()) && (ok.is_none() || nk.is_none() || ok != nk) {
            continue;
        }
        if matched_old.contains(pair.old_node.id.as_str())
            || matched_new.contains(pair.new_node.id.as_str())
        {
            continue;
        }
        matched_old.insert(pair.old_node.id.clone());
        matched_new.insert(pair.new_node.id.clone());
        result.push(pair);
    }

    let mut new_by_key: HashMap<&Vec<String>, Vec<&SemanticNode>> = HashMap::new();
    for (id, key) in &new_keys {
        if matched_new.contains(*id) {
            continue;
        }
        if let Some(node) = new_by_id.get(id).copied() {
            new_by_key.entry(key).or_default().push(node);
        }
    }
    for nodes in new_by_key.values_mut() {
        nodes.sort_by(|a, b| resource_node_sort_key(a).cmp(&resource_node_sort_key(b)));
    }
    let mut old_nodes: Vec<&SemanticNode> = old_keys
        .keys()
        .filter_map(|id| old_by_id.get(id).copied())
        .collect();
    old_nodes.sort_by(|a, b| resource_node_sort_key(a).cmp(&resource_node_sort_key(b)));
    for old_node in old_nodes {
        if matched_old.contains(old_node.id.as_str()) {
            continue;
        }
        let Some(key) = old_keys.get(old_node.id.as_str()) else {
            continue;
        };
        let Some(candidates) = new_by_key.get_mut(key) else {
            continue;
        };
        if candidates.is_empty() {
            continue;
        }
        let new_node = candidates.remove(0);
        matched_old.insert(old_node.id.clone());
        matched_new.insert(new_node.id.clone());
        result.push(MatchPair { old_node, new_node });
    }
    result
}

/// python keyed_profiles.augment_keyed_data_changes: recover keyed review-container ADD/DELETEs
/// hidden by coarse container matches (unmatched-ancestor guarded).
pub(crate) fn augment_keyed_data_changes_drafts<'a>(
    changes: &mut Vec<ChangeDraft<'a>>,
    old_tree: &'a SemanticNode,
    new_tree: &'a SemanticNode,
    language: &str,
    old_source: &str,
    new_source: &str,
) -> Option<Value> {
    if !matches!(language, "json" | "yaml") {
        return None;
    }
    // Positional-label churn suppression (python suppress_array_index_only_modification):
    // an array element with IDENTICAL content whose positional label shifted after a sibling
    // insertion ([1]->[2]) is not a change — drop the MODIFICATION/REORDER pair noise,
    // carrying the suppression as evidence.
    // A same-hash YAML node can also move because sequence order changed. Its own content
    // cannot prove that its ordered context stayed equivalent, so require the document proof
    // here too. On meaningful edits this deliberately retains positional noise with evidence.
    let suppress_positional = language != "yaml"
        || yaml_representation_equivalent_sources(old_source, new_source);
    let mut suppressed_labels: Vec<String> = Vec::new();
    changes.retain(|change| {
        if !suppress_positional {
            return true;
        }
        if !matches!(change.change_type, "MODIFICATION" | "REORDER" | "MOVE") {
            return true;
        }
        let (Some(old_node), Some(new_node)) = (change.old_node, change.new_node) else {
            return true;
        };
        if old_node.structural_hash == new_node.structural_hash {
            suppressed_labels.push(old_node.label.clone());
            return false;
        }
        true
    });
    let old_by_id = semantic_node_refs_by_id_with_root(old_tree);
    let new_by_id = semantic_node_refs_by_id_with_root(new_tree);
    let old_keys: HashMap<&str, Vec<String>> = old_by_id
        .iter()
        .filter_map(|(id, node)| keyed_data_key(node, &old_by_id, language).map(|k| (*id, k)))
        .collect();
    let new_keys: HashMap<&str, Vec<String>> = new_by_id
        .iter()
        .filter_map(|(id, node)| keyed_data_key(node, &new_by_id, language).map(|k| (*id, k)))
        .collect();
    let old_key_set: HashSet<&Vec<String>> = old_keys.values().collect();
    let new_key_set: HashSet<&Vec<String>> = new_keys.values().collect();
    let mentioned_old: HashSet<&str> = changes
        .iter()
        .filter_map(|c| c.old_node.map(|n| n.id.as_str()))
        .collect();
    let mentioned_new: HashSet<&str> = changes
        .iter()
        .filter_map(|c| c.new_node.map(|n| n.id.as_str()))
        .collect();
    let has_unmatched_ancestor = |node: &SemanticNode,
                                  by_id: &HashMap<&str, &SemanticNode>,
                                  keys: &HashMap<&str, Vec<String>>,
                                  opposite: &HashSet<&Vec<String>>| {
        let mut current = node.id.clone();
        while let Some((parent_id, _)) = current.rsplit_once('.') {
            if let Some(parent) = by_id.get(parent_id).copied() {
                if let Some(key) = keys.get(parent_id) {
                    if !opposite.contains(key) && is_keyed_review_container(parent) {
                        return true;
                    }
                }
            }
            current = parent_id.to_string();
        }
        false
    };
    let mut recovered: Vec<ChangeDraft<'a>> = Vec::new();
    let mut old_nodes: Vec<&SemanticNode> = old_keys
        .keys()
        .filter_map(|id| old_by_id.get(id).copied())
        .collect();
    old_nodes.sort_by(|a, b| resource_node_sort_key(a).cmp(&resource_node_sort_key(b)));
    for old_node in old_nodes {
        let Some(key) = old_keys.get(old_node.id.as_str()) else {
            continue;
        };
        if new_key_set.contains(key)
            || mentioned_old.contains(old_node.id.as_str())
            || !is_keyed_review_container(old_node)
            || has_unmatched_ancestor(old_node, &old_by_id, &old_keys, &new_key_set)
        {
            continue;
        }
        recovered.push(ChangeDraft {
            change_type: "DELETION",
            old_node: Some(old_node),
            new_node: None,
            old_index: None,
            new_index: None,
            confidence: 0.94,
            description: format!("Delete {}({:?})", old_node.node_type, old_node.label),
            refactoring_kind: None,
            text_diff: None,
        });
    }
    let mut new_nodes: Vec<&SemanticNode> = new_keys
        .keys()
        .filter_map(|id| new_by_id.get(id).copied())
        .collect();
    new_nodes.sort_by(|a, b| resource_node_sort_key(a).cmp(&resource_node_sort_key(b)));
    for new_node in new_nodes {
        let Some(key) = new_keys.get(new_node.id.as_str()) else {
            continue;
        };
        if old_key_set.contains(key)
            || mentioned_new.contains(new_node.id.as_str())
            || !is_keyed_review_container(new_node)
            || has_unmatched_ancestor(new_node, &new_by_id, &new_keys, &old_key_set)
        {
            continue;
        }
        recovered.push(ChangeDraft {
            change_type: "ADDITION",
            old_node: None,
            new_node: Some(new_node),
            old_index: None,
            new_index: None,
            confidence: 0.94,
            description: format!("Insert -> {}({:?})", new_node.node_type, new_node.label),
            refactoring_kind: None,
            text_diff: None,
        });
    }
    changes.extend(recovered);

    // python presentation._describe_keyed_data_additions: an added array OBJECT is described by
    // its identifying pair ("Insert command 'intentumdiff.toggleEditorDiff'"), not "object([1])".
    const SUMMARY_KEYS: &[&str] = &["command", "name", "id", "key", "title", "label"];
    for change in changes.iter_mut() {
        if change.change_type != "ADDITION" {
            continue;
        }
        let Some(node) = change.new_node else { continue };
        if node.node_type.to_lowercase() != "object" {
            continue;
        }
        'keys: for key in SUMMARY_KEYS {
            for child in &node.children {
                if child.node_type.to_lowercase() != "pair" || child.label != *key {
                    continue;
                }
                let value = child
                    .descendants()
                    .into_iter()
                    .rev()
                    .find(|d| d.children.is_empty() && d.label != *key && !d.label.is_empty());
                if let Some(value) = value {
                    change.description = format!("Insert {} '{}'", key, value.label);
                    break 'keys;
                }
            }
        }
    }

    if suppressed_labels.is_empty() {
        return None;
    }
    Some(json!({
        "kind": "NOISE_SUPPRESSED",
        "raw_change_indices": [],
        "old_labels": suppressed_labels,
        "new_labels": [],
        "old_node_ids": [],
        "new_node_ids": [],
        "confidence": 0.9,
        "rule_id": "presentation.suppress_array_index_only_modification",
        "metadata": {"reason": "Structured-data array elements kept identical content but received different positional labels after sibling insertions or deletions."},
    }))
}

// ── SQL query-profile keying (issue #57) — mirrors python query_profiles' SQL half. Clauses,
// relations, and fields key by their role + normalized identity within their statement, so an
// added JOIN doesn't positionally shift the FROM relation into a bogus MOVE, and unchanged
// SELECT fields never churn. The DAX half is deliberately NOT ported: dax is already routed and
// green without it — adding keys would change its behavior for no contract.

#[cfg(test)]
#[path = "keyed_data_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "number_respelling_tests.rs"]
mod number_respelling_tests;
#[cfg(all(test, feature = "tier-c-wasm"))]
#[path = "number_respelling_source_tests.rs"]
mod number_respelling_source_tests;

/// Rule id recorded as `equivalence_proof` when a review's only delta is member order inside
/// specification-unordered mappings (core #252).
pub(crate) const UNORDERED_MAPPING_RULE: &str = "keyed_data.unordered_mapping_member_order";

fn line_starts(source: &str) -> Vec<usize> {
    std::iter::once(0).chain(source.match_indices('\n').map(|(i, _)| i + 1)).collect()
}

fn node_bytes<'s>(node: &SemanticNode, source: &'s str, starts: &[usize]) -> Option<&'s str> {
    let p = &node.position;
    let start = starts.get(p.start_line as usize)? + p.start_col as usize;
    let end = starts.get(p.end_line as usize)? + p.end_col as usize;
    source.get(start..end)
}

/// Canonical form where only the members of unordered containers are sorted. Leaves compare
/// by their exact source bytes, so no value or spelling equivalence is assumed. `None`
/// declines the proof (duplicate keys, unknown spans).
fn unordered_canonical(node: &SemanticNode, source: &str, starts: &[usize], language: &str) -> Option<String> {
    if node.children.is_empty() {
        return Some(format!("{}:{:?}", node.node_type, node_bytes(node, source, starts)?));
    }
    let mut children = node.children.iter()
        .map(|child| unordered_canonical(child, source, starts, language))
        .collect::<Option<Vec<_>>>()?;
    let unordered = match language {
        "json" => node.node_type == "object",
        "yaml" => matches!(node.node_type.as_str(), "block_mapping" | "flow_mapping"),
        "toml" => matches!(node.node_type.as_str(), "document" | "table" | "inline_table"),
        _ => false,
    };
    if unordered {
        // Keys are the first child (json/yaml pairs) or the node label (toml pair/table).
        let keys: Vec<String> = node.children.iter().zip(&children).map(|(child, canonical)| match language {
            "toml" => format!("{}:{}", child.node_type, child.label),
            _ => child.children.first()
                .and_then(|key| unordered_canonical(key, source, starts, language))
                .unwrap_or_else(|| canonical.clone()),
        }).collect();
        let unique: std::collections::BTreeSet<&String> = keys.iter().collect();
        if unique.len() != keys.len() {
            return None;
        }
        children.sort();
    }
    Some(format!("{}:{}({})", node.node_type, node.label, children.join(",")))
}

/// Whether two keyed-data sources differ only by the order of members of JSON objects
/// (RFC 8259 §4), YAML mappings (YAML 1.2 §3.2.1.1) or TOML tables. Conservative: anchors,
/// aliases, merge keys, tags and arrays of tables decline; so does any dropped token
/// (the non-whitespace, non-separator character multisets must agree).
pub(crate) fn unordered_mapping_equivalent(old_tree: &SemanticNode, new_tree: &SemanticNode,
    old_source: &str, new_source: &str, language: &str) -> bool {
    if !matches!(language, "json" | "yaml" | "toml") || old_source == new_source {
        return false;
    }
    let risky = |source: &str| match language {
        "yaml" => source.contains(['&', '*', '!']) || source.contains("<<"),
        "toml" => source.contains("[["),
        // Escaped keys could spell one key twice; reordering duplicates is not equivalent.
        _ => source.contains('\\'),
    };
    if risky(old_source) || risky(new_source) {
        return false;
    }
    let bag = |source: &str| {
        let mut chars: Vec<char> = source.chars().filter(|c| !c.is_whitespace() && *c != ',').collect();
        chars.sort_unstable();
        chars
    };
    if bag(old_source) != bag(new_source) {
        return false;
    }
    let (old_starts, new_starts) = (line_starts(old_source), line_starts(new_source));
    match (unordered_canonical(old_tree, old_source, &old_starts, language),
        unordered_canonical(new_tree, new_source, &new_starts, language)) {
        (Some(old), Some(new)) => old == new,
        _ => false,
    }
}

/// Rule id of the number respelling equivalence: a `number` leaf whose source spelling changed
/// while its exact decimal value did not (`9` -> `9.0`, `1000` -> `1e3`). Recorded as the
/// invariance group `rule_id` for each proved pair and, when nothing else changed, as the
/// review's `equivalence_proof` (core #252).
pub(crate) const NUMBER_RESPELLING_RULE: &str = "keyed_data.number_exact_decimal_respelling";

/// Routes whose `number` leaves are RFC 8259 JSON numbers: the JSON parser, and the
/// configuration projections (plugin-sdk #36) of ADF and Databricks, whose number labels are
/// the exact source token. YAML (`integer_scalar`/`float_scalar`, typed by the core schema) and
/// TOML (`integer`/`float`) emit no `number` node and stay outside the rule.
pub(crate) fn number_respelling_language(language: &str) -> bool {
    matches!(language, "json" | "adf" | "databricks" | "databricks-workflow")
}

/// The projection domain of plugin-sdk #36 `canonical_decimal`: beyond it a spelling keeps its
/// source identity, so no equivalence is claimed there either.
const NUMBER_EXPONENT_BOUND: u64 = 1_000_000;

/// The exact canonical decimal (`exact_decimal`) of a strict RFC 8259 §6 number token
/// `-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?`, or `None`. Declines every spelling the
/// grammar does not prove (`1_000`, `0x10`, `+1`, `.5`, `1.`, `01`, `Infinity`) and values
/// whose explicit or effective exponent leaves the ±1,000,000 projection domain. Negative zero
/// keeps its sign, so `-0` and `0` are never equated (signed zero is observable in IEEE
/// consumers; the catalogue's number rule forbids hiding it).
pub(crate) fn exact_json_number(token: &str) -> Option<String> {
    static GRAMMAR: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let grammar = GRAMMAR.get_or_init(|| {
        regex::Regex::new(r"^-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?$")
            .expect("valid RFC 8259 number pattern")
    });
    if !grammar.is_match(token) {
        return None;
    }
    if let Some(at) = token.find(['e', 'E']) {
        if token[at + 1..].parse::<i64>().ok()?.unsigned_abs() > NUMBER_EXPONENT_BOUND {
            return None;
        }
    }
    let exact = exact_decimal(token)?;
    if let Some((_, exponent)) = exact.split_once('e') {
        if exponent.parse::<i64>().ok()?.unsigned_abs() > NUMBER_EXPONENT_BOUND {
            return None;
        }
    }
    Some(exact)
}

fn node_span(node: &SemanticNode, source: &str, starts: &[usize]) -> Option<(usize, usize)> {
    let p = &node.position;
    let start = starts.get(p.start_line as usize)? + p.start_col as usize;
    let end = starts.get(p.end_line as usize)? + p.end_col as usize;
    source.get(start..end).map(|_| (start, end))
}

/// The complete source token of a `number` leaf: its span must hold exactly the label, and the
/// bytes on either side must not continue a number (a parser that split `1e+22` and kept only
/// `22` proves nothing about the value).
fn number_leaf_token(node: &SemanticNode, source: &str, starts: &[usize]) -> Option<(usize, usize)> {
    if node.node_type != "number" || !node.children.is_empty() {
        return None;
    }
    let (start, end) = node_span(node, source, starts)?;
    let continues = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '-' | '_');
    if source[start..end] != node.label
        || source[..start].chars().next_back().is_some_and(continues)
        || source[end..].chars().next().is_some_and(continues)
    {
        return None;
    }
    Some((start, end))
}

/// Leaf proof of one respelling: both nodes are whole `number` tokens with different
/// spellings and the same exact decimal. Returns that canonical value.
pub(crate) fn number_respelling_pair(
    old: &SemanticNode, new: &SemanticNode,
    old_source: &str, new_source: &str, old_starts: &[usize], new_starts: &[usize],
) -> Option<String> {
    number_leaf_token(old, old_source, old_starts)?;
    number_leaf_token(new, new_source, new_starts)?;
    if old.label == new.label {
        return None;
    }
    let canonical = exact_json_number(&old.label)?;
    (exact_json_number(&new.label)? == canonical).then_some(canonical)
}

pub(crate) fn source_line_starts(source: &str) -> Vec<usize> {
    line_starts(source)
}

#[derive(PartialEq)]
enum JsonToken<'s> {
    Number(String),
    Other(&'s str),
}

/// Whitespace-free RFC 8259 token stream (strings kept as exact bytes, numbers as exact
/// decimals, with the spelling retained for the respelling count). `None` for anything that
/// is not strictly lexable JSON, including numbers outside `exact_json_number`.
fn json_tokens(source: &str) -> Option<Vec<(JsonToken<'_>, &str)>> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        match bytes[i] {
            b' ' | b'\t' | b'\n' | b'\r' => { i += 1; continue; }
            b'{' | b'}' | b'[' | b']' | b':' | b',' => i += 1,
            b'"' => {
                i += 1;
                loop {
                    match *bytes.get(i)? {
                        b'\\' => i += 2,
                        b'"' => { i += 1; break; }
                        c if c < 0x20 => return None,
                        _ => i += 1,
                    }
                }
            }
            b'-' | b'0'..=b'9' => {
                while i < bytes.len() && matches!(bytes[i], b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-') {
                    i += 1;
                }
                let text = source.get(start..i)?;
                tokens.push((JsonToken::Number(exact_json_number(text)?), text));
                continue;
            }
            _ => {
                let rest = source.get(i..)?;
                let word = ["true", "false", "null"].into_iter().find(|w| rest.starts_with(w))?;
                i += word.len();
            }
        }
        let text = source.get(start..i)?;
        tokens.push((JsonToken::Other(text), text));
    }
    Some(tokens)
}

/// Whether two sources differ ONLY by respelling `number` values with equal exact decimals
/// (at least one). JSON-lexable sources compare their whole token streams, so layout may
/// differ too; any other source (Databricks YAML) must keep identical trees and identical
/// bytes outside the number tokens. Never consults hashes or labels alone.
pub(crate) fn number_respelling_equivalent(old_tree: &SemanticNode, new_tree: &SemanticNode,
    old_source: &str, new_source: &str, language: &str) -> bool {
    if !number_respelling_language(language) || old_source == new_source {
        return false;
    }
    if let (Some(old), Some(new)) = (json_tokens(old_source), json_tokens(new_source)) {
        return old.len() == new.len()
            && old.iter().zip(&new).all(|((a, _), (b, _))| a == b)
            && old.iter().zip(&new).any(|((_, a), (_, b))| a != b);
    }
    let (old_starts, new_starts) = (line_starts(old_source), line_starts(new_source));
    let (mut old_spans, mut new_spans, mut respelled) = (Vec::new(), Vec::new(), 0usize);
    fn walk(o: &SemanticNode, n: &SemanticNode, c: &mut (&str, &str, &[usize], &[usize],
        &mut Vec<(usize, usize)>, &mut Vec<(usize, usize)>, &mut usize)) -> bool {
        if o.node_type != n.node_type || o.children.len() != n.children.len() {
            return false;
        }
        if !o.children.is_empty() {
            return o.label == n.label && o.children.iter().zip(&n.children).all(|(a, b)| walk(a, b, c));
        }
        if let (Some(a), Some(b)) = (number_leaf_token(o, c.0, c.2), number_leaf_token(n, c.1, c.3)) {
            if o.label != n.label {
                if number_respelling_pair(o, n, c.0, c.1, c.2, c.3).is_none() {
                    return false;
                }
                *c.6 += 1;
            }
            c.4.push(a);
            c.5.push(b);
            return true;
        }
        o.label == n.label
            && matches!((node_bytes(o, c.0, c.2), node_bytes(n, c.1, c.3)), (Some(a), Some(b)) if a == b)
    }
    let mut context = (old_source, new_source, &old_starts[..], &new_starts[..], &mut old_spans, &mut new_spans, &mut respelled);
    if !walk(old_tree, new_tree, &mut context) || respelled == 0 {
        return false;
    }
    // Every byte outside the number tokens must be identical, so nothing the tree omits
    // (a dropped token, a layout or comment change) is hidden by this proof.
    fn outside<'s>(source: &'s str, spans: &mut [(usize, usize)]) -> Option<Vec<&'s str>> {
        spans.sort_unstable();
        let (mut pieces, mut at) = (Vec::new(), 0);
        for &(start, end) in spans.iter() {
            if start < at { return None; }
            pieces.push(&source[at..start]);
            at = end;
        }
        pieces.push(&source[at..]);
        Some(pieces)
    }
    matches!((outside(old_source, &mut old_spans), outside(new_source, &mut new_spans)), (Some(a), Some(b)) if a == b)
}
