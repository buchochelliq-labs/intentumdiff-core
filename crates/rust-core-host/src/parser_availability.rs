//! Shared host capability exclusions, applied before compiling components.
//! Windows ARM PowerShell compilation panics in Cranelift (`function too large`).
//! This measured restriction must not exclude Apple Silicon or Windows x86-64.
use serde_json::{json, Value};

pub fn incompatible_reason(name: &str, system: &str, machine: &str) -> Option<&'static str> {
    (name == "powershell" && system.eq_ignore_ascii_case("windows")
        && (machine.eq_ignore_ascii_case("arm64") || machine.eq_ignore_ascii_case("aarch64")))
        .then_some("cranelift cannot emit one of its functions on Windows/aarch64 ('function too large'); loading it aborts the process")
}

pub fn unavailable_for_filename(filename: &str, system: &str, machine: &str) -> Option<(&'static str, &'static str)> {
    let basename = filename.rsplit(['/', '\\']).next()?;
    let suffix = std::path::Path::new(basename).extension()?.to_str()?.to_ascii_lowercase();
    if !matches!(suffix.as_str(), "ps1" | "psm1" | "psd1") { return None; }
    incompatible_reason("powershell", system, machine).map(|reason| ("powershell", reason))
}

pub fn availability_json_impl(name: &str, filename: &str, system: &str, machine: &str) -> String {
    let unavailable: Value = unavailable_for_filename(filename, system, machine)
        .map(|(name, reason)| json!([name, reason])).unwrap_or(Value::Null);
    json!({"reason":incompatible_reason(name, system, machine),"unavailable":unavailable}).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exclusion_is_limited_to_measured_platform_and_component() {
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
}
