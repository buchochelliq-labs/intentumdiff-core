use super::*;

fn yaml_diff(old: &str, new: &str) -> Value {
    let diff = native_wasm_single_diff(
        "yaml",
        &bundled_wasm_path("yaml_parser.wasm"),
        old,
        new,
        "external.yml",
        "external.yml",
        "{}",
        None,
    )
    .expect("real YAML component and production finalization must succeed");
    assert_eq!(diff["parse_errors"], json!([]), "{diff}");
    assert_eq!(diff["is_fallback"], false, "{diff}");
    diff
}

#[test]
fn yaml_representation_proof_requires_complete_typed_values() {
    for (old, new) in [
        ("[baz, baz]", "[baz, qux, baz]"),
        ("[baz, baz]", "[baz]"),
        ("[baz, qux, baz]", "[baz, baz, qux]"),
        ("{foo: bar}", "{foo: qux}"),
        ("{foo: bar}", "{qux: bar}"),
        ("{value: 2}", "{value: '2'}"),
        ("{2: value}", "{'2': value}"),
        ("{foo: one, foo: two}", "{foo: two}"),
        ("{foo: one, foo: two}", "{foo: one, foo: two}"),
        ("[broken", "[broken"),
        ("", "null\n"),
        ("# comment\n", "null\n"),
        ("---\n", "null\n"),
        ("---\none\n---\ntwo\n", "---\none\n---\ntwo\n"),
        ("value: &first thing", "value: &second thing"),
        ("value: &same thing", "value: &same thing"),
        ("value: !tag thing", "value: !tag thing"),
        ("value: *alias", "value: *alias"),
        ("%YAML 1.1\n---\nvalue: yes", "%YAML 1.1\n---\nvalue: 'yes'"),
    ] {
        assert!(
            !yaml_representation_equivalent_sources(old, new),
            "unproven: {old:?} -> {new:?}"
        );
        // Generic wrapper drafts must survive a failed proof even when they themselves have
        // no visible identity tokens or payload children.
        let old_node = node("0", "block_node", "block_node", Vec::new());
        let new_node = node("0", "flow_node", "flow_node", Vec::new());
        let mut drafts = vec![container_guard_draft(
            "MODIFICATION",
            Some(&old_node),
            Some(&new_node),
        )];
        assert!(suppress_yaml_representation_equivalent_drafts(&mut drafts, old, new).is_none());
        assert_eq!(drafts.len(), 1);
    }
    assert!(yaml_representation_equivalent_sources(
        "foo: bar\nvalues:\n- baz\n- baz\n",
        "{foo: bar, values: [baz, baz]}"
    ));
}

#[test]
fn yaml_representation_proof_rejects_rounded_float_values_and_keys() {
    for (old, new) in [
        ("value: 9007199254740992.0", "value: 9007199254740993.0"),
        ("9007199254740992.0: value", "9007199254740993.0: value"),
    ] {
        // Demonstrate the exact collision that makes f64-based equality insufficient.
        assert_eq!(
            serde_yaml::from_str::<serde_yaml::Value>(old).unwrap(),
            serde_yaml::from_str::<serde_yaml::Value>(new).unwrap()
        );
        assert!(!yaml_representation_equivalent_sources(old, new));
    }
    assert!(!yaml_representation_equivalent_sources(
        "value:\n- 1.5\n",
        "value: [1.5]\n"
    ));
}

#[test]
fn yaml_representation_proof_rejects_numeric_string_fallbacks() {
    let binary = format!("0b1{}", "0".repeat(128));
    for number in [
        "0x100000000000000000000000000000000",
        "-0x80000000000000000000000000000001",
        "0o4000000000000000000000000000000000000000000",
        binary.as_str(),
        "1e999",
        "1.0e999",
        "012",
    ] {
        for (old, new) in [
            (
                format!("value: {number}\n"),
                format!("{{value: '{number}'}}\n"),
            ),
            (
                format!("{number}: value\n"),
                format!("{{'{number}': value}}\n"),
            ),
        ] {
            assert_eq!(
                serde_yaml::from_str::<serde_yaml::Value>(&old).unwrap(),
                serde_yaml::from_str::<serde_yaml::Value>(&new).unwrap(),
                "decoder fallback: {number}"
            );
            assert!(
                !yaml_representation_equivalent_sources(&old, &new),
                "numeric type not proven: {old:?} -> {new:?}"
            );
        }
    }
}

