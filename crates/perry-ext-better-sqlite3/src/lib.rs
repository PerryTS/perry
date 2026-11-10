//! Native bindings for the npm `better-sqlite3` package.
//!
//! First database driver port (Phase 5 step 10) and acceptance
//! test for perry-ffi's v0.5.x `JsValue` + object/array surface.
//! Functionally equivalent to `crates/perry-stdlib/src/sqlite.rs`
//! minus `db.transaction(fn)` — that needs perry-ffi's closure
//! invocation surface, which is the next-after-this expansion.
//! Users can still use explicit `BEGIN` / `COMMIT` /
//! `ROLLBACK` calls (those work via `db.exec()`).
//!
//! This crate is also the canonical reference for #424 (Tursodb
//! port) — `perryts/tursodb-bindings` will look almost identical
//! once it lands, with `rusqlite::Connection` swapped for
//! `tursodb::Connection`.

mod dispatch_ext;

use perry_ffi::{
    alloc_bigint_from_str, alloc_string, bigint_value, build_object_shape, drop_handle, get_handle,
    js_array_alloc, js_array_push, js_object_alloc_with_shape, js_object_set_field, read_string,
    register_handle, ArrayHeader, ErrorKind, Handle, JsString, JsValue, ObjectHeader, StringHeader,
    TransientRootScope, TransientRootedNanbox,
};
use rusqlite::{types::Value as SqliteValue, Connection};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

const DATABASE_NOT_OPEN: &str = "The database connection is not open";

#[cold]
fn throw_database_not_open() -> ! {
    perry_ffi::throw_with_code(DATABASE_NOT_OPEN, "", ErrorKind::TypeError)
}

fn require_database_open(db_handle: Handle) {
    if get_handle::<SqliteDbHandle>(db_handle).is_none() {
        throw_database_not_open();
    }
}

fn require_statement_database_open(stmt_handle: Handle) {
    let stmt =
        get_handle::<SqliteStmtHandle>(stmt_handle).unwrap_or_else(|| throw_database_not_open());
    require_database_open(stmt.db_handle);
}

pub struct SqliteDbHandle {
    pub conn: Mutex<Connection>,
}

pub struct SqliteStmtHandle {
    pub sql: String,
    pub db_handle: Handle,
    /// Per-statement "raw" mode flag — `stmt.raw([toggle])` flips this.
    /// When true, `stmt.all(...)` returns row arrays instead of objects
    /// and `stmt.get(...)` returns a single row array. drizzle's
    /// `PreparedQuery.values()` (the standard with-fields path) calls
    /// `this.stmt.raw().all(...params)` to get raw rows for its own
    /// `mapResultRow(fields, row, joinsNotNullableMap)` mapping.
    pub raw_mode: AtomicBool,
    /// Per-statement "safe integers" flag — `stmt.safeIntegers([toggle])`
    /// flips this. When true, an INTEGER column comes back as a JS BigInt
    /// instead of a number, which is the only way to carry a value outside
    /// the double-safe range without losing bits. ActiveRecord's SQLite
    /// adapter turns it on for a statement whose declared column type
    /// matches /bigint/i (`_maybeEnableReadBigInts`).
    pub safe_integers: AtomicBool,
}

unsafe fn read_str(ptr: *const StringHeader) -> Option<String> {
    let handle = JsString::from_raw(ptr as *mut StringHeader);
    read_string(handle).map(String::from)
}

/// Convert a SQLite column value to a perry-ffi `JsValue`.
/// `safe_integers` reports an INTEGER column as a BigInt.
///
/// `from_number` loses bits above 2^53, so a statement that opted into safe
/// integers must not take that path for any integer, not only a large one:
/// better-sqlite3 returns BigInt for every INTEGER column once the flag is
/// set, and a caller that tests `typeof v === "bigint"` has to see it
/// uniformly.
fn sqlite_value_to_jsvalue_with(value: &SqliteValue, safe_integers: bool) -> JsValue {
    match value {
        SqliteValue::Null => JsValue::NULL,
        SqliteValue::Integer(n) => {
            if safe_integers {
                bigint_value(alloc_bigint_from_str(&n.to_string()))
            } else if *n >= i32::MIN as i64 && *n <= i32::MAX as i64 {
                JsValue::from_int32(*n as i32)
            } else {
                JsValue::from_number(*n as f64)
            }
        }
        SqliteValue::Real(n) => JsValue::from_number(*n),
        SqliteValue::Text(s) => JsValue::from_string_ptr(alloc_string(s).as_raw()),
        SqliteValue::Blob(b) => {
            // Hex-encode blobs as a string (matches perry-stdlib's
            // existing convention; avoids pulling in `hex`).
            const HEX: &[u8; 16] = b"0123456789abcdef";
            let mut out = Vec::with_capacity(b.len() * 2);
            for &byte in b {
                out.push(HEX[(byte >> 4) as usize]);
                out.push(HEX[(byte & 0x0f) as usize]);
            }
            // SAFETY: HEX bytes are ASCII, output is valid UTF-8.
            let s = unsafe { std::str::from_utf8_unchecked(&out) };
            JsValue::from_string_ptr(alloc_string(s).as_raw())
        }
    }
}

