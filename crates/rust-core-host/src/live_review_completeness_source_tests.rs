//! Source-judged release blocker (#193): a parse-failed changed file cannot
//! coexist with a successful commit review. Uses the actual pinned Wasm guests
//! and native Rust API, not a mocked or Python semantic result.
use super::*;
use serde_json::{json, Value};
use std::{path::Path, process::Command};

fn git(repo: &Path, args: &[&str]) {
    let output = Command::new("git").args(args).current_dir(repo)
        .output().expect("git available on CI");
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
}

fn fixture() -> (tempfile::TempDir, tempfile::TempDir) {
    let staged_dir = std::env::var("INTENTUMDIFF_TEST_WASM_DIR")
        .expect("mandatory release CI provisions verified parser components");
    let wasm = tempfile::tempdir().expect("Wasm staging");
    for name in ["python_parser.wasm", "js_ts_parser.wasm"] {
        let source = Path::new(&staged_dir).join(name);
        assert!(source.is_file(), "trusted parser component absent: {}", source.display());
        std::fs::copy(source, wasm.path().join(name)).expect("copy pinned component");
    }
    std::fs::write(wasm.path().join("parser_manifest.json"), json!({
        "schema_version": 1,
        "parsers": {
            "python": {"plugin_id": "python", "wasm": "python_parser.wasm", "extensions": [".py"]},
            "javascript": {"plugin_id": "js-ts", "wasm": "js_ts_parser.wasm", "extensions": [".js"]}
        },
        "extension_index": {".py": "python", ".js": "javascript"}
    }).to_string()).expect("isolated manifest");

    let repo = tempfile::tempdir().expect("git repository");
    git(repo.path(), &["init"]);
    git(repo.path(), &["config", "user.name", "IntentumDiff Acceptance"]);
    git(repo.path(), &["config", "user.email", "test@example.invalid"]);
    std::fs::write(repo.path().join("good.py"), "answer = 1\n").unwrap();
    std::fs::write(repo.path().join("broken.js"), "function f() { return 1; }\n").unwrap();
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-m", "baseline"]);
    (repo, wasm)
}

fn review(repo: &Path, wasm: &Path) -> Value {
    let response = live_handle_review_impl(
        repo.to_str().expect("UTF-8 repo"), "HEAD", "", "{}",
        wasm.to_str().expect("UTF-8 Wasm staging"),
    ).expect("structured Rust result");
    serde_json::from_str(&response).expect("valid JSON envelope")
}

#[test]
fn mixed_python_and_wasm_review_rejects_unparseable_changed_file_without_dropping_path() {
    let (repo, wasm) = fixture();
    std::fs::write(repo.path().join("good.py"), "answer = 2\n").unwrap();
    std::fs::write(repo.path().join("broken.js"), "function f(\n").unwrap();
    let result = review(repo.path(), wasm.path());
    assert!(result.get("commit_diff").is_none(),
        "incomplete changed JS must not be certified with valid Python: {result:#}");
    assert_eq!(result["fallback_code"], "native_parse_incomplete", "{result:#}");
    assert!(result["fallback"].as_str().is_some_and(|s| s.contains("broken.js")),
        "failed filename must survive: {result:#}");

    // Independent valid control: same pinned parser bytes can certify both
    // changes and may not be made to fail indiscriminately.
    std::fs::write(repo.path().join("broken.js"), "function f() { return 2; }\n").unwrap();
    let valid = review(repo.path(), wasm.path());
    assert!(valid.get("commit_diff").is_some(),
        "two valid changed files must still be reviewable: {valid:#}");
    assert!(valid.get("fallback").is_none(), "{valid:#}");
    let files = valid["commit_diff"]["file_diffs"].as_array().expect("complete file list");
    assert_eq!(files.len(), 2, "do not drop a valid changed file: {valid:#}");
    assert!(files.iter().all(|d| d["is_fallback"] == false && d["parse_errors"] == json!([])),
        "{valid:#}");
}
