//! Offline, independently specified benchmark gate. Run from any working directory.
mod benchmark_support;
use benchmark_support::*;
use intentumdiff_rust_core::{
    api::{review_sources, ReviewOptions},
    parse_to_tree,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};
fn main() {
    match run() {
        Ok(failed) => {
            if failed {
                std::process::exit(1)
            }
        }
        Err(error) => {
            eprintln!("external benchmark infrastructure: {error}");
            std::process::exit(2)
        }
    }
}
fn run() -> Result<bool> {
    let mut args = std::env::args().skip(1);
    let mut paths = BTreeMap::new();
    let mut strict = false;
    while let Some(arg) = args.next() {
        if arg == "--strict" {
            strict = true;
        } else if ["--fixtures", "--parsers", "--report", "--core-library"].contains(&arg.as_str())
        {
            paths.insert(arg, args.next().ok_or("missing argument value")?);
        } else {
            return Err(format!("unknown argument: {arg}"));
        }
    }
    let get = |key: &str| -> Result<PathBuf> {
        Ok(PathBuf::from(
            paths.get(key).ok_or_else(|| format!("required {key}"))?,
        ))
    };
    let fixtures = get("--fixtures")?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let parsers = get("--parsers")?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let report = get("--report")?;
    let (manifest, manifest_hash) = read_json_hashed(&fixtures.join("manifest.json"))?;
    let (ledger, ledger_hash) = read_json_hashed(&fixtures.join("ledger.json"))?;
    let (staged, staged_hash) = read_json_hashed(&parsers.join("parser_manifest.json"))?;
    for doc in [&manifest, &ledger, &staged] {
        if doc["schema_version"] != 1 {
            return Err("unsupported schema_version".into());
        }
    }
    let sources = array(&manifest, "sources")?;
    let cases = array(&manifest, "cases")?;
    let languages = array(&ledger, "languages")?;
    let components = array(&staged, "components")?;
    let source_ids = unique(sources, "id")?;
    unique(cases, "id")?;
    let language_ids = unique(languages, "language_id")?;
    unique(components, "component")?;
    if language_ids.len() != 74 {
        return Err(format!(
            "expected 74 ledger languages, got {}",
            language_ids.len()
        ));
    }
    let (inventory, inventory_hash) = read_json_hashed(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/external/inventory.json"),
    )?;
    validate_inventory(&inventory, &ledger, &staged)?;
    validate_pilot(cases)?;
    validate_declared_counts(&manifest)?;
    if cases.is_empty() || sources.is_empty() {
        return Err("empty benchmark corpus".into());
    }
    let mut provenance = BTreeMap::new();
    for source in sources {
        for key in ["url", "revision", "license"] {
            if string(source, key)?.trim().is_empty() {
                return Err(format!("empty source {key}"));
            }
        }
        let files = array(source, "files")?;
        unique(files, "path")?;
        for file in files {
            let path = string(file, "path")?;
            let hash = string(file, "sha256")?;
            checked_bytes(&fixtures, path, hash)?;
            provenance.insert((string(source, "id")?, path), hash);
        }
    }
    let mut artifacts = BTreeMap::new();
    for component in components {
        let name = string(component, "component")?;
        checked_bytes(&parsers, name, string(component, "sha256")?)?;
        artifacts.insert(name, component);
    }
    // Validate the entire corpus before executing any case: malformed admission is infrastructure.
    for case in cases {
        validate_expectation(case)?;
        let id = string(case, "id")?;
        let lang = string(case, "language_id")?;
        if !language_ids.contains(lang) {
            return Err(format!("{id}: unknown language {lang}"));
        }
        if !case["required"].is_boolean() {
            return Err(format!("{id}: required must be boolean"));
        }
        if ![
            "upstream_gold",
            "reviewed_external_derived",
            "external_seed_only",
            "handwritten_regression",
        ]
        .contains(&string(case, "class")?)
        {
            return Err(format!("{id}: unknown evidence class"));
        }
        let source = string(case, "source_id")?;
        if !source_ids.contains(source) {
            return Err(format!("{id}: unknown source"));
        }
        for (path, hash) in [("old_path", "old_sha256"), ("new_path", "new_sha256")] {
            let p = string(case, path)?;
            let h = string(case, hash)?;
            checked_bytes(&fixtures, p, h)?;
            if provenance.get(&(source, p)) != Some(&h) {
                return Err(format!("{id}: source provenance does not cover {p}"));
            }
        }
        string(case, "filename")?;
        string(case, "family")?;
        string(case, "upstream_task_id")?;
        if !["changed", "preserved", "unknown"].contains(&string(&case["oracle"], "behavior")?) {
            return Err(format!("{id}: invalid behavior oracle"));
        }
        string(&case["oracle"], "basis")?;
    }
    for language in languages {
        let name = string(language, "component")?;
        let language_id = string(language, "language_id")?;
        if staged["parsers"][language_id]["wasm"] != name {
            return Err(format!(
                "manifest routing mismatch for {language_id}: expected {name}"
            ));
        }
        if cases
            .iter()
            .any(|c| c["language_id"] == language["language_id"])
            && !artifacts.contains_key(name)
        {
            return Err(format!("missing required component {name}"));
        }
    }
    let options = ReviewOptions::default();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let library = paths
        .get("--core-library")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            executable.parent().unwrap().parent().unwrap().join(format!(
                "{}intentumdiff_rust_core{}",
                std::env::consts::DLL_PREFIX,
                std::env::consts::DLL_SUFFIX
            ))
        })
        .canonicalize()
        .map_err(|e| format!("core shared library: {e}; pass --core-library"))?;
    let cdylib_hash = digest(&fs::read(&library).map_err(|e| e.to_string())?);
    let executable_hash = digest(&fs::read(&executable).map_err(|e| e.to_string())?);
    let mut results = vec![];
    let mut failed = false;
    for case in cases {
        let old = String::from_utf8(checked_bytes(
            &fixtures,
            string(case, "old_path")?,
            string(case, "old_sha256")?,
        )?)
        .map_err(|e| e.to_string())?;
        let new = String::from_utf8(checked_bytes(
            &fixtures,
            string(case, "new_path")?,
            string(case, "new_sha256")?,
        )?)
        .map_err(|e| e.to_string())?;
        let filename = string(case, "filename")?;
        let first = review_sources(&repo, &parsers, filename, &old, &new, &options);
        let mut row = json!({"id":case["id"],"language_id":case["language_id"],"class":case["class"],"family":case["family"],"required":case["required"],"admitted":!case["expected"].is_null(),"case":case});
        match first {
            Err(error) => {
                row["status"] = json!("runtime_error");
                row["error"] = json!(error.to_string());
                failed = true;
            }
            Ok(review) => {
                let raw = serde_json::to_value(review).map_err(|e| e.to_string())?;
                let mut infra = vec![];
                let mut diagnostics = validate_review(&raw, &old, &new);
                let parser_errors = raw["parse_errors"]
                    .as_array()
                    .is_some_and(|errors| !errors.is_empty());
                row["parse_errors"] = raw
                    .get("parse_errors")
                    .cloned()
                    .unwrap_or_else(|| json!([]));
                if parser_errors {
                    infra.push(
                        "parser reported errors; result cannot be judged for accuracy".into(),
                    );
                }
                if raw["language"] != case["language_id"] {
                    infra.push(format!("wrong routed language: {}", raw["language"]));
                }
                if raw["is_fallback"] == true {
                    infra.push("fallback result is not a successful parser review".into());
                }
                let config = serde_json::to_string(&options).map_err(|e| e.to_string())?;
                for (side, source) in [("old", old.as_str()), ("new", new.as_str())] {
                    match parse_to_tree(filename, source, &config, &parsers.to_string_lossy()) {
                        Err(e) => infra.push(format!("{side} parse error: {e}")),
                        Ok(tree) => match serde_json::from_str::<Value>(&tree) {
                            Err(e) => infra.push(format!("{side} invalid tree JSON: {e}")),
                            Ok(parsed) => {
                                if parsed["language"] != case["language_id"] {
                                    infra.push(format!("{side} wrong tree language"));
                                }
                                diagnostics.extend(validate_review(&json!({"changes":[{"old_node":parsed["tree"],"new_node":null}],"change_groups":[]}),source,""));
                                row[format!("{side}_tree_sha256")] = json!(digest(tree.as_bytes()));
                                match parse_to_tree(
                                    filename,
                                    source,
                                    &config,
                                    &parsers.to_string_lossy(),
                                ) {
                                    Ok(second) => {
                                        if second != tree {
                                            infra.push(format!("{side} nondeterministic tree"));
                                        }
                                    }
                                    Err(e) => infra.push(format!("{side} repeat parse error: {e}")),
                                }
                            }
                        },
                    }
                }
                match review_sources(&repo, &parsers, filename, &old, &new, &options) {
                    Ok(second) => {
                        if deterministic_review(
                            &serde_json::to_value(second).map_err(|e| e.to_string())?,
                        ) != deterministic_review(&raw)
                        {
                            infra.push("nondeterministic review".into());
                        }
                    }
                    Err(e) => infra.push(format!("repeat runtime error: {e}")),
                }
                row["review"] = raw.clone();
                row["infrastructure_errors"] = json!(infra);
                let mut errors = if case["expected"].is_null() {
                    vec![]
                } else {
                    evaluate_case(case, &raw)
                };
                if case["family"] == "character_literal" {
                    errors.extend(validate_literal_facts(&case["expected"], &raw, &old, &new));
                }
                row["validation_errors"] = json!(diagnostics);
                if !case["expected"].is_null() {
                    errors.extend(diagnostics);
                }
                row["accuracy_errors"] = json!(errors);
                let status = if parser_errors {
                    failed = true;
                    "parse_error"
                } else if !infra.is_empty() {
                    failed = true;
                    "invalid_result"
                } else if case["expected"].is_null() {
                    "unadmitted"
                } else if errors.is_empty() {
                    "passed"
                } else {
                    if strict || case["required"] == true {
                        failed = true;
                    }
                    "accuracy_failure"
                };
                row["status"] = json!(status);
            }
        }
        results.push(row);
    }
    let mut summaries = vec![];
    for language in languages {
        let rows: Vec<_> = results
            .iter()
            .filter(|r| r["language_id"] == language["language_id"])
            .collect();
        let mut classes = BTreeMap::new();
        for class in [
            "upstream_gold",
            "reviewed_external_derived",
            "external_seed_only",
            "handwritten_regression",
        ] {
            let selected: Vec<_> = rows
                .iter()
                .copied()
                .filter(|r| r["class"] == class)
                .collect();
            classes.insert(class, summarize(&selected));
        }
        let mut summary = summarize(&rows);
        summary["language_id"] = language["language_id"].clone();
        summary["ledger"] = language.clone();
        summary["declared_cases"] = json!(cases
            .iter()
            .filter(|c| c["language_id"] == language["language_id"])
            .count());
        summary["evidence_classes"] = json!(classes);
        summaries.push(summary);
    }
    let git = |args: &[&str]| -> String {
        Command::new("git")
            .args(args)
            .current_dir(&repo)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
            .unwrap_or_else(|| "unavailable".into())
    };
    let output = json!({
        "schema_version": 1,
        "interface": "intentumdiff_rust_core::api::review_sources",
        "core": {
            "cdylib_path": library,
            "cdylib_sha256": cdylib_hash,
            "python_backend": "native tree-sitter after staged component metadata selection",
            "git_revision": git(&["rev-parse", "HEAD"]),
            "git_status": git(&["status", "--porcelain"]),
            "executable_sha256": executable_hash,
            "crate_version": env!("CARGO_PKG_VERSION")
        },
        "determinism_excluded_metadata_fields": [
            "duration_ms", "elapsed_ms", "wasm_component_cache_hit",
            "fuel_consumed", "total_fuel_consumed", "max_fuel_used_percent"
        ],
        "options": options,
        "parser_manifest": staged,
        "fixture_manifest_sha256": manifest_hash,
        "parser_manifest_sha256": staged_hash,
        "inventory_sha256": inventory_hash,
        "ledger_sha256": ledger_hash,
        "declared_cases": cases.len(),
        "executed_cases": results.len(),
        "strict": strict,
        "failed": failed,
        "languages": summaries,
        "cases": results
    });
    if let Some(parent) = report.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    fs::write(
        report,
        serde_json::to_vec_pretty(&output).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(failed)
}