/// Read parameters out of a JS array of mixed-type values. The
/// codegen pads omitted-arg slots with `TAG_UNDEFINED` bits which
/// look like a non-null pointer; treat any pointer with non-zero
/// upper-16 bits as "no params" (matches perry-stdlib's existing
/// behavior).
unsafe fn params_from_array(arr_ptr: *const ArrayHeader) -> Vec<Box<dyn rusqlite::ToSql>> {
    if arr_ptr.is_null() {
        return vec![];
    }
    let upper16 = (arr_ptr as usize as u64) >> 48;
    if upper16 != 0 {
        return vec![];
    }
    let len = (*arr_ptr).length as usize;
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::with_capacity(len);

    for i in 0..len {
        let bits = perry_ffi::js_array_get(arr_ptr, i as u32).bits();
        let val = JsValue::from_bits(bits);

        if val.is_null() || val.is_undefined() {
            params.push(Box::new(rusqlite::types::Null));
        } else if val.is_bool() {
            params.push(Box::new(if val.to_bool() { 1i64 } else { 0i64 }));
        } else if val.is_string() {
            let ptr = val.as_string_ptr();
            if let Some(s) = read_str(ptr) {
                params.push(Box::new(s));
            } else {
                params.push(Box::new(rusqlite::types::Null));
            }
        } else if val.is_int32() {
            params.push(Box::new(val.to_int32() as i64));
        } else {
            // Real f64 number — coerce to int64 if it's a whole
            // number in range, else pass through as f64.
            let n = val.to_number();
            if n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
                params.push(Box::new(n as i64));
            } else {
                params.push(Box::new(n));
            }
        }
    }

    params
}

/// `new Database(filename)` — open or create a SQLite database.
/// Returns a Handle to the connection, or `-1` on error.
///
/// # Safety
///
/// `filename_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_open(filename_ptr: *const StringHeader) -> Handle {
    let Some(filename) = read_str(filename_ptr) else {
        return -1;
    };

    let conn = if filename == ":memory:" {
        Connection::open_in_memory()
    } else {
        // Mobile-platform path resolution (sandboxed CWD on iOS)
        // is intentionally NOT replicated here — that's
        // Phase 5-batch UI work and lives in the perry-ext-ios
        // wrapper. CLI / server programs work fine with absolute
        // paths or paths relative to a writable CWD.
        Connection::open(&filename)
    };

    match conn {
        Ok(c) => {
            dispatch_ext::ensure_registered();
            register_handle(SqliteDbHandle {
                conn: Mutex::new(c),
            })
        }
        Err(_) => -1,
    }
}

/// `db.exec(sql)` — execute one or more SQL statements (no
/// parameters). Returns `1` on success, `0` on error.
///
/// # Safety
///
/// `sql_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_exec(db_handle: Handle, sql_ptr: *const StringHeader) -> i32 {
    let Some(sql) = read_str(sql_ptr) else {
        return 0;
    };
    require_database_open(db_handle);
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

/// `db.prepare(sql)` — prepare a statement. Returns a Handle to
/// the statement, or `-1` on error.
///
/// # Safety
///
/// `sql_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_prepare(
    db_handle: Handle,
    sql_ptr: *const StringHeader,
) -> Handle {
    let Some(sql) = read_str(sql_ptr) else {
        return -1;
    };
    require_database_open(db_handle);
    if let Some(db) = get_handle::<SqliteDbHandle>(db_handle) {
        if let Ok(conn) = db.conn.lock() {
            if conn.prepare(&sql).is_ok() {
                return register_handle(SqliteStmtHandle {
                    sql,
                    db_handle,
                    raw_mode: AtomicBool::new(false),
                    safe_integers: AtomicBool::new(false),
                });
            }
        }
    }
    -1
}

