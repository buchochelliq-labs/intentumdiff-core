use super::*;

fn leaf(id: &str, label: &str) -> SemanticNode {
    serde_json::from_value(json!({
        "id": id, "node_type": "string_fragment", "label": label,
        "position": {"start_line": 6, "start_col": 22,
                     "end_line": 6, "end_col": 22 + label.len()},
        "structural_hash": label, "children": []
    })).expect("valid semantic-node fixture")
}

fn modification<'a>(old: &'a SemanticNode, new: &'a SemanticNode) -> ChangeDraft<'a> {
    ChangeDraft {
        change_type: "MODIFICATION", old_node: Some(old), new_node: Some(new),
        old_index: None, new_index: None, confidence: 1.0,
        description: "Source literal changed".to_owned(),
        refactoring_kind: None, text_diff: None,
    }
}

fn raw_parent(old: &str, new: &str) -> (String, Option<String>, Option<String>, Vec<String>, Vec<String>) {
    ("MODIFICATION".to_owned(), Some(old.to_owned()), Some(new.to_owned()),
     vec!["Alice".to_owned()], vec!["AliceX".to_owned()])
}

#[test]
fn precise_literal_edit_does_not_relabel_its_removed_parents_as_style() {
    let old = leaf("0.1.3.0.0.1.1.0", "Alice");
    let new = leaf("0.1.3.0.0.1.1.0", "AliceX");
    let changes = vec![modification(&old, &new)];
    let before = vec![
        raw_parent("0.1.3.0.0.1.1", "0.1.3.0.0.1.1"),
        raw_parent("0.1.3.0", "0.1.3.0"),
    ];
    for language in ["javascript", "typescript", "tsx"] {
        assert!(js_style_group_from_suppression(&before, &changes, language).is_none(),
            "{language}: refining a semantic edit to its leaf is not formatting");
    }
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].new_node.unwrap().label, "AliceX");
}

#[test]
fn ancestor_coverage_requires_a_path_segment_boundary() {
    let old = leaf("0.10.0", "Alice");
    let new = leaf("0.10.0", "AliceX");
    let changes = vec![modification(&old, &new)];
    let before = vec![raw_parent("0.1", "0.1")];
    assert!(js_style_group_from_suppression(&before, &changes, "tsx").is_some(),
        "0.1 is not an ancestor of 0.10.0; unrelated residue must not be removed");
}

#[test]
fn ancestor_coverage_requires_both_sides_of_the_same_surviving_edit() {
    let old_a = leaf("0.1.0", "Alice");
    let new_a = leaf("0.9.0", "AliceX");
    let old_b = leaf("0.9.0", "Bob");
    let new_b = leaf("0.2.0", "BobX");
    let changes = vec![modification(&old_a, &new_a), modification(&old_b, &new_b)];
    let before = vec![raw_parent("0.1", "0.2")];
    assert!(js_style_group_from_suppression(&before, &changes, "tsx").is_some(),
        "unrelated old/new endpoints cannot certify parent refinement");
}

#[cfg(feature = "tier-c-wasm")]
mod native {
    use super::*;

    fn review(old: &str, new: &str, filename: &str) -> Value {
        let wasm = std::env::var("INTENTUMDIFF_TEST_WASM_DIR")
            .expect("the full gate requires checksum-verified parser components");
        // The Tier-C provisioner stages registry-verified Wasm, not the runtime
        // parser_manifest.json. Native live-diff selection is deliberately
        // fail-closed without that manifest, so build a minimal test-only
        // manifest pointing at the EXACT staged JS/TS component bytes.
        let staged = std::path::Path::new(&wasm).join("js_ts_parser.wasm");
        assert!(staged.is_file(), "verified JS/TS Wasm missing: {}", staged.display());
        let fixture = tempfile::tempdir().expect("isolated native parser fixture");
        std::fs::copy(&staged, fixture.path().join("js_ts_parser.wasm"))
            .expect("copy verified JS/TS parser into fixture");
        std::fs::write(
            fixture.path().join("parser_manifest.json"),
            json!({
                "schema_version": 1,
                "parsers": {
                    "javascript": {"plugin_id":"javascript", "wasm":"js_ts_parser.wasm", "extensions":[".js"]},
                    "typescript": {"plugin_id":"typescript", "wasm":"js_ts_parser.wasm", "extensions":[".ts"]},
                    "tsx": {"plugin_id":"tsx", "wasm":"js_ts_parser.wasm", "extensions":[".tsx"]}
                },
                "extension_index": {".js":"javascript", ".ts":"typescript", ".tsx":"tsx"}
            }).to_string(),
        ).expect("write minimal native parser manifest");
        let fixture_path = fixture.path().to_str().expect("UTF-8 fixture path");
        let result = crate::live_server::live_diff_contents_impl(
            env!("CARGO_MANIFEST_DIR"), filename, old, new, "{}", fixture_path,
        ).expect("native source review must succeed");
        let response: Value = serde_json::from_str(&result).expect("native JSON response");
        let diff = response.get("diff").unwrap_or_else(|| {
            panic!("native must not decline a verified JS/TS component: {response:#}")
        }).clone();
        assert_eq!(diff["is_fallback"], false);
        assert_eq!(diff["parse_errors"], json!([]));
        diff
    }

