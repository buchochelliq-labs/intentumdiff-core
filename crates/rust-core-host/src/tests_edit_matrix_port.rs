// Split from tests_inline.rs (issue #85): one file per test family.
// Nested inside cfg(test) mod tests - `super::*` = the tests mod (helpers),
// `crate::*` = the engine.
#![allow(unused_imports)]
use super::*;
use crate::*;

// ==============================================================================================
// Rust LANGUAGE-PARITY harness, part 1 of 3: a 1:1 port of `tests/unit/construct_edit_matrix.py`.
//
// Everything in this file is PURE — no engine calls, no filesystem. It is the mechanical half of
// the matrix: the vocab tables, the construct model, the five `derive_*` mutators and the five
// `check_*` expectation classes. `tests_edit_matrix_corpus.rs` supplies the engine (tree roots,
// mutation diffs); `tests_edit_matrix.rs` holds the tests.
//
// PORTING RULES (each of these has already bitten a naive port):
//   * every Python `sorted` is STABLE -> `sort_by_key`, never `sort_unstable_by_key`;
//   * `walk_order_then_smallest` returns each entity TWICE by design - do not dedupe;
//   * `derive_swap_siblings` groups by node_type in DICT INSERTION ORDER -> ordered Vec, not
//     HashMap;
//   * `str.splitlines()` is NOT `str::lines()` (see `py_splitlines`);
//   * `re.sub(r"\bNAME\b", ...)` is hand-rolled (see `word_replace_all`).
// ==============================================================================================

// ----------------------------------------------------------------------------------------------
// Vocab tables - ported VERBATIM from tests/unit/construct_edit_matrix.py:34-126.
// ----------------------------------------------------------------------------------------------

pub(super) const ENTITY_HINTS: &[&str] = &[
    "function",
    "method",
    "procedure",
    "class",
    "declproc",
    "defproc",
    "sub",
    "form",
    "resource_declaration",
    "markdown_section",
    "rule_set",
    "operation",
];

pub(super) const LITERAL_HINTS: &[&str] = &[
    "string", "literal", "integer", "float", "number", "char", "scalar",
];

/// Exact literal node types (markup text/content) - `_LITERAL_EXACT`.
pub(super) const LITERAL_EXACT: &[&str] =
    &["text", "chardata", "content", "code_content", "property_value"];

/// Exact node-type entities (NOT substring hints) - `_ENTITY_EXACT`.
pub(super) const ENTITY_EXACT: &[&str] = &[
    "block",
    "block_mapping_pair",
    "element",
    "flow_pair",
    "jsx_component",
    "pair",
    "section",
];

/// `_NEWLINE_INCLUSIVE_SPAN_TYPES` - matched against the RAW node_type, not the lowercased one.
pub(super) const NEWLINE_INCLUSIVE_SPAN_TYPES: &[&str] = &[
    "module_directive",
    "require_spec",
    "replace_spec",
    "exclude_spec",
    "retract_spec",
    "rule",
    "variable_assignment",
];

/// `_ENTITY_TOP_LEVEL_ONLY`.
pub(super) const ENTITY_TOP_LEVEL_ONLY: &[&str] = &["clojure"];

/// `_ENTITY_EXACT_BY_LANGUAGE` - language-SCOPED entity node types.
pub(super) const ENTITY_EXACT_BY_LANGUAGE: &[(&str, &[&str])] = &[
    ("adf", &["pipeline", "activity"]),
    ("asciidoc", &["section_level_1", "section_level_2", "section_level_3"]),
    ("asm", &["label"]),
    ("databricks", &["job", "task"]),
    ("databricks-workflow", &["job", "task"]),
    ("dax", &["measure"]),
    ("gomod", &["require_spec", "replace_spec", "module_directive"]),
    ("cmake", &["function_def", "macro_def", "normal_command"]),
    ("m", &["step"]),
    ("proto", &["message", "service", "rpc", "field", "enum"]),
    ("make", &["rule", "variable_assignment"]),
    ("ocaml", &["value"]),
    ("po", &["message"]),
    ("reasonml", &["value"]),
    ("sas", &["data_step", "proc_step"]),
    ("sql", &["create_view", "create_table", "create_function"]),
    (
        "dockerfile",
        &[
            "from_instruction",
            "run_instruction",
            "copy_instruction",
            "env_instruction",
            "expose_instruction",
            "workdir_instruction",
            "label_instruction",
            "healthcheck_instruction",
            "user_instruction",
            "cmd_instruction",
        ],
    ),
    ("elixir", &["call"]),
    ("clojure", &["list_lit"]),
    ("wast", &["func"]),
    ("wat", &["func"]),
];

