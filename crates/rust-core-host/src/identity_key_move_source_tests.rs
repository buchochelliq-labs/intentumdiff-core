//! A MOVE that exists only because an XML schema identity key (here a user-registered
//! dialect keying `book` by `isbn`) forbids pairing elements across identities is not the
//! surviving sub-expression of a modified statement: the #299 suppressor must keep it.
//! Through the real XML parser component.
use crate::*;
use serde_json::Value;

const CATALOG_DESCRIPTOR: &str = "language_id: acme-catalog\nmatch:\n  filename_patterns: [\"*.catalog.xml\"]\n  root_element: catalog\nkeyed_elements:\n  book: [isbn]\n";

fn catalog(isbn: &str, price: &str) -> String {
    format!("<catalog><book><isbn>{isbn}</isbn><price>{price}</price></book></catalog>\n")
}

/// Diff through the xml component with `schema_repo_root` pointing at a repo that either
/// registers the catalog dialect or registers nothing.
fn diff(old: &str, new: &str, registered: bool) -> Value {
    let wasm = std::env::var("INTENTUMDIFF_TEST_WASM_DIR").expect("staged parser components");
    let parser = std::path::Path::new(&wasm).join("xml_parser.wasm");
    assert!(parser.is_file(), "xml_parser.wasm missing: {}", parser.display());
    let root = std::env::temp_dir().join(format!(
        "intentumdiff-idkey-{}-{}-{}",
        std::process::id(),
        registered,
        std::thread::current().name().unwrap_or("t").replace("::", "_")
    ));
    let schemas = root.join(".intentumdiff/schemas");
    std::fs::create_dir_all(&schemas).unwrap();
    if registered {
        std::fs::write(schemas.join("acme.yml"), CATALOG_DESCRIPTOR).unwrap();
    }
    let config = json!({"schema_repo_root": root.to_str().unwrap()}).to_string();
    let result = native_wasm_single_diff(
        "xml", parser.to_str().unwrap(), old, new, "shop.catalog.xml", "shop.catalog.xml", &config, None,
    )
    .expect("wasm diff");
    let _ = std::fs::remove_dir_all(&root);
    result
}

fn shape(diff: &Value) -> Vec<(String, String, String)> {
    diff["changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            let node = if c["new_node"].is_null() { &c["old_node"] } else { &c["new_node"] };
            (
                c["change_type"].as_str().unwrap().to_owned(),
                node["node_type"].as_str().unwrap_or("").to_owned(),
                node["label"].as_str().unwrap_or("").to_owned(),
            )
        })
        .collect()
}

#[test]
fn identity_key_edit_keeps_the_cross_identity_move() {
    // core#299 regression: the dialect forbids pairing book[978-0] with book[978-9], so the
    // unchanged <price> re-parents across identities - a MOVE the sub-expression suppressor
    // dropped, making the registered result indistinguishable from the unregistered one.
    let (old, new) = (catalog("978-0", "10.99"), catalog("978-9", "10.99"));
    let unregistered = diff(&old, &new, false);
    let registered = diff(&old, &new, true);
    assert_eq!(registered["metadata"]["schema"]["provider_id"], "user:acme-catalog", "{registered:#}");
    let modification = ("MODIFICATION".to_owned(), "text".to_owned(), "978-9".to_owned());
    assert_eq!(shape(&unregistered), [modification.clone()], "{unregistered:#}");
    assert_eq!(
        shape(&registered),
        [modification, ("MOVE".to_owned(), "element".to_owned(), "price".to_owned())],
        "{registered:#}"
    );
    assert_eq!(registered["has_semantic_changes"], true);
}

#[test]
fn keyed_value_edit_stays_one_modification() {
    // Inside one identity nothing crosses a key: a price edit is a single MODIFICATION with
    // the dialect registered (no MOVE resurrected by the boundary rule).
    let registered = diff(&catalog("978-0", "10.99"), &catalog("978-0", "12.99"), true);
    assert_eq!(
        shape(&registered),
        [("MODIFICATION".to_owned(), "text".to_owned(), "12.99".to_owned())],
        "{registered:#}"
    );
}
