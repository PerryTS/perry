//! MySQL connection implementation.

use std::sync::Arc;
use std::time::Duration;

use perry_runtime::{js_promise_new_cross_thread, JSValue, Promise};
use sqlx::mysql::MySqlConnection;
use sqlx::Connection;
use tokio::sync::Mutex;

use super::pool::{
    execute_query_on_connection, parse_query_request, MysqlPoolConnectionHandle, MysqlPromiseError,
    QueryRequest, DEFAULT_QUERY_TIMEOUT_SECS,
};
use super::result::QueryOutcome;
use super::types::parse_mysql_config;
use crate::common::{register_handle, take_handle, with_handle, Handle};

const CONNECT_TIMEOUT_SECS: u64 = 10;

pub struct MysqlConnectionHandle {
    pub connection: Arc<Mutex<Option<MySqlConnection>>>,
}

impl MysqlConnectionHandle {
    pub fn new(conn: MySqlConnection) -> Self {
        Self {
            connection: Arc::new(Mutex::new(Some(conn))),
        }
    }
}

#[derive(Clone)]
pub(crate) enum MysqlConnectionTarget {
    Direct(Arc<Mutex<Option<MySqlConnection>>>),
    Pool(Arc<Mutex<Option<sqlx::pool::PoolConnection<sqlx::MySql>>>>),
}

pub(crate) fn connection_target(handle: Handle) -> Option<MysqlConnectionTarget> {
    with_handle::<MysqlConnectionHandle, _, _>(handle, |wrapper| {
        MysqlConnectionTarget::Direct(Arc::clone(&wrapper.connection))
    })
    .or_else(|| {
        with_handle::<MysqlPoolConnectionHandle, _, _>(handle, |wrapper| {
            MysqlConnectionTarget::Pool(Arc::clone(&wrapper.connection))
        })
    })
}

async fn execute_query_on_target(
    target: MysqlConnectionTarget,
    request: &QueryRequest,
) -> Result<QueryOutcome, MysqlPromiseError> {
    match target {
        MysqlConnectionTarget::Direct(connection) => {
            let mut slot = connection.lock().await;
            let connection = slot
                .as_mut()
                .ok_or_else(|| MysqlPromiseError::message("Connection already closed"))?;
            execute_query_on_connection(connection, request).await
        }
        MysqlConnectionTarget::Pool(connection) => {
            let mut slot = connection.lock().await;
            let connection = slot
                .as_mut()
                .ok_or_else(|| MysqlPromiseError::message("Pool connection released"))?;
            execute_query_on_connection(connection, request).await
        }
    }
}

unsafe fn run_connection_query(
    conn_handle: Handle,
    query_f: f64,
    params_f: f64,
    force_prepared: bool,
) -> *mut Promise {
    let promise = js_promise_new_cross_thread();
    let request = parse_query_request(query_f, params_f, force_prepared);
    let target = connection_target(conn_handle);
    let rows_as_array = request
        .as_ref()
        .map(|request| request.rows_as_array)
        .unwrap_or(false);

    crate::common::spawn_for_promise_deferred_with_error(
        promise as *mut u8,
        async move {
            let request = request?;
            let target =
                target.ok_or_else(|| MysqlPromiseError::message("Invalid connection handle"))?;
            execute_query_on_target(target, &request).await
        },
        move |outcome| outcome.to_jsvalue_with_rows_as_array(rows_as_array).bits(),
        MysqlPromiseError::to_jsvalue_bits,
    );
    promise
}

pub(crate) fn transaction_sql_for_method(method: &str) -> Option<&'static str> {
    match method {
        "beginTransaction" => Some("START TRANSACTION"),
        "commit" => Some("COMMIT"),
        "rollback" => Some("ROLLBACK"),
        _ => None,
    }
}

