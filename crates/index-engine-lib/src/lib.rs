//! Index-engine core logic (shared native + Wasm library).
//!
//! The pure symbol/reference-table extraction and cross-file diff, factored out
//! of the `intentumdiff-index-engine` Wasm plugin so the native `rust-core-host`
//! certified commit path can call it directly (no Wasm round-trip). The Wasm
//! plugin (`crates/index-engine`) is now a thin WIT wrapper over these functions.
//! Mirrors the `sql-parser-lib` split.
//!
//! * [`build_symbol_table_impl`] — walks a batch of SemanticNode trees and
//!   extracts a flat qualified-name → [SymbolDefinition] symbol table.
//! * [`diff_symbol_tables_impl`] — compares two symbol tables and produces a
//!   list of CrossFileChange values (MOVE_TO_MODULE / SPLIT_MODULE /
//!   CROSS_FILE_RENAME).
//! * [`build_reference_table_impl`] — walks a batch of SemanticNode trees and
//!   extracts a label → [ReferenceUsage] reference table (call-sites, imports,
//!   type annotations).

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};

// ---------------------------------------------------------------------------
// Input schema: matches SemanticNode JSON produced by the host
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct SemanticNode {
    id: String,
    node_type: String,
    label: String,
    position: NodePosition,
    #[serde(default)]
    children: Vec<SemanticNode>,
    #[serde(default)]
    #[allow(dead_code)]
    parent_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NodePosition {
    start_line: u32,
    start_col: u32,
    end_line: u32,
    end_col: u32,
}

#[derive(Debug, Deserialize)]
struct FileEntry {
    filename: String,
    language: String,
    tree: SemanticNode,
}

// ---------------------------------------------------------------------------
// Output schema: SymbolDefinition
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolDefinition {
    pub qualified_name: String,
    pub file: String,
    pub node_type: String,
    pub node_id: String,
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
    pub language: String,
}

// SymbolTable: qualified_name → list of definitions
type SymbolTable = HashMap<String, Vec<SymbolDefinition>>;

// ---------------------------------------------------------------------------
// Output schema: ReferenceUsage
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferencePosition {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceUsage {
    pub qualified_name: String,
    pub file: String,
    pub node_id: String,
    pub reference_kind: String, // "CALL" | "IMPORT" | "TYPE_USAGE"
    pub position: ReferencePosition,
    pub language: String,
    pub enclosing_scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_definition: Option<SymbolDefinition>,
}

// ReferenceTable: label → list of usages
type ReferenceTable = HashMap<String, Vec<ReferenceUsage>>;

// ---------------------------------------------------------------------------
// Output schema: CrossFileChange
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
struct CrossFileChange {
    change_type: &'static str,
    symbol_name: String,
    old_file: String,
    new_file: String,
    old_node_id: Option<String>,
    new_node_id: Option<String>,
    old_position: Option<ReferencePosition>,
    new_position: Option<ReferencePosition>,
    old_language: String,
    new_language: String,
    node_type: String,
    symbol_kind: String,
    confidence: f64,
    description: String,
}

// ---------------------------------------------------------------------------
// Node types that are considered definitions for symbol extraction. Some
// semantic node names are language-specific: for example, "signature" is a
// Haskell definition but scaffolding in Swift/Kotlin/WAT.
// ---------------------------------------------------------------------------

fn is_common_definition_type(node_type: &str) -> bool {
    matches!(
        node_type,
        "function_definition"
            | "function_declaration"
            | "function_item"
            | "method_declaration"
            | "method_definition"
            | "class_declaration"
            | "class_definition"
            | "interface_declaration"
            | "struct_item"
            | "struct_declaration"
            | "struct_specifier"
            | "enum_declaration"
            | "enum_item"
            | "impl_item"
            | "trait_item"
            | "module"
            | "namespace_definition"
            | "class"
            | "object_declaration"
            | "companion_object"
            | "actor_declaration"
            | "protocol_declaration"
            | "extension_declaration"
    )
}

fn is_language_definition_type(language: &str, node_type: &str) -> bool {
    match language {
        "abap" => matches!(
            node_type,
            "form" | "function_module" | "class_impl" | "interface" | "method"
        ),
        "assemblyscript" => matches!(
            node_type,
            "function_signature" | "method_signature" | "abstract_method_signature"
        ),
        "dart" => matches!(
            node_type,
            "function_signature"
                | "method_signature"
                | "constructor_signature"
                | "getter_signature"
                | "setter_signature"
        ),
        "delphi" => node_type == "defProc",
        "haskell" => node_type == "signature",
        "odin" => node_type == "procedure_declaration",
        "perl" => node_type == "subroutine_declaration_statement",
        "postscript" => node_type == "procedure",
        "powershell" => matches!(node_type, "function_statement" | "method_statement"),
        "qsharp" => matches!(node_type, "namespace" | "operation"),
        "ruby" => node_type == "method",
        "sas" => matches!(node_type, "data_step" | "proc_step"),
        "scala" => node_type == "object_definition",
        "vbnet" => node_type == "module_block",
        "wat" | "wast" => node_type == "func",
        _ => false,
    }
}

fn first_child_label(node: &SemanticNode) -> &str {
    node.children
        .first()
        .map(|child| child.label.as_str())
        .unwrap_or("")
}

fn is_generic_label(label: &str) -> bool {
    matches!(
        label,
        "" | "(anonymous)" | "call" | "list_lit" | "signature"
    )
}

fn is_clojure_definition_form(form: &str) -> bool {
    matches!(
        form,
        "def"
            | "defn"
            | "defn-"
            | "defmacro"
            | "defmethod"
            | "defmulti"
            | "defonce"
            | "defprotocol"
            | "defrecord"
            | "defstruct"
            | "deftype"
            | "definterface"
    )
}

fn is_clojure_import_form(form: &str) -> bool {
    matches!(
        form,
        "import" | "import_form" | "require" | "require_form" | "use" | "use_form"
    )
}

fn is_clojure_definition_node(node: &SemanticNode) -> bool {
    if matches!(
        node.node_type.as_str(),
        "def_form"
            | "defn_form"
            | "defn-_form"
            | "defmacro_form"
            | "defmethod_form"
            | "defmulti_form"
            | "defonce_form"
            | "defprotocol_form"
            | "defrecord_form"
            | "defstruct_form"
            | "deftype_form"
            | "definterface_form"
    ) {
        return !is_generic_label(&node.label);
    }
    node.node_type == "list_lit"
        && is_clojure_definition_form(first_child_label(node))
        && !is_generic_label(&node.label)
}

fn is_elixir_definition_form(form: &str) -> bool {
    matches!(
        form,
        "def" | "defp" | "defmodule" | "defmacro" | "defmacrop" | "defprotocol" | "defimpl"
    )
}

fn is_elixir_definition_node(node: &SemanticNode) -> bool {
    node.node_type == "call"
        && is_elixir_definition_form(first_child_label(node))
        && !is_generic_label(&node.label)
}

fn is_definition_node(node: &SemanticNode, language: &str) -> bool {
    if is_common_definition_type(&node.node_type)
        || is_language_definition_type(language, &node.node_type)
    {
        return !is_generic_label(&node.label);
    }
    match language {
        "clojure" => is_clojure_definition_node(node),
        "elixir" => is_elixir_definition_node(node),
        _ => false,
    }
}

fn position_from_definition(defn: &SymbolDefinition) -> ReferencePosition {
    ReferencePosition {
        start_line: defn.start_line,
        start_col: defn.start_col,
        end_line: defn.end_line,
        end_col: defn.end_col,
    }
}

fn symbol_kind(node_type: &str) -> &'static str {
    let lower = node_type.to_ascii_lowercase();
    if lower.contains("class")
        || matches!(lower.as_str(), "actor_declaration" | "object_definition")
    {
        return "class";
    }
    if lower.contains("interface") || lower.contains("protocol") || lower.contains("trait") {
        return "interface";
    }
    if lower.contains("method") {
        return "method";
    }
    if lower.contains("function")
        || lower.contains("procedure")
        || lower.contains("subroutine")
        || matches!(
            lower.as_str(),
            "defproc" | "form" | "func" | "operation" | "signature"
        )
    {
        return "function";
    }
    if lower.contains("module") || lower.contains("namespace") {
        return "module";
    }
    if lower.contains("struct") {
        return "struct";
    }
    if lower.contains("enum") {
        return "enum";
    }
    "definition"
}

// ---------------------------------------------------------------------------
// Reference extraction: node types and second tree walk
// ---------------------------------------------------------------------------

fn generic_reference_kind_for(node_type: &str) -> Option<&'static str> {
    match node_type {
        // Calls
        "call"
        | "call_expression"
        | "builtin_call_expression"
        | "command"
        | "exprCall"
        | "function_call_expression"
        | "method_invocation"
        | "method_call_expression"
        | "member_call_expression"
        | "invocation_expression"
        | "function_call" => Some("CALL"),
        // Imports
        "import_statement"
        | "import_from_statement"
        | "import_declaration"
        | "use_declaration"
        | "using_directive" => Some("IMPORT"),
        // Type annotations
        "type" | "type_annotation" | "type_identifier" | "generic_type" => Some("TYPE_USAGE"),
        _ => None,
    }
}

