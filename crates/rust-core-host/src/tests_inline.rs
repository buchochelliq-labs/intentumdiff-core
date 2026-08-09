//! The engine's inline test suite, extracted from lib.rs verbatim (issue #29
//! monolith split, phase A). Compiled only under cfg(test) via the #[path] mod
//! declaration in lib.rs; `super::*` still resolves to the crate root.

    use super::*;

    fn container_guard_node(id: &str, node_type: &str, label: &str, children: Value) -> SemanticNode {
        serde_json::from_value(json!({
            "id": id,
            "node_type": node_type,
            "label": label,
            "position": {"start_line": 0, "start_col": 0, "end_line": 0, "end_col": 0},
            "structural_hash": format!("h-{id}"),
            "children": children,
        }))
        .unwrap()
    }

    fn container_guard_draft<'a>(
        change_type: &'static str,
        old_node: Option<&'a SemanticNode>,
        new_node: Option<&'a SemanticNode>,
    ) -> ChangeDraft<'a> {
        ChangeDraft {
            change_type,
            old_node,
            new_node,
            old_index: None,
            new_index: None,
            confidence: 0.9,
            description: String::new(),
            refactoring_kind: None,
            text_diff: None,
        }
    }








    fn temp_test_dir(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{name}-{}-{nanos}", std::process::id()))
    }

    #[cfg(unix)]
    fn create_file_symlink(target: PathBuf, link: &Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[cfg(windows)]
    fn create_file_symlink(target: PathBuf, link: &Path) -> std::io::Result<()> {
        std::os::windows::fs::symlink_file(target, link)
    }

    fn module_with_function(name: &str) -> String {
        json!({
            "type": "module",
            "start_line": 0,
            "start_col": 0,
            "end_line": 1,
            "end_col": 0,
            "children": [
                {
                    "type": "function_definition",
                    "start_line": 0,
                    "start_col": 0,
                    "end_line": 1,
                    "end_col": 0,
                    "children": [
                        {
                            "type": "identifier",
                            "text": name,
                            "start_line": 0,
                            "start_col": 4,
                            "end_line": 0,
                            "end_col": 4 + name.len()
                        }
                    ]
                }
            ]
        })
        .to_string()
    }

    fn node(id: &str, node_type: &str, label: &str, children: Vec<SemanticNode>) -> SemanticNode {
        SemanticNode {
            id: id.to_string(),
            node_type: node_type.to_string(),
            label: label.to_string(),
            position: NodePosition {
                start_line: 0,
                start_col: 0,
                end_line: 0,
                end_col: label.len() as u32,
            },
            structural_hash: format!("hash-{node_type}-{label}"),
            children,
            parent_type: None,
            type_info: None,
            facts: None,
        }
    }

    /// The staged parser-component dir the Tier-C tests load from. Default = the monorepo's
    /// staging (`src/intentumdiff/wasm`); `INTENTUMDIFF_TEST_WASM_DIR` overrides it so an
    /// extracted engine repo (#82 split) can point at any dir of built components.
    fn staged_wasm_dir() -> std::path::PathBuf {
        std::env::var_os("INTENTUMDIFF_TEST_WASM_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src/intentumdiff/wasm")
            })
    }

    fn python_wasm_path() -> String {
        bundled_wasm_path("python_parser.wasm")
    }

    fn bundled_wasm_path(name: &str) -> String {
        staged_wasm_dir()
            .join(name)
            .canonicalize()
            .unwrap_or_else(|exc| {
                panic!(
                    "staged parser component {name:?} not found in {:?} ({exc}); these Tier-C \
                     tests need built parser wasm — set INTENTUMDIFF_TEST_WASM_DIR to a dir of \
                     built components, or run with --no-default-features to skip the \
                     tier-c-wasm gated tests",
                    staged_wasm_dir()
                )
            })
            .to_string_lossy()
            .to_string()
    }

    /// #100 A2.5 step (d) unblocker: `run_python_wasm_process_pair` is language-agnostic — the
    /// `run_python_wasm_*` name is historical. Handed the resolved language + a full-parse Wasm
    /// grammar, the wasmtime host parses a NON-Python source pair into real semantic trees. This
    /// is the concrete proof the host is not Python-bound (previously the language hint was
    /// hardcoded "python"); the live-server's native non-Python diff path is built on it.
    /// `js_ts_parser.wasm` is a full-parse grammar, so the filtered-CST args are unused ("").
    #[test]
    #[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged parser wasm (set INTENTUMDIFF_TEST_WASM_DIR or enable tier-c-wasm)")]
    fn native_wasm_parse_typescript_pair_is_language_agnostic() {
        let wasm = bundled_wasm_path("js_ts_parser.wasm");
        let old_src = "function greet(name: string) {\n  return \"hi \" + name;\n}\n";
        let new_src = "function greet(name: string) {\n  return \"hello \" + name;\n}\n";
        let (old_tree_json, new_tree_json) = run_python_wasm_process_pair(
            &wasm,
            old_src,
            "",
            "greet.ts",
            new_src,
            "",
            "greet.ts",
            u64::MAX,
            16 * 1024 * 1024,
            "typescript",
        )
        .expect("typescript parser must parse the pair natively via the wasm host");

        // Both outputs are well-formed semantic trees (a real tree, not an in-band {error}
        // envelope) parsed exactly as the batch consumes them.
        let old_tree = parse_semantic_tree_json(&old_tree_json)
            .unwrap_or_else(|e| panic!("old tree must be a SemanticNode ({e}): {old_tree_json}"));
        let new_tree = parse_semantic_tree_json(&new_tree_json)
            .unwrap_or_else(|e| panic!("new tree must be a SemanticNode ({e}): {new_tree_json}"));
        assert!(
            !old_tree.children.is_empty(),
            "old tree should have children: {old_tree_json}"
        );
        assert!(
            !new_tree.children.is_empty(),
            "new tree should have children: {new_tree_json}"
        );
        // The string-literal edit is visible: the two trees differ, while the function identity
        // (`greet`) survives on both sides.
        assert_ne!(
            old_tree_json, new_tree_json,
            "the string-literal edit must change the tree"
        );
        assert!(old_tree_json.contains("greet"), "old tree names greet");
        assert!(new_tree_json.contains("greet"), "new tree names greet");
    }

    fn module_with_nodes(children: Vec<SemanticNode>) -> SemanticNode {
        SemanticNode {
            id: "0".to_owned(),
            node_type: "module".to_owned(),
            label: "module".to_owned(),
            position: NodePosition {
                start_line: 0,
                start_col: 0,
                end_line: 10,
                end_col: 0,
            },
            structural_hash: "module-hash".to_owned(),
            children,
            parent_type: None,
            type_info: None,
            facts: None,
        }
    }

    fn candidate_signature_from_payload(payload: &Value) -> Vec<Value> {
        let mut signature = Vec::new();
        for item in payload["diffs"].as_array().unwrap() {
            signature.extend(
                item["candidate_signature"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .cloned(),
            );
        }
        signature
    }

    fn large_python_sources(file_index: usize, functions_per_file: usize) -> (String, String) {
        let mut old_source = String::new();
        let mut new_source = String::new();
        for fn_index in 0..functions_per_file {
            let name = format!("calculate_{file_index}_{fn_index}");
            old_source.push_str(&format!(
                "def {name}(value):\n    total = value + {fn_index}\n    return total\n\n"
            ));
            let new_name = if fn_index % 5 == 0 {
                format!("{name}_checked")
            } else {
                name
            };
            new_source.push_str(&format!(
                "def {new_name}(value):\n    total = value + {fn_index}\n    if total < 0:\n        return 0\n    return total\n\n"
            ));
        }
        (
            old_source.trim_end().to_owned(),
            new_source.trim_end().to_owned(),
        )
    }








    // ---- Markdown section review stage (issue #36) — dual-layer oracle pins ----



    // ---- Generic text review stage (issue #35) — dual-layer oracle pins ----









    fn reorder_draft<'a>(
        old_node: &'a SemanticNode,
        new_node: &'a SemanticNode,
        old_index: usize,
        new_index: usize,
    ) -> ChangeDraft<'a> {
        ChangeDraft {
            change_type: "REORDER",
            old_node: Some(old_node),
            new_node: Some(new_node),
            old_index: Some(old_index),
            new_index: Some(new_index),
            confidence: 1.0,
            description: format!("Reorder [{old_index} -> {new_index}]"),
            refactoring_kind: None,
            text_diff: None,
        }
    }


    // ───────────────────────────────────────────────────────────────────
    // Language-agnostic semantic-tree entrypoint (`diff_semantic_tree_impl`)
    // These tests lock the claim from migration step 2 in
    // `docs/ENGINE_BOUNDARY_AUDIT.md`: the GumTree matching/edit-script
    // pipeline is shape-driven and not Python-specific. Each test feeds a
    // non-Python SemanticNode shape through the entrypoint and asserts the
    // same invariants that the Python path already certifies.
    // ───────────────────────────────────────────────────────────────────

    fn diff_semantic_tree_for_test(
        old_tree: &SemanticNode,
        new_tree: &SemanticNode,
        language: &str,
    ) -> Value {
        let old_json = serde_json::to_string(old_tree).unwrap();
        let new_json = serde_json::to_string(new_tree).unwrap();
        let out = diff_semantic_tree_impl(
            &old_json,
            &new_json,
            "example",
            "example",
            language,
            "{}",
            "rust_core_semantic_tree_v3",
        )
        .expect("diff_semantic_tree_impl should succeed for well-formed trees");
        serde_json::from_str(&out).expect("output should be valid JSON")
    }

    #[allow(dead_code)]
    fn matching_pairs_set(payload: &Value) -> Vec<(String, String)> {
        let mut pairs: Vec<(String, String)> = payload["matching_pairs"]
            .as_array()
            .expect("matching_pairs must be an array")
            .iter()
            .map(|pair| {
                (
                    pair["old_id"]
                        .as_str()
                        .expect("old_id")
                        .to_owned(),
                    pair["new_id"]
                        .as_str()
                        .expect("new_id")
                        .to_owned(),
                )
            })
            .collect();
        pairs.sort();
        pairs
    }

       fn change_types(payload: &Value) -> Vec<String> {
        payload["changes"]
            .as_array()
            .expect("changes must be an array")
            .iter()
            .map(|change| {
                change["change_type"]
                    .as_str()
                    .expect("change_type")
                    .to_owned()
            })
            .collect()
    }












    fn stage11_finalizer_request(old_source: &str, new_source: &str) -> Value {
        let stage11 = diff_python_sources_stage11_impl(
            old_source,
            new_source,
            "example.py",
            "example.py",
            &python_wasm_path(),
            "{}",
        )
        .unwrap();
        json!({
            "old_tree": stage11["old_tree"].clone(),
            "new_tree": stage11["new_tree"].clone(),
            "changes": stage11["changes"].clone(),
            "matching_pairs": stage11["matching_pairs"].clone(),
            "old_source": old_source,
            "new_source": new_source,
            "old_filename": "example.py",
            "new_filename": "example.py",
            "language": "python",
            "config": {},
            "metadata": {"source": "rust-test"},
        })
    }



































    fn addition_draft<'a>(new_node: &'a SemanticNode) -> ChangeDraft<'a> {
        ChangeDraft {
            change_type: "ADDITION",
            old_node: None,
            new_node: Some(new_node),
            old_index: None,
            new_index: Some(0),
            confidence: 1.0,
            description: String::new(),
            refactoring_kind: None,
            text_diff: None,
        }
    }

#[path = "tests_finalize_certification.rs"]
mod finalize_certification;
#[path = "tests_host_and_batch.rs"]
mod host_and_batch;
#[path = "tests_matching_and_promotion.rs"]
mod matching_and_promotion;
#[path = "tests_review_shaping.rs"]
mod review_shaping;

// Rust language-parity harness (#23): a 1:1 port of tests/unit/construct_edit_matrix.py driven
// over the same corpus, so every language the Python matrix covers is also covered from
// `cargo test` - in-process, no subprocess, no GIL.
#[path = "tests_edit_matrix_port.rs"]
mod edit_matrix_port;
#[path = "tests_edit_matrix_corpus.rs"]
mod edit_matrix_corpus;
#[path = "tests_edit_matrix.rs"]
mod edit_matrix;
