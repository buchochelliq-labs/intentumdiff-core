//! Test-only, pyo3-free stand-ins for the retired crate-root `#[pyfunction]` wrappers (#B.6).
//!
//! The gated `*_json` / `diff_batch*` wrappers were deleted with the `python` feature; the crate's
//! `#[cfg(test)]` suites still drive the engine through those exact call shapes. These thin
//! delegators to the ungated `*_impl` functions keep the tests exercising the real engine without
//! reintroducing pyo3 — production reaches the identical `*_impl` code via the C ABI (`c_abi.rs`).
//! `PyResult<T>` becomes `Result<T, String>` (the tests only `.unwrap()`); the wrappers' pyo3
//! error-mapping is dropped because every `*_impl` already returns `Result<_, String>`.

use serde_json::json;

use crate::{
    diff_batch_commit_json_impl, diff_batch_impl, diff_python_cst_impl,
    diff_python_sources_stage11_impl, diff_semantic_tree_impl, enrich_profile_labels_impl,
    finalize_review_impl, markdown_section_review_impl, BATCH_ENGINE, FALLBACK, SCAFFOLD, V3_ENGINE,
};

/// Batch entry: on error the retired wrapper returned a FALLBACK envelope (presentation-only logic
/// that lived in the wrapper, not the impl); the C ABI instead surfaces the error as `ok:false`.
pub(crate) fn diff_batch(request_json: &str) -> Result<String, String> {
    match diff_batch_impl(request_json) {
        Ok(payload) => Ok(payload.to_string()),
        Err(exc) => Ok(json!({
            "schema_version": 1,
            "status": FALLBACK,
            "engine": BATCH_ENGINE,
            "reason": exc,
            "diffs": [],
            "metadata": {
                "rust_core_stage": "batch_boundary",
                "boundary": "source_batch_to_final_diff",
            },
        })
        .to_string()),
    }
}

pub(crate) fn diff_batch_commit_json(
    request_json: &str,
) -> Result<(String, Option<Vec<u8>>), String> {
    match diff_batch_commit_json_impl(request_json) {
        Ok((control, commit_json)) => Ok((control.to_string(), commit_json)),
        Err(exc) => Ok((
            json!({
                "schema_version": 1,
                "status": FALLBACK,
                "engine": BATCH_ENGINE,
                "reason": exc,
                "commit_diff_byte_size": 0,
                "phase_timings": [],
            })
            .to_string(),
            None,
        )),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn diff_python_cst_json(
    old_filtered_cst_json: &str,
    new_filtered_cst_json: &str,
    _old_raw_source: &str,
    _new_raw_source: &str,
    old_filename: &str,
    new_filename: &str,
    _parser_wasm_path: &str,
    config_json: &str,
) -> Result<String, String> {
    diff_python_cst_impl(
        old_filtered_cst_json,
        new_filtered_cst_json,
        old_filename,
        new_filename,
        config_json,
    )
}

pub(crate) fn diff_python_semantic_tree_json(
    old_tree_json: &str,
    new_tree_json: &str,
    old_filename: &str,
    new_filename: &str,
    language: &str,
    config_json: &str,
) -> Result<String, String> {
    if !language.eq_ignore_ascii_case("python") {
        return Ok(json!({
            "status": SCAFFOLD,
            "engine": "rust_core_semantic_tree_v2_stage11",
            "reason": "unsupported language",
        })
        .to_string());
    }
    diff_semantic_tree_impl(
        old_tree_json,
        new_tree_json,
        old_filename,
        new_filename,
        language,
        config_json,
        "rust_core_semantic_tree_v2_stage11",
    )
}

pub(crate) fn diff_python_sources_stage11_json(
    old_source: &str,
    new_source: &str,
    old_filename: &str,
    new_filename: &str,
    parser_wasm_path: &str,
    config_json: &str,
) -> Result<String, String> {
    match diff_python_sources_stage11_impl(
        old_source,
        new_source,
        old_filename,
        new_filename,
        parser_wasm_path,
        config_json,
    ) {
        Ok(payload) => Ok(payload.to_string()),
        Err(exc) => Ok(json!({
            "status": SCAFFOLD,
            "engine": V3_ENGINE,
            "reason": exc,
        })
        .to_string()),
    }
}

pub(crate) fn enrich_profile_labels_json(
    tree_json: &str,
    source: &str,
    language: &str,
    identity_fields: Option<Vec<String>>,
) -> Result<String, String> {
    enrich_profile_labels_impl(tree_json, source, language, identity_fields)
}

pub(crate) fn finalize_review_json(
    old_tree_json: &str,
    new_tree_json: &str,
    old_source: &str,
    new_source: &str,
    language: &str,
    config_json: &str,
) -> Result<String, String> {
    finalize_review_impl(
        old_tree_json,
        new_tree_json,
        old_source,
        new_source,
        language,
        config_json,
    )
}

pub(crate) fn markdown_section_review_json(
    old_source: &str,
    new_source: &str,
) -> Result<String, String> {
    Ok(markdown_section_review_impl(old_source, new_source))
}

pub(crate) fn generic_text_review_json(
    old_source: &str,
    new_source: &str,
    raw_change_count: usize,
) -> Result<String, String> {
    Ok(crate::text_review_generic::generic_text_review_impl(
        old_source,
        new_source,
        raw_change_count,
    ))
}
