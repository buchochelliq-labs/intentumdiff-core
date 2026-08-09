//! The stable C ABI (`intentumdiff_call`) — Phase B, the split-readiness gate.
//!
//! A single `extern "C"` dispatch entrypoint that lets ANY language binding (Python via
//! ctypes/cffi, Go via cgo, Node via N-API, …) drive the pure-Rust engine WITHOUT libpython or
//! pyo3 — the north star's "thin bindings do zero functional work" made concrete. This is now the
//! sole language boundary: the crate ships only as the `cdylib`, and the pyo3 `#[pymodule]` skin
//! was retired once the ctypes binding reached parity over this ABI (#B.6).
//!
//! Contract (JSON in, JSON out — the wire is UTF-8 C strings):
//! - `intentumdiff_call(name, args_json)`: *name* is the function; *args_json* is a JSON array of
//!   positional arguments (or empty/`"[]"` for none). Returns a heap-allocated JSON envelope
//!   `{"ok": true, "result": <value>}` or `{"ok": false, "error": "<message>"}`, which the caller
//!   MUST free with [`intentumdiff_free`]. A NULL return means only an allocation/encoding failure.
//! - The boundary catches panics (unwinding across `extern "C"` is UB) and reports them as errors.
//!
//! Every handler delegates to the crate's plain-Rust `*_impl` functions — the same code the
//! retired pyo3 `#[pyfunction]`s wrapped — so the engine compute has a single implementation.

use std::ffi::{c_char, CStr, CString};

use serde_json::{json, Value};

// ── FFI boundary ────────────────────────────────────────────────────────────────────────

/// Dispatch a named engine call. See the module docs for the contract. `name`/`args_json` are
/// borrowed for the duration of the call; the returned pointer is owned by the caller.
///
/// # Safety
/// `name` and `args_json` must be valid, NUL-terminated UTF-8 C strings (or NULL, which is
/// reported as an error). The returned pointer must be freed with [`intentumdiff_free`] and not
/// used afterwards.
#[no_mangle]
pub unsafe extern "C" fn intentumdiff_call(
    name: *const c_char,
    args_json: *const c_char,
) -> *mut c_char {
    let payload = std::panic::catch_unwind(|| dispatch_raw(name, args_json))
        .unwrap_or_else(|_| error_envelope("internal panic in engine call", "internal"));
    match CString::new(payload) {
        Ok(cstring) => cstring.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Free a string returned by [`intentumdiff_call`].
///
/// # Safety
/// `ptr` must be a pointer previously returned by [`intentumdiff_call`] and not already freed.
#[no_mangle]
pub unsafe extern "C" fn intentumdiff_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(CString::from_raw(ptr));
    }
}

unsafe fn dispatch_raw(name: *const c_char, args_json: *const c_char) -> String {
    let name = match cstr(name) {
        Ok(s) => s,
        Err(e) => return error_envelope(&e, "bad_request"),
    };
    let args_raw = match cstr(args_json) {
        Ok(s) => s,
        Err(e) => return error_envelope(&e, "bad_request"),
    };
    let trimmed = args_raw.trim();
    let args: Vec<Value> = if trimmed.is_empty() {
        Vec::new()
    } else {
        match serde_json::from_str(trimmed) {
            Ok(a) => a,
            Err(e) => {
                return error_envelope(&format!("args_json must be a JSON array: {e}"), "bad_request")
            }
        }
    };
    dispatch(name, &args)
}

unsafe fn cstr<'a>(ptr: *const c_char) -> Result<&'a str, String> {
    if ptr.is_null() {
        return Err("null C string argument".to_owned());
    }
    CStr::from_ptr(ptr)
        .to_str()
        .map_err(|_| "C string argument is not valid UTF-8".to_owned())
}

// ── Envelope + argument helpers (pure, testable) ─────────────────────────────────────────

fn success_envelope(output: &str) -> String {
    // Embed the handler's output as structured JSON when it parses (the common case — impls
    // return JSON strings), else as a plain string.
    let result: Value = serde_json::from_str(output).unwrap_or_else(|_| Value::String(output.to_owned()));
    json!({"ok": true, "result": result}).to_string()
}

/// The classified failure kinds the envelope reports in `error_type`. The ctypes binding maps
/// `not_found` → `FileNotFoundError`, `internal` → a host/`RuntimeError`, and everything else
/// (`value_error`, `bad_request`) → `ValueError`, matching the pyo3 wrappers' exception types.
fn error_envelope(message: &str, kind: &str) -> String {
    json!({"ok": false, "error": message, "error_type": kind}).to_string()
}

fn arg_str<'a>(args: &'a [Value], index: usize, field: &str) -> Result<&'a str, String> {
    match args.get(index) {
        Some(Value::String(s)) => Ok(s),
        Some(_) => Err(format!("argument {index} ({field}) must be a string")),
        None => Err(format!("missing argument {index} ({field})")),
    }
}

/// An optional string arg: `None` when the slot is missing or JSON `null`; an error only when
/// present-but-not-a-string. Used for the `Option<&str>` pyfunction signatures.
fn arg_opt_str<'a>(args: &'a [Value], index: usize, field: &str) -> Result<Option<&'a str>, String> {
    match args.get(index) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(format!("argument {index} ({field}) must be a string or null")),
    }
}

/// Shared shape for the many `*_json` handlers that parse a single `request_json` arg into a
/// `Value`, run an ungated `fn(&Value) -> Result<Value, String>` engine helper, and stringify —
/// the exact body of the pyo3 wrappers, so the ABI and the extension stay in lockstep.
fn value_request(
    args: &[Value],
    f: impl Fn(&Value) -> Result<Value, String>,
) -> Result<String, String> {
    let request: Value = serde_json::from_str(arg_str(args, 0, "request_json")?)
        .map_err(|e| format!("request JSON: {e}"))?;
    f(&request).map(|v| v.to_string())
}

/// Envelope for the certified-commit fast path, which returns `(control JSON, Option<commit-diff
/// bytes>)`. The bytes are serialized `CommitDiff` JSON (valid UTF-8 by construction), so the ABI
/// carries them as a string field rather than re-parsing — preserving the fast path's intent of
/// not running pydantic on the hot path. `commit_diff_json` is null when there is no payload.
fn commit_tuple_envelope((control, payload): (Value, Option<Vec<u8>>)) -> String {
    json!({
        "control": control,
        "commit_diff_json": payload.map(|bytes| String::from_utf8_lossy(&bytes).into_owned()),
    })
    .to_string()
}

fn arg_i64(args: &[Value], index: usize, field: &str) -> Result<i64, String> {
    match args.get(index) {
        Some(v) => v
            .as_i64()
            .ok_or_else(|| format!("argument {index} ({field}) must be an integer")),
        None => Err(format!("missing argument {index} ({field})")),
    }
}

/// An optional boolean arg defaulting to `false` when the slot is missing or JSON `null` — the
/// shape of the pyfunctions with a `bool=false` signature default.
fn arg_opt_bool(args: &[Value], index: usize, field: &str) -> Result<bool, String> {
    match args.get(index) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(format!("argument {index} ({field}) must be a boolean")),
    }
}

