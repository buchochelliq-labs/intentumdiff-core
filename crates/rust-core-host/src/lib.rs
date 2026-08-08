use rayon::prelude::*;
use rayon::ThreadPoolBuilder;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Component as PathComponent, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Instant, UNIX_EPOCH};
use tree_sitter::Parser as TreeSitterParser;
use wasmtime::component::{Component, HasSelf, Linker, ResourceTable};
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};
// The analytics / cache stores are pure Rust (#B.4): `analytics_store` / `cache_store` hold the
// store logic, driven by the clap CLI and reached across the language boundary via the C ABI's
// `cache_*` / `analytics_*` handlers. The retired pyo3 pyclass skins (`analytics` / `cache`) were
// deleted with the `python` feature (#B.6). `duckdb_ffi` (pure dlopen, no pyo3) is ungated too.
pub mod analytics_store;
pub mod analytics_registry;
// pub for the same reason `live_server` is: the perceptual asset entry points are engine API,
// and an in-process consumer (the native live-server binary, #100) reaches them by linking the
// crate rather than through the C ABI.
pub mod asset_diff;
mod duckdb_ffi;
mod git_source;
mod registry;
// pub: the live-server protocol + handler impls are the engine's binding-independent public
// API — the native live-server binary (crates/live-server, #100) links them feature-off.
pub mod c_abi;
pub mod live_server;
mod lsp_enrich;
pub mod lsp_server_shapes;
mod parser_registry;
mod vcs_backend;
mod cache_keys;
pub mod cache_store;
pub mod cache_registry;
mod config;
mod content_type;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const COMPLETE: &str = "complete";
const CANDIDATE: &str = "candidate";
const SCAFFOLD: &str = "scaffold";
const FALLBACK: &str = "fallback";
const PARTIAL: &str = "partial";
const BATCH_ENGINE: &str = "rust_core_batch_v4";
const V3_ENGINE: &str = "rust_core_sources_v3_stage11";
const PYTHON_V4E_CERTIFICATION: &str = "python_v4e";
const PYTHON_NATIVE_V4K_CERTIFICATION: &str = "python_native_v4k";
const PYTHON_NATIVE_V4KB_CERTIFICATION: &str = "python_native_v4kb";
const PYTHON_PARSER_BACKEND_WASM: &str = "wasm";
const PYTHON_PARSER_BACKEND_NATIVE: &str = "native";
const PYTHON_TRIVIA: &[&str] = &["comment", "line_comment", "block_comment", "whitespace"];
const DEFAULT_MAX_CST_BYTES: usize = 4 * 1024 * 1024;
const DEFAULT_MAX_PLUGIN_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
const HOST_UTILS_MAX_TRIVIA_TYPES: usize = 1_024;
const HOST_UTILS_MAX_TRIVIA_TYPE_BYTES: usize = 256;
const HOST_UTILS_MAX_TRIVIA_BYTES: usize = 64 * 1024;

mod parser_plugin {
    wasmtime::component::bindgen!({
        path: "wit/plugin.wit",
        world: "parser-plugin",
    });
}

const SEMANTIC_TYPES: &[&str] = &[
    "module",
    "function_definition",
    "async_function_def",
    "class_definition",
    "decorated_definition",
    "assignment",
    "augmented_assignment",
    "return_statement",
    "import_statement",
    "import_from_statement",
    "if_statement",
    "elif_clause",
    "else_clause",
    "for_statement",
    "while_statement",
    "try_statement",
    "except_clause",
    "with_statement",
    "raise_statement",
    "assert_statement",
    "delete_statement",
    // Trivial statements are REAL body content (issue #41: with pass pruned, `def f(): pass`
    // parsed body-less, so pass -> print(...) had no deletion side and the whole edit
    // vanished into a false style-only).
    "pass_statement",
    "break_statement",
    "continue_statement",
    "ellipsis",
    "expression_statement",
    "call",
    "identifier",
    "string",
    "integer",
    "float",
    "true",
    "false",
    "none",
    "parameters",
    "argument_list",
    "type",
    "type_annotation",
];

/// Certified-fast-path predicate (ungated so the C ABI reaches it too). The
/// matching/edit-script pipeline itself is language-agnostic (see
/// `diff_semantic_tree_json`); this only reports the certified-python path that
/// `try_rust_core_sources_stage11` and the batch loader currently rely on.
pub(crate) fn supports_language_impl(language: &str) -> bool {
    language.eq_ignore_ascii_case("python")
}

/// python raw-source entry point (exact no-change only). The C ABI (`intentumdiff_call`) calls this
/// directly. `config_json` is accepted for signature parity with the CST entry point but unused here.
pub(crate) fn diff_python_impl(
    old_content: &str,
    new_content: &str,
    old_filename: &str,
    new_filename: &str,
) -> String {
    let status = if old_content == new_content {
        COMPLETE
    } else {
        SCAFFOLD
    };
    let payload = semantic_diff_payload(
        old_filename,
        new_filename,
        Vec::new(),
        false,
        status,
        json!({
            "note": "raw-source entry point only accepts exact no-change results; changed files use the CST entry point."
        }),
    );
    payload.to_string()
}

/// python filtered-CST v1 entrypoint. The C ABI (`intentumdiff_call`) calls this directly; the
/// binding keeps the full signature (raw-source + wasm-path args are unused here) for parity.
pub(crate) fn diff_python_cst_impl(
    old_filtered_cst_json: &str,
    new_filtered_cst_json: &str,
    old_filename: &str,
    new_filename: &str,
    config_json: &str,
) -> Result<String, String> {
    let config = RustCoreConfig::from_json(config_json);
    check_byte_limit("old CST JSON", old_filtered_cst_json, config.max_cst_bytes)?;
    check_byte_limit("new CST JSON", new_filtered_cst_json, config.max_cst_bytes)?;
    let old_cst: CstNode =
        serde_json::from_str(old_filtered_cst_json).map_err(|exc| format!("old CST JSON: {exc}"))?;
    let new_cst: CstNode =
        serde_json::from_str(new_filtered_cst_json).map_err(|exc| format!("new CST JSON: {exc}"))?;
    let mut old_tree = convert_cst(&old_cst, "0", None)
        .ok_or_else(|| "old CST produced no semantic tree".to_string())?;
    let mut new_tree = convert_cst(&new_cst, "0", None)
        .ok_or_else(|| "new CST produced no semantic tree".to_string())?;
    // Fill any facts the CST pass could not reach, exactly as the serialized-tree entry point
    // does. `convert_cst` derives facts from the RAW CST, whose shape varies by parser, so it
    // can return a partial bag — a tree-sitter CST nests calls and keeps keyword tokens where
    // the native CST does not, which silently cost this path `recursive`, `has_error_handling`,
    // `method_count`, `control_shape` and `behavior_category`.
    //
    // `enrich_tree_facts` derives from the NORMALISED tree, which is parser-independent, and
    // merges without overwriting — so the pre-pruning CST pass still wins every key it did
    // compute. Without this call the two entry points disagreed about the same source.
    enrich_tree_facts(&mut old_tree);
    enrich_tree_facts(&mut new_tree);
    validate_unique_ids(&old_tree).map_err(|exc| format!("old semantic tree: {exc}"))?;
    validate_unique_ids(&new_tree).map_err(|exc| format!("new semantic tree: {exc}"))?;

    let old_count = 1 + old_tree.descendants().len();
    let new_count = 1 + new_tree.descendants().len();
    if old_count > config.max_nodes || new_count > config.max_nodes {
        let payload = semantic_diff_payload(
            old_filename,
            new_filename,
            Vec::new(),
            false,
            COMPLETE,
            json!({
                "skipped": "tree_too_large",
                "old_nodes": old_count,
                "new_nodes": new_count,
            }),
        );
        return Ok(payload.to_string());
    }

    let matching = compute_matching(
        &old_tree,
        &new_tree,
        config.min_height,
        config.min_similarity,
    );
    let changes = generate_changes(&old_tree, &new_tree, &matching);
    let has_semantic_changes = !changes.is_empty();
    let payload = semantic_diff_payload(
        old_filename,
        new_filename,
        changes,
        has_semantic_changes,
        COMPLETE,
        json!({
            "old_nodes": old_count,
            "new_nodes": new_count,
            "matching_pairs": matching.len(),
            "engine": "rust_core_cst_v1",
            "wasm_boundary": "pending",
            "note": "v1 consumes filtered CST directly; the default Python pipeline still keeps Wasm plugins as the stable boundary."
        }),
    );
    Ok(payload.to_string())
}

/// Language-agnostic semantic-tree diff entrypoint.
///
/// The GumTree matching/edit-script pipeline operates purely on the
/// `SemanticNode` shape — it does not parse source text or call any
/// language-specific runtime. The Python-only guard on the legacy
/// `diff_python_semantic_tree_json` was a certification gate, not an
/// algorithm limitation. This entrypoint accepts any language label
/// and is the engine boundary target for `docs/ENGINE_BOUNDARY_AUDIT.md`
/// migration step 2 ("matching/diff → Rust").
fn diff_semantic_tree_impl(
    old_tree_json: &str,
    new_tree_json: &str,
    old_filename: &str,
    new_filename: &str,
    language: &str,
    config_json: &str,
    engine_label: &str,
) -> Result<String, String> {
    let config = RustCoreConfig::from_json(config_json);
    let mut old_tree: SemanticNode = serde_json::from_str(old_tree_json)
        .map_err(|exc| format!("old SemanticNode JSON: {exc}"))?;
    let mut new_tree: SemanticNode = serde_json::from_str(new_tree_json)
        .map_err(|exc| format!("new SemanticNode JSON: {exc}"))?;
    validate_unique_ids(&old_tree).map_err(|exc| format!("old semantic tree: {exc}"))?;
    validate_unique_ids(&new_tree).map_err(|exc| format!("new semantic tree: {exc}"))?;

    // Cross-language NodeFacts (issue #70): Wasm-parsed trees carry no facts, so derive them
    // language-agnostically from the tree here. Nodes that already have facts (Python native
    // path) are left untouched. Only changes labels/facts, not structure.
    enrich_tree_facts(&mut old_tree);
    enrich_tree_facts(&mut new_tree);

    let old_count = 1 + old_tree.descendants().len();
    let new_count = 1 + new_tree.descendants().len();
    if old_count > config.max_nodes || new_count > config.max_nodes {
        return Ok(json!({
            "status": COMPLETE,
            "engine": engine_label,
            "changes": [],
            "matching_pairs": [],
            "metadata": {
                "skipped": "tree_too_large",
                "old_nodes": old_count,
                "new_nodes": new_count,
            },
        })
        .to_string());
    }

    let matching = compute_matching(
        &old_tree,
        &new_tree,
        config.min_height,
        config.min_similarity,
    );
    let changes = generate_changes(&old_tree, &new_tree, &matching);
    let matching_pairs: Vec<Value> = matching
        .iter()
        .map(|pair| {
            json!({
                "old_id": pair.old_node.id,
                "new_id": pair.new_node.id,
            })
        })
        .collect();

    Ok(json!({
        "status": COMPLETE,
        "engine": engine_label,
        "old_filename": old_filename,
        "new_filename": new_filename,
        "language": language,
        "changes": changes,
        "matching_pairs": matching_pairs,
        "metadata": {
            "old_nodes": old_count,
            "new_nodes": new_count,
            "matching_pairs": matching.len(),
            "wasm_boundary": "python_pipeline",
        },
    })
    .to_string())
}

// ---------------------------------------------------------------------------
// Generic text review stage (issue #35) — the engine-side port of the Python
// generic-text presentation: line diff -> text_line changes, relocated-line
// netting (issue #14), blank symmetry (issue #15), inline char detail, and the
// presentation.generic_text_diff suppression AUDIT group. Python keeps only a
// size-cap fallback.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TextOp {
    Equal,
    Replace,
    Delete,
    Insert,
}

mod text_review_generic;
use text_review_generic::*;


mod markdown_review;
use markdown_review::*;


/// python markdown post-presentation rules (issue #36): section moves (LIS insertion-shift
/// discrimination) + heading renames by unique body hash. The C ABI (`intentumdiff_call`) calls
/// this directly. Infallible.
pub(crate) fn markdown_section_review_impl(old_source: &str, new_source: &str) -> String {
    let old_sections = markdown_sections(old_source, "old");
    let new_sections = markdown_sections(new_source, "new");

    // ---- Moves: unique-common sections whose relative order broke (LIS keeps the
    // stationary anchors; a swap of two sections is ONE move).
    let mut old_by_hash: HashMap<&str, Vec<usize>> = HashMap::new();
    let mut new_by_hash: HashMap<&str, Vec<usize>> = HashMap::new();
    for (idx, section) in old_sections.iter().enumerate() {
        old_by_hash.entry(section.section_hash.as_str()).or_default().push(idx);
    }
    for (idx, section) in new_sections.iter().enumerate() {
        new_by_hash.entry(section.section_hash.as_str()).or_default().push(idx);
    }
    let mut unique_common: Vec<&str> = old_by_hash
        .iter()
        .filter(|(hash, old_idx)| {
            old_idx.len() == 1 && new_by_hash.get(**hash).is_some_and(|n| n.len() == 1)
        })
        .map(|(hash, _)| *hash)
        .collect();
    unique_common.sort_by_key(|hash| old_by_hash[hash][0]);
    let new_order: Vec<usize> = {
        let mut by_new: Vec<&str> = unique_common.clone();
        by_new.sort_by_key(|hash| new_by_hash[hash][0]);
        let rank: HashMap<&str, usize> =
            by_new.iter().enumerate().map(|(rank, hash)| (*hash, rank)).collect();
        unique_common.iter().map(|hash| rank[hash]).collect()
    };
    let stationary = longest_increasing_subsequence_positions(&new_order);
    let mut moves: Vec<Value> = Vec::new();
    let mut moved_labels: Vec<String> = Vec::new();
    let mut moved_old_ids: Vec<String> = Vec::new();
    let mut moved_new_ids: Vec<String> = Vec::new();
    for (position, hash) in unique_common.iter().enumerate() {
        if new_order[position] == position || stationary.contains(&position) {
            continue;
        }
        let old_node = &old_sections[old_by_hash[hash][0]];
        let new_node = &new_sections[new_by_hash[hash][0]];
        moves.push(serde_json::json!({
            "change_type": "MOVE",
            "old_node": markdown_section_node_json(old_node),
            "new_node": markdown_section_node_json(new_node),
            "confidence": 0.9,
            "description": format!("Move Markdown section {:?}", old_node.label),
        }));
        moved_labels.push(old_node.label.clone());
        moved_old_ids.push(old_node.id.clone());
        moved_new_ids.push(new_node.id.clone());
    }
    let move_group = if moves.is_empty() {
        Value::Null
    } else {
        let mut sorted_labels = moved_labels.clone();
        sorted_labels.sort();
        serde_json::json!({
            "kind": "MOVED_CODE",
            "raw_change_indices": [],
            "old_labels": sorted_labels,
            "new_labels": sorted_labels,
            "old_node_ids": moved_old_ids,
            "new_node_ids": moved_new_ids,
            "confidence": 0.9,
            "rule_id": "presentation.markdown_section_move",
            "metadata": {"moved_section_count": moves.len()},
        })
    };

    // ---- Heading renames: unique body hash on both sides, differing heading labels.
    let mut old_by_body: HashMap<&str, Vec<usize>> = HashMap::new();
    let mut new_by_body: HashMap<&str, Vec<usize>> = HashMap::new();
    for (idx, section) in old_sections.iter().enumerate() {
        old_by_body.entry(section.body_hash.as_str()).or_default().push(idx);
    }
    for (idx, section) in new_sections.iter().enumerate() {
        new_by_body.entry(section.body_hash.as_str()).or_default().push(idx);
    }
    let mut renames: Vec<Value> = Vec::new();
    let mut old_heading_lines: Vec<usize> = Vec::new();
    let mut new_heading_lines: Vec<usize> = Vec::new();
    let mut rename_old_labels: Vec<String> = Vec::new();
    let mut rename_new_labels: Vec<String> = Vec::new();
    let mut rename_old_ids: Vec<String> = Vec::new();
    let mut rename_new_ids: Vec<String> = Vec::new();
    let mut body_hashes: Vec<&str> = old_by_body.keys().copied().collect();
    body_hashes.sort_by_key(|hash| old_by_body[hash][0]);
    for body_hash in body_hashes {
        let old_matches = &old_by_body[body_hash];
        let Some(new_matches) = new_by_body.get(body_hash) else { continue };
        if old_matches.len() != 1 || new_matches.len() != 1 {
            continue;
        }
        let old_node = &old_sections[old_matches[0]];
        let new_node = &new_sections[new_matches[0]];
        if old_node.label == new_node.label {
            continue;
        }
        renames.push(serde_json::json!({
            "change_type": "MODIFICATION",
            "old_node": markdown_section_node_json(old_node),
            "new_node": markdown_section_node_json(new_node),
            "confidence": 0.9,
            "description": format!(
                "Rename Markdown section {:?} -> {:?}", old_node.label, new_node.label),
        }));
        old_heading_lines.push(old_node.start_line);
        new_heading_lines.push(new_node.start_line);
        rename_old_labels.push(old_node.label.clone());
        rename_new_labels.push(new_node.label.clone());
        rename_old_ids.push(old_node.id.clone());
        rename_new_ids.push(new_node.id.clone());
    }
    let rename_group = if renames.is_empty() {
        Value::Null
    } else {
        serde_json::json!({
            "kind": "MEANINGFUL_CHANGE",
            "raw_change_indices": [],
            "old_labels": rename_old_labels,
            "new_labels": rename_new_labels,
            "old_node_ids": rename_old_ids,
            "new_node_ids": rename_new_ids,
            "confidence": 0.9,
            "rule_id": "presentation.markdown_section_heading_rename",
            "metadata": {"renamed_section_count": renames.len()},
        })
    };

    serde_json::json!({
        "used": true,
        "moves": moves,
        "moved_labels": moved_labels,
        "move_group": move_group,
        "renames": renames,
        "old_heading_lines": old_heading_lines,
        "new_heading_lines": new_heading_lines,
        "rename_group": rename_group,
    })
    .to_string()
}

// ===================== Resource-profile matching (issue #39) =====================
// Port of python analysis/resource_profiles.py — puppet first. Keys resources by
// type+title and attributes by name so the matcher pairs them by IDENTITY, not by
// position/structure (which cross-pairs a puppet attribute value with an unrelated
// class-parameter default). dockerfile/hcl reuse the same mechanism when they route.

fn resource_profile_language(language: &str) -> bool {
    matches!(language, "puppet" | "hcl" | "dockerfile")
}

/// python resource_profiles._normalize: strip matching surrounding quotes, collapse
/// internal whitespace, lowercase.
fn resource_normalize(label: &str) -> String {
    let mut text = label.trim();
    let mut chars = text.chars();
    if let (Some(first), Some(last)) = (chars.next(), text.chars().last()) {
        if text.chars().count() >= 2
            && ((first == '"' && last == '"') || (first == '\'' && last == '\''))
        {
            text = text[first.len_utf8()..text.len() - last.len_utf8()].trim();
        }
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// python resource_profiles._is_generic_label.
fn is_generic_resource_label(label: &str, node_type: &str) -> bool {
    let text = label.trim();
    if text.is_empty() {
        return true;
    }
    let lowered = text.to_lowercase();
    if lowered == node_type.to_lowercase() {
        return true;
    }
    matches!(
        lowered.as_str(),
        "attribute_list"
            | "block"
            | "body"
            | "collection_value"
            | "config_file"
            | "expression"
            | "image_spec"
            | "literal_value"
            | "manifest"
            | "object"
            | "object_elem"
            | "parameter_list"
            | "resource_body"
            | "source_file"
            | "string_lit"
    )
}

fn resource_node_sort_key(node: &SemanticNode) -> (u32, u32, &str) {
    (
        node.position.start_line,
        node.position.start_col,
        node.id.as_str(),
    )
}

/// Nearest ancestor (walking the dot-path id) whose lowercased node_type is in `types`.
fn nearest_ancestor_of_types<'a>(
    id: &str,
    by_id: &HashMap<&str, &'a SemanticNode>,
    types: &[&str],
) -> Option<&'a SemanticNode> {
    let mut current = id.to_string();
    while let Some((parent_id, _)) = current.rsplit_once('.') {
        let parent_id = parent_id.to_string();
        if let Some(node) = by_id.get(parent_id.as_str()).copied() {
            if types.contains(&node.node_type.to_lowercase().as_str()) {
                return Some(node);
            }
        }
        current = parent_id;
    }
    None
}

/// The child of `ancestor_id` that contains `node` (walk up until the parent is ancestor).
fn direct_child_under<'a>(
    ancestor_id: &str,
    node: &'a SemanticNode,
    by_id: &HashMap<&str, &'a SemanticNode>,
) -> Option<&'a SemanticNode> {
    let mut current = node;
    loop {
        let (parent_id, _) = current.id.rsplit_once('.')?;
        if parent_id == ancestor_id {
            return Some(current);
        }
        current = by_id.get(parent_id).copied()?;
    }
}

/// python resource_profiles._puppet_parent_scope.
fn puppet_parent_scope(id: &str, by_id: &HashMap<&str, &SemanticNode>) -> Vec<String> {
    let scope_types = [
        "class_definition",
        "defined_type",
        "node_definition",
        "node_statement",
    ];
    let mut labels: Vec<String> = Vec::new();
    let mut current = id.to_string();
    while let Some((parent_id, _)) = current.rsplit_once('.') {
        let parent_id = parent_id.to_string();
        if let Some(node) = by_id.get(parent_id.as_str()).copied() {
            let nt = node.node_type.to_lowercase();
            if scope_types.contains(&nt.as_str())
                && !is_generic_resource_label(&node.label, &node.node_type)
            {
                labels.push(format!("{}:{}", nt, resource_normalize(&node.label)));
            }
        }
        current = parent_id;
    }
    labels.reverse();
    labels
}

/// python resource_profiles._puppet_resource_identity_from_children (raw parts, for LABELS).
fn puppet_resource_identity_raw(node: &SemanticNode) -> Vec<String> {
    let mut raw: Vec<String> = Vec::new();
    if !node.label.is_empty() && !is_generic_resource_label(&node.label, &node.node_type) {
        if let Some(first) = node.label.split_whitespace().next() {
            raw.push(first.to_string());
        }
    }
    for child in &node.children {
        let ct = child.node_type.to_lowercase();
        if ct == "string" || ct == "title" {
            if !child.label.is_empty() {
                raw.push(
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
    raw.truncate(2);
    raw
}

/// python resource_profiles._puppet_resource_identity (normalized parts, for KEYS).
fn puppet_resource_identity(node: &SemanticNode) -> Vec<String> {
    puppet_resource_identity_raw(node)
        .into_iter()
        .map(|part| resource_normalize(&part))
        .filter(|part| !part.is_empty())
        .collect()
}

/// python resource_profiles.is_resource_profile_review_container (per-profile review types).
fn is_resource_review_container(node: &SemanticNode, language: &str) -> bool {
    let nt = node.node_type.to_lowercase();
    let is_review = match language {
        "puppet" => matches!(
            nt.as_str(),
            "attribute"
                | "class_definition"
                | "defined_type"
                | "node_definition"
                | "node_statement"
                | "parameter"
                | "resource_declaration"
                | "resource_statement"
        ),
        "hcl" => matches!(nt.as_str(), "attribute" | "block"),
        _ => return false,
    };
    is_review && !is_generic_resource_label(&node.label, &node.node_type)
}

/// python resource_profiles._has_unmatched_resource_ancestor: an ancestor whose key has no
/// partner on the opposite side (its whole resource is added/deleted) covers this node.
fn has_unmatched_resource_ancestor(
    id: &str,
    by_id: &HashMap<&str, &SemanticNode>,
    keys: &HashMap<&str, Vec<String>>,
    opposite_keys: &HashSet<Vec<String>>,
    language: &str,
) -> bool {
    let mut current = id.to_string();
    while let Some((parent_id, _)) = current.rsplit_once('.') {
        let parent_id = parent_id.to_string();
        if let Some(node) = by_id.get(parent_id.as_str()).copied() {
            if let Some(key) = keys.get(parent_id.as_str()) {
                if !opposite_keys.contains(key) && is_resource_review_container(node, language) {
                    return true;
                }
            }
        }
        current = parent_id;
    }
    false
}

/// python resource_profiles.enrich_resource_profile_labels (puppet): fill resource/parameter
/// identity labels from semantic children when the parser emits partial/empty labels.
fn enrich_resource_profile_labels(node: &mut SemanticNode, language: &str) {
    for child in &mut node.children {
        enrich_resource_profile_labels(child, language);
    }
    let nt = node.node_type.to_lowercase();
    if language == "puppet" {
        if nt == "resource_declaration" || nt == "resource_statement" {
            let parts = puppet_resource_identity_raw(node);
            if !parts.is_empty() {
                node.label = parts.join(" ");
            }
        } else if nt == "parameter" {
            if let Some(label) = first_descendant_label(node, "variable") {
                node.label = label;
            }
        }
    } else if language == "hcl" && nt == "block" {
        let parts = hcl_block_identity_raw(node);
        if !parts.is_empty() {
            node.label = parts.join(" ");
        }
    }
}

/// python resource_profiles._first_descendant_label: first descendant (self excluded at the
/// call site) of the given node_type with a non-empty label.
fn first_descendant_label(node: &SemanticNode, node_type: &str) -> Option<String> {
    for child in &node.children {
        if let Some(label) = descendant_label_including(child, node_type) {
            return Some(label);
        }
    }
    None
}

fn descendant_label_including(node: &SemanticNode, node_type: &str) -> Option<String> {
    if node.node_type.to_lowercase() == node_type && !node.label.is_empty() {
        return Some(node.label.clone());
    }
    for child in &node.children {
        if let Some(label) = descendant_label_including(child, node_type) {
            return Some(label);
        }
    }
    None
}

/// python resource_profiles.augment_resource_profile_changes (subset needed for puppet's
/// acceptance test): same-key attribute value MODIFICATION + keyed review-container ADD/DELETE.
/// (The MOVE->MODIFICATION relocation branch is not needed by the puppet playground.)
fn augment_resource_profile_changes_drafts<'a>(
    changes: &mut Vec<ChangeDraft<'a>>,
    old_tree: &'a SemanticNode,
    new_tree: &'a SemanticNode,
    language: &str,
) {
    if !resource_profile_language(language) {
        return;
    }
    let old_by_id = semantic_node_refs_by_id_with_root(old_tree);
    let new_by_id = semantic_node_refs_by_id_with_root(new_tree);
    let old_keys: HashMap<&str, Vec<String>> = old_by_id
        .iter()
        .filter_map(|(id, node)| resource_profile_key(node, &old_by_id, language).map(|k| (*id, k)))
        .collect();
    let new_keys: HashMap<&str, Vec<String>> = new_by_id
        .iter()
        .filter_map(|(id, node)| resource_profile_key(node, &new_by_id, language).map(|k| (*id, k)))
        .collect();
    let old_key_set: HashSet<Vec<String>> = old_keys.values().cloned().collect();
    let new_key_set: HashSet<Vec<String>> = new_keys.values().cloned().collect();

    let mut mentioned_old: HashSet<String> = changes
        .iter()
        .filter_map(|c| c.old_node.map(|n| n.id.clone()))
        .collect();
    let mut mentioned_new: HashSet<String> = changes
        .iter()
        .filter_map(|c| c.new_node.map(|n| n.id.clone()))
        .collect();
    let mut existing_mod_pairs: HashSet<(String, String)> = changes
        .iter()
        .filter(|c| c.change_type == "MODIFICATION")
        .map(|c| {
            (
                c.old_node.map(|n| n.id.clone()).unwrap_or_default(),
                c.new_node.map(|n| n.id.clone()).unwrap_or_default(),
            )
        })
        .collect();

    let mut old_sorted: Vec<&SemanticNode> = old_by_id.values().copied().collect();
    old_sorted.sort_by(|a, b| resource_node_sort_key(a).cmp(&resource_node_sort_key(b)));
    let mut new_sorted: Vec<&SemanticNode> = new_by_id.values().copied().collect();
    new_sorted.sort_by(|a, b| resource_node_sort_key(a).cmp(&resource_node_sort_key(b)));

    // key -> new node (tree order, last wins) for same-key attribute lookup.
    let mut new_node_by_key: HashMap<Vec<String>, &SemanticNode> = HashMap::new();
    for node in &new_sorted {
        if let Some(key) = new_keys.get(node.id.as_str()) {
            new_node_by_key.insert(key.clone(), node);
        }
    }

    let mut additions: Vec<ChangeDraft<'a>> = Vec::new();

    // (a) Same-key attribute whose VALUE changed -> one attribute-level MODIFICATION.
    for old_node in &old_sorted {
        let Some(key) = old_keys.get(old_node.id.as_str()) else {
            continue;
        };
        if old_node.node_type != "attribute" {
            continue;
        }
        let Some(new_node) = new_node_by_key.get(key).copied() else {
            continue;
        };
        if new_node.node_type != "attribute"
            || old_node.structural_hash == new_node.structural_hash
            || existing_mod_pairs.contains(&(old_node.id.clone(), new_node.id.clone()))
        {
            continue;
        }
        additions.push(ChangeDraft {
            change_type: "MODIFICATION",
            old_node: Some(old_node),
            new_node: Some(new_node),
            old_index: None,
            new_index: None,
            confidence: 0.9,
            description: format!(
                "Update attribute({:?}) -> attribute({:?})",
                old_node.label, new_node.label
            ),
            refactoring_kind: None,
            text_diff: None,
        });
        existing_mod_pairs.insert((old_node.id.clone(), new_node.id.clone()));
    }

    // (b) Keyed review-container present only on the old side -> DELETION.
    for old_node in &old_sorted {
        let Some(key) = old_keys.get(old_node.id.as_str()) else {
            continue;
        };
        if new_key_set.contains(key)
            || mentioned_old.contains(old_node.id.as_str())
            || !is_resource_review_container(old_node, language)
            || has_unmatched_resource_ancestor(
                old_node.id.as_str(),
                &old_by_id,
                &old_keys,
                &new_key_set,
                language,
            )
        {
            continue;
        }
        additions.push(ChangeDraft {
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
        mentioned_old.insert(old_node.id.clone());
    }

    // (c) Keyed review-container present only on the new side -> ADDITION.
    for new_node in &new_sorted {
        let Some(key) = new_keys.get(new_node.id.as_str()) else {
            continue;
        };
        if old_key_set.contains(key)
            || mentioned_new.contains(new_node.id.as_str())
            || !is_resource_review_container(new_node, language)
            || has_unmatched_resource_ancestor(
                new_node.id.as_str(),
                &new_by_id,
                &new_keys,
                &old_key_set,
                language,
            )
        {
            continue;
        }
        additions.push(ChangeDraft {
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
        mentioned_new.insert(new_node.id.clone());
    }

    changes.extend(additions);
}

/// python resource_profiles._puppet_key.
fn puppet_key(node: &SemanticNode, by_id: &HashMap<&str, &SemanticNode>) -> Option<Vec<String>> {
    let nt = node.node_type.to_lowercase();
    if matches!(
        nt.as_str(),
        "class_definition" | "defined_type" | "node_definition" | "node_statement"
    ) {
        if is_generic_resource_label(&node.label, &node.node_type) {
            return None;
        }
        let mut key = vec!["puppet".to_string(), nt];
        key.extend(puppet_parent_scope(&node.id, by_id));
        key.push(resource_normalize(&node.label));
        return Some(key);
    }
    if matches!(nt.as_str(), "resource_declaration" | "resource_statement") {
        let identity = puppet_resource_identity(node);
        if identity.is_empty() {
            return None;
        }
        let mut key = vec!["puppet".to_string(), "resource".to_string()];
        key.extend(puppet_parent_scope(&node.id, by_id));
        key.extend(identity);
        return Some(key);
    }
    if nt == "attribute" && !is_generic_resource_label(&node.label, &node.node_type) {
        let resource = nearest_ancestor_of_types(
            &node.id,
            by_id,
            &["resource_declaration", "resource_statement"],
        )?;
        let mut key = puppet_key(resource, by_id)?;
        key.push("attribute".to_string());
        key.push(resource_normalize(&node.label));
        return Some(key);
    }
    if nt == "parameter" && !is_generic_resource_label(&node.label, &node.node_type) {
        let mut key = vec!["puppet".to_string(), "parameter".to_string()];
        key.extend(puppet_parent_scope(&node.id, by_id));
        key.push(resource_normalize(&node.label));
        return Some(key);
    }
    if let Some(resource) = nearest_ancestor_of_types(
        &node.id,
        by_id,
        &["resource_declaration", "resource_statement"],
    ) {
        if let Some(branch) = direct_child_under(&resource.id, node, by_id) {
            let bt = branch.node_type.to_lowercase();
            if bt == "string" || bt == "title" {
                if let Some(mut key) = puppet_key(resource, by_id) {
                    key.push("title".to_string());
                    return Some(key);
                }
            }
        }
    }
    None
}

/// python resource_profiles._first_concrete_label: self-or-first-descendant (DFS) with a
/// non-generic label.
fn first_concrete_label(node: &SemanticNode) -> Option<String> {
    if !node.label.is_empty() && !is_generic_resource_label(&node.label, &node.node_type) {
        return Some(node.label.clone());
    }
    for child in &node.children {
        if let Some(label) = first_concrete_label(child) {
            return Some(label);
        }
    }
    None
}

/// python resource_profiles._hcl_block_identity_from_children (raw parts, for LABELS).
fn hcl_block_identity_raw(node: &SemanticNode) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    if !node.label.is_empty() && !is_generic_resource_label(&node.label, &node.node_type) {
        if node.label.contains(' ') {
            return node
                .label
                .split_whitespace()
                .map(|s| s.to_string())
                .collect();
        }
        parts.push(node.label.clone());
    }
    for child in &node.children {
        if child.node_type.to_lowercase() == "body" {
            break;
        }
        if let Some(label) = first_concrete_label(child) {
            parts.push(label);
        }
    }
    parts
}

/// python resource_profiles._hcl_block_identity (normalized parts, for KEYS).
fn hcl_block_identity(node: &SemanticNode) -> Vec<String> {
    hcl_block_identity_raw(node)
        .into_iter()
        .map(|part| resource_normalize(&part))
        .filter(|part| !part.is_empty())
        .collect()
}

/// python resource_profiles._hcl_block_parent_path.
fn hcl_block_parent_path(id: &str, by_id: &HashMap<&str, &SemanticNode>) -> Vec<String> {
    let mut labels: Vec<String> = Vec::new();
    let mut current = id.to_string();
    while let Some((parent_id, _)) = current.rsplit_once('.') {
        let parent_id = parent_id.to_string();
        if let Some(node) = by_id.get(parent_id.as_str()).copied() {
            if node.node_type.to_lowercase() == "block" {
                let identity = hcl_block_identity(node);
                if !identity.is_empty() {
                    labels.push(identity.join("/"));
                }
            }
        }
        current = parent_id;
    }
    labels.reverse();
    labels
}

/// python resource_profiles._hcl_attribute_path.
fn hcl_attribute_path(node: &SemanticNode, by_id: &HashMap<&str, &SemanticNode>) -> Vec<String> {
    let Some(block) = nearest_ancestor_of_types(&node.id, by_id, &["block"]) else {
        return Vec::new();
    };
    let identity = hcl_block_identity(block);
    if identity.is_empty() {
        return Vec::new();
    }
    let mut path = hcl_block_parent_path(&block.id, by_id);
    path.push(identity.join("/"));
    path
}

/// python resource_profiles._hcl_key (block + attribute branches — the attribute_value / literal
/// branches are not needed by the hcl acceptance test).
fn hcl_key(node: &SemanticNode, by_id: &HashMap<&str, &SemanticNode>) -> Option<Vec<String>> {
    let nt = node.node_type.to_lowercase();
    if nt == "block" {
        let identity = hcl_block_identity(node);
        if identity.is_empty() {
            return None;
        }
        let mut key = vec!["hcl".to_string(), "block".to_string()];
        key.extend(hcl_block_parent_path(&node.id, by_id));
        key.extend(identity);
        return Some(key);
    }
    if nt == "attribute" && !is_generic_resource_label(&node.label, &node.node_type) {
        let mut key = vec!["hcl".to_string(), "attribute".to_string()];
        key.extend(hcl_attribute_path(node, by_id));
        key.push(resource_normalize(&node.label));
        return Some(key);
    }
    None
}

// ── Dockerfile resource-profile keying (issue #57) — mirrors python resource_profiles
// `_dockerfile_key`/`_docker_instruction_key`. Keys RUN/SHELL instructions by a shell-command
// IDENTITY (`_docker_shell_identity`), so an inserted `RUN apt-get update` is a clean ADDITION
// instead of positionally cross-pairing with an unrelated `RUN … compileall app` (which swallowed
// the real `app`→`src` edit under routing).

const DOCKER_INSTRUCTION_TYPES: &[&str] = &[
    "add_instruction", "arg_instruction", "cmd_instruction", "copy_instruction",
    "entrypoint_instruction", "env_instruction", "expose_instruction", "from_instruction",
    "healthcheck_instruction", "label_instruction", "onbuild_instruction", "run_instruction",
    "shell_instruction", "stopsignal_instruction", "user_instruction", "volume_instruction",
    "workdir_instruction",
];

fn is_docker_instruction_type(node_type: &str) -> bool {
    DOCKER_INSTRUCTION_TYPES.contains(&node_type.to_lowercase().as_str())
}

/// python `_normalize_docker_label`: collapse whitespace + lowercase (NO quote strip).
fn normalize_docker_label(label: &str) -> String {
    label.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// python `_docker_instruction_detail`: the shell_command/shell_fragment label under the instruction.
fn docker_instruction_detail(node: &SemanticNode) -> String {
    for candidate in std::iter::once(node).chain(node.descendants()) {
        if matches!(candidate.node_type.to_lowercase().as_str(), "shell_command" | "shell_fragment")
            && !candidate.label.is_empty()
            && !is_generic_resource_label(&candidate.label, &candidate.node_type)
        {
            return candidate.label.clone();
        }
    }
    String::new()
}

mod docker_resource;
use docker_resource::*;


mod statement_keys;
use statement_keys::*;


/// issue #57 bash: a MODIFICATION pairing two DIFFERENT-key statements is a re-merge of a pair
/// the profile deliberately unpaired (`:` -> `echo Hello`: different commands are a replacement,
/// not an edit — the issue-#33 lone del/add repair can't know that). Split it back into
/// DELETE+ADD. Must run right after refine, BEFORE finalize's parent/child suppression replaces
/// the keyed statement pair with its leaf word MODIFICATION.
fn split_cross_key_statement_modifications_drafts<'a>(
    changes: &mut Vec<ChangeDraft<'a>>,
    old_tree: &'a SemanticNode,
    new_tree: &'a SemanticNode,
    language: &str,
) {
    if !statement_profile_language(language) {
        return;
    }
    let old_by_id = semantic_node_refs_by_id_with_root(old_tree);
    let new_by_id = semantic_node_refs_by_id_with_root(new_tree);
    let mut split: Vec<ChangeDraft<'a>> = Vec::new();
    changes.retain(|change| {
        if change.change_type != "MODIFICATION" {
            return true;
        }
        let (Some(old_node), Some(new_node)) = (change.old_node, change.new_node) else {
            return true;
        };
        let old_key = statement_profile_key(old_node, &old_by_id, language);
        let new_key = statement_profile_key(new_node, &new_by_id, language);
        if old_key.is_none() || new_key.is_none() || old_key == new_key {
            return true;
        }
        split.push(ChangeDraft {
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
        split.push(ChangeDraft {
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
        false
    });
    if !split.is_empty() {
        changes.extend(split);
        // The split's DELETE/ADD are new suppression roots; the leaf drafts beneath them
        // (command_name/word churn) predate the split — fold them now.
        suppress_descendant_noise_drafts(changes);
    }
}

/// python statement_profiles.augment_statement_profile_changes (the move/reorder demotion): a
/// MOVE/REORDER of a statement whose profile key is UNCHANGED is only a positional shift (e.g. an
/// instruction pushed down by an insertion above it) — demote it to a MODIFICATION when the label
/// changed (an operand edit), or drop it entirely when the statement is byte-identical.
fn augment_statement_profile_changes_drafts<'a>(
    changes: &mut Vec<ChangeDraft<'a>>,
    old_tree: &'a SemanticNode,
    new_tree: &'a SemanticNode,
    language: &str,
) {
    if !statement_profile_language(language) {
        return;
    }
    let old_by_id = semantic_node_refs_by_id_with_root(old_tree);
    let new_by_id = semantic_node_refs_by_id_with_root(new_tree);
    // Node pairs that already carry a real change — a reorder over one of these is redundant
    // scaffolding (the edit is already surfaced), so drop it rather than duplicate it.
    let already_changed: HashSet<(String, String)> = changes
        .iter()
        .filter(|c| !matches!(c.change_type, "MOVE" | "REORDER"))
        .filter_map(|c| Some((c.old_node?.id.clone(), c.new_node?.id.clone())))
        .collect();
    changes.retain_mut(|change| {
        if !matches!(change.change_type, "MOVE" | "REORDER") {
            return true;
        }
        let (Some(old_node), Some(new_node)) = (change.old_node, change.new_node) else {
            return true;
        };
        let old_key = statement_profile_key(old_node, &old_by_id, language);
        if old_key.is_none()
            || old_key != statement_profile_key(new_node, &new_by_id, language)
        {
            return true;
        }
        // A same-key MOVE/REORDER already covered by another change, or byte-identical, is a
        // pure positional shift — drop it. Only an uncovered label change becomes a MODIFICATION.
        if old_node.label != new_node.label
            && !already_changed.contains(&(old_node.id.clone(), new_node.id.clone()))
        {
            change.change_type = "MODIFICATION";
            change.confidence = change.confidence.min(0.88);
            change.description = format!(
                "Update {}({:?}) -> {}({:?})",
                old_node.node_type, old_node.label, new_node.node_type, new_node.label
            );
            true
        } else {
            false
        }
    });
}

// ── Entity-anchored matching augmentation (issue #57) — mirrors python anchors.py
// `augment_entity_matching`/`recover_entity_pairs`. After the tree diff, the DEFAULT path
// re-pairs same-identity entities (functions/classes keyed by kind + enclosing-entity path +
// label) and their stable descendants, so relocated content becomes MOVEs instead of DELETE+ADD
// churn. The routed finalize lacked this entirely — the resolved "Phase 1 contradiction".
// Scope: the core anchoring; the JS function-valued-declaration / clojure / elixir derived-kind
// profiles are NOT ported (their languages either aren't routed or are green without them).

const ANCHOR_ENTITY_TYPES: &[&str] = &[
    "class_definition", "class_declaration", "class_statement", "constructor_declaration",
    "constructor_signature", "declproc", "defproc", "enum_declaration", "enum_statement",
    "extension_declaration", "extension_type_declaration", "function_body_declaration",
    "function_definition", "function_declaration", "function_heading", "function_item",
    "function_signature", "function_statement", "getter_signature", "form", "function_module",
    "interface", "interface_declaration", "interface_definition", "class_impl",
    "method_definition", "method_declaration", "method", "method_signature", "method_statement",
    "mixin_declaration", "module", "object_type", "operation_declaration", "procedure_definition",
    "procedure_declaration", "procedure_heading", "record_definition", "setter_signature",
    "source_method_declaration", "struct_declaration", "sub_declaration",
    "subroutine_declaration_statement", "trait_item",
    // js-ts async variants: an async function is still the same kind of entity as its
    // non-async counterpart, so it must keep anchoring (moves stay MOVEs, not DELETE+ADD).
    "async_function_declaration", "async_method_definition",
];

const ANCHOR_FUNCTION_TYPES: &[&str] = &[
    "constructor_declaration", "constructor_signature", "declproc", "defproc",
    "function_body_declaration", "function_definition", "function_declaration",
    "function_heading", "function_item", "function_signature", "function_statement",
    "getter_signature", "form", "function_module", "method_definition", "method_declaration",
    "method", "method_signature", "method_statement", "module", "operation_declaration",
    "procedure_definition", "procedure_declaration", "procedure_heading", "setter_signature",
    "source_method_declaration", "sub_declaration", "subroutine_declaration_statement",
    "async_function_declaration", "async_method_definition",
];

const ANCHOR_ROOT_ENTITY_TYPES: &[&str] = &[
    "compilation_unit", "module", "program", "source_file", "translation_unit",
];

const ANCHOR_NAME_TYPES: &[&str] = &[
    "identifier", "name", "symbol", "simple_identifier", "function_name",
    "function_parameter_declaration", "method_name", "class_name", "type_name", "variable",
    "variable_name",
];


mod entity_anchors;
use entity_anchors::*;



mod html_path;
use html_path::*;


mod xml_schema;
use xml_schema::*;


/// python path_profiles.augment_path_profile_matching (html): drop cross-key positional matches,
/// pair unmatched keyed nodes by identical key. Also serves the XML Maven-POM schema profile
/// (issue #63) with coordinate keys instead of element paths.
fn augment_html_path_matching<'a>(
    old_tree: &'a SemanticNode,
    new_tree: &'a SemanticNode,
    matching: Vec<MatchPair<'a>>,
    language: &str,
) -> Vec<MatchPair<'a>> {
    let key_fn: Box<dyn Fn(&SemanticNode, &HashMap<&str, &SemanticNode>) -> Option<Vec<String>>> =
        match language {
            "html" => Box::new(html_path_key),
            "xml" if xml_tree_is_pom(old_tree) || xml_tree_is_pom(new_tree) => {
                Box::new(xml_pom_key)
            }
            "xml" if xml_tree_is_msbuild(old_tree) || xml_tree_is_msbuild(new_tree) => {
                Box::new(xml_msbuild_key)
            }
            // User-registered dialects (issue #86): interpreted through the same
            // xml_schema_key body; bundled dialects above outrank them.
            "xml" => match matching_user_xml_dialect(old_tree, new_tree) {
                Some(dialect) => Box::new(move |node, by_id| {
                    xml_schema_key(node, by_id, &|n| user_dialect_coordinate_key(&dialect, n))
                }),
                None => return matching,
            },
            _ => return matching,
        };
    let old_by_id = semantic_node_refs_by_id_with_root(old_tree);
    let new_by_id = semantic_node_refs_by_id_with_root(new_tree);
    let old_keys: HashMap<&str, Vec<String>> = old_by_id
        .iter()
        .filter_map(|(id, node)| key_fn(node, &old_by_id).map(|k| (*id, k)))
        .collect();
    let new_keys: HashMap<&str, Vec<String>> = new_by_id
        .iter()
        .filter_map(|(id, node)| key_fn(node, &new_by_id).map(|k| (*id, k)))
        .collect();
    let mut result: Vec<MatchPair<'a>> = Vec::new();
    let mut matched_old: HashSet<String> = HashSet::new();
    let mut matched_new: HashSet<String> = HashSet::new();
    for pair in matching {
        let ok = old_keys.get(pair.old_node.id.as_str());
        let nk = new_keys.get(pair.new_node.id.as_str());
        // python _allow_stable_same_label_match: a RELOCATED element with identical content
        // (same type+label+structural hash, non-generic) keeps its gumtree match even though
        // its element PATH changed (the playground's h1 gaining a `header` ancestor) — path
        // keys discriminate positions, not survival.
        let stable_same_label = pair.old_node.node_type == pair.new_node.node_type
            && pair.old_node.label == pair.new_node.label
            && pair.old_node.structural_hash == pair.new_node.structural_hash
            && !pair.old_node.label.is_empty()
            && pair.old_node.label != pair.old_node.node_type;
        if (ok.is_some() || nk.is_some())
            && (ok.is_none() || nk.is_none() || (ok != nk && !stable_same_label))
        {
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

// ── Keyed-data profile keying (issue #57 json/yaml) — mirrors python keyed_profiles' json/yaml
// half. Pairs key by their ANCESTOR KEY PATH + own key, array/sequence items by a label identity,
// and pair keys/values by role — so an inserted array element doesn't positionally cross-pair
// later unchanged values into bogus MODIFICATIONs, and a key reorder pairs by identity. The
// adf/databricks/dbt keys are NOT ported: those languages are already routed-green without them.

mod keyed_data;
use keyed_data::*;


mod sql_profile;
use sql_profile::*;

/// Per-stage finalize routing (issue #57): run the SAME refine + finalize pipeline the
/// certified batch uses, from caller-provided semantic trees. This is the retirement path
/// for the python transitional refinement/presentation layer — languages route here (env/
/// allowlist-gated python-side) and their python mirrors get DELETED, not translated.
fn finalize_review_impl(
    old_tree_json: &str,
    new_tree_json: &str,
    old_source: &str,
    new_source: &str,
    language: &str,
    config_json: &str,
) -> Result<String, String> {
    let config = RustCoreConfig::from_json(config_json);
    let mut old_tree: SemanticNode =
        serde_json::from_str(old_tree_json).map_err(|exc| format!("old tree: {exc}"))?;
    let mut new_tree: SemanticNode =
        serde_json::from_str(new_tree_json).map_err(|exc| format!("new tree: {exc}"))?;
    validate_unique_ids(&old_tree).map_err(|exc| format!("old tree: {exc}"))?;
    validate_unique_ids(&new_tree).map_err(|exc| format!("new tree: {exc}"))?;

    let old_count = 1 + old_tree.descendants().len();
    let new_count = 1 + new_tree.descendants().len();
    if old_count > config.max_nodes || new_count > config.max_nodes {
        return Ok(json!({"used": false, "reason": "tree_too_large"}).to_string());
    }

    // File-lifecycle degenerate case (issue #57 payoff, empty-tree tier): an empty root on
    // one side is a file add/delete, not a tree edit. The gumtree matcher would pair the two
    // roots structurally and emit a bogus root MODIFICATION; the contract shape downstream
    // lifecycle handling expects is the python-parity DELETION(old root) + ADDITION(new root)
    // pair, which _apply_file_lifecycle_to_diff then interprets via file_lifecycle metadata.
    if old_tree.children.is_empty() != new_tree.children.is_empty() {
        let drafts = vec![
            ChangeDraft {
                change_type: "DELETION",
                old_node: Some(&old_tree),
                new_node: None,
                old_index: None,
                new_index: None,
                confidence: 1.0,
                description: format!("Delete {}({:?})", old_tree.node_type, old_tree.label),
                refactoring_kind: None,
                text_diff: None,
            },
            ChangeDraft {
                change_type: "ADDITION",
                old_node: None,
                new_node: Some(&new_tree),
                old_index: None,
                new_index: None,
                confidence: 1.0,
                description: format!(
                    "Insert -> {}({:?})",
                    new_tree.node_type, new_tree.label
                ),
                refactoring_kind: None,
                text_diff: None,
            },
        ];
        let serialized = serialize_change_drafts(&drafts);
        return Ok(json!({
            "used": true,
            "engine": "rust_finalize_review_v1",
            "language": language,
            "changes": serialized.changes,
            "change_groups": [],
            "is_style_only": false,
            "no_surviving_changes": false,
            "ignored_style_changes": [],
        })
        .to_string());
    }

    // Resource-profile label enrichment (issue #39): fill resource/parameter identity labels
    // from semantic children before matching so keys + review labels are stable. Only changes
    // labels (not ids/structural_hash). No-op for non-resource languages.
    if resource_profile_language(language) {
        enrich_resource_profile_labels(&mut old_tree, language);
        enrich_resource_profile_labels(&mut new_tree, language);
    }
    // Cross-language NodeFacts (issue #70): fill facts for function entities lacking them.
    enrich_tree_facts(&mut old_tree);
    enrich_tree_facts(&mut new_tree);

    if config.collect_trace {
        finalize_trace_start();
    }
    let matching = compute_matching(
        &old_tree,
        &new_tree,
        config.min_height,
        config.min_similarity,
    );
    // Entity-anchored matching (issue #57, anchors.py port): re-pair same-identity entities and
    // their stable descendants BEFORE the edit script — relocated content becomes MOVEs instead
    // of DELETE+ADD churn (mirrors the default path's differ.py stage ordering: entity →
    // resource → statement). No-op for non-code-like languages.
    let matching = augment_entity_matching(&old_tree, &new_tree, matching, language);
    // Keyed-data matching (issue #57 json/yaml): pairs/items pair by key path so an inserted
    // array element doesn't positionally cross-pair later unchanged values (differ.py order:
    // entity -> keyed -> resource -> statement -> query).
    let matching = augment_keyed_data_matching(&old_tree, &new_tree, matching, language);
    // HTML path-profile matching (issue #57/#64): elements key by their path with identity
    // attributes beating same-tag ordinals, so edits inside id-bearing elements survive inserts.
    let matching = augment_html_path_matching(&old_tree, &new_tree, matching, language);
    // Resource-profile matching (issue #39): re-pair keyed resource/attribute nodes by
    // identity before the edit script, so a puppet attribute value isn't cross-paired with
    // an unrelated class-parameter default. No-op for non-resource languages.
    let matching = augment_resource_profile_matching(&old_tree, &new_tree, matching, language);
    // Statement-profile matching (issue #57 asm/bash/delphi): re-pair keyed statements by identity
    // so an operand-value edit is a MODIFICATION, not DELETE+ADD. No-op for other languages.
    let matching = augment_statement_profile_matching(&old_tree, &new_tree, matching, language);
    // Query-profile matching (issue #57 sql): clauses/relations/fields pair by role + normalized
    // identity within their statement, so an added JOIN doesn't shift FROM into a bogus MOVE.
    let matching = augment_sql_query_matching(&old_tree, &new_tree, matching, language);
    let script =
        generate_edit_script_with_diagnostics(&old_tree, &new_tree, &matching, None);
    let mut drafts: Vec<ChangeDraft> = script.ops.into_iter().map(edit_op_to_draft).collect();
    // Snapshot the raw drafts so js/ts style suppression residue can be relabelled as
    // IGNORED_STYLE evidence after the passes run (python _style_groups_from_suppression).
    let draft_snapshot: Vec<(String, Option<String>, Option<String>, Vec<String>, Vec<String>)> =
        if matches!(language, "javascript" | "typescript" | "tsx") {
            drafts
                .iter()
                .map(|c| {
                    (
                        c.change_type.to_string(),
                        c.old_node.map(|n| n.id.clone()),
                        c.new_node.map(|n| n.id.clone()),
                        node_labels(c.old_node),
                        node_labels(c.new_node),
                    )
                })
                .collect()
        } else {
            Vec::new()
        };
    refine_candidate_drafts(&mut drafts, &matching, None, language);
    // Statement-profile cross-key split (issue #57 bash): must run BEFORE finalize's parent/child
    // suppression turns the keyed statement pair into a leaf word MODIFICATION.
    split_cross_key_statement_modifications_drafts(&mut drafts, &old_tree, &new_tree, language);
    let mut finalization = PythonReviewFinalization::default();
    finalize_python_review_drafts(
        &mut drafts,
        &old_tree,
        &new_tree,
        old_source,
        new_source,
        &mut finalization,
        language,
    );

    // Language-gated presentation passes, mirroring python `presentation.py`'s
    // per-language dispatch (`elif language == "haskell"`). Kept at the finalize-review
    // level (like formatting_equivalence_group_drafts) so the core finalize stays
    // language-agnostic.
    if language == "haskell" {
        if let Some(group) = suppress_haskell_signature_function_sibling_churn_drafts(&mut drafts) {
            finalization.change_groups.push(group);
        }
    } else if language == "dart" {
        if let Some(group) = suppress_dart_signature_body_scaffold_churn_drafts(&mut drafts) {
            finalization.change_groups.push(group);
        }
    }

    // Resource-profile change augmentation (issue #39): promote same-key attribute value
    // changes to attribute-level MODIFICATIONs and surface keyed review-container add/delete.
    // No-op for non-resource languages.
    augment_resource_profile_changes_drafts(&mut drafts, &old_tree, &new_tree, language);
    // Statement-profile change augmentation (issue #57 asm/bash/delphi): demote a same-key
    // MOVE/REORDER (a stable-identity statement merely shifted by an insertion) to a MODIFICATION
    // or drop it. No-op for other languages.
    augment_statement_profile_changes_drafts(&mut drafts, &old_tree, &new_tree, language);
    // Statement-profile scaffold suppression (issue #57 bash/delphi): fold sub-token churn under a
    // MODIFIED review container into the one MODIFICATION (the general descendant-noise pass only
    // roots on ADD/DELETE/MOVE, not a MODIFICATION). No-op for other languages.
    suppress_statement_container_descendant_noise_drafts(&mut drafts, language);
    // Query-profile change augmentation (issue #57 sql): demote same-key clause MOVEs (positional
    // displacement from an inserted JOIN) and recover keyed clause/field add/deletes.
    augment_sql_query_changes_drafts(&mut drafts, &old_tree, &new_tree, language);
    // Keyed-data change augmentation (issue #57 json/yaml): recover keyed pair/item ADD/DELETEs
    // hidden by coarse container matches; identical-content positional-label churn suppresses
    // with evidence.
    if let Some(group) =
        augment_keyed_data_changes_drafts(&mut drafts, &old_tree, &new_tree, language)
    {
        finalization.change_groups.push(group);
    }
    // YAML representation-graph equivalence (issue #57 yaml): block↔flow style churn suppresses
    // with evidence; a diff fully consumed by it is style-only.
    if language == "yaml" {
        if let Some(group) = suppress_yaml_representation_equivalent_drafts(&mut drafts) {
            finalization.change_groups.push(group);
            if drafts.is_empty() {
                finalization.is_style_only = true;
            }
        }
    }
    // js/ts style rule (issue #57 javascript): relabel suppression residue as IGNORED_STYLE.
    if let Some((group, ignored)) =
        js_style_group_from_suppression(&draft_snapshot, &drafts, language)
    {
        finalization.change_groups.push(group);
        finalization.ignored_style_changes.push(ignored);
    }

    // The issue #51 style discriminator, mirrored from the batch finalization: an empty
    // result on differing sources is style-only ONLY under (normalized) tree equality.
    if drafts.is_empty() && old_source != new_source {
        let style_equivalent = old_tree.structural_hash == new_tree.structural_hash
            || whitespace_normalized_tree_hash(&old_tree)
                == whitespace_normalized_tree_hash(&new_tree);
        if style_equivalent {
            finalization.is_style_only = true;
        } else {
            finalization.is_style_only = false;
            finalization.no_surviving_changes = true;
        }
    }

    let serialized = serialize_change_drafts(&drafts);
    let mut ignored_style_changes = finalization.ignored_style_changes;
    let mut change_groups = finalization.change_groups;
    change_groups.extend(final_change_groups_from_drafts(&drafts));
    change_groups.extend(final_meaningful_groups_from_drafts(&drafts));
    // Entity-anchored grouping (issue #57 abap): "GREET changed" for child modifications
    // under a same-identity entity whose content changed.
    change_groups.extend(entity_child_content_groups(&drafts, &old_tree, &new_tree));
    // Changed-in-place entity surfacing (issue #57 graphql): a matched named entity whose body
    // changed carries its label into a group even though only descendants appear as changes.
    change_groups.extend(surface_changed_in_place_entity_groups(&drafts, &matching));
    if let Some((group, ignored)) = formatting_equivalence_group_drafts(&drafts, language) {
        change_groups.push(group);
        ignored_style_changes.push(ignored);
    }
    let trace: Vec<Value> = if config.collect_trace {
        finalize_trace_take()
            .into_iter()
            .map(|(pass, count)| json!({"pass": pass, "changes_after": count}))
            .collect()
    } else {
        Vec::new()
    };
    Ok(json!({
        "used": true,
        "engine": "rust_finalize_review_v1",
        "language": language,
        "changes": serialized.changes,
        "change_groups": change_groups,
        "is_style_only": finalization.is_style_only,
        "no_surviving_changes": finalization.no_surviving_changes,
        "ignored_style_changes": ignored_style_changes,
        "trace": trace,
    })
    .to_string())
}

/// python statement_profiles._synthetic_hash: sha256(node_type \0 label { \0 child_hash }*).
/// Recomputed bottom-up after label enrichment so hash-based subtree matching sees the
/// enriched identity (python parity — the shell enrichment recomputed hashes too).
fn synthetic_structural_hash(node_type: &str, label: &str, children: &[SemanticNode]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(node_type.as_bytes());
    hasher.update(b"\0");
    hasher.update(label.as_bytes());
    for child in children {
        hasher.update(b"\0");
        hasher.update(child.structural_hash.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

/// python statement_profiles._source_slice + _compact_source: the node's source span,
/// whitespace-collapsed. asm/delphi parsers emit 1-based lines (python parity).
fn profile_source_snippet(source_lines: &[&str], node: &SemanticNode) -> String {
    if source_lines.is_empty() {
        return String::new();
    }
    let pos = &node.position;
    // All parsers emit 0-based rows (issue #52); the asm/delphi one_based
    // compensation that lived here died with their 1-based emission.
    let start_line = pos.start_line as i64;
    let end_line = pos.end_line as i64;
    if start_line < 0 || start_line as usize >= source_lines.len() {
        return String::new();
    }
    let start_line = start_line as usize;
    let end_line = (end_line.max(start_line as i64) as usize).min(source_lines.len() - 1);
    let clip = |line: &str, from: Option<usize>, to: Option<usize>| -> String {
        let chars: Vec<char> = line.chars().collect();
        let from = from.unwrap_or(0).min(chars.len());
        let to = to.unwrap_or(chars.len()).min(chars.len());
        if from >= to {
            return String::new();
        }
        chars[from..to].iter().collect()
    };
    let raw = if start_line == end_line {
        clip(
            source_lines[start_line],
            Some(pos.start_col as usize),
            Some(pos.end_col as usize),
        )
    } else {
        let mut parts = vec![clip(source_lines[start_line], Some(pos.start_col as usize), None)];
        for line in &source_lines[start_line + 1..end_line] {
            parts.push((*line).to_string());
        }
        parts.push(clip(source_lines[end_line], None, Some(pos.end_col as usize)));
        parts.join("\n")
    };
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// python statement_profiles._bash_command_label_from_children.
fn bash_command_label_from_children(children: &[SemanticNode]) -> String {
    for child in children {
        if child.node_type.eq_ignore_ascii_case("command_name") && !child.label.is_empty() {
            return child.label.clone();
        }
        for descendant in child.descendants() {
            if descendant.node_type.eq_ignore_ascii_case("command_name")
                && !descendant.label.is_empty()
            {
                return descendant.label.clone();
            }
        }
    }
    String::new()
}

mod label_enrichment;
use label_enrichment::*;
/// Shell-facing profile-label enrichment (issue #57 profile-enrichment port): the ONE
/// seam differ.py calls per tree before matching/guardrails. Families dispatch by
/// language; unported families return the tree unchanged (the shell keeps its python
/// enrichment for those until their port lands).
/// Register user XML dialects (issue #86): the declarative coordinate specs
/// marshaled from the python descriptor registry. Replaces the whole set
/// (process-level; match-predicated per dialect). Fails closed on bad JSON.
fn enrich_profile_labels_impl(
    tree_json: &str,
    source: &str,
    language: &str,
    identity_fields: Option<Vec<String>>,
) -> Result<String, String> {
    let tree: SemanticNode =
        serde_json::from_str(tree_json).map_err(|exc| format!("tree: {exc}"))?;
    let language = language.to_lowercase();
    let enriched = if matches!(language.as_str(), "asm" | "bash" | "delphi") {
        let source_lines: Vec<&str> = source.lines().collect();
        enrich_statement_profile_labels_node(&tree, &source_lines, &language)
    } else if matches!(language.as_str(), "css" | "scss" | "html" | "xml" | "mdx") {
        let source_lines: Vec<&str> = source.lines().collect();
        enrich_path_profile_labels_node(&tree, &source_lines, &language)
    } else if matches!(language.as_str(), "json" | "yaml") {
        // python keyed identity set: id/key/name + normalized schema identity fields.
        let mut identity_keys: HashSet<String> =
            ["id", "key", "name"].iter().map(|s| s.to_string()).collect();
        for field in identity_fields.unwrap_or_default() {
            identity_keys.insert(normalize_keyed_identity(&field));
        }
        enrich_keyed_data_labels_node(&tree, &language, &identity_keys).0
    } else if language == "sql" {
        let source_lines: Vec<&str> = source.lines().collect();
        enrich_query_profile_labels_node(&tree, &source_lines).0
    } else if matches!(language.as_str(), "hcl" | "puppet") {
        // Resource family (issue #90) — hcl block + puppet resource/parameter
        // identity from children; no source_lines needed.
        enrich_resource_profile_labels_node(&tree, &language)
    } else {
        tree
    };
    serde_json::to_string(&enriched).map_err(|exc| format!("serialize: {exc}"))
}

// ── Guardrail semantic paths (issue #57 follow-up: the guardrail-keying port) ──
// python analysis/guardrails.py::_semantic_paths + the _path_from_* formatters. The
// keying itself (keyed_data_key / resource_profile_key) already lives here from the
// keyed/resource profile ports; this exposes the per-node semantic-path index the
// guardrail engine matches protected-rule paths against.

/// python guardrails._clean_path_part.
fn guardrail_clean_path_part(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_lowercase()
}

fn guardrail_join(parts: &[String]) -> String {
    parts
        .iter()
        .filter(|part| !part.is_empty())
        .map(|part| guardrail_clean_path_part(part))
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(".")
}

/// python guardrails._path_from_profile_key (+ per-language formatters).
fn guardrail_path_from_profile_key(key: &[String]) -> String {
    if key.is_empty() {
        return String::new();
    }
    let language = key[0].as_str();
    match language {
        "json" | "yaml" => {
            if key.len() < 3 {
                return String::new();
            }
            let parts: &[String] = match key[1].as_str() {
                "pair" | "array_item" => &key[2..],
                "key" | "value" => &key[2..key.len() - 1],
                _ => &key[2..],
            };
            guardrail_join(parts)
        }
        "adf" | "databricks" | "databricks-workflow" | "dbt-config" | "dbt-packages"
        | "dbt-yaml" => guardrail_join(&key[1..]),
        "hcl" => {
            if key.len() < 3 {
                return String::new();
            }
            match key[1].as_str() {
                "attribute" | "block" => guardrail_join(&key[2..]),
                "attribute_value" => guardrail_join(&key[2..key.len().saturating_sub(2)]),
                _ => guardrail_join(&key[1..]),
            }
        }
        "dockerfile" => {
            if key.len() < 4 || key[1] != "instruction" {
                return guardrail_join(&key[1..]);
            }
            let tail = guardrail_join(&key[2..]);
            if tail.is_empty() {
                "instruction".to_string()
            } else {
                format!("instruction.{tail}")
            }
        }
        "puppet" => guardrail_join(&key[1..]),
        _ => String::new(),
    }
}

/// python guardrails._semantic_paths: per-node semantic paths from the profile keys —
/// a keyed node's path applies to the node AND every descendant (a protected value
/// inside a keyed container is protected wherever the edit lands).
///
/// Deliberate deviation: python emits the SAME path repeatedly when a pair and its
/// key/value child both key to it (list-wise duplicates); rule matching is membership
/// (`path in paths`), so duplicates are unobservable. Set-wise parity is exact —
/// verified across the full guardrail suite (0 mismatches).
fn guardrail_semantic_paths(
    root: &SemanticNode,
    language: &str,
) -> HashMap<String, Vec<String>> {
    let keyed = matches!(
        language,
        "adf" | "databricks" | "databricks-workflow" | "dbt-config" | "dbt-packages"
            | "dbt-yaml" | "json" | "yaml"
    );
    let resource = matches!(language, "dockerfile" | "hcl" | "puppet");
    if !keyed && !resource {
        return HashMap::new();
    }
    let by_id = semantic_node_refs_by_id_with_root(root);
    let mut result: HashMap<String, Vec<String>> = HashMap::new();
    for node in std::iter::once(root).chain(root.descendants()) {
        let key = if keyed {
            keyed_data_key(node, &by_id, language)
        } else {
            resource_profile_key(node, &by_id, language)
        };
        let Some(key) = key else { continue };
        let path = guardrail_path_from_profile_key(&key);
        if path.is_empty() {
            continue;
        }
        result.entry(node.id.clone()).or_default().push(path.clone());
        for descendant in node.descendants() {
            result
                .entry(descendant.id.clone())
                .or_default()
                .push(path.clone());
        }
    }
    result
}

/// Shell-facing guardrail semantic-path index (issue #57 follow-up): returns
/// {node_id: [semantic paths]} for guardrail rule matching. Empty object for
/// non-guardrail languages.
// ── Guardrail rule evaluation (#91 A1.3b: python guardrails._evaluate_policy_rules) ──
// The path derivation above (`guardrail_semantic_paths`) is reused here; this adds the
// rule filtering + membership matching + violation construction so the whole guardrail
// engine is Rust-authoritative and python keeps only YAML policy parsing + marshalling.

#[derive(Deserialize)]
struct GuardrailRuleInput {
    rule_id: String,
    severity: String,
    language: String,
    path: String,
    message: String,
    #[serde(default)]
    files: Vec<String>,
}

#[derive(Deserialize)]
struct GuardrailChangeInput {
    #[serde(default)]
    old_node_id: Option<String>,
    #[serde(default)]
    new_node_id: Option<String>,
}

#[derive(Deserialize)]
struct GuardrailEvalRequest {
    #[serde(default)]
    language: String,
    #[serde(default)]
    old_filename: String,
    #[serde(default)]
    new_filename: String,
    #[serde(default)]
    old_tree: Option<SemanticNode>,
    #[serde(default)]
    new_tree: Option<SemanticNode>,
    #[serde(default)]
    changes: Vec<GuardrailChangeInput>,
    #[serde(default)]
    rules: Vec<GuardrailRuleInput>,
}

#[derive(Serialize)]
struct GuardrailViolationOutput {
    rule_id: String,
    severity: String,
    file: String,
    language: String,
    semantic_path: String,
    node_type: String,
    old_node_id: Option<String>,
    new_node_id: Option<String>,
    position: Option<NodePosition>,
    old_value: String,
    new_value: String,
    message: String,
}

/// python guardrails._normalise_path: strip, drop leading/trailing dots, clean each part.
fn guardrail_normalise_path(value: &str) -> String {
    value
        .trim()
        .trim_matches('.')
        .split('.')
        .filter(|part| !part.is_empty())
        .map(guardrail_clean_path_part)
        .collect::<Vec<_>>()
        .join(".")
}

/// python guardrails._node_value_summary: up to 4 distinct descendant labels that differ
/// from the node's own label; falls back to the node label.
fn guardrail_node_value_summary(node: Option<&SemanticNode>) -> String {
    let Some(node) = node else {
        return String::new();
    };
    let mut labels: Vec<String> = Vec::new();
    for candidate in std::iter::once(node).chain(node.descendants()) {
        let label = candidate.label.trim();
        if label.is_empty() || label == node.label {
            continue;
        }
        if !labels.iter().any(|existing| existing == label) {
            labels.push(label.to_string());
        }
        if labels.len() >= 4 {
            break;
        }
    }
    if labels.is_empty() {
        node.label.clone()
    } else {
        labels.join(", ")
    }
}

/// fnmatch-style glob match (case-sensitive: fnmatchcase semantics). Supports
/// `*`, `?`, and `[...]` character classes (with a leading `!` negation).
fn guardrail_glob_match(name: &str, pattern: &str) -> bool {
    let name: Vec<char> = name.chars().collect();
    let pat: Vec<char> = pattern.chars().collect();
    // Iterative backtracking: `star` remembers the last '*' to retry a longer match.
    let (mut ni, mut pi) = (0usize, 0usize);
    let (mut star_pi, mut star_ni): (Option<usize>, usize) = (None, 0);
    while ni < name.len() {
        if pi < pat.len() {
            match pat[pi] {
                '*' => {
                    star_pi = Some(pi);
                    star_ni = ni;
                    pi += 1;
                    continue;
                }
                '?' => {
                    ni += 1;
                    pi += 1;
                    continue;
                }
                '[' => {
                    if let Some((matched, next_pi)) = guardrail_glob_class(&pat, pi, name[ni]) {
                        if matched {
                            ni += 1;
                            pi = next_pi;
                            continue;
                        }
                    } else if pat[pi] == name[ni] {
                        // Unterminated class → literal '['.
                        ni += 1;
                        pi += 1;
                        continue;
                    }
                }
                c => {
                    if c == name[ni] {
                        ni += 1;
                        pi += 1;
                        continue;
                    }
                }
            }
        }
        // Mismatch: backtrack to the last '*' if any.
        if let Some(sp) = star_pi {
            pi = sp + 1;
            star_ni += 1;
            ni = star_ni;
        } else {
            return false;
        }
    }
    while pi < pat.len() && pat[pi] == '*' {
        pi += 1;
    }
    pi == pat.len()
}

/// Match a `[...]` class at `pat[start]` against `ch`. Returns (matched, index-after-`]`),
/// or None when the class is unterminated (caller treats `[` as a literal).
fn guardrail_glob_class(pat: &[char], start: usize, ch: char) -> Option<(bool, usize)> {
    let mut i = start + 1;
    let negate = i < pat.len() && (pat[i] == '!' || pat[i] == '^');
    if negate {
        i += 1;
    }
    let class_start = i;
    let mut matched = false;
    while i < pat.len() {
        if pat[i] == ']' && i > class_start {
            return Some((matched != negate, i + 1));
        }
        // Range a-z (not when '-' is first/last in the class).
        if i + 2 < pat.len() && pat[i + 1] == '-' && pat[i + 2] != ']' {
            if pat[i] <= ch && ch <= pat[i + 2] {
                matched = true;
            }
            i += 3;
        } else {
            if pat[i] == ch {
                matched = true;
            }
            i += 1;
        }
    }
    None
}

/// python guardrails._file_matches.
fn guardrail_file_matches(rule: &GuardrailRuleInput, old_filename: &str, new_filename: &str) -> bool {
    if rule.files.is_empty() {
        return true;
    }
    let filenames = [old_filename, new_filename];
    rule.files
        .iter()
        .any(|pattern| filenames.iter().any(|name| guardrail_glob_match(name, pattern)))
}

/// python guardrails._evaluate_policy_rules: match a parsed policy's protected paths
/// against the diff's changes and construct the violations.
fn evaluate_guardrail_rules(request: &GuardrailEvalRequest) -> Vec<GuardrailViolationOutput> {
    let language = request.language.to_lowercase();
    let file = if !request.new_filename.is_empty() {
        request.new_filename.clone()
    } else {
        request.old_filename.clone()
    };

    let relevant: Vec<&GuardrailRuleInput> = request
        .rules
        .iter()
        .filter(|rule| {
            rule.language == language
                && guardrail_file_matches(rule, &request.old_filename, &request.new_filename)
        })
        .collect();
    if relevant.is_empty() {
        return Vec::new();
    }
    let (Some(old_tree), Some(new_tree)) = (&request.old_tree, &request.new_tree) else {
        return Vec::new();
    };

    let old_paths = guardrail_semantic_paths(old_tree, &language);
    let new_paths = guardrail_semantic_paths(new_tree, &language);
    let old_by_id = semantic_node_refs_by_id_with_root(old_tree);
    let new_by_id = semantic_node_refs_by_id_with_root(new_tree);

    let mut seen: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    let mut violations: Vec<GuardrailViolationOutput> = Vec::new();

    for change in &request.changes {
        let old_node = change
            .old_node_id
            .as_deref()
            .and_then(|id| old_by_id.get(id).copied());
        let new_node = change
            .new_node_id
            .as_deref()
            .and_then(|id| new_by_id.get(id).copied());

        // (path, ...) candidates: an old_node's path index + a new_node's path index.
        let mut candidates: Vec<&String> = Vec::new();
        if let Some(node) = old_node {
            if let Some(paths) = old_paths.get(&node.id) {
                candidates.extend(paths.iter());
            }
        }
        if let Some(node) = new_node {
            if let Some(paths) = new_paths.get(&node.id) {
                candidates.extend(paths.iter());
            }
        }

        for path in candidates {
            let normalised = guardrail_normalise_path(path);
            for rule in &relevant {
                if normalised != rule.path {
                    continue;
                }
                let key = (rule.rule_id.clone(), normalised.clone());
                if !seen.insert(key) {
                    continue;
                }
                let changed_node = new_node.or(old_node);
                violations.push(GuardrailViolationOutput {
                    rule_id: rule.rule_id.clone(),
                    severity: rule.severity.clone(),
                    file: file.clone(),
                    language: language.clone(),
                    semantic_path: normalised.clone(),
                    node_type: changed_node.map(|n| n.node_type.clone()).unwrap_or_default(),
                    old_node_id: old_node.map(|n| n.id.clone()),
                    new_node_id: new_node.map(|n| n.id.clone()),
                    position: changed_node.map(|n| n.position.clone()),
                    old_value: guardrail_node_value_summary(old_node),
                    new_value: guardrail_node_value_summary(new_node),
                    message: rule.message.clone(),
                });
            }
        }
    }
    violations
}

/// Shell-facing guardrail rule evaluation (#91 A1.3b): given the diff's changes (as
/// old/new node ids), the old/new trees, and the parsed policy rules, returns the
/// protected-path violations. Python keeps only the YAML policy parse + marshalling.
#[cfg(test)]
mod guardrail_eval_tests {
    use super::*;
    use serde_json::{json, Value};

    fn tnode(id: &str, node_type: &str, label: &str, children: Value) -> Value {
        json!({
            "id": id,
            "node_type": node_type,
            "label": label,
            "position": {"start_line": 0, "start_col": 0, "end_line": 0, "end_col": 1},
            "structural_hash": format!("h-{id}"),
            "children": children,
        })
    }

    #[test]
    fn normalise_path_cleans_and_trims() {
        assert_eq!(guardrail_normalise_path("  Server.HOST.  "), "server.host");
        assert_eq!(guardrail_normalise_path(".a..b."), "a.b");
        assert_eq!(guardrail_normalise_path("\"Quoted\".Part"), "quoted.part");
        assert_eq!(guardrail_normalise_path(""), "");
    }

    #[test]
    fn node_value_summary_distinct_labels_capped_at_four() {
        let node: SemanticNode = serde_json::from_value(tnode(
            "0",
            "block",
            "root",
            json!([
                tnode("1", "leaf", "a", json!([])),
                tnode("2", "leaf", "b", json!([])),
                tnode("3", "leaf", "a", json!([])), // duplicate 'a' — not repeated
                tnode("4", "leaf", "c", json!([])),
                tnode("5", "leaf", "d", json!([])),
                tnode("6", "leaf", "e", json!([])), // 5th distinct — dropped (cap 4)
            ]),
        ))
        .unwrap();
        assert_eq!(guardrail_node_value_summary(Some(&node)), "a, b, c, d");
        assert_eq!(guardrail_node_value_summary(None), "");

        // A childless node falls back to its own label.
        let leaf: SemanticNode =
            serde_json::from_value(tnode("0", "leaf", "solo", json!([]))).unwrap();
        assert_eq!(guardrail_node_value_summary(Some(&leaf)), "solo");
    }

    #[test]
    fn glob_match_supports_star_question_and_classes() {
        assert!(guardrail_glob_match("main.tf", "*.tf"));
        assert!(!guardrail_glob_match("main.yaml", "*.tf"));
        assert!(guardrail_glob_match("a.yaml", "?.yaml"));
        assert!(!guardrail_glob_match("ab.yaml", "?.yaml"));
        assert!(guardrail_glob_match("config/prod.tf", "config/*.tf"));
        assert!(guardrail_glob_match("bx", "[abc]x"));
        assert!(!guardrail_glob_match("dx", "[abc]x"));
        assert!(guardrail_glob_match("dx", "[!abc]x"));
        assert!(guardrail_glob_match("m", "[a-z]"));
        assert!(guardrail_glob_match("anything", "*"));
        assert!(guardrail_glob_match("exact", "exact"));
    }

    #[test]
    fn evaluate_matches_dockerfile_path_and_dedups() {
        // Mirrors test_guardrails.py's dockerfile case: FROM instruction keys to
        // "instruction.from.0". Two identical changes must yield ONE violation.
        let old_tree = tnode(
            "0",
            "source_file",
            "source_file",
            json!([tnode("0.0", "from_instruction", "FROM python:3.11", json!([]))]),
        );
        let new_tree = tnode(
            "0",
            "source_file",
            "source_file",
            json!([tnode("0.0", "from_instruction", "FROM python:3.12", json!([]))]),
        );
        let request: GuardrailEvalRequest = serde_json::from_value(json!({
            "language": "dockerfile",
            "old_filename": "Dockerfile",
            "new_filename": "Dockerfile",
            "old_tree": old_tree,
            "new_tree": new_tree,
            "changes": [
                {"old_node_id": "0.0", "new_node_id": "0.0"},
                {"old_node_id": "0.0", "new_node_id": "0.0"},
            ],
            "rules": [{
                "rule_id": "img",
                "severity": "immutable",
                "language": "dockerfile",
                "path": "instruction.from.0",
                "message": "base image changed",
                "files": [],
            }],
        }))
        .unwrap();

        let violations = evaluate_guardrail_rules(&request);
        assert_eq!(violations.len(), 1, "dedup: one violation per (rule, path)");
        let v = &violations[0];
        assert_eq!(v.rule_id, "img");
        assert_eq!(v.semantic_path, "instruction.from.0");
        assert_eq!(v.file, "Dockerfile");
        assert_eq!(v.node_type, "from_instruction");
        assert_eq!(v.new_node_id.as_deref(), Some("0.0"));
    }

    #[test]
    fn evaluate_skips_unmatched_language_and_file() {
        let tree = tnode(
            "0",
            "source_file",
            "source_file",
            json!([tnode("0.0", "from_instruction", "FROM x", json!([]))]),
        );
        // Rule language mismatch → no violations even though the path would match.
        let request: GuardrailEvalRequest = serde_json::from_value(json!({
            "language": "dockerfile",
            "old_filename": "Dockerfile",
            "new_filename": "Dockerfile",
            "old_tree": tree.clone(),
            "new_tree": tree,
            "changes": [{"old_node_id": "0.0", "new_node_id": "0.0"}],
            "rules": [{
                "rule_id": "img", "severity": "immutable", "language": "hcl",
                "path": "instruction.from.0", "message": "m", "files": [],
            }],
        }))
        .unwrap();
        assert!(evaluate_guardrail_rules(&request).is_empty());
    }
}

/// Detect a file's content type from its leading bytes (magic-byte sniffing).
/// Returns JSON `{mime, extension, category, is_text}`. Callers pass a head slice
/// (a few KB is plenty); never raises.
// ---------------------------------------------------------------------------
// Index engine (symbol/reference tables + cross-file diff), Rust-authoritative.
//
// Thin native wrappers over `index-engine-lib` — the same crate the index-engine
// Wasm plugin wraps — so the Python shell can build symbol tables and detect
// cross-file changes without a Wasm round-trip, and so any future binding
// (Go/Java over the C ABI) computes them identically. These replace the former
// index-engine Wasm-adapter path and the deleted Python fallbacks (#91).
//
// Each returns a JSON string; on malformed input the lib emits an
// `{"error": ...}` envelope (never a panic), matching the plugin contract.
// ---------------------------------------------------------------------------

/// Build a flat qualified-name → [SymbolDefinition] table from a JSON array of
/// `{filename, language, tree}` file entries.
/// Build a label → [ReferenceUsage] table (call-sites / imports / type usages)
/// from a JSON array of `{filename, language, tree}` file entries.
/// Diff two symbol tables, producing MOVE_TO_MODULE / SPLIT_MODULE /
/// CROSS_FILE_RENAME cross-file changes.
#[derive(Debug, Deserialize)]
struct RustCoreConfig {
    min_height: usize,
    min_similarity: f64,
    max_nodes: usize,
    max_cst_bytes: usize,
    max_plugin_output_bytes: usize,
    plugin_fuel: u64,
    profile_phases: bool,
    collect_trace: bool,
}

impl RustCoreConfig {
    fn from_json(raw: &str) -> Self {
        let value: Value = serde_json::from_str(raw).unwrap_or(Value::Null);
        Self {
            min_height: value
                .get("min_height")
                .or_else(|| value.get("minHeight"))
                .and_then(Value::as_u64)
                .unwrap_or(2) as usize,
            min_similarity: value
                .get("min_similarity")
                .or_else(|| value.get("minSimilarity"))
                .and_then(Value::as_f64)
                .unwrap_or(0.5),
            collect_trace: value
                .get("collect_trace")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            max_nodes: value
                .get("max_nodes")
                .or_else(|| value.get("maxNodes"))
                .and_then(Value::as_u64)
                .unwrap_or(50_000) as usize,
            max_cst_bytes: value
                .get("max_cst_bytes")
                .or_else(|| value.get("maxCstBytes"))
                .and_then(Value::as_u64)
                .unwrap_or(DEFAULT_MAX_CST_BYTES as u64) as usize,
            max_plugin_output_bytes: value
                .get("max_plugin_output_bytes")
                .or_else(|| value.get("maxPluginOutputBytes"))
                .and_then(Value::as_u64)
                .unwrap_or(DEFAULT_MAX_PLUGIN_OUTPUT_BYTES as u64)
                as usize,
            plugin_fuel: value
                .get("plugin_fuel")
                .or_else(|| value.get("pluginFuel"))
                .and_then(Value::as_i64)
                .map(|fuel| if fuel < 0 { u64::MAX } else { fuel as u64 })
                .unwrap_or(10_000_000),
            profile_phases: value
                .get("profile_phases")
                .or_else(|| value.get("profilePhases"))
                .and_then(Value::as_bool)
                .unwrap_or(true),
        }
    }
}

fn check_byte_limit(label: &str, text: &str, limit: usize) -> Result<(), String> {
    let size = text.len();
    if size > limit {
        Err(format!("{label} is {size} bytes; limit is {limit} bytes"))
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CstNode {
    #[serde(rename = "type")]
    node_type: String,
    #[serde(default)]
    named: bool,
    #[serde(default)]
    text: String,
    #[serde(default)]
    start_line: u32,
    #[serde(default)]
    start_col: u32,
    #[serde(default)]
    end_line: u32,
    #[serde(default)]
    end_col: u32,
    #[serde(default)]
    children: Vec<CstNode>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct NodePosition {
    start_line: u32,
    start_col: u32,
    end_line: u32,
    end_col: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SemanticNode {
    id: String,
    node_type: String,
    label: String,
    position: NodePosition,
    structural_hash: String,
    #[serde(default)]
    children: Vec<SemanticNode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parent_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    type_info: Option<String>,
    /// Opaque passthrough of parser-emitted structural facts (privacy-safe counts/
    /// enums/flags). The core does not interpret it — it only carries it through so
    /// the field survives the matching round-trip to consumers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    facts: Option<serde_json::Value>,
}

impl SemanticNode {
    fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }

    fn descendants(&self) -> Vec<&SemanticNode> {
        let mut result = Vec::new();
        self.push_descendants(&mut result);
        result
    }

    fn push_descendants<'a>(&'a self, result: &mut Vec<&'a SemanticNode>) {
        for child in &self.children {
            result.push(child);
            child.push_descendants(result);
        }
    }
}

#[derive(Clone, Debug)]
struct MatchPair<'a> {
    old_node: &'a SemanticNode,
    new_node: &'a SemanticNode,
}

struct TreeIndex<'a> {
    nodes: Vec<&'a SemanticNode>,
    parent: HashMap<&'a str, &'a str>,
    by_id: HashMap<&'a str, &'a SemanticNode>,
    children: HashMap<&'a str, Vec<&'a str>>,
    heights: HashMap<&'a str, usize>,
    subtree_sizes: HashMap<&'a str, usize>,
    named_entities: Vec<&'a SemanticNode>,
}

impl<'a> TreeIndex<'a> {
    fn new(root: &'a SemanticNode) -> Self {
        let mut index = Self {
            nodes: Vec::new(),
            parent: HashMap::new(),
            by_id: HashMap::new(),
            children: HashMap::new(),
            heights: HashMap::new(),
            subtree_sizes: HashMap::new(),
            named_entities: Vec::new(),
        };
        index.push_node(root, None);
        index
    }

    fn push_node(&mut self, node: &'a SemanticNode, parent_id: Option<&'a str>) -> usize {
        self.nodes.push(node);
        self.by_id.insert(node.id.as_str(), node);
        if let Some(parent_id) = parent_id {
            self.parent.insert(node.id.as_str(), parent_id);
        }
        if is_named_entity_type(node.node_type.as_str()) {
            self.named_entities.push(node);
        }
        if !node.children.is_empty() {
            self.children.insert(
                node.id.as_str(),
                node.children
                    .iter()
                    .map(|child| child.id.as_str())
                    .collect(),
            );
        }
        let mut height = 0usize;
        let mut subtree_size = 1usize;
        for child in &node.children {
            let child_size = self.push_node(child, Some(node.id.as_str()));
            subtree_size += child_size;
            height = height.max(self.heights.get(child.id.as_str()).copied().unwrap_or(0) + 1);
        }
        self.heights.insert(node.id.as_str(), height);
        self.subtree_sizes.insert(node.id.as_str(), subtree_size);
        subtree_size
    }
}

#[derive(Clone, Debug, Default, Serialize)]
struct MatchingDiagnostics {
    attempted: bool,
    used: bool,
    disabled: bool,
    disabled_reason: String,
    old_entity_count: usize,
    new_entity_count: usize,
    exact_id_matches: usize,
    structural_matches: usize,
    label_parent_matches: usize,
    fuzzy_token_candidates: usize,
    fuzzy_token_matches: usize,
    seeded_matches: usize,
    descendant_seeded_matches: usize,
    bottom_up_matches: usize,
    final_matching_pairs: usize,
    initial_change_count: usize,
    final_change_count: usize,
    refinement_added_count: usize,
    refinement_removed_count: usize,
    suppressed_add_delete_noise: usize,
    unmatched_add_delete_noise: usize,
    edit_script: EditScriptDiagnostics,
}

impl MatchingDiagnostics {
    fn as_entity_fast_path_metadata(&self) -> Value {
        json!({
            "attempted": self.attempted,
            "used": self.used,
            "disabled": self.disabled,
            "disabled_reason": self.disabled_reason,
            "old_entities": self.old_entity_count,
            "new_entities": self.new_entity_count,
            "seeded_matches": self.seeded_matches,
            "descendant_seeded_matches": self.descendant_seeded_matches,
            "fuzzy_token_candidates": self.fuzzy_token_candidates,
            "fuzzy_token_matching_enabled": false,
            "matches_by_strategy": {
                "exact_id": self.exact_id_matches,
                "structural": self.structural_matches,
                "label_parent": self.label_parent_matches,
                "fuzzy_token": self.fuzzy_token_matches,
                "bottom_up": self.bottom_up_matches,
            },
            "refinement": {
                "initial_change_count": self.initial_change_count,
                "final_change_count": self.final_change_count,
                "added_count": self.refinement_added_count,
                "removed_count": self.refinement_removed_count,
                "suppressed_add_delete_noise": self.suppressed_add_delete_noise,
                "unmatched_add_delete_noise": self.unmatched_add_delete_noise,
            },
            "edit_script": self.edit_script.as_metadata(),
            "final_matching_pairs": self.final_matching_pairs,
        })
    }
}

#[derive(Clone, Debug, Default, Serialize)]
struct EditScriptDiagnostics {
    move_candidates: usize,
    delete_candidates: usize,
    add_candidates: usize,
    update_candidates: usize,
    reorder_candidates: usize,
    pruned_old_descendant_deletes: usize,
    pruned_new_descendant_additions: usize,
    skipped_reorders_under_moves: usize,
    pre_refinement_change_count: usize,
    initial_draft_count: usize,
    pruned_before_draft_count: usize,
    serialized_final_change_count: usize,
    json_nodes_serialized_count: usize,
}

impl EditScriptDiagnostics {
    fn as_metadata(&self) -> Value {
        json!({
            "move_candidates": self.move_candidates,
            "delete_candidates": self.delete_candidates,
            "add_candidates": self.add_candidates,
            "update_candidates": self.update_candidates,
            "reorder_candidates": self.reorder_candidates,
            "pruned_old_descendant_deletes": self.pruned_old_descendant_deletes,
            "pruned_new_descendant_additions": self.pruned_new_descendant_additions,
            "skipped_reorders_under_moves": self.skipped_reorders_under_moves,
            "pre_refinement_change_count": self.pre_refinement_change_count,
            "initial_draft_count": self.initial_draft_count,
            "pruned_before_draft_count": self.pruned_before_draft_count,
            "serialized_final_change_count": self.serialized_final_change_count,
            "json_nodes_serialized_count": self.json_nodes_serialized_count,
        })
    }
}

#[derive(Clone, Debug)]
struct MatchingReport<'a> {
    pairs: Vec<MatchPair<'a>>,
    diagnostics: MatchingDiagnostics,
}

#[derive(Clone, Debug)]
struct EditOp<'a> {
    kind: &'static str,
    old_node: Option<&'a SemanticNode>,
    new_node: Option<&'a SemanticNode>,
    old_index: Option<usize>,
    new_index: Option<usize>,
}

#[derive(Clone, Debug)]
struct ChangeDraft<'a> {
    change_type: &'static str,
    old_node: Option<&'a SemanticNode>,
    new_node: Option<&'a SemanticNode>,
    old_index: Option<usize>,
    new_index: Option<usize>,
    confidence: f64,
    description: String,
    refactoring_kind: Option<&'static str>,
    text_diff: Option<String>,
}

#[derive(Clone, Debug)]
struct ChangeGenerationReport<'a> {
    drafts: Vec<ChangeDraft<'a>>,
    diagnostics: EditScriptDiagnostics,
}

#[derive(Clone, Debug)]
#[cfg(test)]
struct ValueChangeGenerationReport {
    changes: Vec<Value>,
    diagnostics: EditScriptDiagnostics,
}

#[derive(Clone, Debug)]
struct EditScriptReport<'a> {
    ops: Vec<EditOp<'a>>,
    diagnostics: EditScriptDiagnostics,
}

struct SerializedChanges {
    changes: Vec<Value>,
    json_nodes_serialized_count: usize,
}

#[derive(Default)]
struct PythonReviewFinalization {
    change_groups: Vec<Value>,
    ignored_style_changes: Vec<Value>,
    is_style_only: bool,
    no_surviving_changes: bool,
}

#[derive(Clone, Debug)]
struct RustSourceEvidence {
    old_label: String,
    new_label: String,
    old_span: (usize, usize),
    new_span: (usize, usize),
}

#[derive(Clone, Debug)]
struct RustSourceLiteralEvidence {
    old_label: String,
    new_label: String,
    canonical: String,
    old_span: (usize, usize),
    new_span: (usize, usize),
}

#[derive(Clone, Debug)]
struct RustColorEvidence {
    old_label: String,
    new_label: String,
    canonical: String,
    old_span: (usize, usize),
    new_span: (usize, usize),
}

struct PhaseProbe {
    enabled: bool,
    phases: Vec<Value>,
}

impl Default for PhaseProbe {
    fn default() -> Self {
        Self {
            enabled: true,
            phases: Vec::new(),
        }
    }
}

impl PhaseProbe {
    fn enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.phases.clear();
        }
    }

    fn measure<T, F>(&mut self, name: &'static str, op: F) -> Result<T, String>
    where
        F: FnOnce() -> Result<T, String>,
    {
        if !self.enabled {
            return op();
        }
        let start = Instant::now();
        let result = op();
        let duration_ms = start.elapsed().as_secs_f64() * 1000.0;
        self.phases.push(json!({
            "name": name,
            "duration_ms": duration_ms,
        }));
        result
    }

    fn measure_value<T, F>(&mut self, name: &'static str, op: F) -> T
    where
        F: FnOnce() -> T,
    {
        if !self.enabled {
            return op();
        }
        let start = Instant::now();
        let result = op();
        let duration_ms = start.elapsed().as_secs_f64() * 1000.0;
        self.phases.push(json!({
            "name": name,
            "duration_ms": duration_ms,
        }));
        result
    }

    fn phases(self) -> Vec<Value> {
        self.phases
    }

    fn push_elapsed(&mut self, name: &'static str, started: Option<Instant>) {
        if !self.enabled {
            return;
        }
        let Some(started) = started else {
            return;
        };
        self.phases.push(json!({
            "name": name,
            "duration_ms": started.elapsed().as_secs_f64() * 1000.0,
        }));
    }
}

struct ParserHostState {
    table: ResourceTable,
    ctx: WasiCtx,
}

struct CachedParserComponent {
    engine: Engine,
    component: Component,
}

#[derive(Clone)]
struct ParserComponentLookup {
    cached: Arc<CachedParserComponent>,
    cache_hit: bool,
    cache_key: String,
}

struct WasmProcessPair {
    old_tree: String,
    new_tree: String,
    cache_hit: bool,
    cache_key: String,
}

static PARSER_COMPONENT_CACHE: OnceLock<Mutex<HashMap<String, Arc<CachedParserComponent>>>> =
    OnceLock::new();

impl ParserHostState {
    fn new() -> Self {
        Self {
            table: ResourceTable::new(),
            ctx: WasiCtxBuilder::new().build(),
        }
    }
}

impl WasiView for ParserHostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.ctx,
            table: &mut self.table,
        }
    }
}

impl parser_plugin::intentdiff::plugin::host_utils::Host for ParserHostState {
    fn strip_trivia(&mut self, cst_json: String, trivia_types: Vec<String>) -> String {
        if cst_json.as_bytes().len() > DEFAULT_MAX_CST_BYTES {
            return json!({"error": "host-utils input exceeds byte limit"}).to_string();
        }
        if let Err(detail) = check_trivia_type_limit(&trivia_types) {
            return json!({"error": detail}).to_string();
        }
        strip_trivia_json(&cst_json, &trivia_types).unwrap_or(cst_json)
    }

    fn structural_hash(&mut self, cst_json: String) -> String {
        if cst_json.as_bytes().len() > DEFAULT_MAX_CST_BYTES {
            let mut hasher = Sha256::new();
            hasher.update(b"host-utils input exceeds byte limit");
            return hex::encode(hasher.finalize());
        }
        semantic_hash_json(&cst_json).unwrap_or_else(|_| {
            let mut hasher = Sha256::new();
            hasher.update(cst_json.as_bytes());
            hex::encode(hasher.finalize())
        })
    }

    fn log(&mut self, _level: String, _message: String) {}
}

fn diff_python_sources_stage11_impl(
    old_source: &str,
    new_source: &str,
    old_filename: &str,
    new_filename: &str,
    parser_wasm_path: &str,
    config_json: &str,
) -> Result<Value, String> {
    if !old_filename.ends_with(".py")
        && !old_filename.ends_with(".pyi")
        && !new_filename.ends_with(".py")
        && !new_filename.ends_with(".pyi")
    {
        return Err("unsupported filename".to_owned());
    }
    if parser_wasm_path.is_empty() {
        return Err("missing parser wasm path".to_owned());
    }
    if !Path::new(parser_wasm_path).exists() {
        return Err(format!("parser wasm path not found: {parser_wasm_path}"));
    }

    let config = RustCoreConfig::from_json(config_json);
    check_byte_limit("old source", old_source, config.max_cst_bytes)?;
    check_byte_limit("new source", new_source, config.max_cst_bytes)?;
    let mut probe = PhaseProbe::default();

    let old_ts_tree = probe.measure("rust_tree_sitter_parse_old", || {
        parse_python_tree(old_source)
    })?;
    let new_ts_tree = probe.measure("rust_tree_sitter_parse_new", || {
        parse_python_tree(new_source)
    })?;
    let old_cst_json = probe.measure("rust_cst_serialization_old", || {
        serialize_tree_json(&old_ts_tree, old_source)
    })?;
    let new_cst_json = probe.measure("rust_cst_serialization_new", || {
        serialize_tree_json(&new_ts_tree, new_source)
    })?;
    check_byte_limit("old CST JSON", &old_cst_json, config.max_cst_bytes)?;
    check_byte_limit("new CST JSON", &new_cst_json, config.max_cst_bytes)?;
    let trivia_types: Vec<String> = PYTHON_TRIVIA
        .iter()
        .map(|item| (*item).to_owned())
        .collect();
    let old_filtered = probe.measure("rust_trivia_stripping_old", || {
        strip_trivia_json(&old_cst_json, &trivia_types)
    })?;
    let new_filtered = probe.measure("rust_trivia_stripping_new", || {
        strip_trivia_json(&new_cst_json, &trivia_types)
    })?;
    let old_hash = probe.measure("rust_style_hashing_old", || {
        semantic_hash_json(&old_filtered)
    })?;
    let new_hash = probe.measure("rust_style_hashing_new", || {
        semantic_hash_json(&new_filtered)
    })?;
    if old_hash == new_hash {
        return Err("style-only shortcut requires Python evidence path".to_owned());
    }

    let old_cst_nodes = count_cst_nodes_json(&old_filtered)?;
    let new_cst_nodes = count_cst_nodes_json(&new_filtered)?;
    let max_cst_nodes = old_cst_nodes.max(new_cst_nodes);
    let adaptive_fuel = fuel_budget(
        config.plugin_fuel,
        20_000_000 + max_cst_nodes as u64 * 200_000,
    );

    let (old_tree_json, new_tree_json) = probe.measure("rust_wasm_parser_execution", || {
        run_python_wasm_process_pair(
            parser_wasm_path,
            old_source,
            &old_filtered,
            old_filename,
            new_source,
            &new_filtered,
            new_filename,
            adaptive_fuel,
            config.max_plugin_output_bytes,
            "python",
        )
    })?;
    let old_tree: SemanticNode = probe.measure("rust_semantic_json_parse_old", || {
        parse_semantic_tree_json(&old_tree_json)
    })?;
    let new_tree: SemanticNode = probe.measure("rust_semantic_json_parse_new", || {
        parse_semantic_tree_json(&new_tree_json)
    })?;
    probe.measure("rust_semantic_node_validation", || {
        validate_unique_ids(&old_tree)?;
        validate_unique_ids(&new_tree)?;
        Ok(())
    })?;

    let old_count = 1 + old_tree.descendants().len();
    let new_count = 1 + new_tree.descendants().len();
    if old_count > config.max_nodes || new_count > config.max_nodes {
        return Err(format!("tree too large: old={old_count}, new={new_count}"));
    }

    let matching = probe.measure("rust_matching_diff", || {
        Ok(compute_matching(
            &old_tree,
            &new_tree,
            config.min_height,
            config.min_similarity,
        ))
    })?;
    let changes = probe.measure("rust_initial_diff_generation", || {
        Ok(generate_changes(&old_tree, &new_tree, &matching))
    })?;
    let matching_pairs: Vec<Value> = matching
        .iter()
        .map(|pair| {
            json!({
                "old_id": pair.old_node.id,
                "new_id": pair.new_node.id,
            })
        })
        .collect();
    let phases = probe.phases();

    Ok(json!({
        "status": COMPLETE,
        "engine": V3_ENGINE,
        "old_filename": old_filename,
        "new_filename": new_filename,
        "language": "python",
        "old_tree": old_tree,
        "new_tree": new_tree,
        "changes": changes,
        "matching_pairs": matching_pairs,
        "adaptive_fuel": adaptive_fuel,
        "metadata": {
            "old_nodes": old_count,
            "new_nodes": new_count,
            "old_cst_nodes": old_cst_nodes,
            "new_cst_nodes": new_cst_nodes,
            "matching_pairs": matching.len(),
            "rust_core_stage": "sources_to_stage11",
            "engine": V3_ENGINE,
            "phase_timings": phases,
            "wasm_boundary": "rust_wasmtime",
        },
    }))
}

fn diff_python_sources_final_impl(
    old_source: &str,
    new_source: &str,
    old_filename: &str,
    new_filename: &str,
    parser_wasm_path: &str,
    config_json: &str,
    status: &str,
    certified_product: bool,
    preloaded_component: Option<&ParserComponentLookup>,
    python_parser_backend: &str,
    file_lifecycle: &str,
) -> Result<Value, String> {
    if !old_filename.ends_with(".py")
        && !old_filename.ends_with(".pyi")
        && !new_filename.ends_with(".py")
        && !new_filename.ends_with(".pyi")
    {
        return Err("unsupported filename".to_owned());
    }
    let use_native_parser = python_parser_backend == PYTHON_PARSER_BACKEND_NATIVE;
    let file_lifecycle =
        infer_file_lifecycle(Some(file_lifecycle), old_source, new_source, None);
    if !use_native_parser && parser_wasm_path.is_empty() {
        return Err("missing parser wasm path".to_owned());
    }
    if !use_native_parser && preloaded_component.is_none() && !Path::new(parser_wasm_path).exists()
    {
        return Err(format!("parser wasm path not found: {parser_wasm_path}"));
    }

    let config = RustCoreConfig::from_json(config_json);
    check_byte_limit("old source", old_source, config.max_cst_bytes)?;
    check_byte_limit("new source", new_source, config.max_cst_bytes)?;
    let mut probe = PhaseProbe::default();
    let certification = if certified_product && use_native_parser {
        PYTHON_NATIVE_V4KB_CERTIFICATION
    } else if certified_product {
        PYTHON_V4E_CERTIFICATION
    } else {
        ""
    };
    let trust_tier = if use_native_parser {
        "first_party_core_builder"
    } else {
        "sandboxed_wasm_plugin"
    };
    let wasm_boundary = if use_native_parser {
        "bypassed_first_party_native_python"
    } else {
        "rust_wasmtime"
    };

    let old_ts_tree = probe.measure("rust_tree_sitter_parse_old", || {
        parse_python_tree(old_source)
    })?;
    let new_ts_tree = probe.measure("rust_tree_sitter_parse_new", || {
        parse_python_tree(new_source)
    })?;
    if certified_product
        && (old_ts_tree.root_node().has_error() || new_ts_tree.root_node().has_error())
    {
        return Err("parse errors require Python token fallback".to_owned());
    }
    let (
        mut old_tree,
        mut new_tree,
        wasm_cache_hit,
        wasm_cache_key,
        wasm_batch_preloaded,
        old_cst_nodes,
        new_cst_nodes,
        adaptive_fuel,
    ) = if use_native_parser {
        let trivia: HashSet<&str> = PYTHON_TRIVIA.iter().copied().collect();
        let old_cst = probe.measure("rust_cst_serialization_old", || {
            serialize_ts_node(old_ts_tree.root_node(), old_source.as_bytes())
        })?;
        let new_cst = probe.measure("rust_cst_serialization_new", || {
            serialize_ts_node(new_ts_tree.root_node(), new_source.as_bytes())
        })?;
        let old_filtered = probe.measure("rust_trivia_stripping_old", || {
            strip_trivia_node(&old_cst, &trivia)
                .ok_or_else(|| "trivia stripping removed the old root node".to_owned())
        })?;
        let new_filtered = probe.measure("rust_trivia_stripping_new", || {
            strip_trivia_node(&new_cst, &trivia)
                .ok_or_else(|| "trivia stripping removed the new root node".to_owned())
        })?;
        let mut old_hash_memo = HashMap::new();
        let mut new_hash_memo = HashMap::new();
        let old_hash = probe.measure("rust_native_semantic_hashing_old", || {
            Ok(structural_hash_cst_with_memo(
                &old_filtered,
                &mut old_hash_memo,
            ))
        })?;
        let new_hash = probe.measure("rust_native_semantic_hashing_new", || {
            Ok(structural_hash_cst_with_memo(
                &new_filtered,
                &mut new_hash_memo,
            ))
        })?;
        let old_cst_nodes = count_cst_nodes(&old_filtered);
        let new_cst_nodes = count_cst_nodes(&new_filtered);
        if old_hash == new_hash {
            if certified_product {
                return Err(
                    "style-only changed file is not certified for Rust product path".to_owned(),
                );
            }
            let phases = probe.phases();
            let mut diff = semantic_diff_payload_with_style(
                old_filename,
                new_filename,
                Vec::new(),
                false,
                true,
                status,
                json!({
                    "engine": BATCH_ENGINE,
                    "rust_core_stage": "candidate_final_diff",
                    "boundary": "source_batch_to_final_diff",
                    "python_parser_backend": python_parser_backend,
                    "trust_tier": trust_tier,
                    "old_cst_nodes": old_cst_nodes,
                    "new_cst_nodes": new_cst_nodes,
                    "phase_timings": phases,
                    "candidate_certification": PYTHON_NATIVE_V4K_CERTIFICATION,
                    "wasm_boundary": wasm_boundary,
                    "note": "style-only Rust V4-K native candidate",
                }),
            );
            diff["metadata"]["rust_core"]["engine"] = json!(BATCH_ENGINE);
            diff["metadata"]["rust_core"]["used"] = json!(true);
            diff["metadata"]["rust_phase_timings"] =
                diff["metadata"]["rust_core"]["details"]["phase_timings"].clone();
            apply_file_lifecycle_to_diff(&mut diff, file_lifecycle);
            return Ok(diff);
        }
        let max_cst_nodes = old_cst_nodes.max(new_cst_nodes);
        let adaptive_fuel = fuel_budget(
            config.plugin_fuel,
            20_000_000 + max_cst_nodes as u64 * 200_000,
        );
        let old_tree = probe.measure("rust_native_semantic_build_old", || {
            convert_cst_with_hash_memo(&old_filtered, "0", None, &old_hash_memo)
                .ok_or_else(|| "native Python builder produced no old semantic tree".to_owned())
        })?;
        let new_tree = probe.measure("rust_native_semantic_build_new", || {
            convert_cst_with_hash_memo(&new_filtered, "0", None, &new_hash_memo)
                .ok_or_else(|| "native Python builder produced no new semantic tree".to_owned())
        })?;
        probe.measure("rust_native_builder_validation", || {
            validate_unique_ids(&old_tree)?;
            validate_unique_ids(&new_tree)?;
            Ok(())
        })?;
        (
            old_tree,
            new_tree,
            false,
            String::new(),
            false,
            old_cst_nodes,
            new_cst_nodes,
            adaptive_fuel,
        )
    } else {
        let old_cst_json = probe.measure("rust_cst_serialization_old", || {
            serialize_tree_json(&old_ts_tree, old_source)
        })?;
        let new_cst_json = probe.measure("rust_cst_serialization_new", || {
            serialize_tree_json(&new_ts_tree, new_source)
        })?;
        check_byte_limit("old CST JSON", &old_cst_json, config.max_cst_bytes)?;
        check_byte_limit("new CST JSON", &new_cst_json, config.max_cst_bytes)?;
        let trivia_types: Vec<String> = PYTHON_TRIVIA
            .iter()
            .map(|item| (*item).to_owned())
            .collect();
        let old_filtered = probe.measure("rust_trivia_stripping_old", || {
            strip_trivia_json(&old_cst_json, &trivia_types)
        })?;
        let new_filtered = probe.measure("rust_trivia_stripping_new", || {
            strip_trivia_json(&new_cst_json, &trivia_types)
        })?;
        let old_hash = probe.measure("rust_style_hashing_old", || {
            semantic_hash_json(&old_filtered)
        })?;
        let new_hash = probe.measure("rust_style_hashing_new", || {
            semantic_hash_json(&new_filtered)
        })?;
        let old_cst_nodes = count_cst_nodes_json(&old_filtered)?;
        let new_cst_nodes = count_cst_nodes_json(&new_filtered)?;
        if old_hash == new_hash {
            if certified_product {
                return Err(
                    "style-only changed file is not certified for Rust product path".to_owned(),
                );
            }
            let phases = probe.phases();
            let mut diff = semantic_diff_payload_with_style(
                old_filename,
                new_filename,
                Vec::new(),
                false,
                true,
                status,
                json!({
                    "engine": BATCH_ENGINE,
                    "rust_core_stage": "candidate_final_diff",
                    "boundary": "source_batch_to_final_diff",
                    "python_parser_backend": python_parser_backend,
                    "trust_tier": trust_tier,
                    "old_cst_nodes": old_cst_nodes,
                    "new_cst_nodes": new_cst_nodes,
                    "phase_timings": phases,
                    "candidate_certification": PYTHON_V4E_CERTIFICATION,
                    "wasm_boundary": wasm_boundary,
                    "note": "style-only Rust V4-E candidate",
                }),
            );
            diff["metadata"]["rust_core"]["engine"] = json!(BATCH_ENGINE);
            diff["metadata"]["rust_core"]["used"] = json!(true);
            diff["metadata"]["rust_phase_timings"] =
                diff["metadata"]["rust_core"]["details"]["phase_timings"].clone();
            apply_file_lifecycle_to_diff(&mut diff, file_lifecycle);
            return Ok(diff);
        }

        let max_cst_nodes = old_cst_nodes.max(new_cst_nodes);
        let adaptive_fuel = fuel_budget(
            config.plugin_fuel,
            20_000_000 + max_cst_nodes as u64 * 200_000,
        );
        if let Some(preloaded) = preloaded_component {
            let wasm_result = run_python_wasm_process_pair_preloaded_profiled(
                preloaded,
                old_source,
                &old_filtered,
                old_filename,
                new_source,
                &new_filtered,
                new_filename,
                adaptive_fuel,
                config.max_plugin_output_bytes,
                &mut probe,
            )?;
            let old_tree: SemanticNode = probe.measure("rust_semantic_json_parse_old", || {
                parse_semantic_tree_json(&wasm_result.old_tree)
            })?;
            let new_tree: SemanticNode = probe.measure("rust_semantic_json_parse_new", || {
                parse_semantic_tree_json(&wasm_result.new_tree)
            })?;
            (
                old_tree,
                new_tree,
                wasm_result.cache_hit,
                wasm_result.cache_key,
                true,
                old_cst_nodes,
                new_cst_nodes,
                adaptive_fuel,
            )
        } else {
            let wasm_result = run_python_wasm_process_pair_detailed_profiled(
                parser_wasm_path,
                old_source,
                &old_filtered,
                old_filename,
                new_source,
                &new_filtered,
                new_filename,
                adaptive_fuel,
                config.max_plugin_output_bytes,
                &mut probe,
            )?;
            let old_tree: SemanticNode = probe.measure("rust_semantic_json_parse_old", || {
                parse_semantic_tree_json(&wasm_result.old_tree)
            })?;
            let new_tree: SemanticNode = probe.measure("rust_semantic_json_parse_new", || {
                parse_semantic_tree_json(&wasm_result.new_tree)
            })?;
            (
                old_tree,
                new_tree,
                wasm_result.cache_hit,
                wasm_result.cache_key,
                false,
                old_cst_nodes,
                new_cst_nodes,
                adaptive_fuel,
            )
        }
    };
    // Fill facts the raw-CST pass could not reach — once, where BOTH parser arms converge.
    //
    // Facts are derived from the RAW CST, whose shape varies by parser: tree-sitter keeps
    // keyword tokens and nests calls where the native builder does not. That pass therefore
    // returns a PARTIAL bag, and nothing downstream completed it, so this path silently lost
    // `recursive`, `has_error_handling`, `method_count`, `control_shape` and
    // `behavior_category`.
    //
    // Deliberately placed after the arms rather than inside either: putting it in one arm is
    // how the two builders drifted apart in the first place.
    //
    // `enrich_tree_facts` derives from the NORMALISED tree, which is parser-independent, and
    // merges without overwriting — the pre-pruning CST pass still wins every key it computed.
    probe.measure("rust_semantic_facts_enrich", || {
        enrich_tree_facts(&mut old_tree);
        enrich_tree_facts(&mut new_tree);
        Ok::<(), String>(())
    })?;
    let old_index = TreeIndex::new(&old_tree);
    let new_index = TreeIndex::new(&new_tree);
    probe.measure("rust_semantic_node_validation", || {
        validate_unique_index_ids(&old_index)?;
        validate_unique_index_ids(&new_index)?;
        Ok(())
    })?;

    let old_count = old_index.nodes.len();
    let new_count = new_index.nodes.len();
    if old_count > config.max_nodes || new_count > config.max_nodes {
        return Err(format!("tree too large: old={old_count}, new={new_count}"));
    }

    let detailed_matching_diagnostics = probe.enabled();
    let mut matching_report = probe.measure("rust_matching", || {
        Ok(compute_matching_with_diagnostics_indexed(
            &old_index,
            &new_index,
            config.min_height,
            config.min_similarity,
            detailed_matching_diagnostics,
        ))
    })?;
    let matching = matching_report.pairs;
    let initial_diff_started = probe.enabled().then(Instant::now);
    let draft_generation_started = probe.enabled().then(Instant::now);
    let script = generate_edit_script_with_diagnostics_indexed(
        &old_index,
        &new_index,
        &matching,
        Some(&mut probe),
    );
    let mut change_report = ChangeGenerationReport {
        drafts: script.ops.into_iter().map(edit_op_to_draft).collect(),
        diagnostics: script.diagnostics,
    };
    probe.push_elapsed("rust_change_draft_generation", draft_generation_started);
    probe.push_elapsed("rust_initial_diff_generation", initial_diff_started);
    let initial_change_count = change_report.drafts.len();
    change_report.diagnostics.initial_draft_count = initial_change_count;
    let initial_add_delete_noise = add_delete_noise_count_drafts(&change_report.drafts);
    let refinement_started = probe.enabled().then(Instant::now);
    let draft_refinement_started = probe.enabled().then(Instant::now);
    refine_candidate_drafts(&mut change_report.drafts, &matching, Some(&mut probe), "python");
    probe.push_elapsed("rust_change_draft_refinement", draft_refinement_started);
    probe.push_elapsed("rust_candidate_refinement", refinement_started);
    let review_finalization_started = probe.enabled().then(Instant::now);
    let mut review_finalization = PythonReviewFinalization::default();
    finalize_python_review_drafts(
        &mut change_report.drafts,
        &old_tree,
        &new_tree,
        old_source,
        new_source,
        &mut review_finalization,
        "python",
    );
    probe.push_elapsed(
        "rust_python_review_finalization",
        review_finalization_started,
    );
    let serialization_started = probe.enabled().then(Instant::now);
    let serialized_changes = if probe.enabled() {
        serialize_change_drafts_with_size_maps(
            &change_report.drafts,
            Some(&old_index.subtree_sizes),
            Some(&new_index.subtree_sizes),
        )
    } else {
        serialize_change_drafts_fast(&change_report.drafts)
    };
    probe.push_elapsed("rust_change_draft_serialization", serialization_started);
    let changes = serialized_changes.changes;
    let final_change_count = changes.len();
    let final_add_delete_noise = add_delete_noise_count_drafts(&change_report.drafts);
    change_report.diagnostics.serialized_final_change_count = final_change_count;
    change_report.diagnostics.json_nodes_serialized_count =
        serialized_changes.json_nodes_serialized_count;
    matching_report.diagnostics.initial_change_count = initial_change_count;
    matching_report.diagnostics.final_change_count = final_change_count;
    matching_report.diagnostics.refinement_added_count =
        final_change_count.saturating_sub(initial_change_count);
    matching_report.diagnostics.refinement_removed_count =
        initial_change_count.saturating_sub(final_change_count);
    matching_report.diagnostics.suppressed_add_delete_noise =
        initial_add_delete_noise.saturating_sub(final_add_delete_noise);
    matching_report.diagnostics.unmatched_add_delete_noise = final_add_delete_noise;
    matching_report.diagnostics.edit_script = change_report.diagnostics;
    let entity_fast_path = matching_report.diagnostics.as_entity_fast_path_metadata();

    if file_lifecycle == "modified" && changes.is_empty() && old_source != new_source {
        // Issue #51: an empty change list on differing sources is STYLE-ONLY only when the
        // SEMANTIC trees are actually hash-equal (whitespace-only edits on routes whose
        // upstream shortcut is position-sensitive land here legitimately). When the trees
        // DIFFER, suppression reduced a real semantic delta to zero — that must never be
        // presented as style equivalence (the #41 pass -> print false negative rode the old
        // blanket claim, with fabricated source-equivalence evidence).
        let style_equivalent = old_tree.structural_hash == new_tree.structural_hash
            || whitespace_normalized_tree_hash(&old_tree)
                == whitespace_normalized_tree_hash(&new_tree);
        if style_equivalent {
            review_finalization.is_style_only = true;
            if let Ok(style_evidence) = rust_build_style_only_evidence(&json!({
                "mode": "style_only",
                "old_source": old_source,
                "new_source": new_source,
                "language": "python",
            })) {
                if let Some(groups) = style_evidence
                    .get("change_groups")
                    .and_then(Value::as_array)
                {
                    review_finalization
                        .change_groups
                        .extend(groups.iter().cloned());
                }
                if let Some(ignored) = style_evidence
                    .get("ignored_style_changes")
                    .and_then(Value::as_array)
                {
                    review_finalization
                        .ignored_style_changes
                        .extend(ignored.iter().cloned());
                }
            }
        } else {
            review_finalization.is_style_only = false;
            review_finalization.no_surviving_changes = true;
        }
    }

    let mut change_groups = review_finalization.change_groups;
    change_groups.extend(final_change_groups_from_drafts(&change_report.drafts));
    let has_semantic_changes = !changes.is_empty();
    let phases = probe.phases();
    let mut diff = semantic_diff_payload(
        old_filename,
        new_filename,
        changes,
        has_semantic_changes,
        status,
        json!({
            "engine": BATCH_ENGINE,
            "rust_core_stage": if certified_product { "batch_final_diff" } else { "candidate_final_diff" },
            "boundary": "source_batch_to_final_diff",
            "certification": certification,
            "candidate_certification": if certified_product {
                ""
            } else if use_native_parser {
                PYTHON_NATIVE_V4K_CERTIFICATION
            } else {
                PYTHON_V4E_CERTIFICATION
            },
            "old_nodes": old_count,
            "new_nodes": new_count,
            "old_cst_nodes": old_cst_nodes,
            "new_cst_nodes": new_cst_nodes,
            "matching_pairs": matching.len(),
            "adaptive_fuel": adaptive_fuel,
            "python_parser_backend": python_parser_backend,
            "trust_tier": trust_tier,
            "native_vs_wasm_signature_status": if use_native_parser { "not_compared_in_hot_path" } else { "" },
            "wasm_boundary": wasm_boundary,
            "wasm_component_cache_hit": wasm_cache_hit,
            "wasm_component_cache_key": wasm_cache_key,
            "wasm_component_batch_preloaded": wasm_batch_preloaded,
            "entity_fast_path": entity_fast_path,
            "phase_timings": phases,
            "note": if certified_product && use_native_parser {
                "V4-K-B certified native Python final diff produced through the Rust batch boundary."
            } else if certified_product {
                "V4-E certified Python final diff produced through the Rust batch boundary."
            } else if use_native_parser {
                "V4-K native Python final-diff candidate is internal benchmark evidence."
            } else {
                "V4-E final-diff candidate is internal benchmark evidence."
            },
        }),
    );
    diff["metadata"]["rust_core"]["engine"] = json!(BATCH_ENGINE);
    diff["metadata"]["rust_core"]["used"] = json!(true);
    diff["metadata"]["rust_phase_timings"] =
        diff["metadata"]["rust_core"]["details"]["phase_timings"].clone();
    diff["change_groups"] = Value::Array(change_groups);
    attach_scope_trails_metadata(&mut diff, &change_report.drafts, &old_index, &new_index);
    diff["is_style_only"] = json!(review_finalization.is_style_only);
    if review_finalization.no_surviving_changes {
        diff["metadata"]["no_surviving_changes"] = json!(true);
    }
    if !review_finalization.ignored_style_changes.is_empty() {
        diff["metadata"]["ignored_style_changes"] =
            Value::Array(review_finalization.ignored_style_changes);
    }
    apply_file_lifecycle_to_diff(&mut diff, file_lifecycle);
    Ok(diff)
}

pub(crate) fn diff_batch_impl(request_json: &str) -> Result<Value, String> {
    let mut probe = PhaseProbe::default();
    let request: Value = probe.measure("rust_batch_request_decode", || {
        serde_json::from_str(request_json).map_err(|exc| format!("parse batch request: {exc}"))
    })?;
    diff_batch_impl_from_request(&request, probe)
}

fn diff_batch_impl_from_request(request: &Value, mut probe: PhaseProbe) -> Result<Value, String> {
    let schema_version = request
        .get("schema_version")
        .or_else(|| request.get("schemaVersion"))
        .and_then(Value::as_u64)
        .unwrap_or(1);
    if schema_version != 1 {
        return Err(format!(
            "unsupported batch schema version: {schema_version}"
        ));
    }
    let candidate_mode = request
        .get("candidate")
        .or_else(|| request.get("candidateMode"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let python_parser_backend = python_parser_backend_from_request(&request)?;
    let parallel = request
        .get("parallel")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let max_workers = request
        .get("max_workers")
        .or_else(|| request.get("maxWorkers"))
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0);
    let files = request
        .get("files")
        .and_then(Value::as_array)
        .ok_or_else(|| "batch request requires a files array".to_owned())?;
    let config_json = request
        .get("config")
        .map(Value::to_string)
        .unwrap_or_else(|| "{}".to_owned());
    let config = RustCoreConfig::from_json(&config_json);
    probe.set_enabled(config.profile_phases);
    let metered = config.plugin_fuel != u64::MAX;
    let preloads = Arc::new(probe.measure("rust_wasm_batch_component_preload", || {
        if python_parser_backend == PYTHON_PARSER_BACKEND_NATIVE {
            Ok(BatchComponentPreloads::empty())
        } else {
            Ok(preload_batch_components(files, metered))
        }
    })?);
    let (effective_workers, custom_pool) = probe.measure("rust_batch_schedule", || {
        let effective_workers = if parallel && files.len() > 1 {
            max_workers.unwrap_or_else(rayon::current_num_threads)
        } else {
            1
        };
        let custom_pool = if parallel && files.len() > 1 {
            if let Some(workers) = max_workers {
                Some(
                    ThreadPoolBuilder::new()
                        .num_threads(workers)
                        .build()
                        .map_err(|exc| format!("build Rust batch worker pool: {exc}"))?,
                )
            } else {
                None
            }
        } else {
            None
        };
        Ok((effective_workers, custom_pool))
    })?;
    let outcomes: Vec<BatchItemResult> = probe.measure("rust_batch_file_execution", || {
        if parallel && files.len() > 1 {
            if let Some(pool) = custom_pool.as_ref() {
                Ok(pool.install(|| {
                    files
                        .par_iter()
                        .enumerate()
                        .map(|(index, file)| {
                            timed_diff_batch_file_item(
                                index,
                                file,
                                &config_json,
                                candidate_mode,
                                &preloads,
                                &python_parser_backend,
                                config.profile_phases,
                            )
                        })
                        .collect()
                }))
            } else {
                Ok(files
                    .par_iter()
                    .enumerate()
                    .map(|(index, file)| {
                        timed_diff_batch_file_item(
                            index,
                            file,
                            &config_json,
                            candidate_mode,
                            &preloads,
                            &python_parser_backend,
                            config.profile_phases,
                        )
                    })
                    .collect())
            }
        } else {
            Ok(files
                .iter()
                .enumerate()
                .map(|(index, file)| {
                    timed_diff_batch_file_item(
                        index,
                        file,
                        &config_json,
                        candidate_mode,
                        &preloads,
                        &python_parser_backend,
                        config.profile_phases,
                    )
                })
                .collect())
        }
    })?;

    let assembly = probe.measure("rust_batch_response_assembly", || {
        let mut diff_items = Vec::with_capacity(files.len());
        let mut complete_count = 0usize;
        let mut candidate_count = 0usize;
        let mut fallback_count = 0usize;
        let mut cache_hits = 0usize;
        let mut file_timings = Vec::with_capacity(files.len());
        for outcome in outcomes {
            complete_count += outcome.complete_count;
            candidate_count += outcome.candidate_count;
            fallback_count += outcome.fallback_count;
            cache_hits += outcome.cache_hit_count;
            let index = batch_diff_item_sort_key(&outcome.item);
            let old_filename = outcome
                .item
                .get("old_filename")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let status = outcome
                .item
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            file_timings.push(json!({
                "index": index,
                "old_filename": old_filename,
                "status": status,
                "duration_ms": outcome.duration_ms,
            }));
            diff_items.push(outcome.item);
        }

        diff_items.sort_by_key(batch_diff_item_sort_key);
        file_timings.sort_by(|left, right| {
            right
                .get("duration_ms")
                .and_then(Value::as_f64)
                .unwrap_or(0.0)
                .partial_cmp(
                    &left
                        .get("duration_ms")
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0),
                )
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let total_ms: f64 = file_timings
            .iter()
            .filter_map(|item| item.get("duration_ms").and_then(Value::as_f64))
            .sum();
        let count = file_timings.len();
        let max_ms = file_timings
            .first()
            .and_then(|item| item.get("duration_ms"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let min_ms = file_timings
            .iter()
            .filter_map(|item| item.get("duration_ms").and_then(Value::as_f64))
            .fold(None, |acc: Option<f64>, value| {
                Some(acc.map_or(value, |current| current.min(value)))
            })
            .unwrap_or(0.0);
        let slowest_files: Vec<Value> = file_timings.into_iter().take(5).collect();
        let file_timing = json!({
            "count": count,
            "total_ms": total_ms,
            "average_ms": if count > 0 { total_ms / count as f64 } else { 0.0 },
            "max_ms": max_ms,
            "min_ms": min_ms,
            "slowest_files": slowest_files,
        });
        Ok(BatchAssembly {
            diff_items,
            complete_count,
            candidate_count,
            fallback_count,
            cache_hits,
            file_timing,
        })
    })?;

    let status = if assembly.complete_count == files.len() {
        COMPLETE
    } else if assembly.candidate_count > 0
        && assembly.candidate_count + assembly.complete_count == files.len()
    {
        CANDIDATE
    } else if assembly.candidate_count > 0 {
        PARTIAL
    } else if assembly.complete_count > 0 {
        PARTIAL
    } else {
        FALLBACK
    };
    Ok(json!({
        "schema_version": 1,
        "status": status,
        "engine": BATCH_ENGINE,
        "diffs": assembly.diff_items,
        "metadata": {
            "rust_core_stage": "batch_boundary",
            "boundary": "source_batch_to_final_diff",
            "supported_languages": ["python"],
            "candidate_mode": candidate_mode,
            "python_parser_backend": python_parser_backend,
            "certification": if !candidate_mode && python_parser_backend == PYTHON_PARSER_BACKEND_NATIVE {
                PYTHON_NATIVE_V4KB_CERTIFICATION
            } else {
                PYTHON_V4E_CERTIFICATION
            },
            "trust_tier": if python_parser_backend == PYTHON_PARSER_BACKEND_NATIVE {
                "first_party_core_builder"
            } else {
                "sandboxed_wasm_plugin"
            },
            "batch_size": files.len(),
            "parallel": parallel,
            "parallel_workers": effective_workers,
            "cache_hits": assembly.cache_hits,
            "batch_component_cache_hits": preloads.cache_hits,
            "batch_component_cache_misses": preloads.cache_misses,
            "batch_component_preload_errors": preloads.error_count,
            "complete_count": assembly.complete_count,
            "candidate_count": assembly.candidate_count,
            "fallback_count": assembly.fallback_count,
            "file_timing": assembly.file_timing,
            "phase_timings": probe.phases(),
            "note": "This is the single planned Rust engine boundary. It is intentionally conservative until changed-file parity is certified.",
        },
    }))
}

pub(crate) fn diff_batch_commit_json_impl(
    request_json: &str,
) -> Result<(Value, Option<Vec<u8>>), String> {
    let mut probe = PhaseProbe::default();
    let request: Value = probe.measure("rust_commit_json_request_decode", || {
        serde_json::from_str(request_json)
            .map_err(|exc| format!("parse commit-json request: {exc}"))
    })?;
    let config_json = request
        .get("config")
        .map(Value::to_string)
        .unwrap_or_else(|| "{}".to_owned());
    let config = RustCoreConfig::from_json(&config_json);
    probe.set_enabled(config.profile_phases);
    let python_parser_backend = python_parser_backend_from_request(&request)?;
    if python_parser_backend != PYTHON_PARSER_BACKEND_NATIVE {
        return Ok((
            commit_json_control(
                FALLBACK,
                "certified commit JSON requires native first-party Python backend",
                &probe,
                None,
                None,
                0,
                0,
            ),
            None,
        ));
    }
    let batch = diff_batch_impl_from_request(&request, PhaseProbe::default())?;
    commit_json_from_batch(&request, batch, &mut probe, &config)
}

pub(crate) fn diff_working_tree_python_commit_json_impl(
    request_json: &str,
) -> Result<(Value, Option<Vec<u8>>), String> {
    let mut probe = PhaseProbe::default();
    let mut request: Value = probe.measure("rust_working_tree_request_decode", || {
        serde_json::from_str(request_json)
            .map_err(|exc| format!("parse working-tree request: {exc}"))
    })?;
    let config_json = request
        .get("config")
        .map(Value::to_string)
        .unwrap_or_else(|| "{}".to_owned());
    let config = RustCoreConfig::from_json(&config_json);
    probe.set_enabled(config.profile_phases);
    let python_parser_backend = python_parser_backend_from_request(&request)?;
    if python_parser_backend != PYTHON_PARSER_BACKEND_NATIVE {
        return Ok((
            commit_json_control(
                FALLBACK,
                "certified working-tree JSON requires native first-party Python backend",
                &probe,
                None,
                None,
                0,
                0,
            ),
            None,
        ));
    }
    let new_ref = request
        .get("new_ref")
        .or_else(|| request.get("newRef"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if !new_ref.is_empty() {
        return Ok((
            commit_json_control(
                FALLBACK,
                "certified Rust working-tree collector only supports an empty new_ref",
                &probe,
                None,
                None,
                0,
                0,
            ),
            None,
        ));
    }
    let repo_path = request
        .get("repo_path")
        .or_else(|| request.get("repoPath"))
        .and_then(Value::as_str)
        .ok_or_else(|| "working-tree request requires repo_path".to_owned())?;
    let old_ref = request
        .get("old_ref")
        .or_else(|| request.get("oldRef"))
        .and_then(Value::as_str)
        .unwrap_or("HEAD");
    let files = probe.measure("rust_working_tree_source_collection", || {
        collect_working_tree_python_files_rust(repo_path, old_ref, config.max_cst_bytes)
    })?;
    if files.is_empty() {
        let payload = CertifiedCommitDiffPayload {
            old_ref,
            new_ref: "",
            guardrail_violations: Vec::new(),
            file_diffs: Vec::new(),
            cross_file_changes: Vec::new(),
            parse_errors: Vec::new(),
        };
        let commit_json = serde_json::to_vec(&payload)
            .map_err(|exc| format!("serialize empty CommitDiff JSON: {exc}"))?;
        let byte_size = commit_json.len();
        return Ok((
            commit_json_control(COMPLETE, "", &probe, None, None, byte_size, 0),
            Some(commit_json),
        ));
    }
    if let Some(object) = request.as_object_mut() {
        object.insert("files".to_owned(), Value::Array(files));
        object.insert(
            "python_parser_backend".to_owned(),
            Value::String(PYTHON_PARSER_BACKEND_NATIVE.to_owned()),
        );
    }
    commit_json_from_request_files_direct(
        &request,
        &mut probe,
        &config,
        PYTHON_PARSER_BACKEND_NATIVE,
    )
}

mod commit_plumbing;
use commit_plumbing::*;


mod certified_validation;
use certified_validation::*;


struct BatchItemResult {
    item: Value,
    complete_count: usize,
    candidate_count: usize,
    fallback_count: usize,
    cache_hit_count: usize,
    duration_ms: f64,
}

#[derive(Serialize)]
struct CertifiedCommitDiffPayload<'a> {
    old_ref: &'a str,
    new_ref: &'a str,
    guardrail_violations: Vec<Value>,
    file_diffs: Vec<Value>,
    cross_file_changes: Vec<Value>,
    parse_errors: Vec<Value>,
}

struct BatchAssembly {
    diff_items: Vec<Value>,
    complete_count: usize,
    candidate_count: usize,
    fallback_count: usize,
    cache_hits: usize,
    file_timing: Value,
}

struct BatchComponentPreloads {
    entries: HashMap<String, BatchComponentPreloadEntry>,
    cache_hits: usize,
    cache_misses: usize,
    error_count: usize,
}

struct BatchComponentPreloadEntry {
    lookup: Option<ParserComponentLookup>,
    error: Option<String>,
}

impl BatchComponentPreloads {
    fn empty() -> Self {
        Self {
            entries: HashMap::new(),
            cache_hits: 0,
            cache_misses: 0,
            error_count: 0,
        }
    }

    fn lookup(&self, wasm_path: &str) -> Result<Option<&ParserComponentLookup>, String> {
        if wasm_path.is_empty() {
            return Ok(None);
        }
        let Some(entry) = self.entries.get(wasm_path) else {
            return Ok(None);
        };
        if let Some(error) = &entry.error {
            return Err(error.clone());
        }
        Ok(entry.lookup.as_ref())
    }
}

fn python_parser_backend_from_request(request: &Value) -> Result<String, String> {
    let backend = request
        .get("python_parser_backend")
        .or_else(|| request.get("pythonParserBackend"))
        .and_then(Value::as_str)
        .unwrap_or(PYTHON_PARSER_BACKEND_WASM)
        .to_ascii_lowercase();
    match backend.as_str() {
        PYTHON_PARSER_BACKEND_WASM | PYTHON_PARSER_BACKEND_NATIVE => Ok(backend),
        _ => Err(format!("unsupported Python parser backend: {backend}")),
    }
}

fn preload_batch_components(files: &[Value], metered: bool) -> BatchComponentPreloads {
    let mut entries = HashMap::new();
    let mut cache_hits = 0usize;
    let mut cache_misses = 0usize;
    let mut error_count = 0usize;
    for file in files {
        let old_source = value_str(file, "old_source", "oldSource").unwrap_or_default();
        let new_source = value_str(file, "new_source", "newSource").unwrap_or_default();
        if old_source == new_source {
            continue;
        }
        let language = value_str(file, "language", "language").unwrap_or("python");
        if !language.eq_ignore_ascii_case("python") {
            continue;
        }
        let parser_plugin_id = value_str(file, "parser_plugin_id", "parserPluginId").unwrap_or("");
        if !is_supported_python_plugin_id(&parser_plugin_id) {
            continue;
        }
        let wasm_path = value_str(file, "parser_wasm_path", "parserWasmPath").unwrap_or("");
        if wasm_path.is_empty() || entries.contains_key(wasm_path) {
            continue;
        }
        let entry = if !Path::new(&wasm_path).exists() {
            error_count += 1;
            BatchComponentPreloadEntry {
                lookup: None,
                error: Some(format!("parser wasm path not found: {wasm_path}")),
            }
        } else {
            match cached_parser_component(&wasm_path, metered) {
                Ok(lookup) => {
                    if lookup.cache_hit {
                        cache_hits += 1;
                    } else {
                        cache_misses += 1;
                    }
                    BatchComponentPreloadEntry {
                        lookup: Some(lookup),
                        error: None,
                    }
                }
                Err(reason) => {
                    error_count += 1;
                    BatchComponentPreloadEntry {
                        lookup: None,
                        error: Some(reason),
                    }
                }
            }
        };
        entries.insert(wasm_path.to_owned(), entry);
    }
    BatchComponentPreloads {
        entries,
        cache_hits,
        cache_misses,
        error_count,
    }
}

/// Attach the detected content type (magic bytes) to a batch diff's `metadata`,
/// so the default single-file path matches the Python `_run_pipeline` enrichment.
fn attach_content_type_metadata(diff: &mut Value, old_source: &str, new_source: &str) {
    let source = if new_source.is_empty() { old_source } else { new_source };
    let bytes = source.as_bytes();
    let head = &bytes[..bytes.len().min(8192)];
    let value = match serde_json::to_value(content_type::detect_content_type(head)) {
        Ok(value) => value,
        Err(_) => return,
    };
    if let Some(Value::Object(metadata)) = diff.get_mut("metadata") {
        metadata.insert("content_type".to_string(), value);
    } else if let Some(object) = diff.as_object_mut() {
        object.insert("metadata".to_string(), json!({ "content_type": value }));
    }
}

fn diff_batch_file_item(
    index: usize,
    file: &Value,
    config_json: &str,
    candidate_mode: bool,
    preloads: &BatchComponentPreloads,
    python_parser_backend: &str,
) -> BatchItemResult {
    let old_filename = value_str(file, "old_filename", "oldFilename").unwrap_or("old.py");
    let new_filename = value_str(file, "new_filename", "newFilename").unwrap_or(old_filename);
    let language = value_str(file, "language", "language").unwrap_or("python");
    if !language.eq_ignore_ascii_case("python") {
        return BatchItemResult::fallback(batch_fallback_item(
            index,
            old_filename,
            new_filename,
            "unsupported language",
        ));
    }
    let parser_plugin_id = value_str(file, "parser_plugin_id", "parserPluginId").unwrap_or("");
    if !is_supported_python_plugin_id(&parser_plugin_id) {
        return BatchItemResult::fallback(batch_fallback_item(
            index,
            old_filename,
            new_filename,
            "unsupported parser plugin",
        ));
    }
    let old_source = value_str(file, "old_source", "oldSource").unwrap_or("");
    let new_source = value_str(file, "new_source", "newSource").unwrap_or("");
    let parser_wasm_path = value_str(file, "parser_wasm_path", "parserWasmPath").unwrap_or("");
    let file_lifecycle = infer_file_lifecycle_from_file(file);

    if old_source == new_source {
        let certification = if python_parser_backend == PYTHON_PARSER_BACKEND_NATIVE {
            PYTHON_NATIVE_V4KB_CERTIFICATION
        } else {
            PYTHON_V4E_CERTIFICATION
        };
        let trust_tier = if python_parser_backend == PYTHON_PARSER_BACKEND_NATIVE {
            "first_party_core_builder"
        } else {
            "sandboxed_wasm_plugin"
        };
        let wasm_boundary = if python_parser_backend == PYTHON_PARSER_BACKEND_NATIVE {
            "bypassed_first_party_native_python"
        } else {
            "rust_wasmtime"
        };
        let mut diff = semantic_diff_payload_with_style(
            old_filename,
            new_filename,
            Vec::new(),
            false,
            true,
            COMPLETE,
            json!({
                "engine": BATCH_ENGINE,
                "rust_core_stage": "batch_final_diff",
                "boundary": "source_batch_to_final_diff",
                "certification": certification,
                "python_parser_backend": python_parser_backend,
                "trust_tier": trust_tier,
                "wasm_boundary": wasm_boundary,
                "note": "Rust batch boundary returned final diff JSON for a no-change Python pair."
            }),
        );
        diff["metadata"]["rust_core"]["engine"] = json!(BATCH_ENGINE);
        attach_content_type_metadata(&mut diff, old_source, new_source);
        apply_file_lifecycle_to_diff(&mut diff, file_lifecycle);
        return BatchItemResult::complete(
            json!({
                "index": index,
                "old_filename": old_filename,
                "new_filename": new_filename,
                "language": "python",
                "status": COMPLETE,
                "diff": diff,
            }),
            false,
        );
    }

    if candidate_mode {
        let preload = if python_parser_backend == PYTHON_PARSER_BACKEND_NATIVE {
            None
        } else {
            match preloads.lookup(&parser_wasm_path) {
                Ok(preload) => preload,
                Err(reason) => {
                    return BatchItemResult::fallback(batch_fallback_item(
                        index,
                        old_filename,
                        new_filename,
                        &reason,
                    ));
                }
            }
        };
        return match diff_python_sources_final_impl(
            &old_source,
            &new_source,
            old_filename,
            new_filename,
            parser_wasm_path,
            config_json,
            CANDIDATE,
            false,
            preload,
            python_parser_backend,
            file_lifecycle,
        ) {
            Ok(candidate_diff) => {
                let phase_timings = candidate_diff
                    .get("metadata")
                    .and_then(|metadata| metadata.get("rust_phase_timings"))
                    .cloned()
                    .unwrap_or_else(|| json!([]));
                let signature = candidate_signature_for_diff(&candidate_diff);
                let cache_hit = rust_diff_cache_hit(&candidate_diff);
                BatchItemResult::candidate(
                    json!({
                        "index": index,
                        "old_filename": old_filename,
                        "new_filename": new_filename,
                        "language": "python",
                        "status": CANDIDATE,
                        "candidate_diff": candidate_diff,
                        "candidate_signature": signature,
                        "phase_timings": phase_timings,
                        "candidate_note": "candidate remains benchmark/parity evidence",
                    }),
                    cache_hit,
                )
            }
            Err(reason) => BatchItemResult::fallback(batch_fallback_item(
                index,
                old_filename,
                new_filename,
                &reason,
            )),
        };
    }

    let preload = if python_parser_backend == PYTHON_PARSER_BACKEND_NATIVE {
        None
    } else {
        match preloads.lookup(&parser_wasm_path) {
            Ok(preload) => preload,
            Err(reason) => {
                return BatchItemResult::fallback(batch_fallback_item(
                    index,
                    old_filename,
                    new_filename,
                    &reason,
                ));
            }
        }
    };
    match diff_python_sources_final_impl(
        &old_source,
        &new_source,
        old_filename,
        new_filename,
        parser_wasm_path,
        config_json,
        COMPLETE,
        true,
        preload,
        python_parser_backend,
        file_lifecycle,
    ) {
        Ok(mut diff) => {
            let phase_timings = diff
                .get("metadata")
                .and_then(|metadata| metadata.get("rust_phase_timings"))
                .cloned()
                .unwrap_or_else(|| json!([]));
            let cache_hit = rust_diff_cache_hit(&diff);
            attach_content_type_metadata(&mut diff, old_source, new_source);
            BatchItemResult::complete(
                json!({
                    "index": index,
                    "old_filename": old_filename,
                    "new_filename": new_filename,
                    "language": "python",
                    "status": COMPLETE,
                    "diff": diff,
                    "phase_timings": phase_timings,
                }),
                cache_hit,
            )
        }
        Err(reason) => BatchItemResult::fallback(batch_fallback_item(
            index,
            old_filename,
            new_filename,
            &reason,
        )),
    }
}

fn timed_diff_batch_file_item(
    index: usize,
    file: &Value,
    config_json: &str,
    candidate_mode: bool,
    preloads: &BatchComponentPreloads,
    python_parser_backend: &str,
    timing_enabled: bool,
) -> BatchItemResult {
    if !timing_enabled {
        return diff_batch_file_item(
            index,
            file,
            config_json,
            candidate_mode,
            preloads,
            python_parser_backend,
        );
    }
    let started = Instant::now();
    let mut result = diff_batch_file_item(
        index,
        file,
        config_json,
        candidate_mode,
        preloads,
        python_parser_backend,
    );
    let duration_ms = started.elapsed().as_secs_f64() * 1000.0;
    result.duration_ms = duration_ms;
    if let Some(item) = result.item.as_object_mut() {
        item.insert("execution_duration_ms".to_owned(), json!(duration_ms));
    }
    result
}

impl BatchItemResult {
    fn complete(item: Value, cache_hit: bool) -> Self {
        Self {
            item,
            complete_count: 1,
            candidate_count: 0,
            fallback_count: 0,
            cache_hit_count: usize::from(cache_hit),
            duration_ms: 0.0,
        }
    }

    fn candidate(item: Value, cache_hit: bool) -> Self {
        Self {
            item,
            complete_count: 0,
            candidate_count: 1,
            fallback_count: 0,
            cache_hit_count: usize::from(cache_hit),
            duration_ms: 0.0,
        }
    }

    fn fallback(item: Value) -> Self {
        Self {
            item,
            complete_count: 0,
            candidate_count: 0,
            fallback_count: 1,
            cache_hit_count: 0,
            duration_ms: 0.0,
        }
    }
}

fn rust_diff_cache_hit(diff: &Value) -> bool {
    diff.pointer("/metadata/rust_core/details/wasm_component_cache_hit")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn batch_diff_item_sort_key(item: &Value) -> u64 {
    item.get("index")
        .and_then(Value::as_u64)
        .unwrap_or(u64::MAX)
}

/// Distributions that ship the FIRST-PARTY Python parser.
///
/// A catalogued plugin id is qualified by the DISTRIBUTION name, and the Python binding is
/// published as `intentumdiff-python` while its import package is `intentumdiff` — so the same
/// certified parser arrives spelled two ways depending on how it was catalogued.
///
/// Accepting only `intentumdiff:...` made the core reject the certified parser as unsupported,
/// so the certified batch path declined and execution fell through to routed finalize. That
/// fallthrough is Rust→Rust, so no engine gate fired: the diff was correct and the engine was
/// Rust, while the certification and the facts only that path derives were silently absent.
///
/// An explicit allowlist, never a prefix or suffix match — accepting an arbitrary
/// `<dist>:python:python` would let a third-party plugin claim the certified path.
const PYTHON_PLUGIN_DISTRIBUTIONS: [&str; 3] =
    ["intentumdiff", "intentumdiff-python", "intentumdiff_python"];

fn is_supported_python_plugin_id(plugin_id: &str) -> bool {
    if matches!(plugin_id, "" | "python" | "python-parser" | "python_parser") {
        return true;
    }
    match plugin_id.split(':').collect::<Vec<_>>()[..] {
        [dist, "python", "python"] => PYTHON_PLUGIN_DISTRIBUTIONS.contains(&dist),
        _ => false,
    }
}

#[cfg(test)]
mod python_plugin_id_tests {
    use super::*;

    #[test]
    fn the_distribution_qualified_id_is_certified() {
        // THE bug. The binding publishes as `intentumdiff-python`, so this is the spelling the
        // core actually receives once parsers can be catalogued at all. Rejecting it made the
        // certified batch path decline with "unsupported parser plugin".
        assert!(is_supported_python_plugin_id("intentumdiff-python:python:python"));
        assert!(is_supported_python_plugin_id("intentumdiff_python:python:python"));
    }

    #[test]
    fn the_import_package_spelling_still_works() {
        assert!(is_supported_python_plugin_id("intentumdiff:python:python"));
    }

    #[test]
    fn the_bare_and_absent_spellings_still_work() {
        for id in ["", "python", "python-parser", "python_parser"] {
            assert!(is_supported_python_plugin_id(id), "{id:?}");
        }
    }

    #[test]
    fn a_third_party_plugin_cannot_claim_the_certified_path() {
        // The reason this is an allowlist rather than a `*:python:python` pattern. Certification
        // is a trust statement; anyone could name their distribution to match a loose rule.
        for id in [
            "evil:python:python",
            "intentumdiff-evil:python:python",
            "notintentumdiff:python:python",
        ] {
            assert!(!is_supported_python_plugin_id(id), "{id:?} must not certify");
        }
    }

    #[test]
    fn a_first_party_distribution_shipping_another_language_is_not_python() {
        assert!(!is_supported_python_plugin_id("intentumdiff-python:ruby:ruby"));
        assert!(!is_supported_python_plugin_id("intentumdiff:python"));
        assert!(!is_supported_python_plugin_id("intentumdiff:python:python:extra"));
    }
}

fn value_str<'a>(value: &'a Value, snake: &str, camel: &str) -> Option<&'a str> {
    value
        .get(snake)
        .or_else(|| value.get(camel))
        .and_then(Value::as_str)
}

fn batch_fallback_item(
    index: usize,
    old_filename: &str,
    new_filename: &str,
    reason: &str,
) -> Value {
    json!({
        "index": index,
        "old_filename": old_filename,
        "new_filename": new_filename,
        "status": FALLBACK,
        "reason": reason,
    })
}

fn parse_python_tree(source: &str) -> Result<tree_sitter::Tree, String> {
    let mut parser = TreeSitterParser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .map_err(|exc| format!("load tree-sitter-python: {exc}"))?;
    parser
        .parse(source, None)
        .ok_or_else(|| "tree-sitter parse returned no tree".to_owned())
}

fn serialize_tree_json(tree: &tree_sitter::Tree, source: &str) -> Result<String, String> {
    let root = serialize_ts_node(tree.root_node(), source.as_bytes())?;
    serde_json::to_string(&root).map_err(|exc| format!("serialize CST: {exc}"))
}

fn serialize_ts_node(node: tree_sitter::Node<'_>, source: &[u8]) -> Result<CstNode, String> {
    let start = node.start_position();
    let end = node.end_position();
    let mut cursor = node.walk();
    let children: Vec<CstNode> = node
        .named_children(&mut cursor)
        .map(|child| serialize_ts_node(child, source))
        .collect::<Result<Vec<_>, _>>()?;
    let keep_text = children.is_empty() || matches!(node.kind(), "string" | "integer" | "float");
    let text = if keep_text {
        node.utf8_text(source)
            .unwrap_or("")
            .chars()
            .take(4096)
            .collect()
    } else {
        String::new()
    };
    // tree-sitter-python encodes `async def` as a plain `function_definition` whose first
    // (anonymous) child is the `async` keyword token. `named_children` drops that token, so
    // `def f()` and `async def f()` produced byte-identical CSTs and the toggle read as
    // STYLE-ONLY — a correctness lie (the call site now gets a coroutine). Surface the
    // distinction via the `async_function_def` vocabulary the engine already understands
    // (NodeFacts `is_async`, entity/definition lists).
    let mut node_type = node.kind().to_owned();
    if node_type == "function_definition"
        && node.child(0).is_some_and(|child| child.kind() == "async")
    {
        node_type = "async_function_def".to_owned();
    }
    Ok(CstNode {
        node_type,
        named: node.is_named(),
        text,
        start_line: start.row as u32,
        start_col: start.column as u32,
        end_line: end.row as u32,
        end_col: end.column as u32,
        children,
    })
}

fn strip_trivia_json(cst_json: &str, trivia_types: &[String]) -> Result<String, String> {
    check_trivia_type_limit(trivia_types)?;
    let root: CstNode = serde_json::from_str(cst_json)
        .map_err(|exc| format!("parse CST for trivia stripping: {exc}"))?;
    let trivia: HashSet<&str> = trivia_types.iter().map(String::as_str).collect();
    let filtered = strip_trivia_node(&root, &trivia)
        .ok_or_else(|| "trivia stripping removed the root node".to_owned())?;
    serde_json::to_string(&filtered).map_err(|exc| format!("serialize filtered CST: {exc}"))
}

fn check_trivia_type_limit(trivia_types: &[String]) -> Result<(), String> {
    if trivia_types.len() > HOST_UTILS_MAX_TRIVIA_TYPES {
        return Err(format!(
            "host-utils trivia type count is {}; limit is {}",
            trivia_types.len(),
            HOST_UTILS_MAX_TRIVIA_TYPES
        ));
    }

    let mut total = 0usize;
    for trivia_type in trivia_types {
        let size = trivia_type.as_bytes().len();
        if size > HOST_UTILS_MAX_TRIVIA_TYPE_BYTES {
            return Err(format!(
                "host-utils trivia type is {size} bytes; limit is {HOST_UTILS_MAX_TRIVIA_TYPE_BYTES} bytes"
            ));
        }
        total += size;
        if total > HOST_UTILS_MAX_TRIVIA_BYTES {
            return Err(format!(
                "host-utils trivia type payload is {total} bytes; limit is {HOST_UTILS_MAX_TRIVIA_BYTES} bytes"
            ));
        }
    }
    Ok(())
}

fn strip_trivia_node(node: &CstNode, trivia: &HashSet<&str>) -> Option<CstNode> {
    if trivia.contains(node.node_type.as_str()) {
        return None;
    }
    let mut copy = node.clone();
    copy.children = node
        .children
        .iter()
        .filter_map(|child| strip_trivia_node(child, trivia))
        .collect();
    Some(copy)
}

fn semantic_hash_json(cst_json: &str) -> Result<String, String> {
    let root: CstNode = serde_json::from_str(cst_json)
        .map_err(|exc| format!("parse CST for structural hash: {exc}"))?;
    Ok(structural_hash_cst(&root))
}

fn count_cst_nodes_json(cst_json: &str) -> Result<usize, String> {
    let root: CstNode =
        serde_json::from_str(cst_json).map_err(|exc| format!("parse CST for node count: {exc}"))?;
    Ok(count_cst_nodes(&root))
}

fn count_cst_nodes(node: &CstNode) -> usize {
    1 + node.children.iter().map(count_cst_nodes).sum::<usize>()
}

fn fuel_budget(floor: u64, candidate: u64) -> u64 {
    if floor == u64::MAX {
        u64::MAX
    } else {
        floor.max(candidate)
    }
}

fn parse_semantic_tree_json(raw: &str) -> Result<SemanticNode, String> {
    let value: Value = serde_json::from_str(raw)
        .map_err(|exc| format!("parse plugin SemanticNode JSON: {exc}"))?;
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        return Err(format!("plugin returned error: {error}"));
    }
    serde_json::from_value(value).map_err(|exc| format!("decode SemanticNode: {exc}"))
}

fn parser_component_cache_key(wasm_path: &str, metered: bool) -> Result<String, String> {
    let path = Path::new(wasm_path);
    let metadata = fs::metadata(path).map_err(|exc| format!("read parser wasm metadata: {exc}"))?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok());
    let (modified_secs, modified_nanos) = modified
        .map(|duration| (duration.as_secs(), duration.subsec_nanos()))
        .unwrap_or((0, 0));
    let resolved = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/");
    Ok(format!(
        "{}|len={}|mtime={}.{}|metered={}",
        resolved,
        metadata.len(),
        modified_secs,
        modified_nanos,
        metered
    ))
}

fn cached_parser_component(
    wasm_path: &str,
    metered: bool,
) -> Result<ParserComponentLookup, String> {
    let cache_key = parser_component_cache_key(wasm_path, metered)?;
    let cache = PARSER_COMPONENT_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    {
        let guard = cache
            .lock()
            .map_err(|_| "parser component cache lock poisoned".to_owned())?;
        if let Some(cached) = guard.get(&cache_key) {
            return Ok(ParserComponentLookup {
                cached: Arc::clone(cached),
                cache_hit: true,
                cache_key,
            });
        }
    }

    let mut cfg = Config::new();
    cfg.consume_fuel(metered);
    cfg.wasm_memory64(false);
    let engine = Engine::new(&cfg).map_err(|exc| format!("create wasmtime engine: {exc}"))?;
    let component = Component::from_file(&engine, wasm_path)
        .map_err(|exc| format!("load parser component: {exc}"))?;
    let cached = Arc::new(CachedParserComponent { engine, component });
    let mut guard = cache
        .lock()
        .map_err(|_| "parser component cache lock poisoned".to_owned())?;
    let cached = guard.entry(cache_key.clone()).or_insert_with(|| cached);
    Ok(ParserComponentLookup {
        cached: Arc::clone(cached),
        cache_hit: false,
        cache_key,
    })
}

/// Parse an old/new source pair through a bundled Wasm parser component into the two semantic
/// tree JSONs. The `run_python_wasm_*` name is historical (the first consumer was the Python
/// wasm backend): the `language` hint passed to `call_process` makes it language-agnostic, so
/// the live-server's non-Python native path (#100, A2.5) reuses it with the resolved language.
/// For full-parse parsers (every non-Python bundled grammar) the `*_filtered_cst` args are
/// unused — the parser reads `*_source` directly — so callers may pass "" for them.
pub(crate) fn run_python_wasm_process_pair(
    wasm_path: &str,
    old_source: &str,
    old_filtered_cst: &str,
    old_filename: &str,
    new_source: &str,
    new_filtered_cst: &str,
    new_filename: &str,
    fuel: u64,
    max_output_bytes: usize,
    language: &str,
) -> Result<(String, String), String> {
    let result = run_python_wasm_process_pair_detailed(
        wasm_path,
        old_source,
        old_filtered_cst,
        old_filename,
        new_source,
        new_filtered_cst,
        new_filename,
        fuel,
        max_output_bytes,
        language,
    )?;
    Ok((result.old_tree, result.new_tree))
}

/// python `_differ_presentation._slice_source_text` (CHAR-based columns for python-string parity).
fn slice_source_text(lines: &[&str], position: &NodePosition) -> String {
    let start_line = position.start_line as usize;
    let end_line = position.end_line as usize;
    if start_line >= lines.len() || end_line < start_line {
        return String::new();
    }
    let char_slice = |line: &str, from: usize, to: Option<usize>| -> String {
        match to {
            Some(t) => line.chars().skip(from).take(t.saturating_sub(from)).collect(),
            None => line.chars().skip(from).collect(),
        }
    };
    if start_line == end_line {
        return char_slice(
            lines[start_line],
            position.start_col as usize,
            Some(position.end_col as usize),
        );
    }
    let mut selected = vec![char_slice(lines[start_line], position.start_col as usize, None)];
    for line in lines
        .iter()
        .take(end_line.min(lines.len()))
        .skip(start_line + 1)
    {
        selected.push((*line).to_owned());
    }
    if end_line < lines.len() {
        selected.push(char_slice(lines[end_line], 0, Some(position.end_col as usize)));
    }
    selected.join("\n")
}

/// python `_differ_presentation._clean_string_literal_label`.
fn clean_string_literal_label(text: &str) -> String {
    let mut value = text.trim();
    while let Some(first) = value.chars().next() {
        if "rRuUbBfF@$".contains(first) {
            value = &value[first.len_utf8()..];
        } else {
            break;
        }
    }
    let chars: Vec<char> = value.chars().collect();
    if chars.len() >= 2
        && chars[0] == chars[chars.len() - 1]
        && matches!(chars[0], '\'' | '"' | '`')
    {
        let inner: String = chars[1..chars.len() - 1].iter().collect();
        return inner.trim().to_owned();
    }
    value.trim().to_owned()
}

/// python `_differ_presentation._enrich_literal_labels` (differ.py stage 4-7): string-literal
/// leaves get their DECODED source value as label (and order_by_clause its "descending" marker).
/// Keyed profile enrichment AND guardrail semantic paths key off these labels — skipping this
/// left native json/yaml pairs unlabeled, which silently produced ZERO guardrail semantic paths
/// (the rule-eval gap found porting native guardrails).
fn enrich_literal_labels_node(node: &SemanticNode, lines: &[&str]) -> SemanticNode {
    let children: Vec<SemanticNode> = node
        .children
        .iter()
        .map(|child| enrich_literal_labels_node(child, lines))
        .collect();
    let mut label = node.label.clone();
    let node_type = node.node_type.to_lowercase();
    if node_type.contains("string") && (node.label == "string" || node.label == "string_literal") {
        let literal = clean_string_literal_label(&slice_source_text(lines, &node.position));
        if !literal.is_empty() {
            label = literal;
        }
    }
    if node_type == "order_by_clause" && (node.label == node.node_type || node.label == node_type) {
        let source_slice = slice_source_text(lines, &node.position).to_lowercase();
        if source_slice.contains("descending") {
            label = format!("{label} descending");
        }
    }
    let mut out = node.clone();
    out.label = label;
    out.children = children;
    out
}

/// Tree-JSON wrapper for `enrich_literal_labels_node`.
fn enrich_literal_labels_json_str(tree_json: &str, source: &str) -> Result<String, String> {
    let tree: SemanticNode =
        serde_json::from_str(tree_json).map_err(|exc| format!("literal enrich tree: {exc}"))?;
    let lines: Vec<&str> = source.lines().collect();
    let enriched = enrich_literal_labels_node(&tree, &lines);
    serde_json::to_string(&enriched).map_err(|exc| format!("literal enrich serialize: {exc}"))
}

/// Merge guardrail violations onto a served diff + recompute `metadata.guardrails` counts,
/// mirroring python `apply_guardrails_to_diff`'s attachment (analysis/guardrails.py:159-171).
pub(crate) fn attach_guardrail_violations(diff: &mut Value, new_violations: Vec<Value>) {
    if new_violations.is_empty() {
        return;
    }
    let mut merged = diff
        .get("guardrail_violations")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    merged.extend(new_violations);
    let immutable = merged
        .iter()
        .filter(|v| v.get("severity").and_then(Value::as_str) == Some("immutable"))
        .count();
    if !diff.get("metadata").is_some_and(Value::is_object) {
        diff["metadata"] = json!({});
    }
    diff["metadata"]["guardrails"] = json!({
        "violation_count": merged.len(),
        "immutable_count": immutable,
    });
    diff["guardrail_violations"] = json!(merged);
}

/// Evaluate protected-path guardrail rules against a served native diff (#100): builds the SAME
/// eval request the Python glue marshals (`analysis/guardrails._evaluate_policy_rules`) from the
/// enriched trees + final changes, runs the A1.3 rule engine, and returns the violations as JSON
/// values. Best-effort: a malformed request yields no violations (the strict policy PARSE in the
/// live-server layer already deferred anything off-spec).
fn evaluate_guardrail_rules_for_diff(
    diff: &Value,
    rules: &[Value],
    language: &str,
    old_filename: &str,
    new_filename: &str,
    old_tree_json: &str,
    new_tree_json: &str,
) -> Vec<Value> {
    let changes: Vec<Value> = diff
        .get("changes")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|c| {
                    json!({
                        "old_node_id": c
                            .get("old_node")
                            .and_then(|n| n.get("id"))
                            .cloned()
                            .unwrap_or(Value::Null),
                        "new_node_id": c
                            .get("new_node")
                            .and_then(|n| n.get("id"))
                            .cloned()
                            .unwrap_or(Value::Null),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let request_value = json!({
        "language": language,
        "old_filename": old_filename,
        "new_filename": new_filename,
        "old_tree": serde_json::from_str::<Value>(old_tree_json).ok(),
        "new_tree": serde_json::from_str::<Value>(new_tree_json).ok(),
        "changes": changes,
        "rules": rules,
    });
    let Ok(request) = serde_json::from_value::<GuardrailEvalRequest>(request_value) else {
        return Vec::new();
    };
    evaluate_guardrail_rules(&request)
        .into_iter()
        .filter_map(|violation| serde_json::to_value(violation).ok())
        .collect()
}

/// python `_differ_presentation._empty_semantic_tree`: the canonical empty tree substituted for
/// an EMPTY source side (file add/delete lifecycle) instead of parsing "" — some Wasm parsers
/// return an in-band error envelope for empty input. Shape + hash recipe match python exactly.
fn empty_semantic_tree_json(language: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("intentumdiff-empty-tree:{language}").as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    json!({
        "id": "0",
        "node_type": "source_file",
        "label": "",
        "position": {"start_line": 0, "start_col": 0, "end_line": 0, "end_col": 0},
        "structural_hash": digest,
        "children": [],
    })
    .to_string()
}

/// Native generic/markdown-as-text diff (#100): the exact Rust review the differ's routed
/// generic branch delegates to (`generic_text_changes_value`), assembled as a SemanticDiff with
/// the stage-12 zero-change style evidence mirrored (differ.py:1918). The audit
/// NOISE_SUPPRESSED group is omitted (it only records the discarded finalize's churn count,
/// which this path never computes).
fn native_generic_text_diff(
    language: &str,
    old_source: &str,
    new_source: &str,
    old_filename: &str,
    new_filename: &str,
) -> Result<Value, String> {
    let changes = generic_text_changes_value(old_source, new_source)
        .ok_or_else(|| "generic text review declined (input too large)".to_owned())?;
    let mut change_groups: Vec<Value> = Vec::new();
    let mut metadata = json!({
        "engine_owner": "rust",
        "semantic_contract": "generic_text_review_native_v1",
        "rust_core": {
            "engine": "generic_text_review_native_v1",
            "stage": "native_live_generic",
            "used": true,
        },
    });
    // Zero net line changes over differing sources = style-only, and the suppression must be
    // RECORDED (stage-12 mirror): the same engine evidence call the differ uses.
    let is_style_only = changes.is_empty();
    if changes.is_empty() && old_source != new_source {
        if let Ok(evidence) = crate::invariance_groups::rust_build_style_only_evidence(&json!({
            "mode": "style_only",
            "old_source": old_source,
            "new_source": new_source,
            "language": language,
        })) {
            if let Some(groups) = evidence.get("change_groups").and_then(Value::as_array) {
                change_groups.extend(groups.iter().cloned());
            }
            if let Some(ignored) = evidence.get("ignored_style_changes").and_then(Value::as_array)
            {
                if !ignored.is_empty() {
                    metadata["ignored_style_changes"] = json!(ignored);
                }
            }
        }
    }
    let has_semantic_changes = !changes.is_empty();
    let mut diff = json!({
        "old_filename": old_filename,
        "new_filename": new_filename,
        "language": language,
        "changes": changes,
        "change_groups": change_groups,
        "has_semantic_changes": has_semantic_changes,
        "is_style_only": is_style_only,
        "parse_errors": [],
        "llm_summary": "",
        "gitignore_excluded": false,
        "is_fallback": false,
        "guardrail_violations": [],
        "metadata": metadata,
    });
    let lifecycle = infer_file_lifecycle(None, old_source, new_source, None);
    apply_file_lifecycle_to_diff(&mut diff, lifecycle);
    Ok(diff)
}

/// Native single-file DIFF for a NON-Python file (#100, A2.5 step d2): the live-server serves
/// the diff from the core, mirroring the differ's routed `_diff_single` branch (differ.py
/// 1740-1946) so the result matches `SemanticDiffer.diff_strings`. Given a resolved full-parse
/// Wasm grammar + language, it parses the pair, runs the SAME finalize + invariance passes the
/// differ routes through, resolves style-only, assembles a `SemanticDiff`, and applies the file
/// lifecycle. Returns the diff `Value` on success, else `Err(reason)` so `live_handle_diff_json`
/// falls back to the Python differ (zero divergence off the covered path).
///
/// Deliberately NOT covered here (the caller falls back instead): an in-effect guardrail policy
/// (`live_handle_diff_json` checks for it — policy discovery is filesystem I/O). Diff-analyzers
/// are optional entry-point plugins with none bundled by default, so they are a no-op for a
/// default install; a third-party analyzer plugin would not be reflected on this path. Markdown
/// is served by this standard chain: since #44 it is a certified routed language (sections/
/// headings are real tree-sitter nodes; the engine's reorder→MOVE promotion and rename grouping
/// produce the section presentation) — the `_differ_presentation.py` markdown section passes only
/// exist for GENERIC-routed md-named files, which the manifest never produces.
/// Parse a single file's *content*, resolving the parser from *path*'s extension against the
/// bundled manifest in *wasm_dir*. Returns JSON `{"language": <id>, "tree": <SemanticNode>}` — the
/// binding-shared parse step the index / cache-warm path needs (there is otherwise no standalone
/// parse entry — parsing only happens inside the diff handlers): non-Python via the resolved Wasm
/// grammar, Python via the native tree-sitter chain (serialize CST → strip trivia → convert).
/// `Err` on an unresolvable extension; empty *content* yields the empty tree for its language.
pub fn parse_to_tree(
    path: &str,
    content: &str,
    config_json: &str,
    wasm_dir: &str,
) -> Result<String, String> {
    let resolved = crate::parser_registry::resolve_parser(path, wasm_dir)
        .ok_or("no bundled parser for this file extension")?;
    let config = RustCoreConfig::from_json(config_json);

    let tree_json: String = if content.is_empty() {
        empty_semantic_tree_json(&resolved.language)
    } else if !resolved.wasm_path.is_empty() {
        // Non-Python: parse via the resolved full-parse Wasm grammar. Both pair sides are the same
        // content (one wasted parse — cheaper than a single-side parser API); take one tree.
        let fuel = fuel_budget(config.plugin_fuel, 20_000_000 + content.len() as u64 * 20_000);
        let (tree, _) = run_python_wasm_process_pair(
            &resolved.wasm_path,
            content,
            "",
            path,
            content,
            "",
            path,
            fuel,
            config.max_plugin_output_bytes,
            &resolved.language,
        )?;
        tree
    } else {
        // Python: the native tree-sitter chain (the same steps the certified batch runs).
        check_byte_limit("source", content, config.max_cst_bytes)?;
        let ts_tree = parse_python_tree(content)?;
        let cst_json = serialize_tree_json(&ts_tree, content)?;
        check_byte_limit("CST JSON", &cst_json, config.max_cst_bytes)?;
        let trivia: Vec<String> = PYTHON_TRIVIA.iter().map(|item| (*item).to_owned()).collect();
        let filtered = strip_trivia_json(&cst_json, &trivia)?;
        let cst: CstNode =
            serde_json::from_str(&filtered).map_err(|exc| format!("CST JSON: {exc}"))?;
        let tree = convert_cst(&cst, "0", None).ok_or("CST produced no semantic tree")?;
        serde_json::to_string(&tree).map_err(|exc| format!("serialize tree: {exc}"))?
    };

    let tree: Value = serde_json::from_str(&tree_json).map_err(|exc| format!("tree: {exc}"))?;
    Ok(json!({ "language": resolved.language, "tree": tree }).to_string())
}

/// python `core.indexer._make_index_key`: `sha256(repo_root \0 commit_sha)` hex — the symbol-index
/// cache key. A native `index` uses this so it warms the SAME entry the differ's cross-file path
/// looks up (kept here as the single source of the key format, matching the retired Python).
pub fn make_index_key(repo_root: &str, commit_sha: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(repo_root.as_bytes());
    hasher.update(b"\0");
    hasher.update(commit_sha.as_bytes());
    hex::encode(hasher.finalize())
}

pub(crate) fn native_wasm_single_diff(
    language: &str,
    parser_wasm_path: &str,
    old_source: &str,
    new_source: &str,
    old_filename: &str,
    new_filename: &str,
    config_json: &str,
    guardrail_rules: Option<&[Value]>,
) -> Result<Value, String> {
    // generic: the differ's routed branch REPLACES parser-token churn wholesale with the Rust
    // text review's line/char spans (normalize_generic_text_for_review is pure delegation to
    // text_review_generic.rs) — so serve the SAME Rust review directly, skipping the throwaway
    // parse+finalize.
    if language == "generic" {
        return native_generic_text_diff(language, old_source, new_source, old_filename, new_filename);
    }
    // NB: guardrails are handled by the caller (`live_handle_diff_json` falls back when a
    // guardrail policy is in effect), so this pure-engine path never attaches violations.
    let config = RustCoreConfig::from_json(config_json);
    check_byte_limit("old source", old_source, config.max_cst_bytes)?;
    check_byte_limit("new source", new_source, config.max_cst_bytes)?;

    // Parse the pair via the resolved full-parse Wasm grammar (the filtered-CST args are unused
    // for full-parse grammars, i.e. every non-Python bundled parser). An EMPTY side (file
    // add/delete lifecycle — e.g. an untracked file's old side) is NOT parsed: some parsers
    // return an in-band error envelope for "" (sql did — the VS Code working-tree review bug).
    // Mirror the differ (`_empty_semantic_tree`, differ.py:1547): substitute the canonical empty
    // tree, and parse only the non-empty side (passed as both pair sides — one wasted parse on a
    // rare edge beats changing the pair API). Finalize's empty-children lifecycle branch then
    // yields the python-parity DELETION+ADDITION shape.
    // Size-scaled budget (the flat 20M floor starved real-world files — a ~25KB .rs blob
    // traps with "all fuel consumed"; the batch scales ~200k fuel per CST node, ~20k/byte).
    let adaptive_fuel = fuel_budget(
        config.plugin_fuel,
        20_000_000 + (old_source.len() + new_source.len()) as u64 * 20_000,
    );
    let (old_tree_json, new_tree_json) = if old_source.is_empty() && new_source.is_empty() {
        (
            empty_semantic_tree_json(language),
            empty_semantic_tree_json(language),
        )
    } else if old_source.is_empty() {
        let (_, new_tree) = run_python_wasm_process_pair(
            parser_wasm_path,
            new_source,
            "",
            new_filename,
            new_source,
            "",
            new_filename,
            adaptive_fuel,
            config.max_plugin_output_bytes,
            language,
        )?;
        (empty_semantic_tree_json(language), new_tree)
    } else if new_source.is_empty() {
        let (old_tree, _) = run_python_wasm_process_pair(
            parser_wasm_path,
            old_source,
            "",
            old_filename,
            old_source,
            "",
            old_filename,
            adaptive_fuel,
            config.max_plugin_output_bytes,
            language,
        )?;
        (old_tree, empty_semantic_tree_json(language))
    } else {
        run_python_wasm_process_pair(
            parser_wasm_path,
            old_source,
            "",
            old_filename,
            new_source,
            "",
            new_filename,
            adaptive_fuel,
            config.max_plugin_output_bytes,
            language,
        )?
    };

    // Stage 4-7 literal-label enrichment (differ.py:1604) BEFORE the profile pass: string
    // literals carry their decoded VALUE as label — keyed enrichment + guardrail semantic paths
    // key off them (unlabeled pairs = zero guardrail paths).
    let old_tree_json = enrich_literal_labels_json_str(&old_tree_json, old_source)
        .map_err(|exc| format!("literal enrich (old): {exc}"))?;
    let new_tree_json = enrich_literal_labels_json_str(&new_tree_json, new_source)
        .map_err(|exc| format!("literal enrich (new): {exc}"))?;
    // Stage-7b profile-label enrichment (differ.py:1611): fill keyed/path/query/statement/resource
    // identity labels from children BEFORE the diff, so e.g. a sql SELECT `term` folds its field
    // identity into one node (otherwise the field surfaces as a spurious second change). No-op for
    // non-profile languages. The differ enriches the trees here too, so finalize sees the same
    // input. identity_fields=None matches the default (no-schema) case the differ passes for
    // json/yaml; a configured schema's identity fields aren't threaded to the live path yet.
    let old_tree_json = enrich_profile_labels_impl(&old_tree_json, old_source, language, None)
        .map_err(|exc| format!("profile enrich (old): {exc}"))?;
    let new_tree_json = enrich_profile_labels_impl(&new_tree_json, new_source, language, None)
        .map_err(|exc| format!("profile enrich (new): {exc}"))?;

    // Tree-diff + finalize in one call — the same core the differ routes through for every
    // certified language (RUST_FINALIZE_LANGUAGES is total). Under the GIL (this is reached from
    // the `live_handle_diff_json` pyfunction), so the PyErr Display is safe to format.
    let finalized_str = finalize_review_impl(
        &old_tree_json,
        &new_tree_json,
        old_source,
        new_source,
        language,
        config_json,
    )
    .map_err(|exc| format!("finalize: {exc}"))?;
    let fin: Value =
        serde_json::from_str(&finalized_str).map_err(|exc| format!("finalize json: {exc}"))?;
    if !fin.get("used").and_then(Value::as_bool).unwrap_or(false) {
        // tree_too_large / declined — the differ degrades to a token fallback here, so defer.
        return Err(format!(
            "finalize declined: {}",
            fin.get("reason").and_then(Value::as_str).unwrap_or("used=false")
        ));
    }

    let mut changes = fin
        .get("changes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut change_groups = fin
        .get("change_groups")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut is_style_only = fin
        .get("is_style_only")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    // Semantic invariances (css color / literal / quote-and-formatting equivalence): the routed
    // short-circuit runs them on the finalized changes (differ.py:1786). No-op when none fire.
    if !changes.is_empty() {
        let inv_request = json!({
            "mode": "apply",
            "changes": changes,
            "old_tree": serde_json::from_str::<Value>(&old_tree_json)
                .map_err(|exc| format!("old tree json: {exc}"))?,
            "new_tree": serde_json::from_str::<Value>(&new_tree_json)
                .map_err(|exc| format!("new tree json: {exc}"))?,
            "old_source": old_source,
            "new_source": new_source,
            "language": language,
        });
        let inv = crate::invariance_groups::rust_apply_invariances_value(&inv_request)?;
        let inv_changes = inv
            .get("changes")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let inv_groups = inv
            .get("change_groups")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if inv_changes.len() != changes.len() || !inv_groups.is_empty() {
            changes = inv_changes;
            change_groups.extend(inv_groups);
            if changes.is_empty() && old_source != new_source {
                is_style_only = true;
            }
        }
    }

    // Stage-12 style-only resolution (differ.py:1876): zero surviving changes with identical or
    // whitespace-collapsed tree-equal sources is a style-only diff for every routed language.
    if changes.is_empty() && !is_style_only {
        let old_tree: SemanticNode =
            serde_json::from_str(&old_tree_json).map_err(|exc| format!("old tree: {exc}"))?;
        let new_tree: SemanticNode =
            serde_json::from_str(&new_tree_json).map_err(|exc| format!("new tree: {exc}"))?;
        is_style_only = old_source == new_source
            || whitespace_normalized_tree_hash(&old_tree)
                == whitespace_normalized_tree_hash(&new_tree);
    }

    let has_semantic_changes = !changes.is_empty() && !is_style_only;
    let mut diff = json!({
        "old_filename": old_filename,
        "new_filename": new_filename,
        "language": language,
        "changes": changes,
        "change_groups": change_groups,
        "has_semantic_changes": has_semantic_changes,
        "is_style_only": is_style_only,
        "parse_errors": [],
        "llm_summary": "",
        "gitignore_excluded": false,
        "is_fallback": false,
        "guardrail_violations": [],
        "metadata": {
            "engine_owner": "rust",
            "semantic_contract": "rust_finalize_review_v1",
            "rust_core": {
                "engine": "rust_finalize_review_v1",
                "stage": "per_stage_finalize_routing_native_live",
                "used": true,
            },
        },
    });

    // File lifecycle (add/delete/modify) — mirrors differ's _apply_file_lifecycle_to_diff.
    let lifecycle = infer_file_lifecycle(None, old_source, new_source, None);
    apply_file_lifecycle_to_diff(&mut diff, lifecycle);
    // Protected-path guardrail rules (#100): evaluated AFTER lifecycle like the differ
    // (apply_guardrails_to_diff runs on the routed diff), using the same enriched trees.
    if let Some(rules) = guardrail_rules {
        if !rules.is_empty() {
            let violations = evaluate_guardrail_rules_for_diff(
                &diff,
                rules,
                language,
                old_filename,
                new_filename,
                &old_tree_json,
                &new_tree_json,
            );
            attach_guardrail_violations(&mut diff, violations);
        }
    }
    Ok(diff)
}

/// Build the trivia-stripped CST JSON for a Python source, as the interpret-cst Python parser
/// consumes it (the batch's wasm-Python path: tree-sitter parse -> serialize -> strip trivia).
/// This is what `differ.parse` feeds the Python parser, so a tree produced from it matches the
/// differ's — the certified native `convert_cst` builder does NOT (its tree lacks the qualified
/// function symbols `build_symbol_table` needs). Used for commit-level cross-file indexing.
fn python_wasm_filtered_cst(source: &str) -> Result<String, String> {
    let ts_tree = parse_python_tree(source)?;
    let cst_json = serialize_tree_json(&ts_tree, source)?;
    let trivia_types: Vec<String> = PYTHON_TRIVIA.iter().map(|item| (*item).to_owned()).collect();
    strip_trivia_json(&cst_json, &trivia_types)
}

/// Commit-level cross-file changes for the native review path (#100 d2-review): mirrors
/// `CommitDiffer` (build a SemanticIndex per side from the changed files, then
/// `detect_cross_file_changes`, which is purely `diff_symbol_tables`). Each file's old and new
/// sides parse to trees the SAME way `differ.parse` does — Python through the bundled Wasm parser
/// in interpret-cst mode (fed the trivia-stripped CST), other languages through their full-parse
/// Wasm parser — so the symbol tables (and thus the cross-file changes) match the differ's. Builds
/// the two symbol tables and diffs them. Best-effort: any parse/build failure just omits that file
/// (or yields an empty list), never raises. *files* are the same commit-batch file objects the
/// per-file diffs used; *wasm_dir* is the bundled parser dir (for the Python parser wasm).
pub(crate) fn compute_commit_cross_file_changes(
    files: &[Value],
    config_json: &str,
    wasm_dir: &str,
) -> Value {
    if wasm_dir.is_empty() {
        return json!([]); // the bundled Python parser is required for Python symbol trees
    }
    let config = RustCoreConfig::from_json(config_json);
    let python_wasm_path = Path::new(wasm_dir)
        .join("python_parser.wasm")
        .to_string_lossy()
        .into_owned();
    let mut old_files: Vec<Value> = Vec::with_capacity(files.len());
    let mut new_files: Vec<Value> = Vec::with_capacity(files.len());
    for f in files {
        let get = |k1: &str, k2: &str| value_str(f, k1, k2).unwrap_or("");
        let (old_c, new_c) = (get("old_source", "oldSource"), get("new_source", "newSource"));
        let (old_p, new_p) = (get("old_filename", "oldFilename"), get("new_filename", "newFilename"));
        let language = get("language", "language");
        let wasm_path = get("parser_wasm_path", "parserWasmPath");
        // Size-scaled like the diff chain — the flat 20M floor trapped on real-world files.
        let fuel = fuel_budget(
            config.plugin_fuel,
            20_000_000 + (old_c.len() + new_c.len()) as u64 * 20_000,
        );
        // Python (empty wasm_path in the manifest -> native diff, but for indexing we need the
        // differ-matching tree): the interpret-cst Wasm parser fed the trivia-stripped CST
        // (computed below for the sides actually parsed). Other languages: full-parse.
        let parser_path = if wasm_path.is_empty() {
            python_wasm_path.as_str()
        } else {
            wasm_path
        };
        // An EMPTY side (file add/delete) is never parsed — some parsers error-envelope on ""
        // (see native_wasm_single_diff). The empty tree has no symbols, so the empty side is
        // simply omitted from its table; the NON-empty side still parses (as both pair sides),
        // keeping move-into-new-file / delete-from-old-file symbols indexed.
        let (old_parse, new_parse): (Option<(&str, &str)>, Option<(&str, &str)>) =
            if old_c.is_empty() && new_c.is_empty() {
                (None, None)
            } else if old_c.is_empty() {
                (None, Some((new_c, new_p)))
            } else if new_c.is_empty() {
                (Some((old_c, old_p)), None)
            } else {
                (Some((old_c, old_p)), Some((new_c, new_p)))
            };
        let both = old_parse.is_some() && new_parse.is_some();
        let (parse_old_src, parse_old_name) =
            old_parse.or(new_parse).unwrap_or(("", ""));
        let (parse_new_src, parse_new_name) =
            new_parse.or(old_parse).unwrap_or(("", ""));
        if parse_old_src.is_empty() && parse_new_src.is_empty() {
            continue;
        }
        let (old_side_cst, new_side_cst) = if wasm_path.is_empty() {
            (
                python_wasm_filtered_cst(parse_old_src).unwrap_or_default(),
                python_wasm_filtered_cst(parse_new_src).unwrap_or_default(),
            )
        } else {
            (String::new(), String::new())
        };
        if let Ok((old_tree, new_tree)) = run_python_wasm_process_pair(
            parser_path,
            parse_old_src,
            &old_side_cst,
            parse_old_name,
            parse_new_src,
            &new_side_cst,
            parse_new_name,
            fuel,
            config.max_plugin_output_bytes,
            language,
        ) {
            if both || old_parse.is_some() {
                if let Ok(tree) = serde_json::from_str::<Value>(&old_tree) {
                    old_files.push(json!({"filename": old_p, "language": language, "tree": tree}));
                }
            }
            if both || new_parse.is_some() {
                if let Ok(tree) = serde_json::from_str::<Value>(&new_tree) {
                    new_files.push(json!({"filename": new_p, "language": language, "tree": tree}));
                }
            }
        }
    }
    let old_table = index_engine_lib::build_symbol_table_impl(&Value::Array(old_files).to_string());
    let new_table = index_engine_lib::build_symbol_table_impl(&Value::Array(new_files).to_string());
    match serde_json::from_str::<Value>(&index_engine_lib::diff_symbol_tables_impl(
        &old_table, &new_table,
    )) {
        Ok(v) if v.is_array() => v,
        _ => json!([]),
    }
}

pub(crate) fn run_python_wasm_process_pair_detailed(
    wasm_path: &str,
    old_source: &str,
    old_filtered_cst: &str,
    old_filename: &str,
    new_source: &str,
    new_filtered_cst: &str,
    new_filename: &str,
    fuel: u64,
    max_output_bytes: usize,
    language: &str,
) -> Result<WasmProcessPair, String> {
    let unlimited = fuel == u64::MAX;
    let lookup = cached_parser_component(wasm_path, !unlimited)?;
    let cached = lookup.cached;
    let (old_tree, new_tree) = run_python_wasm_process_pair_with_cached_component(
        &cached,
        old_source,
        old_filtered_cst,
        old_filename,
        new_source,
        new_filtered_cst,
        new_filename,
        fuel,
        max_output_bytes,
        language,
        None,
    )?;
    Ok(WasmProcessPair {
        old_tree,
        new_tree,
        cache_hit: lookup.cache_hit,
        cache_key: lookup.cache_key,
    })
}

fn run_python_wasm_process_pair_detailed_profiled(
    wasm_path: &str,
    old_source: &str,
    old_filtered_cst: &str,
    old_filename: &str,
    new_source: &str,
    new_filtered_cst: &str,
    new_filename: &str,
    fuel: u64,
    max_output_bytes: usize,
    probe: &mut PhaseProbe,
) -> Result<WasmProcessPair, String> {
    let unlimited = fuel == u64::MAX;
    let lookup = probe.measure("rust_wasm_cache_lookup", || {
        cached_parser_component(wasm_path, !unlimited)
    })?;
    let cached = lookup.cached;
    let (old_tree, new_tree) = run_python_wasm_process_pair_with_cached_component(
        &cached,
        old_source,
        old_filtered_cst,
        old_filename,
        new_source,
        new_filtered_cst,
        new_filename,
        fuel,
        max_output_bytes,
        "python",
        Some(probe),
    )?;
    Ok(WasmProcessPair {
        old_tree,
        new_tree,
        cache_hit: lookup.cache_hit,
        cache_key: lookup.cache_key,
    })
}

fn run_python_wasm_process_pair_preloaded_profiled(
    lookup: &ParserComponentLookup,
    old_source: &str,
    old_filtered_cst: &str,
    old_filename: &str,
    new_source: &str,
    new_filtered_cst: &str,
    new_filename: &str,
    fuel: u64,
    max_output_bytes: usize,
    probe: &mut PhaseProbe,
) -> Result<WasmProcessPair, String> {
    let (old_tree, new_tree) = run_python_wasm_process_pair_with_cached_component(
        &lookup.cached,
        old_source,
        old_filtered_cst,
        old_filename,
        new_source,
        new_filtered_cst,
        new_filename,
        fuel,
        max_output_bytes,
        "python",
        Some(probe),
    )?;
    Ok(WasmProcessPair {
        old_tree,
        new_tree,
        cache_hit: lookup.cache_hit,
        cache_key: lookup.cache_key.clone(),
    })
}

fn run_python_wasm_process_pair_with_cached_component(
    cached: &CachedParserComponent,
    old_source: &str,
    old_filtered_cst: &str,
    old_filename: &str,
    new_source: &str,
    new_filtered_cst: &str,
    new_filename: &str,
    fuel: u64,
    max_output_bytes: usize,
    language: &str,
    mut probe: Option<&mut PhaseProbe>,
) -> Result<(String, String), String> {
    let unlimited = fuel == u64::MAX;
    let linker: Linker<ParserHostState> =
        measure_optional(probe.as_deref_mut(), "rust_wasm_linker_setup", || {
            let mut linker: Linker<ParserHostState> = Linker::new(&cached.engine);
            wasmtime_wasi::p2::add_to_linker_sync(&mut linker)
                .map_err(|exc| format!("link WASIp2: {exc}"))?;
            parser_plugin::ParserPlugin::add_to_linker::<_, HasSelf<_>>(&mut linker, |state| state)
                .map_err(|exc| format!("link host-utils: {exc}"))?;
            Ok(linker)
        })?;
    let mut store = measure_optional(probe.as_deref_mut(), "rust_wasm_store_setup", || {
        let mut store = Store::new(&cached.engine, ParserHostState::new());
        if !unlimited {
            store
                .set_fuel(fuel)
                .map_err(|exc| format!("set wasm fuel: {exc}"))?;
        }
        Ok(store)
    })?;
    let bindings = measure_optional(
        probe.as_deref_mut(),
        "rust_wasm_component_instantiation",
        || {
            parser_plugin::ParserPlugin::instantiate(&mut store, &cached.component, &linker)
                .map_err(|exc| format!("instantiate parser component: {exc}"))
        },
    )?;
    let parser = bindings.intentdiff_plugin_parser();
    let parser_mode = measure_optional(probe.as_deref_mut(), "rust_wasm_parser_mode", || {
        parser
            .call_get_parser_mode(&mut store)
            .map_err(|exc| format!("call parser get-parser-mode: {exc}"))
    })?;
    let full_parse = matches!(
        parser_mode,
        parser_plugin::exports::intentdiff::plugin::parser::ParserMode::FullParse
    );
    if !unlimited {
        measure_optional(
            probe.as_deref_mut(),
            "rust_wasm_fuel_reset_after_mode",
            || {
                store
                    .set_fuel(fuel)
                    .map_err(|exc| format!("reset wasm fuel after parser mode: {exc}"))
            },
        )?;
    }
    let old_input = if full_parse {
        old_source
    } else {
        old_filtered_cst
    };
    let new_input = if full_parse {
        new_source
    } else {
        new_filtered_cst
    };
    let old_tree = measure_optional(probe.as_deref_mut(), "rust_wasm_process_old", || {
        parser
            .call_process(&mut store, old_input, language, old_filename)
            .map_err(|exc| format!("call parser process for old source: {exc:#}"))
    })?;
    check_byte_limit("old parser output", &old_tree, max_output_bytes)?;
    if !unlimited {
        measure_optional(probe.as_deref_mut(), "rust_wasm_fuel_reset", || {
            store
                .set_fuel(fuel)
                .map_err(|exc| format!("reset wasm fuel: {exc}"))
        })?;
    }
    let new_tree = measure_optional(probe.as_deref_mut(), "rust_wasm_process_new", || {
        parser
            .call_process(&mut store, new_input, language, new_filename)
            .map_err(|exc| format!("call parser process for new source: {exc:#}"))
    })?;
    check_byte_limit("new parser output", &new_tree, max_output_bytes)?;
    Ok((old_tree, new_tree))
}

fn measure_optional<T, F>(
    probe: Option<&mut PhaseProbe>,
    name: &'static str,
    op: F,
) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String>,
{
    if let Some(probe) = probe {
        probe.measure(name, op)
    } else {
        op()
    }
}

fn measure_value_optional<T, F>(probe: Option<&mut PhaseProbe>, name: &'static str, op: F) -> T
where
    F: FnOnce() -> T,
{
    if let Some(probe) = probe {
        probe.measure_value(name, op)
    } else {
        op()
    }
}

/// Collect all descendants of a CST node in pre-order (excluding the node itself).
fn cst_descendants<'a>(node: &'a CstNode, out: &mut Vec<&'a CstNode>) {
    for child in &node.children {
        out.push(child);
        cst_descendants(child, out);
    }
}

/// A no-op statement: `pass`, `...`, a lone docstring, or `raise NotImplementedError`.
fn cst_is_trivial_stmt(stmt: &CstNode) -> bool {
    match stmt.node_type.as_str() {
        "pass_statement" => true,
        "expression_statement" => {
            !stmt.children.is_empty()
                && stmt.children.iter().all(|c| {
                    matches!(
                        c.node_type.as_str(),
                        "string" | "concatenated_string" | "ellipsis"
                    )
                })
        }
        "raise_statement" => {
            let mut nodes = Vec::new();
            cst_descendants(stmt, &mut nodes);
            nodes
                .iter()
                .any(|d| d.node_type == "identifier" && d.text == "NotImplementedError")
        }
        _ => false,
    }
}

/// Classify a function/class body block as empty / stub (no-op) / substantive.
/// The scalar-literal KIND a `return_statement` yields (`int`/`float`/`str`/`bool`/`none`), or
/// None when the returned value is computed (call/operator/name/collection) or multi-valued.
/// Kind only — never the literal value itself — so it is safe for the privacy fact sheet.
fn cst_return_literal_kind(ret: &CstNode) -> Option<&'static str> {
    // Ignore the syntax that carries no value. A tree-sitter CST keeps the `return` keyword
    // (and any punctuation) as children of the return statement; the native CST does not.
    // Requiring `children.len() == 1` therefore made this function answer differently for
    // IDENTICAL source depending on which parser produced the tree — "literal" natively,
    // "value" through the Wasm parser — which is how every non-native path silently lost
    // `return_kind`.
    let mut values = ret.children.iter().filter(|c| {
        !matches!(c.node_type.as_str(), "return" | "return_keyword")
            && !c.node_type.chars().all(|ch| ch.is_ascii_punctuation())
    });
    let value = values.next()?;
    if values.next().is_some() {
        // More than one value node: not a single literal.
        return None;
    }
    match value.node_type.as_str() {
        "integer" => Some("int"),
        "float" => Some("float"),
        "string" | "concatenated_string" => Some("str"),
        "true" | "false" => Some("bool"),
        "none" => Some("none"),
        _ => None,
    }
}

fn cst_classify_body(block: &CstNode) -> &'static str {
    if block.children.is_empty() {
        "empty"
    } else if block.children.iter().all(cst_is_trivial_stmt) {
        "stub"
    } else {
        "substantive"
    }
}

/// Privacy-safe structural facts for a Python definition node, computed from the
/// full CST (only counts/enums/flags — never source text, names, or literals).
/// Mirrors the Wasm python-parser so the native batch path emits the same facts.
fn python_node_facts_value(node: &CstNode) -> Option<Value> {
    let is_fn = matches!(
        node.node_type.as_str(),
        "function_definition" | "async_function_def"
    );
    let is_class = node.node_type == "class_definition";
    if !is_fn && !is_class {
        return None;
    }
    let mut facts = serde_json::Map::new();
    if is_fn {
        if let Some(params) = node.children.iter().find(|c| c.node_type == "parameters") {
            facts.insert("param_count".to_owned(), json!(params.children.len()));
            // Param kinds (#69 catalog C): counts/flags only — never a parameter name. A param
            // after `*args`/`*` is keyword-only; `*args`/`**kwargs` make the signature variadic.
            let mut default_count = 0usize;
            let mut keyword_only_count = 0usize;
            let mut has_variadic = false;
            let mut has_kwargs = false;
            let mut after_splat = false;
            for p in &params.children {
                match p.node_type.as_str() {
                    "list_splat_pattern" => {
                        has_variadic = true;
                        after_splat = true;
                    }
                    "keyword_separator" => after_splat = true,
                    "dictionary_splat_pattern" => has_kwargs = true,
                    "default_parameter" | "typed_default_parameter" => {
                        default_count += 1;
                        if after_splat {
                            keyword_only_count += 1;
                        }
                    }
                    "identifier" | "typed_parameter" => {
                        if after_splat {
                            keyword_only_count += 1;
                        }
                    }
                    _ => {}
                }
            }
            if default_count > 0 {
                facts.insert("default_count".to_owned(), json!(default_count));
            }
            if keyword_only_count > 0 {
                facts.insert("keyword_only_count".to_owned(), json!(keyword_only_count));
            }
            if has_variadic {
                facts.insert("has_variadic".to_owned(), json!(true));
            }
            if has_kwargs {
                facts.insert("has_kwargs".to_owned(), json!(true));
            }
        }
        if node.node_type == "async_function_def" {
            facts.insert("is_async".to_owned(), json!(true));
        }
        if let Some(block) = node.children.iter().find(|c| c.node_type == "block") {
            let body_kind = cst_classify_body(block);
            facts.insert("body".to_owned(), json!(body_kind));
            let mut descendants = Vec::new();
            cst_descendants(block, &mut descendants);
            let mut return_kinds: Vec<Option<&'static str>> = Vec::new();
            let mut is_generator = false;
            let mut side_effects = false;
            let mut has_conditional = false;
            let mut has_loop = false;
            let mut has_error_handling = false;
            let mut throws = false;
            let mut mutates = false;
            let mut constructs = false;
            let mut has_computation = false;
            let mut call_count = 0usize;
            let mut recursive = false;
            // The function's own name, to detect self-recursion (name read only for the flag).
            let fn_name = node
                .children
                .iter()
                .find(|c| c.node_type == "identifier")
                .map(|c| c.text.as_str());
            for d in descendants.iter().copied() {
                let dt = d.node_type.as_str();
                match dt {
                    "return_statement" if !d.children.is_empty() => {
                        return_kinds.push(cst_return_literal_kind(d));
                        // A return whose value is a freshly-built collection/object -> factory.
                        if d.children.len() == 1
                            && is_construction_node_type(d.children[0].node_type.as_str())
                        {
                            constructs = true;
                        }
                    }
                    "yield" => is_generator = true,
                    // A BARE call statement (`print(...)`) is a side effect. `x = foo()` and
                    // `return foo()` nest the call under assignment/return, so this matches only
                    // free-standing calls. Flag only — never the callee name (privacy-safe).
                    "expression_statement"
                        if d.children.iter().any(|c| c.node_type == "call") =>
                    {
                        side_effects = true;
                    }
                    // Mutation (#69-H): `x += 1`, or an assignment whose target is an
                    // attribute/subscript (`self.x = …`, `a[i] = …`). Target node_type only.
                    "augmented_assignment" => mutates = true,
                    "assignment" => {
                        if let Some(target) = d.children.first() {
                            if matches!(target.node_type.as_str(), "attribute" | "subscript") {
                                mutates = true;
                            }
                        }
                    }
                    _ => {}
                }
                // Behavior classification (#69-H): control-flow shape.
                if is_conditional_node_type(dt) {
                    has_conditional = true;
                }
                if is_loop_node_type(dt) {
                    has_loop = true;
                }
                if is_error_handling_node_type(dt) {
                    has_error_handling = true;
                }
                if is_throw_node_type(dt) {
                    throws = true;
                }
                if is_computation_node_type(dt) {
                    has_computation = true;
                }
                // Coupling (#69-J): outbound-call fan-out + self-recursion. Count call sites; a
                // call whose callee is the function's own name is recursion (flag only).
                if dt == "call" {
                    call_count += 1;
                    if let Some(callee) = d.children.first() {
                        if callee.node_type == "identifier"
                            && Some(callee.text.as_str()) == fn_name
                        {
                            recursive = true;
                        }
                    }
                }
            }
            if return_kinds.is_empty() {
                facts.insert("returns".to_owned(), json!("none"));
            } else if return_kinds.iter().all(Option::is_some) {
                // Every return yields a scalar constant -> the function returns a literal.
                facts.insert("returns".to_owned(), json!("literal"));
                let first = return_kinds[0];
                let kind = if return_kinds.iter().all(|k| *k == first) {
                    first.expect("all Some")
                } else {
                    "mixed"
                };
                facts.insert("return_kind".to_owned(), json!(kind));
            } else {
                facts.insert("returns".to_owned(), json!("value"));
            }
            if side_effects {
                facts.insert("side_effects".to_owned(), json!(true));
            }
            if is_generator {
                facts.insert("is_generator".to_owned(), json!(true));
            }
            if has_conditional {
                facts.insert("has_conditional".to_owned(), json!(true));
            }
            if has_loop {
                facts.insert("has_loop".to_owned(), json!(true));
            }
            if has_error_handling {
                facts.insert("has_error_handling".to_owned(), json!(true));
            }
            facts.insert(
                "control_shape".to_owned(),
                json!(if has_loop {
                    "looping"
                } else if has_conditional {
                    "branching"
                } else {
                    "linear"
                }),
            );
            if throws {
                facts.insert("throws".to_owned(), json!(true));
            }
            if mutates {
                facts.insert("mutates".to_owned(), json!(true));
            }
            if constructs {
                facts.insert("constructs".to_owned(), json!(true));
            }
            // Emit has_computation ONLY for a substantive body (there is real content to assess).
            // A substantive body that computes nothing is the #68 antidote — the explainer can say
            // "performs no computation" instead of inventing it. Stub/empty bodies omit it.
            if body_kind == "substantive" {
                facts.insert("has_computation".to_owned(), json!(has_computation));
            }
            if call_count > 0 {
                facts.insert("call_count".to_owned(), json!(call_count));
            }
            if recursive {
                facts.insert("recursive".to_owned(), json!(true));
            }
            let returns_value = !return_kinds.is_empty();
            if let Some(category) = behavior_category(
                returns_value,
                side_effects,
                has_conditional,
                has_loop,
                throws,
                mutates,
                constructs,
            ) {
                facts.insert("behavior_category".to_owned(), json!(category));
            }
        } else {
            facts.insert("body".to_owned(), json!("empty"));
            facts.insert("returns".to_owned(), json!("none"));
        }
    } else {
        // Class facts (#69 catalog D): shape (method/field/base counts) + kind (enum/exception),
        // from the class node's own children. Counts + booleans only — never a member/base NAME.
        if let Some(block) = node.children.iter().find(|c| c.node_type == "block") {
            facts.insert("body".to_owned(), json!(cst_classify_body(block)));
            let mut method_count = 0usize;
            let mut field_count = 0usize;
            for stmt in &block.children {
                match stmt.node_type.as_str() {
                    "function_definition" | "async_function_def" => method_count += 1,
                    // A decorated method (`@property def x`) is wrapped — count the inner def.
                    "decorated_definition" => {
                        if stmt.children.iter().any(|c| {
                            matches!(
                                c.node_type.as_str(),
                                "function_definition" | "async_function_def"
                            )
                        }) {
                            method_count += 1;
                        }
                    }
                    // A class-level attribute: `x = …` or `x: T = …` (annotated). Both nest an
                    // `assignment` under an `expression_statement`.
                    "expression_statement" => {
                        if stmt.children.iter().any(|c| c.node_type == "assignment") {
                            field_count += 1;
                        }
                    }
                    _ => {}
                }
            }
            facts.insert("method_count".to_owned(), json!(method_count));
            facts.insert("field_count".to_owned(), json!(field_count));
        } else {
            facts.insert("body".to_owned(), json!("empty"));
        }
        // Bases + class kind from the superclass argument_list (`class C(Base1, Base2)`).
        let mut base_count = 0usize;
        let mut is_enum = false;
        let mut is_exception = false;
        if let Some(args) = node.children.iter().find(|c| c.node_type == "argument_list") {
            for arg in &args.children {
                if let Some(name) = cst_base_name(arg) {
                    base_count += 1;
                    is_enum |= is_enum_base_name(name);
                    is_exception |= is_exception_base_name(name);
                }
            }
        }
        if base_count > 0 {
            facts.insert("base_count".to_owned(), json!(base_count));
        }
        if is_enum {
            facts.insert("is_enum".to_owned(), json!(true));
        }
        if is_exception {
            facts.insert("is_exception".to_owned(), json!(true));
        }
    }
    if facts.is_empty() {
        None
    } else {
        Some(Value::Object(facts))
    }
}

// ============ Language-agnostic NodeFacts (issue #70) ============
// python_node_facts_value covers the native Python CST path; every other language parses via a
// Wasm parser that emits `facts: None`. These derive the SAME privacy-safe facts (param_count,
// returns/return_kind, side_effects) from the pruned SemanticNode tree, using cross-grammar node
// vocabularies, so the intent explainer has structural signal for ALL languages. Kind/count/flag
// only — never the literal value or an identifier.

/// A function/method entity across grammars.
fn is_function_entity_type(node_type: &str) -> bool {
    matches!(
        node_type,
        "function_definition"
            | "function_declaration"
            | "function_item"
            | "function_expression"
            | "arrow_function"
            | "method_definition"
            | "method_declaration"
            | "method"
            | "func_literal"
            | "constructor_declaration"
            | "function_statement"
            | "subroutine_declaration_statement"
            // Generators are functions. These were absent while their `async_*` twins
            // below were present, so toggling `async` on a generator changed whether
            // NodeFacts were derived at all (param_count/returns/side_effects appearing
            // out of nowhere) rather than just flipping is_async. js-ts is the only
            // parser that emits either spelling.
            | "generator_function_declaration"
            | "generator_function"
            // js-ts async variants (the parser's `async_*` node types). `async_function`
            // is deliberately absent: async_variant_of mints it only from a node whose
            // kind() is "function", and in tree-sitter-javascript 0.23.1 "function" is
            // `"named": false` - the anonymous keyword token, never a node kind. The
            // reachable function-expression spelling is `async_function_expression`,
            // which is here, matching its plain counterpart above.
            | "async_function_declaration"
            | "async_function_expression"
            | "async_arrow_function"
            | "async_generator_function_declaration"
            | "async_generator_function"
            | "async_method_definition"
    ) || node_type == "async_function_def"
}

/// An OO class definition that can hold methods (Python uses the native CST path, so its
/// class_definition is handled there; this is the cross-language fallback for js/ts/java/cpp).
fn is_class_entity_type(node_type: &str) -> bool {
    matches!(
        node_type,
        "class_definition" | "class_declaration" | "class_specifier"
    )
}

/// A body container that holds a class's members (methods live one level under it).
fn is_class_body_container(node_type: &str) -> bool {
    matches!(
        node_type,
        "block" | "class_body" | "declaration_list" | "field_declaration_list"
    )
}

/// Cross-language class facts (#69 catalog D, the #70 language-agnostic path): the direct-member
/// method count, from methods that are direct children of the class or nested one level under its
/// body container. Count only — never a member name.
fn derive_class_facts(node: &SemanticNode) -> Option<Value> {
    let mut method_count = 0usize;
    let mut field_count = 0usize;
    for child in &node.children {
        if is_function_entity_type(child.node_type.as_str()) {
            method_count += 1;
        } else if is_class_body_container(child.node_type.as_str()) {
            for member in &child.children {
                if is_function_entity_type(member.node_type.as_str()) {
                    method_count += 1;
                } else if is_class_field_member(member) {
                    field_count += 1;
                }
            }
        }
    }

    // Bases decide the class KIND. The CST pass reads these from an `argument_list` child,
    // which only exists on a tree-sitter Python CST — so every other parser, and every other
    // language, lost `is_enum` / `is_exception` entirely. Reading them from the normalised
    // tree instead makes the facts available regardless of who parsed it.
    let mut base_count = 0usize;
    let mut is_enum = false;
    let mut is_exception = false;
    for child in &node.children {
        if !is_class_bases_container(child.node_type.as_str()) {
            continue;
        }
        for arg in &child.children {
            let Some(name) = semantic_base_name(arg) else {
                continue;
            };
            base_count += 1;
            is_enum |= is_enum_base_name(name);
            is_exception |= is_exception_base_name(name);
        }
    }

    let mut facts = serde_json::Map::new();
    facts.insert("method_count".to_owned(), json!(method_count));
    facts.insert("field_count".to_owned(), json!(field_count));
    if base_count > 0 {
        facts.insert("base_count".to_owned(), json!(base_count));
    }
    if is_enum {
        facts.insert("is_enum".to_owned(), json!(true));
    }
    if is_exception {
        facts.insert("is_exception".to_owned(), json!(true));
    }
    Some(Value::Object(facts))
}

/// A class member that holds data rather than behaviour. Count only — never a field name.
fn is_class_field_member(member: &SemanticNode) -> bool {
    match member.node_type.as_str() {
        "field_declaration" | "property_declaration" | "variable_declaration" => true,
        // Python spells a class attribute as a bare assignment statement.
        "expression_statement" => member
            .children
            .iter()
            .any(|c| matches!(c.node_type.as_str(), "assignment" | "augmented_assignment")),
        "assignment" => true,
        _ => false,
    }
}

/// Where a class lists its bases. Parsers disagree on the wrapper, so accept the known spellings
/// rather than the single tree-sitter-Python one the CST pass assumes.
fn is_class_bases_container(node_type: &str) -> bool {
    matches!(
        node_type,
        "argument_list" | "superclasses" | "base_class_clause" | "class_heritage" | "extends_clause"
    )
}

/// The base's name from a normalised node — its label, or a lone identifier child's label.
/// Used ONLY to classify the class kind (enum / exception); never emitted.
fn semantic_base_name(arg: &SemanticNode) -> Option<&str> {
    if !arg.label.is_empty() {
        return Some(arg.label.as_str());
    }
    arg.children
        .iter()
        .find(|c| !c.label.is_empty())
        .map(|c| c.label.as_str())
}

/// The decorator's rightmost name — `property` from `@property`, `lru_cache` from
/// `@functools.lru_cache()`, `abstractmethod` from `@abc.abstractmethod`. Used ONLY to set a
/// boolean behavior flag; the name is never emitted (privacy-safe).
fn semantic_decorator_name(decorator: &SemanticNode) -> Option<&str> {
    fn rightmost_name(node: &SemanticNode) -> Option<&str> {
        match node.node_type.as_str() {
            "identifier" => Some(node.label.as_str()),
            "attribute" => node
                .children
                .iter()
                .rev()
                .find(|c| c.node_type == "identifier")
                .map(|c| c.label.as_str()),
            // `@deco(args)` — the callee is the first child.
            "call" => node.children.first().and_then(rightmost_name),
            _ => None,
        }
    }
    decorator.children.iter().find_map(rightmost_name)
}

/// Merge extra facts into a node's optional facts map (creating it if absent).
fn merge_into_facts(target: &mut Option<Value>, additions: serde_json::Map<String, Value>) {
    match target {
        Some(Value::Object(map)) => {
            for (k, v) in additions {
                map.insert(k, v);
            }
        }
        _ => *target = Some(Value::Object(additions)),
    }
}

/// Decorator semantics (#69 catalog C/D) for a `decorated_definition` wrapper. Reads each
/// decorator's name to set behavior-changing booleans (`is_property`, `is_staticmethod`,
/// `is_classmethod`, `is_abstract`, `is_cached`, `is_dataclass`) + `decorator_count`, then merges
/// them into the inner definition's facts AND mirrors the result onto the wrapper — so whichever
/// node a change references (a decorator add surfaces on the wrapper; a new decorated def may
/// surface on the inner) carries the facts. Counts + flags only, never a decorator name.
fn apply_decorator_facts(node: &mut SemanticNode) {
    let mut decorator_count = 0usize;
    let mut deco = serde_json::Map::new();
    let mut set = |k: &str| {
        deco.insert(k.to_owned(), json!(true));
    };
    for child in &node.children {
        if child.node_type != "decorator" {
            continue;
        }
        decorator_count += 1;
        match semantic_decorator_name(child) {
            Some("property") => set("is_property"),
            Some("staticmethod") => set("is_staticmethod"),
            Some("classmethod") => set("is_classmethod"),
            Some("abstractmethod" | "abstractproperty") => set("is_abstract"),
            Some("cache" | "lru_cache") => set("is_cached"),
            Some("cached_property") => {
                set("is_cached");
                set("is_property");
            }
            Some("dataclass") => set("is_dataclass"),
            _ => {}
        }
    }
    if decorator_count == 0 {
        return;
    }
    deco.insert("decorator_count".to_owned(), json!(decorator_count));
    // Merge into the inner definition, then mirror its full facts onto the wrapper.
    let inner_facts = node
        .children
        .iter_mut()
        .find(|c| {
            is_function_entity_type(c.node_type.as_str())
                || is_class_entity_type(c.node_type.as_str())
        })
        .map(|inner| {
            merge_into_facts(&mut inner.facts, deco.clone());
            inner.facts.clone()
        });
    match inner_facts {
        Some(Some(facts)) => node.facts = Some(facts),
        // No inner def found (unusual) — still record the decorator facts on the wrapper.
        _ => merge_into_facts(&mut node.facts, deco),
    }
}

mod node_facts;
use node_facts::*;
mod uast;

/// Derive privacy-safe facts for a function or class entity from its pruned SemanticNode subtree.
fn derive_node_facts(node: &SemanticNode) -> Option<Value> {
    let node_type = node.node_type.as_str();
    if is_class_entity_type(node_type) {
        // Cross-language class facts (Python classes carry facts from the CST path already).
        return derive_class_facts(node);
    }
    if !is_function_entity_type(node_type) {
        // Non-function shapes (#179). Until now this returned None, so a changed YAML key,
        // Terraform block, TOML table or INI setting carried NO facts and the explainer had
        // only the change type and label to work from — across 69 parsers that is most of a
        // real review. Ordered before the bail-out so function/class facts are unaffected.
        if let Some(keyed) = derive_keyed_facts(node) {
            return Some(keyed);
        }
        if let Some(resource) = derive_resource_facts(node) {
            return Some(resource);
        }
        return None;
    }
    let mut facts = serde_json::Map::new();
    if let Some(params) = node
        .children
        .iter()
        .find(|c| is_parameter_list_type(c.node_type.as_str()))
    {
        facts.insert("param_count".to_owned(), json!(params.children.len()));
        // Signature shape, counts only — never a parameter name. The CST pass derives these
        // from tree-sitter-Python spellings, so every other parser lost them; reading the
        // normalised tree makes them available whoever parsed it.
        let mut default_count = 0usize;
        let mut keyword_only_count = 0usize;
        let mut has_variadic = false;
        let mut has_kwargs = false;
        // Anything after `*args` or a bare `*` is keyword-only.
        let mut after_splat = false;
        for p in &params.children {
            match p.node_type.as_str() {
                "default_parameter" | "typed_default_parameter" | "optional_parameter"
                | "default_value" => {
                    default_count += 1;
                    if after_splat {
                        keyword_only_count += 1;
                    }
                }
                "list_splat_pattern" | "variadic_parameter" | "rest_parameter" => {
                    has_variadic = true;
                    after_splat = true;
                }
                "keyword_separator" => after_splat = true,
                "dictionary_splat_pattern" | "keyword_parameter" => has_kwargs = true,
                "identifier" | "typed_parameter" => {
                    if after_splat {
                        keyword_only_count += 1;
                    }
                }
                _ => {}
            }
        }
        if default_count > 0 {
            facts.insert("default_count".to_owned(), json!(default_count));
        }
        if keyword_only_count > 0 {
            facts.insert("keyword_only_count".to_owned(), json!(keyword_only_count));
        }
        if has_variadic {
            facts.insert("has_variadic".to_owned(), json!(true));
        }
        if has_kwargs {
            facts.insert("has_kwargs".to_owned(), json!(true));
        }
    }
    let mut has_return = false;
    let mut value_return_kinds: Vec<Option<&'static str>> = Vec::new();
    let mut side_effects = false;
    let mut has_conditional = false;
    let mut has_loop = false;
    let mut has_error_handling = false;
    let mut throws = false;
    let mut mutates = false;
    let mut constructs = false;
    let mut has_computation = false;
    let mut call_count = 0usize;
    let mut recursive = false;
    // The function's own name, for self-recursion detection (name read only for the flag).
    let fn_name = node.label.as_str();
    for descendant in node.descendants() {
        let descendant_type = descendant.node_type.as_str();
        // Coupling (#69-J): outbound-call fan-out + self-recursion. A call whose callee label is
        // the function's own name is recursion (flag only, never the name).
        if is_call_node_type(descendant_type) {
            call_count += 1;
            if let Some(callee) = descendant.children.first() {
                if !fn_name.is_empty() && callee.label == fn_name {
                    recursive = true;
                }
            }
        }
        if matches!(descendant_type, "return_statement" | "return_expression" | "return") {
            has_return = true;
            // A return whose value node survived pruning: classify its literal kind. A parser
            // that prunes the value leaves an empty return node — we know it returns SOMETHING
            // but not the kind, so it stays "value" below (never a false "none"). Java retained
            // its value literals in #72; kotlin/swift (line scanners) still emit childless
            // returns. Ruby's node type is plain "return" with an argument_list wrapper.
            if !descendant.children.is_empty() {
                value_return_kinds.push(semantic_return_literal_kind(descendant));
            }
            if let Some(value) = single_return_value(descendant) {
                if is_construction_node_type(value.node_type.as_str()) {
                    constructs = true;
                }
            }
        } else if is_bare_call_statement(descendant) {
            side_effects = true;
        }
        if is_mutating_statement(descendant) {
            mutates = true;
        }
        if is_computation_node_type(descendant_type) {
            has_computation = true;
        }
        // Behavior classification (#69-H): control-flow shape. Flags are set independently of the
        // return/call arm above so a conditional that returns is counted.
        if is_conditional_node_type(descendant_type) {
            has_conditional = true;
        }
        if is_loop_node_type(descendant_type) {
            has_loop = true;
        }
        if is_error_handling_node_type(descendant_type) {
            has_error_handling = true;
        }
        if is_throw_node_type(descendant_type) {
            throws = true;
        }
    }
    if !has_return {
        facts.insert("returns".to_owned(), json!("none"));
    } else if !value_return_kinds.is_empty() && value_return_kinds.iter().all(Option::is_some) {
        facts.insert("returns".to_owned(), json!("literal"));
        let first = value_return_kinds[0];
        let kind = if value_return_kinds.iter().all(|k| *k == first) {
            first.expect("all Some")
        } else {
            "mixed"
        };
        facts.insert("return_kind".to_owned(), json!(kind));
    } else {
        facts.insert("returns".to_owned(), json!("value"));
    }
    if side_effects {
        facts.insert("side_effects".to_owned(), json!(true));
    }
    if has_conditional {
        facts.insert("has_conditional".to_owned(), json!(true));
    }
    if has_loop {
        facts.insert("has_loop".to_owned(), json!(true));
    }
    if has_error_handling {
        facts.insert("has_error_handling".to_owned(), json!(true));
    }
    // Rollup so the explainer can lead with the body's shape.
    facts.insert(
        "control_shape".to_owned(),
        json!(if has_loop {
            "looping"
        } else if has_conditional {
            "branching"
        } else {
            "linear"
        }),
    );
    if throws {
        facts.insert("throws".to_owned(), json!(true));
    }
    if mutates {
        facts.insert("mutates".to_owned(), json!(true));
    }
    if constructs {
        facts.insert("constructs".to_owned(), json!(true));
    }
    // `has_computation` is emitted as an explicit bool ONLY for a substantive body (there is real
    // content to assess) — a substantive body that computes nothing is the #68 antidote. An
    // empty/trivial body (all flags false) omits it rather than assert an uninformative "false".
    let body_is_substantive = has_return
        || side_effects
        || has_conditional
        || has_loop
        || has_error_handling
        || throws
        || mutates
        || constructs
        || has_computation;
    if body_is_substantive {
        facts.insert("has_computation".to_owned(), json!(has_computation));
    }
    if call_count > 0 {
        facts.insert("call_count".to_owned(), json!(call_count));
    }
    if recursive {
        facts.insert("recursive".to_owned(), json!(true));
    }
    // `has_return` implies returns "literal"/"value" above (never "none"), so it is the
    // returns-a-value signal for the category rollup.
    if let Some(category) = behavior_category(
        has_return,
        side_effects,
        has_conditional,
        has_loop,
        throws,
        mutates,
        constructs,
    ) {
        facts.insert("behavior_category".to_owned(), json!(category));
    }
    if facts.is_empty() {
        None
    } else {
        Some(Value::Object(facts))
    }
}

/// Fill `facts` for function entities that lack them (non-Python trees), in place. Leaves nodes
/// that already carry facts (the Python native path) untouched.
fn enrich_tree_facts(node: &mut SemanticNode) {
    // Derive and MERGE, rather than deriving only when facts are entirely absent.
    //
    // `python_node_facts_value` reads the raw CST, whose shape varies by parser, so it can
    // return a PARTIAL bag — enough to make `facts.is_none()` false while `side_effects`,
    // `return_kind` and friends are still missing. Skipping on "has any facts at all" then
    // froze that partial bag in place forever.
    //
    // `derive_node_facts` works from the NORMALISED SemanticNode, which is parser-independent,
    // so it is the more reliable of the two. It still cannot win outright: the CST pass sees
    // the tree before pruning, so where both produce a key the earlier one saw more.
    // `merge_facts` never overwrites, so this can only fill gaps.
    let had_facts = node.facts.is_some();
    let derived = derive_node_facts(node);
    let derived_keys = match derived.as_ref() {
        Some(Value::Object(map)) => map.len(),
        _ => 0,
    };
    let added = derived.map_or(0, |d| merge_facts(&mut node.facts, d));
    if facts_trace_enabled() && (had_facts || derived_keys > 0) {
        // Records what each pass contributed, so "why is this fact missing?" is answerable
        // from a user's log instead of a rebuild. `cst` alone on a function entity means the
        // raw-CST pass produced everything and the normalised pass added nothing; a large
        // `derived` with a small `+n` means the CST pass had already claimed those keys.
        let trace = format!(
            "{}enrich(derived={derived_keys},added={added})",
            if had_facts { "cst," } else { "" }
        );
        push_facts_trace(&mut node.facts, &trace);
    }
    // Cross-language structural facts (#9). This tree is PRUNED, so only what survives
    // pruning is derivable: statement order does, operators do not. That yields
    // early_exit_count for every language, while has_guard_clause stays absent rather than
    // false — see uast.rs for why omitting beats claiming.
    //
    // Runs for function entities regardless of whether facts already exist, and merges, so
    // the native Python path keeps its richer guard facts and everything else still gains
    // the early-exit count. Idempotent: merge_facts never overwrites.
    if is_function_entity_type(node.node_type.as_str()) {
        if let Some(structural) = uast::uast_structural_facts_pruned(node, "") {
            merge_facts(&mut node.facts, structural);
        }
    }
    for child in &mut node.children {
        enrich_tree_facts(child);
    }
    // Decorators (#69 catalog C/D) are a WRAPPER sibling of the def, so fold them in after the
    // children (incl. the inner def) have their own facts. Idempotent: re-running merges the same
    // flags.
    if node.node_type == "decorated_definition" {
        apply_decorator_facts(node);
    }
}

/// Language-agnostic NodeFacts enrichment for a serialized SemanticNode tree (issue #70). The
/// thin Python wrapper calls this on Wasm-parsed trees (which carry no facts) so every language
/// gets the same privacy-safe structural facts the native Python path already has. Idempotent —
/// nodes that already have facts are untouched.
fn convert_cst(
    node: &CstNode,
    id_prefix: &str,
    parent_class: Option<&str>,
) -> Option<SemanticNode> {
    let mut hash_memo = HashMap::new();
    structural_hash_cst_with_memo(node, &mut hash_memo);
    convert_cst_with_hash_memo(node, id_prefix, parent_class, &hash_memo)
}

fn convert_cst_with_hash_memo(
    node: &CstNode,
    id_prefix: &str,
    parent_class: Option<&str>,
    hash_memo: &HashMap<usize, String>,
) -> Option<SemanticNode> {
    let own_class = if node.node_type == "class_definition" {
        Some(label_for(node))
    } else {
        None
    };
    let child_parent_class = own_class.as_deref().or(parent_class);
    let children: Vec<SemanticNode> = node
        .children
        .iter()
        .enumerate()
        .filter_map(|(idx, child)| {
            convert_cst_with_hash_memo(
                child,
                &format!("{id_prefix}.{idx}"),
                child_parent_class,
                hash_memo,
            )
        })
        .collect();
    if !is_semantic(&node.node_type) && children.is_empty() {
        return None;
    }
    let parent_type = if matches!(
        node.node_type.as_str(),
        "function_definition" | "async_function_def"
    ) {
        parent_class.map(ToOwned::to_owned)
    } else {
        None
    };
    let mut semantic = SemanticNode {
        id: id_prefix.to_owned(),
        node_type: node.node_type.clone(),
        label: label_for(node),
        position: NodePosition {
            start_line: node.start_line,
            start_col: node.start_col,
            end_line: node.end_line,
            end_col: node.end_col,
        },
        structural_hash: hash_memo
            .get(&(node as *const CstNode as usize))
            .cloned()
            .unwrap_or_else(|| structural_hash_cst(node)),
        children,
        parent_type,
        type_info: None,
        facts: python_node_facts_value(node),
    };
    // UAST-derived structural facts (#9): guard clauses, early exits, negated conditions.
    // These describe how a function is ARRANGED, which the flag-and-count vocabulary cannot
    // reach — `if not x: return` and `if x: work()` are both has_conditional + a call.
    //
    // Merged rather than replacing: the existing facts are correct and widely relied upon;
    // this adds what they could not say. Only fires for function-shaped nodes, so the cost
    // stays proportional to the number of functions rather than the number of nodes.
    if is_function_entity_type(semantic.node_type.as_str()) {
        if let Some(structural) = uast::uast_structural_facts(node, "python") {
            merge_facts(&mut semantic.facts, structural);
        }
    }
    // Decorator semantics (#69 catalog C/D): decorators are on this WRAPPER, not the inner def
    // python_node_facts_value saw — fold their flags in here so the native Python path carries
    // them (the #70 enrich pass does the same for non-Python trees). Idempotent.
    if semantic.node_type == "decorated_definition" {
        apply_decorator_facts(&mut semantic);
    }
    Some(semantic)
}

/// Fold derived facts into a node's existing bag.
///
/// Existing keys WIN: `python_node_facts_value` reads the full CST before pruning, so where
/// the two disagree the earlier pass saw more. This can only add.
fn merge_facts(target: &mut Option<Value>, extra: Value) -> usize {
    let Value::Object(extra) = extra else { return 0 };
    let mut added = 0usize;
    match target {
        Some(Value::Object(existing)) => {
            for (k, v) in extra {
                // A null carries no information. `entry().or_insert()` treated a key present
                // with a null as already answered, so a partial bag from the raw-CST pass
                // permanently blocked the normalised pass from completing it — the node kept
                // `behavior_category: null` forever rather than gaining the real value.
                if v.is_null() {
                    continue;
                }
                match existing.get(&k) {
                    // `returns` has a specificity order. "value" means "returns something,
                    // kind unknown" — it is what the CST pass falls back to whenever it
                    // cannot identify the literal, which happens whenever the parser's tree
                    // shape differs from the one it assumes. "literal" is a strictly better
                    // answer, so letting the vaguer one win would discard a real result.
                    Some(current)
                        if k == "returns"
                            && current.as_str() == Some("value")
                            && v.as_str() == Some("literal") =>
                    {
                        existing.insert(k, v);
                        added += 1;
                    }
                    Some(current) if !current.is_null() => {}
                    _ => {
                        existing.insert(k, v);
                        added += 1;
                    }
                }
            }
        }
        _ => {
            added = extra.len();
            *target = Some(Value::Object(extra));
        }
    }
    added
}

/// Whether to record which pass produced which facts. Off by default; one env read, cached.
///
/// Exists so a user can attach `INTENTUMDIFF_TRACE_FACTS=1` output to a bug report and we can
/// see which derivation ran without reproducing their tree. Diagnosing this by rebuilding took
/// four cycles; the trace answers it in one run.
fn facts_trace_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        matches!(
            std::env::var("INTENTUMDIFF_TRACE_FACTS").as_deref(),
            Ok("1") | Ok("true") | Ok("yes")
        )
    })
}

/// Append a pass marker to a node's `facts_trace`, creating the bag if needed.
fn push_facts_trace(target: &mut Option<Value>, marker: &str) {
    let entry = match target {
        Some(Value::Object(existing)) => existing,
        _ => {
            *target = Some(json!({ "facts_trace": marker }));
            return;
        }
    };
    match entry.get("facts_trace").and_then(Value::as_str) {
        Some(prev) => {
            let combined = format!("{prev};{marker}");
            entry.insert("facts_trace".to_owned(), json!(combined));
        }
        None => {
            entry.insert("facts_trace".to_owned(), json!(marker));
        }
    }
}

/// Merge semantics for the two fact derivations.
///
/// These are the engine invariants; `tests/unit/test_fact_derivation_provenance.py` is the
/// acceptance half that proves the wiring survives the binding. Both are required — a Go or
/// Java binding gets the guarantee from here, not from pytest.
#[cfg(test)]
mod fact_merge_tests {
    use super::*;

    #[test]
    fn a_null_does_not_block_the_pass_that_can_supply_the_value() {
        // THE bug. The CST pass emits a partial bag whose unreachable facts sit as explicit
        // nulls. `entry().or_insert()` treated those as answered, so the normalised pass —
        // which ran on every node, every time — could never complete them.
        let mut target = Some(json!({ "behavior_category": null, "returns": "none" }));
        let added = merge_facts(&mut target, json!({ "behavior_category": "validator" }));
        assert_eq!(added, 1, "a null must not count as an existing answer");
        assert_eq!(target.as_ref().unwrap()["behavior_category"], json!("validator"));
    }

    #[test]
    fn a_real_existing_value_still_wins() {
        // The CST pass reads the tree BEFORE pruning, so where both produce a key it saw more.
        // Gap-filling must not become overwriting.
        let mut target = Some(json!({ "body": "substantive" }));
        let added = merge_facts(&mut target, json!({ "body": "stub" }));
        assert_eq!(added, 0);
        assert_eq!(target.as_ref().unwrap()["body"], json!("substantive"));
    }

    #[test]
    fn a_vague_returns_is_upgraded_by_a_specific_one() {
        // "value" is the CST pass's fallback for "returns something, kind unknown" — which is
        // what it answers whenever the parser's tree shape differs from the one it assumes.
        // "literal" is strictly better, so the vaguer answer must not win.
        let mut target = Some(json!({ "returns": "value" }));
        let added = merge_facts(&mut target, json!({ "returns": "literal", "return_kind": "int" }));
        assert_eq!(added, 2);
        assert_eq!(target.as_ref().unwrap()["returns"], json!("literal"));
        assert_eq!(target.as_ref().unwrap()["return_kind"], json!("int"));
    }

    #[test]
    fn the_upgrade_does_not_run_backwards() {
        let mut target = Some(json!({ "returns": "literal" }));
        merge_facts(&mut target, json!({ "returns": "value" }));
        assert_eq!(target.as_ref().unwrap()["returns"], json!("literal"));
    }

    #[test]
    fn an_incoming_null_is_never_written() {
        // A null carries no information in either direction.
        let mut target = Some(json!({}));
        let added = merge_facts(&mut target, json!({ "is_enum": null }));
        assert_eq!(added, 0);
        assert!(target.as_ref().unwrap().get("is_enum").is_none());
    }

    #[test]
    fn merge_reports_what_it_actually_contributed() {
        // `derived=N,added=0` in a trace is the signature of the null-blocking bug, so the
        // count has to be truthful for the diagnostic to be worth anything.
        let mut target = Some(json!({ "a": 1, "b": null }));
        let added = merge_facts(&mut target, json!({ "a": 2, "b": 3, "c": 4 }));
        assert_eq!(added, 2, "b was null and c was absent; a was already answered");
    }

    #[test]
    fn tracing_is_off_unless_the_environment_asks_for_it() {
        // Diagnostic payload must never appear in normal output.
        assert!(
            !facts_trace_enabled(),
            "INTENTUMDIFF_TRACE_FACTS must default off; a set env var breaks this test's premise"
        );
    }

    #[test]
    fn a_trace_marker_accumulates_rather_than_replacing() {
        // Enrichment runs more than once; the trace has to show the sequence, not the last one.
        let mut target = Some(json!({ "returns": "none" }));
        push_facts_trace(&mut target, "cst");
        push_facts_trace(&mut target, "enrich(derived=8,added=6)");
        assert_eq!(
            target.as_ref().unwrap()["facts_trace"],
            json!("cst;enrich(derived=8,added=6)")
        );
    }
}

fn is_semantic(node_type: &str) -> bool {
    SEMANTIC_TYPES.contains(&node_type)
}

fn label_for(node: &CstNode) -> String {
    if matches!(node.node_type.as_str(), "string" | "integer" | "float") && !node.text.is_empty() {
        return node.text.clone();
    }
    if node.children.is_empty() {
        return node.text.clone();
    }
    for child in &node.children {
        if child.node_type == "identifier" {
            return child.text.clone();
        }
    }
    node.node_type.clone()
}

fn structural_hash_cst(node: &CstNode) -> String {
    let mut memo = HashMap::new();
    structural_hash_cst_with_memo(node, &mut memo)
}

fn structural_hash_cst_with_memo(node: &CstNode, memo: &mut HashMap<usize, String>) -> String {
    let key = node as *const CstNode as usize;
    if let Some(cached) = memo.get(&key) {
        return cached.clone();
    }
    let mut hasher = Sha256::new();
    if node.children.is_empty() {
        hasher.update(node.node_type.as_bytes());
        hasher.update(b":");
        hasher.update(node.text.as_bytes());
    } else {
        hasher.update(node.node_type.as_bytes());
        for child in &node.children {
            hasher.update(b"|");
            hasher.update(structural_hash_cst_with_memo(child, memo).as_bytes());
        }
    }
    let result = hex::encode(hasher.finalize());
    memo.insert(key, result.clone());
    result
}

fn validate_unique_ids(root: &SemanticNode) -> Result<(), String> {
    let mut seen = HashSet::new();
    for node in std::iter::once(root).chain(root.descendants()) {
        if !seen.insert(node.id.as_str()) {
            return Err(format!("duplicate id {}", node.id));
        }
    }
    Ok(())
}

fn validate_unique_index_ids(index: &TreeIndex<'_>) -> Result<(), String> {
    let mut seen = HashSet::new();
    for node in &index.nodes {
        if !seen.insert(node.id.as_str()) {
            return Err(format!("duplicate id {}", node.id));
        }
    }
    Ok(())
}

fn compute_matching<'a>(
    old_root: &'a SemanticNode,
    new_root: &'a SemanticNode,
    min_height: usize,
    min_similarity: f64,
) -> Vec<MatchPair<'a>> {
    compute_matching_with_diagnostics(old_root, new_root, min_height, min_similarity).pairs
}

fn compute_matching_with_diagnostics<'a>(
    old_root: &'a SemanticNode,
    new_root: &'a SemanticNode,
    min_height: usize,
    min_similarity: f64,
) -> MatchingReport<'a> {
    compute_matching_with_diagnostics_mode(old_root, new_root, min_height, min_similarity, true)
}

fn compute_matching_with_diagnostics_mode<'a>(
    old_root: &'a SemanticNode,
    new_root: &'a SemanticNode,
    min_height: usize,
    min_similarity: f64,
    detailed_diagnostics: bool,
) -> MatchingReport<'a> {
    let old_index = TreeIndex::new(old_root);
    let new_index = TreeIndex::new(new_root);
    compute_matching_with_diagnostics_indexed(
        &old_index,
        &new_index,
        min_height,
        min_similarity,
        detailed_diagnostics,
    )
}

fn compute_matching_with_diagnostics_indexed<'a>(
    old_index: &TreeIndex<'a>,
    new_index: &TreeIndex<'a>,
    min_height: usize,
    min_similarity: f64,
    detailed_diagnostics: bool,
) -> MatchingReport<'a> {
    let mut diagnostics = MatchingDiagnostics {
        attempted: true,
        old_entity_count: old_index.named_entities.len(),
        new_entity_count: new_index.named_entities.len(),
        ..MatchingDiagnostics::default()
    };
    let mut matches = entity_first_match(
        &old_index,
        &new_index,
        &mut diagnostics,
        detailed_diagnostics,
    );
    let before_top_down = matches.len();
    matches = top_down_match_with_existing(&old_index, &new_index, min_height, matches);
    diagnostics.structural_matches += matches.len().saturating_sub(before_top_down);
    let old_root = old_index.nodes[0];
    let new_root = new_index.nodes[0];
    if old_root.node_type == new_root.node_type
        && !matches
            .iter()
            .any(|pair| pair.old_node.id == old_root.id || pair.new_node.id == new_root.id)
    {
        matches.insert(
            0,
            MatchPair {
                old_node: old_root,
                new_node: new_root,
            },
        );
    }
    let before_label = matches.len();
    let matches = label_match(&old_index, &new_index, matches);
    diagnostics.label_parent_matches += matches.len().saturating_sub(before_label);
    let before_bottom_up = matches.len();
    let matches = bottom_up_match(&old_index, &new_index, matches, min_similarity);
    diagnostics.bottom_up_matches += matches.len().saturating_sub(before_bottom_up);
    let matches = prune_cross_statement_leaf_pairs(&old_index, &new_index, matches);
    diagnostics.final_matching_pairs = matches.len();
    diagnostics.used = diagnostics.seeded_matches > 0;
    MatchingReport {
        pairs: matches,
        diagnostics,
    }
}

/// Post-matching statement-coherence prune (issue #26). A SAME-LABEL leaf pair whose
/// enclosing statements are not matched partners is a harvest across statements — e.g. the
/// deleted console.log's `console` identifier paired by proximity into the surviving
/// console.error call, which consumed the statement's clean DELETION and fabricated MOVE
/// noise. This runs AFTER all recovery phases (label match bootstraps bottom-up; bottom-up
/// matches statements whose leaves paired), so a rename's statement is already matched by the
/// time we check — no bootstrap starvation. Pairs with DIFFERENT labels (rename promotions)
/// and pairs without statement ancestors are kept.
fn prune_cross_statement_leaf_pairs<'a>(
    old_index: &TreeIndex<'a>,
    new_index: &TreeIndex<'a>,
    matches: Vec<MatchPair<'a>>,
) -> Vec<MatchPair<'a>> {
    fn enclosing_statement<'b>(index: &TreeIndex<'b>, id: &str) -> Option<&'b SemanticNode> {
        let mut cursor = id;
        while let Some(parent) = index.parent.get(cursor) {
            if let Some(node) = index.by_id.get(*parent) {
                if node.node_type.contains("statement") {
                    return Some(node);
                }
            }
            cursor = parent;
        }
        None
    }
    let paired: HashMap<&str, &str> = matches
        .iter()
        .map(|pair| (pair.old_node.id.as_str(), pair.new_node.id.as_str()))
        .collect();
    // Callee coherence: a matched pair of call-bearing statements whose CALLEES differ
    // (console.log vs console.error) is a dice artifact bootstrapped by a harvested leaf —
    // the pair map cannot be trusted to bless leaves under it. Collect those statement pairs
    // as incoherent; their descendant pairs are pruned along with the leaf pairs they bless.
    fn first_callee_label<'b>(node: &'b SemanticNode) -> Option<&'b str> {
        if matches!(
            node.node_type.as_str(),
            "member_expression" | "identifier" | "property_identifier"
        ) {
            return Some(node.label.as_str());
        }
        for child in &node.children {
            if let Some(label) = first_callee_label(child) {
                return Some(label);
            }
        }
        None
    }
    let incoherent_statements: HashSet<(&str, &str)> = matches
        .iter()
        .filter(|pair| {
            pair.old_node.node_type.contains("statement")
                && pair.old_node.node_type == pair.new_node.node_type
                && !pair.old_node.is_leaf()
                && !pair.new_node.is_leaf()
        })
        .filter(|pair| {
            match (
                first_callee_label(pair.old_node),
                first_callee_label(pair.new_node),
            ) {
                (Some(old_callee), Some(new_callee)) => {
                    old_callee != new_callee
                        // A renamed callee is legitimate when the callee nodes themselves are
                        // a promoted pair elsewhere; same-label requirement keeps this prune
                        // to identity-ambiguous harvests only.
                        && !old_callee.is_empty()
                        && !new_callee.is_empty()
                }
                _ => false,
            }
        })
        .map(|pair| (pair.old_node.id.as_str(), pair.new_node.id.as_str()))
        .collect();
    let keep: Vec<bool> = matches
        .iter()
        .map(|pair| {
            let old_statement = enclosing_statement(old_index, pair.old_node.id.as_str());
            let new_statement = enclosing_statement(new_index, pair.new_node.id.as_str());
            // Any pair inside (or being) an incoherent statement pair goes.
            if incoherent_statements.contains(&(
                pair.old_node.id.as_str(),
                pair.new_node.id.as_str(),
            )) {
                return false;
            }
            if let (Some(old_statement), Some(new_statement)) = (old_statement, new_statement) {
                if incoherent_statements
                    .contains(&(old_statement.id.as_str(), new_statement.id.as_str()))
                {
                    return false;
                }
            }
            if !pair.old_node.is_leaf() || pair.old_node.label != pair.new_node.label {
                return true;
            }
            match (old_statement, new_statement) {
                (Some(old_statement), Some(new_statement)) => {
                    paired.get(old_statement.id.as_str()).copied()
                        == Some(new_statement.id.as_str())
                }
                (None, None) => true,
                _ => false,
            }
        })
        .collect();
    matches
        .into_iter()
        .zip(keep)
        .filter_map(|(pair, keep)| keep.then_some(pair))
        .collect()
}

fn top_down_match_with_existing<'a>(
    old_index: &TreeIndex<'a>,
    new_index: &TreeIndex<'a>,
    min_height: usize,
    existing: Vec<MatchPair<'a>>,
) -> Vec<MatchPair<'a>> {
    let mut new_by_hash: HashMap<&str, Vec<&SemanticNode>> = HashMap::new();
    for node in &new_index.nodes {
        new_by_hash
            .entry(node.structural_hash.as_str())
            .or_default()
            .push(node);
    }

    let mut old_nodes: Vec<&SemanticNode> = old_index
        .nodes
        .iter()
        .copied()
        .filter(|node| {
            old_index
                .heights
                .get(node.id.as_str())
                .copied()
                .unwrap_or(0)
                >= min_height
        })
        .collect();
    old_nodes.sort_by(|a, b| {
        old_index
            .heights
            .get(b.id.as_str())
            .copied()
            .unwrap_or(0)
            .cmp(&old_index.heights.get(a.id.as_str()).copied().unwrap_or(0))
            .then_with(|| a.id.cmp(&b.id))
    });

    let mut matched = existing;
    let mut matched_old: HashSet<&str> = matched
        .iter()
        .map(|pair| pair.old_node.id.as_str())
        .collect();
    let mut matched_new: HashSet<&str> = matched
        .iter()
        .map(|pair| pair.new_node.id.as_str())
        .collect();
    for old_node in old_nodes {
        if matched_old.contains(old_node.id.as_str()) {
            continue;
        }
        let Some(candidates) = new_by_hash.get(old_node.structural_hash.as_str()) else {
            continue;
        };
        let Some(new_node) = candidates
            .iter()
            .copied()
            .filter(|candidate| !matched_new.contains(candidate.id.as_str()))
            .min_by_key(|candidate| {
                (
                    (candidate.position.start_line as i64 - old_node.position.start_line as i64)
                        .abs(),
                    candidate.id.as_str(),
                )
            })
        else {
            continue;
        };
        matched.push(MatchPair { old_node, new_node });
        matched_old.insert(old_node.id.as_str());
        matched_new.insert(new_node.id.as_str());
        seed_descendant_pairs(
            old_node,
            new_node,
            &mut matched,
            &mut matched_old,
            &mut matched_new,
            None,
        );
    }
    matched
}

fn entity_first_match<'a>(
    old_index: &TreeIndex<'a>,
    new_index: &TreeIndex<'a>,
    diagnostics: &mut MatchingDiagnostics,
    detailed_diagnostics: bool,
) -> Vec<MatchPair<'a>> {
    // NB: the ExactId seed is keyed by (id, type, LABEL). A node id is a position path, so
    // same position + same type with a DIFFERENT name is not the same entity — seeding on id
    // alone position-paired swapped/replaced functions (greet<->add) before the structural /
    // label strategies could pair them correctly, which cross-matched their leaves, made a
    // pure swap read style-only, and let a deleted function vanish (issues #12/#31/#32).
    let mut new_by_id_type: HashMap<(&str, &str, &str), Vec<&SemanticNode>> = HashMap::new();
    let mut new_by_hash_type: HashMap<(&str, &str), Vec<&SemanticNode>> = HashMap::new();
    let mut new_by_label_type: HashMap<(&str, &str), Vec<&SemanticNode>> = HashMap::new();
    for node in new_index.named_entities.iter().copied() {
        new_by_id_type
            .entry((
                node.id.as_str(),
                node.node_type.as_str(),
                node.label.as_str(),
            ))
            .or_default()
            .push(node);
        new_by_hash_type
            .entry((node.structural_hash.as_str(), node.node_type.as_str()))
            .or_default()
            .push(node);
        new_by_label_type
            .entry((node.label.as_str(), node.node_type.as_str()))
            .or_default()
            .push(node);
    }

    let mut result = Vec::new();
    let mut matched_old: HashSet<&str> = HashSet::new();
    let mut matched_new: HashSet<&str> = HashSet::new();
    for old_node in old_index.named_entities.iter().copied() {
        if matched_old.contains(old_node.id.as_str()) {
            continue;
        }
        if let Some(new_node) = best_entity_match(
            old_node,
            new_by_id_type.get(&(
                old_node.id.as_str(),
                old_node.node_type.as_str(),
                old_node.label.as_str(),
            )),
            &matched_new,
            old_index,
            new_index,
            EntityStrategy::ExactId,
        ) {
            seed_entity_pair(
                &mut result,
                &mut matched_old,
                &mut matched_new,
                old_node,
                new_node,
                diagnostics,
                EntityStrategy::ExactId,
            );
            continue;
        }
        if let Some(new_node) = best_entity_match(
            old_node,
            new_by_hash_type.get(&(
                old_node.structural_hash.as_str(),
                old_node.node_type.as_str(),
            )),
            &matched_new,
            old_index,
            new_index,
            EntityStrategy::Structural,
        ) {
            seed_entity_pair(
                &mut result,
                &mut matched_old,
                &mut matched_new,
                old_node,
                new_node,
                diagnostics,
                EntityStrategy::Structural,
            );
            continue;
        }
        if let Some(new_node) = best_entity_match(
            old_node,
            new_by_label_type.get(&(old_node.label.as_str(), old_node.node_type.as_str())),
            &matched_new,
            old_index,
            new_index,
            EntityStrategy::LabelParent,
        ) {
            seed_entity_pair(
                &mut result,
                &mut matched_old,
                &mut matched_new,
                old_node,
                new_node,
                diagnostics,
                EntityStrategy::LabelParent,
            );
        }
    }
    if detailed_diagnostics {
        diagnostics.fuzzy_token_candidates = fuzzy_entity_candidate_count(
            &old_index.named_entities,
            &new_index.named_entities,
            &matched_old,
            &matched_new,
            old_index,
            new_index,
        );
    }
    result
}

#[derive(Clone, Copy, Debug)]
enum EntityStrategy {
    ExactId,
    Structural,
    LabelParent,
}

fn best_entity_match<'a>(
    old_node: &'a SemanticNode,
    candidates: Option<&Vec<&'a SemanticNode>>,
    matched_new: &HashSet<&str>,
    old_index: &TreeIndex<'a>,
    new_index: &TreeIndex<'a>,
    strategy: EntityStrategy,
) -> Option<&'a SemanticNode> {
    candidates
        .into_iter()
        .flat_map(|items| items.iter().copied())
        .filter(|candidate| !matched_new.contains(candidate.id.as_str()))
        .filter(|candidate| {
            entity_match_is_conservative(old_node, candidate, old_index, new_index, strategy)
        })
        .min_by_key(|candidate| {
            (
                (candidate.position.start_line as i64 - old_node.position.start_line as i64).abs(),
                candidate.id.as_str(),
            )
        })
}

fn entity_match_is_conservative(
    old_node: &SemanticNode,
    new_node: &SemanticNode,
    old_index: &TreeIndex<'_>,
    new_index: &TreeIndex<'_>,
    strategy: EntityStrategy,
) -> bool {
    if old_node.node_type != new_node.node_type {
        return false;
    }
    if !label_match_parent_compatible(old_node, new_node, old_index, new_index) {
        return false;
    }
    match strategy {
        EntityStrategy::ExactId => {
            old_node.id == new_node.id
                && (old_node.label == new_node.label
                    || old_node.structural_hash == new_node.structural_hash)
        }
        EntityStrategy::Structural => old_node.structural_hash == new_node.structural_hash,
        EntityStrategy::LabelParent => old_node.label == new_node.label,
    }
}

fn seed_entity_pair<'a>(
    result: &mut Vec<MatchPair<'a>>,
    matched_old: &mut HashSet<&'a str>,
    matched_new: &mut HashSet<&'a str>,
    old_node: &'a SemanticNode,
    new_node: &'a SemanticNode,
    diagnostics: &mut MatchingDiagnostics,
    strategy: EntityStrategy,
) {
    if matched_old.contains(old_node.id.as_str()) || matched_new.contains(new_node.id.as_str()) {
        return;
    }
    result.push(MatchPair { old_node, new_node });
    matched_old.insert(old_node.id.as_str());
    matched_new.insert(new_node.id.as_str());
    diagnostics.seeded_matches += 1;
    match strategy {
        EntityStrategy::ExactId => diagnostics.exact_id_matches += 1,
        EntityStrategy::Structural => diagnostics.structural_matches += 1,
        EntityStrategy::LabelParent => diagnostics.label_parent_matches += 1,
    }
    if old_node.structural_hash == new_node.structural_hash {
        seed_descendant_pairs(
            old_node,
            new_node,
            result,
            matched_old,
            matched_new,
            Some(diagnostics),
        );
    }
}

fn seed_descendant_pairs<'a>(
    old_node: &'a SemanticNode,
    new_node: &'a SemanticNode,
    result: &mut Vec<MatchPair<'a>>,
    matched_old: &mut HashSet<&'a str>,
    matched_new: &mut HashSet<&'a str>,
    mut diagnostics: Option<&mut MatchingDiagnostics>,
) {
    for (old_child, new_child) in old_node.children.iter().zip(&new_node.children) {
        if !matched_old.contains(old_child.id.as_str())
            && !matched_new.contains(new_child.id.as_str())
            && old_child.node_type == new_child.node_type
        {
            result.push(MatchPair {
                old_node: old_child,
                new_node: new_child,
            });
            matched_old.insert(old_child.id.as_str());
            matched_new.insert(new_child.id.as_str());
            if let Some(diagnostics) = diagnostics.as_deref_mut() {
                diagnostics.descendant_seeded_matches += 1;
            }
        }
        seed_descendant_pairs(
            old_child,
            new_child,
            result,
            matched_old,
            matched_new,
            diagnostics.as_deref_mut(),
        );
    }
}

fn fuzzy_entity_candidate_count(
    old_entities: &[&SemanticNode],
    new_entities: &[&SemanticNode],
    matched_old: &HashSet<&str>,
    matched_new: &HashSet<&str>,
    old_index: &TreeIndex<'_>,
    new_index: &TreeIndex<'_>,
) -> usize {
    old_entities
        .iter()
        .copied()
        .filter(|old_node| !matched_old.contains(old_node.id.as_str()))
        .filter(|old_node| {
            new_entities.iter().copied().any(|new_node| {
                !matched_new.contains(new_node.id.as_str())
                    && old_node.node_type == new_node.node_type
                    && label_match_parent_compatible(old_node, new_node, old_index, new_index)
                    && token_similarity(&old_node.label, &new_node.label) >= 0.75
            })
        })
        .count()
}

fn token_similarity(left: &str, right: &str) -> f64 {
    let left_tokens = label_tokens(left);
    let right_tokens = label_tokens(right);
    if left_tokens.is_empty() || right_tokens.is_empty() {
        return if left == right { 1.0 } else { 0.0 };
    }
    let common = left_tokens.intersection(&right_tokens).count();
    2.0 * common as f64 / (left_tokens.len() + right_tokens.len()) as f64
}

fn label_tokens(label: &str) -> HashSet<String> {
    let mut tokens = HashSet::new();
    let mut current = String::new();
    for ch in label.chars() {
        if ch.is_ascii_alphanumeric() {
            current.push(ch.to_ascii_lowercase());
        } else if !current.is_empty() {
            tokens.insert(current.clone());
            current.clear();
        }
    }
    if !current.is_empty() {
        tokens.insert(current);
    }
    tokens
}

fn label_match<'a>(
    old_index: &TreeIndex<'a>,
    new_index: &TreeIndex<'a>,
    existing: Vec<MatchPair<'a>>,
) -> Vec<MatchPair<'a>> {
    let mut result = existing;
    let mut matched_old: HashSet<&str> = result
        .iter()
        .map(|pair| pair.old_node.id.as_str())
        .collect();
    let mut matched_new: HashSet<&str> = result
        .iter()
        .map(|pair| pair.new_node.id.as_str())
        .collect();
    // old id -> new id for pairs already established (entity seeds + top-down, extended as
    // this pass adds pairs — nodes iterate pre-order, so parents land before children). Used
    // by the ancestry gate (renamed-but-matched scopes) and to prefer structure-consistent
    // candidates over mere line proximity.
    let mut paired_old_to_new: HashMap<&str, &str> = result
        .iter()
        .map(|pair| (pair.old_node.id.as_str(), pair.new_node.id.as_str()))
        .collect();
    let mut new_by_type_label: HashMap<(&str, &str), Vec<&SemanticNode>> = HashMap::new();
    for node in new_index.nodes.iter().skip(1).copied() {
        if matched_new.contains(node.id.as_str()) {
            continue;
        }
        new_by_type_label
            .entry((node.node_type.as_str(), node.label.as_str()))
            .or_default()
            .push(node);
    }
    for old_node in old_index.nodes.iter().skip(1).copied() {
        if matched_old.contains(old_node.id.as_str()) {
            continue;
        }
        let Some(candidates) =
            new_by_type_label.get_mut(&(old_node.node_type.as_str(), old_node.label.as_str()))
        else {
            continue;
        };
        // Generic structural nodes (block / parameters / return_statement / identifier ...)
        // take their identity from context: they may only label-match inside SAME-NAMED
        // enclosing entities. Without this, the internals of a DELETED function label-matched
        // the internals of an unrelated ADDED function (old_one -> new_one), fabricating
        // literal modifications — whose covered-label suppression then swallowed the real
        // DELETION (issue #31). Named entities are exempt (their name IS their identity).
        // NB: this is deliberately ancestry-based, not matched-parent-based — label matches
        // run BEFORE bottom-up recovery and are its bootstrap, so requiring already-matched
        // parents here would starve bottom-up (it broke in-function variable renames).
        let generic_anchored = |candidate: &SemanticNode| -> bool {
            if is_named_entity_type(old_node.node_type.as_str()) {
                return true;
            }
            let old_entity = enclosing_entity_node(old_index, old_node.id.as_str());
            let new_entity = enclosing_entity_node(new_index, candidate.id.as_str());
            match (old_entity, new_entity) {
                (None, None) => true,
                // Same name, or a renamed-but-already-matched pair (entity seeds and
                // top-down ran before this phase) — both are the same scope.
                (Some(old_entity), Some(new_entity)) => {
                    old_entity.label == new_entity.label
                        || paired_old_to_new.get(old_entity.id.as_str()).copied()
                            == Some(new_entity.id.as_str())
                }
                _ => false,
            }
        };
        // Prefer the candidate whose parent is the matched PARTNER of the old node's parent
        // (structure consistency) before falling back to line proximity. Proximity alone
        // picked a same-label leaf in an inserted sibling statement (the new `if`'s `total`)
        // over the shifted `return`'s `total`, orphaning the latter into ADDITION noise.
        let old_parent_partner: Option<&str> = old_index
            .parent
            .get(old_node.id.as_str())
            .and_then(|parent| paired_old_to_new.get(*parent))
            .copied();
        let Some((position, _)) = candidates
            .iter()
            .enumerate()
            .filter(|(_, candidate)| {
                !matched_new.contains(candidate.id.as_str())
                    && label_match_parent_compatible(old_node, candidate, old_index, new_index)
                    && generic_anchored(candidate)
            })
            .min_by_key(|(_, candidate)| {
                let candidate_parent = new_index.parent.get(candidate.id.as_str()).copied();
                let parent_consistent =
                    matches!((old_parent_partner, candidate_parent), (Some(p), Some(c)) if p == c);
                (
                    if parent_consistent { 0u8 } else { 1u8 },
                    (candidate.position.start_line as i64 - old_node.position.start_line as i64)
                        .abs(),
                    candidate.id.as_str(),
                )
            })
        else {
            continue;
        };
        let new_node = candidates.remove(position);
        result.push(MatchPair { old_node, new_node });
        matched_old.insert(old_node.id.as_str());
        matched_new.insert(new_node.id.as_str());
        paired_old_to_new
            .insert(old_node.id.as_str(), new_node.id.as_str());
    }
    result
}

/// Nearest ancestor that is not a decorator wrapper. `decorated_definition` merely wraps a
/// function/class when a decorator is added — it must be transparent for parent compatibility,
/// or adding `@cached` above `def calc` re-parents `calc` and the matcher refuses to pair the
/// (otherwise identical) definitions, reporting the whole wrapper as an ADDITION (issue #32).
fn effective_parent_node<'a>(index: &TreeIndex<'a>, id: &str) -> Option<&'a SemanticNode> {
    let mut cursor = id;
    while let Some(parent_id) = index.parent.get(cursor) {
        let node = index.by_id.get(*parent_id)?;
        if node.node_type != "decorated_definition" {
            return Some(node);
        }
        cursor = parent_id;
    }
    None
}

/// Nearest named-entity ancestor of a node (its enclosing scope), or None at module level.
fn enclosing_entity_node<'a>(index: &TreeIndex<'a>, id: &str) -> Option<&'a SemanticNode> {
    let mut cursor = id;
    while let Some(parent) = index.parent.get(cursor) {
        if let Some(node) = index.by_id.get(*parent) {
            if is_named_entity_type(node.node_type.as_str()) {
                return Some(node);
            }
        }
        cursor = parent;
    }
    None
}

fn label_match_parent_compatible(
    old_node: &SemanticNode,
    new_node: &SemanticNode,
    old_index: &TreeIndex<'_>,
    new_index: &TreeIndex<'_>,
) -> bool {
    if !matches!(
        old_node.node_type.as_str(),
        "function_definition" | "async_function_def" | "class_definition"
    ) {
        return true;
    }
    let Some(old_parent_node) = effective_parent_node(old_index, old_node.id.as_str()) else {
        return true;
    };
    let Some(new_parent_node) = effective_parent_node(new_index, new_node.id.as_str()) else {
        return true;
    };
    old_parent_node.node_type == new_parent_node.node_type
        && old_parent_node.label == new_parent_node.label
}

fn bottom_up_match<'a>(
    old_index: &TreeIndex<'a>,
    new_index: &TreeIndex<'a>,
    existing: Vec<MatchPair<'a>>,
    min_similarity: f64,
) -> Vec<MatchPair<'a>> {
    let mut result = existing;
    let mut matched_old: HashSet<&str> = result
        .iter()
        .map(|pair| pair.old_node.id.as_str())
        .collect();
    let mut matched_new: HashSet<&str> = result
        .iter()
        .map(|pair| pair.new_node.id.as_str())
        .collect();
    let unmatched_old: Vec<&SemanticNode> = old_index
        .nodes
        .iter()
        .copied()
        .filter(|node| !matched_old.contains(node.id.as_str()) && !node.is_leaf())
        .collect();
    if unmatched_old.is_empty() {
        return result;
    }
    let mut new_by_type: HashMap<&str, Vec<&SemanticNode>> = HashMap::new();
    for node in new_index
        .nodes
        .iter()
        .copied()
        .filter(|node| !matched_new.contains(node.id.as_str()) && !node.is_leaf())
    {
        new_by_type
            .entry(node.node_type.as_str())
            .or_default()
            .push(node);
    }
    let mut desc_new_cache: HashMap<&str, HashSet<&str>> = HashMap::new();
    for candidates in new_by_type.values() {
        for node in candidates {
            let mut ids = HashSet::new();
            collect_descendant_id_refs(node, &mut ids);
            desc_new_cache.insert(node.id.as_str(), ids);
        }
    }
    let mut old_to_new: HashMap<&str, &str> = result
        .iter()
        .map(|pair| (pair.old_node.id.as_str(), pair.new_node.id.as_str()))
        .collect();
    for old_node in unmatched_old {
        let Some(candidates) = new_by_type.get(old_node.node_type.as_str()) else {
            continue;
        };
        let old_desc_count = old_index
            .subtree_sizes
            .get(old_node.id.as_str())
            .copied()
            .unwrap_or(1)
            .saturating_sub(1);
        let mut best_score = min_similarity;
        let mut best_match: Option<&SemanticNode> = None;
        for new_node in candidates {
            if matched_new.contains(new_node.id.as_str()) {
                continue;
            }
            if !bottom_up_match_candidate_compatible(old_node, new_node, old_index, new_index) {
                continue;
            }
            // Scope gate (issue #19, delphi statement scoping): a container may only
            // bottom-up match inside the same-named or already-matched enclosing entity.
            // Without it, dice similarity cross-matched Alpha's and Beta's identical-shaped
            // statements, swapping their string literals between routines.
            let old_entity = enclosing_entity_node(old_index, old_node.id.as_str());
            let new_entity = enclosing_entity_node(new_index, new_node.id.as_str());
            let scope_ok = match (old_entity, new_entity) {
                (None, None) => true,
                (Some(old_scope), Some(new_scope)) => {
                    old_scope.label == new_scope.label
                        || old_to_new.get(old_scope.id.as_str()).copied()
                            == Some(new_scope.id.as_str())
                }
                _ => false,
            };
            if !scope_ok {
                continue;
            }
            let Some(new_desc) = desc_new_cache.get(new_node.id.as_str()) else {
                continue;
            };
            let score = dice_node(
                old_node,
                old_desc_count,
                new_desc,
                &old_to_new,
                old_node.node_type.as_str(),
                new_node.node_type.as_str(),
            );
            if score > best_score {
                best_score = score;
                best_match = Some(new_node);
            }
        }
        if let Some(new_node) = best_match {
            result.push(MatchPair { old_node, new_node });
            matched_old.insert(old_node.id.as_str());
            matched_new.insert(new_node.id.as_str());
            old_to_new.insert(old_node.id.as_str(), new_node.id.as_str());
        }
    }
    result
}

fn bottom_up_match_candidate_compatible(
    old_node: &SemanticNode,
    new_node: &SemanticNode,
    old_index: &TreeIndex<'_>,
    new_index: &TreeIndex<'_>,
) -> bool {
    if is_named_entity_type(old_node.node_type.as_str()) {
        return old_node.label == new_node.label
            && label_match_parent_compatible(old_node, new_node, old_index, new_index);
    }
    true
}

fn dice_node(
    old_node: &SemanticNode,
    old_desc_count: usize,
    new_desc: &HashSet<&str>,
    old_to_new: &HashMap<&str, &str>,
    old_type: &str,
    new_type: &str,
) -> f64 {
    let total = old_desc_count + new_desc.len();
    if total == 0 {
        return if old_type == new_type { 1.0 } else { 0.0 };
    }
    let common = count_common_mapped_descendants(old_node, new_desc, old_to_new);
    2.0 * common as f64 / total as f64
}

fn collect_descendant_id_refs<'a>(node: &'a SemanticNode, result: &mut HashSet<&'a str>) {
    for child in &node.children {
        result.insert(child.id.as_str());
        collect_descendant_id_refs(child, result);
    }
}

fn count_common_mapped_descendants(
    node: &SemanticNode,
    new_desc: &HashSet<&str>,
    old_to_new: &HashMap<&str, &str>,
) -> usize {
    let mut count = 0usize;
    for child in &node.children {
        if old_to_new
            .get(child.id.as_str())
            .is_some_and(|new_id| new_desc.contains(new_id))
        {
            count += 1;
        }
        count += count_common_mapped_descendants(child, new_desc, old_to_new);
    }
    count
}

fn generate_changes<'a>(
    old_root: &'a SemanticNode,
    new_root: &'a SemanticNode,
    matching: &[MatchPair<'a>],
) -> Vec<Value> {
    let report = generate_change_drafts_with_diagnostics(old_root, new_root, matching, None);
    serialize_change_drafts(&report.drafts).changes
}

fn generate_change_drafts_with_diagnostics<'a>(
    old_root: &'a SemanticNode,
    new_root: &'a SemanticNode,
    matching: &[MatchPair<'a>],
    probe: Option<&mut PhaseProbe>,
) -> ChangeGenerationReport<'a> {
    let script = generate_edit_script_with_diagnostics(old_root, new_root, matching, probe);
    let drafts = script.ops.into_iter().map(edit_op_to_draft).collect();
    ChangeGenerationReport {
        drafts,
        diagnostics: script.diagnostics,
    }
}

#[cfg(test)]
fn generate_changes_with_diagnostics<'a>(
    old_root: &'a SemanticNode,
    new_root: &'a SemanticNode,
    matching: &[MatchPair<'a>],
    probe: Option<&mut PhaseProbe>,
) -> ValueChangeGenerationReport {
    let report = generate_change_drafts_with_diagnostics(old_root, new_root, matching, probe);
    let serialized = serialize_change_drafts(&report.drafts);
    ValueChangeGenerationReport {
        changes: serialized.changes,
        diagnostics: report.diagnostics,
    }
}

fn generate_edit_script_with_diagnostics<'a>(
    old_root: &'a SemanticNode,
    new_root: &'a SemanticNode,
    matching: &[MatchPair<'a>],
    probe: Option<&mut PhaseProbe>,
) -> EditScriptReport<'a> {
    let old_index = TreeIndex::new(old_root);
    let new_index = TreeIndex::new(new_root);
    generate_edit_script_with_diagnostics_indexed(&old_index, &new_index, matching, probe)
}

fn generate_edit_script_with_diagnostics_indexed<'a>(
    old_index: &TreeIndex<'a>,
    new_index: &TreeIndex<'a>,
    matching: &[MatchPair<'a>],
    mut probe: Option<&mut PhaseProbe>,
) -> EditScriptReport<'a> {
    let old_to_new: HashMap<&str, &SemanticNode> = matching
        .iter()
        .map(|pair| (pair.old_node.id.as_str(), pair.new_node))
        .collect();
    let matched_new: HashMap<&str, &SemanticNode> = matching
        .iter()
        .map(|pair| (pair.new_node.id.as_str(), pair.old_node))
        .collect();
    let mut ops = Vec::new();
    let mut move_old_descendants: HashSet<&str> = HashSet::new();
    let mut move_new_descendants: HashSet<&str> = HashSet::new();
    let mut move_old_all_descendants: HashSet<&str> = HashSet::new();
    let mut move_new_all_descendants: HashSet<&str> = HashSet::new();
    let mut diagnostics = EditScriptDiagnostics::default();

    measure_value_optional(probe.as_deref_mut(), "rust_edit_move_detection", || {
        for pair in matching {
            let Some(old_pid) = old_index.parent.get(pair.old_node.id.as_str()) else {
                continue;
            };
            let Some(new_pid) = new_index.parent.get(pair.new_node.id.as_str()) else {
                continue;
            };
            // A matched node is a MOVE when its CONTAINER identity changed. Same container
            // iff the old parent is matched to the new parent. The old code bailed
            // (`continue`) whenever the old parent was UNMATCHED — but an unmatched old
            // parent means the container was DELETED, so the matched child necessarily
            // relocated out of it. That silent bail dropped the relocated node from the
            // diff entirely (csharp `name = "guest"` leaving a collapsed `if`, issue #57
            // Root A). Treating "old parent deleted" (and, symmetrically, "new parent added"
            // — its id is never a match target) as a container change fixes it.
            let same_container = old_to_new
                .get(*old_pid)
                .is_some_and(|partner| partner.id.as_str() == *new_pid);
            if !same_container {
                // Decorator wrappers are transparent: adding/removing @decorator re-parents
                // the definition under/out of decorated_definition without relocating it —
                // that is not a MOVE (issue #32).
                let old_effective = effective_parent_node(old_index, pair.old_node.id.as_str());
                let new_effective = effective_parent_node(new_index, pair.new_node.id.as_str());
                let effectively_same_parent = match (old_effective, new_effective) {
                    (Some(old_parent), Some(new_parent)) => old_to_new
                        .get(old_parent.id.as_str())
                        .is_some_and(|partner| partner.id == new_parent.id),
                    _ => false,
                };
                if effectively_same_parent {
                    continue;
                }
                ops.push(EditOp {
                    kind: "MOVE",
                    old_node: Some(pair.old_node),
                    new_node: Some(pair.new_node),
                    old_index: None,
                    new_index: None,
                });
                diagnostics.move_candidates += 1;
                for desc in pair.old_node.descendants() {
                    move_old_all_descendants.insert(desc.id.as_str());
                    if !old_to_new.contains_key(desc.id.as_str()) {
                        move_old_descendants.insert(desc.id.as_str());
                    }
                }
                for desc in pair.new_node.descendants() {
                    move_new_all_descendants.insert(desc.id.as_str());
                    if !matched_new.contains_key(desc.id.as_str()) {
                        move_new_descendants.insert(desc.id.as_str());
                    }
                }
            }
        }
    });

    let pruned_old_deletes = unmatched_named_entity_descendant_ids(&old_index.nodes, |node| {
        !old_to_new.contains_key(node.id.as_str())
    });
    let pruned_new_additions = unmatched_named_entity_descendant_ids(&new_index.nodes, |node| {
        !matched_new.contains_key(node.id.as_str())
    });

    measure_value_optional(probe.as_deref_mut(), "rust_edit_delete_generation", || {
        for node in old_index.nodes.iter().rev() {
            if old_to_new.contains_key(node.id.as_str()) {
                continue;
            }
            if move_old_descendants.contains(node.id.as_str())
                || pruned_old_deletes.contains(node.id.as_str())
            {
                diagnostics.pruned_old_descendant_deletes += 1;
                continue;
            }
            ops.push(EditOp {
                kind: "DELETE",
                old_node: Some(node),
                new_node: None,
                old_index: None,
                new_index: None,
            });
            diagnostics.delete_candidates += 1;
        }
    });
    measure_value_optional(probe.as_deref_mut(), "rust_edit_add_generation", || {
        for node in &new_index.nodes {
            if matched_new.contains_key(node.id.as_str()) {
                continue;
            }
            if move_new_descendants.contains(node.id.as_str())
                || pruned_new_additions.contains(node.id.as_str())
            {
                diagnostics.pruned_new_descendant_additions += 1;
                continue;
            }
            ops.push(EditOp {
                kind: "INSERT",
                old_node: None,
                new_node: Some(node),
                old_index: None,
                new_index: None,
            });
            diagnostics.add_candidates += 1;
        }
    });
    measure_value_optional(probe.as_deref_mut(), "rust_edit_update_generation", || {
        for pair in matching {
            if pair.old_node.label != pair.new_node.label || should_report_structural_update(pair) {
                let kind = if update_looks_moved(
                    pair,
                    &old_index.parent,
                    &new_index.parent,
                    &old_to_new,
                ) {
                    "UPDATE_MOVED"
                } else {
                    "UPDATE"
                };
                ops.push(EditOp {
                    kind,
                    old_node: Some(pair.old_node),
                    new_node: Some(pair.new_node),
                    old_index: None,
                    new_index: None,
                });
                diagnostics.update_candidates += 1;
            }
        }
    });


    // Moved-pair literal update recovery (issue #57 js "Moved Code" / python #37 family):
    // unmatched leaves INSIDE a moved subtree are pruned from DELETE/INSERT (ride-along noise),
    // but an unmatched literal on the old side with a scored counterpart on the new side of the
    // SAME moved pair is the internal edit itself (`'md5'` -> `'sha256'` inside a relocated
    // calc_hash). The score is python refinement._leaf_pair_score, move-pair scoped: same
    // literal type + same PARENT type (the `{flag: 'r'}` string under a pair never competes
    // with the createHash argument), position/first-char/parent-label bonuses. Cross-pair
    // leaves never pair (the batch-signature oracle rejected absolute/relative-slot repairs).
    {
        let move_pairs: Vec<(&SemanticNode, &SemanticNode)> = ops
            .iter()
            .filter(|op| op.kind == "MOVE")
            .filter_map(|op| Some((op.old_node?, op.new_node?)))
            .collect();
        let mut candidates: Vec<(f64, &SemanticNode, &SemanticNode)> = Vec::new();
        let literal_like = |node_type: &str| {
            node_type.contains("string")
                || node_type.contains("number")
                || node_type.contains("integer")
                || node_type.contains("float")
                || node_type.contains("literal")
                || node_type.contains("fragment")
        };
        for (old_root, new_root) in move_pairs {
            let old_sub = semantic_node_refs_by_id_with_root(old_root);
            let new_sub = semantic_node_refs_by_id_with_root(new_root);
            let parent_in = |leaf: &SemanticNode, sub: &HashMap<&str, &SemanticNode>| {
                leaf.id
                    .rsplit_once('.')
                    .and_then(|(pid, _)| sub.get(pid).copied())
                    .map(|p| (p.node_type.clone(), p.label.clone()))
            };
            let mut old_by_type: HashMap<String, Vec<&SemanticNode>> = HashMap::new();
            for leaf in old_root.descendants() {
                if leaf.children.is_empty()
                    && !leaf.label.is_empty()
                    && literal_like(&leaf.node_type.to_lowercase())
                    && !old_to_new.contains_key(leaf.id.as_str())
                {
                    old_by_type.entry(leaf.node_type.clone()).or_default().push(leaf);
                }
            }
            let mut new_by_type: HashMap<String, Vec<&SemanticNode>> = HashMap::new();
            for leaf in new_root.descendants() {
                if leaf.children.is_empty()
                    && !leaf.label.is_empty()
                    && literal_like(&leaf.node_type.to_lowercase())
                    && !matched_new.contains_key(leaf.id.as_str())
                {
                    new_by_type.entry(leaf.node_type.clone()).or_default().push(leaf);
                }
            }
            let mut used_new: HashSet<String> = HashSet::new();
            let mut sorted_types: Vec<String> = old_by_type.keys().cloned().collect();
            sorted_types.sort();
            // Equal-label leaves consume each other SILENTLY before any scoring: a value
            // that survives the move unchanged is covered by the MOVE itself, and skipping
            // (rather than consuming) equal labels let the greedy residue cross-pair —
            // go's relocated `add(3, 4)` fabricated 3→4 and 4→3 int_literal updates.
            let mut stable_old: HashSet<String> = HashSet::new();
            for node_type in &sorted_types {
                let old_leaves = &old_by_type[node_type];
                let Some(new_leaves) = new_by_type.get(node_type) else {
                    continue;
                };
                for old_leaf in old_leaves {
                    let equal = new_leaves.iter().find(|new_leaf| {
                        !used_new.contains(new_leaf.id.as_str())
                            && new_leaf.label == old_leaf.label
                    });
                    if let Some(new_leaf) = equal {
                        used_new.insert(new_leaf.id.clone());
                        stable_old.insert(old_leaf.id.clone());
                    }
                }
            }
            for node_type in sorted_types {
                let old_leaves = &old_by_type[&node_type];
                let Some(new_leaves) = new_by_type.get(&node_type) else {
                    continue;
                };
                for old_leaf in old_leaves {
                    if stable_old.contains(old_leaf.id.as_str()) {
                        continue;
                    }
                    let old_parent = parent_in(old_leaf, &old_sub);
                    let mut best: Option<(&SemanticNode, f64)> = None;
                    for new_leaf in new_leaves {
                        if used_new.contains(new_leaf.id.as_str())
                            || old_leaf.label == new_leaf.label
                        {
                            continue;
                        }
                        let new_parent = parent_in(new_leaf, &new_sub);
                        let (Some(op_sig), Some(np_sig)) = (&old_parent, &new_parent) else {
                            continue;
                        };
                        if op_sig.0 != np_sig.0 {
                            continue;
                        }
                        let line_delta = (old_leaf.position.start_line as i64
                            - new_leaf.position.start_line as i64)
                            .unsigned_abs()
                            .min(20) as f64;
                        let position_bonus = (0.2f64 - line_delta / 100.0).max(0.0);
                        let label_bonus = if old_leaf
                            .label
                            .chars()
                            .next()
                            .map(|c| c.to_ascii_lowercase())
                            == new_leaf.label.chars().next().map(|c| c.to_ascii_lowercase())
                        {
                            0.1
                        } else {
                            0.0
                        };
                        let parent_label_bonus = if !op_sig.1.is_empty() && op_sig.1 == np_sig.1 {
                            0.1
                        } else {
                            0.0
                        };
                        // Declarator-context signal: the nearest LABELED ancestor inside the
                        // moved subtree names the slot the value lives in (`const hash = …`
                        // vs `const bytes = …`). Same name on both sides outweighs raw line
                        // proximity — without it `'r'` (under bytes) beat `'sha256'` (under
                        // hash) for old md5 purely by being one line closer.
                        let nearest_labeled = |leaf: &SemanticNode,
                                               sub: &HashMap<&str, &SemanticNode>|
                         -> Option<String> {
                            let mut cursor = leaf.id.as_str();
                            while let Some((pid, _)) = cursor.rsplit_once('.') {
                                let Some(node) = sub.get(pid) else { break };
                                if !node.label.is_empty() && node.label != leaf.label {
                                    return Some(node.label.clone());
                                }
                                cursor = pid;
                            }
                            None
                        };
                        let context_bonus = match (
                            nearest_labeled(old_leaf, &old_sub),
                            nearest_labeled(new_leaf, &new_sub),
                        ) {
                            (Some(old_ctx), Some(new_ctx)) if old_ctx == new_ctx => 0.15,
                            _ => 0.0,
                        };
                        let score = (0.6
                            + position_bonus
                            + label_bonus
                            + parent_label_bonus
                            + context_bonus)
                            .min(0.95);
                        if best.is_none() || score > best.unwrap().1 {
                            best = Some((new_leaf, score));
                        }
                    }
                    if let Some((new_leaf, score)) = best {
                        used_new.insert(new_leaf.id.clone());
                        candidates.push((score, old_leaf, new_leaf));
                    }
                }
            }
        }
        // Nested MOVE pairs (a container and its parent can both change parents) each see
        // the same unmatched leaves; without GLOBAL uniqueness the same old leaf recovers
        // once per covering pair (js stage4: md5 paired with BOTH sha256 and 'r'). One
        // edit per leaf: best score wins across all pairs.
        candidates.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.1.id.cmp(&b.1.id))
        });
        let mut emitted_old: HashSet<&str> = HashSet::new();
        let mut emitted_new: HashSet<&str> = HashSet::new();
        for (_, old_leaf, new_leaf) in candidates {
            if !emitted_old.insert(old_leaf.id.as_str())
                || !emitted_new.insert(new_leaf.id.as_str())
            {
                continue;
            }
            ops.push(EditOp {
                kind: "UPDATE_MOVED",
                old_node: Some(old_leaf),
                new_node: Some(new_leaf),
                old_index: None,
                new_index: None,
            });
            diagnostics.update_candidates += 1;
        }
    }

    measure_value_optional(probe.as_deref_mut(), "rust_edit_reorder_generation", || {
        for pair in matching {
            if move_old_descendants.contains(pair.old_node.id.as_str())
                || move_old_all_descendants.contains(pair.old_node.id.as_str())
                || move_new_all_descendants.contains(pair.new_node.id.as_str())
            {
                diagnostics.skipped_reorders_under_moves += 1;
                continue;
            }
            let Some(old_pid) = old_index.parent.get(pair.old_node.id.as_str()) else {
                continue;
            };
            let Some(new_pid) = new_index.parent.get(pair.new_node.id.as_str()) else {
                continue;
            };
            let Some(old_parent_partner) = old_to_new.get(*old_pid) else {
                continue;
            };
            if old_parent_partner.id.as_str() != *new_pid {
                continue;
            }
            let Some(old_siblings) = old_index.children.get(*old_pid) else {
                continue;
            };
            let Some(new_siblings) = new_index.children.get(*new_pid) else {
                continue;
            };
            let Some(old_index) = old_siblings
                .iter()
                .position(|id| *id == pair.old_node.id.as_str())
            else {
                continue;
            };
            let Some(new_index) = new_siblings
                .iter()
                .position(|id| *id == pair.new_node.id.as_str())
            else {
                continue;
            };
            if old_index != new_index {
                ops.push(EditOp {
                    kind: "REORDER",
                    old_node: Some(pair.old_node),
                    new_node: Some(pair.new_node),
                    old_index: Some(old_index),
                    new_index: Some(new_index),
                });
                diagnostics.reorder_candidates += 1;
            }
        }
    });

    diagnostics.pre_refinement_change_count = ops.len();
    diagnostics.initial_draft_count = ops.len();
    diagnostics.pruned_before_draft_count = diagnostics.pruned_old_descendant_deletes
        + diagnostics.pruned_new_descendant_additions
        + diagnostics.skipped_reorders_under_moves;
    EditScriptReport { ops, diagnostics }
}

fn unmatched_named_entity_descendant_ids<'a, F>(
    nodes: &[&'a SemanticNode],
    is_unmatched: F,
) -> HashSet<&'a str>
where
    F: Fn(&SemanticNode) -> bool,
{
    let mut result = HashSet::new();
    for node in nodes {
        if !is_named_entity_type(node.node_type.as_str()) || !is_unmatched(node) {
            continue;
        }
        for descendant in node.descendants() {
            result.insert(descendant.id.as_str());
        }
    }
    result
}

/// Human-readable intent for a change on an ignore-file node (issue #58). The gitignore
/// parser emits `pattern` / `negated_pattern` / `comment` nodes, so the engine — not a
/// per-frontend explainer — can say what an ignore edit MEANS. Frontend-agnostic: the
/// review tree, CodeLens, CLI, and release notes all read `Change.description`. Returns
/// None for every other node type, leaving the structural description untouched. The
/// "which tracked files this untracks" impact needs the working tree and stays a frontend
/// concern.
fn ignore_intent_description(
    change_type: &str,
    old_node: Option<&SemanticNode>,
    new_node: Option<&SemanticNode>,
) -> Option<String> {
    let node = new_node.or(old_node)?;
    let node_type = node.node_type.as_str();
    let is_pattern = matches!(node_type, "pattern" | "negated_pattern");
    let is_comment = node_type == "comment";
    if !is_pattern && !is_comment {
        return None;
    }
    let target = node.label.trim();
    if target.is_empty() {
        return None;
    }
    let bare = target.trim_start_matches('!').trim();
    let negation = node_type == "negated_pattern";
    let description = match (change_type, is_comment) {
        ("ADDITION", false) if negation => {
            format!("Adds an exception for {bare} (no longer ignored)")
        }
        ("ADDITION", false) => format!("Adds an ignore rule for {target}"),
        ("DELETION", false) if negation => format!("Removes the exception for {bare}"),
        ("DELETION", false) => format!("Stops ignoring {target}"),
        ("MODIFICATION", false) => {
            let before = old_node.map(|n| n.label.trim()).unwrap_or_default();
            let after = new_node.map(|n| n.label.trim()).unwrap_or_default();
            if !before.is_empty() && !after.is_empty() && before != after {
                format!("Changes ignore rule {before} -> {after}")
            } else {
                format!("Updates the ignore rule {target}")
            }
        }
        ("ADDITION", true) => format!("Adds comment {target}"),
        ("DELETION", true) => format!("Removes comment {target}"),
        ("MODIFICATION", true) => format!("Edits comment {target}"),
        _ => return None,
    };
    Some(description)
}

fn edit_op_to_draft(op: EditOp<'_>) -> ChangeDraft<'_> {
    let change_type = match op.kind {
        "INSERT" => "ADDITION",
        "DELETE" => "DELETION",
        "UPDATE" | "UPDATE_MOVED" => "MODIFICATION",
        "MOVE" => "MOVE",
        "REORDER" => "REORDER",
        _ => op.kind,
    };
    let mut desc_parts = vec![if op.kind == "UPDATE_MOVED" {
        "Update moved".to_owned()
    } else {
        capitalize(op.kind)
    }];
    if let Some(old_node) = op.old_node {
        desc_parts.push(format_node_ref(old_node));
    }
    if let Some(new_node) = op.new_node {
        desc_parts.push(format!("-> {}", format_node_ref(new_node)));
    }
    if op.kind == "REORDER" {
        if let (Some(old_index), Some(new_index)) = (op.old_index, op.new_index) {
            desc_parts.push(format!("[{old_index} -> {new_index}]"));
        }
    }
    let description = ignore_intent_description(change_type, op.old_node, op.new_node)
        .unwrap_or_else(|| desc_parts.join(" "));
    ChangeDraft {
        change_type,
        old_node: op.old_node,
        new_node: op.new_node,
        old_index: op.old_index,
        new_index: op.new_index,
        confidence: 1.0,
        description,
        refactoring_kind: None,
        text_diff: None,
    }
}

fn serialize_change_drafts(drafts: &[ChangeDraft<'_>]) -> SerializedChanges {
    serialize_change_drafts_with_size_maps(drafts, None, None)
}

fn serialize_change_drafts_fast(drafts: &[ChangeDraft<'_>]) -> SerializedChanges {
    SerializedChanges {
        changes: drafts.iter().map(draft_to_change).collect(),
        json_nodes_serialized_count: 0,
    }
}

fn serialize_change_drafts_with_size_maps(
    drafts: &[ChangeDraft<'_>],
    old_subtree_sizes: Option<&HashMap<&str, usize>>,
    new_subtree_sizes: Option<&HashMap<&str, usize>>,
) -> SerializedChanges {
    let mut json_nodes_serialized_count = 0usize;
    let changes = drafts
        .iter()
        .map(|draft| {
            json_nodes_serialized_count += draft
                .old_node
                .map(|node| semantic_node_subtree_size_cached(node, old_subtree_sizes))
                .unwrap_or_default();
            json_nodes_serialized_count += draft
                .new_node
                .map(|node| semantic_node_subtree_size_cached(node, new_subtree_sizes))
                .unwrap_or_default();
            draft_to_change(draft)
        })
        .collect();
    SerializedChanges {
        changes,
        json_nodes_serialized_count,
    }
}

fn draft_to_change(draft: &ChangeDraft<'_>) -> Value {
    let mut value = serde_json::Map::with_capacity(8);
    value.insert(
        "change_type".to_owned(),
        Value::String(draft.change_type.to_owned()),
    );
    value.insert(
        "confidence".to_owned(),
        serde_json::Number::from_f64(draft.confidence)
            .map(Value::Number)
            .unwrap_or_else(|| json!(draft.confidence)),
    );
    value.insert(
        "description".to_owned(),
        Value::String(draft.description.clone()),
    );
    if let Some(old_node) = draft.old_node {
        value.insert("old_node".to_owned(), json!(old_node));
    }
    if let Some(new_node) = draft.new_node {
        value.insert("new_node".to_owned(), json!(new_node));
    }
    if let Some(old_index) = draft.old_index {
        value.insert("old_index".to_owned(), json!(old_index));
    }
    if let Some(new_index) = draft.new_index {
        value.insert("new_index".to_owned(), json!(new_index));
    }
    if let Some(refactoring_kind) = draft.refactoring_kind {
        value.insert(
            "refactoring_kind".to_owned(),
            Value::String(refactoring_kind.to_owned()),
        );
    }
    if let Some(text_diff) = &draft.text_diff {
        value.insert("text_diff".to_owned(), json!(text_diff));
    }
    // Fact delta (#178): what MOVED between the two fact bags, not what they hold. Derived
    // here so every binding reads the same finding instead of each re-deriving it — this
    // was extension-only TypeScript, invisible to the Go/Java skins. Omitted entirely when
    // empty or when either side has no facts, so consumers can treat presence as meaning.
    if let (Some(old_node), Some(new_node)) = (draft.old_node, draft.new_node) {
        if let (Some(before), Some(after)) = (&old_node.facts, &new_node.facts) {
            let delta = compute_fact_delta(before, after);
            if !delta.is_empty() {
                value.insert("fact_delta".to_owned(), Value::Array(delta));
            }
        }
    }
    Value::Object(value)
}

fn semantic_node_subtree_size(node: &SemanticNode) -> usize {
    1 + node.descendants().len()
}

fn semantic_node_subtree_size_cached(
    node: &SemanticNode,
    sizes: Option<&HashMap<&str, usize>>,
) -> usize {
    sizes
        .and_then(|items| items.get(node.id.as_str()).copied())
        .unwrap_or_else(|| semantic_node_subtree_size(node))
}

fn format_node_ref(node: &SemanticNode) -> String {
    format!("{}('{}')", node.node_type, node.label.replace('\'', "\\'"))
}

fn should_report_structural_update(pair: &MatchPair<'_>) -> bool {
    pair.old_node.structural_hash != pair.new_node.structural_hash
        && matches!(
            pair.old_node.node_type.as_str(),
            "if_statement"
                | "elif_clause"
                | "else_clause"
                | "for_statement"
                | "while_statement"
                | "try_statement"
                | "except_clause"
                | "with_statement"
                | "return_statement"
                | "assignment"
                | "augmented_assignment"
                | "expression_statement"
                | "call"
                | "binary_operator"
        )
}

fn update_looks_moved(
    pair: &MatchPair<'_>,
    old_parent: &HashMap<&str, &str>,
    new_parent: &HashMap<&str, &str>,
    old_to_new: &HashMap<&str, &SemanticNode>,
) -> bool {
    if pair.old_node.position.start_line != pair.new_node.position.start_line {
        return true;
    }
    let Some(old_pid) = old_parent.get(pair.old_node.id.as_str()) else {
        return false;
    };
    let Some(new_pid) = new_parent.get(pair.new_node.id.as_str()) else {
        return false;
    };
    old_to_new
        .get(*old_pid)
        .is_some_and(|partner| partner.id.as_str() != *new_pid)
}

fn add_delete_noise_count_drafts(changes: &[ChangeDraft<'_>]) -> usize {
    changes
        .iter()
        .filter(|change| matches!(change.change_type, "ADDITION" | "DELETION"))
        .count()
}

fn refine_candidate_drafts<'a>(
    changes: &mut Vec<ChangeDraft<'a>>,
    matching: &[MatchPair<'a>],
    mut probe: Option<&mut PhaseProbe>,
    language: &str,
) {
    finalize_debug_probe("refine:input", changes);
    measure_value_optional(
        probe.as_deref_mut(),
        "rust_refine_promote_matched_parent_updates",
        || promote_matched_parent_statement_updates_drafts(changes, matching),
    );
    finalize_debug_probe("refine:promote_matched_parent_updates", changes);
    measure_value_optional(
        probe.as_deref_mut(),
        "rust_refine_promote_same_id_line_moves",
        || promote_same_id_named_line_moves_drafts(changes, matching),
    );
    finalize_debug_probe("refine:promote_same_id_line_moves", changes);
    measure_value_optional(
        probe.as_deref_mut(),
        "rust_refine_promote_same_id_renames",
        || promote_same_id_named_renames_from_add_delete_drafts(changes),
    );
    finalize_debug_probe("refine:promote_same_id_renames", changes);
    measure_value_optional(
        probe.as_deref_mut(),
        "rust_refine_promote_imported_renames",
        || promote_imported_function_variable_renames_drafts(changes),
    );
    finalize_debug_probe("refine:promote_imported_renames", changes);
    measure_value_optional(
        probe.as_deref_mut(),
        "rust_refine_promote_label_updates_inside_moves",
        || promote_label_updates_inside_moved_entities_drafts(changes),
    );
    finalize_debug_probe("refine:promote_label_updates_inside_moves", changes);
    measure_value_optional(
        probe.as_deref_mut(),
        "rust_refine_suppress_same_label_delete",
        || suppress_same_label_function_delete_for_addition_drafts(changes),
    );
    finalize_debug_probe("refine:suppress_same_label_delete", changes);
    measure_value_optional(
        probe.as_deref_mut(),
        "rust_refine_suppress_descendant_noise",
        || suppress_descendant_noise_drafts(changes),
    );
    finalize_debug_probe("refine:suppress_descendant_noise", changes);
    measure_value_optional(
        probe.as_deref_mut(),
        "rust_refine_suppress_move_noise",
        || suppress_candidate_move_noise_drafts(changes),
    );
    finalize_debug_probe("refine:suppress_move_noise", changes);
    measure_value_optional(
        probe.as_deref_mut(),
        "rust_refine_suppress_child_modifications",
        || suppress_child_modifications_under_preferred_parent_drafts(changes),
    );
    finalize_debug_probe("refine:suppress_child_modifications", changes);
    measure_value_optional(
        probe.as_deref_mut(),
        "rust_refine_suppress_parent_modifications",
        || suppress_parent_modifications_drafts(changes, language),
    );
    finalize_debug_probe("refine:suppress_parent_modifications", changes);
    measure_value_optional(
        probe.as_deref_mut(),
        "rust_refine_suppress_container_noise",
        || suppress_candidate_container_noise_drafts(changes, matching),
    );
    finalize_debug_probe("refine:suppress_container_noise", changes);
    measure_value_optional(
        probe.as_deref_mut(),
        "rust_refine_annotate_moved_context",
        || annotate_moved_context_descriptions_drafts(changes),
    );
    measure_value_optional(
        probe.as_deref_mut(),
        "rust_refine_annotate_leaf_text",
        || annotate_leaf_text_diffs_drafts(changes),
    );
    measure_value_optional(probe.as_deref_mut(), "rust_refine_sort_changes", || {
        sort_candidate_drafts(changes)
    });
}

// Env-gated draft tracer for parity debugging: set INTENTUMDIFF_FINALIZE_DEBUG=1 to see
// the surviving drafts after each finalize/refine pass on stderr. (Plain comment — a
// macro invocation cannot carry an outer doc comment.)
thread_local! {
    /// Per-pass trace for diagnostics mode (issue #54): None = off; Some = collecting
    /// (pass name, surviving change count) after every probed refine/finalize pass.
    static FINALIZE_TRACE: std::cell::RefCell<Option<Vec<(String, usize)>>> =
        const { std::cell::RefCell::new(None) };
}

fn finalize_trace_start() {
    FINALIZE_TRACE.with(|trace| *trace.borrow_mut() = Some(Vec::new()));
}

fn finalize_trace_take() -> Vec<(String, usize)> {
    FINALIZE_TRACE.with(|trace| trace.borrow_mut().take().unwrap_or_default())
}

fn finalize_debug_probe(stage: &str, changes: &[ChangeDraft<'_>]) {
    FINALIZE_TRACE.with(|trace| {
        if let Some(records) = trace.borrow_mut().as_mut() {
            records.push((stage.to_string(), changes.len()));
        }
    });
    if std::env::var("INTENTUMDIFF_FINALIZE_DEBUG").is_err() {
        return;
    }
    let summary: Vec<String> = changes
        .iter()
        .map(|change| {
            let node = change.new_node.or(change.old_node);
            format!(
                "{}:{}({})",
                change.change_type,
                node.map(|n| n.node_type.as_str()).unwrap_or("?"),
                node.map(|n| n.label.as_str()).unwrap_or(""),
            )
        })
        .collect();
    eprintln!("[finalize-debug] {stage}: [{}]", summary.join(", "));
}

macro_rules! probed {
    ($changes:expr, $name:literal, $call:expr) => {{
        $call;
        finalize_debug_probe($name, $changes);
    }};
}

fn finalize_python_review_drafts<'a>(
    changes: &mut Vec<ChangeDraft<'a>>,
    old_tree: &'a SemanticNode,
    new_tree: &'a SemanticNode,
    old_source: &str,
    new_source: &str,
    finalization: &mut PythonReviewFinalization,
    language: &str,
) {
    finalize_debug_probe("finalize:input", changes);
    probed!(changes, "promote_named_additions_to_moves", promote_named_additions_to_moves_from_old_tree(changes, old_tree));
    probed!(changes, "promote_same_id_identifier_renames", promote_same_id_identifier_renames_from_add_delete_drafts(changes));
    probed!(changes, "promote_python_signature_changes", promote_python_signature_changes_from_sources(
        changes, old_tree, new_tree, old_source, new_source,
    ));
    probed!(changes, "promote_signature_changes_from_annotations", promote_signature_changes_from_annotations_drafts(
        changes, old_tree, new_tree,
    ));
    probed!(changes, "promote_parameter_renames", promote_parameter_renames_from_signature_changes(changes));
    probed!(changes, "promote_parameter_identifier_renames", promote_parameter_identifier_modification_renames(changes, old_tree, new_tree));
    probed!(changes, "promote_moved_empty_read_condition", promote_moved_empty_read_condition_updates(changes));
    probed!(changes, "promote_descendant_leaf_updates", promote_descendant_leaf_updates_drafts(changes));
    probed!(changes, "promote_tree_leaf_value_updates", promote_tree_leaf_value_updates_drafts(changes, old_tree, new_tree, language));
    probed!(changes, "promote_unique_domain_string_labels", promote_unique_domain_string_label_updates_drafts(changes, old_tree, new_tree));
    probed!(changes, "promote_source_string_literal_updates", promote_source_string_literal_updates_drafts(
        changes, old_tree, new_tree, old_source, new_source,
    ));
    probed!(changes, "promote_string_concat_to_fstring", promote_string_concat_to_fstring_modifications(changes));
    // Runs after the leaf-update promoters have created the body identifier MODIFICATIONs it
    // classifies — a body reference rename corroborated by an anchored callable's param rename.
    probed!(changes, "promote_corroborated_variable_renames", promote_corroborated_variable_renames(changes, old_tree, new_tree));
    // One review event per rename: entity anchoring matches every occurrence of a renamed
    // identifier (issue #57 anchors port), and each matched occurrence would otherwise promote to
    // its own RENAME_VARIABLE — the python side reports a scoped rename ONCE.
    probed!(changes, "dedupe_variable_renames", dedupe_variable_rename_drafts(changes));
    probed!(changes, "suppress_add_delete_by_signature", suppress_add_delete_noise_covered_by_signature_refactorings(changes));
    probed!(changes, "suppress_deletions_by_literal_mods", suppress_deletions_covered_by_literal_modifications(changes));
    // The no-delta filter must run BEFORE the pairings suppression: a same-label
    // MODIFICATION with no id-stable leaf delta is about to die, so it must not first
    // swallow the ADD+DELETE pair it covers — that composition erased the go
    // error-wrapping edit entirely (issue #57 pilot: only the import addition survived).
    probed!(changes, "suppress_same_label_mods_no_delta", suppress_same_label_modifications_without_leaf_label_delta(changes));
    probed!(changes, "suppress_add_delete_by_pairings", suppress_add_delete_drafts_covered_by_pairings(changes));
    probed!(changes, "suppress_same_label_add_delete_pairs", suppress_same_label_add_delete_pair_drafts(changes, language));
    probed!(changes, "promote_removed_print_calls", promote_removed_print_call_deletions_from_source(changes, old_tree, new_source));
    probed!(changes, "suppress_mods_by_refactoring_labels", suppress_modifications_covered_by_refactoring_labels(changes));
    probed!(changes, "suppress_child_moves_under_refactoring_pairs", suppress_child_moves_under_refactoring_pair_drafts(changes));
    probed!(changes, "suppress_parent_modifications", suppress_parent_modifications_drafts(changes, language));
    probed!(changes, "suppress_child_mods_under_parent", suppress_child_modifications_under_preferred_parent_drafts(changes));
    let (suppressed_reorders, promoted_indices) =
        suppress_low_signal_reorders_drafts(changes);
    if !promoted_indices.is_empty() {
        // python refinement.entity_reorder_to_moved_code: the promoted entity reorders
        // are one review event of moved code.
        let recent: Vec<&ChangeDraft<'_>> = promoted_indices
            .iter()
            .filter_map(|&idx| changes.get(idx))
            .collect();
        finalization.change_groups.push(json!({
            "kind": "MOVED_CODE",
            "raw_change_indices": [],
            "old_labels": recent
                .iter()
                .filter_map(|c| c.old_node.map(|n| n.label.clone()))
                .collect::<Vec<_>>(),
            "new_labels": recent
                .iter()
                .filter_map(|c| c.new_node.map(|n| n.label.clone()))
                .collect::<Vec<_>>(),
            "old_node_ids": recent
                .iter()
                .filter_map(|c| c.old_node.map(|n| n.id.clone()))
                .collect::<Vec<_>>(),
            "new_node_ids": recent
                .iter()
                .filter_map(|c| c.new_node.map(|n| n.id.clone()))
                .collect::<Vec<_>>(),
            "confidence": 0.85,
            "rule_id": "refinement.entity_reorder_to_moved_code",
            "metadata": {"reordered_count": recent.len()},
        }));
    }
    if suppressed_reorders > 0 {
        finalization.change_groups.push(json!({
            "kind": "NOISE_SUPPRESSED",
            "raw_change_indices": [],
            "old_labels": [],
            "new_labels": [],
            "old_node_ids": [],
            "new_node_ids": [],
            "confidence": 0.8,
            "rule_id": "refinement.suppress_low_signal_reorders",
            "metadata": {"suppressed_count": suppressed_reorders},
        }));
        // The formatting-equivalence relabel is a PYTHON style rule; it carried
        // "python.formatting.call_wrapping_equivalence" into a ts function swap.
        if language == "python" {
            finalization
                .change_groups
                .push(python_formatting_equivalence_group(changes));
        }
    }
    add_compact_superseded_group_for_refactorings(changes, finalization);
    apply_python_literal_invariances(
        changes,
        old_tree,
        new_tree,
        old_source,
        new_source,
        finalization,
    );
    sort_candidate_drafts(changes);
}

mod draft_promotions;
use draft_promotions::*;


/// Positions (into `sequence`) of one longest strictly-increasing subsequence.
fn longest_increasing_subsequence_positions(sequence: &[usize]) -> HashSet<usize> {
    let n = sequence.len();
    if n == 0 {
        return HashSet::new();
    }
    let mut lengths = vec![1usize; n];
    let mut prev = vec![usize::MAX; n];
    let mut best_end = 0;
    for i in 0..n {
        for j in 0..i {
            if sequence[j] < sequence[i] && lengths[j] + 1 > lengths[i] {
                lengths[i] = lengths[j] + 1;
                prev[i] = j;
            }
        }
        if lengths[i] > lengths[best_end] {
            best_end = i;
        }
    }
    let mut positions = HashSet::new();
    let mut cursor = best_end;
    loop {
        positions.insert(cursor);
        if prev[cursor] == usize::MAX {
            break;
        }
        cursor = prev[cursor];
    }
    positions
}

fn rust_finalize_stage11_value(request: &Value) -> Result<Value, String> {
    let language = request
        .get("language")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !language.eq_ignore_ascii_case("python") {
        return Ok(json!({
            "schema_version": 1,
            "status": FALLBACK,
            "certified": false,
            "engine": "rust_core_stage11_finalizer_v1",
            "reason": format!("unsupported language for certified finalizer: {language}"),
        }));
    }

    let config_json = request
        .get("config")
        .map(Value::to_string)
        .unwrap_or_else(|| "{}".to_owned());
    let config = RustCoreConfig::from_json(&config_json);
    let old_source = request
        .get("old_source")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let new_source = request
        .get("new_source")
        .and_then(Value::as_str)
        .unwrap_or_default();
    check_byte_limit("old source", old_source, config.max_cst_bytes)?;
    check_byte_limit("new source", new_source, config.max_cst_bytes)?;

    let old_filename = request
        .get("old_filename")
        .and_then(Value::as_str)
        .unwrap_or("old.py");
    let new_filename = request
        .get("new_filename")
        .and_then(Value::as_str)
        .unwrap_or(old_filename);
    let requested_lifecycle = request
        .get("metadata")
        .and_then(|metadata| metadata.get("file_lifecycle"))
        .and_then(Value::as_str);
    let file_lifecycle =
        infer_file_lifecycle(requested_lifecycle, old_source, new_source, None);
    let old_tree: SemanticNode = serde_json::from_value(
        request
            .get("old_tree")
            .cloned()
            .ok_or_else(|| "missing old_tree".to_owned())?,
    )
    .map_err(|exc| format!("old_tree: {exc}"))?;
    let new_tree: SemanticNode = serde_json::from_value(
        request
            .get("new_tree")
            .cloned()
            .ok_or_else(|| "missing new_tree".to_owned())?,
    )
    .map_err(|exc| format!("new_tree: {exc}"))?;
    let old_index = TreeIndex::new(&old_tree);
    let new_index = TreeIndex::new(&new_tree);
    validate_unique_index_ids(&old_index)?;
    validate_unique_index_ids(&new_index)?;

    let matching = stage11_matching_from_values(
        request
            .get("matching_pairs")
            .and_then(Value::as_array)
            .ok_or_else(|| "matching_pairs must be an array".to_owned())?,
        &old_index,
        &new_index,
    )?;
    let mut changes = stage11_change_drafts_from_values(
        request
            .get("changes")
            .and_then(Value::as_array)
            .ok_or_else(|| "changes must be an array".to_owned())?,
        &old_index,
        &new_index,
    )?;

    let initial_change_count = changes.len();
    let mut finalization = PythonReviewFinalization::default();
    if initial_change_count == 0 {
        if old_source != new_source {
            let style_evidence = rust_build_style_only_evidence(&json!({
                "old_source": old_source,
                "new_source": new_source,
                "language": language,
            }))?;
            finalization
                .change_groups
                .extend(value_array_items(&style_evidence, "change_groups"));
            finalization
                .ignored_style_changes
                .extend(value_array_items(&style_evidence, "ignored_style_changes"));
        }
    } else {
        finalize_python_review_drafts(
            &mut changes,
            &old_tree,
            &new_tree,
            old_source,
            new_source,
            &mut finalization,
            language,
        );

        if changes.is_empty() {
            let zero_literal = rust_build_zero_change_literal_evidence(&json!({
                "old_tree": &old_tree,
                "new_tree": &new_tree,
                "old_source": old_source,
                "new_source": new_source,
                "language": language,
            }))?;
            finalization
                .change_groups
                .extend(value_array_items(&zero_literal, "change_groups"));
            finalization
                .ignored_style_changes
                .extend(value_array_items(&zero_literal, "ignored_style_changes"));

            if file_lifecycle == "modified"
                && finalization.change_groups.is_empty()
                && old_source != new_source
            {
                let style_evidence = rust_build_style_only_evidence(&json!({
                    "old_source": old_source,
                    "new_source": new_source,
                    "language": language,
                }))?;
                finalization
                    .change_groups
                    .extend(value_array_items(&style_evidence, "change_groups"));
                finalization
                    .ignored_style_changes
                    .extend(value_array_items(&style_evidence, "ignored_style_changes"));
            }
        }
    }

    if !changes.is_empty() && !finalization.ignored_style_changes.is_empty() {
        return Ok(json!({
            "schema_version": 1,
            "status": FALLBACK,
            "certified": false,
            "engine": "rust_core_stage11_finalizer_v1",
            "reason": "residual semantic changes remain after Rust invariance evidence",
        }));
    }
    if let Some(reason) = rust_stage11_certification_blocker(&changes, &finalization) {
        return Ok(json!({
            "schema_version": 1,
            "status": FALLBACK,
            "certified": false,
            "engine": "rust_core_stage11_finalizer_v1",
            "reason": reason,
        }));
    }

    sort_candidate_drafts(&mut changes);
    let serialized = serialize_change_drafts_fast(&changes);
    let mut change_groups = finalization.change_groups;
    change_groups.extend(final_change_groups_from_drafts(&changes));
    let is_style_only = file_lifecycle == "modified"
        && changes.is_empty()
        && (old_source == new_source || !finalization.ignored_style_changes.is_empty());
    let has_semantic_changes = !changes.is_empty() && !is_style_only;

    let mut diff = semantic_diff_payload_with_style(
        old_filename,
        new_filename,
        serialized.changes,
        has_semantic_changes,
        is_style_only,
        COMPLETE,
        json!({
            "engine": "rust_core_stage11_finalizer_v1",
            "rust_core_stage": "stage11_to_final_diff",
            "boundary": "stage11_to_final_diff",
            "input_change_count": initial_change_count,
            "final_change_count": changes.len(),
            "matching_pair_count": matching.len(),
            "candidate_certification": "rust_finalized_v1",
        }),
    );

    let mut metadata = request
        .get("metadata")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if !metadata.is_object() {
        metadata = json!({});
    }
    metadata["rust_core"] = json!({
        "status": COMPLETE,
        "backend": "intentumdiff_rust_core",
        "supported_language": "python",
        "version": VERSION,
        "engine": "rust_core_stage11_finalizer_v1",
        "stage": "stage11_to_final_diff",
        "used": true,
        "details": {
            "input_change_count": initial_change_count,
            "final_change_count": changes.len(),
            "matching_pair_count": matching.len(),
        },
    });
    metadata["engine_owner"] = json!("rust");
    metadata["semantic_contract"] = json!("rust_finalized_v1");
    if !finalization.ignored_style_changes.is_empty() {
        metadata["ignored_style_changes"] = Value::Array(finalization.ignored_style_changes);
    }

    diff["change_groups"] = Value::Array(change_groups);
    diff["metadata"] = metadata;
    attach_scope_trails_metadata(&mut diff, &changes, &old_index, &new_index);
    apply_file_lifecycle_to_diff(&mut diff, file_lifecycle);

    Ok(json!({
        "schema_version": 1,
        "status": COMPLETE,
        "certified": true,
        "engine": "rust_core_stage11_finalizer_v1",
        "diff": diff,
    }))
}

fn rust_stage11_certification_blocker(
    changes: &[ChangeDraft<'_>],
    finalization: &PythonReviewFinalization,
) -> Option<&'static str> {
    if !finalization.ignored_style_changes.is_empty() {
        return Some("Rust finalizer wave 1 does not certify style/invariance-only evidence yet");
    }
    changes.iter().find_map(|change| {
        if change.refactoring_kind.is_some() {
            return Some("Rust finalizer wave 1 does not certify refactoring evidence yet");
        }
        if change.change_type != "MODIFICATION" {
            return Some("Rust finalizer wave 1 certifies modification-only output");
        }
        None
    })
}

fn value_array_items(value: &Value, key: &str) -> Vec<Value> {
    value
        .get(key)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn stage11_matching_from_values<'a>(
    raw_pairs: &[Value],
    old_index: &'a TreeIndex<'a>,
    new_index: &'a TreeIndex<'a>,
) -> Result<Vec<MatchPair<'a>>, String> {
    let mut pairs = Vec::with_capacity(raw_pairs.len());
    for (idx, raw_pair) in raw_pairs.iter().enumerate() {
        let old_id = raw_pair
            .get("old_id")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("matching_pairs[{idx}].old_id is required"))?;
        let new_id = raw_pair
            .get("new_id")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("matching_pairs[{idx}].new_id is required"))?;
        let old_node = old_index.by_id.get(old_id).copied().ok_or_else(|| {
            format!("matching_pairs[{idx}] references unknown old node: {old_id}")
        })?;
        let new_node = new_index.by_id.get(new_id).copied().ok_or_else(|| {
            format!("matching_pairs[{idx}] references unknown new node: {new_id}")
        })?;
        pairs.push(MatchPair { old_node, new_node });
    }
    Ok(pairs)
}

fn stage11_change_drafts_from_values<'a>(
    raw_changes: &[Value],
    old_index: &'a TreeIndex<'a>,
    new_index: &'a TreeIndex<'a>,
) -> Result<Vec<ChangeDraft<'a>>, String> {
    let mut changes = Vec::with_capacity(raw_changes.len());
    for (idx, raw_change) in raw_changes.iter().enumerate() {
        let change_type = raw_change
            .get("change_type")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("changes[{idx}].change_type is required"))
            .and_then(stage11_change_type)?;
        let old_node = stage11_node_ref(raw_change, "old_node", old_index)
            .map_err(|exc| format!("changes[{idx}].old_node: {exc}"))?;
        let new_node = stage11_node_ref(raw_change, "new_node", new_index)
            .map_err(|exc| format!("changes[{idx}].new_node: {exc}"))?;
        let old_index_value = raw_change
            .get("old_index")
            .and_then(Value::as_u64)
            .map(|value| value as usize);
        let new_index_value = raw_change
            .get("new_index")
            .and_then(Value::as_u64)
            .map(|value| value as usize);
        let confidence = raw_change
            .get("confidence")
            .and_then(Value::as_f64)
            .unwrap_or(1.0)
            .clamp(0.0, 1.0);
        let description = raw_change
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| stage11_change_description(change_type, old_node, new_node));
        let refactoring_kind = raw_change
            .get("refactoring_kind")
            .and_then(Value::as_str)
            .map(stage11_refactoring_kind)
            .transpose()?;
        let text_diff = raw_change
            .get("text_diff")
            .and_then(Value::as_str)
            .map(str::to_owned);
        changes.push(ChangeDraft {
            change_type,
            old_node,
            new_node,
            old_index: old_index_value,
            new_index: new_index_value,
            confidence,
            description,
            refactoring_kind,
            text_diff,
        });
    }
    Ok(changes)
}

fn stage11_node_ref<'a>(
    raw_change: &Value,
    key: &str,
    index: &'a TreeIndex<'a>,
) -> Result<Option<&'a SemanticNode>, String> {
    let Some(raw_node) = raw_change.get(key) else {
        return Ok(None);
    };
    if raw_node.is_null() {
        return Ok(None);
    }
    let node_id = raw_node
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "id is required".to_owned())?;
    index
        .by_id
        .get(node_id)
        .copied()
        .map(Some)
        .ok_or_else(|| format!("unknown node id: {node_id}"))
}

fn stage11_change_type(raw: &str) -> Result<&'static str, String> {
    match raw {
        "ADDITION" => Ok("ADDITION"),
        "DELETION" => Ok("DELETION"),
        "MODIFICATION" => Ok("MODIFICATION"),
        "MOVE" => Ok("MOVE"),
        "REORDER" => Ok("REORDER"),
        "STYLE_ONLY" => Ok("STYLE_ONLY"),
        "REFACTORING" => Ok("REFACTORING"),
        other => Err(format!("unsupported change_type: {other}")),
    }
}

fn stage11_refactoring_kind(raw: &str) -> Result<&'static str, String> {
    match raw {
        "RENAME_SYMBOL" => Ok("RENAME_SYMBOL"),
        "RENAME_CLASS" => Ok("RENAME_CLASS"),
        "RENAME_METHOD" => Ok("RENAME_METHOD"),
        "RENAME_VARIABLE" => Ok("RENAME_VARIABLE"),
        "EXTRACT_FUNCTION" => Ok("EXTRACT_FUNCTION"),
        "INLINE_FUNCTION" => Ok("INLINE_FUNCTION"),
        "CHANGE_SIGNATURE" => Ok("CHANGE_SIGNATURE"),
        "PULL_UP" => Ok("PULL_UP"),
        "PUSH_DOWN" => Ok("PUSH_DOWN"),
        "EXTRACT_CLASS" => Ok("EXTRACT_CLASS"),
        "INLINE_CLASS" => Ok("INLINE_CLASS"),
        "INLINE_VARIABLE" => Ok("INLINE_VARIABLE"),
        "EXTRACT_VARIABLE" => Ok("EXTRACT_VARIABLE"),
        "MERGE_METHOD" => Ok("MERGE_METHOD"),
        "INTRODUCE_PARAMETER_OBJECT" => Ok("INTRODUCE_PARAMETER_OBJECT"),
        "REPLACE_CONDITIONAL_WITH_POLYMORPHISM" => Ok("REPLACE_CONDITIONAL_WITH_POLYMORPHISM"),
        "REPLACE_LOOP_WITH_PIPELINE" => Ok("REPLACE_LOOP_WITH_PIPELINE"),
        other => Err(format!("unsupported refactoring_kind: {other}")),
    }
}

fn stage11_change_description(
    change_type: &str,
    old_node: Option<&SemanticNode>,
    new_node: Option<&SemanticNode>,
) -> String {
    let mut parts = vec![capitalize(change_type)];
    if let Some(old_node) = old_node {
        parts.push(format_node_ref(old_node));
    }
    if let Some(new_node) = new_node {
        parts.push(format!("-> {}", format_node_ref(new_node)));
    }
    parts.join(" ")
}

mod invariance_groups;
use invariance_groups::*;
/// MEANINGFUL_CHANGE groups for the finalize-routed path ONLY (issue #57): python's
/// presentation surfaces every surviving semantic change as a MEANINGFUL_CHANGE group
/// carrying the change's labels; routed languages skip python presentation entirely, so
/// downstream classifiers saw group-less output and read every edit as 'other' (csharp
/// pilot). Deliberately NOT part of final_change_groups_from_drafts — the certified
/// batch path feeds python presentation, which builds its own groups.
fn final_meaningful_groups_from_drafts(changes: &[ChangeDraft<'_>]) -> Vec<Value> {
    changes
        .iter()
        .enumerate()
        .filter(|(_, change)| {
            matches!(change.change_type, "ADDITION" | "DELETION" | "MODIFICATION")
        })
        .map(|(idx, change)| {
            final_change_group_from_draft(
                idx,
                change,
                "MEANINGFUL_CHANGE",
                "presentation.final_meaningful_group",
            )
        })
        .collect()
}

/// python refinement._group_child_modifications_under_entities (issue #57 abap): a MODIFICATION
/// whose nearest same-identity entity ancestor changed content gets an entity-ANCHORED
/// MEANINGFUL_CHANGE group (rule refinement.entity_child_content_changed) so the review reads
/// "GREET changed" with the statement edits beneath it — not a free-floating leaf edit.
fn entity_child_content_groups(
    changes: &[ChangeDraft<'_>],
    old_tree: &SemanticNode,
    new_tree: &SemanticNode,
) -> Vec<Value> {
    const ROOT_CONTAINERS: &[&str] = &["module", "document", "program", "source_file"];
    fn nearest_entity<'a>(
        id: &str,
        by_id: &HashMap<&str, &'a SemanticNode>,
    ) -> Option<&'a SemanticNode> {
        let mut current = id.to_string();
        while let Some((parent_id, _)) = current.rsplit_once('.') {
            if let Some(node) = by_id.get(parent_id).copied() {
                if is_entity_container_type(node.node_type.to_lowercase().as_str()) {
                    return Some(node);
                }
            }
            current = parent_id.to_string();
        }
        None
    }
    let old_by_id = semantic_node_refs_by_id_with_root(old_tree);
    let new_by_id = semantic_node_refs_by_id_with_root(new_tree);
    let all_labels = |node: Option<&SemanticNode>| -> Vec<String> {
        let Some(node) = node else { return Vec::new() };
        std::iter::once(node)
            .chain(node.descendants())
            .filter(|n| !n.label.is_empty())
            .map(|n| n.label.clone())
            .collect()
    };
    let all_ids = |node: &SemanticNode| -> Vec<String> {
        std::iter::once(node)
            .chain(node.descendants())
            .map(|n| n.id.clone())
            .collect()
    };

    let mut groups = Vec::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    for (idx, change) in changes.iter().enumerate() {
        if change.change_type != "MODIFICATION" {
            continue;
        }
        let (Some(old_node), Some(new_node)) = (change.old_node, change.new_node) else {
            continue;
        };
        let (Some(old_entity), Some(new_entity)) = (
            nearest_entity(old_node.id.as_str(), &old_by_id),
            nearest_entity(new_node.id.as_str(), &new_by_id),
        ) else {
            continue;
        };
        if ROOT_CONTAINERS.contains(&old_entity.node_type.to_lowercase().as_str())
            || ROOT_CONTAINERS.contains(&new_entity.node_type.to_lowercase().as_str())
        {
            continue;
        }
        if old_entity.id == old_node.id || new_entity.id == new_node.id {
            continue;
        }
        if old_entity.node_type != new_entity.node_type
            || old_entity.label != new_entity.label
            || old_entity.structural_hash == new_entity.structural_hash
        {
            continue;
        }
        let key = (old_entity.id.clone(), new_entity.id.clone());
        if !seen.insert(key) {
            continue;
        }
        let mut old_labels = vec![old_entity.label.clone()];
        old_labels.extend(all_labels(change.old_node));
        let mut new_labels = vec![new_entity.label.clone()];
        new_labels.extend(all_labels(change.new_node));
        groups.push(json!({
            "kind": "MEANINGFUL_CHANGE",
            "raw_change_indices": [idx],
            "old_labels": old_labels,
            "new_labels": new_labels,
            "old_node_ids": all_ids(old_entity),
            "new_node_ids": all_ids(new_entity),
            "confidence": 0.86,
            "rule_id": "refinement.entity_child_content_changed",
            "metadata": {"entity_node_id": old_entity.id},
        }));
    }
    groups
}

/// python differ._surface_changed_in_place_entities (issue #57 graphql): a MATCHED named entity
/// (``type User``, ``query UserCard``, ``fragment UserFields``) whose body changed in place never
/// appears in the change list itself — only its changed descendants do. Emit a
/// MEANINGFUL_CHANGE group carrying the ENTITY's label so review UIs surface the container name.
fn surface_changed_in_place_entity_groups(
    changes: &[ChangeDraft<'_>],
    matching: &[MatchPair<'_>],
) -> Vec<Value> {
    fn is_named_entity_node(node: &SemanticNode) -> bool {
        if node.label.is_empty() || node.label == node.node_type {
            return false;
        }
        is_named_entity_type(node.node_type.as_str())
            || node.node_type.ends_with("_definition")
            || node.node_type.ends_with("_declaration")
    }
    let surfaced_old: HashSet<&str> = changes
        .iter()
        .filter_map(|c| c.old_node.map(|n| n.id.as_str()))
        .collect();
    let surfaced_new: HashSet<&str> = changes
        .iter()
        .filter_map(|c| c.new_node.map(|n| n.id.as_str()))
        .collect();
    let mut groups = Vec::new();
    for pair in matching {
        if !is_named_entity_node(pair.old_node) || !is_named_entity_node(pair.new_node) {
            continue;
        }
        if surfaced_old.contains(pair.old_node.id.as_str())
            || surfaced_new.contains(pair.new_node.id.as_str())
        {
            continue;
        }
        // A descendant appearing in the change list is the reliable "changed in place" signal.
        let changed = pair
            .old_node
            .descendants()
            .iter()
            .any(|d| surfaced_old.contains(d.id.as_str()))
            || pair
                .new_node
                .descendants()
                .iter()
                .any(|d| surfaced_new.contains(d.id.as_str()));
        if !changed {
            continue;
        }
        groups.push(json!({
            "kind": "MEANINGFUL_CHANGE",
            "raw_change_indices": [],
            "old_labels": [pair.old_node.label],
            "new_labels": [pair.new_node.label],
            "old_node_ids": [pair.old_node.id],
            "new_node_ids": [pair.new_node.id],
            "confidence": 0.9,
            "rule_id": "presentation.surface_changed_in_place_entity",
            "metadata": {
                "entity_type": pair.old_node.node_type,
                "old_label": pair.old_node.label,
                "new_label": pair.new_node.label,
            },
        }));
    }
    groups
}

fn final_change_groups_from_drafts(changes: &[ChangeDraft<'_>]) -> Vec<Value> {
    let mut groups = Vec::new();
    for (idx, change) in changes.iter().enumerate() {
        match change.change_type {
            "MOVE" => groups.push(final_change_group_from_draft(
                idx,
                change,
                "MOVED_CODE",
                "presentation.final_move_group",
            )),
            "REFACTORING" => groups.push(final_change_group_from_draft(
                idx,
                change,
                "REFACTORING",
                "presentation.final_refactoring_group",
            )),
            _ => {}
        }
    }
    groups
}

fn final_change_group_from_draft(
    idx: usize,
    change: &ChangeDraft<'_>,
    kind: &str,
    rule_id: &str,
) -> Value {
    let mut group = json!({
        "kind": kind,
        "raw_change_indices": [idx],
        "old_labels": node_labels(change.old_node),
        "new_labels": node_labels(change.new_node),
        "old_node_ids": node_ids(change.old_node),
        "new_node_ids": node_ids(change.new_node),
        "confidence": change.confidence,
        "rule_id": rule_id,
        "metadata": {"index_space": "final_changes"},
    });
    if let Some(refactoring_kind) = change.refactoring_kind {
        group["refactoring_kind"] = json!(refactoring_kind);
    }
    group
}

fn attach_scope_trails_metadata(
    diff: &mut Value,
    changes: &[ChangeDraft<'_>],
    old_index: &TreeIndex<'_>,
    new_index: &TreeIndex<'_>,
) {
    let old_entries = scope_entries_for_changes(changes, true, old_index);
    let new_entries = scope_entries_for_changes(changes, false, new_index);
    if old_entries.is_empty() && new_entries.is_empty() {
        return;
    }
    diff["metadata"]["scope_trails"] = json!({
        "old": old_entries,
        "new": new_entries,
    });
}

fn rust_scope_trails_value(request: &Value) -> Result<Value, String> {
    let old_tree: SemanticNode = serde_json::from_value(
        request
            .get("old_tree")
            .cloned()
            .ok_or_else(|| "missing old_tree".to_owned())?,
    )
    .map_err(|exc| format!("old_tree: {exc}"))?;
    let new_tree: SemanticNode = serde_json::from_value(
        request
            .get("new_tree")
            .cloned()
            .ok_or_else(|| "missing new_tree".to_owned())?,
    )
    .map_err(|exc| format!("new_tree: {exc}"))?;
    let changes = request
        .get("changes")
        .and_then(Value::as_array)
        .ok_or_else(|| "changes must be an array".to_owned())?;
    let old_index = TreeIndex::new(&old_tree);
    let new_index = TreeIndex::new(&new_tree);
    let old_entries = scope_entries_for_change_values(changes, true, &old_index);
    let new_entries = scope_entries_for_change_values(changes, false, &new_index);
    Ok(json!({
        "scope_trails": {
            "old": old_entries,
            "new": new_entries,
        },
    }))
}

fn scope_entries_for_change_values(
    changes: &[Value],
    old_side: bool,
    index: &TreeIndex<'_>,
) -> Vec<Value> {
    let mut entries = Vec::new();
    let node_key = if old_side { "old_node" } else { "new_node" };
    for (change_index, change) in changes.iter().enumerate() {
        let Some(node_id) = change
            .get(node_key)
            .and_then(|node| node.get("id"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        let Some(node) = index.by_id.get(node_id) else {
            continue;
        };
        let trail = scope_trail_for_node(node, index);
        if trail.is_empty() {
            continue;
        }
        entries.push(json!({
            "change_index": change_index,
            "node_id": node.id.as_str(),
            "line": node.position.start_line,
            "trail": trail,
        }));
    }
    entries
}

fn scope_entries_for_changes(
    changes: &[ChangeDraft<'_>],
    old_side: bool,
    index: &TreeIndex<'_>,
) -> Vec<Value> {
    let mut entries = Vec::new();
    for (change_index, change) in changes.iter().enumerate() {
        let node = if old_side {
            change.old_node
        } else {
            change.new_node
        };
        let Some(node) = node else {
            continue;
        };
        let trail = scope_trail_for_node(node, index);
        if trail.is_empty() {
            continue;
        }
        entries.push(json!({
            "change_index": change_index,
            "node_id": node.id.as_str(),
            "line": node.position.start_line,
            "trail": trail,
        }));
    }
    entries
}

fn scope_trail_for_node(node: &SemanticNode, index: &TreeIndex<'_>) -> Vec<String> {
    let mut current = Some(node.id.as_str());
    let mut trail = Vec::new();
    while let Some(node_id) = current {
        if let Some(current_node) = index.by_id.get(node_id) {
            if let Some(label) = scope_label_for_node(current_node) {
                trail.push(label);
            }
        }
        current = index.parent.get(node_id).copied();
    }
    trail.reverse();
    trail.dedup();
    if trail.len() > 5 {
        trail[trail.len() - 5..].to_vec()
    } else {
        trail
    }
}

fn scope_label_for_node(node: &SemanticNode) -> Option<String> {
    match node.node_type.as_str() {
        "module" if node.label != "module" && !node.label.is_empty() => {
            Some(format!("module {}", node.label))
        }
        "class_definition" | "class_declaration" | "struct_declaration"
            if !node.label.is_empty() =>
        {
            Some(format!("class {}", node.label))
        }
        "function_definition"
        | "async_function_def"
        | "function_declaration"
        | "async_function_declaration"
        | "method_declaration"
        | "method_definition"
        | "async_method_definition"
        | "function_item"
            if !node.label.is_empty() =>
        {
            Some(format!("function {}", node.label))
        }
        "block" | "compound_statement"
            if !node.label.is_empty()
                && node.label != "block"
                && node.label != "compound_statement" =>
        {
            Some(format!("block {}", node.label))
        }
        _ => None,
    }
}

fn node_labels(node: Option<&SemanticNode>) -> Vec<String> {
    let Some(node) = node else {
        return Vec::new();
    };
    let mut labels = Vec::new();
    if !node.label.is_empty() {
        labels.push(node.label.clone());
        if node.node_type == "string" {
            if let Some(decoded) = decode_simple_python_string(&node.label) {
                labels.push(decoded);
            }
        }
    }
    for descendant in node.descendants() {
        if !descendant.label.is_empty() {
            labels.push(descendant.label.clone());
            if descendant.node_type == "string" {
                if let Some(decoded) = decode_simple_python_string(&descendant.label) {
                    labels.push(decoded);
                }
            }
        }
    }
    labels
}

fn node_ids(node: Option<&SemanticNode>) -> Vec<String> {
    let Some(node) = node else {
        return Vec::new();
    };
    let mut ids = Vec::new();
    ids.push(node.id.clone());
    for descendant in node.descendants() {
        ids.push(descendant.id.clone());
    }
    ids
}

mod draft_suppressors;
use draft_suppressors::*;


fn annotate_moved_context_descriptions_drafts(changes: &mut [ChangeDraft<'_>]) {
    let mut moved_contexts: Vec<(String, String)> = Vec::new();
    let mut old_descendant_context: HashMap<String, usize> = HashMap::new();
    let mut new_descendant_context: HashMap<String, usize> = HashMap::new();
    for (context_index, (entity_type, entity_label, old_descendants, new_descendants)) in changes
        .iter()
        .filter(|change| change.change_type == "MOVE")
        .filter_map(|change| {
            let old_node = change.old_node?;
            let new_node = change.new_node?;
            if !is_named_entity_type(old_node.node_type.as_str()) {
                return None;
            }
            let mut old_descendants = HashSet::new();
            let mut new_descendants = HashSet::new();
            collect_descendant_ids_node(old_node, &mut old_descendants);
            collect_descendant_ids_node(new_node, &mut new_descendants);
            Some((
                old_node.node_type.clone(),
                old_node.label.clone(),
                old_descendants,
                new_descendants,
            ))
        })
        .enumerate()
    {
        moved_contexts.push((entity_type, entity_label));
        for old_id in old_descendants {
            old_descendant_context
                .entry(old_id)
                .or_insert(context_index);
        }
        for new_id in new_descendants {
            new_descendant_context
                .entry(new_id)
                .or_insert(context_index);
        }
    }
    if moved_contexts.is_empty() {
        return;
    }
    for change in changes {
        if change.change_type != "MODIFICATION" {
            continue;
        }
        let Some(old_node) = change.old_node else {
            continue;
        };
        let Some(new_node) = change.new_node else {
            continue;
        };
        let Some(old_context_index) = old_descendant_context.get(&old_node.id) else {
            continue;
        };
        if new_descendant_context.get(&new_node.id) != Some(old_context_index) {
            continue;
        }
        let Some((entity_type, entity_label)) = moved_contexts.get(*old_context_index) else {
            continue;
        };
        change.description = format!(
            "Update inside moved {entity_type} '{entity_label}': {} -> {}",
            format_node_ref(old_node),
            format_node_ref(new_node),
        );
    }
}

fn annotate_leaf_text_diffs_drafts(changes: &mut [ChangeDraft<'_>]) {
    for change in changes {
        if change.change_type != "MODIFICATION" {
            continue;
        }
        let Some(old_node) = change.old_node else {
            continue;
        };
        let Some(new_node) = change.new_node else {
            continue;
        };
        if old_node.is_leaf() && new_node.is_leaf() && old_node.label != new_node.label {
            change.text_diff = Some(format!("{} -> {}", old_node.label, new_node.label));
        }
    }
}

fn sort_candidate_drafts(changes: &mut [ChangeDraft<'_>]) {
    changes.sort_by_key(candidate_draft_sort_key);
}

fn candidate_draft_sort_key(change: &ChangeDraft<'_>) -> (u32, u32, u8, String) {
    let node = change.old_node.or(change.new_node);
    let start_line = node
        .map(|node| node.position.start_line)
        .unwrap_or(u32::MAX);
    let start_col = node.map(|node| node.position.start_col).unwrap_or(u32::MAX);
    let rank = match change.change_type {
        "DELETION" => 0,
        "MODIFICATION" => 1,
        "MOVE" | "REORDER" => 2,
        "ADDITION" => 3,
        _ => 4,
    };
    let id = node.map(|node| node.id.clone()).unwrap_or_default();
    (start_line, start_col, rank, id)
}

fn change_pair_exists_drafts(
    changes: &[ChangeDraft<'_>],
    change_type: &str,
    old_id: Option<&str>,
    new_id: Option<&str>,
) -> bool {
    changes.iter().any(|change| {
        change.change_type == change_type
            && change.old_node.map(|node| node.id.as_str()) == old_id
            && change.new_node.map(|node| node.id.as_str()) == new_id
    })
}

fn change_has_node_id(
    changes: &[ChangeDraft<'_>],
    change_type: &str,
    old_id: Option<&str>,
    new_id: Option<&str>,
) -> bool {
    changes.iter().any(|change| {
        change.change_type == change_type
            && match old_id {
                Some(id) => change.old_node.is_some_and(|node| node.id.as_str() == id),
                None => true,
            }
            && match new_id {
                Some(id) => change.new_node.is_some_and(|node| node.id.as_str() == id),
                None => true,
            }
    })
}

fn first_parameter_identifier_node(node: &SemanticNode) -> Option<&SemanticNode> {
    node.children
        .iter()
        .find(|child| child.node_type == "parameters")
        .and_then(|parameters| first_descendant_node(parameters, "identifier"))
}

fn best_import_identifier_node<'a>(
    node: &'a SemanticNode,
    deleted_function_label: &str,
) -> Option<&'a SemanticNode> {
    let identifiers = descendant_nodes(node, "identifier");
    identifiers
        .iter()
        .copied()
        .filter(|candidate| {
            candidate.label.contains(deleted_function_label)
                || deleted_function_label.contains(candidate.label.as_str())
        })
        .max_by_key(|candidate| candidate.label.len())
        .or_else(|| {
            identifiers
                .into_iter()
                .max_by_key(|candidate| candidate.label.len())
        })
}

fn first_descendant_node<'a>(node: &'a SemanticNode, node_type: &str) -> Option<&'a SemanticNode> {
    if node.node_type == node_type {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|child| first_descendant_node(child, node_type))
}

fn descendant_nodes<'a>(node: &'a SemanticNode, node_type: &str) -> Vec<&'a SemanticNode> {
    let mut result = Vec::new();
    collect_descendant_nodes(node, node_type, &mut result);
    result
}

fn collect_descendant_nodes<'a>(
    node: &'a SemanticNode,
    node_type: &str,
    result: &mut Vec<&'a SemanticNode>,
) {
    if node.node_type == node_type {
        result.push(node);
    }
    for child in &node.children {
        collect_descendant_nodes(child, node_type, result);
    }
}

fn all_descendant_node_refs_by_id(node: &SemanticNode) -> HashMap<&str, &SemanticNode> {
    let mut result = HashMap::new();
    collect_all_descendant_node_refs_by_id(node, &mut result);
    result
}

fn semantic_node_refs_by_id_with_root(node: &SemanticNode) -> HashMap<&str, &SemanticNode> {
    let mut result = HashMap::new();
    result.insert(node.id.as_str(), node);
    collect_all_descendant_node_refs_by_id(node, &mut result);
    result
}

fn collect_all_descendant_node_refs_by_id<'a>(
    node: &'a SemanticNode,
    result: &mut HashMap<&'a str, &'a SemanticNode>,
) {
    for child in &node.children {
        result.insert(child.id.as_str(), child);
        collect_all_descendant_node_refs_by_id(child, result);
    }
}

fn is_named_entity_type(node_type: &str) -> bool {
    // Broad entity recognition across all supported parsers. Used by the
    // matcher's label-match phase to identify containers whose labels
    // carry semantic weight (function names, type names, section titles,
    // etc.). See ``docs/NOISE_SUPPRESSION_RETUNE.md`` Step A2.
    matches!(
        node_type,
        // Delphi/Pascal routines (camelCase in the parser output; the Python profile lists
        // them lowercase — the mismatch left routines invisible to entity matching, so
        // identical-shaped statements cross-matched between routines, issue #19)
        "defProc"
        | "declProc"
        // Puppet resources are title-identified scopes (issue #24: without this, a resource
        // attribute's old value cross-matched an identical class-parameter default because
        // both resolved to the same enclosing class scope)
        | "resource_declaration"
        | "node_definition"
        // Tree-sitter common
        | "function_definition"
        | "async_function_def"
        | "class_definition"
        | "method_declaration"
        | "function_declaration"
        | "async_function_declaration"
        // PowerShell functions (issue #57/#66: a pure two-function swap yielded ZERO changes
        // routed — the reorder promotion's named-entity guard didn't know the type, so both
        // REORDERs were suppressed as low-signal instead of promoting the order-inverted one
        // to a MOVE)
        | "function_statement"
        | "constructor_declaration"
        | "destructor_declaration"
        | "property_declaration"
        | "field_declaration"
        | "variable_declaration"
        | "lexical_declaration"
        | "struct_definition"
        | "interface_declaration"
        | "enum_declaration"
        | "enum_member_declaration"
        | "operator_declaration"
        | "event_declaration"
        | "delegate_declaration"
        | "record_declaration"
        | "record_struct_declaration"
        // GraphQL
        | "type_definition"
        | "operation_definition"
        | "fragment_definition"
        | "field_definition"
        | "directive_definition"
        | "schema_definition"
        | "enum_definition"
        | "input_object_definition"
        // PO/gettext
        | "message"
        | "obsolete_message"
        // AsciiDoc
        | "section_level_1"
        | "section_level_2"
        | "section_level_3"
        | "section_level_4"
        | "section_level_5"
        | "section_level_6"
        // LaTeX
        | "document_class"
        | "package"
        | "section"
        | "environment"
        // OCaml / ReasonML ("type_definition" is shared with — and already matched by —
        // the GraphQL arm above)
        | "module_binding"
        | "module"
        | "value_definition"
        | "recursive_value"
        | "component"
        | "signature_value"
        | "class_type"
        | "exception"
    )
}

fn has_suppressed_ancestor_id(id: &str, roots: &HashSet<String>) -> bool {
    let mut current = id;
    while let Some((parent, _)) = current.rsplit_once('.') {
        if roots.contains(parent) {
            return true;
        }
        current = parent;
    }
    false
}

/// A grammar-agnostic parameter-list container node type. Mirrors python
/// `analysis/anchors.py::_PARAM_LIST_TYPES` so per-grammar spellings
/// (`formal_parameter_list` for dart, `parameter_clause` for swift, `declargs`, …)
/// are all recognised as the parameter scope for rename promotion.
fn is_parameter_list_type(node_type: &str) -> bool {
    matches!(
        node_type,
        "declargs"
            | "decl_args"
            | "formal_parameter_list"
            | "parameters"
            | "parameter_list"
            | "formal_parameters"
            | "params"
            | "param_block"
            | "function_parameters"
            | "method_parameters"
            | "parameter_clause"
    )
}

/// True when any ancestor of `id` is a parameter-list container (any grammar spelling).
fn ancestor_is_in_parameter_list(id: &str, nodes_by_id: &HashMap<&str, &SemanticNode>) -> bool {
    let mut current = id;
    while let Some((parent, _)) = current.rsplit_once('.') {
        if nodes_by_id
            .get(parent)
            .is_some_and(|node| is_parameter_list_type(node.node_type.as_str()))
        {
            return true;
        }
        current = parent;
    }
    false
}

fn is_move_noise_type(node_type: &str) -> bool {
    matches!(
        node_type,
        "module" | "block" | "expression_statement" | "return_statement" | "identifier" | "call"
    )
}

fn is_preferred_modification_parent_type(node_type: &str) -> bool {
    matches!(node_type, "if_statement" | "binary_operator")
}

fn is_moved_child_update_type(node_type: &str) -> bool {
    matches!(node_type, "identifier")
}

/// Port of the python refinement vocabulary `_ENTITY_CONTAINER_TYPES`
/// (analysis/refinement.py) for passes that mirror python refinement behavior — the
/// csharp finalize pilot found `class_declaration` missing from `is_named_entity_type`,
/// which silenced the moved-entity edit recovery entirely. Deliberately a SEPARATE
/// predicate: `is_named_entity_type` also drives the matcher's label-match seeding and
/// has its own tuning history; merging the two vocabularies is issue #49 (DRY audit).
fn is_entity_container_type(node_type: &str) -> bool {
    is_named_entity_type(node_type)
        || matches!(
            node_type,
            // Doc-component leaf entities (python refinement._LEAF_ENTITY_TYPES): an mdx
            // <Step/> prop edit anchors to the COMPONENT (Step Verify), not the raw
            // attribute string (issue #57 payoff, intent-truth mdx scenarios).
            "jsx_component"
                | "section"
                | "markdown_section"
                | "class_statement"
                | "constructor_signature"
                | "declproc"
                | "defproc"
                | "procedure_declaration"
                | "operation"
                | "rule_set"
                | "procedure"
                | "enum_statement"
                | "extension_declaration"
                | "extension_type_declaration"
                | "function_body_declaration"
                | "function_heading"
                | "function_signature"
                | "function_statement"
                | "getter_signature"
                | "form"
                | "function_module"
                | "method_definition"
                | "method"
                | "method_signature"
                | "method_statement"
                | "mixin_declaration"
                | "arrow_function"
                | "async_arrow_function"
                | "async_method_definition"
                | "class_declaration"
                | "class_impl"
                | "interface"
                | "interface_definition"
                | "object_type"
                | "procedure_definition"
                | "procedure_heading"
                | "record_definition"
                | "setter_signature"
                | "struct_declaration"
                | "subroutine_declaration_statement"
        )
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str().to_lowercase()),
        None => String::new(),
    }
}

fn lifecycle_from_status(status: Option<&str>) -> Option<&'static str> {
    let status = status?.trim();
    if status.is_empty() {
        return None;
    }
    if ["added", "add", "a", "new", "untracked"]
        .iter()
        .any(|item| status.eq_ignore_ascii_case(item))
    {
        return Some("added");
    }
    if ["deleted", "delete", "d", "removed", "remove"]
        .iter()
        .any(|item| status.eq_ignore_ascii_case(item))
    {
        return Some("deleted");
    }
    None
}

fn infer_file_lifecycle(
    explicit: Option<&str>,
    old_source: &str,
    new_source: &str,
    staging_status: Option<&str>,
) -> &'static str {
    if let Some(lifecycle) = lifecycle_from_status(explicit) {
        return lifecycle;
    }
    if let Some(lifecycle) = lifecycle_from_status(staging_status) {
        return lifecycle;
    }
    if old_source.is_empty() && !new_source.is_empty() {
        return "added";
    }
    if !old_source.is_empty() && new_source.is_empty() {
        return "deleted";
    }
    "modified"
}

fn infer_file_lifecycle_from_file(file: &Value) -> &'static str {
    let old_source = value_str(file, "old_source", "oldSource").unwrap_or("");
    let new_source = value_str(file, "new_source", "newSource").unwrap_or("");
    let explicit = value_str(file, "file_lifecycle", "fileLifecycle");
    let staging_status = value_str(file, "staging_status", "stagingStatus");
    infer_file_lifecycle(explicit, old_source, new_source, staging_status)
}

fn apply_file_lifecycle_to_diff(diff: &mut Value, lifecycle: &str) {
    if !diff.get("metadata").is_some_and(Value::is_object) {
        diff["metadata"] = json!({});
    }
    diff["metadata"]["file_lifecycle"] = json!(lifecycle);
    if lifecycle == "modified" {
        return;
    }
    diff["is_style_only"] = json!(false);
    let has_changes = diff
        .get("changes")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty());
    diff["has_semantic_changes"] = json!(has_changes);
    if let Some(groups) = diff.get_mut("change_groups").and_then(Value::as_array_mut) {
        groups.retain(|group| {
            group
                .get("kind")
                .and_then(Value::as_str)
                != Some("IGNORED_STYLE")
        });
    }
}

fn semantic_diff_payload_with_style(
    old_filename: &str,
    new_filename: &str,
    changes: Vec<Value>,
    has_semantic_changes: bool,
    is_style_only: bool,
    status: &str,
    extra_metadata: Value,
) -> Value {
    let mut payload = semantic_diff_payload(
        old_filename,
        new_filename,
        changes,
        has_semantic_changes,
        status,
        extra_metadata,
    );
    payload["is_style_only"] = json!(is_style_only);
    payload
}

/// Hash a semantic tree with whitespace-collapsed labels (issue #51): the style-only
/// discriminator for the empty-changes case. Labels like `a + b` vs `a  +  b` are the SAME
/// program; labels differing in content are not.
fn whitespace_normalized_tree_hash(node: &SemanticNode) -> String {
    let mut hasher = Sha256::new();
    let normalized: String = node.label.split_whitespace().collect::<Vec<_>>().join(" ");
    hasher.update(node.node_type.as_bytes());
    hasher.update(b":");
    hasher.update(normalized.as_bytes());
    for child in &node.children {
        hasher.update(b"|");
        hasher.update(whitespace_normalized_tree_hash(child).as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn semantic_diff_payload(
    old_filename: &str,
    new_filename: &str,
    changes: Vec<Value>,
    has_semantic_changes: bool,
    status: &str,
    extra_metadata: Value,
) -> Value {
    let engine_telemetry = rust_engine_telemetry_from_details(&extra_metadata);
    let metadata = json!({
        "rust_core": {
            "status": status,
            "backend": "intentumdiff_rust_core",
            "supported_language": "python",
            "version": VERSION,
            "details": extra_metadata,
        },
        "engine_telemetry": engine_telemetry,
    });
    json!({
        "old_filename": old_filename,
        "new_filename": new_filename,
        "language": "python",
        "changes": changes,
        "change_groups": [],
        "has_semantic_changes": has_semantic_changes,
        "is_style_only": false,
        "parse_errors": [],
        "llm_summary": "",
        "gitignore_excluded": false,
        "is_fallback": false,
        "metadata": metadata,
    })
}

fn rust_engine_telemetry_from_details(details: &Value) -> Value {
    let engine = details
        .get("engine")
        .and_then(Value::as_str)
        .unwrap_or("intentumdiff_rust_core");
    let parser_backend = details
        .get("python_parser_backend")
        .and_then(Value::as_str)
        .unwrap_or("native");
    let wasm_boundary = details
        .get("wasm_boundary")
        .and_then(Value::as_str)
        .unwrap_or("rust_core");
    let provenance = if parser_backend == PYTHON_PARSER_BACKEND_NATIVE {
        "first_party_native"
    } else if wasm_boundary == "rust_wasmtime" {
        "first_party_wasm"
    } else {
        "rust_core"
    };
    let fuel_budget = details
        .get("adaptive_fuel")
        .and_then(Value::as_u64)
        .map(Value::from)
        .unwrap_or(Value::Null);
    let elapsed_ms = details
        .get("phase_timings")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("duration_ms").and_then(Value::as_f64))
                .sum::<f64>()
        })
        .unwrap_or(0.0);

    json!({
        "schema_version": 1,
        "calls": [{
            "plugin": "intentumdiff_rust_core",
            "function": "finalize",
            "engine_owner": "rust",
            "engine": engine,
            "provenance": provenance,
            "parser_backend": parser_backend,
            "wasm_boundary": wasm_boundary,
            "call_count": 1,
            "elapsed_ms": elapsed_ms,
            "fuel_budget": fuel_budget,
            "fuel_consumed": Value::Null,
            "max_fuel_used_percent": Value::Null,
            "statuses": {"ok": 1},
            "trusted": true,
        }],
    })
}

fn candidate_signature_for_diff(diff: &Value) -> Value {
    let mut signature = Vec::new();
    let old_filename = diff
        .get("old_filename")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let new_filename = diff
        .get("new_filename")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if let Some(changes) = diff.get("changes").and_then(Value::as_array) {
        for change in changes {
            let change_type = change
                .get("change_type")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            signature.push(json!({
                "old_filename": old_filename,
                "new_filename": new_filename,
                "change_type": change_type,
                "old": node_signature_value(change.get("old_node")),
                "new": node_signature_value(change.get("new_node")),
                "refactoring_kind": change.get("refactoring_kind").and_then(Value::as_str).unwrap_or(""),
                "description": change.get("description").and_then(Value::as_str).unwrap_or(""),
            }));
        }
    }
    Value::Array(signature)
}

fn node_signature_value(node: Option<&Value>) -> Value {
    let Some(node) = node else {
        return Value::Null;
    };
    let position = node.get("position").unwrap_or(&Value::Null);
    json!({
        "id": node.get("id").and_then(Value::as_str).unwrap_or(""),
        "node_type": node.get("node_type").and_then(Value::as_str).unwrap_or(""),
        "label": node.get("label").and_then(Value::as_str).unwrap_or(""),
        "position": {
            "start_line": position.get("start_line").cloned().unwrap_or(Value::Null),
            "start_col": position.get("start_col").cloned().unwrap_or(Value::Null),
            "end_line": position.get("end_line").cloned().unwrap_or(Value::Null),
            "end_col": position.get("end_col").cloned().unwrap_or(Value::Null),
        }
    })
}

#[cfg(test)]
#[path = "tests_inline.rs"]
mod tests;

// Pyo3-free `#[cfg(test)]` stand-ins for the retired crate-root `#[pyfunction]` wrappers (#B.6),
// re-exported at crate root so the test suites' `use crate::*` resolves the former wrapper names.
#[cfg(test)]
#[path = "test_wrappers.rs"]
mod test_wrappers;
#[cfg(test)]
pub(crate) use test_wrappers::*;
