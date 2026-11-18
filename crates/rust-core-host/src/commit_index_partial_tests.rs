//! Issue #298: one changed file that exhausts the parser fuel budget while being indexed must not
//! discard the commit's cross-file analysis, and the degraded result must say so structurally
//! (`index_errors` + `cross_file_index_status: "partial"`), never silently claim completeness.
//! Uses the real pinned Python parser component.
use super::*;
use std::{path::Path, process::Command};

/// The issue's repro shape: ~3,000 lines, 600 functions. Indexing it costs ~845M fuel per side.
fn big_python() -> String {
    (0..600)
        .map(|i| format!("def f{i}(a, b):\n    if a > b:\n        return a * {i} + b\n    return b - {i}\n\n"))
        .collect()
}

const A_OLD: &str = "def moved():\n    return 1\n";
const A_NEW: &str = "def keep():\n    return 2\n";
const B_OLD: &str = "def other():\n    return 3\n";
const B_NEW: &str = "def moved():\n    return 1\n";

/// Below 1M the configured fuel is the per-call budget as-is (`fuel_budget`). A one-function
/// file indexes in ~0.45M; the big file needs ~845M, so only it runs out of fuel.
const TIGHT_FUEL: &str = r#"{"plugin_fuel":999999}"#;

fn wasm_dir() -> String {
    std::env::var("INTENTUMDIFF_TEST_WASM_DIR")
        .expect("mandatory release CI provisions verified parser components")
}

fn py(path: &str, old: &str, new: &str) -> Value {
    json!({"old_source": old, "new_source": new, "old_filename": path, "new_filename": path,
        "language": "python", "parser_wasm_path": ""})
}

fn commit_files() -> Vec<Value> {
    let big = big_python();
    vec![
        py("pkg/big.py", &big, &format!("{big}x = 1\n")),
        py("pkg/a.py", A_OLD, A_NEW),
        py("pkg/b.py", B_OLD, B_NEW),
    ]
}

fn assert_moved_a_to_b(changes: &Value) {
    let moves: Vec<&Value> = changes.as_array().expect("cross-file change list").iter()
        .filter(|c| c["change_type"] == "MOVE_TO_MODULE" && c["symbol_name"] == "module.moved")
        .collect();
    assert_eq!(moves.len(), 1, "{changes:#}");
    assert_eq!(moves[0]["old_file"], "pkg/a.py");
    assert_eq!(moves[0]["new_file"], "pkg/b.py");
    assert!(changes.to_string().find("pkg/big.py").is_none(),
        "an unindexed file must contribute no cross-file evidence: {changes:#}");
}

fn assert_big_file_unindexed(index_errors: &Value) {
    let errors = index_errors.as_array().expect("structured index_errors");
    assert_eq!(errors.len(), 1, "{index_errors:#}");
    assert_eq!(errors[0]["path"], "pkg/big.py");
    assert_eq!(errors[0]["old_filename"], "pkg/big.py");
    assert_eq!(errors[0]["new_filename"], "pkg/big.py");
    assert_eq!(errors[0]["language"], "python");
    assert_eq!(errors[0]["code"], "fuel_exceeded", "{index_errors:#}");
    assert!(errors[0]["reason"].as_str().is_some_and(|r| !r.is_empty()));
}

#[test]
fn over_budget_file_does_not_block_cross_file_move_between_small_files() {
    let index = compute_commit_cross_file_changes(&commit_files(), TIGHT_FUEL, &wasm_dir())
        .map_err(|reason| format!("one over-budget file must not fail the commit index: {reason}"))
        .unwrap();
    assert_eq!(index.status(), CROSS_FILE_INDEX_PARTIAL);
    assert_eq!(index.indexed_file_count, 2);
    assert_moved_a_to_b(&index.changes);
    let completeness = index.completeness();
    assert_eq!(completeness["cross_file_index_status"], "partial");
    assert_eq!(completeness["cross_file_index_complete"], false);
    assert_eq!(completeness["indexed_file_count"], 2);
    assert_big_file_unindexed(&completeness["index_errors"]);
}

#[test]
fn fully_indexed_commit_reports_complete_cross_file_index() {
    let index = compute_commit_cross_file_changes(&commit_files(), "{}", &wasm_dir()).unwrap();
    assert_eq!(index.status(), CROSS_FILE_INDEX_COMPLETE);
    assert!(index.index_errors.is_empty());
    assert_eq!(index.indexed_file_count, 3);
    let completeness = index.completeness();
    assert_eq!(completeness["cross_file_index_complete"], true);
    assert_eq!(completeness["index_errors"], json!([]));
    let moves = index.changes.as_array().unwrap().iter()
        .filter(|c| c["change_type"] == "MOVE_TO_MODULE" && c["symbol_name"] == "module.moved").count();
    assert_eq!(moves, 1, "{:#}", index.changes);
}

fn git(repo: &Path, args: &[&str]) {
    let output = Command::new("git").args(args).current_dir(repo)
        .output().expect("git available on CI");
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn live_review_serves_partial_cross_file_index_with_structured_index_errors() {
    let staged = wasm_dir();
    let wasm = tempfile::tempdir().expect("Wasm staging");
    std::fs::copy(Path::new(&staged).join("python_parser.wasm"),
        wasm.path().join("python_parser.wasm")).expect("copy pinned component");
    std::fs::write(wasm.path().join("parser_manifest.json"), json!({
        "schema_version": 1,
        "parsers": {"python": {"plugin_id": "python", "wasm": "python_parser.wasm", "extensions": [".py"]}},
        "extension_index": {".py": "python"}
    }).to_string()).expect("isolated manifest");

    let repo = tempfile::tempdir().expect("git repository");
    git(repo.path(), &["init"]);
    git(repo.path(), &["config", "user.name", "IntentumDiff Acceptance"]);
    git(repo.path(), &["config", "user.email", "test@example.invalid"]);
    let pkg = repo.path().join("pkg");
    std::fs::create_dir(&pkg).unwrap();
    let big = big_python();
    std::fs::write(pkg.join("big.py"), &big).unwrap();
    std::fs::write(pkg.join("a.py"), A_OLD).unwrap();
    std::fs::write(pkg.join("b.py"), B_OLD).unwrap();
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-m", "baseline"]);
    std::fs::write(pkg.join("big.py"), format!("{big}x = 1\n")).unwrap();
    std::fs::write(pkg.join("a.py"), A_NEW).unwrap();
    std::fs::write(pkg.join("b.py"), B_NEW).unwrap();

    let response = crate::live_server::live_handle_review_impl(
        repo.path().to_str().unwrap(), "HEAD", "", TIGHT_FUEL, wasm.path().to_str().unwrap(),
    ).expect("one over-budget file must not fail the commit review");
    let result: Value = serde_json::from_str(&response).expect("valid JSON envelope");
    assert!(result.get("fallback").is_none(), "{result:#}");
    let commit = &result["commit_diff"];
    assert_eq!(commit["file_diffs"].as_array().map(Vec::len), Some(3),
        "every per-file diff still served: {result:#}");
    assert_moved_a_to_b(&commit["cross_file_changes"]);
    let metadata = &result["metadata"];
    assert_eq!(metadata["cross_file_index_status"], "partial", "{metadata:#}");
    assert_eq!(metadata["cross_file_index_complete"], false);
    assert_eq!(metadata["indexed_file_count"], 2);
    assert_big_file_unindexed(&metadata["index_errors"]);
    assert_eq!(metadata["review_fuel_capped"], false, "existing metadata kept: {metadata:#}");
}
