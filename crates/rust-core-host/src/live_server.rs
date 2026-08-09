//! Live-server protocol layer (#100, A2.5 slice 1) — the wire contract for the keystroke-level
//! diff server: request validation, the repo-relative path security guard, and response shaping.
//! Moving it into the core defines the protocol ONCE so every binding's server shares it (the
//! north star). Transport (unix sockets / Windows named pipes + #88 DACLs), debounce, dispatch,
//! and the differ/review orchestration remain in the Python shell for now — later A2.5 slices
//! move those, ending in a native server the extension spawns.

use serde_json::{json, Value};

// Wire limits — the source of truth (Python `LiveServer` reads these back via `live_limits`).
pub const MAX_LINE_BYTES: u64 = 1_048_576; // 1 MiB
pub const MAX_CONTENT_BYTES: u64 = 512 * 1024; // 512 KiB
pub const MAX_DELTAS: u64 = 1_000;
pub const MAX_CLIENTS: u64 = 4;

fn error_obj(seq: i64, code: &str, message: &str, op: &str) -> Value {
    json!({
        "op": op,
        "seq": seq,
        "ok": false,
        "error": { "code": code, "message": message },
    })
}

/// python `LiveServer._error_response` (pure Rust — the binding-independent protocol API the
/// native live-server binary and the C ABI's `live_*` handlers call).
pub fn live_error_response_impl(seq: i64, code: &str, message: &str, op: &str) -> String {
    error_obj(seq, code, message, op).to_string()
}

/// python `LiveServer._limits` (pure Rust).
pub fn live_limits_impl() -> String {
    json!({
        "max_line_bytes": MAX_LINE_BYTES,
        "max_content_bytes": MAX_CONTENT_BYTES,
        "max_deltas": MAX_DELTAS,
        "max_clients": MAX_CLIENTS,
    })
    .to_string()
}

/// python `LiveServer._capabilities` (pure Rust).
pub fn live_capabilities_impl() -> String {
    json!({
        "operations": ["hello", "diff", "review", "cancel", "asset_diff"],
        "legacy_diff": true,
        "stream": true,
        "per_request_ref": true,
        "edit_deltas": true,
        "review": true,
        "review_streaming": true,
        "cross_file_changes": true,
    })
    .to_string()
}

/// The repo-relative path security guard (mirrors python `_normalise_request_path`): reject
/// absolute paths, `..` traversal, and any drive/URI-scheme (`:`), returning the backslash-
/// normalised path. Returns `Ok(normalised)` or `Err((code, message))`.
fn normalise_path(path: &str) -> Result<String, (&'static str, &'static str)> {
    let normalised = path.replace('\\', "/");
    let parts: Vec<&str> = normalised.split('/').collect();
    if normalised.starts_with('/') || parts.iter().any(|p| *p == "..") {
        return Err((
            "invalid_path",
            "path must be repository-relative and must not contain '..'",
        ));
    }
    if parts.iter().any(|p| p.contains(':')) {
        return Err(("invalid_path", "path must not contain a drive or URI scheme"));
    }
    Ok(normalised)
}

/// python `LiveServer._normalise_request_path` (seq-less, pure Rust): the caller wraps an error
/// with seq.
pub fn live_normalise_request_path_impl(path: &str) -> String {
    match normalise_path(path) {
        Ok(p) => json!({ "ok": true, "path": p }).to_string(),
        Err((code, message)) => json!({ "ok": false, "code": code, "message": message }).to_string(),
    }
}

/// python `LiveServer._request_seq` — `int(request.get("seq", 0))` with the try/except guard.
/// Absent seq -> 0; a JSON number (float truncates toward zero) or numeric string parses;
/// null / list / object -> `invalid_seq`.
fn parse_seq(req: &Value) -> Result<i64, ()> {
    match req.get("seq") {
        None => Ok(0),
        Some(Value::Number(n)) => n
            .as_i64()
            .or_else(|| n.as_f64().map(|f| f.trunc() as i64))
            .ok_or(()),
        Some(Value::String(s)) => s.trim().parse::<i64>().map_err(|_| ()),
        Some(Value::Bool(b)) => Ok(i64::from(*b)), // Python: int(True)==1, int(False)==0
        _ => Err(()), // null / array / object -> TypeError in Python
    }
}

/// python `LiveServer._request_seq` -> `{"seq": N}` or `{"error": <response>}` (pure Rust).
pub fn live_request_seq_impl(request_json: &str) -> Result<String, String> {
    let req: Value = serde_json::from_str(request_json)
        .map_err(|e| format!("invalid request json: {e}"))?;
    Ok(match parse_seq(&req) {
        Ok(seq) => json!({ "seq": seq }).to_string(),
        Err(()) => json!({ "error": error_obj(0, "invalid_seq", "invalid seq field", "error") })
            .to_string(),
    })
}

/// python `LiveServer._parse_diff_request` — full diff-request validation (pure Rust).
/// Returns `{"parsed": {op, seq, path, content, ref, stream, deltas}}` (deltas raw for the
/// caller to build `EditDelta` from) or `{"error": <response>}`.
pub fn live_parse_diff_request_impl(request_json: &str, default_ref: &str) -> Result<String, String> {
    let req: Value = serde_json::from_str(request_json)
        .map_err(|e| format!("invalid request json: {e}"))?;

    let seq = match parse_seq(&req) {
        Ok(s) => s,
        Err(()) => {
            return Ok(json!({
                "error": error_obj(0, "invalid_seq", "invalid seq field", "error")
            })
            .to_string());
        }
    };
    let err = |code: &str, message: &str| -> Result<String, String> {
        Ok(json!({ "error": error_obj(seq, code, message, "diff") }).to_string())
    };

    // path (required, non-empty string) + security guard
    let path = match req.get("path").and_then(Value::as_str) {
        Some(p) if !p.is_empty() => match normalise_path(p) {
            Ok(p) => p,
            Err((code, message)) => return err(code, message),
        },
        _ => return err("invalid_path", "path must be a string"),
    };

    // content (optional, default ""; must be a string; UTF-8 byte cap)
    let content = match req.get("content") {
        None => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(_) => return err("invalid_content", "content must be a string"),
    };
    if content.len() as u64 > MAX_CONTENT_BYTES {
        return err(
            "content_too_large",
            &format!("content exceeds {MAX_CONTENT_BYTES} byte limit"),
        );
    }

    // ref (optional; null/empty -> default_ref; must be a string)
    let ref_val = match req.get("ref") {
        None | Some(Value::Null) => default_ref.to_string(),
        Some(Value::String(s)) if s.is_empty() => default_ref.to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(_) => return err("invalid_ref", "ref must be a string"),
    };

    // stream (optional bool)
    let stream = match req.get("stream") {
        None | Some(Value::Null) => Value::Null,
        Some(Value::Bool(b)) => Value::Bool(*b),
        Some(_) => return err("invalid_stream", "stream must be a boolean"),
    };

    // deltas (optional list; count cap; per-entry validation stays in the Python shell)
    let deltas = match req.get("deltas") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(a)) => a.clone(),
        Some(_) => return err("invalid_deltas", "deltas must be a list"),
    };
    if deltas.len() as u64 > MAX_DELTAS {
        return err(
            "too_many_deltas",
            &format!("deltas count exceeds {MAX_DELTAS} limit"),
        );
    }

    Ok(json!({
        "parsed": {
            "op": "diff",
            "seq": seq,
            "path": path,
            "content": content,
            "ref": ref_val,
            "stream": stream,
            "deltas": deltas,
        }
    })
    .to_string())
}

