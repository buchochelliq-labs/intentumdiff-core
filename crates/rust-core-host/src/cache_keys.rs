//! Deterministic cache-key generation (#101, A2.2) — the pure, binding-shared subset of the
//! cache subsystem, split out of the (python-gated) `cache` module so the C ABI reaches it in
//! the pure-Rust build. The SQLite parse/diff cache and the DuckDB analytics store stay
//! python-only (they carry the `#[pyclass]` I/O surface); only the key derivation is shared.
//!
//! Key encoding mirrors python `_make_key` exactly: each part is
//! `<4-byte little-endian length><utf-8 bytes>`, concatenated, then SHA-256 hex.
//! The length prefix makes the encoding injective (`["a\0b"]` != `["a", "b"]`).

use sha2::{Digest, Sha256};

/// python `store._make_key`: SHA-256 hex of length-prefixed parts.
pub(crate) fn make_key(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        let bytes = part.as_bytes();
        hasher.update((bytes.len() as u32).to_le_bytes());
        hasher.update(bytes);
    }
    hex::encode(hasher.finalize())
}

/// python `store._make_key` (variadic): SHA-256 hex of length-prefixed parts.
pub(crate) fn cache_make_key(parts: Vec<String>) -> String {
    let refs: Vec<&str> = parts.iter().map(String::as_str).collect();
    make_key(&refs)
}

/// python `CacheStore.parse_key`: key for a single-file parser result.
pub(crate) fn cache_parse_key(filtered_cst_or_content: &str, grammar_id: &str, wasm_hash: &str) -> String {
    make_key(&[filtered_cst_or_content, grammar_id, wasm_hash])
}

/// python `CacheStore.diff_key`: key for a full-pipeline SemanticDiff result.
pub(crate) fn cache_diff_key(
    old_preprocessed: &str,
    new_preprocessed: &str,
    grammar_id: &str,
    wasm_hash: &str,
) -> String {
    make_key(&[old_preprocessed, new_preprocessed, grammar_id, wasm_hash])
}

/// python `CacheStore.hover_map_key`: key for a file's LSP hover-type map, keyed by
/// the SHA-256 of the content (so path-independent) and the language.
pub(crate) fn cache_hover_map_key(content: &str, language: &str) -> String {
    let content_hash = hex::encode(Sha256::digest(content.as_bytes()));
    make_key(&[&content_hash, language])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_prefix_makes_the_encoding_injective() {
        assert_ne!(make_key(&["a\u{0}b"]), make_key(&["a", "b"]));
    }

    #[test]
    fn parse_and_hover_keys_match_python_reference() {
        // Cross-language parity: values computed by the python `_make_key`
        // (cache/store.py) — `_make_key("cst", "python", "abc123")` and the
        // hover key `_make_key(sha256("x = 1\n"), "python")`.
        assert_eq!(
            cache_parse_key("cst", "python", "abc123"),
            "16a26c3a8bfb24c6f9ee9175b206419c4daf70aed70a974fc4b9c77aa32ec400"
        );
        assert_eq!(
            cache_hover_map_key("x = 1\n", "python"),
            "a7779c52b77992a324ee4082e54e87c2ef6d8bbbc20ba9d71cb64bb90a1ac3f8"
        );
    }

    #[test]
    fn diff_key_and_hover_key_are_stable_and_distinct() {
        let d1 = cache_diff_key("old", "new", "python", "w");
        let d2 = cache_diff_key("old", "new", "python", "w");
        assert_eq!(d1, d2, "deterministic");
        assert_ne!(d1, cache_diff_key("new", "old", "python", "w"), "order matters");

        let h = cache_hover_map_key("x = 1\n", "python");
        assert_ne!(h, cache_hover_map_key("x = 1\n", "rust"), "language matters");
    }
}
