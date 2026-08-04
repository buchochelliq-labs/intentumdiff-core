//! Shared VCS backend (#98, A2.4.4) — git/hg/svn/perforce backend operations live in the
//! core, shelling to each VCS CLI. The Python `vcs/*_backend.py` classes become THIN wrappers
//! over these entrypoints, so every binding shares one implementation and no third-party VCS
//! library (GitPython / python-hglib / p4python) is needed at runtime.
//!
//! git, hg, svn and perforce (p4) are implemented here. Perforce shells the `p4` CLI with
//! `-ztag` output and resolves its connection from the environment / P4CONFIG + cwd (like the
//! other CLIs), so no p4python is needed. Unknown VCS ids return an explicit error.

use std::process::Command;

use serde::Serialize;

use crate::commit_plumbing::parse_name_status_z;

/// The CLI binary for a backend id.
fn vcs_binary(vcs: &str) -> Result<&'static str, String> {
    match vcs {
        "git" => Ok("git"),
        "hg" => Ok("hg"),
        "svn" => Ok("svn"),
        "p4" => Ok("p4"),
        other => Err(format!("unknown VCS backend id: {other:?}")),
    }
}

/// Reject a ref/path that could be mis-read as a CLI option (argument injection) or that
/// contains a newline/NUL. A leading '-' is never a legitimate ref for git/hg/svn/p4.
fn validate_arg(value: &str, what: &str) -> Result<(), String> {
    if value.starts_with('-') {
        return Err(format!("{what} must not start with '-' (argument-injection guard)"));
    }
    if value.contains('\n') || value.contains('\r') || value.contains('\0') {
        return Err(format!("{what} must not contain a newline or NUL"));
    }
    Ok(())
}

