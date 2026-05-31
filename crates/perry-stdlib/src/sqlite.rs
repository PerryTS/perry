//! SQLite module (better-sqlite3 compatible)
//!
//! Native implementation of the 'better-sqlite3' npm package using rusqlite.
//! Provides synchronous SQLite database operations.

use crate::common::{get_handle, register_handle, Handle};
use perry_runtime::{
    js_array_alloc, js_array_push, js_get_string_pointer_unified, js_nanbox_pointer,
    js_object_alloc_with_shape, js_object_get_field_by_name, js_object_set_field,
    js_string_from_bytes, ArrayHeader, JSValue, ObjectHeader, StringHeader,
};
use rusqlite::{ffi, limits::Limit, types::Value as SqliteValue, Connection, OpenFlags};
use std::ffi::{CStr, CString};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// Helper to extract string from StringHeader pointer
unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    Some(String::from_utf8_lossy(bytes).to_string())
}

fn undefined_f64() -> f64 {
    f64::from_bits(TAG_UNDEFINED_BITS)
}

fn null_f64() -> f64 {
    f64::from_bits(TAG_NULL_BITS)
}

fn bool_f64(value: bool) -> f64 {
    f64::from_bits(JSValue::bool(value).bits())
}

fn value_from_f64(value: f64) -> JSValue {
    JSValue::from_bits(value.to_bits())
}

fn throw_type(message: &str) -> ! {
    perry_runtime::fs::validate::throw_type_error_with_code(message, "ERR_INVALID_ARG_TYPE")
}

fn throw_construct_required() -> ! {
    perry_runtime::fs::validate::throw_type_error_with_code(
        "Class constructor DatabaseSync cannot be invoked without 'new'",
        "ERR_CONSTRUCT_CALL_REQUIRED",
    )
}

fn throw_range(message: &str) -> ! {
    perry_runtime::fs::validate::throw_range_error_with_code(message)
}

fn throw_invalid_state(message: &str) -> ! {
    perry_runtime::fs::validate::throw_error_with_code(message, "ERR_INVALID_STATE")
}

fn throw_sqlite_error(message: &str) -> ! {
    perry_runtime::fs::validate::throw_error_with_code(message, "ERR_SQLITE_ERROR")
}

unsafe fn node_sqlite_exec_batch(conn: &Connection, sql: &str) -> Result<(), String> {
    let c_sql =
        CString::new(sql).map_err(|_| "SQL string must not contain null bytes".to_string())?;
    let mut error_message = std::ptr::null_mut();
    let rc = ffi::sqlite3_exec(
        conn.handle(),
        c_sql.as_ptr(),
        None,
        std::ptr::null_mut(),
        &mut error_message,
    );
    if rc == ffi::SQLITE_OK {
        return Ok(());
    }

    let message = if error_message.is_null() {
        CStr::from_ptr(ffi::sqlite3_errmsg(conn.handle()))
            .to_string_lossy()
            .into_owned()
    } else {
        let message = CStr::from_ptr(error_message).to_string_lossy().into_owned();
        ffi::sqlite3_free(error_message.cast());
        message
    };
    Err(message)
}

unsafe fn string_from_value(value: f64, name: &str) -> String {
    let js = value_from_f64(value);
    if !js.is_any_string() {
        throw_type(&format!("The \"{}\" argument must be of type string", name));
    }
    let ptr = js_get_string_pointer_unified(value) as *const StringHeader;
    let s = string_from_header(ptr).unwrap_or_else(|| {
        throw_type(&format!("The \"{}\" argument must be of type string", name))
    });
    if s.as_bytes().contains(&0) {
        throw_type(&format!(
            "The \"{}\" argument must not contain null bytes",
            name
        ));
    }
    s
}

fn is_object_like(value: f64) -> bool {
    value_from_f64(value).is_pointer()
}

unsafe fn object_field(object_value: f64, name: &str) -> JSValue {
    if !is_object_like(object_value) {
        return JSValue::undefined();
    }
    let obj_ptr = value_from_f64(object_value).as_pointer::<ObjectHeader>();
    if obj_ptr.is_null() || (obj_ptr as usize) < 0x1000 {
        return JSValue::undefined();
    }
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_get_field_by_name(obj_ptr, key)
}

unsafe fn bool_option(options_value: f64, name: &str, default: bool) -> bool {
    let value = object_field(options_value, name);
    if value.is_undefined() {
        return default;
    }
    if !value.is_bool() {
        throw_type(&format!("The \"{}\" option must be of type boolean", name));
    }
    value.as_bool()
}

fn non_negative_i32_value(value: JSValue, name: &str, allow_infinity: bool) -> i32 {
    let number = if value.is_int32() {
        value.as_int32() as f64
    } else if value.is_number() {
        value.as_number()
    } else {
        throw_type(&format!("The \"{}\" option must be a number", name));
    };

    if allow_infinity && number == f64::INFINITY {
        return i32::MAX;
    }
    if !number.is_finite() || number < 0.0 || number.fract() != 0.0 || number > i32::MAX as f64 {
        throw_range(&format!(
            "The value of \"{}\" is out of range. It must be a non-negative integer.",
            name
        ));
    }
    number as i32
}

unsafe fn non_negative_i32_option(options_value: f64, name: &str, default: i32) -> i32 {
    let value = object_field(options_value, name);
    if value.is_undefined() {
        return default;
    }
    non_negative_i32_value(value, name, false)
}

