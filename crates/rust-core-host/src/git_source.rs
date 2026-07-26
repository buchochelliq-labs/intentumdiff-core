//! Git source-collection read operations (#98, A2.4) — the single-file read side.
//!
//! Shells out to the `git` CLI (reusing `commit_plumbing::run_git`; NO libgit2/git2
//! crate), matching the already-certified Rust commit path and how GitPython itself
//! works. This is the first A2.4 increment: the git object-model operations behind
//! python `GitSource` / `WorkingTreeSource` — `repo.commit(ref)`, `commit.tree[path]`,
//! `blob.data_stream.read()`, `repo.working_dir` — become CLI calls here so the Python
//! shell drops GitPython for these paths. The changed-source iterators (working-tree /
//! staged / unpushed) follow in A2.4.2.

use std::collections::HashSet;
use std::path::Path;

use serde::Serialize;

use crate::commit_plumbing::{
    parse_name_status_z, run_git, safe_working_tree_path_rust, validate_git_ref,
};
use crate::content_type::detect_content_type;

/// Change-type codes that produce binary-safe text blobs on both sides — mirrors
/// python `git_source._TEXT_CHANGE_TYPES` (Add/Delete/Modify/Rename/Copy).
const TEXT_CHANGE_TYPES: [char; 5] = ['A', 'D', 'M', 'R', 'C'];

/// Run a git command and return its stdout trimmed to a `String` (for rev-parse etc.).
fn run_git_line<const N: usize>(repo_path: &str, args: [&str; N]) -> Result<String, String> {
    let out = run_git(repo_path, args, None)?;
    Ok(String::from_utf8_lossy(&out).trim().to_owned())
}

/// python `repo.commit(ref)`: resolve *git_ref* to a full commit sha (peeling tags).
/// `rev-parse --verify <ref>^{commit}` fails on an unknown ref, mirroring GitPython.
pub(crate) fn resolve_commit(repo_path: &str, git_ref: &str) -> Result<String, String> {
    validate_git_ref(git_ref)?;
    // The ^{commit} peel is our own trusted suffix (appended AFTER validating the
    // caller-supplied ref), so it cannot be argument-injection.
    let spec = format!("{git_ref}^{{commit}}");
    let sha = run_git_line(repo_path, ["rev-parse", "--verify", &spec])?;
    if sha.is_empty() {
        return Err(format!("could not resolve git ref: {git_ref}"));
    }
    Ok(sha)
}

/// python `commit.tree[path]` + `blob.data_stream.read()`: read a file from a commit
/// tree as a lossy-UTF-8 string. `Ok(None)` = the path is absent in that commit (the
/// caller raises FileNotFoundError), distinguishing "missing" from an empty file.
pub(crate) fn read_blob_at_rev(
    repo_path: &str,
    rev: &str,
    file_path: &str,
) -> Result<Option<String>, String> {
    if file_path.contains('\n') || file_path.contains('\r') || file_path.contains('\0') {
        return Err("git blob path contains a newline or NUL".to_owned());
    }
    // `rev` here is a resolved sha from resolve_commit (never option-like); the spec
    // `<rev>:<path>` is a single argv entry, so no shell / injection surface.
    let spec = format!("{rev}:{file_path}");
    match run_git(repo_path, ["cat-file", "blob", &spec], None) {
        Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
        // A cat-file failure on an already-resolved rev means the path is not in the
        // tree — GitPython surfaces this as KeyError -> FileNotFoundError.
        Err(_) => Ok(None),
    }
}

/// python `repo.working_dir`: the absolute working-tree root. Errors when *repo_path*
/// is not inside a work tree (bare repo / non-repo), mirroring GitPython's discovery.
pub(crate) fn repo_toplevel(repo_path: &str) -> Result<String, String> {
    let top = run_git_line(repo_path, ["rev-parse", "--show-toplevel"])?;
    if top.is_empty() {
        return Err("repository has no working tree (bare repository)".to_owned());
    }
    Ok(top)
}

#[derive(Serialize)]
struct ContentPair {
    old_content: String,
    new_content: String,
}

