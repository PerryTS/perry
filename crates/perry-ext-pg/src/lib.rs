//! Native bindings for the npm `pg` PostgreSQL client — uses only
//! perry-ffi. Async via `sqlx::postgres` bridged through
//! `spawn_blocking + JsPromise + tokio::Handle::current().block_on`.
//!
//! Mirrors perry-stdlib's existing surface: `Client` (pre-connect
//! / connected handle states with `.connect()` deferring the TCP
//! handshake), `Pool` (lazy `connect_lazy`-style + eager
//! `pg.createPool`), parameterized `query()` with `Null`/`String`/
//! `Number`/`Int`/`Bool` param types, result objects with
//! `rows`/`fields`/`rowCount`/`command` keys, row objects keyed by
//! column name. BigInt param support deferred — perry-ffi's BigInt
//! surface is in place (v0.5.556) but the JS-side array iteration
//! shape needs an extra adapter; followup once any wrapper actually
//! demands it.

use perry_ffi::{
    alloc_string, build_object_shape, get_handle_mut, js_array_alloc, js_array_get, js_array_push,
    js_object_alloc_with_shape, js_object_get_field, js_object_set_field, register_handle,
    spawn_blocking, take_handle, ArrayHeader, Handle, JsPromise, JsValue, ObjectHeader, Promise,
    StringHeader,
};
use sqlx::postgres::{PgColumn, PgConnection, PgPool, PgPoolOptions, PgRow};
use sqlx::{Column, Connection, Row, TypeInfo};

/// Connection config — same field shape as perry-stdlib's PgConfig.
#[derive(Debug, Clone)]
pub struct PgConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub database: Option<String>,
}

impl Default for PgConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 5432,
            user: "postgres".to_string(),
            password: String::new(),
            database: None,
        }
    }
}

impl PgConfig {
    pub fn to_url(&self) -> String {
        let db = self
            .database
            .as_ref()
            .map(|d| format!("/{}", d))
            .unwrap_or_default();
        format!(
            "postgres://{}:{}@{}:{}{}",
            self.user, self.password, self.host, self.port, db
        )
    }
}

unsafe fn jsvalue_to_string(value: JsValue) -> Option<String> {
    if value.is_string() {
        let ptr = value.as_string_ptr();
        if !ptr.is_null() {
            let len = (*ptr).byte_len as usize;
            let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
            let bytes = std::slice::from_raw_parts(data, len);
            return std::str::from_utf8(bytes).ok().map(String::from);
        }
    }
    None
}

/// Object layout matches perry-stdlib's positional convention:
///   field 0: host (string)
///   field 1: port (number)
///   field 2: user (string)
///   field 3: password (string)
///   field 4: database (string, optional)
unsafe fn parse_pg_config(config: JsValue) -> PgConfig {
    let mut result = PgConfig::default();
    let obj_ptr = config.as_pointer::<ObjectHeader>();
    if obj_ptr.is_null() {
        return result;
    }

    if let Some(s) = jsvalue_to_string(js_object_get_field(obj_ptr, 0)) {
        result.host = s;
    }
    let port_val = js_object_get_field(obj_ptr, 1);
    if port_val.is_number() {
        result.port = port_val.to_number() as u16;
    }
    if let Some(s) = jsvalue_to_string(js_object_get_field(obj_ptr, 2)) {
        result.user = s;
    }
    if let Some(s) = jsvalue_to_string(js_object_get_field(obj_ptr, 3)) {
        result.password = s;
    }
    let db_val = js_object_get_field(obj_ptr, 4);
    if !db_val.is_undefined() && !db_val.is_null() {
        if let Some(s) = jsvalue_to_string(db_val) {
            result.database = Some(s);
        }
    }
    result
}

