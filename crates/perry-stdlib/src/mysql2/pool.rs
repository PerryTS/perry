//! MySQL connection pool implementation

use std::time::Duration;

use perry_runtime::{js_array_get_jsvalue, js_array_length, js_promise_new, JSValue, Promise};
use sqlx::mysql::{MySqlPool, MySqlPoolOptions};
use sqlx::pool::PoolConnection;
use sqlx::MySql;

use crate::common::{register_handle, take_handle, Handle};
use super::result::{RawQueryResult, QueryOutcome, is_row_returning_query};
use super::types::parse_mysql_config;

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
        Ok(pool) => {
            register_handle(MysqlPoolHandle::new(pool))
        }
        Err(_e) => {
            0
        }
    }
}

/// pool.end() -> Promise<void>
///
/// Closes all connections in the pool.
#[no_mangle]
pub unsafe extern "C" fn js_mysql2_pool_end(pool_handle: Handle) -> *mut Promise {
    let promise = js_promise_new();

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        use crate::common::take_handle;
        use tokio::time::timeout;

        if let Some(wrapper) = take_handle::<MysqlPoolHandle>(pool_handle) {
            // Wrap pool close in a timeout (use shorter timeout since close should be fast)
            match timeout(Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS), wrapper.pool.close()).await {
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

/// pool.query(sql) -> Promise<[rows, fields]>
///
/// Executes a query using a connection from the pool.
#[no_mangle]
pub unsafe extern "C" fn js_mysql2_pool_query(
    pool_handle: Handle,
    sql_ptr: *const u8,
) -> *mut Promise {
    let promise = js_promise_new();

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

    let is_select = is_row_returning_query(&sql);

    // Use spawn_for_promise_deferred to safely create JSValues on the main thread
    // The async block returns raw Rust data, and the converter creates JSValues
    crate::common::spawn_for_promise_deferred(
        promise as *mut u8,
        async move {
            use crate::common::get_handle;
            use tokio::time::timeout;

            if let Some(wrapper) = get_handle::<MysqlPoolHandle>(pool_handle) {
                if is_select {
                    // SELECT/SHOW/DESCRIBE: fetch rows
                    let query_future = sqlx::query(&sql).fetch_all(&wrapper.pool);
                    match timeout(Duration::from_secs(DEFAULT_QUERY_TIMEOUT_SECS), query_future).await {
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
                    let query_future = sqlx::query(&sql).execute(&wrapper.pool);
                    match timeout(Duration::from_secs(DEFAULT_QUERY_TIMEOUT_SECS), query_future).await {
                        Ok(Ok(result)) => {
                            Ok(QueryOutcome::Executed {
                                affected_rows: result.rows_affected(),
                                last_insert_id: result.last_insert_id(),
                            })
                        }
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
        |outcome: QueryOutcome| {
            outcome.to_jsvalue().bits()
        },
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
    params: JSValue,
) -> *mut Promise {
    let promise = js_promise_new();

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

            if let Some(wrapper) = get_handle::<MysqlPoolHandle>(pool_handle) {
                // Build the query with parameter bindings
                let mut query = sqlx::query(&sql);

                for param in &param_values {
                    query = match param {
                        ParamValue::Null => query.bind(Option::<String>::None),
                        ParamValue::String(s) => query.bind(s.clone()),
                        ParamValue::Number(n) => query.bind(*n),
                        ParamValue::Int(i) => query.bind(*i),
                        ParamValue::Bool(b) => query.bind(*b),
                    };
                }

                if is_select {
                    let query_future = query.fetch_all(&wrapper.pool);
                    match timeout(Duration::from_secs(DEFAULT_QUERY_TIMEOUT_SECS), query_future).await {
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
                    match timeout(Duration::from_secs(DEFAULT_QUERY_TIMEOUT_SECS), query_future).await {
                        Ok(Ok(result)) => {
                            Ok(QueryOutcome::Executed {
                                affected_rows: result.rows_affected(),
                                last_insert_id: result.last_insert_id(),
                            })
                        }
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
        |outcome: QueryOutcome| {
            outcome.to_jsvalue().bits()
        },
    );

    promise
}

/// Enum to hold different parameter value types
#[derive(Clone, Debug)]
pub(crate) enum ParamValue {
    Null,
    String(String),
    Number(f64),
    Int(i64),
    Bool(bool),
}

/// Extract parameter values from a JSValue array
pub(crate) unsafe fn extract_params_from_jsvalue(params: JSValue) -> Vec<ParamValue> {
    let mut result = Vec::new();

    let bits = params.bits();

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
            return result;
        }
    } else {
        return result;
    };

    if arr_ptr.is_null() {
        return result;
    }

    let length = js_array_length(arr_ptr);

    for i in 0..length {
        let element_bits = js_array_get_jsvalue(arr_ptr, i);
        let element = JSValue::from_bits(element_bits);

        let param = if element.is_null() || element.is_undefined() {
            ParamValue::Null
        } else if element.is_string() {
            // Extract string value
            let str_ptr = element.as_string_ptr();
            if !str_ptr.is_null() {
                let len = (*str_ptr).byte_len as usize;
                let data_ptr = (str_ptr as *const u8).add(std::mem::size_of::<perry_runtime::StringHeader>());
                let bytes = std::slice::from_raw_parts(data_ptr, len);
                ParamValue::String(String::from_utf8_lossy(bytes).to_string())
            } else {
                ParamValue::Null
            }
        } else if element.is_bigint() {
            // Convert BigInt to string (MySQL handles numeric strings correctly)
            let bigint_ptr = element.as_bigint_ptr();
            if !bigint_ptr.is_null() {
                let str_ptr = perry_runtime::bigint::js_bigint_to_string(bigint_ptr);
                if !str_ptr.is_null() {
                    let len = (*str_ptr).byte_len as usize;
                    let data_ptr = (str_ptr as *const u8).add(std::mem::size_of::<perry_runtime::StringHeader>());
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
            // Unknown type - try to treat as number
            let n = element.to_number();
            if n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
                ParamValue::Int(n as i64)
            } else {
                ParamValue::Number(n)
            }
        };

        result.push(param);
    }

    result
}

/// pool.getConnection() -> Promise<PoolConnection>
///
/// Gets a connection from the pool.
#[no_mangle]
pub unsafe extern "C" fn js_mysql2_pool_get_connection(pool_handle: Handle) -> *mut Promise {
    let promise = js_promise_new();

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        use crate::common::get_handle;
        use tokio::time::timeout;

        if let Some(wrapper) = get_handle::<MysqlPoolHandle>(pool_handle) {
            // Acquire a connection from the pool with timeout
            match timeout(Duration::from_secs(DEFAULT_ACQUIRE_TIMEOUT_SECS), wrapper.pool.acquire()).await {
                Ok(Ok(conn)) => {
                    // Register the connection handle
                    let handle = register_handle(MysqlPoolConnectionHandle::new(conn));
                    // NaN-box the handle with POINTER_TAG so it can be properly extracted later
                    // when conn.query() is called (codegen uses js_nanbox_get_pointer)
                    let nanboxed = perry_runtime::js_nanbox_pointer(handle as i64);
                    Ok(nanboxed.to_bits())
                }
                Ok(Err(e)) => {
                    Err(format!("Failed to get connection: {}", e))
                }
                Err(_) => {
                    Err(format!(
                        "Connection acquisition timed out after {} seconds",
                        DEFAULT_ACQUIRE_TIMEOUT_SECS
                    ))
                }
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
    } else {
    }
}

/// poolConnection.query(sql) -> Promise<[rows, fields]>
///
/// Execute a query on the pool connection.
#[no_mangle]
pub unsafe extern "C" fn js_mysql2_pool_connection_query(
    conn_handle: Handle,
    sql_ptr: *const u8,
) -> *mut Promise {
    let promise = js_promise_new();

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


    let is_select = is_row_returning_query(&sql);

    crate::common::spawn_for_promise_deferred(
        promise as *mut u8,
        async move {
            use crate::common::get_handle_mut;
            use tokio::time::timeout;

            if let Some(wrapper) = get_handle_mut::<MysqlPoolConnectionHandle>(conn_handle) {
                if let Some(ref mut conn) = wrapper.connection {
                    if is_select {
                        let query_future = sqlx::query(&sql).fetch_all(&mut **conn);
                        match timeout(Duration::from_secs(DEFAULT_QUERY_TIMEOUT_SECS), query_future).await {
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
                        let query_future = sqlx::query(&sql).execute(&mut **conn);
                        match timeout(Duration::from_secs(DEFAULT_QUERY_TIMEOUT_SECS), query_future).await {
                            Ok(Ok(result)) => {
                                Ok(QueryOutcome::Executed {
                                    affected_rows: result.rows_affected(),
                                    last_insert_id: result.last_insert_id(),
                                })
                            }
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
        |outcome: QueryOutcome| {
            outcome.to_jsvalue().bits()
        },
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
    params: JSValue,
) -> *mut Promise {
    let promise = js_promise_new();

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

            if let Some(wrapper) = get_handle_mut::<MysqlPoolConnectionHandle>(conn_handle) {
                if let Some(ref mut conn) = wrapper.connection {
                    // Build the query with parameter bindings
                    let mut query = sqlx::query(&sql);

                    for param in &param_values {
                        query = match param {
                            ParamValue::Null => query.bind(Option::<String>::None),
                            ParamValue::String(s) => query.bind(s.clone()),
                            ParamValue::Number(n) => query.bind(*n),
                            ParamValue::Int(i) => query.bind(*i),
                            ParamValue::Bool(b) => query.bind(*b),
                        };
                    }

                    if is_select {
                        let query_future = query.fetch_all(&mut **conn);
                        match timeout(Duration::from_secs(DEFAULT_QUERY_TIMEOUT_SECS), query_future).await {
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
                        match timeout(Duration::from_secs(DEFAULT_QUERY_TIMEOUT_SECS), query_future).await {
                            Ok(Ok(result)) => {
                                Ok(QueryOutcome::Executed {
                                    affected_rows: result.rows_affected(),
                                    last_insert_id: result.last_insert_id(),
                                })
                            }
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
        |outcome: QueryOutcome| {
            outcome.to_jsvalue().bits()
        },
    );

    promise
}
