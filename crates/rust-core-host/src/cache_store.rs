//! The SQLite parse/diff/index/hover cache — the pure, binding-shared store (#101, A2.2). A
//! pure-Rust consumer (the clap CLI, #B.4) drives it in-process, and language bindings reach it
//! across the boundary via the C ABI's `cache_*` handlers (`c_abi.rs`); `StoreError` maps to the
//! envelope `error_type` the binding re-raises as the matching exception (the retired pyo3
//! `#[pyclass] SqliteCacheStore` skin was deleted with the `python` feature in #B.6).
//!
//! Stateful port of python `cache/sqlite_store.py`. Values are gzip BLOBs (parity with the Python
//! `gzip.compress(..., 6)` format — interoperable with existing cache DBs). Table names in dynamic
//! SQL come ONLY from the allowlisted constants below, never from caller input (the #88 B608 control).

use std::io::{Read, Write};
use std::sync::Mutex;

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use rusqlite::Connection;

/// A cache-store failure. `Value` maps to the pyo3 `ValueError` the wrappers raised for bad
/// input (unknown table / bad limit / corrupt gzip); `Db` to `RuntimeError` for I/O and SQL.
#[derive(Debug)]
pub enum StoreError {
    Db(String),
    Value(String),
}

const CACHE_SCHEMA_VERSION: i64 = 1;

const CACHE_SCHEMA: &str = "\
PRAGMA journal_mode = WAL;\n\
PRAGMA synchronous  = NORMAL;\n\
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);\n\
CREATE TABLE IF NOT EXISTS parse_cache (key TEXT PRIMARY KEY, grammar_id TEXT NOT NULL DEFAULT '', result BLOB NOT NULL, size_bytes INTEGER NOT NULL, created_at INTEGER NOT NULL);\n\
CREATE INDEX IF NOT EXISTS parse_cache_created ON parse_cache (created_at);\n\
CREATE TABLE IF NOT EXISTS diff_cache (key TEXT PRIMARY KEY, language TEXT NOT NULL DEFAULT '', old_filename TEXT NOT NULL DEFAULT '', new_filename TEXT NOT NULL DEFAULT '', result BLOB NOT NULL, size_bytes INTEGER NOT NULL, created_at INTEGER NOT NULL);\n\
CREATE INDEX IF NOT EXISTS diff_cache_created ON diff_cache (created_at);\n\
CREATE TABLE IF NOT EXISTS symbol_index_cache (cache_key TEXT PRIMARY KEY, symbols_bin BLOB NOT NULL, refs_bin BLOB NOT NULL, file_count INTEGER NOT NULL DEFAULT 0, size_bytes INTEGER NOT NULL, created_at INTEGER NOT NULL);\n\
CREATE INDEX IF NOT EXISTS symbol_index_created ON symbol_index_cache (created_at);\n\
CREATE TABLE IF NOT EXISTS hover_map_cache (key TEXT PRIMARY KEY, result BLOB NOT NULL, size_bytes INTEGER NOT NULL, created_at INTEGER NOT NULL);\n\
CREATE INDEX IF NOT EXISTS hover_map_created ON hover_map_cache (created_at);\n\
CREATE TABLE IF NOT EXISTS cache_metrics (table_name TEXT PRIMARY KEY, hits INTEGER NOT NULL DEFAULT 0, misses INTEGER NOT NULL DEFAULT 0, last_hit_at INTEGER, last_miss_at INTEGER);\n";

/// (key column, non-blob metadata columns) per listable table. The ONLY source of
/// table/column identifiers interpolated into dynamic SQL (python `_LISTABLE_TABLES`).
fn listable_table(table: &str) -> Option<(&'static str, &'static [&'static str])> {
    match table {
        "parse_cache" => Some(("key", &["grammar_id", "size_bytes", "created_at"])),
        "diff_cache" => Some((
            "key",
            &["language", "old_filename", "new_filename", "size_bytes", "created_at"],
        )),
        "symbol_index_cache" => Some(("cache_key", &["file_count", "size_bytes", "created_at"])),
        "hover_map_cache" => Some(("key", &["size_bytes", "created_at"])),
        _ => None,
    }
}

fn now_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn gzip_compress(data: &str) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::new(6));
    let _ = encoder.write_all(data.as_bytes());
    encoder.finish().unwrap_or_default()
}