    #[test]
    fn tsx_literal_mutation_retains_exact_spans_without_false_style() {
        let old = "interface User {\n  id: number;\n  name: string;\n}\n\nfunction getUser(id: number): User {\n  return { id, name: \"Alice\" };\n}\n\nfunction formatName(user: User): string {\n  return user.name.toUpperCase();\n}\n";
        let diff = review(old, &old.replace("Alice", "AliceX"), "component.tsx");
        assert_eq!(diff["has_semantic_changes"], true);
        assert_eq!(diff["is_style_only"], false);
        let changes = diff["changes"].as_array().unwrap();
        assert_eq!(changes.len(), 1, "{diff:#}");
        assert_eq!(changes[0]["change_type"], "MODIFICATION");
        for (side, label, end_col) in [("old_node", "Alice", 27), ("new_node", "AliceX", 28)] {
            assert_eq!(changes[0][side]["label"], label);
            assert_eq!(changes[0][side]["position"], json!({
                "start_line": 6, "start_col": 22, "end_line": 6, "end_col": end_col
            }));
        }
        assert!(diff["metadata"].get("ignored_style_changes")
            .and_then(Value::as_array).is_none_or(Vec::is_empty), "{diff:#}");
        assert!(diff["change_groups"].as_array().unwrap().iter()
            .all(|g| g["kind"] != "IGNORED_STYLE"), "{diff:#}");
    }

    #[test]
    fn javascript_call_literal_mutation_is_not_argument_wrapping() {
        let old = "function f() { return foo(\"Alice\"); }\n";
        let diff = review(old, &old.replace("Alice", "AliceX"), "example.js");
        assert_eq!(diff["changes"].as_array().unwrap().len(), 1, "{diff:#}");
        assert_eq!(diff["changes"][0]["new_node"]["label"], "AliceX");
        assert!(diff["change_groups"].as_array().unwrap().iter()
            .all(|g| g["kind"] != "IGNORED_STYLE"), "{diff:#}");
    }

    #[test]
    fn genuine_javascript_wrapping_preserves_deletions_and_the_message_edit() {
        let old = r#"const fs = require('fs');
const zlib = require('zlib');
const { pipeline } = require('stream');
const { promisify } = require('util');

const asyncPipeline = promisify(pipeline);
  
async function compress() {
    const myReadStream = fs.createReadStream(__dirname + '/data.txt');
    const myWriteStream = fs.createWriteStream(__dirname + '/data.txt.gz');
    const transform = zlib.createGzip();

    console.log("foo", myReadStream);
    console.log("bar", myWriteStream);

    try {
        await asyncPipeline(myReadStream, transform, myWriteStream);
    } catch (err) {
        console.error('Oh no! :(', err);
   }
}

compress();"#;
        let new = r#"const fs = require('fs');
const zlib = require('zlib');
const { pipeline } = require('stream');
const { promisify } = require('util');

const asyncPipeline = promisify(pipeline);
  
async function compress() {
    const myReadStream = fs.createReadStream(
        __dirname + '/data.txt'
    );
    const myWriteStream = fs.createWriteStream(
        __dirname + '/data.txt.gz'
    );
    const transform = zlib.createGzip();

    try {
        await asyncPipeline(
            myReadStream, 
            transform,
            myWriteStream
        );
    } catch (err) {
        console.error('An error occurred:', err);
   }
}

compress();"#;
        let diff = review(old, new, "example.js");
        let changes = diff["changes"].as_array().unwrap();
        assert_eq!(changes.len(), 3, "{diff:#}");
        assert_eq!(changes.iter().filter(|c| c["change_type"] == "DELETION").count(), 2);
        for label in ["foo", "bar"] {
            let expected = format!("\"label\":\"{label}\"");
            assert!(changes.iter().any(|c| c["change_type"] == "DELETION"
                && c["old_node"].to_string().contains(&expected)), "{diff:#}");
        }
        let modification = changes.iter().find(|c| c["change_type"] == "MODIFICATION").unwrap();
        assert_eq!(modification["old_node"]["label"], "Oh no! :(");
        assert_eq!(modification["new_node"]["label"], "An error occurred:");
        assert!(diff["change_groups"].as_array().unwrap().iter().any(|g|
            g["kind"] == "IGNORED_STYLE"
                && g["rule_id"] == "javascript.formatting.call_argument_wrapping_equivalence"
                && !g["old_node_ids"].as_array().unwrap().is_empty()), "{diff:#}");
    }
}
