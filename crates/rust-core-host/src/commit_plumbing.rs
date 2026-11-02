// Extracted verbatim from lib.rs (issue #85, lib.rs split round 2).
use crate::*;

/// Reject a git ref that could be mis-read as a `git` option (argument
/// injection, e.g. `--output=<path>` turning `git diff <ref>` into an arbitrary
/// file write) or that could inject extra `cat-file --batch` request lines.
/// A ref starting with `-` is invalid per git's own ref-format rules, so this
/// never rejects a legitimate ref. The empty ref (working tree) is allowed.
pub(crate) fn validate_git_ref(old_ref: &str) -> Result<(), String> {
    if old_ref.starts_with('-') {
        return Err("git ref must not start with '-' (argument-injection guard)".to_owned());
    }
    if old_ref.contains('\n') || old_ref.contains('\r') || old_ref.contains('\0') {
        return Err("git ref must not contain newline or NUL".to_owned());
    }
    Ok(())
}

pub(crate) fn collect_working_tree_python_files_rust(
    repo_path: &str,
    old_ref: &str,
    max_source_bytes: usize,
) -> Result<Vec<Value>, String> {
    validate_git_ref(old_ref)?;
    let diff_output = run_git(repo_path, ["diff", "--name-status", "-z", old_ref], None)?;
    let entries = parse_name_status_z(&diff_output)?;
    if entries.is_empty() {
        return Ok(Vec::new());
    }
    for (_code, old_path, new_path) in &entries {
        if !(is_python_path(old_path) || is_python_path(new_path)) {
            return Err("certified commit JSON requires all changed files to be Python".to_owned());
        }
        validate_relative_git_path(old_path)?;
        validate_relative_git_path(new_path)?;
    }
    let staged_output = run_git(
        repo_path,
        ["diff", "--cached", "--name-only", "-z", old_ref],
        None,
    )?;
    if !staged_output.is_empty() && !staged_output.ends_with(&[0]) {
        return Err("incomplete staged Git inventory: missing NUL".to_owned());
    }
    let staged_paths: HashSet<String> = staged_output
        .split(|byte| *byte == 0)
        .filter(|token| !token.is_empty())
        .map(|token| {
            std::str::from_utf8(token)
                .map(str::to_owned)
                .map_err(|e| format!("invalid UTF-8 in staged Git path: {e}"))
        })
        .collect::<Result<_, _>>()?;
    let old_paths: Vec<&str> = entries
        .iter()
        .filter(|(code, _, _)| *code != "A")
        .map(|(_, old_path, _)| old_path.as_str())
        .collect();
    let old_contents =
        read_commit_paths_batch_rust(repo_path, old_ref, &old_paths, max_source_bytes)?;
    let root = Path::new(repo_path);
    let mut files = Vec::with_capacity(entries.len());
    for (code, old_path, new_path) in entries {
        let old_source = if code == "A" {
            String::new()
        } else {
            old_contents
                .get(&old_path)
                .cloned()
                .ok_or_else(|| format!("expected Git baseline {old_path:?} is missing"))?
        };
        let new_source = if code == "D" {
            String::new()
        } else {
            let path = safe_working_tree_path_rust(root, &new_path)?
                .ok_or_else(|| format!("changed working-tree path {new_path:?} disappeared"))?;
            let len = fs::metadata(&path)
                .map_err(|exc| format!("metadata working-tree file {new_path:?}: {exc}"))?
                .len() as usize;
            if len > max_source_bytes {
                return Err(format!("working-tree file exceeds byte limit: {new_path}"));
            }
            fs::read_to_string(&path)
                .map_err(|exc| format!("read UTF-8 working-tree source {new_path:?}: {exc}"))?
        };
        let staging_status = if staged_paths.contains(&old_path) || staged_paths.contains(&new_path)
        {
            "staged"
        } else {
            "unstaged"
        };
        files.push(json!({
            "old_source": old_source,
            "new_source": new_source,
            "old_filename": old_path,
            "new_filename": new_path,
            "language": "python",
            "parser_plugin_id": "python",
            "parser_wasm_path": "",
            "staging_status": staging_status,
        }));
    }
    Ok(files)
}

