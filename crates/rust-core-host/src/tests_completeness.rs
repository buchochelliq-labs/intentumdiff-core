//! Core #252: an empty review over differing sources is `incomplete`, never a clean result.
//! Real staged parser components (INTENTUMDIFF_TEST_WASM_DIR) drive every C-ABI review path.
use super::*;

/// The Tier-C provisioner stages registry-verified Wasm but not the runtime parser_manifest.json,
/// and native selection is fail-closed without it. So build one isolated fixture dir holding the
/// EXACT staged component bytes these tests use plus a minimal test-only manifest.
fn wasm_dir() -> String {
    static DIR: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    let dir = DIR.get_or_init(|| {
        let staged = std::env::var("INTENTUMDIFF_TEST_WASM_DIR")
            .expect("Tier-C tests need INTENTUMDIFF_TEST_WASM_DIR (checksum-verified parser components)");
        let fixture = tempfile::tempdir().expect("isolated parser fixture");
        let entries: &[(&str, &str, &[&str])] = &[
            ("python", "python_parser.wasm", &[".py", ".pyi"]),
            ("go", "go_parser.wasm", &[".go"]),
            ("bash", "bash_parser.wasm", &[".sh"]),
            ("json", "json_parser.wasm", &[".json"]),
            ("yaml", "yaml_parser.wasm", &[".yaml", ".yml"]),
            ("toml", "toml_parser.wasm", &[".toml"]),
            ("javascript", "js_ts_parser.wasm", &[".js"]),
        ];
        let mut parsers = serde_json::Map::new();
        let mut index = serde_json::Map::new();
        for (language, wasm, extensions) in entries {
            let source = std::path::Path::new(&staged).join(wasm);
            assert!(source.is_file(), "verified component missing: {}", source.display());
            std::fs::copy(&source, fixture.path().join(wasm)).expect("copy verified component");
            parsers.insert((*language).into(), json!({"plugin_id": language, "wasm": wasm, "extensions": extensions}));
            for extension in *extensions {
                index.insert((*extension).into(), json!(language));
            }
        }
        std::fs::write(
            fixture.path().join("parser_manifest.json"),
            json!({"schema_version": 1, "parsers": parsers, "extension_index": index}).to_string(),
        )
        .expect("write test parser manifest");
        fixture
    });
    dir.path().to_str().expect("UTF-8 fixture path").to_owned()
}

fn call(name: &str, args: Value) -> Value {
    let envelope: Value = serde_json::from_str(&crate::c_abi::dispatch(name, args.as_array().unwrap())).unwrap();
    assert_eq!(envelope["ok"], true, "{name}: {envelope}");
    envelope["result"].clone()
}

fn config(language: &str) -> String {
    json!({"language_hint": language, "strict_plugins": true}).to_string()
}

fn live(path: &str, language: &str, old: &str, new: &str) -> Value {
    let result = call("live_diff_contents", json!([".", path, old, new, config(language), wasm_dir()]));
    assert!(result.get("fallback").is_none(), "{path}: {result}");
    result["diff"].clone()
}

/// A tree as a binding prepares it for `finalize_review`: parse, then the literal and profile
/// label enrichment ops the native chain runs.
fn tree(path: &str, language: &str, source: &str) -> Value {
    let parsed = call("parse_to_tree", json!([path, source, config(language), wasm_dir()]))["tree"].clone();
    let literal = call("enrich_literal_labels", json!([parsed.to_string(), source]));
    call("enrich_profile_labels", json!([literal.to_string(), source, language]))
}

/// The routed boundary as a binding drives it: given finalized output, complete the review.
fn complete_with(path: &str, language: &str, old: &str, new: &str, finalized: Value) -> Value {
    call("complete_routed_review", json!([json!({"language":language,"old_source":old,"new_source":new,
        "old_filename":path,"new_filename":path,"old_tree":tree(path, language, old),
        "new_tree":tree(path, language, new),"finalized":finalized}).to_string()]))
}

fn assert_incomplete(diff: &Value, language: &str) {
    assert_eq!(diff["completeness"], "incomplete", "{diff}");
    assert_eq!(diff["changes"], json!([]), "no fabricated semantic changes: {diff}");
    assert_eq!(diff["has_semantic_changes"], false, "{diff}");
    assert_eq!(diff["is_style_only"], false, "{diff}");
    let incomplete = &diff["incomplete"];
    assert_eq!(incomplete["reason"], crate::completeness::REASON_UNEXPLAINED_SOURCE_DELTA);
    assert_eq!(incomplete["language"], language);
    let message = incomplete["message"].as_str().unwrap();
    assert!(message.contains("could not explain") && message.contains("report a bug"), "{message}");
    assert!(message.contains(crate::completeness::REPORT_URL) && message.contains(language));
    assert!(incomplete["report_url"].as_str().unwrap().starts_with(crate::completeness::REPORT_URL));
    assert!(!incomplete["source_ranges"].as_array().unwrap().is_empty(), "honest source evidence: {diff}");
}