/// `_LITERAL_EXACT_BY_LANGUAGE`.
pub(super) const LITERAL_EXACT_BY_LANGUAGE: &[(&str, &[&str])] = &[
    ("asciidoc", &["paragraph"]),
    ("clojure", &["str_lit", "num_lit"]),
    ("ini", &["setting_value"]),
    ("make", &["recipe_line"]),
    ("proto", &["field_number"]),
    ("gomod", &["version"]),
    ("po", &["msgstr"]),
];

pub(super) fn entity_exact_for(language_lower: &str) -> &'static [&'static str] {
    ENTITY_EXACT_BY_LANGUAGE
        .iter()
        .find(|(lang, _)| *lang == language_lower)
        .map(|(_, types)| *types)
        .unwrap_or(&[])
}

pub(super) fn literal_exact_for(language_lower: &str) -> &'static [&'static str] {
    LITERAL_EXACT_BY_LANGUAGE
        .iter()
        .find(|(lang, _)| *lang == language_lower)
        .map(|(_, types)| *types)
        .unwrap_or(&[])
}

// ----------------------------------------------------------------------------------------------
// Python primitives Rust does not give for free.
// ----------------------------------------------------------------------------------------------

/// Python `str.splitlines()`. Rust's `str::lines()` breaks ONLY on `\n` (absorbing a preceding
/// `\r`); Python additionally breaks on a lone `\r`, `\v`, `\f`, `\x1c`-`\x1e`, `\x85`, ` `
/// and ` `. A form feed is legal in real source, and every span-based verb (delete /
/// duplicate / swap) indexes into this list - using `lines()` would silently shift spans for any
/// fixture containing one.
pub(super) fn py_splitlines(source: &str) -> Vec<String> {
    fn is_line_break(c: char) -> bool {
        matches!(
            c,
            '\n' | '\r'
                | '\u{000b}'
                | '\u{000c}'
                | '\u{001c}'
                | '\u{001d}'
                | '\u{001e}'
                | '\u{0085}'
                | '\u{2028}'
                | '\u{2029}'
        )
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut chars = source.chars().peekable();
    while let Some(c) = chars.next() {
        if is_line_break(c) {
            if c == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            lines.push(std::mem::take(&mut current));
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// `"\n".join(lines) + "\n"` - the trailing-newline normalisation the span verbs apply (and that
/// `rename_entity` / `modify_literal` deliberately do NOT).
pub(super) fn join_lines(lines: &[String]) -> String {
    format!("{}\n", lines.join("\n"))
}

/// Python's `\w`: `_` plus anything `str.isalnum()` accepts (Unicode alphanumerics).
fn is_word_char(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}

/// `re.sub(rf"\b{re.escape(name)}\b", replacement, source)` with no count limit.
///
/// `entity_name` guarantees `name` matches `[A-Za-z_][A-Za-z0-9_]*`, so both `\b`s reduce to "the
/// adjacent character is not a word char" - which removes the Python-`\w`-vs-Rust-`regex`-`\w`
/// ambiguity entirely (only the NEIGHBOURS can be non-ASCII, and both engines agree there).
pub(super) fn word_replace_all(source: &str, name: &str, replacement: &str) -> String {
    if name.is_empty() {
        return source.to_owned();
    }
    let mut out = String::with_capacity(source.len());
    let mut cursor = 0usize;
    while let Some(offset) = source[cursor..].find(name) {
        let start = cursor + offset;
        let end = start + name.len();
        let before_ok = source[..start]
            .chars()
            .next_back()
            .map_or(true, |c| !is_word_char(c));
        let after_ok = source[end..].chars().next().map_or(true, |c| !is_word_char(c));
        if before_ok && after_ok {
            out.push_str(&source[cursor..start]);
            out.push_str(replacement);
            cursor = end;
        } else {
            // Regex semantics: a failed match retries one character further along. `name` starts
            // with an ASCII byte, so `start + 1` is always a char boundary.
            out.push_str(&source[cursor..start + 1]);
            cursor = start + 1;
        }
    }
    out.push_str(&source[cursor..]);
    out
}

/// Python `f"{s!r}"` for a `str`. Prefers `'`; switches to `"` only when the value contains `'`
/// and no `"`. The recorded `xfail` reasons embed these, so the formatter is byte-checked by the
/// parity test.
pub(super) fn py_repr(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Python `f"{dict!r}"` for the swap context: `{'first': 'greet', 'second': 'add'}`.
pub(super) fn py_dict_repr(pairs: &[(&str, &str)]) -> String {
    let body: Vec<String> = pairs
        .iter()
        .map(|(k, v)| format!("{}: {}", py_repr(k), py_repr(v)))
        .collect();
    format!("{{{}}}", body.join(", "))
}

// ----------------------------------------------------------------------------------------------
// Node / diff model.
// ----------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Deserialize)]
pub(super) struct MatrixPos {
    #[serde(default)]
    pub start_line: i64,
    #[serde(default)]
    pub end_line: i64,
    #[serde(default)]
    pub end_col: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct MatrixNode {
    #[serde(default)]
    pub id: String,
    pub node_type: String,
    #[serde(default)]
    pub label: String,
    pub position: MatrixPos,
    #[serde(default)]
    pub structural_hash: String,
    #[serde(default)]
    pub children: Vec<MatrixNode>,
}

/// `_walk` - pre-order, node first.
pub(super) fn walk<'a>(node: &'a MatrixNode, out: &mut Vec<&'a MatrixNode>) {
    out.push(node);
    for child in &node.children {
        walk(child, out);
    }
}

pub(super) fn walked(node: &MatrixNode) -> Vec<&MatrixNode> {
    let mut out = Vec::new();
    walk(node, &mut out);
    out
}

/// `_mentions` - plain substring over every non-empty label in the subtree; `None` node => false.
pub(super) fn mentions(node: Option<&MatrixNode>, token: &str) -> bool {
    match node {
        None => false,
        Some(node) => walked(node)
            .into_iter()
            .any(|n| !n.label.is_empty() && n.label.contains(token)),
    }
}

#[derive(Clone, Debug)]
pub(super) struct MatrixChange {
    pub change_type: String,
    pub old_node: Option<MatrixNode>,
    pub new_node: Option<MatrixNode>,
}

#[derive(Clone, Debug)]
pub(super) struct MatrixDiff {
    pub changes: Vec<MatrixChange>,
    pub is_style_only: bool,
}

impl MatrixDiff {
    /// STRICT decode. Both dispatches must hand over a bare `SemanticDiff` object; if the batch
    /// wrapper (`{"diff": ...}`) leaks through, `is_style_only` and `changes` would read as
    /// absent, every check would pass vacuously and the harness would report a false green. So
    /// missing/ill-typed keys PANIC rather than defaulting - that is the whole point of the file.
    pub fn from_value(context: &str, value: &Value) -> MatrixDiff {
        let changes_value = value.get("changes").unwrap_or_else(|| {
            panic!(
                "{context}: diff payload has no `changes` key - keys were {:?}. \
                 (Did a dispatch forget to unwrap the batch item's `diff`?)",
                value.as_object().map(|o| o.keys().collect::<Vec<_>>())
            )
        });
        let changes_array = changes_value
            .as_array()
            .unwrap_or_else(|| panic!("{context}: `changes` is not an array"));
        let is_style_only = value
            .get("is_style_only")
            .and_then(Value::as_bool)
            .unwrap_or_else(|| {
                panic!("{context}: diff payload has no boolean `is_style_only` key")
            });
        let changes = changes_array
            .iter()
            .enumerate()
            .map(|(index, change)| {
                // `check_duplicate_entity` / `check_swap_siblings` read `change_type.value` in
                // Python; a non-string there raises AttributeError, which is PAST the tolerance
                // boundary. Mirror that: fail loudly, never `unwrap_or_default`.
                let change_type = change
                    .get("change_type")
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| {
                        panic!("{context}: change[{index}] has no string `change_type`")
                    })
                    .to_owned();
                let decode = |key: &str| -> Option<MatrixNode> {
                    change.get(key).filter(|v| !v.is_null()).map(|v| {
                        serde_json::from_value(v.clone()).unwrap_or_else(|exc| {
                            panic!("{context}: change[{index}].{key} is not a SemanticNode: {exc}")
                        })
                    })
                };
                MatrixChange {
                    change_type,
                    old_node: decode("old_node"),
                    new_node: decode("new_node"),
                }
            })
            .collect();
        MatrixDiff {
            changes,
            is_style_only,
        }
    }
}

// ----------------------------------------------------------------------------------------------
// Construct discovery.
// ----------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub(super) struct Construct {
    pub node_type: String,
    pub label: String,
    pub start_line: i64,
    pub end_line: i64,
    /// -1 = unknown; 0 marks a newline-inclusive span (see `entity_lines`).
    pub end_col: i64,
}

/// `find_entities` minus the engine call: the caller supplies `_tree_roots(...)`.
/// Returns `(constructs, base)` where `base` is hard-coded 0 (issue #52 made every parser emit
/// 0-based rows) but is still threaded through and subtracted, exactly as Python does.
pub(super) fn find_entities_from_roots(
    language: &str,
    source: &str,
    roots: &[MatrixNode],
) -> (Vec<Construct>, i64) {
    let lines = py_splitlines(source);
    let language_lower = language.to_lowercase();
    let lang_exact = entity_exact_for(&language_lower);
    let top_level_only = ENTITY_TOP_LEVEL_ONLY.contains(&language_lower.as_str());

    let mut constructs: Vec<Construct> = Vec::new();
    let mut exact_constructs: Vec<Construct> = Vec::new();

    for root in roots {
        let top_level_ids: HashSet<&str> =
            root.children.iter().map(|c| c.id.as_str()).collect();
        for node in walked(root) {
            let node_type = node.node_type.to_lowercase();
            let hinted = ENTITY_HINTS.iter().any(|h| node_type.contains(h));
            let language_scoped = lang_exact.contains(&node_type.as_str());
            let mut exact = ENTITY_EXACT.contains(&node_type.as_str()) || language_scoped;
            if exact
                && top_level_only
                && !top_level_ids.contains(node.id.as_str())
                && node.id != root.id
            {
                exact = false;
            }
            let mut label = node.label.clone();
            if language_scoped && (label.is_empty() || label == node.node_type) {
                // Label rescue - LANGUAGE-scoped types ONLY. Global-exact types
                // (block/pair/element) keep the strict rule; rescuing them turned squirrel
                // function bodies into deletable "entities".
                for descendant in walked(node) {
                    if !descendant.label.is_empty() && descendant.label != descendant.node_type {
                        label = descendant.label.clone();
                        break;
                    }
                }
            }
            if (hinted || exact) && !label.is_empty() && label != node.node_type {
                let bucket = if hinted {
                    &mut constructs
                } else {
                    &mut exact_constructs
                };
                bucket.push(Construct {
                    node_type: node.node_type.clone(),
                    label,
                    start_line: node.position.start_line,
                    end_line: node.position.end_line,
                    end_col: node.position.end_col,
                });
            }
        }
    }
    // Two BUCKETS concatenated (hinted-in-walk-order, then exact-in-walk-order) - not one
    // interleaved pass. Putting exact first shifted vue's ratcheted derivations.
    constructs.extend(exact_constructs);

    // Degenerate positions (adf/databricks emit no node positions): invalidate every span so the
    // span verbs skip honestly while `rename_entity` (name-only) still derives.
    if lines.len() > 3
        && !constructs.is_empty()
        && constructs
            .iter()
            .all(|c| c.start_line == 0 && c.end_line == 0)
    {
        constructs = constructs
            .into_iter()
            .map(|c| Construct {
                node_type: c.node_type,
                label: c.label,
                start_line: -1,
                end_line: -1,
                end_col: -1,
            })
            .collect();
    }
    (constructs, 0)
}

/// `find_literals` minus the engine call. NOTE the deliberate asymmetry with
/// `derive_modify_literal`: discovery scopes `LITERAL_EXACT_BY_LANGUAGE` to the CURRENT language,
/// while the mutator scans EVERY language's set. Unifying them diverges from Python.
pub(super) fn find_literals_from_roots(
    language: &str,
    source: &str,
    roots: &[MatrixNode],
) -> Vec<Construct> {
    let language_lower = language.to_lowercase();
    let lang_literals = literal_exact_for(&language_lower);
    let mut literals = Vec::new();
    for root in roots {
        for node in walked(root) {
            let node_type = node.node_type.to_lowercase();
            let vocab_hit = LITERAL_HINTS.iter().any(|h| node_type.contains(h))
                || LITERAL_EXACT.contains(&node_type.as_str())
                || lang_literals.contains(&node_type.as_str());
            if node.children.is_empty()
                && vocab_hit
                && !node.label.is_empty()
                && node.label != node.node_type
                && source.matches(node.label.as_str()).count() == 1
                && node.label.trim().chars().count() >= 2
            {
                literals.push(Construct {
                    node_type: node.node_type.clone(),
                    label: node.label.clone(),
                    start_line: node.position.start_line,
                    end_line: node.position.end_line,
                    end_col: -1,
                });
            }
        }
    }
    literals
}

/// `_entity_name`: `re.search(r"[A-Za-z_][A-Za-z0-9_]*", label)` - SEARCH, so it scans forward
/// (`"[server]"` -> `server`).
pub(super) fn entity_name(construct: &Construct) -> Option<String> {
    let bytes = construct.label.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        let c = bytes[index];
        if c.is_ascii_alphabetic() || c == b'_' {
            let start = index;
            index += 1;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
            {
                index += 1;
            }
            return Some(construct.label[start..index].to_owned());
        }
        index += 1;
    }
    None
}

/// `_entity_lines`. The `end -= 1` trim fires only when ALL THREE hold: `end_col == 0`,
/// `end > start`, and the RAW (not lowercased) node_type is newline-inclusive.
pub(super) fn entity_lines(
    construct: &Construct,
    base: i64,
    lines: &[String],
) -> Option<(usize, usize)> {
    let start = construct.start_line - base;
    let mut end = construct.end_line - base;
    if construct.end_col == 0
        && end > start
        && NEWLINE_INCLUSIVE_SPAN_TYPES.contains(&construct.node_type.as_str())
    {
        end -= 1;
    }
    if 0 <= start && start <= end && end < lines.len() as i64 {
        Some((start as usize, end as usize))
    } else {
        None
    }
}

// ----------------------------------------------------------------------------------------------
// Edit derivations.
// ----------------------------------------------------------------------------------------------

/// The five verbs, in `EDIT_VERBS` insertion order (which `build_manifest` output depends on).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Verb {
    DeleteEntity,
    RenameEntity,
    DuplicateEntity,
    ModifyLiteral,
    SwapSiblings,
}

pub(super) const EDIT_VERBS: [Verb; 5] = [
    Verb::DeleteEntity,
    Verb::RenameEntity,
    Verb::DuplicateEntity,
    Verb::ModifyLiteral,
    Verb::SwapSiblings,
];

impl Verb {
    pub fn name(self) -> &'static str {
        match self {
            Verb::DeleteEntity => "delete_entity",
            Verb::RenameEntity => "rename_entity",
            Verb::DuplicateEntity => "duplicate_entity",
            Verb::ModifyLiteral => "modify_literal",
            Verb::SwapSiblings => "swap_siblings",
        }
    }

    /// "entities" or "literals" - the pool the verb draws from, and the noun in the empty-pool
    /// skip message.
    pub fn needs(self) -> &'static str {
        match self {
            Verb::ModifyLiteral => "literals",
            _ => "entities",
        }
    }
}

