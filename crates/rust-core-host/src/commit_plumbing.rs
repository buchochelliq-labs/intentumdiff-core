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
    let staged_paths: HashSet<String> = staged_output
        .split(|byte| *byte == 0)
        .filter(|token| !token.is_empty())
        .map(|token| String::from_utf8_lossy(token).to_string())
        .collect();
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
            old_contents.get(&old_path).cloned().unwrap_or_default()
        };
        let new_source = if code == "D" {
            String::new()
        } else {
            match safe_working_tree_path_rust(root, &new_path)? {
                Some(path) => {
                    let len = fs::metadata(&path)
                        .map_err(|exc| format!("metadata working-tree file: {exc}"))?
                        .len() as usize;
                    if len > max_source_bytes {
                        return Err(format!("working-tree file exceeds byte limit: {new_path}"));
                    }
                    fs::read_to_string(path).unwrap_or_default()
                }
                None => String::new(),
            }
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
    let tokens: Vec<&[u8]> = data
        .split(|byte| *byte == 0)
        .filter(|token| !token.is_empty())
        .collect();
    let mut entries = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let status = String::from_utf8_lossy(tokens[index]).to_string();
        index += 1;
        let code = status.chars().next().unwrap_or_default().to_string();
        if !matches!(code.as_str(), "A" | "D" | "M" | "R" | "C") {
            if index < tokens.len() {
                index += 1;
            }
            continue;
        }
        let (old_path, new_path) = if matches!(code.as_str(), "R" | "C") {
            if index + 1 >= tokens.len() {
                return Err("malformed git name-status rename entry".to_owned());
            }
            let old_path = String::from_utf8_lossy(tokens[index]).to_string();
            let new_path = String::from_utf8_lossy(tokens[index + 1]).to_string();
            index += 2;
            (old_path, new_path)
        } else {
            if index >= tokens.len() {
                return Err("malformed git name-status entry".to_owned());
            }
            let path = String::from_utf8_lossy(tokens[index]).to_string();
            index += 1;
            (path.clone(), path)
        };
        entries.push((code, old_path, new_path));
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
        if path.contains('\n') || path.contains('\r') {
            return Err("git cat-file path contains a newline".to_owned());
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
            return Err("malformed git cat-file batch output".to_owned());
        };
        let header_end = pos + relative_end;
        let header = &output[pos..header_end];
        pos = header_end + 1;
        if header.ends_with(b" missing") {
            result.insert(path.to_owned(), String::new());
            continue;
        }
        let header_text = String::from_utf8_lossy(header);
        let mut parts = header_text.split_whitespace();
        let _object = parts.next();
        let _kind = parts.next();
        let size = parts
            .next()
            .ok_or_else(|| "malformed git cat-file batch header".to_owned())?
            .parse::<usize>()
            .map_err(|_| "malformed git cat-file batch size".to_owned())?;
        if size > max_source_bytes {
            return Err(format!("git blob exceeds byte limit: {path}"));
        }
        if pos + size > output.len() {
            return Err("truncated git cat-file batch content".to_owned());
        }
        let content = &output[pos..pos + size];
        pos += size;
        if output.get(pos) == Some(&b'\n') {
            pos += 1;
        }
        result.insert(
            path.to_owned(),
            String::from_utf8_lossy(content).to_string(),
        );
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
