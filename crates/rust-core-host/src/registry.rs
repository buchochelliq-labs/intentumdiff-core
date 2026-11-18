//! Registry-client security validators (#97, A2.3) — the shared, security-critical
//! subset of python `plugins/hub.py`. The network fetch (HTTPS GET) and `pip install`
//! stay per-binding I/O (a Go binding does its own); only the validation that must be
//! identical across bindings — the #88 controls — moves here: the registry-ref
//! path-traversal guard and the dep-hash format + coverage check (hash-pinned installs).

use std::sync::LazyLock;

use regex::Regex;

// Mirrors the python module-level regexes in hub.py.
static REGISTRY_REF_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-zA-Z0-9][a-zA-Z0-9._-]{0,199}$").unwrap());
static COMMIT_SHA_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9a-f]{40}$").unwrap());
static DEP_HASH_KEY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[A-Za-z0-9]([A-Za-z0-9._-]*[A-Za-z0-9])?==[0-9][A-Za-z0-9._+!-]*$").unwrap()
});
static DEP_HASH_VALUE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^sha256:[A-Fa-f0-9]{64}$").unwrap());
static PKG_SEP_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[-_.]+").unwrap());

/// python `_normalise_package_name`: collapse `[-_.]+` runs to `_`, lowercase.
fn normalise_package_name(name: &str) -> String {
    PKG_SEP_RE.replace_all(name, "_").to_lowercase()
}

/// python `_package_from_dep_spec`: normalised name from a `package==version` key.
fn package_from_dep_spec(dep_spec: &str) -> String {
    normalise_package_name(dep_spec.split("==").next().unwrap_or(dep_spec))
}

/// python `hub._validate_registry_ref`: reject refs unsafe for URL-path interpolation.
/// In `strict` mode the ref must be a full 40-char lowercase commit SHA (reproducible,
/// tamper-evident installs). `Err` carries the traversal-guard message the binding
/// raises as `ValueError`. The C ABI shares this exact #88 security check.
pub(crate) fn validate_registry_ref_impl(git_ref: &str, strict: bool) -> Result<(), String> {
    if !REGISTRY_REF_RE.is_match(git_ref) || git_ref.contains("..") {
        return Err(format!(
            "Unsafe registry ref '{git_ref}'. Must be a branch name, tag, or full commit \
             SHA with no path traversal or slash-containing refs."
        ));
    }
    if strict && !COMMIT_SHA_RE.is_match(git_ref) {
        return Err(format!(
            "Strict registry mode requires a full 40-character commit SHA; got '{git_ref}'. \
             Pin the registry ref to a commit SHA for reproducible, tamper-evident installs."
        ));
    }
    Ok(())
}

/// python `hub._validate_dep_hashes`: validate a plugin's `dep_hashes` keys/values and
/// coverage. Returns human-readable errors ([] = well-formed). `dep_hashes` is passed as
/// ordered `(key, value)` pairs so error ordering matches the Python dict iteration. The body
/// is already pyo3-free, so the C ABI (`intentumdiff_call`) calls it directly.
pub(crate) fn validate_dep_hashes_impl(
    dep_hashes: Vec<(String, String)>,
    allowed_dependencies: Vec<String>,
    package_name: &str,
    install_target: Option<&str>,
) -> Vec<String> {
    if dep_hashes.is_empty() {
        return Vec::new(); // empty = self-contained wheel
    }
    let mut errors: Vec<String> = Vec::new();
    for (key, value) in &dep_hashes {
        if !DEP_HASH_KEY_RE.is_match(key) {
            errors.push(format!(
                "Invalid dep_hashes key '{key}': must be 'package==version' (exact version \
                 pin with no wildcards or ranges)."
            ));
        }
        if !DEP_HASH_VALUE_RE.is_match(value) {
            errors.push(format!(
                "Invalid dep_hashes value for '{key}': must be 'sha256:' followed by a \
                 64-character hex digest."
            ));
        }
    }

    // accepted = the main package name(s); allowed = the explicit allow-list.
    let mut accepted: Vec<String> = vec![normalise_package_name(package_name)];
    if let Some(target) = install_target {
        accepted.push(package_from_dep_spec(target));
    }
    let allowed: Vec<String> = allowed_dependencies
        .iter()
        .map(|d| normalise_package_name(d))
        .collect();

    let main_found = dep_hashes
        .iter()
        .any(|(k, _)| accepted.contains(&package_from_dep_spec(k)));
    if !main_found {
        errors.push(format!(
            "dep_hashes must include the main plugin package '{package_name}' so its wheel \
             hash is verified."
        ));
    }
    for (key, _) in &dep_hashes {
        let package = package_from_dep_spec(key);
        if !accepted.contains(&package) && !allowed.contains(&package) {
            errors.push(format!(
                "Unexpected dep_hashes package '{key}': packages other than the main plugin \
                 must be listed in allowed_dependencies."
            ));
        }
    }
    errors
}

