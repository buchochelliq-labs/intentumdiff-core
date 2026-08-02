//! Statement-profile keying (issue #57): asm/bash/delphi statement identities and
//! the matching augmentation, extracted from lib.rs verbatim (issue #29 monolith
//! split, phase B).

use crate::*;

// ── Statement-profile keying (issue #57) — mirrors python statement_profiles. asm/bash/delphi
// are STATEMENT-profile languages (parallel to the resource-profile family): their statements key
// by an identity (asm: mnemonic + first operand register) so an operand-VALUE edit
// (`mov ebx, 0` → `mov ebx, 42`) pairs as ONE MODIFICATION instead of DELETE+ADD churn.

pub(crate) fn statement_profile_language(language: &str) -> bool {
    matches!(language, "asm" | "bash" | "delphi")
}

/// python statement_profiles `_is_generic_label` (its own generic set, distinct from resource).
pub(crate) fn is_generic_statement_label(label: &str, node_type: &str) -> bool {
    let text = label.trim();
    if text.is_empty() {
        return true;
    }
    let lowered = text.to_lowercase();
    lowered == node_type.to_lowercase()
        || matches!(
            lowered.as_str(),
            "command"
                | "compound_statement"
                | "exprargs"
                | "exprbinary"
                | "instruction"
                | "pipeline"
                | "program"
                | "root"
                | "statement"
        )
}

/// The tree root, walking up the dot-path id (python `_root_from_parents`).
pub(crate) fn root_from_by_id<'a>(
    node: &'a SemanticNode,
    by_id: &HashMap<&str, &'a SemanticNode>,
) -> &'a SemanticNode {
    let mut current = node;
    while let Some((parent_id, _)) = current.id.rsplit_once('.') {
        match by_id.get(parent_id).copied() {
            Some(parent) => current = parent,
            None => break,
        }
    }
    current
}

/// python `_asm_instruction_parts`: (mnemonic, operands) from a compacted instruction label.
pub(crate) fn asm_instruction_parts(label: &str) -> (String, String) {
    let text = label.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return (String::new(), String::new());
    }
    let mut parts = text.splitn(2, ' ');
    let mnemonic = resource_normalize(parts.next().unwrap_or(""));
    let operands = parts.next().unwrap_or("").trim().to_string();
    (mnemonic, operands)
}

/// python `_asm_first_operand`: the normalized first operand (before the comma) — the register/
/// target IDENTITY, NOT its value, so a value edit is a modification of the same instruction.
pub(crate) fn asm_first_operand(operands: &str) -> String {
    if operands.is_empty() {
        return String::new();
    }
    resource_normalize(operands.splitn(2, ',').next().unwrap_or(""))
}

/// python `_looks_like_data_definition`: the operands begin with a data directive (db/dw/equ/…).
pub(crate) fn asm_looks_like_data_definition(operands: &str) -> bool {
    let first = operands
        .split_whitespace()
        .next()
        .map(resource_normalize)
        .unwrap_or_default();
    matches!(
        first.as_str(),
        "db" | "dw" | "dd" | "dq" | "equ" | "resb" | "resw" | "resd" | "resq"
    )
}

/// python `_asm_section`: the most recent `section` directive before this node (root children).
pub(crate) fn asm_section(node: &SemanticNode, by_id: &HashMap<&str, &SemanticNode>) -> String {
    let root = root_from_by_id(node, by_id);
    let mut children: Vec<&SemanticNode> = root.children.iter().collect();
    children.sort_by(|a, b| resource_node_sort_key(a).cmp(&resource_node_sort_key(b)));
    let node_key = resource_node_sort_key(node);
    let mut current = String::new();
    for candidate in children {
        if resource_node_sort_key(candidate) >= node_key {
            break;
        }
        if candidate.node_type.to_lowercase() == "instruction" {
            let (mnemonic, operands) = asm_instruction_parts(&candidate.label);
            if mnemonic == "section" {
                current = resource_normalize(if operands.is_empty() {
                    &candidate.label
                } else {
                    &operands
                });
            }
        }
    }
    current
}

