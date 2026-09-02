//! MySQL connection pool implementation.

use std::sync::Arc;
use std::time::Duration;

use perry_runtime::{
    js_array_get_jsvalue, js_array_length, js_object_get_field_by_name,
    js_promise_new_cross_thread, js_string_from_bytes, JSValue, Promise,
};
use sqlx::mysql::{MySqlConnection, MySqlDatabaseError, MySqlPool, MySqlPoolOptions};
use sqlx::pool::PoolConnection;
use sqlx::MySql;
use tokio::sync::Mutex;

use super::result::{is_row_returning_query, QueryOutcome, RawQueryResult};
use super::types::parse_mysql_config;
use crate::common::{register_handle, take_handle, with_handle, Handle};

pub(crate) const DEFAULT_ACQUIRE_TIMEOUT_SECS: u64 = 10;
const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 10;
pub(crate) const DEFAULT_QUERY_TIMEOUT_SECS: u64 = 30;

pub struct MysqlPoolHandle {
    pub pool: MySqlPool,
}

impl MysqlPoolHandle {
    pub fn new(pool: MySqlPool) -> Self {
        Self { pool }
    }
}

/// A checked-out pool connection. The registry entry can be removed while an
/// operation is in flight, so the connection itself is shared and serialized.
pub struct MysqlPoolConnectionHandle {
    pub connection: Arc<Mutex<Option<PoolConnection<MySql>>>>,
}

impl MysqlPoolConnectionHandle {
    pub fn new(conn: PoolConnection<MySql>) -> Self {
        Self {
            connection: Arc::new(Mutex::new(Some(conn))),
        }
    }
}

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

/// Owned data for one mysql2 request. No pointer into the Perry heap crosses
/// the async boundary.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct QueryRequest {
    pub(crate) sql: String,
    pub(crate) params: Vec<ParamValue>,
    pub(crate) rows_as_array: bool,
    force_prepared: bool,
}

impl QueryRequest {
    fn is_row_returning(&self) -> bool {
        is_row_returning_query(&self.sql)
    }

    fn uses_prepared_statement(&self) -> bool {
        self.force_prepared || !self.params.is_empty()
    }
}

#[derive(Debug)]
pub(crate) struct MysqlPromiseError {
    message: String,
    code: Option<&'static str>,
    errno: Option<u16>,
}

impl MysqlPromiseError {
    pub(crate) fn message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: None,
            errno: None,
        }
    }

    pub(crate) fn from_sqlx(context: &str, error: sqlx::Error) -> Self {
        let errno = error
            .as_database_error()
            .and_then(|database| database.try_downcast_ref::<MySqlDatabaseError>())
            .map(MySqlDatabaseError::number);
        Self {
            message: format!("{context}: {error}"),
            code: errno.and_then(mysql2_error_code),
            errno,
        }
    }

    /// Build the rejection value on the main thread. mysql2 rejects with an
    /// Error object, not the bare string previously emitted by the fallback.
    pub(crate) fn to_jsvalue_bits(self) -> u64 {
        if let Some(errno) = self.errno {
            let code = self.code.unwrap_or("");
            return unsafe {
                perry_runtime::error::js_node_system_error_value(
                    self.message.as_ptr(),
                    self.message.len(),
                    code.as_ptr(),
                    code.len(),
                    std::ptr::null(),
                    0,
                    f64::from(errno),
                )
                .to_bits()
            };
        }

        let message = js_string_from_bytes(self.message.as_ptr(), self.message.len() as u32);
        let error = perry_runtime::error::js_error_new_with_message(message);
        JSValue::pointer(error as *const u8).bits()
    }
}