/// Convert a single column value to a JsValue, mapping common
/// PostgreSQL OIDs to JS scalars. Unknown types fall back to a
/// string read.
fn column_value_to_jsvalue(row: &PgRow, index: usize) -> JsValue {
    let col = &row.columns()[index];
    let type_name = col.type_info().name();
    match type_name {
        "INT4" | "INT2" => row
            .try_get::<i32, _>(index)
            .map(JsValue::from_int32)
            .unwrap_or(JsValue::NULL),
        "INT8" => row
            .try_get::<i64, _>(index)
            .map(|n| JsValue::from_number(n as f64))
            .unwrap_or(JsValue::NULL),
        "FLOAT4" | "FLOAT8" | "NUMERIC" => row
            .try_get::<f64, _>(index)
            .map(JsValue::from_number)
            .unwrap_or(JsValue::NULL),
        "VARCHAR" | "CHAR" | "TEXT" | "BPCHAR" | "NAME" => row
            .try_get::<String, _>(index)
            .map(|s| JsValue::from_string_ptr(alloc_string(&s).as_raw()))
            .unwrap_or(JsValue::NULL),
        "BOOL" => row
            .try_get::<bool, _>(index)
            .map(JsValue::from_bool)
            .unwrap_or(JsValue::NULL),
        _ => row
            .try_get::<String, _>(index)
            .map(|s| JsValue::from_string_ptr(alloc_string(&s).as_raw()))
            .unwrap_or(JsValue::NULL),
    }
}

/// Build a row object keyed by column names. Replaces perry-stdlib's
/// `js_object_alloc(0, n)` no-shape pattern with a perry-ffi
/// shape-aware allocation — same observable behavior since user code
/// accesses `row.id` through dynamic property lookup either way.
fn row_to_js_object(row: &PgRow) -> *mut ObjectHeader {
    let cols: Vec<&str> = row.columns().iter().map(|c| c.name()).collect();
    let (packed, shape_id) = build_object_shape(&cols);
    let obj = unsafe {
        js_object_alloc_with_shape(
            shape_id,
            cols.len() as u32,
            packed.as_ptr(),
            packed.len() as u32,
        )
    };
    for i in 0..cols.len() {
        let val = column_value_to_jsvalue(row, i);
        unsafe { js_object_set_field(obj, i as u32, val) };
    }
    obj
}

/// Build a `FieldDef`-shaped object matching node-pg's `result.fields[i]`
/// (#4917): `dataTypeID` is the numeric type OID, `tableID`/`columnID` come
/// from the RowDescription (0 for expression columns, like Node).
/// `dataTypeSize`/`dataTypeModifier` are not exposed by sqlx 0.8 and report
/// the "unknown/variable" sentinel -1. Twin of
/// `perry_stdlib::pg::types::column_to_field_def` — keep in sync.
fn column_to_field_def(col: &PgColumn) -> *mut ObjectHeader {
    let (packed, shape_id) = build_object_shape(&[
        "name",
        "tableID",
        "columnID",
        "dataTypeID",
        "dataTypeSize",
        "dataTypeModifier",
        "format",
    ]);
    let obj =
        unsafe { js_object_alloc_with_shape(shape_id, 7, packed.as_ptr(), packed.len() as u32) };
    let name_str = alloc_string(col.name());
    let table_id = col.relation_id().map(|oid| oid.0 as f64).unwrap_or(0.0);
    let column_id = col
        .relation_attribute_no()
        .map(|attno| attno as f64)
        .unwrap_or(0.0);
    let data_type_id = col.type_info().oid().map(|oid| oid.0 as f64).unwrap_or(0.0);
    let format_str = alloc_string("text");
    unsafe {
        js_object_set_field(obj, 0, JsValue::from_string_ptr(name_str.as_raw()));
        js_object_set_field(obj, 1, JsValue::from_number(table_id));
        js_object_set_field(obj, 2, JsValue::from_number(column_id));
        js_object_set_field(obj, 3, JsValue::from_number(data_type_id));
        js_object_set_field(obj, 4, JsValue::from_number(-1.0));
        js_object_set_field(obj, 5, JsValue::from_number(-1.0));
        js_object_set_field(obj, 6, JsValue::from_string_ptr(format_str.as_raw()));
    }
    obj
}

