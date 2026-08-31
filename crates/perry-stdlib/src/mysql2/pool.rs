//! MySQL connection pool implementation

use std::time::Duration;

use perry_runtime::{
    js_array_get_jsvalue, js_array_length, js_promise_new_cross_thread, JSValue, Promise,
};
use sqlx::mysql::{MySqlPool, MySqlPoolOptions};
use sqlx::pool::PoolConnection;
use sqlx::MySql;

use super::result::{is_row_returning_query, QueryOutcome, RawQueryResult};
use super::types::parse_mysql_config;
use crate::common::{register_handle, take_handle, Handle};

/// Default timeout for acquiring a connection from the pool (in seconds)
const DEFAULT_ACQUIRE_TIMEOUT_SECS: u64 = 10;
/// Default timeout for connecting to the database (in seconds)
const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 10;
/// Default timeout for overall query operation (in seconds)
const DEFAULT_QUERY_TIMEOUT_SECS: u64 = 30;

/// Wrapper around MySqlPool
pub struct MysqlPoolHandle {
    pub pool: MySqlPool,
}

impl MysqlPoolHandle {
    pub fn new(pool: MySqlPool) -> Self {
        Self { pool }
    }
}

/// Wrapper around a pool connection
/// When dropped, the connection is automatically returned to the pool
pub struct MysqlPoolConnectionHandle {
    pub connection: Option<PoolConnection<MySql>>,
}

impl MysqlPoolConnectionHandle {
    pub fn new(conn: PoolConnection<MySql>) -> Self {
        Self {
            connection: Some(conn),
        }
    }

    /// Take the connection out of this handle
    pub fn take(&mut self) -> Option<PoolConnection<MySql>> {
        self.connection.take()
    }
}

/// mysql.createPool(config) -> Pool
///
/// Creates a new connection pool. The pool connects lazily, so this
/// returns synchronously.
///
/// # Safety
/// The config parameter must be a valid JSValue representing a config object.
#[no_mangle]
pub unsafe extern "C" fn js_mysql2_create_pool(config_f: f64) -> Handle {
    // Take f64 at the FFI boundary to avoid SysV AMD64 ABI mismatch:
    // JSValue is `#[repr(transparent)] u64` (integer register), but the
    // LLVM call site declares the arg as `double` (XMM register). On ARM64
    // these aliases (d0/x0 same phys reg) so the bug is invisible, but on
    // x86_64 they're distinct registers and the pointer bits never arrive.
    let config = JSValue::from_bits(config_f.to_bits());
    let mysql_config = parse_mysql_config(config);
    let url = mysql_config.to_url();

    // Create pool with lazy connection using the tokio runtime context
    // We need to enter the runtime context for connect_lazy to work
    let _guard = crate::common::runtime().enter();

    // Use eager connection to get immediate error feedback
    let pool_result = crate::common::runtime().block_on(async {
        MySqlPoolOptions::new()
            .max_connections(10)
            .acquire_timeout(Duration::from_secs(DEFAULT_ACQUIRE_TIMEOUT_SECS))
            .connect(&url)
            .await
    });

    match pool_result {
        Ok(pool) => register_handle(MysqlPoolHandle::new(pool)),
        Err(_e) => 0,
    }
}

/// pool.end() -> Promise<void>
///
/// Closes all connections in the pool.
#[no_mangle]
pub unsafe extern "C" fn js_mysql2_pool_end(pool_handle: Handle) -> *mut Promise {
    let promise = js_promise_new_cross_thread();

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        use crate::common::take_handle;
        use tokio::time::timeout;

        if let Some(wrapper) = take_handle::<MysqlPoolHandle>(pool_handle) {
            // Wrap pool close in a timeout (use shorter timeout since close should be fast)
            match timeout(
                Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS),
                wrapper.pool.close(),
            )
            .await
            {
                Ok(()) => Ok(JSValue::undefined().bits()),
                Err(_) => {
                    // Pool close timed out, but we've already taken the handle so just return
                    Ok(JSValue::undefined().bits())
                }
            }
        } else {
            Err("Invalid pool handle".to_string())
        }
    });

    promise
}