/// The two failure modes the content readers distinguish, so the binding raises the right
/// Python exception (FileNotFoundError vs ValueError). The C ABI maps the variant to its
/// `error_type` (`not_found` / `value_error`) in `dispatch_git_reader` so the ctypes binding
/// re-raises the same exception type without inspecting the message text.
pub(crate) enum GitReadError {
    /// An absent path / blob / working-tree file — python `FileNotFoundError`.
    NotFound(String),
    /// A bad ref, bare repo, or read/serialise failure — python `ValueError`.
    Invalid(String),
}

/// python `GitSource.get_content` git half: resolve *old_ref*/*new_ref* and read *file_path* from
/// each commit. Returns JSON `{old_content, new_content}`; `NotFound` when the path is absent on
/// either side. The C ABI (`dispatch_git_reader`) maps the error variant to an `error_type`.
pub(crate) fn git_source_content_impl(
    repo_path: &str,
    file_path: &str,
    old_ref: &str,
    new_ref: &str,
) -> Result<String, GitReadError> {
    let old_sha = resolve_commit(repo_path, old_ref).map_err(GitReadError::Invalid)?;
    let new_sha = resolve_commit(repo_path, new_ref).map_err(GitReadError::Invalid)?;
    let old_content = read_blob_at_rev(repo_path, &old_sha, file_path)
        .map_err(GitReadError::Invalid)?
        .ok_or_else(|| {
            GitReadError::NotFound(format!(
                "{file_path:?} not found in commit {}",
                &old_sha[..old_sha.len().min(8)]
            ))
        })?;
    let new_content = read_blob_at_rev(repo_path, &new_sha, file_path)
        .map_err(GitReadError::Invalid)?
        .ok_or_else(|| {
            GitReadError::NotFound(format!(
                "{file_path:?} not found in commit {}",
                &new_sha[..new_sha.len().min(8)]
            ))
        })?;
    serde_json::to_string(&ContentPair { old_content, new_content })
        .map_err(|e| GitReadError::Invalid(format!("serialize git source content: {e}")))
}

/// python `WorkingTreeSource.get_content` git half: old = *file_path* blob at *ref*; new = the
/// file on disk in the working tree. Returns JSON `{old_content, new_content}`. `NotFound` when
/// the blob is absent at *ref* or the working-tree file is missing; `Invalid` for a bare repo.
pub(crate) fn working_tree_source_content_impl(
    repo_path: &str,
    file_path: &str,
    git_ref: &str,
) -> Result<String, GitReadError> {
    let sha = resolve_commit(repo_path, git_ref).map_err(GitReadError::Invalid)?;
    let old_content = read_blob_at_rev(repo_path, &sha, file_path)
        .map_err(GitReadError::Invalid)?
        .ok_or_else(|| {
            GitReadError::NotFound(format!(
                "{file_path:?} not found in commit {}",
                &sha[..sha.len().min(8)]
            ))
        })?;
    let top = repo_toplevel(repo_path).map_err(GitReadError::Invalid)?;
    let disk = safe_working_tree_path_rust(Path::new(&top), file_path)
        .map_err(GitReadError::Invalid)?
        .ok_or_else(|| {
            GitReadError::NotFound(format!("{file_path:?} not found in working tree at {top}"))
        })?;
    let bytes = std::fs::read(&disk)
        .map_err(|e| GitReadError::NotFound(format!("read working-tree file: {e}")))?;
    let new_content = String::from_utf8_lossy(&bytes).into_owned();
    serde_json::to_string(&ContentPair { old_content, new_content })
        .map_err(|e| GitReadError::Invalid(format!("serialize working-tree content: {e}")))
}

/// python `git.Repo(path, search_parent_directories=True)` validation half: return the
/// working-tree root, raising ValueError when *repo_path* is not inside a git work tree
/// (replaces GitPython's InvalidGitRepositoryError at construction time).
/// A changed-source row: `(old_content, new_content, old_path, new_path, staging_status)`
/// — serialises to a JSON array matching the tuple python `iter_changed_sources` yields.
/// For commit-to-commit diffs `staging_status` is always `None`.
type ChangedRow = (String, String, String, String, Option<String>);

