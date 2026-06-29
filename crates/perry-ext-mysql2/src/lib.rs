//! Native bindings for the npm `mysql2` MySQL client — uses only
//! perry-ffi. Async via `sqlx::mysql` bridged through
//! `spawn_blocking + JsPromise + tokio::Handle::current().block_on`.
//!
//! Mirrors perry-stdlib's existing surface: `Connection` (eager
//! `createConnection` with TCP timeout + transaction methods),
//! `Pool` (lazy `createPool` with `getConnection` + `release` for
//! per-conn semantics), parameterized `query()` + `execute()`,
//! result tuple `[rows, fields]` per mysql2 npm convention,
//! ResultSetHeader for non-SELECT writes (`{ affectedRows,
//! insertId, warningStatus }`).
//!
//! BigInt param support is deferred (perry-ffi v0.5.556's BigInt
//! surface is in place but the JS array iteration shape needs an
//! adapter; followup once a wrapper actually demands it).

use perry_ffi::{
    alloc_string, build_object_shape, get_handle_mut, js_array_alloc, js_array_get, js_array_push,
    js_object_alloc_with_shape, js_object_get_field, js_object_set_field, register_handle,
    spawn_blocking, take_handle, ArrayHeader, Handle, JsPromise, JsValue, ObjectHeader, Promise,
    StringHeader,
};
use sqlx::mysql::{MySqlConnection, MySqlPool, MySqlPoolOptions, MySqlRow};
use sqlx::pool::PoolConnection;
use sqlx::{Column, Connection, MySql, Row, TypeInfo};
use std::time::Duration;

const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 10;
const DEFAULT_QUERY_TIMEOUT_SECS: u64 = 30;
const DEFAULT_ACQUIRE_TIMEOUT_SECS: u64 = 10;

/// Connection config — matches perry-stdlib's `MySqlConfig` shape.
#[derive(Debug, Clone)]
pub struct MySqlConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub database: Option<String>,
}

impl Default for MySqlConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 3306,
            user: "root".to_string(),
            password: String::new(),
            database: None,
        }
    }
}