/// `stmt.raw([toggle])` — toggle raw mode for the statement. Returns
/// the same statement handle so `stmt.raw().all(...)` chains. Raw mode
/// makes subsequent `stmt.all(...)` / `stmt.get(...)` return rows as
/// arrays of column values instead of objects keyed by column name.
///
/// drizzle's `PreparedQuery.values()` chains `this.stmt.raw().all(...)`
/// to get back row arrays it then maps via
/// `mapResultRow(fields, row, joinsNotNullableMap)` against declared
/// field projections. Without this method, `stmt.raw` is undefined and
/// `stmt.raw().all(...)` surfaces as `(number).all is not a function`
/// because perry's runtime returns a number sentinel when calling
/// `undefined()` instead of throwing immediately.
///
/// Argument handling: drizzle only ever calls `.raw()` with no arg —
/// real better-sqlite3 also accepts an explicit boolean to disable
/// raw mode (`.raw(false)`). We don't carry the toggle through the
/// codegen's NativeMethodCall dispatch yet (it requires an
/// `NA_F64`/`NA_BOOL` slot), so the no-arg form is the only path
/// exercised. Conservative behavior: always enable on call. If a
/// downstream use needs explicit `.raw(false)`, extend the dispatch
/// arg list and read the toggle here.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_stmt_raw(stmt_handle: Handle) -> Handle {
    if let Some(stmt) = get_handle::<SqliteStmtHandle>(stmt_handle) {
        stmt.raw_mode.store(true, Ordering::Relaxed);
    }
    stmt_handle
}

/// `stmt.safeIntegers([toggle])` — read INTEGER columns as BigInt. Returns
/// the same statement handle so the call chains, as `stmt.raw()` does.
///
/// # Safety
///
/// `stmt_handle` must be a registry id; a foreign or dead one is ignored.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_stmt_safe_integers(stmt_handle: Handle, toggle: f64) -> Handle {
    // better-sqlite3 defaults the argument to true: `safeIntegers()` with no
    // argument turns the mode ON, so an absent argument is not "off".
    let toggle = JsValue::from_bits(toggle.to_bits());
    let on = toggle.is_undefined() || toggle.to_bool();
    if let Some(stmt) = get_handle::<SqliteStmtHandle>(stmt_handle) {
        stmt.safe_integers.store(on, Ordering::Relaxed);
    }
    stmt_handle
}

/// `stmt.columns()` — one record per result column, with better-sqlite3's
/// five keys: `name`, `column`, `table`, `database`, `type`.
///
/// `name` is the column's name in the result set and `type` its declared
/// type; the other three name where the value came from and are null for a
/// column that is an expression rather than a table column, which is what
/// better-sqlite3 reports too. ActiveRecord's SQLite adapter reads `type` to
/// decide whether to turn on safe integers.
///
/// # Safety
///
/// `stmt_handle` must be a registry id; a foreign or dead one yields an
/// empty array.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_stmt_columns(stmt_handle: Handle) -> *mut ArrayHeader {
    require_statement_database_open(stmt_handle);
    let Some(stmt) = get_handle::<SqliteStmtHandle>(stmt_handle) else {
        return js_array_alloc(0);
    };
    let Some(db) = get_handle::<SqliteDbHandle>(stmt.db_handle) else {
        return js_array_alloc(0);
    };
    let Ok(conn) = db.conn.lock() else {
        return js_array_alloc(0);
    };
    // The handle stores only the SQL, so the shape comes from a fresh
    // prepare — the same way `reader` and `readonly` are answered.
    let Ok(prepared) = conn.prepare(&stmt.sql) else {
        return js_array_alloc(0);
    };

    // `columns()` carries the declared type, `columns_with_metadata()` the
    // origin names; they are indexed alike, so zip them by position.
    let decl_types: Vec<Option<String>> = prepared
        .columns()
        .map(|c| c.decl_type().map(str::to_string))
        .collect();
    let origins: Vec<(String, Option<String>, Option<String>, Option<String>)> = prepared
        .columns_with_metadata()
        .map(|c| {
            (
                c.name().to_string(),
                c.database_name().map(str::to_string),
                c.table_name().map(str::to_string),
                c.origin_name().map(str::to_string),
            )
        })
        .collect();

    let shape = build_object_shape(&COLUMN_KEYS);
    let records_scope = TransientRootScope::enter();
    let records: Vec<TransientRootedNanbox> = origins
        .iter()
        .enumerate()
        .map(|(idx, (name, database, table, column))| {
            let decl_type = decl_types.get(idx).cloned().flatten();
            let scope = TransientRootScope::enter();
            // Slot order matches COLUMN_KEYS. Each string is rooted before
            // the next allocation, and the record is born after all five.
            let values: Vec<TransientRootedNanbox> = [
                Some(name.as_str()),
                column.as_deref(),
                table.as_deref(),
                database.as_deref(),
                decl_type.as_deref(),
            ]
            .into_iter()
            .map(|value| {
                scope.root_nanbox(bits(match value {
                    // better-sqlite3 reports an absent origin as null, not
                    // as undefined or an empty string.
                    None => JsValue::NULL,
                    Some(text) => JsValue::from_string_ptr(alloc_string(text).as_raw()),
                }))
            })
            .collect();
            let record = record_from_rooted(&scope, &shape, &values);
            drop(scope);
            // Nothing allocates between the inner scope's exit and this root.
            records_scope.root_nanbox(bits(JsValue::from_object_ptr(record)))
        })
        .collect();
    array_from_rooted(&records)
}