/// Read a blob at *rev*:*path* as RAW bytes (for magic-byte content routing, where the
/// lossy String would hide binary markers). `Ok(None)` = the path is absent in that tree.
fn read_blob_bytes_at_rev(
    repo_path: &str,
    rev: &str,
    file_path: &str,
) -> Result<Option<Vec<u8>>, String> {
    if file_path.contains('\n') || file_path.contains('\r') || file_path.contains('\0') {
        return Err("git blob path contains a newline or NUL".to_owned());
    }
    match run_git(repo_path, ["cat-file", "blob", &format!("{rev}:{file_path}")], None) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(_) => Ok(None),
    }
}

/// Read the INDEX (staged) version of *path* — `git cat-file blob :<path>` — as a lossy
/// UTF-8 string. `Ok(None)` = not present in the index.
fn read_index_blob(repo_path: &str, file_path: &str) -> Result<Option<String>, String> {
    if file_path.contains('\n') || file_path.contains('\r') || file_path.contains('\0') {
        return Err("git index path contains a newline or NUL".to_owned());
    }
    match run_git(repo_path, ["cat-file", "blob", &format!(":{file_path}")], None) {
        Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
        Err(_) => Ok(None),
    }
}

/// Shared `git diff --name-status -z <old> <new>` walk: read each side's blob (empty for
/// the absent side of an Add/Delete), NUL-skip binaries. *staging* labels every row. Used
/// by the commit-to-commit and unpushed branches (both are two-commit diffs).
fn diff_sources_between(
    repo_path: &str,
    old_sha: &str,
    new_sha: &str,
    staging: Option<&str>,
) -> Result<Vec<ChangedRow>, String> {
    let diff_out = run_git(repo_path, ["diff", "--name-status", "-z", old_sha, new_sha], None)?;
    let entries = parse_name_status_z(&diff_out)?;
    let mut rows: Vec<ChangedRow> = Vec::new();
    for (code, old_path, new_path) in entries {
        let c = code.chars().next().unwrap_or(' ');
        if !TEXT_CHANGE_TYPES.contains(&c) {
            continue;
        }
        let old_content = if c == 'A' {
            String::new()
        } else {
            read_blob_at_rev(repo_path, old_sha, &old_path)?.unwrap_or_default()
        };
        let new_content = if c == 'D' {
            String::new()
        } else {
            read_blob_at_rev(repo_path, new_sha, &new_path)?.unwrap_or_default()
        };
        if old_content.contains('\0') || new_content.contains('\0') {
            continue;
        }
        rows.push((old_content, new_content, old_path, new_path, staging.map(str::to_owned)));
    }
    Ok(rows)
}