impl MySqlConfig {
    pub fn to_url(&self) -> String {
        let db_part = self
            .database
            .as_ref()
            .map(|d| format!("/{}", d))
            .unwrap_or_default();
        // URL-encode password to handle special characters
        let encoded_password: String = self
            .password
            .chars()
            .map(|c| match c {
                'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
                c => format!("%{:02X}", c as u32),
            })
            .collect();
        format!(
            "mysql://{}:{}@{}:{}{}?ssl-mode=disabled",
            self.user, encoded_password, self.host, self.port, db_part
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

fn parse_mysql_uri(uri: &str) -> Option<MySqlConfig> {
    let uri = uri.strip_prefix("mysql://")?;
    let (credentials, host_part) = if let Some(idx) = uri.rfind('@') {
        (&uri[..idx], &uri[idx + 1..])
    } else {
        ("", uri)
    };
    let (user, password) = if let Some(idx) = credentials.find(':') {
        (
            credentials[..idx].to_string(),
            credentials[idx + 1..].to_string(),
        )
    } else {
        (credentials.to_string(), String::new())
    };
    let (host_port, database) = if let Some(idx) = host_part.find('/') {
        (&host_part[..idx], Some(host_part[idx + 1..].to_string()))
    } else {
        (host_part, None)
    };
    let (host, port) = if let Some(idx) = host_port.rfind(':') {
        let port: u16 = host_port[idx + 1..].parse().unwrap_or(3306);
        (host_port[..idx].to_string(), port)
    } else {
        (host_port.to_string(), 3306)
    };
    Some(MySqlConfig {
        host,
        port,
        user,
        password,
        database,
    })
}

/// Object layout — mysql2 uses a "first field is uri" or
/// positional `host`/`port`/`user`/`password`/`database` shape.
/// We resolve by positional index since perry-ffi's
/// `js_object_get_field` is index-based; perry-stdlib's existing
/// copy uses `js_object_get_field_by_name` which we don't have, so
/// we replicate the behavior by checking field 0 for the URI shape
/// (string-typed) and falling back to fields 0..4 for the field
/// shape if the first field looks numeric (port).
unsafe fn parse_mysql_config(config: JsValue) -> MySqlConfig {
    let mut result = MySqlConfig::default();
    let obj_ptr = config.as_pointer::<ObjectHeader>();
    if obj_ptr.is_null() {
        return result;
    }
    // Conventional perry-stdlib field layout (host=0, port=1, user=2,
    // password=3, database=4). Same trick as nodemailer/pg config
    // parsing — relies on the user declaring the keys in this order
    // in the object literal so perry-runtime's shape-ordered storage
    // puts them at these indices.
    let f0 = js_object_get_field(obj_ptr, 0);
    if let Some(s) = jsvalue_to_string(f0) {
        // First field is a string. Could be `host` or `uri`.
        if let Some(parsed) = parse_mysql_uri(&s) {
            return parsed;
        }
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

// ── Result types (thread-safe intermediate) ───────────────────────

#[derive(Clone, Debug)]
enum RawValue {
    Null,
    Bool(bool),
    Float64(f64),
    String(String),
}

#[derive(Clone, Debug)]
struct RawColumnInfo {
    name: String,
    type_name: String,
}

#[derive(Clone, Debug)]
struct RawRowData {
    values: Vec<(String, RawValue)>,
}

#[derive(Clone, Debug)]
struct RawQueryResult {
    rows: Vec<RawRowData>,
    columns: Vec<RawColumnInfo>,
}

#[derive(Clone, Debug)]
enum QueryOutcome {
    Rows(RawQueryResult),
    Executed {
        affected_rows: u64,
        last_insert_id: u64,
    },
}

fn extract_raw_value(row: &MySqlRow, index: usize, type_name: &str) -> RawValue {
    match type_name {
        "INT" | "TINYINT" | "SMALLINT" | "MEDIUMINT" | "INT UNSIGNED" | "TINYINT UNSIGNED"
        | "SMALLINT UNSIGNED" | "MEDIUMINT UNSIGNED" => row
            .try_get::<i32, _>(index)
            .map(|n| RawValue::Float64(n as f64))
            .unwrap_or(RawValue::Null),
        "BIGINT" | "BIGINT UNSIGNED" => row
            .try_get::<i64, _>(index)
            .map(|n| RawValue::Float64(n as f64))
            .unwrap_or(RawValue::Null),
        "FLOAT" | "DOUBLE" | "DECIMAL" => row
            .try_get::<f64, _>(index)
            .map(RawValue::Float64)
            .unwrap_or(RawValue::Null),
        "BOOLEAN" | "BOOL" => row
            .try_get::<bool, _>(index)
            .map(RawValue::Bool)
            .unwrap_or(RawValue::Null),
        "DATETIME" | "TIMESTAMP" => row
            .try_get::<chrono::NaiveDateTime, _>(index)
            .map(|d| RawValue::String(d.format("%Y-%m-%d %H:%M:%S").to_string()))
            .unwrap_or(RawValue::Null),
        "DATE" => row
            .try_get::<chrono::NaiveDate, _>(index)
            .map(|d| RawValue::String(d.format("%Y-%m-%d").to_string()))
            .unwrap_or(RawValue::Null),
        "TIME" => row
            .try_get::<chrono::NaiveTime, _>(index)
            .map(|d| RawValue::String(d.format("%H:%M:%S").to_string()))
            .unwrap_or(RawValue::Null),
        _ => row
            .try_get::<String, _>(index)
            .map(RawValue::String)
            .or_else(|_| {
                row.try_get::<Vec<u8>, _>(index)
                    .map(|b| RawValue::String(String::from_utf8_lossy(&b).to_string()))
            })
            .unwrap_or(RawValue::Null),
    }
}

fn raws_from_mysql_rows(rows: Vec<MySqlRow>) -> RawQueryResult {
    let columns: Vec<RawColumnInfo> = if !rows.is_empty() {
        rows[0]
            .columns()
            .iter()
            .map(|c| RawColumnInfo {
                name: c.name().to_string(),
                type_name: c.type_info().name().to_string(),
            })
            .collect()
    } else {
        Vec::new()
    };

    let raw_rows: Vec<RawRowData> = rows
        .iter()
        .map(|row| {
            let values = row
                .columns()
                .iter()
                .enumerate()
                .map(|(i, col)| {
                    let value = extract_raw_value(row, i, col.type_info().name());
                    (col.name().to_string(), value)
                })
                .collect();
            RawRowData { values }
        })
        .collect();

    RawQueryResult {
        rows: raw_rows,
        columns,
    }
}

fn raw_value_to_jsvalue(v: &RawValue) -> JsValue {
    match v {
        RawValue::Null => JsValue::NULL,
        RawValue::Bool(b) => JsValue::from_bool(*b),
        RawValue::Float64(f) => JsValue::from_number(*f),
        RawValue::String(s) => JsValue::from_string_ptr(alloc_string(s).as_raw()),
    }
}

fn raw_row_to_js_object(row: &RawRowData) -> *mut ObjectHeader {
    let names: Vec<&str> = row.values.iter().map(|(n, _)| n.as_str()).collect();
    let (packed, shape_id) = build_object_shape(&names);
    let obj = unsafe {
        js_object_alloc_with_shape(
            shape_id,
            names.len() as u32,
            packed.as_ptr(),
            packed.len() as u32,
        )
    };
    for (i, (_, val)) in row.values.iter().enumerate() {
        unsafe { js_object_set_field(obj, i as u32, raw_value_to_jsvalue(val)) };
    }
    obj
}

/// Build a row as a positional ARRAY `[v0, v1, …]` in column order. mysql2's
/// `{ rowsAsArray: true }` option (which Drizzle sets for its relational-query
/// and `select()` paths) returns rows this way; Drizzle's `mapResultRow` then
/// maps positions to columns via the selected-fields list.
fn raw_row_to_js_array(row: &RawRowData) -> *mut ArrayHeader {
    let mut arr = unsafe { js_array_alloc(row.values.len() as u32) };
    for (_, val) in &row.values {
        arr = unsafe { js_array_push(arr, raw_value_to_jsvalue(val)) };
    }
    arr
}

/// Map sqlx's MySQL type *name* back to the wire-protocol numeric type ID
/// (`enum_field_types`, what Node's mysql2 puts in `field.type`/`columnType`).
/// Twin of `perry_stdlib::mysql2::types::mysql_type_id_from_name` (#4917) —
/// this crate cannot depend on perry-stdlib, keep the two in sync.
fn mysql_type_id_from_name(name: &str) -> f64 {
    let base = name.strip_suffix(" UNSIGNED").unwrap_or(name);
    let id: u8 = match base {
        "BOOLEAN" | "TINYINT" => 1,
        "SMALLINT" => 2,
        "INT" => 3,
        "FLOAT" => 4,
        "DOUBLE" => 5,
        "NULL" => 6,
        "TIMESTAMP" => 7,
        "BIGINT" => 8,
        "MEDIUMINT" => 9,
        "DATE" => 10,
        "TIME" => 11,
        "DATETIME" => 12,
        "YEAR" => 13,
        "BIT" => 16,
        "JSON" => 245,
        "DECIMAL" => 246,
        "ENUM" => 247,
        "SET" => 248,
        "TINYBLOB" | "TINYTEXT" => 249,
        "MEDIUMBLOB" | "MEDIUMTEXT" => 250,
        "LONGBLOB" | "LONGTEXT" => 251,
        "BLOB" | "TEXT" => 252,
        "VARCHAR" | "VARBINARY" => 253,
        "CHAR" | "BINARY" => 254,
        "GEOMETRY" => 255,
        _ => 0,
    };
    id as f64
}

fn raw_column_to_field_packet(col: &RawColumnInfo) -> *mut ObjectHeader {
    let (packed, shape_id) = build_object_shape(&["name", "type", "columnType", "length"]);
    let obj =
        unsafe { js_object_alloc_with_shape(shape_id, 4, packed.as_ptr(), packed.len() as u32) };
    let name_str = alloc_string(&col.name);
    // #4917: `type`/`columnType` carry the numeric wire ID mysql2 exposes;
    // `length` stays 0 (sqlx 0.8 keeps the wire `max_size` pub(crate)).
    let type_id = mysql_type_id_from_name(&col.type_name);
    unsafe {
        js_object_set_field(obj, 0, JsValue::from_string_ptr(name_str.as_raw()));
        js_object_set_field(obj, 1, JsValue::from_number(type_id));
        js_object_set_field(obj, 2, JsValue::from_number(type_id));
        js_object_set_field(obj, 3, JsValue::from_number(0.0));
    }
    obj
}

/// Build the mysql2 result tuple `[rows, fields]`. When `rows_as_array` each row
/// is a positional array (mysql2 `{ rowsAsArray: true }`), else a column→value
/// object.
fn raws_to_result_tuple(raw: &RawQueryResult, rows_as_array: bool) -> JsValue {
    let mut result = unsafe { js_array_alloc(2) };
    let mut rows_arr = unsafe { js_array_alloc(raw.rows.len() as u32) };
    for r in &raw.rows {
        let row_val = if rows_as_array {
            JsValue::from_object_ptr(raw_row_to_js_array(r))
        } else {
            JsValue::from_object_ptr(raw_row_to_js_object(r))
        };
        rows_arr = unsafe { js_array_push(rows_arr, row_val) };
    }
    result = unsafe { js_array_push(result, JsValue::from_object_ptr(rows_arr)) };

    let mut fields_arr = unsafe { js_array_alloc(raw.columns.len() as u32) };
    for c in &raw.columns {
        let obj = raw_column_to_field_packet(c);
        fields_arr = unsafe { js_array_push(fields_arr, JsValue::from_object_ptr(obj)) };
    }
    result = unsafe { js_array_push(result, JsValue::from_object_ptr(fields_arr)) };
    JsValue::from_object_ptr(result)
}

/// `[ResultSetHeader, []]` for non-SELECT queries.
fn affected_rows_result(affected: u64, last_insert_id: u64) -> JsValue {
    let mut result = unsafe { js_array_alloc(2) };
    let (packed, shape_id) = build_object_shape(&["affectedRows", "insertId", "warningStatus"]);
    let header =
        unsafe { js_object_alloc_with_shape(shape_id, 3, packed.as_ptr(), packed.len() as u32) };
    unsafe {
        js_object_set_field(header, 0, JsValue::from_number(affected as f64));
        js_object_set_field(header, 1, JsValue::from_number(last_insert_id as f64));
        js_object_set_field(header, 2, JsValue::from_number(0.0));
    }
    result = unsafe { js_array_push(result, JsValue::from_object_ptr(header)) };
    let empty_fields = unsafe { js_array_alloc(0) };
    result = unsafe { js_array_push(result, JsValue::from_object_ptr(empty_fields)) };
    JsValue::from_object_ptr(result)
}

fn outcome_to_jsvalue(outcome: &QueryOutcome, rows_as_array: bool) -> JsValue {
    match outcome {
        QueryOutcome::Rows(raw) => raws_to_result_tuple(raw, rows_as_array),
        QueryOutcome::Executed {
            affected_rows,
            last_insert_id,
        } => affected_rows_result(*affected_rows, *last_insert_id),
    }
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
    let length = (*arr_ptr).length;
    let mut result = Vec::with_capacity(length as usize);
    for i in 0..length {
        let element = js_array_get(arr_ptr, i);
        let p = if element.is_null() || element.is_undefined() {
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
        result.push(p);
    }
    result
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

// ── Connection ────────────────────────────────────────────────────

pub struct MysqlConnectionHandle {
    pub connection: Option<MySqlConnection>,
}

impl MysqlConnectionHandle {
    pub fn new(conn: MySqlConnection) -> Self {
        Self {
            connection: Some(conn),
        }
    }
}

/// `mysql.createConnection(config) -> Promise<Connection>`.
///
/// # Safety
/// `config_f` is a NaN-boxed JsValue.
#[no_mangle]
pub unsafe extern "C" fn js_mysql2_create_connection(config_f: f64) -> *mut Promise {
    let config = JsValue::from_bits(config_f.to_bits());
    let mysql_config = parse_mysql_config(config);
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    spawn_blocking(move || {
        let result = tokio::runtime::Handle::current().block_on(async move {
            let url = mysql_config.to_url();
            tokio::time::timeout(
                Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS),
                MySqlConnection::connect(&url),
            )
            .await
            .map_err(|_| "MySQL connection timed out".to_string())?
            .map_err(|e| format!("Failed to connect: {}", e))
        });
        match result {
            Ok(conn) => {
                let handle = register_handle(MysqlConnectionHandle::new(conn));
                promise.resolve(JsValue::from_number(handle as f64));
            }
            Err(e) => promise.reject_string(&e),
        }
    });
    raw
}

#[no_mangle]
pub extern "C" fn js_mysql2_connection_end(conn_handle: Handle) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    spawn_blocking(move || {
        if let Some(mut wrapper) = take_handle::<MysqlConnectionHandle>(conn_handle) {
            if let Some(conn) = wrapper.connection.take() {
                let result = tokio::runtime::Handle::current().block_on(conn.close());
                match result {
                    Ok(()) => promise.resolve_undefined(),
                    Err(e) => promise.reject_string(&format!("Failed to close: {}", e)),
                }
            } else {
                promise.reject_string("Connection already closed");
            }
        } else {
            promise.reject_string("Invalid connection handle");
        }
    });
    raw
}

unsafe fn run_connection_query(
    conn_handle: Handle,
    sql_ptr: *const u8,
    params_f: f64,
    rows_as_array: bool,
) -> *mut Promise {
    let sql = read_sql(sql_ptr);
    let params = JsValue::from_bits(params_f.to_bits());
    let param_values = extract_params_from_jsvalue(params);
    let is_select = is_row_returning_query(&sql);

    let promise = JsPromise::new();
    let raw = promise.as_raw();

    spawn_blocking(move || {
        let outcome: Result<QueryOutcome, String> =
            tokio::runtime::Handle::current().block_on(async move {
                let wrapper = get_handle_mut::<MysqlConnectionHandle>(conn_handle)
                    .ok_or_else(|| "Invalid connection handle".to_string())?;
                let conn = wrapper
                    .connection
                    .as_mut()
                    .ok_or_else(|| "Connection already closed".to_string())?;
                let mut q = sqlx::query(sqlx::AssertSqlSafe(sql.clone()));
                for p in &param_values {
                    q = match p {
                        ParamValue::Null => q.bind(Option::<String>::None),
                        ParamValue::String(s) => q.bind(s.clone()),
                        ParamValue::Number(n) => q.bind(*n),
                        ParamValue::Int(i) => q.bind(*i),
                        ParamValue::Bool(b) => q.bind(*b),
                    };
                }
                if is_select {
                    let rows = tokio::time::timeout(
                        Duration::from_secs(DEFAULT_QUERY_TIMEOUT_SECS),
                        q.fetch_all(conn),
                    )
                    .await
                    .map_err(|_| "Query timed out".to_string())?
                    .map_err(|e| format!("Query failed: {}", e))?;
                    Ok(QueryOutcome::Rows(raws_from_mysql_rows(rows)))
                } else {
                    let res = tokio::time::timeout(
                        Duration::from_secs(DEFAULT_QUERY_TIMEOUT_SECS),
                        q.execute(conn),
                    )
                    .await
                    .map_err(|_| "Query timed out".to_string())?
                    .map_err(|e| format!("Query failed: {}", e))?;
                    Ok(QueryOutcome::Executed {
                        affected_rows: res.rows_affected(),
                        last_insert_id: res.last_insert_id(),
                    })
                }
            });
        match outcome {
            // #1824: build the JS result on the MAIN thread. outcome_to_jsvalue
            // allocates arrays/objects/strings, which is UB on this blocking-pool
            // thread (worker thread-local arena → dangling on the main thread once
            // the pooled thread idles out). `out` is plain Send Rust data.
            Ok(out) => promise.resolve_with(move || outcome_to_jsvalue(&out, rows_as_array)),
            Err(e) => promise.reject_string(&e),
        }
    });
    raw
}

/// `connection.query(sql, params) -> Promise<[rows, fields]>`.
///
/// # Safety
/// `sql_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_mysql2_connection_query(
    conn_handle: Handle,
    sql_ptr: *const u8,
    params_f: f64,
) -> *mut Promise {
    run_connection_query(conn_handle, sql_ptr, params_f, false)
}

/// `connection.execute(sql, params) -> Promise<[rows, fields]>`.
/// Same backing as `query` for now (sqlx prepares all queries).
///
/// # Safety
/// `sql_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_mysql2_connection_execute(
    conn_handle: Handle,
    sql_ptr: *const u8,
    params_f: f64,
) -> *mut Promise {
    run_connection_query(conn_handle, sql_ptr, params_f, false)
}

fn run_simple_command(conn_handle: Handle, sql: &'static str) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    spawn_blocking(move || {
        let result = tokio::runtime::Handle::current().block_on(async move {
            let wrapper = get_handle_mut::<MysqlConnectionHandle>(conn_handle)
                .ok_or_else(|| "Invalid connection handle".to_string())?;
            let conn = wrapper
                .connection
                .as_mut()
                .ok_or_else(|| "Connection already closed".to_string())?;
            sqlx::query(sql)
                .execute(conn)
                .await
                .map(|_| ())
                .map_err(|e| format!("{}: {}", sql, e))
        });
        match result {
            Ok(()) => promise.resolve_undefined(),
            Err(e) => promise.reject_string(&e),
        }
    });
    raw
}

#[no_mangle]
pub extern "C" fn js_mysql2_connection_begin_transaction(conn_handle: Handle) -> *mut Promise {
    run_simple_command(conn_handle, "START TRANSACTION")
}

#[no_mangle]
pub extern "C" fn js_mysql2_connection_commit(conn_handle: Handle) -> *mut Promise {
    run_simple_command(conn_handle, "COMMIT")
}

#[no_mangle]
pub extern "C" fn js_mysql2_connection_rollback(conn_handle: Handle) -> *mut Promise {
    run_simple_command(conn_handle, "ROLLBACK")
}

// ── Pool ──────────────────────────────────────────────────────────

pub struct MysqlPoolHandle {
    pub pool: MySqlPool,
}

impl MysqlPoolHandle {
    pub fn new(pool: MySqlPool) -> Self {
        Self { pool }
    }
}

pub struct MysqlPoolConnectionHandle {
    pub connection: Option<PoolConnection<MySql>>,
}

impl MysqlPoolConnectionHandle {
    pub fn new(conn: PoolConnection<MySql>) -> Self {
        Self {
            connection: Some(conn),
        }
    }
}

/// `mysql.createPool(config) -> Pool` — sync; eager connect on
/// first use (matches perry-stdlib's existing eager-first-call
/// behavior). Returns 0 if connection fails.
///
/// # Safety
/// `config_f` is a NaN-boxed JsValue.
#[no_mangle]
pub unsafe extern "C" fn js_mysql2_create_pool(config_f: f64) -> Handle {
    // Register the handle method dispatch so generic `pool.query(...)` calls work
    // (Drizzle threads the pool through interface-typed values, losing the static
    // "Pool" type the codegen needs to route `.query`/`.execute` natively).
    ensure_dispatch_registered();
    let config = JsValue::from_bits(config_f.to_bits());
    let mysql_config = parse_mysql_config(config);
    let url = mysql_config.to_url();

    // mysql2's `createPool` is SYNCHRONOUS and does NOT open a connection —
    // it returns a pool that connects lazily on first query. Mirror that with
    // sqlx `connect_lazy`: no eager connect (which returned handle 0 when the
    // DB was unreachable, so the JS pool value was null and every
    // `pool.constructor` / drizzle `isConfig(pool)` read crashed). `connect_lazy`
    // still spawns the pool's background reaper, which needs an entered Tokio
    // context — run it inside `spawn_blocking` (where the global runtime handle
    // is current) rather than at the bare module-init call site (which panicked
    // "no reactor running").
    let (tx, rx) = std::sync::mpsc::channel();
    spawn_blocking(move || {
        // `connect_lazy` is synchronous but still spawns the pool's reaper task,
        // which needs the runtime context. Run it inside `block_on` (same as the
        // old eager path) so the spawn has a live reactor; the body returns
        // immediately because no connection is opened.
        let pool_result = tokio::runtime::Handle::current().block_on(async {
            MySqlPoolOptions::new()
                .max_connections(10)
                .acquire_timeout(Duration::from_secs(DEFAULT_ACQUIRE_TIMEOUT_SECS))
                .connect_lazy(&url)
        });
        let _ = tx.send(pool_result);
    });
    match rx.recv().ok().and_then(|r| r.ok()) {
        Some(pool) => register_handle(MysqlPoolHandle::new(pool)),
        None => 0,
    }
}

// ── Generic handle method dispatch ────────────────────────────────
//
// The codegen routes `pool.query(sql, params)` to the native query fns ONLY
// when the receiver is statically typed as a mysql2 `Pool`. Drizzle stores the
// pool behind interface-typed fields (`this.client: Pool | Connection`), so by
// the time it calls `client.query(query, params)` the static type is lost and
// the call falls back to a generic dynamic dispatch that returned garbage. The
// runtime consults HANDLE_METHOD_DISPATCH for a generic call on a small handle;
// register an extension here so a mysql2 pool/connection handle answers
// `query`/`execute` (and `end`/`getConnection`) regardless of static type.

const DISPATCH_POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const DISPATCH_POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
const DISPATCH_TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

extern "C" {
    fn js_register_handle_method_dispatch_extension(
        f: unsafe extern "C" fn(i64, *const u8, usize, *const f64, usize, *mut f64) -> i32,
    );
    // Runtime generic field read; returns the runtime `JSValue` (repr-transparent
    // u64), ABI-compatible with `u64` here.
    fn js_object_get_field_by_name(obj: *const ObjectHeader, key: *const StringHeader) -> u64;
}

fn dispatch_nanbox_ptr<T>(ptr: *mut T) -> f64 {
    f64::from_bits(DISPATCH_POINTER_TAG | (ptr as u64 & DISPATCH_POINTER_MASK))
}

fn ensure_dispatch_registered() {
    static REGISTER: std::sync::Once = std::sync::Once::new();
    REGISTER.call_once(|| unsafe {
        js_register_handle_method_dispatch_extension(js_mysql2_handle_method_dispatch);
    });
}

/// Read a named field off a JS object value. Returns `JsValue::UNDEFINED` for a
/// non-object receiver or a missing key.
unsafe fn object_field_by_name(obj: JsValue, name: &str) -> JsValue {
    let obj_ptr = obj.as_pointer::<ObjectHeader>();
    if obj_ptr.is_null() {
        return JsValue::UNDEFINED;
    }
    let key = alloc_string(name);
    let bits = js_object_get_field_by_name(obj_ptr, key.as_raw());
    JsValue::from_bits(bits)
}

/// Resolve the SQL `StringHeader` pointer and `rowsAsArray` flag from a `query`
/// argument that is either a SQL string or a mysql2 options object
/// (`{ sql, rowsAsArray? }` — Drizzle's shape).
unsafe fn query_sql_ptr_and_rows_as_array(arg: JsValue) -> Option<(*const u8, bool)> {
    if arg.is_string() {
        let p = arg.as_string_ptr();
        if p.is_null() {
            return None;
        }
        return Some((p as *const u8, false));
    }
    if arg.is_pointer() {
        let sql_val = object_field_by_name(arg, "sql");
        if sql_val.is_string() {
            let p = sql_val.as_string_ptr();
            if !p.is_null() {
                let rows_as_array = object_field_by_name(arg, "rowsAsArray").to_bool();
                return Some((p as *const u8, rows_as_array));
            }
        }
    }
    None
}

/// Handle-method dispatch extension for mysql2 pool / connection handles.
#[no_mangle]
unsafe extern "C" fn js_mysql2_handle_method_dispatch(
    handle: i64,
    method_name_ptr: *const u8,
    method_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
    out: *mut f64,
) -> i32 {
    if method_name_ptr.is_null() || method_name_len == 0 {
        return 0;
    }
    let method =
        match std::str::from_utf8(std::slice::from_raw_parts(method_name_ptr, method_name_len)) {
            Ok(m) => m,
            Err(_) => return 0,
        };
    let args: &[f64] = if args_ptr.is_null() || args_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(args_ptr, args_len)
    };
    let arg = |i: usize| -> JsValue {
        args.get(i)
            .copied()
            .map(|f| JsValue::from_bits(f.to_bits()))
            .unwrap_or(JsValue::UNDEFINED)
    };

