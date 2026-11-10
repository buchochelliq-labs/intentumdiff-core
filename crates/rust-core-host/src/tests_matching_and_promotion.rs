// Split from tests_inline.rs (issue #85): one file per test family.
// Nested inside cfg(test) mod tests - `super::*` = the tests mod (helpers),
// `crate::*` = the engine.
#![allow(unused_imports)]
use super::*;
use crate::*;

#[test]
fn callable_renames_accept_actual_parser_shapes_without_parameter_children() {
    // Captured from the published Wasm components listed in the fixture README.
    let cases: Value = serde_json::from_str(include_str!("../tests/fixtures/callable_parser_shapes.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let result: Value = serde_json::from_str(&finalize_review_impl(
            &case["old_tree"].to_string(), &case["new_tree"].to_string(),
            case["old_source"].as_str().unwrap(), case["new_source"].as_str().unwrap(),
            case["language"].as_str().unwrap(), "{}",
        ).unwrap()).unwrap();
        assert!(result["changes"].as_array().unwrap().iter().any(|c|
            c["refactoring_kind"] == "RENAME_SYMBOL"
            && c["old_node"]["label"] == case["old_name"]
            && c["new_node"]["label"] == case["new_name"]), "{}: {result}", case["language"]);
        if case["language"] == "vue" {
            let old: SemanticNode = serde_json::from_value(case["old_tree"].clone()).unwrap();
            let new: SemanticNode = serde_json::from_value(case["new_tree"].clone()).unwrap();
            let changed = format!("{}\nother();", case["new_source"].as_str().unwrap());
            assert!(!callable_renames::exact_source_rename(&old.children[0], &new.children[0],
                Some((case["old_source"].as_str().unwrap(), &changed))));
        }
    }
}

fn rename_review_routes(old: &str, new: &str) -> Vec<(&'static str, Value)> {
    let request = json!({
        "schema_version": 1, "python_parser_backend": "native",
        "files": [{"old_source": old, "new_source": new,
            "old_filename": "example.py", "new_filename": "example.py",
            "language": "python", "parser_wasm_path": "unused.wasm"}]
    });
    let batch: Value = serde_json::from_str(&diff_batch(&request.to_string()).unwrap()).unwrap();
    assert_eq!(batch["status"], COMPLETE, "{batch}");
    fn tree(source: &str) -> String {
        let cst: CstNode = serde_json::from_str(&python_wasm_filtered_cst(source).unwrap()).unwrap();
        serde_json::to_string(&convert_cst(&cst, "0", None).unwrap()).unwrap()
    }
    let routed: Value = serde_json::from_str(&finalize_review_impl(
        &tree(old), &tree(new), old, new, "python", "{}"
    ).unwrap()).unwrap();
    vec![("batch", batch["diffs"][0]["diff"].clone()), ("routed", routed)]
}

fn assert_function_rename(result: &Value, old: &str, new: &str) {
    let changes = result["changes"].as_array().unwrap();
    assert!(changes.iter().any(|c| c["refactoring_kind"] == "RENAME_SYMBOL"
        && c["old_node"]["label"] == old && c["new_node"]["label"] == new
        && c["old_node"]["node_type"] == "function_definition"), "{result}");
}

#[test]
fn python_global_read_survives_parameter_rename_in_another_function() {
    let old = "def f(a):\n    return a\ndef g():\n    return a\n";
    let new = "def f(b):\n    return b\ndef g():\n    return b\n";
    for (route, result) in rename_review_routes(old, new) {
        let changes = result["changes"].as_array().unwrap();
        assert!(changes.iter().any(|change| {
            change["change_type"] == "MODIFICATION"
                && change["old_node"]["position"]["start_line"] == 3
        }), "{route}: unrelated global read in g must remain a semantic edit: {result}");
    }
}

#[test]
fn same_id_variable_binder_rename_preserves_unrelated_read_of_the_same_name() {
    fn node(id: &str, kind: &str, label: &str, children: Vec<SemanticNode>) -> SemanticNode {
        SemanticNode {
            id: id.to_owned(),
            node_type: kind.to_owned(),
            label: label.to_owned(),
            position: NodePosition { start_line: 0, start_col: 0, end_line: 0, end_col: 1 },
            structural_hash: id.to_owned(),
            children,
            parent_type: None,
            type_info: None,
            facts: None,
        }
    }
    let old_binder = node("0.0.0", "identifier", "hasCloseElements", vec![]);
    let new_binder = node("0.0.0", "identifier", "nearbyElements", vec![]);
    let old_initializer = node("0.0.1", "identifier", "sourceName", vec![]);
    let new_initializer = node("0.0.1", "identifier", "renamedSource", vec![]);
    let old_read = node("0.1.1", "identifier", "hasCloseElements", vec![]);
    let new_read = node("0.1.1", "identifier", "nearbyElements", vec![]);
    let old_tree = node("0", "module", "module", vec![
        node("0.0", "variable_declarator", "declarator", vec![old_binder.clone(), old_initializer.clone()]),
        node("0.1", "function_definition", "g", vec![old_read.clone()]),
    ]);
    let new_tree = node("0", "module", "module", vec![
        node("0.0", "variable_declarator", "declarator", vec![new_binder.clone(), new_initializer.clone()]),
        node("0.1", "function_definition", "g", vec![new_read.clone()]),
    ]);
    let mut changes = vec![
        ChangeDraft { change_type: "DELETION", old_node: Some(&old_binder), new_node: None,
            old_index: None, new_index: None, confidence: 1.0, description: String::new(),
            refactoring_kind: None, text_diff: None },
        ChangeDraft { change_type: "ADDITION", old_node: None, new_node: Some(&new_binder),
            old_index: None, new_index: None, confidence: 1.0, description: String::new(),
            refactoring_kind: None, text_diff: None },
        ChangeDraft { change_type: "DELETION", old_node: Some(&old_initializer), new_node: None,
            old_index: None, new_index: None, confidence: 1.0, description: String::new(),
            refactoring_kind: None, text_diff: None },
        ChangeDraft { change_type: "ADDITION", old_node: None, new_node: Some(&new_initializer),
            old_index: None, new_index: None, confidence: 1.0, description: String::new(),
            refactoring_kind: None, text_diff: None },
        ChangeDraft { change_type: "DELETION", old_node: Some(&old_read), new_node: None,
            old_index: None, new_index: None, confidence: 1.0, description: String::new(),
            refactoring_kind: None, text_diff: None },
        ChangeDraft { change_type: "ADDITION", old_node: None, new_node: Some(&new_read),
            old_index: None, new_index: None, confidence: 1.0, description: String::new(),
            refactoring_kind: None, text_diff: None },
    ];

    promote_same_id_identifier_renames_from_add_delete_drafts(&mut changes, &old_tree, &new_tree);

    assert_eq!(changes.len(), 5, "only the actual binder pair is promoted");
    assert!(changes.iter().any(|change| change.change_type == "REFACTORING"
        && change.refactoring_kind == Some("RENAME_VARIABLE")
        && change.old_node.is_some_and(|node| node.id == "0.0.0")));
    assert!(changes.iter().any(|change| change.change_type == "DELETION"
        && change.old_node.is_some_and(|node| node.id == "0.1.1")));
    assert!(changes.iter().any(|change| change.change_type == "ADDITION"
        && change.new_node.is_some_and(|node| node.id == "0.1.1")));
    assert!(changes.iter().any(|change| change.change_type == "DELETION"
        && change.old_node.is_some_and(|node| node.id == "0.0.1")));
    assert!(changes.iter().any(|change| change.change_type == "ADDITION"
        && change.new_node.is_some_and(|node| node.id == "0.0.1")));
}

#[test]
fn rename_prefers_unique_exact_body_over_position() {
    for (old, new) in [
        ("def a(x): return x + 1\ndef b(x): return x + 2\n", "def c(x): return x + 2\n"),
        ("def a(x): return x + 1\ndef b(x): return x * 1\n", "def c(x): return x * 1\n"),
    ] {
      for (_, result) in rename_review_routes(old, new) {
        assert_function_rename(&result, "b", "c");
        assert!(result["changes"].as_array().unwrap().iter().any(|c|
            c["change_type"] == "DELETION" && c["old_node"]["label"] == "a"), "{result}");
      }
    }
}

#[test]
fn decorator_order_is_behavior_with_or_without_rename() {
    for name in ["calc", "compute"] {
        let old = "@cache\n@authorize\ndef calc(x): return x + 1\n";
        let new = format!("@authorize\n@cache\ndef {name}(x): return x + 1\n");
        for (_, result) in rename_review_routes(old, &new) {
            assert!(result["changes"].as_array().unwrap().iter().any(|c|
                c["change_type"] == "MODIFICATION"
                && c["description"].as_str().unwrap().contains("Reorder decorator")), "{result}");
        }
    }
}

#[test]
fn decorator_insertion_does_not_hide_existing_order_change() {
    for (_, result) in rename_review_routes("@a\n@b\ndef f(x): return x\n",
        "@c\n@b\n@a\ndef f(x): return x\n") {
        assert!(result["changes"].as_array().unwrap().iter().any(|c|
            c["change_type"] == "MODIFICATION"
            && c["description"].as_str().unwrap().contains("Reorder decorator")), "{result}");
        assert!(result["changes"].as_array().unwrap().iter().any(|c| c["change_type"] == "ADDITION"), "{result}");
    }
}

#[test]
fn extraction_requires_exact_expression_arguments_and_context() {
    let old = "def calc(x):\n    return x * 2\n";
    for new in [
        "def helper(x):\n    return x + 2\ndef calc(x):\n    return helper(x)\n",
        "def helper(x):\n    return x * 2\ndef calc(x):\n    return helper(3)\n",
        "def helper(x):\n    log(x)\n    return x * 2\ndef calc(x):\n    return helper(x)\n",
        "@decorate\ndef helper(x):\n    return x * 2\ndef calc(x):\n    return helper(x)\n",
        "def helper(x):\n    return x * 2\ndef calc(x):\n    print(helper(x))\n",
    ] {
        for (_, result) in rename_review_routes(old, new) {
            assert!(!result["changes"].as_array().unwrap().iter().any(|c|
                c["refactoring_kind"] == "EXTRACT_FUNCTION"), "{result}");
        }
    }
    for (_, result) in rename_review_routes(old,
        "def helper(x):\n    return x * 2\ndef calc(x):\n    return helper(x)\n") {
        assert!(result["changes"].as_array().unwrap().iter().any(|c|
            c["refactoring_kind"] == "EXTRACT_FUNCTION"), "{result}");
    }
}

#[test]
fn python_operator_change_is_a_semantic_modification_on_every_route() {
    // Oracle (Core #229): changing only an operator changes behaviour. Both the batch
    // product route and the routed finalize_review route must report it as a semantic
    // change - never `is_style_only`, never an empty change list.
    let cases = [
        ("def f(a, b):\n    return a + b\n", "def f(a, b):\n    return a - b\n"),
        ("def f(a, b):\n    return a < b\n", "def f(a, b):\n    return a > b\n"),
        ("def f(a, b):\n    return a and b\n", "def f(a, b):\n    return a or b\n"),
        ("x = 1\nx += 2\n", "x = 1\nx -= 2\n"),
        // Matched parent gaining an operator leaf: must surface, not be skipped as covered.
        ("def f(a, b):\n    return a is b\n", "def f(a, b):\n    return a is not b\n"),
        // `async` on a comprehension clause is the iteration protocol, not layout.
        ("async def f(xs):\n    return [x async for x in xs]\n",
         "async def f(xs):\n    return [x for x in xs]\n"),
    ];
    for (old, new) in cases {
        for (route, result) in rename_review_routes(old, new) {
            assert_ne!(result["is_style_only"], true, "{route} {old:?}->{new:?}: {result}");
            // `has_semantic_changes` belongs to the final diff (batch route); the routed
            // finalize payload carries changes + groups only.
            if route == "batch" {
                assert_eq!(result["has_semantic_changes"], true, "{route} {old:?}->{new:?}: {result}");
            }
            assert!(
                result["changes"].as_array().unwrap().iter().any(|c| c["change_type"] == "MODIFICATION"),
                "{route} {old:?}->{new:?}: operator change is not a MODIFICATION: {result}"
            );
            assert!(
                !result["changes"].as_array().unwrap().is_empty(),
                "{route} {old:?}->{new:?}: no changes reported: {result}"
            );
            assert!(
                !result["change_groups"].as_array().unwrap().iter().any(|g| g["kind"] == "IGNORED_STYLE"),
                "{route} {old:?}->{new:?}: operator change filed as IGNORED_STYLE: {result}"
            );
        }
    }
}

