//! Source-judged regressions for release/v0.0.3-rc draft-promotion passes, driven through
//! the real registry-pinned JS/TS, Python and Go parser Wasm via the native review path.
//!
//! R1 (#242 follow-up): an exported declaration (`export function a(){}`) is an entity, not an
//! executable statement. Swapping two hoisted declarations is a MOVE, never a behaviour-changing
//! "Reorder statement" MODIFICATION.
//!
//! R3 (#245 follow-up): the same-id rename binder gate must recognise every language's parameter
//! list and the ordinary local binders (Python assignment targets, JS/Python loop binders, Go
//! `:=`/`var` names), while keeping #245's same-callable scoping requirement.
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

fn has_rename(records: &[String], old: &str, new: &str) -> bool {
    records.iter().any(|record| record.starts_with("REFACTORING RENAME_VARIABLE ")
        && record.contains(&format!("'{old}' -> '{new}'")))
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

// ---------------------------------------------------------------- R3: rename binder gate

#[test]
fn python_local_assignment_rename_is_a_variable_rename() {
    let old = "def f():\n    start = now()\n    return start\n";
    let new = "def f():\n    start_time = now()\n    return start_time\n";
    let records = records(&review("code.py", old, new));
    assert!(has_rename(&records, "start", "start_time"), "{records:#?}");
    assert!(records.iter().all(|record| !record.contains("Update identifier")), "{records:#?}");
}

#[test]
fn js_for_of_binder_rename_is_a_variable_rename() {
    let old = "for (const file of d) { use(file) }\n";
    let new = "for (const file_name of d) { use(file_name) }\n";
    let records = records(&review("code.js", old, new));
    assert!(has_rename(&records, "file", "file_name"), "{records:#?}");
    assert!(records.iter().all(|record| record.starts_with("REFACTORING RENAME_VARIABLE ")),
        "the binder and its use are one rename: {records:#?}");
}

#[test]
fn go_parameter_rename_is_a_variable_rename_without_spurious_move() {
    let old = "package main\n\nfunc add(a, b int) int { return a + b }\n";
    let new = "package main\n\nfunc add(a, y int) int { return a + y }\n";
    let records = records(&review("code.go", old, new));
    assert!(has_rename(&records, "b", "y"), "{records:#?}");
    assert!(records.iter().all(|record| !record.starts_with("DELETION ")
        && !record.starts_with("ADDITION ") && !record.starts_with("MOVE ")), "{records:#?}");
}

#[test]
fn python_parameter_rename_does_not_leak_into_an_independent_callable() {
    // #245 scoping: `f(a) -> f(b)` must not drag an unrelated `g`'s `a -> b` read edit along.
    let old = "def f(a):\n    return a\n\ndef g():\n    return a\n";
    let new = "def f(b):\n    return b\n\ndef g():\n    return b\n";
    let records = records(&review("code.py", old, new));
    assert!(has_rename(&records, "a", "b"), "{records:#?}");
    assert!(records.iter().any(|record| !record.starts_with("REFACTORING ")),
        "g's independent read edit must still surface: {records:#?}");
}

#[test]
fn python_for_loop_binder_rename_is_a_variable_rename() {
    let old = "def f(items):\n    for item in items:\n        use(item)\n";
    let new = "def f(items):\n    for entry in items:\n        use(entry)\n";
    let records = records(&review("code.py", old, new));
    assert!(has_rename(&records, "item", "entry"), "{records:#?}");
    assert!(records.iter().all(|record| record.starts_with("REFACTORING RENAME_VARIABLE ")),
        "{records:#?}");
}

#[test]
fn go_short_var_declaration_rename_is_a_variable_rename() {
    // (The Go parser omits an empty `parameter_list`, so a parameterless `func f()` has no
    // callable scope to anchor a local rename; this fixture takes one parameter.)
    let old = "package main\n\nfunc f(n int) int {\n\tx := n\n\treturn x\n}\n";
    let new = "package main\n\nfunc f(n int) int {\n\tcount := n\n\treturn count\n}\n";
    let records = records(&review("code.go", old, new));
    assert!(has_rename(&records, "x", "count"), "{records:#?}");
    assert!(records.iter().all(|record| record.starts_with("REFACTORING RENAME_VARIABLE ")),
        "{records:#?}");
}

#[test]
fn python_read_of_an_outer_name_is_not_a_local_rename() {
    // No binder of `limit` inside `f`: the edited read is a real reference change.
    let old = "def f(x):\n    y = x\n    return limit\n";
    let new = "def f(x):\n    y = x\n    return maximum\n";
    let records = records(&review("code.py", old, new));
    assert!(records.iter().all(|record| !record.contains("RENAME_VARIABLE")), "{records:#?}");
}