/// python `iter_working_tree_sources`: old = blob at *old_ref*, new = the working-tree file
/// on disk; per-file staging ("staged" if in the index diff vs old, else "unstaged"), plus
/// untracked files as additions ("untracked"). Text/binary routing is MAGIC-BYTE
/// (detect_content_type on both sides), matching `_decode_text_or_none` — not the NUL rule.
fn working_tree_sources(repo_path: &str, old_ref: &str) -> Result<Vec<ChangedRow>, String> {
    let old_sha = resolve_commit(repo_path, old_ref)?;
    let top = repo_toplevel(repo_path)?;
    let root = Path::new(&top);

    // Files staged relative to old (the index diff) — used only for the staging label.
    let staged_out =
        run_git(repo_path, ["diff", "--cached", "--name-only", "-z", &old_sha], None)?;
    let staged: HashSet<String> = staged_out
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect();

    // Working-tree diff (old commit vs the working tree — staged + unstaged combined).
    let diff_out = run_git(repo_path, ["diff", "--name-status", "-z", &old_sha], None)?;
    let entries = parse_name_status_z(&diff_out)?;
    let mut rows: Vec<ChangedRow> = Vec::new();
    for (code, old_path, new_path) in entries {
        let c = code.chars().next().unwrap_or(' ');
        if !TEXT_CHANGE_TYPES.contains(&c) {
            continue;
        }
        let old_bytes = if c == 'A' {
            Vec::new()
        } else {
            read_blob_bytes_at_rev(repo_path, &old_sha, &old_path)?.unwrap_or_default()
        };
        let new_bytes = if c == 'D' {
            Vec::new()
        } else {
            match safe_working_tree_path_rust(root, &new_path)? {
                Some(p) => std::fs::read(&p).unwrap_or_default(),
                None => Vec::new(),
            }
        };
        // Magic-byte routing on BOTH sides (a modified PNG must never reach the parser).
        if !detect_content_type(&old_bytes).is_text || !detect_content_type(&new_bytes).is_text {
            continue;
        }
        let staging = if staged.contains(&new_path) { "staged" } else { "unstaged" };
        rows.push((
            String::from_utf8_lossy(&old_bytes).into_owned(),
            String::from_utf8_lossy(&new_bytes).into_owned(),
            old_path,
            new_path,
            Some(staging.to_owned()),
        ));
    }

    // Untracked files → additions.
    let untracked_out =
        run_git(repo_path, ["ls-files", "--others", "--exclude-standard", "-z"], None)?;
    for path_bytes in untracked_out.split(|&b| b == 0).filter(|s| !s.is_empty()) {
        let path = String::from_utf8_lossy(path_bytes).into_owned();
        let disk = match safe_working_tree_path_rust(root, &path)? {
            Some(p) => p,
            None => continue,
        };
        let bytes = std::fs::read(&disk).unwrap_or_default();
        if !detect_content_type(&bytes).is_text {
            continue;
        }
        rows.push((
            String::new(),
            String::from_utf8_lossy(&bytes).into_owned(),
            path.clone(),
            path,
            Some("untracked".to_owned()),
        ));
    }
    Ok(rows)
}

/// python `iter_staged_sources`: index-vs-*old_ref* (`git diff --cached`); old = the commit
/// blob, new = the INDEX blob (`:path`). NUL-skip binaries; every row is "staged".
fn staged_sources(repo_path: &str, old_ref: &str) -> Result<Vec<ChangedRow>, String> {
    let old_sha = resolve_commit(repo_path, old_ref)?;
    let diff_out =
        run_git(repo_path, ["diff", "--cached", "--name-status", "-z", &old_sha], None)?;
    let entries = parse_name_status_z(&diff_out)?;
    let mut rows: Vec<ChangedRow> = Vec::new();
    for (code, old_path, new_path) in entries {
        let c = code.chars().next().unwrap_or(' ');
        if !TEXT_CHANGE_TYPES.contains(&c) {
            continue;
        }
        let old_content = if c == 'A' {
            String::new()
        } else {
            read_blob_at_rev(repo_path, &old_sha, &old_path)?.unwrap_or_default()
        };
        let new_content = if c == 'D' {
            String::new()
        } else {
            read_index_blob(repo_path, &new_path)?.unwrap_or_default()
        };
        if old_content.contains('\0') || new_content.contains('\0') {
            continue;
        }
        rows.push((old_content, new_content, old_path, new_path, Some("staged".to_owned())));
    }
    Ok(rows)
}

/// python `iter_unpushed_sources`: diff the upstream tracking branch (`@{u}`) → HEAD.
/// Errors (ValueError) when the branch has no configured upstream. Every row is "unpushed".
fn unpushed_sources(repo_path: &str) -> Result<Vec<ChangedRow>, String> {
    let no_upstream = || {
        "Current branch has no remote tracking branch. Set one with \
         'git branch --set-upstream-to=<remote>/<branch>'."
            .to_owned()
    };
    let upstream = run_git(repo_path, ["rev-parse", "--verify", "--quiet", "@{u}"], None)
        .map_err(|_| no_upstream())?;
    let upstream_sha = String::from_utf8_lossy(&upstream).trim().to_owned();
    if upstream_sha.is_empty() {
        return Err(no_upstream());
    }
    let head = run_git(repo_path, ["rev-parse", "--verify", "HEAD"], None)?;
    let head_sha = String::from_utf8_lossy(&head).trim().to_owned();
    diff_sources_between(repo_path, &upstream_sha, &head_sha, Some("unpushed"))
}