#[test]
#[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged parser wasm (set INTENTUMDIFF_TEST_WASM_DIR or enable tier-c-wasm)")]
fn order_sensitive_statement_swap_survives_python_and_javascript_finalization() {
    let python_old = "x = 1\nx += 2\nx *= 3\n";
    let python_new = "x = 1\nx *= 3\nx += 2\n";
    for (route, result) in rename_review_routes(python_old, python_new) {
        if route == "batch" {
            assert_eq!(result["has_semantic_changes"], true, "{route}: {result}");
        }
        let change = result["changes"].as_array().unwrap().iter().find(|change|
            change["change_type"] == "MODIFICATION"
                && change["description"].as_str().unwrap().contains("Reorder statement"))
            .unwrap_or_else(|| panic!("{route}: executable order change disappeared: {result}"));
        let old_line = change["old_node"]["position"]["start_line"].as_u64().unwrap() as usize;
        let new_line = change["new_node"]["position"]["start_line"].as_u64().unwrap() as usize;
        assert_ne!(old_line, new_line, "{route}: reorder evidence must point to both locations");
        for (source, line) in [(python_old, old_line), (python_new, new_line)] {
            assert!(matches!(source.lines().nth(line), Some("x += 2" | "x *= 3")),
                "{route}: source anchor must select a reordered statement, line {line}: {change}");
        }
    }

    let javascript_old = "let x = 1;\nx += 2;\nx *= 3;\n";
    let javascript_new = "let x = 1;\nx *= 3;\nx += 2;\n";
    let wasm = bundled_wasm_path("js_ts_parser.wasm");
    let (old_json, new_json) = run_python_wasm_process_pair(
        &wasm, javascript_old, "", "example.js", javascript_new, "", "example.js",
        u64::MAX, 16 * 1024 * 1024, "javascript",
    ).expect("JavaScript parser must produce both semantic trees");
    let old_tree = parse_semantic_tree_json(&old_json).expect("old JavaScript tree");
    let new_tree = parse_semantic_tree_json(&new_json).expect("new JavaScript tree");
    let result: Value = serde_json::from_str(&finalize_review_impl(
        &serde_json::to_string(&old_tree).unwrap(),
        &serde_json::to_string(&new_tree).unwrap(),
        javascript_old, javascript_new, "javascript", "{}",
    ).unwrap()).unwrap();
    let change = result["changes"].as_array().unwrap().iter().find(|change|
        change["change_type"] == "MODIFICATION"
            && change["description"].as_str().unwrap().contains("Reorder statement"))
        .unwrap_or_else(|| panic!("JavaScript executable order change disappeared: {result}"));
    let old_line = change["old_node"]["position"]["start_line"].as_u64().unwrap() as usize;
    let new_line = change["new_node"]["position"]["start_line"].as_u64().unwrap() as usize;
    assert_ne!(old_line, new_line, "JavaScript reorder evidence must point to both locations");
    for (source, line) in [(javascript_old, old_line), (javascript_new, new_line)] {
        assert!(matches!(source.lines().nth(line), Some("x += 2;" | "x *= 3;")),
            "JavaScript source anchor must select a reordered statement, line {line}: {change}");
    }

    let arguments_old = "invoke(alpha, beta);\n";
    let arguments_new = "invoke(beta, alpha);\n";
    let (old_json, new_json) = run_python_wasm_process_pair(
        &wasm, arguments_old, "", "example.js", arguments_new, "", "example.js",
        u64::MAX, 16 * 1024 * 1024, "javascript",
    ).expect("JavaScript parser must produce both argument trees");
    let old_tree = parse_semantic_tree_json(&old_json).expect("old JavaScript argument tree");
    let new_tree = parse_semantic_tree_json(&new_json).expect("new JavaScript argument tree");
    let result: Value = serde_json::from_str(&finalize_review_impl(
        &serde_json::to_string(&old_tree).unwrap(),
        &serde_json::to_string(&new_tree).unwrap(),
        arguments_old, arguments_new, "javascript", "{}",
    ).unwrap()).unwrap();
    let change = result["changes"].as_array().unwrap().iter().find(|change|
        change["change_type"] == "MODIFICATION"
            && change["description"].as_str().unwrap().contains("Reorder argument"))
        .unwrap_or_else(|| panic!("JavaScript ordered argument change disappeared: {result}"));
    assert_ne!(change["old_node"]["position"]["start_col"],
        change["new_node"]["position"]["start_col"], "argument source anchors must move: {change}");

    let initializer_old = "int values[] = {1, 2, 3};\n";
    let initializer_new = "int values[] = {3, 2, 1};\n";
    let cpp_wasm_dir = std::env::var_os("INTENTUMDIFF_TEST_WASM_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../src/intentumdiff/wasm")
        });
    let cpp_wasm = cpp_wasm_dir.join("cpp_parser.wasm");
    if cpp_wasm.is_file() {
        let cpp_wasm = cpp_wasm.to_string_lossy();
        let (old_json, new_json) = run_python_wasm_process_pair(
            &cpp_wasm, initializer_old, "", "example.cpp", initializer_new, "", "example.cpp",
            u64::MAX, 16 * 1024 * 1024, "cpp",
        ).expect("C++ parser must produce both initializer trees");
        let old_tree = parse_semantic_tree_json(&old_json).expect("old C++ initializer tree");
        let new_tree = parse_semantic_tree_json(&new_json).expect("new C++ initializer tree");
        let result: Value = serde_json::from_str(&finalize_review_impl(
            &serde_json::to_string(&old_tree).unwrap(),
            &serde_json::to_string(&new_tree).unwrap(),
            initializer_old, initializer_new, "cpp", "{}",
        ).unwrap()).unwrap();
        assert!(result["changes"].as_array().unwrap().iter().any(|change|
            change["change_type"] == "MODIFICATION"
                && change["description"].as_str().unwrap().contains("Reorder initializer")),
            "C++ ordered initializer change disappeared: {result}");
    } else {
        eprintln!("Skipping C++ initializer route: no cpp_parser.wasm staged in {cpp_wasm_dir:?}");
    }
}

#[test]
fn renamed_function_with_body_edit_preserves_both_intents() {
    for (_, result) in rename_review_routes("def calc(x):\n    return x + 1\n", "def compute(x):\n    return x + 2\n") {
        assert_function_rename(&result, "calc", "compute");
        assert!(result["changes"].as_array().unwrap().iter().any(|c|
            c["change_type"] == "MODIFICATION" && c["old_node"]["label"] == "1"
            && c["new_node"]["label"] == "2"), "{result}");
        assert_ne!(result["is_style_only"], true);
        let literal_index = result["changes"].as_array().unwrap().iter().position(|c|
            c["change_type"] == "MODIFICATION" && c["new_node"]["label"] == "2").unwrap();
        assert!(result["change_groups"].as_array().unwrap().iter().any(|g|
            g["kind"] == "MEANINGFUL_CHANGE"
            && g["raw_change_indices"].as_array().unwrap().contains(&json!(literal_index))), "{result}");

    }
}

#[test]
fn renamed_function_with_inserted_helper_keeps_cap_edit() {
    let old = "import os\nimport sys\n\ndef calculate_total(items):\n    total = 0\n    for i in items:\n        total += i.price * i.qty\n    return min(total, 50)\n";
    let new = "import sys\nimport os\n\ndef _subtotal(i):\n    return i.price * i.qty\n\ndef compute_order_total(items):\n    total = 0\n    for i in items:\n        total += _subtotal(i)\n    return min(total, 75)\n";
    for (route, result) in rename_review_routes(old, new) {
        assert!(result["changes"].as_array().unwrap().iter().all(|c| c["change_type"] != "REORDER" && c["change_type"] != "MOVE"), "{route}: insertion is not relocation: {result}");
        assert_function_rename(&result, "calculate_total", "compute_order_total");
        let changes = result["changes"].as_array().unwrap();
        assert!(changes.iter().any(|c| c["change_type"] == "MODIFICATION"
            && c["old_node"]["label"] == "50" && c["new_node"]["label"] == "75"), "{result}");
        assert!(!changes.iter().any(|c| c["refactoring_kind"] == "RENAME_SYMBOL"
            && c["new_node"]["label"] == "_subtotal"), "{result}");
        assert!(changes.iter().any(|c| c["new_node"]["label"] == "_subtotal"), "helper must remain visible: {result}");
        assert!(changes.iter().any(|c| c["refactoring_kind"] == "EXTRACT_FUNCTION"
            && c["new_node"]["label"] == "_subtotal"), "{result}");
    }
}

#[test]
fn renamed_function_controls_decline_ambiguous_and_unrelated_pairs() {
    for (old, new) in [
        ("def a(): return 1\n", "def b(): return 2\n"),
        ("def a(x): return x + 1\ndef b(x): return x + 1\n", "def c(x): return x + 1\n"),
        ("def old_one(): return 1\ndef keep(): return 0\n",
         "def keep(): return 0\ndef new_one(): return 2\n"),
        ("def alpha(a): return a * 3\n", "def beta(b): return b - 9\n"),
        ("def alpha(x): return x + 1\ndef beta(x): return x + 1\n",
         "def gamma(x): return x + 2\ndef delta(x): return x + 2\n"),
        ("class A:\n    class Inner:\n        def calc(x): return x + 1\nclass B:\n    class Inner:\n        pass\n",
         "class A:\n    class Inner:\n        pass\nclass B:\n    class Inner:\n        def compute(x): return x + 2\n"),
        ("class A:\n    def calc(x): return x + 1\n",
         "class B:\n    def compute(x): return x + 2\n"),
    ] {
        for (_, result) in rename_review_routes(old, new) {
            assert!(!result["changes"].as_array().unwrap().iter().any(|c|
                c["refactoring_kind"] == "RENAME_SYMBOL"), "{result}");
        }
    }
}

#[test]
fn renamed_function_control_preserves_pure_rename_and_existing_identity() {
    for (_, result) in rename_review_routes("def calc(x): return x + 1\n", "def compute(x): return x + 1\n") {
        assert_function_rename(&result, "calc", "compute");
    }
    for (_, result) in rename_review_routes("def calc(x): return x + 1\n",
        "def compute(x): return x + 2\ndef calc(x): return x + 1\n") {
        assert!(!result["changes"].as_array().unwrap().iter().any(|c|
            c["refactoring_kind"] == "RENAME_SYMBOL"), "{result}");
    }
}