/// python truthiness of an optional JSON value (`bool(request.get(key, False))`).
fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Number(n)) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// python `LiveServer._process_review_request` input validation — old_ref/new_ref/stream
/// (pure Rust). *seq* is already validated by the caller (`_request_seq`). Returns
/// `{"parsed": {old_ref, new_ref, stream}}` or `{"error": <response>}` (op="review").
pub fn live_parse_review_request_impl(
    request_json: &str,
    default_ref: &str,
    seq: i64,
) -> Result<String, String> {
    let req: Value = serde_json::from_str(request_json)
        .map_err(|e| format!("invalid request json: {e}"))?;
    let err = |code: &str, message: &str| -> Result<String, String> {
        Ok(json!({ "error": error_obj(seq, code, message, "review") }).to_string())
    };

    // old_ref: absent / null / "" -> default_ref; non-string -> error.
    let old_ref = match req.get("old_ref") {
        None | Some(Value::Null) => default_ref.to_string(),
        Some(Value::String(s)) if s.is_empty() => default_ref.to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(_) => return err("invalid_ref", "old_ref must be a string"),
    };
    // new_ref: absent / null -> ""; non-string -> error (empty string is kept).
    let new_ref = match req.get("new_ref") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(_) => return err("invalid_ref", "new_ref must be a string"),
    };
    let stream = truthy(req.get("stream"));

    Ok(json!({
        "parsed": { "old_ref": old_ref, "new_ref": new_ref, "stream": stream }
    })
    .to_string())
}

/// Mirror `guardrails._find_policy_path` / `_is_policy_file` for the live path: a guardrail policy
/// is in effect when the target file IS `intentumdiff.yaml`, a `guardrail_policy_path` is configured,
/// or an `intentumdiff.yaml` is discovered walking from the repo root upward. The native non-Python
/// diff chain doesn't attach guardrail violations, so the caller defers to the full differ when
/// this returns true (the common no-policy case is served natively).
/// Load the repo's `intentumdiff.yaml` `config:` section (the #99 loader) — the SAME config the
/// Python live-server builds its DiffConfig from (`_load_cli_config(config_start_path=repo)`).
/// Returns the validated mapping as JSON ("{}" when no file), Err on a malformed file.
pub fn live_load_project_config_impl(repo_path: &str) -> Result<String, String> {
    crate::config::load_config_section(Some(repo_path), None)
}

fn is_guardrail_policy_filename(p: &str) -> bool {
    std::path::Path::new(p)
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.eq_ignore_ascii_case("intentumdiff.yaml"))
        .unwrap_or(false)
}

/// The loaded guardrail-policy state for a request (#100 native guardrails):
/// - `NoRules`: no policy anywhere, guardrails disabled, or `guardrails.protected` empty/absent —
///   nothing to evaluate; serving is fully differ-faithful.
/// - `Rules`: strictly-parsed protected rules (python `_parse_rule` mirror) the native chain
///   evaluates via the A1.3 engine.
/// - `Defer`: unreadable/unparseable policy or an off-spec rule — python RAISES for these, so the
///   differ's behaviour must win; fall back.
pub(crate) enum GuardrailPolicyState {
    NoRules,
    Rules(Vec<Value>),
    Defer(String),
}

/// The languages protected rules may target (python `GUARDRAIL_CONFIG_LANGUAGES` =
/// keyed-data | resource-profile) — all served by the native wasm chain, never by the batch.
const GUARDRAIL_RULE_LANGUAGES: [&str; 11] = [
    "adf",
    "databricks",
    "databricks-workflow",
    "dbt-config",
    "dbt-packages",
    "dbt-yaml",
    "dockerfile",
    "hcl",
    "json",
    "puppet",
    "yaml",
];

fn load_guardrail_policy_state(repo_path: &str, config: &Value) -> GuardrailPolicyState {
    // python apply_guardrails_to_diff: `if not config.guardrails_enabled: return diff`.
    if config.get("guardrails_enabled").and_then(Value::as_bool) == Some(false) {
        return GuardrailPolicyState::NoRules;
    }
    let explicit = config
        .get("guardrail_policy_path")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(std::path::PathBuf::from);
    let discovered = explicit.or_else(|| {
        let mut dir = std::path::Path::new(repo_path).to_path_buf();
        loop {
            let candidate = dir.join("intentumdiff.yaml");
            if candidate.exists() {
                return Some(candidate);
            }
            match dir.parent() {
                Some(parent) => dir = parent.to_path_buf(),
                None => return None,
            }
        }
    });
    let Some(policy_path) = discovered else {
        return GuardrailPolicyState::NoRules;
    };
    let raw = match std::fs::read_to_string(&policy_path) {
        Ok(raw) => raw,
        Err(e) => return GuardrailPolicyState::Defer(format!("guardrail policy unreadable: {e}")),
    };
    let doc: Value = match serde_yaml::from_str(&raw) {
        Ok(doc) => doc,
        Err(e) => {
            return GuardrailPolicyState::Defer(format!("guardrail policy unparseable: {e}"))
        }
    };
    let Some(raw_rules) = doc
        .get("guardrails")
        .and_then(|g| g.get("protected"))
        .and_then(Value::as_array)
    else {
        return GuardrailPolicyState::NoRules;
    };
    if raw_rules.is_empty() {
        return GuardrailPolicyState::NoRules;
    }
    // Strict python `_parse_rule` mirror — python RAISES on any off-spec rule, so anything
    // unexpected defers rather than silently dropping a rule.
    let mut rules: Vec<Value> = Vec::with_capacity(raw_rules.len());
    for (index, raw_rule) in raw_rules.iter().enumerate() {
        let Some(map) = raw_rule.as_object() else {
            return GuardrailPolicyState::Defer(format!(
                "guardrails.protected[{index}] must be a mapping"
            ));
        };
        let language = map
            .get("language")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_lowercase();
        if !GUARDRAIL_RULE_LANGUAGES.contains(&language.as_str()) {
            return GuardrailPolicyState::Defer(format!(
                "guardrails.protected[{index}] targets unsupported language '{language}'"
            ));
        }
        let path =
            crate::guardrail_normalise_path(map.get("path").and_then(Value::as_str).unwrap_or(""));
        if path.is_empty() {
            return GuardrailPolicyState::Defer(format!(
                "guardrails.protected[{index}] requires path"
            ));
        }
        let severity = map
            .get("severity")
            .and_then(Value::as_str)
            .unwrap_or("important")
            .to_lowercase();
        if severity != "important" && severity != "immutable" {
            return GuardrailPolicyState::Defer(format!(
                "guardrails.protected[{index}] has unsupported severity '{severity}'"
            ));
        }
        let files: Vec<String> = match map.get("files") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::String(s)) => vec![s.clone()],
            Some(Value::Array(items)) => items
                .iter()
                .map(|item| match item {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect(),
            Some(_) => {
                return GuardrailPolicyState::Defer(format!(
                    "guardrails.protected[{index}].files is invalid"
                ));
            }
        };
        let rule_id = map
            .get("id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("guardrail.{}", index + 1));
        let message = map
            .get("message")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("Protected path {path} changed"));
        rules.push(json!({
            "rule_id": rule_id,
            "severity": severity,
            "language": language,
            "path": path,
            "message": message,
            "files": files,
        }));
    }
    GuardrailPolicyState::Rules(rules)
}

