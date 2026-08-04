//! Runtime-loaded DuckDB C API — the "provided" analytics engine (#101, A2.2).
//!
//! `dlopen`s a configurable `libduckdb` (via `libloading`) and binds the stable DuckDB C
//! API. Used only when a libduckdb is available (`$INTENTUMDIFF_DUCKDB_LIB` or a standard
//! name on the loader path); the analytics store falls back to SQLite otherwise, so the
//! core never links or requires DuckDB. Parameterized via prepared statements (no SQL
//! interpolation of values); `duckdb_value_varchar` results are freed with `duckdb_free`
//! and every result/prepared handle is destroyed.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::sync::Arc;

use libloading::Library;
use serde_json::{json, Map, Value};

use crate::analytics_store::Param;

type IdxT = u64;
type State = c_int; // DuckDBSuccess = 0, DuckDBError = 1
type DType = c_int; // duckdb_type
type Handle = *mut c_void; // opaque database / connection / prepared_statement

const OK: State = 0;

// duckdb_type enum (subset the analytics queries produce).
const T_BOOLEAN: DType = 1;
const T_TINYINT: DType = 2;
const T_SMALLINT: DType = 3;
const T_INTEGER: DType = 4;
const T_BIGINT: DType = 5;
const T_UTINYINT: DType = 6;
const T_USMALLINT: DType = 7;
const T_UINTEGER: DType = 8;
const T_UBIGINT: DType = 9;
const T_FLOAT: DType = 10;
const T_DOUBLE: DType = 11;
const T_HUGEINT: DType = 16;

/// Matches the C `duckdb_result` layout (accessed only via accessor functions).
#[repr(C)]
struct DuckResult {
    deprecated_column_count: IdxT,
    deprecated_row_count: IdxT,
    deprecated_rows_changed: IdxT,
    deprecated_columns: *mut c_void,
    deprecated_error_message: *mut c_char,
    internal_data: *mut c_void,
}

type FnOpen = unsafe extern "C" fn(*const c_char, *mut Handle) -> State;
type FnClose = unsafe extern "C" fn(*mut Handle);
type FnConnect = unsafe extern "C" fn(Handle, *mut Handle) -> State;
type FnDisconnect = unsafe extern "C" fn(*mut Handle);
type FnQuery = unsafe extern "C" fn(Handle, *const c_char, *mut DuckResult) -> State;
type FnDestroyResult = unsafe extern "C" fn(*mut DuckResult);
type FnColCount = unsafe extern "C" fn(*mut DuckResult) -> IdxT;
type FnRowCount = unsafe extern "C" fn(*mut DuckResult) -> IdxT;
type FnColName = unsafe extern "C" fn(*mut DuckResult, IdxT) -> *const c_char;
type FnColType = unsafe extern "C" fn(*mut DuckResult, IdxT) -> DType;
type FnValBool = unsafe extern "C" fn(*mut DuckResult, IdxT, IdxT) -> bool;
type FnValI64 = unsafe extern "C" fn(*mut DuckResult, IdxT, IdxT) -> i64;
type FnValF64 = unsafe extern "C" fn(*mut DuckResult, IdxT, IdxT) -> f64;
type FnValVarchar = unsafe extern "C" fn(*mut DuckResult, IdxT, IdxT) -> *mut c_char;
type FnValIsNull = unsafe extern "C" fn(*mut DuckResult, IdxT, IdxT) -> bool;
type FnResultError = unsafe extern "C" fn(*mut DuckResult) -> *const c_char;
type FnFree = unsafe extern "C" fn(*mut c_void);
type FnPrepare = unsafe extern "C" fn(Handle, *const c_char, *mut Handle) -> State;
type FnPrepareError = unsafe extern "C" fn(Handle) -> *const c_char;
type FnDestroyPrepare = unsafe extern "C" fn(*mut Handle);
type FnBindVarchar = unsafe extern "C" fn(Handle, IdxT, *const c_char) -> State;
type FnBindI64 = unsafe extern "C" fn(Handle, IdxT, i64) -> State;
type FnBindF64 = unsafe extern "C" fn(Handle, IdxT, f64) -> State;
type FnBindBool = unsafe extern "C" fn(Handle, IdxT, bool) -> State;
type FnBindNull = unsafe extern "C" fn(Handle, IdxT) -> State;
type FnExecPrepared = unsafe extern "C" fn(Handle, *mut DuckResult) -> State;

