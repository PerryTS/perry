use super::*;
use crate::common::{get_handle, Handle};
use rusqlite::{limits::Limit, Connection, OpenFlags};
use std::sync::atomic::Ordering;
use std::time::Duration;

pub(crate) fn open_node_sqlite_connection(db: &BunSqliteDbHandle) -> rusqlite::Result<Connection> {
    let mut flags = OpenFlags::SQLITE_OPEN_URI | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    if db.read_only {
        flags |= OpenFlags::SQLITE_OPEN_READ_ONLY;
    }
    if db.read_write {
        flags |= OpenFlags::SQLITE_OPEN_READ_WRITE;
    }
    if db.create {
        flags |= OpenFlags::SQLITE_OPEN_CREATE;
    }

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
                conn.set_limit(*limit, *value)?;
            }
        }
    }

    Ok(conn)
}

pub(crate) unsafe fn with_sqlite_connection<R, F>(db_handle: Handle, f: F) -> Option<R>
where
    F: FnOnce(&Connection) -> R,
{
    if let Some(db) = get_handle::<SqliteDbHandle>(db_handle) {
        if let Ok(conn) = db.conn.lock() {
            return Some(f(&conn));
        }
    }
    if let Some(db) = get_handle::<BunSqliteDbHandle>(db_handle) {
        if let Ok(conn) = db.conn.lock() {
            if let Some(conn) = conn.as_ref() {
                return Some(f(conn));
            }
        }
    }
    None
}

pub(crate) unsafe fn with_open_bun_connection<R, F>(db_handle: Handle, f: F) -> R
where
    F: FnOnce(&Connection) -> R,
{
    let db = get_handle::<BunSqliteDbHandle>(db_handle)
        .unwrap_or_else(|| throw_invalid_state("database is not open"));
    let conn_ptr = {
        let conn = db
            .conn
            .lock()
            .unwrap_or_else(|_| throw_invalid_state("database is not open"));
        if let Some(conn) = conn.as_ref() {
            conn as *const Connection
        } else {
            drop(conn);
            throw_invalid_state("database is not open")
        }
    };
    f(&*conn_ptr)
}

pub(crate) unsafe fn ensure_open_bun_database(db_handle: Handle) {
    let db = get_handle::<BunSqliteDbHandle>(db_handle)
        .unwrap_or_else(|| throw_invalid_state("database is not open"));
    let conn = db
        .conn
        .lock()
        .unwrap_or_else(|_| throw_invalid_state("database is not open"));
    if conn.is_none() {
        drop(conn);
        throw_invalid_state("database is not open");
    }
}

pub(crate) unsafe fn finalize_bun_sqlite_statements(db: &BunSqliteDbHandle) {
    let handles: Vec<Handle> = db
        .statements
        .lock()
        .map(|mut statements| statements.drain().collect())
        .unwrap_or_default();

    for handle in handles {
        if let Some(stmt) = get_handle::<BunSqliteStmtHandle>(handle) {
            stmt.finalized.store(true, Ordering::Relaxed);
        }
    }
}

pub(crate) unsafe fn finalize_bun_sqlite_statement_handle(stmt_handle: Handle) {
    let Some(stmt) = get_handle::<BunSqliteStmtHandle>(stmt_handle) else {
        return;
    };
    stmt.finalized.store(true, Ordering::Relaxed);
    if let Some(db) = get_handle::<BunSqliteDbHandle>(stmt.db_handle) {
        if let Ok(mut statements) = db.statements.lock() {
            statements.remove(&stmt_handle);
        }
    }
}