fn node_sqlite_limit(name: &str) -> Option<(usize, Limit)> {
    match name {
        "length" => Some((0, Limit::SQLITE_LIMIT_LENGTH)),
        "sqlLength" => Some((1, Limit::SQLITE_LIMIT_SQL_LENGTH)),
        "column" => Some((2, Limit::SQLITE_LIMIT_COLUMN)),
        "exprDepth" => Some((3, Limit::SQLITE_LIMIT_EXPR_DEPTH)),
        "compoundSelect" => Some((4, Limit::SQLITE_LIMIT_COMPOUND_SELECT)),
        "vdbeOp" => Some((5, Limit::SQLITE_LIMIT_VDBE_OP)),
        "functionArg" => Some((6, Limit::SQLITE_LIMIT_FUNCTION_ARG)),
        "attach" => Some((7, Limit::SQLITE_LIMIT_ATTACHED)),
        "likePatternLength" => Some((8, Limit::SQLITE_LIMIT_LIKE_PATTERN_LENGTH)),
        "variableNumber" => Some((9, Limit::SQLITE_LIMIT_VARIABLE_NUMBER)),
        "triggerDepth" => Some((10, Limit::SQLITE_LIMIT_TRIGGER_DEPTH)),
        _ => None,
    }
}

unsafe fn parse_node_sqlite_options(options_value: f64) -> NodeSqliteOptions {
    let mut options = NodeSqliteOptions::default();
    let js = value_from_f64(options_value);
    if js.is_undefined() {
        return options;
    }
    if js.is_null() || !is_object_like(options_value) {
        throw_type("The \"options\" argument must be an object");
    }

    options.open = bool_option(options_value, "open", options.open);
    options.read_only = bool_option(options_value, "readOnly", options.read_only);
    options.enable_foreign_keys = bool_option(
        options_value,
        "enableForeignKeyConstraints",
        options.enable_foreign_keys,
    );
    options.enable_dqs = bool_option(
        options_value,
        "enableDoubleQuotedStringLiterals",
        options.enable_dqs,
    );
    options.timeout_ms = non_negative_i32_option(options_value, "timeout", options.timeout_ms);
    options.read_bigints = bool_option(options_value, "readBigInts", options.read_bigints);
    options.return_arrays = bool_option(options_value, "returnArrays", options.return_arrays);
    options.allow_bare_named_parameters = bool_option(
        options_value,
        "allowBareNamedParameters",
        options.allow_bare_named_parameters,
    );
    options.allow_unknown_named_parameters = bool_option(
        options_value,
        "allowUnknownNamedParameters",
        options.allow_unknown_named_parameters,
    );
    options.defensive = bool_option(options_value, "defensive", options.defensive);

    let limits = object_field(options_value, "limits");
    if !limits.is_undefined() {
        let limits_value = f64::from_bits(limits.bits());
        if limits.is_null() || !is_object_like(limits_value) {
            throw_type("The \"limits\" option must be an object");
        }
        for name in [
            "length",
            "sqlLength",
            "column",
            "exprDepth",
            "compoundSelect",
            "vdbeOp",
            "functionArg",
            "attach",
            "likePatternLength",
            "variableNumber",
            "triggerDepth",
        ] {
            if let Some((idx, _)) = node_sqlite_limit(name) {
                let value = object_field(limits_value, name);
                if !value.is_undefined() {
                    options.initial_limits[idx] = Some(non_negative_i32_value(value, name, false));
                }
            }
        }
    }

    options
}

fn resolve_sqlite_path(filename: &str) -> String {
    if filename == ":memory:" || filename.starts_with('/') || filename.starts_with(':') {
        return filename.to_string();
    }
    #[cfg(target_os = "ios")]
    {
        extern "C" {
            fn getenv(name: *const i8) -> *const i8;
        }
        unsafe {
            let home = getenv(b"HOME\0".as_ptr() as *const i8);
            if !home.is_null() {
                let home_str = std::ffi::CStr::from_ptr(home).to_str().unwrap_or("");
                let docs = format!("{}/Documents", home_str);
                let _ = std::fs::create_dir_all(&docs);
                return format!("{}/{}", docs, filename);
            }
        }
    }
    filename.to_string()
}

fn open_node_sqlite_connection(db: &NodeSqliteDbHandle) -> rusqlite::Result<Connection> {
    let flags = if db.read_only {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
    } | OpenFlags::SQLITE_OPEN_URI
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;

    let conn = if db.path == ":memory:" {
        Connection::open_in_memory_with_flags(flags)?
    } else {
        Connection::open_with_flags(resolve_sqlite_path(&db.path), flags)?
    };

    if db.timeout_ms > 0 {
        conn.busy_timeout(Duration::from_millis(db.timeout_ms as u64))?;
    }

    conn.execute_batch(if db.enable_foreign_keys {
        "PRAGMA foreign_keys = ON"
    } else {
        "PRAGMA foreign_keys = OFF"
    })?;

    for (idx, value) in db.initial_limits.iter().enumerate() {
        if let Some(value) = value {
            if let Some(limit) = [
                Limit::SQLITE_LIMIT_LENGTH,
                Limit::SQLITE_LIMIT_SQL_LENGTH,
                Limit::SQLITE_LIMIT_COLUMN,
                Limit::SQLITE_LIMIT_EXPR_DEPTH,
                Limit::SQLITE_LIMIT_COMPOUND_SELECT,
                Limit::SQLITE_LIMIT_VDBE_OP,
                Limit::SQLITE_LIMIT_FUNCTION_ARG,
                Limit::SQLITE_LIMIT_ATTACHED,
                Limit::SQLITE_LIMIT_LIKE_PATTERN_LENGTH,
                Limit::SQLITE_LIMIT_VARIABLE_NUMBER,
                Limit::SQLITE_LIMIT_TRIGGER_DEPTH,
            ]
            .get(idx)
            {
                conn.set_limit(*limit, *value);
            }
        }
    }

    Ok(conn)
}

