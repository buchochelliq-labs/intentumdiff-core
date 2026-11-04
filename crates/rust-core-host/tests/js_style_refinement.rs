//! Source-judged controls for JavaScript refinement versus real formatting.
#![cfg(feature = "tier-c-wasm")]

use serde_json::{json, Value};

fn review(old: &str, new: &str) -> Value {
    let wasm = std::env::var("INTENTUMDIFF_TEST_WASM_DIR")
        .expect("the full gate requires checksum-verified parser components");
    // Tier-C provisioning supplies SHA-256-verified guest bytes, but not the
    // production package's parser_manifest.json. The live Rust entry point is
    // deliberately fail-closed without that manifest. Isolate a minimal,
    // explicit JS manifest in a temporary directory; never weaken selection
    // or fall back to a different parser when the staged component is missing.
    let staged = std::path::Path::new(&wasm).join("js_ts_parser.wasm");
    assert!(staged.is_file(), "verified Tier-C JS/TS component is missing: {}", staged.display());
    let temp = tempfile::tempdir().expect("isolated native parser test directory");
    std::fs::copy(&staged, temp.path().join("js_ts_parser.wasm"))
        .expect("copy exactly the staged, checksum-verified JS/TS component");
    let manifest = json!({
        "parsers": {
            "javascript": {
                "extensions": [".js"],
                "plugin_id": "javascript",
                "wasm": "js_ts_parser.wasm"
            }
        },
        "extension_index": {".js": "javascript"}
    });
    std::fs::write(temp.path().join("parser_manifest.json"), manifest.to_string())
        .expect("stage production-format parser manifest");
    let result = intentumdiff_rust_core::live_server::live_diff_contents_impl(
        env!("CARGO_MANIFEST_DIR"), "example.js", old, new, "{}",
        &temp.path().to_string_lossy(),
    ).expect("native source review must succeed");
    let response: Value = serde_json::from_str(&result).expect("native JSON response");
    let diff = response.get("diff").unwrap_or_else(|| {
        panic!("native parser returned no diff (fail-closed selection or engine error): {response:#}")
    }).clone();
    assert_eq!(diff["is_fallback"], false);
    assert_eq!(diff["parse_errors"], json!([]));
    diff
}

#[test]
fn javascript_call_literal_mutation_is_not_argument_wrapping() {
    let old = "function f() { return foo(\"Alice\"); }\n";
    let diff = review(old, &old.replace("Alice", "AliceX"));
    assert_eq!(diff["has_semantic_changes"], true);
    assert_eq!(diff["is_style_only"], false);
    assert_eq!(diff["changes"].as_array().unwrap().len(), 1, "{diff:#}");
    assert_eq!(diff["changes"][0]["change_type"], "MODIFICATION");
    assert_eq!(diff["changes"][0]["old_node"]["label"], "Alice");
    assert_eq!(diff["changes"][0]["new_node"]["label"], "AliceX");
    assert!(diff["metadata"].get("ignored_style_changes")
        .and_then(Value::as_array).is_none_or(Vec::is_empty), "{diff:#}");
    assert!(diff["change_groups"].as_array().unwrap().iter()
        .all(|g| g["kind"] != "IGNORED_STYLE"), "{diff:#}");
}

#[test]
fn genuine_wrapping_preserves_deletions_and_the_message_edit() {
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
    let diff = review(old, new);
    let changes = diff["changes"].as_array().unwrap();
    assert_eq!(diff["has_semantic_changes"], true);
    assert_eq!(diff["is_style_only"], false);
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