#[test]
fn renamed_function_keeps_behavioral_statement_order_changes() {
    for (old, new) in [
        ("def calc(x):\n    x = x + 1\n    x = x * 2\n    return x\n",
         "def compute(x):\n    x = x * 2\n    x = x + 1\n    return x\n"),
        ("def calc(x):\n    first(x)\n    second(x)\n    return x\n",
         "def compute(x):\n    second(x)\n    first(x)\n    return x\n"),
    ] {
        for (_, result) in rename_review_routes(old, new) {
            assert_function_rename(&result, "calc", "compute");
            assert!(result["changes"].as_array().unwrap().iter().any(|c|
                c["change_type"] == "MODIFICATION"
                && c["description"].as_str().unwrap().contains("Reorder statement")), "{result}");
        }
    }
}

    #[test]
    fn cst_diff_reports_update_for_renamed_python_function() {
        let out = diff_python_cst_json(
            &module_with_function("old_name"),
            &module_with_function("new_name"),
            "",
            "",
            "example.py",
            "example.py",
            "python_parser.wasm",
            "{}",
        )
        .unwrap();
        let payload: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(payload["metadata"]["rust_core"]["status"], COMPLETE);
        let changes = payload["changes"].as_array().unwrap();
        assert!(!changes.is_empty());
    }
    #[test]
    fn positional_callable_relabel_without_body_evidence_stays_explicit() {
        // IDs are positions, not identities. Empty synthetic callables provide no
        // evidence for a rename; real-source pure-renames are covered separately.
        let old_fn = node("0.1", "function_definition", "greet", vec![]);
        let new_fn = node("0.1", "function_definition", "welcome", vec![]);
        let mut changes = vec![
            ChangeDraft {
                change_type: "DELETION",
                old_node: Some(&old_fn),
                new_node: None,
                old_index: None,
                new_index: None,
                confidence: 1.0,
                description: "Delete function_definition('greet')".to_owned(),
                refactoring_kind: None,
                text_diff: None,
            },
            ChangeDraft {
                change_type: "ADDITION",
                old_node: None,
                new_node: Some(&new_fn),
                old_index: None,
                new_index: None,
                confidence: 1.0,
                description: "Insert -> function_definition('welcome')".to_owned(),
                refactoring_kind: None,
                text_diff: None,
            },
        ];

        promote_same_id_named_renames_from_add_delete_drafts(&mut changes);

        assert_eq!(changes.len(), 2, "position alone cannot collapse a callable: {changes:?}");
        assert!(changes.iter().all(|c| c.refactoring_kind.is_none()));
    }
    #[test]
    fn markdown_section_swap_is_one_move() {
        let old = "# T\n\n## A\n\nbody a\n\n## B\n\nbody b\n";
        let new = "# T\n\n## B\n\nbody b\n\n## A\n\nbody a\n";
        let payload: Value =
            serde_json::from_str(&markdown_section_review_json(old, new).unwrap()).unwrap();
        let moves = payload["moves"].as_array().unwrap();
        assert_eq!(moves.len(), 1, "LIS keeps one section stationary: {moves:?}");
        assert_eq!(payload["move_group"]["rule_id"], "presentation.markdown_section_move");
        assert!(payload["renames"].as_array().unwrap().is_empty());
    }
    #[test]
    fn markdown_heading_rename_is_one_modification_by_body_identity() {
        let old = "# T\n\n## Old Name\n\nsame body\n";
        let new = "# T\n\n## New Name\n\nsame body\n";
        let payload: Value =
            serde_json::from_str(&markdown_section_review_json(old, new).unwrap()).unwrap();
        assert!(payload["moves"].as_array().unwrap().is_empty());
        let renames = payload["renames"].as_array().unwrap();
        assert_eq!(renames.len(), 1, "{renames:?}");
        assert_eq!(renames[0]["old_node"]["label"], "## Old Name");
        assert_eq!(renames[0]["new_node"]["label"], "## New Name");
        assert_eq!(
            payload["rename_group"]["rule_id"],
            "presentation.markdown_section_heading_rename"
        );
    }
    #[test]
    fn generic_text_reordered_lines_net_to_zero() {
        let old = "line one\nline two\nline three\n";
        let new = "line two\nline one\nline three\n";
        let payload: Value =
            serde_json::from_str(&generic_text_review_json(old, new, 0).unwrap()).unwrap();
        assert_eq!(payload["changes"].as_array().unwrap().len(), 0);
        assert!(payload["group"].is_null(), "no raw churn -> no audit group");
    }
    #[test]
    fn swapped_named_entities_match_by_identity_not_position() {
        // Issue #12/#31: two functions swap order; their subtrees are IDENTICAL (equal
        // structural hashes), so matching must pair greet<->greet / add<->add across
        // positions — never greet<->add by position, which fabricates cross-modifications
        // and cancels the swap into style-only.
        fn fn_node(id_prefix: &str, name: &str) -> SemanticNode {
            let body = node(&format!("{id_prefix}.0"), "block", &format!("body-{name}"), vec![]);
            node(id_prefix, "function_definition", name, vec![body])
        }
        // Realistic hashes: a parent's structural hash covers its children IN ORDER, so the
        // swapped module hashes DIFFER (the hardcoded module_with_nodes hash would wrongly
        // let top-down match the modules as identical subtrees and position-pair descendants).
        fn module(children: Vec<SemanticNode>) -> SemanticNode {
            let order: String = children.iter().map(|c| c.label.as_str()).collect::<Vec<_>>().join("|");
            let mut root = module_with_nodes(children);
            root.structural_hash = format!("module-hash-{order}");
            root
        }
        let old_tree = module(vec![fn_node("0.0", "greet"), fn_node("0.1", "add")]);
        let new_tree = module(vec![fn_node("0.0", "add"), fn_node("0.1", "greet")]);
        let payload = diff_semantic_tree_for_test(&old_tree, &new_tree, "python");
        let changes = payload["changes"].as_array().unwrap();
        let cross_modified: Vec<String> = changes
            .iter()
            .filter(|c| c["change_type"] == "MODIFICATION")
            .filter_map(|c| {
                let old_label = c["old_node"]["label"].as_str()?;
                let new_label = c["new_node"]["label"].as_str()?;
                (old_label != new_label).then(|| format!("{old_label}->{new_label}"))
            })
            .collect();
        assert!(
            cross_modified.is_empty(),
            "swap must not cross-pair different entities: {cross_modified:?}\nmatching: {}",
            payload["matching_pairs"]
        );
        assert!(
            changes
                .iter()
                .any(|c| c["change_type"] == "MOVE" || c["change_type"] == "REORDER"),
            "the swap must surface as moved code, got {changes:?}"
        );
    }
    #[test]
    fn matched_parent_single_add_delete_pair_promotes_to_modification() {
        // Oracle scenario (issue #33): `return p` -> `return os.path.basename(p)` misses the
        // similarity threshold and arrives as DELETE+ADD of the whole return_statement. With
        // the statements' parents matched, the unique same-type pair is one edited statement.
        let old_parent = node("0.0.2", "block", "block", vec![]);
        let new_parent = node("0.1.2", "block", "block", vec![]);
        let old_stmt = node("0.0.2.0", "return_statement", "p", vec![]);
        let new_stmt = node("0.1.2.0", "return_statement", "os.path.basename(p)", vec![]);
        let matching = vec![MatchPair {
            old_node: &old_parent,
            new_node: &new_parent,
        }];
        let mut changes = vec![
            ChangeDraft {
                change_type: "DELETION",
                old_node: Some(&old_stmt),
                new_node: None,
                old_index: None,
                new_index: None,
                confidence: 1.0,
                description: "Delete return_statement('p')".to_owned(),
                refactoring_kind: None,
                text_diff: None,
            },
            ChangeDraft {
                change_type: "ADDITION",
                old_node: None,
                new_node: Some(&new_stmt),
                old_index: None,
                new_index: None,
                confidence: 1.0,
                description: "Insert -> return_statement('os.path.basename(p)')".to_owned(),
                refactoring_kind: None,
                text_diff: None,
            },
        ];

        promote_matched_parent_statement_updates_drafts(&mut changes, &matching);

        assert_eq!(changes.len(), 1, "pair collapses to one edit: {changes:?}");
        assert_eq!(changes[0].change_type, "MODIFICATION");
        assert_eq!(changes[0].old_node.unwrap().label, "p");
        assert_eq!(changes[0].new_node.unwrap().label, "os.path.basename(p)");
    }
    #[test]
    fn genuine_entity_reorder_promotes_to_move_insertion_shift_stays_suppressed() {
        // Oracle scenario (issue #12): swapping two named functions is a genuine relocation —
        // one MOVE must surface (previously every same-identity reorder was suppressed, making
        // the swap read as style-only). An insertion-shift (relative order preserved) is still
        // suppressed as noise.
        // Vocabulary guard (issue #57/#66): the promotion is only as good as the named-entity
        // list — powershell's function_statement was missing, so a pure ps1 two-function swap
        // read as ZERO changes under routing.
        assert!(is_named_entity_type("function_statement"));
        let old_a = node("0.0", "function_definition", "greet", vec![]);
        let new_a = node("0.1", "function_definition", "greet", vec![]);
        let old_b = node("0.1", "function_definition", "add", vec![]);
        let new_b = node("0.0", "function_definition", "add", vec![]);
        let mut swapped = vec![
            reorder_draft(&old_a, &new_a, 0, 1),
            reorder_draft(&old_b, &new_b, 1, 0),
        ];
        let (suppressed, promoted) = suppress_low_signal_reorders_drafts(&mut swapped, &[]);
        assert_eq!(suppressed, 1, "one side of the swap is the mover: {swapped:?}");
        assert_eq!(promoted, vec![0], "the mover's index is reported for grouping");
        assert_eq!(swapped.len(), 1);
        assert_eq!(swapped[0].change_type, "MOVE");
        assert_eq!(swapped[0].old_node.unwrap().node_type, "function_definition");

        // Insertion shift: both functions moved DOWN by one slot (something added above);
        // relative order preserved -> all suppressed, nothing promoted.
        let old_c = node("0.0", "function_definition", "first", vec![]);
        let new_c = node("0.1", "function_definition", "first", vec![]);
        let old_d = node("0.1", "function_definition", "second", vec![]);
        let new_d = node("0.2", "function_definition", "second", vec![]);
        let mut shifted = vec![
            reorder_draft(&old_c, &new_c, 0, 1),
            reorder_draft(&old_d, &new_d, 1, 2),
        ];
        let (suppressed, promoted) = suppress_low_signal_reorders_drafts(&mut shifted, &[]);
        assert_eq!(suppressed, 2, "insertion shifts remain suppressed: {shifted:?}");
        assert!(promoted.is_empty());
        assert!(shifted.is_empty());
    }

    #[test]
    fn ordered_argument_and_initializer_permutations_survive_but_insertion_shift_does_not() {
        for (node_type, parent_type, description) in [
            ("identifier", "argument_list", "Reorder argument"),
            ("initializer_element", "initializer_list", "Reorder initializer"),
        ] {
            let mut old_left = node("root.0", node_type, "left", vec![]);
            let mut new_left = node("root.1", node_type, "left", vec![]);
            let mut old_right = node("root.1", node_type, "right", vec![]);
            let mut new_right = node("root.0", node_type, "right", vec![]);
            for item in [&mut old_left, &mut new_left, &mut old_right, &mut new_right] {
                item.parent_type = Some(parent_type.to_owned());
            }
            let old_parent = node("root", parent_type, "", vec![]);
            let new_parent = node("root", parent_type, "", vec![]);
            let matching = vec![
                MatchPair { old_node: &old_parent, new_node: &new_parent },
                MatchPair { old_node: &old_left, new_node: &new_left },
                MatchPair { old_node: &old_right, new_node: &new_right },
            ];
            let mut swapped = vec![
                reorder_draft(&old_left, &new_left, 0, 1),
                reorder_draft(&old_right, &new_right, 1, 0),
            ];
            let (suppressed, _) = suppress_low_signal_reorders_drafts(&mut swapped, &matching);
            assert_eq!(suppressed, 1, "ordered {parent_type} permutation: {swapped:?}");
            assert_eq!(swapped.len(), 1, "ordered {parent_type} permutation: {swapped:?}");
            assert_eq!(swapped[0].change_type, "MODIFICATION");
            assert!(swapped[0].description.contains(description), "{swapped:?}");
        }
    }

    #[test]
    fn executable_statement_reorder_is_preserved_but_insertion_displacement_is_suppressed() {
        let old_first = node("root.0", "expression_statement", "x += 2", vec![]);
        let new_first = node("root.1", "expression_statement", "x += 2", vec![]);
        let old_second = node("root.1", "expression_statement", "x *= 3", vec![]);
        let new_second = node("root.0", "expression_statement", "x *= 3", vec![]);
        let mut swapped = vec![
            reorder_draft(&old_first, &new_first, 0, 1),
            reorder_draft(&old_second, &new_second, 1, 0),
        ];

        let old_parent = node("root", "block", "", vec![]);
        let new_parent = node("root", "block", "", vec![]);
        let matching = vec![
            MatchPair { old_node: &old_parent, new_node: &new_parent },
            MatchPair { old_node: &old_first, new_node: &new_first },
            MatchPair { old_node: &old_second, new_node: &new_second },
        ];
        let (suppressed, promoted) = suppress_low_signal_reorders_drafts(&mut swapped, &matching);

        assert_eq!(suppressed, 1, "one executable statement must survive: {swapped:?}");
        assert!(promoted.is_empty(), "statement edits are not entity moves: {swapped:?}");
        assert_eq!(swapped.len(), 1);
        assert_eq!(swapped[0].change_type, "MODIFICATION");
        assert!(swapped[0].description.contains("Reorder statement"));

        let old_a = node("root.0", "expression_statement", "a()", vec![]);
        let new_a = node("root.1", "expression_statement", "a()", vec![]);
        let old_b = node("root.1", "expression_statement", "b()", vec![]);
        let new_b = node("root.2", "expression_statement", "b()", vec![]);
        let inserted_shift = node("root.0", "expression_statement", "x()", vec![]);
        let mut shifted = vec![
            reorder_draft(&old_a, &new_a, 0, 1),
            reorder_draft(&old_b, &new_b, 1, 2),
            ChangeDraft {
                change_type: "ADDITION", old_node: None, new_node: Some(&inserted_shift),
                old_index: None, new_index: Some(0), confidence: 1.0,
                description: "Insert unrelated statement".to_owned(),
                refactoring_kind: None, text_diff: None,
            },
        ];
        let matching = vec![
            MatchPair { old_node: &old_parent, new_node: &new_parent },
            MatchPair { old_node: &old_a, new_node: &new_a },
            MatchPair { old_node: &old_b, new_node: &new_b },
        ];
        let (suppressed, promoted) = suppress_low_signal_reorders_drafts(&mut shifted, &matching);
        assert_eq!(suppressed, 2, "insertion displacement remains noise: {shifted:?}");
        assert!(promoted.is_empty());
        assert_eq!(shifted.len(), 1, "unrelated insertion survives displacement suppression");
        assert_eq!(shifted[0].change_type, "ADDITION");

        let old_a = node("root.0", "expression_statement", "a()", vec![]);
        let old_b = node("root.1", "expression_statement", "b()", vec![]);
        let old_c = node("root.2", "expression_statement", "c()", vec![]);
        let new_x = node("root.0", "expression_statement", "x()", vec![]);
        let new_c = node("root.1", "expression_statement", "c()", vec![]);
        let new_b = node("root.2", "expression_statement", "b()", vec![]);
        let new_a = node("root.3", "expression_statement", "a()", vec![]);
        let mut mixed = vec![
            reorder_draft(&old_a, &new_a, 0, 3),
            reorder_draft(&old_b, &new_b, 1, 2),
            reorder_draft(&old_c, &new_c, 2, 1),
            ChangeDraft {
                change_type: "ADDITION", old_node: None, new_node: Some(&new_x),
                old_index: None, new_index: Some(0), confidence: 1.0,
                description: "Insert unrelated statement".to_owned(),
                refactoring_kind: None, text_diff: None,
            },
        ];
        let matching = vec![
            MatchPair { old_node: &old_parent, new_node: &new_parent },
            MatchPair { old_node: &old_a, new_node: &new_a },
            MatchPair { old_node: &old_b, new_node: &new_b },
            MatchPair { old_node: &old_c, new_node: &new_c },
        ];
        let (suppressed, _) = suppress_low_signal_reorders_drafts(&mut mixed, &matching);
        assert!(suppressed > 0, "one side of the real reorder is compacted: {mixed:?}");
        assert!(mixed.iter().any(|change| change.change_type == "MODIFICATION"),
            "the reorder remains meaningful alongside insertion: {mixed:?}");
        assert!(mixed.iter().any(|change| change.change_type == "ADDITION"
            && change.new_node.is_some_and(|node| node.label == "x()")),
            "the unrelated insertion remains visible: {mixed:?}");

        let old_moved = node("root.0", "expression_statement", "move()", vec![]);
        let new_moved = node("root.1", "expression_statement", "move()", vec![]);
        let old_edited = node("root.1", "expression_statement", "before()", vec![]);
        let new_edited = node("root.0", "expression_statement", "after()", vec![]);
        let mut reorder_with_modification = vec![
            reorder_draft(&old_moved, &new_moved, 0, 1),
            ChangeDraft {
                change_type: "MODIFICATION", old_node: Some(&old_edited), new_node: Some(&new_edited),
                old_index: Some(1), new_index: Some(0), confidence: 1.0,
                description: "Edit the other statement".to_owned(),
                refactoring_kind: None, text_diff: None,
            },
        ];
        let matching = vec![
            MatchPair { old_node: &old_parent, new_node: &new_parent },
            MatchPair { old_node: &old_moved, new_node: &new_moved },
            MatchPair { old_node: &old_edited, new_node: &new_edited },
        ];
        suppress_low_signal_reorders_drafts(&mut reorder_with_modification, &matching);
        assert!(reorder_with_modification.iter().any(|change| change.change_type == "MODIFICATION"
            && change.description.contains("Reorder statement")),
            "a MODIFICATION counterpart cannot hide the only order change: {reorder_with_modification:?}");

        // An inversion among entity siblings must not promote a stationary statement
        // displacement elsewhere in the same parent.
        let old_function_a = node("root.0", "function_declaration", "a", vec![]);
        let old_function_b = node("root.1", "function_declaration", "b", vec![]);
        let new_function_a = node("root.1", "function_declaration", "a", vec![]);
        let new_function_b = node("root.0", "function_declaration", "b", vec![]);
        let old_stationary_a = node("root.2", "expression_statement", "a()", vec![]);
        let old_stationary_b = node("root.3", "expression_statement", "b()", vec![]);
        let new_stationary_a = node("root.3", "expression_statement", "a()", vec![]);
        let new_stationary_b = node("root.4", "expression_statement", "b()", vec![]);
        let new_insertion = node("root.2", "expression_statement", "inserted()", vec![]);
        let mut unrelated_inversion = vec![
            reorder_draft(&old_function_a, &new_function_a, 0, 1),
            reorder_draft(&old_function_b, &new_function_b, 1, 0),
            reorder_draft(&old_stationary_a, &new_stationary_a, 2, 3),
            ChangeDraft {
                change_type: "ADDITION", old_node: None, new_node: Some(&new_insertion),
                old_index: None, new_index: Some(2), confidence: 1.0,
                description: "Insert before stationary statements".to_owned(),
                refactoring_kind: None, text_diff: None,
            },
        ];
        let matching = vec![
            MatchPair { old_node: &old_parent, new_node: &new_parent },
            MatchPair { old_node: &old_function_a, new_node: &new_function_a },
            MatchPair { old_node: &old_function_b, new_node: &new_function_b },
            MatchPair { old_node: &old_stationary_a, new_node: &new_stationary_a },
            MatchPair { old_node: &old_stationary_b, new_node: &new_stationary_b },
        ];
        suppress_low_signal_reorders_drafts(&mut unrelated_inversion, &matching);
        assert!(!unrelated_inversion.iter().any(|change| change.description.contains("Reorder statement")),
            "an unrelated entity inversion cannot promote insertion displacement: {unrelated_inversion:?}");
        assert!(unrelated_inversion.iter().any(|change| change.change_type == "ADDITION"));

        let old_only = node("root.0", "expression_statement", "only()", vec![]);
        let new_only = node("root.1", "expression_statement", "only()", vec![]);
        let mut unproven = vec![reorder_draft(&old_only, &new_only, 0, 1)];
        let matching = vec![
            MatchPair { old_node: &old_parent, new_node: &new_parent },
            MatchPair { old_node: &old_only, new_node: &new_only },
        ];
        suppress_low_signal_reorders_drafts(&mut unproven, &matching);
        assert_eq!(unproven.len(), 1, "a singleton has no relative-order proof: {unproven:?}");
        assert_eq!(unproven[0].change_type, "MODIFICATION");

        let inserted_before = node("root.0", "expression_statement", "inserted()", vec![]);
        let mut inserted_shift = vec![
            reorder_draft(&old_only, &new_only, 0, 1),
            ChangeDraft {
                change_type: "ADDITION", old_node: None, new_node: Some(&inserted_before),
                old_index: None, new_index: Some(0), confidence: 1.0,
                description: "Insert before the only matched statement".to_owned(),
                refactoring_kind: None, text_diff: None,
            },
        ];
        suppress_low_signal_reorders_drafts(&mut inserted_shift, &matching);
        assert_eq!(inserted_shift.len(), 1, "proven insertion displacement is suppressed");
        assert_eq!(inserted_shift[0].change_type, "ADDITION");

        let deleted_before = node("root.0", "expression_statement", "deleted()", vec![]);
        let old_after_deletion = node("root.1", "expression_statement", "after()", vec![]);
        let new_after_deletion = node("root.0", "expression_statement", "after()", vec![]);
        let mut deleted_shift = vec![
            reorder_draft(&old_after_deletion, &new_after_deletion, 1, 0),
            ChangeDraft {
                change_type: "DELETION", old_node: Some(&deleted_before), new_node: None,
                old_index: Some(0), new_index: None, confidence: 1.0,
                description: "Delete before the only matched statement".to_owned(),
                refactoring_kind: None, text_diff: None,
            },
        ];
        let matching = vec![
            MatchPair { old_node: &old_parent, new_node: &new_parent },
            MatchPair { old_node: &old_after_deletion, new_node: &new_after_deletion },
        ];
        suppress_low_signal_reorders_drafts(&mut deleted_shift, &matching);
        assert_eq!(deleted_shift.len(), 1, "proven deletion displacement is suppressed");
        assert_eq!(deleted_shift[0].change_type, "DELETION");
    }
    #[test]
    fn diff_semantic_tree_detects_javascript_function_move() {
        // JS function moved between two sibling positions: same parent,
        // different sibling index → at least one MOVE or REORDER change.
        let old_tree = module_with_nodes(vec![
            node(
                "0.0",
                "function_declaration",
                "first",
                vec![node("0.0.0", "identifier", "first", Vec::new())],
            ),
            node(
                "0.1",
                "function_declaration",
                "second",
                vec![node("0.1.0", "identifier", "second", Vec::new())],
            ),
        ]);
        // Swap the sibling order.
        let new_tree = module_with_nodes(vec![
            node(
                "0.1",
                "function_declaration",
                "second",
                vec![node("0.1.0", "identifier", "second", Vec::new())],
            ),
            node(
                "0.0",
                "function_declaration",
                "first",
                vec![node("0.0.0", "identifier", "first", Vec::new())],
            ),
        ]);

        let payload = diff_semantic_tree_for_test(&old_tree, &new_tree, "javascript");

        assert_eq!(payload["status"], COMPLETE);
        let types = change_types(&payload);
        assert!(
            types
                .iter()
                .any(|t| t == "MOVE" || t == "REORDER" || t == "MODIFICATION"),
            "expected a structural change for the JS function move, got {types:?}"
        );
        // Both functions should still be matched (by structural hash) — no spurious ADDITION/DELETION.
        assert!(!types.iter().any(|t| t == "ADDITION"), "no additions expected for a pure move");
        assert!(!types.iter().any(|t| t == "DELETION"), "no deletions expected for a pure move");
    }
    #[test]
    fn diff_semantic_tree_detects_java_override_reorder() {
        // Two methods swap sibling order in a Java class body. Mirrors the
        // competitor regression at tests/fixtures (Java @Override + reorder).
        // Both methods are structurally identical, so the matcher's job is to
        // preserve both entities (no spurious ADDITION/DELETION) and report
        // the sibling reorder — not to pair any specific id, since identical
        // subtrees make the pairing ambiguous by design.
        let method = |id: &str, label: &str| {
            node(
                id,
                "method_declaration",
                label,
                vec![node(&format!("{id}.0"), "identifier", label, Vec::new())],
            )
        };
        let old_tree = module_with_nodes(vec![method("0.0", "alpha"), method("0.1", "beta")]);
        let new_tree = module_with_nodes(vec![method("0.1", "beta"), method("0.0", "alpha")]);

        let payload = diff_semantic_tree_for_test(&old_tree, &new_tree, "java");

        assert_eq!(payload["status"], COMPLETE);
        let types = change_types(&payload);
        assert!(
            !types.iter().any(|t| t == "ADDITION"),
            "reorder should not invent additions, got {types:?}"
        );
        assert!(
            !types.iter().any(|t| t == "DELETION"),
            "reorder should not invent deletions, got {types:?}"
        );
        // Both methods should be matched by structural hash. With identical
        // positions and identical hashes the pairing is ambiguous, but the
        // *count* of matched pairs must cover both methods + both identifiers.
        let matched_count = payload["matching_pairs"]
            .as_array()
            .expect("matching_pairs must be an array")
            .len();
        assert!(
            matched_count >= 4,
            "both methods and their identifiers should be matched, got {matched_count}"
        );
    }
    #[test]
    fn diff_semantic_tree_detects_go_method_rename() {
        // Go method label change: same node shape, different label →
        // at least one MODIFICATION change and the same node id matched.
        let old_tree = module_with_nodes(vec![node(
            "0.0",
            "method_declaration",
            "OldName",
            vec![node("0.0.0", "identifier", "OldName", Vec::new())],
        )]);
        let new_tree = module_with_nodes(vec![node(
            "0.0",
            "method_declaration",
            "NewName",
            vec![node("0.0.0", "identifier", "NewName", Vec::new())],
        )]);

        let payload = diff_semantic_tree_for_test(&old_tree, &new_tree, "go");

        assert_eq!(payload["status"], COMPLETE);
        let types = change_types(&payload);
        assert!(
            types.iter().any(|t| t == "MODIFICATION" || t == "ADDITION" || t == "DELETION"),
            "expected a rename-driven change for the Go method, got {types:?}"
        );
    }
    #[test]
    fn diff_semantic_tree_preserves_json_keyed_identity() {
        // JSON keyed-data identity: two object children with the same `id`
        // key should be treated as the same entity regardless of array
        // position. This mirrors the JSON reorder noise-suppression
        // fixture covered for the Python path.
        let keyed = |node_id: &str, key: &str, value: &str| {
            node(
                node_id,
                "pair",
                key,
                vec![
                    node(&format!("{node_id}.k"), "string", key, Vec::new()),
                    node(&format!("{node_id}.v"), "string", value, Vec::new()),
                ],
            )
        };
        let old_tree = module_with_nodes(vec![keyed("0.0", "id", "alpha"), keyed("0.1", "id", "beta")]);
        // Reverse the array order — keyed identity should keep both matched.
        let new_tree = module_with_nodes(vec![keyed("0.1", "id", "beta"), keyed("0.0", "id", "alpha")]);

        let payload = diff_semantic_tree_for_test(&old_tree, &new_tree, "json");

        assert_eq!(payload["status"], COMPLETE);
        let types = change_types(&payload);
        assert!(
            !types.iter().any(|t| t == "ADDITION"),
            "keyed identity should not invent additions, got {types:?}"
        );
        assert!(
            !types.iter().any(|t| t == "DELETION"),
            "keyed identity should not invent deletions, got {types:?}"
        );
    }
    #[test]
    fn entity_recognition_matches_a_suffix_rename() {
        // A suffix rename (`calculate_total` -> `calculate_total_checked`, ~0.8
        // token similarity) is recognised as a rename by entity-aware matching:
        // the two function nodes are paired, giving a clean from->to, rather than
        // reported as an unrelated delete + add. (This previously left the pair
        // unmatched and only counted it as a fuzzy candidate; the improved matcher
        // now trusts high-similarity entity renames, so nothing is left as an
        // unmatched candidate.)
        let old_tree = module_with_nodes(vec![node(
            "0.0",
            "function_definition",
            "calculate_total",
            Vec::new(),
        )]);
        let new_tree = module_with_nodes(vec![node(
            "0.1",
            "function_definition",
            "calculate_total_checked",
            Vec::new(),
        )]);

        let report = compute_matching_with_diagnostics(&old_tree, &new_tree, 2, 0.5);

        assert!(
            report
                .pairs
                .iter()
                .any(|pair| pair.old_node.id == "0.0" && pair.new_node.id == "0.1"),
            "the suffix rename should be matched as a rename",
        );
        // Matched entities are not left as unmatched fuzzy candidates.
        assert_eq!(report.diagnostics.fuzzy_token_candidates, 0);
    }
    #[test]
    fn edit_generation_prunes_descendant_delete_noise_under_unmatched_entities() {
        let old_tree = module_with_nodes(vec![node(
            "0.0",
            "function_definition",
            "removed",
            vec![node("0.0.0", "identifier", "removed", Vec::new())],
        )]);
        let new_tree = module_with_nodes(Vec::new());
        let matching = compute_matching(&old_tree, &new_tree, 2, 0.5);
        let report = generate_changes_with_diagnostics(&old_tree, &new_tree, &matching, None);

        assert!(report.changes.iter().any(|change| {
            change.get("change_type").and_then(Value::as_str) == Some("DELETION")
                && change
                    .get("old_node")
                    .and_then(|node| node.get("node_type"))
                    .and_then(Value::as_str)
                    == Some("function_definition")
        }));
        assert!(!report.changes.iter().any(|change| {
            change.get("change_type").and_then(Value::as_str) == Some("DELETION")
                && change
                    .get("old_node")
                    .and_then(|node| node.get("node_type"))
                    .and_then(Value::as_str)
                    == Some("identifier")
        }));
        assert_eq!(report.diagnostics.delete_candidates, 1);
        assert_eq!(report.diagnostics.pruned_old_descendant_deletes, 1);
    }
    #[test]
    fn semantic_tree_diff_serializes_matching_pairs() {
        let old_tree = node(
            "old.root",
            "module",
            "module",
            vec![node("old.fn", "function", "total", Vec::new())],
        );
        let new_tree = node(
            "new.root",
            "module",
            "module",
            vec![node("new.fn", "function", "total", Vec::new())],
        );
        let out = diff_python_semantic_tree_json(
            &serde_json::to_string(&old_tree).unwrap(),
            &serde_json::to_string(&new_tree).unwrap(),
            "example.py",
            "example.py",
            "python",
            r#"{"min_height": 1}"#,
        )
        .unwrap();
        let payload: Value = serde_json::from_str(&out).unwrap();

        assert_eq!(payload["status"], COMPLETE);
        assert_eq!(payload["engine"], "rust_core_semantic_tree_v2_stage11");
        let pairs = payload["matching_pairs"].as_array().unwrap();
        assert!(pairs
            .iter()
            .any(|pair| pair["old_id"] == "old.fn" && pair["new_id"] == "new.fn"));
        assert_eq!(payload["metadata"]["wasm_boundary"], "python_pipeline");
    }
    #[test]
    fn container_noise_keeps_sole_carrier_and_drops_matched_rewrap() {
        // Oracle (issue #57 pilot): an empty go body gaining its first statements
        // collapses to ADDITION:block after descendant-noise suppression — that block is
        // the SOLE carrier of unmatched content and must survive; the old blanket drop
        // produced a zero-change diff for a real edit.
        let block = node(
            "1.0",
            "block",
            "block",
            vec![node("1.0.0", "identifier", "println", Vec::new())],
        );
        let mut changes = vec![addition_draft(&block)];
        suppress_candidate_container_noise_drafts(&mut changes, &[]);
        assert_eq!(changes.len(), 1, "sole-carrier block must survive");

        // Re-wrap: the same content leaf is MATCHED to the old side — the block addition
        // is wrapper noise and must still drop (the truthiness contracts the first,
        // matching-blind guard attempt broke).
        let old_leaf = node("9.0", "identifier", "println", Vec::new());
        let matching = vec![MatchPair {
            old_node: &old_leaf,
            new_node: &block.children[0],
        }];
        let mut changes = vec![addition_draft(&block)];
        suppress_candidate_container_noise_drafts(&mut changes, &matching);
        assert!(changes.is_empty(), "matched-content re-wrap block is noise");
    }
    #[test]
    fn csharp_formatting_anchor_emits_ignored_style_group() {
        // Oracle (issue #57 Root B): when surviving csharp MODIFICATIONs carry a formatting
        // anchor (an order_by_clause, or a format-string label), the routed finalize emits
        // an IGNORED_STYLE group recording that formatter wrapper churn was compacted.
        let old_lit = node("1", "integer_literal", "25363", Vec::new());
        let new_lit = node("2", "integer_literal", "25362", Vec::new());
        let old_ob = node("3", "order_by_clause", "order_by_clause", Vec::new());
        let new_ob = node("4", "order_by_clause", "order_by_clause descending", Vec::new());
        let mk = |old: &'static SemanticNode, new: &'static SemanticNode| ChangeDraft {
            change_type: "MODIFICATION",
            old_node: Some(old),
            new_node: Some(new),
            old_index: None,
            new_index: None,
            confidence: 1.0,
            description: String::new(),
            refactoring_kind: None,
            text_diff: None,
        };
        // Leak to get 'static refs for the closure signature convenience in the test.
        let old_lit: &'static SemanticNode = Box::leak(Box::new(old_lit));
        let new_lit: &'static SemanticNode = Box::leak(Box::new(new_lit));
        let old_ob: &'static SemanticNode = Box::leak(Box::new(old_ob));
        let new_ob: &'static SemanticNode = Box::leak(Box::new(new_ob));
        let changes = vec![mk(old_lit, new_lit), mk(old_ob, new_ob)];

        let (group, ignored) =
            formatting_equivalence_group_drafts(&changes, "csharp").expect("csharp anchor");
        assert_eq!(group["kind"], "IGNORED_STYLE");
        assert_eq!(
            group["rule_id"],
            "csharp.formatting.initializer_query_output_wrapping_equivalence"
        );
        let old_labels: Vec<String> = serde_json::from_value(group["old_labels"].clone()).unwrap();
        let new_labels: Vec<String> = serde_json::from_value(group["new_labels"].clone()).unwrap();
        assert!(old_labels.contains(&"25363".to_string()));
        assert!(new_labels.contains(&"25362".to_string()));
        assert_eq!(ignored["provenance"], "suppression");

        // A language with no formatting rule yields nothing.
        assert!(formatting_equivalence_group_drafts(&changes, "go").is_none());
    }
    #[test]
    fn corroborated_variable_rename_promotes_body_reference() {
        // Oracle (issue #57 dart flip): a body identifier rename (`a`->`x`) corroborated by an
        // anchored callable's parameter rename becomes a RENAME_VARIABLE; an uncorroborated one
        // (and a param swap) stay put — the routed analogue of refactoring.py inferred_rename_pairs.
        fn callable(param_labels: &[&str], body_labels: &[&str]) -> SemanticNode {
            let params: Vec<SemanticNode> = param_labels
                .iter()
                .enumerate()
                .map(|(i, l)| node(&format!("0.0.2.{i}"), "formal_parameter", l, Vec::new()))
                .collect();
            let plist = node("0.0.2", "formal_parameter_list", "formal_parameter_list", params);
            let body = node("0.0.3", "block", "block", body_labels.iter().enumerate()
                .map(|(i, label)| node(&format!("0.0.3.{i}"), "identifier", label, Vec::new()))
                .collect());
            node("0", "module", "module", vec![node("0.0", "function_definition", "add", vec![plist, body])])
        }
        let old_root: &'static SemanticNode = Box::leak(Box::new(callable(&["a", "b"], &["a", "b"])));
        let new_root: &'static SemanticNode = Box::leak(Box::new(callable(&["x", "y"], &["x", "y"])));

        let body_a: &'static SemanticNode = Box::leak(Box::new(old_root.descendants().into_iter().find(|n| n.id == "0.0.3.0").unwrap().clone()));
        let body_x: &'static SemanticNode = Box::leak(Box::new(new_root.descendants().into_iter().find(|n| n.id == "0.0.3.0").unwrap().clone()));
        let foo: &'static SemanticNode = Box::leak(Box::new(node("9.2", "identifier", "foo", Vec::new())));
        let bar: &'static SemanticNode = Box::leak(Box::new(node("9.3", "identifier", "bar", Vec::new())));
        let modif = |o: &'static SemanticNode, n: &'static SemanticNode| ChangeDraft {
            change_type: "MODIFICATION",
            old_node: Some(o),
            new_node: Some(n),
            old_index: None,
            new_index: None,
            confidence: 0.5,
            description: String::new(),
            refactoring_kind: None,
            text_diff: None,
        };
        let mut changes = vec![modif(body_a, body_x), modif(foo, bar)];
        promote_corroborated_variable_renames(&mut changes, old_root, new_root);
        assert_eq!(changes[0].change_type, "REFACTORING");
        assert_eq!(changes[0].refactoring_kind, Some("RENAME_VARIABLE"));
        // Uncorroborated foo->bar is left as a plain MODIFICATION.
        assert_eq!(changes[1].change_type, "MODIFICATION");
        assert_eq!(changes[1].refactoring_kind, None);

        // A parameter SWAP (a,b)->(b,a) infers nothing — the body reference stays a MODIFICATION.
        let swap_new: &'static SemanticNode = Box::leak(Box::new(callable(&["b", "a"], &["b", "a"])));
        let sb: &'static SemanticNode = Box::leak(Box::new(old_root.descendants().into_iter().find(|n| n.id == "0.0.3.0").unwrap().clone()));
        let sb2: &'static SemanticNode = Box::leak(Box::new(swap_new.descendants().into_iter().find(|n| n.id == "0.0.3.0").unwrap().clone()));
        let mut swap_changes = vec![modif(sb, sb2)];
        promote_corroborated_variable_renames(&mut swap_changes, old_root, swap_new);
        assert_eq!(swap_changes[0].change_type, "MODIFICATION");
    }
    #[test]
    fn puppet_resource_and_attribute_keys_are_identity_based() {
        // Oracle (issue #39): puppet resources key by type+title and attributes by name
        // (scoped by enclosing class), so the matcher pairs them by IDENTITY, not position.
        let value = node("0.0.1.0.2.1", "string", "'Hello'", Vec::new());
        let attr = node("0.0.1.0.2", "attribute", "message", vec![value]);
        let title = node("0.0.1.0.1", "string", "'hello'", Vec::new());
        let resource = node("0.0.1.0", "resource_declaration", "notify hello", vec![title, attr]);
        let block = node("0.0.1", "block", "block", vec![resource]);
        let class = node("0.0", "class_definition", "greeting", vec![block]);
        let root = node("0", "source_file", "source_file", vec![class]);
        let by_id = semantic_node_refs_by_id_with_root(&root);

        let resource_ref = *by_id.get("0.0.1.0").unwrap();
        let attr_ref = *by_id.get("0.0.1.0.2").unwrap();
        assert_eq!(
            resource_profile_key(resource_ref, &by_id, "puppet").unwrap(),
            vec![
                "puppet",
                "resource",
                "class_definition:greeting",
                "notify",
                "hello"
            ]
        );
        assert_eq!(
            resource_profile_key(attr_ref, &by_id, "puppet").unwrap(),
            vec![
                "puppet",
                "resource",
                "class_definition:greeting",
                "notify",
                "hello",
                "attribute",
                "message"
            ]
        );
        // Non-resource language yields no key (mechanism is inert unless the language routes).
        assert!(resource_profile_key(attr_ref, &by_id, "go").is_none());
        // _normalize strips matching quotes + lowercases.
        assert_eq!(resource_normalize("'Hello, World!'"), "hello, world!");
    }
    #[test]
    fn dockerfile_run_instructions_key_by_shell_command_identity() {
        // Oracle (issue #57 dockerfile flip): RUN instructions key by a shell-command IDENTITY so
        // an inserted RUN is a clean ADDITION instead of positionally cross-pairing with an
        // unrelated RUN (which swallowed the real compileall app->src edit under routing).
        let frag1 = node("0.0.0", "shell_fragment", "pip install -r requirements.txt", Vec::new());
        let run1 = node("0.0", "run_instruction", "RUN", vec![frag1]);
        let frag2 = node("0.1.0", "shell_fragment", "python -m compileall app", Vec::new());
        let run2 = node("0.1", "run_instruction", "RUN", vec![frag2]);
        let root = node("0", "source_file", "source_file", vec![run1, run2]);
        let by_id = semantic_node_refs_by_id_with_root(&root);

        let k1 = resource_profile_key(*by_id.get("0.0").unwrap(), &by_id, "dockerfile").unwrap();
        let k2 = resource_profile_key(*by_id.get("0.1").unwrap(), &by_id, "dockerfile").unwrap();
        assert_eq!(k1, vec!["dockerfile", "instruction", "run", "pip install -r", "0"]);
        assert_eq!(k2, vec!["dockerfile", "instruction", "run", "python -m compileall", "0"]);
        assert_ne!(k1, k2, "different RUN commands must NOT share an identity key");
        // Inert for a non-resource language.
        assert!(resource_profile_key(*by_id.get("0.0").unwrap(), &by_id, "go").is_none());
    }
    #[test]
    fn asm_instructions_key_by_mnemonic_and_operand_identity() {
        // Oracle (issue #57 asm flip): an instruction keys by mnemonic + first-operand IDENTITY —
        // NOT the operand value — so `mov ebx, 0` (old) and `mov ebx, 42` (new) share a key and
        // pair as ONE modification; `add ecx, 8` is a distinct instruction.
        let old_root = node(
            "0",
            "program",
            "program",
            vec![node("0.0", "instruction", "mov ebx, 0", Vec::new())],
        );
        let new_root = node(
            "0",
            "program",
            "program",
            vec![
                node("0.0", "instruction", "mov ebx, 42", Vec::new()),
                node("0.1", "instruction", "add ecx, 8", Vec::new()),
            ],
        );
        let old_by = semantic_node_refs_by_id_with_root(&old_root);
        let new_by = semantic_node_refs_by_id_with_root(&new_root);
        let k_old = statement_profile_key(*old_by.get("0.0").unwrap(), &old_by, "asm").unwrap();
        let k_new = statement_profile_key(*new_by.get("0.0").unwrap(), &new_by, "asm").unwrap();
        assert_eq!(k_old, vec!["asm", "instruction", "", "mov", "ebx", "0"]);
        assert_eq!(k_old, k_new, "mov ebx keys the same regardless of the operand value");
        let k_add = statement_profile_key(*new_by.get("0.1").unwrap(), &new_by, "asm").unwrap();
        assert_ne!(k_old, k_add);
        // Inert for a non-statement-profile language.
        assert!(statement_profile_key(*old_by.get("0.0").unwrap(), &old_by, "go").is_none());
    }
    #[test]
    fn entity_anchoring_recovers_pairs_and_gates_zips_by_statement_scope() {
        // Oracle (issue #57 anchors port): a relocated same-key function pairs by identity
        // (nearest line among same-key candidates), and the exact (type,label) zip only pairs
        // content within MATCHED statement scopes.
        let old_fn = node(
            "0.0",
            "function_declaration",
            "calc",
            vec![node("0.0.0", "identifier", "calc", Vec::new())],
        );
        let old_root = node("0", "source_file", "source_file", vec![old_fn]);
        let new_other = node("0.0", "function_declaration", "other", Vec::new());
        let new_fn = node(
            "0.1",
            "function_declaration",
            "calc",
            vec![node("0.1.0", "identifier", "calc", Vec::new())],
        );
        let new_root = node("0", "source_file", "source_file", vec![new_other, new_fn]);

        let pairs = recover_entity_pairs(&old_root, &new_root, &[]);
        assert_eq!(pairs.len(), 1, "same-key calc pairs; 'other' has no counterpart");
        assert_eq!(pairs[0].0.id, "0.0");
        assert_eq!(pairs[0].1.id, "0.1");

        let matching = augment_entity_matching(&old_root, &new_root, Vec::new(), "go");
        assert!(
            matching
                .iter()
                .any(|m| m.old_node.id == "0.0" && m.new_node.id == "0.1"),
            "the entity pair joins the matching"
        );
        // Inert for a non-code-like language.
        assert!(augment_entity_matching(&old_root, &new_root, Vec::new(), "yaml").is_empty());
        // The entity key excludes the tree root and label-less nodes.
        let parents = anchor_parent_map(&old_root);
        assert!(anchor_entity_key(&old_root, &parents).is_none());
    }
    #[test]
    fn delphi_statements_share_identity_across_argument_edits() {
        // Oracle (issue #57 delphi): a statement's identity is its call's CALLEE —
        // `WriteLn('Hello, ' + Name)` and `WriteLn(Format('Hello, %s!', [Name]))` share
        // `call:writeln`, so the statement matches as ONE unit and the inner literal folds.
        let old_call = node("0.0.0.0", "exprCall", "WriteLn('Hello, ' + Name)", Vec::new());
        let old_stmt = node("0.0.0", "statement", "WriteLn('Hello, ' + Name)", vec![old_call]);
        let old_proc = node("0.0", "defProc", "Greet", vec![old_stmt]);
        let old_tree = node("0", "program", "program", vec![old_proc]);
        let new_call = node(
            "0.0.0.0",
            "exprCall",
            "WriteLn(Format('Hello, %s!', [Name]))",
            Vec::new(),
        );
        let new_stmt = node(
            "0.0.0",
            "statement",
            "WriteLn(Format('Hello, %s!', [Name]))",
            vec![new_call],
        );
        let new_proc = node("0.0", "defProc", "Greet", vec![new_stmt]);
        let new_tree = node("0", "program", "program", vec![new_proc]);

        let old_by = semantic_node_refs_by_id_with_root(&old_tree);
        let new_by = semantic_node_refs_by_id_with_root(&new_tree);
        let k_old = statement_profile_key(*old_by.get("0.0.0").unwrap(), &old_by, "delphi");
        let k_new = statement_profile_key(*new_by.get("0.0.0").unwrap(), &new_by, "delphi");
        assert_eq!(
            k_old,
            Some(vec![
                "delphi".to_string(),
                "statement".to_string(),
                "routine:greet".to_string(),
                "call:writeln".to_string(),
                "0".to_string(),
            ])
        );
        assert_eq!(k_old, k_new, "argument edits must not change the statement identity");
        // The callee extractor reads through the parenthesised argument list.
        assert_eq!(delphi_callee("WriteLn('x')"), "WriteLn");
        assert_eq!(delphi_callee("Foo.Bar(1, 2)"), "Foo.Bar");
    }
    #[test]
    fn move_out_of_a_deleted_container_survives_descendant_noise() {
        // Oracle (issue #57 Root A): a node that MOVED out of a container that is itself
        // being DELETED (csharp `name = "guest"` leaving a collapsed `if`) is the edit — it
        // must NOT be suppressed as "covered by" the deletion. A DELETION only covers other
        // DELETIONs, not a MOVE that escaped it.
        let deleted_if = node("1", "if_statement", "if_statement", Vec::new());
        // The moved node's OLD id is a descendant (by id prefix) of the deleted if.
        let old_stmt = node("1.0.0", "expression_statement", "stmt", Vec::new());
        let new_stmt = node("2", "expression_statement", "stmt", Vec::new());
        let mut changes = vec![
            ChangeDraft {
                change_type: "MOVE",
                old_node: Some(&old_stmt),
                new_node: Some(&new_stmt),
                old_index: None,
                new_index: None,
                confidence: 1.0,
                description: String::new(),
                refactoring_kind: None,
                text_diff: None,
            },
            ChangeDraft {
                change_type: "DELETION",
                old_node: Some(&deleted_if),
                new_node: None,
                old_index: None,
                new_index: None,
                confidence: 1.0,
                description: String::new(),
                refactoring_kind: None,
                text_diff: None,
            },
        ];
        suppress_descendant_noise_drafts(&mut changes);
        let kinds: Vec<&str> = changes.iter().map(|c| c.change_type).collect();
        assert!(
            kinds.contains(&"MOVE") && kinds.contains(&"DELETION"),
            "the escaped MOVE and the container DELETION must both survive: {kinds:?}"
        );

        // Control: a MOVE that rode along INSIDE a moved container IS noise and drops.
        let moved_parent_old = node("3", "block", "block", Vec::new());
        let moved_parent_new = node("4", "block", "block", Vec::new());
        let inner_old = node("3.0", "expression_statement", "s", Vec::new());
        let inner_new = node("4.0", "expression_statement", "s", Vec::new());
        let mut riders = vec![
            ChangeDraft {
                change_type: "MOVE",
                old_node: Some(&moved_parent_old),
                new_node: Some(&moved_parent_new),
                old_index: None,
                new_index: None,
                confidence: 1.0,
                description: String::new(),
                refactoring_kind: None,
                text_diff: None,
            },
            ChangeDraft {
                change_type: "MOVE",
                old_node: Some(&inner_old),
                new_node: Some(&inner_new),
                old_index: None,
                new_index: None,
                confidence: 1.0,
                description: String::new(),
                refactoring_kind: None,
                text_diff: None,
            },
        ];
        suppress_descendant_noise_drafts(&mut riders);
        assert_eq!(riders.len(), 1, "inner move riding inside a moved parent is noise");
    }