unsafe fn with_sqlite_connection<R, F>(db_handle: Handle, f: F) -> Option<R>
where
    F: FnOnce(&Connection) -> R,
{
    if let Some(db) = get_handle::<SqliteDbHandle>(db_handle) {
        if let Ok(conn) = db.conn.lock() {
            return Some(f(&conn));
        }
    }
    if let Some(db) = get_handle::<NodeSqliteDbHandle>(db_handle) {
        if let Ok(conn) = db.conn.lock() {
            if let Some(conn) = conn.as_ref() {
                return Some(f(conn));
            }
        }
    }
    None
}

unsafe fn with_open_node_connection<R, F>(db_handle: Handle, f: F) -> R
where
    F: FnOnce(&Connection) -> R,
{
    let db = get_handle::<NodeSqliteDbHandle>(db_handle)
        .unwrap_or_else(|| throw_invalid_state("Database is not open"));
    let conn = db
        .conn
        .lock()
        .unwrap_or_else(|_| throw_invalid_state("Database is not open"));
    if let Some(conn) = conn.as_ref() {
        f(conn)
    } else {
        drop(conn);
        throw_invalid_state("Database is not open")
    }
}

unsafe fn ensure_open_node_database(db_handle: Handle) {
    let db = get_handle::<NodeSqliteDbHandle>(db_handle)
        .unwrap_or_else(|| throw_invalid_state("Database is not open"));
    let conn = db
        .conn
        .lock()
        .unwrap_or_else(|_| throw_invalid_state("Database is not open"));
    if conn.is_none() {
        drop(conn);
        throw_invalid_state("Database is not open");
    }
}

/// SQLite database handle
pub struct SqliteDbHandle {
    pub conn: Mutex<Connection>,
}

/// Node `node:sqlite` DatabaseSync handle.
///
/// Kept separate from `SqliteDbHandle` so the historical better-sqlite3
/// close/exec/prepare behavior remains unchanged.
pub struct NodeSqliteDbHandle {
    pub conn: Mutex<Option<Connection>>,
    pub path: String,
    pub read_only: bool,
    pub enable_foreign_keys: bool,
    pub enable_dqs: bool,
    pub timeout_ms: i32,
    pub read_bigints: bool,
    pub return_arrays: bool,
    pub allow_bare_named_parameters: bool,
    pub allow_unknown_named_parameters: bool,
    pub defensive: bool,
    pub initial_limits: [Option<i32>; NODE_SQLITE_LIMIT_COUNT],
    pub limits_handle: Mutex<Option<Handle>>,
}

pub struct NodeSqliteLimitsHandle {
    pub db_handle: Handle,
}

#[derive(Clone)]
struct NodeSqliteOptions {
    open: bool,
    read_only: bool,
    enable_foreign_keys: bool,
    enable_dqs: bool,
    timeout_ms: i32,
    read_bigints: bool,
    return_arrays: bool,
    allow_bare_named_parameters: bool,
    allow_unknown_named_parameters: bool,
    defensive: bool,
    initial_limits: [Option<i32>; NODE_SQLITE_LIMIT_COUNT],
}

impl Default for NodeSqliteOptions {
    fn default() -> Self {
        Self {
            open: true,
            read_only: false,
            enable_foreign_keys: true,
            enable_dqs: false,
            timeout_ms: 0,
            read_bigints: false,
            return_arrays: false,
            allow_bare_named_parameters: true,
            allow_unknown_named_parameters: false,
            defensive: true,
            initial_limits: [None; NODE_SQLITE_LIMIT_COUNT],
        }
    }
}

const NODE_SQLITE_LIMIT_COUNT: usize = 11;
const TAG_UNDEFINED_BITS: u64 = 0x7FFC_0000_0000_0001;
const TAG_NULL_BITS: u64 = 0x7FFC_0000_0000_0002;

/// SQLite statement handle
pub struct SqliteStmtHandle {
    pub sql: String,
    pub db_handle: Handle,
    /// Per-statement raw mode flag — `stmt.raw([toggle])` enables this.
    /// In raw mode, `stmt.all(...)` returns array-of-arrays (one inner
    /// array per row, column values in declared order) and
    /// `stmt.get(...)` returns a single column-value array. drizzle's
    /// `PreparedQuery.values()` chains `this.stmt.raw().all(...)` to
    /// feed `mapResultRow(fields, row, joinsNotNullableMap)`. Without
    /// this method `stmt.raw` is undefined and the call surfaces as
    /// `(number).all is not a function` deeper in the chain. Refs #643.
    pub raw_mode: AtomicBool,
}

/// Convert SQLite value to JSValue
unsafe fn sqlite_value_to_jsvalue(value: &SqliteValue) -> JSValue {
    match value {
        SqliteValue::Null => JSValue::null(),
        SqliteValue::Integer(n) => {
            if *n >= i32::MIN as i64 && *n <= i32::MAX as i64 {
                JSValue::int32(*n as i32)
            } else {
                JSValue::number(*n as f64)
            }
        }
        SqliteValue::Real(n) => JSValue::number(*n),
        SqliteValue::Text(s) => {
            let ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
            JSValue::string_ptr(ptr)
        }
        SqliteValue::Blob(b) => {
            // Return blob as hex string. Hand-rolled to avoid pulling in
            // the `hex` crate, which lives behind the `crypto` Cargo
            // feature — auto-optimize builds that enable only
            // `database-sqlite` (e.g. mango: better-sqlite3 + mongodb +
            // fetch, no crypto) would otherwise fail to resolve `hex::`
            // and fall back to the prebuilt full stdlib.
            const HEX: &[u8; 16] = b"0123456789abcdef";
            let mut out = Vec::with_capacity(b.len() * 2);
            for &byte in b {
                out.push(HEX[(byte >> 4) as usize]);
                out.push(HEX[(byte & 0x0f) as usize]);
            }
            let ptr = js_string_from_bytes(out.as_ptr(), out.len() as u32);
            JSValue::string_ptr(ptr)
        }
    }
}

