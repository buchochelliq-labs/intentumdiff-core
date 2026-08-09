// Split from tests_inline.rs (issue #85): one file per test family.
// Nested inside cfg(test) mod tests - `super::*` = the tests mod (helpers),
// `crate::*` = the engine.
#![allow(unused_imports)]
use super::*;
use crate::*;

    #[test]
    fn container_noise_guard_keeps_sole_carrier_and_drops_rewrap() {
        // Sole carrier: a block ADDITION whose content leaves are unmatched must survive —
        // the blanket drop turned `func f() {}` gaining its first statement into a
        // zero-change diff (issue #57 go pilot).
        let sole = container_guard_node(
            "b1",
            "block",
            "block",
            json!([{
                "id": "b1.0", "node_type": "identifier", "label": "println",
                "position": {"start_line": 0, "start_col": 0, "end_line": 0, "end_col": 0},
                "structural_hash": "h-b1.0", "children": []
            }]),
        );
        let mut drafts = vec![container_guard_draft("ADDITION", None, Some(&sole))];
        suppress_candidate_container_noise_drafts(&mut drafts, &[]);
        assert_eq!(drafts.len(), 1, "sole-carrier block must survive");

        // Re-wrap noise: the same shape but with its content leaf MATCHED to the old side
        // is wrapper churn and must drop (the 7-test blast radius of the e8b5ce8 guard).
        let old_leaf_owner = container_guard_node(
            "o1",
            "block",
            "block",
            json!([{
                "id": "o1.0", "node_type": "identifier", "label": "println",
                "position": {"start_line": 0, "start_col": 0, "end_line": 0, "end_col": 0},
                "structural_hash": "h-o1.0", "children": []
            }]),
        );
        let matching = vec![MatchPair {
            old_node: &old_leaf_owner.children[0],
            new_node: &sole.children[0],
        }];
        let mut drafts = vec![container_guard_draft("ADDITION", None, Some(&sole))];
        suppress_candidate_container_noise_drafts(&mut drafts, &matching);
        assert!(drafts.is_empty(), "re-wrap block with matched content must drop");

        // Empty shells stay suppressed (structural leaves label themselves by type).
        let empty = container_guard_node(
            "b2",
            "block",
            "block",
            json!([{
                "id": "b2.0", "node_type": "statement_list", "label": "statement_list",
                "position": {"start_line": 0, "start_col": 0, "end_line": 0, "end_col": 0},
                "structural_hash": "h-b2.0", "children": []
            }]),
        );
        let mut drafts = vec![container_guard_draft("ADDITION", None, Some(&empty))];
        suppress_candidate_container_noise_drafts(&mut drafts, &[]);
        assert!(drafts.is_empty(), "structural-only block must still drop");
    }
    #[test]
    fn native_cst_distinguishes_async_function_def() {
        // Oracle: `def f()` -> `async def f()` changes runtime semantics (the call site now
        // gets a coroutine) — it must NEVER hash style-only-equal. tree-sitter drops the
        // anonymous `async` token from named children, so the serializer surfaces it in the
        // node type instead.
        let sync_src = "def f():\n    return 1\n";
        let async_src = "async def f():\n    return 1\n";
        let sync_json =
            serialize_tree_json(&parse_python_tree(sync_src).unwrap(), sync_src).unwrap();
        let async_json =
            serialize_tree_json(&parse_python_tree(async_src).unwrap(), async_src).unwrap();
        assert!(async_json.contains("async_function_def"), "{async_json}");
        assert!(!sync_json.contains("async_function_def"));
    }
    #[test]
    fn paired_change_endpoints_suppress_duplicate_add_delete_drafts() {
        // Oracle scenario (issue #13): a promoted literal MODIFICATION must absorb the raw
        // ADDITION/DELETION drafts of the same nodes — one edit is reported exactly once.
        let old_str = node("0.5", "string", "'Hi '", vec![]);
        let new_str = node("0.5", "string", "'Hello '", vec![]);
        let unrelated = node("0.9", "string", "'kept'", vec![]);
        let mut changes = vec![
            ChangeDraft {
                change_type: "MODIFICATION",
                old_node: Some(&old_str),
                new_node: Some(&new_str),
                old_index: None,
                new_index: None,
                confidence: 1.0,
                description: "Update string(''Hi '') -> string(''Hello '')".to_owned(),
                refactoring_kind: None,
                text_diff: None,
            },
            // The double-report: the same new string node also drafted as an ADDITION,
            // and the same old string node also drafted as a DELETION.
            ChangeDraft {
                change_type: "ADDITION",
                old_node: None,
                new_node: Some(&new_str),
                old_index: None,
                new_index: None,
                confidence: 1.0,
                description: "Insert -> string(''Hello '')".to_owned(),
                refactoring_kind: None,
                text_diff: None,
            },
            ChangeDraft {
                change_type: "DELETION",
                old_node: Some(&old_str),
                new_node: None,
                old_index: None,
                new_index: None,
                confidence: 1.0,
                description: "Delete string(''Hi '')".to_owned(),
                refactoring_kind: None,
                text_diff: None,
            },
            // An unrelated ADDITION with a different node id must survive.
            ChangeDraft {
                change_type: "ADDITION",
                old_node: None,
                new_node: Some(&unrelated),
                old_index: None,
                new_index: None,
                confidence: 1.0,
                description: "Insert -> string(''kept'')".to_owned(),
                refactoring_kind: None,
                text_diff: None,
            },
        ];

        suppress_add_delete_drafts_covered_by_pairings(&mut changes);

        assert_eq!(changes.len(), 2, "duplicates suppressed, unrelated kept: {changes:?}");
        assert_eq!(changes[0].change_type, "MODIFICATION");
        assert_eq!(changes[1].change_type, "ADDITION");
        assert_eq!(changes[1].new_node.unwrap().label, "'kept'");
    }
    #[test]
    fn generic_text_single_added_line_with_audit_group() {
        let old = "a\nb";
        let new = "a\nb\n\n/.intentumdiff";
        let payload: Value =
            serde_json::from_str(&generic_text_review_json(old, new, 4).unwrap()).unwrap();
        assert_eq!(payload["used"], Value::Bool(true));
        let changes = payload["changes"].as_array().unwrap();
        assert_eq!(changes.len(), 1, "{changes:?}");
        assert_eq!(changes[0]["change_type"], "ADDITION");
        assert_eq!(changes[0]["new_node"]["label"], "/.intentumdiff");
        let group = &payload["group"];
        assert_eq!(group["rule_id"], "presentation.generic_text_diff");
        assert_eq!(group["metadata"]["suppressed_count"], 4);
        assert_eq!(group["metadata"]["replacement_count"], 1);
    }
    #[test]
    fn generic_text_changed_line_is_one_modification_with_inline_detail() {
        let old = "the quick fox\n";
        let new = "the quick dog\n";
        let payload: Value =
            serde_json::from_str(&generic_text_review_json(old, new, 0).unwrap()).unwrap();
        let changes = payload["changes"].as_array().unwrap();
        assert_eq!(changes.len(), 1, "{changes:?}");
        assert_eq!(changes[0]["change_type"], "MODIFICATION");
        let text_diff = changes[0]["text_diff"].as_str().unwrap();
        assert!(
            text_diff.contains("[-") && text_diff.contains("[+"),
            "inline char detail expected, got {text_diff:?}"
        );
    }
    #[test]
    fn generic_text_deleted_blank_lines_are_layout_not_content() {
        let old = "alpha\n\n\nbeta\n";
        let new = "alpha\nbeta\n";
        let payload: Value =
            serde_json::from_str(&generic_text_review_json(old, new, 0).unwrap()).unwrap();
        assert_eq!(payload["changes"].as_array().unwrap().len(), 0);
    }
    #[test]
    fn diff_semantic_tree_accepts_non_python_language_label() {
        // The legacy entrypoint returns SCAFFOLD for non-Python; the
        // language-agnostic entrypoint must complete instead.
        let old_tree = module_with_nodes(vec![node(
            "0.0",
            "function_declaration",
            "doThing",
            vec![node("0.0.0", "identifier", "doThing", Vec::new())],
        )]);
        let new_tree = module_with_nodes(vec![node(
            "0.0",
            "function_declaration",
            "doThing",
            vec![node("0.0.0", "identifier", "doThing", Vec::new())],
        )]);

        let payload = diff_semantic_tree_for_test(&old_tree, &new_tree, "javascript");

        assert_eq!(payload["status"], COMPLETE);
        assert_eq!(payload["engine"], "rust_core_semantic_tree_v3");
        assert_eq!(payload["language"], "javascript");
    }
    #[test]
    fn duplicate_ids_are_rejected() {
        let root = SemanticNode {
            id: "0".to_string(),
            node_type: "module".to_string(),
            label: "module".to_string(),
            position: NodePosition {
                start_line: 0,
                start_col: 0,
                end_line: 0,
                end_col: 0,
            },
            structural_hash: "h".to_string(),
            children: vec![SemanticNode {
                id: "0".to_string(),
                node_type: "identifier".to_string(),
                label: "x".to_string(),
                position: NodePosition {
                    start_line: 0,
                    start_col: 0,
                    end_line: 0,
                    end_col: 1,
                },
                structural_hash: "c".to_string(),
                children: Vec::new(),
                parent_type: None,
                type_info: None,
                facts: None,
            }],
            parent_type: None,
            type_info: None,
            facts: None,
        };
        assert!(validate_unique_ids(&root).is_err());
    }
    #[test]
    fn python_source_serializes_to_cst_shape() {
        let tree = parse_python_tree("def total(items):\n    return sum(items)\n").unwrap();
        let cst_json =
            serialize_tree_json(&tree, "def total(items):\n    return sum(items)\n").unwrap();
        let cst: CstNode = serde_json::from_str(&cst_json).unwrap();

        assert_eq!(cst.node_type, "module");
        assert!(cst.named);
        assert!(cst
            .children
            .iter()
            .any(|child| child.node_type == "function_definition"));
    }
    #[test]
    fn scope_trails_cover_non_python_declaration_ancestry() {
        let old_tree = json!({
            "id": "old-root",
            "node_type": "module",
            "label": "module",
            "position": {"start_line": 1, "start_col": 0, "end_line": 1, "end_col": 70},
            "structural_hash": "old-root",
            "children": [{
                "id": "old-class",
                "node_type": "class_declaration",
                "label": "Demo",
                "position": {"start_line": 1, "start_col": 0, "end_line": 1, "end_col": 70},
                "structural_hash": "old-class",
                "children": [{
                    "id": "old-method",
                    "node_type": "method_declaration",
                    "label": "run",
                    "position": {"start_line": 1, "start_col": 13, "end_line": 1, "end_col": 68},
                    "structural_hash": "old-method",
                    "children": [{
                        "id": "old-literal",
                        "node_type": "integer_literal",
                        "label": "1",
                        "position": {"start_line": 1, "start_col": 37, "end_line": 1, "end_col": 38},
                        "structural_hash": "old-literal"
                    }]
                }]
            }]
        });
        let new_tree = json!({
            "id": "new-root",
            "node_type": "module",
            "label": "module",
            "position": {"start_line": 1, "start_col": 0, "end_line": 1, "end_col": 70},
            "structural_hash": "new-root",
            "children": [{
                "id": "new-class",
                "node_type": "class_declaration",
                "label": "Demo",
                "position": {"start_line": 1, "start_col": 0, "end_line": 1, "end_col": 70},
                "structural_hash": "new-class",
                "children": [{
                    "id": "new-method",
                    "node_type": "method_declaration",
                    "label": "run",
                    "position": {"start_line": 1, "start_col": 13, "end_line": 1, "end_col": 68},
                    "structural_hash": "new-method",
                    "children": [{
                        "id": "new-literal",
                        "node_type": "integer_literal",
                        "label": "2",
                        "position": {"start_line": 1, "start_col": 37, "end_line": 1, "end_col": 38},
                        "structural_hash": "new-literal"
                    }]
                }]
            }]
        });
        let request = json!({
            "old_tree": old_tree,
            "new_tree": new_tree,
            "changes": [{
                "old_node": {"id": "old-literal"},
                "new_node": {"id": "new-literal"}
            }]
        });
        let payload = rust_scope_trails_value(&request).unwrap();

        for side in ["old", "new"] {
            let trail = payload["scope_trails"][side][0]["trail"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>();
            assert_eq!(trail, vec!["class Demo", "function run"]);
        }
    }
    #[test]
    fn semantic_tree_duplicate_ids_are_rejected() {
        let old_tree = node(
            "old.root",
            "module",
            "module",
            vec![node("old.root", "function", "duplicate", Vec::new())],
        );
        let new_tree = node("new.root", "module", "module", Vec::new());

        assert!(diff_python_semantic_tree_json(
            &serde_json::to_string(&old_tree).unwrap(),
            &serde_json::to_string(&new_tree).unwrap(),
            "example.py",
            "example.py",
            "python",
            "{}",
        )
        .is_err());
    }
    #[test]
    fn dying_no_delta_modification_cannot_swallow_its_add_delete_pair() {
        // Oracle (issue #57 pilot, go error-wrapping): a same-label MODIFICATION with no
        // id-stable leaf delta used to swallow the return DELETE+ADD pair via the
        // pairings suppression and then die in the no-delta filter — erasing the edit.
        // The no-delta filter now runs first, so the pair must survive finalize.
        let old_ret = node(
            "1.1",
            "return_statement",
            "return_statement",
            vec![node("1.1.0", "identifier", "err", Vec::new())],
        );
        let new_ret = node(
            "2.1",
            "return_statement",
            "return_statement",
            vec![node("2.1.0", "call_expression", "call_expression", Vec::new())],
        );
        let old_if = node("1", "if_statement", "if_statement", vec![old_ret]);
        let new_if = node("2", "if_statement", "if_statement", vec![new_ret]);
        let old_tree = module_with_nodes(vec![old_if]);
        let new_tree = module_with_nodes(vec![new_if]);

        let mut changes = vec![
            ChangeDraft {
                change_type: "MODIFICATION",
                old_node: Some(&old_tree.children[0]),
                new_node: Some(&new_tree.children[0]),
                old_index: Some(0),
                new_index: Some(0),
                confidence: 1.0,
                description: String::new(),
                refactoring_kind: None,
                text_diff: None,
            },
            ChangeDraft {
                change_type: "DELETION",
                old_node: Some(&old_tree.children[0].children[0]),
                new_node: None,
                old_index: Some(1),
                new_index: None,
                confidence: 1.0,
                description: String::new(),
                refactoring_kind: None,
                text_diff: None,
            },
            addition_draft(&new_tree.children[0].children[0]),
        ];
        let mut finalization = PythonReviewFinalization::default();
        finalize_python_review_drafts(&mut changes, &old_tree, &new_tree, "a", "b", &mut finalization, "python");

        let surviving: Vec<&str> = changes.iter().map(|change| change.change_type).collect();
        assert!(
            surviving.contains(&"DELETION") && surviving.contains(&"ADDITION"),
            "the return pair must survive when its covering modification dies: {surviving:?}"
        );
        assert!(
            !surviving.contains(&"MODIFICATION"),
            "the no-id-stable-delta modification itself should die: {surviving:?}"
        );
    }
    #[test]
    fn ignore_intent_descriptions_read_as_human_review() {
        let add = node("1", "pattern", "/.intentumdiff", Vec::new());
        assert_eq!(
            ignore_intent_description("ADDITION", None, Some(&add)).as_deref(),
            Some("Adds an ignore rule for /.intentumdiff")
        );
        let del = node("2", "pattern", "*.log", Vec::new());
        assert_eq!(
            ignore_intent_description("DELETION", Some(&del), None).as_deref(),
            Some("Stops ignoring *.log")
        );
        let neg = node("3", "negated_pattern", "!build/keep.txt", Vec::new());
        assert_eq!(
            ignore_intent_description("ADDITION", None, Some(&neg)).as_deref(),
            Some("Adds an exception for build/keep.txt (no longer ignored)")
        );
        // Non-ignore node types are untouched (structural description preserved).
        let fun = node("4", "function_declaration", "f", Vec::new());
        assert_eq!(ignore_intent_description("ADDITION", None, Some(&fun)), None);
    }
    #[test]
    fn dart_param_container_is_recognised_across_grammar_spellings() {
        // Oracle (issue #21/#57): the parameter-rename promoter must accept dart's
        // `formal_parameter_list` (and swift's `parameter_clause`, etc.), not only "parameters".
        assert!(is_parameter_list_type("formal_parameter_list"));
        assert!(is_parameter_list_type("parameter_clause"));
        assert!(is_parameter_list_type("parameters"));
        assert!(!is_parameter_list_type("function_body"));
    }
    #[test]
    fn keyed_data_array_scalars_key_by_content_and_yaml_restyle_is_suppressed() {
        // Oracle (issue #57 json/yaml): array SCALARS key by content identity + same-label
        // ordinal — never by position — so an insertion cannot re-identify later siblings.
        // The ':'-stripping pair normalizer must NOT apply (onCommand:* values are distinct).
        let s1 = node("0.0.1.0", "string", "onCommand:intentumdiff.toggle", Vec::new());
        let s2 = node("0.0.1.1", "string", "onCommand:intentumdiff.showOutput", Vec::new());
        let array = node("0.0.1", "array", "array", vec![s1, s2]);
        let key_node = node("0.0.0", "string", "activationEvents", Vec::new());
        let pair = node("0.0", "pair", "activationEvents", vec![key_node, array]);
        let root = node("0", "document", "document", vec![pair]);
        let by_id = semantic_node_refs_by_id_with_root(&root);
        let k1 = keyed_data_key(*by_id.get("0.0.1.0").unwrap(), &by_id, "json").unwrap();
        let k2 = keyed_data_key(*by_id.get("0.0.1.1").unwrap(), &by_id, "json").unwrap();
        assert_ne!(k1, k2, "distinct scalar contents must not share a key");
        assert!(k1.contains(&"onCommand:intentumdiff.toggle".to_string()));
        // The pair itself keys by its key path.
        let kp = keyed_data_key(*by_id.get("0.0").unwrap(), &by_id, "json").unwrap();
        assert_eq!(kp, vec!["json", "pair", "activationevents"]);
        // Inert for non-keyed languages.
        assert!(keyed_data_key(*by_id.get("0.0").unwrap(), &by_id, "go").is_none());

        // YAML representation equivalence: a block_node -> flow_node wrapper MODIFICATION with
        // generic labels is style, suppressed with the cataloged rule's evidence group.
        let old_wrap = node("1.0", "block_node", "block_node", Vec::new());
        let new_wrap = node("1.0", "flow_node", "flow_node", Vec::new());
        let old_wrap: &'static SemanticNode = Box::leak(Box::new(old_wrap));
        let new_wrap: &'static SemanticNode = Box::leak(Box::new(new_wrap));
        let mut changes = vec![ChangeDraft {
            change_type: "MODIFICATION",
            old_node: Some(old_wrap),
            new_node: Some(new_wrap),
            old_index: None,
            new_index: None,
            confidence: 1.0,
            description: String::new(),
            refactoring_kind: None,
            text_diff: None,
        }];
        let group = suppress_yaml_representation_equivalent_drafts(&mut changes)
            .expect("restyle suppressed with evidence");
        assert!(changes.is_empty());
        assert_eq!(
            group["rule_id"],
            "presentation.suppress_yaml_representation_equivalent_modification"
        );
    }
    #[test]
    fn bash_commands_key_by_name_and_cross_key_modifications_split() {
        // Oracle (issue #57 bash): commands key by their NAME within their scope, so `:` and
        // `echo Hello` are DIFFERENT statements — a MODIFICATION pairing them (the issue-#33
        // lone del/add repair re-merging what the profile unpaired) splits back to DELETE+ADD.
        let old_cmd = node("0.0.1.0", "command", ":", Vec::new());
        let old_body = node("0.0.1", "compound_statement", "compound_statement", vec![old_cmd]);
        let old_fn = node("0.0", "function_definition", "f", vec![old_body]);
        let old_tree = node("0", "program", "program", vec![old_fn]);
        let new_cmd = node("0.0.1.0", "command", "echo Hello", Vec::new());
        let new_body = node("0.0.1", "compound_statement", "compound_statement", vec![new_cmd]);
        let new_fn = node("0.0", "function_definition", "f", vec![new_body]);
        let new_tree = node("0", "program", "program", vec![new_fn]);

        let old_by = semantic_node_refs_by_id_with_root(&old_tree);
        let new_by = semantic_node_refs_by_id_with_root(&new_tree);
        let k_old = statement_profile_key(*old_by.get("0.0.1.0").unwrap(), &old_by, "bash");
        let k_new = statement_profile_key(*new_by.get("0.0.1.0").unwrap(), &new_by, "bash");
        assert!(k_old.is_some() && k_new.is_some());
        assert_ne!(k_old, k_new, "different commands must not share a key");
        assert_eq!(
            k_old.unwrap(),
            vec!["bash", "command", "function:f", ":", "0"]
        );

        let old_ref = *old_by.get("0.0.1.0").unwrap();
        let new_ref = *new_by.get("0.0.1.0").unwrap();
        let mut changes = vec![ChangeDraft {
            change_type: "MODIFICATION",
            old_node: Some(old_ref),
            new_node: Some(new_ref),
            old_index: None,
            new_index: None,
            confidence: 1.0,
            description: String::new(),
            refactoring_kind: None,
            text_diff: None,
        }];
        split_cross_key_statement_modifications_drafts(&mut changes, &old_tree, &new_tree, "bash");
        let kinds: Vec<&str> = changes.iter().map(|c| c.change_type).collect();
        assert_eq!(kinds, vec!["DELETION", "ADDITION"]);
        // Same-key modifications are left alone (an operand edit, not a replacement).
        let mut same = vec![ChangeDraft {
            change_type: "MODIFICATION",
            old_node: Some(old_ref),
            new_node: Some(old_ref),
            old_index: None,
            new_index: None,
            confidence: 1.0,
            description: String::new(),
            refactoring_kind: None,
            text_diff: None,
        }];
        split_cross_key_statement_modifications_drafts(&mut same, &old_tree, &old_tree, "bash");
        assert_eq!(same.len(), 1);
        assert_eq!(same[0].change_type, "MODIFICATION");
    }
    #[test]
    fn statement_container_modification_folds_descendant_token_churn() {
        // Oracle (#57 bash/delphi): a MODIFIED review container (a bash variable_assignment whose
        // identity is unchanged but body edited) folds its descendant token churn (a new
        // `expansion`) into the single MODIFICATION — the general descendant-noise pass never
        // roots on a MODIFICATION.
        let old_var = node("0.0", "variable_assignment", "NAME=$1", Vec::new());
        let expansion = node("0.0.1", "expansion", "World", Vec::new());
        let new_var = node("0.0", "variable_assignment", "NAME=${1:-World}", vec![expansion]);
        let old_var: &'static SemanticNode = Box::leak(Box::new(old_var));
        let new_var: &'static SemanticNode = Box::leak(Box::new(new_var));
        let exp_ref: &'static SemanticNode = &new_var.children[0];
        let mk = |ct: &'static str,
                  o: Option<&'static SemanticNode>,
                  n: Option<&'static SemanticNode>|
         -> ChangeDraft<'static> {
            ChangeDraft {
                change_type: ct,
                old_node: o,
                new_node: n,
                old_index: None,
                new_index: None,
                confidence: 1.0,
                description: String::new(),
                refactoring_kind: None,
                text_diff: None,
            }
        };

        let mut changes = vec![
            mk("MODIFICATION", Some(old_var), Some(new_var)),
            mk("ADDITION", None, Some(exp_ref)),
        ];
        suppress_statement_container_descendant_noise_drafts(&mut changes, "bash");
        assert_eq!(changes.len(), 1, "the descendant expansion ADDITION is folded away");
        assert_eq!(changes[0].change_type, "MODIFICATION");

        // No-op for a non-statement-profile language.
        let mut other = vec![
            mk("MODIFICATION", Some(old_var), Some(new_var)),
            mk("ADDITION", None, Some(exp_ref)),
        ];
        suppress_statement_container_descendant_noise_drafts(&mut other, "go");
        assert_eq!(other.len(), 2);
    }
    #[test]
    fn puppet_new_parameter_addition_uses_enriched_variable_label() {
        // Oracle (issue #39): a puppet `parameter` carries an empty label from the parser; label
        // enrichment fills it from the `variable` child, and the change augmentation surfaces the
        // new class parameter as an ADDITION labelled by that identity (not the parameter_list
        // scaffold).
        let var = node("0.0.0.0.1", "variable", "message", Vec::new());
        let param = node("0.0.0.0", "parameter", "", vec![var]);
        let plist = node("0.0.0", "parameter_list", "parameter_list", vec![param]);
        let new_class = node("0.0", "class_definition", "greeting", vec![plist]);
        let mut new_tree = node("0", "source_file", "source_file", vec![new_class]);
        let old_class = node("0.0", "class_definition", "greeting", Vec::new());
        let old_tree = node("0", "source_file", "source_file", vec![old_class]);

        enrich_resource_profile_labels(&mut new_tree, "puppet");
        let mut drafts: Vec<ChangeDraft> = Vec::new();
        augment_resource_profile_changes_drafts(&mut drafts, &old_tree, &new_tree, "puppet");
        assert!(
            drafts.iter().any(|change| change.change_type == "ADDITION"
                && change
                    .new_node
                    .is_some_and(|node| node.node_type == "parameter" && node.label == "message")),
            "expected an ADDITION for parameter 'message', got: {:?}",
            drafts
                .iter()
                .map(|c| (c.change_type, c.new_node.map(|n| (n.node_type.as_str(), n.label.as_str()))))
                .collect::<Vec<_>>()
        );
    }
    #[test]
    fn return_literal_kind_classifies_scalar_constants() {
        // Oracle (issue #68/#69): a return of a scalar constant reports its KIND (never the
        // value) so the explainer can say "returns a constant integer" instead of inventing
        // computation; a computed or multi-value return is not a literal.
        let ret = |children: Value| -> Option<&'static str> {
            let node: CstNode = serde_json::from_value(json!({
                "type": "return_statement",
                "children": children,
            }))
            .unwrap();
            cst_return_literal_kind(&node)
        };
        assert_eq!(ret(json!([{"type": "integer"}])), Some("int"));
        assert_eq!(ret(json!([{"type": "float"}])), Some("float"));
        assert_eq!(ret(json!([{"type": "string"}])), Some("str"));
        assert_eq!(ret(json!([{"type": "true"}])), Some("bool"));
        assert_eq!(ret(json!([{"type": "none"}])), Some("none"));
        // Computed values are not literals.
        assert_eq!(ret(json!([{"type": "call"}])), None);
        assert_eq!(ret(json!([{"type": "binary_operator"}])), None);
        assert_eq!(ret(json!([{"type": "identifier"}])), None);
        // A multi-value (tuple) return is not a scalar literal.
        assert_eq!(ret(json!([{"type": "integer"}, {"type": "integer"}])), None);
    }
    #[test]
    fn coupling_facts_count_calls_and_detect_recursion() {
        // Oracle (issue #69-J): fan-out count + self-recursion, in BOTH producers. CST path:
        // `def fact(n): return fact(n - 1) + call2()` -> 2 calls, recursive (callee == fn name).
        let cst: CstNode = serde_json::from_value(json!({
            "type": "function_definition",
            "children": [
                {"type": "identifier", "text": "fact"},
                {"type": "parameters", "children": [{"type": "identifier", "text": "n"}]},
                {"type": "block", "children": [
                    {"type": "return_statement", "children": [
                        {"type": "binary_operator", "children": [
                            {"type": "call", "children": [
                                {"type": "identifier", "text": "fact"},
                                {"type": "argument_list"}
                            ]},
                            {"type": "call", "children": [
                                {"type": "identifier", "text": "call2"},
                                {"type": "argument_list"}
                            ]}
                        ]}
                    ]}
                ]}
            ]
        }))
        .unwrap();
        let f = python_node_facts_value(&cst).expect("fn facts");
        assert_eq!(f["call_count"], 2);
        assert_eq!(f["recursive"], true);

        // Language-agnostic path: a js-shaped `function walk(){ walk(); }` -> recursive, 1 call.
        let call = node(
            "0.0.0.0",
            "call_expression",
            "walk",
            vec![node("0.0.0.0.0", "identifier", "walk", Vec::new())],
        );
        let stmt = node("0.0.0", "expression_statement", "", vec![call]);
        let body = node("0.0", "statement_block", "block", vec![stmt]);
        let func = node("0", "function_declaration", "walk", vec![body]);
        let jf = derive_node_facts(&func).expect("facts");
        assert_eq!(jf["call_count"], 1);
        assert_eq!(jf["recursive"], true);

        // A non-recursive helper: `def g(): return other()` -> 1 call, not recursive.
        let cst2: CstNode = serde_json::from_value(json!({
            "type": "function_definition",
            "children": [
                {"type": "identifier", "text": "g"},
                {"type": "parameters", "children": []},
                {"type": "block", "children": [
                    {"type": "return_statement", "children": [
                        {"type": "call", "children": [
                            {"type": "identifier", "text": "other"},
                            {"type": "argument_list"}
                        ]}
                    ]}
                ]}
            ]
        }))
        .unwrap();
        let g = python_node_facts_value(&cst2).expect("fn facts");
        assert_eq!(g["call_count"], 1);
        assert!(g.get("recursive").is_none());
    }
    #[test]
    fn python_param_kinds_count_optional_keyword_only_and_variadic() {
        // Oracle (issue #69 catalog C): `def f(a, b=1, *args, c, d=2, **kwargs)` — counts of
        // params with defaults and keyword-only params, plus variadic/kwargs flags. Names never
        // appear.
        let node: CstNode = serde_json::from_value(json!({
            "type": "function_definition",
            "children": [
                {"type": "identifier", "text": "f"},
                {"type": "parameters", "children": [
                    {"type": "identifier", "text": "a"},
                    {"type": "default_parameter", "children": [
                        {"type": "identifier"}, {"type": "integer"}
                    ]},
                    {"type": "list_splat_pattern", "children": [{"type": "identifier"}]},
                    {"type": "identifier", "text": "c"},
                    {"type": "default_parameter", "children": [
                        {"type": "identifier"}, {"type": "integer"}
                    ]},
                    {"type": "dictionary_splat_pattern", "children": [{"type": "identifier"}]}
                ]},
                {"type": "block", "children": [
                    {"type": "return_statement", "children": [{"type": "identifier"}]}
                ]}
            ]
        }))
        .unwrap();
        let f = python_node_facts_value(&node).expect("fn facts");
        assert_eq!(f["default_count"], 2); // b=1, d=2
        assert_eq!(f["keyword_only_count"], 2); // c and d are after *args
        assert_eq!(f["has_variadic"], true);
        assert_eq!(f["has_kwargs"], true);

        // A plain `def g(a, b)` reports none of the kind facts.
        let plain: CstNode = serde_json::from_value(json!({
            "type": "function_definition",
            "children": [
                {"type": "identifier", "text": "g"},
                {"type": "parameters", "children": [
                    {"type": "identifier", "text": "a"}, {"type": "identifier", "text": "b"}
                ]},
                {"type": "block", "children": [{"type": "pass_statement"}]}
            ]
        }))
        .unwrap();
        let g = python_node_facts_value(&plain).expect("fn facts");
        assert_eq!(g["param_count"], 2);
        assert!(g.get("default_count").is_none());
        assert!(g.get("has_variadic").is_none());
    }
    #[test]
    fn python_class_facts_report_shape_and_kind() {
        // Oracle (issue #69 catalog D): a class definition yields method/field/base counts and a
        // kind (enum/exception) from its OWN children — counts + booleans only, never a name.
        let facts = |v: Value| -> Value {
            let node: CstNode = serde_json::from_value(v).unwrap();
            python_node_facts_value(&node).expect("class facts")
        };

        // `class Color(Enum): RED = 1; GREEN = 2` -> an enum, one base, two fields, no methods.
        let color = facts(json!({
            "type": "class_definition",
            "children": [
                {"type": "identifier", "text": "Color"},
                {"type": "argument_list", "children": [
                    {"type": "(", "text": "("},
                    {"type": "identifier", "text": "Enum"},
                    {"type": ")", "text": ")"}
                ]},
                {"type": "block", "children": [
                    {"type": "expression_statement", "children": [{"type": "assignment"}]},
                    {"type": "expression_statement", "children": [{"type": "assignment"}]}
                ]}
            ]
        }));
        assert_eq!(color["is_enum"], true);
        assert_eq!(color["base_count"], 1);
        assert_eq!(color["field_count"], 2);
        assert_eq!(color["method_count"], 0);
        assert!(color.get("is_exception").is_none());

        // `class ParseError(ValueError): def __init__(self): ...` -> exception (base ends "Error").
        let err = facts(json!({
            "type": "class_definition",
            "children": [
                {"type": "identifier", "text": "ParseError"},
                {"type": "argument_list", "children": [
                    {"type": "identifier", "text": "ValueError"}
                ]},
                {"type": "block", "children": [
                    {"type": "function_definition", "children": []}
                ]}
            ]
        }));
        assert_eq!(err["is_exception"], true);
        assert_eq!(err["method_count"], 1);
        assert!(err.get("is_enum").is_none());

        // A plain class: a field + a plain method + a decorated method -> 2 methods, 1 field, no base.
        let plain = facts(json!({
            "type": "class_definition",
            "children": [
                {"type": "identifier", "text": "Widget"},
                {"type": "block", "children": [
                    {"type": "expression_statement", "children": [{"type": "assignment"}]},
                    {"type": "function_definition", "children": []},
                    {"type": "decorated_definition", "children": [
                        {"type": "decorator"},
                        {"type": "function_definition", "children": []}
                    ]}
                ]}
            ]
        }));
        assert_eq!(plain["method_count"], 2);
        assert_eq!(plain["field_count"], 1);
        assert!(plain.get("base_count").is_none());
    }
    #[test]
    fn cross_language_class_facts_count_methods() {
        // Oracle (issue #69 catalog D + #70): a non-Python class (js-shaped) yields method_count
        // over the pruned SemanticNode tree — methods nested one level under the class body.
        let m1 = node("0.0.0", "method_definition", "x", Vec::new());
        let m2 = node("0.0.1", "method_definition", "y", Vec::new());
        let body = node("0.0", "class_body", "class_body", vec![m1, m2]);
        let class = node("0", "class_declaration", "Point", vec![body]);
        let facts = derive_node_facts(&class).expect("class facts");
        assert_eq!(facts["method_count"], 2);

        // Methods as direct children (no body container) are counted too; an empty class -> 0.
        let direct = node(
            "1",
            "class_declaration",
            "Empty",
            vec![node("1.0", "method_declaration", "m", Vec::new())],
        );
        assert_eq!(derive_node_facts(&direct).expect("facts")["method_count"], 1);
        let empty = node("2", "class_declaration", "Bare", Vec::new());
        assert_eq!(derive_node_facts(&empty).expect("facts")["method_count"], 0);
    }
    #[test]
    fn decorator_facts_flag_behavior_and_mirror_to_wrapper() {
        // Oracle (issue #69 catalog C/D): decorators live on the `decorated_definition` WRAPPER;
        // enrich folds their behavior flags into the inner def AND mirrors them onto the wrapper,
        // reading the decorator name only to set a boolean (never emitting it).
        let deco = node(
            "0.0",
            "decorator",
            "decorator",
            vec![node("0.0.0", "identifier", "property", Vec::new())],
        );
        let inner = node("0.1", "function_definition", "x", Vec::new());
        let mut wrapper = node("0", "decorated_definition", "x", vec![deco, inner]);
        enrich_tree_facts(&mut wrapper);
        let inner_facts = wrapper
            .children
            .iter()
            .find(|c| c.node_type == "function_definition")
            .and_then(|c| c.facts.as_ref())
            .expect("inner facts");
        assert_eq!(inner_facts["is_property"], true);
        assert_eq!(inner_facts["decorator_count"], 1);
        assert_eq!(wrapper.facts.as_ref().unwrap()["is_property"], true);

        // `@lru_cache()` (call form) on a method -> cached; callee name reached through the call.
        let call = node(
            "1.0.0",
            "call",
            "call",
            vec![node("1.0.0.0", "identifier", "lru_cache", Vec::new())],
        );
        let mut w2 = node(
            "1",
            "decorated_definition",
            "get",
            vec![
                node("1.0", "decorator", "decorator", vec![call]),
                node("1.1", "function_definition", "get", Vec::new()),
            ],
        );
        enrich_tree_facts(&mut w2);
        assert_eq!(w2.facts.as_ref().unwrap()["is_cached"], true);

        // `@dataclass` on a class -> is_dataclass, mirrored onto the wrapper.
        let mut w3 = node(
            "2",
            "decorated_definition",
            "Point",
            vec![
                node(
                    "2.0",
                    "decorator",
                    "decorator",
                    vec![node("2.0.0", "identifier", "dataclass", Vec::new())],
                ),
                node("2.1", "class_definition", "Point", Vec::new()),
            ],
        );
        enrich_tree_facts(&mut w3);
        assert_eq!(w3.facts.as_ref().unwrap()["is_dataclass"], true);
    }
    #[test]
    fn cross_language_facts_derive_from_the_semantic_tree() {
        // Oracle (issue #70): the SAME privacy-safe facts as Python, derived from a non-Python
        // (js-shaped) SemanticNode tree — no per-parser work. `function ccc(){ call(); return 99 }`.
        let number = node("0.1.1.0", "number", "99", Vec::new());
        let ret = node("0.1.1", "return_statement", "return_statement", vec![number]);
        let call = node("0.1.0.0", "call_expression", "call_expression", Vec::new());
        let expr = node("0.1.0", "expression_statement", "expression_statement", vec![call]);
        let block = node("0.1", "statement_block", "statement_block", vec![expr, ret]);
        let params = node("0.0", "formal_parameters", "formal_parameters", Vec::new());
        let func = node("0", "function_declaration", "ccc", vec![params, block]);
        let facts = derive_node_facts(&func).expect("facts for a function entity");
        assert_eq!(facts["param_count"], 0);
        assert_eq!(facts["returns"], "literal");
        assert_eq!(facts["return_kind"], "number"); // JS `number` node — int/float indistinguishable
        assert_eq!(facts["side_effects"], true);

        // A return whose value was pruned (java) is honest: "value", never a false "none".
        let bare = node("1.0.0", "return_statement", "return_statement", Vec::new());
        let jblock = node("1.0", "block", "block", vec![bare]);
        let jmethod = node("1", "method_declaration", "ccc", vec![jblock]);
        let jfacts = derive_node_facts(&jmethod).expect("facts");
        assert_eq!(jfacts["returns"], "value");
        assert!(jfacts.get("return_kind").is_none());

        // A non-entity node yields nothing.
        assert!(derive_node_facts(&node("2", "if_statement", "if", Vec::new())).is_none());
    }
    #[test]
    fn control_flow_behavior_facts_classify_the_body() {
        // Oracle (issue #69-H): the body's control-flow shape, cross-grammar and privacy-safe.
        // A function that loops with an inner branch -> control_shape "looping".
        let inner_if = node("0.0.0.0", "if_statement", "if_statement", Vec::new());
        let for_stmt = node("0.0.0", "for_statement", "for_statement", vec![inner_if]);
        let body = node("0.0", "block", "block", vec![for_stmt]);
        let func = node("0", "function_declaration", "scan", vec![body]);
        let facts = derive_node_facts(&func).expect("facts");
        assert_eq!(facts["control_shape"], "looping");
        assert_eq!(facts["has_loop"], true);
        assert_eq!(facts["has_conditional"], true);

        // try/except is error handling, not a branch -> control_shape stays "linear".
        let try_stmt = node("1.0.0", "try_statement", "try_statement", Vec::new());
        let tbody = node("1.0", "block", "block", vec![try_stmt]);
        let tfunc = node("1", "function_declaration", "load", vec![tbody]);
        let tfacts = derive_node_facts(&tfunc).expect("facts");
        assert_eq!(tfacts["has_error_handling"], true);
        assert_eq!(tfacts["control_shape"], "linear");
        assert!(tfacts.get("has_loop").is_none());
    }
    #[test]
    fn behavior_category_rollup_classifies_purpose() {
        // Oracle (issue #69-H): a coarse "what kind of function" enum from the behavior flags.
        // args: returns_value, side_effects, has_conditional, has_loop, throws, mutates, constructs.
        assert_eq!(behavior_category(false, false, true, false, true, false, false), Some("validator"));
        assert_eq!(behavior_category(false, true, false, false, false, false, false), Some("io"));
        assert_eq!(behavior_category(true, false, false, false, false, false, false), Some("accessor"));
        assert_eq!(behavior_category(true, false, true, false, false, false, false), Some("transformer"));
        assert_eq!(behavior_category(true, false, false, true, false, false, false), Some("transformer"));
        // Constructs + returns a value -> factory (builds and hands back a new object/collection).
        assert_eq!(behavior_category(true, false, false, false, false, false, true), Some("factory"));
        // Mutates state and returns nothing -> mutator (a setter/in-place update).
        assert_eq!(behavior_category(false, false, false, false, false, true, false), Some("mutator"));
        // Nothing conclusive -> no category (never guess).
        assert_eq!(behavior_category(false, false, false, false, false, false, false), None);
        // A value returned WITH a side effect is neither a pure accessor nor pure io.
        assert_eq!(behavior_category(true, true, false, false, false, false, false), None);
    }
    #[test]
    fn has_computation_distinguishes_real_work_from_a_stub_that_only_calls_and_returns() {
        // Oracle (issue #69, catalog B — the #68 antidote): a substantive body that only calls out
        // and returns a constant does NO computation, so has_computation is an explicit `false` —
        // the signal that lets the explainer refuse to invent "performs some internal computation".
        let call = node("0.0.0.0", "call", "call", Vec::new());
        let expr = node("0.0.0", "expression_statement", "expression_statement", vec![call]);
        let lit = node("0.0.1.0", "integer", "integer", Vec::new());
        let ret = node("0.0.1", "return_statement", "return_statement", vec![lit]);
        let body = node("0.0", "block", "block", vec![expr, ret]);
        let ccc = node("0", "function_declaration", "ccc", vec![body]);
        let facts = derive_node_facts(&ccc).expect("facts");
        assert_eq!(facts["has_computation"], false);
        assert_eq!(facts["side_effects"], true);
        assert_eq!(facts["return_kind"], "int");

        // A body with a real operator computes — has_computation flips to true.
        let bin = node("1.0.0.0", "binary_expression", "binary_expression", Vec::new());
        let ret2 = node("1.0.0", "return_statement", "return_statement", vec![bin]);
        let body2 = node("1.0", "block", "block", vec![ret2]);
        let add = node("1", "function_declaration", "add", vec![body2]);
        let facts2 = derive_node_facts(&add).expect("facts");
        assert_eq!(facts2["has_computation"], true);

        // An empty body has nothing to assess -> the fact is omitted (never an uninformative false).
        let empty_body = node("2.0", "block", "block", Vec::new());
        let empty_fn = node("2", "function_declaration", "noop", vec![empty_body]);
        let facts3 = derive_node_facts(&empty_fn).expect("facts");
        assert!(facts3.get("has_computation").is_none());
    }
    #[test]
    fn statement_enrichment_recovers_bash_command_labels_and_rehashes() {
        // python statement_profiles.enrich_statement_profile_labels parity: a bash
        // command node with a weak label takes its compacted source span; the
        // structural hash is recomputed bottom-up (sha256(type   label {  child}*)).
        let name = node("0.0.0", "command_name", "echo", Vec::new());
        let mut cmd = node("0.0", "command", "command", vec![name]);
        cmd.position.end_col = 10; // span "echo hello" on line 0
        let root = module_with_nodes(vec![cmd]);
        let payload = enrich_profile_labels_json(
            &serde_json::to_string(&root).unwrap(),
            "echo hello
",
            "bash",
            None,
        )
        .unwrap();
        let enriched: SemanticNode = serde_json::from_str(&payload).unwrap();
        let cmd = &enriched.children[0];
        assert_eq!(cmd.label, "echo hello", "label from compacted source span");
        assert_eq!(
            cmd.structural_hash,
            synthetic_structural_hash("command", "echo hello", &cmd.children),
            "hash recomputed over the enriched label"
        );
        // Non-profile language: tree unchanged.
        let same = enrich_profile_labels_json(
            &serde_json::to_string(&root).unwrap(),
            "echo hello
",
            "python",
            None,
        )
        .unwrap();
        let same: SemanticNode = serde_json::from_str(&same).unwrap();
        assert_eq!(same.children[0].label, "command");
    }
    #[test]
    fn guardrail_semantic_paths_index_keyed_and_resource_languages() {
        // python guardrails._semantic_paths parity (set-wise): a keyed pair's path
        // applies to itself and every descendant; non-guardrail languages are empty.
        let value = node("0.0.0.1", "string", "secret", Vec::new());
        let key = node("0.0.0.0", "string", "api_key", Vec::new());
        let pair = node("0.0.0", "pair", "api_key", vec![key, value]);
        let object = node("0.0", "object", "object", vec![pair]);
        let root = module_with_nodes(vec![object]);
        let paths = guardrail_semantic_paths(&root, "json");
        let pair_paths = paths.get("0.0.0").expect("pair keyed");
        assert!(
            pair_paths.contains(&"api_key".to_string()),
            "pair carries its own path: {pair_paths:?}"
        );
        let value_paths = paths.get("0.0.0.1").expect("descendant covered");
        assert!(
            value_paths.contains(&"api_key".to_string()),
            "descendants inherit the container path: {value_paths:?}"
        );
        assert!(
            guardrail_semantic_paths(&root, "python").is_empty(),
            "non-guardrail languages have no semantic-path index"
        );
    }