fn bits(value: JsValue) -> f64 {
    f64::from_bits(value.bits())
}

/// A shaped record over `values`, which the caller rooted before this birth.
/// The record is rooted in `scope` while its fields are written and re-read
/// for each store.
unsafe fn record_from_rooted(
    scope: &TransientRootScope,
    (packed_keys, shape_id): &(Vec<u8>, u32),
    values: &[TransientRootedNanbox],
) -> *mut ObjectHeader {
    let record = scope.root_addr(js_object_alloc_with_shape(
        *shape_id,
        values.len() as u32,
        packed_keys.as_ptr(),
        packed_keys.len() as u32,
    ) as i64);
    for (field, value) in values.iter().enumerate() {
        js_object_set_field(
            record.get() as *mut ObjectHeader,
            field as u32,
            JsValue::from_bits(value.get().to_bits()),
        );
    }
    record.get() as *mut ObjectHeader
}

/// One result row as JS values, each rooted before the next conversion (a
/// string, a BigInt or a Buffer column allocates).
fn rooted_row_values(
    scope: &TransientRootScope,
    row: &rusqlite::Row<'_>,
    columns: usize,
    safe_ints: bool,
) -> Vec<TransientRootedNanbox> {
    (0..columns)
        .map(|idx| {
            let value: SqliteValue = row.get(idx).unwrap_or(SqliteValue::Null);
            scope.root_nanbox(bits(sqlite_value_to_jsvalue_with(&value, safe_ints)))
        })
        .collect()
}

/// An array of rooted values in order. Each push returns the array to use
/// next and nothing else is held, so no earlier pointer outlives a push.
unsafe fn array_from_rooted(values: &[TransientRootedNanbox]) -> *mut ArrayHeader {
    values
        .iter()
        .fold(js_array_alloc(values.len() as u32), |arr, v| {
            js_array_push(arr, JsValue::from_bits(v.get().to_bits()))
        })
}

/// One row as better-sqlite3 returns it: an array in raw mode, else a record
/// keyed by column name.
unsafe fn row_value(
    row: &rusqlite::Row<'_>,
    columns: usize,
    raw_or_shape: Option<&(Vec<u8>, u32)>,
    safe_ints: bool,
) -> JsValue {
    let scope = TransientRootScope::enter();
    let values = rooted_row_values(&scope, row, columns, safe_ints);
    match raw_or_shape {
        None => JsValue::from_object_ptr(array_from_rooted(&values)),
        Some(shape) => JsValue::from_object_ptr(record_from_rooted(&scope, shape, &values)),
    }
}