/// Wrap a query outcome in pg's `{ rows, fields, rowCount, command }`
/// result object.
fn rows_to_pg_result(rows: Vec<PgRow>, columns: &[PgColumn], command: &str) -> JsValue {
    let (packed, shape_id) = build_object_shape(&["rows", "fields", "rowCount", "command"]);
    let result_obj =
        unsafe { js_object_alloc_with_shape(shape_id, 4, packed.as_ptr(), packed.len() as u32) };

    // rows array
    let mut rows_arr = unsafe { js_array_alloc(rows.len() as u32) };
    for row in &rows {
        let row_obj = row_to_js_object(row);
        rows_arr = unsafe { js_array_push(rows_arr, JsValue::from_object_ptr(row_obj)) };
    }
    unsafe { js_object_set_field(result_obj, 0, JsValue::from_object_ptr(rows_arr)) };

    // fields array
    let mut fields_arr = unsafe { js_array_alloc(columns.len() as u32) };
    for col in columns {
        let field_obj = column_to_field_def(col);
        fields_arr = unsafe { js_array_push(fields_arr, JsValue::from_object_ptr(field_obj)) };
    }
    unsafe { js_object_set_field(result_obj, 1, JsValue::from_object_ptr(fields_arr)) };

    unsafe {
        js_object_set_field(result_obj, 2, JsValue::from_number(rows.len() as f64));
        let cmd_str = alloc_string(command);
        js_object_set_field(result_obj, 3, JsValue::from_string_ptr(cmd_str.as_raw()));
    }
    JsValue::from_object_ptr(result_obj)
}

fn empty_pg_result(command: &str, row_count: u64) -> JsValue {
    let value = rows_to_pg_result(Vec::new(), &[], command);
    let obj: *mut ObjectHeader = value.as_pointer();
    if !obj.is_null() {
        unsafe {
            js_object_set_field(obj, 2, JsValue::from_number(row_count as f64));
        }
    }
    value
}

#[derive(Clone, Debug)]
enum ParamValue {
    Null,
    String(String),
    Number(f64),
    Int(i64),
    Bool(bool),
}

unsafe fn extract_params_from_jsvalue(params: JsValue) -> Vec<ParamValue> {
    let arr_ptr = params.as_pointer::<ArrayHeader>();
    if arr_ptr.is_null() {
        return Vec::new();
    }
    // Pull the array length out of the header — the layout matches
    // perry-runtime's `ArrayHeader { length: u32, capacity: u32 }`.
    let length = (*arr_ptr).length;

    let mut result = Vec::with_capacity(length as usize);
    for i in 0..length {
        let element = js_array_get(arr_ptr, i);
        let param = if element.is_null() || element.is_undefined() {
            ParamValue::Null
        } else if element.is_string() {
            jsvalue_to_string(element)
                .map(ParamValue::String)
                .unwrap_or(ParamValue::Null)
        } else if element.is_int32() {
            ParamValue::Int(element.to_int32() as i64)
        } else if element.is_bool() {
            ParamValue::Bool(element.to_bool())
        } else if element.is_number() {
            let n = element.to_number();
            if n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
                ParamValue::Int(n as i64)
            } else {
                ParamValue::Number(n)
            }
        } else {
            ParamValue::Null
        };
        result.push(param);
    }
    result
}

fn is_row_returning_query(sql: &str) -> bool {
    let trimmed = sql.trim_start();
    let upper = trimmed.get(..10).unwrap_or(trimmed).to_uppercase();
    upper.starts_with("SELECT")
        || upper.starts_with("SHOW")
        || upper.starts_with("DESC")
        || upper.starts_with("EXPLAIN")
        || upper.starts_with("WITH")
}

unsafe fn read_sql(sql_ptr: *const u8) -> String {
    if sql_ptr.is_null() {
        return String::new();
    }
    let header = sql_ptr as *const StringHeader;
    let len = (*header).byte_len as usize;
    let data = sql_ptr.add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data, len);
    std::str::from_utf8(bytes).unwrap_or("").to_string()
}

// ── Connection (Client) ───────────────────────────────────────────

/// Wraps a `PgConnection` so it can sit in the handle registry.
/// Pre-connect: `pending_config = Some, connection = None`.
/// Connected:   `pending_config = None, connection = Some`.
pub struct PgConnectionHandle {
    pub connection: Option<PgConnection>,
    pub pending_config: Option<PgConfig>,
}

impl PgConnectionHandle {
    pub fn new(conn: PgConnection) -> Self {
        Self {
            connection: Some(conn),
            pending_config: None,
        }
    }
    pub fn pending(config: PgConfig) -> Self {
        Self {
            connection: None,
            pending_config: Some(config),
        }
    }
}

/// `new Client(config)` — sync constructor, no TCP touch.
///
/// # Safety
/// `config_f` is a NaN-boxed JsValue (passed as f64 at the FFI
/// boundary).
#[no_mangle]
pub unsafe extern "C" fn js_pg_client_new(config_f: f64) -> Handle {
    let config = JsValue::from_bits(config_f.to_bits());
    let pg_config = parse_pg_config(config);
    register_handle(PgConnectionHandle::pending(pg_config))
}