    // Only claim methods for handles we actually own.
    let is_pool = perry_ffi::get_handle::<MysqlPoolHandle>(handle).is_some();
    let is_pool_conn = perry_ffi::get_handle::<MysqlPoolConnectionHandle>(handle).is_some();
    let is_conn = perry_ffi::get_handle::<MysqlConnectionHandle>(handle).is_some();
    if !is_pool && !is_pool_conn && !is_conn {
        return 0;
    }

    let result: f64 = match method {
        "query" | "execute" => {
            let Some((sql_ptr, rows_as_array)) = query_sql_ptr_and_rows_as_array(arg(0)) else {
                return 0;
            };
            let params_f = args
                .get(1)
                .copied()
                .unwrap_or(f64::from_bits(DISPATCH_TAG_UNDEFINED));
            let promise = if is_pool {
                run_pool_query(handle, sql_ptr, params_f, rows_as_array)
            } else if is_pool_conn {
                run_pool_conn_query(handle, sql_ptr, params_f, rows_as_array)
            } else {
                run_connection_query(handle, sql_ptr, params_f, rows_as_array)
            };
            dispatch_nanbox_ptr(promise)
        }
        "getConnection" if is_pool => dispatch_nanbox_ptr(js_mysql2_pool_get_connection(handle)),
        "end" if is_pool => dispatch_nanbox_ptr(js_mysql2_pool_end(handle)),
        "release" if is_pool_conn => {
            js_mysql2_pool_connection_release(handle);
            f64::from_bits(DISPATCH_TAG_UNDEFINED)
        }
        "end" if is_conn => dispatch_nanbox_ptr(js_mysql2_connection_end(handle)),
        // `mysql2/promise` pools are already promise-based: `pool.promise()`
        // returns the pool itself. Drizzle's `isCallbackClient` only reaches this
        // when it mis-detects; return the same handle to be safe.
        "promise" => dispatch_nanbox_ptr(handle as *mut u8),
        _ => return 0,
    };

