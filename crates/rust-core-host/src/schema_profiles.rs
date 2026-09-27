//! Pure schema discovery and user profile interpretation; hosts provide all I/O.
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn declared_schema_and_unicode_keys() {
        assert_eq!(
            discover_declared_schema(r#"{"$schema":" https://example.org/é "}"#, Some("json")),
            Some("https://example.org/é".into())
        );
        assert_eq!(
            discover_declared_schema(
                "# yaml-language-server: $schema=https://example.org/a\nx: 1",
                Some("yaml")
            ),
            Some("https://example.org/a".into())
        );
        assert_eq!(
            discover_declared_schema(
                "\n\n\n\n\n# yaml-language-server: $schema=https://late",
                None
            ),
            None
        );
        let fields = derive_identity_fields(
            &json!({"$defs":{"Entry":{"properties":{"task-key":{},"name":{},"body":{}}}}}),
        );
        assert_eq!(fields, ["name".into(), "task_key".into()].into());
    }
    #[test]
    fn profile_url_precedence_custom_identity_and_errors() {
        let first = validate_descriptor(&json!({"language_id":"acme", "match":{"filename_patterns":["*.json"]},"keyed_arrays":{"/routes":["路径","method"]}}),"profile.yml",0,"",None,None,None).unwrap();
        let second = validate_descriptor(&json!({"language_id":"claimed", "match":{"schema_urls":["https://claimed"]},"identity_fields":["uuid"]}),"profile.yml",1,"",None,None,None).unwrap();
        assert!(first.identity_fields.contains("路径"));
        assert_eq!(
            match_user_profile(&[first, second], "routes.json", "", Some("https://claimed")),
            Some(1)
        );
        let errors=validate_descriptor(&json!({"language_id":"dbt:bad", "match":{"filename_patterns":["*.json"]},"identity_fields":[1]}),"bad.yml",0,"",None,None,None).unwrap_err();
        assert!(errors.iter().any(|e| e.contains("collides")));
        assert!(errors.iter().any(|e| e.contains("non-empty strings")));
    }
    #[test]
    fn json_adapter_matches_typed_resolution_and_rejects_bad_request() {
        let request = json!({"operation":"resolve","filename":"openapi.yaml","language":"yaml","content":"openapi: 3.1.0"});
        let actual: Value =
            serde_json::from_str(&schema_profiles_impl(&request.to_string()).unwrap()).unwrap();
        assert_eq!(
            actual,
            serde_json::to_value(resolve_profile(
                &[],
                "openapi.yaml",
                Some("yaml"),
                "openapi: 3.1.0"
            ))
            .unwrap()
        );
        assert!(schema_profiles_impl("{").is_err());
        assert!(schema_profiles_impl(r#"{"operation":"unknown"}"#).is_err());
    }
    #[test]
    fn local_loader_first_registration_wins_and_bad_file_is_atomic() {
        let dir =
            std::env::temp_dir().join(format!("intentumdiff-schema-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("a.yml"),
            "language_id: acme\nmatch: {filename_patterns: ['*.json']}\nidentity_fields: [route]\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("b.yml"),
            "language_id: acme\nmatch: {filename_patterns: ['*.json']}\nidentity_fields: [wrong]\n",
        )
        .unwrap();
        std::fs::write(dir.join("c.yml"),"language_id: partial\nmatch: {filename_patterns: ['*.json']}\nidentity_fields: [bad]\n---\nlanguage_id: invalid\n").unwrap();
        let loaded = load_local_profiles(&[dir.clone()]);
        std::fs::remove_dir_all(dir).unwrap();
        assert_eq!(loaded.profiles.len(), 1);
        assert_eq!(loaded.profiles[0].identity_fields, ["route".into()].into());
        assert!(!loaded.errors.is_empty());
    }
    #[test]
    fn generic_yaml_and_nested_keys_are_not_dbt_profiles() {
        for content in [
            "version: 1\nservices: {web: {image: nginx}}",
            "version: 2\nservices: {web: {image: nginx}}",
            "application:\n  models: 42",
            "version: 2\nmodels: 42",
        ] {
            assert!(
                provider_schema_candidate("config.yml", Some("yaml"), content).is_none(),
                "{content}"
            );
        }
        assert_eq!(
            provider_schema_candidate("models/schema.yml", Some("yaml"), "version: 2\nmodels: []")
                .unwrap()
                .provider_id,
            "dbt:dbt_yml_files"
        );
        assert_eq!(
            provider_schema_candidate("dbt_project.yml", Some("yaml"), "")
                .unwrap()
                .provider_id,
            "dbt:dbt_project"
        );
        assert_eq!(
            provider_schema_candidate("custom.yml", Some("dbt-yaml"), "")
                .unwrap()
                .provider_id,
            "dbt:dbt_yml_files"
        );
    }
    #[test]
    fn user_root_markers_require_top_level_keys() {
        let p=validate_descriptor(&json!({"language_id":"acme","match":{"root_markers":["models"]},"identity_fields":["name"]}),"p.yml",0,"",None,None,None).unwrap();
        for content in [
            "application:\n  models: []",
            "- models: []",
            "application:\n  models: [broken",
        ] {
            assert_eq!(
                match_user_profile(&[p.clone()], "a.yml", content, None),
                None
            );
        }
        assert_eq!(
            match_user_profile(&[p.clone()], "a.yml", "'models': []", None),
            Some(0)
        );
        assert_eq!(
            match_user_profile(&[p], "a.yml", "  models: []", None),
            Some(0)
        );
    }
    #[test]
    fn descriptor_yaml_interpretation_is_unambiguous() {
        let docs = parse_descriptor_documents("identity_fields: [on, off, yes, no]\n").unwrap();
        assert_eq!(
            docs[0]["identity_fields"],
            json!(["on", "off", "yes", "no"])
        );
        assert!(parse_descriptor_documents("language_id: first\nlanguage_id: second").is_err());
        assert!(parse_descriptor_documents("keyed_arrays: {1: [name]}").is_err());
        let response:Value=serde_json::from_str(&schema_profiles_impl(&json!({"operation":"parse_documents","raw_text":"identity_fields: [on, off, yes, no]"}).to_string()).unwrap()).unwrap();
        assert_eq!(response["documents"], json!(docs));
    }
    #[test]
    fn provider_expected_contract() {
        assert_eq!(
            provider_schema_candidate("openapi.yaml", Some("yaml"), "openapi: 3.1.0")
                .unwrap()
                .provider_id,
            "openapi:3.1"
        );
        assert_eq!(
            provider_schema_candidate("C:\\repo\\.github\\workflows\\ci.yml", Some("yaml"), "")
                .unwrap()
                .provider_id,
            "github-actions:workflow"
        );
        assert!(
            provider_schema_candidate("notes.yml", Some("yaml"), "name: docs\nitems: []").is_none()
        );
        assert_eq!(
            provider_schema_candidate(
                "manifest.json",
                Some("json"),
                r#"{"apiVersion":"v1","kind":"Pod","metadata":{"name":"web"}}"#
            )
            .unwrap()
            .provider_id,
            "kubernetes:manifest"
        );
    }
}
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchemaCandidate {
    pub provider_id: String,
    pub url: Option<String>,
    #[serde(default)]
    pub command: Vec<String>,
    pub status: Option<String>,
    pub advisory_url: Option<String>,
    #[serde(default)]
    pub identity_fields: BTreeSet<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserSchemaProfile {
    pub language_id: String,
    #[serde(default)]
    pub source_path: String,
    #[serde(default)]
    pub filename_patterns: Vec<String>,
    #[serde(default)]
    pub root_markers: Vec<String>,
    #[serde(default)]
    pub schema_urls: Vec<String>,
    #[serde(default)]
    pub identity_fields: BTreeSet<String>,
    #[serde(default)]
    pub important_paths: Vec<String>,
    #[serde(default)]
    pub scaffold_paths: Vec<String>,
    pub schema: Option<Value>,
    pub schema_path: Option<String>,
    #[serde(default)]
    pub keyed_elements: BTreeMap<String, Vec<String>>,
    pub namespace: Option<String>,
    pub root_element: Option<String>,
    #[serde(default)]
    pub fingerprint: String,
}
fn matches(pattern: &str, content: &str) -> bool {
    Regex::new(pattern).is_ok_and(|r| r.is_match(content))
}
fn marker(content: &str, key: &str) -> bool {
    if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(content) {
        return map.contains_key(key);
    }
    matches(
        &format!(r#"(?m)^\s*"?{}"?\s*:"#, regex::escape(key)),
        content,
    )
}
// Profile markers describe document roots, not arbitrary indented text. Parsing
// also handles quoted Unicode keys and indented root mappings. Malformed inputs
// provide no evidence and therefore cannot claim a user profile.
fn root_markers_match(content: &str, keys: &[String]) -> bool {
    if keys.is_empty() {
        return true;
    }
    let Ok(Value::Object(map)) = serde_yaml::from_str::<Value>(content) else {
        return false;
    };
    keys.iter().all(|key| map.contains_key(key))
}
fn has_dbt_schema_shape(content: &str) -> bool {
    let Ok(Value::Object(map)) = serde_yaml::from_str::<Value>(content) else {
        return false;
    };
    let version_two = map
        .get("version")
        .is_some_and(|v| v.as_u64() == Some(2) || v.as_str() == Some("2"));
    version_two
        && [
            "models",
            "sources",
            "exposures",
            "seeds",
            "snapshots",
            "metrics",
            "semantic_models",
            "groups",
            "unit_tests",
        ]
        .iter()
        .any(|key| {
            map.get(*key)
                .and_then(Value::as_array)
                .is_some_and(|items| items.iter().all(Value::is_object))
        })
}
fn scalar(content: &str, key: &str) -> String {
    if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(content) {
        return map
            .get(key)
            .filter(|v| !v.is_null())
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| v.to_string())
                    .trim()
                    .to_owned()
            })
            .unwrap_or_default();
    }
    Regex::new(&format!(
        r#"(?m)^{}\s*:\s*['"]?([^'"\n#]+)"#,
        regex::escape(key)
    ))
    .unwrap()
    .captures(content)
    .map(|c| c[1].trim().to_string())
    .unwrap_or_default()
}
/// Extract a declared JSON schema URL or a YAML language-server modeline in the first five lines.
pub fn discover_declared_schema(content: &str, language: Option<&str>) -> Option<String> {
    if matches!(
        language.unwrap_or("").to_lowercase().as_str(),
        "json" | "adf"
    ) || content.trim_start().starts_with('{')
    {
        if let Ok(v) = serde_json::from_str::<Value>(content) {
            if let Some(s) = v
                .get("$schema")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                return Some(s.into());
            }
        }
    }
    let re = Regex::new(r"^\s*#\s*yaml-language-server:\s*\$schema=(\S+)\s*$").unwrap();
    content
        .lines()
        .take(5)
        .find_map(|line| re.captures(line).map(|c| c[1].into()))
}
fn candidate(id: &str, url: &str, fields: &[&str]) -> SchemaCandidate {
    SchemaCandidate {
        provider_id: id.into(),
        url: Some(url.into()),
        command: vec![],
        status: None,
        advisory_url: None,
        identity_fields: fields.iter().map(|s| (*s).into()).collect(),
    }
}
/// Built-in provider hints. This operation never fetches URLs or runs commands.
pub fn provider_schema_candidate(
    filename: &str,
    language: Option<&str>,
    content: &str,
) -> Option<SchemaCandidate> {
    let path = filename.replace('\\', "/").to_lowercase();
    let base = path.rsplit('/').next().unwrap_or("");
    let lang = language.unwrap_or("").to_lowercase();
    let yaml = base.ends_with(".yaml") || base.ends_with(".yml");
    let structured = yaml || base.ends_with(".json");
    if structured {
        let version = scalar(content, "openapi");
        if !version.is_empty()
            || !scalar(content, "swagger").is_empty()
            || [
                "openapi.json",
                "openapi.yaml",
                "openapi.yml",
                "swagger.json",
                "swagger.yaml",
                "swagger.yml",
            ]
            .contains(&base)
        {
            return Some(if version.starts_with("3.1") {
                candidate(
                    "openapi:3.1",
                    "https://spec.openapis.org/oas/3.1/schema/2022-10-07",
                    &["operationId", "name"],
                )
            } else {
                candidate(
                    "openapi:3.0",
                    "https://spec.openapis.org/oas/3.0/schema/2021-09-28",
                    &["operationId", "name"],
                )
            });
        }
        if !scalar(content, "apiVersion").is_empty()
            && !scalar(content, "kind").is_empty()
            && marker(content, "metadata")
        {
            return Some(candidate(
                "kubernetes:manifest",
                "https://json.schemastore.org/kubernetes.json",
                &["name"],
            ));
        }
    }
    if yaml
        && (format!("/{path}").contains("/.github/workflows/")
            || (marker(content, "on")
                && marker(content, "jobs")
                && (marker(content, "uses") || marker(content, "runs-on"))))
    {
        return Some(candidate(
            "github-actions:workflow",
            "https://json.schemastore.org/github-workflow.json",
            &["id", "name", "run", "runs-on", "uses"],
        ));
    }
    if yaml
        && (["azure-pipelines.yml", "azure-pipelines.yaml"].contains(&base)
            || ((marker(content, "trigger") || marker(content, "pr"))
                && ["stages", "jobs", "steps"]
                    .iter()
                    .any(|k| marker(content, k))
                && ["pool", "vmImage", "task"]
                    .iter()
                    .any(|k| marker(content, k))))
    {
        return Some(candidate(
            "azure-pipelines:pipeline",
            "https://json.schemastore.org/azure-pipelines.json",
            &["job", "stage", "task", "script", "displayName"],
        ));
    }
    let stem = base
        .strip_suffix(".yml")
        .or_else(|| base.strip_suffix(".yaml"))
        .unwrap_or("");
    let dbt = if [
        "dbt_project",
        "packages",
        "dependencies",
        "selectors",
        "dbt_cloud",
    ]
    .contains(&stem)
    {
        Some(stem)
    } else if lang == "dbt-config" {
        Some("dbt_project")
    } else if lang == "dbt-packages" {
        Some("packages")
    } else if lang == "dbt-yaml" || (yaml && has_dbt_schema_shape(content)) {
        Some("dbt_yml_files")
    } else {
        None
    };
    if let Some(key) = dbt {
        return Some(candidate(&format!("dbt:{key}"),&format!("https://raw.githubusercontent.com/dbt-labs/dbt-jsonschema/main/schemas/latest/{key}-latest.json"),&["exposure","macro","metric","model","models","name","package_name","packages","project_name","resource_type","selector","selectors","seed","snapshot","snapshots","source","sources","test","tests","version"]));
    }
    if ["databricks", "databricks-workflow"].contains(&lang.as_str())
        || ["databricks.yml", "databricks.yaml"].contains(&base)
    {
        let mut c = candidate(
            "databricks:bundle",
            "https://raw.githubusercontent.com/databricks/cli/main/bundle/schema/jsonschema.json",
            &[
                "depends_on",
                "job_cluster_key",
                "library",
                "libraries",
                "task_key",
            ],
        );
        c.command = vec!["databricks".into(), "bundle".into(), "schema".into()];
        return Some(c);
    }
    if lang == "adf"
        || [
            "pipeline.json",
            "dataset.json",
            "linkedservice.json",
            "factory.json",
        ]
        .contains(&base)
        || [
            ".pipeline.json",
            ".dataset.json",
            ".linkedservice.json",
            ".trigger.json",
        ]
        .iter()
        .any(|s| base.ends_with(s))
    {
        let mut c = candidate("adf:no_raw_schema", "", &[]);
        c.url = None;
        c.status = Some("no_raw_schema".into());
        c.advisory_url=Some("https://learn.microsoft.com/en-sg/answers/questions/2125264/are-there-json-schemas-for-adf-source-files".into());
        return Some(c);
    }
    None
}
/// Derive conservative identity hints, including schemas nested in definitions.
pub fn derive_identity_fields(schema: &Value) -> BTreeSet<String> {
    let mut result = BTreeSet::new();
    let mut stack = vec![schema];
    while let Some(v) = stack.pop() {
        match v {
            Value::Object(m) => {
                if let Some(Value::Object(p)) = m.get("properties") {
                    for (key, value) in p {
                        let key = key.trim().to_lowercase().replace('-', "_");
                        if [
                            "git",
                            "id",
                            "job_cluster_key",
                            "key",
                            "local",
                            "name",
                            "operationid",
                            "package",
                            "task",
                            "task_key",
                        ]
                        .contains(&key.as_str())
                        {
                            result.insert(key);
                        }
                        stack.push(value);
                    }
                }
                for key in ["items", "anyOf", "oneOf", "allOf"] {
                    if let Some(v) = m.get(key) {
                        stack.push(v)
                    }
                }
                for key in ["$defs", "definitions", "patternProperties"] {
                    if let Some(Value::Object(defs)) = m.get(key) {
                        stack.extend(defs.values())
                    }
                }
            }
            Value::Array(a) => stack.extend(a),
            _ => {}
        }
    }
    result
}
fn strings(v: Option<&Value>, where_: &str, errors: &mut Vec<String>) -> Vec<String> {
    match v {
        None | Some(Value::Null) => vec![],
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(
                |v| match v.as_str().map(str::trim).filter(|s| !s.is_empty()) {
                    Some(s) => Some(s.into()),
                    None => {
                        errors.push(format!("{where_} entries must be non-empty strings"));
                        None
                    }
                },
            )
            .collect(),
        _ => {
            errors.push(format!("{where_} must be a list of strings"));
            vec![]
        }
    }
}
fn optional_string(v: Option<&Value>, where_: &str, errors: &mut Vec<String>) -> Option<String> {
    match v {
        None | Some(Value::Null) => None,
        Some(v) => match v.as_str().map(str::trim).filter(|s| !s.is_empty()) {
            Some(s) => Some(s.into()),
            None => {
                errors.push(format!("{where_} must be a non-empty string"));
                None
            }
        },
    }
}
/// Validate a descriptor. The caller supplies the contents and resolved path of any local schema.
/// Errors reject the complete descriptor, never a partially valid profile.
pub fn validate_descriptor(
    document: &Value,
    path: &str,
    index: usize,
    raw_text: &str,
    schema: Option<&Value>,
    schema_path: Option<&str>,
    schema_raw: Option<&str>,
) -> Result<UserSchemaProfile, Vec<String>> {
    let w = format!("{path}#{index}");
    let mut errors = vec![];
    let Some(doc) = document.as_object() else {
        return Err(vec![format!("{w}: descriptor must be a mapping")]);
    };
    let language_id = doc
        .get("language_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            errors.push(format!(
                "{w}: 'language_id' is required and must be a string"
            ));
            ""
        })
        .to_string();
    let root = language_id.split(':').next().unwrap_or("").to_lowercase();
    if [
        "adf",
        "azure-pipelines",
        "databricks",
        "dbt",
        "embedded",
        "github-actions",
        "kubernetes",
        "none",
        "openapi",
        "user",
    ]
    .contains(&root.as_str())
    {
        errors.push(format!(
            "{w}: language_id '{language_id}' collides with the built-in provider root '{root}'"
        ))
    }
    let empty = serde_json::Map::new();
    let match_ = match doc.get("match") {
        None | Some(Value::Null) => &empty,
        Some(Value::Object(m)) => m,
        _ => {
            errors.push(format!("{w}: 'match' must be a mapping"));
            &empty
        }
    };
    let filename_patterns = strings(
        match_.get("filename_patterns"),
        &format!("{w}: match.filename_patterns"),
        &mut errors,
    );
    let root_markers = strings(
        match_.get("root_markers"),
        &format!("{w}: match.root_markers"),
        &mut errors,
    );
    let schema_urls = strings(
        match_.get("schema_urls"),
        &format!("{w}: match.schema_urls"),
        &mut errors,
    );
    let namespace = optional_string(
        match_.get("namespace"),
        &format!("{w}: match.namespace"),
        &mut errors,
    );
    let root_element = optional_string(
        match_.get("root_element"),
        &format!("{w}: match.root_element"),
        &mut errors,
    );
    if filename_patterns.is_empty()
        && root_markers.is_empty()
        && schema_urls.is_empty()
        && namespace.is_none()
        && root_element.is_none()
    {
        errors.push(format!(
            "{w}: 'match' needs filename_patterns, root_markers, or schema_urls"
        ))
    }
    let mut identity_fields: BTreeSet<_> = strings(
        doc.get("identity_fields"),
        &format!("{w}: identity_fields"),
        &mut errors,
    )
    .into_iter()
    .collect();
    let mut keyed_elements = BTreeMap::new();
    for kind in ["keyed_arrays", "keyed_elements"] {
        match doc.get(kind) {
            None | Some(Value::Null) => {}
            Some(Value::Object(m)) => {
                for (key, value) in m {
                    let vals = match value {
                        Value::Array(a) => a.clone(),
                        _ => vec![value.clone()],
                    };
                    let mut fields = vec![];
                    for value in vals {
                        if let Some(s) = value.as_str().map(str::trim).filter(|s| !s.is_empty()) {
                            fields.push(s.to_string())
                        } else {
                            errors.push(format!("{w}: {kind}['{key}'] entries must be strings"))
                        }
                    }
                    if kind == "keyed_arrays" {
                        identity_fields.extend(fields)
                    } else if key.trim().is_empty() {
                        errors.push(format!("{w}: keyed_elements tags must be strings"))
                    } else if !fields.is_empty() {
                        keyed_elements.insert(key.trim().into(), fields);
                    }
                }
            }
            _ => errors.push(format!(
                "{w}: '{kind}' must be a mapping of {} -> {}",
                if kind == "keyed_arrays" {
                    "path"
                } else {
                    "element tag"
                },
                if kind == "keyed_arrays" {
                    "identity fields"
                } else {
                    "key fields"
                }
            )),
        }
    }
    if !keyed_elements.is_empty() && namespace.is_none() && root_element.is_none() {
        errors.push(format!(
            "{w}: an XML dialect (keyed_elements) needs match.namespace or match.root_element"
        ))
    }
    let important_paths = strings(
        doc.get("important_paths"),
        &format!("{w}: important_paths"),
        &mut errors,
    );
    let scaffold_paths = strings(
        doc.get("scaffold_paths"),
        &format!("{w}: scaffold_paths"),
        &mut errors,
    );
    if let Some(v) = doc.get("schema").filter(|v| !v.is_null()) {
        if v.as_str().is_none_or(|s| s.trim().is_empty()) {
            errors.push(format!("{w}: 'schema' must be a local file path"))
        } else if schema.is_none() {
            errors.push(format!("{w}: schema file contents not supplied"))
        }
    }
    if let Some(schema) = schema {
        if !schema.is_object() {
            errors.push(format!("{w}: schema file must contain a JSON object"))
        } else {
            identity_fields.extend(derive_identity_fields(schema));
        }
    }
    if identity_fields.is_empty() && keyed_elements.is_empty() {
        errors.push(format!("{w}: no identity source — provide identity_fields, keyed_arrays, or a local schema that yields identity hints"))
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let mut hash = Sha256::new();
    hash.update(raw_text.as_bytes());
    if let Some(raw) = schema_raw {
        hash.update(raw.as_bytes())
    }
    let digest = format!("{:x}", hash.finalize());
    Ok(UserSchemaProfile {
        fingerprint: format!("user:{language_id}:{}", &digest[..16]),
        language_id,
        source_path: path.into(),
        filename_patterns,
        root_markers,
        schema_urls,
        identity_fields,
        important_paths,
        scaffold_paths,
        schema: schema.cloned(),
        schema_path: schema_path.map(str::to_owned),
        keyed_elements,
        namespace,
        root_element,
    })
}
// Python fnmatch-compatible wildcards, including character classes; slash is ordinary text.
fn glob_matches(pattern: &str, text: &str) -> bool {
    compile_filename_glob(pattern).is_ok_and(|r| r.is_match(text))
}