/// python `_same_asm_instruction_ordinal`: index among same (mnemonic, operand-identity, section).
pub(crate) fn same_asm_instruction_ordinal(
    node: &SemanticNode,
    by_id: &HashMap<&str, &SemanticNode>,
    mnemonic: &str,
    operand_identity: &str,
    section: &str,
) -> usize {
    let root = root_from_by_id(node, by_id);
    let mut children: Vec<&SemanticNode> = root.children.iter().collect();
    children.sort_by(|a, b| resource_node_sort_key(a).cmp(&resource_node_sort_key(b)));
    let mut ordinal = 0;
    for candidate in children {
        if candidate.id == node.id {
            return ordinal;
        }
        if candidate.node_type.to_lowercase() != "instruction" {
            continue;
        }
        let (candidate_mnemonic, candidate_operands) = asm_instruction_parts(&candidate.label);
        if candidate_mnemonic == mnemonic
            && asm_first_operand(&candidate_operands) == operand_identity
            && asm_section(candidate, by_id) == section
        {
            ordinal += 1;
        }
    }
    ordinal
}

/// python `_asm_key`.
pub(crate) fn asm_key(node: &SemanticNode, by_id: &HashMap<&str, &SemanticNode>) -> Option<Vec<String>> {
    let node_type = node.node_type.to_lowercase();
    if matches!(node_type.as_str(), "label" | "local_label" | "global_label") {
        if is_generic_statement_label(&node.label, &node.node_type) {
            return None;
        }
        return Some(vec![
            "asm".into(),
            "label".into(),
            asm_section(node, by_id),
            resource_normalize(&node.label),
        ]);
    }
    if node_type != "instruction" || is_generic_statement_label(&node.label, &node.node_type) {
        return None;
    }
    let (mnemonic, operands) = asm_instruction_parts(&node.label);
    if mnemonic.is_empty() {
        return None;
    }
    let section = asm_section(node, by_id);
    if mnemonic == "section" {
        return Some(vec![
            "asm".into(),
            "section".into(),
            resource_normalize(if operands.is_empty() { &node.label } else { &operands }),
        ]);
    }
    if matches!(mnemonic.as_str(), "global" | "extern") && !operands.is_empty() {
        return Some(vec!["asm".into(), mnemonic, resource_normalize(&operands)]);
    }
    if asm_looks_like_data_definition(&operands) {
        return Some(vec!["asm".into(), "data".into(), section, mnemonic]);
    }
    let operand_identity = asm_first_operand(&operands);
    let ordinal = same_asm_instruction_ordinal(node, by_id, &mnemonic, &operand_identity, &section);
    Some(vec![
        "asm".into(),
        "instruction".into(),
        section,
        mnemonic,
        operand_identity,
        ordinal.to_string(),
    ])
}

/// python statement_profiles._bash_scope: the enclosing function, else top level.
pub(crate) fn bash_scope(node: &SemanticNode, by_id: &HashMap<&str, &SemanticNode>) -> String {
    if let Some(function) =
        nearest_ancestor_of_types(node.id.as_str(), by_id, &["function_definition"])
    {
        if function.id != node.id {
            return format!("function:{}", resource_normalize(&function.label));
        }
    }
    "top".to_string()
}

/// python statement_profiles._bash_command_name: the first token of a non-generic command label.
pub(crate) fn bash_command_name(label: &str, node_type: &str) -> String {
    let text = label.trim();
    if text.is_empty() || is_statement_generic_label(text, node_type) {
        return String::new();
    }
    text.split_whitespace().next().unwrap_or_default().to_string()
}