/// Symbolic names exposed by mysql2 for common server errors. Unknown server
/// errors still carry their numeric `.errno`.
fn mysql2_error_code(errno: u16) -> Option<&'static str> {
    Some(match errno {
        1022 => "ER_DUP_KEY",
        1045 => "ER_ACCESS_DENIED_ERROR",
        1048 => "ER_BAD_NULL_ERROR",
        1049 => "ER_BAD_DB_ERROR",
        1050 => "ER_TABLE_EXISTS_ERROR",
        1051 => "ER_BAD_TABLE_ERROR",
        1052 => "ER_NON_UNIQ_ERROR",
        1054 => "ER_BAD_FIELD_ERROR",
        1062 => "ER_DUP_ENTRY",
        1064 => "ER_PARSE_ERROR",
        1146 => "ER_NO_SUCH_TABLE",
        1169 => "ER_DUP_UNIQUE",
        1205 => "ER_LOCK_WAIT_TIMEOUT",
        1213 => "ER_LOCK_DEADLOCK",
        1216 => "ER_NO_REFERENCED_ROW",
        1217 => "ER_ROW_IS_REFERENCED",
        1264 => "ER_WARN_DATA_OUT_OF_RANGE",
        1292 => "ER_TRUNCATED_WRONG_VALUE",
        1364 => "ER_NO_DEFAULT_FOR_FIELD",
        1406 => "ER_DATA_TOO_LONG",
        1451 => "ER_ROW_IS_REFERENCED_2",
        1452 => "ER_NO_REFERENCED_ROW_2",
        1586 => "ER_DUP_ENTRY_WITH_KEY_NAME",
        1830 => "ER_FK_COLUMN_NOT_NULL",
        1834 => "ER_FK_CANNOT_DELETE_PARENT",
        1859 => "ER_DUP_UNKNOWN_IN_INDEX",
        3819 => "ER_CHECK_CONSTRAINT_VIOLATED",
        4025 => "ER_CONSTRAINT_FAILED",
        _ => return None,
    })
}

unsafe fn jsvalue_to_string(value: JSValue) -> Option<String> {
    let mut scratch = [0; perry_runtime::value::SHORT_STRING_MAX_LEN];
    let (ptr, len) =
        perry_runtime::string::str_bytes_from_jsvalue(f64::from_bits(value.bits()), &mut scratch)?;
    if ptr.is_null() {
        return Some(String::new());
    }
    let bytes = std::slice::from_raw_parts(ptr, len as usize);
    Some(String::from_utf8_lossy(bytes).into_owned())
}

unsafe fn object_pointer(value: JSValue) -> Option<*const perry_runtime::ObjectHeader> {
    if value.is_pointer() {
        let ptr = value.as_pointer::<perry_runtime::ObjectHeader>();
        return (!ptr.is_null()).then_some(ptr);
    }

    // Some generic call sites still pass an untagged object pointer.
    let bits = value.bits();
    if bits != 0 && bits <= 0x0000_7FFF_FFFF_FFFF {
        return Some(bits as *const perry_runtime::ObjectHeader);
    }
    None
}

unsafe fn object_field(value: JSValue, name: &str) -> JSValue {
    // Allocating the lookup key can trigger a moving collection. Root and
    // refresh the receiver before dereferencing it afterwards.
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_u64(value.bits());
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let Some(object) = object_pointer(JSValue::from_bits(receiver.get_nanbox_u64())) else {
        return JSValue::undefined();
    };
    js_object_get_field_by_name(object, key)
}

