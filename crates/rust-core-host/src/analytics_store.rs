//! Analytics / telemetry store (#101, A2.2) — the pure, binding-shared store. A pure-Rust
//! consumer (the clap CLI, #B.4) drives it in-process, and language bindings reach it across the
//! boundary via the C ABI's `analytics_*` handlers (`c_abi.rs`). Errors reuse
//! `cache_store::StoreError` → the envelope `error_type` (the retired pyo3 `#[pyclass]
//! AnalyticsStore` skin was deleted with the `python` feature in #B.6).
//!
//! Port of python `cache/duckdb_store.py`. Storage engine is runtime-selected: the "provided"
//! DuckDB (dlopen of a configurable `libduckdb` — `duckdb_ffi`) when available, else the bundled
//! SQLite. The schema + queries use an engine-agnostic SQL subset so both backends run the same
//! statements. NOTE: a DuckDB file and a SQLite file are not interchangeable (append-only telemetry).

use std::sync::Mutex;

use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::cache_store::StoreError;

// Engine-agnostic DDL: VARCHAR/BIGINT/INTEGER/DOUBLE/BOOLEAN are native in DuckDB and map
// via affinity in SQLite; `recorded_at` is a BIGINT epoch (minted in Rust) not TIMESTAMP,
// and JSON payloads are plain VARCHAR (no JSON-path ops are used).
const ANALYTICS_SCHEMA: &str = "\
CREATE TABLE IF NOT EXISTS diff_history (id VARCHAR PRIMARY KEY, recorded_at BIGINT NOT NULL, old_filename VARCHAR NOT NULL, new_filename VARCHAR NOT NULL, language VARCHAR NOT NULL, has_semantic_changes BOOLEAN NOT NULL, is_style_only BOOLEAN NOT NULL, change_count INTEGER NOT NULL, diff_json VARCHAR NOT NULL);\
CREATE TABLE IF NOT EXISTS diagnostic_runs (id VARCHAR PRIMARY KEY, recorded_at BIGINT NOT NULL, command VARCHAR NOT NULL, repo VARCHAR NOT NULL, argv_json VARCHAR NOT NULL, diff_count INTEGER NOT NULL, total_fuel BIGINT NOT NULL, peak_fuel BIGINT NOT NULL, hotspot_count INTEGER NOT NULL);\
CREATE TABLE IF NOT EXISTS diagnostic_files (id VARCHAR PRIMARY KEY, run_id VARCHAR NOT NULL, old_filename VARCHAR NOT NULL, new_filename VARCHAR NOT NULL, language VARCHAR NOT NULL, staging_status VARCHAR, file_lifecycle VARCHAR, has_semantic_changes BOOLEAN NOT NULL, is_style_only BOOLEAN NOT NULL, is_fallback BOOLEAN NOT NULL, change_count INTEGER NOT NULL, parse_error_count INTEGER NOT NULL, peak_fuel BIGINT NOT NULL, total_fuel BIGINT NOT NULL, hotspot_count INTEGER NOT NULL, diff_json VARCHAR NOT NULL);\
CREATE TABLE IF NOT EXISTS diagnostic_parser_calls (id VARCHAR PRIMARY KEY, run_id VARCHAR NOT NULL, file_id VARCHAR NOT NULL, plugin VARCHAR NOT NULL, function_name VARCHAR NOT NULL, language VARCHAR, filename VARCHAR, provenance VARCHAR, engine VARCHAR, trusted BOOLEAN NOT NULL, status VARCHAR, call_count INTEGER NOT NULL, elapsed_ms DOUBLE, fuel_consumed BIGINT NOT NULL, total_fuel_consumed BIGINT NOT NULL, fuel_budget BIGINT, fuel_used_percent DOUBLE, input_bytes BIGINT, input_lines BIGINT, fuel_per_kb DOUBLE, fuel_per_line DOUBLE);\
CREATE TABLE IF NOT EXISTS diagnostic_hotspots (id VARCHAR PRIMARY KEY, run_id VARCHAR NOT NULL, file_id VARCHAR NOT NULL, plugin VARCHAR, function_name VARCHAR, language VARCHAR, filename VARCHAR, fuel_consumed BIGINT NOT NULL, fuel_budget BIGINT, fuel_used_percent DOUBLE, input_bytes BIGINT, input_lines BIGINT, fuel_per_kb DOUBLE, fuel_per_line DOUBLE, thresholds_json VARCHAR NOT NULL);\
CREATE TABLE IF NOT EXISTS diagnostic_events (id VARCHAR PRIMARY KEY, run_id VARCHAR NOT NULL, file_id VARCHAR NOT NULL, stage VARCHAR, action VARCHAR, rule_id VARCHAR, reason VARCHAR, metadata_json VARCHAR NOT NULL);";