/// Deserialize a structured JSON arg into `T` — for the handlers whose pyfunctions take typed
/// collections (e.g. `Vec<(String, String)>` dep-hash pairs) rather than scalars.
fn arg_json<T: serde::de::DeserializeOwned>(
    args: &[Value],
    index: usize,
    field: &str,
) -> Result<T, String> {
    match args.get(index) {
        Some(v) => serde_json::from_value(v.clone())
            .map_err(|e| format!("argument {index} ({field}): {e}")),
        None => Err(format!("missing argument {index} ({field})")),
    }
}

/// Like [`arg_json`] but `None` when the slot is missing or JSON `null` — the shape of the
/// pyfunctions with an `Option<Vec<..>>=None` signature default.
fn arg_opt_json<T: serde::de::DeserializeOwned>(
    args: &[Value],
    index: usize,
    field: &str,
) -> Result<Option<T>, String> {
    match args.get(index) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => serde_json::from_value(v.clone())
            .map(Some)
            .map_err(|e| format!("argument {index} ({field}): {e}")),
    }
}

// ── Stateless disk-cache helpers (#B.5, Rust-internal caching) ───────────────────────────
// The `cache_*` handlers drive the process-global `cache_registry` (a warm `SqliteStore` per
// path); these adapt its `Result<_, StoreError>` returns to the dispatch's `Result<String,
// String>`. `StoreError` collapses to its message (→ `value_error` on the envelope), matching
// the ValueError the pyo3 store wrappers raised for bad input; DB/IO failures are rare and
// carry their message too.
fn store_msg(e: crate::cache_store::StoreError) -> String {
    match e {
        crate::cache_store::StoreError::Db(m) | crate::cache_store::StoreError::Value(m) => m,
    }
}

/// A nullable string result (`get_*`): JSON `null` on miss, a JSON-encoded string on hit, so the
/// value survives `success_envelope`'s parse as a string (not a re-parsed object).
fn store_opt(r: Result<Option<String>, crate::cache_store::StoreError>) -> Result<String, String> {
    r.map(|opt| json!(opt).to_string()).map_err(store_msg)
}

/// A `(symbols, refs)` tuple result: JSON `null` on miss, `[symbols, refs]` on hit.
fn store_opt_pair(
    r: Result<Option<(String, String)>, crate::cache_store::StoreError>,
) -> Result<String, String> {
    r.map(|opt| json!(opt).to_string()).map_err(store_msg)
}

/// A unit result (`put_*` / `clear` / `close`): JSON `null`.
fn store_unit(r: Result<(), crate::cache_store::StoreError>) -> Result<String, String> {
    r.map(|_| "null".to_owned()).map_err(store_msg)
}

/// An already-JSON result (`stats` / `metrics` / `list_entries` / `export_entries` / …).
fn store_json(r: Result<String, crate::cache_store::StoreError>) -> Result<String, String> {
    r.map_err(store_msg)
}

/// A bare (non-JSON) scalar string result — JSON-string-encoded so it survives
/// `success_envelope` as a string regardless of content (e.g. the analytics backend name or a
/// numeric-looking run id).
fn store_scalar(r: Result<String, crate::cache_store::StoreError>) -> Result<String, String> {
    r.map(|s| json!(s).to_string()).map_err(store_msg)
}

// ── The dispatch table (grows as pyfunctions migrate onto the ABI) ───────────────────────

