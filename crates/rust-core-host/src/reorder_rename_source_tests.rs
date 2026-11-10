//! Source-judged regressions for release/v0.0.3-rc draft-promotion passes, driven through
//! the real registry-pinned JS/TS, Python and Go parser Wasm via the native review path.
//!
//! R1 (#242 follow-up): an exported declaration (`export function a(){}`) is an entity, not an
//! executable statement. Swapping two hoisted declarations is a MOVE, never a behaviour-changing
//! "Reorder statement" MODIFICATION.
use serde_json::{json, Value};

fn review(file: &str, old: &str, new: &str) -> Value {
    let (language, wasm, extension) = match file.rsplit_once('.').map(|(_, ext)| ext) {
        Some("js") => ("javascript", "js_ts_parser.wasm", ".js"),
        Some("ts") => ("typescript", "js_ts_parser.wasm", ".ts"),
        Some("py") => ("python", "python_parser.wasm", ".py"),
        Some("go") => ("go", "go_parser.wasm", ".go"),
        other => panic!("unsupported fixture extension {other:?}"),
    };
    let staged_dir = std::env::var("INTENTUMDIFF_TEST_WASM_DIR")
        .expect("full CI must provision trusted parser Wasm");
    let staged = std::path::Path::new(&staged_dir).join(wasm);
    assert!(staged.is_file(), "registry-pinned component absent: {}", staged.display());
    let fixture = tempfile::tempdir().expect("isolated parser staging");
    std::fs::copy(staged, fixture.path().join(wasm)).expect("copy exact verified parser bytes");
    std::fs::write(
        fixture.path().join("parser_manifest.json"),
        json!({
            "schema_version": 1,
            "parsers": {
                language: {"plugin_id": language, "wasm": wasm, "extensions": [extension]}
            },
            "extension_index": {extension: language}
        }).to_string(),
    ).expect("test-only manifest for already-pinned component");
    let raw = crate::live_server::live_diff_contents_impl(
        env!("CARGO_MANIFEST_DIR"), file, old, new, "{}",
        fixture.path().to_str().expect("UTF-8 fixture path"),
    ).expect("native review");
    let response: Value = serde_json::from_str(&raw).expect("valid native response");
    let diff = response.get("diff").unwrap_or_else(||
        panic!("{language} review unexpectedly declined: {response:#}")
    ).clone();
    assert_eq!(diff["language"], language);
    assert_eq!(diff["is_fallback"], false, "{diff:#}");
    assert_eq!(diff["parse_errors"], json!([]), "{diff:#}");
    diff
}

fn records(diff: &Value) -> Vec<String> {
    diff["changes"].as_array().expect("changes array").iter().map(|change| format!(
        "{} {} {}",
        change["change_type"].as_str().unwrap_or(""),
        change["refactoring_kind"].as_str().unwrap_or("-"),
        change["description"].as_str().unwrap_or(""),
    )).collect()
}

fn assert_no_reorder_modification(records: &[String]) {
    assert!(records.iter().all(|record| !record.contains("Reorder")),
        "hoisted declarations reorder as a MOVE, not a behaviour change: {records:#?}");
}

fn has_move_of(records: &[String], node: &str) -> bool {
    records.iter().any(|record| record.starts_with("MOVE ") && record.contains(node))
}

// ---------------------------------------------------------------- R1: exported declarations

#[test]
fn js_exported_function_swap_is_a_move_not_a_statement_reorder() {
    let old = "export function a(){return 1;}\nexport function b(){return 2;}\n";
    let new = "export function b(){return 2;}\nexport function a(){return 1;}\n";
    let records = records(&review("code.js", old, new));
    assert_no_reorder_modification(&records);
    assert!(has_move_of(&records, "('b')") || has_move_of(&records, "('a')"), "{records:#?}");
}

#[test]
fn ts_exported_function_pure_move_is_a_move() {
    let old = "export function a(): number { return 1; }\nexport function b(): number { return 2; }\n";
    let new = "export function b(): number { return 2; }\nexport function a(): number { return 1; }\n";
    let records = records(&review("code.ts", old, new));
    assert_no_reorder_modification(&records);
    assert!(has_move_of(&records, "('b')") || has_move_of(&records, "('a')"), "{records:#?}");
}

#[test]
fn ts_exported_function_move_plus_edit_reports_move_and_edit() {
    let old = "export function a(): number { return 1; }\nexport function b(): number { return 2; }\n";
    let new = "export function b(): number { return 3; }\nexport function a(): number { return 1; }\n";
    let records = records(&review("code.ts", old, new));
    assert_no_reorder_modification(&records);
    assert!(has_move_of(&records, "('b')") || has_move_of(&records, "('a')"), "{records:#?}");
    assert!(records.iter().any(|record| !record.starts_with("MOVE ")),
        "the body edit must still surface: {records:#?}");
}

#[test]
fn ts_exported_adjacent_sibling_reorder_is_a_move() {
    let old = "export function a(): number { return 1; }\nexport function b(): number { return 2; }\nexport function c(): number { return 3; }\n";
    let new = "export function a(): number { return 1; }\nexport function c(): number { return 3; }\nexport function b(): number { return 2; }\n";
    let records = records(&review("code.ts", old, new));
    assert_no_reorder_modification(&records);
    assert!(has_move_of(&records, "('b')") || has_move_of(&records, "('c')"), "{records:#?}");
}

#[test]
fn js_unexported_function_swap_stays_a_move() {
    let old = "function a(){return 1;}\nfunction b(){return 2;}\n";
    let new = "function b(){return 2;}\nfunction a(){return 1;}\n";
    let records = records(&review("code.js", old, new));
    assert_no_reorder_modification(&records);
    assert!(has_move_of(&records, "function_declaration"), "{records:#?}");
}

#[test]
fn js_executable_statement_swap_still_reports_a_reorder() {
    let old = "let x = 1;\nx += 2;\nx *= 3;\n";
    let new = "let x = 1;\nx *= 3;\nx += 2;\n";
    let records = records(&review("code.js", old, new));
    assert!(records.iter().any(|record| record.starts_with("MODIFICATION ")
        && record.contains("Reorder statement")),
        "executable statement order changes behaviour: {records:#?}");
}