#[derive(Clone, Debug)]
pub(super) enum Ctx {
    Delete { name: String },
    Rename { old: String, new: String },
    Duplicate { copy: String, original: String },
    Literal { old: String, new: String },
    Swap { first: String, second: String },
}

impl Ctx {
    /// Human-readable rendering for the divergence report (NOT used by any check).
    pub fn render(&self) -> String {
        match self {
            Ctx::Delete { name } => py_dict_repr(&[("name", name)]),
            Ctx::Rename { old, new } => py_dict_repr(&[("old", old), ("new", new)]),
            Ctx::Duplicate { copy, original } => {
                py_dict_repr(&[("copy", copy), ("original", original)])
            }
            Ctx::Literal { old, new } => py_dict_repr(&[("old", old), ("new", new)]),
            Ctx::Swap { first, second } => py_dict_repr(&[("first", first), ("second", second)]),
        }
    }
}

/// `_walk_order_then_smallest`. Returns each entity TWICE (walk order, then smallest-span order).
/// A revisited entity derives identically, so the duplicate pass costs only a retry - and removing
/// it shifts which entity several ratcheted languages pick.
pub(super) fn walk_order_then_smallest(entities: &[Construct]) -> Vec<Construct> {
    let mut by_span = entities.to_vec();
    by_span.sort_by_key(|e| e.end_line - e.start_line); // STABLE, like Python's sorted()
    let mut out = entities.to_vec();
    out.extend(by_span);
    out
}