fn reference_kind_for(node: &SemanticNode, language: &str) -> Option<&'static str> {
    if is_generic_label(&node.label) {
        return None;
    }
    if language == "clojure"
        && matches!(
            node.node_type.as_str(),
            "import_form" | "require_form" | "use_form"
        )
    {
        return Some("IMPORT");
    }
    if language == "clojure" && node.node_type == "list_lit" {
        let form = first_child_label(node);
        if is_clojure_definition_form(form) {
            return None;
        }
        if is_clojure_import_form(form) {
            return Some("IMPORT");
        }
        return Some("CALL");
    }
    if language == "elixir" && is_elixir_definition_node(node) {
        return None;
    }
    generic_reference_kind_for(&node.node_type)
}

fn extract_references(
    node: &SemanticNode,
    filename: &str,
    language: &str,
    scope: Option<&str>,
    table: &mut ReferenceTable,
) {
    if let Some(kind) = reference_kind_for(node, language) {
        let usage = ReferenceUsage {
            qualified_name: node.label.clone(),
            file: filename.to_string(),
            node_id: node.id.clone(),
            reference_kind: kind.to_owned(),
            position: ReferencePosition {
                start_line: node.position.start_line,
                start_col: node.position.start_col,
                end_line: node.position.end_line,
                end_col: node.position.end_col,
            },
            language: language.to_string(),
            enclosing_scope: scope.map(str::to_string),
            resolved_definition: None,
        };
        table.entry(node.label.clone()).or_default().push(usage);
    }

    // Advance scope into definition bodies so usages carry the correct
    // enclosing_scope (mirrors the Python _extract_references logic).
    let new_scope: Option<String> = if is_definition_node(node, language) {
        Some(match scope {
            Some(s) => format!("{}.{}", s, node.label),
            None => node.label.clone(),
        })
    } else {
        None
    };
    let child_scope: Option<&str> = new_scope.as_deref().or(scope);
    for child in &node.children {
        extract_references(child, filename, language, child_scope, table);
    }
}