struct Api {
    open: FnOpen,
    close: FnClose,
    connect: FnConnect,
    disconnect: FnDisconnect,
    query: FnQuery,
    destroy_result: FnDestroyResult,
    col_count: FnColCount,
    row_count: FnRowCount,
    col_name: FnColName,
    col_type: FnColType,
    val_bool: FnValBool,
    val_i64: FnValI64,
    val_f64: FnValF64,
    val_varchar: FnValVarchar,
    val_is_null: FnValIsNull,
    result_error: FnResultError,
    free: FnFree,
    prepare: FnPrepare,
    prepare_error: FnPrepareError,
    destroy_prepare: FnDestroyPrepare,
    bind_varchar: FnBindVarchar,
    bind_i64: FnBindI64,
    bind_f64: FnBindF64,
    bind_bool: FnBindBool,
    bind_null: FnBindNull,
    exec_prepared: FnExecPrepared,
    _lib: Library, // keep the loaded library alive for the lifetime of the fn pointers
}

impl Api {
    unsafe fn load(lib_path: &str) -> Result<Api, String> {
        let lib = Library::new(lib_path).map_err(|e| e.to_string())?;
        macro_rules! sym {
            ($name:literal) => {
                *lib.get($name).map_err(|e| e.to_string())?
            };
        }
        // Extract every fn pointer BEFORE moving `lib` into the struct.
        let open: FnOpen = sym!(b"duckdb_open\0");
        let close: FnClose = sym!(b"duckdb_close\0");
        let connect: FnConnect = sym!(b"duckdb_connect\0");
        let disconnect: FnDisconnect = sym!(b"duckdb_disconnect\0");
        let query: FnQuery = sym!(b"duckdb_query\0");
        let destroy_result: FnDestroyResult = sym!(b"duckdb_destroy_result\0");
        let col_count: FnColCount = sym!(b"duckdb_column_count\0");
        let row_count: FnRowCount = sym!(b"duckdb_row_count\0");
        let col_name: FnColName = sym!(b"duckdb_column_name\0");
        let col_type: FnColType = sym!(b"duckdb_column_type\0");
        let val_bool: FnValBool = sym!(b"duckdb_value_boolean\0");
        let val_i64: FnValI64 = sym!(b"duckdb_value_int64\0");
        let val_f64: FnValF64 = sym!(b"duckdb_value_double\0");
        let val_varchar: FnValVarchar = sym!(b"duckdb_value_varchar\0");
        let val_is_null: FnValIsNull = sym!(b"duckdb_value_is_null\0");
        let result_error: FnResultError = sym!(b"duckdb_result_error\0");
        let free: FnFree = sym!(b"duckdb_free\0");
        let prepare: FnPrepare = sym!(b"duckdb_prepare\0");
        let prepare_error: FnPrepareError = sym!(b"duckdb_prepare_error\0");
        let destroy_prepare: FnDestroyPrepare = sym!(b"duckdb_destroy_prepare\0");
        let bind_varchar: FnBindVarchar = sym!(b"duckdb_bind_varchar\0");
        let bind_i64: FnBindI64 = sym!(b"duckdb_bind_int64\0");
        let bind_f64: FnBindF64 = sym!(b"duckdb_bind_double\0");
        let bind_bool: FnBindBool = sym!(b"duckdb_bind_boolean\0");
        let bind_null: FnBindNull = sym!(b"duckdb_bind_null\0");
        let exec_prepared: FnExecPrepared = sym!(b"duckdb_execute_prepared\0");
        Ok(Api {
            open, close, connect, disconnect, query, destroy_result, col_count, row_count,
            col_name, col_type, val_bool, val_i64, val_f64, val_varchar, val_is_null,
            result_error, free, prepare, prepare_error, destroy_prepare, bind_varchar,
            bind_i64, bind_f64, bind_bool, bind_null, exec_prepared,
            _lib: lib,
        })
    }
}

