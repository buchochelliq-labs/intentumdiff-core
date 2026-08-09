//! Project configuration loading (`intentumdiff.yaml`) — the config subsystem port
//! (#99, A2.1). Python's `core/config.py` (`find_intentumdiff_config` +
//! `load_project_diff_config`) moves here so every binding resolves config identically;
//! the Python shell keeps only the `DiffConfig` DTO construction from the returned
//! mapping. Behaviour mirrors the retired Python exactly (file walk, YAML parse, the
//! `config` section extraction, and the unknown-key rejection message).

use std::collections::HashSet;
use std::env;
use std::path::PathBuf;

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

/// python `config.find_intentumdiff_config`: the nearest `intentumdiff.yaml` from
/// *start_path* (and cwd) upward, or an explicit path when it exists.
pub(crate) fn find_config_path(start_path: Option<&str>, explicit_path: Option<&str>) -> Option<PathBuf> {
    if let Some(ep) = explicit_path {
        let p = PathBuf::from(ep);
        return p.exists().then_some(p);
    }

    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut starts: Vec<PathBuf> = Vec::new();
    if let Some(sp) = start_path {
        if !sp.is_empty() && !sp.starts_with('<') {
            let raw = PathBuf::from(sp);
            let start = if raw.is_dir() {
                raw
            } else {
                raw.parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| cwd.clone())
            };
            starts.push(start);
        }
    }
    starts.push(cwd.clone());

    let mut seen: HashSet<PathBuf> = HashSet::new();
    for start in starts {
        // python uses Path.resolve() (falling back to cwd on OSError); canonicalize is
        // the closest equivalent, with the same cwd fallback when the path is missing.
        let current = start.canonicalize().unwrap_or_else(|_| cwd.clone());
        for directory in current.ancestors() {
            let directory = directory.to_path_buf();
            if !seen.insert(directory.clone()) {
                continue;
            }
            let candidate = directory.join(INTENTUMDIFF_CONFIG_FILENAME);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    None
}

/// python `config.load_project_diff_config`: find + parse + validate the `config`
/// section, returning it as a JSON object string (`"{}"` when there is no file /
/// section). `Err` carries the message the Python shell raises as `ValueError`.
pub(crate) fn load_config_section(
    start_path: Option<&str>,
    explicit_path: Option<&str>,
) -> Result<String, String> {
    let path = match find_config_path(start_path, explicit_path) {
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
    fn non_mapping_root_is_rejected() {
        let f = TempYaml::new("- a\n- b\n");
        let err = load_config_section(None, Some(f.as_str())).unwrap_err();
        assert!(err.contains("must contain a YAML mapping"), "got: {err}");
    }
}