/// python statement_profiles._bash_key: functions by name; assignments by variable name;
/// commands by command NAME + same-name ordinal in scope — so `:` and `echo Hello` get
/// DIFFERENT keys and the matching augmentation UNPAIRS the positional match between them.
pub(crate) fn bash_key(node: &SemanticNode, by_id: &HashMap<&str, &SemanticNode>) -> Option<Vec<String>> {
    let node_type = node.node_type.to_lowercase();
    let scope = bash_scope(node, by_id);

    if node_type == "function_definition"
        && !is_statement_generic_label(&node.label, &node.node_type)
    {
        return Some(vec![
            "bash".into(),
            "function".into(),
            resource_normalize(&node.label),
        ]);
    }

    if node_type == "variable_assignment" {
        let text = node.label.trim();
        let name = match text.split_once('=') {
            Some((lhs, _)) => lhs.trim_start_matches("local ").trim().to_string(),
            None => text.to_string(),
        };
        if name.is_empty() {
            return None;
        }
        return Some(vec![
            "bash".into(),
            "assignment".into(),
            scope,
            resource_normalize(&name),
        ]);
    }

    if matches!(node_type.as_str(), "command" | "declaration_command") {
        let command_name = bash_command_name(&node.label, &node.node_type);
        if command_name.is_empty() {
            return None;
        }
        // Same-name ordinal within the scope (position order over the whole tree).
        let root = {
            let mut current: &SemanticNode = node;
            loop {
                let Some((parent_id, _)) = current.id.rsplit_once('.') else {
                    break current;
                };
                let Some(parent) = by_id.get(parent_id).copied() else {
                    break current;
                };
                current = parent;
            }
        };
        let mut ordinal = 0usize;
        let mut all: Vec<&SemanticNode> =
            std::iter::once(root).chain(root.descendants()).collect();
        all.sort_by(|a, b| resource_node_sort_key(a).cmp(&resource_node_sort_key(b)));
        for candidate in all {
            if candidate.id == node.id {
                break;
            }
            if candidate.node_type.to_lowercase() == node_type
                && bash_scope(candidate, by_id) == scope
                && resource_normalize(&bash_command_name(&candidate.label, &candidate.node_type))
                    == resource_normalize(&command_name)
            {
                ordinal += 1;
            }
        }
        return Some(vec![
            "bash".into(),
            node_type,
            scope,
            resource_normalize(&command_name),
            ordinal.to_string(),
        ]);
    }

    None
}

/// python statement_profiles._delphi_scope: enclosing routine, program block, or program.
/// (delphi node types are camelCase — defProc — so comparisons lowercase, like python's.)
pub(crate) fn delphi_scope(node: &SemanticNode, by_id: &HashMap<&str, &SemanticNode>) -> String {
    if let Some(routine) =
        nearest_ancestor_of_types(node.id.as_str(), by_id, &["defproc", "declproc"])
    {
        if routine.id != node.id {
            return format!("routine:{}", resource_normalize(&routine.label));
        }
    }
    if let Some(block) = nearest_ancestor_of_types(node.id.as_str(), by_id, &["block"]) {
        if let Some((parent_id, _)) = block.id.rsplit_once('.') {
            if by_id
                .get(parent_id)
                .is_some_and(|p| p.node_type.to_lowercase() == "program")
            {
                return "program:block".to_string();
            }
        }
    }
    "program".to_string()
}

/// python `_delphi_callee`: the leading dotted identifier before `(`, else the first token.
pub(crate) fn delphi_callee(label: &str) -> String {
    let text = label.trim();
    let mut end = 0usize;
    for (i, ch) in text.char_indices() {
        if i == 0 {
            if !(ch.is_ascii_alphabetic() || ch == '_') {
                break; // end stays 0
            }
            end = i + ch.len_utf8();
        } else if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' {
            end = i + ch.len_utf8();
        } else {
            if text[end..].trim_start().starts_with('(') {
                return text[..end].to_string();
            }
            break;
        }
    }
    text.split_whitespace().next().unwrap_or_default().to_string()
}