/// `client.connect()` — opens the TCP connection using the config
/// stored at `js_pg_client_new` time. No-op success if already
/// connected.
#[no_mangle]
pub extern "C" fn js_pg_client_connect(client_handle: Handle) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();

    // Snapshot the pending config before entering spawn_blocking —
    // can't hold a `&mut` across the boundary.
    let pending =
        get_handle_mut::<PgConnectionHandle>(client_handle).and_then(|h| h.pending_config.take());

    let Some(pg_config) = pending else {
        promise.resolve_undefined();
        return raw;
    };

    spawn_blocking(move || {
        let result = tokio::runtime::Handle::current()
            .block_on(async move { PgConnection::connect(&pg_config.to_url()).await });
        match result {
            Ok(conn) => {
                if let Some(h) = get_handle_mut::<PgConnectionHandle>(client_handle) {
                    h.connection = Some(conn);
                }
                promise.resolve_undefined();
            }
            Err(e) => promise.reject_string(&format!("Failed to connect: {}", e)),
        }
    });
    raw
}

/// Combined `pg.connect(config)` — sync `new` + async connect; older
/// API kept for back-compat with perry-stdlib callers.
///
/// # Safety
/// `config_f` is a NaN-boxed JsValue.
#[no_mangle]
pub unsafe extern "C" fn js_pg_connect(config_f: f64) -> *mut Promise {
    let config = JsValue::from_bits(config_f.to_bits());
    let pg_config = parse_pg_config(config);
    let promise = JsPromise::new();
    let raw = promise.as_raw();

    spawn_blocking(move || {
        let result = tokio::runtime::Handle::current()
            .block_on(async move { PgConnection::connect(&pg_config.to_url()).await });
        match result {
            Ok(conn) => {
                let handle = register_handle(PgConnectionHandle::new(conn));
                promise.resolve(JsValue::from_number(handle as f64));
            }
            Err(e) => promise.reject_string(&format!("Failed to connect: {}", e)),
        }
    });
    raw
}

/// `client.end()` — close the connection.
#[no_mangle]
pub extern "C" fn js_pg_client_end(client_handle: Handle) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    spawn_blocking(move || {
        if let Some(mut wrapper) = take_handle::<PgConnectionHandle>(client_handle) {
            if let Some(conn) = wrapper.connection.take() {
                let result = tokio::runtime::Handle::current().block_on(conn.close());
                match result {
                    Ok(()) => promise.resolve_undefined(),
                    Err(e) => promise.reject_string(&format!("Failed to close connection: {}", e)),
                }
            } else {
                promise.reject_string("Connection already closed");
            }
        } else {
            promise.reject_string("Invalid client handle");
        }
    });
    raw
}

/// `client.query(sql)` — no params.
///
/// # Safety
/// `sql_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_pg_client_query(
    client_handle: Handle,
    sql_ptr: *const u8,
) -> *mut Promise {
    let sql = read_sql(sql_ptr);
    let command = sql
        .split_whitespace()
        .next()
        .unwrap_or("SELECT")
        .to_uppercase();

    let promise = JsPromise::new();
    let raw = promise.as_raw();

    spawn_blocking(move || {
        let outcome = tokio::runtime::Handle::current().block_on(async move {
            let wrapper = get_handle_mut::<PgConnectionHandle>(client_handle)
                .ok_or_else(|| "Invalid client handle".to_string())?;
            let conn = wrapper
                .connection
                .as_mut()
                .ok_or_else(|| "Connection already closed".to_string())?;
            sqlx::query(sqlx::AssertSqlSafe(sql.clone()))
                .fetch_all(conn)
                .await
                .map_err(|e| format!("Query failed: {}", e))
        });
        match outcome {
            Ok(rows) => {
                let columns: Vec<_> = if !rows.is_empty() {
                    rows[0].columns().to_vec()
                } else {
                    Vec::new()
                };
                let result = rows_to_pg_result(rows, &columns, &command);
                promise.resolve(result);
            }
            Err(e) => promise.reject_string(&e),
        }
    });
    raw
}

