//! Regressions for changed parser payload absent from its exported labels.
use super::*;

fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!(
        "../../../tests/fixtures/language_assessment/data_config.json"
    ))
    .unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}

fn case(id: &str) -> Value {
    cases().into_iter().find(|c| c["id"] == id).unwrap()
}

fn review(case: &Value) -> Value {
    let language = case["language_id"].as_str().unwrap();
    let wasm = PathBuf::from(
        std::env::var("INTENTUMDIFF_TEST_WASM_DIR").expect("verified parser components"),
    )
    .join(format!("{language}_parser.wasm"));
    native_wasm_single_diff(
        language,
        wasm.to_str().unwrap(),
        case["old"].as_str().unwrap(),
        case["new"].as_str().unwrap(),
        case["filename"].as_str().unwrap(),
        case["filename"].as_str().unwrap(),
        "{}",
        None,
    )
    .unwrap()
}

fn meaningful(id: &str) {
    let input = case(id);
    let language = input["language_id"].as_str().unwrap();
    let wasm = PathBuf::from(std::env::var("INTENTUMDIFF_TEST_WASM_DIR").unwrap())
        .join(format!("{language}_parser.wasm"));
    let pair = run_python_wasm_process_pair_detailed(
        wasm.to_str().unwrap(),
        input["old"].as_str().unwrap(),
        "",
        input["filename"].as_str().unwrap(),
        input["new"].as_str().unwrap(),
        "",
        input["filename"].as_str().unwrap(),
        200_000_000,
        16 * 1024 * 1024,
        language,
    )
    .unwrap();
    let old: SemanticNode = serde_json::from_str(&pair.old_tree).unwrap();
    let new: SemanticNode = serde_json::from_str(&pair.new_tree).unwrap();
    assert_ne!(
        old.structural_hash, new.structural_hash,
        "{id}: raw parser evidence"
    );
    assert_eq!(
        whitespace_normalized_tree_hash(&old),
        whitespace_normalized_tree_hash(&new),
        "{id}: lossy exported projection"
    );
    assert_eq!(
        review_trees_equivalent_impl(&pair.old_tree, &pair.new_tree).unwrap(),
        "false",
        "{id}: a source-less projection cannot discharge opaque hash evidence"
    );
    let enriched_old: SemanticNode = serde_json::from_str(
        &enrich_profile_labels_impl(
            &pair.old_tree,
            input["old"].as_str().unwrap(),
            language,
            None,
        )
        .unwrap(),
    )
    .unwrap();
    let enriched_new: SemanticNode = serde_json::from_str(
        &enrich_profile_labels_impl(
            &pair.new_tree,
            input["new"].as_str().unwrap(),
            language,
            None,
        )
        .unwrap(),
    )
    .unwrap();
    assert_ne!(
        enriched_old.structural_hash, enriched_new.structural_hash,
        "{id}: identity enrichment must preserve opaque parser evidence"
    );
    let diff = review(&input);
    assert_eq!(diff["parse_errors"], json!([]), "{id}: {diff:#}");
    assert_eq!(diff["is_fallback"], false, "{id}: {diff:#}");
    assert_eq!(diff["has_semantic_changes"], true, "{id}: {diff:#}");
    assert_eq!(diff["is_style_only"], false, "{id}: {diff:#}");
    assert!(
        !diff["changes"].as_array().unwrap().is_empty(),
        "{id}: {diff:#}"
    );
    assert!(
        diff["changes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|change| change["text_diff"]
                .as_str()
                .is_some_and(|text| !text.is_empty() && text.contains(" -> "))),
        "{id}: missing inspectable source payload: {diff:#}"
    );
    let (before, after) = input["old"]
        .as_str()
        .unwrap()
        .lines()
        .zip(input["new"].as_str().unwrap().lines())
        .find(|(a, b)| a != b)
        .unwrap();
    let prefix = before
        .chars()
        .zip(after.chars())
        .take_while(|(a, b)| a == b)
        .map(|(c, _)| c.len_utf8())
        .sum::<usize>();
    let suffix = before[prefix..]
        .chars()
        .rev()
        .zip(after[prefix..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(c, _)| c.len_utf8())
        .sum::<usize>();
    let before_payload = &before[prefix..before.len() - suffix];
    let after_payload = &after[prefix..after.len() - suffix];
    assert!(
        diff["changes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|change| change["text_diff"]
                .as_str()
                .is_some_and(|text| text.contains(before_payload) && text.contains(after_payload))),
        "{id}: evidence does not retain the actual changed source payload: {diff:#}"
    );
    assert!(
        diff["change_groups"]
            .as_array()
            .unwrap()
            .iter()
            .all(|g| g["rule_id"] != "generic.style_only_shortcut.source_equivalence"),
        "{id}: {diff:#}"
    );
}

fn source_case(language: &str, filename: &str, old: &str, new: &str) -> Value {
    json!({"language_id":language,"filename":filename,"old":old,"new":new})
}

#[test]
fn semantic_payload_source_hash_delta_alone_does_not_create_an_edit() {
    // Some components hash raw source. An opaque hash delta still needs changed
    // payload tokens; this unit isolates that contract from guest hash choices.
    for (old, new) in [
        (
            "option java_package = \"example\";",
            "option java_package  =  \"example\" ;",
        ),
        (
            "option java_package = \"example\"; // old comment",
            "option java_package = \"example\"; // new comment",
        ),
    ] {
        let tree = |source: &str| {
            json!({
                "id":"0", "node_type":"option", "label":"option", "children":[],
                "position":{"start_line":0,"start_col":0,"end_line":0,"end_col":source.len()},
                "structural_hash":format!("{:x}",Sha256::digest(source.as_bytes()))
            })
        };
        let a = tree(old).to_string();
        let b = tree(new).to_string();
        assert_eq!(review_trees_equivalent_impl(&a, &b).unwrap(), "false");
        let fin: Value =
            serde_json::from_str(&finalize_review_impl(&a, &b, old, new, "proto", "{}").unwrap())
                .unwrap();
        assert_eq!(fin["changes"], json!([]), "{fin:#}");
        assert_eq!(fin["is_style_only"], true, "{fin:#}");
        assert_eq!(fin["no_surviving_changes"], false, "{fin:#}");
    }
}

#[test]
fn semantic_payload_internal_layout_and_quote_spelling_remain_style_only() {
    for input in [
        source_case(
            "proto",
            "edit.proto",
            "syntax = \"proto3\";\noption java_package = \"example.λ\";\n",
            "syntax = \"proto3\";\noption java_package  =  \"example.λ\" ;\n",
        ),
        source_case(
            "puppet",
            "edit.pp",
            "class greeting (String $message = 'hello') {}\n",
            "class greeting (String  $message  =  'hello') {}\n",
        ),
        source_case(
            "toml",
            "edit.toml",
            "[servers.alpha]\nip = \"127.0.0.1\"\n",
            "[ servers.alpha ]\nip  =  \"127.0.0.1\"\n",
        ),
        source_case(
            "make",
            "edit.mk",
            "LABEL = \"#old // comment\"\n",
            "LABEL = \"#new // comment\"\n",
        ),
        source_case(
            "proto",
            "edit.proto",
            "syntax = \"proto3\";\noption java_package = \"example\";\n",
            "syntax = \"proto3\";\noption java_package = 'example';\n",
        ),
        source_case(
            "toml",
            "edit.toml",
            "[servers.\"alpha\"]\nip = \"127.0.0.1\"\n",
            "[servers.'alpha']\nip = \"127.0.0.1\"\n",
        ),
        source_case(
            "puppet",
            "edit.pp",
            "class greeting (Enum['hello'] $message = 'hello') {}\n",
            "class greeting (Enum[\"hello\"] $message = 'hello') {}\n",
        ),
    ] {
        let diff = review(&input);
        assert_eq!(diff["parse_errors"], json!([]), "{input}: {diff:#}");
        assert_eq!(diff["has_semantic_changes"], false, "{input}: {diff:#}");
        assert_eq!(diff["is_style_only"], true, "{input}: {diff:#}");
        assert!(
            diff["changes"].as_array().unwrap().is_empty(),
            "{input}: {diff:#}"
        );
    }
}

#[test]
fn semantic_payload_make_urls_spaces_and_comment_markers_are_data() {
    for input in [
        source_case(
            "make",
            "edit.mk",
            "URL = http://old.example/path\n",
            "URL = http://new.example/path\n",
        ),
        source_case(
            "make",
            "edit.mk",
            "LABEL = \"old // value\"\n",
            "LABEL = \"new // value\"\n",
        ),
        source_case(
            "make",
            "edit.mk",
            "LABEL = old\\#value\n",
            "LABEL = new\\#value\n",
        ),
        source_case("make", "edit.mk", "LABEL = one two\n", "LABEL = one  two\n"),
        source_case(
            "make",
            "edit.mk",
            "FIRST = gcc\nSECOND = clang\nCC = $(FIRST)\nall:\n\t@echo $(CC)\n",
            "FIRST = gcc\nSECOND = clang\nCC = $(SECOND)\nall:\n\t@echo $(CC)\n",
        ),
        source_case(
            "proto",
            "edit.proto",
            "syntax = \"proto3\";\noption java_package = \"http://old#value\";\n",
            "syntax = \"proto3\";\noption java_package = \"http://new#value\";\n",
        ),
        source_case(
            "toml",
            "edit.toml",
            "[servers.\"old#//value\"]\nip = \"127.0.0.1\"\n",
            "[servers.\"new#//value\"]\nip = \"127.0.0.1\"\n",
        ),
        source_case(
            "proto",
            "edit.proto",
            "syntax = \"proto3\";\noption java_package = \"example.\\u0061\";\n",
            "syntax = \"proto3\";\noption java_package = \"example.\\u0062\";\n",
        ),
        source_case(
            "toml",
            "edit.toml",
            "[servers.\"\\u0061\"]\nip = \"127.0.0.1\"\n",
            "[servers.\"\\u0062\"]\nip = \"127.0.0.1\"\n",
        ),
    ] {
        let diff = review(&input);
        assert_eq!(diff["parse_errors"], json!([]), "{input}: {diff:#}");
        assert_eq!(diff["is_fallback"], false, "{input}: {diff:#}");
        assert_eq!(diff["has_semantic_changes"], true, "{input}: {diff:#}");
        assert_eq!(diff["is_style_only"], false, "{input}: {diff:#}");
        assert!(
            !diff["changes"].as_array().unwrap().is_empty(),
            "{input}: {diff:#}"
        );
    }
}

#[test]
fn semantic_payload_parent_gap_survives_child_quote_spelling() {
    for (input, before, after) in [
        (
            source_case(
                "toml",
                "edit.toml",
                "[servers.alpha]\nip = \"127.0.0.1\"\n",
                "[servers.primary]\nip = '127.0.0.1'\n",
            ),
            "[servers.alpha]",
            "[servers.primary]",
        ),
        (
            source_case(
                "puppet",
                "edit.pp",
                "class greeting (String $message = 'hello') {}\n",
                "class greeting (Optional[String] $message = \"hello\") {}\n",
            ),
            "String $message",
            "Optional[String] $message",
        ),
    ] {
        let diff = review(&input);
        assert_eq!(diff["parse_errors"], json!([]), "{diff:#}");
        assert_eq!(diff["has_semantic_changes"], true, "{diff:#}");
        assert!(
            diff["changes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|change| change["text_diff"]
                    .as_str()
                    .is_some_and(|text| text.contains(before) && text.contains(after))),
            "opaque parent payload is missing beside a child quote edit: {diff:#}"
        );
    }
}

#[test]
fn semantic_payload_make_compiler_value() {
    meaningful("data_config.make.meaningful_1_value");
}
#[test]
fn semantic_payload_make_unicode_value() {
    meaningful("data_config.make.unicode_value");
}
#[test]
fn semantic_payload_proto_unicode_option() {
    meaningful("data_config.proto.unicode_value");
}
#[test]
fn semantic_payload_puppet_parameter_type() {
    meaningful("data_config.puppet.meaningful_2_type");
}
#[test]
fn semantic_payload_toml_nested_table_identity() {
    meaningful("data_config.toml.external_meaningful_3_configuration");
}

#[test]
fn semantic_payload_preserves_assessed_formatting_comments_and_identity() {
    for input in cases().into_iter().filter(|c| {
        ["make", "proto", "puppet", "toml"].contains(&c["language_id"].as_str().unwrap())
            && (c["id"].as_str().unwrap().ends_with("identity")
                || c["id"].as_str().unwrap().ends_with("safe_preservation"))
    }) {
        let diff = review(&input);
        assert_eq!(diff["parse_errors"], json!([]), "{}: {diff:#}", input["id"]);
        assert_eq!(
            diff["has_semantic_changes"], false,
            "{}: {diff:#}",
            input["id"]
        );
        assert!(
            diff["changes"].as_array().unwrap().is_empty(),
            "{}: {diff:#}",
            input["id"]
        );
    }
}