/// Parse mysql2's `query(sql, values?)` and `query({ sql, values?,
/// rowsAsArray? }, values?)` forms while all JS values are still rooted by the
/// native call.
pub(crate) unsafe fn parse_query_request(
    query_f: f64,
    params_f: f64,
    force_prepared: bool,
) -> Result<QueryRequest, MysqlPromiseError> {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let query = scope.root_nanbox_f64(query_f);
    let supplied_params = scope.root_nanbox_f64(params_f);

    let query_value = JSValue::from_bits(query.get_nanbox_u64());
    let (sql, rows_as_array, option_values) = if let Some(sql) = jsvalue_to_string(query_value) {
        (sql, false, JSValue::undefined())
    } else {
        let sql_value = object_field(JSValue::from_bits(query.get_nanbox_u64()), "sql");
        let sql = jsvalue_to_string(sql_value).ok_or_else(|| {
            MysqlPromiseError::message("Query must be a SQL string or an options object with sql")
        })?;
        let rows_as_array = object_field(JSValue::from_bits(query.get_nanbox_u64()), "rowsAsArray");
        let rows_as_array = rows_as_array.is_bool() && rows_as_array.as_bool();
        (
            sql,
            rows_as_array,
            object_field(JSValue::from_bits(query.get_nanbox_u64()), "values"),
        )
    };

    let supplied_params = JSValue::from_bits(supplied_params.get_nanbox_u64());
    let params = if supplied_params.is_undefined() {
        option_values
    } else {
        supplied_params
    };
    let params = extract_params_from_jsvalue(params).map_err(MysqlPromiseError::message)?;

    Ok(QueryRequest {
        sql,
        params,
        rows_as_array,
        force_prepared,
    })
}

pub(crate) async fn execute_query_on_connection(
    conn: &mut MySqlConnection,
    request: &QueryRequest,
) -> Result<QueryOutcome, MysqlPromiseError> {
    let is_select = request.is_row_returning();

    if !request.uses_prepared_statement() {
        // mysql2 `query()` uses MySQL's text protocol when there are no bind
        // values. This is required for commands such as BEGIN that the server
        // refuses through the prepared-statement protocol (#9517).
        let query = sqlx::raw_sql(sqlx::AssertSqlSafe(request.sql.clone()));
        if is_select {
            let rows = tokio::time::timeout(
                Duration::from_secs(DEFAULT_QUERY_TIMEOUT_SECS),
                query.fetch_all(&mut *conn),
            )
            .await
            .map_err(|_| MysqlPromiseError::message("Query timed out"))?
            .map_err(|error| MysqlPromiseError::from_sqlx("Query failed", error))?;
            return Ok(QueryOutcome::Rows(RawQueryResult::from_mysql_rows(rows)));
        }

        let result = tokio::time::timeout(
            Duration::from_secs(DEFAULT_QUERY_TIMEOUT_SECS),
            query.execute(&mut *conn),
        )
        .await
        .map_err(|_| MysqlPromiseError::message("Query timed out"))?
        .map_err(|error| MysqlPromiseError::from_sqlx("Query failed", error))?;
        return Ok(QueryOutcome::Executed {
            affected_rows: result.rows_affected(),
            last_insert_id: result.last_insert_id(),
        });
    }

    // Do not retain prepared statements between calls. This keeps each mysql2
    // request's SQL, bind metadata, and arguments together (#8745).
    let mut query = sqlx::query(sqlx::AssertSqlSafe(request.sql.clone())).persistent(false);
    for param in &request.params {
        query = match param {
            ParamValue::Null => query.bind(Option::<String>::None),
            ParamValue::String(value) => query.bind(value.clone()),
            ParamValue::Bytes(value) => query.bind(value.clone()),
            ParamValue::DateTime(value) => query.bind(*value),
            ParamValue::Number(value) => query.bind(*value),
            ParamValue::Int(value) => query.bind(*value),
            ParamValue::Bool(value) => query.bind(*value),
        };
    }

    if is_select {
        let rows = tokio::time::timeout(
            Duration::from_secs(DEFAULT_QUERY_TIMEOUT_SECS),
            query.fetch_all(&mut *conn),
        )
        .await
        .map_err(|_| MysqlPromiseError::message("Query timed out"))?
        .map_err(|error| MysqlPromiseError::from_sqlx("Query failed", error))?;
        Ok(QueryOutcome::Rows(RawQueryResult::from_mysql_rows(rows)))
    } else {
        let result = tokio::time::timeout(
            Duration::from_secs(DEFAULT_QUERY_TIMEOUT_SECS),
            query.execute(&mut *conn),
        )
        .await
        .map_err(|_| MysqlPromiseError::message("Query timed out"))?
        .map_err(|error| MysqlPromiseError::from_sqlx("Query failed", error))?;
        Ok(QueryOutcome::Executed {
            affected_rows: result.rows_affected(),
            last_insert_id: result.last_insert_id(),
        })
    }
}

