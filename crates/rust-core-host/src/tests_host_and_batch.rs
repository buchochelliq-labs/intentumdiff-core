// Split from tests_inline.rs (issue #85): one file per test family.
// Nested inside cfg(test) mod tests - `super::*` = the tests mod (helpers),
// `crate::*` = the engine.
#![allow(unused_imports)]
use super::*;
use crate::*;

    #[test]
    fn byte_limit_rejects_oversized_text() {
        assert!(check_byte_limit("parser output", "abcdef", 3).is_err());
        assert!(check_byte_limit("parser output", "abc", 3).is_ok());
    }
    #[test]
    fn host_utils_rejects_excessive_trivia_type_count() {
        let trivia = vec!["comment".to_owned(); HOST_UTILS_MAX_TRIVIA_TYPES + 1];

        let result = check_trivia_type_limit(&trivia);

        assert!(result.unwrap_err().contains("host-utils trivia type count"));
    }
    #[test]
    fn host_utils_rejects_excessive_trivia_type_bytes() {
        let trivia = vec!["x".repeat(HOST_UTILS_MAX_TRIVIA_TYPE_BYTES + 1)];

        let result = check_trivia_type_limit(&trivia);

        assert!(result.unwrap_err().contains("host-utils trivia type is"));
    }
    #[test]
    fn strip_trivia_still_accepts_normal_trivia_types() {
        let payload = json!({
            "type": "module",
            "children": [
                {"type": "comment", "text": "# ignored", "children": []},
                {"type": "identifier", "text": "x", "children": []}
            ]
        })
        .to_string();

        let stripped = strip_trivia_json(&payload, &["comment".to_owned()]).unwrap();

        assert!(!stripped.contains("# ignored"));
        assert!(stripped.contains("\"identifier\""));
    }
    #[test]
    fn safe_working_tree_path_accepts_contained_file() {
        let root = temp_test_dir("safe-working-tree-contained");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/app.py"), "print('ok')\n").unwrap();

        let resolved = safe_working_tree_path_rust(&root, "src/app.py")
            .unwrap()
            .unwrap();

        assert!(resolved.starts_with(root.canonicalize().unwrap()));
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn safe_working_tree_path_rejects_symlink_escape_when_supported() {
        let root = temp_test_dir("safe-working-tree-symlink-root");
        let outside = temp_test_dir("safe-working-tree-symlink-outside");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.py"), "print('secret')\n").unwrap();
        let link = root.join("src/linked.py");
        if create_file_symlink(outside.join("secret.py"), &link).is_err() {
            let _ = fs::remove_dir_all(root);
            let _ = fs::remove_dir_all(outside);
            return;
        }

        let result = safe_working_tree_path_rust(&root, "src/linked.py");

        assert!(result.is_err());
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }
    #[test]
    #[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged parser wasm (set INTENTUMDIFF_TEST_WASM_DIR or enable tier-c-wasm)")]
    fn batch_swap_of_two_functions_surfaces_moved_code() {
        // Issue #12 end-to-end: swapping two functions through the REAL batch pipeline must
        // surface moved code, never read as style-only / zero changes.
        let wasm_path = python_wasm_path();
        let request = json!({
            "schema_version": 1,
            "config": {},
            "files": [{
                "old_source": "def greet(name):\n    print('Hi ' + name)\n\ndef add(a, b):\n    return a + b\n",
                "new_source": "def add(a, b):\n    return a + b\n\ndef greet(name):\n    print('Hi ' + name)\n",
                "old_filename": "m.py",
                "new_filename": "m.py",
                "language": "python",
                "parser_wasm_path": wasm_path
            }]
        });
        let out = diff_batch(&request.to_string()).unwrap();
        let payload: Value = serde_json::from_str(&out).unwrap();
        let item = &payload["diffs"][0];
        let diff = if item.get("diff").is_some() { &item["diff"] } else { item };
        let changes = diff["changes"].as_array().unwrap();
        assert!(
            !changes.is_empty(),
            "swap must not be zero changes/style-only: {diff}"
        );
        assert!(
            changes.iter().any(|c| c["change_type"] == "MOVE"),
            "swap must surface a MOVE: {changes:?}"
        );
    }
    #[test]
    #[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged parser wasm (set INTENTUMDIFF_TEST_WASM_DIR or enable tier-c-wasm)")]
    fn batch_adding_decorator_is_one_addition_no_false_moves_or_pairings() {
        // Issue #32 end-to-end: adding @cached above calc must not (a) report the untouched
        // class Box as a MOVE (it only shifted down one line), (b) fabricate an x->calc
        // identifier pairing, or (c) fail to match calc across the decorated_definition
        // re-parenting. The decorator addition surfaces as the wrapper's ADDITION.
        let wasm_path = python_wasm_path();
        let request = json!({
            "schema_version": 1,
            "config": {},
            "files": [{
                "old_source": "def calc(x):\n    return x * 2\n\nclass Box:\n    def get(self):\n        return self.v\n",
                "new_source": "@cached\ndef calc(x):\n    return x * 2\n\nclass Box:\n    def get(self):\n        return self.v\n",
                "old_filename": "m.py",
                "new_filename": "m.py",
                "language": "python",
                "parser_wasm_path": wasm_path
            }]
        });
        let out = diff_batch(&request.to_string()).unwrap();
        let payload: Value = serde_json::from_str(&out).unwrap();
        let item = &payload["diffs"][0];
        let diff = if item.get("diff").is_some() { &item["diff"] } else { item };
        let changes = diff["changes"].as_array().unwrap();
        assert!(
            !changes.iter().any(|c| c["change_type"] == "MOVE"),
            "no false moves of shifted siblings: {changes:?}"
        );
        assert!(
            !changes.iter().any(|c| c["change_type"] == "MODIFICATION"
                && c["old_node"]["label"] == "x"
                && c["new_node"]["label"] == "calc"),
            "no fabricated x->calc pairing: {changes:?}"
        );
        assert!(
            changes.iter().any(|c| c["change_type"] == "ADDITION"
                && c["new_node"]["node_type"] == "decorated_definition"),
            "the decorator addition surfaces: {changes:?}"
        );
    }
    #[test]
    #[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged parser wasm (set INTENTUMDIFF_TEST_WASM_DIR or enable tier-c-wasm)")]
    fn batch_deleted_function_never_vanishes_behind_cross_matched_literals() {
        // Issue #31 end-to-end: deleting one function and adding an unrelated one must yield
        // DELETION + ADDITION. Previously the deleted function's internals label-matched the
        // added function's internals, fabricating literal modifications whose covered-label
        // suppression swallowed the DELETION — removed code became invisible.
        let wasm_path = python_wasm_path();
        let request = json!({
            "schema_version": 1,
            "config": {},
            "files": [{
                "old_source": "def old_one():\n    return 1\n\ndef keep():\n    return 0\n",
                "new_source": "def keep():\n    return 0\n\ndef new_one():\n    return 2\n",
                "old_filename": "m.py",
                "new_filename": "m.py",
                "language": "python",
                "parser_wasm_path": wasm_path
            }]
        });
        let out = diff_batch(&request.to_string()).unwrap();
        let payload: Value = serde_json::from_str(&out).unwrap();
        let item = &payload["diffs"][0];
        let diff = if item.get("diff").is_some() { &item["diff"] } else { item };
        let changes = diff["changes"].as_array().unwrap();
        assert!(
            changes.iter().any(|c| c["change_type"] == "DELETION"
                && c["old_node"]["label"] == "old_one"),
            "the deleted function must surface: {changes:?}"
        );
        assert!(
            changes.iter().any(|c| c["change_type"] == "ADDITION"
                && c["new_node"]["label"] == "new_one"),
            "the added function must surface: {changes:?}"
        );
        assert!(
            !changes.iter().any(|c| c["change_type"] == "MODIFICATION"
                && c["old_node"]["node_type"] == "integer"),
            "no cross-matched literal modifications: {changes:?}"
        );
    }
    #[test]
    fn entity_fast_path_seeds_exact_id_entities_without_changing_matching_shape() {
        let old_tree = module_with_nodes(vec![node(
            "0.0",
            "function_definition",
            "charge_total",
            vec![node("0.0.0", "identifier", "charge_total", Vec::new())],
        )]);
        let new_tree = module_with_nodes(vec![node(
            "0.0",
            "function_definition",
            "charge_total",
            vec![node("0.0.0", "identifier", "charge_total", Vec::new())],
        )]);

        let report = compute_matching_with_diagnostics(&old_tree, &new_tree, 2, 0.5);

        assert!(report
            .pairs
            .iter()
            .any(|pair| pair.old_node.id == "0.0" && pair.new_node.id == "0.0"));
        assert!(report.diagnostics.used);
        assert_eq!(report.diagnostics.exact_id_matches, 1);
        assert_eq!(report.diagnostics.seeded_matches, 1);
        assert!(report.diagnostics.descendant_seeded_matches >= 1);
    }
    #[test]
    fn source_stage_reports_scaffold_when_wasm_missing() {
        let out = diff_python_sources_stage11_json(
            "def old():\n    pass\n",
            "def new():\n    pass\n",
            "example.py",
            "example.py",
            "missing_python_parser.wasm",
            "{}",
        )
        .unwrap();
        let payload: Value = serde_json::from_str(&out).unwrap();

        assert_eq!(payload["status"], SCAFFOLD);
        assert_eq!(payload["engine"], V3_ENGINE);
        assert!(payload["reason"]
            .as_str()
            .unwrap()
            .contains("parser wasm path not found"));
    }
    #[test]
    fn batch_boundary_returns_final_diff_for_no_change_python_pair() {
        let request = json!({
            "schema_version": 1,
            "config": {},
            "files": [{
                "old_source": "def total(items):\n    return sum(items)\n",
                "new_source": "def total(items):\n    return sum(items)\n",
                "old_filename": "example.py",
                "new_filename": "example.py",
                "language": "python",
                "parser_wasm_path": "not-needed-for-no-change.wasm"
            }]
        });
        let out = diff_batch(&request.to_string()).unwrap();
        let payload: Value = serde_json::from_str(&out).unwrap();

        assert_eq!(payload["status"], COMPLETE);
        assert_eq!(payload["engine"], BATCH_ENGINE);
        assert_eq!(payload["diffs"][0]["status"], COMPLETE);
        assert_eq!(
            payload["diffs"][0]["diff"]["metadata"]["rust_core"]["engine"],
            BATCH_ENGINE
        );
    }
    #[test]
    #[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged parser wasm (set INTENTUMDIFF_TEST_WASM_DIR or enable tier-c-wasm)")]
    fn batch_commit_json_suppresses_bytes_when_any_item_falls_back() {
        let request = json!({
            "schema_version": 1,
            "python_parser_backend": "native",
            "config": {},
            "files": [
                {
                    "old_source": "def ok():\n    return 1\n",
                    "new_source": "def ok():\n    return 2\n",
                    "old_filename": "ok.py",
                    "new_filename": "ok.py",
                    "language": "python",
                    "parser_plugin_id": "python",
                    "parser_wasm_path": python_wasm_path()
                },
                {
                    "old_source": "package main\n",
                    "new_source": "package main\n",
                    "old_filename": "main.go",
                    "new_filename": "main.go",
                    "language": "go",
                    "parser_wasm_path": "go_parser.wasm"
                }
            ]
        });
        let (control_json, commit_json) = diff_batch_commit_json(&request.to_string()).unwrap();
        let control: Value = serde_json::from_str(&control_json).unwrap();

        assert_eq!(control["status"], FALLBACK);
        assert!(commit_json.is_none());
    }
    #[test]
    #[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged parser wasm (set INTENTUMDIFF_TEST_WASM_DIR or enable tier-c-wasm)")]
    fn batch_commit_json_requires_native_backend() {
        let request = json!({
            "schema_version": 1,
            "python_parser_backend": "wasm",
            "config": {},
            "files": [{
                "old_source": "def ok():\n    return 1\n",
                "new_source": "def ok():\n    return 1\n",
                "old_filename": "ok.py",
                "new_filename": "ok.py",
                "language": "python",
                "parser_wasm_path": python_wasm_path()
            }]
        });
        let (control_json, commit_json) = diff_batch_commit_json(&request.to_string()).unwrap();
        let control: Value = serde_json::from_str(&control_json).unwrap();

        assert_eq!(control["status"], FALLBACK);
        assert!(control["reason"]
            .as_str()
            .unwrap()
            .contains("native first-party Python backend"));
        assert!(commit_json.is_none());
    }
    #[test]
    #[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged parser wasm (set INTENTUMDIFF_TEST_WASM_DIR or enable tier-c-wasm)")]
    fn batch_boundary_returns_final_diff_for_changed_python_pair() {
        let request = json!({
            "schema_version": 1,
            "config": {},
            "files": [{
                "old_source": "def old():\n    pass\n",
                "new_source": "def new():\n    pass\n",
                "old_filename": "example.py",
                "new_filename": "example.py",
                "language": "python",
                "parser_wasm_path": python_wasm_path()
            }]
        });
        let out = diff_batch(&request.to_string()).unwrap();
        let payload: Value = serde_json::from_str(&out).unwrap();

        assert_eq!(payload["status"], COMPLETE);
        assert_eq!(payload["engine"], BATCH_ENGINE);
        assert_eq!(payload["diffs"][0]["status"], COMPLETE);
        assert_eq!(
            payload["diffs"][0]["diff"]["metadata"]["rust_core"]["status"],
            COMPLETE
        );
        assert_eq!(
            payload["diffs"][0]["diff"]["metadata"]["rust_core"]["details"]["certification"],
            PYTHON_V4E_CERTIFICATION
        );
        assert!(
            payload["diffs"][0]["diff"]["changes"]
                .as_array()
                .unwrap()
                .len()
                > 0
        );
    }
    #[test]
    #[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged parser wasm (set INTENTUMDIFF_TEST_WASM_DIR or enable tier-c-wasm)")]
    fn batch_boundary_preserves_original_order_for_multiple_python_files() {
        let request = json!({
            "schema_version": 1,
            "config": {},
            "files": [
                {
                    "old_source": "def b():\n    return 1\n",
                    "new_source": "def b():\n    return 2\n",
                    "old_filename": "zeta.py",
                    "new_filename": "zeta.py",
                    "language": "python",
                    "parser_wasm_path": python_wasm_path()
                },
                {
                    "old_source": "def a():\n    return 1\n",
                    "new_source": "def a():\n    return 2\n",
                    "old_filename": "alpha.py",
                    "new_filename": "alpha.py",
                    "language": "python",
                    "parser_wasm_path": python_wasm_path()
                }
            ]
        });
        let out = diff_batch(&request.to_string()).unwrap();
        let payload: Value = serde_json::from_str(&out).unwrap();

        assert_eq!(payload["status"], COMPLETE);
        assert_eq!(payload["diffs"][0]["new_filename"], "zeta.py");
        assert_eq!(payload["diffs"][1]["new_filename"], "alpha.py");
        assert_eq!(payload["metadata"]["batch_size"], 2);
        assert_eq!(payload["metadata"]["complete_count"], 2);
    }
    #[test]
    #[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged parser wasm (set INTENTUMDIFF_TEST_WASM_DIR or enable tier-c-wasm)")]
    fn parallel_batch_matches_sequential_order_and_reports_workers() {
        let files = json!([
            {
                "old_source": "def one():\n    return 1\n",
                "new_source": "def one():\n    return 2\n",
                "old_filename": "one.py",
                "new_filename": "one.py",
                "language": "python",
                "parser_wasm_path": python_wasm_path()
            },
            {
                "old_source": "def two():\n    return 1\n",
                "new_source": "def two():\n    return 2\n",
                "old_filename": "two.py",
                "new_filename": "two.py",
                "language": "python",
                "parser_wasm_path": python_wasm_path()
            }
        ]);
        let sequential_request = json!({
            "schema_version": 1,
            "config": {},
            "parallel": false,
            "files": files.clone()
        });
        let parallel_request = json!({
            "schema_version": 1,
            "config": {},
            "parallel": true,
            "max_workers": 2,
            "files": files
        });
        let sequential: Value =
            serde_json::from_str(&diff_batch(&sequential_request.to_string()).unwrap()).unwrap();
        let parallel: Value =
            serde_json::from_str(&diff_batch(&parallel_request.to_string()).unwrap()).unwrap();

        assert_eq!(parallel["status"], sequential["status"]);
        assert_eq!(parallel["metadata"]["parallel"], true);
        assert_eq!(parallel["metadata"]["parallel_workers"], 2);
        assert_eq!(parallel["metadata"]["file_timing"]["count"], 2);
        assert!(
            parallel["metadata"]["file_timing"]["slowest_files"]
                .as_array()
                .unwrap()
                .len()
                <= 2
        );
        assert!(parallel["metadata"]["phase_timings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|phase| phase["name"] == "rust_batch_schedule"));
        assert!(parallel["metadata"]["phase_timings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|phase| phase["name"] == "rust_batch_file_execution"));
        assert!(parallel["metadata"]["phase_timings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|phase| phase["name"] == "rust_batch_response_assembly"));
        assert_eq!(
            parallel["diffs"][0]["new_filename"],
            sequential["diffs"][0]["new_filename"]
        );
        assert_eq!(
            parallel["diffs"][1]["new_filename"],
            sequential["diffs"][1]["new_filename"]
        );
        assert_eq!(
            parallel["diffs"][0]["diff"]["changes"]
                .as_array()
                .unwrap()
                .len(),
            sequential["diffs"][0]["diff"]["changes"]
                .as_array()
                .unwrap()
                .len()
        );
    }
    #[test]
    fn mixed_batch_reports_partial_fallback_without_panicking() {
        let request = json!({
            "schema_version": 1,
            "config": {},
            "files": [
                {
                    "old_source": "def ok():\n    return 1\n",
                    "new_source": "def ok():\n    return 1\n",
                    "old_filename": "ok.py",
                    "new_filename": "ok.py",
                    "language": "python",
                    "parser_wasm_path": "not-needed.wasm"
                },
                {
                    "old_source": "package main\n",
                    "new_source": "package main\n",
                    "old_filename": "main.go",
                    "new_filename": "main.go",
                    "language": "go",
                    "parser_wasm_path": "go_parser.wasm"
                }
            ]
        });
        let out = diff_batch(&request.to_string()).unwrap();
        let payload: Value = serde_json::from_str(&out).unwrap();

        assert_eq!(payload["status"], PARTIAL);
        assert_eq!(payload["metadata"]["complete_count"], 1);
        assert_eq!(payload["metadata"]["fallback_count"], 1);
        assert_eq!(payload["diffs"][1]["status"], FALLBACK);
        assert_eq!(payload["diffs"][1]["reason"], "unsupported language");
    }
    #[test]
    #[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged parser wasm (set INTENTUMDIFF_TEST_WASM_DIR or enable tier-c-wasm)")]
    fn batch_candidate_returns_changed_python_candidate_when_wasm_available() {
        let request = json!({
            "schema_version": 1,
            "candidate": true,
            "config": {"plugin_fuel": 40000000},
            "files": [{
                "old_source": "def total(items):\n    return sum(items)\n",
                "new_source": "def total(items):\n    return sum(item for item in items)\n",
                "old_filename": "example.py",
                "new_filename": "example.py",
                "language": "python",
                "parser_wasm_path": python_wasm_path()
            }]
        });
        let out = diff_batch(&request.to_string()).unwrap();
        let payload: Value = serde_json::from_str(&out).unwrap();

        assert_eq!(payload["status"], CANDIDATE);
        assert_eq!(payload["diffs"][0]["status"], CANDIDATE);
        assert!(
            payload["diffs"][0]["candidate_diff"]["changes"]
                .as_array()
                .unwrap()
                .len()
                > 0
        );
        assert!(payload["diffs"][0]["candidate_signature"].is_array());
        assert!(payload["diffs"][0]["phase_timings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|phase| phase["name"] == "rust_wasm_process_old"));
        assert!(payload["diffs"][0]["phase_timings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|phase| phase["name"] == "rust_wasm_process_new"));
        assert!(payload["diffs"][0]["phase_timings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|phase| phase["name"] == "rust_edit_delete_generation"));
        assert!(payload["diffs"][0]["phase_timings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|phase| phase["name"] == "rust_change_draft_generation"));
        assert!(payload["diffs"][0]["phase_timings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|phase| phase["name"] == "rust_change_draft_refinement"));
        assert!(payload["diffs"][0]["phase_timings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|phase| phase["name"] == "rust_change_draft_serialization"));
        assert!(
            payload["diffs"][0]["candidate_diff"]["metadata"]["rust_core"]["details"]
                ["entity_fast_path"]["edit_script"]
                .is_object()
        );
        assert!(
            payload["diffs"][0]["candidate_diff"]["metadata"]["rust_core"]["details"]
                ["entity_fast_path"]["edit_script"]["serialized_final_change_count"]
                .as_u64()
                .is_some()
        );
        assert_eq!(
            payload["diffs"][0]["candidate_note"],
            "candidate remains benchmark/parity evidence"
        );
    }
    #[test]
    fn batch_native_candidate_bypasses_wasm_and_reports_native_phases() {
        let request = json!({
            "schema_version": 1,
            "candidate": true,
            "python_parser_backend": "native",
            "config": {"plugin_fuel": 40000000},
            "files": [{
                "old_source": "def total(items):\n    return sum(items)\n",
                "new_source": "def total(items):\n    return sum(item for item in items)\n",
                "old_filename": "example.py",
                "new_filename": "example.py",
                "language": "python",
                "parser_wasm_path": "missing_python_parser.wasm"
            }]
        });
        let out = diff_batch(&request.to_string()).unwrap();
        let payload: Value = serde_json::from_str(&out).unwrap();
        let phases = payload["diffs"][0]["phase_timings"].as_array().unwrap();

        assert_eq!(payload["status"], CANDIDATE);
        assert_eq!(
            payload["metadata"]["python_parser_backend"],
            PYTHON_PARSER_BACKEND_NATIVE
        );
        assert_eq!(payload["diffs"][0]["status"], CANDIDATE);
        assert_eq!(
            payload["diffs"][0]["candidate_diff"]["metadata"]["rust_core"]["details"]
                ["python_parser_backend"],
            PYTHON_PARSER_BACKEND_NATIVE
        );
        assert_eq!(
            payload["diffs"][0]["candidate_diff"]["metadata"]["rust_core"]["details"]
                ["wasm_boundary"],
            "bypassed_first_party_native_python"
        );
        assert!(phases
            .iter()
            .any(|phase| phase["name"] == "rust_native_semantic_build_old"));
        assert!(phases
            .iter()
            .any(|phase| phase["name"] == "rust_native_semantic_hashing_new"));
        assert!(!phases
            .iter()
            .any(|phase| phase["name"] == "rust_wasm_process_old"));
    }
    #[test]
    fn batch_native_product_returns_complete_with_v4kb_metadata() {
        let request = json!({
            "schema_version": 1,
            "python_parser_backend": "native",
            "config": {"plugin_fuel": 40000000},
            "files": [{
                "old_source": "def total(items):\n    return sum(items)\n",
                "new_source": "def total(items):\n    return sum(item for item in items)\n",
                "old_filename": "example.py",
                "new_filename": "example.py",
                "language": "python",
                "parser_wasm_path": "missing_python_parser.wasm"
            }]
        });
        let out = diff_batch(&request.to_string()).unwrap();
        let payload: Value = serde_json::from_str(&out).unwrap();
        let diff = &payload["diffs"][0]["diff"];
        let details = &diff["metadata"]["rust_core"]["details"];
        let phases = payload["diffs"][0]["phase_timings"].as_array().unwrap();

        assert_eq!(payload["status"], COMPLETE);
        assert_eq!(
            payload["metadata"]["python_parser_backend"],
            PYTHON_PARSER_BACKEND_NATIVE
        );
        assert_eq!(
            payload["metadata"]["certification"],
            PYTHON_NATIVE_V4KB_CERTIFICATION
        );
        assert_eq!(
            payload["metadata"]["trust_tier"],
            "first_party_core_builder"
        );
        assert_eq!(payload["diffs"][0]["status"], COMPLETE);
        assert_eq!(details["certification"], PYTHON_NATIVE_V4KB_CERTIFICATION);
        assert_eq!(
            details["python_parser_backend"],
            PYTHON_PARSER_BACKEND_NATIVE
        );
        assert_eq!(details["trust_tier"], "first_party_core_builder");
        assert_eq!(
            details["wasm_boundary"],
            "bypassed_first_party_native_python"
        );
        let telemetry = &diff["metadata"]["engine_telemetry"];
        let call = &telemetry["calls"][0];
        assert_eq!(telemetry["schema_version"], 1);
        assert_eq!(call["plugin"], "intentumdiff_rust_core");
        assert_eq!(call["function"], "finalize");
        assert_eq!(call["engine_owner"], "rust");
        assert_eq!(call["engine"], BATCH_ENGINE);
        assert_eq!(call["provenance"], "first_party_native");
        assert_eq!(call["parser_backend"], PYTHON_PARSER_BACKEND_NATIVE);
        assert_eq!(
            call["wasm_boundary"],
            "bypassed_first_party_native_python"
        );
        assert_eq!(call["trusted"], true);
        assert!(phases
            .iter()
            .any(|phase| phase["name"] == "rust_native_semantic_build_old"));
        assert!(phases
            .iter()
            .any(|phase| phase["name"] == "rust_refine_annotate_moved_context"));
        assert!(!phases
            .iter()
            .any(|phase| phase["name"] == "rust_wasm_process_old"));
    }
    #[test]
    fn batch_native_product_attaches_scope_trails_for_nested_python_changes() {
        let request = json!({
            "schema_version": 1,
            "python_parser_backend": "native",
            "config": {"plugin_fuel": 40000000},
            "files": [{
                "old_source": "class Demo:\n    def run(self):\n        value = 1\n        return value\n",
                "new_source": "class Demo:\n    def run(self):\n        value = 2\n        return value\n",
                "old_filename": "demo.py",
                "new_filename": "demo.py",
                "language": "python",
                "parser_wasm_path": "missing_python_parser.wasm"
            }]
        });
        let out = diff_batch(&request.to_string()).unwrap();
        let payload: Value = serde_json::from_str(&out).unwrap();
        let scope_trails = &payload["diffs"][0]["diff"]["metadata"]["scope_trails"];

        assert_eq!(payload["status"], COMPLETE);
        assert!(
            scope_trails["old"].as_array().unwrap().iter().any(|entry| {
                entry["trail"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    == vec!["class Demo", "function run"]
            }),
            "old scope trails: {scope_trails}"
        );
        assert!(
            scope_trails["new"].as_array().unwrap().iter().any(|entry| {
                entry["trail"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    == vec!["class Demo", "function run"]
            }),
            "new scope trails: {scope_trails}"
        );
    }
    #[test]
    fn batch_native_product_oversized_source_falls_back() {
        let request = json!({
            "schema_version": 1,
            "python_parser_backend": "native",
            "config": {"plugin_fuel": 40000000, "max_cst_bytes": 24},
            "files": [{
                "old_source": "def total(items):\n    return sum(items)\n",
                "new_source": "def total(items):\n    return sum(item for item in items)\n",
                "old_filename": "example.py",
                "new_filename": "example.py",
                "language": "python",
                "parser_wasm_path": "missing_python_parser.wasm"
            }]
        });
        let out = diff_batch(&request.to_string()).unwrap();
        let payload: Value = serde_json::from_str(&out).unwrap();

        assert_eq!(payload["status"], FALLBACK);
        assert_eq!(payload["diffs"][0]["status"], FALLBACK);
        assert!(payload["diffs"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("source"));
    }
    #[test]
    fn batch_native_product_style_only_falls_back() {
        let request = json!({
            "schema_version": 1,
            "python_parser_backend": "native",
            "config": {"plugin_fuel": 40000000},
            "files": [{
                "old_source": "def total(items):\n    return sum(items)\n",
                "new_source": "# comment\n\ndef total(items):\n    return sum(items)\n",
                "old_filename": "example.py",
                "new_filename": "example.py",
                "language": "python",
                "parser_wasm_path": "missing_python_parser.wasm"
            }]
        });
        let out = diff_batch(&request.to_string()).unwrap();
        let payload: Value = serde_json::from_str(&out).unwrap();

        assert_eq!(payload["status"], FALLBACK);
        assert_eq!(payload["diffs"][0]["status"], FALLBACK);
        assert_eq!(
            payload["diffs"][0]["reason"],
            "style-only changed file is not certified for Rust product path"
        );
    }
    #[test]
    fn batch_native_product_parse_error_falls_back() {
        let request = json!({
            "schema_version": 1,
            "python_parser_backend": "native",
            "config": {"plugin_fuel": 40000000},
            "files": [{
                "old_source": "def broken(:\n    pass\n",
                "new_source": "def broken():\n    pass\n",
                "old_filename": "example.py",
                "new_filename": "example.py",
                "language": "python",
                "parser_wasm_path": "missing_python_parser.wasm"
            }]
        });
        let out = diff_batch(&request.to_string()).unwrap();
        let payload: Value = serde_json::from_str(&out).unwrap();

        assert_eq!(payload["status"], FALLBACK);
        assert_eq!(payload["diffs"][0]["status"], FALLBACK);
        assert_eq!(
            payload["diffs"][0]["reason"],
            "parse errors require Python token fallback"
        );
    }
    #[test]
    #[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged parser wasm (set INTENTUMDIFF_TEST_WASM_DIR or enable tier-c-wasm)")]
    fn native_candidate_matches_wasm_candidate_for_working_tree_small() {
        let old_source = concat!(
            "def charge_total(order):\n",
            "    def discount(subtotal):\n",
            "        if subtotal > 100:\n",
            "            return subtotal * 0.1\n",
            "        return 0\n",
            "\n",
            "    subtotal = sum(item[\"price\"] for item in order[\"items\"])\n",
            "    return subtotal - discount(subtotal)\n",
        );
        let new_source = concat!(
            "def discount(subtotal, vip=False):\n",
            "    if vip:\n",
            "        return subtotal * 0.2\n",
            "    if subtotal > 150:\n",
            "        return subtotal * 0.1\n",
            "    return 0\n",
            "\n",
            "\n",
            "def charge_total(order):\n",
            "    subtotal = sum(item[\"price\"] for item in order[\"items\"])\n",
            "    return subtotal - discount(subtotal, order.get(\"vip\", False))\n",
        );
        let base_file = json!({
            "old_source": old_source,
            "new_source": new_source,
            "old_filename": "billing.py",
            "new_filename": "billing.py",
            "language": "python",
            "parser_wasm_path": python_wasm_path()
        });
        let wasm_request = json!({
            "schema_version": 1,
            "candidate": true,
            "config": {"plugin_fuel": 40000000},
            "files": [base_file.clone()]
        });
        let native_request = json!({
            "schema_version": 1,
            "candidate": true,
            "python_parser_backend": "native",
            "config": {"plugin_fuel": 40000000},
            "files": [base_file]
        });
        let wasm: Value =
            serde_json::from_str(&diff_batch(&wasm_request.to_string()).unwrap()).unwrap();
        let native: Value =
            serde_json::from_str(&diff_batch(&native_request.to_string()).unwrap()).unwrap();

        assert_eq!(
            candidate_signature_from_payload(&native),
            candidate_signature_from_payload(&wasm)
        );
    }
    #[test]
    #[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged parser wasm (set INTENTUMDIFF_TEST_WASM_DIR or enable tier-c-wasm)")]
    fn batch_candidate_preloads_component_once_for_repeated_parser_path() {
        let wasm_path = python_wasm_path();
        let request = json!({
            "schema_version": 1,
            "candidate": true,
            "config": {"plugin_fuel": 40000000},
            "files": [
                {
                    "old_source": "def one():\n    return 1\n",
                    "new_source": "def one():\n    return 2\n",
                    "old_filename": "one.py",
                    "new_filename": "one.py",
                    "language": "python",
                    "parser_wasm_path": wasm_path
                },
                {
                    "old_source": "def two():\n    return 1\n",
                    "new_source": "def two():\n    return 2\n",
                    "old_filename": "two.py",
                    "new_filename": "two.py",
                    "language": "python",
                    "parser_wasm_path": wasm_path
                }
            ]
        });
        let out = diff_batch(&request.to_string()).unwrap();
        let payload: Value = serde_json::from_str(&out).unwrap();

        assert_eq!(payload["status"], CANDIDATE);
        assert_eq!(
            payload["metadata"]["batch_component_cache_hits"]
                .as_u64()
                .unwrap()
                + payload["metadata"]["batch_component_cache_misses"]
                    .as_u64()
                    .unwrap(),
            1
        );
        assert_eq!(
            payload["diffs"][0]["candidate_diff"]["metadata"]["rust_core"]["details"]
                ["wasm_component_batch_preloaded"],
            true
        );
        assert_eq!(
            payload["diffs"][1]["candidate_diff"]["metadata"]["rust_core"]["details"]
                ["wasm_component_batch_preloaded"],
            true
        );
    }
    #[test]
    fn batch_candidate_invalid_wasm_path_returns_fallback_record() {
        let request = json!({
            "schema_version": 1,
            "candidate": true,
            "config": {},
            "files": [{
                "old_source": "def old():\n    pass\n",
                "new_source": "def new():\n    pass\n",
                "old_filename": "example.py",
                "new_filename": "example.py",
                "language": "python",
                "parser_wasm_path": "missing_python_parser.wasm"
            }]
        });
        let out = diff_batch(&request.to_string()).unwrap();
        let payload: Value = serde_json::from_str(&out).unwrap();

        assert_eq!(payload["status"], FALLBACK);
        assert_eq!(payload["diffs"][0]["status"], FALLBACK);
        assert!(payload["diffs"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("parser wasm path not found"));
    }
    #[test]
    #[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged parser wasm (set INTENTUMDIFF_TEST_WASM_DIR or enable tier-c-wasm)")]
    fn batch_product_unsupported_parser_plugin_returns_fallback_record() {
        let request = json!({
            "schema_version": 1,
            "config": {},
            "files": [{
                "old_source": "def old():\n    pass\n",
                "new_source": "def new():\n    pass\n",
                "old_filename": "example.py",
                "new_filename": "example.py",
                "language": "python",
                "parser_plugin_id": "custom-python-parser",
                "parser_wasm_path": python_wasm_path()
            }]
        });
        let out = diff_batch(&request.to_string()).unwrap();
        let payload: Value = serde_json::from_str(&out).unwrap();

        assert_eq!(payload["status"], FALLBACK);
        assert_eq!(payload["diffs"][0]["status"], FALLBACK);
        assert_eq!(payload["diffs"][0]["reason"], "unsupported parser plugin");
    }
    #[test]
    #[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged parser wasm (set INTENTUMDIFF_TEST_WASM_DIR or enable tier-c-wasm)")]
    fn batch_product_style_only_changed_file_returns_fallback_record() {
        let request = json!({
            "schema_version": 1,
            "config": {},
            "files": [{
                "old_source": "def total(items):\n    return sum(items)\n",
                "new_source": "# comment\n\ndef total(items):\n    return sum(items)\n",
                "old_filename": "example.py",
                "new_filename": "example.py",
                "language": "python",
                "parser_wasm_path": python_wasm_path()
            }]
        });
        let out = diff_batch(&request.to_string()).unwrap();
        let payload: Value = serde_json::from_str(&out).unwrap();

        assert_eq!(payload["status"], FALLBACK);
        assert_eq!(payload["diffs"][0]["status"], FALLBACK);
        assert!(payload["diffs"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("style-only changed file"));
    }
    #[test]
    #[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged parser wasm (set INTENTUMDIFF_TEST_WASM_DIR or enable tier-c-wasm)")]
    fn batch_product_parse_error_returns_fallback_record() {
        let request = json!({
            "schema_version": 1,
            "config": {},
            "files": [{
                "old_source": "def broken(:\n    pass\n",
                "new_source": "def fixed():\n    pass\n",
                "old_filename": "example.py",
                "new_filename": "example.py",
                "language": "python",
                "parser_wasm_path": python_wasm_path()
            }]
        });
        let out = diff_batch(&request.to_string()).unwrap();
        let payload: Value = serde_json::from_str(&out).unwrap();

        assert_eq!(payload["status"], FALLBACK);
        assert_eq!(payload["diffs"][0]["status"], FALLBACK);
        assert!(payload["diffs"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("parse errors require Python token fallback"));
    }
    #[test]
    fn path_enrichment_recovers_markup_labels_attributes_and_text() {
        // python path_profiles.enrich_path_profile_labels parity (html): a generic
        // element label takes the tag name; opening-tag attributes re-inject as
        // synthetic attribute children under a synthetic start_tag; the direct text
        // becomes a synthetic text child; hashes recompute bottom-up.
        let mut element = node("0.0", "element", "element", Vec::new());
        element.position.end_col = 34; // <div id="hero" class="a">Hello</div>
        let root = module_with_nodes(vec![element]);
        let payload = enrich_profile_labels_json(
            &serde_json::to_string(&root).unwrap(),
            "<div id=\"hero\" class=\"a\">Hi</div>
",
            "html",
            None,
        )
        .unwrap();
        let enriched: SemanticNode = serde_json::from_str(&payload).unwrap();
        let element = &enriched.children[0];
        assert_eq!(element.label, "div", "generic label takes the tag name");
        let start_tag = &element.children[0];
        assert_eq!(start_tag.node_type, "start_tag");
        let attr_labels: Vec<&str> = start_tag
            .children
            .iter()
            .map(|c| c.label.as_str())
            .collect();
        assert_eq!(attr_labels, vec!["id=hero", "class=a"], "attrs re-injected");
        assert!(
            element.children.iter().any(|c| c.node_type == "text" && c.label == "Hi"),
            "direct text becomes a synthetic child: {:?}",
            element.children.iter().map(|c| (&c.node_type, &c.label)).collect::<Vec<_>>()
        );
        assert_eq!(
            element.structural_hash,
            synthetic_structural_hash("element", "div", &element.children),
            "hash recomputed over enriched label + children"
        );
    }
    #[test]
    fn path_enrichment_recovers_css_selectors_and_declaration_values() {
        // css: rule_set takes the pre-brace selector; declarations split name:value
        // with a synthetic property_value child.
        let mut decl = node("0.0.0", "declaration", "declaration", Vec::new());
        decl.position.start_col = 9;
        decl.position.end_col = 20;
        let mut rule = node("0.0", "rule_set", "rule_set", vec![decl]);
        rule.position.end_col = 22;
        let root = module_with_nodes(vec![rule]);
        let payload = enrich_profile_labels_json(
            &serde_json::to_string(&root).unwrap(),
            ".button { color: red }
",
            "css",
            None,
        )
        .unwrap();
        let enriched: SemanticNode = serde_json::from_str(&payload).unwrap();
        let rule = &enriched.children[0];
        assert_eq!(rule.label, ".button", "selector from the pre-brace span");
        let decl = &rule.children[0];
        assert_eq!(decl.label, "color", "declaration name before the colon");
        assert!(
            decl.children.iter().any(|c| c.node_type == "property_value" && c.label == "red"),
            "synthetic property_value: {:?}",
            decl.children.iter().map(|c| (&c.node_type, &c.label)).collect::<Vec<_>>()
        );
    }

#[test]
fn validate_git_ref_blocks_option_injection() {
    // A ref that git would parse as an option (arbitrary file write via
    // `git diff --output=<path>`) is rejected. Security review #88.
    assert!(validate_git_ref("--output=/tmp/pwned").is_err());
    assert!(validate_git_ref("-O/tmp/x").is_err());
    // Newline/NUL injection into the cat-file --batch request is rejected.
    assert!(validate_git_ref("HEAD\n../../etc:x").is_err());
    assert!(validate_git_ref("HEAD\0").is_err());
    // Legitimate refs (and the empty working-tree ref) pass.
    assert!(validate_git_ref("HEAD").is_ok());
    assert!(validate_git_ref("origin/main").is_ok());
    assert!(validate_git_ref("v1.2.3").is_ok());
    assert!(validate_git_ref("a1b2c3d4e5f6").is_ok());
    assert!(validate_git_ref("").is_ok());
}