/// Run `<vcs-binary> <args>` with the working directory set to *repo_path*.
fn run_vcs(vcs: &str, repo_path: &str, args: &[&str]) -> Result<Vec<u8>, String> {
    let bin = vcs_binary(vcs)?;
    let output = Command::new(bin)
        .current_dir(repo_path)
        .args(args)
        .output()
        .map_err(|e| format!("spawn {bin}: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "{bin} {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output.stdout)
}

#[derive(Serialize)]
struct ChangedFile {
    old_path: Option<String>,
    new_path: Option<String>,
    change_type: String,
    is_binary: bool,
}

fn git_change_type(code: char) -> Option<&'static str> {
    match code {
        'A' => Some("added"),
        'D' => Some("deleted"),
        'M' => Some("modified"),
        'R' => Some("renamed"),
        'C' => Some("copied"),
        _ => None,
    }
}

/// git: a NUL byte in a blob's content marks it binary (the Python backend heuristic).
fn git_blob_is_binary(repo_path: &str, spec: &str) -> bool {
    match run_vcs("git", repo_path, &["cat-file", "blob", spec]) {
        Ok(data) => data[..data.len().min(8192)].contains(&0),
        Err(_) => false,
    }
}

/// Internal blob reader (raw bytes at *git_ref*, `None` when absent) — reused by the native
/// live-diff handler (`live_server::live_handle_diff_json`) so it can read the "old" side
/// without a Python round-trip. git/hg/svn only (the live path is git).
pub(crate) fn read_blob(vcs: &str, repo_path: &str, path: &str, git_ref: &str) -> Option<Vec<u8>> {
    match vcs {
        "git" => run_vcs("git", repo_path, &["cat-file", "blob", &format!("{git_ref}:{path}")]),
        "hg" => run_vcs("hg", repo_path, &["cat", "-r", git_ref, path]),
        "svn" => run_vcs("svn", repo_path, &["cat", "-r", git_ref, path]),
        _ => Err(String::new()),
    }
    .ok()
}

fn changed_file(path: String, change_type: &str) -> ChangedFile {
    ChangedFile {
        old_path: if change_type == "added" { None } else { Some(path.clone()) },
        new_path: if change_type == "deleted" { None } else { Some(path) },
        change_type: change_type.to_owned(),
        is_binary: false,
    }
}

/// Parse `svn diff --summarize --xml` (mirrors python `_parse_svn_diff_summary`).
fn parse_svn_diff_summary_xml(xml: &str) -> Vec<ChangedFile> {
    let Ok(doc) = roxmltree::Document::parse(xml) else { return Vec::new() };
    let mut rows = Vec::new();
    for node in doc.descendants().filter(|n| n.has_tag_name("path")) {
        if node.attribute("kind") != Some("file") {
            continue;
        }
        let ct = match node.attribute("item").unwrap_or("modified") {
            "added" => "added",
            "deleted" => "deleted",
            "modified" | "replaced" => "modified", // SVN "replaced" -> modified
            _ => continue,
        };
        let path = node.text().unwrap_or("").trim();
        if !path.is_empty() {
            rows.push(changed_file(path.to_owned(), ct));
        }
    }
    rows
}

/// Parse `svn status --xml` (mirrors python `_parse_svn_status`).
fn parse_svn_status_xml(xml: &str) -> Vec<ChangedFile> {
    let Ok(doc) = roxmltree::Document::parse(xml) else { return Vec::new() };
    let mut rows = Vec::new();
    for entry in doc.descendants().filter(|n| n.has_tag_name("entry")) {
        let path = entry.attribute("path").unwrap_or("").trim();
        if path.is_empty() {
            continue;
        }
        let Some(wc) = entry.children().find(|n| n.has_tag_name("wc-status")) else { continue };
        let ct = match wc.attribute("item").unwrap_or("") {
            "added" => "added",
            "deleted" => "deleted",
            "modified" | "replaced" | "conflicted" | "merged" => "modified",
            _ => continue, // unversioned / ignored / normal / external / none
        };
        rows.push(changed_file(path.to_owned(), ct));
    }
    rows
}

/// Parse `hg status` text (`<code> <path>`), mirroring python `_parse_hg_status`.
fn parse_hg_status(text: &str) -> Vec<ChangedFile> {
    let mut rows = Vec::new();
    for line in text.lines() {
        if line.len() < 3 {
            continue;
        }
        let ct = match line.as_bytes()[0] {
            b'A' => "added",
            b'R' | b'!' => "deleted", // R = removed; ! = missing but tracked
            b'M' => "modified",
            _ => continue, // ? unknown, C clean, I ignored, etc.
        };
        let path = line[2..].trim();
        if !path.is_empty() {
            rows.push(changed_file(path.to_owned(), ct));
        }
    }
    rows
}

/// p4 injection guard: a depot path (`//...`) must not smuggle an `@`/`#` revision specifier
/// into the filespec (mirrors python `_validate_p4_depot_path`).
fn validate_p4_depot_path(path: &str) -> Result<(), String> {
    if path.starts_with("//") && (path.contains('@') || path.contains('#')) {
        return Err(format!(
            "Depot path {path:?} contains '@' or '#' which would create an unintended \
             Perforce revision specifier."
        ));
    }
    Ok(())
}

/// p4 ref guard: changelist number / label / keyword only — no filespec metacharacters
/// (mirrors python `_validate_p4_ref`).
fn validate_p4_ref(value: &str) -> Result<(), String> {
    let ok = !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ' ' | '-'));
    if !ok {
        return Err(format!(
            "Unsafe Perforce revision specifier {value:?}. Must be a changelist number or \
             label name (alphanumeric/dot/hyphen/space)."
        ));
    }
    Ok(())
}