/// Extract parameter values from a JS array before scheduling async work.
pub(crate) unsafe fn extract_params_from_jsvalue(
    params: JSValue,
) -> Result<Vec<ParamValue>, String> {
    if params.bits() == 0 || params.is_undefined() || params.is_null() {
        return Ok(Vec::new());
    }

    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let params_handle = scope.root_nanbox_u64(params.bits());
    let is_array = JSValue::from_bits(
        perry_runtime::js_array_is_array(params_handle.get_nanbox_f64()).to_bits(),
    )
    .as_bool();
    if !is_array {
        return Err("Bind parameters must be an array".to_string());
    }

    let refreshed_params = JSValue::from_bits(params_handle.get_nanbox_u64());
    let bits = refreshed_params.bits();
    let array: *const perry_runtime::ArrayHeader = if refreshed_params.is_pointer() {
        refreshed_params.as_pointer()
    } else if bits != 0 && bits <= 0x0000_FFFF_FFFF_FFFF {
        bits as *const perry_runtime::ArrayHeader
    } else {
        return Err("Bind parameters array has no valid runtime pointer".to_string());
    };
    if array.is_null() {
        return Err("Bind parameters array has no valid runtime pointer".to_string());
    }

    let length = js_array_length(array);
    let mut result = Vec::with_capacity(length as usize);
    for index in 0..length {
        let refreshed_params = JSValue::from_bits(params_handle.get_nanbox_u64());
        let array: *const perry_runtime::ArrayHeader = if refreshed_params.is_pointer() {
            refreshed_params.as_pointer()
        } else {
            refreshed_params.bits() as *const perry_runtime::ArrayHeader
        };
        let element_bits = js_array_get_jsvalue(array, index);
        let element = JSValue::from_bits(element_bits);
        let value = if element.is_null() {
            ParamValue::Null
        } else if element.is_undefined() {
            return Err(format!("Bind parameter at index {index} is undefined"));
        } else if let Some(value) = jsvalue_to_string(element) {
            ParamValue::String(value)
        } else if element.is_bigint() {
            let bigint = element.as_bigint_ptr();
            let string = perry_runtime::bigint::js_bigint_to_string(bigint);
            let value = crate::common::string_from_header_lossy(string)
                .ok_or_else(|| format!("Could not read bigint at index {index}"))?;
            ParamValue::String(value)
        } else if element.is_int32() {
            ParamValue::Int(i64::from(element.as_int32()))
        } else if element.is_bool() {
            ParamValue::Bool(element.as_bool())
        } else if element.is_number() {
            let number = element.to_number();
            if number.fract() == 0.0 && number >= i64::MIN as f64 && number <= i64::MAX as f64 {
                ParamValue::Int(number as i64)
            } else {
                ParamValue::Number(number)
            }
        } else {
            let mut byte_len = 0;
            let byte_ptr = perry_runtime::buffer::js_value_buffer_or_typedarray_data(
                f64::from_bits(element_bits),
                &mut byte_len,
            );
            if !byte_ptr.is_null() {
                ParamValue::Bytes(std::slice::from_raw_parts(byte_ptr, byte_len as usize).to_vec())
            } else if perry_runtime::date::is_date_value(f64::from_bits(element_bits)) {
                let millis = perry_runtime::date::js_date_get_time(f64::from_bits(element_bits));
                if !millis.is_finite() {
                    return Err(format!(
                        "Bind parameter at index {index} is an invalid Date"
                    ));
                }
                let date = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(millis as i64)
                    .ok_or_else(|| {
                        format!("Bind parameter at index {index} is outside MySQL's Date range")
                    })?
                    .naive_utc();
                ParamValue::DateTime(date)
            } else {
                return Err(format!("Unsupported bind parameter at index {index}"));
            }
        };
        result.push(value);
    }
    Ok(result)
}