/// Case-sensitive fnmatch syntax, with path separators treated as ordinary characters.
pub(crate) fn compile_filename_glob(pattern: &str) -> Result<Regex, regex::Error> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut regex = String::from("\\A");
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' => regex.push_str(".*"),
            '?' => regex.push('.'),
            '[' => {
                let start = i;
                i += 1;
                let neg = i < chars.len() && chars[i] == '!';
                if neg {
                    i += 1
                }
                let begin = i;
                if i < chars.len() && chars[i] == ']' {
                    i += 1
                }
                while i < chars.len() && chars[i] != ']' {
                    i += 1
                }
                if i == chars.len() {
                    i = start;
                    regex.push_str("\\[")
                } else {
                    // fnmatch classes are unions, never regex set operations.
                    // Invalid descending ranges are removed, as in Python fnmatch.
                    let mut class = String::new();
                    let mut cursor = begin;
                    while cursor < i {
                        if cursor + 2 < i && chars[cursor + 1] == '-' {
                            if chars[cursor] <= chars[cursor + 2] {
                                class.push_str(&regex::escape(&chars[cursor].to_string()));
                                class.push('-');
                                class.push_str(&regex::escape(&chars[cursor + 2].to_string()));
                            }
                            cursor += 3;
                        } else {
                            class.push_str(&regex::escape(&chars[cursor].to_string()));
                            cursor += 1;
                        }
                    }
                    if class.is_empty() {
                        regex.push_str(if neg { "." } else { r"\b\B" });
                    } else {
                        regex.push('[');
                        if neg { regex.push('^'); }
                        regex.push_str(&class);
                        regex.push(']');
                    }
                }
            }
            c => regex.push_str(&regex::escape(&c.to_string())),
        }
        i += 1;
    }
    regex.push_str("\\z");
    Regex::new(&format!("(?s:{regex})"))
}
/// URL claims take precedence over filename/root-marker matches across all profiles.
pub fn match_user_profile(
    profiles: &[UserSchemaProfile],
    filename: &str,
    content: &str,
    declared_url: Option<&str>,
) -> Option<usize> {
    if let Some(url) = declared_url.filter(|s| !s.is_empty()) {
        if let Some(index) = profiles
            .iter()
            .position(|p| p.schema_urls.iter().any(|s| s == url))
        {
            return Some(index);
        }
    }
    let path = filename.replace('\\', "/").to_lowercase();
    let base = path.rsplit('/').next().unwrap_or("");
    profiles.iter().position(|p| {
        let root_match = root_markers_match(content, &p.root_markers);
        if !p.filename_patterns.is_empty() {
            p.filename_patterns
                .iter()
                .any(|s| glob_matches(s, base) || glob_matches(s, &path))
                && root_match
        } else {
            !p.root_markers.is_empty() && root_match
        }
    })
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileResolution {
    pub profile_index: Option<usize>,
    pub candidate: Option<SchemaCandidate>,
    pub identity_fields: BTreeSet<String>,
}
/// Resolve host-loaded profiles, embedded declarations, and built-ins in that order.
pub fn resolve_profile(
    profiles: &[UserSchemaProfile],
    filename: &str,
    language: Option<&str>,
    content: &str,
) -> ProfileResolution {
    let declared = discover_declared_schema(content, language);
    if let Some(index) = match_user_profile(profiles, filename, content, declared.as_deref()) {
        return ProfileResolution {
            profile_index: Some(index),
            candidate: None,
            identity_fields: profiles[index].identity_fields.clone(),
        };
    }
    let c = declared
        .map(|url| candidate("embedded", &url, &[]))
        .or_else(|| provider_schema_candidate(filename, language, content));
    ProfileResolution {
        profile_index: None,
        identity_fields: c
            .as_ref()
            .map(|c| c.identity_fields.clone())
            .unwrap_or_default(),
        candidate: c,
    }
}
/// Parse descriptor YAML with one shared YAML 1.2 interpretation for every host.
/// Duplicate mapping keys and non-string keys are rejected rather than coerced.
pub fn parse_descriptor_documents(raw: &str) -> Result<Vec<Value>, String> {
    fn check_keys(value: &serde_yaml::Value) -> Result<(), String> {
        match value {
            serde_yaml::Value::Mapping(map) => {
                for (key, value) in map {
                    if !key.is_string() {
                        return Err("descriptor mapping keys must be strings".into());
                    }
                    check_keys(value)?;
                }
            }
            serde_yaml::Value::Sequence(items) => {
                for value in items {
                    check_keys(value)?;
                }
            }
            serde_yaml::Value::Tagged(value) => check_keys(&value.value)?,
            _ => {}
        }
        Ok(())
    }
    let mut documents = Vec::new();
    for doc in serde_yaml::Deserializer::from_str(raw) {
        let value = serde_yaml::Value::deserialize(doc).map_err(|e| e.to_string())?;
        if value.is_null() {
            continue;
        }
        check_keys(&value)?;
        documents.push(serde_json::to_value(value).map_err(|e| e.to_string())?);
    }
    Ok(documents)
}
/// JSON transport adapter for every language binding. All semantics live in typed operations above.
pub fn schema_profiles_impl(request_json: &str) -> Result<String, String> {
    let v: Value = serde_json::from_str(request_json).map_err(|e| e.to_string())?;
    let s = |key: &str| v.get(key).and_then(Value::as_str).unwrap_or("");
    let profiles = || {
        serde_json::from_value::<Vec<UserSchemaProfile>>(
            v.get("profiles").cloned().unwrap_or_else(|| json!([])),
        )
        .map_err(|e| e.to_string())
    };
    let result = match s("operation") {
        "parse_documents" => match parse_descriptor_documents(s("raw_text")) {
            Ok(documents) => json!({"documents":documents,"error":null}),
            Err(error) => json!({"documents":[],"error":error}),
        },
        "discover" => json!(discover_declared_schema(
            s("content"),
            v.get("language").and_then(Value::as_str)
        )),
        "provider" => json!(provider_schema_candidate(
            s("filename"),
            v.get("language").and_then(Value::as_str),
            s("content")
        )),
        "derive" => json!(derive_identity_fields(
            v.get("schema").unwrap_or(&Value::Null)
        )),
        "match" => json!(match_user_profile(
            &profiles()?,
            s("filename"),
            s("content"),
            v.get("declared_url").and_then(Value::as_str)
        )),
        "resolve" => json!(resolve_profile(
            &profiles()?,
            s("filename"),
            v.get("language").and_then(Value::as_str),
            s("content")
        )),
        "validate" => match validate_descriptor(
            v.get("document").unwrap_or(&Value::Null),
            s("path"),
            v.get("index").and_then(Value::as_u64).unwrap_or(0) as usize,
            s("raw_text"),
            v.get("schema").filter(|v| !v.is_null()),
            v.get("schema_path").and_then(Value::as_str),
            v.get("schema_raw").and_then(Value::as_str),
        ) {
            Ok(p) => json!({"profile":p,"errors":[]}),
            Err(errors) => json!({"profile":null,"errors":errors}),
        },
        "select_unique" => {
            let mut ids = BTreeSet::new();
            json!(profiles()?
                .iter()
                .enumerate()
                .filter_map(|(i, p)| if ids.insert(p.language_id.clone()) {
                    Some(i)
                } else {
                    None
                })
                .collect::<Vec<_>>())
        }
        _ => return Err("unknown schema profile operation".into()),
    };
    serde_json::to_string(&result).map_err(|e| e.to_string())
}

/// Native host filesystem adapter. Directory order is precedence order; malformed
/// descriptor files are rejected as a unit, with diagnostics returned to the host.
/// Remote schemas are never fetched.
#[derive(Debug, Default)]
pub struct LoadedProfiles {
    pub profiles: Vec<UserSchemaProfile>,
    pub errors: Vec<String>,
}
pub fn load_local_profiles(directories: &[std::path::PathBuf]) -> LoadedProfiles {
    let mut result = LoadedProfiles::default();
    let mut ids = BTreeSet::new();
    for directory in directories {
        let entries = match std::fs::read_dir(directory) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                result.errors.push(format!("{}: {e}", directory.display()));
                continue;
            }
        };
        let mut paths = Vec::new();
        for entry in entries {
            match entry {
                Ok(e) => {
                    let p = e.path();
                    if p.is_file()
                        && p.extension().and_then(|s| s.to_str()).is_some_and(|s| {
                            ["yml", "yaml", "json"].contains(&s.to_lowercase().as_str())
                        })
                    {
                        paths.push(p)
                    }
                }
                Err(e) => result.errors.push(e.to_string()),
            }
        }
        paths.sort();
        for path in paths {
            let path_string = path.to_string_lossy();
            let raw = match std::fs::read_to_string(&path) {
                Ok(s) => s,
                Err(e) => {
                    result
                        .errors
                        .push(format!("{path_string}: unreadable descriptor ({e})"));
                    continue;
                }
            };
            let mut pending = vec![];
            let mut errors = vec![];
            let documents = match parse_descriptor_documents(&raw) {
                Ok(documents) => documents,
                Err(error) => {
                    result
                        .errors
                        .push(format!("{path_string}: unreadable descriptor ({error})"));
                    continue;
                }
            };
            for (index, document) in documents.into_iter().enumerate() {
                let schema_ref = document
                    .get("schema")
                    .and_then(Value::as_str)
                    .filter(|s| !s.trim().is_empty());
                let mut schema_raw = None;
                let mut schema = None;
                let mut schema_path = None;
                if let Some(reference) = schema_ref {
                    let joined = path
                        .parent()
                        .unwrap_or_else(|| std::path::Path::new("."))
                        .join(reference);
                    match std::fs::canonicalize(&joined)
                        .and_then(|p| std::fs::read_to_string(&p).map(|s| (p, s)))
                    {
                        Ok((p, s)) => match serde_json::from_str::<Value>(&s) {
                            Ok(v) => {
                                schema_raw = Some(s);
                                schema = Some(v);
                                schema_path = Some(p.to_string_lossy().into_owned())
                            }
                            Err(e) => {
                                errors.push(format!(
                                    "{path_string}#{index}: schema file unreadable ({e})"
                                ));
                                continue;
                            }
                        },
                        Err(e) => {
                            errors.push(format!(
                                "{path_string}#{index}: schema file unreadable ({e})"
                            ));
                            continue;
                        }
                    }
                }
                match validate_descriptor(
                    &document,
                    &path_string,
                    index,
                    &raw,
                    schema.as_ref(),
                    schema_path.as_deref(),
                    schema_raw.as_deref(),
                ) {
                    Ok(p) => pending.push(p),
                    Err(e) => errors.extend(e),
                }
            }
            if pending.is_empty() && errors.is_empty() {
                errors.push(format!(
                    "{path_string}: descriptor file contains no profiles"
                ))
            }
            if errors.is_empty() {
                for p in pending {
                    if ids.insert(p.language_id.clone()) {
                        result.profiles.push(p)
                    }
                }
            } else {
                result.errors.extend(errors)
            }
        }
    }
    result
}

#[cfg(test)]
mod glob_regressions {
    use super::*;
    #[test]
    fn fnmatch_classes_are_not_regex_set_operations() {
        for (pattern, name) in [("[a&&b].json", "a.json"), ("[a~~b].json", "~.json"), ("[a--c].json", "c.json"), ("[!a--c].json", "a.json"), ("[a-z].json", "m.json")] {
            assert!(glob_matches(pattern, name), "{pattern} {name}");
        }
        assert!(!glob_matches("[z-a].json", "z.json"));
        assert!(!glob_matches("[!a-z].json", "m.json"));
    }
}