/// pool.query(sql, params?) -> Promise<[rows, fields]>
///
/// Executes a query using a connection from the pool.
///
/// `params` is the optional second arg user code passes to `db.query(sql, [..])`.
/// The codegen dispatch table for `("mysql2", "Pool", "query")` declares
/// `args: &[NA_STR, NA_F64]` so the call site always emits 3 arguments
/// (handle + sql + params). When the user omits `params`, codegen pads the
/// slot with JS `undefined`; `extract_params_from_jsvalue` returns an
/// empty Vec for that case. When the user passes an array, sqlx builds a
/// prepared statement and binds each value — same code path as
/// `js_mysql2_pool_execute`. Without binding, sqlx sends the binary execute
/// frame with 1 placeholder but 0 bind values, MySQL replies with error
/// 1835 ("Malformed communication packet"), and the connection becomes
/// unusable. See issue #414.
#[no_mangle]
pub unsafe extern "C" fn js_mysql2_pool_query(
    pool_handle: Handle,
    sql_ptr: *const u8,
    params_f: f64,
) -> *mut Promise {
    let promise = js_promise_new_cross_thread();
    let params = JSValue::from_bits(params_f.to_bits());

    // Extract the SQL string
    let sql = if sql_ptr.is_null() {
        String::new()
    } else {
        let header = sql_ptr as *const perry_runtime::StringHeader;
        let len = (*header).byte_len as usize;
        let data_ptr = sql_ptr.add(std::mem::size_of::<perry_runtime::StringHeader>());
        let bytes = std::slice::from_raw_parts(data_ptr, len);
        String::from_utf8_lossy(bytes).to_string()
    };

    // Extract parameters from the JSValue array (empty Vec when caller
    // passed no params — `extract_params_from_jsvalue` short-circuits on
    // 0/undefined/non-array).
    let param_values = extract_params_from_jsvalue(params);
    let is_select = is_row_returning_query(&sql);

    // Use spawn_for_promise_deferred to safely create JSValues on the main thread
    // The async block returns raw Rust data, and the converter creates JSValues
    crate::common::spawn_for_promise_deferred(
        promise as *mut u8,
        async move {
            use crate::common::get_handle;
            use tokio::time::timeout;

            let param_values = param_values?;

            if let Some(wrapper) = get_handle::<MysqlPoolHandle>(pool_handle) {
                // Build the query with parameter bindings (no-op when
                // param_values is empty, preserving the no-param call shape).
                let mut query = sqlx::query(sqlx::AssertSqlSafe(sql.clone()));
                for param in &param_values {
                    query = match param {
                        ParamValue::Null => query.bind(Option::<String>::None),
                        ParamValue::String(s) => query.bind(s.clone()),
                        ParamValue::Bytes(bytes) => query.bind(bytes.clone()),
                        ParamValue::DateTime(date) => query.bind(*date),
                        ParamValue::Number(n) => query.bind(*n),
                        ParamValue::Int(i) => query.bind(*i),
                        ParamValue::Bool(b) => query.bind(*b),
                    };
                }

                if is_select {
                    // SELECT/SHOW/DESCRIBE: fetch rows
                    let query_future = query.fetch_all(&wrapper.pool);
                    match timeout(
                        Duration::from_secs(DEFAULT_QUERY_TIMEOUT_SECS),
                        query_future,
                    )
                    .await
                    {
                        Ok(Ok(rows)) => {
                            let raw_result = RawQueryResult::from_mysql_rows(rows);
                            Ok(QueryOutcome::Rows(raw_result))
                        }
                        Ok(Err(e)) => Err(format!("Query failed: {}", e)),
                        Err(_) => Err(format!(
                            "Query timed out after {} seconds (MySQL server may be unavailable)",
                            DEFAULT_QUERY_TIMEOUT_SECS
                        )),
                    }
                } else {
                    // INSERT/UPDATE/DELETE: execute and return metadata
                    let query_future = query.execute(&wrapper.pool);
                    match timeout(
                        Duration::from_secs(DEFAULT_QUERY_TIMEOUT_SECS),
                        query_future,
                    )
                    .await
                    {
                        Ok(Ok(result)) => Ok(QueryOutcome::Executed {
                            affected_rows: result.rows_affected(),
                            last_insert_id: result.last_insert_id(),
                        }),
                        Ok(Err(e)) => Err(format!("Query failed: {}", e)),
                        Err(_) => Err(format!(
                            "Query timed out after {} seconds (MySQL server may be unavailable)",
                            DEFAULT_QUERY_TIMEOUT_SECS
                        )),
                    }
                }
            } else {
                Err("Invalid pool handle".to_string())
            }
        },
        // Converter runs on main thread - safe to create JSValues here
        |outcome: QueryOutcome| outcome.to_jsvalue().bits(),
    );

    promise
}