// ── Backend-agnostic bound parameter ────────────────────────────────────────

pub(crate) enum Param {
    Text(String),
    OptText(Option<String>),
    Int(i64),
    OptInt(Option<i64>),
    Float(Option<f64>),
    Bool(bool),
}

impl rusqlite::ToSql for Param {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        use rusqlite::types::{ToSqlOutput, Value as SqlValue, ValueRef};
        Ok(match self {
            Param::Text(s) => ToSqlOutput::Borrowed(ValueRef::Text(s.as_bytes())),
            Param::OptText(Some(s)) => ToSqlOutput::Borrowed(ValueRef::Text(s.as_bytes())),
            Param::OptText(None) => ToSqlOutput::Owned(SqlValue::Null),
            Param::Int(i) => ToSqlOutput::Owned(SqlValue::Integer(*i)),
            Param::OptInt(Some(i)) => ToSqlOutput::Owned(SqlValue::Integer(*i)),
            Param::OptInt(None) => ToSqlOutput::Owned(SqlValue::Null),
            Param::Float(Some(f)) => ToSqlOutput::Owned(SqlValue::Real(*f)),
            Param::Float(None) => ToSqlOutput::Owned(SqlValue::Null),
            Param::Bool(b) => ToSqlOutput::Owned(SqlValue::Integer(i64::from(*b))),
        })
    }
}

// ── Storage backend (SQLite now; provided-DuckDB dlopen slots in as a variant) ──

enum Backend {
    DuckDb(Mutex<crate::duckdb_ffi::DuckDbHandle>),
    Sqlite(Mutex<rusqlite::Connection>),
}

impl Backend {
    fn name(&self) -> &'static str {
        match self {
            Backend::DuckDb(_) => "duckdb",
            Backend::Sqlite(_) => "sqlite",
        }
    }

    fn execute(&self, sql: &str, params: &[Param]) -> Result<(), StoreError> {
        match self {
            Backend::DuckDb(handle) => {
                let handle = handle.lock().map_err(|_| db_err("analytics lock poisoned"))?;
                handle.execute(sql, params).map_err(db_err)
            }
            Backend::Sqlite(conn) => {
                let conn = conn.lock().map_err(|_| db_err("analytics lock poisoned"))?;
                conn.execute(sql, rusqlite::params_from_iter(params.iter()))
                    .map_err(db_err)?;
                Ok(())
            }
        }
    }

    /// Run *sql* and return rows as a JSON array of column→value objects.
    fn query(&self, sql: &str) -> Result<String, StoreError> {
        match self {
            Backend::DuckDb(handle) => {
                let handle = handle.lock().map_err(|_| db_err("analytics lock poisoned"))?;
                handle.query(sql).map_err(db_err)
            }
            Backend::Sqlite(conn) => {
                let conn = conn.lock().map_err(|_| db_err("analytics lock poisoned"))?;
                let mut stmt = conn.prepare(sql).map_err(db_err)?;
                let names: Vec<String> =
                    stmt.column_names().iter().map(|s| s.to_string()).collect();
                let mut out: Vec<Value> = Vec::new();
                let mut rows = stmt.query([]).map_err(db_err)?;
                while let Some(row) = rows.next().map_err(db_err)? {
                    let mut map = Map::new();
                    for (i, name) in names.iter().enumerate() {
                        map.insert(name.clone(), sqlite_value_to_json(row, i));
                    }
                    out.push(Value::Object(map));
                }
                serde_json::to_string(&out).map_err(db_err)
            }
        }
    }
}

