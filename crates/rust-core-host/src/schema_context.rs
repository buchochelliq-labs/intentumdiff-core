//! Native host profile loading. Selection and identity policy live in schema_profiles.
use crate::schema_profiles;
use serde_json::{json, Value};
use std::path::PathBuf;

pub(crate) fn resolve(
    config: &Value,
    filename: &str,
    language: &str,
    content: &str,
) -> Result<(Vec<String>, Value, Vec<crate::UserXmlDialect>), String> {
    // A non-Rust host may supply resolved schema bytes/identities; it must not need network in core.
    if let Some(context) = config.get("schema_context") {
        let mut identities: std::collections::BTreeSet<String> =
            serde_json::from_value(context.get("identity_fields").cloned().unwrap_or(json!([])))
                .map_err(|e| format!("schema identity fields: {e}"))?;
        if let Some(schema) = context.get("schema") {
            identities.extend(schema_profiles::derive_identity_fields(schema));
        }
        let fields: Vec<_> = identities.into_iter().collect();
        return Ok((
            fields.clone(),
            json!({"provider_id":context.get("provider_id").and_then(Value::as_str).unwrap_or("host"),"status":"host","identity_fields":fields}),
            Vec::new(),
        ));
    }
    let root = config
        .get("schema_repo_root")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir().map_err(|e| e.to_string())?);
    let mut dirs = vec![root.join(".intentumdiff/schemas")];
    if let Some(override_dir) = std::env::var_os("INTENTUMDIFF_USER_SCHEMA_DIR") {
        dirs.push(override_dir.into());
    } else if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))
    {
        dirs.push(PathBuf::from(home).join(".intentumdiff/schemas"));
    }
    let enabled = !matches!(
        std::env::var("INTENTUMDIFF_USER_SCHEMAS")
            .unwrap_or_default()
            .trim()
            .to_lowercase()
            .as_str(),
        "off" | "0" | "false"
    );
    let loaded = schema_profiles::load_local_profiles(if enabled { &dirs } else { &[] });
    let dialects = loaded
        .profiles
        .iter()
        .filter(|p| !p.keyed_elements.is_empty())
        .map(|p| crate::UserXmlDialect {
            language_id: p.language_id.clone(),
            root_element: p.root_element.clone(),
            namespace: p.namespace.clone(),
            keyed_elements: p
                .keyed_elements
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        })
        .collect();
    let result =
        schema_profiles::resolve_profile(&loaded.profiles, filename, Some(language), content);
    let (provider, status) = if let Some(index) = result.profile_index {
        (
            format!("user:{}", loaded.profiles[index].language_id),
            "user-profile",
        )
    } else {
        (
            result
                .candidate
                .as_ref()
                .map(|c| c.provider_id.clone())
                .unwrap_or_else(|| "none".into()),
            "local-hints",
        )
    };
    let fields: Vec<_> = result.identity_fields.into_iter().collect();
    Ok((
        fields.clone(),
        json!({"provider_id":provider,"status":status,"identity_fields":fields,"errors":loaded.errors}),
        dialects,
    ))
}

pub(crate) fn config_for_repo(config_json: &str, repo_path: &str) -> Result<String, String> {
    let mut config: Value = if config_json.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(config_json).map_err(|e| format!("config: {e}"))?
    };
    if !config.is_object() {
        return Err("config must be an object".into());
    }
    config["schema_repo_root"] = json!(repo_path);
    Ok(config.to_string())
}
