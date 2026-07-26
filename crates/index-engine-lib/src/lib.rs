//! Index-engine core logic (shared native + Wasm library).
//!
//! The pure symbol/reference-table extraction and cross-file diff, factored out
//! of the `intentdiff-index-engine` Wasm plugin so the native `rust-core-host`
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
use std::collections::HashMap;

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
struct SymbolDefinition {
    qualified_name: String,
    file: String,
    node_type: String,
    node_id: String,
    start_line: u32,
    start_col: u32,
    end_line: u32,
    end_col: u32,
    language: String,
}

// SymbolTable: qualified_name → list of definitions
type SymbolTable = HashMap<String, Vec<SymbolDefinition>>;

// ---------------------------------------------------------------------------
// Output schema: ReferenceUsage
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
struct ReferencePosition {
    start_line: u32,
    start_col: u32,
    end_line: u32,
    end_col: u32,
}

#[derive(Debug, Serialize)]
struct ReferenceUsage {
    qualified_name: String,
    file: String,
    node_id: String,
    reference_kind: &'static str, // "CALL" | "IMPORT" | "TYPE_USAGE"
    position: ReferencePosition,
    language: String,
    enclosing_scope: Option<String>,
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
            reference_kind: kind,
            position: ReferencePosition {
                start_line: node.position.start_line,
                start_col: node.position.start_col,
                end_line: node.position.end_line,
                end_col: node.position.end_col,
            },
            language: language.to_string(),
            enclosing_scope: scope.map(str::to_string),
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
    for (qname, old_defs) in &old_table {
        if let Some(new_defs) = new_table.get(qname) {
            for old_def in old_defs {
                for new_def in new_defs {
                    if old_def.file != new_def.file {
                        changes.push(CrossFileChange {
                            change_type: "MOVE_TO_MODULE",
                            symbol_name: qname.clone(),
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
        }
    }

    // --- Pass 1.5: SPLIT_MODULE ------------------------------------------
    // Aggregate exact symbol moves when one old file fans out into multiple
    // new files. Only unambiguous one-definition symbols contribute.
    let mut split_candidates: HashMap<String, Vec<CrossFileChange>> = HashMap::new();
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
    let old_names: std::collections::HashSet<&str> = old_table.keys().map(|s| s.as_str()).collect();
    let new_names: std::collections::HashSet<&str> = new_table.keys().map(|s| s.as_str()).collect();

    let removed: Vec<&str> = old_names.difference(&new_names).cloned().collect();
    let added: Vec<&str> = new_names.difference(&old_names).cloned().collect();

    // Group by (file, node_type, parent_scope)
    fn group_key(qname: &str, defs: &[SymbolDefinition]) -> String {
        let defn = &defs[0];
        let parent = qname.rfind('.').map(|i| &qname[..i]).unwrap_or("");
        format!("{}|{}|{}", defn.file, defn.node_type, parent)
    }

    let mut removed_by_key: HashMap<String, Vec<(&str, &SymbolDefinition)>> = HashMap::new();
    for qname in &removed {
        let defs = &old_table[*qname];
        let key = group_key(qname, defs);
        removed_by_key
            .entry(key)
            .or_default()
            .push((qname, &defs[0]));
    }

    let mut added_by_key: HashMap<String, Vec<(&str, &SymbolDefinition)>> = HashMap::new();
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

    match serde_json::to_string(&changes) {
        Ok(s) => s,
        Err(e) => format!(r#"{{"error":"Serialisation error: {}"}}"#, e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

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
