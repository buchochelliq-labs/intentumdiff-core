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
    String::from_utf8(out)
        .map(|text| text.trim().to_owned())
        .map_err(|e| format!("invalid UTF-8 in Git metadata: {e}"))
}

/// Read one Git object specification without conflating a missing blob with a
/// failed Git invocation. In --batch mode, a missing spec has an explicit
/// "SPEC missing" record; process failures and malformed records remain errors.
fn read_blob_spec_bytes(repo_path: &str, spec: &str) -> Result<Option<Vec<u8>>, String> {
    if spec.bytes().any(|byte| matches!(byte, b'\n' | b'\r' | b'\0')) {
        return Err("git blob spec contains a newline or NUL".to_owned());
    }
    let request = format!("{spec}\n");
    let output = run_git(repo_path, ["cat-file", "--batch"], Some(request.as_bytes()))?;
    let header_end = output
        .iter()
        .position(|byte| *byte == b'\n')
        .ok_or_else(|| format!("missing git cat-file batch header for {spec:?}"))?;
    let header = std::str::from_utf8(&output[..header_end])
        .map_err(|e| format!("invalid git cat-file batch header for {spec:?}: {e}"))?;
    let payload = &output[header_end + 1..];

    if header == format!("{spec} missing") {
        if !payload.is_empty() {
            return Err(format!("unexpected payload for missing Git blob {spec:?}"));
        }
        return Ok(None);
    }

    let mut parts = header.split_ascii_whitespace();
    let object_id = parts
        .next()
        .ok_or_else(|| format!("missing object ID for Git blob {spec:?}"))?;
    let kind = parts
        .next()
        .ok_or_else(|| format!("missing object type for Git blob {spec:?}"))?;
    let size = parts
        .next()
        .ok_or_else(|| format!("missing size for Git blob {spec:?}"))?
        .parse::<usize>()
        .map_err(|e| format!("invalid size for Git blob {spec:?}: {e}"))?;
    if parts.next().is_some()
        || kind != "blob"
        || !matches!(object_id.len(), 40 | 64)
        || !object_id.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(format!("unexpected Git blob header for {spec:?}: {header:?}"));
    }
    let expected_len = size
        .checked_add(1)
        .ok_or_else(|| format!("Git blob size overflow for {spec:?}"))?;
    if payload.len() != expected_len || payload[size] != b'\n' {
        return Err(format!("incomplete Git blob payload for {spec:?}"));
    }
    Ok(Some(payload[..size].to_vec()))
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
/// tree as exact UTF-8. `Ok(None)` = the path is absent in that commit (the
/// caller raises FileNotFoundError), distinguishing "missing" from an empty file.
pub(crate) fn read_blob_at_rev(
    repo_path: &str,
    rev: &str,
    file_path: &str,
) -> Result<Option<String>, String> {
    if file_path.contains('\n') || file_path.contains('\r') || file_path.contains('\0') {
        return Err("git blob path contains a newline or NUL".to_owned());
    }
    read_blob_bytes_at_rev(repo_path, rev, file_path)?
        .map(|bytes| {
            String::from_utf8(bytes)
                .map_err(|e| format!("invalid UTF-8 in Git blob {rev}:{file_path}: {e}"))
        })
        .transpose()
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
    let bytes = std::fs::read(&disk).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            GitReadError::NotFound(format!("working-tree file disappeared {file_path:?}: {e}"))
        } else {
            GitReadError::Invalid(format!("read working-tree file {file_path:?}: {e}"))
        }
    })?;
    let new_content = String::from_utf8(bytes)
        .map_err(|e| GitReadError::Invalid(format!(
            "invalid UTF-8 in working-tree file {file_path:?}: {e}"
        )))?;
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

/// Read a blob at *rev*:*path* as RAW bytes for binary routing. `Ok(None)`
/// means the path is actually absent, never that Git failed.
fn read_blob_bytes_at_rev(
    repo_path: &str,
    rev: &str,
    file_path: &str,
) -> Result<Option<Vec<u8>>, String> {
    if file_path.contains('\n') || file_path.contains('\r') || file_path.contains('\0') {
        return Err("git blob path contains a newline or NUL".to_owned());
    }
    read_blob_spec_bytes(repo_path, &format!("{rev}:{file_path}"))
}