/// pool.execute(sql, params) -> Promise<[rows, fields]>
///
/// Executes a prepared statement with parameters using a connection from the pool.
#[no_mangle]
pub unsafe extern "C" fn js_mysql2_pool_execute(
    pool_handle: Handle,
    sql_ptr: *const u8,
    params_f: f64,
) -> *mut Promise {
    let promise = js_promise_new_cross_thread();
    let params = JSValue::from_bits(params_f.to_bits());

    // Extract the SQL string
    let sql = if sql_ptr.is_null() {
        String::new()
    } else {
        let header = sql_ptr as *const perry_runtime::StringHeader;
        let len = (*header).byte_len as usize;
        let data_ptr = sql_ptr.add(std::mem::size_of::<perry_runtime::StringHeader>());
        let bytes = std::slice::from_raw_parts(data_ptr, len);
        String::from_utf8_lossy(bytes).to_string()
    };

    // Extract parameters from the JSValue array
    let param_values = extract_params_from_jsvalue(params);
    let is_select = is_row_returning_query(&sql);

    // Use spawn_for_promise_deferred to safely create JSValues on the main thread
    crate::common::spawn_for_promise_deferred(
        promise as *mut u8,
        async move {
            use crate::common::get_handle;
            use tokio::time::timeout;

            let param_values = param_values?;

            if let Some(wrapper) = get_handle::<MysqlPoolHandle>(pool_handle) {
                // Build the query with parameter bindings
                let mut query = sqlx::query(sqlx::AssertSqlSafe(sql.clone()));

                for param in &param_values {
                    query = match param {
                        ParamValue::Null => query.bind(Option::<String>::None),
                        ParamValue::String(s) => query.bind(s.clone()),
                        ParamValue::Bytes(bytes) => query.bind(bytes.clone()),
                        ParamValue::DateTime(date) => query.bind(*date),
                        ParamValue::Number(n) => query.bind(*n),
                        ParamValue::Int(i) => query.bind(*i),
                        ParamValue::Bool(b) => query.bind(*b),
                    };
                }

                if is_select {
                    let query_future = query.fetch_all(&wrapper.pool);
                    match timeout(
                        Duration::from_secs(DEFAULT_QUERY_TIMEOUT_SECS),
                        query_future,
                    )
                    .await
                    {
                        Ok(Ok(rows)) => {
                            let raw_result = RawQueryResult::from_mysql_rows(rows);
                            Ok(QueryOutcome::Rows(raw_result))
                        }
                        Ok(Err(e)) => Err(format!("Query failed: {}", e)),
                        Err(_) => Err(format!(
                            "Query timed out after {} seconds (MySQL server may be unavailable)",
                            DEFAULT_QUERY_TIMEOUT_SECS
                        )),
                    }
                } else {
                    let query_future = query.execute(&wrapper.pool);
                    match timeout(
                        Duration::from_secs(DEFAULT_QUERY_TIMEOUT_SECS),
                        query_future,
                    )
                    .await
                    {
                        Ok(Ok(result)) => Ok(QueryOutcome::Executed {
                            affected_rows: result.rows_affected(),
                            last_insert_id: result.last_insert_id(),
                        }),
                        Ok(Err(e)) => Err(format!("Query failed: {}", e)),
                        Err(_) => Err(format!(
                            "Query timed out after {} seconds (MySQL server may be unavailable)",
                            DEFAULT_QUERY_TIMEOUT_SECS
                        )),
                    }
                }
            } else {
                Err("Invalid pool handle".to_string())
            }
        },
        |outcome: QueryOutcome| outcome.to_jsvalue().bits(),
    );

    promise
}