/// Pick the storage engine: the "provided" DuckDB (dlopen a configurable libduckdb) when
/// available, else the always-present bundled SQLite.
fn select_backend(path: &str) -> Result<Backend, StoreError> {
    // Prefer the "provided" DuckDB (dlopen a configurable libduckdb); if none is
    // available/loadable, fall back to the always-present bundled SQLite.
    if let Some(handle) = crate::duckdb_ffi::try_open(path) {
        if handle.execute_batch(ANALYTICS_SCHEMA).is_ok() {
            return Ok(Backend::DuckDb(Mutex::new(handle)));
        }
    }
    let conn = rusqlite::Connection::open(path).map_err(db_err)?;
    conn.execute_batch(ANALYTICS_SCHEMA).map_err(db_err)?;
    Ok(Backend::Sqlite(Mutex::new(conn)))
}

// ── JSON-normalization helpers (python module-level _records/_int/_string/...) ──

fn obj(value: Option<&Value>) -> Map<String, Value> {
    match value {
        Some(Value::Object(m)) => m.clone(),
        _ => Map::new(),
    }
}

fn records(value: Option<&Value>) -> Vec<Map<String, Value>> {
    match value {
        Some(Value::Array(items)) => items.iter().filter_map(|v| v.as_object().cloned()).collect(),
        _ => Vec::new(),
    }
}

fn int_field(value: Option<&Value>, default: i64) -> i64 {
    match value {
        Some(v) if v.is_i64() => v.as_i64().unwrap_or(default),
        Some(v) if v.is_u64() => v.as_u64().map(|n| n as i64).unwrap_or(default),
        Some(v) if v.is_f64() => v.as_f64().map(|n| n as i64).unwrap_or(default),
        _ => default,
    }
}

fn opt_int_field(value: Option<&Value>) -> Option<i64> {
    match value {
        None | Some(Value::Null) => None,
        other => Some(int_field(other, 0)),
    }
}

fn float_field(value: Option<&Value>) -> Option<f64> {
    match value {
        Some(v) if v.is_number() => v.as_f64(),
        _ => None,
    }
}

fn string_field(value: Option<&Value>, default: &str) -> String {
    match value {
        Some(Value::String(s)) if !s.is_empty() => s.clone(),
        _ => default.to_string(),
    }
}

fn opt_string(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(s)) => Some(s.clone()),
        _ => None,
    }
}

fn status_summary(value: Option<&Value>) -> String {
    match value {
        Some(Value::Object(m)) => m
            .iter()
            .map(|(k, v)| format!("{k}:{}", v.as_i64().unwrap_or(0)))
            .collect::<Vec<_>>()
            .join(", "),
        _ => String::new(),
    }
}

fn fuel_per_kb(fuel: i64, input_bytes: Option<i64>) -> f64 {
    fuel as f64 / ((input_bytes.unwrap_or(0) as f64 / 1024.0).max(1.0))
}

fn fuel_per_line(fuel: i64, input_lines: Option<i64>) -> f64 {
    fuel as f64 / (input_lines.unwrap_or(0).max(1) as f64)
}

/// python `_file_fuel_summary` → (peak_fuel, total_fuel, hotspot_count).
fn file_fuel_summary(diff: &Value) -> (i64, i64, i64) {
    let telemetry = obj(obj(diff.get("metadata")).get("engine_telemetry"));
    let calls = records(telemetry.get("calls"));
    let hotspots = records(telemetry.get("fuel_hotspots"));
    let peak = calls
        .iter()
        .map(|c| int_field(c.get("fuel_consumed"), 0))
        .max()
        .unwrap_or(0);
    let total: i64 = calls
        .iter()
        .map(|c| {
            let fuel = int_field(c.get("fuel_consumed"), 0);
            int_field(c.get("total_fuel_consumed"), fuel)
        })
        .sum();
    (peak, total, hotspots.len() as i64)
}