// ── Registry manifest validator (python #329) ─────────────────────────────────────────────
//
// The structural + policy validator for an `intentumdiff-registry` manifest (`registry.yaml`
// parsed to JSON). Every problem is returned as `{path, message}` (RFC 6901 JSON Pointer into
// the manifest) — wrong JSON types are errors, never panics. Policy:
//
// - official tier (the default when `trust_tier` is omitted): a `git` entry needs an immutable
//   40-hex lowercase `ref`; a `pypi` entry needs an exact version `ref`; non-empty
//   `wasm_checksums`; a non-empty `capabilities` list; an `abi_target`; and no custom `repo`
//   (a custom repo is by definition not the org-built source).
// - community tier: every field is optional; any field that IS present must be well-formed
//   (`ref` must pass the non-strict registry-ref traversal guard / version pattern,
//   checksums must be lowercase SHA-256, capabilities from the vocabulary, abi_target
//   MAJOR.MINOR.PATCH). Community entries are listed but install unverified.
// - `capabilities` come from a closed vocabulary derived from the WIT: the interface each
//   `world` exports (`parser`, `renderer`, `enricher`, `index-engine`, `diff-analyzer`);
//   duplicates are rejected.

static PLUGIN_NAME_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:intentumdiff-)?[A-Za-z0-9][A-Za-z0-9._-]{0,127}$").unwrap());
static WASM_FILE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9._-]+\.wasm$").unwrap());
static SHA256_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-f0-9]{64}$").unwrap());
static MANIFEST_DEP_KEY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9][A-Za-z0-9._-]*==[^=\s]+$").unwrap());
static MANIFEST_DEP_HASH_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^sha256:[a-f0-9]{64}$").unwrap());
static ABI_VERSION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$").unwrap());
static PYPI_VERSION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9][A-Za-z0-9.+!-]{0,63}$").unwrap());

const ENTRY_FIELDS: &[&str] = &[
    "source", "ref", "repo", "description", "wasm_checksums", "dep_hashes",
    "allowed_dependencies", "trust_tier", "capabilities", "abi_target", "provenance_manifest_ref",
];
const TOP_LEVEL_FIELDS: &[&str] = &["version", "plugins", "renderer_components"];
const RENDERER_FILES: &[&str] =
    &["html_renderer.wasm", "llm_renderer.wasm", "patch_renderer.wasm", "terminal_renderer.wasm"];

/// The closed capability vocabulary: every interface a `world` in the pinned WIT exports.
pub(crate) fn plugin_capability_vocabulary() -> &'static [String] {
    static VOCAB: LazyLock<Vec<String>> = LazyLock::new(|| {
        let mut names: Vec<String> = include_str!("../wit/plugin.wit")
            .lines()
            .filter_map(|line| line.trim().strip_prefix("export ")?.strip_suffix(';').map(str::to_owned))
            .collect();
        names.sort();
        names.dedup();
        names
    });
    &VOCAB
}

fn pointer(base: &str, token: &str) -> String {
    format!("{base}/{}", token.replace('~', "~0").replace('/', "~1"))
}

fn type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

struct ManifestErrors(Vec<(String, String)>);
impl ManifestErrors {
    fn push(&mut self, path: impl Into<String>, message: impl Into<String>) {
        self.0.push((path.into(), message.into()));
    }
    fn wrong_type(&mut self, path: impl Into<String>, expected: &str, value: &serde_json::Value) {
        self.push(path, format!("must be {expected}, got {}", type_name(value)));
    }
}