/// python `apply_guardrails_to_diff`'s policy-file check: a change to intentumdiff.yaml itself is
/// an IMMUTABLE violation regardless of rules.
fn policy_file_violation(diff: &Value, old_filename: &str, new_filename: &str) -> Value {
    let file = if !new_filename.is_empty() {
        new_filename
    } else {
        old_filename
    };
    json!({
        "rule_id": "intentumdiff.policy_file",
        "severity": "immutable",
        "file": file,
        "language": diff.get("language").cloned().unwrap_or_else(|| json!("")),
        "semantic_path": "intentumdiff.yaml",
        "node_type": "",
        "old_node_id": Value::Null,
        "new_node_id": Value::Null,
        "position": Value::Null,
        "old_value": "project policy",
        "new_value": "project policy",
        "message": "Project guardrail policy changed",
    })
}

/// python `LiveServer._process_request_with_source` (non-streaming) — the native single-file
/// diff handler (#100, A2.5 Phase A1). Reads the "old" side (blob@ref), takes the live *content*
/// as the "new" side, and runs the certified native batch (`diff_batch_impl`) for Python.
///
/// Two native paths, resolved via the bundled parser manifest: Python (`.py`/`.pyi`, empty
/// wasm_path) runs the certified native batch (`diff_batch_impl`); every other resolved language
/// runs the native non-Python chain (`native_wasm_single_diff`: wasm-parse -> finalize ->
/// invariances -> assemble, mirroring the differ's routed `_diff_single`). Either path returns
/// `{"fallback": <reason>}` for an uncovered case (unresolved extension, uncertified batch item,
/// off-spec guardrail policy, finalize declined) so the Python differ path runs
/// unchanged. *config_json* is `self._differ._config` marshalled by the caller. Returns
/// `{"diff": <SemanticDiff>}` on a served result, else `{"fallback": <reason>}`.
pub fn live_handle_diff_impl(
    repo_path: &str,
    path: &str,
    git_ref: &str,
    content: &str,
    config_json: &str,
    wasm_dir: &str,
) -> Result<String, String> {
    // old side = blob@ref ("" when absent, matching LiveBufferSource); new side = live buffer.
    let old_source = crate::vcs_backend::read_blob("git", repo_path, path, git_ref)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    diff_resolved_sources(repo_path, path, &old_source, content, config_json, wasm_dir)
}

/// Content-vs-content diff with the SAME resolution/guardrail/branch behaviour as the live
/// diff handler, minus the git read — the native LSP server's `intentumdiff/semanticDiff`
/// two-file mode (#100 S3; the Python server serves it via `StringSource` + the differ).
pub fn live_diff_contents_impl(
    repo_path: &str,
    path: &str,
    old_content: &str,
    new_content: &str,
    config_json: &str,
    wasm_dir: &str,
) -> Result<String, String> {
    diff_resolved_sources(repo_path, path, old_content, new_content, config_json, wasm_dir)
}

/// The shared compute tail: resolve the parser, load the guardrail policy, and serve via
/// the native wasm chain (non-Python) or the certified batch (Python). Returns the
/// `{"diff"}` / `{"fallback"}` JSON both public entry points emit.
fn diff_resolved_sources(
    repo_path: &str,
    path: &str,
    old_source: &str,
    content: &str,
    config_json: &str,
    wasm_dir: &str,
) -> Result<String, String> {
    let fallback = |reason: &str| Ok(json!({ "fallback": reason }).to_string());

    // Resolve the parser via the bundled manifest (extension-based). An unknown extension or a
    // missing manifest -> the Python differ path. Python resolves to the native tree-sitter path
    // (empty wasm_path); other languages carry their wasm parser path (`lib.rs:3303`).
    let resolved = match crate::parser_registry::resolve_parser(path, wasm_dir) {
        Some(r) => r,
        None => return fallback("no bundled parser for this file extension"),
    };

    let config: Value = if config_json.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(config_json).map_err(|e| format!("invalid config json: {e}"))?
    };
    // Native guardrails (#100): strictly load the discovered policy's protected rules once.
    // Off-spec/unreadable policies defer (python raises for those on EVERY diff, python files
    // included); a rules-bearing policy is evaluated natively in the wasm chain below.
    let guardrail_rules: Option<Vec<Value>> =
        match load_guardrail_policy_state(repo_path, &config) {
            GuardrailPolicyState::Defer(reason) => return fallback(&reason),
            GuardrailPolicyState::Rules(rules) => Some(rules),
            GuardrailPolicyState::NoRules => None,
        };
    let guardrails_enabled =
        config.get("guardrails_enabled").and_then(Value::as_bool) != Some(false);

    // Non-Python (the manifest resolved a Wasm parser): the certified batch is Python-only, so
    // serve the diff from the native chain (parse -> finalize -> invariances -> assemble),
    // mirroring the differ's routed `_diff_single`. Any uncovered case returns Err -> fallback.
    if !resolved.wasm_path.is_empty() {
        return match crate::native_wasm_single_diff(
            &resolved.language,
            &resolved.wasm_path,
            &old_source,
            content,
            path,
            path,
            config_json,
            guardrail_rules.as_deref(),
        ) {
            Ok(mut diff) => {
                // python apply_guardrails_to_diff's policy-file check: editing intentumdiff.yaml
                // itself is an IMMUTABLE violation.
                if guardrails_enabled
                    && is_guardrail_policy_filename(path)
                    && old_source != content
                {
                    let violation = policy_file_violation(&diff, path, path);
                    crate::attach_guardrail_violations(&mut diff, vec![violation]);
                }
                Ok(json!({ "diff": diff }).to_string())
            }
            Err(reason) => fallback(&reason),
        };
    }

    let request = json!({
        "schema_version": 1,
        "mode": "commit_batch",
        "python_parser_backend": "native",
        "config": config,
        "parallel": false,
        "max_workers": Value::Null,
        "files": [{
            "old_source": old_source,
            "new_source": content,
            "old_filename": path,
            "new_filename": path,
            "language": resolved.language,
            "parser_plugin_id": resolved.plugin_id,
            "parser_wasm_path": resolved.wasm_path,
            "file_lifecycle": "",
        }],
    });

    // The batch is authoritative; only a per-file item with status "complete" + a "diff" is a
    // certified result (mirrors try_rust_core_batch_diffs). Anything else -> the Python path.
    match crate::diff_batch_impl(&request.to_string()) {
        Ok(result) => match result.get("diffs").and_then(Value::as_array).and_then(|d| d.first()) {
            Some(item) if item.get("status").and_then(Value::as_str) == Some("complete") => {
                match item.get("diff") {
                    Some(diff) => Ok(json!({ "diff": diff }).to_string()),
                    None => fallback("batch item missing diff"),
                }
            }
            Some(item) => fallback(
                item.get("reason")
                    .and_then(Value::as_str)
                    .or_else(|| item.get("status").and_then(Value::as_str))
                    .unwrap_or("batch item incomplete"),
            ),
            None => fallback(
                result
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or("batch produced no diff"),
            ),
        },
        Err(reason) => fallback(&reason),
    }
}