#[test]
fn user_xml_dialect_predicate_and_coordinate_key() {
    let dialect: UserXmlDialect = serde_json::from_value(json!({
        "language_id": "acme-catalog",
        "root_element": "catalog",
        "keyed_elements": {"book": ["isbn"], "part": ["attr:sku"]}
    }))
    .expect("dialect spec parses");
    let tree: SemanticNode = serde_json::from_value(json!({
        "id": "0", "node_type": "document", "label": "",
        "structural_hash": "h", "position": {"start_line": 0, "start_col": 0, "end_line": 0, "end_col": 0},
        "children": [{
            "id": "0.0", "node_type": "element", "label": "catalog",
            "structural_hash": "h", "position": {"start_line": 0, "start_col": 0, "end_line": 0, "end_col": 0},
            "children": [{
                "id": "0.0.0", "node_type": "element", "label": "book",
                "structural_hash": "h", "position": {"start_line": 0, "start_col": 0, "end_line": 0, "end_col": 0},
                "children": [{
                    "id": "0.0.0.0", "node_type": "element", "label": "isbn",
                    "structural_hash": "h", "position": {"start_line": 0, "start_col": 0, "end_line": 0, "end_col": 0},
                    "children": [{
                        "id": "0.0.0.0.0", "node_type": "text", "label": "978-0",
                        "structural_hash": "h", "position": {"start_line": 0, "start_col": 0, "end_line": 0, "end_col": 0},
                        "children": []
                    }]
                }]
            }, {
                "id": "0.0.1", "node_type": "element", "label": "part",
                "structural_hash": "h", "position": {"start_line": 0, "start_col": 0, "end_line": 0, "end_col": 0},
                "children": [{
                    "id": "0.0.1.0", "node_type": "attribute", "label": "sku=A1",
                    "structural_hash": "h", "position": {"start_line": 0, "start_col": 0, "end_line": 0, "end_col": 0},
                    "children": []
                }]
            }]
        }]
    }))
    .expect("tree parses");

    assert!(xml_tree_matches_user_dialect(&dialect, &tree));
    let book = &tree.children[0].children[0];
    assert_eq!(
        user_dialect_coordinate_key(&dialect, book),
        Some(vec![
            "xml".to_string(),
            "acme-catalog".to_string(),
            "book".to_string(),
            "978-0".to_string(),
        ])
    );
    let part = &tree.children[0].children[1];
    assert_eq!(
        user_dialect_coordinate_key(&dialect, part),
        Some(vec![
            "xml".to_string(),
            "acme-catalog".to_string(),
            "part".to_string(),
            "A1".to_string(),
        ])
    );
    // A dialect with no predicate never claims a tree (fail closed).
    let unpredicated: UserXmlDialect = serde_json::from_value(json!({
        "language_id": "acme-open", "keyed_elements": {"x": ["y"]}
    }))
    .expect("spec parses");
    assert!(!xml_tree_matches_user_dialect(&unpredicated, &tree));
}