pub(super) fn derive_delete_entity(
    source: &str,
    entities: &[Construct],
    base: i64,
) -> Option<(String, Ctx)> {
    let lines = py_splitlines(source);
    // Smallest span FIRST, and no walk-order pass at all (unlike rename/duplicate).
    let mut ordered = entities.to_vec();
    ordered.sort_by_key(|e| e.end_line - e.start_line);
    for entity in &ordered {
        let span = entity_lines(entity, base, &lines);
        let name = entity_name(entity);
        // Guard precedence mirrors Python: `A or B or (C and D)`.
        let (span, name) = match (span, name) {
            (Some(span), Some(name)) => (span, name),
            _ => continue,
        };
        if span.0 == 0 && span.1 as i64 >= lines.len() as i64 - 1 {
            continue;
        }
        let mut remaining: Vec<String> = lines[..span.0].to_vec();
        remaining.extend_from_slice(&lines[span.1 + 1..]);
        let mutated = join_lines(&remaining);
        // Plain substring over the WHOLE mutated file - not a word-boundary test.
        if !mutated.contains(&name) {
            return Some((mutated, Ctx::Delete { name }));
        }
    }
    None
}

pub(super) fn derive_rename_entity(
    source: &str,
    entities: &[Construct],
    _base: i64,
) -> Option<(String, Ctx)> {
    // Never touches `base` or spans - which is why degenerate-position languages still derive here.
    for entity in walk_order_then_smallest(entities) {
        let name = match entity_name(&entity) {
            Some(name) => name,
            None => continue,
        };
        // `len(name) < 3`: names are ASCII per `entity_name`, so chars == bytes.
        if name.len() < 3 || !source.contains(&name) {
            continue;
        }
        let new = format!("{name}Renamed");
        // CASE-SENSITIVE by design (`flags = 0`): an IGNORECASE fallback made SAS rename keywords.
        let renamed = word_replace_all(source, &name, &new);
        // NB: no trailing-newline normalisation here.
        if renamed != source && renamed.contains(&new) {
            return Some((renamed, Ctx::Rename { old: name, new }));
        }
    }
    None
}

