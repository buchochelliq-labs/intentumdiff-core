//! Shared host capability exclusions, applied before compiling components.
//!
//! The old PowerShell Wasm aborts Wasmtime/Cranelift on Windows ARM64.
//! A specific rebuilt component is proven to compile on that platform. Trust
//! its *bytes*, not a parser name, a moving git ref, or the host platform alone.
//! Callers must hash the actual staged bytes before the component is loaded.
use serde_json::{json, Value};

const LEGACY_ARM64_REASON: &str =
    "cranelift cannot emit one of its functions on Windows/aarch64 ('function too large'); loading it aborts the process";
const UNVERIFIED_ARM64_REASON: &str =
    "PowerShell Wasm component is not verified for Windows ARM64; refusing to compile a component that may abort the process";

/// Source-judged PowerShell parser PR #5 (9b8bd6d8), build #40665277189:
/// exact CI-produced component compiled by official Wasmtime 47.0.3 on
/// Windows ARM64 and passed the native PowerShell UTF-8/CRLF span test.
/// This is an immutable *artifact* digest, not a source commit or ZIP digest.
pub const VERIFIED_POWERSHELL_WINDOWS_ARM64_SHA256: &str =
    "29a7be79db83f80627e98da8513a160dc047ea972fae4e3be07d741793a30c15";

fn is_windows_arm64(system: &str, machine: &str) -> bool {
    system.eq_ignore_ascii_case("windows")
        && (machine.eq_ignore_ascii_case("arm64") || machine.eq_ignore_ascii_case("aarch64"))
}

/// Legacy API: no component identity is available, so it **must** keep denying
/// PowerShell on Windows ARM64 even after a new compatible Wasm is published.
pub fn incompatible_reason(name: &str, system: &str, machine: &str) -> Option<&'static str> {
    (name == "powershell" && is_windows_arm64(system, machine))
        .then_some(LEGACY_ARM64_REASON)
}

/// Shared Rust policy for an actual staged component's SHA-256. Only the
/// independently verified Wasm bytes are allowed on Windows ARM64. Unknown,
/// absent and legacy checksums fail closed; all other platforms are unchanged.
pub fn incompatible_reason_with_digest(
    name: &str,
    system: &str,
    machine: &str,
    component_sha256: &str,
) -> Option<&'static str> {
    if name != "powershell" || !is_windows_arm64(system, machine) {
        return None;
    }
    if component_sha256.eq_ignore_ascii_case(VERIFIED_POWERSHELL_WINDOWS_ARM64_SHA256) {
        None
    } else {
        Some(UNVERIFIED_ARM64_REASON)
    }
}

fn is_powershell_filename(filename: &str) -> bool {
    let basename = filename.rsplit(['/', '\\']).next().unwrap_or("");
    let Some(suffix) = std::path::Path::new(basename).extension().and_then(|s| s.to_str()) else {
        return false;
    };
    matches!(suffix.to_ascii_lowercase().as_str(), "ps1" | "psm1" | "psd1")
}

pub fn unavailable_for_filename(
    filename: &str,
    system: &str,
    machine: &str,
) -> Option<(&'static str, &'static str)> {
    is_powershell_filename(filename)
        .then(|| incompatible_reason("powershell", system, machine))
        .flatten()
        .map(|reason| ("powershell", reason))
}

pub fn unavailable_for_filename_with_digest(
    filename: &str,
    system: &str,
    machine: &str,
    component_sha256: &str,
) -> Option<(&'static str, &'static str)> {
    is_powershell_filename(filename)
        .then(|| incompatible_reason_with_digest("powershell", system, machine, component_sha256))
        .flatten()
        .map(|reason| ("powershell", reason))
}

/// Retained for existing C ABI consumers. An unknown component is never
/// promoted to compatible just because the parser build process was fixed.
pub fn availability_json_impl(name: &str, filename: &str, system: &str, machine: &str) -> String {
    let unavailable: Value = unavailable_for_filename(filename, system, machine)
        .map(|(name, reason)| json!([name, reason])).unwrap_or(Value::Null);
    json!({"reason":incompatible_reason(name, system, machine),"unavailable":unavailable}).to_string()
}