#[test]
#[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged YAML component")]
fn yaml_representation_preserves_oversized_numeric_value_and_key_types() {
    for number in [
        "0x100000000000000000000000000000000",
        "0o4000000000000000000000000000000000000000000",
        "1e999",
        "1.0e999",
    ] {
        for (old, new) in [
            (
                format!("- {number}\n- baz\n"),
                format!("['{number}', baz]\n"),
            ),
            (
                format!("value: {number}\n"),
                format!("{{value: '{number}'}}\n"),
            ),
            (
                format!("{number}: value\n"),
                format!("{{'{number}': value}}\n"),
            ),
        ] {
            let diff = yaml_diff(&old, &new);
            assert_eq!(
                diff["has_semantic_changes"], true,
                "{old:?} -> {new:?}: {diff}"
            );
            assert!(!diff["changes"].as_array().unwrap().is_empty(), "{diff}");
        }
    }
}

#[test]
#[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged YAML component")]
fn yaml_representation_preserves_nested_duplicate_sequence_edits() {
    // core#279: the reviewed 6BCT-derived insertion was entirely erased as scaffolding.
    let old = "- foo: bar\n- - baz\n  - baz\n";
    let inserted = "- foo: bar\n- - baz\n  - qux\n  - baz\n";
    let (old_tree, new_tree) = run_python_wasm_process_pair(
        &bundled_wasm_path("yaml_parser.wasm"),
        old,
        "",
        "external.yml",
        inserted,
        "",
        "external.yml",
        u64::MAX,
        16 * 1024 * 1024,
        "yaml",
    )
    .unwrap();
    let old_tree = parse_semantic_tree_json(&old_tree).unwrap();
    let new_tree = parse_semantic_tree_json(&new_tree).unwrap();
    assert_ne!(old_tree.structural_hash, new_tree.structural_hash);
    assert_eq!(
        new_tree
            .descendants()
            .into_iter()
            .filter(|n| n.label == "qux")
            .count(),
        1
    );
    assert_eq!(
        new_tree
            .descendants()
            .into_iter()
            .filter(|n| n.label == "baz")
            .count(),
        2
    );
    for (name, new) in [
        ("assessment insertion", inserted),
        ("deletion", "- foo: bar\n- - baz\n"),
        ("replacement", "- foo: bar\n- - baz\n  - qux\n"),
        ("mapping payload", "- foo: qux\n- - baz\n  - baz\n"),
        ("mapping key", "- qux: bar\n- - baz\n  - baz\n"),
        ("mapping payload and restyle", "[{foo: qux}, [baz, baz]]\n"),
    ] {
        let diff = yaml_diff(old, new);
        assert_eq!(diff["has_semantic_changes"], true, "{name}: {diff}");
        assert_eq!(diff["is_style_only"], false, "{name}: {diff}");
        assert!(
            !diff["changes"].as_array().unwrap().is_empty(),
            "{name}: {diff}"
        );
    }
    let old = "- foo: bar\n- - baz\n  - qux\n  - baz\n";
    let new = "- foo: bar\n- - baz\n  - baz\n  - qux\n";
    let diff = yaml_diff(old, new);
    assert_eq!(
        diff["has_semantic_changes"], true,
        "ordered sequence: {diff}"
    );
    assert!(!diff["changes"].as_array().unwrap().is_empty(), "{diff}");
}

#[test]
#[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged YAML component")]
fn yaml_representation_retains_true_block_flow_controls() {
    for (old, new) in [
        (
            "- foo: bar\n- - baz\n  - baz\n",
            "[{foo: bar}, [baz, baz]]\n",
        ),
        ("values:\n  - one\n  - two\n", "values: [one, two]\n"),
        ("foo: bar\ncount: 2\n", "{foo: bar, count: 2}\n"),
    ] {
        for (old, new) in [(old, new), (new, old)] {
            let diff = yaml_diff(old, new);
            assert_eq!(
                diff["has_semantic_changes"], false,
                "{old:?} -> {new:?}: {diff}"
            );
            assert!(diff["changes"].as_array().unwrap().is_empty(), "{diff}");
        }
    }
}

#[test]
#[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged YAML component")]
fn yaml_representation_preserves_identity_and_scalar_types() {
    for (old, new) in [
        ("value: 2\n", "value: \"2\"\n"),
        ("value: &first thing\n", "value: &second thing\n"),
        (
            "first: &a one\nsecond: &b two\nvalue: *a\n",
            "first: &a one\nsecond: &b two\nvalue: *b\n",
        ),
        ("value: !first thing\n", "value: !second thing\n"),
    ] {
        let diff = yaml_diff(old, new);
        assert_eq!(
            diff["has_semantic_changes"], true,
            "{old:?} -> {new:?}: {diff}"
        );
        assert!(!diff["changes"].as_array().unwrap().is_empty(), "{diff}");
    }
}