/// Enum to hold different parameter value types
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ParamValue {
    Null,
    String(String),
    Bytes(Vec<u8>),
    DateTime(chrono::NaiveDateTime),
    Number(f64),
    Int(i64),
    Bool(bool),
}

/// Extract parameter values from a JSValue array
pub(crate) unsafe fn extract_params_from_jsvalue(
    params: JSValue,
) -> Result<Vec<ParamValue>, String> {
    let mut result = Vec::new();

    let bits = params.bits();

    if bits == 0 || params.is_undefined() || params.is_null() {
        return Ok(result);
    }

    let is_array =
        JSValue::from_bits(perry_runtime::js_array_is_array(f64::from_bits(bits)).to_bits())
            .as_bool();
    if !is_array {
        return Err("Bind parameters must be an array".to_string());
    }

    // Handle both NaN-boxed pointers and raw pointers
    let arr_ptr: *const perry_runtime::ArrayHeader = if params.is_pointer() {
        // NaN-boxed pointer (POINTER_TAG = 0x7FFD)
        params.as_pointer() as *const perry_runtime::ArrayHeader
    } else if bits != 0 && bits <= 0x0000_FFFF_FFFF_FFFF {
        // Raw pointer (not NaN-boxed) - the bits ARE the pointer
        // Check upper bits don't match any NaN-box tag (0x7FFC-0x7FFF)
        let upper = bits >> 48;
        if upper == 0 || (upper > 0 && upper < 0x7FF0) {
            bits as *const perry_runtime::ArrayHeader
        } else {
            return Err("Bind parameters array has no valid runtime pointer".to_string());
        }
    } else {
        return Err("Bind parameters array has no valid runtime pointer".to_string());
    };

    if arr_ptr.is_null() {
        return Err("Bind parameters array has no valid runtime pointer".to_string());
    }

    let length = js_array_length(arr_ptr);

    for i in 0..length {
        let element_bits = js_array_get_jsvalue(arr_ptr, i);
        let element = JSValue::from_bits(element_bits);

        let param = if element.is_null() {
            ParamValue::Null
        } else if element.is_undefined() {
            return Err(format!("Bind parameter at index {i} is undefined"));
        } else if element.is_any_string() {
            // Extract string value
            if element.is_short_string() {
                let mut bytes = [0; perry_runtime::value::SHORT_STRING_MAX_LEN];
                let len = element.short_string_to_buf(&mut bytes);
                ParamValue::String(String::from_utf8_lossy(&bytes[..len]).to_string())
            } else {
                let str_ptr = element.as_string_ptr();
                if str_ptr.is_null() {
                    return Err(format!("Could not read string bind parameter at index {i}"));
                }
                let len = (*str_ptr).byte_len as usize;
                let data_ptr =
                    (str_ptr as *const u8).add(std::mem::size_of::<perry_runtime::StringHeader>());
                let bytes = std::slice::from_raw_parts(data_ptr, len);
                ParamValue::String(String::from_utf8_lossy(bytes).to_string())
            }
        } else if element.is_bigint() {
            // Convert BigInt to string (MySQL handles numeric strings correctly)
            let bigint_ptr = element.as_bigint_ptr();
            if !bigint_ptr.is_null() {
                let str_ptr = perry_runtime::bigint::js_bigint_to_string(bigint_ptr);
                if !str_ptr.is_null() {
                    let len = (*str_ptr).byte_len as usize;
                    let data_ptr = (str_ptr as *const u8)
                        .add(std::mem::size_of::<perry_runtime::StringHeader>());
                    let bytes = std::slice::from_raw_parts(data_ptr, len);
                    ParamValue::String(String::from_utf8_lossy(bytes).to_string())
                } else {
                    ParamValue::String("0".to_string())
                }
            } else {
                ParamValue::String("0".to_string())
            }
        } else if element.is_int32() {
            ParamValue::Int(element.as_int32() as i64)
        } else if element.is_bool() {
            ParamValue::Bool(element.as_bool())
        } else if element.is_number() {
            let n = element.to_number();
            // If the number is a whole number, send as Int for MySQL compatibility
            // (MySQL prepared statements require integers for LIMIT, OFFSET, etc.)
            if n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
                ParamValue::Int(n as i64)
            } else {
                ParamValue::Number(n)
            }
        } else {
            let mut byte_len = 0;
            let byte_ptr = perry_runtime::buffer::js_value_buffer_or_typedarray_data(
                f64::from_bits(element_bits),
                &mut byte_len,
            );
            if !byte_ptr.is_null() {
                let bytes = std::slice::from_raw_parts(byte_ptr, byte_len as usize);
                ParamValue::Bytes(bytes.to_vec())
            } else if perry_runtime::date::is_date_value(f64::from_bits(element_bits)) {
                let millis = perry_runtime::date::js_date_get_time(f64::from_bits(element_bits));
                if !millis.is_finite() {
                    return Err(format!("Bind parameter at index {i} is an invalid Date"));
                }
                let date = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(millis as i64)
                    .ok_or_else(|| {
                        format!("Bind parameter at index {i} is outside MySQL's Date range")
                    })?
                    .naive_utc();
                ParamValue::DateTime(date)
            } else {
                return Err(format!("Unsupported bind parameter at index {i}"));
            }
        };

        result.push(param);
    }

    Ok(result)
}

