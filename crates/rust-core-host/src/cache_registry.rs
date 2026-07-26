//! Process-global, path-keyed registry over the ungated `SqliteStore` (`cache_store.rs`) — the
//! binding-shared, STATELESS face of the disk cache (#B.5, Rust-internal caching).
//!
//! Bindings (the Python ctypes shell, the clap CLI) drive the cache through stateless C-ABI
//! functions (`cache_get_diff`, `cache_put_diff`, …) whose identity is the `(path, ttl_days,
//! max_mb)` tuple — never an opaque handle exposed across the boundary (the locked "no
//! handle-based store ABI" decision). Opening a fresh `SqliteStore` per call would re-run the
//! schema setup + TTL/size eviction on every hot get/put; instead this registry keeps the
//! `SqliteStore` (and its persistent WAL connection) alive for the process and reuses it, so a
//! caller sees a stateless API over a warm connection.
//!
//! Lifecycle: entries live until `close` is called for their key (or the process exits). Cache
//! paths are few (usually one per repo), so the map stays tiny.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use crate::cache_store::{SqliteStore, StoreError};

/// `(canonicalised path, ttl_days, max_mb)` — a store opened with different retention is a
/// distinct entry (SQLite WAL tolerates multiple same-process connections to one file).
type StoreKey = (String, i64, i64);

fn registry() -> &'static Mutex<HashMap<StoreKey, Arc<SqliteStore>>> {
    static REG: OnceLock<Mutex<HashMap<StoreKey, Arc<SqliteStore>>>> = OnceLock::new();
    REG.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock_err() -> StoreError {
    StoreError::Db("cache registry lock poisoned".to_string())
}

/// Return the warm store for `(path, ttl_days, max_mb)`, opening + registering it on first use.
fn store_for(path: &str, ttl_days: i64, max_mb: i64) -> Result<Arc<SqliteStore>, StoreError> {
    let key = (path.to_string(), ttl_days, max_mb);
    {
        let reg = registry().lock().map_err(|_| lock_err())?;
        if let Some(store) = reg.get(&key) {
            return Ok(Arc::clone(store));
        }
    }
    // Open outside the registry lock so a slow open never blocks other cache paths.
    let store = Arc::new(SqliteStore::open(path, ttl_days, max_mb)?);
    let mut reg = registry().lock().map_err(|_| lock_err())?;
    // Another thread may have opened it while we were opening; keep the first winner.
    let entry = reg.entry(key).or_insert_with(|| Arc::clone(&store));
    Ok(Arc::clone(entry))
}

// ── Stateless entry points (mirror `SqliteStore`) ──────────────────────────────────────────

/// Eagerly open + register the store (creating the DB file + schema, running eviction), matching
/// the retired pyclass's open-on-construct. The Python `SqliteCacheStore(...)` calls this so that
/// constructing a store persists an (empty) cache — the CLI's "no cache found" check depends on it.
pub fn open(path: &str, ttl_days: i64, max_mb: i64) -> Result<(), StoreError> {
    store_for(path, ttl_days, max_mb)?;
    Ok(())
}

pub fn get_parse(path: &str, ttl_days: i64, max_mb: i64, key: &str) -> Result<Option<String>, StoreError> {
    store_for(path, ttl_days, max_mb)?.get_parse(key)
}

pub fn put_parse(
    path: &str,
    ttl_days: i64,
    max_mb: i64,
    key: &str,
    value: &str,
    grammar_id: &str,
) -> Result<(), StoreError> {
    store_for(path, ttl_days, max_mb)?.put_parse(key, value, grammar_id)
}

pub fn get_diff(path: &str, ttl_days: i64, max_mb: i64, key: &str) -> Result<Option<String>, StoreError> {
    store_for(path, ttl_days, max_mb)?.get_diff(key)
}

#[allow(clippy::too_many_arguments)]
pub fn put_diff(
    path: &str,
    ttl_days: i64,
    max_mb: i64,
    key: &str,
    value: &str,
    language: &str,
    old_filename: &str,
    new_filename: &str,
) -> Result<(), StoreError> {
    store_for(path, ttl_days, max_mb)?.put_diff(key, value, language, old_filename, new_filename)
}

pub fn get_hover_map(path: &str, ttl_days: i64, max_mb: i64, key: &str) -> Result<Option<String>, StoreError> {
    store_for(path, ttl_days, max_mb)?.get_hover_map(key)
}

pub fn put_hover_map(
    path: &str,
    ttl_days: i64,
    max_mb: i64,
    key: &str,
    value_json: &str,
) -> Result<(), StoreError> {
    store_for(path, ttl_days, max_mb)?.put_hover_map(key, value_json)
}