/// python `LiveServer._process_review_request` COMPUTE (#100, A2.5 Phase A2 + d2-review) — the
/// native commit-review handler. Each changed file resolves via the bundled parser manifest: an
/// all-Python commit runs the certified batch commit (`diff_batch_commit_json_impl`, unchanged);
/// a commit with resolvable non-Python files is served per-file (Python via the certified batch
/// item, other languages via `native_wasm_single_diff`) and assembled into a CommitDiff. Returns
/// `{commit_diff}` on a served result, else `{fallback}` so the Python `CommitDiffer` path runs
/// (unresolvable extension, uncertified Python file, off-spec guardrail policy, finalize
/// declined). Cross-file changes are computed for both branches
/// (`compute_commit_cross_file_changes`, mirroring CommitDiffer); commit guardrail_violations stay
/// empty (they are the union of the per-file violations, which are empty whenever the native path
/// serves — it defers on any in-effect policy). *config_json* = `self._differ._config`. *wasm_dir*
/// = the package wasm dir (see `live_handle_diff_json`).
pub fn live_handle_review_impl(
    repo_path: &str,
    old_ref: &str,
    new_ref: &str,
    config_json: &str,
    wasm_dir: &str,
) -> Result<String, String> {
    let fallback = |reason: &str| Ok(json!({ "fallback": reason }).to_string());

    // The FULL changed-sources dispatcher: new_ref "" = HEAD-vs-WORKING-TREE (the extension's
    // default review), ":staged" / ":unpushed" sentinels, else commit-to-commit.
    let rows = match crate::git_source::changed_sources(repo_path, old_ref, new_ref) {
        Ok(f) => f,
        Err(reason) => return fallback(&reason),
    };
    let config_value: Value = if config_json.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(config_json).map_err(|e| format!("invalid config json: {e}"))?
    };
    // Native guardrails (#100): load once for the whole commit; off-spec policies defer (python
    // raises for those on every file); rules are evaluated per non-Python file below.
    let guardrail_rules: Option<Vec<Value>> =
        match load_guardrail_policy_state(repo_path, &config_value) {
            GuardrailPolicyState::Defer(reason) => return fallback(&reason),
            GuardrailPolicyState::Rules(rules) => Some(rules),
            GuardrailPolicyState::NoRules => None,
        };
    let guardrails_enabled =
        config_value.get("guardrails_enabled").and_then(Value::as_bool) != Some(false);

    // Rows are tuples [old_content, new_content, old_path, new_path, staging]. Resolve each file's
    // parser via the manifest; an unresolvable extension -> fall back the WHOLE commit (the Python
    // CommitDiffer handles unknown extensions). Python files resolve to an empty wasm_path (native
    // tree-sitter), other languages to their wasm parser path.
    struct ReviewFile {
        old_content: String,
        new_content: String,
        old_path: String,
        new_path: String,
        staging: String,
        path: String,
        resolved: crate::parser_registry::ResolvedParser,
    }
    let mut review_files: Vec<ReviewFile> = Vec::with_capacity(rows.len());
    for row in &rows {
        let get = |i: usize| row.get(i).and_then(Value::as_str).unwrap_or("").to_owned();
        let (old_content, new_content, old_path, new_path, staging) =
            (get(0), get(1), get(2), get(3), get(4));
        let path = if !new_path.is_empty() { new_path.clone() } else { old_path.clone() };
        let resolved = match crate::parser_registry::resolve_parser(&path, wasm_dir) {
            Some(r) => r,
            None => return fallback(&format!("no bundled parser for {path}")),
        };
        review_files.push(ReviewFile {
            old_content,
            new_content,
            old_path,
            new_path,
            staging,
            path,
            resolved,
        });
    }

    // Commit-batch file objects with the RESOLVED language/plugin/wasm_path.
    let files: Vec<Value> = review_files
        .iter()
        .map(|f| {
            json!({
                "old_source": f.old_content,
                "new_source": f.new_content,
                "old_filename": f.old_path,
                "new_filename": f.new_path,
                "language": f.resolved.language,
                "parser_plugin_id": f.resolved.plugin_id,
                "parser_wasm_path": f.resolved.wasm_path,
                "file_lifecycle": "",
                "staging_status": f.staging,
            })
        })
        .collect();

    let any_non_python = review_files.iter().any(|f| !f.resolved.wasm_path.is_empty());

    // Cross-file changes (MOVE_TO_MODULE / SPLIT_MODULE / CROSS_FILE_RENAME) — mirror
    // CommitDiffer: build a per-side symbol table from the changed files' trees and diff them. The
    // certified commit payload leaves this empty, so compute + inject it for BOTH branches so the
    // native review matches `CommitDiffer.diff_commit`. Best-effort (empty on any failure).
    let cross_file_changes = crate::compute_commit_cross_file_changes(&files, config_json, wasm_dir);

    if !any_non_python {
        // All-Python: the certified commit path, unchanged (preserves the A2 parity contract).
        let request = json!({
            "schema_version": 1,
            "mode": "commit_json",
            "old_ref": old_ref,
            "new_ref": new_ref,
            "python_parser_backend": "native",
            "config": config_value,
            "parallel": false,
            "max_workers": Value::Null,
            "files": files,
        });
        return match crate::diff_batch_commit_json_impl(&request.to_string()) {
            Ok((control, payload)) => {
                let status = control.get("status").and_then(Value::as_str).unwrap_or("");
                let cert = control.get("certification").and_then(Value::as_str).unwrap_or("");
                if status == "complete" && cert == "python_native_v4kb" {
                    match payload {
                        // The commit payload is plain UTF-8 JSON (Python: `bytes(commit_payload)`).
                        Some(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                            Ok(mut commit_diff) => {
                                commit_diff["cross_file_changes"] = cross_file_changes.clone();
                                Ok(json!({ "commit_diff": commit_diff }).to_string())
                            }
                            Err(e) => fallback(&format!("commit payload parse: {e}")),
                        },
                        None => fallback("certified commit missing payload"),
                    }
                } else {
                    let reason = control
                        .get("reason")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .unwrap_or(if status.is_empty() { "commit not certified" } else { status });
                    fallback(reason)
                }
            }
            Err(reason) => fallback(&reason),
        };
    }

    // Mixed / non-Python commit: per-file dispatch. Run the certified batch to get the Python
    // files' diffs (non-Python items come back as fallbacks); compute the non-Python files via the
    // native chain. Assemble the CommitDiff (per-file diffs; cross-file/guardrails empty, matching
    // the certified commit contract).
    let batch_request = json!({
        "schema_version": 1,
        "mode": "commit_batch",
        "python_parser_backend": "native",
        "config": config_value,
        "parallel": false,
        "max_workers": Value::Null,
        "files": files,
    });
    let batch = match crate::diff_batch_impl(&batch_request.to_string()) {
        Ok(b) => b,
        Err(reason) => return fallback(&reason),
    };
    let empty_items: Vec<Value> = Vec::new();
    let items = batch
        .get("diffs")
        .and_then(Value::as_array)
        .unwrap_or(&empty_items);

    let mut file_diffs: Vec<Value> = Vec::with_capacity(review_files.len());
    for (index, f) in review_files.iter().enumerate() {
        let mut diff = if f.resolved.wasm_path.is_empty() {
            // Python: reuse the certified batch item (identical to the all-Python path's per-file diff).
            let item = items
                .iter()
                .find(|it| it.get("index").and_then(Value::as_u64) == Some(index as u64));
            match item {
                Some(it) if it.get("status").and_then(Value::as_str) == Some("complete") => {
                    match it.get("diff") {
                        Some(d) => d.clone(),
                        None => return fallback("certified batch item missing diff"),
                    }
                }
                _ => return fallback(&format!("python file not certified: {}", f.path)),
            }
        } else {
            match crate::native_wasm_single_diff(
                &f.resolved.language,
                &f.resolved.wasm_path,
                &f.old_content,
                &f.new_content,
                &f.old_path,
                &f.new_path,
                config_json,
                guardrail_rules.as_deref(),
            ) {
                Ok(d) => d,
                // Name the file: an anonymous per-file error cost a whole debugging round.
                Err(reason) => return fallback(&format!("{}: {reason}", f.path)),
            }
        };
        // python apply_guardrails_to_diff's policy-file check: an edit to intentumdiff.yaml itself.
        if guardrails_enabled
            && is_guardrail_policy_filename(&f.path)
            && f.old_content != f.new_content
        {
            let violation = policy_file_violation(&diff, &f.old_path, &f.new_path);
            crate::attach_guardrail_violations(&mut diff, vec![violation]);
        }
        // Carry the staging status (working-tree reviews); commit-to-commit rows leave it empty.
        if !f.staging.is_empty() {
            diff["staging_status"] = json!(f.staging);
        }
        file_diffs.push(diff);
    }

    // Commit-level guardrails = the union of per-file violations (commit_differ.py:365).
    let commit_guardrails: Vec<Value> = file_diffs
        .iter()
        .flat_map(|d| {
            d.get("guardrail_violations")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        })
        .collect();
    let commit_diff = json!({
        "old_ref": old_ref,
        "new_ref": new_ref,
        "file_diffs": file_diffs,
        "cross_file_changes": cross_file_changes,
        "guardrail_violations": commit_guardrails,
        "parse_errors": [],
    });
    Ok(json!({ "commit_diff": commit_diff }).to_string())
}