/// Split `p4 -ztag ...` output into records: blank lines separate records; each
/// `... <field> <value>` line becomes one map entry.
fn parse_p4_ztag(text: &str) -> Vec<std::collections::HashMap<String, String>> {
    let mut records = Vec::new();
    let mut cur: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for raw in text.lines() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() {
            if !cur.is_empty() {
                records.push(std::mem::take(&mut cur));
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("... ") {
            match rest.split_once(' ') {
                Some((field, value)) => {
                    cur.insert(field.to_owned(), value.to_owned());
                }
                None => {
                    cur.insert(rest.to_owned(), String::new());
                }
            }
        }
    }
    if !cur.is_empty() {
        records.push(cur);
    }
    records
}

/// Parse `p4 -ztag diff2 -q` records (mirrors python `PerforceVcsBackend.list_changed_files`).
fn parse_p4_diff2(text: &str) -> Vec<ChangedFile> {
    let mut rows = Vec::new();
    for rec in parse_p4_ztag(text) {
        let Some(depot) = rec
            .get("depotFile")
            .or_else(|| rec.get("depotFile1"))
            .filter(|d| !d.is_empty())
        else {
            continue;
        };
        let ct = match rec.get("status").map(String::as_str).unwrap_or("content") {
            "content" => "modified",
            "missing exists" => "added",
            "exists missing" => "deleted",
            _ => continue, // "exists" == unmodified
        };
        rows.push(changed_file(depot.clone(), ct));
    }
    rows
}

/// Parse `p4 -ztag opened` records (mirrors python `_P4_ACTION_MAP` in the perforce backend).
fn parse_p4_opened(text: &str) -> Vec<ChangedFile> {
    let mut rows = Vec::new();
    for rec in parse_p4_ztag(text) {
        let Some(depot) = rec.get("depotFile").filter(|d| !d.is_empty()) else {
            continue;
        };
        let ct = match rec.get("action").map(String::as_str).unwrap_or("edit") {
            "add" | "import" => "added",
            "delete" | "purge" => "deleted",
            "edit" | "integrate" => "modified",
            "branch" => "copied",
            "move/add" | "move/delete" => "renamed",
            _ => "modified",
        };
        rows.push(changed_file(depot.clone(), ct));
    }
    rows
}

/// hg ref guard: changeset hash / rev number / tag / bookmark / "." — rejects revset
/// expressions and shell metacharacters (mirrors python `_validate_hg_rev`, an #88 control).
fn validate_hg_ref(value: &str) -> Result<(), String> {
    let ok = !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-' | '+' | ':'));
    if !ok {
        return Err(format!(
            "Unsafe Mercurial revision {value:?}. Must be a changeset hash, revision number, \
             tag, bookmark, or '.'."
        ));
    }
    Ok(())
}

/// svn ref guard: integer / HEAD / BASE / PREV / COMMITTED / date {..} / range — rejects
/// shell metacharacters (mirrors python `_validate_svn_rev`, an #88 control).
fn validate_svn_ref(value: &str) -> Result<(), String> {
    let ok = !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '{' | '}' | '_' | ':' | '.' | '-'));
    if !ok {
        return Err(format!(
            "Unsafe SVN revision {value:?}. Must be an integer, HEAD/BASE/PREV/COMMITTED, \
             a date {{...}}, or a range."
        ));
    }
    Ok(())
}

/// python `VcsBackend.resolve_root` — work-tree root for *repo_path* under *vcs*, dispatched per
/// VCS. The C ABI (`intentumdiff_call`) calls this directly.
pub(crate) fn vcs_backend_resolve_root_impl(vcs: &str, repo_path: &str) -> Result<String, String> {
    // Perforce: the client root is a field of `p4 info`, not raw stdout.
    if vcs == "p4" {
        let out = run_vcs("p4", repo_path, &["-ztag", "info"])?;
        let root = parse_p4_ztag(&String::from_utf8_lossy(&out))
            .into_iter()
            .find_map(|mut r| r.remove("clientRoot"))
            .unwrap_or_default();
        let root = root.trim();
        if root.is_empty() {
            return Err("p4: could not resolve client root".to_owned());
        }
        return Ok(root.to_owned());
    }
    let out = match vcs {
        "git" => run_vcs("git", repo_path, &["rev-parse", "--show-toplevel"]),
        "hg" => run_vcs("hg", repo_path, &["root"]),
        "svn" => run_vcs("svn", repo_path, &["info", "--show-item", "wc-root"]),
        other => Err(format!("resolve_root not yet ported for VCS {other:?}")),
    }?;
    let root = String::from_utf8_lossy(&out).trim().to_owned();
    if root.is_empty() {
        return Err(format!("{vcs}: could not resolve repository root"));
    }
    Ok(root)
}