/// New C ABI consumer contract. The SHA-256 is computed from the actual staged
/// Wasm bytes by the host (host I/O) and evaluated only by this Rust policy.
pub fn availability_json_with_digest_impl(
    name: &str,
    filename: &str,
    system: &str,
    machine: &str,
    component_sha256: &str,
) -> String {
    let unavailable: Value =
        unavailable_for_filename_with_digest(filename, system, machine, component_sha256)
            .map(|(parser, reason)| json!([parser, reason]))
            .unwrap_or(Value::Null);
    json!({
        "reason": incompatible_reason_with_digest(name, system, machine, component_sha256),
        "unavailable": unavailable
    }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_exclusion_is_limited_to_measured_platform_and_component() {
        for machine in ["ARM64", "aarch64", "AArch64"] {
            assert!(incompatible_reason("powershell", "Windows", machine).is_some());
            assert!(incompatible_reason("python", "Windows", machine).is_none());
        }
        for (os, arch) in [("macos","aarch64"),("Darwin","arm64"),("linux","aarch64"),("windows","x86_64")] {
            assert!(incompatible_reason("powershell", os, arch).is_none());
        }
        assert!(unavailable_for_filename("DEPLOY.PS1", "windows", "arm64").is_some());
        assert!(unavailable_for_filename("x.py", "windows", "arm64").is_none());
        for path in [".ps1", "dir/.ps1", r"dir\.ps1"] {
            assert!(unavailable_for_filename(path, "windows", "arm64").is_none());
        }
    }

    #[test]
    fn verified_digest_only_lifts_the_actual_windows_arm64_component() {
        let good = VERIFIED_POWERSHELL_WINDOWS_ARM64_SHA256;
        let legacy = "db4db1515d13b3f452057d0a6c3c659ae9b3b484ef3cb572c7a19d670981c30a";
        for machine in ["ARM64", "aarch64", "AArch64"] {
            assert_eq!(incompatible_reason_with_digest("powershell", "Windows", machine, good), None);
            assert_eq!(
                incompatible_reason_with_digest("powershell", "Windows", machine, &good.to_ascii_uppercase()),
                None
            );
            for digest in ["", legacy, "0", "29a7be79"] {
                assert!(incompatible_reason_with_digest("powershell", "Windows", machine, digest).is_some(),
                    "{digest} should not bypass the ARM64 guard");
            }
            assert!(incompatible_reason_with_digest("python", "Windows", machine, "").is_none());
            assert!(unavailable_for_filename_with_digest("DEPLOY.PS1", "Windows", machine, good).is_none());
            assert!(unavailable_for_filename_with_digest("deploy.ps1", "Windows", machine, legacy).is_some());
            assert!(unavailable_for_filename_with_digest("x.py", "Windows", machine, legacy).is_none());
            // Older C ABI consumers cannot prove their component's identity.
            assert!(unavailable_for_filename("deploy.ps1", "Windows", machine).is_some());
        }
        for (os, arch) in [("Darwin","arm64"),("Linux","aarch64"),("Windows","x86_64")] {
            assert!(incompatible_reason_with_digest("powershell", os, arch, "").is_none());
        }
    }

    #[test]
    fn digest_availability_json_fail_closed_and_no_false_exclusions() {
        let good = VERIFIED_POWERSHELL_WINDOWS_ARM64_SHA256;
        let accepted: Value =
            serde_json::from_str(&availability_json_with_digest_impl("powershell", "src/deploy.ps1", "Windows", "ARM64", good)).unwrap();
        assert!(accepted["reason"].is_null());
        assert!(accepted["unavailable"].is_null());
        let rejected: Value =
            serde_json::from_str(&availability_json_with_digest_impl("powershell", "src/deploy.ps1", "Windows", "ARM64", "")).unwrap();
        assert!(rejected["reason"].as_str().unwrap().contains("not verified"));
        assert_eq!(rejected["unavailable"][0], "powershell");
        let other: Value =
            serde_json::from_str(&availability_json_with_digest_impl("python", "src/test.py", "Windows", "ARM64", "")).unwrap();
        assert!(other["reason"].is_null());
        assert!(other["unavailable"].is_null());
    }
}