// ---------------------------------------------------------------------------
// Perceptual asset diff (op "asset_diff")
// ---------------------------------------------------------------------------

const ASSET_OP: &str = "asset_diff";
/// Where engine-written artifacts land: inside the repo's cache, never the work tree.
const ASSET_CACHE_DIR: &str = ".intentumdiff-cache";

/// python `LiveServer._asset_diff_request` — the perceptual image op, served for BOTH servers
/// from one implementation.
///
/// The engine has produced overlay/heatmap/mask/diff/contact-sheet artifacts all along and
/// nothing ever asked for them; this is the protocol method that does. Only *marshalling*
/// lives here — paths in, the engine's artifact manifest out.
///
/// Two request shapes, both engine-served:
///
/// * `{"path": …, "ref"?: …, "head"?: …}` — one tracked image against a git ref. This is what a
///   reviewing editor has: a repo-relative path and a base. The *before* bytes are in the object
///   store, and materialising them is git work, so the engine does it.
/// * `{"before_path": …, "after_path": …}` — two files that already exist on disk.
///
/// Paths resolve against the served repo and are refused if they escape it: a protocol method
/// that accepts file paths must not become an arbitrary-file reader, and the output directory
/// must not be able to write outside the cache.
///
/// Never fails: every problem comes back as a protocol error response, because one bad request
/// must not take the connection down with it.
pub fn live_handle_asset_diff_impl(
    repo_path: &str,
    default_ref: &str,
    request_json: &str,
    seq: i64,
) -> String {
    asset_diff_response(repo_path, default_ref, request_json, seq).to_string()
}

fn asset_diff_response(repo_path: &str, default_ref: &str, request_json: &str, seq: i64) -> Value {
    let request: Value = match serde_json::from_str(request_json) {
        Ok(value) => value,
        Err(_) => return error_obj(seq, "invalid_json", "request is not valid JSON", ASSET_OP),
    };
    // `options` absent or null is an empty option set; anything else goes to the engine as-is so
    // a malformed one is reported by the parser that owns the shape, not guessed at here.
    let options_json = match request.get("options") {
        Some(value) if !value.is_null() => value.to_string(),
        _ => "{}".to_owned(),
    };
    let root = resolve_for_containment(std::path::Path::new(repo_path));
    // Belt and braces: an empty root would make every containment test below trivially pass,
    // which is the one failure mode a path guard must never have.
    if root.as_os_str().is_empty() {
        return error_obj(
            seq,
            "invalid_request",
            "no repository is being served",
            ASSET_OP,
        );
    }

    let rel_path = request.get("path").and_then(Value::as_str).unwrap_or("");
    if !rel_path.is_empty() {
        // The path is repo-relative by construction and validated in the core, and is checked
        // here too: containment for this op must not depend on a single implementation
        // continuing to care.
        let normalized = rel_path.replace('\\', "/").trim_start_matches('/').to_owned();
        let candidate = resolve_for_containment(&root.join(&normalized));
        if candidate == root || !candidate.starts_with(&root) {
            return error_obj(
                seq,
                "invalid_request",
                "path escapes the served repository",
                ASSET_OP,
            );
        }
        let out_dir = match prepare_asset_output_dir(repo_path) {
            Ok(dir) => dir,
            Err(message) => return error_obj(seq, "internal", &message, ASSET_OP),
        };
        let request_ref = request.get("ref").and_then(Value::as_str).unwrap_or("");
        let base = match (request_ref.is_empty(), default_ref.is_empty()) {
            (false, _) => request_ref,
            (true, false) => default_ref,
            (true, true) => "HEAD",
        };
        let head = request.get("head").and_then(Value::as_str).unwrap_or("");
        return asset_manifest_response(
            crate::asset_diff::diff_git_asset_path_impl(
                repo_path,
                base,
                head,
                &normalized,
                &out_dir.to_string_lossy(),
                &options_json,
            ),
            seq,
        );
    }

    let before = request
        .get("before_path")
        .and_then(Value::as_str)
        .unwrap_or("");
    let after = request
        .get("after_path")
        .and_then(Value::as_str)
        .unwrap_or("");
    if before.is_empty() || after.is_empty() {
        return error_obj(
            seq,
            "invalid_request",
            "asset_diff needs path, or before_path and after_path",
            ASSET_OP,
        );
    }
    let mut resolved = Vec::with_capacity(2);
    for (label, value) in [("before_path", before), ("after_path", after)] {
        let raw = std::path::Path::new(value);
        let joined = if raw.is_absolute() {
            raw.to_path_buf()
        } else {
            root.join(raw)
        };
        let candidate = resolve_for_containment(&joined);
        if !candidate.starts_with(&root) {
            return error_obj(
                seq,
                "invalid_request",
                &format!("{label} escapes the served repository"),
                ASSET_OP,
            );
        }
        resolved.push(candidate);
    }
    let out_dir = match prepare_asset_output_dir(repo_path) {
        Ok(dir) => dir,
        Err(message) => return error_obj(seq, "internal", &message, ASSET_OP),
    };
    asset_manifest_response(
        crate::asset_diff::diff_asset_image_impl(
            &resolved[0].to_string_lossy(),
            &resolved[1].to_string_lossy(),
            &out_dir.to_string_lossy(),
            &options_json,
        ),
        seq,
    )
}