unsafe fn run_pool_query(
    pool_handle: Handle,
    query_f: f64,
    params_f: f64,
    force_prepared: bool,
) -> *mut Promise {
    let promise = js_promise_new_cross_thread();
    let request = parse_query_request(query_f, params_f, force_prepared);
    let pool = with_handle::<MysqlPoolHandle, _, _>(pool_handle, |wrapper| wrapper.pool.clone());
    let rows_as_array = request
        .as_ref()
        .map(|request| request.rows_as_array)
        .unwrap_or(false);

    crate::common::spawn_for_promise_deferred_with_error(
        promise as *mut u8,
        async move {
            let request = request?;
            let pool = pool.ok_or_else(|| MysqlPromiseError::message("Invalid pool handle"))?;
            // Pin a single physical connection for the complete operation.
            let mut connection = tokio::time::timeout(
                Duration::from_secs(DEFAULT_ACQUIRE_TIMEOUT_SECS),
                pool.acquire(),
            )
            .await
            .map_err(|_| MysqlPromiseError::message("Pool acquire timed out"))?
            .map_err(|error| MysqlPromiseError::from_sqlx("Pool acquire failed", error))?;
            execute_query_on_connection(&mut connection, &request).await
        },
        move |outcome| outcome.to_jsvalue_with_rows_as_array(rows_as_array).bits(),
        MysqlPromiseError::to_jsvalue_bits,
    );
    promise
}

unsafe fn run_pool_connection_query(
    conn_handle: Handle,
    query_f: f64,
    params_f: f64,
    force_prepared: bool,
) -> *mut Promise {
    let promise = js_promise_new_cross_thread();
    let request = parse_query_request(query_f, params_f, force_prepared);
    let connection = with_handle::<MysqlPoolConnectionHandle, _, _>(conn_handle, |wrapper| {
        Arc::clone(&wrapper.connection)
    });
    let rows_as_array = request
        .as_ref()
        .map(|request| request.rows_as_array)
        .unwrap_or(false);

    crate::common::spawn_for_promise_deferred_with_error(
        promise as *mut u8,
        async move {
            let request = request?;
            let connection = connection
                .ok_or_else(|| MysqlPromiseError::message("Invalid pool connection handle"))?;
            let mut slot = connection.lock().await;
            let connection = slot
                .as_mut()
                .ok_or_else(|| MysqlPromiseError::message("Pool connection released"))?;
            execute_query_on_connection(connection, &request).await
        },
        move |outcome| outcome.to_jsvalue_with_rows_as_array(rows_as_array).bits(),
        MysqlPromiseError::to_jsvalue_bits,
    );
    promise
}

/// mysql.createPool(config) -> Pool. Like mysql2, construction is synchronous
/// and the first physical connection is opened lazily.
#[no_mangle]
pub unsafe extern "C" fn js_mysql2_create_pool(config_f: f64) -> Handle {
    let config = JSValue::from_bits(config_f.to_bits());
    let url = parse_mysql_config(config).to_url();
    let _runtime = crate::common::runtime().enter();
    MySqlPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(DEFAULT_ACQUIRE_TIMEOUT_SECS))
        .connect_lazy(&url)
        .map(MysqlPoolHandle::new)
        .map(register_handle)
        .unwrap_or(0)
}