/// `client.query(sql, params)` — parameterized.
///
/// # Safety
/// `sql_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_pg_client_query_params(
    client_handle: Handle,
    sql_ptr: *const u8,
    params_f: f64,
) -> *mut Promise {
    let sql = read_sql(sql_ptr);
    let params = JsValue::from_bits(params_f.to_bits());
    let param_values = extract_params_from_jsvalue(params);
    let command = sql
        .split_whitespace()
        .next()
        .unwrap_or("SELECT")
        .to_uppercase();
    let is_select = is_row_returning_query(&sql);

    let promise = JsPromise::new();
    let raw = promise.as_raw();

    spawn_blocking(move || {
        let outcome = tokio::runtime::Handle::current().block_on(async move {
            let wrapper = get_handle_mut::<PgConnectionHandle>(client_handle)
                .ok_or_else(|| "Invalid client handle".to_string())?;
            let conn = wrapper
                .connection
                .as_mut()
                .ok_or_else(|| "Connection already closed".to_string())?;
            let mut query = sqlx::query(sqlx::AssertSqlSafe(sql.clone()));
            for p in &param_values {
                query = match p {
                    ParamValue::Null => query.bind(Option::<String>::None),
                    ParamValue::String(s) => query.bind(s.clone()),
                    ParamValue::Number(n) => query.bind(*n),
                    ParamValue::Int(i) => query.bind(*i),
                    ParamValue::Bool(b) => query.bind(*b),
                };
            }
            if is_select {
                let rows = query
                    .fetch_all(conn)
                    .await
                    .map_err(|e| format!("Query failed: {}", e))?;
                Ok::<_, String>(QueryOutcome::Rows(rows))
            } else {
                let exec_result = query
                    .execute(conn)
                    .await
                    .map_err(|e| format!("Query failed: {}", e))?;
                Ok(QueryOutcome::RowsAffected(exec_result.rows_affected()))
            }
        });
        match outcome {
            Ok(QueryOutcome::Rows(rows)) => {
                let columns: Vec<_> = if !rows.is_empty() {
                    rows[0].columns().to_vec()
                } else {
                    Vec::new()
                };
                promise.resolve(rows_to_pg_result(rows, &columns, &command));
            }
            Ok(QueryOutcome::RowsAffected(n)) => {
                promise.resolve(empty_pg_result(&command, n));
            }
            Err(e) => promise.reject_string(&e),
        }
    });
    raw
}

enum QueryOutcome {
    Rows(Vec<PgRow>),
    RowsAffected(u64),
}

// ── Pool ──────────────────────────────────────────────────────────

pub struct PgPoolHandle {
    pub pool: Option<PgPool>,
    pub pending_url: Option<String>,
}

impl PgPoolHandle {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool: Some(pool),
            pending_url: None,
        }
    }
    pub fn pending(url: String) -> Self {
        Self {
            pool: None,
            pending_url: Some(url),
        }
    }

    pub async fn ensure_pool(&mut self) -> Result<&PgPool, String> {
        if self.pool.is_none() {
            let url = self
                .pending_url
                .take()
                .ok_or_else(|| "Pool config missing".to_string())?;
            let pool = PgPoolOptions::new()
                .max_connections(10)
                .connect(&url)
                .await
                .map_err(|e| format!("Failed to create pool: {}", e))?;
            self.pool = Some(pool);
        }
        Ok(self.pool.as_ref().unwrap())
    }
}

/// `new Pool(config)` — sync constructor; sqlx's pool is built lazily
/// on first query (sqlx 0.8's `connect_lazy` panics outside a Tokio
/// runtime, so we can't even pre-arm it here).
///
/// # Safety
/// `config_f` is a NaN-boxed JsValue.
#[no_mangle]
pub unsafe extern "C" fn js_pg_pool_new(config_f: f64) -> Handle {
    let config = JsValue::from_bits(config_f.to_bits());
    let pg_config = parse_pg_config(config);
    register_handle(PgPoolHandle::pending(pg_config.to_url()))
}