pub fn build_reference_table_impl(files_json: &str) -> String {
    let files: Vec<FileEntry> = match serde_json::from_str(files_json) {
        Ok(f) => f,
        Err(e) => return format!(r#"{{"error":"Failed to parse input: {}"}}"#, e),
    };
    let mut table: ReferenceTable = HashMap::new();
    for entry in &files {
        extract_references(
            &entry.tree,
            &entry.filename,
            &entry.language,
            None,
            &mut table,
        );
    }
    match serde_json::to_string(&table) {
        Ok(s) => s,
        Err(e) => format!(r#"{{"error":"Serialisation error: {}"}}"#, e),
    }
}

// ---------------------------------------------------------------------------
// Symbol extraction: recursive tree walk
// ---------------------------------------------------------------------------

fn extract_symbols(
    node: &SemanticNode,
    filename: &str,
    language: &str,
    scope: Option<&str>,
    table: &mut SymbolTable,
) {
    let qualified: Option<String> = if is_definition_node(node, language) {
        let qname = match scope {
            Some(s) => format!("{}.{}", s, node.label),
            None => node.label.clone(),
        };
        let defn = SymbolDefinition {
            qualified_name: qname.clone(),
            file: filename.to_string(),
            node_type: node.node_type.clone(),
            node_id: node.id.clone(),
            start_line: node.position.start_line,
            start_col: node.position.start_col,
            end_line: node.position.end_line,
            end_col: node.position.end_col,
            language: language.to_string(),
        };
        table.entry(qname.clone()).or_default().push(defn);
        Some(qname)
    } else {
        None
    };

    let child_scope: Option<&str> = qualified.as_deref().or(scope);
    for child in &node.children {
        extract_symbols(child, filename, language, child_scope, table);
    }
}

// ---------------------------------------------------------------------------
// WIT implementation
// ---------------------------------------------------------------------------

pub fn build_symbol_table_impl(files_json: &str) -> String {
    let files: Vec<FileEntry> = match serde_json::from_str(files_json) {
        Ok(f) => f,
        Err(e) => return format!(r#"{{"error":"Failed to parse input: {}"}}"#, e),
    };
    let mut table: SymbolTable = HashMap::new();
    for entry in &files {
        extract_symbols(
            &entry.tree,
            &entry.filename,
            &entry.language,
            None,
            &mut table,
        );
    }
    match serde_json::to_string(&table) {
        Ok(s) => s,
        Err(e) => format!(r#"{{"error":"Serialisation error: {}"}}"#, e),
    }
}

pub fn diff_symbol_tables_impl(old_json: &str, new_json: &str) -> String {
    let old_table: SymbolTable = match serde_json::from_str(old_json) {
        Ok(t) => t,
        Err(e) => return format!(r#"{{"error":"Failed to parse old table: {}"}}"#, e),
    };
    let new_table: SymbolTable = match serde_json::from_str(new_json) {
        Ok(t) => t,
        Err(e) => return format!(r#"{{"error":"Failed to parse new table: {}"}}"#, e),
    };

    let mut changes: Vec<CrossFileChange> = Vec::new();

    // --- Pass 1: MOVE_TO_MODULE -------------------------------------------
    // Consume definitions that remain in the same file first. A qualified
    // name is not unique: taking the Cartesian product of duplicate names
    // invents moves (and grows quadratically) even when the project is
    // unchanged. Only one unmatched old/new pair is strong enough evidence
    // for a move; duplicate unmatched groups stay ambiguous.
    let mut qnames: Vec<&String> = old_table.keys().collect();
    qnames.sort();
    for qname in qnames {
        let old_defs = &old_table[qname];
        if let Some(new_defs) = new_table.get(qname) {
            if let Some((old_def, new_def)) = unique_unmatched_cross_file_move(old_defs, new_defs) {
                changes.push(CrossFileChange {
                    change_type: "MOVE_TO_MODULE",
                    symbol_name: qname.to_string(),
                    old_file: old_def.file.clone(),
                    new_file: new_def.file.clone(),
                    old_node_id: Some(old_def.node_id.clone()),
                    new_node_id: Some(new_def.node_id.clone()),
                    old_position: Some(position_from_definition(old_def)),
                    new_position: Some(position_from_definition(new_def)),
                    old_language: old_def.language.clone(),
                    new_language: new_def.language.clone(),
                    node_type: old_def.node_type.clone(),
                    symbol_kind: symbol_kind(&old_def.node_type).to_string(),
                    confidence: 1.0,
                    description: format!(
                        "'{}' moved from '{}' to '{}'",
                        qname, old_def.file, new_def.file
                    ),
                });
            }
        }
    }

    // --- Pass 1.5: SPLIT_MODULE ------------------------------------------
    // Aggregate exact symbol moves when one old file fans out into multiple
    // new files. Only unambiguous one-definition symbols contribute.
    // BTreeMap, not HashMap: SPLIT_MODULE changes are emitted in iteration order (#251).
    let mut split_candidates: BTreeMap<String, Vec<CrossFileChange>> = BTreeMap::new();
    for change in &changes {
        if change.change_type != "MOVE_TO_MODULE" {
            continue;
        }
        let old_defs = old_table.get(&change.symbol_name);
        let new_defs = new_table.get(&change.symbol_name);
        if old_defs.map(Vec::len) != Some(1) || new_defs.map(Vec::len) != Some(1) {
            continue;
        }
        split_candidates
            .entry(change.old_file.clone())
            .or_default()
            .push(CrossFileChange {
                change_type: change.change_type,
                symbol_name: change.symbol_name.clone(),
                old_file: change.old_file.clone(),
                new_file: change.new_file.clone(),
                old_node_id: change.old_node_id.clone(),
                new_node_id: change.new_node_id.clone(),
                old_position: change.old_position.clone(),
                new_position: change.new_position.clone(),
                old_language: change.old_language.clone(),
                new_language: change.new_language.clone(),
                node_type: change.node_type.clone(),
                symbol_kind: change.symbol_kind.clone(),
                confidence: change.confidence,
                description: change.description.clone(),
            });
    }
    for (old_file, moves) in split_candidates {
        let mut new_files: Vec<String> = moves
            .iter()
            .filter(|m| m.new_file != old_file)
            .map(|m| m.new_file.clone())
            .collect();
        new_files.sort();
        new_files.dedup();
        if moves.len() < 2 || new_files.len() < 2 {
            continue;
        }
        let mut symbols: Vec<String> = moves.iter().map(|m| m.symbol_name.clone()).collect();
        symbols.sort();
        let shown = symbols
            .iter()
            .take(6)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        let suffix = if symbols.len() > 6 { "..." } else { "" };
        changes.push(CrossFileChange {
            change_type: "SPLIT_MODULE",
            symbol_name: old_file.clone(),
            old_file: old_file.clone(),
            new_file: new_files.join(", "),
            old_node_id: None,
            new_node_id: None,
            old_position: None,
            new_position: None,
            old_language: String::new(),
            new_language: String::new(),
            node_type: String::new(),
            symbol_kind: String::new(),
            confidence: 0.9,
            description: format!(
                "'{}' was split across {} files: {} ({}{})",
                old_file,
                new_files.len(),
                new_files.join(", "),
                shown,
                suffix
            ),
        });
    }

    // --- Pass 2: CROSS_FILE_RENAME ----------------------------------------
    let old_names: BTreeSet<&str> = old_table.keys().map(|s| s.as_str()).collect();
    let new_names: BTreeSet<&str> = new_table.keys().map(|s| s.as_str()).collect();

    let removed: Vec<&str> = old_names.difference(&new_names).cloned().collect();
    let added: Vec<&str> = new_names.difference(&old_names).cloned().collect();

    // Group by (file, node_type, parent_scope)
    fn group_key(qname: &str, defs: &[SymbolDefinition]) -> String {
        let defn = &defs[0];
        let parent = qname.rfind('.').map(|i| &qname[..i]).unwrap_or("");
        format!("{}|{}|{}", defn.file, defn.node_type, parent)
    }

    let mut removed_by_key: BTreeMap<String, Vec<(&str, &SymbolDefinition)>> = BTreeMap::new();
    for qname in &removed {
        let defs = &old_table[*qname];
        let key = group_key(qname, defs);
        removed_by_key
            .entry(key)
            .or_default()
            .push((qname, &defs[0]));
    }

    let mut added_by_key: BTreeMap<String, Vec<(&str, &SymbolDefinition)>> = BTreeMap::new();
    for qname in &added {
        let defs = &new_table[*qname];
        let key = group_key(qname, defs);
        added_by_key.entry(key).or_default().push((qname, &defs[0]));
    }

    for (key, removed_items) in &removed_by_key {
        if let Some(added_items) = added_by_key.get(key) {
            if removed_items.len() == 1 && added_items.len() == 1 {
                let (old_qname, old_def) = removed_items[0];
                let (new_qname, new_def) = added_items[0];
                changes.push(CrossFileChange {
                    change_type: "CROSS_FILE_RENAME",
                    symbol_name: old_qname.to_string(),
                    old_file: old_def.file.clone(),
                    new_file: new_def.file.clone(),
                    old_node_id: Some(old_def.node_id.clone()),
                    new_node_id: Some(new_def.node_id.clone()),
                    old_position: Some(position_from_definition(old_def)),
                    new_position: Some(position_from_definition(new_def)),
                    old_language: old_def.language.clone(),
                    new_language: new_def.language.clone(),
                    node_type: old_def.node_type.clone(),
                    symbol_kind: symbol_kind(&old_def.node_type).to_string(),
                    confidence: 0.8,
                    description: format!(
                        "'{}' in '{}' may have been renamed to '{}'",
                        old_qname, old_def.file, new_qname
                    ),
                });
            }
        }
    }

    sort_cross_file_changes(&mut changes);
    match serde_json::to_string(&changes) {
        Ok(s) => s,
        Err(e) => format!(r#"{{"error":"Serialisation error: {}"}}"#, e),
    }
}

/// The documented total order of `diff_symbol_tables` output (#251): pass (moves, splits,
/// renames), then symbol, files, node ids and positions. Identical inputs therefore produce
/// identical arrays across calls and processes. The sort is stable and only reorders, so no
/// distinct change is merged or dropped.
fn sort_cross_file_changes(changes: &mut [CrossFileChange]) {
    fn pass(change_type: &str) -> u8 {
        match change_type {
            "MOVE_TO_MODULE" => 0,
            "SPLIT_MODULE" => 1,
            "CROSS_FILE_RENAME" => 2,
            _ => 3,
        }
    }
    fn position(p: &Option<ReferencePosition>) -> Option<(u32, u32, u32, u32)> {
        p.as_ref().map(|p| (p.start_line, p.start_col, p.end_line, p.end_col))
    }
    changes.sort_by(|a, b| {
        (pass(a.change_type), &a.symbol_name, &a.old_file, &a.new_file)
            .cmp(&(pass(b.change_type), &b.symbol_name, &b.old_file, &b.new_file))
            .then_with(|| (&a.old_node_id, &a.new_node_id).cmp(&(&b.old_node_id, &b.new_node_id)))
            .then_with(|| {
                (position(&a.old_position), position(&a.new_position))
                    .cmp(&(position(&b.old_position), position(&b.new_position)))
            })
    });
}

fn unique_unmatched_cross_file_move<'a>(
    old_defs: &'a [SymbolDefinition],
    new_defs: &'a [SymbolDefinition],
) -> Option<(&'a SymbolDefinition, &'a SymbolDefinition)> {
    let mut old_matched = vec![false; old_defs.len()];
    let mut new_matched = vec![false; new_defs.len()];
    let mut old_by_identity: HashMap<(String, String, String), usize> = HashMap::new();
    let mut new_by_identity: HashMap<(String, String, String), Vec<usize>> = HashMap::new();
    for old_def in old_defs {
        *old_by_identity
            .entry((old_def.file.clone(), old_def.language.clone(), old_def.node_type.clone()))
            .or_default() += 1;
    }
    for (new_index, new_def) in new_defs.iter().enumerate() {
        new_by_identity
            .entry((new_def.file.clone(), new_def.language.clone(), new_def.node_type.clone()))
            .or_default()
            .push(new_index);
    }
    // A changed multiplicity in a duplicate file-local bucket cannot tell us
    // whether a definition moved, was copied, or was deleted. Refuse to infer
    // a move even if the arbitrary remainder would otherwise be one-to-one.
    for (identity, &old_count) in &old_by_identity {
        let new_count = new_by_identity.get(identity).map_or(0, Vec::len);
        if old_count.max(new_count) > 1 && old_count != new_count {
            return None;
        }
    }
    for (identity, new_indices) in &new_by_identity {
        let old_count = old_by_identity.get(identity).copied().unwrap_or(0);
        if old_count.max(new_indices.len()) > 1 && old_count != new_indices.len() {
            return None;
        }
    }
    for (old_index, old_def) in old_defs.iter().enumerate() {
        let identity = (old_def.file.clone(), old_def.language.clone(), old_def.node_type.clone());
        if let Some(new_index) = new_by_identity.get_mut(&identity).and_then(|indices| indices.pop()) {
            old_matched[old_index] = true;
            new_matched[new_index] = true;
        }
    }

    let old_unmatched: Vec<&SymbolDefinition> = old_defs
        .iter()
        .zip(old_matched)
        .filter_map(|(definition, matched)| (!matched).then_some(definition))
        .collect();
    let new_unmatched: Vec<&SymbolDefinition> = new_defs
        .iter()
        .zip(new_matched)
        .filter_map(|(definition, matched)| (!matched).then_some(definition))
        .collect();
    if let ([old_def], [new_def]) = (old_unmatched.as_slice(), new_unmatched.as_slice()) {
        if old_def.file != new_def.file
            && old_def.language == new_def.language
            && old_def.node_type == new_def.node_type
        {
            return Some((*old_def, *new_def));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    // #251: identical inputs give byte-identical change arrays. The old code appended
    // SPLIT_MODULE and CROSS_FILE_RENAME changes while iterating randomly seeded HashMaps.
    #[test]
    fn diff_symbol_tables_output_order_is_deterministic() {
        fn def(qname: &str, file: &str, line: u32) -> Value {
            json!({"qualified_name": qname, "file": file, "node_id": format!("{file}:{line}"),
                   "node_type": "function_definition", "language": "python",
                   "start_line": line, "start_col": 0, "end_line": line + 1, "end_col": 0})
        }
        let mut old = serde_json::Map::new();
        let mut new = serde_json::Map::new();
        for i in 0..8 {
            // A unique move per name, plus a same-scope rename per file pair.
            old.insert(format!("f{i}"), json!([def(&format!("f{i}"), &format!("old{i}.py"), i)]));
            new.insert(format!("f{i}"), json!([def(&format!("f{i}"), &format!("new{i}.py"), i)]));
            old.insert(format!("g{i}"), json!([def(&format!("g{i}"), &format!("r{i}.py"), 40 + i)]));
            new.insert(format!("h{i}"), json!([def(&format!("h{i}"), &format!("r{i}.py"), 40 + i)]));
        }
        // One old file split into two new files.
        for (name, target) in [("s0", "split_a.py"), ("s1", "split_b.py")] {
            old.insert(name.into(), json!([def(name, "whole.py", 90)]));
            new.insert(name.into(), json!([def(name, target, 90)]));
        }
        let (old, new) = (Value::Object(old).to_string(), Value::Object(new).to_string());
        let first = diff_symbol_tables_impl(&old, &new);
        let changes: Vec<Value> = serde_json::from_str(&first).unwrap();
        let kinds = |kind: &str| changes.iter().filter(|c| c["change_type"] == kind).count();
        assert_eq!((kinds("MOVE_TO_MODULE"), kinds("SPLIT_MODULE"), kinds("CROSS_FILE_RENAME")), (10, 1, 8));
        for _ in 0..30 {
            // Re-parsing re-seeds every HashMap, so each call iterates in a fresh order.
            assert_eq!(diff_symbol_tables_impl(&old, &new), first);
        }
        let names: Vec<&str> = changes.iter().map(|c| c["symbol_name"].as_str().unwrap()).collect();
        let mut moves: Vec<&str> = names[..10].to_vec();
        moves.sort();
        assert_eq!(names[..10], moves[..], "moves are ordered by symbol name");
    }

    fn pos() -> Value {
        json!({"start_line": 0, "start_col": 0, "end_line": 0, "end_col": 0})
    }

    fn node(id: &str, node_type: &str, label: &str, children: Vec<Value>) -> Value {
        json!({
            "id": id,
            "node_type": node_type,
            "label": label,
            "position": pos(),
            "children": children,
        })
    }

    fn file(language: &str, tree: Value) -> String {
        json!([{"filename": format!("code.{}", language), "language": language, "tree": tree}])
            .to_string()
    }

    fn symbol_table(language: &str, tree: Value) -> Value {
        serde_json::from_str(&build_symbol_table_impl(&file(language, tree))).unwrap()
    }

    #[test]
    fn unchanged_duplicate_names_are_not_reported_as_module_moves() {
        for count in [2usize, 8, 32] {
            let definitions: Vec<Value> = (0..count)
                .map(|index| json!({
                    "qualified_name": "f",
                    "file": format!("module_{index}.py"),
                    "node_type": "function_definition",
                    "node_id": format!("node_{index}"),
                    "start_line": 0,
                    "start_col": 0,
                    "end_line": 1,
                    "end_col": 1,
                    "language": "python"
                }))
                .collect();
            let table = json!({"f": definitions});
            let changes: Value = serde_json::from_str(&diff_symbol_tables_impl(
                &table.to_string(),
                &table.to_string(),
            ))
            .unwrap();
            assert_eq!(changes, json!([]), "identical {count}-definition tables");
        }
    }

    #[test]
    fn duplicate_name_matching_preserves_copies_and_only_reports_unique_moves() {
        let definition = |file: &str, node_id: &str| json!({
            "qualified_name": "f",
            "file": file,
            "node_type": "function_definition",
            "node_id": node_id,
            "start_line": 0,
            "start_col": 0,
            "end_line": 1,
            "end_col": 1,
            "language": "python"
        });
        let identity = |old: Value, new: Value| -> Value {
            serde_json::from_str(&diff_symbol_tables_impl(&old.to_string(), &new.to_string())).unwrap()
        };

        assert_eq!(
            identity(json!({"f": [definition("a.py", "a")]}), json!({"f": [definition("b.py", "b")]})).as_array().unwrap().len(),
            1,
            "one unmatched old and new definition is a unique move",
        );
        assert_eq!(
            identity(
                json!({"f": [definition("a.py", "old")]}),
                json!({"f": [definition("a.py", "retained"), definition("b.py", "copy")]}),
            ),
            json!([]),
            "a same-file definition plus a new copy is not a move",
        );
        assert_eq!(
            identity(
                json!({"f": [definition("a.py", "a"), definition("b.py", "b")]}),
                json!({"f": [definition("c.py", "c")]}),
            ),
            json!([]),
            "an unmatched duplicate-name group is ambiguous",
        );
        assert_eq!(
            identity(
                json!({"f": [definition("a.py", "a1"), definition("a.py", "a2")]}),
                json!({"f": [definition("a.py", "retained"), definition("b.py", "possible-copy")]}),
            ),
            json!([]),
            "a duplicate same-file bucket with an imbalanced count cannot identify a moved occurrence",
        );
        let retained_and_moved = identity(
            json!({"f": [definition("a.py", "retained-old"), definition("b.py", "old-move")]}),
            json!({"f": [definition("a.py", "retained-new"), definition("c.py", "new-move")]}),
        );
        assert!(retained_and_moved.as_array().unwrap().iter().any(|change| {
            change["change_type"] == "MOVE_TO_MODULE"
                && change["old_file"] == "b.py"
                && change["new_file"] == "c.py"
        }), "same-file occurrences must be consumed before matching a unique move");
    }

    fn reference_table(language: &str, tree: Value) -> Value {
        serde_json::from_str(&build_reference_table_impl(&file(language, tree))).unwrap()
    }

    #[test]
    fn indexes_newer_language_definition_node_types() {
        let cases = [
            ("powershell", "function_statement", "Add-Numbers"),
            ("dart", "function_signature", "add"),
            ("delphi", "defProc", "Add"),
            ("abap", "form", "GREET"),
            ("perl", "subroutine_declaration_statement", "greet"),
            ("ruby", "method", "greet"),
            ("vbnet", "module_block", "HelloWorld"),
            ("qsharp", "operation", "FlipBit"),
            ("odin", "procedure_declaration", "add"),
            ("scala", "object_definition", "Main"),
            ("postscript", "procedure", "greet"),
            ("sas", "data_step", "WORK.GREETING"),
            ("wat", "func", "$add"),
        ];
        for (language, node_type, label) in cases {
            let table = symbol_table(language, node("0", node_type, label, vec![]));
            assert!(table.get(label).is_some(), "{language}:{node_type}:{label}");
        }
    }

    #[test]
    fn signature_is_only_a_definition_for_haskell() {
        let swift = symbol_table("swift", node("0", "signature", "func add()", vec![]));
        assert!(swift.as_object().unwrap().is_empty());

        let haskell = symbol_table("haskell", node("0", "signature", "add", vec![]));
        assert!(haskell.get("add").is_some());
    }

    #[test]
    fn clojure_defn_is_symbol_and_plain_list_is_call_reference() {
        let tree = node(
            "0",
            "source",
            "source",
            vec![
                node(
                    "0.0",
                    "list_lit",
                    "greet",
                    vec![
                        node("0.0.0", "sym_lit", "defn", vec![]),
                        node("0.0.1", "sym_lit", "greet", vec![]),
                    ],
                ),
                node(
                    "0.1",
                    "list_lit",
                    "println",
                    vec![node("0.1.0", "sym_lit", "println", vec![])],
                ),
            ],
        );

        let symbols = symbol_table("clojure", tree.clone());
        assert!(symbols.get("greet").is_some());
        assert!(symbols.get("println").is_none());

        let refs = reference_table("clojure", tree);
        assert!(refs.get("greet").is_none());
        assert_eq!(refs["println"][0]["reference_kind"], "CALL");
    }

    #[test]
    fn elixir_def_calls_are_symbols_not_call_references() {
        let tree = node(
            "0",
            "source",
            "source",
            vec![node(
                "0.0",
                "call",
                "Greeter",
                vec![
                    node("0.0.0", "identifier", "defmodule", vec![]),
                    node(
                        "0.0.1",
                        "do_block",
                        "do_block",
                        vec![
                            node(
                                "0.0.1.0",
                                "call",
                                "greet",
                                vec![node("0.0.1.0.0", "identifier", "def", vec![])],
                            ),
                            node(
                                "0.0.1.1",
                                "call",
                                "IO.puts",
                                vec![node("0.0.1.1.0", "identifier", "IO", vec![])],
                            ),
                        ],
                    ),
                ],
            )],
        );

        let symbols = symbol_table("elixir", tree.clone());
        assert!(symbols.get("Greeter").is_some());
        assert!(symbols.get("Greeter.greet").is_some());

        let refs = reference_table("elixir", tree);
        assert!(refs.get("Greeter").is_none());
        assert!(refs.get("greet").is_none());
        assert_eq!(refs["IO.puts"][0]["enclosing_scope"], "Greeter");
    }

    #[test]
    fn indexes_newer_language_call_node_types() {
        let tree = node(
            "0",
            "source_file",
            "source_file",
            vec![
                node("0.0", "function_call_expression", "print", vec![]),
                node("0.1", "member_call_expression", "run", vec![]),
                node("0.2", "builtin_call_expression", "@import", vec![]),
                node("0.3", "exprCall", "WriteLn", vec![]),
                node("0.4", "command", "Write-Host", vec![]),
            ],
        );
        let refs = reference_table("python", tree);
        for label in ["print", "run", "@import", "WriteLn", "Write-Host"] {
            assert_eq!(refs[label][0]["reference_kind"], "CALL");
        }
    }

    #[test]
    fn detects_file_split_from_exact_symbol_moves() {
        let old_table = json!({
            "a": [{
                "qualified_name": "a",
                "file": "file_a.py",
                "node_type": "function_definition",
                "node_id": "old-a",
                "start_line": 0,
                "start_col": 0,
                "end_line": 1,
                "end_col": 0,
                "language": "python"
            }],
            "b": [{
                "qualified_name": "b",
                "file": "file_a.py",
                "node_type": "function_definition",
                "node_id": "old-b",
                "start_line": 2,
                "start_col": 0,
                "end_line": 3,
                "end_col": 0,
                "language": "python"
            }]
        });
        let new_table = json!({
            "a": [{
                "qualified_name": "a",
                "file": "file_b.py",
                "node_type": "function_definition",
                "node_id": "new-a",
                "start_line": 0,
                "start_col": 0,
                "end_line": 1,
                "end_col": 0,
                "language": "python"
            }],
            "b": [{
                "qualified_name": "b",
                "file": "file_c.py",
                "node_type": "function_definition",
                "node_id": "new-b",
                "start_line": 0,
                "start_col": 0,
                "end_line": 1,
                "end_col": 0,
                "language": "python"
            }]
        });

        let changes: Value = serde_json::from_str(&diff_symbol_tables_impl(
            &old_table.to_string(),
            &new_table.to_string(),
        ))
        .unwrap();

        assert!(changes.as_array().unwrap().iter().any(|change| {
            change["change_type"] == "SPLIT_MODULE"
                && change["old_file"] == "file_a.py"
                && change["new_file"] == "file_b.py, file_c.py"
        }));
    }

    #[test]
    fn cross_file_moves_include_symbol_locations_and_metadata() {
        let old_table = json!({
            "helper": [{
                "qualified_name": "helper",
                "file": "utils.py",
                "node_type": "function_definition",
                "node_id": "old-helper",
                "start_line": 10,
                "start_col": 2,
                "end_line": 12,
                "end_col": 0,
                "language": "python"
            }]
        });
        let new_table = json!({
            "helper": [{
                "qualified_name": "helper",
                "file": "helpers.py",
                "node_type": "function_definition",
                "node_id": "new-helper",
                "start_line": 20,
                "start_col": 4,
                "end_line": 22,
                "end_col": 0,
                "language": "python"
            }]
        });

        let changes: Value = serde_json::from_str(&diff_symbol_tables_impl(
            &old_table.to_string(),
            &new_table.to_string(),
        ))
        .unwrap();
        let move_change = changes
            .as_array()
            .unwrap()
            .iter()
            .find(|change| change["change_type"] == "MOVE_TO_MODULE")
            .unwrap();

        assert_eq!(move_change["old_position"]["start_line"], 10);
        assert_eq!(move_change["new_position"]["start_line"], 20);
        assert_eq!(move_change["old_language"], "python");
        assert_eq!(move_change["new_language"], "python");
        assert_eq!(move_change["node_type"], "function_definition");
        assert_eq!(move_change["symbol_kind"], "function");
    }
}

/// Resolve each usage only when its exact qualified name has one definition.
/// Ambiguity and missing definitions explicitly clear prior resolution; inputs are unchanged.
pub fn resolve_references(definitions: &[SymbolDefinition], references: &[ReferenceUsage]) -> Vec<ReferenceUsage> {
    references.iter().map(|reference| {
        let mut matches = definitions.iter().filter(|definition| definition.qualified_name == reference.qualified_name);
        let first = matches.next();
        let resolved = if matches.next().is_none() { first.cloned() } else { None };
        let mut result = reference.clone();
        result.resolved_definition = resolved;
        result
    }).collect()
}