    if !out.is_null() {
        *out = result;
    }
    1
}

#[no_mangle]
pub extern "C" fn js_mysql2_pool_end(pool_handle: Handle) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    spawn_blocking(move || {
        if let Some(wrapper) = take_handle::<MysqlPoolHandle>(pool_handle) {
            tokio::runtime::Handle::current().block_on(wrapper.pool.close());
            promise.resolve_undefined();
        } else {
            promise.reject_string("Invalid pool handle");
        }
    });
    raw
}

unsafe fn run_pool_query(
    pool_handle: Handle,
    sql_ptr: *const u8,
    params_f: f64,
    rows_as_array: bool,
) -> *mut Promise {
    let sql = read_sql(sql_ptr);
    let params = JsValue::from_bits(params_f.to_bits());
    let param_values = extract_params_from_jsvalue(params);
    let is_select = is_row_returning_query(&sql);
    let promise = JsPromise::new();
    let raw = promise.as_raw();

    spawn_blocking(move || {
        let outcome: Result<QueryOutcome, String> =
            tokio::runtime::Handle::current().block_on(async move {
                let wrapper = get_handle_mut::<MysqlPoolHandle>(pool_handle)
                    .ok_or_else(|| "Invalid pool handle".to_string())?;
                let pool = &wrapper.pool;
                let mut q = sqlx::query(sqlx::AssertSqlSafe(sql.clone()));
                for p in &param_values {
                    q = match p {
                        ParamValue::Null => q.bind(Option::<String>::None),
                        ParamValue::String(s) => q.bind(s.clone()),
                        ParamValue::Number(n) => q.bind(*n),
                        ParamValue::Int(i) => q.bind(*i),
                        ParamValue::Bool(b) => q.bind(*b),
                    };
                }
                if is_select {
                    let rows = tokio::time::timeout(
                        Duration::from_secs(DEFAULT_QUERY_TIMEOUT_SECS),
                        q.fetch_all(pool),
                    )
                    .await
                    .map_err(|_| "Query timed out".to_string())?
                    .map_err(|e| format!("Query failed: {}", e))?;
                    Ok(QueryOutcome::Rows(raws_from_mysql_rows(rows)))
                } else {
                    let res = tokio::time::timeout(
                        Duration::from_secs(DEFAULT_QUERY_TIMEOUT_SECS),
                        q.execute(pool),
                    )
                    .await
                    .map_err(|_| "Query timed out".to_string())?
                    .map_err(|e| format!("Query failed: {}", e))?;
                    Ok(QueryOutcome::Executed {
                        affected_rows: res.rows_affected(),
                        last_insert_id: res.last_insert_id(),
                    })
                }
            });
        match outcome {
            // #1824: build the JS result on the MAIN thread. outcome_to_jsvalue
            // allocates arrays/objects/strings, which is UB on this blocking-pool
            // thread (worker thread-local arena → dangling on the main thread once
            // the pooled thread idles out). `out` is plain Send Rust data.
            Ok(out) => promise.resolve_with(move || outcome_to_jsvalue(&out, rows_as_array)),
            Err(e) => promise.reject_string(&e),
        }
    });
    raw
}