pub(super) fn derive_duplicate_entity(
    source: &str,
    entities: &[Construct],
    base: i64,
) -> Option<(String, Ctx)> {
    let lines = py_splitlines(source);
    for entity in walk_order_then_smallest(entities) {
        let span = entity_lines(&entity, base, &lines);
        let name = entity_name(&entity);
        let (span, name) = match (span, name) {
            (Some(span), Some(name)) => (span, name),
            _ => continue,
        };
        let block = &lines[span.0..=span.1];
        if !block.iter().any(|line| line.contains(&name)) {
            continue;
        }
        let copy_name = format!("{name}Copy");
        // Replacement is applied PER LINE of the block only.
        let copy: Vec<String> = block
            .iter()
            .map(|line| word_replace_all(line, &name, &copy_name))
            .collect();
        if copy.iter().all(|line| !line.contains(&copy_name)) {
            continue;
        }
        let mut mutated_lines: Vec<String> = lines[..=span.1].to_vec();
        mutated_lines.push(String::new());
        mutated_lines.extend(copy);
        mutated_lines.extend_from_slice(&lines[span.1 + 1..]);
        return Some((
            join_lines(&mutated_lines),
            Ctx::Duplicate {
                copy: copy_name,
                original: name,
            },
        ));
    }
    None
}

