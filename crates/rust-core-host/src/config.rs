//! Project configuration loading (`intentumdiff.yaml`) — the config subsystem port
//! (#99, A2.1). Python's `core/config.py` (`find_intentumdiff_config` +
//! `load_project_diff_config`) moves here so every binding resolves config identically;
//! the Python shell keeps only the `DiffConfig` DTO construction from the returned
//! mapping. Behaviour mirrors the retired Python exactly (file walk, YAML parse, the
//! `config` section extraction, and the unknown-key rejection message).

use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};

const INTENTUMDIFF_CONFIG_FILENAME: &str = "intentumdiff.yaml";

/// The keys accepted under the `config:` mapping — mirrors python `_DIFF_CONFIG_KEYS`.
const DIFF_CONFIG_KEYS: &[&str] = &[
    "approx_move_threshold",
    "detect_refactorings",
    "ignore_style",
    "max_cst_bytes",
    "min_height",
    "min_similarity",
    "plugin_fuel",
    "strict_plugins",
];

/// Find the nearest project config from one physical origin only. A caller that
/// supplies a source/repository path must never inherit an unrelated process cwd.
pub(crate) fn find_config_path(
    start_path: Option<&str>,
    explicit_path: Option<&str>,
) -> Result<Option<PathBuf>, String> {
    // Preserve the existing public contract: a missing explicit file is absent.
    if let Some(ep) = explicit_path {
        let path = PathBuf::from(ep);
        return Ok(path.exists().then_some(path));
    }
    let cwd = env::current_dir()
        .map_err(|error| format!("cannot determine cwd for project configuration: {error}"))?;
    find_config_path_from(start_path, &cwd)
}