fn validate_manifest_entry(where_: &str, entry: &serde_json::Value, errors: &mut ManifestErrors) {
    use serde_json::Value;
    let Some(fields) = entry.as_object() else {
        return errors.wrong_type(where_, "an object", entry);
    };
    let mut unknown: Vec<&str> = fields.keys().map(String::as_str).filter(|k| !ENTRY_FIELDS.contains(k)).collect();
    unknown.sort_unstable();
    for key in unknown {
        errors.push(pointer(where_, key), "unknown field");
    }
    let enum_field = |errors: &mut ManifestErrors, key: &str, allowed: &[&'static str], default: &'static str| -> Option<&'static str> {
        match fields.get(key) {
            None => Some(default),
            Some(Value::String(s)) => match allowed.iter().find(|a| **a == s.as_str()) {
                Some(found) => Some(*found),
                None => { errors.push(pointer(where_, key), format!("{s:?} is not one of {allowed:?}")); None }
            },
            Some(other) => { errors.wrong_type(pointer(where_, key), "a string", other); None }
        }
    };
    let source = enum_field(errors, "source", &["git", "pypi"], "git");
    let tier = enum_field(errors, "trust_tier", &["official", "community"], "official");
    let string = |errors: &mut ManifestErrors, key: &str| -> Option<String> {
        match fields.get(key) {
            None => None,
            Some(Value::String(s)) => Some(s.clone()),
            Some(other) => { errors.wrong_type(pointer(where_, key), "a string", other); None }
        }
    };
    let git_ref = string(errors, "ref");
    let repo = string(errors, "repo");
    let abi_target = string(errors, "abi_target");
    string(errors, "description");
    string(errors, "provenance_manifest_ref");
    let official = tier == Some("official");

    match (source, git_ref.as_deref()) {
        (Some("git"), Some(r)) if official && !COMMIT_SHA_RE.is_match(r) => errors.push(
            pointer(where_, "ref"), "official git entries need an immutable 40-character lowercase commit SHA"),
        (Some("git"), Some(r)) if validate_registry_ref_impl(r, false).is_err() => errors.push(
            pointer(where_, "ref"), "not a safe git ref (branch, tag or commit SHA without traversal)"),
        (Some("pypi"), Some(r)) if !(r.is_empty() && !official) && !PYPI_VERSION_RE.is_match(r) => errors.push(
            pointer(where_, "ref"), if official { "official pypi entries need an exact version" } else { "not a version string" }),
        (Some(_), None) if official => errors.push(
            pointer(where_, "ref"), "required for official entries (an immutable commit SHA or exact version)"),
        _ => {}
    }
    if official && repo.as_deref().is_some_and(|r| !r.is_empty()) {
        errors.push(pointer(where_, "repo"), "official entries are built from the org repository; a custom repo is community tier");
    }
    match abi_target.as_deref() {
        Some(v) if !ABI_VERSION_RE.is_match(v) => errors.push(pointer(where_, "abi_target"), "must be MAJOR.MINOR.PATCH"),
        None if official => errors.push(pointer(where_, "abi_target"), "required for official entries"),
        _ => {}
    }

    let checksum_path = pointer(where_, "wasm_checksums");
    match fields.get("wasm_checksums") {
        None if official => errors.push(checksum_path, "required for official entries"),
        None => {}
        Some(Value::Object(map)) => {
            if official && map.is_empty() {
                errors.push(checksum_path.clone(), "official entries must pin at least one component");
            }
            for (file, digest) in map {
                let path = pointer(&checksum_path, file);
                if !WASM_FILE_RE.is_match(file) {
                    errors.push(path.clone(), "key is not a .wasm filename");
                }
                if !digest.as_str().is_some_and(|d| SHA256_RE.is_match(d)) {
                    errors.push(path, "not a lowercase SHA-256 hex digest");
                }
            }
        }
        Some(other) => errors.wrong_type(checksum_path, "an object", other),
    }

    let dep_path = pointer(where_, "dep_hashes");
    match fields.get("dep_hashes") {
        None => {}
        Some(Value::Object(map)) => {
            for (key, value) in map {
                let path = pointer(&dep_path, key);
                if !MANIFEST_DEP_KEY_RE.is_match(key) {
                    errors.push(path.clone(), "key is not 'package==version'");
                }
                if !value.as_str().is_some_and(|v| MANIFEST_DEP_HASH_RE.is_match(v)) {
                    errors.push(path, "not 'sha256:<lowercase hex>'");
                }
            }
        }
        Some(other) => errors.wrong_type(dep_path, "an object", other),
    }

    for key in ["allowed_dependencies", "capabilities"] {
        let path = pointer(where_, key);
        let Some(value) = fields.get(key) else {
            if key == "capabilities" && official {
                errors.push(path, "required for official entries");
            }
            continue;
        };
        let Some(items) = value.as_array() else {
            errors.wrong_type(path, "an array of strings", value);
            continue;
        };
        let mut seen = std::collections::BTreeSet::new();
        for (index, item) in items.iter().enumerate() {
            let item_path = pointer(&path, &index.to_string());
            let Some(text) = item.as_str() else {
                errors.wrong_type(item_path, "a string", item);
                continue;
            };
            if key == "capabilities" {
                if !plugin_capability_vocabulary().iter().any(|c| c == text) {
                    errors.push(item_path.clone(), format!("unknown capability {text:?}; expected one of {:?}", plugin_capability_vocabulary()));
                }
                if !seen.insert(text) {
                    errors.push(item_path, format!("duplicate capability {text:?}"));
                }
            }
        }
        if key == "capabilities" && official && items.is_empty() {
            errors.push(path, "official entries must declare at least one capability");
        }
    }
}

