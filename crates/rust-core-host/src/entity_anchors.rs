// Extracted verbatim from lib.rs (issue #85, lib.rs split round 2).
use crate::*;

/// python language_profiles.FunctionValuedDeclarationProfiles.JAVASCRIPT: a lexical/variable
/// declaration whose declarator's value is a function (arrow/function expression) IS a function
/// declaration named by the declarator — `const circleArea = (r) => ...` pairs with
/// `function circleArea(...)` as the same entity (derived kind "function_declaration").
pub(crate) fn js_function_valued_declaration_label(node: &SemanticNode) -> Option<String> {
    let node_type = node.node_type.to_lowercase();
    if !matches!(node_type.as_str(), "lexical_declaration" | "variable_declaration") {
        return None;
    }
    for child in &node.children {
        if child.node_type.to_lowercase() != "variable_declarator" || child.label.is_empty() {
            continue;
        }
        let has_function_value = std::iter::once(child)
            .chain(child.descendants())
            .any(|d| {
                matches!(
                    d.node_type.to_lowercase().as_str(),
                    "arrow_function" | "function" | "function_expression"
                )
            });
        if has_function_value {
            return Some(child.label.clone());
        }
    }
    None
}

pub(crate) fn anchor_is_entity(node: &SemanticNode) -> bool {
    !node.children.is_empty()
        && ANCHOR_ENTITY_TYPES.contains(&node.node_type.to_lowercase().as_str())
}

pub(crate) fn anchor_is_function(node: &SemanticNode) -> bool {
    ANCHOR_FUNCTION_TYPES.contains(&node.node_type.to_lowercase().as_str())
        || js_function_valued_declaration_label(node).is_some()
}

pub(crate) fn anchor_is_name(node: &SemanticNode) -> bool {
    ANCHOR_NAME_TYPES.contains(&node.node_type.to_lowercase().as_str())
}

/// python anchors._is_root_entity: a root-entity TYPE with no parent (the tree root).
pub(crate) fn anchor_is_root_entity(node: &SemanticNode, parents: &HashMap<&str, &SemanticNode>) -> bool {
    ANCHOR_ROOT_ENTITY_TYPES.contains(&node.node_type.to_lowercase().as_str())
        && !parents.contains_key(node.id.as_str())
}

/// Parent map keyed by child id (python anchors._parent_map).
pub(crate) fn anchor_parent_map<'a>(root: &'a SemanticNode) -> HashMap<&'a str, &'a SemanticNode> {
    let mut parents: HashMap<&str, &SemanticNode> = HashMap::new();
    let mut stack: Vec<&SemanticNode> = vec![root];
    while let Some(node) = stack.pop() {
        for child in &node.children {
            if child.id != node.id {
                parents.insert(child.id.as_str(), node);
            }
            stack.push(child);
        }
    }
    parents
}

/// python anchors._entity_key: (kind, enclosing-entity label path, label). None for non-entities,
/// label-less nodes, and the tree root.
pub(crate) fn anchor_entity_key(
    node: &SemanticNode,
    parents: &HashMap<&str, &SemanticNode>,
) -> Option<(String, Vec<String>, String)> {
    // js function-valued declaration: derived kind + declarator label (cross-type entity).
    if let Some(label) = js_function_valued_declaration_label(node) {
        if !anchor_is_root_entity(node, parents) {
            let mut path: Vec<String> = Vec::new();
            let mut current = parents.get(node.id.as_str()).copied();
            let mut seen: HashSet<&str> = HashSet::new();
            while let Some(ancestor) = current {
                if !seen.insert(ancestor.id.as_str()) {
                    break;
                }
                if (anchor_is_entity(ancestor)
                    || js_function_valued_declaration_label(ancestor).is_some())
                    && !ancestor.label.is_empty()
                    && !anchor_is_root_entity(ancestor, parents)
                {
                    path.push(ancestor.label.clone());
                }
                current = parents.get(ancestor.id.as_str()).copied();
            }
            path.reverse();
            return Some(("function_declaration".to_string(), path, label));
        }
        return None;
    }
    if !anchor_is_entity(node) || node.label.is_empty() {
        return None;
    }
    if anchor_is_root_entity(node, parents) {
        return None;
    }
    let mut path: Vec<String> = Vec::new();
    let mut current = parents.get(node.id.as_str()).copied();
    let mut seen: HashSet<&str> = HashSet::new();
    while let Some(ancestor) = current {
        if !seen.insert(ancestor.id.as_str()) {
            break;
        }
        if anchor_is_entity(ancestor)
            && !ancestor.label.is_empty()
            && !anchor_is_root_entity(ancestor, parents)
        {
            path.push(ancestor.label.clone());
        }
        current = parents.get(ancestor.id.as_str()).copied();
    }
    path.reverse();
    Some((node.node_type.clone(), path, node.label.clone()))
}