/// Map a function *name* + positional *args* to the engine's plain-Rust impl, returning the
/// JSON envelope. Pure and panic-free for known inputs — the FFI wrapper adds the C-string and
/// panic-safety layers. Public so a Rust consumer (and the tests) can exercise the ABI without
/// the FFI dance.
pub fn dispatch(name: &str, args: &[Value]) -> String {
    // The git content readers carry a NotFound/Invalid distinction the string-error path below
    // can't express; route them through a dedicated path so `not_found` survives to the envelope.
    if name == "git_source_content" || name == "working_tree_source_content" {
        return dispatch_git_reader(name, args);
    }
    // Every other handler surfaces a plain-string error → `value_error` (which the ctypes binding
    // maps to ValueError, matching the pyo3 wrappers, whose only other exception type is the
    // FileNotFoundError the git readers raise).
    let outcome: Result<String, String> = (|| match name {
        "version" => Ok(crate::VERSION.to_owned()),
        "live_capabilities" => Ok(crate::live_server::live_capabilities_impl()),
        "live_limits" => Ok(crate::live_server::live_limits_impl()),
        "live_normalise_request_path" => Ok(crate::live_server::live_normalise_request_path_impl(
            arg_str(args, 0, "path")?,
        )),
        "live_error_response" => Ok(crate::live_server::live_error_response_impl(
            arg_i64(args, 0, "seq")?,
            arg_str(args, 1, "code")?,
            arg_str(args, 2, "message")?,
            arg_str(args, 3, "op")?,
        )),
        "live_request_seq" => {
            crate::live_server::live_request_seq_impl(arg_str(args, 0, "request_json")?)
        }
        "live_parse_diff_request" => crate::live_server::live_parse_diff_request_impl(
            arg_str(args, 0, "request_json")?,
            arg_str(args, 1, "default_ref")?,
        ),
        "live_parse_review_request" => crate::live_server::live_parse_review_request_impl(
            arg_str(args, 0, "request_json")?,
            arg_str(args, 1, "default_ref")?,
            arg_i64(args, 2, "seq")?,
        ),
        "live_load_project_config" => {
            crate::live_server::live_load_project_config_impl(arg_str(args, 0, "repo_path")?)
        }
        "live_handle_diff" => crate::live_server::live_handle_diff_impl(
            arg_str(args, 0, "repo_path")?,
            arg_str(args, 1, "path")?,
            arg_str(args, 2, "git_ref")?,
            arg_str(args, 3, "content")?,
            arg_str(args, 4, "config_json")?,
            arg_str(args, 5, "wasm_dir")?,
        ),
        "live_diff_contents" => crate::live_server::live_diff_contents_impl(
            arg_str(args, 0, "repo_path")?,
            arg_str(args, 1, "path")?,
            arg_str(args, 2, "old_content")?,
            arg_str(args, 3, "new_content")?,
            arg_str(args, 4, "config_json")?,
            arg_str(args, 5, "wasm_dir")?,
        ),
        // Perceptual image op. Returns a whole protocol response (success OR error envelope) —
        // a bad request is a response, not a failed call, so this handler never errors.
        "live_handle_asset_diff" => Ok(crate::live_server::live_handle_asset_diff_impl(
            arg_str(args, 0, "repo_path")?,
            arg_str(args, 1, "default_ref")?,
            arg_str(args, 2, "request_json")?,
            arg_i64(args, 3, "seq")?,
        )),
        "live_handle_review" => crate::live_server::live_handle_review_impl(
            arg_str(args, 0, "repo_path")?,
            arg_str(args, 1, "old_ref")?,
            arg_str(args, 2, "new_ref")?,
            arg_str(args, 3, "config_json")?,
            arg_str(args, 4, "wasm_dir")?,
        ),
        "diff_batch" => {
            crate::diff_batch_impl(arg_str(args, 0, "request_json")?).map(|v| v.to_string())
        }
        "lsp_collect_hover_targets" => {
            crate::lsp_enrich::collect_hover_targets_json_impl(arg_str(args, 0, "tree_json")?)
        }
        "lsp_server_codelens" => {
            crate::lsp_server_shapes::codelens_json_impl(arg_str(args, 0, "diff_json")?)
        }
        "lsp_server_diagnostics" => {
            crate::lsp_server_shapes::diagnostics_json_impl(arg_str(args, 0, "diff_json")?)
        }
        "load_project_diff_config" => crate::config::load_config_section(
            arg_opt_str(args, 0, "start_path")?,
            arg_opt_str(args, 1, "explicit_path")?,
        ),
        "find_intentumdiff_config_path" => Ok(json!(crate::config::find_config_path(
            arg_opt_str(args, 0, "start_path")?,
            arg_opt_str(args, 1, "explicit_path")?,
        )
        .map(|p| p.display().to_string()))
        .to_string()),
        "git_repo_toplevel" => {
            crate::git_source::repo_toplevel(arg_str(args, 0, "repo_path")?)
        }
        "changed_sources" => crate::git_source::changed_sources(
            arg_str(args, 0, "repo_path")?,
            arg_str(args, 1, "old_ref")?,
            arg_str(args, 2, "new_ref")?,
        )
        .map(|rows| Value::Array(rows).to_string()),
        "changed_commit_sources" => crate::git_source::changed_commit_sources(
            arg_str(args, 0, "repo_path")?,
            arg_str(args, 1, "old_ref")?,
            arg_str(args, 2, "new_ref")?,
        )
        .map(|rows| Value::Array(rows).to_string()),
        // (git content readers — `git_source_content` / `working_tree_source_content` — are
        // handled by `dispatch_git_reader` above so their NotFound kind survives.)
        // Predicate + index-engine tables + invariance/scope value-helpers — each delegates to
        // the crate's ungated engine core (the same code the pyo3 wrappers call).
        "supports_language" => {
            Ok(json!(crate::supports_language_impl(arg_str(args, 0, "language")?)).to_string())
        }
        "build_symbol_table" => {
            Ok(index_engine_lib::build_symbol_table_impl(arg_str(args, 0, "files_json")?))
        }
        "build_reference_table" => {
            Ok(index_engine_lib::build_reference_table_impl(arg_str(args, 0, "files_json")?))
        }
        "diff_symbol_tables" => Ok(index_engine_lib::diff_symbol_tables_impl(
            arg_str(args, 0, "old_json")?,
            arg_str(args, 1, "new_json")?,
        )),
        "apply_invariances" => {
            value_request(args, crate::invariance_groups::rust_apply_invariances_value)
        }
        "scope_trails" => value_request(args, crate::rust_scope_trails_value),
        "finalize_stage11" => value_request(args, crate::rust_finalize_stage11_value),
        // The semantic-tree diff family — all delegate to the crate's ungated `*_impl` cores.
        "diff_semantic_tree" => crate::diff_semantic_tree_impl(
            arg_str(args, 0, "old_tree_json")?,
            arg_str(args, 1, "new_tree_json")?,
            arg_str(args, 2, "old_filename")?,
            arg_str(args, 3, "new_filename")?,
            arg_str(args, 4, "language")?,
            arg_str(args, 5, "config_json")?,
            "rust_core_semantic_tree_v3",
        ),
        "diff_python_semantic_tree" => {
            let language = arg_str(args, 4, "language")?;
            if !language.eq_ignore_ascii_case("python") {
                Ok(json!({
                    "status": crate::SCAFFOLD,
                    "engine": "rust_core_semantic_tree_v2_stage11",
                    "reason": "unsupported language",
                })
                .to_string())
            } else {
                crate::diff_semantic_tree_impl(
                    arg_str(args, 0, "old_tree_json")?,
                    arg_str(args, 1, "new_tree_json")?,
                    arg_str(args, 2, "old_filename")?,
                    arg_str(args, 3, "new_filename")?,
                    language,
                    arg_str(args, 5, "config_json")?,
                    "rust_core_semantic_tree_v2_stage11",
                )
            }
        }
        "diff_python_sources_stage11" => Ok(match crate::diff_python_sources_stage11_impl(
            arg_str(args, 0, "old_source")?,
            arg_str(args, 1, "new_source")?,
            arg_str(args, 2, "old_filename")?,
            arg_str(args, 3, "new_filename")?,
            arg_str(args, 4, "parser_wasm_path")?,
            arg_str(args, 5, "config_json")?,
        ) {
            Ok(payload) => payload.to_string(),
            Err(exc) => {
                json!({"status": crate::SCAFFOLD, "engine": crate::V3_ENGINE, "reason": exc})
                    .to_string()
            }
        }),
        "evaluate_guardrail_rules" => {
            let request: crate::GuardrailEvalRequest =
                serde_json::from_str(arg_str(args, 0, "request_json")?)
                    .map_err(|e| format!("request: {e}"))?;
            serde_json::to_string(&crate::evaluate_guardrail_rules(&request))
                .map_err(|e| format!("serialize: {e}"))
        }
        "enrich_node_facts" => {
            let mut tree: crate::SemanticNode =
                serde_json::from_str(arg_str(args, 0, "tree_json")?)
                    .map_err(|e| format!("tree JSON: {e}"))?;
            crate::enrich_tree_facts(&mut tree);
            serde_json::to_string(&tree).map_err(|e| format!("serialize tree: {e}"))
        }
        // VCS backend read ops (git/hg/svn/p4) — delegate to the newly-factored ungated impls.
        "vcs_backend_resolve_root" => crate::vcs_backend::vcs_backend_resolve_root_impl(
            arg_str(args, 0, "vcs")?,
            arg_str(args, 1, "repo_path")?,
        ),
        // get_blob returns arbitrary file bytes-as-text; wrap it in a JSON object (as the git
        // content readers do) so content that is itself valid JSON is not re-parsed by the envelope.
        "vcs_backend_get_blob" => crate::vcs_backend::vcs_backend_get_blob_impl(
            arg_str(args, 0, "vcs")?,
            arg_str(args, 1, "repo_path")?,
            arg_str(args, 2, "path")?,
            arg_str(args, 3, "git_ref")?,
            arg_opt_str(args, 4, "svn_repo_url")?,
        )
        .map(|content| json!({ "content": content }).to_string()),
        "vcs_backend_changed_files" => crate::vcs_backend::vcs_backend_changed_files_impl(
            arg_str(args, 0, "vcs")?,
            arg_str(args, 1, "repo_path")?,
            arg_str(args, 2, "ref_a")?,
            arg_str(args, 3, "ref_b")?,
            arg_opt_str(args, 4, "svn_repo_url")?,
        ),
        "vcs_backend_working_tree_changes" => {
            crate::vcs_backend::vcs_backend_working_tree_changes_impl(
                arg_str(args, 0, "vcs")?,
                arg_str(args, 1, "repo_path")?,
                arg_str(args, 2, "git_ref")?,
            )
        }
        "vcs_backend_merge_base" => crate::vcs_backend::vcs_backend_merge_base_impl(
            arg_str(args, 0, "vcs")?,
            arg_str(args, 1, "repo_path")?,
            arg_str(args, 2, "ref_a")?,
            arg_str(args, 3, "ref_b")?,
        ),
        // Registry-client #88 security validators. validate_registry_ref returns null on a
        // clean ref (the guard passed); validate_dep_hashes returns the JSON list of errors.
        "validate_registry_ref" => crate::registry::validate_registry_ref_impl(
            arg_str(args, 0, "git_ref")?,
            arg_opt_bool(args, 1, "strict")?,
        )
        .map(|()| "null".to_owned()),
        "validate_dep_hashes" => serde_json::to_string(&crate::registry::validate_dep_hashes_impl(
            arg_json(args, 0, "dep_hashes")?,
            arg_json(args, 1, "allowed_dependencies")?,
            arg_str(args, 2, "package_name")?,
            arg_opt_str(args, 3, "install_target")?,
        ))
        .map_err(|e| format!("serialize dep-hash errors: {e}")),
        // Deterministic cache-key derivation — the shared, binding-agnostic key module. A key is a
        // SHA-256 hex string (never valid JSON), so the envelope carries it verbatim.
        "cache_make_key" => Ok(crate::cache_keys::cache_make_key(arg_json(args, 0, "parts")?)),
        "cache_parse_key" => Ok(crate::cache_keys::cache_parse_key(
            arg_str(args, 0, "filtered_cst_or_content")?,
            arg_str(args, 1, "grammar_id")?,
            arg_str(args, 2, "wasm_hash")?,
        )),
        "cache_diff_key" => Ok(crate::cache_keys::cache_diff_key(
            arg_str(args, 0, "old_preprocessed")?,
            arg_str(args, 1, "new_preprocessed")?,
            arg_str(args, 2, "grammar_id")?,
            arg_str(args, 3, "wasm_hash")?,
        )),
        "cache_hover_map_key" => Ok(crate::cache_keys::cache_hover_map_key(
            arg_str(args, 0, "content")?,
            arg_str(args, 1, "language")?,
        )),
        // Stateless disk cache (#B.5): every op takes (path, ttl_days, max_mb) as its first three
        // args — the registry keys a warm SqliteStore on that tuple. The differ's `_cache` shim and
        // the CLI cache-admin commands drive these instead of a stateful pyclass store.
        "cache_open" => store_unit(crate::cache_registry::open(
            arg_str(args, 0, "path")?,
            arg_i64(args, 1, "ttl_days")?,
            arg_i64(args, 2, "max_mb")?,
        )),
        "cache_get_parse" => store_opt(crate::cache_registry::get_parse(
            arg_str(args, 0, "path")?,
            arg_i64(args, 1, "ttl_days")?,
            arg_i64(args, 2, "max_mb")?,
            arg_str(args, 3, "key")?,
        )),
        "cache_put_parse" => store_unit(crate::cache_registry::put_parse(
            arg_str(args, 0, "path")?,
            arg_i64(args, 1, "ttl_days")?,
            arg_i64(args, 2, "max_mb")?,
            arg_str(args, 3, "key")?,
            arg_str(args, 4, "value")?,
            arg_str(args, 5, "grammar_id")?,
        )),
        "cache_get_diff" => store_opt(crate::cache_registry::get_diff(
            arg_str(args, 0, "path")?,
            arg_i64(args, 1, "ttl_days")?,
            arg_i64(args, 2, "max_mb")?,
            arg_str(args, 3, "key")?,
        )),
        "cache_put_diff" => store_unit(crate::cache_registry::put_diff(
            arg_str(args, 0, "path")?,
            arg_i64(args, 1, "ttl_days")?,
            arg_i64(args, 2, "max_mb")?,
            arg_str(args, 3, "key")?,
            arg_str(args, 4, "value")?,
            arg_str(args, 5, "language")?,
            arg_str(args, 6, "old_filename")?,
            arg_str(args, 7, "new_filename")?,
        )),
        "cache_get_hover_map" => store_opt(crate::cache_registry::get_hover_map(
            arg_str(args, 0, "path")?,
            arg_i64(args, 1, "ttl_days")?,
            arg_i64(args, 2, "max_mb")?,
            arg_str(args, 3, "key")?,
        )),
        "cache_put_hover_map" => store_unit(crate::cache_registry::put_hover_map(
            arg_str(args, 0, "path")?,
            arg_i64(args, 1, "ttl_days")?,
            arg_i64(args, 2, "max_mb")?,
            arg_str(args, 3, "key")?,
            arg_str(args, 4, "value_json")?,
        )),
        "cache_get_symbol_index" => store_opt_pair(crate::cache_registry::get_symbol_index(
            arg_str(args, 0, "path")?,
            arg_i64(args, 1, "ttl_days")?,
            arg_i64(args, 2, "max_mb")?,
            arg_str(args, 3, "cache_key")?,
        )),
        "cache_put_symbol_index" => store_unit(crate::cache_registry::put_symbol_index(
            arg_str(args, 0, "path")?,
            arg_i64(args, 1, "ttl_days")?,
            arg_i64(args, 2, "max_mb")?,
            arg_str(args, 3, "cache_key")?,
            arg_str(args, 4, "symbols_json")?,
            arg_str(args, 5, "refs_json")?,
            arg_i64(args, 6, "file_count")?,
        )),
        "cache_stats" => store_json(crate::cache_registry::stats(
            arg_str(args, 0, "path")?,
            arg_i64(args, 1, "ttl_days")?,
            arg_i64(args, 2, "max_mb")?,
        )),
        "cache_metrics" => store_json(crate::cache_registry::metrics(
            arg_str(args, 0, "path")?,
            arg_i64(args, 1, "ttl_days")?,
            arg_i64(args, 2, "max_mb")?,
        )),
        "cache_list_entries" => store_json(crate::cache_registry::list_entries(
            arg_str(args, 0, "path")?,
            arg_i64(args, 1, "ttl_days")?,
            arg_i64(args, 2, "max_mb")?,
            arg_str(args, 3, "table")?,
            arg_opt_str(args, 4, "language")?,
            arg_opt_json(args, 5, "since")?,
            arg_opt_json(args, 6, "before")?,
            arg_opt_json(args, 7, "min_size")?,
            arg_opt_json(args, 8, "max_size")?,
            arg_i64(args, 9, "limit")?,
            arg_opt_bool(args, 10, "with_glob")?,
        )),
        "cache_get_entry_metadata" => store_opt(crate::cache_registry::get_entry_metadata(
            arg_str(args, 0, "path")?,
            arg_i64(args, 1, "ttl_days")?,
            arg_i64(args, 2, "max_mb")?,
            arg_str(args, 3, "key")?,
            arg_str(args, 4, "table")?,
        )),
        "cache_get_entry_payload" => store_opt(crate::cache_registry::get_entry_payload(
            arg_str(args, 0, "path")?,
            arg_i64(args, 1, "ttl_days")?,
            arg_i64(args, 2, "max_mb")?,
            arg_str(args, 3, "key")?,
            arg_str(args, 4, "table")?,
        )),
        "cache_export_entries" => store_json(crate::cache_registry::export_entries(
            arg_str(args, 0, "path")?,
            arg_i64(args, 1, "ttl_days")?,
            arg_i64(args, 2, "max_mb")?,
            arg_str(args, 3, "table")?,
        )),
        "cache_clear" => store_unit(crate::cache_registry::clear(
            arg_str(args, 0, "path")?,
            arg_i64(args, 1, "ttl_days")?,
            arg_i64(args, 2, "max_mb")?,
            arg_opt_bool(args, 3, "parse")?,
            arg_opt_bool(args, 4, "diff")?,
            arg_opt_bool(args, 5, "index")?,
            arg_opt_bool(args, 6, "hover")?,
        )),
        "cache_close" => store_unit(crate::cache_registry::close(
            arg_str(args, 0, "path")?,
            arg_i64(args, 1, "ttl_days")?,
            arg_i64(args, 2, "max_mb")?,
        )),
        // Stateless analytics store (#B.5): the append-only diff-history / fuel-diagnostics store.
        // Every op takes `path` first (the engine — provided-DuckDB vs bundled-SQLite — is
        // auto-selected at open); the registry keeps one warm store per path.
        "analytics_open" => store_unit(crate::analytics_registry::open(
            arg_str(args, 0, "path")?,
        )),
        "analytics_backend" => store_scalar(crate::analytics_registry::backend(
            arg_str(args, 0, "path")?,
        )),
        "analytics_record_diff" => store_unit(crate::analytics_registry::record_diff(
            arg_str(args, 0, "path")?,
            arg_str(args, 1, "diff_json")?,
        )),
        "analytics_record_diagnostics_run" => {
            store_scalar(crate::analytics_registry::record_diagnostics_run(
                arg_str(args, 0, "path")?,
                arg_json(args, 1, "diffs_json")?,
                arg_str(args, 2, "command")?,
                arg_str(args, 3, "repo")?,
                arg_str(args, 4, "argv_json")?,
                arg_opt_str(args, 5, "run_id")?.map(str::to_owned),
            ))
        }
        "analytics_query" => store_json(crate::analytics_registry::query(
            arg_str(args, 0, "path")?,
            arg_str(args, 1, "sql")?,
        )),
        "analytics_query_readonly" => store_json(crate::analytics_registry::query_readonly(
            arg_str(args, 0, "path")?,
            arg_str(args, 1, "sql")?,
        )),
        "analytics_most_changed_files" => store_json(crate::analytics_registry::most_changed_files(
            arg_str(args, 0, "path")?,
            arg_i64(args, 1, "limit")?,
        )),
        "analytics_changes_by_language" => store_json(
            crate::analytics_registry::changes_by_language(arg_str(args, 0, "path")?),
        ),
        "analytics_recent_diagnostic_runs" => {
            store_json(crate::analytics_registry::recent_diagnostic_runs(
                arg_str(args, 0, "path")?,
                arg_i64(args, 1, "limit")?,
            ))
        }
        "analytics_fuel_by_language" => store_json(crate::analytics_registry::fuel_by_language(
            arg_str(args, 0, "path")?,
            arg_i64(args, 1, "limit")?,
        )),
        "analytics_top_fuel_hotspots" => store_json(crate::analytics_registry::top_fuel_hotspots(
            arg_str(args, 0, "path")?,
            arg_i64(args, 1, "limit")?,
        )),
        "analytics_close" => store_unit(crate::analytics_registry::close(
            arg_str(args, 0, "path")?,
        )),
        // Perceptual asset (image) diff — parse options + run in the engine, JSON artifacts out.
        "diff_asset_image" => crate::asset_diff::diff_asset_image_impl(
            arg_str(args, 0, "before_path")?,
            arg_str(args, 1, "after_path")?,
            arg_str(args, 2, "output_dir")?,
            arg_str(args, 3, "options_json")?,
        ),
        "diff_git_assets" => crate::asset_diff::diff_git_assets_impl(
            arg_str(args, 0, "repo_path")?,
            arg_str(args, 1, "base")?,
            arg_str(args, 2, "head")?,
            arg_str(args, 3, "output_dir")?,
            arg_str(args, 4, "options_json")?,
        ),
        // One tracked image against a ref — what an editor reviewing a single file needs. The
        // engine materialises the base blob, so no binding has to shell out to git for it.
        "diff_git_asset_path" => crate::asset_diff::diff_git_asset_path_impl(
            arg_str(args, 0, "repo_path")?,
            arg_str(args, 1, "base")?,
            arg_str(args, 2, "head")?,
            arg_str(args, 3, "rel_path")?,
            arg_str(args, 4, "output_dir")?,
            arg_str(args, 5, "options_json")?,
        ),
        // Review finalization + profile/guardrail enrichment — the remaining lib.rs engine seams.
        "finalize_review" => crate::finalize_review_impl(
            arg_str(args, 0, "old_tree_json")?,
            arg_str(args, 1, "new_tree_json")?,
            arg_str(args, 2, "old_source")?,
            arg_str(args, 3, "new_source")?,
            arg_str(args, 4, "language")?,
            arg_str(args, 5, "config_json")?,
        ),
        "enrich_profile_labels" => crate::enrich_profile_labels_impl(
            arg_str(args, 0, "tree_json")?,
            arg_str(args, 1, "source")?,
            arg_str(args, 2, "language")?,
            arg_opt_json(args, 3, "identity_fields")?,
        ),
        "guardrail_semantic_paths" => {
            let tree: crate::SemanticNode =
                serde_json::from_str(arg_str(args, 0, "tree_json")?)
                    .map_err(|e| format!("tree: {e}"))?;
            serde_json::to_string(&crate::guardrail_semantic_paths(
                &tree,
                &arg_str(args, 1, "language")?.to_lowercase(),
            ))
            .map_err(|e| format!("serialize: {e}"))
        }
        "register_user_xml_dialects" => {
            let dialects: Vec<crate::UserXmlDialect> = arg_json(args, 0, "dialects")?;
            Ok(json!(crate::set_user_xml_dialects(dialects)).to_string())
        }
        // Text-presentation review + raw-source stub + content sniffing.
        "diff_python" => Ok(crate::diff_python_impl(
            arg_str(args, 0, "old_content")?,
            arg_str(args, 1, "new_content")?,
            arg_str(args, 2, "old_filename")?,
            arg_str(args, 3, "new_filename")?,
        )),
        "generic_text_review" => Ok(crate::text_review_generic::generic_text_review_impl(
            arg_str(args, 0, "old_source")?,
            arg_str(args, 1, "new_source")?,
            arg_json(args, 2, "raw_change_count")?,
        )),
        // detect_content_type sniffs leading bytes; over the ABI the head slice arrives as a JSON
        // array of byte values (dependency-free, and a head slice is small). Never errors.
        "detect_content_type" => {
            let data: Vec<u8> = arg_json(args, 0, "data")?;
            serde_json::to_string(&crate::content_type::detect_content_type(&data))
                .map_err(|e| format!("serialize content type: {e}"))
        }
        // Filtered-CST v1 diff + markdown section review (moves/renames).
        "diff_python_cst" => crate::diff_python_cst_impl(
            arg_str(args, 0, "old_filtered_cst_json")?,
            arg_str(args, 1, "new_filtered_cst_json")?,
            arg_str(args, 2, "old_filename")?,
            arg_str(args, 3, "new_filename")?,
            arg_str(args, 4, "config_json")?,
        ),
        "markdown_section_review" => Ok(crate::markdown_section_review_impl(
            arg_str(args, 0, "old_source")?,
            arg_str(args, 1, "new_source")?,
        )),
        "parse_to_tree" => crate::parse_to_tree(
            arg_str(args, 0, "path")?,
            arg_str(args, 1, "content")?,
            arg_str(args, 2, "config_json")?,
            arg_str(args, 3, "wasm_dir")?,
        ),
        // The certified-commit fast path — a (control, commit-diff-bytes) tuple, marshalled into a
        // single {control, commit_diff_json} envelope (the bytes are certified CommitDiff JSON).
        "diff_batch_commit_json" => {
            crate::diff_batch_commit_json_impl(arg_str(args, 0, "request_json")?)
                .map(commit_tuple_envelope)
        }
        "diff_working_tree_python_commit_json" => {
            crate::diff_working_tree_python_commit_json_impl(arg_str(args, 0, "request_json")?)
                .map(commit_tuple_envelope)
        }
        other => Err(format!("unknown function: {other}")),
    })();
    match outcome {
        Ok(output) => success_envelope(&output),
        Err(message) => error_envelope(&message, "value_error"),
    }
}