/// A live DuckDB database + connection over a dlopen'd libduckdb.
pub struct DuckDbHandle {
    api: Arc<Api>,
    db: Handle,
    con: Handle,
}

// Access is always serialized through the AnalyticsStore's per-backend Mutex, so the raw
// handles are never touched concurrently.
unsafe impl Send for DuckDbHandle {}

impl Drop for DuckDbHandle {
    fn drop(&mut self) {
        unsafe {
            (self.api.disconnect)(&mut self.con);
            (self.api.close)(&mut self.db);
        }
    }
}

/// Read a DuckDB-owned C string (column name / error) — do NOT free.
unsafe fn borrowed(ptr: *const c_char) -> String {
    if ptr.is_null() {
        String::new()
    } else {
        CStr::from_ptr(ptr).to_string_lossy().into_owned()
    }
}

impl DuckDbHandle {
    /// Open + connect against a dlopen'd libduckdb.
    pub fn open(db_path: &str, lib_path: &str) -> Result<DuckDbHandle, String> {
        unsafe {
            let api = Arc::new(Api::load(lib_path)?);
            let cpath = CString::new(db_path).map_err(|e| e.to_string())?;
            let mut db: Handle = std::ptr::null_mut();
            if (api.open)(cpath.as_ptr(), &mut db) != OK {
                return Err("duckdb_open failed".to_string());
            }
            let mut con: Handle = std::ptr::null_mut();
            if (api.connect)(db, &mut con) != OK {
                (api.close)(&mut db);
                return Err("duckdb_connect failed".to_string());
            }
            Ok(DuckDbHandle { api, db, con })
        }
    }

    /// Run each `;`-separated statement (schema DDL) — no results.
    pub fn execute_batch(&self, sql: &str) -> Result<(), String> {
        for stmt in sql.split(';') {
            let stmt = stmt.trim();
            if stmt.is_empty() {
                continue;
            }
            self.execute(stmt, &[])?;
        }
        Ok(())
    }

    /// Prepare + bind + execute a single parameterized statement.
    pub fn execute(&self, sql: &str, params: &[Param]) -> Result<(), String> {
        unsafe {
            let csql = CString::new(sql).map_err(|e| e.to_string())?;
            let mut prep: Handle = std::ptr::null_mut();
            if (self.api.prepare)(self.con, csql.as_ptr(), &mut prep) != OK {
                let err = borrowed((self.api.prepare_error)(prep));
                (self.api.destroy_prepare)(&mut prep);
                return Err(err);
            }
            // Keep bound CStrings alive until execute.
            let mut owned: Vec<CString> = Vec::new();
            for (i, p) in params.iter().enumerate() {
                let idx = (i + 1) as IdxT;
                let state = match p {
                    Param::Text(s) => {
                        let c = CString::new(s.as_str()).map_err(|e| e.to_string())?;
                        let st = (self.api.bind_varchar)(prep, idx, c.as_ptr());
                        owned.push(c);
                        st
                    }
                    Param::OptText(Some(s)) => {
                        let c = CString::new(s.as_str()).map_err(|e| e.to_string())?;
                        let st = (self.api.bind_varchar)(prep, idx, c.as_ptr());
                        owned.push(c);
                        st
                    }
                    Param::OptText(None) => (self.api.bind_null)(prep, idx),
                    Param::Int(v) => (self.api.bind_i64)(prep, idx, *v),
                    Param::OptInt(Some(v)) => (self.api.bind_i64)(prep, idx, *v),
                    Param::OptInt(None) => (self.api.bind_null)(prep, idx),
                    Param::Float(Some(v)) => (self.api.bind_f64)(prep, idx, *v),
                    Param::Float(None) => (self.api.bind_null)(prep, idx),
                    Param::Bool(v) => (self.api.bind_bool)(prep, idx, *v),
                };
                if state != OK {
                    let err = borrowed((self.api.prepare_error)(prep));
                    (self.api.destroy_prepare)(&mut prep);
                    return Err(err);
                }
            }
            let mut result: DuckResult = std::mem::zeroed();
            let state = (self.api.exec_prepared)(prep, &mut result);
            (self.api.destroy_prepare)(&mut prep);
            drop(owned);
            if state != OK {
                let err = borrowed((self.api.result_error)(&mut result));
                (self.api.destroy_result)(&mut result);
                return Err(err);
            }
            (self.api.destroy_result)(&mut result);
            Ok(())
        }
    }