/// The engine's manifest, shaped into a protocol response. `done` is always true: this op has
/// no streaming form, so the client is waiting for exactly one answer.
fn asset_manifest_response(engine_result: Result<String, String>, seq: i64) -> Value {
    let raw = match engine_result {
        Ok(raw) => raw,
        Err(exc) => {
            return error_obj(
                seq,
                "internal",
                &format!("asset diff failed: {exc}"),
                ASSET_OP,
            )
        }
    };
    match serde_json::from_str::<Value>(&raw) {
        Ok(result) => json!({
            "op": ASSET_OP,
            "seq": seq,
            "ok": true,
            "done": true,
            "result": result,
        }),
        Err(exc) => error_obj(
            seq,
            "internal",
            &format!("asset diff returned unreadable JSON: {exc}"),
            ASSET_OP,
        ),
    }
}

/// Create `<repo>/.intentumdiff-cache/assets`, and make the cache ignore itself on the way.
fn prepare_asset_output_dir(repo_path: &str) -> Result<std::path::PathBuf, String> {
    let cache_dir = std::path::Path::new(repo_path).join(ASSET_CACHE_DIR);
    let out_dir = cache_dir.join("assets");
    std::fs::create_dir_all(&out_dir)
        .map_err(|exc| format!("create artifact directory {}: {exc}", out_dir.display()))?;
    ensure_cache_is_self_ignoring(&cache_dir);
    Ok(out_dir)
}

/// Write `.gitignore` containing `*` inside our own cache directory.
///
/// A tool that writes generated images into someone's repository and then relies on them
/// remembering to ignore it will show up as untracked noise in their next `git status`, and
/// eventually in a commit. Making the directory ignore ITSELF means a user never has to know it
/// exists — the same trick `.pytest_cache` and Cargo's `target/` use.
///
/// Written once and left alone; a user who deliberately edits it keeps their version. A failure
/// is swallowed on purpose: a read-only checkout must not break diffing.
fn ensure_cache_is_self_ignoring(cache_dir: &std::path::Path) {
    let marker = cache_dir.join(".gitignore");
    if marker.exists() {
        return;
    }
    let _ = std::fs::write(
        &marker,
        "# Created by IntentumDiff. Generated artifacts only - safe to delete.\n*\n",
    );
}