fn all_ascii_digits(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

/// `int(s) + 7`. Python has bignum and never overflows; a Rust port that saturated or wrapped
/// would fabricate a divergence, so overflow PANICS.
fn bump_int(digits: &str, context: &str) -> String {
    let value: u128 = digits.parse().unwrap_or_else(|exc| {
        panic!(
            "{context}: `int({digits:?}) + 7` does not fit u128 ({exc}). Python has bignum; \
             widen the port rather than degrading silently."
        )
    });
    value
        .checked_add(7)
        .unwrap_or_else(|| panic!("{context}: `int({digits:?}) + 7` overflowed u128"))
        .to_string()
    // NB `int("007") + 7 -> 14`: zero padding is lost, exactly as here.
}

/// Two-arg (no `base`), unlike the entity verbs.
pub(super) fn derive_modify_literal(source: &str, literals: &[Construct]) -> Option<(String, Ctx)> {
    for literal in literals {
        let old = literal.label.clone();
        let stripped = old.trim().to_owned();
        let node_type_lower = literal.node_type.to_lowercase();
        let new = if all_ascii_digits(&stripped) {
            bump_int(&stripped, "derive_modify_literal")
        } else if stripped.matches('.').count() == 1
            && stripped
                .split_once('.')
                .map(|(whole, frac)| all_ascii_digits(whole) && all_ascii_digits(frac))
                .unwrap_or(false)
        {
            let (whole, frac) = stripped.split_once('.').expect("checked above");
            format!("{}.{}", bump_int(whole, "derive_modify_literal"), frac)
        } else if stripped.starts_with('\'')
            || stripped.starts_with('"')
            || node_type_lower.starts_with("string")
            || LITERAL_EXACT.contains(&node_type_lower.as_str())
            // Fourth clause: scans EVERY language's literal set, not just this language's.
            || LITERAL_EXACT_BY_LANGUAGE
                .iter()
                .any(|(_, types)| types.contains(&node_type_lower.as_str()))
        {
            let core = old.trim_matches(|c| c == '\'' || c == '"');
            if core.is_empty() {
                continue;
            }
            old.replacen(core, &format!("{core}X"), 1)
        } else {
            // Bare non-numeric words cannot be mutated safely.
            continue;
        };
        if new != old && source.matches(old.as_str()).count() == 1 {
            return Some((source.replacen(old.as_str(), &new, 1), Ctx::Literal { old, new }));
        }
    }
    None
}

pub(super) fn derive_swap_siblings(
    source: &str,
    entities: &[Construct],
    base: i64,
) -> Option<(String, Ctx)> {
    let lines = py_splitlines(source);
    // Grouping is by RAW node_type in DICT INSERTION ORDER - an ordered Vec, never a HashMap.
    let mut by_type: Vec<(String, Vec<Construct>)> = Vec::new();
    for entity in entities {
        match by_type.iter_mut().find(|(key, _)| key == &entity.node_type) {
            Some((_, group)) => group.push(entity.clone()),
            None => by_type.push((entity.node_type.clone(), vec![entity.clone()])),
        }
    }
    for (_, group) in &by_type {
        let mut group = group.clone();
        group.sort_by_key(|c| c.start_line); // STABLE
        for pair in group.windows(2) {
            let (first, second) = (&pair[0], &pair[1]);
            let span1 = entity_lines(first, base, &lines);
            let span2 = entity_lines(second, base, &lines);
            let (span1, span2) = match (span1, span2) {
                (Some(a), Some(b)) => (a, b),
                _ => continue,
            };
            if span1.1 >= span2.0 {
                continue;
            }
            let gap = &lines[span1.1 + 1..span2.0];
            let block1 = &lines[span1.0..=span1.1];
            let block2 = &lines[span2.0..=span2.1];
            let mut mutated_lines: Vec<String> = lines[..span1.0].to_vec();
            mutated_lines.extend_from_slice(block2);
            mutated_lines.extend_from_slice(gap);
            mutated_lines.extend_from_slice(block1);
            mutated_lines.extend_from_slice(&lines[span2.1 + 1..]);
            let (name1, name2) = (entity_name(first), entity_name(second));
            let (name1, name2) = match (name1, name2) {
                (Some(a), Some(b)) => (a, b),
                _ => continue,
            };
            if name1 == name2 {
                continue;
            }
            // Unlike duplicate there is NO check that a block actually contains its own name.
            let _ = source;
            return Some((
                join_lines(&mutated_lines),
                Ctx::Swap {
                    first: name1,
                    second: name2,
                },
            ));
        }
    }
    None
}

pub(super) fn derive(
    verb: Verb,
    source: &str,
    pool: &[Construct],
    base: i64,
) -> Option<(String, Ctx)> {
    match verb {
        Verb::DeleteEntity => derive_delete_entity(source, pool, base),
        Verb::RenameEntity => derive_rename_entity(source, pool, base),
        Verb::DuplicateEntity => derive_duplicate_entity(source, pool, base),
        Verb::ModifyLiteral => derive_modify_literal(source, pool),
        Verb::SwapSiblings => derive_swap_siblings(source, pool, base),
    }
}

// ----------------------------------------------------------------------------------------------
// Expectation classes. Ported to the CODE, not the module docstring - the docstring claims two
// assertions (`delete` "must not appear as ADDITION", `rename` "must not survive as a bare
// DELETION-only story") that Python never implements. Implementing the prose breaks the ratchets.
// ----------------------------------------------------------------------------------------------

pub(super) fn check_delete_entity(diff: &MatrixDiff, name: &str) -> Option<String> {
    if diff.is_style_only {
        return Some("style-only for a deleted entity".to_owned());
    }
    if diff.changes.is_empty() {
        return Some("zero changes for a deleted entity".to_owned());
    }
    if !diff
        .changes
        .iter()
        .any(|c| mentions(c.old_node.as_ref(), name))
    {
        return Some(format!(
            "no change mentions deleted entity {} on the old side",
            py_repr(name)
        ));
    }
    None
}

pub(super) fn check_rename_entity(diff: &MatrixDiff, old: &str, new: &str) -> Option<String> {
    if diff.is_style_only {
        return Some("style-only for a renamed entity".to_owned());
    }
    // NB `new == old + "Renamed"` CONTAINS `old`, so the old-side test is weak by design.
    let paired = diff.changes.iter().any(|c| {
        c.old_node.is_some()
            && c.new_node.is_some()
            && mentions(c.old_node.as_ref(), old)
            && mentions(c.new_node.as_ref(), new)
    });
    if !paired {
        return Some(format!(
            "no paired change carries {} -> {}",
            py_repr(old),
            py_repr(new)
        ));
    }
    None
}

pub(super) fn check_duplicate_entity(
    diff: &MatrixDiff,
    copy: &str,
    original: &str,
) -> Option<String> {
    if diff.is_style_only {
        return Some("style-only for a duplicated entity".to_owned());
    }
    if !diff
        .changes
        .iter()
        .any(|c| c.change_type == "ADDITION" && mentions(c.new_node.as_ref(), copy))
    {
        return Some(format!("no ADDITION mentions the copy {}", py_repr(copy)));
    }
    // Substring semantics again: `original` is a prefix of `copy`, so this guard fires on either.
    if diff
        .changes
        .iter()
        .any(|c| c.change_type == "DELETION" && mentions(c.old_node.as_ref(), original))
    {
        return Some(format!(
            "original {} falsely deleted",
            py_repr(original)
        ));
    }
    None
}

pub(super) fn check_modify_literal(diff: &MatrixDiff, old: &str) -> Option<String> {
    if diff.is_style_only {
        return Some("style-only for a literal value change (issue #41/#46 class)".to_owned());
    }
    if diff.changes.is_empty() {
        return Some("zero changes for a literal value change".to_owned());
    }
    let core = old.trim_matches(|c| c == '\'' || c == '"');
    let carried = diff
        .changes
        .iter()
        .filter(|c| c.old_node.is_some() && c.new_node.is_some())
        .any(|c| mentions(c.old_node.as_ref(), core) || mentions(c.old_node.as_ref(), old));
    if !carried {
        return Some(format!(
            "no paired change carries the old literal {}",
            py_repr(old)
        ));
    }
    None
}

/// NEGATIVE-ONLY, unlike every other check: it does NOT reject `is_style_only`, and an EMPTY diff
/// passes. That is Python's behaviour and the ratchets depend on it.
pub(super) fn check_swap_siblings(diff: &MatrixDiff, first: &str, second: &str) -> Option<String> {
    let ctx_repr = py_dict_repr(&[("first", first), ("second", second)]);
    for change in &diff.changes {
        if change.change_type == "DELETION"
            && change.new_node.is_none()
            && (mentions(change.old_node.as_ref(), first)
                || mentions(change.old_node.as_ref(), second))
        {
            return Some(format!("swap decomposed into DELETION of {ctx_repr}"));
        }
        if change.change_type == "ADDITION"
            && change.old_node.is_none()
            && (mentions(change.new_node.as_ref(), first)
                || mentions(change.new_node.as_ref(), second))
        {
            return Some(format!("swap decomposed into ADDITION of {ctx_repr}"));
        }
    }
    None
}

pub(super) fn check(diff: &MatrixDiff, ctx: &Ctx) -> Option<String> {
    match ctx {
        Ctx::Delete { name } => check_delete_entity(diff, name),
        Ctx::Rename { old, new } => check_rename_entity(diff, old, new),
        Ctx::Duplicate { copy, original } => check_duplicate_entity(diff, copy, original),
        Ctx::Literal { old, .. } => check_modify_literal(diff, old),
        Ctx::Swap { first, second } => check_swap_siblings(diff, first, second),
    }
}

// ----------------------------------------------------------------------------------------------
// Unit tests for the PORT itself (no engine, no corpus, no wasm - these always run).
// ----------------------------------------------------------------------------------------------

#[test]
fn py_splitlines_breaks_where_python_breaks_and_rust_lines_does_not() {
    assert_eq!(py_splitlines("a\nb\n"), vec!["a", "b"]);
    assert_eq!(py_splitlines("a\r\nb"), vec!["a", "b"]);
    // Lone CR and form feed: Python splits, `str::lines()` does not.
    assert_eq!(py_splitlines("a\rb"), vec!["a", "b"]);
    assert_eq!(py_splitlines("a\u{000c}b"), vec!["a", "b"]);
    assert_eq!("a\u{000c}b".lines().count(), 1, "guard: this is the trap");
    assert_eq!(py_splitlines(""), Vec::<String>::new());
    assert_eq!(py_splitlines("\n"), vec![""]);
}

#[test]
fn word_replace_all_honours_word_boundaries_everywhere() {
    assert_eq!(word_replace_all("foo foobar foo", "foo", "fooX"), "fooX foobar fooX");
    assert_eq!(word_replace_all("a_foo foo", "foo", "Z"), "a_foo Z");
    assert_eq!(word_replace_all("call print_msg", "print_msg", "print_msgRenamed"), "call print_msgRenamed");
    // Neighbour is a non-ASCII alphanumeric: Python's \w matches it, so no boundary.
    assert_eq!(word_replace_all("\u{00e9}foo", "foo", "Z"), "\u{00e9}foo");
}

#[test]
fn py_repr_matches_the_recorded_xfail_quoting() {
    assert_eq!(py_repr("print_msg"), "'print_msg'");
    assert_eq!(py_repr("it's"), "\"it's\"");
    assert_eq!(py_repr("say \"hi\""), "'say \"hi\"'");
    assert_eq!(py_repr("both ' and \""), "'both \\' and \"'");
    assert_eq!(
        py_dict_repr(&[("first", "greet"), ("second", "add")]),
        "{'first': 'greet', 'second': 'add'}"
    );
}

#[test]
fn entity_name_searches_forward_like_re_search() {
    let make = |label: &str| Construct {
        node_type: "x".to_owned(),
        label: label.to_owned(),
        start_line: 0,
        end_line: 0,
        end_col: -1,
    };
    assert_eq!(entity_name(&make("[server]")).as_deref(), Some("server"));
    assert_eq!(entity_name(&make("def foo(x)")).as_deref(), Some("def"));
    assert_eq!(entity_name(&make("123")), None);
    assert_eq!(entity_name(&make("_start:")).as_deref(), Some("_start"));
}

#[test]
fn walk_order_then_smallest_yields_each_entity_twice() {
    let make = |label: &str, start: i64, end: i64| Construct {
        node_type: "t".to_owned(),
        label: label.to_owned(),
        start_line: start,
        end_line: end,
        end_col: -1,
    };
    let entities = vec![make("big", 0, 10), make("small", 20, 21)];
    let ordered = walk_order_then_smallest(&entities);
    assert_eq!(ordered.len(), 4, "do NOT dedupe - the retry pass is load-bearing");
    assert_eq!(
        ordered.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(),
        vec!["big", "small", "small", "big"]
    );
}