#[no_mangle]
pub unsafe extern "C" fn js_mysql2_pool_end(pool_handle: Handle) -> *mut Promise {
    let promise = js_promise_new_cross_thread();
    let pool = take_handle::<MysqlPoolHandle>(pool_handle).map(|wrapper| wrapper.pool);
    crate::common::spawn_for_promise_deferred_with_error(
        promise as *mut u8,
        async move {
            let pool = pool.ok_or_else(|| MysqlPromiseError::message("Invalid pool handle"))?;
            let _ = tokio::time::timeout(
                Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS),
                pool.close(),
            )
            .await;
            Ok(JSValue::undefined().bits())
        },
        |bits| bits,
        MysqlPromiseError::to_jsvalue_bits,
    );
    promise
}

#[no_mangle]
pub unsafe extern "C" fn js_mysql2_pool_query(
    pool_handle: Handle,
    query_f: f64,
    params_f: f64,
) -> *mut Promise {
    run_pool_query(pool_handle, query_f, params_f, false)
}

#[no_mangle]
pub unsafe extern "C" fn js_mysql2_pool_execute(
    pool_handle: Handle,
    query_f: f64,
    params_f: f64,
) -> *mut Promise {
    run_pool_query(pool_handle, query_f, params_f, true)
}

#[no_mangle]
pub unsafe extern "C" fn js_mysql2_pool_get_connection(pool_handle: Handle) -> *mut Promise {
    let promise = js_promise_new_cross_thread();
    let pool = with_handle::<MysqlPoolHandle, _, _>(pool_handle, |wrapper| wrapper.pool.clone());
    crate::common::spawn_for_promise_deferred_with_error(
        promise as *mut u8,
        async move {
            let pool = pool.ok_or_else(|| MysqlPromiseError::message("Invalid pool handle"))?;
            let connection = tokio::time::timeout(
                Duration::from_secs(DEFAULT_ACQUIRE_TIMEOUT_SECS),
                pool.acquire(),
            )
            .await
            .map_err(|_| MysqlPromiseError::message("Pool acquire timed out"))?
            .map_err(|error| MysqlPromiseError::from_sqlx("Pool acquire failed", error))?;
            Ok(connection)
        },
        |connection| {
            let handle = register_handle(MysqlPoolConnectionHandle::new(connection));
            perry_runtime::js_nanbox_pointer(handle).to_bits()
        },
        MysqlPromiseError::to_jsvalue_bits,
    );
    promise
}

#[no_mangle]
pub unsafe extern "C" fn js_mysql2_pool_connection_release(conn_handle: Handle) {
    if let Some(wrapper) = take_handle::<MysqlPoolConnectionHandle>(conn_handle) {
        crate::common::spawn(async move {
            wrapper.connection.lock().await.take();
        });
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_mysql2_pool_connection_query(
    conn_handle: Handle,
    query_f: f64,
    params_f: f64,
) -> *mut Promise {
    run_pool_connection_query(conn_handle, query_f, params_f, false)
}

#[no_mangle]
pub unsafe extern "C" fn js_mysql2_pool_connection_execute(
    conn_handle: Handle,
    query_f: f64,
    params_f: f64,
) -> *mut Promise {
    run_pool_connection_query(conn_handle, query_f, params_f, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_protocol_is_used_only_for_query_without_values() {
        let query = QueryRequest {
            sql: "BEGIN".to_string(),
            params: Vec::new(),
            rows_as_array: false,
            force_prepared: false,
        };
        assert!(!query.uses_prepared_statement());

        let execute = QueryRequest {
            force_prepared: true,
            ..query.clone()
        };
        assert!(execute.uses_prepared_statement());

        let parameterized = QueryRequest {
            params: vec![ParamValue::Int(1)],
            ..query
        };
        assert!(parameterized.uses_prepared_statement());
    }

    #[test]
    fn mysql_error_code_names_match_mysql2() {
        assert_eq!(mysql2_error_code(1062), Some("ER_DUP_ENTRY"));
        assert_eq!(mysql2_error_code(1064), Some("ER_PARSE_ERROR"));
        assert_eq!(mysql2_error_code(65_000), None);
    }
}