/// pool.getConnection() -> Promise<PoolConnection>
///
/// Gets a connection from the pool.
#[no_mangle]
pub unsafe extern "C" fn js_mysql2_pool_get_connection(pool_handle: Handle) -> *mut Promise {
    let promise = js_promise_new_cross_thread();

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        use crate::common::get_handle;
        use tokio::time::timeout;

        if let Some(wrapper) = get_handle::<MysqlPoolHandle>(pool_handle) {
            // Acquire a connection from the pool with timeout
            match timeout(
                Duration::from_secs(DEFAULT_ACQUIRE_TIMEOUT_SECS),
                wrapper.pool.acquire(),
            )
            .await
            {
                Ok(Ok(conn)) => {
                    // Register the connection handle
                    let handle = register_handle(MysqlPoolConnectionHandle::new(conn));
                    // NaN-box the handle with POINTER_TAG so it can be properly extracted later
                    // when conn.query() is called (codegen uses js_nanbox_get_pointer)
                    let nanboxed = perry_runtime::js_nanbox_pointer(handle as i64);
                    Ok(nanboxed.to_bits())
                }
                Ok(Err(e)) => Err(format!("Failed to get connection: {}", e)),
                Err(_) => Err(format!(
                    "Connection acquisition timed out after {} seconds",
                    DEFAULT_ACQUIRE_TIMEOUT_SECS
                )),
            }
        } else {
            Err("Invalid pool handle".to_string())
        }
    });

    promise
}