/// # Safety
/// `sql_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_mysql2_pool_query(
    pool_handle: Handle,
    sql_ptr: *const u8,
    params_f: f64,
) -> *mut Promise {
    run_pool_query(pool_handle, sql_ptr, params_f, false)
}

/// # Safety
/// `sql_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_mysql2_pool_execute(
    pool_handle: Handle,
    sql_ptr: *const u8,
    params_f: f64,
) -> *mut Promise {
    run_pool_query(pool_handle, sql_ptr, params_f, false)
}

#[no_mangle]
pub extern "C" fn js_mysql2_pool_get_connection(pool_handle: Handle) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    spawn_blocking(move || {
        let result = tokio::runtime::Handle::current().block_on(async move {
            let wrapper = get_handle_mut::<MysqlPoolHandle>(pool_handle)
                .ok_or_else(|| "Invalid pool handle".to_string())?;
            tokio::time::timeout(
                Duration::from_secs(DEFAULT_ACQUIRE_TIMEOUT_SECS),
                wrapper.pool.acquire(),
            )
            .await
            .map_err(|_| "Pool acquire timed out".to_string())?
            .map_err(|e| format!("Pool acquire failed: {}", e))
        });
        match result {
            Ok(conn) => {
                let h = register_handle(MysqlPoolConnectionHandle::new(conn));
                promise.resolve(JsValue::from_number(h as f64));
            }
            Err(e) => promise.reject_string(&e),
        }
    });
    raw
}