#[test]
fn resource_enricher_fills_hcl_block_and_puppet_identity() {
    // hcl: `resource "aws_instance" "web" { ... }` — block identity from the
    // two string_lit children (readiness #90 resource port).
    let hcl: SemanticNode = serde_json::from_value(json!({
        "id": "0", "node_type": "block", "label": "resource", "structural_hash": "h",
        "position": {"start_line": 0, "start_col": 0, "end_line": 0, "end_col": 0},
        "children": [
            {"id": "0.0", "node_type": "string_lit", "label": "aws_instance", "structural_hash": "h",
             "position": {"start_line": 0, "start_col": 0, "end_line": 0, "end_col": 0}, "children": []},
            {"id": "0.1", "node_type": "string_lit", "label": "web", "structural_hash": "h",
             "position": {"start_line": 0, "start_col": 0, "end_line": 0, "end_col": 0}, "children": []},
            {"id": "0.2", "node_type": "body", "label": "body", "structural_hash": "h",
             "position": {"start_line": 0, "start_col": 0, "end_line": 0, "end_col": 0}, "children": []}
        ]
    })).expect("hcl tree parses");
    let enriched = enrich_resource_profile_labels_node(&hcl, "hcl");
    assert_eq!(enriched.label, "resource aws_instance web");

    // puppet: resource_declaration with a title string — identity is type + title,
    // quotes stripped, capped at 2 parts.
    let puppet: SemanticNode = serde_json::from_value(json!({
        "id": "0", "node_type": "resource_declaration", "label": "file", "structural_hash": "h",
        "position": {"start_line": 0, "start_col": 0, "end_line": 0, "end_col": 0},
        "children": [
            {"id": "0.0", "node_type": "title", "label": "'/tmp/x'", "structural_hash": "h",
             "position": {"start_line": 0, "start_col": 0, "end_line": 0, "end_col": 0}, "children": []},
            {"id": "0.1", "node_type": "attribute_list", "label": "attribute_list", "structural_hash": "h",
             "position": {"start_line": 0, "start_col": 0, "end_line": 0, "end_col": 0}, "children": []}
        ]
    })).expect("puppet tree parses");
    let enriched = enrich_resource_profile_labels_node(&puppet, "puppet");
    assert_eq!(enriched.label, "file /tmp/x");

    // A non-resource language is untouched.
    let untouched = enrich_resource_profile_labels_node(&hcl, "python");
    assert_eq!(untouched.label, "resource");
}