/// Build packed keys (null-separated) and a shape_id from column names.
fn build_packed_keys(column_names: &[String]) -> (Vec<u8>, u32) {
    let mut packed = Vec::new();
    let mut shape_id: u32 = 0x5143_0000; // "SQ" prefix
    for (i, name) in column_names.iter().enumerate() {
        if i > 0 {
            packed.push(0u8);
        }
        packed.extend_from_slice(name.as_bytes());
        // Simple hash for shape_id
        for &b in name.as_bytes() {
            shape_id = shape_id.wrapping_mul(31).wrapping_add(b as u32);
        }
    }
    shape_id = shape_id.wrapping_add(column_names.len() as u32);
    (packed, shape_id)
}

/// new Database(filename) -> Database
///
/// Open or create a SQLite database.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_open(filename_ptr: *const StringHeader) -> Handle {
    let filename = match string_from_header(filename_ptr) {
        Some(f) => f,
        None => return -1,
    };

    let conn = if filename == ":memory:" {
        Connection::open_in_memory()
    } else {
        // On iOS/Android, resolve relative paths to a writable directory
        // (the CWD is typically the read-only app bundle on mobile platforms)
        let resolved = if !filename.starts_with('/') && !filename.starts_with(':') {
            #[cfg(target_os = "ios")]
            {
                extern "C" {
                    fn getenv(name: *const i8) -> *const i8;
                }
                let home = getenv(b"HOME\0".as_ptr() as *const i8);
                if !home.is_null() {
                    let home_str = std::ffi::CStr::from_ptr(home).to_str().unwrap_or("");
                    let docs = format!("{}/Documents", home_str);
                    let _ = std::fs::create_dir_all(&docs);
                    format!("{}/{}", docs, filename)
                } else {
                    filename.clone()
                }
            }
            #[cfg(not(target_os = "ios"))]
            {
                filename.clone()
            }
        } else {
            filename.clone()
        };
        Connection::open(&resolved)
    };

    match conn {
        Ok(c) => register_handle(SqliteDbHandle {
            conn: Mutex::new(c),
        }),
        Err(_) => -1,
    }
}

/// db.exec(sql) -> Database
///
/// Execute one or more SQL statements.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_exec(db_handle: Handle, sql_ptr: *const StringHeader) -> i32 {
    let sql = match string_from_header(sql_ptr) {
        Some(s) => s,
        None => return 0,
    };

    if let Some(db) = get_handle::<SqliteDbHandle>(db_handle) {
        if let Ok(conn) = db.conn.lock() {
            return if conn.execute_batch(&sql).is_ok() {
                1
            } else {
                0
            };
        }
    }
    0
}

/// db.prepare(sql) -> Statement
///
/// Create a prepared statement.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_prepare(
    db_handle: Handle,
    sql_ptr: *const StringHeader,
) -> Handle {
    let sql = match string_from_header(sql_ptr) {
        Some(s) => s,
        None => return -1,
    };

    // Verify the SQL is valid
    if let Some(db) = get_handle::<SqliteDbHandle>(db_handle) {
        if let Ok(conn) = db.conn.lock() {
            if conn.prepare(&sql).is_ok() {
                return register_handle(SqliteStmtHandle {
                    sql,
                    db_handle,
                    raw_mode: AtomicBool::new(false),
                });
            }
        }
    }
    -1
}

/// stmt.raw([toggle]) -> stmt
///
/// Toggle raw mode on the statement and return the same handle so
/// `stmt.raw().all(...)` chains. Raw mode makes subsequent `.all()` /
/// `.get()` return rows as arrays of column values (in declared
/// column order) instead of objects keyed by column name.
///
/// drizzle's `PreparedQuery.values()` chains
/// `this.stmt.raw().all(...params)` to get back row arrays it then
/// hands to `mapResultRow(fields, row, joinsNotNullableMap)`. Without
/// this method `stmt.raw` is undefined and the call surfaces as
/// `(number).all is not a function` deeper in the chain because perry
/// returns a number sentinel when calling `undefined()` instead of
/// throwing immediately. Refs #643.
///
/// Argument handling: drizzle only ever uses the no-arg form. Real
/// better-sqlite3 also accepts `.raw(false)` to disable. We don't
/// thread the toggle through the codegen's NativeMethodCall dispatch
/// yet (it would need an `NA_F64` slot), so the no-arg form is the
/// only path. Conservative: always enable on call.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_stmt_raw(stmt_handle: Handle) -> Handle {
    if let Some(stmt) = get_handle::<SqliteStmtHandle>(stmt_handle) {
        stmt.raw_mode.store(true, Ordering::Relaxed);
    }
    stmt_handle
}