/// CS-01 (executable reorder) and CS-02 (raw/bytes prefix edit) from the audit. Today's
/// suppressors keep their changes; the boundary must still fail closed if one regresses and
/// suppresses every draft, which is simulated by an empty finalized result.
#[test]
fn cs01_cs02_suppressed_to_empty_are_incomplete_with_evidence_and_message() {
    for (path, language, old, new) in [
        ("a.py", "python", "x = 1\nx += 2\nx *= 3\n", "x = 1\nx *= 3\nx += 2\n"),
        ("a.js", "javascript", "let x = 1;\nx += 2;\nx *= 3;\n", "let x = 1;\nx *= 3;\nx += 2;\n"),
        ("a.py", "python", "x = r\"\\n\"\n", "x = \"\\n\"\n"),
        ("a.py", "python", "x = b\"safe\"\n", "x = \"safe\"\n"),
    ] {
        let served = live(path, language, old, new);
        assert_eq!(served["completeness"], "complete", "{path} {old:?}: {served}");
        assert!(!served["changes"].as_array().unwrap().is_empty(), "{served}");
        assert!(served["incomplete"].is_null(), "{served}");
        let regressed = complete_with(path, language, old, new,
            json!({"changes":[],"change_groups":[],"is_style_only":false,"no_surviving_changes":true}));
        assert_incomplete(&regressed, language);
        assert_eq!(regressed["metadata"]["no_surviving_changes"], true, "{regressed}");
        let lines = &regressed["incomplete"]["source_ranges"][0];
        assert!(lines["old_end_byte"].as_u64().unwrap() <= old.len() as u64, "{lines}");
        assert!(lines["new_end_byte"].as_u64().unwrap() <= new.len() as u64, "{lines}");
    }
}

/// Real inputs that today lose every draft without an equivalence proof (b): order-sensitive
/// statements reordered. Incomplete is the correct, honest result until the suppressor learns.
#[test]
fn real_suppressed_reorders_are_incomplete_on_every_c_abi_path() {
    for (path, language, old, new) in [
        ("a.go", "go", "package m\nfunc f() {\n\ta()\n\tb()\n}\n", "package m\nfunc f() {\n\tb()\n\ta()\n}\n"),
        ("a.sh", "bash", "x=1\nx=2\n", "x=2\nx=1\n"),
        ("a.json", "json", "{\"a\": [1, 2]}\n", "{\"a\": [2, 1]}\n"),
    ] {
        let served = live(path, language, old, new);
        assert_incomplete(&served, language);
        // Positions only, from the shared source-evidence diff.
        let range = &served["incomplete"]["source_ranges"][0];
        assert!(range["old_start_line"].as_u64().is_some() && range["new_end_byte"].as_u64().is_some(), "{range}");
        // finalize_review (the intermediate a binding feeds back) states the same verdict.
        let (old_tree, new_tree) = (tree(path, language, old), tree(path, language, new));
        let finalized = call("finalize_review", json!([old_tree.to_string(),
            new_tree.to_string(), old, new, language, "{}"]));
        assert_eq!(finalized["completeness"], "incomplete", "{path}: {finalized}");
        let routed = complete_with(path, language, old, new, finalized);
        assert_incomplete(&routed, language);
        assert_eq!(routed["incomplete"]["source_ranges"], served["incomplete"]["source_ranges"]);
        // Source-less lifecycle finalization keeps the sourced verdict and its evidence.
        let lifecycle = call("finalize_file_lifecycle", json!([served, "modified"]));
        assert_eq!(lifecycle["completeness"], "incomplete", "{lifecycle}");
        assert_eq!(lifecycle["incomplete"], served["incomplete"]);
        // CLI presentation renders the core's text, never "No changes detected".
        let cli = call("render_cli_review", json!([json!({"diff":served,"width":120,"color":false}).to_string()]));
        let cli = cli.as_str().unwrap();
        assert!(cli.contains("Incomplete review") && cli.contains("could not explain"), "{cli}");
        assert!(!cli.contains("No changes detected"), "{cli}");
    }
}