pub fn get_symbol_index(
    path: &str,
    ttl_days: i64,
    max_mb: i64,
    cache_key: &str,
) -> Result<Option<(String, String)>, StoreError> {
    store_for(path, ttl_days, max_mb)?.get_symbol_index(cache_key)
}

#[allow(clippy::too_many_arguments)]
pub fn put_symbol_index(
    path: &str,
    ttl_days: i64,
    max_mb: i64,
    cache_key: &str,
    symbols_json: &str,
    refs_json: &str,
    file_count: i64,
) -> Result<(), StoreError> {
    store_for(path, ttl_days, max_mb)?.put_symbol_index(cache_key, symbols_json, refs_json, file_count)
}

pub fn stats(path: &str, ttl_days: i64, max_mb: i64) -> Result<String, StoreError> {
    store_for(path, ttl_days, max_mb)?.stats()
}

pub fn metrics(path: &str, ttl_days: i64, max_mb: i64) -> Result<String, StoreError> {
    store_for(path, ttl_days, max_mb)?.metrics()
}

#[allow(clippy::too_many_arguments)]
pub fn list_entries(
    path: &str,
    ttl_days: i64,
    max_mb: i64,
    table: &str,
    language: Option<&str>,
    since: Option<i64>,
    before: Option<i64>,
    min_size: Option<i64>,
    max_size: Option<i64>,
    limit: i64,
    with_glob: bool,
) -> Result<String, StoreError> {
    store_for(path, ttl_days, max_mb)?.list_entries(
        table, language, since, before, min_size, max_size, limit, with_glob,
    )
}

pub fn get_entry_metadata(
    path: &str,
    ttl_days: i64,
    max_mb: i64,
    key: &str,
    table: &str,
) -> Result<Option<String>, StoreError> {
    store_for(path, ttl_days, max_mb)?.get_entry_metadata(key, table)
}

pub fn get_entry_payload(
    path: &str,
    ttl_days: i64,
    max_mb: i64,
    key: &str,
    table: &str,
) -> Result<Option<String>, StoreError> {
    store_for(path, ttl_days, max_mb)?.get_entry_payload(key, table)
}

pub fn export_entries(path: &str, ttl_days: i64, max_mb: i64, table: &str) -> Result<String, StoreError> {
    store_for(path, ttl_days, max_mb)?.export_entries(table)
}

pub fn clear(
    path: &str,
    ttl_days: i64,
    max_mb: i64,
    parse: bool,
    diff: bool,
    index: bool,
    hover: bool,
) -> Result<(), StoreError> {
    store_for(path, ttl_days, max_mb)?.clear(parse, diff, index, hover)
}

/// Close + drop the store for this key (removes it from the registry). A no-op if absent.
pub fn close(path: &str, ttl_days: i64, max_mb: i64) -> Result<(), StoreError> {
    let key = (path.to_string(), ttl_days, max_mb);
    let removed = {
        let mut reg = registry().lock().map_err(|_| lock_err())?;
        reg.remove(&key)
    };
    if let Some(store) = removed {
        store.close()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_reuses_connection_and_roundtrips() {
        let dir = std::env::temp_dir().join(format!("idf_cacheReg_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let db = dir.join("reg.db");
        let path = db.to_str().unwrap();

        // Miss, then put, then hit — across separate stateless calls (same warm store).
        assert!(get_diff(path, 30, 500, "k").expect("get").is_none());
        put_diff(path, 30, 500, "k", "DIFF", "python", "a.py", "b.py").expect("put");
        assert_eq!(get_diff(path, 30, 500, "k").expect("get").as_deref(), Some("DIFF"));
        put_parse(path, 30, 500, "p", "TREE", "python").expect("put parse");
        assert_eq!(get_parse(path, 30, 500, "p").expect("get parse").as_deref(), Some("TREE"));

        // Same key returns the same Arc (connection reused, not reopened).
        let a = store_for(path, 30, 500).expect("store a");
        let b = store_for(path, 30, 500).expect("store b");
        assert!(Arc::ptr_eq(&a, &b));

        // Admin surface reaches the same warm store.
        let stats: serde_json::Value = serde_json::from_str(&stats(path, 30, 500).expect("stats")).unwrap();
        assert_eq!(stats["diff_cache"]["count"], 1);

        drop(a);
        drop(b);
        close(path, 30, 500).expect("close");
        // After close the key is gone; a fresh get re-opens (and the row persisted on disk).
        assert_eq!(get_diff(path, 30, 500, "k").expect("get").as_deref(), Some("DIFF"));
        close(path, 30, 500).expect("close 2");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