/// python `_delphi_statement_identity`: the first call's callee or assignment's target beneath
/// the statement — `WriteLn('Hello, ' + Name)` and `WriteLn(Format(...))` share `call:writeln`,
/// so the statement matches as ONE unit and the inner literal folds into its MODIFICATION.
pub(crate) fn delphi_statement_identity(node: &SemanticNode) -> String {
    for candidate in std::iter::once(node).chain(node.descendants()) {
        let nt = candidate.node_type.to_lowercase();
        if matches!(nt.as_str(), "exprcall" | "procedure_call") {
            let callee = delphi_callee(&candidate.label);
            if !callee.is_empty() {
                return format!("call:{}", resource_normalize(&callee));
            }
        }
        if matches!(nt.as_str(), "assignment" | "assignment_statement") {
            let target = match candidate.label.split_once(":=") {
                Some((lhs, _)) => lhs.trim().to_string(),
                None => match candidate.label.split_once('=') {
                    Some((lhs, _)) => lhs.trim().to_string(),
                    None => candidate.label.trim().to_string(),
                },
            };
            if !target.is_empty() {
                return format!("assign:{}", resource_normalize(&target));
            }
        }
    }
    String::new()
}

pub(crate) fn delphi_tree_root<'a>(
    node: &'a SemanticNode,
    by_id: &HashMap<&str, &'a SemanticNode>,
) -> &'a SemanticNode {
    let mut current = node;
    loop {
        let Some((parent_id, _)) = current.id.rsplit_once('.') else {
            return current;
        };
        let Some(parent) = by_id.get(parent_id).copied() else {
            return current;
        };
        current = parent;
    }
}

/// python statement_profiles._delphi_key.
pub(crate) fn delphi_key(node: &SemanticNode, by_id: &HashMap<&str, &SemanticNode>) -> Option<Vec<String>> {
    let node_type = node.node_type.to_lowercase();
    let scope = delphi_scope(node, by_id);

    if matches!(node_type.as_str(), "assignment" | "assignment_statement") {
        let target = match node.label.split_once(":=") {
            Some((lhs, _)) => lhs.trim().to_string(),
            None => match node.label.split_once('=') {
                Some((lhs, _)) => lhs.trim().to_string(),
                None => node.label.trim().to_string(),
            },
        };
        if target.is_empty() {
            return None;
        }
        return Some(vec![
            "delphi".into(),
            "assignment".into(),
            scope,
            resource_normalize(&target),
        ]);
    }

    if matches!(node_type.as_str(), "exprcall" | "procedure_call") {
        let callee = delphi_callee(&node.label);
        if callee.is_empty() {
            return None;
        }
        if let Some(statement) =
            nearest_ancestor_of_types(node.id.as_str(), by_id, &["statement"])
        {
            if let Some(mut statement_key) = delphi_key(statement, by_id) {
                statement_key.push("call".into());
                statement_key.push(resource_normalize(&callee));
                return Some(statement_key);
            }
        }
        let root = delphi_tree_root(node, by_id);
        let mut all: Vec<&SemanticNode> =
            std::iter::once(root).chain(root.descendants()).collect();
        all.sort_by(|a, b| resource_node_sort_key(a).cmp(&resource_node_sort_key(b)));
        let mut ordinal = 0usize;
        for candidate in all {
            if candidate.id == node.id {
                break;
            }
            if matches!(
                candidate.node_type.to_lowercase().as_str(),
                "exprcall" | "procedure_call"
            ) && delphi_scope(candidate, by_id) == scope
                && resource_normalize(&delphi_callee(&candidate.label))
                    == resource_normalize(&callee)
            {
                ordinal += 1;
            }
        }
        return Some(vec![
            "delphi".into(),
            "call".into(),
            scope,
            resource_normalize(&callee),
            ordinal.to_string(),
        ]);
    }

    if node_type == "statement" {
        let identity = delphi_statement_identity(node);
        if identity.is_empty() {
            return None;
        }
        let root = delphi_tree_root(node, by_id);
        let mut all: Vec<&SemanticNode> =
            std::iter::once(root).chain(root.descendants()).collect();
        all.sort_by(|a, b| resource_node_sort_key(a).cmp(&resource_node_sort_key(b)));
        let mut ordinal = 0usize;
        for candidate in all {
            if candidate.id == node.id {
                break;
            }
            if candidate.node_type.to_lowercase() == "statement"
                && delphi_scope(candidate, by_id) == scope
                && delphi_statement_identity(candidate) == identity
            {
                ordinal += 1;
            }
        }
        return Some(vec![
            "delphi".into(),
            "statement".into(),
            scope,
            identity,
            ordinal.to_string(),
        ]);
    }

    None
}