fn validate_renderer_bundle(bundle: &serde_json::Value, errors: &mut ManifestErrors) {
    let where_ = "/renderer_components";
    let Some(fields) = bundle.as_object() else {
        return errors.wrong_type(where_, "an object", bundle);
    };
    for key in fields.keys().filter(|k| !["repository", "ref", "artifact", "wasm_checksums"].contains(&k.as_str())) {
        errors.push(pointer(where_, key), "unknown field");
    }
    for (key, required) in [("repository", "intentumdiff-core"), ("artifact", "renderer-components")] {
        if fields.get(key).and_then(serde_json::Value::as_str) != Some(required) {
            errors.push(pointer(where_, key), format!("must be {required:?}"));
        }
    }
    if !fields.get("ref").and_then(serde_json::Value::as_str).is_some_and(|r| COMMIT_SHA_RE.is_match(r)) {
        errors.push(pointer(where_, "ref"), "must be an immutable 40-character lowercase commit SHA");
    }
    let pins_path = pointer(where_, "wasm_checksums");
    match fields.get("wasm_checksums") {
        Some(serde_json::Value::Object(pins)) => {
            for file in RENDERER_FILES.iter().filter(|f| !pins.contains_key(**f)) {
                errors.push(pointer(&pins_path, file), "missing renderer pin");
            }
            for (file, digest) in pins {
                let path = pointer(&pins_path, file);
                if !RENDERER_FILES.contains(&file.as_str()) {
                    errors.push(path.clone(), "unexpected renderer component");
                }
                if !digest.as_str().is_some_and(|d| SHA256_RE.is_match(d)) {
                    errors.push(path, "not a lowercase SHA-256 hex digest");
                }
            }
        }
        Some(other) => errors.wrong_type(pins_path, "an object", other),
        None => errors.push(pins_path, "required"),
    }
}

/// Validate a registry manifest (already parsed from YAML/JSON). `[]` = valid. Each error is
/// `(json_pointer_path, message)`; ordering is deterministic (document order, keys sorted).
pub(crate) fn validate_registry_manifest(manifest: &serde_json::Value) -> Vec<(String, String)> {
    let mut errors = ManifestErrors(Vec::new());
    let Some(top) = manifest.as_object() else {
        errors.wrong_type("", "an object", manifest);
        return errors.0;
    };
    for key in top.keys().filter(|k| !TOP_LEVEL_FIELDS.contains(&k.as_str())) {
        errors.push(pointer("", key), "unknown top-level field");
    }
    match top.get("version") {
        Some(v) if v.as_u64().is_some_and(|n| n >= 1) => {}
        Some(v) => errors.push("/version", format!("must be an integer >= 1, got {v}")),
        None => errors.push("/version", "required"),
    }
    if let Some(bundle) = top.get("renderer_components") {
        validate_renderer_bundle(bundle, &mut errors);
    }
    match top.get("plugins") {
        Some(serde_json::Value::Object(plugins)) => {
            for (name, entry) in plugins {
                let where_ = pointer("/plugins", name);
                if !PLUGIN_NAME_RE.is_match(name) {
                    errors.push(where_.clone(), "not a package-safe plugin name");
                }
                validate_manifest_entry(&where_, entry, &mut errors);
            }
        }
        Some(other) => errors.wrong_type("/plugins", "an object of name -> entry", other),
        None => errors.push("/plugins", "required"),
    }
    errors.0
}

