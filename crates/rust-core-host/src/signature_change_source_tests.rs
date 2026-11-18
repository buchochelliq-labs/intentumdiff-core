//! Wasm-route regressions for issue #297: a parameter-list edit must surface as ONE
//! definition-level CHANGE_SIGNATURE whose old/new nodes carry the parameter facts, so a
//! consumer can tell a required parameter (breaking) from an optional one (compatible)
//! without knowing each grammar's parameter node names.
use crate::*;
use serde_json::Value;

fn diff(language: &str, filename: &str, old: &str, new: &str) -> Value {
    let wasm = std::env::var("INTENTUMDIFF_TEST_WASM_DIR").expect("staged parser components");
    let component = match language {
        "javascript" | "typescript" => "js_ts_parser.wasm".to_owned(),
        other => format!("{other}_parser.wasm"),
    };
    let parser = std::path::Path::new(&wasm).join(component);
    assert!(parser.is_file(), "parser missing: {}", parser.display());
    native_wasm_single_diff(language, parser.to_str().unwrap(), old, new, filename, filename, "{}", None)
        .expect("wasm diff")
}

fn signature_changes(d: &Value) -> Vec<&Value> {
    d["changes"].as_array().unwrap().iter()
        .filter(|c| c["refactoring_kind"] == "CHANGE_SIGNATURE")
        .collect()
}

/// The one definition-level signature change, with its (param_count, default_count) facts.
fn one_signature_change(d: &Value, name: &str, what: &str) -> ((u64, u64), (u64, u64)) {
    let found = signature_changes(d);
    assert_eq!(found.len(), 1, "{what}: {d:#}");
    let change = found[0];
    let shape = |side: &str| {
        let node = &change[side];
        assert_eq!(node["label"], name, "{what}: {change:#}");
        let facts = &node["facts"];
        (facts["param_count"].as_u64().unwrap_or_else(|| panic!("{what}: {facts}")),
         facts["default_count"].as_u64().unwrap_or_else(|| panic!("{what}: {facts}")))
    };
    // The parameter nodes are detail of the signature change, not separate events.
    assert!(d["changes"].as_array().unwrap().iter().all(|c| {
        c["refactoring_kind"] == "CHANGE_SIGNATURE"
            || !matches!(c["change_type"].as_str(), Some("ADDITION" | "DELETION"))
    }), "{what}: parameter leaked as its own change: {d:#}");
    assert_eq!(d["has_semantic_changes"], true, "{what}: {d:#}");
    (shape("old_node"), shape("new_node"))
}

#[test]
fn python_parameter_edits_report_required_vs_optional_by_facts() {
    let base = "def main(rows):\n    return rows\n";
    for (what, old, new, before, after) in [
        ("optional parameter added", base, "def main(rows, csv_path=None):\n    return rows\n", (1, 0), (2, 1)),
        ("required parameter added", base, "def main(rows, csv_path):\n    return rows\n", (1, 0), (2, 0)),
        ("parameter removed", "def main(rows, csv_path):\n    return rows\n", base, (2, 0), (1, 0)),
        ("default removed", "def main(rows, csv_path=None):\n    return rows\n",
            "def main(rows, csv_path):\n    return rows\n", (2, 1), (2, 0)),
    ] {
        let d = diff("python", "cli.py", old, new);
        assert_eq!(one_signature_change(&d, "main", what), (before, after), "{what}: {d:#}");
    }
}

#[test]
fn python_parameter_rename_is_not_a_signature_shape_change() {
    let d = diff("python", "cli.py", "def main(rows):\n    return rows\n",
        "def main(items):\n    return items\n");
    assert!(signature_changes(&d).is_empty(), "a rename keeps the call shape: {d:#}");
    assert_eq!(d["has_semantic_changes"], true, "{d:#}");
}

#[test]
fn body_edit_alone_is_not_a_signature_change() {
    let d = diff("python", "cli.py", "def main(rows):\n    return rows\n",
        "def main(rows):\n    return len(rows)\n");
    assert!(signature_changes(&d).is_empty(), "{d:#}");
}

#[test]
fn parameter_added_is_a_definition_level_signature_change_across_languages() {
    for (lang, file, name, old, new, after_defaults) in [
        ("javascript", "a.js", "main", "function main(rows) {\n  return rows;\n}\n",
            "function main(rows, path = null) {\n  return rows;\n}\n", 1),
        ("javascript", "a.js", "main", "function main(rows) {\n  return rows;\n}\n",
            "function main(rows, path) {\n  return rows;\n}\n", 0),
        ("typescript", "a.ts", "main", "function main(rows: number): number {\n  return rows;\n}\n",
            "function main(rows: number, path?: string): number {\n  return rows;\n}\n", 1),
        ("typescript", "a.ts", "main", "function main(rows: number): number {\n  return rows;\n}\n",
            "function main(rows: number, path: string): number {\n  return rows;\n}\n", 0),
        ("java", "A.java", "main", "class A {\n  int main(int rows) {\n    return rows;\n  }\n}\n",
            "class A {\n  int main(int rows, String path) {\n    return rows;\n  }\n}\n", 0),
        ("go", "a.go", "Main", "package a\n\nfunc Main(rows int) int {\n\treturn rows\n}\n",
            "package a\n\nfunc Main(rows int, path string) int {\n\treturn rows\n}\n", 0),
        ("rust", "a.rs", "main", "fn main(rows: u32) -> u32 {\n    rows\n}\n",
            "fn main(rows: u32, path: &str) -> u32 {\n    rows\n}\n", 0),
        ("csharp", "A.cs", "Main", "class A {\n  int Main(int rows) {\n    return rows;\n  }\n}\n",
            "class A {\n  int Main(int rows, string path = null) {\n    return rows;\n  }\n}\n", 1),
        ("csharp", "A.cs", "Main", "class A {\n  int Main(int rows) {\n    return rows;\n  }\n}\n",
            "class A {\n  int Main(int rows, string path) {\n    return rows;\n  }\n}\n", 0),
    ] {
        let what = format!("{lang} {new:?}");
        let d = diff(lang, file, old, new);
        assert_eq!(one_signature_change(&d, name, &what), ((1, 0), (2, after_defaults)), "{what}: {d:#}");
        // And the reverse edit removes it again.
        let d = diff(lang, file, new, old);
        assert_eq!(one_signature_change(&d, name, &what), ((2, after_defaults), (1, 0)), "{what} reversed: {d:#}");
    }
}

#[test]
fn added_definition_reports_zero_defaults_not_unknown() {
    let d = diff("python", "cli.py", "x = 1\n", "x = 1\n\ndef main(rows):\n    return rows\n");
    let added = d["changes"].as_array().unwrap().iter()
        .find(|c| c["change_type"] == "ADDITION" && c["new_node"]["label"] == "main")
        .unwrap_or_else(|| panic!("{d:#}"));
    assert_eq!(added["new_node"]["facts"]["param_count"], 1, "{added:#}");
    assert_eq!(added["new_node"]["facts"]["default_count"], 0, "{added:#}");
}