// Keeping cwd injected makes isolation tests deterministic without global chdir.
fn find_config_path_from(start_path: Option<&str>, cwd: &Path) -> Result<Option<PathBuf>, String> {
    let start = if let Some(sp) = start_path.filter(|path| !path.is_empty() && !path.starts_with('<')) {
        let raw = PathBuf::from(sp);
        let absolute = if raw.is_absolute() { raw } else { cwd.join(raw) };
        if absolute.is_dir() {
            absolute
        } else {
            absolute.parent().map(PathBuf::from).ok_or_else(|| {
                format!("cannot locate parent for project configuration source {}", absolute.display())
            })?
        }
    } else {
        // No physical source/synthetic source: cwd is intentionally the origin.
        cwd.to_path_buf()
    };

    // Canonicalization failure is an I/O error, never permission to take
    // configuration from whichever unrelated directory happens to be cwd.
    let current = start.canonicalize().map_err(|error| {
        format!("cannot resolve project configuration search root {}: {error}", start.display())
    })?;
    for directory in current.ancestors() {
        let candidate = directory.join(INTENTUMDIFF_CONFIG_FILENAME);
        if candidate.exists() {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

/// python `config.load_project_diff_config`: find + parse + validate the `config`
/// section, returning it as a JSON object string (`"{}"` when there is no file /
/// section). `Err` carries the message the Python shell raises as `ValueError`.
pub(crate) fn load_config_section(
    start_path: Option<&str>,
    explicit_path: Option<&str>,
) -> Result<String, String> {
    let path = match find_config_path(start_path, explicit_path)? {
        Some(p) => p,
        None => return Ok("{}".to_string()),
    };
    let display = path.display();
    let text = std::fs::read_to_string(&path).map_err(|exc| format!("{display}: {exc}"))?;
    if text.trim().is_empty() {
        return Ok("{}".to_string());
    }
    let raw: serde_yaml::Value =
        serde_yaml::from_str(&text).map_err(|exc| format!("{display}: {exc}"))?;
    if raw.is_null() {
        return Ok("{}".to_string());
    }
    let mapping = raw
        .as_mapping()
        .ok_or_else(|| format!("{display} must contain a YAML mapping"))?;

    let config = match mapping.get("config") {
        None => return Ok("{}".to_string()),
        Some(value) if value.is_null() => return Ok("{}".to_string()),
        Some(value) => value
            .as_mapping()
            .ok_or_else(|| format!("{display} config section must be a mapping"))?,
    };

    let allowed: HashSet<&str> = DIFF_CONFIG_KEYS.iter().copied().collect();
    let mut unknown: Vec<String> = config
        .iter()
        .filter_map(|(key, _)| {
            let key = key.as_str().unwrap_or("<non-string key>").to_string();
            (!allowed.contains(key.as_str())).then_some(key)
        })
        .collect();
    if !unknown.is_empty() {
        unknown.sort();
        let mut allowed_sorted: Vec<&str> = DIFF_CONFIG_KEYS.to_vec();
        allowed_sorted.sort_unstable();
        return Err(format!(
            "{display} config contains unsupported key(s): {}. Supported keys: {}",
            unknown.join(", "),
            allowed_sorted.join(", ")
        ));
    }

    let mut json_value =
        serde_json::to_value(config).map_err(|exc| format!("{display}: serialise config: {exc}"))?;
    coerce_underscore_numbers(&mut json_value);
    serde_json::to_string(&json_value).map_err(|exc| format!("{display}: {exc}"))
}

/// PyYAML (YAML 1.1) resolves underscore-separated numeric literals like `10_000_000`
/// to numbers; serde_yaml (YAML 1.2 core schema) leaves them as strings. Coerce those
/// string values back to numbers so config parity with the retired PyYAML path holds.
/// Only strings that CONTAIN an underscore and are wholly number-shaped are touched —
/// serde_yaml already handles ordinary numerics, and genuine strings never match.
fn coerce_underscore_numbers(value: &mut serde_json::Value) {
    let serde_json::Value::Object(map) = value else {
        return;
    };
    for entry in map.values_mut() {
        if let serde_json::Value::String(text) = entry {
            if let Some(number) = parse_underscore_number(text) {
                *entry = number;
            }
        }
    }
}

fn parse_underscore_number(text: &str) -> Option<serde_json::Value> {
    if !text.contains('_')
        || !text
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '_' | '+' | '-' | '.' | 'e' | 'E'))
    {
        return None;
    }
    let cleaned = text.replace('_', "");
    if let Ok(int) = cleaned.parse::<i64>() {
        return Some(serde_json::json!(int));
    }
    if let Ok(uint) = cleaned.parse::<u64>() {
        return Some(serde_json::json!(uint));
    }
    match cleaned.parse::<f64>() {
        Ok(float) if float.is_finite() => Some(serde_json::json!(float)),
        _ => None,
    }
}

/// Shell-facing config loader (#99): returns the validated `config` mapping as JSON
/// (`"{}"` when absent); raises `ValueError` on a malformed file / unsupported keys.
/// Shell-facing config finder (#99): the resolved `intentumdiff.yaml` path, or `None`.
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    struct TempYaml {
        path: PathBuf,
    }

    impl TempYaml {
        fn new(body: &str) -> Self {
            let n = COUNTER.fetch_add(1, Ordering::SeqCst);
            let path = env::temp_dir().join(format!(
                "intentumdiff_cfg_{}_{}.yaml",
                std::process::id(),
                n
            ));
            std::fs::write(&path, body).unwrap();
            Self { path }
        }
        fn as_str(&self) -> &str {
            self.path.to_str().unwrap()
        }
    }

    impl Drop for TempYaml {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    #[test]
    fn loads_and_typed_config_section() {
        let f = TempYaml::new(
            "config:\n  min_similarity: 0.7\n  detect_refactorings: false\n  plugin_fuel: 10_000_000\n  max_cst_bytes: 4194304\n",
        );
        let json = load_config_section(None, Some(f.as_str())).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["min_similarity"], serde_json::json!(0.7));
        assert_eq!(value["detect_refactorings"], serde_json::json!(false));
        assert_eq!(value["plugin_fuel"], serde_json::json!(10_000_000));
        assert_eq!(value["max_cst_bytes"], serde_json::json!(4_194_304));
    }

    #[test]
    fn rejects_unknown_keys() {
        let f = TempYaml::new("config:\n  min_similarity: 0.7\n  surprise_knob: true\n");
        let err = load_config_section(None, Some(f.as_str())).unwrap_err();
        assert!(err.contains("unsupported key"), "got: {err}");
        assert!(err.contains("surprise_knob"), "got: {err}");
    }

    #[test]
    fn missing_file_and_absent_section_return_empty() {
        assert_eq!(
            load_config_section(None, Some("/no/such/intentumdiff.yaml")).unwrap(),
            "{}"
        );
        let f = TempYaml::new("guardrails:\n  protected: []\n");
        assert_eq!(load_config_section(None, Some(f.as_str())).unwrap(), "{}");
        let empty = TempYaml::new("");
        assert_eq!(load_config_section(None, Some(empty.as_str())).unwrap(), "{}");
    }

    #[test]
    fn physical_source_never_inherits_unrelated_cwd_config() {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let root = env::temp_dir().join(format!("idf_cfg_scope_{}_{}", std::process::id(), n));
        let selected = root.join("selected");
        let foreign = root.join("foreign");
        std::fs::create_dir_all(&selected).unwrap();
        std::fs::create_dir_all(&foreign).unwrap();
        let source = selected.join("sample.rs");
        std::fs::write(&source, "fn sample() {}\n").unwrap();
        let foreign_policy = foreign.join(INTENTUMDIFF_CONFIG_FILENAME);
        std::fs::write(&foreign_policy, "config:\n  min_similarity: 0.9\n").unwrap();

        let found = find_config_path_from(Some(source.to_str().unwrap()), &foreign).unwrap();
        assert_ne!(found, Some(foreign_policy), "selected source inherited foreign cwd config");

        let selected_policy = selected.join(INTENTUMDIFF_CONFIG_FILENAME);
        std::fs::write(&selected_policy, "config:\n  min_similarity: 0.2\n").unwrap();
        let found = find_config_path_from(Some(source.to_str().unwrap()), &foreign).unwrap();
        assert_eq!(found, Some(selected_policy));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unresolvable_physical_root_is_error_not_cwd_fallback() {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let root = env::temp_dir().join(format!("idf_cfg_missing_{}_{}", std::process::id(), n));
        let foreign = root.join("foreign");
        std::fs::create_dir_all(&foreign).unwrap();
        std::fs::write(foreign.join(INTENTUMDIFF_CONFIG_FILENAME), "config: {}\n").unwrap();

        // The parent is missing: canonicalize must fail instead of searching foreign.
        let source = root.join("missing-parent").join("inner").join("file.rs");
        let error = find_config_path_from(Some(source.to_str().unwrap()), &foreign).unwrap_err();
        assert!(error.contains("cannot resolve project configuration search root"), "{error}");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn non_mapping_root_is_rejected() {
        let f = TempYaml::new("- a\n- b\n");
        let err = load_config_section(None, Some(f.as_str())).unwrap_err();
        assert!(err.contains("must contain a YAML mapping"), "got: {err}");
    }
}