/// C ABI `validate_registry_manifest`: `{"ok": bool, "errors": [{"path","message"}]}`.
pub(crate) fn validate_registry_manifest_json_impl(manifest_json: &str) -> Result<String, String> {
    let manifest: serde_json::Value =
        serde_json::from_str(manifest_json).map_err(|e| format!("manifest JSON: {e}"))?;
    let errors = validate_registry_manifest(&manifest);
    Ok(serde_json::json!({
        "ok": errors.is_empty(),
        "errors": errors.into_iter().map(|(path, message)| serde_json::json!({"path": path, "message": message})).collect::<Vec<_>>(),
    })
    .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ref_validator_rejects_traversal_and_slashes() {
        assert!(validate_registry_ref("main", false).is_ok());
        assert!(validate_registry_ref("v1.2.3", false).is_ok());
        assert!(validate_registry_ref("a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0", false).is_ok());
        assert!(validate_registry_ref("../etc/passwd", false).is_err());
        assert!(validate_registry_ref("feature/x", false).is_err()); // slash not allowed
        assert!(validate_registry_ref("a..b", false).is_err());
        assert!(validate_registry_ref("", false).is_err());
    }

    #[test]
    fn ref_validator_strict_requires_commit_sha() {
        let sha = "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0";
        assert!(validate_registry_ref(sha, true).is_ok());
        assert!(validate_registry_ref("main", true).is_err());
        assert!(validate_registry_ref("A1B2C3D4E5F6A7B8C9D0E1F2A3B4C5D6E7F8A9B0", true).is_err()); // uppercase
    }

    #[test]
    fn dep_hashes_wellformed_and_covered() {
        let ok = validate_dep_hashes(
            vec![("intentumdiff-foo==1.0.0".into(), format!("sha256:{}", "a".repeat(64)))],
            vec![],
            "intentumdiff-foo",
            None,
        );
        assert!(ok.is_empty(), "{ok:?}");

        // empty = self-contained wheel, no errors.
        assert!(validate_dep_hashes(vec![], vec![], "intentumdiff-foo", None).is_empty());
    }

    #[test]
    fn dep_hashes_flags_bad_key_value_and_missing_main() {
        let errs = validate_dep_hashes(
            vec![
                ("badkey".into(), "notahash".into()),
                ("other-pkg==2.0".into(), format!("sha256:{}", "b".repeat(64))),
            ],
            vec![],
            "intentumdiff-foo",
            None,
        );
        let joined = errs.join(" | ");
        assert!(joined.contains("Invalid dep_hashes key 'badkey'"), "{joined}");
        assert!(joined.contains("Invalid dep_hashes value for 'badkey'"), "{joined}");
        assert!(joined.contains("must include the main plugin package"), "{joined}");
        assert!(joined.contains("Unexpected dep_hashes package"), "{joined}");
    }

    #[test]
    fn dep_hashes_allows_listed_extra_dependency() {
        let errs = validate_dep_hashes(
            vec![
                ("intentumdiff-foo==1.0.0".into(), format!("sha256:{}", "a".repeat(64))),
                ("some-dep==3.1.4".into(), format!("sha256:{}", "c".repeat(64))),
            ],
            vec!["some_dep".into()],
            "intentumdiff-foo",
            None,
        );
        assert!(errs.is_empty(), "{errs:?}");
    }
}

// Pyo3-free `#[cfg(test)]` stand-ins for the retired registry `#[pyfunction]` wrappers (#B.6): the
// #88 security-validator test module drives these call shapes; production reaches the same `*_impl`
// functions via the C ABI's `validate_registry_ref` / `validate_dep_hashes` handlers.
#[cfg(test)]
pub(crate) fn validate_registry_ref(git_ref: &str, strict: bool) -> Result<(), String> {
    validate_registry_ref_impl(git_ref, strict)
}

#[cfg(test)]
pub(crate) fn validate_dep_hashes(
    dep_hashes: Vec<(String, String)>,
    allowed_dependencies: Vec<String>,
    package_name: &str,
    install_target: Option<&str>,
) -> Vec<String> {
    validate_dep_hashes_impl(dep_hashes, allowed_dependencies, package_name, install_target)
}