pub(crate) fn run_git<const N: usize>(
    repo_path: &str,
    args: [&str; N],
    input: Option<&[u8]>,
) -> Result<Vec<u8>, String> {
    let mut command = Command::new("git");
    command.arg("-C").arg(repo_path).args(args);
    if input.is_some() {
        command.stdin(Stdio::piped());
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|exc| format!("spawn git: {exc}"))?;
    if let Some(input) = input {
        child
            .stdin
            .as_mut()
            .ok_or_else(|| "git stdin unavailable".to_owned())?
            .write_all(input)
            .map_err(|exc| format!("write git stdin: {exc}"))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|exc| format!("wait for git: {exc}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git failed: {stderr}"));
    }
    Ok(output.stdout)
}

pub(crate) fn parse_name_status_z(data: &[u8]) -> Result<Vec<(String, String, String)>, String> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    if !data.ends_with(&[0]) {
        return Err("incomplete git name-status -z inventory: missing NUL".to_owned());
    }
    // Do not filter empty tokens: a missing filename/status is malformed input,
    // and collapsing tokens can change which source file receives a status.
    let tokens: Vec<&[u8]> = data[..data.len() - 1]
        .split(|byte| *byte == 0)
        .collect();
    let mut entries = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let status = std::str::from_utf8(tokens[index])
            .map_err(|e| format!("invalid UTF-8 in Git status: {e}"))?;
        index += 1;
        let code = status.chars().next().unwrap_or_default();
        let valid = matches!(status, "A" | "D" | "M")
            || (matches!(code, 'R' | 'C')
                && status.len() > 1
                && status[1..].bytes().all(|byte| byte.is_ascii_digit()));
        if !valid {
            return Err(format!("unsupported or malformed Git change status {status:?}"));
        }
        let count = if matches!(code, 'R' | 'C') { 2 } else { 1 };
        if index + count > tokens.len() {
            return Err(format!("truncated Git name-status record {status:?}"));
        }
        let old_path = std::str::from_utf8(tokens[index])
            .map_err(|e| format!("invalid UTF-8 in Git changed path: {e}"))?;
        if old_path.is_empty() {
            return Err("empty Git changed path".to_owned());
        }
        index += 1;
        let new_path = if count == 2 {
            let path = std::str::from_utf8(tokens[index])
                .map_err(|e| format!("invalid UTF-8 in Git renamed path: {e}"))?;
            if path.is_empty() {
                return Err("empty Git renamed path".to_owned());
            }
            index += 1;
            path
        } else {
            old_path
        };
        entries.push((code.to_string(), old_path.to_owned(), new_path.to_owned()));
    }
    Ok(entries)
}

pub(crate) fn read_commit_paths_batch_rust(
    repo_path: &str,
    old_ref: &str,
    paths: &[&str],
    max_source_bytes: usize,
) -> Result<HashMap<String, String>, String> {
    if paths.is_empty() {
        return Ok(HashMap::new());
    }
    // The ref is embedded as `{old_ref}:{path}` in the batch stdin; a newline in
    // the ref would inject extra request lines (paths are already checked below).
    validate_git_ref(old_ref)?;
    let mut request = Vec::new();
    let mut unique_paths = Vec::new();
    let mut seen = HashSet::new();
    for path in paths {
        if path.contains('\n') || path.contains('\r') || path.contains('\0') {
            return Err("git cat-file path contains a newline or NUL".to_owned());
        }
        if seen.insert(*path) {
            unique_paths.push(*path);
            request.extend_from_slice(format!("{old_ref}:{path}\n").as_bytes());
        }
    }
    let output = run_git(repo_path, ["cat-file", "--batch"], Some(&request))?;
    let mut result = HashMap::new();
    let mut pos = 0;
    for path in unique_paths {
        let Some(relative_end) = output[pos..].iter().position(|byte| *byte == b'\n') else {
            return Err(format!("incomplete Git cat-file header for {path:?}"));
        };
        let header_end = pos + relative_end;
        let header = std::str::from_utf8(&output[pos..header_end])
            .map_err(|e| format!("invalid Git batch header for {path:?}: {e}"))?;
        pos = header_end + 1;

        if header == format!("{old_ref}:{path} missing") {
            return Err(format!("expected Git baseline {path:?} is missing at {old_ref}"));
        }

        let mut fields = header.split_ascii_whitespace();
        let object_id = fields
            .next()
            .ok_or_else(|| format!("missing Git object ID for {path:?}"))?;
        let kind = fields
            .next()
            .ok_or_else(|| format!("missing Git object type for {path:?}"))?;
        let size = fields
            .next()
            .ok_or_else(|| format!("missing Git object size for {path:?}"))?
            .parse::<usize>()
            .map_err(|e| format!("invalid Git blob size for {path:?}: {e}"))?;
        if fields.next().is_some()
            || kind != "blob"
            || !matches!(object_id.len(), 40 | 64)
            || !object_id.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(format!("unexpected Git batch header for {path:?}: {header:?}"));
        }
        if size > max_source_bytes {
            return Err(format!("git blob exceeds byte limit: {path}"));
        }
        let end = pos
            .checked_add(size)
            .ok_or_else(|| format!("Git blob size overflow for {path:?}"))?;
        if output.get(end) != Some(&b'\n') {
            return Err(format!("incomplete Git blob record for {path:?}"));
        }
        let text = std::str::from_utf8(&output[pos..end])
            .map_err(|e| format!("invalid UTF-8 in committed Git source {path:?}: {e}"))?;
        pos = end + 1;
        result.insert(path.to_owned(), text.to_owned());
    }
    if pos != output.len() {
        return Err("extra unrequested Git cat-file batch records".to_owned());
    }
    Ok(result)
}