/// `pg.createPool(config)` — async eager pool factory (back-compat
/// with perry-stdlib's older entry).
///
/// # Safety
/// `config_f` is a NaN-boxed JsValue.
#[no_mangle]
pub unsafe extern "C" fn js_pg_create_pool(config_f: f64) -> *mut Promise {
    let config = JsValue::from_bits(config_f.to_bits());
    let pg_config = parse_pg_config(config);
    let promise = JsPromise::new();
    let raw = promise.as_raw();

    spawn_blocking(move || {
        let url = pg_config.to_url();
        let result = tokio::runtime::Handle::current()
            .block_on(async move { PgPoolOptions::new().max_connections(10).connect(&url).await });
        match result {
            Ok(pool) => {
                let handle = register_handle(PgPoolHandle::new(pool));
                promise.resolve(JsValue::from_number(handle as f64));
            }
            Err(e) => promise.reject_string(&format!("Failed to create pool: {}", e)),
        }
    });
    raw
}

/// `pool.query(sql)` — runs against the lazy-built sqlx pool.
///
/// # Safety
/// `sql_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_pg_pool_query(pool_handle: Handle, sql_ptr: *const u8) -> *mut Promise {
    let sql = read_sql(sql_ptr);
    let command = sql
        .split_whitespace()
        .next()
        .unwrap_or("SELECT")
        .to_uppercase();

    let promise = JsPromise::new();
    let raw = promise.as_raw();
    spawn_blocking(move || {
        let outcome = tokio::runtime::Handle::current().block_on(async move {
            let wrapper = get_handle_mut::<PgPoolHandle>(pool_handle)
                .ok_or_else(|| "Invalid pool handle".to_string())?;
            let pool = wrapper.ensure_pool().await?;
            sqlx::query(sqlx::AssertSqlSafe(sql.clone()))
                .fetch_all(pool)
                .await
                .map_err(|e| format!("Query failed: {}", e))
        });
        match outcome {
            Ok(rows) => {
                let columns: Vec<_> = if !rows.is_empty() {
                    rows[0].columns().to_vec()
                } else {
                    Vec::new()
                };
                promise.resolve(rows_to_pg_result(rows, &columns, &command));
            }
            Err(e) => promise.reject_string(&e),
        }
    });
    raw
}

/// `pool.end()` — close all connections in the pool.
#[no_mangle]
pub extern "C" fn js_pg_pool_end(pool_handle: Handle) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    spawn_blocking(move || {
        if let Some(mut wrapper) = take_handle::<PgPoolHandle>(pool_handle) {
            tokio::runtime::Handle::current().block_on(async move {
                if let Some(pool) = wrapper.pool.take() {
                    pool.close().await;
                }
            });
            promise.resolve_undefined();
        } else {
            promise.reject_string("Invalid pool handle");
        }
    });
    raw
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pg_config_defaults() {
        let cfg = PgConfig::default();
        assert_eq!(cfg.host, "localhost");
        assert_eq!(cfg.port, 5432);
        assert_eq!(cfg.user, "postgres");
        assert!(cfg.database.is_none());
    }

    #[test]
    fn to_url_omits_db_when_absent() {
        let cfg = PgConfig::default();
        let url = cfg.to_url();
        assert_eq!(url, "postgres://postgres:@localhost:5432");
    }

    #[test]
    fn to_url_with_db() {
        let mut cfg = PgConfig::default();
        cfg.database = Some("mydb".to_string());
        cfg.user = "u".to_string();
        cfg.password = "p".to_string();
        cfg.host = "db.example.com".to_string();
        cfg.port = 5433;
        assert_eq!(cfg.to_url(), "postgres://u:p@db.example.com:5433/mydb");
    }

    #[test]
    fn is_row_returning_query_classifier() {
        assert!(is_row_returning_query("SELECT * FROM x"));
        assert!(is_row_returning_query("  select 1"));
        assert!(is_row_returning_query("WITH cte AS ..."));
        assert!(!is_row_returning_query("INSERT INTO x VALUES (1)"));
        assert!(!is_row_returning_query("UPDATE x SET y = 1"));
    }

    #[test]
    fn client_new_returns_handle() {
        let cfg_obj = unsafe {
            let (packed, shape_id) =
                build_object_shape(&["host", "port", "user", "password", "database"]);
            let obj = js_object_alloc_with_shape(shape_id, 5, packed.as_ptr(), packed.len() as u32);
            let host_str = alloc_string("localhost");
            js_object_set_field(obj, 0, JsValue::from_string_ptr(host_str.as_raw()));
            js_object_set_field(obj, 1, JsValue::from_number(5432.0));
            JsValue::from_object_ptr(obj)
        };
        let h = unsafe { js_pg_client_new(f64::from_bits(cfg_obj.bits())) };
        assert!(h > 0);
    }
}
