//! mysql2 compatible native implementation
//!
//! Provides a drop-in replacement for the mysql2 npm package using sqlx.

pub mod connection;
pub mod pool;
pub mod result;
pub mod types;

pub use connection::*;
pub use pool::*;
pub use result::*;
pub use types::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum MysqlHandleKind {
    Pool,
    PoolConnection,
    Connection,
}

fn mysql_handle_kind(handle: crate::common::Handle) -> Option<MysqlHandleKind> {
    crate::common::with_handle::<pool::MysqlPoolHandle, _, _>(handle, |_| MysqlHandleKind::Pool)
        .or_else(|| {
            crate::common::with_handle::<pool::MysqlPoolConnectionHandle, _, _>(handle, |_| {
                MysqlHandleKind::PoolConnection
            })
        })
        .or_else(|| {
            crate::common::with_handle::<connection::MysqlConnectionHandle, _, _>(handle, |_| {
                MysqlHandleKind::Connection
            })
        })
}

fn method_is_available(kind: MysqlHandleKind, method: &str) -> bool {
    match kind {
        MysqlHandleKind::Pool => {
            matches!(
                method,
                "query" | "execute" | "end" | "getConnection" | "promise"
            )
        }
        MysqlHandleKind::PoolConnection => matches!(
            method,
            "query" | "execute" | "release" | "beginTransaction" | "commit" | "rollback"
        ),
        MysqlHandleKind::Connection => matches!(
            method,
            "query" | "execute" | "end" | "beginTransaction" | "commit" | "rollback" | "promise"
        ),
    }
}

/// Runtime method dispatch for mysql2 handles whose static TypeScript class
/// was erased (notably Drizzle's interface-typed client fields).
pub(crate) unsafe fn dispatch_mysql2_method(
    handle: crate::common::Handle,
    method: &str,
    args: &[f64],
) -> Option<f64> {
    let kind = mysql_handle_kind(handle)?;
    if !method_is_available(kind, method) {
        return None;
    }
    let undefined = crate::common::TAG_UNDEFINED_F64;
    let arg = |index: usize| args.get(index).copied().unwrap_or(undefined);
    let pointer = |ptr: *mut perry_runtime::Promise| {
        f64::from_bits(perry_runtime::JSValue::pointer(ptr as *const u8).bits())
    };

    Some(match (kind, method) {
        (MysqlHandleKind::Pool, "query") => {
            pointer(pool::js_mysql2_pool_query(handle, arg(0), arg(1)))
        }
        (MysqlHandleKind::Pool, "execute") => {
            pointer(pool::js_mysql2_pool_execute(handle, arg(0), arg(1)))
        }
        (MysqlHandleKind::Pool, "getConnection") => {
            pointer(pool::js_mysql2_pool_get_connection(handle))
        }
        (MysqlHandleKind::Pool, "end") => pointer(pool::js_mysql2_pool_end(handle)),
        (MysqlHandleKind::PoolConnection, "query") => pointer(
            pool::js_mysql2_pool_connection_query(handle, arg(0), arg(1)),
        ),
        (MysqlHandleKind::PoolConnection, "execute") => pointer(
            pool::js_mysql2_pool_connection_execute(handle, arg(0), arg(1)),
        ),
        (MysqlHandleKind::PoolConnection, "release") => {
            pool::js_mysql2_pool_connection_release(handle);
            undefined
        }
        (MysqlHandleKind::Connection, "query") => pointer(connection::js_mysql2_connection_query(
            handle,
            arg(0),
            arg(1),
        )),
        (MysqlHandleKind::Connection, "execute") => pointer(
            connection::js_mysql2_connection_execute(handle, arg(0), arg(1)),
        ),
        (MysqlHandleKind::Connection, "end") => {
            pointer(connection::js_mysql2_connection_end(handle))
        }
        (MysqlHandleKind::PoolConnection | MysqlHandleKind::Connection, method)
            if connection::transaction_sql_for_method(method).is_some() =>
        {
            let sql = connection::transaction_sql_for_method(method)?;
            pointer(connection::run_simple_command(handle, sql))
        }
        (MysqlHandleKind::Pool | MysqlHandleKind::Connection, "promise") => {
            crate::common::nanbox_handle_value(handle)
        }
        _ => return None,
    })
}

/// Property reads for mysql2 methods return a bound method. This makes
/// `Reflect.has(pool, "getConnection")`, `"getConnection" in pool`, and
/// `typeof pool.getConnection` agree with the real mysql2 objects.
pub(crate) unsafe fn dispatch_mysql2_property(
    handle: crate::common::Handle,
    property: &str,
) -> Option<f64> {
    let kind = mysql_handle_kind(handle)?;
    if !method_is_available(kind, property) {
        return None;
    }
    Some(perry_runtime::object::js_class_method_bind(
        crate::common::nanbox_handle_value(handle),
        property.as_ptr(),
        property.len(),
    ))
}