/// Fold `.` and `..` away without touching the filesystem.
fn normalize_lexically(path: &std::path::Path) -> std::path::PathBuf {
    let mut out = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Resolve *path* to the spelling `fs::canonicalize` would give, WITHOUT requiring the leaf to
/// exist.
///
/// Containment has to hold for files that are not there — a deleted image is precisely a case
/// this op must still answer — and `canonicalize` refuses a missing path. Canonicalising the
/// nearest ancestor that does exist and re-attaching the remainder keeps both sides of the
/// comparison in one spelling: on Windows the canonical form carries a `\\?\` prefix an
/// ordinary `C:\…` path does not, and comparing across the two would refuse a legitimate
/// in-repo path for a reason that has nothing to do with containment.
fn resolve_for_containment(path: &std::path::Path) -> std::path::PathBuf {
    // The ordinary case, and the only one that also follows symlinks: a path that exists.
    // Doing this FIRST matters for a relative path — the binary's repo argument defaults to
    // "." — which folds to nothing lexically and would leave containment comparing against an
    // empty root, i.e. not comparing at all.
    if let Ok(real) = std::fs::canonicalize(path) {
        return real;
    }
    let lexical = normalize_lexically(path);
    let mut suffix: Vec<std::ffi::OsString> = Vec::new();
    let mut probe = lexical.as_path();
    loop {
        if let Ok(real) = std::fs::canonicalize(probe) {
            let mut out = real;
            for part in suffix.iter().rev() {
                out.push(part);
            }
            return out;
        }
        match (probe.file_name(), probe.parent()) {
            (Some(name), Some(parent)) => {
                suffix.push(name.to_os_string());
                probe = parent;
            }
            _ => return lexical,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(req: &str) -> Value {
        serde_json::from_str(&live_parse_diff_request_json(req, "HEAD").unwrap()).unwrap()
    }

    #[test]
    fn parse_valid_request() {
        let v = parse(r#"{"path":"src/a.py","content":"x=1","seq":7}"#);
        let p = &v["parsed"];
        assert_eq!(p["seq"], 7);
        assert_eq!(p["path"], "src/a.py");
        assert_eq!(p["ref"], "HEAD"); // default applied
        assert!(p["stream"].is_null());
    }

    #[test]
    fn parse_rejects_bad_fields() {
        // invalid seq (non-numeric string) -> op "error"
        let v = parse(r#"{"path":"a.py","content":"x","seq":"nope"}"#);
        assert_eq!(v["error"]["error"]["code"], "invalid_seq");
        assert_eq!(v["error"]["op"], "error");
        // non-string content
        let v = parse(r#"{"path":"a.py","content":12345,"seq":1}"#);
        assert_eq!(v["error"]["error"]["code"], "invalid_content");
        assert_eq!(v["error"]["op"], "diff");
        // non-list deltas
        let v = parse(r#"{"path":"a.py","content":"x","seq":1,"deltas":{"bad":true}}"#);
        assert_eq!(v["error"]["error"]["code"], "invalid_deltas");
        // null seq -> invalid_seq
        let v = parse(r#"{"path":"a.py","content":"x","seq":null}"#);
        assert_eq!(v["error"]["error"]["code"], "invalid_seq");
        // missing/empty path
        let v = parse(r#"{"content":"x","seq":1}"#);
        assert_eq!(v["error"]["error"]["code"], "invalid_path");
    }

    #[test]
    fn path_guard_rejects_traversal_and_drive() {
        assert!(normalise_path("a/b/c.py").is_ok());
        assert_eq!(normalise_path("a\\b.py").unwrap(), "a/b.py");
        assert!(normalise_path("/etc/passwd").is_err());
        assert!(normalise_path("a/../../secret").is_err());
        assert!(normalise_path("C:/Windows").is_err());
    }

    fn parse_review(req: &str) -> Value {
        serde_json::from_str(&live_parse_review_request_json(req, "HEAD", 3).unwrap()).unwrap()
    }

    #[test]
    fn review_ref_defaults_and_validation() {
        // absent old_ref -> default; absent new_ref -> ""; stream truthy of False -> false
        let v = parse_review(r#"{}"#);
        assert_eq!(v["parsed"]["old_ref"], "HEAD");
        assert_eq!(v["parsed"]["new_ref"], "");
        assert_eq!(v["parsed"]["stream"], false);
        // explicit refs + stream true
        let v = parse_review(r#"{"old_ref":"abc","new_ref":"def","stream":true}"#);
        assert_eq!(v["parsed"]["old_ref"], "abc");
        assert_eq!(v["parsed"]["new_ref"], "def");
        assert_eq!(v["parsed"]["stream"], true);
        // empty old_ref -> default; null new_ref -> ""
        let v = parse_review(r#"{"old_ref":"","new_ref":null}"#);
        assert_eq!(v["parsed"]["old_ref"], "HEAD");
        assert_eq!(v["parsed"]["new_ref"], "");
        // non-string refs -> invalid_ref (op review, carries the caller seq)
        let v = parse_review(r#"{"old_ref":5}"#);
        assert_eq!(v["error"]["error"]["code"], "invalid_ref");
        assert_eq!(v["error"]["op"], "review");
        assert_eq!(v["error"]["seq"], 3);
        let v = parse_review(r#"{"new_ref":[1,2]}"#);
        assert_eq!(v["error"]["error"]["code"], "invalid_ref");
    }

    #[test]
    fn seq_coercion() {
        // float truncates toward zero (Python int(4.9)==4)
        let v = parse(r#"{"path":"a.py","content":"x","seq":4.9}"#);
        assert_eq!(v["parsed"]["seq"], 4);
        // numeric string parses
        let v = parse(r#"{"path":"a.py","content":"x","seq":"42"}"#);
        assert_eq!(v["parsed"]["seq"], 42);
        // absent -> 0
        let v = parse(r#"{"path":"a.py","content":"x"}"#);
        assert_eq!(v["parsed"]["seq"], 0);
    }

    // ── asset_diff (intentumdiff-vscode#25) ────────────────────────────────────────────────
    // The engine's own pins are `single_git_asset_path_*` in asset_diff.rs; these pin the
    // PROTOCOL layer above them — the piece the native live-server was missing entirely, where
    // an unknown op made the review panel report "Perceptual comparison unavailable".

    fn asset_temp_dir(name: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("intentumdiff-live-asset-{name}-{nonce}"));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn asset_git(repo: &std::path::Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .status();
        match status {
            Ok(status) if status.success() => {}
            Ok(status) => panic!("git {} failed with {status}", args.join(" ")),
            Err(_) => {}
        }
    }

    fn have_git() -> bool {
        std::process::Command::new("git")
            .arg("--version")
            .output()
            .is_ok()
    }

    /// A deterministic image; `shift` moves a block so two calls differ perceptibly.
    fn write_asset_png(path: &std::path::Path, shift: u8) {
        let mut image = image::RgbaImage::new(32, 32);
        for y in 0..32u32 {
            for x in 0..32u32 {
                let base: u8 = if (x / 4 + y / 4) % 2 == 0 { 30 } else { 200 };
                let lit = (8..22).contains(&y) && (6..20).contains(&x);
                let value = if lit { base.max(120).saturating_add(shift) } else { base };
                image.put_pixel(x, y, image::Rgba([value, 60, 140, 255]));
            }
        }
        image.save(path).expect("save test png");
    }

    /// A repo whose committed image differs from its working-tree copy.
    fn asset_repo(name: &str) -> std::path::PathBuf {
        let repo = asset_temp_dir(name);
        asset_git(&repo, &["init"]);
        asset_git(&repo, &["config", "user.email", "intentumdiff@example.test"]);
        asset_git(&repo, &["config", "user.name", "IntentumDiff Test"]);
        std::fs::create_dir_all(repo.join("assets")).expect("assets dir");
        write_asset_png(&repo.join("assets").join("card.png"), 0);
        asset_git(&repo, &["add", "."]);
        asset_git(&repo, &["commit", "-m", "base"]);
        write_asset_png(&repo.join("assets").join("card.png"), 90);
        repo
    }

    fn asset_diff(repo: &std::path::Path, default_ref: &str, request: Value) -> Value {
        serde_json::from_str(&live_handle_asset_diff_impl(
            &repo.to_string_lossy(),
            default_ref,
            &request.to_string(),
            7,
        ))
        .expect("the handler always answers with JSON")
    }

    /// The whole point of the op: a repo-relative path and a ref go in, and the ENGINE's artifact
    /// manifest comes back — not a summary assembled on the protocol side of the boundary.
    #[test]
    fn asset_diff_returns_the_engines_manifest() {
        if !have_git() {
            return;
        }
        let repo = asset_repo("manifest");
        let response = asset_diff(&repo, "HEAD", json!({ "path": "assets/card.png" }));

        assert_eq!(response["op"], "asset_diff");
        assert_eq!(response["seq"], 7);
        assert_eq!(response["ok"], true);
        assert_eq!(response["done"], true);
        let result = &response["result"];
        assert_eq!(result["status"], "compared");
        assert_eq!(result["file_path"], "assets/card.png");
        for layer in [
            "before",
            "after",
            "diff",
            "heatmap",
            "mask",
            "overlay",
            "contact_sheet",
        ] {
            let path = result["artifacts"][layer]
                .as_str()
                .unwrap_or_else(|| panic!("artifact {layer} missing from the manifest"));
            assert!(
                std::path::Path::new(path).is_file(),
                "artifact {layer} was announced at {path} but not written"
            );
        }
        assert!(result["changed_pixel_percentage"].as_f64().unwrap_or(0.0) > 0.0);
    }

    /// Artifacts land in the cache, and the cache ignores ITSELF — a tool that writes generated
    /// images into someone's repository must not leave them staring at untracked noise.
    #[test]
    fn asset_diff_artifacts_land_in_a_self_ignoring_cache() {
        if !have_git() {
            return;
        }
        let repo = asset_repo("cache");
        let response = asset_diff(&repo, "HEAD", json!({ "path": "assets/card.png" }));

        let cache = repo.join(ASSET_CACHE_DIR);
        let artifact = std::path::Path::new(
            response["result"]["artifacts"]["heatmap"]
                .as_str()
                .expect("heatmap artifact"),
        )
        .to_path_buf();
        assert!(
            artifact.starts_with(&cache),
            "artifacts must not land in the work tree: {}",
            artifact.display()
        );
        let ignore = std::fs::read_to_string(cache.join(".gitignore")).expect("cache .gitignore");
        assert!(ignore.contains('*'), "cache must ignore itself: {ignore}");

        // Written once and left alone: a user who edits it keeps their version.
        std::fs::write(cache.join(".gitignore"), "mine\n").expect("rewrite");
        asset_diff(&repo, "HEAD", json!({ "path": "assets/card.png" }));
        assert_eq!(
            std::fs::read_to_string(cache.join(".gitignore")).expect("cache .gitignore"),
            "mine\n"
        );
    }

    /// A protocol method that takes file paths must not become an arbitrary-file reader.
    #[test]
    fn asset_diff_refuses_paths_that_escape_the_served_repo() {
        let repo = asset_temp_dir("escape");
        std::fs::create_dir_all(repo.join("assets")).expect("assets dir");
        // A leading slash is stripped, not treated as an absolute path — "/etc/passwd" addresses
        // `<repo>/etc/passwd`, which is contained. What must never resolve is a path that CLIMBS.
        for path in ["../outside.png", "..\\outside.png", "assets/../../outside.png"] {
            let response = asset_diff(&repo, "HEAD", json!({ "path": path }));
            assert_eq!(response["ok"], false, "{path} should be refused");
            assert_eq!(response["error"]["code"], "invalid_request", "{path}");
            assert_eq!(response["op"], "asset_diff", "{path}");
        }

        let pair = asset_diff(
            &repo,
            "HEAD",
            json!({ "before_path": "../a.png", "after_path": "assets/b.png" }),
        );
        assert_eq!(pair["ok"], false);
        assert!(pair["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("before_path"));

        // An unusable root must refuse, not degrade to a containment test that compares against
        // nothing and therefore lets everything through.
        let rootless: Value = serde_json::from_str(&live_handle_asset_diff_impl(
            "",
            "HEAD",
            &json!({ "path": "../../outside.png" }).to_string(),
            1,
        ))
        .expect("json answer");
        assert_eq!(rootless["ok"], false);
        assert_eq!(rootless["error"]["code"], "invalid_request");
    }

    /// A request naming neither shape is a client mistake, and says so instead of erroring
    /// somewhere deeper with a message about a path nobody sent.
    #[test]
    fn asset_diff_without_a_path_or_pair_is_refused() {
        let repo = asset_temp_dir("empty-request");
        let response = asset_diff(&repo, "HEAD", json!({ "seq": 7 }));
        assert_eq!(response["ok"], false);
        assert_eq!(response["error"]["code"], "invalid_request");
        assert!(response["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("before_path"));
    }

    /// A request without a `ref` uses the ref the server was started with. Pinned through an
    /// unresolvable default, because a silently-wrong base produces a plausible comparison.
    #[test]
    fn asset_diff_falls_back_to_the_servers_default_ref() {
        if !have_git() {
            return;
        }
        let repo = asset_repo("default-ref");
        let response = asset_diff(&repo, "no-such-ref", json!({ "path": "assets/card.png" }));

        assert_eq!(response["ok"], false);
        let message = response["error"]["message"].as_str().unwrap_or_default();
        assert!(
            message.contains("git rev not found"),
            "an unresolvable ref must surface as one: {message}"
        );

        // An explicit ref still wins over the server default.
        let explicit = asset_diff(
            &repo,
            "no-such-ref",
            json!({ "path": "assets/card.png", "ref": "HEAD" }),
        );
        assert_eq!(explicit["ok"], true, "{explicit}");
    }

    /// The second request shape: two files already on disk, no git involved.
    #[test]
    fn asset_diff_compares_an_explicit_before_after_pair() {
        let repo = asset_temp_dir("pair");
        std::fs::create_dir_all(repo.join("assets")).expect("assets dir");
        write_asset_png(&repo.join("assets").join("before.png"), 0);
        write_asset_png(&repo.join("assets").join("after.png"), 90);

        let response = asset_diff(
            &repo,
            "HEAD",
            json!({ "before_path": "assets/before.png", "after_path": "assets/after.png" }),
        );

        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["result"]["status"], "compared");
        assert!(response["result"]["artifacts"]["overlay"].is_string());
    }

    /// An engine failure is a protocol error, not a dropped connection or a panic.
    #[test]
    fn asset_diff_reports_engine_failures_as_errors() {
        let repo = asset_temp_dir("engine-error");
        let response = asset_diff(&repo, "HEAD", json!({ "path": "assets/notes.txt" }));
        assert_eq!(response["ok"], false);
        assert!(response["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("asset diff failed"));

        let malformed = serde_json::from_str::<Value>(&live_handle_asset_diff_impl(
            &repo.to_string_lossy(),
            "HEAD",
            "{not json",
            3,
        ))
        .expect("even a malformed request gets a JSON answer");
        assert_eq!(malformed["error"]["code"], "invalid_json");
        assert_eq!(malformed["seq"], 3);
    }

    /// Containment must hold for a file that is not there: a DELETED image is exactly the case
    /// the op still has to answer, so the check cannot depend on the leaf existing.
    #[test]
    fn asset_diff_still_answers_for_a_deleted_image() {
        if !have_git() {
            return;
        }
        let repo = asset_repo("deleted");
        std::fs::remove_file(repo.join("assets").join("card.png")).expect("delete image");

        let response = asset_diff(&repo, "HEAD", json!({ "path": "assets/card.png" }));

        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["result"]["status"], "skipped");
        assert_eq!(response["result"]["change_type"], "D");
    }

    /// Advertised capability and served op have to agree, or a client that trusts the ready
    /// line asks for something that comes back as `invalid_op`.
    #[test]
    fn capabilities_advertise_the_asset_diff_op() {
        let capabilities: Value =
            serde_json::from_str(&live_capabilities_impl()).expect("capabilities json");
        let operations = capabilities["operations"]
            .as_array()
            .expect("operations list");
        assert!(
            operations.iter().any(|op| op == "asset_diff"),
            "asset_diff must be advertised: {operations:?}"
        );
    }

    #[test]
    fn native_diff_falls_back_without_manifest() {
        // With no wasm_dir/manifest, resolution fails -> fallback so the Python differ path runs.
        let v: Value = serde_json::from_str(
            &live_handle_diff_json("/nonexistent", "src/app.ts", "HEAD", "x = 1", "{}", "").unwrap(),
        )
        .unwrap();
        assert!(v.get("fallback").is_some(), "no manifest must fall back: {v}");
        assert!(v.get("diff").is_none());
    }
}

// Pyo3-free `#[cfg(test)]` stand-ins for the retired live-server `#[pyfunction]` wrappers (#B.6):
// the protocol test module drives these call shapes; production reaches the same `*_impl` functions
// via the native live-server binary and the C ABI's `live_*` handlers.
#[cfg(test)]
pub(crate) fn live_parse_diff_request_json(
    request_json: &str,
    default_ref: &str,
) -> Result<String, String> {
    live_parse_diff_request_impl(request_json, default_ref)
}

#[cfg(test)]
pub(crate) fn live_parse_review_request_json(
    request_json: &str,
    default_ref: &str,
    seq: i64,
) -> Result<String, String> {
    live_parse_review_request_impl(request_json, default_ref, seq)
}

#[cfg(test)]
pub(crate) fn live_handle_diff_json(
    repo_path: &str,
    path: &str,
    git_ref: &str,
    content: &str,
    config_json: &str,
    wasm_dir: &str,
) -> Result<String, String> {
    live_handle_diff_impl(repo_path, path, git_ref, content, config_json, wasm_dir)
}
