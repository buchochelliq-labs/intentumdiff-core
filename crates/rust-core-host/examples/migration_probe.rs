use intentumdiff_rust_core::api::{review_sources, review_text, ReviewOptions};
use serde_json::Value;
use std::io::{self, Read};
use std::path::Path;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let request: Value = serde_json::from_str(&input)?;
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