/// poolConnection.release()
///
/// Returns a connection to the pool.
/// In sqlx, connections are automatically returned when dropped,
/// so we just need to drop the handle.
#[no_mangle]
pub unsafe extern "C" fn js_mysql2_pool_connection_release(conn_handle: Handle) {
    // Enter the tokio runtime context before dropping the connection
    // sqlx requires a runtime context when dropping pool connections
    let _guard = crate::common::runtime().enter();

    // Take and drop the connection handle - this releases the connection back to the pool
    if let Some(_conn) = take_handle::<MysqlPoolConnectionHandle>(conn_handle) {
        // Connection is automatically returned to pool when dropped
    }
}

/// poolConnection.query(sql, params?) -> Promise<[rows, fields]>
///
/// Execute a query on the pool connection. See `js_mysql2_pool_query` for
/// rationale on accepting and binding `params` here. Issue #414.
#[no_mangle]
pub unsafe extern "C" fn js_mysql2_pool_connection_query(
    conn_handle: Handle,
    sql_ptr: *const u8,
    params_f: f64,
) -> *mut Promise {
    let promise = js_promise_new_cross_thread();
    let params = JSValue::from_bits(params_f.to_bits());

    // Extract the SQL string
    let sql = if sql_ptr.is_null() {
        String::new()
    } else {
        let header = sql_ptr as *const perry_runtime::StringHeader;
        let len = (*header).byte_len as usize;
        let data_ptr = sql_ptr.add(std::mem::size_of::<perry_runtime::StringHeader>());
        let bytes = std::slice::from_raw_parts(data_ptr, len);
        String::from_utf8_lossy(bytes).to_string()
    };

    let param_values = extract_params_from_jsvalue(params);
    let is_select = is_row_returning_query(&sql);

    crate::common::spawn_for_promise_deferred(
        promise as *mut u8,
        async move {
            use crate::common::get_handle_mut;
            use tokio::time::timeout;

            let param_values = param_values?;

            if let Some(wrapper) = get_handle_mut::<MysqlPoolConnectionHandle>(conn_handle) {
                if let Some(ref mut conn) = wrapper.connection {
                    let mut query = sqlx::query(sqlx::AssertSqlSafe(sql.clone()));
                    for param in &param_values {
                        query = match param {
                            ParamValue::Null => query.bind(Option::<String>::None),
                            ParamValue::String(s) => query.bind(s.clone()),
                            ParamValue::Bytes(bytes) => query.bind(bytes.clone()),
                            ParamValue::DateTime(date) => query.bind(*date),
                            ParamValue::Number(n) => query.bind(*n),
                            ParamValue::Int(i) => query.bind(*i),
                            ParamValue::Bool(b) => query.bind(*b),
                        };
                    }

                    if is_select {
                        let query_future = query.fetch_all(&mut **conn);
                        match timeout(
                            Duration::from_secs(DEFAULT_QUERY_TIMEOUT_SECS),
                            query_future,
                        )
                        .await
                        {
                            Ok(Ok(rows)) => {
                                let raw_result = RawQueryResult::from_mysql_rows(rows);
                                Ok(QueryOutcome::Rows(raw_result))
                            }
                            Ok(Err(e)) => Err(format!("Query failed: {}", e)),
                            Err(_) => Err(format!(
                                "Query timed out after {} seconds",
                                DEFAULT_QUERY_TIMEOUT_SECS
                            )),
                        }
                    } else {
                        let query_future = query.execute(&mut **conn);
                        match timeout(
                            Duration::from_secs(DEFAULT_QUERY_TIMEOUT_SECS),
                            query_future,
                        )
                        .await
                        {
                            Ok(Ok(result)) => Ok(QueryOutcome::Executed {
                                affected_rows: result.rows_affected(),
                                last_insert_id: result.last_insert_id(),
                            }),
                            Ok(Err(e)) => Err(format!("Query failed: {}", e)),
                            Err(_) => Err(format!(
                                "Query timed out after {} seconds",
                                DEFAULT_QUERY_TIMEOUT_SECS
                            )),
                        }
                    }
                } else {
                    Err("Connection has been released".to_string())
                }
            } else {
                Err("Invalid connection handle".to_string())
            }
        },
        |outcome: QueryOutcome| outcome.to_jsvalue().bits(),
    );

    promise
}