pub(crate) fn is_python_path(path: &str) -> bool {
    path.to_ascii_lowercase().ends_with(".py") || path.to_ascii_lowercase().ends_with(".pyi")
}

pub(crate) fn validate_relative_git_path(path: &str) -> Result<(), String> {
    let path = Path::new(path);
    if path.is_absolute()
        || path.components().any(|part| {
            matches!(
                part,
                PathComponent::ParentDir | PathComponent::RootDir | PathComponent::Prefix(_)
            )
        })
    {
        return Err("unsafe git path".to_owned());
    }
    Ok(())
}

pub(crate) fn safe_working_tree_path_rust(root: &Path, rel_path: &str) -> Result<Option<PathBuf>, String> {
    validate_relative_git_path(rel_path)?;
    let root = root
        .canonicalize()
        .map_err(|exc| format!("canonicalize repo root: {exc}"))?;
    let candidate = root.join(rel_path);
    if !candidate.exists() {
        return Ok(None);
    }
    let resolved = candidate
        .canonicalize()
        .map_err(|exc| format!("canonicalize working-tree path: {exc}"))?;
    if !resolved.starts_with(&root) {
        return Err("unsafe git path resolves outside repository".to_owned());
    }
    Ok(Some(resolved))
}

pub(crate) fn commit_json_from_request_files_direct(
    request: &Value,
    probe: &mut PhaseProbe,
    config: &RustCoreConfig,
    python_parser_backend: &str,
) -> Result<(Value, Option<Vec<u8>>), String> {
    let files = request
        .get("files")
        .and_then(Value::as_array)
        .ok_or_else(|| "commit JSON request requires a files array".to_owned())?;
    let config_json = request
        .get("config")
        .map(Value::to_string)
        .unwrap_or_else(|| "{}".to_owned());
    let parallel = request
        .get("parallel")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let max_workers = request
        .get("max_workers")
        .or_else(|| request.get("maxWorkers"))
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0);
    let preloads = BatchComponentPreloads::empty();
    let custom_pool = if parallel && files.len() > 1 {
        if let Some(workers) = max_workers {
            Some(
                ThreadPoolBuilder::new()
                    .num_threads(workers)
                    .build()
                    .map_err(|exc| format!("build Rust direct commit worker pool: {exc}"))?,
            )
        } else {
            None
        }
    } else {
        None
    };
    let outcomes: Vec<BatchItemResult> =
        probe.measure("rust_commit_json_file_execution", || {
            if parallel && files.len() > 1 {
                if let Some(pool) = custom_pool.as_ref() {
                    Ok(pool.install(|| {
                        files
                            .par_iter()
                            .enumerate()
                            .map(|(index, file)| {
                                timed_diff_batch_file_item(
                                    index,
                                    file,
                                    &config_json,
                                    false,
                                    &preloads,
                                    python_parser_backend,
                                    config.profile_phases,
                                    config.fallback_to_token_diff,
                                )
                            })
                            .collect()
                    }))
                } else {
                    Ok(files
                        .par_iter()
                        .enumerate()
                        .map(|(index, file)| {
                            timed_diff_batch_file_item(
                                index,
                                file,
                                &config_json,
                                false,
                                &preloads,
                                python_parser_backend,
                                config.profile_phases,
                                config.fallback_to_token_diff,
                            )
                        })
                        .collect())
                }
            } else {
                Ok(files
                    .iter()
                    .enumerate()
                    .map(|(index, file)| {
                        timed_diff_batch_file_item(
                            index,
                            file,
                            &config_json,
                            false,
                            &preloads,
                            python_parser_backend,
                            config.profile_phases,
                            config.fallback_to_token_diff,
                        )
                    })
                    .collect())
            }
        })?;
    let mut items = Vec::with_capacity(outcomes.len());
    for outcome in outcomes {
        if outcome.item.get("status").and_then(Value::as_str) != Some(COMPLETE) {
            let reason = outcome
                .item
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("Rust direct commit item did not complete");
            return Ok((
                commit_json_control(FALLBACK, reason, &probe, None, None, 0, 0),
                None,
            ));
        }
        items.push(outcome.item);
    }
    items.sort_by_key(batch_diff_item_sort_key);
    commit_json_from_owned_items(request, files, items, probe, config)
}

