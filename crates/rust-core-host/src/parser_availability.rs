//! Shared host capability exclusions, applied before compiling components.
//!
//! Metered Wasmtime 47.0.3 compilation of the currently pinned PowerShell
//! component aborts Cranelift on Windows ARM64 even though the unmetered CLI
//! compiled it. No digest is currently certified safe with metering on that
//! platform. Deny the known candidate *before* instantiating the Wasm compiler;
//! do not disable fuel as a workaround or manufacture successful results.
use serde_json::{json, Value};
use sha2::Digest;
use std::path::Path;

const LEGACY_ARM64_REASON: &str =
    "cranelift cannot emit one of its functions on Windows/aarch64 ('function too large'); loading it aborts the process";
const UNVERIFIED_ARM64_REASON: &str =
    "PowerShell Wasm component is not verified under metered Wasmtime on Windows ARM64; refusing to compile a component that may abort the process";
const UNMETERED_ONLY_ARM64_REASON: &str =
    "PowerShell Wasm compiled without fuel, but Wasmtime 47.0.3 Cranelift panics with fuel metering enabled on Windows ARM64; refusing the CLI-only vetted component";

/// Provenance of the *unmetered-only* PowerShell parser from PR #5 (9b8bd6d8).
/// The official CLI compiled these bytes, but production-like Rust host CI
/// #40683114538 and #40683768275 reproduced a fatal Cranelift panic with
/// Config::consume_fuel(true), across all tested Cranelift opt levels.
/// This digest MUST NOT authorize execution until metered compilation is fixed.
pub const UNMETERED_ONLY_POWERSHELL_WINDOWS_ARM64_SHA256: &str =
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

/// Shared Rust policy for an actual staged component's SHA-256. A digest
/// proven only with fuel disabled is insufficient: the full Rust/Python host
/// uses metering, and the known candidate aborts Cranelift on Windows ARM64.
/// Until an independently host-verified replacement is certified, deny all
/// PowerShell components on that platform. Other platforms are unchanged.
pub fn incompatible_reason_with_digest(
    name: &str,
    system: &str,
    machine: &str,
    component_sha256: &str,
) -> Option<&'static str> {
    if name != "powershell" || !is_windows_arm64(system, machine) {
        return None;
    }
    if component_sha256.eq_ignore_ascii_case(UNMETERED_ONLY_POWERSHELL_WINDOWS_ARM64_SHA256) {
        Some(UNMETERED_ONLY_ARM64_REASON)
    } else {
        Some(UNVERIFIED_ARM64_REASON)
    }
}

/// Recognise the standard PowerShell Wasm artifact before invoking Wasmtime.
/// This guard is a last line of defence if a host bypasses parser discovery.
fn is_powershell_component_filename(filename: &str) -> bool {
    // CI publishes the prefixed name; provisioning strips the prefix. Guard
    // *both* representations or a direct native loader can bypass the block.
    filename.rsplit(['/', '\\']).next().is_some_and(|basename| {
        matches!(basename.to_ascii_lowercase().as_str(),
            "powershell_parser.wasm"
            | "intentdiff_powershell_parser.wasm"
            | "intentumdiff_powershell_parser.wasm")
    })
}

pub(crate) fn guarded_powershell_component(path: &Path, system: &str, machine: &str) -> bool {
    incompatible_reason("powershell", system, machine).is_some()
        && is_powershell_component_filename(&path.to_string_lossy())
}

/// Rust-owned pre-compilation policy for raw Wasm loader clients. A known
/// unmetered-only digest is still rejected to avoid an uncaught compiler panic.
/// The host must never skip fuel metering to accept that component.
pub fn component_availability_json_impl(
    wasm_filename: &str,
    system: &str,
    machine: &str,
    component_sha256: &str,
) -> String {
    let reason = is_powershell_component_filename(wasm_filename)
        .then(|| incompatible_reason_with_digest("powershell", system, machine, component_sha256))
        .flatten();
    json!({"reason": reason}).to_string()
}

