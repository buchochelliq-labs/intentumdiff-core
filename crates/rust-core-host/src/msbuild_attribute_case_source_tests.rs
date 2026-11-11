//! MSBuild item identity through the real XML parser. XML attribute labels keep
//! their source case (`Include=...`, `Version=...`, `Sdk=...`); the MSBuild
//! profile must match attribute names case-insensitively (MSBuild attribute
//! names are case-insensitive) so items stay keyed by Include/Update.
use serde_json::{json, Value};

/// Stage only the registry-pinned component this review needs, with a test-only manifest,
/// as the other source-judged regressions do: the provisioned directory carries the
/// verified bytes but no `parser_manifest.json`.
fn staged_parsers(path: &str) -> tempfile::TempDir {
    let staged_dir = std::env::var("INTENTUMDIFF_TEST_WASM_DIR")
        .expect("mandatory release CI provisions verified parser components");
    let (language, wasm, extension) = match std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
    {
        Some("csproj") => ("xml", "xml_parser.wasm", ".csproj"),
        other => panic!("no staged component for extension {other:?}"),
    };
    let staged = std::path::Path::new(&staged_dir).join(wasm);
    assert!(staged.is_file(), "registry-pinned component absent: {}", staged.display());
    let fixture = tempfile::tempdir().expect("isolated parser staging");
    std::fs::copy(&staged, fixture.path().join(wasm)).expect("copy exact verified parser bytes");
    std::fs::write(
        fixture.path().join("parser_manifest.json"),
        json!({
            "schema_version": 1,
            "parsers": {
                language: {"plugin_id": language, "wasm": wasm, "extensions": [extension]}
            },
            "extension_index": {extension: language}
        })
        .to_string(),
    )
    .expect("test-only manifest for already-pinned component");
    fixture
}

fn review(path: &str, old: &str, new: &str) -> Value {
    let parsers = staged_parsers(path);
    let raw = crate::live_server::live_diff_contents_impl(
        env!("CARGO_MANIFEST_DIR"), path, old, new, "{}",
        parsers.path().to_str().expect("UTF-8 fixture path"),
    )
    .expect("native review");
    let response: Value = serde_json::from_str(&raw).expect("valid native response");
    let diff = response
        .get("diff")
        .unwrap_or_else(|| panic!("review unexpectedly declined: {response:#}"))
        .clone();
    assert_eq!(diff["language"], "xml", "{diff:#}");
    assert_eq!(diff["is_fallback"], false, "{diff:#}");
    assert_eq!(diff["parse_errors"], json!([]), "{diff:#}");
    diff
}

fn csproj(items: &[(&str, &str)], include_attr: &str, sdk_attr: &str) -> String {
    let refs: String = items
        .iter()
        .map(|(name, version)| {
            format!("    <PackageReference {include_attr}=\"{name}\" Version=\"{version}\" />\n")
        })
        .collect();
    format!(
        "<Project {sdk_attr}=\"Microsoft.NET.Sdk\">\n  <ItemGroup>\n{refs}  </ItemGroup>\n</Project>\n"
    )
}

#[test]
fn msbuild_package_reference_reorder_is_not_a_change() {
    for (include, sdk) in [("Include", "Sdk"), ("include", "sdk"), ("Update", "Sdk")] {
        let old = csproj(&[("Newtonsoft.Json", "13.0.3"), ("Serilog", "3.1.1")], include, sdk);
        let new = csproj(&[("Serilog", "3.1.1"), ("Newtonsoft.Json", "13.0.3")], include, sdk);
        let diff = review("app.csproj", &old, &new);
        assert_eq!(diff["changes"], json!([]), "{include}/{sdk}: reorder churn: {diff:#}");
        assert_eq!(diff["has_semantic_changes"], false, "{diff:#}");
    }
}

#[test]
fn msbuild_reorder_with_version_bump_is_one_version_modification() {
    let old = csproj(&[("Newtonsoft.Json", "13.0.3"), ("Serilog", "3.1.1")], "Include", "Sdk");
    let new = csproj(&[("Serilog", "3.1.1"), ("Newtonsoft.Json", "13.0.4")], "Include", "Sdk");
    let diff = review("app.csproj", &old, &new);
    let changes = diff["changes"].as_array().unwrap();
    assert_eq!(changes.len(), 1, "{diff:#}");
    let change = &changes[0];
    assert_eq!(change["change_type"], "MODIFICATION", "{diff:#}");
    assert_eq!(change["old_node"]["node_type"], "attribute", "{diff:#}");
    assert_eq!(change["old_node"]["label"], "Version=13.0.3", "{diff:#}");
    assert_eq!(change["new_node"]["label"], "Version=13.0.4", "{diff:#}");
    assert_eq!(change["text_diff"], "Version=13.0.3 -> Version=13.0.4", "{diff:#}");
}

#[test]
fn msbuild_profile_matches_attribute_names_case_insensitively() {
    use crate::xml_schema::{msbuild_coordinate_key, xml_tree_is_msbuild};
    use crate::SemanticNode;
    let node = |id: &str, node_type: &str, label: &str, children: Vec<Value>| {
        json!({
            "id": id, "node_type": node_type, "label": label, "children": children,
            "position": {"start_line":0,"start_col":0,"end_line":0,"end_col":0},
            "structural_hash": "h"
        })
    };
    for (include, sdk) in [("Include", "Sdk"), ("include", "sdk"), ("INCLUDE", "SDK"), ("Update", "Sdk")] {
        let item = node(
            "0.0.0.0", "element", "PackageReference",
            vec![node("0.0.0.0.0", "attribute", &format!("{include}=Serilog"), vec![])],
        );
        let project = node(
            "0.0", "element", "Project",
            vec![node("0.0.0", "attribute", &format!("{sdk}=Microsoft.NET.Sdk"), vec![])],
        );
        let root: SemanticNode =
            serde_json::from_value(node("0", "document", "doc", vec![project])).unwrap();
        assert!(xml_tree_is_msbuild(&root), "{sdk}");
        let item: SemanticNode = serde_json::from_value(item).unwrap();
        assert_eq!(
            msbuild_coordinate_key(&item),
            Some(vec!["xml".into(), "msbuild".into(), "PackageReference".into(), "Serilog".into()]),
            "{include}"
        );
    }
    // A different attribute that merely starts with the key name is not the key.
    let other: SemanticNode = serde_json::from_value(node(
        "0", "element", "PackageReference",
        vec![node("0.0", "attribute", "IncludeAssets=all", vec![])],
    ))
    .unwrap();
    assert_eq!(msbuild_coordinate_key(&other), None);
}