pub(crate) fn run_simple_command(conn_handle: Handle, sql: &'static str) -> *mut Promise {
    let promise = js_promise_new_cross_thread();
    let target = connection_target(conn_handle);
    unsafe {
        crate::common::spawn_for_promise_deferred_with_error(
            promise as *mut u8,
            async move {
                let target = target
                    .ok_or_else(|| MysqlPromiseError::message("Invalid connection handle"))?;
                let execute = async {
                    match target {
                        MysqlConnectionTarget::Direct(connection) => {
                            let mut slot = connection.lock().await;
                            let connection = slot.as_mut().ok_or_else(|| {
                                MysqlPromiseError::message("Connection already closed")
                            })?;
                            sqlx::raw_sql(sql)
                                .execute(connection)
                                .await
                                .map_err(|error| MysqlPromiseError::from_sqlx(sql, error))?;
                        }
                        MysqlConnectionTarget::Pool(connection) => {
                            let mut slot = connection.lock().await;
                            let connection = slot.as_mut().ok_or_else(|| {
                                MysqlPromiseError::message("Pool connection released")
                            })?;
                            sqlx::raw_sql(sql)
                                .execute(&mut **connection)
                                .await
                                .map_err(|error| MysqlPromiseError::from_sqlx(sql, error))?;
                        }
                    }
                    Ok::<_, MysqlPromiseError>(JSValue::undefined().bits())
                };
                tokio::time::timeout(Duration::from_secs(DEFAULT_QUERY_TIMEOUT_SECS), execute)
                    .await
                    .map_err(|_| MysqlPromiseError::message(format!("{sql} timed out")))?
            },
            |bits| bits,
            MysqlPromiseError::to_jsvalue_bits,
        );
    }
    promise
}

/// mysql.createConnection(config) -> Promise<Connection>.
#[no_mangle]
pub unsafe extern "C" fn js_mysql2_create_connection(config_f: f64) -> *mut Promise {
    let config = JSValue::from_bits(config_f.to_bits());
    let mysql_config = parse_mysql_config(config);
    let promise = js_promise_new_cross_thread();

    crate::common::spawn_for_promise_deferred_with_error(
        promise as *mut u8,
        async move {
            let connection = tokio::time::timeout(
                Duration::from_secs(CONNECT_TIMEOUT_SECS),
                MySqlConnection::connect(&mysql_config.to_url()),
            )
            .await
            .map_err(|_| MysqlPromiseError::message("MySQL connection timed out"))?
            .map_err(|error| MysqlPromiseError::from_sqlx("Failed to connect", error))?;
            Ok(connection)
        },
        |connection| {
            let handle = register_handle(MysqlConnectionHandle::new(connection));
            perry_runtime::js_nanbox_pointer(handle).to_bits()
        },
        MysqlPromiseError::to_jsvalue_bits,
    );
    promise
}

#[no_mangle]
pub unsafe extern "C" fn js_mysql2_connection_end(conn_handle: Handle) -> *mut Promise {
    let promise = js_promise_new_cross_thread();
    let connection = take_handle::<MysqlConnectionHandle>(conn_handle)
        .map(|wrapper| Arc::clone(&wrapper.connection));
    crate::common::spawn_for_promise_deferred_with_error(
        promise as *mut u8,
        async move {
            let connection = connection
                .ok_or_else(|| MysqlPromiseError::message("Invalid connection handle"))?;
            let connection = connection
                .lock()
                .await
                .take()
                .ok_or_else(|| MysqlPromiseError::message("Connection already closed"))?;
            tokio::time::timeout(
                Duration::from_secs(CONNECT_TIMEOUT_SECS),
                connection.close(),
            )
            .await
            .map_err(|_| MysqlPromiseError::message("Connection close timed out"))?
            .map_err(|error| MysqlPromiseError::from_sqlx("Failed to close", error))?;
            Ok(JSValue::undefined().bits())
        },
        |bits| bits,
        MysqlPromiseError::to_jsvalue_bits,
    );
    promise
}

#[no_mangle]
pub unsafe extern "C" fn js_mysql2_connection_query(
    conn_handle: Handle,
    query_f: f64,
    params_f: f64,
) -> *mut Promise {
    run_connection_query(conn_handle, query_f, params_f, false)
}

#[no_mangle]
pub unsafe extern "C" fn js_mysql2_connection_execute(
    conn_handle: Handle,
    query_f: f64,
    params_f: f64,
) -> *mut Promise {
    run_connection_query(conn_handle, query_f, params_f, true)
}

#[no_mangle]
pub unsafe extern "C" fn js_mysql2_connection_begin_transaction(
    conn_handle: Handle,
) -> *mut Promise {
    run_simple_command(conn_handle, "START TRANSACTION")
}

#[no_mangle]
pub unsafe extern "C" fn js_mysql2_connection_commit(conn_handle: Handle) -> *mut Promise {
    run_simple_command(conn_handle, "COMMIT")
}

#[no_mangle]
pub unsafe extern "C" fn js_mysql2_connection_rollback(conn_handle: Handle) -> *mut Promise {
    run_simple_command(conn_handle, "ROLLBACK")
}