/// The keys of a `Statement#columns()` record, in better-sqlite3's own
/// insertion order — `Object.keys()` on the result observes it, and node
/// reports `["name","column","table","database","type"]`.
///
/// Deliberately NOT the runtime's `SQLITE_COLUMN_METADATA` layout, which
/// perry-stdlib's same-named function uses: that one serves `node:sqlite`,
/// whose documented key order is alphabetical. The two APIs agree on the
/// five names and differ on their order, so each keeps its own.
const COLUMN_KEYS: [&str; 5] = ["name", "column", "table", "database", "type"];

/// Build better-sqlite3's `RunResult` — `{ changes, lastInsertRowid }`.
unsafe fn run_result(changes: f64, last_id: i64) -> *mut ObjectHeader {
    let result = unsafe {
        perry_ffi::runtime_layouts::alloc_runtime_record(
            perry_ffi::runtime_layouts::RECORD_F0_0::desc(),
        )
    };
    js_object_set_field(result, 0, JsValue::from_number(changes));
    js_object_set_field(result, 1, JsValue::from_number(last_id as f64));
    result
}

/// `stmt.run(...params)` — run the statement and report
/// `{ changes, lastInsertRowid }` as an ObjectHeader pointer, or null
/// on error.
///
/// better-sqlite3 accepts **any** statement here, including one that
/// returns rows: `run()` steps it to completion, discards the rows and
/// reports the connection's change count, so `select 1` gives
/// `{ changes: 0, lastInsertRowid: 0 }` on a fresh connection.
/// rusqlite's `Connection::execute` refuses a row-returning statement
/// with `ExecuteReturnedResults` instead, so dispatching on the
/// statement's column count (0 = no rows possible) keeps both kinds
/// working and runs the statement exactly once. Before this, every
/// `run()` on a SELECT returned null, surfacing in JS as `undefined`
/// with no error — ActiveRecord's sqlite3 adapter calls `run()` on
/// `SELECT sqlite_version(*)` through its `step()` and read
/// `.changes` off that `undefined`.
///
/// # Safety
///
/// `params_arr` must be null or a Perry-runtime `ArrayHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_stmt_run(
    stmt_handle: Handle,
    params_arr: *const ArrayHeader,
) -> *mut ObjectHeader {
    let sqlite_params = params_from_array(params_arr);
    require_statement_database_open(stmt_handle);

    if let Some(stmt) = get_handle::<SqliteStmtHandle>(stmt_handle) {
        if let Some(db) = get_handle::<SqliteDbHandle>(stmt.db_handle) {
            if let Ok(conn) = db.conn.lock() {
                let param_refs: Vec<&dyn rusqlite::ToSql> =
                    sqlite_params.iter().map(|p| p.as_ref()).collect();
                // better-sqlite3 reports the rows THIS statement changed, not
                // the connection's running counter: a CREATE TABLE or a SELECT
                // run straight after an INSERT still reports 0, where
                // `sqlite3_changes()` would still hold the INSERT's 1. Diffing
                // `total_changes` across the statement gives that directly.
                let before = conn.total_changes();
                if let Ok(mut prepared) = conn.prepare(&stmt.sql) {
                    if prepared.column_count() == 0 {
                        if prepared.execute(param_refs.as_slice()).is_ok() {
                            let changed = conn.total_changes().saturating_sub(before);
                            return run_result(changed as f64, conn.last_insert_rowid());
                        }
                    } else if let Ok(mut rows) = prepared.query(param_refs.as_slice()) {
                        // Step to completion so the statement's side effects
                        // (e.g. `INSERT ... RETURNING`) all happen.
                        while matches!(rows.next(), Ok(Some(_))) {}
                        drop(rows);
                        let changed = conn.total_changes().saturating_sub(before);
                        return run_result(changed as f64, conn.last_insert_rowid());
                    }
                }
            }
        }
    }
    std::ptr::null_mut()
}

