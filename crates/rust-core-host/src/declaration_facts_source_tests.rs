//! Real-component tests for the `visibility` and `stub_sentinel` NodeFacts
//! (refs intentumdiff-vscode#132): each language's rule, decided from the component's
//! normalised tree plus the declaration header.
use crate::*;
use serde_json::Value;

fn component(language: &str) -> String {
    let wasm = std::env::var("INTENTUMDIFF_TEST_WASM_DIR").expect("staged parser components");
    let file = match language {
        "javascript" | "typescript" => "js_ts_parser.wasm".to_owned(),
        other => format!("{other}_parser.wasm"),
    };
    let path = std::path::Path::new(&wasm).join(file);
    assert!(path.is_file(), "parser missing: {}", path.display());
    path.to_string_lossy().into_owned()
}

/// (label, visibility, body, stub_sentinel) for every declaration, in tree order, after the
/// same fact passes the finalize route runs.
fn facts(language: &str, filename: &str, source: &str) -> Vec<(String, String, String, String)> {
    let pair = run_python_wasm_process_pair_detailed(&component(language), source, "", filename, source, "",
        filename, 2_000_000_000, 50_000_000, language).expect("parse");
    let tree = enrich_literal_labels_json_str(&pair.new_tree, source).expect("literal labels");
    let mut tree: SemanticNode = serde_json::from_str(&tree).expect("tree");
    enrich_tree_facts(&mut tree);
    crate::declaration_facts::enrich_declaration_facts(&mut tree, source, language);
    let mut out = Vec::new();
    fn walk(node: &SemanticNode, out: &mut Vec<(String, String, String, String)>) {
        if let Some(f) = node.facts.as_ref().filter(|f| f.get("visibility").is_some()) {
            let s = |k: &str| f.get(k).and_then(Value::as_str).unwrap_or("-").to_owned();
            out.push((node.label.clone(), s("visibility"), s("body"), s("stub_sentinel")));
        }
        for child in &node.children {
            walk(child, out);
        }
    }
    walk(&tree, &mut out);
    out
}

fn vis(rows: &[(String, String, String, String)]) -> Vec<(&str, &str)> {
    rows.iter().map(|(l, v, _, _)| (l.as_str(), v.as_str())).collect()
}

fn stubs(rows: &[(String, String, String, String)]) -> Vec<(&str, &str)> {
    rows.iter().filter(|r| r.2 == "stub").map(|(l, _, _, s)| (l.as_str(), s.as_str())).collect()
}

#[test]
fn python_visibility_follows_naming_convention_and_stub_sentinels() {
    let rows = facts("python", "a.py", "def public(a):\n    return a\n\ndef _internal():\n    pass\n\n\
        def __mangled():\n    raise NotImplementedError\n\ndef __init__(self):\n    ...\n\n\
        def documented():\n    \"\"\"Doc.\"\"\"\n\nclass _Hidden:\n    pass\n");
    assert_eq!(vis(&rows), [("public", "public"), ("_internal", "internal"), ("__mangled", "private"),
        ("__init__", "special"), ("documented", "public"), ("_Hidden", "internal")]);
    assert_eq!(stubs(&rows), [("_internal", "pass"), ("__mangled", "not_implemented"),
        ("__init__", "ellipsis"), ("documented", "docstring_only"), ("_Hidden", "pass")]);
    // A substantive body never carries a sentinel.
    assert_eq!(rows[0].3, "-");
}

#[test]
fn rust_visibility_reads_pruned_pub_modifiers_and_stub_macros() {
    let rows = facts("rust", "a.rs", "pub fn a() -> i32 { 1 }\nfn b() { todo!() }\npub(crate) fn c() { unimplemented!() }\n\
        fn d() { panic!(\"x\") }\npub(self) fn e() {}\n#[cfg(test)]\npub struct S;\nimpl S {\n    pub fn m(&self) {}\n}\n\
        impl Clone for S {\n    fn clone(&self) -> S { S }\n}\n");
    assert_eq!(vis(&rows), [("a", "public"), ("b", "private"), ("c", "internal"), ("d", "private"),
        ("e", "private"), ("S", "public"), ("m", "public"), ("clone", "unknown")]);
    assert_eq!(stubs(&rows), [("b", "todo_macro"), ("c", "todo_macro"), ("d", "panic")]);
}