/// The git content readers, kept out of the string-error match so their `NotFound` vs `Invalid`
/// distinction reaches the envelope as `not_found` / `value_error` (the ctypes binding re-raises
/// FileNotFoundError vs ValueError from it, matching the pyo3 wrappers). A malformed *call* (a
/// missing/non-string arg) is a `value_error`, not a not-found.
fn dispatch_git_reader(name: &str, args: &[Value]) -> String {
    let parsed: Result<Result<String, crate::git_source::GitReadError>, String> = (|| {
        Ok(match name {
            "git_source_content" => crate::git_source::git_source_content_impl(
                arg_str(args, 0, "repo_path")?,
                arg_str(args, 1, "file_path")?,
                arg_str(args, 2, "old_ref")?,
                arg_str(args, 3, "new_ref")?,
            ),
            _ => crate::git_source::working_tree_source_content_impl(
                arg_str(args, 0, "repo_path")?,
                arg_str(args, 1, "file_path")?,
                arg_str(args, 2, "git_ref")?,
            ),
        })
    })();
    match parsed {
        Err(arg_error) => error_envelope(&arg_error, "value_error"),
        Ok(Ok(json)) => success_envelope(&json),
        Ok(Err(crate::git_source::GitReadError::NotFound(m))) => error_envelope(&m, "not_found"),
        Ok(Err(crate::git_source::GitReadError::Invalid(m))) => error_envelope(&m, "value_error"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    fn call(name: &str, args: Value) -> Value {
        serde_json::from_str(&dispatch(name, args.as_array().unwrap())).unwrap()
    }

    #[test]
    fn version_dispatches_with_no_args() {
        let env = call("version", json!([]));
        assert_eq!(env["ok"], true);
        assert_eq!(env["result"], crate::VERSION);
    }

    #[test]
    fn no_arg_json_impls_return_structured_results() {
        for name in ["live_capabilities", "live_limits"] {
            let env = call(name, json!([]));
            assert_eq!(env["ok"], true, "{name}");
            assert!(env["result"].is_object(), "{name} result should be a JSON object");
        }
    }

    #[test]
    fn multi_arg_impl_marshals_positional_args() {
        let env = call("live_error_response", json!([7, "bad_thing", "it broke", "diff"]));
        assert_eq!(env["ok"], true);
        assert_eq!(env["result"]["seq"], 7);
        assert_eq!(env["result"]["error"]["code"], "bad_thing");
    }

    #[test]
    fn fallible_impl_success_and_error() {
        // A valid request seq parses.
        let ok = call("live_request_seq", json!([r#"{"seq": 3}"#]));
        assert_eq!(ok["ok"], true);
        // A malformed one surfaces the impl's error through the envelope.
        let bad = call("live_request_seq", json!(["not json"]));
        assert_eq!(bad["ok"], false);
        assert!(bad["error"].as_str().is_some());
    }

    #[test]
    fn unknown_function_and_bad_args_are_errors() {
        let env = call("nope", json!([]));
        assert_eq!(env["ok"], false);
        assert_eq!(env["error_type"], "value_error");
        // Wrong arg type — a malformed call classified as value_error (→ ValueError in the binding).
        let env = call("live_error_response", json!(["not-an-int", "c", "m", "o"]));
        assert_eq!(env["ok"], false);
        assert!(env["error"].as_str().unwrap().contains("seq"));
        assert_eq!(env["error_type"], "value_error");
    }

    #[test]
    fn ffi_round_trip_through_intentumdiff_call() {
        let name = CString::new("version").unwrap();
        let args = CString::new("[]").unwrap();
        let ptr = unsafe { intentumdiff_call(name.as_ptr(), args.as_ptr()) };
        assert!(!ptr.is_null());
        let out = unsafe { CStr::from_ptr(ptr).to_str().unwrap().to_owned() };
        unsafe { intentumdiff_free(ptr) };
        let env: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(env["ok"], true);
        assert_eq!(env["result"], crate::VERSION);
    }

    #[test]
    fn lsp_shape_handlers_dispatch_over_the_abi() {
        // codelens/diagnostics take a diff JSON and return arrays — pure, no wasm needed.
        let empty_diff = r#"{"changes": [], "parse_errors": []}"#;
        for name in ["lsp_server_codelens", "lsp_server_diagnostics"] {
            let env = call(name, json!([empty_diff]));
            assert_eq!(env["ok"], true, "{name}");
            assert!(env["result"].is_array(), "{name} should return an array");
            assert_eq!(env["result"].as_array().unwrap().len(), 0, "{name} empty");
        }
        // hover-target collection over a bare tree.
        let tree = r#"{"id":"m","node_type":"module","label":"m",
            "position":{"start_line":0,"start_col":0,"end_line":9,"end_col":0},
            "structural_hash":"h","children":[]}"#;
        let env = call("lsp_collect_hover_targets", json!([tree]));
        assert_eq!(env["ok"], true);
        assert!(env["result"].is_array());
    }

    #[test]
    fn config_handlers_take_optional_args() {
        // No args (both None) -> engine defaults, valid JSON config.
        let env = call("load_project_diff_config", json!([]));
        assert_eq!(env["ok"], true);
        assert!(env["result"].is_object());
        // find_config_path returns a path string or null (the walk-up may find a repo config
        // from the cwd fallback) — either is a valid, non-error result.
        let env = call("find_intentumdiff_config_path", json!([null, null]));
        assert_eq!(env["ok"], true);
        assert!(env["result"].is_string() || env["result"].is_null());
        // A non-string, non-null arg is a clear error.
        let env = call("load_project_diff_config", json!([42]));
        assert_eq!(env["ok"], false);
        assert!(env["error"].as_str().unwrap().contains("start_path"));
    }

    #[test]
    fn stateless_disk_cache_dispatches_over_the_abi() {
        let dir = std::env::temp_dir().join(format!("idf_cabi_cache_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let db = dir.join("c.db");
        let path = db.to_str().unwrap();

        // Miss -> result is JSON null (not an error).
        let miss = call("cache_get_diff", json!([path, 30, 500, "k"]));
        assert_eq!(miss["ok"], true);
        assert!(miss["result"].is_null());

        // Put, then hit -> the cached string comes back as a STRING (not a re-parsed object),
        // even though its content is itself valid JSON.
        let put = call(
            "cache_put_diff",
            json!([path, 30, 500, "k", "{\"changes\":[]}", "python", "a.py", "b.py"]),
        );
        assert_eq!(put["ok"], true);
        let hit = call("cache_get_diff", json!([path, 30, 500, "k"]));
        assert_eq!(hit["ok"], true);
        assert_eq!(hit["result"], "{\"changes\":[]}");

        // Admin surface reaches the same warm store.
        let stats = call("cache_stats", json!([path, 30, 500]));
        assert_eq!(stats["result"]["diff_cache"]["count"], 1);

        // A bad-limit list is a value_error (the store's ValueError parity).
        let bad = call("cache_list_entries", json!([path, 30, 500, "diff_cache", null, null, null, null, null, 0, false]));
        assert_eq!(bad["ok"], false);
        assert_eq!(bad["error_type"], "value_error");

        assert_eq!(call("cache_clear", json!([path, 30, 500, true, true, true, true]))["ok"], true);
        assert!(call("cache_get_diff", json!([path, 30, 500, "k"]))["result"].is_null());
        let _ = call("cache_close", json!([path, 30, 500]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stateless_analytics_dispatches_over_the_abi() {
        let dir = std::env::temp_dir().join(format!("idf_cabi_an_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let db = dir.join("a.db");
        let path = db.to_str().unwrap();

        // Backend name comes back as a plain string (bundled SQLite fallback in tests).
        assert_eq!(call("analytics_backend", json!([path]))["result"], "sqlite");

        // Record a diagnostics run (diffs arrive as a JSON array of diff-JSON strings) and get a
        // non-empty run id string back.
        let diff = r#"{"old_filename":"a.py","new_filename":"a.py","language":"python",
            "has_semantic_changes":true,"is_style_only":false,"is_fallback":false,
            "changes":[{"change_type":"ADDITION"}],"parse_errors":[],
            "metadata":{"engine_telemetry":{"calls":[{"plugin":"p","function":"process",
            "language":"python","trusted":true,"call_count":1,"fuel_consumed":5000000,
            "total_fuel_consumed":5000000,"input_bytes":100,"input_lines":5}],"fuel_hotspots":[]},
            "diagnostics":{"events":[]}}}"#;
        let run = call("analytics_record_diagnostics_run", json!([path, [diff], "c", ".", "[]", null]));
        assert_eq!(run["ok"], true);
        assert!(run["result"].as_str().is_some_and(|s| !s.is_empty()));

        // A query result is a JSON array; the read-only guard rejects mutations as value_error.
        let langs = call("analytics_fuel_by_language", json!([path, 20]));
        assert_eq!(langs["result"][0]["language"], "python");
        let blocked = call("analytics_query_readonly", json!([path, "DELETE FROM diff_history"]));
        assert_eq!(blocked["ok"], false);
        assert_eq!(blocked["error_type"], "value_error");

        let _ = call("analytics_close", json!([path]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn index_and_value_helpers_dispatch() {
        // Certified-language predicate marshals a bool result.
        assert_eq!(call("supports_language", json!(["python"]))["result"], true);
        assert_eq!(call("supports_language", json!(["go"]))["result"], false);
        // Index-engine tables over an empty file set: a valid, non-error result.
        let env = call("build_symbol_table", json!(["[]"]));
        assert_eq!(env["ok"], true);
        let env = call("diff_symbol_tables", json!(["{}", "{}"]));
        assert_eq!(env["ok"], true);
        // The value-helper shape: a non-JSON request_json is a clean error envelope, not a panic.
        let env = call("apply_invariances", json!(["not json"]));
        assert_eq!(env["ok"], false);
        assert!(env["error"].as_str().unwrap().contains("request JSON"));
        // A well-formed (if minimal) request dispatches to the engine and returns an envelope.
        assert!(call("scope_trails", json!(["{}"]))["ok"].is_boolean());
    }

    #[test]
    fn diff_family_dispatches_over_the_abi() {
        let tree = r#"{"id":"m","node_type":"module","label":"m",
            "position":{"start_line":0,"start_col":0,"end_line":9,"end_col":0},
            "structural_hash":"h","children":[]}"#;
        // A well-formed tree pair diffs to a complete result.
        let env = call(
            "diff_semantic_tree",
            json!([tree, tree, "a.py", "a.py", "python", "{}"]),
        );
        assert_eq!(env["ok"], true);
        assert_eq!(env["result"]["status"], "complete");
        // The python-gated variant scaffolds on a non-python language rather than diffing.
        let env = call(
            "diff_python_semantic_tree",
            json!([tree, tree, "a.go", "a.go", "go", "{}"]),
        );
        assert_eq!(env["ok"], true);
        assert_eq!(env["result"]["reason"], "unsupported language");
        // enrich_node_facts round-trips a tree; a malformed guardrail request is a clean error.
        assert_eq!(call("enrich_node_facts", json!([tree]))["ok"], true);
        let env = call("evaluate_guardrail_rules", json!(["not json"]));
        assert_eq!(env["ok"], false);
        assert!(env["error"].as_str().unwrap().contains("request"));
    }

    #[test]
    fn cache_keys_dispatch_and_match_the_reference() {
        // Same length-prefixed SHA-256 the python `_make_key` produces (cache/store.py parity).
        assert_eq!(
            call("cache_parse_key", json!(["cst", "python", "abc123"]))["result"],
            "16a26c3a8bfb24c6f9ee9175b206419c4daf70aed70a974fc4b9c77aa32ec400"
        );
        // Variadic form takes a JSON array of parts; deterministic + order-sensitive.
        let a = call("cache_make_key", json!([["a", "b"]]));
        assert_eq!(a["ok"], true);
        assert_ne!(
            a["result"],
            call("cache_make_key", json!([["b", "a"]]))["result"]
        );
    }

    #[test]
    fn parse_to_tree_dispatches_and_surfaces_errors() {
        // An unresolvable extension (empty wasm_dir -> no manifest) is a clean error envelope,
        // not a panic. (A successful parse is covered end-to-end by the CLI's index command.)
        let env = call("parse_to_tree", json!(["x.unknownext", "content", "{}", ""]));
        assert_eq!(env["ok"], false);
        assert!(env["error"].as_str().unwrap().contains("no bundled parser"));
    }

    #[test]
    fn cst_diff_and_markdown_review_dispatch() {
        // diff_python_cst surfaces a clean error envelope on malformed CST JSON (no valid tree).
        let env = call(
            "diff_python_cst",
            json!(["not a cst", "not a cst", "a.py", "a.py", "{}"]),
        );
        assert_eq!(env["ok"], false);
        assert!(env["error"].as_str().unwrap().contains("CST JSON"));
        // markdown_section_review is infallible and reports `used` over the ABI.
        let env = call(
            "markdown_section_review",
            json!(["# A\n\nbody\n", "# A\n\nbody\n"]),
        );
        assert_eq!(env["ok"], true);
        assert_eq!(env["result"]["used"], true);
    }

    #[test]
    fn text_review_and_content_sniffing_dispatch() {
        // Raw-source stub: identical content is a complete no-change result.
        let env = call("diff_python", json!(["x=1", "x=1", "a.py", "a.py"]));
        assert_eq!(env["ok"], true);
        // Generic text review over a simple line change (raw_change_count as the 3rd arg).
        let env = call("generic_text_review", json!(["a\nb\n", "a\nc\n", 2]));
        assert_eq!(env["ok"], true);
        assert!(env["result"]["used"].is_boolean());
        // Content sniffing: PNG magic bytes as a JSON byte array -> an image content type.
        let env = call(
            "detect_content_type",
            json!([[137, 80, 78, 71, 13, 10, 26, 10]]),
        );
        assert_eq!(env["ok"], true);
        assert!(env["result"].is_object());
    }

    #[test]
    fn review_and_enrichment_seams_dispatch() {
        let tree = r#"{"id":"m","node_type":"module","label":"m",
            "position":{"start_line":0,"start_col":0,"end_line":9,"end_col":0},
            "structural_hash":"h","children":[]}"#;
        // finalize_review over a well-formed tree pair.
        let env = call(
            "finalize_review",
            json!([tree, tree, "", "", "python", "{}"]),
        );
        assert_eq!(env["ok"], true);
        // profile-label enrichment (optional identity_fields defaulting to null) round-trips.
        assert_eq!(
            call("enrich_profile_labels", json!([tree, "", "json", null]))["ok"],
            true
        );
        // guardrail semantic-path index for a non-guardrail language is an empty object.
        let env = call("guardrail_semantic_paths", json!([tree, "python"]));
        assert_eq!(env["ok"], true);
        assert!(env["result"].is_object());
        // registering an empty XML-dialect set replaces with zero dialects.
        assert_eq!(call("register_user_xml_dialects", json!([[]]))["result"], 0);
    }

    #[test]
    fn asset_diff_dispatches_and_surfaces_errors() {
        // Malformed options JSON is a clean error envelope from the shared impl (no image I/O).
        let env = call(
            "diff_asset_image",
            json!(["a.png", "b.png", "/out", "not json"]),
        );
        assert_eq!(env["ok"], false);
        assert!(env["error"].as_str().unwrap().contains("options JSON"));
        // The single-path git variant is reachable by name and rejects a non-image path before
        // it touches git — the editor's per-file asset request lands here.
        let env = call(
            "diff_git_asset_path",
            json!(["/repo", "HEAD", "", "src/main.rs", "/out", "{}"]),
        );
        assert_eq!(env["ok"], false);
        assert!(env["error"]
            .as_str()
            .unwrap()
            .contains("not a perceptually comparable image path"));
    }

    #[test]
    fn registry_validators_dispatch_over_the_abi() {
        // A clean ref passes -> null result; a traversal ref is a #88 guard error.
        assert!(call("validate_registry_ref", json!(["main", false]))["result"].is_null());
        let env = call("validate_registry_ref", json!(["../etc/passwd", false]));
        assert_eq!(env["ok"], false);
        assert!(env["error"].as_str().unwrap().contains("Unsafe registry ref"));
        // strict mode (default false when the slot is null) requires a full commit SHA.
        let env = call("validate_registry_ref", json!(["main", true]));
        assert_eq!(env["ok"], false);
        // dep-hashes: a well-formed, self-covered pinning yields an empty error list.
        let env = call(
            "validate_dep_hashes",
            json!([[["intentumdiff-foo==1.0.0", format!("sha256:{}", "a".repeat(64))]], [], "intentumdiff-foo", null]),
        );
        assert_eq!(env["ok"], true);
        assert_eq!(env["result"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn vcs_backend_ops_dispatch_over_the_abi() {
        // An unknown VCS is a clean error envelope from the ported impl (no git/hg/svn/p4 needed).
        let env = call("vcs_backend_resolve_root", json!(["mercurialish", "/repo"]));
        assert_eq!(env["ok"], false);
        assert!(env["error"].as_str().unwrap().contains("resolve_root not yet ported"));
        // get_blob's argument-injection guard rejects a ref starting with '-' before any
        // subprocess runs (an absent-but-valid ref would instead yield empty content, not an error).
        let env = call("vcs_backend_get_blob", json!(["git", "/repo", "f", "--upload-pack=x", null]));
        assert_eq!(env["ok"], false);
        assert!(env["error"].as_str().unwrap().contains("argument-injection"));
        // merge_base for an unsupported VCS surfaces its ported message.
        let env = call("vcs_backend_merge_base", json!(["svn", "/repo", "a", "b"]));
        assert_eq!(env["ok"], false);
        assert!(env["error"].as_str().unwrap().contains("merge_base not supported"));
    }

    #[test]
    fn git_read_ops_dispatch_and_surface_errors() {
        // Missing positional args are rejected with a message naming the slot — deterministic,
        // no git binary or repo required.
        let env = call("changed_sources", json!(["/some/repo"]));
        assert_eq!(env["ok"], false);
        assert!(env["error"].as_str().unwrap().contains("old_ref"));
        // A path that is not a git work tree surfaces an error envelope, never a panic (whether
        // git is absent or the path is simply not a repo, the outcome is a clean ok:false).
        let env = call("git_repo_toplevel", json!(["/no/such/repo/anywhere"]));
        assert_eq!(env["ok"], false);
        assert!(env["error"].as_str().is_some());
        // A not-a-repo error is `value_error` (matches the pyo3 wrapper's ValueError), NOT a
        // not-found — the ref itself is unresolvable.
        let env = call(
            "git_source_content",
            json!(["/no/such/repo", "f.py", "HEAD~1", "HEAD"]),
        );
        assert_eq!(env["ok"], false);
        assert_eq!(env["error_type"], "value_error");
        // A missing arg on a content reader is also value_error, never a crash.
        let env = call("git_source_content", json!(["/repo"]));
        assert_eq!(env["ok"], false);
        assert_eq!(env["error_type"], "value_error");
    }

    /// Create a throwaway git repo with one committed file; None if git is unavailable (skip).
    fn temp_git_repo(file: &str, content: &str) -> Option<std::path::PathBuf> {
        use std::process::Command;
        let dir = std::env::temp_dir().join(format!("idf_cabi_gs_{}", uuid::Uuid::new_v4()));
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
    fn git_content_reader_absent_path_is_not_found() {
        let Some(dir) = temp_git_repo("present.py", "x = 1\n") else {
            return; // git unavailable — skip
        };
        let path = dir.to_str().unwrap();
        // A file absent at HEAD -> the reader returns NotFound -> the envelope carries not_found
        // (the ctypes binding re-raises FileNotFoundError from it).
        let env = call(
            "working_tree_source_content",
            json!([path, "absent.py", "HEAD"]),
        );
        assert_eq!(env["ok"], false, "{env}");
        assert_eq!(env["error_type"], "not_found", "{env}");
        assert!(env["error"].as_str().unwrap().contains("not found"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn diff_batch_dispatches_and_surfaces_errors() {
        // A malformed batch request is an error envelope, not a panic.
        let env = call("diff_batch", json!(["not a valid request"]));
        assert_eq!(env["ok"], false);
        assert!(env["error"].as_str().is_some());
    }

    #[test]
    fn certified_commit_fast_path_marshals_the_tuple() {
        // The commit fast path returns (control, commit-diff bytes); a malformed request still
        // surfaces a clean error envelope rather than panicking across the boundary.
        let env = call("diff_batch_commit_json", json!(["not a valid request"]));
        assert_eq!(env["ok"], false);
        assert!(env["error"].as_str().is_some());
        let env = call("diff_working_tree_python_commit_json", json!(["{}"]));
        // An empty request is either a structured {control, commit_diff_json} result or a clean
        // error — never a panic; in both cases the envelope is well-formed.
        assert!(env["ok"].is_boolean());
        if env["ok"] == true {
            assert!(env["result"].get("control").is_some());
        }
    }

    #[test]
    fn null_args_are_rejected_without_crashing() {
        let name = CString::new("version").unwrap();
        let ptr = unsafe { intentumdiff_call(name.as_ptr(), std::ptr::null()) };
        let out = unsafe { CStr::from_ptr(ptr).to_str().unwrap().to_owned() };
        unsafe { intentumdiff_free(ptr) };
        let env: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(env["ok"], false);
        // A NULL/malformed call at the FFI boundary is classified bad_request.
        assert_eq!(env["error_type"], "bad_request");
    }
}