/// python `iter_changed_sources(repo, old_ref, new_ref)` — the FULL dispatcher. Returns a
/// JSON array of `[old, new, old_path, new_path, staging|null]` rows. `new_ref` sentinels:
/// `""` = working tree, `":staged"` = the index, `":unpushed"` = commits not yet pushed
/// (old_ref ignored); anything else is a commit-to-commit diff (staging = null).
/// Internal: the FULL dispatcher as JSON values — the native review handler calls this so the
/// extension's default review (`new_ref = ""` = HEAD vs WORKING TREE) is served natively, not
/// just commit-to-commit. Same sentinels as `changed_sources_json`. (The binary has no Python
/// differ to fall back to, so feeding "" into the commit iterator was a hard
/// `fatal: Needed a single revision` error — the VS Code review storm bug.)
pub(crate) fn changed_sources(
    repo_path: &str,
    old_ref: &str,
    new_ref: &str,
) -> Result<Vec<serde_json::Value>, String> {
    let rows = if new_ref.is_empty() {
        working_tree_sources(repo_path, old_ref)
    } else if new_ref == ":staged" {
        staged_sources(repo_path, old_ref)
    } else if new_ref == ":unpushed" {
        unpushed_sources(repo_path)
    } else {
        let old_sha = resolve_commit(repo_path, old_ref)?;
        let new_sha = resolve_commit(repo_path, new_ref)?;
        diff_sources_between(repo_path, &old_sha, &new_sha, None)
    }?;
    rows.iter()
        .map(|r| serde_json::to_value(r).map_err(|e| format!("serialize changed source: {e}")))
        .collect()
}

/// Internal: the commit-to-commit changed sources as JSON values. (The native review handler
/// uses the FULL `changed_sources` dispatcher above so working-tree reviews work; this focused
/// commit-to-commit form remains for callers with explicit refs.)
pub(crate) fn changed_commit_sources(
    repo_path: &str,
    old_ref: &str,
    new_ref: &str,
) -> Result<Vec<serde_json::Value>, String> {
    let old_sha = resolve_commit(repo_path, old_ref)?;
    let new_sha = resolve_commit(repo_path, new_ref)?;
    let rows = diff_sources_between(repo_path, &old_sha, &new_sha, None)?;
    rows.iter()
        .map(|r| serde_json::to_value(r).map_err(|e| format!("serialize changed source: {e}")))
        .collect()
}