#[test]
fn variable_rename_inference_does_not_cross_callable_scope() {
    fn tree(f_param: &str, f_ref: &str, g_param: &str, g_ref: &str) -> SemanticNode {
        let callable = |id: &str, name: &str, param: &str, reference: &str| {
            node(id, "function_definition", name, vec![
                node(&format!("{id}.0"), "parameters", "parameters", vec![
                    node(&format!("{id}.0.0"), "identifier", param, vec![]),
                ]),
                node(&format!("{id}.1"), "block", "block", vec![
                    node(&format!("{id}.1.0"), "identifier", reference, vec![]),
                ]),
            ])
        };
        node("0", "module", "module", vec![
            callable("0.0", "f", f_param, f_ref),
            callable("0.1", "g", g_param, g_ref),
        ])
    }
    let old_tree = tree("a", "a", "a", "a");
    let new_tree = tree("b", "b", "a", "b");
    let old_ref = old_tree.descendants().into_iter().find(|n| n.id == "0.1.1.0").unwrap();
    let new_ref = new_tree.descendants().into_iter().find(|n| n.id == "0.1.1.0").unwrap();
    let mut changes = vec![ChangeDraft { change_type: "MODIFICATION", old_node: Some(old_ref),
        new_node: Some(new_ref), old_index: None, new_index: None, confidence: 1.0,
        description: String::new(), refactoring_kind: None, text_diff: None }];
    promote_corroborated_variable_renames(&mut changes, &old_tree, &new_tree);
    assert_eq!(changes[0].change_type, "MODIFICATION");
    assert_eq!(changes[0].refactoring_kind, None);
}