/// `connection.release()` — drops the pool-connection handle so the
/// underlying `PoolConnection<MySql>` returns to the pool via Drop.
#[no_mangle]
pub extern "C" fn js_mysql2_pool_connection_release(conn_handle: Handle) {
    take_handle::<MysqlPoolConnectionHandle>(conn_handle);
}

unsafe fn run_pool_conn_query(
    conn_handle: Handle,
    sql_ptr: *const u8,
    params_f: f64,
    rows_as_array: bool,
) -> *mut Promise {
    let sql = read_sql(sql_ptr);
    let params = JsValue::from_bits(params_f.to_bits());
    let param_values = extract_params_from_jsvalue(params);
    let is_select = is_row_returning_query(&sql);
    let promise = JsPromise::new();
    let raw = promise.as_raw();

    spawn_blocking(move || {
        let outcome: Result<QueryOutcome, String> =
            tokio::runtime::Handle::current().block_on(async move {
                let wrapper = get_handle_mut::<MysqlPoolConnectionHandle>(conn_handle)
                    .ok_or_else(|| "Invalid pool-connection handle".to_string())?;
                let conn = wrapper
                    .connection
                    .as_mut()
                    .ok_or_else(|| "Pool connection released".to_string())?;
                let mut q = sqlx::query(sqlx::AssertSqlSafe(sql.clone()));
                for p in &param_values {
                    q = match p {
                        ParamValue::Null => q.bind(Option::<String>::None),
                        ParamValue::String(s) => q.bind(s.clone()),
                        ParamValue::Number(n) => q.bind(*n),
                        ParamValue::Int(i) => q.bind(*i),
                        ParamValue::Bool(b) => q.bind(*b),
                    };
                }
                if is_select {
                    let rows = tokio::time::timeout(
                        Duration::from_secs(DEFAULT_QUERY_TIMEOUT_SECS),
                        q.fetch_all(&mut **conn),
                    )
                    .await
                    .map_err(|_| "Query timed out".to_string())?
                    .map_err(|e| format!("Query failed: {}", e))?;
                    Ok(QueryOutcome::Rows(raws_from_mysql_rows(rows)))
                } else {
                    let res = tokio::time::timeout(
                        Duration::from_secs(DEFAULT_QUERY_TIMEOUT_SECS),
                        q.execute(&mut **conn),
                    )
                    .await
                    .map_err(|_| "Query timed out".to_string())?
                    .map_err(|e| format!("Query failed: {}", e))?;
                    Ok(QueryOutcome::Executed {
                        affected_rows: res.rows_affected(),
                        last_insert_id: res.last_insert_id(),
                    })
                }
            });
        match outcome {
            // #1824: build the JS result on the MAIN thread. outcome_to_jsvalue
            // allocates arrays/objects/strings, which is UB on this blocking-pool
            // thread (worker thread-local arena → dangling on the main thread once
            // the pooled thread idles out). `out` is plain Send Rust data.
            Ok(out) => promise.resolve_with(move || outcome_to_jsvalue(&out, rows_as_array)),
            Err(e) => promise.reject_string(&e),
        }
    });
    raw
}