/// Read the INDEX (staged) version of *path* as raw bytes so binary files
/// can be excluded before strict decoding. `Ok(None)` = absent in the index.
fn read_index_blob(repo_path: &str, file_path: &str) -> Result<Option<Vec<u8>>, String> {
    if file_path.contains('\n') || file_path.contains('\r') || file_path.contains('\0') {
        return Err("git index path contains a newline or NUL".to_owned());
    }
    read_blob_spec_bytes(repo_path, &format!(":{file_path}"))
}

/// Convert a magic-byte-classified source to exact text, never replacing invalid
/// bytes. Opaque unknown content with no NUL is not sufficient evidence of
/// intentional binary input: a corrupt source must not silently disappear.
fn decode_changed_source(bytes: &[u8], path: &str) -> Result<Option<String>, String> {
    let kind = detect_content_type(bytes);
    if !kind.is_text {
        if kind.mime == "application/octet-stream" && !bytes.contains(&0) {
            std::str::from_utf8(bytes)
                .map_err(|e| format!("invalid UTF-8 in Git source {path:?}: {e}"))?;
        }
        return Ok(None);
    }
    std::str::from_utf8(bytes)
        .map(|text| Some(text.to_owned()))
        .map_err(|e| format!("invalid UTF-8 in Git source {path:?}: {e}"))
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
        let old_bytes = if c == 'A' {
            Vec::new()
        } else {
            read_blob_bytes_at_rev(repo_path, old_sha, &old_path)?
                .ok_or_else(|| format!("changed path {old_path:?} missing at {old_sha}"))?
        };
        let new_bytes = if c == 'D' {
            Vec::new()
        } else {
            read_blob_bytes_at_rev(repo_path, new_sha, &new_path)?
                .ok_or_else(|| format!("changed path {new_path:?} missing at {new_sha}"))?
        };
        // Preserve the existing commit-to-commit NUL binary exclusion, but
        // reject invalid UTF-8 rather than generating replacement characters.
        if old_bytes.contains(&0) || new_bytes.contains(&0) {
            continue;
        }
        let old_content = String::from_utf8(old_bytes)
            .map_err(|e| format!("invalid UTF-8 in Git blob {old_path:?}: {e}"))?;
        let new_content = String::from_utf8(new_bytes)
            .map_err(|e| format!("invalid UTF-8 in Git blob {new_path:?}: {e}"))?;
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
    if !staged_out.is_empty() && !staged_out.ends_with(&[0]) {
        return Err("incomplete staged-path inventory: missing NUL".to_owned());
    }
    let staged: HashSet<String> = staged_out
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .map(|path| {
            std::str::from_utf8(path)
                .map(str::to_owned)
                .map_err(|e| format!("invalid UTF-8 in staged path: {e}"))
        })
        .collect::<Result<_, _>>()?;

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
            read_blob_bytes_at_rev(repo_path, &old_sha, &old_path)?
                .ok_or_else(|| format!("changed path {old_path:?} missing at {old_sha}"))?
        };
        let new_bytes = if c == 'D' {
            Vec::new()
        } else {
            let disk = safe_working_tree_path_rust(root, &new_path)?
                .ok_or_else(|| format!("changed working-tree path {new_path:?} disappeared"))?;
            std::fs::read(&disk)
                .map_err(|e| format!("read changed working-tree file {new_path:?}: {e}"))?
        };
        // Magic-byte routing on BOTH sides (a modified PNG must never reach the parser).
        // Unknown opaque source bytes without NUL must fail closed, not disappear.
        let old_content = decode_changed_source(&old_bytes, &old_path)?;
        let new_content = decode_changed_source(&new_bytes, &new_path)?;
        let (Some(old_content), Some(new_content)) = (old_content, new_content) else {
            continue;
        };
        let staging = if staged.contains(&new_path) { "staged" } else { "unstaged" };
        rows.push((old_content, new_content, old_path, new_path, Some(staging.to_owned())));
    }

    // Untracked files → additions.
    let untracked_out =
        run_git(repo_path, ["ls-files", "--others", "--exclude-standard", "-z"], None)?;
    if !untracked_out.is_empty() && !untracked_out.ends_with(&[0]) {
        return Err("incomplete untracked-path inventory: missing NUL".to_owned());
    }
    for path_bytes in untracked_out.split(|&b| b == 0).filter(|s| !s.is_empty()) {
        let path = std::str::from_utf8(path_bytes)
            .map_err(|e| format!("invalid UTF-8 in untracked path: {e}"))?
            .to_owned();
        let disk = safe_working_tree_path_rust(root, &path)?
            .ok_or_else(|| format!("untracked path {path:?} disappeared"))?;
        let bytes = std::fs::read(&disk)
            .map_err(|e| format!("read untracked source {path:?}: {e}"))?;
        let Some(content) = decode_changed_source(&bytes, &path)? else {
            continue;
        };
        rows.push((String::new(), content, path.clone(), path, Some("untracked".to_owned())));
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
        let old_bytes = if c == 'A' {
            Vec::new()
        } else {
            read_blob_bytes_at_rev(repo_path, &old_sha, &old_path)?
                .ok_or_else(|| format!("staged old path {old_path:?} missing at {old_sha}"))?
        };
        let new_bytes = if c == 'D' {
            Vec::new()
        } else {
            read_index_blob(repo_path, &new_path)?
                .ok_or_else(|| format!("staged new path {new_path:?} missing in index"))?
        };
        // Preserve NUL-binary exclusion before decoding staged bytes.
        if old_bytes.contains(&0) || new_bytes.contains(&0) {
            continue;
        }
        let old_content = String::from_utf8(old_bytes)
            .map_err(|e| format!("invalid UTF-8 in staged old source {old_path:?}: {e}"))?;
        let new_content = String::from_utf8(new_bytes)
            .map_err(|e| format!("invalid UTF-8 in staged new source {new_path:?}: {e}"))?;
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
    let upstream_sha = String::from_utf8(upstream)
        .map_err(|e| format!("invalid UTF-8 in upstream Git ref: {e}"))?
        .trim()
        .to_owned();
    if upstream_sha.is_empty() {
        return Err(no_upstream());
    }
    let head = run_git(repo_path, ["rev-parse", "--verify", "HEAD"], None)?;
    let head_sha = String::from_utf8(head)
        .map_err(|e| format!("invalid UTF-8 in HEAD Git ref: {e}"))?
        .trim()
        .to_owned();
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
    fn git_source_rejects_invalid_committed_and_staged_utf8() {
        let Some(dir) = temp_repo("app.py", "value = 1\n") else { return };
        let path = dir.to_str().unwrap();
        // The index contains bytes that cannot be represented by a String.
        std::fs::write(dir.join("app.py"), b"value = \xff\n").unwrap();
        git_in(&dir, &["add", "app.py"]).expect("stage invalid UTF-8");

        let staged_error = changed_sources_json(path, "HEAD", ":staged").unwrap_err();
        assert!(staged_error.contains("invalid UTF-8"), "{staged_error}");
        let tree_error = changed_sources_json(path, "HEAD", "").unwrap_err();
        assert!(tree_error.contains("invalid UTF-8"), "{tree_error}");

        git_in(&dir, &["commit", "-m", "raw bytes"]).expect("commit invalid source");
        let sha = resolve_commit(path, "HEAD").expect("resolve raw commit");
        let committed_error = read_blob_at_rev(path, &sha, "app.py").unwrap_err();
        assert!(committed_error.contains("invalid UTF-8"), "{committed_error}");
        assert!(git_source_content_impl(path, "app.py", "HEAD", "HEAD").is_err());
        let commit_error = changed_commit_sources_json(path, "HEAD~1", "HEAD").unwrap_err();
        assert!(commit_error.contains("invalid UTF-8"), "{commit_error}");

        // Missing object remains distinct from invalid content and Git failure.
        assert!(read_blob_at_rev(path, &sha, "not-there.py").unwrap().is_none());
        assert!(read_blob_at_rev("/nonexistent", &sha, "app.py").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn git_source_preserves_valid_multibyte_utf8() {
        let Some(dir) = temp_repo("app.py", "value = 'é'\n") else { return };
        let path = dir.to_str().unwrap();
        let sha = resolve_commit(path, "HEAD").unwrap();
        let content = read_blob_at_rev(path, &sha, "app.py").unwrap().unwrap();
        assert_eq!(content, "value = 'é'\n");
        let source_json = git_source_content_impl(path, "app.py", "HEAD", "HEAD")
            .ok()
            .expect("valid UTF-8 is available");
        assert!(source_json.contains("é"), "{source_json}");
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
