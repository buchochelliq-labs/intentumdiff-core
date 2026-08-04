// Split from tests_inline.rs (issue #85): one file per test family.
// Nested inside cfg(test) mod tests - `super::*` = the tests mod (helpers),
// `crate::*` = the engine.
#![allow(unused_imports)]
use super::*;
use crate::*;

    #[test]
    #[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged parser wasm (set INTENTUMDIFF_TEST_WASM_DIR or enable tier-c-wasm)")]
    fn stage11_finalizer_returns_certified_public_diff_for_python_change() {
        let mut request = stage11_finalizer_request(
            "def answer():\n    return 1\n",
            "def answer():\n    return 2\n",
        );
        let modification_only_changes: Vec<Value> = request["changes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|change| change["change_type"] == "MODIFICATION")
            .cloned()
            .collect();
        request["changes"] = Value::Array(modification_only_changes);

        let payload = rust_finalize_stage11_value(&request).unwrap();

        assert_eq!(payload["status"], COMPLETE);
        assert_eq!(payload["certified"], true);
        assert_eq!(payload["diff"]["metadata"]["engine_owner"], "rust");
        assert_eq!(
            payload["diff"]["metadata"]["semantic_contract"],
            "rust_finalized_v1"
        );
        assert_eq!(payload["diff"]["has_semantic_changes"], true);
        assert!(payload["diff"]["changes"].as_array().unwrap().len() >= 1);
    }
    #[test]
    #[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged parser wasm (set INTENTUMDIFF_TEST_WASM_DIR or enable tier-c-wasm)")]
    fn stage11_finalizer_certifies_compacted_literal_edit_as_one_modification() {
        // Before issue #13 was fixed, `return 1` -> `return 2` reached the finalizer as an
        // uncompacted MODIFICATION + stray literal ADDITION/DELETION, and the modification-only
        // gate correctly refused to certify the noisy output (this test asserted FALLBACK).
        // suppress_add_delete_drafts_covered_by_pairings now compacts the drafts, so the same
        // input certifies as exactly ONE literal modification.
        let request = stage11_finalizer_request(
            "def answer():\n    return 1\n",
            "def answer():\n    return 2\n",
        );

        let payload = rust_finalize_stage11_value(&request).unwrap();

        assert_eq!(payload["status"], COMPLETE);
        assert_eq!(payload["certified"], true);
        let changes = payload["diff"]["changes"].as_array().unwrap();
        assert_eq!(changes.len(), 1, "one edit reported once: {changes:?}");
        assert_eq!(changes[0]["change_type"], "MODIFICATION");
    }
    #[test]
    #[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged parser wasm (set INTENTUMDIFF_TEST_WASM_DIR or enable tier-c-wasm)")]
    fn stage11_finalizer_still_falls_back_for_non_modification_output() {
        // The modification-only certification gate must stay reachable: a genuine ADDITION
        // (a brand-new function) is not certified by finalizer wave 1 and falls back.
        let request = stage11_finalizer_request(
            "def answer():\n    return 1\n",
            "def answer():\n    return 1\n\ndef extra():\n    return 3\n",
        );

        let payload = rust_finalize_stage11_value(&request).unwrap();

        assert_eq!(payload["status"], FALLBACK);
        assert_eq!(payload["certified"], false);
        assert!(payload["reason"]
            .as_str()
            .unwrap()
            .contains("modification-only"));
    }
    #[test]
    #[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged parser wasm (set INTENTUMDIFF_TEST_WASM_DIR or enable tier-c-wasm)")]
    fn stage11_finalizer_rejects_unknown_change_node_id() {
        let mut request = stage11_finalizer_request(
            "def answer():\n    return 1\n",
            "def answer():\n    return 2\n",
        );
        request["changes"][0]["old_node"]["id"] = json!("missing-node");

        let err = rust_finalize_stage11_value(&request).unwrap_err();

        assert!(err.contains("unknown node id"));
    }
    #[test]
    #[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged parser wasm (set INTENTUMDIFF_TEST_WASM_DIR or enable tier-c-wasm)")]
    fn stage11_finalizer_rejects_oversized_sources() {
        let mut request = stage11_finalizer_request(
            "def answer():\n    return 1\n",
            "def answer():\n    return 2\n",
        );
        request["config"] = json!({"max_cst_bytes": 1});

        let err = rust_finalize_stage11_value(&request).unwrap_err();

        assert!(err.contains("old source is"));
    }
    #[test]
    #[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged parser wasm (set INTENTUMDIFF_TEST_WASM_DIR or enable tier-c-wasm)")]
    fn batch_commit_json_returns_certified_commit_bytes_for_complete_native_batch() {
        let request = json!({
            "schema_version": 1,
            "old_ref": "HEAD",
            "new_ref": "",
            "python_parser_backend": "native",
            "config": {},
            "files": [{
                "old_source": "def total(items):\n    return sum(items)\n",
                "new_source": "def total(items):\n    return sum(item for item in items)\n",
                "old_filename": "example.py",
                "new_filename": "example.py",
                "language": "python",
                "parser_plugin_id": "python",
                "parser_wasm_path": python_wasm_path(),
                "staging_status": "unstaged"
            }]
        });
        let (control_json, commit_json) = diff_batch_commit_json(&request.to_string()).unwrap();
        let control: Value = serde_json::from_str(&control_json).unwrap();
        let commit: Value = serde_json::from_slice(&commit_json.unwrap()).unwrap();

        assert_eq!(control["status"], COMPLETE);
        assert_eq!(control["certification"], PYTHON_NATIVE_V4KB_CERTIFICATION);
        assert!(control["semantic_signature_hash"].as_str().unwrap().len() >= 32);
        assert_eq!(commit["old_ref"], "HEAD");
        assert_eq!(commit["file_diffs"][0]["staging_status"], "unstaged");
        assert_eq!(
            commit["file_diffs"][0]["metadata"]["rust_core"]["details"]["certification"],
            PYTHON_NATIVE_V4KB_CERTIFICATION
        );
        assert!(control["phase_timings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|phase| phase["name"] == "rust_commit_json_output_validation"));
        assert!(control["phase_timings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|phase| phase["name"] == "rust_commit_json_response_assembly"));
        assert!(control["phase_timings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|phase| phase["name"] == "rust_commit_json_serialize"));
        assert!(control["batch_metadata"]["phase_timings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|phase| phase["name"] == "rust_batch_file_execution"));
        assert_eq!(control["batch_metadata"]["file_timing"]["count"], 1);
    }
    #[test]
    fn certified_diff_validation_rejects_bad_change_type_and_position() {
        let mut diff = semantic_diff_payload(
            "old.py",
            "new.py",
            vec![json!({
                "change_type": "NOPE",
                "new_node": {
                    "id": "0.1",
                    "node_type": "identifier",
                    "label": "x",
                    "position": {
                        "start_line": 2,
                        "start_col": 0,
                        "end_line": 1,
                        "end_col": 0
                    },
                    "children": []
                },
            })],
            true,
            COMPLETE,
            json!({
                "certification": PYTHON_NATIVE_V4KB_CERTIFICATION,
                "trust_tier": "first_party_core_builder",
            }),
        );
        diff["metadata"]["rust_core"]["details"]["certification"] =
            json!(PYTHON_NATIVE_V4KB_CERTIFICATION);
        diff["metadata"]["rust_core"]["details"]["trust_tier"] = json!("first_party_core_builder");

        let error = validate_certified_semantic_diff(&diff).unwrap_err();
        assert!(error.contains("unsupported change_type"));

        diff["changes"][0]["change_type"] = json!("ADDITION");
        let error = validate_certified_semantic_diff(&diff).unwrap_err();
        assert!(error.contains("end must be >= start"));
    }
    #[test]
    #[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged parser wasm (set INTENTUMDIFF_TEST_WASM_DIR or enable tier-c-wasm)")]
    fn batch_candidate_matches_working_tree_small_python_signature() {
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
        let request = json!({
            "schema_version": 1,
            "candidate": true,
            "config": {"plugin_fuel": 40000000},
            "files": [{
                "old_source": old_source,
                "new_source": new_source,
                "old_filename": "billing.py",
                "new_filename": "billing.py",
                "language": "python",
                "parser_wasm_path": python_wasm_path()
            }]
        });
        let out = diff_batch(&request.to_string()).unwrap();
        let payload: Value = serde_json::from_str(&out).unwrap();
        let signature = &payload["diffs"][0]["candidate_signature"];
        let expected = json!([
            {
                "old_filename": "billing.py",
                "new_filename": "billing.py",
                "change_type": "MOVE",
                "old": {
                    "id": "0.0.2.0",
                    "node_type": "function_definition",
                    "label": "discount",
                    "position": {
                        "start_line": 1,
                        "start_col": 4,
                        "end_line": 4,
                        "end_col": 16
                    }
                },
                "new": {
                    "id": "0.0",
                    "node_type": "function_definition",
                    "label": "discount",
                    "position": {
                        "start_line": 0,
                        "start_col": 0,
                        "end_line": 5,
                        "end_col": 12
                    }
                },
                "refactoring_kind": "",
                "description": "Move function_definition('discount') -> function_definition('discount')"
            },
            {
                "old_filename": "billing.py",
                "new_filename": "billing.py",
                "change_type": "MODIFICATION",
                "old": {
                    "id": "0.0.2.0.2.0.0.1",
                    "node_type": "integer",
                    "label": "100",
                    "position": {
                        "start_line": 2,
                        "start_col": 22,
                        "end_line": 2,
                        "end_col": 25
                    }
                },
                "new": {
                    "id": "0.0.2.1.0.1",
                    "node_type": "integer",
                    "label": "150",
                    "position": {
                        "start_line": 3,
                        "start_col": 18,
                        "end_line": 3,
                        "end_col": 21
                    }
                },
                "refactoring_kind": "",
                "description": "Update integer('100') -> integer('150')"
            },
            {
                // The call site gained a new argument (`order.get("vip", False)`). Before the
                // label-match parent-anchoring fix (issue #31) this real addition was hidden
                // by a bogus cross-scope label match; the signature now reports it.
                "old_filename": "billing.py",
                "new_filename": "billing.py",
                "change_type": "ADDITION",
                "old": null,
                "new": {
                    "id": "0.1.2.1.0.1.1.1",
                    "node_type": "call",
                    "label": "call",
                    "position": {
                        "start_line": 10,
                        "start_col": 41,
                        "end_line": 10,
                        "end_col": 64
                    }
                },
                "refactoring_kind": "",
                "description": "Insert -> call('call')"
            }
        ]);

        assert_eq!(signature, &expected);
    }
    #[test]
    #[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged parser wasm (set INTENTUMDIFF_TEST_WASM_DIR or enable tier-c-wasm)")]
    fn batch_candidate_matches_rename_move_heavy_python_signature() {
        let old_billing = concat!(
            "def calculate_discount(customer, total):\n",
            "    if customer == 'vip':\n",
            "        return total * 0.2\n",
            "    return 0\n",
            "\n",
            "def invoice_total(customer, subtotal):\n",
            "    return subtotal - calculate_discount(customer, subtotal)\n",
        );
        let new_billing = concat!(
            "from src.pricing import calculate_discount_for_customer\n",
            "\n",
            "def invoice_total(customer, subtotal):\n",
            "    return subtotal - calculate_discount_for_customer(customer, subtotal)\n",
        );
        let old_pricing = "def base_price(amount):\n    return amount\n";
        let new_pricing = concat!(
            "def base_price(amount):\n",
            "    return amount\n",
            "\n",
            "def calculate_discount_for_customer(customer, total):\n",
            "    if customer == 'vip':\n",
            "        return total * 0.25\n",
            "    return 0\n",
        );
        let wasm_path = python_wasm_path();
        let request = json!({
            "schema_version": 1,
            "candidate": true,
            "config": {"plugin_fuel": 40000000},
            "files": [
                {
                    "old_source": old_billing,
                    "new_source": new_billing,
                    "old_filename": "src/billing.py",
                    "new_filename": "src/billing.py",
                    "language": "python",
                    "parser_wasm_path": wasm_path
                },
                {
                    "old_source": old_pricing,
                    "new_source": new_pricing,
                    "old_filename": "src/pricing.py",
                    "new_filename": "src/pricing.py",
                    "language": "python",
                    "parser_wasm_path": wasm_path
                }
            ]
        });
        let out = diff_batch(&request.to_string()).unwrap();
        let payload: Value = serde_json::from_str(&out).unwrap();
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
        let expected = json!([
            {
                "old_filename": "src/billing.py",
                "new_filename": "src/billing.py",
                "change_type": "DELETION",
                "old": {
                    "id": "0.0",
                    "node_type": "function_definition",
                    "label": "calculate_discount",
                    "position": {
                        "start_line": 0,
                        "start_col": 0,
                        "end_line": 3,
                        "end_col": 12
                    }
                },
                "new": null,
                "refactoring_kind": "",
                "description": "Delete function_definition('calculate_discount')"
            },
            {
                "old_filename": "src/billing.py",
                "new_filename": "src/billing.py",
                "change_type": "ADDITION",
                "old": null,
                "new": {
                    "id": "0.0",
                    "node_type": "import_from_statement",
                    "label": "import_from_statement",
                    "position": {
                        "start_line": 0,
                        "start_col": 0,
                        "end_line": 0,
                        "end_col": 55
                    }
                },
                "refactoring_kind": "",
                "description": "Insert -> import_from_statement('import_from_statement')"
            },
            {
                "old_filename": "src/billing.py",
                "new_filename": "src/billing.py",
                "change_type": "REFACTORING",
                "old": {
                    "id": "0.0.1.0",
                    "node_type": "identifier",
                    "label": "customer",
                    "position": {
                        "start_line": 0,
                        "start_col": 23,
                        "end_line": 0,
                        "end_col": 31
                    }
                },
                "new": {
                    "id": "0.0.1.0",
                    "node_type": "identifier",
                    "label": "calculate_discount_for_customer",
                    "position": {
                        "start_line": 0,
                        "start_col": 24,
                        "end_line": 0,
                        "end_col": 55
                    }
                },
                "refactoring_kind": "RENAME_VARIABLE",
                "description": "Rename variable 'customer' -> 'calculate_discount_for_customer'"
            },
            {
                "old_filename": "src/billing.py",
                "new_filename": "src/billing.py",
                "change_type": "REFACTORING",
                "old": {
                    "id": "0.1.2.0.0.1.0",
                    "node_type": "identifier",
                    "label": "calculate_discount",
                    "position": {
                        "start_line": 6,
                        "start_col": 22,
                        "end_line": 6,
                        "end_col": 40
                    }
                },
                "new": {
                    "id": "0.1.2.0.0.1.0",
                    "node_type": "identifier",
                    "label": "calculate_discount_for_customer",
                    "position": {
                        "start_line": 3,
                        "start_col": 22,
                        "end_line": 3,
                        "end_col": 53
                    }
                },
                "refactoring_kind": "RENAME_VARIABLE",
                "description": "Rename variable 'calculate_discount' -> 'calculate_discount_for_customer'"
            },
            {
                "old_filename": "src/pricing.py",
                "new_filename": "src/pricing.py",
                "change_type": "ADDITION",
                "old": null,
                "new": {
                    "id": "0.1",
                    "node_type": "function_definition",
                    "label": "calculate_discount_for_customer",
                    "position": {
                        "start_line": 3,
                        "start_col": 0,
                        "end_line": 6,
                        "end_col": 12
                    }
                },
                "refactoring_kind": "",
                "description": "Insert -> function_definition('calculate_discount_for_customer')"
            }
        ]);

        assert_eq!(Value::Array(signature), expected);
    }
    #[test]
    #[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged parser wasm (set INTENTUMDIFF_TEST_WASM_DIR or enable tier-c-wasm)")]
    fn batch_candidate_large_python_signature_promotes_renamed_functions() {
        let wasm_path = python_wasm_path();
        let mut files = Vec::new();
        for file_index in 0..30 {
            let (old_source, new_source) = large_python_sources(file_index, 12);
            files.push(json!({
                "old_source": old_source,
                "new_source": new_source,
                "old_filename": format!("src/module_{file_index}.py"),
                "new_filename": format!("src/module_{file_index}.py"),
                "language": "python",
                "parser_wasm_path": wasm_path
            }));
        }
        let request = json!({
            "schema_version": 1,
            "candidate": true,
            "config": {"plugin_fuel": 40000000},
            "files": files
        });
        let out = diff_batch(&request.to_string()).unwrap();
        let payload: Value = serde_json::from_str(&out).unwrap();
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

        assert_eq!(payload["status"], CANDIDATE);
        // 12 entries per file: 12 body modifications, with each of the 3 renamed functions
        // collapsing to ONE REFACTORING rename (issue #10 — previously MOVE + a redundant
        // identifier MODIFICATION, i.e. 15 entries per file / 450 total).
        assert_eq!(signature.len(), 360);
        assert!(signature.iter().any(|item| {
            item == &json!({
                "old_filename": "src/module_0.py",
                "new_filename": "src/module_0.py",
                "change_type": "REFACTORING",
                "old": {
                    "id": "0.0",
                    "node_type": "function_definition",
                    "label": "calculate_0_0",
                    "position": {
                        "start_line": 0,
                        "start_col": 0,
                        "end_line": 2,
                        "end_col": 16
                    }
                },
                "new": {
                    "id": "0.0",
                    "node_type": "function_definition",
                    "label": "calculate_0_0_checked",
                    "position": {
                        "start_line": 0,
                        "start_col": 0,
                        "end_line": 4,
                        "end_col": 16
                    }
                },
                "refactoring_kind": "RENAME_SYMBOL",
                "description": "Rename function_definition('calculate_0_0') -> ('calculate_0_0_checked')"
            })
        }));
        // The rename is ONE change: no MOVE for the renamed function (id 0.0) and no redundant
        // name-identifier MODIFICATION riding alongside it. (Unrenamed siblings still surface
        // as same-label line-shift MOVEs — positional-shift accuracy is tracked separately.)
        assert!(!signature.iter().any(|item| {
            let old_node = item.get("old").unwrap_or(&Value::Null);
            item.get("old_filename").and_then(Value::as_str) == Some("src/module_0.py")
                && item.get("change_type").and_then(Value::as_str) == Some("MOVE")
                && old_node.get("id").and_then(Value::as_str) == Some("0.0")
        }));
        assert!(!signature.iter().any(|item| {
            let old_node = item.get("old").unwrap_or(&Value::Null);
            item.get("old_filename").and_then(Value::as_str) == Some("src/module_0.py")
                && old_node.get("id").and_then(Value::as_str) == Some("0.0.0")
        }));
        assert!(!signature.iter().any(|item| {
            let old_node = item.get("old").unwrap_or(&Value::Null);
            let new_node = item.get("new").unwrap_or(&Value::Null);
            item.get("old_filename").and_then(Value::as_str) == Some("src/module_0.py")
                && matches!(
                    item.get("change_type").and_then(Value::as_str),
                    Some("DELETION") | Some("ADDITION")
                )
                && (old_node.get("id").and_then(Value::as_str) == Some("0.0")
                    || new_node.get("id").and_then(Value::as_str) == Some("0.0"))
        }));
    }
    #[test]
    #[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged parser wasm (set INTENTUMDIFF_TEST_WASM_DIR or enable tier-c-wasm)")]
    fn native_candidate_large_python_signature_matches_wasm_candidate() {
        let wasm_path = python_wasm_path();
        let mut files = Vec::new();
        for file_index in 0..30 {
            let (old_source, new_source) = large_python_sources(file_index, 12);
            files.push(json!({
                "old_source": old_source,
                "new_source": new_source,
                "old_filename": format!("src/module_{file_index}.py"),
                "new_filename": format!("src/module_{file_index}.py"),
                "language": "python",
                "parser_wasm_path": wasm_path
            }));
        }
        let wasm_request = json!({
            "schema_version": 1,
            "candidate": true,
            "config": {"plugin_fuel": 40000000},
            "files": files.clone()
        });
        let native_request = json!({
            "schema_version": 1,
            "candidate": true,
            "python_parser_backend": "native",
            "config": {"plugin_fuel": 40000000},
            "files": files
        });
        let wasm: Value =
            serde_json::from_str(&diff_batch(&wasm_request.to_string()).unwrap()).unwrap();
        let native: Value =
            serde_json::from_str(&diff_batch(&native_request.to_string()).unwrap()).unwrap();
        let native_signature = candidate_signature_from_payload(&native);

        // 360 = 12 entries × 30 files; renames collapse to one REFACTORING each (issue #10).
        assert_eq!(native_signature.len(), 360);
        assert_eq!(native_signature, candidate_signature_from_payload(&wasm));
    }
    #[test]
    fn haskell_signature_addition_folds_into_function_addition() {
        // Oracle (issue #57 haskell flip): a haskell routine added as a whole surfaces BOTH a
        // `signature` and a `function` ADDITION sharing the label. The sibling `signature`
        // change is scaffold — fold it into the `function` addition, keeping the review compact.
        let add_fn = node("1", "function", "multiply", Vec::new());
        let add_sig = node("2", "signature", "multiply", vec![node("2.0", "variable", "multiply", Vec::new())]);
        // A signature whose function is NOT (also) added must survive.
        let lone_sig = node("3", "signature", "orphan", Vec::new());
        let add_fn: &'static SemanticNode = Box::leak(Box::new(add_fn));
        let add_sig: &'static SemanticNode = Box::leak(Box::new(add_sig));
        let lone_sig: &'static SemanticNode = Box::leak(Box::new(lone_sig));
        let addition = |n: &'static SemanticNode| ChangeDraft {
            change_type: "ADDITION",
            old_node: None,
            new_node: Some(n),
            old_index: None,
            new_index: None,
            confidence: 1.0,
            description: String::new(),
            refactoring_kind: None,
            text_diff: None,
        };
        let mut changes = vec![addition(add_fn), addition(add_sig), addition(lone_sig)];

        let group = suppress_haskell_signature_function_sibling_churn_drafts(&mut changes)
            .expect("companion signature suppressed");
        assert_eq!(group["kind"], "NOISE_SUPPRESSED");
        assert_eq!(
            group["rule_id"],
            "presentation.haskell.suppress_signature_function_sibling_churn"
        );
        // The multiply signature is gone; the function and the orphan signature remain.
        let kinds: Vec<(&str, &str)> = changes
            .iter()
            .map(|c| {
                (
                    c.new_node.unwrap().node_type.as_str(),
                    c.new_node.unwrap().label.as_str(),
                )
            })
            .collect();
        assert!(kinds.contains(&("function", "multiply")));
        assert!(kinds.contains(&("signature", "orphan")));
        assert!(!kinds.contains(&("signature", "multiply")));

        // No function additions -> nothing to fold, no group.
        let mut orphan_only = vec![addition(lone_sig)];
        assert!(suppress_haskell_signature_function_sibling_churn_drafts(&mut orphan_only).is_none());
    }
    #[test]
    fn dart_body_edits_survive_after_the_signature_body_merge() {
        // The old oracle ("a routine add leaks a sibling function_body ADDITION — drop it")
        // died with the dart parser's signature+body MERGE (#46/#72): whole-routine adds now
        // travel as one function_definition wrapper, so a bare body ADDITION is a REAL body
        // edit (the trivial-body matrix's dart case) and must SURVIVE this pass.
        let add_sig = node("1", "function_signature", "multiply", Vec::new());
        let add_body = node("2", "function_body", "function_body", Vec::new());
        let add_sig: &'static SemanticNode = Box::leak(Box::new(add_sig));
        let add_body: &'static SemanticNode = Box::leak(Box::new(add_body));
        let addition = |n: &'static SemanticNode| ChangeDraft {
            change_type: "ADDITION",
            old_node: None,
            new_node: Some(n),
            old_index: None,
            new_index: None,
            confidence: 1.0,
            description: String::new(),
            refactoring_kind: None,
            text_diff: None,
        };
        let mut changes = vec![addition(add_sig), addition(add_body)];
        let group = suppress_dart_signature_body_scaffold_churn_drafts(&mut changes);
        assert!(group.is_none(), "no scaffold suppression under the merged shape");
        let kept: Vec<&str> = changes
            .iter()
            .map(|c| c.new_node.unwrap().node_type.as_str())
            .collect();
        assert_eq!(kept, vec!["function_signature", "function_body"]);
    }
    #[test]
    fn added_annotation_promotes_method_to_change_signature() {
        // Oracle (issue #57 java pilot): adding @Override to a matched method is a
        // CHANGE_SIGNATURE refactoring anchored at the method — the raw modifiers /
        // marker_annotation child drafts must not surface, but a body edit (string
        // literal) survives.
        // Distinct hashes: the promotion's fast-path requires the method subtree hashes to
        // differ (they always do in real trees once a child is added). The synthetic
        // `node()` helper derives its hash from type+label only, so set them explicitly.
        let with_hash = |mut n: SemanticNode, hash: &str| {
            n.structural_hash = hash.to_string();
            n
        };
        let old_body = node("1.b", "block", "block", vec![
            node("1.b.0", "string_literal", "\"old\"", Vec::new()),
        ]);
        let old_method = with_hash(
            node(
                "1",
                "method_declaration",
                "name",
                vec![
                    node("1.m", "modifiers", "modifiers", vec![
                        node("1.m.0", "modifier", "public", Vec::new()),
                    ]),
                    old_body,
                ],
            ),
            "method-old",
        );
        let new_body = node("2.b", "block", "block", vec![
            node("2.b.0", "string_literal", "\"new\"", Vec::new()),
        ]);
        let new_method = with_hash(
            node(
                "2",
                "method_declaration",
                "name",
                vec![
                    node("2.m", "modifiers", "modifiers", vec![
                        node("2.m.0", "modifier", "public", Vec::new()),
                        node("2.m.1", "marker_annotation", "Override", Vec::new()),
                    ]),
                    new_body,
                ],
            ),
            "method-new",
        );
        let old_tree = module_with_nodes(vec![old_method]);
        let new_tree = module_with_nodes(vec![new_method]);

        let mut changes = vec![
            ChangeDraft {
                change_type: "MODIFICATION",
                old_node: Some(&old_tree.children[0].children[0]),
                new_node: Some(&new_tree.children[0].children[0]),
                old_index: Some(0),
                new_index: Some(0),
                confidence: 1.0,
                description: String::new(),
                refactoring_kind: None,
                text_diff: None,
            },
            addition_draft(&new_tree.children[0].children[0].children[1]),
            ChangeDraft {
                change_type: "MODIFICATION",
                old_node: Some(&old_tree.children[0].children[1].children[0]),
                new_node: Some(&new_tree.children[0].children[1].children[0]),
                old_index: Some(1),
                new_index: Some(1),
                confidence: 1.0,
                description: String::new(),
                refactoring_kind: None,
                text_diff: None,
            },
        ];
        promote_signature_changes_from_annotations_drafts(&mut changes, &old_tree, &new_tree);

        let refactoring = changes.iter().find(|c| c.change_type == "REFACTORING");
        assert!(
            refactoring.is_some_and(|c| c.refactoring_kind == Some("CHANGE_SIGNATURE")
                && c.new_node.is_some_and(|n| n.node_type == "method_declaration")),
            "expected a method-level CHANGE_SIGNATURE"
        );
        assert!(
            !changes
                .iter()
                .any(|c| c.new_node.is_some_and(|n| n.node_type == "marker_annotation")),
            "the raw annotation addition must be suppressed"
        );
        assert!(
            changes
                .iter()
                .any(|c| c.new_node.is_some_and(|n| n.node_type == "string_literal")),
            "the body literal edit must survive"
        );
    }
    #[test]
    fn finalize_review_empty_root_is_lifecycle_delete_add_pair() {
        // Issue #57 payoff (empty-tree tier): an empty root on one side is a file
        // add/delete. The matcher would pair the roots structurally and emit a bogus
        // root MODIFICATION; the contract shape is DELETION(old root) + ADDITION(new
        // root), python-parity, interpreted downstream via file_lifecycle metadata.
        let empty = module_with_nodes(Vec::new());
        let full = module_with_nodes(vec![node(
            "0.0",
            "function_declaration",
            "hello",
            Vec::new(),
        )]);
        let payload = finalize_review_json(
            &serde_json::to_string(&empty).unwrap(),
            &serde_json::to_string(&full).unwrap(),
            "",
            "func hello() {}",
            "go",
            "{}",
        )
        .unwrap();
        let data: Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(data["used"], true);
        let kinds: Vec<&str> = data["changes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["change_type"].as_str().unwrap())
            .collect();
        assert_eq!(kinds, vec!["DELETION", "ADDITION"], "lifecycle pair: {kinds:?}");
        assert_eq!(data["is_style_only"], false);
    }
