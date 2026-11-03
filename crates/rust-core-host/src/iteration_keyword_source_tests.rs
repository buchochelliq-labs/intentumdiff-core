//! Wasm-route controls for iteration-protocol keywords (`for await`, `for ... in` / `of`).
//! tree-sitter keeps these keywords as anonymous tokens on `for_in_statement`, which the Wasm
//! CST drops; without recovery the toggles hashed equal and were certified style-only.
use crate::*;
use serde_json::Value;

fn diff(language: &str, filename: &str, old: &str, new: &str) -> Value {
    let wasm = std::env::var("INTENTUMDIFF_TEST_WASM_DIR").expect("staged parser components");
    let parser = std::path::Path::new(&wasm).join("js_ts_parser.wasm");
    assert!(parser.is_file(), "js_ts_parser.wasm missing: {}", parser.display());
    native_wasm_single_diff(language, parser.to_str().unwrap(), old, new, filename, filename, "{}", None)
        .expect("wasm diff")
}

#[test]
fn iteration_protocol_keyword_changes_are_semantic_not_style() {
    let cases = [
        ("javascript", "loop.js",
         "async function f(xs) {\n  for await (const x of xs) { g(x); }\n}\n",
         "async function f(xs) {\n  for (const x of xs) { g(x); }\n}\n"),
        ("typescript", "loop.ts",
         "async function f(xs: AsyncIterable<number>) {\n  for await (const x of xs) { g(x); }\n}\n",
         "async function f(xs: AsyncIterable<number>) {\n  for (const x of xs) { g(x); }\n}\n"),
        ("javascript", "keys.js",
         "function f(o) {\n  for (const k in o) { g(k); }\n}\n",
         "function f(o) {\n  for (const k of o) { g(k); }\n}\n"),
    ];
    for (language, filename, old, new) in cases {
        for (a, b) in [(old, new), (new, old)] {
            let d = diff(language, filename, a, b);
            assert_eq!(d["is_fallback"], false, "{filename}: {d}");
            assert_ne!(d["is_style_only"], true, "{filename} {a:?} -> {b:?} served as style-only: {d}");
            assert_eq!(d["has_semantic_changes"], true, "{filename} {a:?} -> {b:?}: {d}");
            assert!(!d["changes"].as_array().unwrap().is_empty(), "{filename}: no changes: {d}");
        }
    }
}

#[test]
fn iteration_keywords_unchanged_stay_style_only() {
    // Controls: layout-only edits around the same keywords must remain style-only.
    for (language, filename, old, new) in [
        ("javascript", "loop.js",
         "async function f(xs) {\n  for await (const x of xs) { g(x); }\n}\n",
         "async function f(xs) {\n  for   await   ( const x  of  xs ) {\n    g(x);\n  }\n}\n"),
        ("javascript", "keys.js",
         "function f(o) {\n  for (const k in o) { g(k); }\n}\n",
         "function f(o) {\n  // keys\n  for (const k in /* own */ o) { g(k); }\n}\n"),
    ] {
        let d = diff(language, filename, old, new);
        assert_eq!(d["is_style_only"], true, "{filename}: layout-only edit lost style-only: {d}");
    }
}