pub(crate) fn anchor_pos_key(node: &SemanticNode) -> (u32, u32, &str) {
    (
        node.position.start_line,
        node.position.start_col,
        node.id.as_str(),
    )
}

/// python anchors.recover_entity_pairs: gumtree same-key pairs first, then unmatched old
/// entities paired to same-key new candidates by nearest start line. Returned outer-first.
pub(crate) fn recover_entity_pairs<'a>(
    old_root: &'a SemanticNode,
    new_root: &'a SemanticNode,
    matching: &[MatchPair<'a>],
) -> Vec<(&'a SemanticNode, &'a SemanticNode)> {
    let old_parents = anchor_parent_map(old_root);
    let new_parents = anchor_parent_map(new_root);
    let old_keys: HashMap<&str, (String, Vec<String>, String)> =
        std::iter::once(old_root)
            .chain(old_root.descendants())
            .filter_map(|n| anchor_entity_key(n, &old_parents).map(|k| (n.id.as_str(), k)))
            .collect();
    let new_keys: HashMap<&str, (String, Vec<String>, String)> =
        std::iter::once(new_root)
            .chain(new_root.descendants())
            .filter_map(|n| anchor_entity_key(n, &new_parents).map(|k| (n.id.as_str(), k)))
            .collect();

    let mut pairs: Vec<(&SemanticNode, &SemanticNode, usize)> = Vec::new();
    let mut used_old: HashSet<&str> = HashSet::new();
    let mut used_new: HashSet<&str> = HashSet::new();

    for pair in matching {
        let (Some(ok), Some(nk)) = (
            old_keys.get(pair.old_node.id.as_str()),
            new_keys.get(pair.new_node.id.as_str()),
        ) else {
            continue;
        };
        if ok != nk {
            continue;
        }
        pairs.push((pair.old_node, pair.new_node, ok.1.len()));
        used_old.insert(pair.old_node.id.as_str());
        used_new.insert(pair.new_node.id.as_str());
    }

    // Unmatched new entities grouped by key.
    let mut new_by_key: HashMap<&(String, Vec<String>, String), Vec<&SemanticNode>> =
        HashMap::new();
    let new_by_id = semantic_node_refs_by_id_with_root(new_root);
    for (id, key) in &new_keys {
        if used_new.contains(id) {
            continue;
        }
        if let Some(node) = new_by_id.get(id).copied() {
            new_by_key.entry(key).or_default().push(node);
        }
    }
    for candidates in new_by_key.values_mut() {
        candidates.sort_by(|a, b| anchor_pos_key(a).cmp(&anchor_pos_key(b)));
    }

    let old_by_id = semantic_node_refs_by_id_with_root(old_root);
    let mut old_entities: Vec<&SemanticNode> = old_keys
        .keys()
        .filter_map(|id| old_by_id.get(id).copied())
        .collect();
    old_entities.sort_by(|a, b| {
        (a.position.start_line, a.id.as_str()).cmp(&(b.position.start_line, b.id.as_str()))
    });
    for old_node in old_entities {
        if used_old.contains(old_node.id.as_str()) {
            continue;
        }
        let Some(key) = old_keys.get(old_node.id.as_str()) else {
            continue;
        };
        let Some(candidates) = new_by_key.get(key) else {
            continue;
        };
        let best = candidates
            .iter()
            .filter(|n| !used_new.contains(n.id.as_str()))
            .min_by_key(|n| {
                (n.position.start_line as i64 - old_node.position.start_line as i64).abs()
            })
            .copied();
        if let Some(new_node) = best {
            pairs.push((old_node, new_node, key.1.len()));
            used_old.insert(old_node.id.as_str());
            used_new.insert(new_node.id.as_str());
        }
    }

    // Outer entities before nested ones (python sorts by parent-path depth then position).
    pairs.sort_by(|a, b| {
        (a.2, anchor_pos_key(a.0)).cmp(&(b.2, anchor_pos_key(b.0)))
    });
    pairs.into_iter().map(|(o, n, _)| (o, n)).collect()
}