/// (a) Suppressions that PROVE equivalence: member order of specification-unordered mappings.
#[test]
fn unordered_mapping_member_order_is_equivalent_not_incomplete() {
    for (path, language, old, new) in [
        ("a.json", "json", "{\"a\": 1, \"b\": 2}\n", "{\"b\": 2, \"a\": 1}\n"),
        ("a.yaml", "yaml", "a: 1\nb: 2\n", "b: 2\na: 1\n"),
        ("a.yaml", "yaml", "x:\n  a: 1\n  b: 2\n", "x:\n  b: 2\n  a: 1\n"),
        ("a.toml", "toml", "[a]\nx = 1\n[b]\ny = 2\n", "[b]\ny = 2\n[a]\nx = 1\n"),
    ] {
        let served = live(path, language, old, new);
        assert_eq!(served["completeness"], "equivalent", "{path} {old:?}: {served}");
        assert_eq!(served["is_style_only"], false, "{served}");
        assert!(served["incomplete"].is_null(), "{served}");
        assert_eq!(served["metadata"]["equivalence_proof"], crate::keyed_data::UNORDERED_MAPPING_RULE);
        assert!(served["metadata"].get("no_surviving_changes").is_none(), "{served}");
    }
    // Controls: values that move between keys, duplicate keys, ordered sequences and YAML
    // anchors are never proved equivalent.
    for (path, language, old, new) in [
        ("a.json", "json", "{\"a\": 1, \"b\": 2}\n", "{\"b\": 1, \"a\": 2}\n"),
        ("a.json", "json", "{\"a\": 1, \"a\": 2}\n", "{\"a\": 2, \"a\": 1}\n"),
        ("a.yaml", "yaml", "a: &k 1\nb: *k\n", "b: *k\na: &k 1\n"),
        ("a.toml", "toml", "[a]\nx = 1\n[b]\ny = 2\n", "[a]\ny = 2\n[b]\nx = 1\n"),
    ] {
        let served = live(path, language, old, new);
        assert_ne!(served["completeness"], "equivalent", "{path} {old:?}: {served}");
        assert!(served["metadata"].get("equivalence_proof").is_none(), "{served}");
    }
}

/// True unchanged input is `complete` with no message; proved style-only stays style-only and
/// is `equivalent`; a real change is `complete`. Every review path agrees.
#[test]
fn unchanged_style_and_changed_states_agree_across_paths() {
    for (old, new, state, style) in [
        ("x = 1\n", "x = 1\n", "complete", None),
        ("x = 1\n", "x  =  1\n", "equivalent", Some(true)),
        ("x = 1\n", "x = 2\n", "complete", Some(false)),
    ] {
        let served = live("a.py", "python", old, new);
        assert_eq!(served["completeness"], state, "{old:?}->{new:?}: {served}");
        assert!(served["incomplete"].is_null(), "{served}");
        if let Some(style) = style { assert_eq!(served["is_style_only"], style, "{served}"); }
        let file = json!({"old_filename":"a.py","new_filename":"a.py","language":"python",
            "old_source":old,"new_source":new,"parser_plugin_id":"python","parser_wasm_path":""});
        let batch = call("diff_batch", json!([json!({"files":[file.clone()],
            "python_parser_backend":"native","config":{}}).to_string()]));
        // The certified batch declines style-only pairs (served by the routed review above);
        // every DTO it does serve carries the same state.
        if batch["diffs"][0]["status"] == "complete" {
            let diff = &batch["diffs"][0]["diff"];
            assert_eq!(diff["completeness"], state, "batch {old:?}: {batch}");
            assert!(diff["incomplete"].is_null(), "{diff}");
        } else {
            assert_eq!(state, "equivalent", "only style-only declines: {batch}");
        }
        let commit = crate::diff_batch_commit_json_impl(&json!({"schema_version":1,"mode":"commit_json",
            "old_ref":"HEAD","new_ref":"","python_parser_backend":"native","config":{},"parallel":false,
            "files":[file]}).to_string()).unwrap();
        match commit.1 {
            Some(bytes) => {
                let commit: Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(commit["file_diffs"][0]["completeness"], state, "commit {old:?}: {commit}");
            }
            None => assert_eq!(state, "equivalent", "only style-only declines: {}", commit.0),
        }
    }
    let text = call("review_text", json!(["a\n", "a\n", "a.txt", "a.txt"]));
    assert_eq!(text["completeness"], "complete", "{text}");
    let text = call("review_text", json!(["a\n", "b\n", "a.txt", "a.txt"]));
    assert_eq!(text["completeness"], "complete", "{text}");
    let fallback = call("source_fallback_diff", json!(["def f(", "def g(", "a.py", "a.py", "python", "parse_errors"]));
    assert_eq!(fallback["completeness"], "complete", "{fallback}");
}