/// Extract SQLite parameters from a NaN-boxed array
unsafe fn params_from_array(arr_ptr: *const ArrayHeader) -> Vec<Box<dyn rusqlite::ToSql>> {
    if arr_ptr.is_null() {
        return vec![];
    }
    // Codegen pads omitted-arg slots with TAG_UNDEFINED bits when a stmt
    // method is called with no params (e.g. `stmt.run()` / `stmt.all()`).
    // Those bits look like a non-null pointer but actually carry the
    // 0x7FFC NaN-box tag in the high 16; dereferencing as ArrayHeader is
    // UB and reads a garbage `length` that crashes the loop below.
    // Treat any value with non-zero upper-16 as "no params".
    let upper16 = (arr_ptr as usize as u64) >> 48;
    if upper16 != 0 {
        return vec![];
    }
    let len = (*arr_ptr).length as usize;
    let elements = (arr_ptr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::with_capacity(len);

    for i in 0..len {
        let val = *elements.add(i);
        let bits = val.to_bits();

        const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
        const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
        const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
        const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
        const STRING_TAG: u64 = 0x7FFF;
        const INT32_TAG: u64 = 0x7FFE;

        let top16 = bits >> 48;

        if bits == TAG_NULL || bits == TAG_UNDEFINED {
            params.push(Box::new(rusqlite::types::Null));
        } else if bits == TAG_TRUE {
            params.push(Box::new(1i64));
        } else if bits == TAG_FALSE {
            params.push(Box::new(0i64));
        } else if top16 == STRING_TAG {
            // String: extract pointer
            let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
            if let Some(s) = string_from_header(ptr) {
                params.push(Box::new(s));
            } else {
                params.push(Box::new(rusqlite::types::Null));
            }
        } else if top16 == INT32_TAG {
            let n = (bits & 0xFFFF_FFFF) as i32;
            params.push(Box::new(n as i64));
        } else {
            // Regular f64 number
            if val.fract() == 0.0 && val >= i64::MIN as f64 && val <= i64::MAX as f64 {
                params.push(Box::new(val as i64));
            } else {
                params.push(Box::new(val));
            }
        }
    }

    params
}

/// stmt.run(...params) -> RunResult
///
/// Execute a prepared statement with parameters.
/// Returns { changes: number, lastInsertRowid: number }
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_stmt_run(
    stmt_handle: Handle,
    params_arr: *const ArrayHeader,
) -> *mut ObjectHeader {
    let sqlite_params = params_from_array(params_arr);

    if let Some(stmt) = get_handle::<SqliteStmtHandle>(stmt_handle) {
        if let Some(result) = with_sqlite_connection(stmt.db_handle, |conn| {
            let param_refs: Vec<&dyn rusqlite::ToSql> =
                sqlite_params.iter().map(|p| p.as_ref()).collect();

            if let Ok(changes) = conn.execute(&stmt.sql, param_refs.as_slice()) {
                let last_id = conn.last_insert_rowid();
                let keys = vec!["changes".to_string(), "lastInsertRowid".to_string()];
                let (packed_keys, shape_id) = build_packed_keys(&keys);
                let result = js_object_alloc_with_shape(
                    shape_id,
                    2,
                    packed_keys.as_ptr(),
                    packed_keys.len() as u32,
                );
                js_object_set_field(result, 0, JSValue::number(changes as f64));
                js_object_set_field(result, 1, JSValue::number(last_id as f64));
                return result;
            }
            std::ptr::null_mut()
        }) {
            return result;
        }
    }

    std::ptr::null_mut()
}

/// stmt.get(...params) -> Row | undefined
///
/// Get a single row from a query. Returns f64 (NaN-boxed bits) instead
/// of JSValue to avoid SysV AMD64 ABI mismatch on x86_64 (JSValue's
/// `#[repr(transparent)] u64` returns in RAX but LLVM reads from XMM0
/// when the call site declares a `double` return).
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_stmt_get(
    stmt_handle: Handle,
    params_arr: *const ArrayHeader,
) -> f64 {
    let sqlite_params = params_from_array(params_arr);

    if let Some(stmt) = get_handle::<SqliteStmtHandle>(stmt_handle) {
        let raw = stmt.raw_mode.load(Ordering::Relaxed);
        if let Some(result) = with_sqlite_connection(stmt.db_handle, |conn| {
            let param_refs: Vec<&dyn rusqlite::ToSql> =
                sqlite_params.iter().map(|p| p.as_ref()).collect();

            if let Ok(mut prepared) = conn.prepare(&stmt.sql) {
                let column_names: Vec<String> = prepared
                    .column_names()
                    .iter()
                    .map(|s| s.to_string())
                    .collect();

                let mut rows = prepared.query(param_refs.as_slice());
                if let Ok(ref mut rows) = rows {
                    if let Ok(Some(row)) = rows.next() {
                        if raw {
                            let row_arr = js_array_alloc(0);
                            for (idx, _) in column_names.iter().enumerate() {
                                let value: SqliteValue = row.get(idx).unwrap_or(SqliteValue::Null);
                                js_array_push(row_arr, sqlite_value_to_jsvalue(&value));
                            }
                            return f64::from_bits(JSValue::object_ptr(row_arr as *mut u8).bits());
                        }
                        let (packed_keys, shape_id) = build_packed_keys(&column_names);
                        let obj = js_object_alloc_with_shape(
                            shape_id,
                            column_names.len() as u32,
                            packed_keys.as_ptr(),
                            packed_keys.len() as u32,
                        );

                        for (idx, _name) in column_names.iter().enumerate() {
                            let value: SqliteValue = row.get(idx).unwrap_or(SqliteValue::Null);
                            js_object_set_field(obj, idx as u32, sqlite_value_to_jsvalue(&value));
                        }

                        return f64::from_bits(JSValue::object_ptr(obj as *mut u8).bits());
                    }
                }
            }
            f64::from_bits(JSValue::undefined().bits())
        }) {
            return result;
        }
    }

    f64::from_bits(JSValue::undefined().bits())
}

/// stmt.all(...params) -> Row[]
///
/// Get all rows from a query.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_stmt_all(
    stmt_handle: Handle,
    params_arr: *const ArrayHeader,
) -> *mut ArrayHeader {
    let sqlite_params = params_from_array(params_arr);
    let result_array = js_array_alloc(0);

    if let Some(stmt) = get_handle::<SqliteStmtHandle>(stmt_handle) {
        let raw = stmt.raw_mode.load(Ordering::Relaxed);
        let _ = with_sqlite_connection(stmt.db_handle, |conn| {
            let param_refs: Vec<&dyn rusqlite::ToSql> =
                sqlite_params.iter().map(|p| p.as_ref()).collect();

            if let Ok(mut prepared) = conn.prepare(&stmt.sql) {
                let column_names: Vec<String> = prepared
                    .column_names()
                    .iter()
                    .map(|s| s.to_string())
                    .collect();

                // Only build the per-row object shape in non-raw
                // mode. In raw mode each row is its own array of
                // column values; no per-row object shape needed.
                let object_shape = if raw {
                    None
                } else {
                    Some(build_packed_keys(&column_names))
                };

                let mut rows = prepared.query(param_refs.as_slice());
                if let Ok(ref mut rows) = rows {
                    while let Ok(Some(row)) = rows.next() {
                        if raw {
                            let row_arr = js_array_alloc(0);
                            for (idx, _) in column_names.iter().enumerate() {
                                let value: SqliteValue = row.get(idx).unwrap_or(SqliteValue::Null);
                                js_array_push(row_arr, sqlite_value_to_jsvalue(&value));
                            }
                            js_array_push(result_array, JSValue::object_ptr(row_arr as *mut u8));
                            continue;
                        }
                        let (packed_keys, shape_id) = object_shape.as_ref().unwrap();
                        let obj = js_object_alloc_with_shape(
                            *shape_id,
                            column_names.len() as u32,
                            packed_keys.as_ptr(),
                            packed_keys.len() as u32,
                        );

                        for (idx, _name) in column_names.iter().enumerate() {
                            let value: SqliteValue = row.get(idx).unwrap_or(SqliteValue::Null);
                            js_object_set_field(obj, idx as u32, sqlite_value_to_jsvalue(&value));
                        }

                        js_array_push(result_array, JSValue::object_ptr(obj as *mut u8));
                    }
                }
            }
        });
    }

    result_array
}