/// python anchors._param_entries (simplified: label-carrying direct params whose name node is a
/// descendant with the same label; the _param_name_nodes type-context refinement is not ported —
/// name-label equality covers the routed beneficiaries).
pub(crate) fn anchor_param_entries<'a>(fn_node: &'a SemanticNode) -> Vec<(&'a str, &'a SemanticNode)> {
    let param_list = std::iter::once(fn_node)
        .chain(fn_node.descendants())
        .find(|n| is_parameter_list_type(&n.node_type.to_lowercase()));
    let Some(list) = param_list else {
        return Vec::new();
    };
    let mut entries: Vec<(&str, &SemanticNode)> = Vec::new();
    for param in &list.children {
        if param.label.is_empty() || matches!(param.label.as_str(), "self" | "cls" | "this") {
            continue;
        }
        let name_node = if anchor_is_name(param) {
            Some(param)
        } else {
            param
                .descendants()
                .into_iter()
                .find(|d| anchor_is_name(d) && d.label == param.label)
        };
        if let Some(name) = name_node {
            entries.push((param.label.as_str(), name));
        }
    }
    entries
}

/// python anchors.augment_entity_matching: add entity pairs, then (for function pairs)
/// direct-child-by-type, positional param zips + renamed-param identifier occurrences, exact
/// (type,label) descendant zips, and nearest compatible string literals.
pub(crate) fn augment_entity_matching<'a>(
    old_root: &'a SemanticNode,
    new_root: &'a SemanticNode,
    mut matching: Vec<MatchPair<'a>>,
    language: &str,
) -> Vec<MatchPair<'a>> {
    if !anchor_language(language) {
        return matching;
    }
    let mut matched_old: HashSet<String> =
        matching.iter().map(|m| m.old_node.id.clone()).collect();
    let mut matched_new: HashSet<String> =
        matching.iter().map(|m| m.new_node.id.clone()).collect();

    let entity_pairs = recover_entity_pairs(old_root, new_root, &matching);
    let mut add =
        |matching: &mut Vec<MatchPair<'a>>,
         matched_old: &mut HashSet<String>,
         matched_new: &mut HashSet<String>,
         old_node: &'a SemanticNode,
         new_node: &'a SemanticNode,
         allow_cross_type: bool| {
            if matched_old.contains(old_node.id.as_str())
                || matched_new.contains(new_node.id.as_str())
            {
                return;
            }
            if old_node.node_type != new_node.node_type && !allow_cross_type {
                return;
            }
            matching.push(MatchPair { old_node, new_node });
            matched_old.insert(old_node.id.clone());
            matched_new.insert(new_node.id.clone());
        };

    for (old_e, new_e) in &entity_pairs {
        let cross = old_e.node_type != new_e.node_type;
        add(&mut matching, &mut matched_old, &mut matched_new, old_e, new_e, cross);
    }

    for (old_entity, new_entity) in &entity_pairs {
        if anchor_is_function(old_entity) && anchor_is_function(new_entity) {
            // Direct children by type, nearest to the old child's position. Scaffold alignment
            // only: NAME nodes are excluded — pairing identifiers positionally across a signature
            // retype invents cross-name renames (groovy `add(a,b)`→`add(int x,int y)` paired b↔x);
            // names are matched by the label-aware param/occurrence/exact passes instead.
            for old_child in &old_entity.children {
                if anchor_is_name(old_child) {
                    continue;
                }
                let candidate = new_entity
                    .children
                    .iter()
                    .filter(|c| {
                        c.node_type == old_child.node_type
                            && !matched_new.contains(c.id.as_str())
                    })
                    .min_by_key(|c| {
                        (
                            (c.position.start_line as i64
                                - old_child.position.start_line as i64)
                                .abs(),
                            (c.position.start_col as i64
                                - old_child.position.start_col as i64)
                                .abs(),
                            c.id.clone(),
                        )
                    });
                if let Some(new_child) = candidate {
                    add(
                        &mut matching,
                        &mut matched_old,
                        &mut matched_new,
                        old_child,
                        new_child,
                        false,
                    );
                }
            }
            // Params by position; renamed params corroborate body identifier occurrences.
            // ONLY when the param counts agree — a grammar that drops a param node (groovy
            // `add(a,b)` parses one formal_parameter, `add(int x,int y)` two) misaligns the zip
            // and invents cross-name renames (b↔x). A count mismatch is a signature change,
            // not a rename.
            let old_params = anchor_param_entries(old_entity);
            let new_params = anchor_param_entries(new_entity);
            let mut renames: Vec<(&str, &str, &SemanticNode, &SemanticNode)> = Vec::new();
            if old_params.len() == new_params.len() {
                for ((old_label, old_name), (new_label, new_name)) in
                    old_params.iter().zip(new_params.iter())
                {
                    if old_name.node_type == new_name.node_type {
                        add(
                            &mut matching,
                            &mut matched_old,
                            &mut matched_new,
                            old_name,
                            new_name,
                            false,
                        );
                        if old_label != new_label {
                            renames.push((old_label, new_label, old_name, new_name));
                        }
                    }
                }
            }
            for (old_label, new_label, old_param, new_param) in renames {
                let old_occ: Vec<&SemanticNode> = old_entity
                    .descendants()
                    .into_iter()
                    .filter(|d| {
                        d.id != old_param.id && anchor_is_name(d) && d.label == old_label
                    })
                    .collect();
                let new_occ: Vec<&SemanticNode> = new_entity
                    .descendants()
                    .into_iter()
                    .filter(|d| {
                        d.id != new_param.id && anchor_is_name(d) && d.label == new_label
                    })
                    .collect();
                for (o, n) in old_occ.iter().zip(new_occ.iter()) {
                    add(&mut matching, &mut matched_old, &mut matched_new, o, n, false);
                }
            }
        }

        // Exact (node_type, label) descendant zips inside the anchored entity, position-ordered.
        // CONTAINERS only: zipping bare leaves pairs literals/identifiers across unrelated
        // statements (go: the `3` in a deleted `add(3, 4)` paired with the `3` in a new
        // `subtract(10, 3)`, marking the deleted statement "covered" and swallowing it).
        let old_desc: Vec<&SemanticNode> = old_entity
            .descendants()
            .into_iter()
            .filter(|n| {
                !matched_old.contains(n.id.as_str())
                    && !n.label.is_empty()
                    && !n.children.is_empty()
            })
            .collect();
        let new_desc: Vec<&SemanticNode> = new_entity
            .descendants()
            .into_iter()
            .filter(|n| {
                !matched_new.contains(n.id.as_str())
                    && !n.label.is_empty()
                    && !n.children.is_empty()
            })
            .collect();
        let mut old_exact: HashMap<(&str, &str), Vec<&SemanticNode>> = HashMap::new();
        for node in &old_desc {
            old_exact
                .entry((node.node_type.as_str(), node.label.as_str()))
                .or_default()
                .push(node);
        }
        let mut new_exact: HashMap<(&str, &str), Vec<&SemanticNode>> = HashMap::new();
        for node in &new_desc {
            new_exact
                .entry((node.node_type.as_str(), node.label.as_str()))
                .or_default()
                .push(node);
        }
        // Statement-scope gate: content pairs only within CORRESPONDING statements. The statement
        // scope of a node is its id prefix entity_depth+2 segments deep (entity → body container →
        // statement); a zip pair is allowed only when the two scopes are themselves a matched
        // pair. Without this, the zip pairs a `call add(3,4)` inside a DELETED go assignment with
        // the same call inside a NEW `fmt.Println(...)` statement, and the parent-repair passes
        // then merge two unrelated statements. In a genuine statement rewrite (rust match→if-let)
        // the statement pair IS matched, so its content still zips.
        let stmt_scope = |id: &str, entity_id: &str| -> String {
            let want = entity_id.split('.').count() + 2;
            let mut segments = id.split('.');
            let mut prefix = String::new();
            for index in 0..want {
                let Some(segment) = segments.next() else { break };
                if index > 0 {
                    prefix.push('.');
                }
                prefix.push_str(segment);
            }
            prefix
        };
        let matched_map: HashMap<&str, &str> = matching
            .iter()
            .map(|m| (m.old_node.id.as_str(), m.new_node.id.as_str()))
            .collect();
        let mut keys: Vec<(&str, &str)> = old_exact.keys().copied().collect();
        keys.sort_unstable();
        for key in keys {
            let mut old_nodes = old_exact.remove(&key).unwrap_or_default();
            let Some(mut new_nodes) = new_exact.remove(&key) else {
                continue;
            };
            old_nodes.sort_by(|a, b| anchor_pos_key(a).cmp(&anchor_pos_key(b)));
            new_nodes.sort_by(|a, b| anchor_pos_key(a).cmp(&anchor_pos_key(b)));
            for (o, n) in old_nodes.iter().zip(new_nodes.iter()) {
                let so = stmt_scope(o.id.as_str(), old_entity.id.as_str());
                let sn = stmt_scope(n.id.as_str(), new_entity.id.as_str());
                let scopes_matched = (so == o.id && sn == n.id)
                    || matched_map.get(so.as_str()).copied() == Some(sn.as_str());
                if !scopes_matched {
                    continue;
                }
                add(&mut matching, &mut matched_old, &mut matched_new, o, n, false);
            }
        }
    }

    matching
}