fn gzip_decompress(data: &[u8]) -> Result<String, StoreError> {
    let mut decoder = GzDecoder::new(data);
    let mut out = String::new();
    decoder
        .read_to_string(&mut out)
        .map_err(|e| StoreError::Value(format!("gzip decompress: {e}")))?;
    Ok(out)
}

fn db_err<E: std::fmt::Display>(e: E) -> StoreError {
    StoreError::Db(e.to_string())
}

/// Disk-persistent SQLite cache for parse trees / diffs / symbol indexes / hover maps.
/// Bindings drive it over the stateless `cache_*` C ABI (the `cache_registry` warm-store layer);
/// the Python `SqliteCacheStore(CacheStore)` delegates through `rust_core.py`'s `_RustCacheStore`
/// shim. The deterministic key methods live on the Python base / `cache_keys`.
pub struct SqliteStore {
    conn: Mutex<Option<Connection>>,
    ttl_seconds: i64,
    max_bytes: i64,
}

impl SqliteStore {
    fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T, StoreError>) -> Result<T, StoreError> {
        let guard = self
            .conn
            .lock()
            .map_err(|_| db_err("cache connection lock poisoned"))?;
        match guard.as_ref() {
            Some(conn) => f(conn),
            None => Err(db_err("cache store is closed")),
        }
    }

    fn bump_metric(conn: &Connection, table: &str, hit: bool) -> Result<(), StoreError> {
        let now = now_epoch();
        conn.execute(
            "INSERT INTO cache_metrics (table_name, hits, misses, last_hit_at, last_miss_at) \
             VALUES (?1, ?2, ?3, ?4, ?5) \
             ON CONFLICT(table_name) DO UPDATE SET \
               hits = hits + excluded.hits, misses = misses + excluded.misses, \
               last_hit_at = CASE WHEN excluded.last_hit_at IS NOT NULL THEN excluded.last_hit_at ELSE last_hit_at END, \
               last_miss_at = CASE WHEN excluded.last_miss_at IS NOT NULL THEN excluded.last_miss_at ELSE last_miss_at END",
            rusqlite::params![
                table,
                if hit { 1 } else { 0 },
                if hit { 0 } else { 1 },
                if hit { Some(now) } else { None },
                if hit { None } else { Some(now) },
            ],
        )
        .map_err(db_err)?;
        Ok(())
    }

    fn evict(&self, conn: &Connection) -> Result<(), StoreError> {
        let cutoff = now_epoch() - self.ttl_seconds;
        for table in ["parse_cache", "diff_cache", "symbol_index_cache", "hover_map_cache"] {
            // table is a fixed literal — never caller input (B608).
            conn.execute(
                &format!("DELETE FROM {table} WHERE created_at < ?1"),
                rusqlite::params![cutoff],
            )
            .map_err(db_err)?;
        }
        // Size-based eviction: drop oldest ~10% per table when over the cap.
        let sized = ["parse_cache", "diff_cache", "hover_map_cache"];
        let mut total: i64 = 0;
        for table in sized {
            let n: i64 = conn
                .query_row(
                    &format!("SELECT COALESCE(SUM(size_bytes), 0) FROM {table}"),
                    [],
                    |r| r.get(0),
                )
                .map_err(db_err)?;
            total += n;
        }
        if total > self.max_bytes {
            let target_per_table = (total as f64 * 0.10) as i64;
            for table in sized {
                let mut removed: i64 = 0;
                let rows: Vec<(String, i64)> = {
                    let mut stmt = conn
                        .prepare(&format!(
                            "SELECT key, size_bytes FROM {table} ORDER BY created_at ASC"
                        ))
                        .map_err(db_err)?;
                    let mapped = stmt
                        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
                        .map_err(db_err)?;
                    mapped.collect::<rusqlite::Result<Vec<_>>>().map_err(db_err)?
                };
                for (key, size) in rows {
                    if removed >= target_per_table {
                        break;
                    }
                    conn.execute(
                        &format!("DELETE FROM {table} WHERE key = ?1"),
                        rusqlite::params![key],
                    )
                    .map_err(db_err)?;
                    removed += size;
                }
            }
        }
        Ok(())
    }

    pub fn open(path: &str, ttl_days: i64, max_mb: i64) -> Result<Self, StoreError> {
        if let Some(parent) = std::path::Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(db_err)?;
            }
        }
        let conn = Connection::open(path).map_err(db_err)?;
        conn.execute_batch(CACHE_SCHEMA).map_err(db_err)?;
        // schema-version handling (purge on mismatch).
        let stored: Option<String> = conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |r| r.get(0),
            )
            .ok();
        match stored {
            None => {
                conn.execute(
                    "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)",
                    rusqlite::params![CACHE_SCHEMA_VERSION.to_string()],
                )
                .map_err(db_err)?;
            }
            Some(v) if v.parse::<i64>().ok() != Some(CACHE_SCHEMA_VERSION) => {
                conn.execute_batch(
                    "DROP TABLE IF EXISTS parse_cache; DROP TABLE IF EXISTS diff_cache; \
                     DROP TABLE IF EXISTS symbol_index_cache; DROP TABLE IF EXISTS hover_map_cache;",
                )
                .map_err(db_err)?;
                conn.execute_batch(CACHE_SCHEMA).map_err(db_err)?;
                conn.execute(
                    "UPDATE meta SET value = ?1 WHERE key = 'schema_version'",
                    rusqlite::params![CACHE_SCHEMA_VERSION.to_string()],
                )
                .map_err(db_err)?;
            }
            Some(_) => {}
        }
        let store = Self {
            conn: Mutex::new(Some(conn)),
            ttl_seconds: ttl_days * 86_400,
            max_bytes: max_mb * 1024 * 1024,
        };
        store.with_conn(|conn| store.evict(conn))?;
        Ok(store)
    }

    pub fn get_parse(&self, key: &str) -> Result<Option<String>, StoreError> {
        self.with_conn(|conn| {
            let blob: Option<Vec<u8>> = conn
                .query_row(
                    "SELECT result FROM parse_cache WHERE key = ?1",
                    rusqlite::params![key],
                    |r| r.get(0),
                )
                .ok();
            match blob {
                Some(bytes) => {
                    Self::bump_metric(conn, "parse_cache", true)?;
                    Ok(Some(gzip_decompress(&bytes)?))
                }
                None => {
                    Self::bump_metric(conn, "parse_cache", false)?;
                    Ok(None)
                }
            }
        })
    }

    pub fn put_parse(&self, key: &str, value: &str, grammar_id: &str) -> Result<(), StoreError> {
        let compressed = gzip_compress(value);
        let size = compressed.len() as i64;
        self.with_conn(|conn| {
            conn.execute(
                "INSERT OR REPLACE INTO parse_cache (key, grammar_id, result, size_bytes, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![key, grammar_id, compressed, size, now_epoch()],
            )
            .map_err(db_err)?;
            Ok(())
        })
    }

    pub fn get_diff(&self, key: &str) -> Result<Option<String>, StoreError> {
        self.with_conn(|conn| {
            let blob: Option<Vec<u8>> = conn
                .query_row(
                    "SELECT result FROM diff_cache WHERE key = ?1",
                    rusqlite::params![key],
                    |r| r.get(0),
                )
                .ok();
            match blob {
                Some(bytes) => {
                    Self::bump_metric(conn, "diff_cache", true)?;
                    Ok(Some(gzip_decompress(&bytes)?))
                }
                None => {
                    Self::bump_metric(conn, "diff_cache", false)?;
                    Ok(None)
                }
            }
        })
    }

    pub fn put_diff(
        &self,
        key: &str,
        value: &str,
        language: &str,
        old_filename: &str,
        new_filename: &str,
    ) -> Result<(), StoreError> {
        let compressed = gzip_compress(value);
        let size = compressed.len() as i64;
        self.with_conn(|conn| {
            conn.execute(
                "INSERT OR REPLACE INTO diff_cache (key, language, old_filename, new_filename, result, size_bytes, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![key, language, old_filename, new_filename, compressed, size, now_epoch()],
            )
            .map_err(db_err)?;
            Ok(())
        })
    }

    pub fn get_symbol_index(&self, cache_key: &str) -> Result<Option<(String, String)>, StoreError> {
        self.with_conn(|conn| {
            let row: Option<(Vec<u8>, Vec<u8>)> = conn
                .query_row(
                    "SELECT symbols_bin, refs_bin FROM symbol_index_cache WHERE cache_key = ?1",
                    rusqlite::params![cache_key],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .ok();
            match row {
                Some((symbols, refs)) => {
                    Self::bump_metric(conn, "symbol_index_cache", true)?;
                    Ok(Some((gzip_decompress(&symbols)?, gzip_decompress(&refs)?)))
                }
                None => {
                    Self::bump_metric(conn, "symbol_index_cache", false)?;
                    Ok(None)
                }
            }
        })
    }

    pub fn put_symbol_index(
        &self,
        cache_key: &str,
        symbols_json: &str,
        refs_json: &str,
        file_count: i64,
    ) -> Result<(), StoreError> {
        let symbols_bin = gzip_compress(symbols_json);
        let refs_bin = gzip_compress(refs_json);
        let size = (symbols_bin.len() + refs_bin.len()) as i64;
        self.with_conn(|conn| {
            conn.execute(
                "INSERT OR REPLACE INTO symbol_index_cache (cache_key, symbols_bin, refs_bin, file_count, size_bytes, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![cache_key, symbols_bin, refs_bin, file_count, size, now_epoch()],
            )
            .map_err(db_err)?;
            Ok(())
        })
    }

    /// Returns the hover map as a JSON object string (the Python delegator json-loads it).
    pub fn get_hover_map(&self, key: &str) -> Result<Option<String>, StoreError> {
        self.with_conn(|conn| {
            let blob: Option<Vec<u8>> = conn
                .query_row(
                    "SELECT result FROM hover_map_cache WHERE key = ?1",
                    rusqlite::params![key],
                    |r| r.get(0),
                )
                .ok();
            match blob {
                Some(bytes) => {
                    Self::bump_metric(conn, "hover_map_cache", true)?;
                    Ok(Some(gzip_decompress(&bytes)?))
                }
                None => {
                    Self::bump_metric(conn, "hover_map_cache", false)?;
                    Ok(None)
                }
            }
        })
    }

    /// *value_json* is the JSON object string of the hover map (the delegator json-dumps it).
    pub fn put_hover_map(&self, key: &str, value_json: &str) -> Result<(), StoreError> {
        let compressed = gzip_compress(value_json);
        let size = compressed.len() as i64;
        self.with_conn(|conn| {
            conn.execute(
                "INSERT OR REPLACE INTO hover_map_cache (key, result, size_bytes, created_at) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![key, compressed, size, now_epoch()],
            )
            .map_err(db_err)?;
            Ok(())
        })
    }

    /// Per-table row counts / sizes / oldest / newest as a JSON object string.
    pub fn stats(&self) -> Result<String, StoreError> {
        self.with_conn(|conn| {
            let mut out = serde_json::Map::new();
            let ttl_days = self.ttl_seconds / 86_400;
            for table in ["parse_cache", "diff_cache", "symbol_index_cache", "hover_map_cache"] {
                let row = conn.query_row(
                    &format!(
                        "SELECT COUNT(*), COALESCE(SUM(size_bytes),0), MIN(created_at), MAX(created_at) FROM {table}"
                    ),
                    [],
                    |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            r.get::<_, i64>(1)?,
                            r.get::<_, Option<i64>>(2)?,
                            r.get::<_, Option<i64>>(3)?,
                        ))
                    },
                );
                let (count, size, oldest, newest) = row.unwrap_or((0, 0, None, None));
                out.insert(
                    table.to_string(),
                    serde_json::json!({
                        "count": count,
                        "size_bytes": size,
                        "oldest": oldest,
                        "newest": newest,
                        "ttl_days": ttl_days,
                    }),
                );
            }
            serde_json::to_string(&out).map_err(db_err)
        })
    }

    /// Per-table hit/miss counters as a JSON object string.
    pub fn metrics(&self) -> Result<String, StoreError> {
        self.with_conn(|conn| {
            let mut out = serde_json::Map::new();
            for table in ["parse_cache", "diff_cache", "symbol_index_cache", "hover_map_cache"] {
                let row = conn
                    .query_row(
                        "SELECT hits, misses, last_hit_at, last_miss_at FROM cache_metrics WHERE table_name = ?1",
                        rusqlite::params![table],
                        |r| {
                            Ok((
                                r.get::<_, i64>(0)?,
                                r.get::<_, i64>(1)?,
                                r.get::<_, Option<i64>>(2)?,
                                r.get::<_, Option<i64>>(3)?,
                            ))
                        },
                    )
                    .ok();
                let value = match row {
                    Some((hits, misses, last_hit, last_miss)) => {
                        let total = hits + misses;
                        serde_json::json!({
                            "hits": hits,
                            "misses": misses,
                            "hit_rate_pct": if total > 0 { hits as f64 / total as f64 * 100.0 } else { 0.0 },
                            "last_hit_at": last_hit,
                            "last_miss_at": last_miss,
                        })
                    }
                    None => serde_json::json!({
                        "hits": 0, "misses": 0, "hit_rate_pct": 0.0,
                        "last_hit_at": serde_json::Value::Null, "last_miss_at": serde_json::Value::Null,
                    }),
                };
                out.insert(table.to_string(), value);
            }
            serde_json::to_string(&out).map_err(db_err)
        })
    }

    /// Metadata rows (no BLOBs) as a JSON array string. Filters mirror the Python method;
    /// the file_glob filter is applied in the Python delegator (fnmatch), so this returns
    /// up to `limit*10` rows when a glob is requested.
    #[allow(clippy::too_many_arguments)]
    pub fn list_entries(
        &self,
        table: &str,
        language: Option<&str>,
        since: Option<i64>,
        before: Option<i64>,
        min_size: Option<i64>,
        max_size: Option<i64>,
        limit: i64,
        with_glob: bool,
    ) -> Result<String, StoreError> {
        let (key_col, meta_cols) = listable_table(table)
            .ok_or_else(|| StoreError::Value(format!("Unknown table {table:?}")))?;
        if limit < 1 {
            return Err(StoreError::Value("limit must be a positive integer".to_string()));
        }
        let mut conditions: Vec<String> = Vec::new();
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        if let Some(s) = since {
            conditions.push("created_at >= ?".into());
            params.push(Box::new(s));
        }
        if let Some(b) = before {
            conditions.push("created_at <= ?".into());
            params.push(Box::new(b));
        }
        if let Some(m) = min_size {
            conditions.push("size_bytes >= ?".into());
            params.push(Box::new(m));
        }
        if let Some(m) = max_size {
            conditions.push("size_bytes <= ?".into());
            params.push(Box::new(m));
        }
        if let (Some(lang), "diff_cache") = (language, table) {
            conditions.push("LOWER(language) = LOWER(?)".into());
            params.push(Box::new(lang.to_string()));
        }
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        let fetch_limit = if with_glob { limit * 10 } else { limit };
        let select_cols = std::iter::once(key_col)
            .chain(meta_cols.iter().copied())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT {select_cols} FROM {table} {where_clause} ORDER BY created_at DESC LIMIT {fetch_limit}"
        );
        let now = now_epoch();
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(&sql).map_err(db_err)?;
            let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                params.iter().map(|p| p.as_ref()).collect();
            let cols: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
            let rows = stmt
                .query_map(param_refs.as_slice(), |row| {
                    let mut map = serde_json::Map::new();
                    for (i, col) in cols.iter().enumerate() {
                        map.insert(col.clone(), sqlite_value_to_json(row, i));
                    }
                    Ok(map)
                })
                .map_err(db_err)?;
            let mut result: Vec<serde_json::Value> = Vec::new();
            for row in rows {
                let mut map = row.map_err(db_err)?;
                let created = map.get("created_at").and_then(|v| v.as_i64()).unwrap_or(0);
                map.insert("age_seconds".into(), serde_json::json!(now - created));
                map.insert(
                    "expires_in_seconds".into(),
                    serde_json::json!(self.ttl_seconds - (now - created)),
                );
                if key_col != "key" {
                    if let Some(v) = map.remove(key_col) {
                        map.insert("key".into(), v);
                    }
                }
                result.push(serde_json::Value::Object(map));
            }
            serde_json::to_string(&result).map_err(db_err)
        })
    }

    /// Non-BLOB columns for one entry as a JSON object string, or None.
    pub fn get_entry_metadata(&self, key: &str, table: &str) -> Result<Option<String>, StoreError> {
        let (key_col, meta_cols) = listable_table(table)
            .ok_or_else(|| StoreError::Value(format!("Unknown table {table:?}")))?;
        let select_cols = std::iter::once(key_col)
            .chain(meta_cols.iter().copied())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!("SELECT {select_cols} FROM {table} WHERE {key_col} = ?1");
        let now = now_epoch();
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(&sql).map_err(db_err)?;
            let cols: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
            let row = stmt
                .query_row(rusqlite::params![key], |row| {
                    let mut map = serde_json::Map::new();
                    for (i, col) in cols.iter().enumerate() {
                        map.insert(col.clone(), sqlite_value_to_json(row, i));
                    }
                    Ok(map)
                })
                .ok();
            match row {
                Some(mut map) => {
                    let created = map.get("created_at").and_then(|v| v.as_i64()).unwrap_or(0);
                    map.insert("age_seconds".into(), serde_json::json!(now - created));
                    map.insert(
                        "expires_in_seconds".into(),
                        serde_json::json!(self.ttl_seconds - (now - created)),
                    );
                    if key_col != "key" {
                        if let Some(v) = map.remove(key_col) {
                            map.insert("key".into(), v);
                        }
                    }
                    Ok(Some(
                        serde_json::to_string(&serde_json::Value::Object(map)).map_err(db_err)?,
                    ))
                }
                None => Ok(None),
            }
        })
    }

    /// Decompressed JSON payload for one entry (symbol_index → {"symbols","refs"}), or None.
    pub fn get_entry_payload(&self, key: &str, table: &str) -> Result<Option<String>, StoreError> {
        self.with_conn(|conn| match table {
            "parse_cache" | "diff_cache" | "hover_map_cache" => {
                let blob: Option<Vec<u8>> = conn
                    .query_row(
                        &format!("SELECT result FROM {table} WHERE key = ?1"),
                        rusqlite::params![key],
                        |r| r.get(0),
                    )
                    .ok();
                match blob {
                    Some(bytes) => Ok(Some(gzip_decompress(&bytes)?)),
                    None => Ok(None),
                }
            }
            "symbol_index_cache" => {
                let row: Option<(Vec<u8>, Vec<u8>)> = conn
                    .query_row(
                        "SELECT symbols_bin, refs_bin FROM symbol_index_cache WHERE cache_key = ?1",
                        rusqlite::params![key],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .ok();
                match row {
                    Some((symbols, refs)) => {
                        let symbols: serde_json::Value =
                            serde_json::from_str(&gzip_decompress(&symbols)?).map_err(db_err)?;
                        let refs: serde_json::Value =
                            serde_json::from_str(&gzip_decompress(&refs)?).map_err(db_err)?;
                        Ok(Some(
                            serde_json::json!({"symbols": symbols, "refs": refs}).to_string(),
                        ))
                    }
                    None => Ok(None),
                }
            }
            _ => Err(StoreError::Value(format!("Unknown table {table:?}"))),
        })
    }

    /// Every entry as a JSON array of {table, key, ...metadata, payload} objects (the
    /// Python delegator yields from it). Non-streaming, matching the export use case.
    pub fn export_entries(&self, table: &str) -> Result<String, StoreError> {
        let (key_col, _) = listable_table(table)
            .ok_or_else(|| StoreError::Value(format!("Unknown table {table:?}")))?;
        // Collect keys first (payload lookups reuse the same connection).
        let keys: Vec<String> = self.with_conn(|conn| {
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {key_col} FROM {table} ORDER BY created_at DESC"
                ))
                .map_err(db_err)?;
            let rows = stmt
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(db_err)?;
            rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db_err)
        })?;
        let mut out: Vec<serde_json::Value> = Vec::new();
        for key in keys {
            let meta = self.get_entry_metadata(&key, table)?;
            let payload = self.get_entry_payload(&key, table)?;
            let mut obj: serde_json::Map<String, serde_json::Value> = match meta {
                Some(m) => serde_json::from_str(&m).map_err(db_err)?,
                None => serde_json::Map::new(),
            };
            obj.remove("age_seconds");
            obj.remove("expires_in_seconds");
            obj.insert("table".into(), serde_json::json!(table));
            obj.insert(
                "payload".into(),
                match payload {
                    Some(p) => serde_json::from_str(&p).unwrap_or(serde_json::Value::Null),
                    None => serde_json::Value::Null,
                },
            );
            out.push(serde_json::Value::Object(obj));
        }
        serde_json::to_string(&out).map_err(db_err)
    }

    pub fn clear(&self, parse: bool, diff: bool, index: bool, hover: bool) -> Result<(), StoreError> {
        self.with_conn(|conn| {
            if parse {
                conn.execute("DELETE FROM parse_cache", []).map_err(db_err)?;
            }
            if diff {
                conn.execute("DELETE FROM diff_cache", []).map_err(db_err)?;
            }
            if index {
                conn.execute("DELETE FROM symbol_index_cache", [])
                    .map_err(db_err)?;
            }
            if hover {
                conn.execute("DELETE FROM hover_map_cache", [])
                    .map_err(db_err)?;
            }
            Ok(())
        })
    }

    pub fn close(&self) -> Result<(), StoreError> {
        if let Ok(mut guard) = self.conn.lock() {
            guard.take(); // drop the Connection
        }
        Ok(())
    }
}

