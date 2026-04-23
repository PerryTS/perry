//! PostgreSQL connection implementation

use perry_runtime::{js_array_get_jsvalue, js_array_length, js_promise_new, JSValue, Promise};
use sqlx::postgres::PgConnection;
use sqlx::{Connection, Row};

use crate::common::{register_handle, Handle};
use super::result::{rows_to_pg_result, empty_pg_result};
use super::types::{parse_pg_config, PgConfig};

/// Wrapper around PgConnection that we can store in the handle registry
pub struct PgConnectionHandle {
    pub connection: Option<PgConnection>,
}

impl PgConnectionHandle {
    pub fn new(conn: PgConnection) -> Self {
        Self {
            connection: Some(conn),
        }
    }

    pub fn take(&mut self) -> Option<PgConnection> {
        self.connection.take()
    }
}

/// pg.connect(config) -> Promise<Client>
///
/// Creates a new PostgreSQL connection with the given configuration.
/// Returns a Promise that resolves to a client handle.
///
/// # Safety
/// The config parameter must be a valid JSValue representing a config object.
#[no_mangle]
pub unsafe extern "C" fn js_pg_connect(config_f: f64) -> *mut Promise {
    // Take f64 at the FFI boundary to avoid SysV AMD64 ABI mismatch
    // (see js_mysql2_create_pool for details).
    let config = JSValue::from_bits(config_f.to_bits());
    let promise = js_promise_new();

    // Parse the config
    let pg_config = parse_pg_config(config);

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let url = pg_config.to_url();

        match PgConnection::connect(&url).await {
            Ok(conn) => {
                let handle = register_handle(PgConnectionHandle::new(conn));
                // Return the handle as bits
                Ok(handle as u64)
            }
            Err(e) => Err(format!("Failed to connect: {}", e)),
        }
    });

    promise
}

/// client.end() -> Promise<void>
///
/// Closes the PostgreSQL connection.
#[no_mangle]
pub unsafe extern "C" fn js_pg_client_end(client_handle: Handle) -> *mut Promise {
    let promise = js_promise_new();

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        use crate::common::take_handle;

        if let Some(mut wrapper) = take_handle::<PgConnectionHandle>(client_handle) {
            if let Some(conn) = wrapper.take() {
                match conn.close().await {
                    Ok(()) => Ok(JSValue::undefined().bits()),
                    Err(e) => Err(format!("Failed to close connection: {}", e)),
                }
            } else {
                Err("Connection already closed".to_string())
            }
        } else {
            Err("Invalid client handle".to_string())
        }
    });

    promise
}

/// client.query(sql) -> Promise<Result>
///
/// Executes a query and returns the results.
#[no_mangle]
pub unsafe extern "C" fn js_pg_client_query(
    client_handle: Handle,
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

    // Determine command type from SQL
    let command = sql.trim().split_whitespace().next()
        .unwrap_or("SELECT").to_uppercase();

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        use crate::common::get_handle_mut;

        if let Some(wrapper) = get_handle_mut::<PgConnectionHandle>(client_handle) {
            if let Some(conn) = wrapper.connection.as_mut() {
                match sqlx::query(&sql).fetch_all(conn).await {
                    Ok(rows) => {
                        // Get column info from first row (if any)
                        let columns: Vec<_> = if !rows.is_empty() {
                            rows[0].columns().to_vec()
                        } else {
                            Vec::new()
                        };

                        let result = rows_to_pg_result(rows, &columns, &command);
                        Ok(result.bits())
                    }
                    Err(e) => Err(format!("Query failed: {}", e)),
                }
            } else {
                Err("Connection already closed".to_string())
            }
        } else {
            Err("Invalid client handle".to_string())
        }
    });

    promise
}

/// Enum to hold different parameter value types for pg
#[derive(Clone, Debug)]
enum ParamValue {
    Null,
    String(String),
    Number(f64),
    Int(i64),
    Bool(bool),
}

/// Extract parameter values from a JSValue array
unsafe fn extract_params_from_jsvalue(params: JSValue) -> Vec<ParamValue> {
    let mut result = Vec::new();

    let bits = params.bits();

    let arr_ptr: *const perry_runtime::ArrayHeader = if params.is_pointer() {
        params.as_pointer() as *const perry_runtime::ArrayHeader
    } else if bits != 0 && bits <= 0x0000_FFFF_FFFF_FFFF {
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
            if n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
                ParamValue::Int(n as i64)
            } else {
                ParamValue::Number(n)
            }
        } else {
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

fn is_row_returning_query(sql: &str) -> bool {
    let trimmed = sql.trim_start();
    let upper = trimmed.get(..10).unwrap_or(trimmed).to_uppercase();
    upper.starts_with("SELECT")
        || upper.starts_with("SHOW")
        || upper.starts_with("DESC")
        || upper.starts_with("EXPLAIN")
        || upper.starts_with("WITH")
}

/// client.query(sql, params) -> Promise<Result>
///
/// Executes a parameterized query.
#[no_mangle]
pub unsafe extern "C" fn js_pg_client_query_params(
    client_handle: Handle,
    sql_ptr: *const u8,
    params: JSValue,
) -> *mut Promise {
    let promise = js_promise_new();

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
    let command = sql.trim().split_whitespace().next()
        .unwrap_or("SELECT").to_uppercase();
    let is_select = is_row_returning_query(&sql);

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        use crate::common::get_handle_mut;

        if let Some(wrapper) = get_handle_mut::<PgConnectionHandle>(client_handle) {
            if let Some(conn) = wrapper.connection.as_mut() {
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
                    match query.fetch_all(conn).await {
                        Ok(rows) => {
                            let columns: Vec<_> = if !rows.is_empty() {
                                rows[0].columns().to_vec()
                            } else {
                                Vec::new()
                            };
                            let result = rows_to_pg_result(rows, &columns, &command);
                            Ok(result.bits())
                        }
                        Err(e) => Err(format!("Query failed: {}", e)),
                    }
                } else {
                    match query.execute(conn).await {
                        Ok(result) => {
                            let pg_result = empty_pg_result(&command, result.rows_affected());
                            Ok(pg_result.bits())
                        }
                        Err(e) => Err(format!("Query failed: {}", e)),
                    }
                }
            } else {
                Err("Connection already closed".to_string())
            }
        } else {
            Err("Invalid client handle".to_string())
        }
    });

    promise
}