/// python `VcsBackend.get_blob` — content of *path* at *git_ref* ("" when absent).
/// *svn_repo_url* (svn only) targets a repository URL instead of the working copy.
pub(crate) fn vcs_backend_get_blob_impl(
    vcs: &str,
    repo_path: &str,
    path: &str,
    git_ref: &str,
    svn_repo_url: Option<&str>,
) -> Result<String, String> {
    validate_arg(git_ref, "ref")?;
    validate_arg(path, "path")?;
    let out = match vcs {
        "git" => run_vcs("git", repo_path, &["cat-file", "blob", &format!("{git_ref}:{path}")]),
        "hg" => {
            validate_hg_ref(git_ref)?;
            run_vcs("hg", repo_path, &["cat", "-r", git_ref, path])
        }
        "svn" => {
            validate_svn_ref(git_ref)?;
            // repo-URL mode targets `{url}/{path}`; else the WC-relative path (cwd = repo_path).
            let target = match svn_repo_url {
                Some(url) if !url.is_empty() => format!("{}/{}", url.trim_end_matches('/'), path),
                _ => path.to_string(),
            };
            run_vcs("svn", repo_path, &["cat", "-r", git_ref, target.as_str()])
        }
        "p4" => {
            // Depot path + changelist/label are joined into a `path@ref` filespec.
            validate_p4_depot_path(path)?;
            validate_p4_ref(git_ref)?;
            run_vcs("p4", repo_path, &["print", "-q", &format!("{path}@{git_ref}")])
        }
        other => return Err(format!("get_blob not yet ported for VCS {other:?}")),
    };
    // An absent path yields "" (matches the Python backends).
    Ok(out.map(|d| String::from_utf8_lossy(&d).into_owned()).unwrap_or_default())
}

/// python `VcsBackend.list_changed_files` — changes between two refs.
/// *svn_repo_url* (svn only) targets a repository URL instead of the working copy.
pub(crate) fn vcs_backend_changed_files_impl(
    vcs: &str,
    repo_path: &str,
    ref_a: &str,
    ref_b: &str,
    svn_repo_url: Option<&str>,
) -> Result<String, String> {
    validate_arg(ref_a, "ref")?;
    validate_arg(ref_b, "ref")?;
    let rows: Vec<ChangedFile> = match vcs {
        "git" => {
            let out = run_vcs("git", repo_path, &["diff", "--name-status", "-z", ref_a, ref_b])?;
            let mut rows: Vec<ChangedFile> = Vec::new();
            for (code, old_path, new_path) in parse_name_status_z(&out)? {
                let c = code.chars().next().unwrap_or(' ');
                let Some(change_type) = git_change_type(c) else { continue };
                let op = if c == 'A' { None } else { Some(old_path) };
                let np = if c == 'D' { None } else { Some(new_path) };
                if op.is_none() && np.is_none() {
                    continue;
                }
                let (probe_ref, probe_path) = match &np {
                    Some(p) => (ref_b, p.as_str()),
                    None => (ref_a, op.as_deref().unwrap_or("")),
                };
                let is_binary = git_blob_is_binary(repo_path, &format!("{probe_ref}:{probe_path}"));
                rows.push(ChangedFile { old_path: op, new_path: np, change_type: change_type.to_owned(), is_binary });
            }
            rows
        }
        "svn" => {
            validate_svn_ref(ref_a)?;
            validate_svn_ref(ref_b)?;
            let rng = format!("-r{ref_a}:{ref_b}");
            // repo-URL mode diffs the URL; else the working copy (".", cwd = repo_path).
            let base = match svn_repo_url {
                Some(url) if !url.is_empty() => url,
                _ => ".",
            };
            let out = run_vcs("svn", repo_path, &["diff", "--summarize", "--xml", &rng, base])?;
            parse_svn_diff_summary_xml(&String::from_utf8_lossy(&out))
        }
        "hg" => {
            validate_hg_ref(ref_a)?;
            validate_hg_ref(ref_b)?;
            let out = run_vcs("hg", repo_path, &["status", "--rev", ref_a, "--rev", ref_b])?;
            parse_hg_status(&String::from_utf8_lossy(&out))
        }
        "p4" => {
            validate_p4_ref(ref_a)?;
            validate_p4_ref(ref_b)?;
            let (spec_a, spec_b) = (format!("//...@{ref_a}"), format!("//...@{ref_b}"));
            let out = run_vcs("p4", repo_path, &["-ztag", "diff2", "-q", &spec_a, &spec_b])?;
            parse_p4_diff2(&String::from_utf8_lossy(&out))
        }
        other => return Err(format!("changed_files not yet ported for VCS {other:?}")),
    };
    serde_json::to_string(&rows).map_err(|e| format!("serialize changed files: {e}"))
}