pub(crate) fn statement_profile_key(
    node: &SemanticNode,
    by_id: &HashMap<&str, &SemanticNode>,
    language: &str,
) -> Option<Vec<String>> {
    match language {
        "asm" => asm_key(node, by_id),
        "bash" => bash_key(node, by_id),
        "delphi" => delphi_key(node, by_id),
        _ => None,
    }
}

/// python statement_profiles.augment_statement_profile_matching — the statement-profile parallel of
/// augment_resource_profile_matching (same algorithm: drop cross-key positional matches, then pair
/// unmatched keyed nodes by identical key).
pub(crate) fn augment_statement_profile_matching<'a>(
    old_tree: &'a SemanticNode,
    new_tree: &'a SemanticNode,
    matching: Vec<MatchPair<'a>>,
    language: &str,
) -> Vec<MatchPair<'a>> {
    if !statement_profile_language(language) {
        return matching;
    }
    let old_by_id = semantic_node_refs_by_id_with_root(old_tree);
    let new_by_id = semantic_node_refs_by_id_with_root(new_tree);
    let old_keys: HashMap<&str, Vec<String>> = old_by_id
        .iter()
        .filter_map(|(id, node)| statement_profile_key(node, &old_by_id, language).map(|k| (*id, k)))
        .collect();
    let new_keys: HashMap<&str, Vec<String>> = new_by_id
        .iter()
        .filter_map(|(id, node)| statement_profile_key(node, &new_by_id, language).map(|k| (*id, k)))
        .collect();

    let mut result: Vec<MatchPair<'a>> = Vec::new();
    let mut matched_old: HashSet<String> = HashSet::new();
    let mut matched_new: HashSet<String> = HashSet::new();
    for pair in &matching {
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
        result.push(MatchPair {
            old_node: pair.old_node,
            new_node: pair.new_node,
        });
        matched_old.insert(pair.old_node.id.clone());
        matched_new.insert(pair.new_node.id.clone());
    }

    let mut new_by_key: HashMap<Vec<String>, Vec<&SemanticNode>> = HashMap::new();
    for (id, node) in &new_by_id {
        if let Some(key) = new_keys.get(id) {
            if !matched_new.contains(*id) {
                new_by_key.entry(key.clone()).or_default().push(node);
            }
        }
    }
    for nodes in new_by_key.values_mut() {
        nodes.sort_by(|a, b| resource_node_sort_key(a).cmp(&resource_node_sort_key(b)));
    }

    let mut old_sorted: Vec<&SemanticNode> = old_by_id.values().copied().collect();
    old_sorted.sort_by(|a, b| resource_node_sort_key(a).cmp(&resource_node_sort_key(b)));
    for old_node in old_sorted {
        if matched_old.contains(old_node.id.as_str()) {
            continue;
        }
        let Some(key) = old_keys.get(old_node.id.as_str()) else {
            continue;
        };
        if let Some(candidates) = new_by_key.get_mut(key) {
            if candidates.is_empty() {
                continue;
            }
            let new_node = candidates.remove(0);
            result.push(MatchPair { old_node, new_node });
            matched_old.insert(old_node.id.clone());
            matched_new.insert(new_node.id.clone());
        }
    }
    result
}
