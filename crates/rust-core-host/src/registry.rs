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
/// is already pyo3-free, so the C ABI (`intentdiff_call`) calls it directly.
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
            vec![("intentdiff-foo==1.0.0".into(), format!("sha256:{}", "a".repeat(64)))],
            vec![],
            "intentdiff-foo",
            None,
        );
        assert!(ok.is_empty(), "{ok:?}");

        // empty = self-contained wheel, no errors.
        assert!(validate_dep_hashes(vec![], vec![], "intentdiff-foo", None).is_empty());
    }

    #[test]
    fn dep_hashes_flags_bad_key_value_and_missing_main() {
        let errs = validate_dep_hashes(
            vec![
                ("badkey".into(), "notahash".into()),
                ("other-pkg==2.0".into(), format!("sha256:{}", "b".repeat(64))),
            ],
            vec![],
            "intentdiff-foo",
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
                ("intentdiff-foo==1.0.0".into(), format!("sha256:{}", "a".repeat(64))),
                ("some-dep==3.1.4".into(), format!("sha256:{}", "c".repeat(64))),
            ],
            vec!["some_dep".into()],
            "intentdiff-foo",
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