pub(crate) fn commit_json_from_batch(
    request: &Value,
    batch: Value,
    probe: &mut PhaseProbe,
    config: &RustCoreConfig,
) -> Result<(Value, Option<Vec<u8>>), String> {
    if batch.get("status").and_then(Value::as_str) != Some(COMPLETE) {
        return Ok((
            commit_json_control(
                FALLBACK,
                batch
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or("Rust batch did not complete every file"),
                &probe,
                Some(&batch),
                None,
                0,
                0,
            ),
            None,
        ));
    }
    let metadata = batch.get("metadata").and_then(Value::as_object);
    if metadata
        .and_then(|item| item.get("certification"))
        .and_then(Value::as_str)
        != Some(PYTHON_NATIVE_V4KB_CERTIFICATION)
    {
        return Ok((
            commit_json_control(
                FALLBACK,
                "Rust batch is not certified for native Python commit JSON",
                &probe,
                Some(&batch),
                None,
                0,
                0,
            ),
            None,
        ));
    }

    let files = request
        .get("files")
        .and_then(Value::as_array)
        .ok_or_else(|| "commit JSON request requires a files array".to_owned())?;
    let items = batch
        .get("diffs")
        .and_then(Value::as_array)
        .ok_or_else(|| "batch response did not include diff items".to_owned())?;

    commit_json_from_items(request, files, items, Some(&batch), probe, config)
}