/// db.pragma(pragma, value?) -> any
///
/// Execute a PRAGMA statement.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_pragma(
    db_handle: Handle,
    pragma_ptr: *const StringHeader,
    value_ptr: *const StringHeader,
) -> *mut StringHeader {
    let pragma = match string_from_header(pragma_ptr) {
        Some(p) => p,
        None => return std::ptr::null_mut(),
    };

    let value = string_from_header(value_ptr);

    if let Some(db) = get_handle::<SqliteDbHandle>(db_handle) {
        if let Ok(conn) = db.conn.lock() {
            let sql = if let Some(v) = value {
                format!("PRAGMA {} = {}", pragma, v)
            } else {
                format!("PRAGMA {}", pragma)
            };

            if let Ok(mut stmt) = conn.prepare(&sql) {
                let mut rows = stmt.query([]);
                if let Ok(ref mut rows) = rows {
                    if let Ok(Some(row)) = rows.next() {
                        let result: String = row.get(0).unwrap_or_default();
                        return js_string_from_bytes(result.as_ptr(), result.len() as u32);
                    }
                }
            }
        }
    }

    std::ptr::null_mut()
}

/// The transaction wrapper function — called when the returned closure is invoked.
/// Captures: [0] = db_handle (as f64), [1] = original closure ptr (as i64)
unsafe extern "C" fn sqlite_tx_wrapper(
    wrapper_closure: *const perry_runtime::ClosureHeader,
    arg0: f64,
) -> f64 {
    use perry_runtime::closure::{
        js_closure_call1, js_closure_get_capture_f64, js_closure_get_capture_ptr,
    };

    let db_handle_f64 = js_closure_get_capture_f64(wrapper_closure, 0);
    let db_handle = db_handle_f64 as i64;
    let original_closure =
        js_closure_get_capture_ptr(wrapper_closure, 1) as *const perry_runtime::ClosureHeader;

    // BEGIN
    js_sqlite_begin_transaction(db_handle);

    // Call original closure with argument
    let result = js_closure_call1(original_closure, arg0);

    // COMMIT
    js_sqlite_commit(db_handle);

    result
}

/// db.transaction(fn) -> wrapping closure
///
/// Returns a closure that wraps fn in BEGIN/COMMIT.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_transaction(
    db_handle: Handle,
    closure_ptr: i64,
) -> *mut perry_runtime::ClosureHeader {
    use perry_runtime::closure::{
        js_closure_alloc, js_closure_set_capture_f64, js_closure_set_capture_ptr,
    };

    let wrapper = js_closure_alloc(sqlite_tx_wrapper as *const u8, 2);
    js_closure_set_capture_f64(wrapper, 0, db_handle as f64);
    js_closure_set_capture_ptr(wrapper, 1, closure_ptr);

    wrapper
}

/// Begin a transaction.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_begin_transaction(db_handle: Handle) -> i32 {
    if let Some(db) = get_handle::<SqliteDbHandle>(db_handle) {
        if let Ok(conn) = db.conn.lock() {
            return if conn.execute("BEGIN TRANSACTION", []).is_ok() {
                1
            } else {
                0
            };
        }
    }
    0
}

/// Commit a transaction.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_commit(db_handle: Handle) -> i32 {
    if let Some(db) = get_handle::<SqliteDbHandle>(db_handle) {
        if let Ok(conn) = db.conn.lock() {
            return if conn.execute("COMMIT", []).is_ok() {
                1
            } else {
                0
            };
        }
    }
    0
}

/// Rollback a transaction.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_rollback(db_handle: Handle) -> i32 {
    if let Some(db) = get_handle::<SqliteDbHandle>(db_handle) {
        if let Ok(conn) = db.conn.lock() {
            return if conn.execute("ROLLBACK", []).is_ok() {
                1
            } else {
                0
            };
        }
    }
    0
}

/// db.close() -> void
///
/// Close the database connection.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_close(db_handle: Handle) -> i32 {
    // The connection will be closed when the handle is dropped
    // For now, we just verify the handle is valid
    if get_handle::<SqliteDbHandle>(db_handle).is_some() {
        1
    } else {
        0
    }
}

