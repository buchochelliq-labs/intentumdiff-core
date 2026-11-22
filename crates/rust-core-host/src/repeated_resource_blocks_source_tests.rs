//! terraform-parser#4: repeated unlabelled resource blocks (`a { ... }` twice, several `provider`
//! blocks) share one resource key. The same-key comparison must pair the n-th repeated block
//! with the n-th, so byte-identical input reports nothing and a real edit stays at its block.
use serde_json::{json, Value};

fn review(old: &str, new: &str) -> Value {
    let staged_dir = std::env::var("INTENTUMDIFF_TEST_WASM_DIR")
        .expect("full CI must provision trusted parser Wasm");
    let staged = std::path::Path::new(&staged_dir).join("terraform_parser.wasm");
    assert!(staged.is_file(), "registry-pinned component absent: {}", staged.display());
    let fixture = tempfile::tempdir().expect("isolated parser staging");
    std::fs::copy(staged, fixture.path().join("terraform_parser.wasm")).expect("copy verified bytes");
    std::fs::write(fixture.path().join("parser_manifest.json"), json!({"schema_version": 1,
        "parsers": {"hcl": {"plugin_id": "hcl", "wasm": "terraform_parser.wasm", "extensions": [".tf"]}},
        "extension_index": {".tf": "hcl"}}).to_string()).expect("test-only manifest");
    let raw = crate::live_server::live_diff_contents_impl(
        env!("CARGO_MANIFEST_DIR"), "main.tf", old, new, "{}",
        fixture.path().to_str().expect("UTF-8 fixture path"),
    ).expect("native review");
    let response: Value = serde_json::from_str(&raw).expect("valid native response");
    response.get("diff").unwrap_or_else(|| panic!("review declined: {response:#}")).clone()
}

const REPEATED: &str = "a {\n  x = 1\n}\na {\n  x = 2\n}\n";

#[test]
#[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged parser wasm (set INTENTUMDIFF_TEST_WASM_DIR or enable tier-c-wasm)")]
fn identical_repeated_blocks_report_nothing() {
    for source in [REPEATED,
                   "provider \"aws\" {\n  region = \"a\"\n}\nprovider \"aws\" {\n  region = \"b\"\n  alias = \"b\"\n}\n"] {
        let diff = review(source, source);
        assert_eq!(diff["changes"], json!([]), "{source:?}: {diff:#}");
        assert_eq!(diff["has_semantic_changes"], false, "{source:?}");
    }
}

#[test]
#[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged parser wasm (set INTENTUMDIFF_TEST_WASM_DIR or enable tier-c-wasm)")]
fn an_edit_inside_a_repeated_block_stays_at_that_block() {
    // Control: numbering occurrences must not hide a real value change, nor move it to the
    // other block.
    for (new, line, old_label) in [("a {\n  x = 3\n}\na {\n  x = 2\n}\n", 1, "1"),
                                   ("a {\n  x = 1\n}\na {\n  x = 4\n}\n", 4, "2")] {
        let diff = review(REPEATED, new);
        let changes = diff["changes"].as_array().unwrap();
        assert_eq!(changes.len(), 1, "{new:?}: {diff:#}");
        let change = &changes[0];
        assert_eq!(change["change_type"], "MODIFICATION", "{new:?}: {diff:#}");
        assert_eq!(change["old_node"]["position"]["start_line"], line, "{new:?}: {diff:#}");
        assert!(change.to_string().contains(old_label), "{new:?}: {diff:#}");
    }
}

#[test]
#[cfg_attr(not(feature = "tier-c-wasm"), ignore = "needs staged parser wasm (set INTENTUMDIFF_TEST_WASM_DIR or enable tier-c-wasm)")]
fn removing_an_attribute_from_an_earlier_block_does_not_touch_the_later_one() {
    // An attribute is counted within its own repeated block: dropping `x` from the first `a`
    // must not re-pair the second block's unchanged `x = 2` with the removed `x = 1`.
    let diff = review(REPEATED, "a {\n}\na {\n  x = 2\n}\n");
    let changes = diff["changes"].as_array().unwrap();
    assert!(!changes.is_empty(), "{diff:#}");
    for change in changes {
        assert_ne!(change["change_type"], "MODIFICATION", "{diff:#}");
        assert_ne!(change["new_node"]["position"]["start_line"], 4, "the later block is unchanged: {diff:#}");
        assert_ne!(change["old_node"]["position"]["start_line"], 4, "the later block is unchanged: {diff:#}");
    }
}