#[test]
fn java_and_csharp_visibility_use_modifiers_and_language_defaults() {
    let rows = facts("java", "A.java", "public class A {\n  @Override public int a() { return 1; }\n\
        private void b() { throw new UnsupportedOperationException(); }\n  protected void c() {}\n  void d() {}\n}\n\
        interface I {\n  void e();\n}\n");
    assert_eq!(vis(&rows), [("A", "public"), ("a", "public"), ("b", "private"), ("c", "protected"),
        ("d", "internal"), ("I", "internal"), ("e", "public")]);
    assert_eq!(stubs(&rows), [("b", "not_implemented")]);
    let rows = facts("csharp", "A.cs", "namespace N {\n  public class A {\n    public int M() { return 1; }\n\
        private void B() { throw new NotImplementedException(); }\n    internal void C() {}\n\
        protected internal void D() {}\n    void E() {}\n  }\n  class F {}\n  public interface J { void Q(); }\n}\n");
    assert_eq!(vis(&rows), [("A", "public"), ("M", "public"), ("B", "private"), ("C", "internal"),
        ("D", "protected"), ("E", "private"), ("F", "internal"), ("J", "public"), ("Q", "public")]);
    assert_eq!(stubs(&rows), [("B", "not_implemented")]);
}

#[test]
fn go_visibility_is_identifier_capitalisation() {
    let rows = facts("go", "a.go", "package a\n\nfunc Public() int { return 1 }\n\nfunc private() { panic(\"todo\") }\n");
    assert_eq!(vis(&rows), [("Public", "public"), ("private", "internal")]);
    assert_eq!(stubs(&rows), [("private", "panic")]);
}

#[test]
fn typescript_visibility_uses_export_and_accessibility_modifiers() {
    let rows = facts("typescript", "a.ts", "export function a(): number { return 1; }\nfunction b() {}\n\
        class C {\n  private x(): void {}\n  protected y(): void {}\n  public z(): number { return 1; }\n  w() {}\n  #p() {}\n}\n");
    assert_eq!(vis(&rows), [("a", "public"), ("b", "private"), ("C", "private"), ("x", "private"),
        ("y", "protected"), ("z", "public"), ("w", "public"), ("#p", "private")]);
    // A script (no import/export) has a global top level: undecided, never guessed.
    let rows = facts("javascript", "a.js", "function f() {}\nclass K { m() {} }\n");
    assert_eq!(vis(&rows), [("f", "unknown"), ("K", "unknown"), ("m", "public")]);
}

#[test]
fn kotlin_and_swift_apply_their_default_visibility() {
    let rows = facts("kotlin", "a.kt", "fun a(): Int = 1\nfun b() {\n    TODO()\n}\n");
    assert_eq!(vis(&rows), [("a", "public"), ("b", "public")]);
    assert_eq!(stubs(&rows), [("b", "todo_macro")]);
    let rows = facts("swift", "a.swift", "func c() -> Int { return 1 }\nfunc b() {\n    fatalError(\"x\")\n}\n");
    assert_eq!(vis(&rows), [("c", "internal"), ("b", "internal")]);
    assert_eq!(stubs(&rows), [("b", "panic")]);
}

#[test]
fn served_wasm_diff_carries_declaration_facts_on_change_nodes() {
    let d = native_wasm_single_diff("rust", &component("rust"), "pub fn keep() {}\n",
        "pub fn keep() {}\npub(crate) fn added() { todo!() }\n", "a.rs", "a.rs", "{}", None).expect("diff");
    let added = d["changes"].as_array().unwrap().iter()
        .find(|c| c["new_node"]["label"] == "added").unwrap_or_else(|| panic!("{d:#}"));
    let facts = &added["new_node"]["facts"];
    assert_eq!(facts["visibility"], "internal", "{facts}");
    assert_eq!(facts["body"], "stub", "{facts}");
    assert_eq!(facts["stub_sentinel"], "todo_macro", "{facts}");
}