    /// Run *sql* and return rows as a JSON array of column→value objects.
    pub fn query(&self, sql: &str) -> Result<String, String> {
        unsafe {
            let csql = CString::new(sql).map_err(|e| e.to_string())?;
            let mut result: DuckResult = std::mem::zeroed();
            if (self.api.query)(self.con, csql.as_ptr(), &mut result) != OK {
                let err = borrowed((self.api.result_error)(&mut result));
                (self.api.destroy_result)(&mut result);
                return Err(err);
            }
            let cols = (self.api.col_count)(&mut result);
            let rows = (self.api.row_count)(&mut result);
            let names: Vec<String> = (0..cols)
                .map(|c| borrowed((self.api.col_name)(&mut result, c)))
                .collect();
            let types: Vec<DType> = (0..cols).map(|c| (self.api.col_type)(&mut result, c)).collect();

            let mut out: Vec<Value> = Vec::new();
            for r in 0..rows {
                let mut map = Map::new();
                for c in 0..cols {
                    let value = if (self.api.val_is_null)(&mut result, c, r) {
                        Value::Null
                    } else {
                        match types[c as usize] {
                            T_BOOLEAN => json!((self.api.val_bool)(&mut result, c, r)),
                            T_TINYINT | T_SMALLINT | T_INTEGER | T_BIGINT | T_UTINYINT
                            | T_USMALLINT | T_UINTEGER | T_UBIGINT | T_HUGEINT => {
                                json!((self.api.val_i64)(&mut result, c, r))
                            }
                            T_FLOAT | T_DOUBLE => json!((self.api.val_f64)(&mut result, c, r)),
                            _ => {
                                let ptr = (self.api.val_varchar)(&mut result, c, r);
                                let s = borrowed(ptr);
                                (self.api.free)(ptr as *mut c_void);
                                Value::String(s)
                            }
                        }
                    };
                    map.insert(names[c as usize].clone(), value);
                }
                out.push(Value::Object(map));
            }
            (self.api.destroy_result)(&mut result);
            serde_json::to_string(&out).map_err(|e| e.to_string())
        }
    }
}

/// Try to open a DuckDB database at *db_path* using a provided libduckdb. Tries
/// `$INTENTUMDIFF_DUCKDB_LIB` first, then platform default names on the loader path.
pub fn try_open(db_path: &str) -> Option<DuckDbHandle> {
    let mut candidates: Vec<String> = Vec::new();
    if let Ok(env_lib) = std::env::var("INTENTUMDIFF_DUCKDB_LIB") {
        if !env_lib.is_empty() {
            candidates.push(env_lib);
        }
    }
    for name in ["duckdb.dll", "libduckdb.so", "libduckdb.dylib", "libduckdb"] {
        candidates.push(name.to_string());
    }
    for candidate in candidates {
        if let Ok(handle) = DuckDbHandle::open(db_path, &candidate) {
            return Some(handle);
        }
    }
    None
}