/// `stmt.get(...params)` — return a single row, or `undefined`.
///
/// Returns `f64` carrying the NaN-boxed bits — perry-stdlib's
/// existing copy explains the SysV AMD64 ABI mismatch
/// (`#[repr(transparent)] u64` returns in RAX but the LLVM call
/// site expects `double` in XMM0). Same trick here.
///
/// # Safety
///
/// `params_arr` must be null or a Perry-runtime `ArrayHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_stmt_get(
    stmt_handle: Handle,
    params_arr: *const ArrayHeader,
) -> f64 {
    let sqlite_params = params_from_array(params_arr);
    require_statement_database_open(stmt_handle);

    if let Some(stmt) = get_handle::<SqliteStmtHandle>(stmt_handle) {
        let raw = stmt.raw_mode.load(Ordering::Relaxed);
        let safe_ints = stmt.safe_integers.load(Ordering::Relaxed);
        if let Some(db) = get_handle::<SqliteDbHandle>(stmt.db_handle) {
            if let Ok(conn) = db.conn.lock() {
                let param_refs: Vec<&dyn rusqlite::ToSql> =
                    sqlite_params.iter().map(|p| p.as_ref()).collect();
                if let Ok(mut prepared) = conn.prepare(&stmt.sql) {
                    let column_names: Vec<String> =
                        prepared.column_names().map(|s| s.to_string()).collect();
                    let column_refs: Vec<&str> = column_names.iter().map(String::as_str).collect();

                    let shape = (!raw).then(|| build_object_shape(&column_refs));
                    let mut rows = prepared.query(param_refs.as_slice());
                    if let Ok(ref mut rows) = rows {
                        if let Ok(Some(row)) = rows.next() {
                            return bits(row_value(
                                row,
                                column_names.len(),
                                shape.as_ref(),
                                safe_ints,
                            ));
                        }
                    }
                }
            }
        }
    }
    f64::from_bits(JsValue::UNDEFINED.bits())
}

/// `stmt.all(...params)` — return every row as an array of
/// objects.
///
/// # Safety
///
/// `params_arr` must be null or a Perry-runtime `ArrayHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_stmt_all(
    stmt_handle: Handle,
    params_arr: *const ArrayHeader,
) -> *mut ArrayHeader {
    let sqlite_params = params_from_array(params_arr);
    require_statement_database_open(stmt_handle);
    // Every row is rooted as it is built; the result array is born last.
    let rows_scope = TransientRootScope::enter();
    let mut built: Vec<TransientRootedNanbox> = Vec::new();

    if let Some(stmt) = get_handle::<SqliteStmtHandle>(stmt_handle) {
        let raw = stmt.raw_mode.load(Ordering::Relaxed);
        let safe_ints = stmt.safe_integers.load(Ordering::Relaxed);
        if let Some(db) = get_handle::<SqliteDbHandle>(stmt.db_handle) {
            if let Ok(conn) = db.conn.lock() {
                let param_refs: Vec<&dyn rusqlite::ToSql> =
                    sqlite_params.iter().map(|p| p.as_ref()).collect();
                if let Ok(mut prepared) = conn.prepare(&stmt.sql) {
                    let column_names: Vec<String> =
                        prepared.column_names().map(|s| s.to_string()).collect();
                    let column_refs: Vec<&str> = column_names.iter().map(String::as_str).collect();
                    // Only build the object shape in non-raw mode — raw rows
                    // skip the per-row object allocation entirely.
                    let object_shape = if raw {
                        None
                    } else {
                        Some(build_object_shape(&column_refs))
                    };

                    let mut rows = prepared.query(param_refs.as_slice());
                    if let Ok(ref mut rows) = rows {
                        while let Ok(Some(row)) = rows.next() {
                            // Raw mode: each row is an array of column values
                            // in declared column order. drizzle's
                            // `mapResultRow(fields, row, …)` indexes by
                            // numeric position into this array.
                            let value = row_value(
                                row,
                                column_names.len(),
                                object_shape.as_ref(),
                                safe_ints,
                            );
                            built.push(rows_scope.root_nanbox(bits(value)));
                        }
                    }
                }
            }
        }
    }
    array_from_rooted(&built)
}

/// `db.pragma(name, value?)` — execute a `PRAGMA` and return the
/// first row's first column as a string.
///
/// # Safety
///
/// Both pointers must be null or Perry-runtime `StringHeader`s.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_pragma(
    db_handle: Handle,
    pragma_ptr: *const StringHeader,
    value_ptr: *const StringHeader,
) -> *mut StringHeader {
    let Some(pragma) = read_str(pragma_ptr) else {
        return std::ptr::null_mut();
    };
    require_database_open(db_handle);
    let value = read_str(value_ptr);

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
                        // PRAGMA results can be string OR integer
                        // (e.g. `user_version` is int 0). Try string
                        // first, fall back to int → string. Matches
                        // node-better-sqlite3's pragma_results
                        // behavior; perry-stdlib's existing copy
                        // didn't handle the int case (returned "")
                        // — fixed in this port.
                        if let Ok(s) = row.get::<_, String>(0) {
                            return alloc_string(&s).as_raw();
                        }
                        if let Ok(n) = row.get::<_, i64>(0) {
                            return alloc_string(&n.to_string()).as_raw();
                        }
                        if let Ok(n) = row.get::<_, f64>(0) {
                            return alloc_string(&n.to_string()).as_raw();
                        }
                    }
                }
            }
        }
    }
    std::ptr::null_mut()
}