/// python `VcsBackend.list_working_tree_changes` — uncommitted changes vs *git_ref*
/// (binaries skipped).
pub(crate) fn vcs_backend_working_tree_changes_impl(
    vcs: &str,
    repo_path: &str,
    git_ref: &str,
) -> Result<String, String> {
    validate_arg(git_ref, "ref")?;
    let rows: Vec<ChangedFile> = match vcs {
        "git" => {
            let root = vcs_backend_resolve_root_impl("git", repo_path)?;
            let out = run_vcs("git", repo_path, &["diff", "--name-status", "-z", git_ref])?;
            let mut rows: Vec<ChangedFile> = Vec::new();
            for (code, old_path, new_path) in parse_name_status_z(&out)? {
                let c = code.chars().next().unwrap_or(' ');
                let Some(change_type) = git_change_type(c) else { continue };
                // Skip binary — new side on disk, old blob for deletions.
                let is_binary = if c == 'D' {
                    git_blob_is_binary(repo_path, &format!("{git_ref}:{old_path}"))
                } else {
                    std::fs::read(std::path::Path::new(&root).join(&new_path))
                        .map(|d| d[..d.len().min(8192)].contains(&0))
                        .unwrap_or(false)
                };
                if is_binary {
                    continue;
                }
                let op = if old_path.is_empty() { None } else { Some(old_path) };
                let np = if new_path.is_empty() { None } else { Some(new_path) };
                rows.push(ChangedFile { old_path: op, new_path: np, change_type: change_type.to_owned(), is_binary: false });
            }
            rows
        }
        "svn" => {
            let out = run_vcs("svn", repo_path, &["status", "--xml", "."])?;
            parse_svn_status_xml(&String::from_utf8_lossy(&out))
        }
        "hg" => {
            let out = run_vcs("hg", repo_path, &["status"])?;
            parse_hg_status(&String::from_utf8_lossy(&out))
        }
        "p4" => {
            // Files opened in the default pending changelist (mirrors `opened -c default`).
            let out = run_vcs("p4", repo_path, &["-ztag", "opened", "-c", "default"])?;
            parse_p4_opened(&String::from_utf8_lossy(&out))
        }
        other => return Err(format!("working_tree_changes not yet ported for VCS {other:?}")),
    };
    serde_json::to_string(&rows).map_err(|e| format!("serialize wt changes: {e}"))
}

