//! Registry-manifest validator (python #329): the real published manifest passes; every
//! audit probe and a bounded wrong-type matrix returns structured errors without panicking.
use intentumdiff_rust_core::api::validate_registry_manifest;
use intentumdiff_rust_core::c_abi::dispatch;
use serde_json::{json, Value};

/// `intentumdiff-registry/registry.yaml` @ f95ceaaf87008ec724fbd4d6866785f20f421be6, as JSON.
fn published() -> Value {
    serde_json::from_str(include_str!("fixtures/registry_manifest_v1.json")).unwrap()
}

fn audit_base() -> Value {
    json!({"version": 1, "plugins": {"intentumdiff-audit-parser": {
        "source": "git", "ref": "a".repeat(40), "trust_tier": "official",
        "wasm_checksums": {"audit.wasm": "b".repeat(64)},
        "capabilities": ["parser"], "abi_target": "1.0.0"}}})
}

fn paths(manifest: &Value) -> Vec<String> {
    validate_registry_manifest(manifest).into_iter().map(|e| e.0).collect()
}

const ENTRY: &str = "/plugins/intentumdiff-audit-parser";

#[test]
fn published_manifest_and_audit_base_are_valid() {
    assert_eq!(validate_registry_manifest(&published()), vec![]);
    assert_eq!(validate_registry_manifest(&audit_base()), vec![]);
    let envelope: Value = serde_json::from_str(&dispatch("validate_registry_manifest", &[json!(published().to_string())])).unwrap();
    assert_eq!(envelope, json!({"ok": true, "result": {"ok": true, "errors": []}}));
}

#[test]
fn every_audit_probe_is_a_structured_error() {
    let cases: Vec<(&str, Box<dyn Fn(&mut Value)>, String)> = vec![
        ("mutable ref", Box::new(|m| m["plugins"]["intentumdiff-audit-parser"]["ref"] = json!("main")), format!("{ENTRY}/ref")),
        ("missing ref", Box::new(|m| { m["plugins"]["intentumdiff-audit-parser"].as_object_mut().unwrap().remove("ref"); }), format!("{ENTRY}/ref")),
        ("garbage abi", Box::new(|m| m["plugins"]["intentumdiff-audit-parser"]["abi_target"] = json!("garbage")), format!("{ENTRY}/abi_target")),
        ("partial abi", Box::new(|m| m["plugins"]["intentumdiff-audit-parser"]["abi_target"] = json!("1.0")), format!("{ENTRY}/abi_target")),
        ("duplicate capability", Box::new(|m| m["plugins"]["intentumdiff-audit-parser"]["capabilities"] = json!(["parser", "parser"])), format!("{ENTRY}/capabilities/1")),
        ("unknown capability", Box::new(|m| m["plugins"]["intentumdiff-audit-parser"]["capabilities"] = json!(["network-shell"])), format!("{ENTRY}/capabilities/0")),
        ("checksum newline", Box::new(|m| m["plugins"]["intentumdiff-audit-parser"]["wasm_checksums"] = json!({"audit.wasm": format!("{}\n", "b".repeat(64))})), format!("{ENTRY}/wasm_checksums/audit.wasm")),
        ("filename newline", Box::new(|m| m["plugins"]["intentumdiff-audit-parser"]["wasm_checksums"] = json!({"audit.wasm\n": "b".repeat(64)})), format!("{ENTRY}/wasm_checksums/audit.wasm\n")),
        ("source list", Box::new(|m| m["plugins"]["intentumdiff-audit-parser"]["source"] = json!([])), format!("{ENTRY}/source")),
        ("trust_tier object", Box::new(|m| m["plugins"]["intentumdiff-audit-parser"]["trust_tier"] = json!({})), format!("{ENTRY}/trust_tier")),
        ("no checksums", Box::new(|m| { m["plugins"]["intentumdiff-audit-parser"].as_object_mut().unwrap().remove("wasm_checksums"); }), format!("{ENTRY}/wasm_checksums")),
        ("custom repo", Box::new(|m| m["plugins"]["intentumdiff-audit-parser"]["repo"] = json!("https://example.invalid/x.git")), format!("{ENTRY}/repo")),
        ("unknown field", Box::new(|m| m["plugins"]["intentumdiff-audit-parser"]["install_script"] = json!("x")), format!("{ENTRY}/install_script")),
        ("bad name", Box::new(|m| { let e = m["plugins"]["intentumdiff-audit-parser"].clone(); m["plugins"]["../evil"] = e; }), "/plugins/..~1evil".to_owned()),
        ("version zero", Box::new(|m| m["version"] = json!(0)), "/version".to_owned()),
        ("renderer ref", Box::new(|m| m["renderer_components"] = json!({"repository": "intentumdiff-core", "ref": "main", "artifact": "renderer-components",
            "wasm_checksums": {"html_renderer.wasm": "a".repeat(64), "llm_renderer.wasm": "a".repeat(64), "patch_renderer.wasm": "a".repeat(64), "terminal_renderer.wasm": "a".repeat(64)}})), "/renderer_components/ref".to_owned()),
    ];
    for (name, mutate, expected) in cases {
        let mut manifest = audit_base();
        mutate(&mut manifest);
        let found = paths(&manifest);
        assert_eq!(found, vec![expected.clone()], "{name}: {:?}", validate_registry_manifest(&manifest));
    }
}