/// Convert a single SQLite column to a JSON value (int/float/text/null; BLOBs excluded
/// from the metadata SELECTs).
fn sqlite_value_to_json(row: &rusqlite::Row, idx: usize) -> serde_json::Value {
    use rusqlite::types::ValueRef;
    match row.get_ref(idx) {
        Ok(ValueRef::Null) => serde_json::Value::Null,
        Ok(ValueRef::Integer(i)) => serde_json::json!(i),
        Ok(ValueRef::Real(f)) => serde_json::json!(f),
        Ok(ValueRef::Text(t)) => {
            serde_json::Value::String(String::from_utf8_lossy(t).into_owned())
        }
        Ok(ValueRef::Blob(_)) | Err(_) => serde_json::Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_diff_hover_roundtrip_and_stats() {
        let dir = std::env::temp_dir().join(format!("idf_cachestore_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let db = dir.join("c.db");
        let store = SqliteStore::open(db.to_str().unwrap(), 30, 500).expect("open");

        assert!(store.get_parse("k").expect("get").is_none());
        store.put_parse("k", "PARSED", "python").expect("put");
        assert_eq!(store.get_parse("k").expect("get").as_deref(), Some("PARSED"));

        store.put_diff("d", "DIFF", "python", "a.py", "b.py").expect("put diff");
        assert_eq!(store.get_diff("d").expect("get diff").as_deref(), Some("DIFF"));

        store.put_hover_map("h", "{\"x\":\"int\"}").expect("put hover");
        assert_eq!(store.get_hover_map("h").expect("get hover").as_deref(), Some("{\"x\":\"int\"}"));

        // stats is a JSON object keyed by table; parse_cache has one row.
        let stats: serde_json::Value = serde_json::from_str(&store.stats().expect("stats")).unwrap();
        assert_eq!(stats["parse_cache"]["count"], 1);

        // Unknown table is a Value error (maps to ValueError under python).
        assert!(matches!(store.list_entries("nope", None, None, None, None, None, 5, false), Err(StoreError::Value(_))));

        store.clear(true, true, true, true).expect("clear");
        assert!(store.get_parse("k").expect("get").is_none());
        store.close().expect("close");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
