//! Process-global, path-keyed registry over the ungated `AnalyticsStore` (`analytics_store.rs`) —
//! the binding-shared, STATELESS face of the diff-history / fuel-diagnostics store (#B.5).
//!
//! Mirrors `cache_registry`: bindings drive analytics through stateless C-ABI functions
//! (`analytics_record_diff`, `analytics_query`, …) keyed only on the store path (the engine —
//! provided-DuckDB vs bundled-SQLite — is auto-selected at open), never an opaque handle across
//! the boundary. The store (and its connection) stays warm for the process so append-only
//! telemetry writes don't re-open the DB per call.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use crate::analytics_store::AnalyticsStore;
use crate::cache_store::StoreError;

fn registry() -> &'static Mutex<HashMap<String, Arc<AnalyticsStore>>> {
    static REG: OnceLock<Mutex<HashMap<String, Arc<AnalyticsStore>>>> = OnceLock::new();
    REG.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock_err() -> StoreError {
    StoreError::Db("analytics registry lock poisoned".to_string())
}

fn store_for(path: &str) -> Result<Arc<AnalyticsStore>, StoreError> {
    {
        let reg = registry().lock().map_err(|_| lock_err())?;
        if let Some(store) = reg.get(path) {
            return Ok(Arc::clone(store));
        }
    }
    let store = Arc::new(AnalyticsStore::open(path)?);
    let mut reg = registry().lock().map_err(|_| lock_err())?;
    let entry = reg.entry(path.to_string()).or_insert_with(|| Arc::clone(&store));
    Ok(Arc::clone(entry))
}

// ── Stateless entry points (mirror `AnalyticsStore`) ───────────────────────────────────────

/// Eagerly open + register the store (creating the DB file + schema), matching the retired
/// pyclass's open-on-construct — the Python `DuckDBAnalyticsStore(...)` calls this on construction.
pub fn open(path: &str) -> Result<(), StoreError> {
    store_for(path)?;
    Ok(())
}

pub fn backend(path: &str) -> Result<String, StoreError> {
    Ok(store_for(path)?.backend_name().to_string())
}

pub fn record_diff(path: &str, diff_json: &str) -> Result<(), StoreError> {
    store_for(path)?.record_diff(diff_json)
}

pub fn record_diagnostics_run(
    path: &str,
    diffs_json: Vec<String>,
    command: &str,
    repo: &str,
    argv_json: &str,
    run_id: Option<String>,
) -> Result<String, StoreError> {
    store_for(path)?.record_diagnostics_run(diffs_json, command, repo, argv_json, run_id)
}

pub fn query(path: &str, sql: &str) -> Result<String, StoreError> {
    store_for(path)?.query(sql)
}

pub fn query_readonly(path: &str, sql: &str) -> Result<String, StoreError> {
    store_for(path)?.query_readonly(sql)
}

pub fn most_changed_files(path: &str, limit: i64) -> Result<String, StoreError> {
    store_for(path)?.most_changed_files(limit)
}

pub fn changes_by_language(path: &str) -> Result<String, StoreError> {
    store_for(path)?.changes_by_language()
}

pub fn recent_diagnostic_runs(path: &str, limit: i64) -> Result<String, StoreError> {
    store_for(path)?.recent_diagnostic_runs(limit)
}

pub fn fuel_by_language(path: &str, limit: i64) -> Result<String, StoreError> {
    store_for(path)?.fuel_by_language(limit)
}

pub fn top_fuel_hotspots(path: &str, limit: i64) -> Result<String, StoreError> {
    store_for(path)?.top_fuel_hotspots(limit)
}

/// Close + drop the store for this path (removes it from the registry). A no-op if absent.
pub fn close(path: &str) -> Result<(), StoreError> {
    let removed = {
        let mut reg = registry().lock().map_err(|_| lock_err())?;
        reg.remove(path)
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
    fn registry_records_and_queries_over_a_warm_store() {
        let dir = std::env::temp_dir().join(format!("idf_anReg_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let db = dir.join("a.db");
        let path = db.to_str().unwrap();

        assert_eq!(backend(path).expect("backend"), "sqlite");

        let diff = serde_json::json!({
            "old_filename": "a.py", "new_filename": "a.py", "language": "python",
            "has_semantic_changes": true, "is_style_only": false, "is_fallback": false,
            "changes": [{"change_type": "ADDITION"}], "parse_errors": [],
            "metadata": {"engine_telemetry": {"calls": [{"plugin": "p", "function": "process",
                "language": "python", "trusted": true, "call_count": 1,
                "fuel_consumed": 5000000, "total_fuel_consumed": 5000000,
                "input_bytes": 100, "input_lines": 5}], "fuel_hotspots": []},
                "diagnostics": {"events": []}}
        });
        let run_id = record_diagnostics_run(path, vec![diff.to_string()], "c", ".", "[]", None)
            .expect("record");
        assert!(!run_id.is_empty());

        // Same path returns the same warm store (connection reused).
        assert!(Arc::ptr_eq(&store_for(path).unwrap(), &store_for(path).unwrap()));

        let langs: serde_json::Value =
            serde_json::from_str(&fuel_by_language(path, 20).expect("fuel")).unwrap();
        assert_eq!(langs[0]["language"], serde_json::json!("python"));

        // Read-only guard is enforced through the registry too.
        assert!(query_readonly(path, "DELETE FROM diff_history").is_err());

        close(path).expect("close");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