/// python `iter_changed_sources` commit-to-commit branch only — a focused entry point kept
/// alongside the full `changed_sources_json` dispatcher (both share `diff_sources_between`).
#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// Create a throwaway repo with one commit; returns its dir. Skips (returns None)
    /// if `git` is unavailable so the suite still runs in a git-less sandbox.
    fn temp_repo(file: &str, content: &str) -> Option<std::path::PathBuf> {
        let dir = std::env::temp_dir().join(format!("idf_gs_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).ok()?;
        let run = |args: &[&str]| {
            Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(args)
                .output()
                .ok()
                .filter(|o| o.status.success())
        };
        run(&["init"])?;
        run(&["config", "user.email", "t@example.com"])?;
        run(&["config", "user.name", "T"])?;
        std::fs::write(dir.join(file), content).ok()?;
        run(&["add", file])?;
        run(&["commit", "-m", "init"])?;
        Some(dir)
    }

    #[test]
    fn resolve_and_read_roundtrip() {
        let Some(dir) = temp_repo("hello.py", "print('hi')\n") else {
            return; // git unavailable — skip
        };
        let path = dir.to_str().unwrap();
        let sha = resolve_commit(path, "HEAD").expect("resolve HEAD");
        assert_eq!(sha.len(), 40, "sha: {sha}");
        let content = read_blob_at_rev(path, &sha, "hello.py")
            .expect("read ok")
            .expect("present");
        assert!(content.contains("print('hi')"), "{content}");
        // Missing path -> None (caller raises FileNotFoundError).
        assert!(read_blob_at_rev(path, &sha, "ghost.py").expect("read ok").is_none());
        // Toplevel resolves.
        assert!(!repo_toplevel(path).expect("toplevel").is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_rejects_bad_ref_and_injection() {
        let Some(dir) = temp_repo("a.py", "x = 1\n") else { return };
        let path = dir.to_str().unwrap();
        assert!(resolve_commit(path, "nonexistent-branch").is_err());
        // Argument-injection guard (leading '-') from validate_git_ref.
        assert!(resolve_commit(path, "--output=/tmp/evil").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn changed_commit_sources_add_and_modify() {
        let Some(dir) = temp_repo("a.py", "x = 1\n") else { return };
        let path = dir.to_str().unwrap();
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(args)
                .output()
                .ok()
                .filter(|o| o.status.success())
        };
        // Second commit: modify a.py + add b.py.
        std::fs::write(dir.join("a.py"), "x = 2\n").unwrap();
        std::fs::write(dir.join("b.py"), "y = 1\n").unwrap();
        run(&["add", "a.py", "b.py"]).expect("add");
        run(&["commit", "-m", "v2"]).expect("commit");

        let json = changed_commit_sources_json(path, "HEAD~1", "HEAD").expect("changed");
        // a.py modified: old x=1, new x=2. b.py added: old "", new y=1.
        assert!(json.contains("a.py"), "{json}");
        assert!(json.contains("x = 1"), "{json}");
        assert!(json.contains("x = 2"), "{json}");
        assert!(json.contains("b.py"), "{json}");
        assert!(json.contains("y = 1"), "{json}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn git_in(dir: &std::path::Path, args: &[&str]) -> Option<std::process::Output> {
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
    }

    #[test]
    fn changed_sources_working_tree_unstaged_and_untracked() {
        let Some(dir) = temp_repo("app.py", "x = 1\n") else { return };
        let path = dir.to_str().unwrap();
        // Modify the committed file on disk (unstaged) + drop an untracked file.
        std::fs::write(dir.join("app.py"), "x = 2\n").unwrap();
        std::fs::write(dir.join("new_mod.py"), "y = 1\n").unwrap();

        let json = changed_sources_json(path, "HEAD", "").expect("working-tree");
        assert!(json.contains("app.py") && json.contains("unstaged"), "{json}");
        assert!(json.contains("new_mod.py") && json.contains("untracked"), "{json}");
        assert!(json.contains("x = 2") && json.contains("y = 1"), "{json}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn changed_sources_staged_reads_index() {
        let Some(dir) = temp_repo("app.py", "x = 1\n") else { return };
        let path = dir.to_str().unwrap();
        std::fs::write(dir.join("app.py"), "x = 2\n").unwrap();
        git_in(&dir, &["add", "app.py"]).expect("add");

        let json = changed_sources_json(path, "HEAD", ":staged").expect("staged");
        assert!(json.contains("app.py") && json.contains("staged"), "{json}");
        assert!(json.contains("x = 1"), "old from commit: {json}");
        assert!(json.contains("x = 2"), "new from index: {json}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

// Pyo3-free `#[cfg(test)]` stand-ins for the retired `changed_*_sources_json` `#[pyfunction]`
// wrappers (#B.6): the git-reader test module drives these call shapes; production reaches the
// same `changed_sources` / `changed_commit_sources` impls via the C ABI's `changed_sources`
// handlers. `PyResult<String>` becomes `Result<String, String>` (the impl error is already a String).
#[cfg(test)]
pub(crate) fn changed_sources_json(
    repo_path: &str,
    old_ref: &str,
    new_ref: &str,
) -> Result<String, String> {
    let rows = changed_sources(repo_path, old_ref, new_ref)?;
    serde_json::to_string(&rows).map_err(|e| format!("serialize changed sources: {e}"))
}

#[cfg(test)]
pub(crate) fn changed_commit_sources_json(
    repo_path: &str,
    old_ref: &str,
    new_ref: &str,
) -> Result<String, String> {
    let rows = changed_commit_sources(repo_path, old_ref, new_ref)?;
    serde_json::to_string(&rows).map_err(|e| format!("serialize changed commit sources: {e}"))
}