/// python anchors._supports_entity_anchoring / _CODE_LIKE_LANGUAGES.
pub(crate) fn anchor_language(language: &str) -> bool {
    matches!(
        language,
        "abap" | "asm" | "assemblyscript" | "bash" | "c" | "clojure" | "cpp" | "csharp"
            | "dart" | "delphi" | "elixir" | "freebasic" | "go" | "groovy" | "haskell"
            | "java" | "javascript" | "kotlin" | "lua" | "odin" | "perl" | "php"
            | "powershell" | "python" | "qsharp" | "r" | "ruby" | "rust" | "scala"
            | "squirrel" | "swift" | "tsx" | "typescript" | "vbnet" | "zig"
    )
}

/// Review node types per statement profile (python statement_profiles `*_PROFILE.review_node_types`).
pub(crate) fn statement_profile_review_types(language: &str) -> &'static [&'static str] {
    match language {
        "asm" => &["global_label", "instruction", "label", "local_label"],
        "bash" => &[
            "command",
            "declaration_command",
            "function_definition",
            "pipeline",
            "variable_assignment",
        ],
        "delphi" => &[
            "assignment",
            "assignment_statement",
            "exprcall",
            "procedure_call",
            "statement",
        ],
        _ => &[],
    }
}

/// python statement_profiles `_is_generic_label` (its own generic set, distinct from resource's).
pub(crate) fn is_statement_generic_label(label: &str, node_type: &str) -> bool {
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

/// python statement_profiles.is_statement_profile_review_container: a profile review type carrying
/// a real (non-generic) identity label.
pub(crate) fn is_statement_review_container(node: &SemanticNode, language: &str) -> bool {
    let lowered = node.node_type.to_lowercase();
    statement_profile_review_types(language).contains(&lowered.as_str())
        && !node.label.is_empty()
        && !is_statement_generic_label(&node.label, &node.node_type)
}

/// A review type that is a SCOPE (holds other statements) rather than a leaf statement — a
/// function body's statement/token changes are the content, so a modified scope must NOT fold its
/// descendants (only leaf statements like command/assignment collapse their sub-token churn).
pub(crate) fn is_statement_scope_container(node_type: &str) -> bool {
    matches!(node_type.to_lowercase().as_str(), "function_definition")
}

/// issue #57 (bash/delphi scaffold suppression): a MODIFIED statement-profile review container (a
/// command / assignment / statement whose identity is unchanged but whose body was edited) folds
/// its descendant TOKEN churn (bash `expansion`/`word`/`string`; delphi `exprBinary`/scaffold
/// `statement`) into the single MODIFICATION. The general suppress_descendant_noise only roots on
/// ADD/DELETE/MOVE ancestors — never a MODIFICATION — so this token churn otherwise leaks as
/// separate ADD/DELETEs under the routed path.
pub(crate) fn suppress_statement_container_descendant_noise_drafts(
    changes: &mut Vec<ChangeDraft<'_>>,
    language: &str,
) {
    if !statement_profile_language(language) {
        return;
    }
    let mut old_roots: HashSet<String> = HashSet::new();
    let mut new_roots: HashSet<String> = HashSet::new();
    // A suppression root is a MODIFIED LEAF statement (command/assignment/instruction) — NOT a
    // scope (function_definition), whose body statement/token changes are the real diff.
    let is_leaf_root = |n: &SemanticNode| {
        is_statement_review_container(n, language) && !is_statement_scope_container(&n.node_type)
    };
    for change in changes.iter() {
        if change.change_type != "MODIFICATION" {
            continue;
        }
        if !(change.old_node.is_some_and(is_leaf_root) || change.new_node.is_some_and(is_leaf_root))
        {
            continue;
        }
        if let Some(n) = change.old_node {
            collect_suppression_root_id_node(n, &mut old_roots);
        }
        if let Some(n) = change.new_node {
            collect_suppression_root_id_node(n, &mut new_roots);
        }
    }
    if old_roots.is_empty() && new_roots.is_empty() {
        return;
    }
    // Only fold LEAF/scaffold token churn — never a nested review container. A modified
    // function_definition still surfaces the real command ADD/DELETE inside its body (a `command`
    // is itself a review container); only sub-tokens (expansion/word/string/exprBinary) collapse.
    let is_container = |change: &ChangeDraft| {
        change
            .old_node
            .is_some_and(|n| is_statement_review_container(n, language))
            || change
                .new_node
                .is_some_and(|n| is_statement_review_container(n, language))
    };
    changes.retain(|change| {
        let inside = match change.change_type {
            "DELETION" => change
                .old_node
                .is_some_and(|n| has_suppressed_ancestor_id(&n.id, &old_roots)),
            "ADDITION" => change
                .new_node
                .is_some_and(|n| has_suppressed_ancestor_id(&n.id, &new_roots)),
            "MOVE" | "REORDER" => {
                change
                    .old_node
                    .is_some_and(|n| has_suppressed_ancestor_id(&n.id, &old_roots))
                    || change
                        .new_node
                        .is_some_and(|n| has_suppressed_ancestor_id(&n.id, &new_roots))
            }
            _ => false,
        };
        !(inside && !is_container(change))
    });
}
