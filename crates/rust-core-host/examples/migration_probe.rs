use intentumdiff_rust_core::api::{review_sources, review_text, ReviewOptions};
use serde_json::Value;
use std::io::{self, Read};
use std::path::Path;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let request: Value = serde_json::from_str(&input)?;
    if request["handler"] == "cache_list_entries_filtered" {
        let q = &request["query"];
        let store = intentumdiff_rust_core::cache_store::SqliteStore::open(request["path"].as_str().ok_or("path")?, 30, 500).map_err(|e| format!("{e:?}"))?;
        let output = store.list_entries_filtered("diff_cache", q["language"].as_str(), q["since"].as_i64(), q["before"].as_i64(), q["min_size"].as_i64(), q["max_size"].as_i64(), q["limit"].as_i64().unwrap_or(50), q["file_glob"].as_str()).map_err(|e| format!("{e:?}"))?;
        println!("{output}");
        return Ok(());
    }
    if request["handler"] == "parser_candidate_shortlist" {
        let entries: Vec<intentumdiff_rust_core::parser_routing::Candidate> = serde_json::from_value(request["entries"].clone())?;
        let query = serde_json::from_value(request["query"].clone())?;
        println!("{}", serde_json::to_string(&intentumdiff_rust_core::parser_routing::shortlist(&entries, &query)?)?);
        return Ok(());
    }
    if request["handler"] == "match_ignore_rules" {
        let files: Vec<intentumdiff_rust_core::ignore_rules::IgnoreFile> = serde_json::from_value(request["files"].clone())?;
        let rules = intentumdiff_rust_core::ignore_rules::IgnoreRules::new(&files)?;
        println!("{}", rules.is_ignored(request["path"].as_str().ok_or("path")?, request["is_dir"].as_bool().unwrap_or(false))?);
        return Ok(());
    }
    if request["handler"] == "detect_content_type" {
        let bytes: Vec<u8> = serde_json::from_value(request["bytes"].clone())?;
        println!("{}", serde_json::to_string(&intentumdiff_rust_core::content_type::detect_content_type(&bytes))?);
        return Ok(());
    }
    if request["handler"] == "reconstruct_patch" {
        let result = intentumdiff_rust_core::patch_source::reconstruct(request["patch"].as_str().ok_or("patch")?, request["original"].as_str(), request["filename"].as_str(), request["require_complete"].as_bool().unwrap_or(false));
        let output = match result { Ok(result) => serde_json::to_value(result)?, Err(error) => serde_json::json!({"error":error}) };
        println!("{}", output);
        return Ok(());
    }
    if request["handler"] == "hover_targets_utf16" {
        println!("{}", serde_json::to_string(&intentumdiff_rust_core::lsp_enrich::collect_utf16_hover_targets(&request["tree"], request["source"].as_str().ok_or("source")?)?)?);
        return Ok(());
    }
    if request["handler"] == "hover_targets" {
        println!("{}", serde_json::to_string(&intentumdiff_rust_core::lsp_enrich::collect_hover_targets(&request["tree"]))?);
        return Ok(());
    }
    if request["handler"] == "resolve_references" {
        let definitions: Vec<intentumdiff_rust_core::symbol_index::SymbolDefinition> = serde_json::from_value(request["definitions"].clone())?;
        let references: Vec<intentumdiff_rust_core::symbol_index::ReferenceUsage> = serde_json::from_value(request["references"].clone())?;
        let result = intentumdiff_rust_core::symbol_index::resolve_references(&definitions, &references);
        println!("{}", serde_json::to_string(&result)?);
        return Ok(());
    }
    if request["handler"] == "complete_routed_review" {
        println!("{}", intentumdiff_rust_core::routed_review::complete(&request["request"])?);
        return Ok(());
    }
    if request["handler"] == "parse_guardrail_policy" {
        let result = intentumdiff_rust_core::guardrail_policy::parse_policy(request["source"].as_str().ok_or("source required")?);
        let output = match result {
            Ok(value) => serde_json::json!({"result": value}),
            Err(error) => serde_json::json!({"error": error}),
        };
        println!("{}", output);
        return Ok(());
    }
    let get = |key| {
        request
            .get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| format!("missing {key}"))
    };
    if request["handler"] == "infer_file_lifecycle" {
        let args = request["args"].as_array().ok_or("missing args")?;
        let result = intentumdiff_rust_core::lifecycle::infer(args[0].as_str().ok_or("old")?, args[1].as_str().ok_or("new")?, args[2].as_str());
        println!("{}", serde_json::to_string(&result)?);
        return Ok(());
    }
    if request["handler"] == "glob_profile" {
        use intentumdiff_rust_core::schema_profiles::{validate_descriptor, match_user_profile};
        let doc = serde_json::json!({"language_id":"test", "match":{"filename_patterns":[get("pattern")?]}, "identity_fields":["key"]});
        let profile = validate_descriptor(&doc, "profile.json", 0, "", None, None, None).map_err(|e| e.join("; "))?;
        println!("{}", match_user_profile(&[profile], get("filename")?, "{}", None).is_some());
        return Ok(());
    }
    let result = if request["mode"] == "text" {
        review_text(get("old")?, get("new")?, get("filename")?, get("filename")?)?
    } else {
        review_sources(
            Path::new(get("repo")?),
            Path::new(get("wasm")?),
            get("filename")?,
            get("old")?,
            get("new")?,
            &ReviewOptions::default(),
        )?
    };
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}