#[no_mangle]
pub extern "C" fn js_sqlite_begin_transaction(db_handle: Handle) -> i32 {
    require_database_open(db_handle);
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

#[no_mangle]
pub extern "C" fn js_sqlite_commit(db_handle: Handle) -> i32 {
    require_database_open(db_handle);
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

#[no_mangle]
pub extern "C" fn js_sqlite_rollback(db_handle: Handle) -> i32 {
    require_database_open(db_handle);
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

/// `db.close()` — drop the connection. Drops the handle from
/// the registry; subsequent operations throw the same closed-connection
/// TypeError as better-sqlite3.
#[no_mangle]
pub extern "C" fn js_sqlite_close(db_handle: Handle) -> i32 {
    if drop_handle(db_handle) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn js_sqlite_in_transaction(db_handle: Handle) -> i32 {
    if let Some(db) = get_handle::<SqliteDbHandle>(db_handle) {
        if let Ok(conn) = db.conn.lock() {
            return if !conn.is_autocommit() { 1 } else { 0 };
        }
    }
    0
}

/// Returns `1` if `handle` currently resolves to a `SqliteDbHandle` in
/// this crate's perry-ffi handle registry, `0` otherwise. Used by the
/// V8 bridge in `perry-jsruntime::bridge::native_object_to_v8` to
/// decide whether to materialize a `v8::Object` proxy with `prepare` /
/// `exec` / `pragma` / `close` / `transaction` method callbacks when a
/// sqlite Database crosses the native→V8 boundary (drizzle's
/// `BetterSQLiteSession` does `this.client.prepare(query.sql)` from
/// session.js — refs #1022).
#[no_mangle]
pub extern "C" fn js_sqlite_is_db_handle(handle: Handle) -> i32 {
    if get_handle::<SqliteDbHandle>(handle).is_some() {
        1
    } else {
        0
    }
}

/// Returns `1` if `handle` currently resolves to a `SqliteStmtHandle`
/// in this crate's perry-ffi handle registry, `0` otherwise. Mirror
/// of `js_sqlite_is_db_handle` for the Statement side of the V8 proxy
/// materialization — drizzle's PreparedQuery calls `stmt.run(...)` /
/// `stmt.all(...)` / `stmt.get(...)` / `stmt.raw().all(...)` on the
/// handle returned from `client.prepare(...)`. Refs #1022.
#[no_mangle]
pub extern "C" fn js_sqlite_is_stmt_handle(handle: Handle) -> i32 {
    if get_handle::<SqliteStmtHandle>(handle).is_some() {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_ffi::{js_array_get, js_array_length, js_object_get_field};

    fn make_in_memory_db() -> Handle {
        let path = alloc_string(":memory:");
        unsafe { js_sqlite_open(path.as_raw() as *const _) }
    }

    #[test]
    fn open_in_memory_returns_valid_handle() {
        let h = make_in_memory_db();
        assert!(h > 0, "open returned {}", h);
    }

    #[test]
    fn exec_create_then_insert() {
        let h = make_in_memory_db();
        let create = alloc_string("CREATE TABLE t (id INTEGER, name TEXT)");
        let r = unsafe { js_sqlite_exec(h, create.as_raw() as *const _) };
        assert_eq!(r, 1);
        let insert = alloc_string("INSERT INTO t VALUES (1, 'alice')");
        let r = unsafe { js_sqlite_exec(h, insert.as_raw() as *const _) };
        assert_eq!(r, 1);
    }

    #[test]
    fn pragma_returns_value() {
        let h = make_in_memory_db();
        let pragma = alloc_string("user_version");
        let result_ptr =
            unsafe { js_sqlite_pragma(h, pragma.as_raw() as *const _, std::ptr::null()) };
        assert!(!result_ptr.is_null());
        let s = read_string(unsafe { JsString::from_raw(result_ptr) }).expect("non-null");
        assert_eq!(s, "0");
    }

    #[test]
    fn transactions() {
        let h = make_in_memory_db();
        let create = alloc_string("CREATE TABLE t (id INTEGER)");
        unsafe { js_sqlite_exec(h, create.as_raw() as *const _) };
        assert_eq!(js_sqlite_begin_transaction(h), 1);
        assert_eq!(js_sqlite_in_transaction(h), 1);
        assert_eq!(js_sqlite_commit(h), 1);
        assert_eq!(js_sqlite_in_transaction(h), 0);
    }

    /// `all()` (record and raw mode), `get()` and `columns()` build their
    /// values, records and result arrays under transient roots; the results
    /// must read back exactly as before.
    #[test]
    fn rows_and_columns_read_back_through_their_roots() {
        perry_ffi::initialize_runtime_fixture();
        let h = make_in_memory_db();
        for sql in [
            "CREATE TABLE people (ident INTEGER, label_text TEXT)",
            "INSERT INTO people VALUES (1, 'alice-row'), (2, 'bobby-row'), (3, 'carol-row')",
        ] {
            let sql = alloc_string(sql);
            assert_eq!(unsafe { js_sqlite_exec(h, sql.as_raw() as *const _) }, 1);
        }
        let sql = alloc_string("SELECT ident, label_text FROM people ORDER BY ident");
        let stmt = unsafe { js_sqlite_prepare(h, sql.as_raw() as *const _) };
        let text = |v: JsValue| {
            read_string(unsafe { JsString::from_raw(v.as_string_ptr()) }).map(str::to_owned)
        };
        unsafe {
            let all = js_sqlite_stmt_all(stmt, std::ptr::null());
            assert_eq!(js_array_length(all), 3);
            let row = js_array_get(all, 1).as_pointer::<ObjectHeader>();
            assert_eq!(
                text(js_object_get_field(row, 1)).as_deref(),
                Some("bobby-row")
            );

            let one = JsValue::from_bits(js_sqlite_stmt_get(stmt, std::ptr::null()).to_bits())
                .as_pointer::<ObjectHeader>();
            assert_eq!(js_object_get_field(one, 0).to_number(), 1.0);
            assert_eq!(
                text(js_object_get_field(one, 1)).as_deref(),
                Some("alice-row")
            );

            let columns = js_sqlite_stmt_columns(stmt);
            assert_eq!(js_array_length(columns), 2);
            let label = js_array_get(columns, 1).as_pointer::<ObjectHeader>();
            // COLUMN_KEYS order: name, column, table, database, type.
            assert_eq!(
                text(js_object_get_field(label, 0)).as_deref(),
                Some("label_text")
            );
            assert_eq!(
                text(js_object_get_field(label, 2)).as_deref(),
                Some("people")
            );

            js_sqlite_stmt_raw(stmt);
            let raw = js_sqlite_stmt_all(stmt, std::ptr::null());
            assert_eq!(js_array_length(raw), 3);
            let last = js_array_get(raw, 2).as_pointer::<ArrayHeader>();
            assert_eq!(js_array_length(last), 2);
            assert_eq!(text(js_array_get(last, 1)).as_deref(), Some("carol-row"));
        }
    }

    #[test]
    fn close_drops_handle() {
        let h = make_in_memory_db();
        assert_eq!(js_sqlite_close(h), 1);
        // Second close is a no-op (handle already gone).
        assert_eq!(js_sqlite_close(h), 0);
    }
}

#[cfg(test)]
mod static_record_layout_tests {
    #[test]
    fn static_layout_binding_records_keep_all_declared_properties() {
        perry_ffi::initialize_runtime_fixture();
        let roots = perry_ffi::TransientRootScope::enter();
        let record = roots.root_addr(unsafe {
            perry_ffi::runtime_layouts::alloc_runtime_record(
                perry_ffi::runtime_layouts::RECORD_F0_0::desc(),
            )
        } as i64);
        for slot in 0..2 {
            assert_eq!(
                unsafe {
                    perry_ffi::js_object_get_field(
                        record.get() as *const perry_ffi::ObjectHeader,
                        slot,
                    )
                }
                .bits(),
                perry_ffi::JsValue::UNDEFINED.bits()
            );
        }
    }
}