#[test]
fn duplicate_variable_rename_labels_keep_one_anchor_per_callable() {
    fn tree(name: &str, param: &str) -> SemanticNode {
        let function = |id: &str| node(id, "function_definition", name, vec![
            node(&format!("{id}.0"), "parameters", "parameters", vec![
                node(&format!("{id}.0.0"), "identifier", param, vec![]),
            ]),
            node(&format!("{id}.1"), "block", "block", vec![]),
        ]);
        node("0", "module", "module", vec![function("0.0"), function("0.1")])
    }
    let old_tree = tree("f", "a");
    let new_tree = tree("f", "b");
    let old_a = old_tree.descendants().into_iter().find(|n| n.id == "0.0.0.0").unwrap();
    let new_b = new_tree.descendants().into_iter().find(|n| n.id == "0.0.0.0").unwrap();
    let old_a2 = old_tree.descendants().into_iter().find(|n| n.id == "0.1.0.0").unwrap();
    let new_b2 = new_tree.descendants().into_iter().find(|n| n.id == "0.1.0.0").unwrap();
    let mut changes = vec![
        ChangeDraft { change_type: "REFACTORING", old_node: Some(old_a), new_node: Some(&new_b),
            old_index: None, new_index: None, confidence: 1.0, description: String::new(),
            refactoring_kind: Some("RENAME_VARIABLE"), text_diff: None },
        ChangeDraft { change_type: "REFACTORING", old_node: Some(old_a2), new_node: Some(&new_b2),
            old_index: None, new_index: None, confidence: 1.0, description: String::new(),
            refactoring_kind: Some("RENAME_VARIABLE"), text_diff: None },
    ];
    dedupe_variable_rename_drafts(&mut changes, &old_tree, &new_tree);
    assert_eq!(changes.len(), 2, "same labels in separate callable binders are distinct");
}