pub(crate) fn commit_json_from_items(
    request: &Value,
    files: &[Value],
    items: &[Value],
    batch: Option<&Value>,
    probe: &mut PhaseProbe,
    config: &RustCoreConfig,
) -> Result<(Value, Option<Vec<u8>>), String> {
    let old_ref = request
        .get("old_ref")
        .or_else(|| request.get("oldRef"))
        .and_then(Value::as_str)
        .unwrap_or("HEAD");
    let new_ref = request
        .get("new_ref")
        .or_else(|| request.get("newRef"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let mut file_diffs = Vec::with_capacity(items.len());
    let mut signature_hasher = config.profile_phases.then(Sha256::new);
    probe.measure("rust_commit_json_output_validation", || {
        for item in items {
            if item.get("status").and_then(Value::as_str) != Some(COMPLETE) {
                return Err("commit JSON requires every batch item to be complete".to_owned());
            }
            let index = item
                .get("index")
                .and_then(Value::as_u64)
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or(file_diffs.len());
            let diff = item
                .get("diff")
                .ok_or_else(|| "complete batch item did not include diff".to_owned())?;
            if config.profile_phases {
                validate_certified_semantic_diff(&diff)?;
            } else {
                validate_certified_semantic_diff_envelope(diff)?;
            }
            if let Some(hasher) = signature_hasher.as_mut() {
                update_semantic_signature_hash_from_diff(hasher, diff);
            }
            let mut diff = diff.clone();
            if let Some(staging_status) = files
                .get(index)
                .and_then(|file| {
                    file.get("staging_status")
                        .or_else(|| file.get("stagingStatus"))
                        .and_then(Value::as_str)
                })
                .filter(|value| !value.is_empty())
            {
                diff["staging_status"] = json!(staging_status);
            }
            if let Some(file) = files.get(index) {
                apply_file_lifecycle_to_diff(
                    &mut diff,
                    infer_file_lifecycle_from_file(file),
                );
            }
            file_diffs.push(diff);
        }
        Ok(())
    })?;

    let signature_hash = probe.measure_value("rust_commit_json_response_assembly", || {
        signature_hasher.map(finalize_hex_hash).unwrap_or_default()
    });
    let commit_diff = CertifiedCommitDiffPayload {
        old_ref,
        new_ref,
        guardrail_violations: Vec::new(),
        file_diffs,
        cross_file_changes: Vec::new(),
        parse_errors: Vec::new(),
    };
    let commit_json = probe.measure("rust_commit_json_serialize", || {
        serde_json::to_vec(&commit_diff)
            .map_err(|exc| format!("serialize certified CommitDiff JSON: {exc}"))
    })?;
    let byte_size = commit_json.len();
    let control = commit_json_control(
        COMPLETE,
        "",
        &probe,
        batch,
        (!signature_hash.is_empty()).then_some(signature_hash.as_str()),
        byte_size,
        items.len(),
    );
    Ok((control, Some(commit_json)))
}

pub(crate) fn commit_json_from_owned_items(
    request: &Value,
    files: &[Value],
    items: Vec<Value>,
    probe: &mut PhaseProbe,
    config: &RustCoreConfig,
) -> Result<(Value, Option<Vec<u8>>), String> {
    let old_ref = request
        .get("old_ref")
        .or_else(|| request.get("oldRef"))
        .and_then(Value::as_str)
        .unwrap_or("HEAD");
    let new_ref = request
        .get("new_ref")
        .or_else(|| request.get("newRef"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let mut file_diffs = Vec::with_capacity(items.len());
    let mut signature_hasher = config.profile_phases.then(Sha256::new);
    probe.measure("rust_commit_json_output_validation", || {
        for mut item in items {
            if item.get("status").and_then(Value::as_str) != Some(COMPLETE) {
                return Err("commit JSON requires every batch item to be complete".to_owned());
            }
            let index = item
                .get("index")
                .and_then(Value::as_u64)
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or(file_diffs.len());
            let diff = item
                .as_object_mut()
                .and_then(|object| object.remove("diff"))
                .ok_or_else(|| "complete batch item did not include diff".to_owned())?;
            if config.profile_phases {
                validate_certified_semantic_diff(&diff)?;
            } else {
                validate_certified_semantic_diff_envelope(&diff)?;
            }
            if let Some(hasher) = signature_hasher.as_mut() {
                update_semantic_signature_hash_from_diff(hasher, &diff);
            }
            let mut diff = diff;
            if let Some(staging_status) = files
                .get(index)
                .and_then(|file| {
                    file.get("staging_status")
                        .or_else(|| file.get("stagingStatus"))
                        .and_then(Value::as_str)
                })
                .filter(|value| !value.is_empty())
            {
                diff["staging_status"] = json!(staging_status);
            }
            if let Some(file) = files.get(index) {
                apply_file_lifecycle_to_diff(
                    &mut diff,
                    infer_file_lifecycle_from_file(file),
                );
            }
            file_diffs.push(diff);
        }
        Ok(())
    })?;

    let signature_hash = probe.measure_value("rust_commit_json_response_assembly", || {
        signature_hasher.map(finalize_hex_hash).unwrap_or_default()
    });
    let commit_diff = CertifiedCommitDiffPayload {
        old_ref,
        new_ref,
        guardrail_violations: Vec::new(),
        file_diffs,
        cross_file_changes: Vec::new(),
        parse_errors: Vec::new(),
    };
    let commit_json = probe.measure("rust_commit_json_serialize", || {
        serde_json::to_vec(&commit_diff)
            .map_err(|exc| format!("serialize certified CommitDiff JSON: {exc}"))
    })?;
    let byte_size = commit_json.len();
    let control = commit_json_control(
        COMPLETE,
        "",
        &probe,
        None,
        (!signature_hash.is_empty()).then_some(signature_hash.as_str()),
        byte_size,
        commit_diff.file_diffs.len(),
    );
    Ok((control, Some(commit_json)))
}

pub(crate) fn commit_json_control(
    status: &str,
    reason: &str,
    probe: &PhaseProbe,
    batch: Option<&Value>,
    signature_hash: Option<&str>,
    byte_size: usize,
    file_count: usize,
) -> Value {
    let batch_metadata = batch
        .and_then(|payload| payload.get("metadata"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    json!({
        "schema_version": 1,
        "status": status,
        "engine": BATCH_ENGINE,
        "reason": reason,
        "certification": if status == COMPLETE {
            PYTHON_NATIVE_V4KB_CERTIFICATION
        } else {
            ""
        },
        "trust_tier": if status == COMPLETE {
            "first_party_core_builder"
        } else {
            ""
        },
        "python_parser_backend": PYTHON_PARSER_BACKEND_NATIVE,
        "commit_diff_byte_size": byte_size,
        "file_count": file_count,
        "semantic_signature_hash": signature_hash.unwrap_or(""),
        "phase_timings": probe.phases.clone(),
        "batch_metadata": batch_metadata,
    })
}


#[cfg(test)]
mod strict_git_inventory_tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn rejects_truncated_or_lossy_git_status_records() {
        assert!(parse_name_status_z(b"").unwrap().is_empty());
        assert_eq!(
            parse_name_status_z(b"M\0a.py\0").unwrap(),
            vec![("M".to_owned(), "a.py".to_owned(), "a.py".to_owned())]
        );
        assert!(parse_name_status_z(b"M\0a.py").is_err());
        assert!(parse_name_status_z(b"M\0\0").is_err());
        assert!(parse_name_status_z(b"M\0bad_\xff.py\0").is_err());
        assert!(parse_name_status_z(b"T\0a.py\0").is_err());
        assert_eq!(
            parse_name_status_z("R100\0old.py\0café.py\0".as_bytes()).unwrap(),
            vec![("R".to_owned(), "old.py".to_owned(), "café.py".to_owned())]
        );
    }

    fn git(repo: &Path, args: &[&str]) {
        let output = Command::new("git").arg("-C").arg(repo).args(args)
            .output().expect("Git must be installed for source-integrity regressions");
        assert!(output.status.success(), "git {args:?}: {:?}",
                String::from_utf8_lossy(&output.stderr));
    }

    #[test]
    fn certified_commit_collection_never_invents_empty_sources() {
        let repo = tempfile::tempdir().expect("temp Git repo");
        let root = repo.path();
        git(root, &["init", "--quiet"]);
        git(root, &["config", "user.name", "Integrity Test"]);
        git(root, &["config", "user.email", "integrity@example.invalid"]);
        fs::write(root.join("app.py"), "msg = 'é'\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "valid baseline"]);
        let path = root.to_str().unwrap();
        // Readable multibyte content is preserved exactly.
        fs::write(root.join("app.py"), "msg = 'été'\n").unwrap();
        let files = collect_working_tree_python_files_rust(path, "HEAD", 1024).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0]["old_source"], "msg = 'é'\n");
        assert_eq!(files[0]["new_source"], "msg = 'été'\n");

        // Invalid working-tree bytes must not be replaced or made empty.
        fs::write(root.join("app.py"), b"msg = \xff\n").unwrap();
        let err = collect_working_tree_python_files_rust(path, "HEAD", 1024).unwrap_err();
        assert!(err.contains("read UTF-8 working-tree source"), "{err}");

        // Invalid committed baseline bytes must not become certified source.
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "invalid committed source"]);
        fs::write(root.join("app.py"), "msg = 'recovered'\n").unwrap();
        let err = collect_working_tree_python_files_rust(path, "HEAD", 1024).unwrap_err();
        assert!(err.contains("invalid UTF-8 in committed Git source"), "{err}");
    }
}