/// db.inTransaction -> boolean
///
/// Check if currently in a transaction.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_in_transaction(db_handle: Handle) -> i32 {
    if let Some(db) = get_handle::<SqliteDbHandle>(db_handle) {
        if let Ok(conn) = db.conn.lock() {
            // SQLite's autocommit mode is off when in a transaction
            return if !conn.is_autocommit() { 1 } else { 0 };
        }
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_call(
    _path_value: f64,
    _options_value: f64,
) -> Handle {
    throw_construct_required()
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_new(
    path_value: f64,
    options_value: f64,
) -> Handle {
    let path = string_from_value(path_value, "path");
    let options = parse_node_sqlite_options(options_value);
    let open = options.open;
    let handle = register_handle(NodeSqliteDbHandle {
        conn: Mutex::new(None),
        path,
        read_only: options.read_only,
        enable_foreign_keys: options.enable_foreign_keys,
        enable_dqs: options.enable_dqs,
        timeout_ms: options.timeout_ms,
        read_bigints: options.read_bigints,
        return_arrays: options.return_arrays,
        allow_bare_named_parameters: options.allow_bare_named_parameters,
        allow_unknown_named_parameters: options.allow_unknown_named_parameters,
        defensive: options.defensive,
        initial_limits: options.initial_limits,
        limits_handle: Mutex::new(None),
    });
    if open {
        js_node_sqlite_database_sync_open(handle);
    }
    handle
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_open(db_handle: Handle) -> i32 {
    let db = get_handle::<NodeSqliteDbHandle>(db_handle)
        .unwrap_or_else(|| throw_invalid_state("Database is not open"));
    {
        let conn = db
            .conn
            .lock()
            .unwrap_or_else(|_| throw_invalid_state("Database is not open"));
        if conn.is_some() {
            drop(conn);
            throw_invalid_state("Database is already open");
        }
    }
    let opened = match open_node_sqlite_connection(db) {
        Ok(opened) => opened,
        Err(err) => throw_sqlite_error(&err.to_string()),
    };
    let mut conn = db
        .conn
        .lock()
        .unwrap_or_else(|_| throw_invalid_state("Database is not open"));
    *conn = Some(opened);
    1
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_close(db_handle: Handle) -> i32 {
    let db = get_handle::<NodeSqliteDbHandle>(db_handle)
        .unwrap_or_else(|| throw_invalid_state("Database is not open"));
    let mut conn = db
        .conn
        .lock()
        .unwrap_or_else(|_| throw_invalid_state("Database is not open"));
    if conn.is_none() {
        drop(conn);
        throw_invalid_state("Database is not open");
    }
    *conn = None;
    1
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_dispose(db_handle: Handle) -> i32 {
    if let Some(db) = get_handle::<NodeSqliteDbHandle>(db_handle) {
        if let Ok(mut conn) = db.conn.lock() {
            *conn = None;
        }
    }
    1
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_is_open(db_handle: Handle) -> f64 {
    let is_open = get_handle::<NodeSqliteDbHandle>(db_handle)
        .and_then(|db| db.conn.lock().ok().map(|conn| conn.is_some()))
        .unwrap_or(false);
    bool_f64(is_open)
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_is_transaction(db_handle: Handle) -> f64 {
    with_open_node_connection(db_handle, |conn| bool_f64(!conn.is_autocommit()))
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_exec(
    db_handle: Handle,
    sql_value: f64,
) -> i32 {
    ensure_open_node_database(db_handle);
    let sql = string_from_value(sql_value, "sql");
    let result = with_open_node_connection(db_handle, |conn| node_sqlite_exec_batch(conn, &sql));
    match result {
        Ok(_) => 1,
        Err(err) => throw_sqlite_error(&err),
    }
}

unsafe fn validate_statement_options(options_value: f64) {
    let js = value_from_f64(options_value);
    if js.is_undefined() {
        return;
    }
    if js.is_null() || !is_object_like(options_value) {
        throw_type("The \"options\" argument must be an object");
    }
    let _ = bool_option(options_value, "readBigInts", false);
    let _ = bool_option(options_value, "returnArrays", false);
    let _ = bool_option(options_value, "allowBareNamedParameters", true);
    let _ = bool_option(options_value, "allowUnknownNamedParameters", false);
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_prepare(
    db_handle: Handle,
    sql_value: f64,
    options_value: f64,
) -> Handle {
    ensure_open_node_database(db_handle);
    let sql = string_from_value(sql_value, "sql");
    validate_statement_options(options_value);
    let result = with_open_node_connection(db_handle, |conn| {
        conn.prepare(&sql)
            .map(|_| ())
            .map_err(|err| err.to_string())
    });
    if let Err(err) = result {
        throw_sqlite_error(&err);
    }
    register_handle(SqliteStmtHandle {
        sql,
        db_handle,
        raw_mode: AtomicBool::new(false),
    })
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_location(
    db_handle: Handle,
    db_name_value: f64,
) -> f64 {
    ensure_open_node_database(db_handle);
    let db_name = if value_from_f64(db_name_value).is_undefined() {
        "main".to_string()
    } else {
        string_from_value(db_name_value, "dbName")
    };
    let c_name = CString::new(db_name)
        .unwrap_or_else(|_| throw_type("The \"dbName\" argument must not contain null bytes"));
    with_open_node_connection(db_handle, |conn| {
        let filename =
            unsafe { rusqlite::ffi::sqlite3_db_filename(conn.handle(), c_name.as_ptr()) };
        if filename.is_null() {
            return null_f64();
        }
        let filename = unsafe { CStr::from_ptr(filename) }.to_str().unwrap_or("");
        if filename.is_empty() {
            null_f64()
        } else {
            let ptr = js_string_from_bytes(filename.as_ptr(), filename.len() as u32);
            f64::from_bits(JSValue::string_ptr(ptr).bits())
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_limits(db_handle: Handle) -> Handle {
    ensure_open_node_database(db_handle);
    let db = get_handle::<NodeSqliteDbHandle>(db_handle)
        .unwrap_or_else(|| throw_invalid_state("Database is not open"));
    let mut limits_handle = db
        .limits_handle
        .lock()
        .unwrap_or_else(|_| throw_invalid_state("Database is not open"));
    if let Some(handle) = *limits_handle {
        return handle;
    }
    let handle = register_handle(NodeSqliteLimitsHandle { db_handle });
    *limits_handle = Some(handle);
    handle
}

pub unsafe fn dispatch_node_sqlite_database_method(
    handle: Handle,
    method: &str,
    args: &[f64],
) -> Option<f64> {
    if js_node_sqlite_is_database_sync_handle(handle) == 0 {
        return None;
    }
    let arg0 = args.first().copied().unwrap_or_else(undefined_f64);
    let arg1 = args.get(1).copied().unwrap_or_else(undefined_f64);
    match method {
        "open" => {
            js_node_sqlite_database_sync_open(handle);
            Some(undefined_f64())
        }
        "close" => {
            js_node_sqlite_database_sync_close(handle);
            Some(undefined_f64())
        }
        "__perry_dispose__" | "@@__perry_wk_dispose" => {
            js_node_sqlite_database_sync_dispose(handle);
            Some(undefined_f64())
        }
        "exec" => {
            js_node_sqlite_database_sync_exec(handle, arg0);
            Some(undefined_f64())
        }
        "prepare" => {
            let stmt = js_node_sqlite_database_sync_prepare(handle, arg0, arg1);
            Some(js_nanbox_pointer(stmt))
        }
        "location" => Some(js_node_sqlite_database_sync_location(handle, arg0)),
        _ => None,
    }
}

pub unsafe fn dispatch_node_sqlite_database_property(
    handle: Handle,
    property_name: &str,
) -> Option<f64> {
    if js_node_sqlite_is_database_sync_handle(handle) == 0 {
        return None;
    }
    match property_name {
        "isOpen" => Some(js_node_sqlite_database_sync_is_open(handle)),
        "isTransaction" => Some(js_node_sqlite_database_sync_is_transaction(handle)),
        "limits" => Some(js_nanbox_pointer(js_node_sqlite_database_sync_limits(
            handle,
        ))),
        "open"
        | "close"
        | "exec"
        | "prepare"
        | "location"
        | "__perry_dispose__"
        | "@@__perry_wk_dispose" => {
            extern "C" {
                fn js_class_method_bind(
                    instance: f64,
                    method_name_ptr: *const u8,
                    method_name_len: usize,
                ) -> f64;
            }
            let instance = js_nanbox_pointer(handle);
            Some(js_class_method_bind(
                instance,
                property_name.as_ptr(),
                property_name.len(),
            ))
        }
        _ => None,
    }
}

pub unsafe fn dispatch_node_sqlite_limits_property(
    handle: Handle,
    property_name: &str,
) -> Option<f64> {
    let limits = get_handle::<NodeSqliteLimitsHandle>(handle)?;
    let (_, limit) = node_sqlite_limit(property_name)?;
    Some(with_open_node_connection(limits.db_handle, |conn| {
        JSValue::int32(conn.limit(limit))
    }))
    .map(|value| f64::from_bits(value.bits()))
}

pub unsafe fn dispatch_node_sqlite_limits_set(
    handle: Handle,
    property_name: &str,
    value: f64,
) -> bool {
    let Some(limits) = get_handle::<NodeSqliteLimitsHandle>(handle) else {
        return false;
    };
    let Some((_, limit)) = node_sqlite_limit(property_name) else {
        return false;
    };
    let new_value = non_negative_i32_value(value_from_f64(value), property_name, true);
    with_open_node_connection(limits.db_handle, |conn| {
        conn.set_limit(limit, new_value);
    });
    true
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_is_database_sync_handle(handle: Handle) -> i32 {
    if get_handle::<NodeSqliteDbHandle>(handle).is_some() {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_is_limits_handle(handle: Handle) -> i32 {
    if get_handle::<NodeSqliteLimitsHandle>(handle).is_some() {
        1
    } else {
        0
    }
}

/// Returns `1` if `handle` currently resolves to a `SqliteDbHandle` in
/// this crate's handle registry, `0` otherwise. Used by the V8 bridge
/// in `perry-jsruntime::bridge::native_object_to_v8` to decide whether
/// to materialize a `v8::Object` proxy with `prepare`/`exec`/etc.
/// method callbacks when a sqlite Database crosses the native→V8
/// boundary (drizzle's `BetterSQLiteSession` does
/// `this.client.prepare(query.sql)` from session.js — refs #1022).
///
/// Mirrors `perry-ext-better-sqlite3::js_sqlite_is_db_handle`. The
/// duplicate-symbol resolution at link time picks one impl; whichever
/// crate's `js_sqlite_open` registered the handle is the same impl
/// whose `is_db_handle` answers the membership check (each crate
/// keeps its own registry).
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_is_db_handle(handle: Handle) -> i32 {
    if get_handle::<SqliteDbHandle>(handle).is_some() {
        1
    } else {
        0
    }
}

/// Returns `1` if `handle` currently resolves to a `SqliteStmtHandle`
/// in this crate's handle registry, `0` otherwise. Mirror of
/// `js_sqlite_is_db_handle` for the Statement side — drizzle's
/// PreparedQuery calls `stmt.run(...)` / `stmt.all(...)` /
/// `stmt.get(...)` / `stmt.raw().all(...)` on the handle returned from
/// `client.prepare(...)`. Refs #1022.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_is_stmt_handle(handle: Handle) -> i32 {
    if get_handle::<SqliteStmtHandle>(handle).is_some() {
        1
    } else {
        0
    }
}