#[test]
fn rename_label_pair_does_not_suppress_an_independent_callable_edit() {
    let callable = |id: &str, name: &str, param: &str, reference: &str| {
        node(id, "function_definition", name, vec![
            node(&format!("{id}.0"), "parameters", "parameters", vec![
                node(&format!("{id}.0.0"), "identifier", param, vec![]),
            ]),
            node(&format!("{id}.1"), "block", "block", vec![
                node(&format!("{id}.1.0"), "identifier", reference, vec![]),
            ]),
        ])
    };
    let old_tree = node("0", "module", "module", vec![
        callable("0.0", "f", "a", "a"),
        callable("0.1", "g", "x", "a"),
    ]);
    let new_tree = node("0", "module", "module", vec![
        callable("0.0", "f", "b", "b"),
        callable("0.1", "g", "x", "b"),
    ]);
    let old_rename = old_tree.descendants().into_iter().find(|n| n.id == "0.0.0.0").unwrap();
    let new_rename = new_tree.descendants().into_iter().find(|n| n.id == "0.0.0.0").unwrap();
    let old_other = old_tree.descendants().into_iter().find(|n| n.id == "0.1.1.0").unwrap();
    let new_other = new_tree.descendants().into_iter().find(|n| n.id == "0.1.1.0").unwrap();
    let mut changes = vec![
        ChangeDraft { change_type: "REFACTORING", old_node: Some(old_rename), new_node: Some(new_rename),
            old_index: None, new_index: None, confidence: 1.0, description: String::new(),
            refactoring_kind: Some("RENAME_VARIABLE"), text_diff: None },
        ChangeDraft { change_type: "MODIFICATION", old_node: Some(old_other), new_node: Some(new_other),
            old_index: None, new_index: None, confidence: 1.0, description: String::new(),
            refactoring_kind: None, text_diff: None },
    ];
    suppress_modifications_covered_by_refactoring_labels(&mut changes, &old_tree, &new_tree);
    assert_eq!(changes.len(), 2, "a rename in another callable cannot own this edit");
}

#[test]
fn outer_parameter_rename_does_not_capture_nested_callable_edit() {
    let tree = |outer_param: &str, nested_ref: &str| {
        node("0", "module", "module", vec![node("0.0", "function_definition", "outer", vec![
            node("0.0.0", "parameters", "parameters", vec![
                node("0.0.0.0", "identifier", outer_param, vec![]),
            ]),
            node("0.0.1", "block", "block", vec![node("0.0.1.0", "function_definition", "inner", vec![
                node("0.0.1.0.0", "parameters", "parameters", vec![]),
                node("0.0.1.0.1", "block", "block", vec![
                    node("0.0.1.0.1.0", "identifier", nested_ref, vec![]),
                ]),
            ])]),
        ])])
    };
    let old_tree = tree("a", "a");
    let new_tree = tree("b", "b");
    let old_ref = old_tree.descendants().into_iter().find(|n| n.id == "0.0.1.0.1.0").unwrap();
    let new_ref = new_tree.descendants().into_iter().find(|n| n.id == "0.0.1.0.1.0").unwrap();
    let mut changes = vec![ChangeDraft { change_type: "MODIFICATION", old_node: Some(old_ref),
        new_node: Some(new_ref), old_index: None, new_index: None, confidence: 1.0,
        description: String::new(), refactoring_kind: None, text_diff: None }];
    promote_corroborated_variable_renames(&mut changes, &old_tree, &new_tree);
    assert_eq!(changes[0].change_type, "MODIFICATION");
}

#[test]
fn inserted_statement_does_not_reorder_existing_abap_statement() {
    let cases: Value = serde_json::from_str(include_str!("../tests/fixtures/reorder_insertion.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let result: Value = serde_json::from_str(&finalize_review_impl(
            &case["old_tree"].to_string(), &case["new_tree"].to_string(),
            case["old"].as_str().unwrap(), case["new"].as_str().unwrap(),
            "abap", "{}",
        ).unwrap()).unwrap();
        let changes = result["changes"].as_array().unwrap();
        assert!(changes.iter().any(|c| c["change_type"] == "ADDITION"), "{result}");
        assert!(changes.iter().any(|c| c["change_type"] == "MODIFICATION"), "{result}");
        assert!(!changes.iter().any(|c| c["change_type"] == "REORDER" || c["change_type"] == "MOVE"), "{result}");
    }
}