fn now_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn uuid() -> String {
    Uuid::new_v4().to_string()
}

fn db_err<E: std::fmt::Display>(e: E) -> StoreError {
    StoreError::Db(e.to_string())
}

fn check_limit(limit: i64) -> Result<(), StoreError> {
    if limit < 1 {
        return Err(StoreError::Value("limit must be a positive integer".to_string()));
    }
    Ok(())
}

fn sqlite_value_to_json(row: &rusqlite::Row, idx: usize) -> Value {
    use rusqlite::types::ValueRef;
    match row.get_ref(idx) {
        Ok(ValueRef::Null) => Value::Null,
        Ok(ValueRef::Integer(i)) => json!(i),
        Ok(ValueRef::Real(f)) => json!(f),
        Ok(ValueRef::Text(t)) => Value::String(String::from_utf8_lossy(t).into_owned()),
        Ok(ValueRef::Blob(_)) | Err(_) => Value::Null,
    }
}

// ── Public store ─────────────────────────────────────────────────────────────

/// Append-only diff-history + fuel-diagnostics store (python `DuckDBAnalyticsStore`).
pub struct AnalyticsStore {
    backend: Backend,
    open: Mutex<bool>,
}

impl AnalyticsStore {
    pub fn open(path: &str) -> Result<Self, StoreError> {
        if let Some(parent) = std::path::Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(db_err)?;
            }
        }
        Ok(Self {
            backend: select_backend(path)?,
            open: Mutex::new(true),
        })
    }

    /// Active storage engine name ("sqlite" or "duckdb").
    pub fn backend_name(&self) -> &'static str {
        self.backend.name()
    }

    fn record_diff_into(&self, diff_json: &str) -> Result<(), StoreError> {
        let data: Value = match serde_json::from_str(diff_json) {
            Ok(v) => v,
            Err(_) => return Ok(()), // invalid JSON is silently skipped (python parity)
        };
        self.backend.execute(
            "INSERT INTO diff_history (id, recorded_at, old_filename, new_filename, language, has_semantic_changes, is_style_only, change_count, diff_json) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            &[
                Param::Text(uuid()),
                Param::Int(now_epoch()),
                Param::Text(string_field(data.get("old_filename"), "")),
                Param::Text(string_field(data.get("new_filename"), "")),
                Param::Text(string_field(data.get("language"), "")),
                Param::Bool(data.get("has_semantic_changes").and_then(Value::as_bool).unwrap_or(false)),
                Param::Bool(data.get("is_style_only").and_then(Value::as_bool).unwrap_or(false)),
                Param::Int(records(data.get("changes")).len() as i64),
                Param::Text(diff_json.to_string()),
            ],
        )
    }

    fn record_parser_calls(&self, run_id: &str, file_id: &str, telemetry: &Map<String, Value>) -> Result<(), StoreError> {
        for call in records(telemetry.get("calls")) {
            let fuel = int_field(call.get("fuel_consumed"), 0);
            let total_fuel = int_field(call.get("total_fuel_consumed"), fuel);
            let input_bytes = opt_int_field(call.get("input_bytes"));
            let input_lines = opt_int_field(call.get("input_lines"));
            let status = {
                let summary = status_summary(call.get("statuses"));
                if summary.is_empty() {
                    string_field(call.get("status"), "")
                } else {
                    summary
                }
            };
            self.backend.execute(
                "INSERT INTO diagnostic_parser_calls (id, run_id, file_id, plugin, function_name, language, filename, provenance, engine, trusted, status, call_count, elapsed_ms, fuel_consumed, total_fuel_consumed, fuel_budget, fuel_used_percent, input_bytes, input_lines, fuel_per_kb, fuel_per_line) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                &[
                    Param::Text(uuid()),
                    Param::Text(run_id.to_string()),
                    Param::Text(file_id.to_string()),
                    Param::Text(string_field(call.get("plugin"), "plugin")),
                    Param::Text(string_field(call.get("function"), "call")),
                    Param::OptText(opt_string(call.get("language"))),
                    Param::OptText(opt_string(call.get("filename"))),
                    Param::OptText(opt_string(call.get("provenance"))),
                    Param::OptText(opt_string(call.get("engine"))),
                    Param::Bool(call.get("trusted").and_then(Value::as_bool).unwrap_or(false)),
                    Param::Text(status),
                    Param::Int(int_field(call.get("call_count"), 1)),
                    Param::Float(float_field(call.get("elapsed_ms"))),
                    Param::Int(fuel),
                    Param::Int(total_fuel),
                    Param::OptInt(opt_int_field(call.get("fuel_budget"))),
                    Param::Float(float_field(call.get("max_fuel_used_percent")).or_else(|| float_field(call.get("fuel_used_percent")))),
                    Param::OptInt(input_bytes),
                    Param::OptInt(input_lines),
                    Param::Float(Some(fuel_per_kb(fuel, input_bytes))),
                    Param::Float(Some(fuel_per_line(fuel, input_lines))),
                ],
            )?;
        }
        Ok(())
    }

    fn record_hotspots(&self, run_id: &str, file_id: &str, telemetry: &Map<String, Value>) -> Result<(), StoreError> {
        for hotspot in records(telemetry.get("fuel_hotspots")) {
            let fuel = int_field(hotspot.get("fuel_consumed"), 0);
            let input_bytes = opt_int_field(hotspot.get("input_bytes"));
            let input_lines = opt_int_field(hotspot.get("input_lines"));
            let thresholds = hotspot.get("thresholds_exceeded").cloned().unwrap_or_else(|| json!([]));
            self.backend.execute(
                "INSERT INTO diagnostic_hotspots (id, run_id, file_id, plugin, function_name, language, filename, fuel_consumed, fuel_budget, fuel_used_percent, input_bytes, input_lines, fuel_per_kb, fuel_per_line, thresholds_json) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                &[
                    Param::Text(uuid()),
                    Param::Text(run_id.to_string()),
                    Param::Text(file_id.to_string()),
                    Param::OptText(opt_string(hotspot.get("plugin"))),
                    Param::OptText(opt_string(hotspot.get("function"))),
                    Param::OptText(opt_string(hotspot.get("language"))),
                    Param::OptText(opt_string(hotspot.get("filename"))),
                    Param::Int(fuel),
                    Param::OptInt(opt_int_field(hotspot.get("fuel_budget"))),
                    Param::Float(float_field(hotspot.get("fuel_used_percent"))),
                    Param::OptInt(input_bytes),
                    Param::OptInt(input_lines),
                    Param::Float(Some(float_field(hotspot.get("fuel_per_kb")).filter(|v| *v != 0.0).unwrap_or_else(|| fuel_per_kb(fuel, input_bytes)))),
                    Param::Float(Some(float_field(hotspot.get("fuel_per_line")).filter(|v| *v != 0.0).unwrap_or_else(|| fuel_per_line(fuel, input_lines)))),
                    Param::Text(serde_json::to_string(&thresholds).unwrap_or_else(|_| "[]".to_string())),
                ],
            )?;
        }
        Ok(())
    }

    fn record_events(&self, run_id: &str, file_id: &str, diagnostics: &Map<String, Value>) -> Result<(), StoreError> {
        for event in records(diagnostics.get("events")) {
            let metadata = event.get("metadata").cloned().unwrap_or_else(|| json!({}));
            self.backend.execute(
                "INSERT INTO diagnostic_events (id, run_id, file_id, stage, action, rule_id, reason, metadata_json) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                &[
                    Param::Text(uuid()),
                    Param::Text(run_id.to_string()),
                    Param::Text(file_id.to_string()),
                    Param::OptText(opt_string(event.get("stage"))),
                    Param::OptText(opt_string(event.get("action"))),
                    Param::OptText(opt_string(event.get("rule_id"))),
                    Param::OptText(opt_string(event.get("reason"))),
                    Param::Text(serde_json::to_string(&metadata).unwrap_or_else(|_| "{}".to_string())),
                ],
            )?;
        }
        Ok(())
    }

    pub fn record_diff(&self, diff_json: &str) -> Result<(), StoreError> {
        self.record_diff_into(diff_json)
    }

    pub fn record_diagnostics_run(
        &self,
        diffs_json: Vec<String>,
        command: &str,
        repo: &str,
        argv_json: &str,
        run_id: Option<String>,
    ) -> Result<String, StoreError> {
        let run_id = run_id.unwrap_or_else(uuid);
        let parsed: Vec<Value> = diffs_json
            .iter()
            .filter_map(|s| serde_json::from_str::<Value>(s).ok())
            .filter(Value::is_object)
            .collect();
        let summaries: Vec<(i64, i64, i64)> = parsed.iter().map(file_fuel_summary).collect();

        self.backend.execute(
            "INSERT INTO diagnostic_runs (id, recorded_at, command, repo, argv_json, diff_count, total_fuel, peak_fuel, hotspot_count) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            &[
                Param::Text(run_id.clone()),
                Param::Int(now_epoch()),
                Param::Text(command.to_string()),
                Param::Text(repo.to_string()),
                Param::Text(argv_json.to_string()),
                Param::Int(parsed.len() as i64),
                Param::Int(summaries.iter().map(|s| s.1).sum()),
                Param::Int(summaries.iter().map(|s| s.0).max().unwrap_or(0)),
                Param::Int(summaries.iter().map(|s| s.2).sum()),
            ],
        )?;

        for (diff, summary) in parsed.iter().zip(summaries.iter()) {
            let diff_json = serde_json::to_string(diff).map_err(db_err)?;
            self.record_diff_into(&diff_json)?;
            let file_id = uuid();
            let metadata = obj(diff.get("metadata"));
            let telemetry = obj(metadata.get("engine_telemetry"));
            let diagnostics = obj(metadata.get("diagnostics"));
            self.backend.execute(
                "INSERT INTO diagnostic_files (id, run_id, old_filename, new_filename, language, staging_status, file_lifecycle, has_semantic_changes, is_style_only, is_fallback, change_count, parse_error_count, peak_fuel, total_fuel, hotspot_count, diff_json) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                &[
                    Param::Text(file_id.clone()),
                    Param::Text(run_id.clone()),
                    Param::Text(string_field(diff.get("old_filename"), "")),
                    Param::Text(string_field(diff.get("new_filename"), "")),
                    Param::Text(string_field(diff.get("language"), "unknown")),
                    Param::OptText(opt_string(diff.get("staging_status"))),
                    Param::OptText(opt_string(diff.get("file_lifecycle"))),
                    Param::Bool(diff.get("has_semantic_changes").and_then(Value::as_bool).unwrap_or(false)),
                    Param::Bool(diff.get("is_style_only").and_then(Value::as_bool).unwrap_or(false)),
                    Param::Bool(diff.get("is_fallback").and_then(Value::as_bool).unwrap_or(false)),
                    Param::Int(records(diff.get("changes")).len() as i64),
                    Param::Int(diff.get("parse_errors").and_then(Value::as_array).map(|a| a.len()).unwrap_or(0) as i64),
                    Param::Int(summary.0),
                    Param::Int(summary.1),
                    Param::Int(summary.2),
                    Param::Text(diff_json),
                ],
            )?;
            self.record_parser_calls(&run_id, &file_id, &telemetry)?;
            self.record_hotspots(&run_id, &file_id, &telemetry)?;
            self.record_events(&run_id, &file_id, &diagnostics)?;
        }
        Ok(run_id)
    }

    pub fn query(&self, sql: &str) -> Result<String, StoreError> {
        self.backend.query(sql)
    }

    /// python `query_readonly`: conservative SELECT-only guard (the security control).
    pub fn query_readonly(&self, sql: &str) -> Result<String, StoreError> {
        let stripped = sql.trim().trim_end_matches(';');
        let lowered = stripped.to_lowercase();
        let allowed = ["select ", "with ", "show ", "describe ", "summarize "];
        let blocked = [
            " insert ", " update ", " delete ", " drop ", " alter ", " create ", " attach ",
            " detach ", " copy ", " export ", " import ", " pragma ",
        ];
        let padded = format!(" {lowered} ");
        if !allowed.iter().any(|p| lowered.starts_with(p)) || blocked.iter().any(|t| padded.contains(t)) {
            return Err(StoreError::Value(
                "diagnostics query only allows read-only SQL".to_string(),
            ));
        }
        self.backend.query(stripped)
    }

    pub fn most_changed_files(&self, limit: i64) -> Result<String, StoreError> {
        check_limit(limit)?;
        self.backend.query(&format!(
            "SELECT new_filename, COUNT(*) AS diff_count, SUM(change_count) AS total_changes FROM diff_history WHERE has_semantic_changes GROUP BY new_filename ORDER BY total_changes DESC LIMIT {limit}"
        ))
    }

    pub fn changes_by_language(&self) -> Result<String, StoreError> {
        self.backend.query(
            "SELECT language, COUNT(*) AS diffs, SUM(change_count) AS total_changes, SUM(CAST(is_style_only AS INTEGER)) AS style_only_diffs FROM diff_history GROUP BY language ORDER BY diffs DESC",
        )
    }

    pub fn recent_diagnostic_runs(&self, limit: i64) -> Result<String, StoreError> {
        check_limit(limit)?;
        self.backend.query(&format!(
            "SELECT id, recorded_at, command, repo, diff_count, total_fuel, peak_fuel, hotspot_count FROM diagnostic_runs ORDER BY recorded_at DESC LIMIT {limit}"
        ))
    }

    pub fn fuel_by_language(&self, limit: i64) -> Result<String, StoreError> {
        check_limit(limit)?;
        self.backend.query(&format!(
            "SELECT COALESCE(language, 'unknown') AS language, COUNT(*) AS parser_calls, SUM(total_fuel_consumed) AS total_fuel, MAX(fuel_consumed) AS peak_fuel, AVG(fuel_per_kb) AS avg_fuel_per_kb, AVG(fuel_per_line) AS avg_fuel_per_line FROM diagnostic_parser_calls GROUP BY COALESCE(language, 'unknown') ORDER BY total_fuel DESC LIMIT {limit}"
        ))
    }

    pub fn top_fuel_hotspots(&self, limit: i64) -> Result<String, StoreError> {
        check_limit(limit)?;
        self.backend.query(&format!(
            "SELECT h.language, h.filename, h.plugin, h.function_name AS function, h.fuel_consumed, h.fuel_per_kb, h.fuel_per_line, h.thresholds_json, r.recorded_at, r.command FROM diagnostic_hotspots h JOIN diagnostic_runs r ON r.id = h.run_id ORDER BY h.fuel_per_line DESC, h.fuel_per_kb DESC, h.fuel_consumed DESC LIMIT {limit}"
        ))
    }

    pub fn close(&self) -> Result<(), StoreError> {
        if let Ok(mut open) = self.open.lock() {
            *open = false; // connection is dropped with the store
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db(name: &str) -> String {
        std::env::temp_dir()
            .join(format!("intentumdiff_analytics_{}_{}.db", std::process::id(), name))
            .to_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn fallback_backend_is_sqlite() {
        let store = AnalyticsStore::open(&temp_db("be")).unwrap();
        assert_eq!(store.backend_name(), "sqlite");
    }

    /// Runs ONLY when a libduckdb is provided (INTENTUMDIFF_DUCKDB_LIB) — the default test
    /// run exercises the SQLite fallback. Run in isolation (`cargo test
    /// duckdb_backend_when_lib_provided`) so the env var doesn't flip the fallback asserts.
    #[test]
    fn duckdb_backend_when_lib_provided() {
        if std::env::var("INTENTUMDIFF_DUCKDB_LIB").map(|v| v.is_empty()).unwrap_or(true) {
            return;
        }
        let path = temp_db("duck");
        let _ = std::fs::remove_file(&path);
        let store = AnalyticsStore::open(&path).unwrap();
        assert_eq!(store.backend_name(), "duckdb", "a provided libduckdb should select duckdb");

        let diff = json!({
            "old_filename": "a.py", "new_filename": "a.py", "language": "python",
            "has_semantic_changes": true, "is_style_only": false, "is_fallback": false,
            "changes": [{"change_type": "ADDITION"}], "parse_errors": [],
            "metadata": {"engine_telemetry": {"calls": [{"plugin": "p", "function": "process",
                "language": "python", "trusted": true, "call_count": 1,
                "fuel_consumed": 5000000, "total_fuel_consumed": 5000000,
                "input_bytes": 100, "input_lines": 5}], "fuel_hotspots": []},
                "diagnostics": {"events": []}}
        });
        store.record_diagnostics_run(vec![diff.to_string()], "c", ".", "[]", None).unwrap();
        let langs: Value = serde_json::from_str(&store.fuel_by_language(20).unwrap()).unwrap();
        assert_eq!(langs[0]["language"], json!("python"));
        assert_eq!(langs[0]["peak_fuel"], json!(5_000_000));
        assert!(store.query_readonly("delete from diff_history").is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn read_only_guard_blocks_mutations() {
        let store = AnalyticsStore::open(&temp_db("guard")).unwrap();
        assert!(store.query_readonly("SELECT 1").is_ok());
        assert!(store.query_readonly("DELETE FROM diff_history").is_err());
        assert!(store.query_readonly("select 1; drop table diff_history").is_err());
        assert!(store.most_changed_files(0).is_err());
    }

    #[test]
    fn records_and_queries_normalized_fuel() {
        let store = AnalyticsStore::open(&temp_db("fuel")).unwrap();
        let diff = json!({
            "old_filename": "src/main.ts", "new_filename": "src/main.ts", "language": "typescript",
            "has_semantic_changes": true, "is_style_only": false, "is_fallback": false,
            "changes": [{"change_type": "ADDITION"}], "parse_errors": [],
            "metadata": {
                "engine_telemetry": {
                    "calls": [{"plugin": "js-ts-parser.wasm", "function": "process", "language": "typescript",
                        "trusted": true, "statuses": {"ok": 1}, "call_count": 1,
                        "fuel_consumed": 25000000, "total_fuel_consumed": 25000000,
                        "input_bytes": 500, "input_lines": 10}],
                    "fuel_hotspots": [{"plugin": "js-ts-parser.wasm", "function": "process", "language": "typescript",
                        "filename": "src/main.ts", "fuel_consumed": 25000000,
                        "fuel_per_kb": 25000000, "fuel_per_line": 2500000,
                        "thresholds_exceeded": ["absolute", "per_line"]}]
                },
                "diagnostics": {"events": [{"stage": "engine.telemetry", "action": "wasm_fuel_hotspot"}]}
            }
        });
        let run_id = store
            .record_diagnostics_run(vec![diff.to_string()], "string", ".", "[\"string\"]", None)
            .unwrap();
        assert!(!run_id.is_empty());

        let langs: Value = serde_json::from_str(&store.fuel_by_language(20).unwrap()).unwrap();
        assert_eq!(langs[0]["language"], json!("typescript"));
        assert_eq!(langs[0]["parser_calls"], json!(1));
        assert_eq!(langs[0]["peak_fuel"], json!(25_000_000));

        let hotspots: Value = serde_json::from_str(&store.top_fuel_hotspots(20).unwrap()).unwrap();
        assert_eq!(hotspots[0]["filename"], json!("src/main.ts"));
        assert_eq!(hotspots[0]["fuel_per_line"], json!(2_500_000.0));

        let queried: Value = serde_json::from_str(
            &store.query_readonly("select language, peak_fuel from diagnostic_files").unwrap(),
        )
        .unwrap();
        assert_eq!(queried[0]["language"], json!("typescript"));
    }
}
