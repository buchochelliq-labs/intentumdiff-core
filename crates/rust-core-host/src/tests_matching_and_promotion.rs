// Split from tests_inline.rs (issue #85): one file per test family.
// Nested inside cfg(test) mod tests - `super::*` = the tests mod (helpers),
// `crate::*` = the engine.
#![allow(unused_imports)]
use super::*;
use crate::*;

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
    fn same_id_named_relabel_promotes_to_refactoring_rename_not_move() {
        // Oracle scenario (intentumdiff-diff-expectations / issue #10): a clean function rename
        // (greet -> welcome) — same structural id, same node type, same position, only the
        // label changed — is a REFACTORING rename, never a MOVE. The old behavior emitted
        // change_type "MOVE" here, surfacing the rename as MOVE + a redundant identifier
        // modification while the Python oracle collapses it to one change.
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

        assert_eq!(
            changes.len(),
            1,
            "add+delete must collapse to one change: {changes:?}"
        );
        assert_eq!(changes[0].change_type, "REFACTORING");
        assert_eq!(changes[0].refactoring_kind, Some("RENAME_SYMBOL"));
        assert_eq!(changes[0].old_node.unwrap().label, "greet");
        assert_eq!(changes[0].new_node.unwrap().label, "welcome");
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
        let (suppressed, promoted) = suppress_low_signal_reorders_drafts(&mut swapped);
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
        let (suppressed, promoted) = suppress_low_signal_reorders_drafts(&mut shifted);
        assert_eq!(suppressed, 2, "insertion shifts remain suppressed: {shifted:?}");
        assert!(promoted.is_empty());
        assert!(shifted.is_empty());
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
        fn callable(param_labels: &[&str]) -> SemanticNode {
            let params: Vec<SemanticNode> = param_labels
                .iter()
                .enumerate()
                .map(|(i, l)| node(&format!("0.0.2.{i}"), "formal_parameter", l, Vec::new()))
                .collect();
            let plist = node("0.0.2", "formal_parameter_list", "formal_parameter_list", params);
            node("0", "module", "module", vec![node("0.0", "function_signature", "add", vec![plist])])
        }
        let old_root: &'static SemanticNode = Box::leak(Box::new(callable(&["a", "b"])));
        let new_root: &'static SemanticNode = Box::leak(Box::new(callable(&["x", "y"])));

        let body_a: &'static SemanticNode = Box::leak(Box::new(node("9.0", "identifier", "a", Vec::new())));
        let body_x: &'static SemanticNode = Box::leak(Box::new(node("9.1", "identifier", "x", Vec::new())));
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
        let swap_new: &'static SemanticNode = Box::leak(Box::new(callable(&["b", "a"])));
        let sb: &'static SemanticNode = Box::leak(Box::new(node("8.0", "identifier", "a", Vec::new())));
        let sb2: &'static SemanticNode = Box::leak(Box::new(node("8.1", "identifier", "b", Vec::new())));
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