/// poolConnection.execute(sql, params) -> Promise<[rows, fields]>
///
/// Execute a prepared statement with parameters on the pool connection.
#[no_mangle]
pub unsafe extern "C" fn js_mysql2_pool_connection_execute(
    conn_handle: Handle,
    sql_ptr: *const u8,
    params_f: f64,
) -> *mut Promise {
    let promise = js_promise_new_cross_thread();
    let params = JSValue::from_bits(params_f.to_bits());

    // Extract the SQL string
    let sql = if sql_ptr.is_null() {
        String::new()
    } else {
        let header = sql_ptr as *const perry_runtime::StringHeader;
        let len = (*header).byte_len as usize;
        let data_ptr = sql_ptr.add(std::mem::size_of::<perry_runtime::StringHeader>());
        let bytes = std::slice::from_raw_parts(data_ptr, len);
        String::from_utf8_lossy(bytes).to_string()
    };

    // Extract parameters from the JSValue array
    let param_values = extract_params_from_jsvalue(params);
    let is_select = is_row_returning_query(&sql);

    crate::common::spawn_for_promise_deferred(
        promise as *mut u8,
        async move {
            use crate::common::get_handle_mut;
            use tokio::time::timeout;

            let param_values = param_values?;

            if let Some(wrapper) = get_handle_mut::<MysqlPoolConnectionHandle>(conn_handle) {
                if let Some(ref mut conn) = wrapper.connection {
                    // Build the query with parameter bindings
                    let mut query = sqlx::query(sqlx::AssertSqlSafe(sql.clone()));

                    for param in &param_values {
                        query = match param {
                            ParamValue::Null => query.bind(Option::<String>::None),
                            ParamValue::String(s) => query.bind(s.clone()),
                            ParamValue::Bytes(bytes) => query.bind(bytes.clone()),
                            ParamValue::DateTime(date) => query.bind(*date),
                            ParamValue::Number(n) => query.bind(*n),
                            ParamValue::Int(i) => query.bind(*i),
                            ParamValue::Bool(b) => query.bind(*b),
                        };
                    }

                    if is_select {
                        let query_future = query.fetch_all(&mut **conn);
                        match timeout(
                            Duration::from_secs(DEFAULT_QUERY_TIMEOUT_SECS),
                            query_future,
                        )
                        .await
                        {
                            Ok(Ok(rows)) => {
                                let raw_result = RawQueryResult::from_mysql_rows(rows);
                                Ok(QueryOutcome::Rows(raw_result))
                            }
                            Ok(Err(e)) => Err(format!("Query failed: {}", e)),
                            Err(_) => Err(format!(
                                "Query timed out after {} seconds",
                                DEFAULT_QUERY_TIMEOUT_SECS
                            )),
                        }
                    } else {
                        let query_future = query.execute(&mut **conn);
                        match timeout(
                            Duration::from_secs(DEFAULT_QUERY_TIMEOUT_SECS),
                            query_future,
                        )
                        .await
                        {
                            Ok(Ok(result)) => Ok(QueryOutcome::Executed {
                                affected_rows: result.rows_affected(),
                                last_insert_id: result.last_insert_id(),
                            }),
                            Ok(Err(e)) => Err(format!("Query failed: {}", e)),
                            Err(_) => Err(format!(
                                "Query timed out after {} seconds",
                                DEFAULT_QUERY_TIMEOUT_SECS
                            )),
                        }
                    }
                } else {
                    Err("Connection has been released".to_string())
                }
            } else {
                Err("Invalid connection handle".to_string())
            }
        },
        |outcome: QueryOutcome| outcome.to_jsvalue().bits(),
    );

    promise
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Issue #414: every shape the codegen dispatch table can pass to a
    /// mysql2 query/execute params slot must be safely consumable.
    ///
    /// The dispatcher emits `args: &[NA_STR, NA_F64]` for both `db.query`
    /// and `db.execute`, which in user code can be:
    ///   - missing (`db.query(sql)`) — codegen passes JS `undefined`
    ///   - undefined (`db.query(sql, undefined)`) — codegen passes TAG_UNDEFINED
    ///   - an array (`db.query(sql, [42])`) — codegen passes its NaN-boxed value
    ///   - a raw pointer (defensive compatibility with the old NA_PTR ABI)
    ///
    /// Pre-fix, only the no-params + execute-only paths were exercised; the
    /// query path silently dropped a non-zero params arg because the FFI
    /// signature only declared 2 args. Now query and execute share the
    /// extract-and-bind path; these tests pin the four shapes.
    #[test]
    fn extract_params_returns_empty_for_codegen_no_args_pad() {
        // Keep accepting the literal `0` used by the old NA_PTR ABI.
        let v = unsafe { extract_params_from_jsvalue(JSValue::from_bits(0)) }.unwrap();
        assert!(v.is_empty(), "raw 0 must yield no params");
    }

    #[test]
    fn extract_params_returns_empty_for_undefined_and_null() {
        let undef =
            unsafe { extract_params_from_jsvalue(JSValue::from_bits(0x7FFC_0000_0000_0001)) }
                .unwrap();
        assert!(undef.is_empty(), "TAG_UNDEFINED must yield no params");
        let null =
            unsafe { extract_params_from_jsvalue(JSValue::from_bits(0x7FFC_0000_0000_0002)) }
                .unwrap();
        assert!(null.is_empty(), "TAG_NULL must yield no params");
    }

    #[test]
    fn extract_params_handles_int_array_via_raw_pointer() {
        unsafe {
            let arr = perry_runtime::js_array_alloc(2);
            let arr = perry_runtime::js_array_push_f64(
                arr,
                f64::from_bits(0x7FFE_0000_0000_002A), // INT32 42
            );
            let _arr = perry_runtime::js_array_push_f64(
                arr,
                f64::from_bits(0x7FFE_0000_0000_0001), // INT32 1
            );

            // Codegen unboxes the NaN-boxed pointer to a raw i64. Mimic that
            // by passing the raw lower-48-bits pointer (no tag).
            let raw_ptr = arr as u64;
            let v = extract_params_from_jsvalue(JSValue::from_bits(raw_ptr)).unwrap();
            assert_eq!(v.len(), 2, "should extract two int params");
            match &v[0] {
                ParamValue::Int(n) => assert_eq!(*n, 42),
                other => panic!("expected Int(42), got {:?}", other),
            }
            match &v[1] {
                ParamValue::Int(n) => assert_eq!(*n, 1),
                other => panic!("expected Int(1), got {:?}", other),
            }
        }
    }

    #[test]
    fn extract_params_handles_int_array_via_nanboxed_pointer() {
        unsafe {
            let arr = perry_runtime::js_array_alloc(1);
            let _arr = perry_runtime::js_array_push_f64(
                arr,
                f64::from_bits(0x7FFE_0000_0000_007B), // INT32 123
            );

            // Defensive path: caller forgets to unbox before passing.
            let nan_boxed = (arr as u64) | 0x7FFD_0000_0000_0000;
            let v = extract_params_from_jsvalue(JSValue::from_bits(nan_boxed)).unwrap();
            assert_eq!(v.len(), 1);
            match &v[0] {
                ParamValue::Int(n) => assert_eq!(*n, 123),
                other => panic!("expected Int(123), got {:?}", other),
            }
        }
    }
}