#[test]
fn community_policy_is_explicit() {
    // Community: optional fields may be absent; present fields must be well-formed.
    let minimal = json!({"version": 1, "plugins": {"intentumdiff-x": {"trust_tier": "community"}}});
    assert_eq!(validate_registry_manifest(&minimal), vec![]);
    let branch = json!({"version": 1, "plugins": {"intentumdiff-x": {"trust_tier": "community", "ref": "main", "repo": "https://example.invalid/x.git"}}});
    assert_eq!(validate_registry_manifest(&branch), vec![]);
    let pypi_latest = json!({"version": 1, "plugins": {"intentumdiff-x": {"trust_tier": "community", "source": "pypi", "ref": ""}}});
    assert_eq!(validate_registry_manifest(&pypi_latest), vec![]);
    for (field, value) in [("ref", json!("../x")), ("abi_target", json!("v1")), ("capabilities", json!(["shell"])),
                           ("wasm_checksums", json!({"x.wasm": "A".repeat(64)}))] {
        let mut manifest = minimal.clone();
        manifest["plugins"]["intentumdiff-x"][field] = value;
        assert_eq!(validate_registry_manifest(&manifest).len(), 1, "{field}");
    }
    // Official pypi entries need an exact version.
    let mut pypi = audit_base();
    pypi["plugins"]["intentumdiff-audit-parser"]["source"] = json!("pypi");
    pypi["plugins"]["intentumdiff-audit-parser"]["ref"] = json!("1.2.3");
    assert_eq!(validate_registry_manifest(&pypi), vec![]);
    pypi["plugins"]["intentumdiff-audit-parser"]["ref"] = json!("");
    assert_eq!(paths(&pypi), vec![format!("{ENTRY}/ref")]);
}

#[test]
fn bounded_wrong_type_matrix_never_panics() {
    let wrong = [json!(null), json!(true), json!(1), json!(-1.5), json!("s"), json!([]), json!([1, {}]), json!({}), json!({"k": []})];
    let base = audit_base();
    let fields: Vec<String> = base["plugins"]["intentumdiff-audit-parser"].as_object().unwrap().keys().cloned()
        .chain(["repo", "description", "dep_hashes", "allowed_dependencies", "provenance_manifest_ref"].map(String::from)).collect();
    for value in &wrong {
        for field in &fields {
            let mut manifest = base.clone();
            manifest["plugins"]["intentumdiff-audit-parser"][field] = value.clone();
            let _ = validate_registry_manifest(&manifest);
        }
        for top in ["version", "plugins", "renderer_components"] {
            let mut manifest = base.clone();
            manifest[top] = value.clone();
            let _ = validate_registry_manifest(&manifest);
        }
        let mut manifest = base.clone();
        manifest["plugins"]["intentumdiff-audit-parser"] = value.clone();
        assert!(!validate_registry_manifest(&manifest).is_empty(), "entry {value}");
        assert!(!validate_registry_manifest(value).is_empty());
    }
    // ABI: malformed JSON is a value_error envelope, not a crash.
    let bad: Value = serde_json::from_str(&dispatch("validate_registry_manifest", &[json!("{")])).unwrap();
    assert_eq!(bad["ok"], false);
    let shape: Value = serde_json::from_str(&dispatch("validate_registry_manifest", &[json!("[]")])).unwrap();
    assert_eq!(shape["result"], json!({"ok": false, "errors": [{"path": "", "message": "must be an object, got array"}]}));
}

#[test]
fn capability_vocabulary_is_the_wit_exports() {
    let envelope: Value = serde_json::from_str(&dispatch("validate_registry_manifest", &[json!(json!({"version": 1, "plugins": {"intentumdiff-x": {
        "trust_tier": "community", "capabilities": ["parser", "renderer", "enricher", "index-engine", "diff-analyzer", "host-utils"]}}}).to_string())])).unwrap();
    assert_eq!(envelope["result"]["errors"].as_array().unwrap().iter().map(|e| e["path"].as_str().unwrap()).collect::<Vec<_>>(),
               vec!["/plugins/intentumdiff-x/capabilities/5"], "host-utils is an import, not a capability");
}