/// # Safety
/// `sql_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_mysql2_pool_connection_query(
    conn_handle: Handle,
    sql_ptr: *const u8,
    params_f: f64,
) -> *mut Promise {
    run_pool_conn_query(conn_handle, sql_ptr, params_f, false)
}

/// # Safety
/// `sql_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_mysql2_pool_connection_execute(
    conn_handle: Handle,
    sql_ptr: *const u8,
    params_f: f64,
) -> *mut Promise {
    run_pool_conn_query(conn_handle, sql_ptr, params_f, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults() {
        let cfg = MySqlConfig::default();
        assert_eq!(cfg.host, "localhost");
        assert_eq!(cfg.port, 3306);
        assert_eq!(cfg.user, "root");
    }

    #[test]
    fn url_encodes_password_special_chars() {
        let cfg = MySqlConfig {
            host: "h".into(),
            port: 1,
            user: "u".into(),
            password: "p@s/s#".into(),
            database: None,
        };
        let url = cfg.to_url();
        assert!(url.contains("p%40s%2Fs%23"));
    }

    #[test]
    fn parse_uri_basic() {
        let p = parse_mysql_uri("mysql://root:secret@db.example.com:3307/mydb").unwrap();
        assert_eq!(p.host, "db.example.com");
        assert_eq!(p.port, 3307);
        assert_eq!(p.user, "root");
        assert_eq!(p.password, "secret");
        assert_eq!(p.database.as_deref(), Some("mydb"));
    }

    #[test]
    fn is_row_returning_query_classifier() {
        assert!(is_row_returning_query("SELECT 1"));
        assert!(is_row_returning_query("SHOW TABLES"));
        assert!(is_row_returning_query(
            "WITH cte AS (SELECT 1) SELECT * FROM cte"
        ));
        assert!(!is_row_returning_query("INSERT INTO t VALUES (1)"));
        assert!(!is_row_returning_query("UPDATE t SET x = 1"));
        assert!(!is_row_returning_query("DELETE FROM t"));
    }
}