/// Check the bytes that will actually be compiled. Do not hash one file read
/// and then reopen the path for compilation: the component may have changed.
pub(crate) fn verify_staged_powershell_bytes(
    path: &Path,
    bytes: &[u8],
    system: &str,
    machine: &str,
) -> Result<(), String> {
    if !guarded_powershell_component(path, system, machine) {
        return Ok(());
    }
    let digest = hex::encode(sha2::Sha256::digest(bytes));
    if let Some(reason) = incompatible_reason_with_digest("powershell", system, machine, &digest) {
        return Err(format!("refusing unsafe PowerShell component {}: {reason}", path.display()));
    }
    Ok(())
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

/// C ABI consumer contract. The SHA-256 is computed from the actual staged
/// Wasm bytes by the host (host I/O) and evaluated only by this Rust policy.
/// This does not authorize any known unsafe Windows ARM64 metered component.
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
    fn unmetered_only_digest_cannot_lift_the_windows_arm64_restriction() {
        let good = UNMETERED_ONLY_POWERSHELL_WINDOWS_ARM64_SHA256;
        let legacy = "db4db1515d13b3f452057d0a6c3c659ae9b3b484ef3cb572c7a19d670981c30a";
        for machine in ["ARM64", "aarch64", "AArch64"] {
            assert!(incompatible_reason_with_digest("powershell", "Windows", machine, good).is_some());
            assert_eq!(
                incompatible_reason_with_digest("powershell", "Windows", machine, &good.to_ascii_uppercase()),
                Some(UNMETERED_ONLY_ARM64_REASON)
            );
            for digest in ["", legacy, "0", "29a7be79"] {
                assert!(incompatible_reason_with_digest("powershell", "Windows", machine, digest).is_some(),
                    "{digest} should not bypass the ARM64 guard");
            }
            assert!(incompatible_reason_with_digest("python", "Windows", machine, "").is_none());
            assert!(unavailable_for_filename_with_digest("DEPLOY.PS1", "Windows", machine, good).is_some());
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
    fn component_loader_probe_and_digest_follow_shared_rust_policy() {
        let unmetered = UNMETERED_ONLY_POWERSHELL_WINDOWS_ARM64_SHA256;
        let probe: Value = serde_json::from_str(&component_availability_json_impl(
            "powershell_parser.wasm", "windows", "ARM64", ""
        )).unwrap();
        assert!(probe["reason"].is_string());
        let known_unmetered: Value = serde_json::from_str(&component_availability_json_impl(
            r"plugins\powershell_parser.wasm", "windows", "ARM64", unmetered
        )).unwrap();
        assert!(known_unmetered["reason"].as_str().unwrap().contains("fuel"));
        let forbidden: Value = serde_json::from_str(&component_availability_json_impl(
            "powershell_parser.wasm", "windows", "aarch64", "0000"
        )).unwrap();
        assert!(forbidden["reason"].is_string());
        let unrelated: Value = serde_json::from_str(&component_availability_json_impl(
            "python_parser.wasm", "windows", "ARM64", ""
        )).unwrap();
        assert!(unrelated["reason"].is_null());
    }

    #[test]
    fn native_wasmtime_guard_rejects_unverified_bytes_before_compilation() {
        let standard = Path::new("powershell_parser.wasm");
        assert!(guarded_powershell_component(standard, "windows", "aarch64"));
        for path in [
            "intentdiff_powershell_parser.wasm",
            "intentumdiff_powershell_parser.wasm",
            r"subdir\INTENTUMDIFF_POWERSHELL_PARSER.WASM",
        ] {
            assert!(guarded_powershell_component(Path::new(path), "windows", "arm64"), "{path}");
            let refused: Value = serde_json::from_str(&component_availability_json_impl(
                path, "windows", "ARM64", UNMETERED_ONLY_POWERSHELL_WINDOWS_ARM64_SHA256
            )).unwrap();
            assert!(refused["reason"].is_string(), "{path} bypassed metered safety guard");
        }
        let rejected = verify_staged_powershell_bytes(
            standard, b"not the vetted PowerShell Wasm", "windows", "aarch64"
        ).unwrap_err();
        assert!(rejected.contains("refusing unsafe PowerShell component"), "{rejected}");
        assert!(!guarded_powershell_component(standard, "windows", "x86_64"));
        assert!(verify_staged_powershell_bytes(
            standard, b"not the vetted PowerShell Wasm", "Darwin", "arm64"
        ).is_ok());
        assert!(!guarded_powershell_component(Path::new("python_parser.wasm"), "windows", "aarch64"));
    }

    #[test]
    fn digest_availability_json_fail_closed_and_no_false_exclusions() {
        let good = UNMETERED_ONLY_POWERSHELL_WINDOWS_ARM64_SHA256;
        let known_unmetered: Value =
            serde_json::from_str(&availability_json_with_digest_impl("powershell", "src/deploy.ps1", "Windows", "ARM64", good)).unwrap();
        assert!(known_unmetered["reason"].as_str().unwrap().contains("fuel metering"));
        assert_eq!(known_unmetered["unavailable"][0], "powershell");
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