/// python `GitVcsBackend.get_merge_base` — common ancestor of two refs.
pub(crate) fn vcs_backend_merge_base_impl(
    vcs: &str,
    repo_path: &str,
    ref_a: &str,
    ref_b: &str,
) -> Result<String, String> {
    validate_arg(ref_a, "ref")?;
    validate_arg(ref_b, "ref")?;
    let out = match vcs {
        "git" => run_vcs("git", repo_path, &["merge-base", ref_a, ref_b]),
        other => return Err(format!("merge_base not supported for VCS {other:?}")),
    }
    .map_err(|_| format!("No common ancestor found between {ref_a:?} and {ref_b:?}"))?;
    let sha = String::from_utf8_lossy(&out).trim().to_owned();
    if sha.is_empty() {
        return Err(format!("No common ancestor found between {ref_a:?} and {ref_b:?}"));
    }
    Ok(sha)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_git_repo() -> Option<std::path::PathBuf> {
        let dir = std::env::temp_dir().join(format!("idf_vcs_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).ok()?;
        let run = |args: &[&str]| {
            Command::new("git")
                .current_dir(&dir)
                .args(args)
                .output()
                .ok()
                .filter(|o| o.status.success())
        };
        run(&["init"])?;
        run(&["config", "user.email", "t@example.com"])?;
        run(&["config", "user.name", "T"])?;
        std::fs::write(dir.join("a.py"), "x = 1\n").ok()?;
        run(&["add", "a.py"])?;
        run(&["commit", "-m", "v1"])?;
        std::fs::write(dir.join("a.py"), "x = 2\n").ok()?;
        std::fs::write(dir.join("b.py"), "y = 1\n").ok()?;
        run(&["add", "a.py", "b.py"])?;
        run(&["commit", "-m", "v2"])?;
        Some(dir)
    }

    #[test]
    fn git_backend_ops() {
        let Some(dir) = temp_git_repo() else { return };
        let p = dir.to_str().unwrap();
        // resolve_root
        assert!(!vcs_backend_resolve_root("git", p).unwrap().is_empty());
        // get_blob (present + absent)
        assert!(vcs_backend_get_blob("git", p, "a.py", "HEAD", None).unwrap().contains("x = 2"));
        assert_eq!(vcs_backend_get_blob("git", p, "ghost.py", "HEAD", None).unwrap(), "");
        // changed_files HEAD~1..HEAD: a.py modified, b.py added
        let cf = vcs_backend_changed_files_json("git", p, "HEAD~1", "HEAD", None).unwrap();
        assert!(cf.contains("a.py") && cf.contains("modified"), "{cf}");
        assert!(cf.contains("b.py") && cf.contains("added"), "{cf}");
        // merge-base of HEAD~1 and HEAD is HEAD~1
        assert_eq!(vcs_backend_merge_base("git", p, "HEAD~1", "HEAD").unwrap().len(), 40);
        // injection guard
        assert!(vcs_backend_get_blob("git", p, "a.py", "--evil", None).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_svn_diff_summary() {
        let xml = r#"<?xml version="1.0"?>
<diff><paths>
<path props="none" kind="file" item="modified">src/foo.c</path>
<path props="none" kind="file" item="added">bar.c</path>
<path props="none" kind="file" item="deleted">gone.c</path>
<path props="none" kind="dir" item="added">newdir</path>
</paths></diff>"#;
        let rows = parse_svn_diff_summary_xml(xml);
        assert_eq!(rows.len(), 3, "dir entry should be skipped");
        assert!(rows.iter().any(|r| r.new_path.as_deref() == Some("src/foo.c") && r.change_type == "modified"));
        let added = rows.iter().find(|r| r.change_type == "added").unwrap();
        assert_eq!(added.new_path.as_deref(), Some("bar.c"));
        assert!(added.old_path.is_none());
        let del = rows.iter().find(|r| r.change_type == "deleted").unwrap();
        assert!(del.new_path.is_none() && del.old_path.as_deref() == Some("gone.c"));
    }

    #[test]
    fn parse_svn_status() {
        let xml = r#"<?xml version="1.0"?>
<status><target path=".">
<entry path="mod.py"><wc-status item="modified" props="none"/></entry>
<entry path="new.py"><wc-status item="added" props="none"/></entry>
<entry path="clean.py"><wc-status item="normal" props="none"/></entry>
<entry path="junk.tmp"><wc-status item="unversioned" props="none"/></entry>
</target></status>"#;
        let rows = parse_svn_status_xml(xml);
        assert_eq!(rows.len(), 2, "normal + unversioned should be skipped");
        assert!(rows.iter().any(|r| r.new_path.as_deref() == Some("mod.py") && r.change_type == "modified"));
        assert!(rows.iter().any(|r| r.new_path.as_deref() == Some("new.py") && r.change_type == "added"));
    }

    #[test]
    fn parse_hg_status_lines() {
        let text = "M src/foo.py\nA bar.py\nR gone.py\n! missing.py\n? junk.tmp\nC clean.py\n";
        let rows = parse_hg_status(text);
        assert_eq!(rows.len(), 4, "? and C are skipped");
        assert!(rows.iter().any(|r| r.new_path.as_deref() == Some("src/foo.py") && r.change_type == "modified"));
        let added = rows.iter().find(|r| r.change_type == "added").unwrap();
        assert_eq!(added.new_path.as_deref(), Some("bar.py"));
        assert!(added.old_path.is_none());
        // R (removed) and ! (missing) both map to deleted.
        assert_eq!(rows.iter().filter(|r| r.change_type == "deleted").count(), 2);
    }

    #[test]
    fn parse_p4_diff2_records() {
        let text = "\
... depotFile1 //depot/main/foo.py
... status content
... type text

... depotFile //depot/main/bar.py
... status missing exists

... depotFile //depot/main/gone.py
... status exists missing

... depotFile //depot/main/same.py
... status exists
";
        let rows = parse_p4_diff2(text);
        assert_eq!(rows.len(), 3, "'exists' (unmodified) is skipped");
        assert!(rows.iter().any(|r| r.new_path.as_deref() == Some("//depot/main/foo.py") && r.change_type == "modified"));
        let added = rows.iter().find(|r| r.change_type == "added").unwrap();
        assert_eq!(added.new_path.as_deref(), Some("//depot/main/bar.py"));
        assert!(added.old_path.is_none());
        let del = rows.iter().find(|r| r.change_type == "deleted").unwrap();
        assert!(del.new_path.is_none() && del.old_path.as_deref() == Some("//depot/main/gone.py"));
    }

    #[test]
    fn parse_p4_opened_actions() {
        let text = "\
... depotFile //depot/main/a.py
... action edit

... depotFile //depot/main/b.py
... action add

... depotFile //depot/main/c.py
... action move/add
";
        let rows = parse_p4_opened(text);
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().any(|r| r.change_type == "modified" && r.new_path.as_deref() == Some("//depot/main/a.py")));
        assert!(rows.iter().any(|r| r.change_type == "added" && r.new_path.as_deref() == Some("//depot/main/b.py")));
        assert!(rows.iter().any(|r| r.change_type == "renamed" && r.new_path.as_deref() == Some("//depot/main/c.py")));
    }

    #[test]
    fn p4_ref_and_depot_validation() {
        // Accept: changelist number, label, keyword.
        assert!(validate_p4_ref("12345").is_ok());
        assert!(validate_p4_ref("rel-1.0").is_ok());
        assert!(validate_p4_ref("default").is_ok());
        // Reject: @/# filespec injection.
        assert!(validate_p4_ref("12345@admin").is_err());
        assert!(validate_p4_ref("head#1").is_err());
        // Depot-path @/# injection.
        assert!(validate_p4_depot_path("//depot/file.py").is_ok());
        assert!(validate_p4_depot_path("//depot/file@label").is_err());
        assert!(validate_p4_depot_path("//depot/file#123").is_err());
    }

    #[test]
    fn hg_ref_validation() {
        // Accept: working-dir parent, hash prefix, rev number.
        assert!(validate_hg_ref(".").is_ok());
        assert!(validate_hg_ref("a1b2c3d4").is_ok());
        assert!(validate_hg_ref("42").is_ok());
        // Reject: revset expressions + shell metacharacters.
        assert!(validate_hg_ref("tip or branch(default)").is_err());
        assert!(validate_hg_ref("tip; rm -rf /").is_err());
    }

    #[test]
    fn svn_ref_validation() {
        // Accept: keywords, integer, date range.
        assert!(validate_svn_ref("HEAD").is_ok());
        assert!(validate_svn_ref("42").is_ok());
        assert!(validate_svn_ref("{2024-01-01}").is_ok());
        // Reject: shell metacharacters / pipes.
        assert!(validate_svn_ref("HEAD; rm -rf /").is_err());
        assert!(validate_svn_ref("1|echo evil").is_err());
    }

    #[test]
    fn unknown_vcs_errors() {
        // An unknown VCS id + svn/hg merge-base are unsupported -> clear errors.
        assert!(vcs_backend_changed_files_json("bzr", ".", "a", "b", None).is_err());
        assert!(vcs_backend_merge_base("svn", ".", "1", "2").is_err());
    }
}

// Pyo3-free `#[cfg(test)]` stand-ins for the retired VCS-backend `#[pyfunction]` wrappers (#B.6):
// the multi-VCS test module drives these call shapes; production reaches the same `*_impl`
// functions via the C ABI's `vcs_backend_*` handlers. Every impl already returns `Result<_, String>`.
#[cfg(test)]
pub(crate) fn vcs_backend_resolve_root(vcs: &str, repo_path: &str) -> Result<String, String> {
    vcs_backend_resolve_root_impl(vcs, repo_path)
}

#[cfg(test)]
pub(crate) fn vcs_backend_get_blob(
    vcs: &str,
    repo_path: &str,
    path: &str,
    git_ref: &str,
    svn_repo_url: Option<&str>,
) -> Result<String, String> {
    vcs_backend_get_blob_impl(vcs, repo_path, path, git_ref, svn_repo_url)
}

#[cfg(test)]
pub(crate) fn vcs_backend_changed_files_json(
    vcs: &str,
    repo_path: &str,
    ref_a: &str,
    ref_b: &str,
    svn_repo_url: Option<&str>,
) -> Result<String, String> {
    vcs_backend_changed_files_impl(vcs, repo_path, ref_a, ref_b, svn_repo_url)
}

#[cfg(test)]
pub(crate) fn vcs_backend_merge_base(
    vcs: &str,
    repo_path: &str,
    ref_a: &str,
    ref_b: &str,
) -> Result<String, String> {
    vcs_backend_merge_base_impl(vcs, repo_path, ref_a, ref_b)
}