const SECRETS: [&str; 6] = ["SECRET_TOKEN_xyz", "hunter2", "private note", "Private docstring", "helper_qq", "4242"];

fn assert_private(value: &Value, what: &str) {
    let text = value.to_string();
    for secret in SECRETS {
        assert!(!text.contains(secret), "{what} leaks {secret:?}: {text}");
    }
    // Percent-encoding must not smuggle them either.
    let decoded = value["report_url"].as_str().map(decode).unwrap_or_default();
    for secret in SECRETS {
        assert!(!decoded.contains(secret), "{what} URL leaks {secret:?}: {decoded}");
    }
}

fn decode(url: &str) -> String {
    let bytes = url.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() + 0 && i + 2 <= bytes.len() - 1 {
            out.push(u8::from_str_radix(&url[i + 1..i + 3], 16).unwrap());
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap()
}

/// The repro outline: sanitised, consistent placeholders, deterministic, plugin-attributed.
#[test]
fn incomplete_report_outline_is_private_consistent_and_deterministic() {
    let old = "package m\n\n// private note\nfunc helper_qq() {\n\tSECRET_TOKEN_xyz(\"hunter2\", 4242)\n\thelper_qq()\n}\n";
    let new = "package m\n\n// private note\nfunc helper_qq() {\n\thelper_qq()\n\tSECRET_TOKEN_xyz(\"hunter2\", 4242)\n}\n";
    let served = live("a.go", "go", old, new);
    assert_incomplete(&served, "go");
    let incomplete = &served["incomplete"];
    assert_private(incomplete, "go incomplete");
    let report = &incomplete["report"];
    let outline = &report["outline"];
    assert_eq!(outline["plugin"]["id"], "go", "{outline}");
    assert!(outline["plugin"]["version"].as_str().unwrap().starts_with("sha256:"), "{outline}");
    assert_eq!(outline["core_version"], crate::VERSION);
    assert_eq!(report["truncated"], false);
    let url = incomplete["report_url"].as_str().unwrap();
    assert_eq!(url, report["issue_url"].as_str().unwrap());
    assert!(url.len() <= crate::repro_outline::MAX_ISSUE_URL_BYTES);
    assert!(decode(url).ends_with(report["body_markdown"].as_str().unwrap()), "URL carries the body");
    let region = &outline["regions"][0];
    let shapes = format!("{} {}", region["old_shape"], region["new_shape"]);
    assert!(region["old_skeleton"].as_str().unwrap().contains("function_declaration"), "{region}");
    assert!(shapes.contains("string(9)"), "literal reduced to type and length: {shapes}");
    // One placeholder per identifier across both sides of the report.
    let ids: std::collections::BTreeSet<&str> = shapes.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| t.starts_with("id") && t[2..].chars().all(|c| c.is_ascii_digit()) && t.len() > 2).collect();
    assert_eq!(ids.len(), 2, "SECRET_TOKEN_xyz and helper_qq: {shapes}");
    assert_eq!(outline["placeholder_count"], 2, "{outline}");
    let suppressions = outline["suppressions"].as_array().expect("finalize records its passes");
    assert!(suppressions.iter().any(|p| !p["removed"].as_array().unwrap().is_empty()), "{outline}");
    // Byte-identical on repeat.
    assert_eq!(live("a.go", "go", old, new)["incomplete"].to_string(), incomplete.to_string());

    // Python via the routed boundary (a regressed suppressor), with docstrings and comments.
    let old = "def helper_qq():\n    \"\"\"Private docstring.\"\"\"\n    SECRET_TOKEN_xyz = \"hunter2\"  # private note\n    return SECRET_TOKEN_xyz\n";
    let new = "def helper_qq():\n    \"\"\"Private docstring.\"\"\"\n    SECRET_TOKEN_xyz = \"hunter2!\"  # private note\n    return SECRET_TOKEN_xyz\n";
    let regressed = complete_with("a.py", "python", old, new,
        json!({"changes":[],"change_groups":[],"is_style_only":false,"no_surviving_changes":true,"suppression_trace":[]}));
    assert_incomplete(&regressed, "python");
    assert_private(&regressed["incomplete"], "python incomplete");
    let shape = regressed["incomplete"]["report"]["outline"]["regions"][0]["old_shape"].to_string();
    assert!(shape.contains("string(9)") && !shape.contains("string(21)"), "docstring dropped, literal typed: {shape}");
}