#[test]
fn reorder_compares_surviving_siblings_not_absolute_positions() {
    for (old_labels, new_labels, reordered) in [
        (vec!["a", "b"], vec!["x", "a", "b"], false),
        (vec!["x", "a", "b"], vec!["a", "b"], false),
        (vec!["a", "b"], vec!["b", "a"], true),
        (vec!["a", "b"], vec!["x", "b", "a"], true),
    ] {
        let make = |labels: &[&str]| node("0", "block", "block", labels.iter().enumerate()
            .map(|(i, label)| node(&format!("0.{i}"), "statement", label, vec![])).collect());
        let old = make(&old_labels);
        let new = make(&new_labels);
        let mut pairs = vec![MatchPair { old_node: &old, new_node: &new }];
        for old_node in &old.children {
            if let Some(new_node) = new.children.iter().find(|n| n.label == old_node.label) {
                pairs.push(MatchPair { old_node, new_node });
            }
        }
        let script = generate_edit_script_with_diagnostics_indexed(
            &TreeIndex::new(&old), &TreeIndex::new(&new), &pairs, None);
        let reorders: Vec<_> = script.ops.iter().filter(|op| op.kind == "REORDER").collect();
        assert_eq!(!reorders.is_empty(), reordered, "{old_labels:?} -> {new_labels:?}");
        if new_labels == ["x", "b", "a"] {
            // B has the same absolute index but changed its order relative to A.
            assert!(reorders.iter().any(|op| op.old_node.unwrap().label == "b"));
        }
    }
}

#[test]
fn call_layout_evidence_excludes_real_edits_and_deleted_calls() {
    let cases: Value = serde_json::from_str(include_str!("../tests/fixtures/call_layout_evidence.json")).unwrap();
    for case in cases.as_array().unwrap() {
        for (route, diff) in rename_review_routes(case["old"].as_str().unwrap(), case["new"].as_str().unwrap()) {
            assert!(!diff["changes"].as_array().unwrap().iter().any(|c| c["change_type"] == "MOVE"), "{route}: {diff}");
            let groups: Vec<_> = diff["change_groups"].as_array().unwrap().iter()
                .filter(|g| g["rule_id"] == "python.formatting.call_wrapping_equivalence").collect();
            assert_eq!(!groups.is_empty(), case["style"].as_bool().unwrap(), "{route}: {diff}");
            for group in groups {
                assert_eq!(group["metadata"]["evidence"], "matched_call_source");
                let labels = group["old_labels"].as_array().unwrap();
                assert!(labels.contains(&json!("send")));
                assert!(!labels.contains(&json!("foo")) && !labels.contains(&json!("use")));
            }
            assert!(diff["changes"].as_array().unwrap().iter().any(|c| c["change_type"] == "DELETION"));
            assert!(diff["changes"].as_array().unwrap().iter().any(|c| c["change_type"] == "MODIFICATION"));
        }
    }
}

#[test]
fn argument_lists_follow_their_owning_calls() {
    let cases: Value = serde_json::from_str(include_str!("../tests/fixtures/argument_owner_matching.json")).unwrap();
    for case in cases.as_array().unwrap() {
        for (route, diff) in rename_review_routes(case["old"].as_str().unwrap(), case["new"].as_str().unwrap()) {
            let mut actual: Vec<String> = diff["changes"].as_array().unwrap().iter().map(|c|
                json!([c["change_type"], c["old_node"]["node_type"], c["old_node"]["label"], c["new_node"]["node_type"], c["new_node"]["label"]]).to_string()).collect();
            let mut expected: Vec<String> = case["expected"].as_array().unwrap().iter().map(Value::to_string).collect();
            actual.sort(); expected.sort();
            assert_eq!(actual, expected, "{} {route}: {diff}", case["name"]);
            if !case["expected"].as_array().unwrap().iter().any(|e| e[0] == "MOVE") {
                assert!(!diff["changes"].as_array().unwrap().iter().any(|c| c["description"].as_str().unwrap_or("").contains("moved")), "{route}: {diff}");
            }
        }
    }
}

#[test]
fn elixir_sibling_swap_does_not_rewrite_unchanged_definitions() {
    let cases: Value = serde_json::from_str(include_str!("../tests/fixtures/elixir_sibling_swap.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let diff: Value = serde_json::from_str(&finalize_review_impl(
            &case["old_tree"].to_string(), &case["new_tree"].to_string(),
            case["source"]["old"].as_str().unwrap(), case["source"]["new"].as_str().unwrap(), "elixir", "{}",
        ).unwrap()).unwrap();
        // Unchanged definitions contribute only their module's coarse structural update;
        // a real edit inside a relocated definition remains exactly that value edit.
        let actual: Vec<Value> = diff["changes"].as_array().unwrap().iter().map(|c|
            json!([c["change_type"], c["old_node"]["node_type"], c["old_node"]["label"], c["new_node"]["node_type"], c["new_node"]["label"]])).collect();
        assert_eq!(json!(actual), case["expected"], "{}: {diff}", case["source"]["name"]);
    }
}


#[test]
fn same_line_duplicate_go_literals_prefer_actual_source_column() {
    // Source: "\tfmt.Println(add(3, 4))" at zero-based row 13.
    // The first argument changes 3 -> 4 at byte col 17; the second
    // argument 4 remains unchanged at col 20. Hash/label-only matching
    // must not steal that unchanged second argument for the first slot.
    fn literal(id: &str, label: &str, col: u32) -> SemanticNode {
        let mut node = node(id, "int_literal", label, vec![]);
        node.position = NodePosition {
            start_line: 13,
            start_col: col,
            end_line: 13,
            end_col: col + 1,
        };
        node
    }
    fn tree(first: &str, call_hash: &str, root_hash: &str) -> SemanticNode {
        let mut call = node("0.0", "call_expression", "add", vec![
            literal("0.0.0", first, 17),
            literal("0.0.1", "4", 20),
        ]);
        call.structural_hash = call_hash.into();
        let mut root = node("0", "source_file", "root", vec![call]);
        root.structural_hash = root_hash.into();
        root
    }
    let old = tree("3", "old-call", "old-root");
    let new = tree("4", "new-call", "new-root");
    let old_index = TreeIndex::new(&old);
    let new_index = TreeIndex::new(&new);
    for (stage, pairs) in [
        ("label_match", label_match(&old_index, &new_index, vec![])),
        ("top_down", top_down_match_with_existing(&old_index, &new_index, 0, vec![])),
    ] {
        let unchanged = pairs.iter()
            .find(|pair| pair.old_node.id == "0.0.1")
            .unwrap_or_else(|| panic!("{stage}: unchanged old 4 must match"));
        assert_eq!(unchanged.new_node.id, "0.0.1",
            "{stage}: matching duplicated 4 at byte col 17 instead of unchanged col 20");
        assert_eq!(unchanged.new_node.position.start_col, 20, "{stage}");
        assert!(pairs.iter().all(|pair|
            !(pair.old_node.id == "0.0.1" && pair.new_node.id == "0.0.0")),
            "{stage}: duplicate literal must not steal edited span");
    }
}


#[test]
fn shifted_go_duplicate_literal_keeps_unchanged_argument_slot() {
    // Two independent source edits on the same line: the earlier identifier
    // shortens and the last argument changes 3 -> 4. The existing middle 4
    // must not be paired to the newly edited final 4 merely because its
    // absolute byte column is now nearer the old middle argument.
    let old_source = "add(longName, 4, 3)";
    let new_source = "add(x, 4, 4)";
    assert_eq!(old_source.find(", 4").unwrap() + 2, 14);
    assert_eq!(new_source.find(", 4").unwrap() + 2, 7);
    assert_eq!(new_source.rfind(", 4").unwrap() + 2, 10);

    fn leaf(id: &str, kind: &str, label: &str, col: u32) -> SemanticNode {
        let mut result = node(id, kind, label, vec![]);
        result.position = NodePosition {
            start_line: 13,
            start_col: col,
            end_line: 13,
            end_col: col + label.len() as u32,
        };
        result
    }
    fn tree(prior: &str, middle_col: u32, last: &str, last_col: u32, hash: &str) -> SemanticNode {
        let mut call = node("0.0", "call_expression", "add", vec![
            leaf("0.0.0", "identifier", prior, 4),
            leaf("0.0.1", "int_literal", "4", middle_col),
            leaf("0.0.2", "int_literal", last, last_col),
        ]);
        call.structural_hash = format!("{hash}-call");
        let mut root = node("0", "source_file", "root", vec![call]);
        root.structural_hash = format!("{hash}-root");
        root
    }
    let old = tree("longName", 14, "3", 17, "old");
    let new = tree("x", 7, "4", 10, "new");
    let old_index = TreeIndex::new(&old);
    let new_index = TreeIndex::new(&new);
    for (stage, pairs) in [
        ("label_match", label_match(&old_index, &new_index, vec![])),
        ("top_down", top_down_match_with_existing(&old_index, &new_index, 0, vec![])),
    ] {
        let unchanged = pairs.iter()
            .find(|pair| pair.old_node.id == "0.0.1")
            .unwrap_or_else(|| panic!("{stage}: old middle literal must match"));
        assert_eq!(unchanged.new_node.id, "0.0.1",
            "{stage}: preceding width change must not steal the unchanged middle argument");
        assert_eq!(unchanged.new_node.position.start_col, 7, "{stage}");
        assert!(pairs.iter().all(|pair| !(pair.old_node.id == "0.0.1"
            && pair.new_node.id == "0.0.2")), "{stage}: edited last 4 must remain distinct");
    }
}

#[test]
fn deleting_preceding_go_argument_preserves_unchanged_duplicate_source_position() {
    // The deleted first argument shifts the child slot of the unchanged 4
    // from 1 to 0, while its actual line and UTF-8 byte column remain
    // identical. The last argument changes 3 -> 4, creating a duplicate
    // which MUST NOT steal the existing literal from its real source.
    let old_source = "add(x,\n    4, 3)";
    let new_source = "add(\n    4, 4)";
    assert!(old_source.contains("\n    4, 3"));
    assert!(new_source.contains("\n    4, 4"));
    fn literal(id: &str, kind: &str, label: &str, line: u32, col: u32) -> SemanticNode {
        let mut result = node(id, kind, label, vec![]);
        result.position = NodePosition {
            start_line: line,
            start_col: col,
            end_line: line,
            end_col: col + label.len() as u32,
        };
        result
    }
    fn tree(with_first: bool, final_value: &str, hash: &str) -> SemanticNode {
        let mut children = Vec::new();
        if with_first {
            children.push(literal("0.0.0", "identifier", "x", 0, 4));
        }
        let unchanged_id = if with_first { "0.0.1" } else { "0.0.0" };
        let changed_id = if with_first { "0.0.2" } else { "0.0.1" };
        children.push(literal(unchanged_id, "int_literal", "4", 1, 4));
        children.push(literal(changed_id, "int_literal", final_value, 1, 7));
        let mut call = node("0.0", "call_expression", "add", children);
        call.structural_hash = format!("{hash}-call");
        let mut root = node("0", "source_file", "root", vec![call]);
        root.structural_hash = format!("{hash}-root");
        root
    }
    let old = tree(true, "3", "old");
    let new = tree(false, "4", "new");
    let old_index = TreeIndex::new(&old);
    let new_index = TreeIndex::new(&new);
    assert_eq!(old_index.sibling_slot.get("0.0.1"), Some(&1));
    assert_eq!(new_index.sibling_slot.get("0.0.0"), Some(&0));
    for (stage, pairs) in [
        ("label_match", label_match(&old_index, &new_index, vec![])),
        ("top_down", top_down_match_with_existing(&old_index, &new_index, 0, vec![])),
    ] {
        let unchanged = pairs.iter()
            .find(|pair| pair.old_node.id == "0.0.1")
            .unwrap_or_else(|| panic!("{stage}: unchanged old 4 must match"));
        assert_eq!(unchanged.new_node.id, "0.0.0",
            "{stage}: slot shift must not reassign original 4 to edited final 4");
        assert_eq!(unchanged.new_node.position.start_col, 4, "{stage}");
        assert!(pairs.iter().all(|pair| !(pair.old_node.id == "0.0.1"
            && pair.new_node.id == "0.0.1")), "{stage}: edited final 4 must stay distinct");
    }
}

#[test]
fn duplicate_sibling_slots_are_indexed_once_for_candidate_ranking() {
    let children = (0..512usize)
        .map(|i| node(&format!("0.{i}"), "int_literal", "4", vec![]))
        .collect();
    let root = node("0", "argument_list", "args", children);
    let index = TreeIndex::new(&root);
    for (i, child) in root.children.iter().enumerate() {
        assert_eq!(index.sibling_slot.get(child.id.as_str()), Some(&i));
    }
}
