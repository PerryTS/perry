//! `mysql2` on a turnloop socket (P7).
//!
//! What this replaces, one for one:
//!
//! | before | after |
//! |---|---|
//! | `spawn_blocking` + `Handle::current().block_on` per call — one tokio blocking-pool thread held for the whole round trip | one command on a sans-I/O core, submitted where the FFI call happens |
//! | `sqlx::MySqlConnection`, whose own tokio task owns the socket | `turnloop_mysql::Connection` driven over P1's `turnloop_net` |
//! | `tokio::time::timeout` per call, which needs a tokio timer | the command's own deadline, armed as a real turnloop deadline |
//! | `sqlx::MySqlPool` with `max_connections(10)` | a FIFO pool of up to ten loop-driven connections ([`pool`]) |
//!
//! The JS-visible surface does not move: the same seventeen `js_mysql2_*`
//! symbols, the same result shapes, the same rejection messages, the same
//! 10-second connect / 30-second query / 10-second acquire deadlines.
//!
//! # Which connections come here
//!
//! [`enabled`] is false on a `worker_threads` agent (no loop of its own) and in
//! the `tokio-wait-driver` A/B arm; those keep the sqlx transport, which is
//! left intact. The transport is decided **once**, at `createConnection` /
//! `createPool`, and never changes — P1's rule for sockets, for the same
//! reason: a client that switched mid-life would have two connections to the
//! same server and no way to keep a transaction on one of them.
//!
//! **MySQL TLS is not supported on either transport**, and this change does not
//! move that: `MySqlConfig::to_url` hardcodes `?ssl-mode=disabled`, so every
//! MySQL connection Perry has opened has been plaintext. The core is given
//! `tls: false` to match, and would otherwise ask this host for an upgrade it
//! has no TLS layer to perform.
//!
//! # Threading and the GC
//!
//! The sink runs on the agent thread from the loop's own completion dispatch,
//! so it may touch the connection table directly. It builds **no JS value**: a
//! result is settled through `JsPromise::resolve_with`, whose closure carries
//! owned Rust data and runs on the main thread during the resolution pump.
//! That is the same #1824 rule the `spawn_blocking` path obeyed, now with no
//! worker thread involved at all. Rows are copied out of the receive buffer
//! before any further call on the core, because `turnloop_mysql`'s `Row` and
//! `Column` borrow it.

mod connection;
mod convert;
pub(crate) mod pool;

#[cfg(test)]
mod tests;

use perry_db_turnloop::{subsystem, NetCompletion, Registry};
use perry_ffi::{register_handle, take_handle, with_handle, Handle, JsPromise, Promise};

use crate::{MySqlConfig, MysqlPromiseError, QueryRequest};
pub(crate) use connection::MysqlCore;
use connection::{Answer, Command, Request};

/// This binding's slot in the runtime's sink registry.
pub(crate) const SUBSYSTEM: u8 = subsystem::MYSQL;

thread_local! {
    /// The connection table. Thread-local because a turnloop handle belongs to
    /// the loop that created it — see `perry_db_turnloop`'s module docs.
    static REGISTRY: Registry<MysqlCore> = Registry::new(SUBSYSTEM);
}

extern "C" fn sink(completion: *const NetCompletion) {
    if completion.is_null() {
        return;
    }
    // SAFETY: the runtime borrows one completion for the duration of this call.
    let completion = unsafe { &*completion };
    REGISTRY.with(|reg| reg.dispatch(completion));
    // A completion is the only moment a pooled connection can have become free,
    // died, or run past its acquire deadline. Run after `dispatch` so the
    // registry's own borrow is released first.
    pool::pump();
}

/// Whether a client created *now, on this thread* should live on turnloop.
pub(crate) fn enabled() -> bool {
    REGISTRY.with(|reg| reg.enabled(sink))
}

/// Install the sink and report whether the runtime accepted it.
///
/// Separate from [`enabled`] so a test can assert the part that is a property
/// of the build — the completion-layout digest check — without also asserting
/// that the thread it happens to run on owns a loop. `cargo test` puts each
/// test on its own thread and only some of them do, which made this assertion
/// flaky when it went through `enabled`.
#[cfg(test)]
pub(crate) fn register_only() -> bool {
    REGISTRY.with(|reg| reg.register(sink))
}

// ── Registry access ───────────────────────────────────────────────

fn with_core<R>(id: i64, f: impl FnOnce(&mut MysqlCore) -> R) -> Option<R> {
    REGISTRY.with(|reg| reg.with_core(id, f))
}

fn inspect<R>(id: i64, f: impl FnOnce(&MysqlCore) -> R) -> Option<R> {
    REGISTRY.with(|reg| reg.inspect(id, f))
}

fn is_live(id: i64) -> bool {
    REGISTRY.with(|reg| reg.is_live(id))
}

fn abort(id: i64, reason: &str) {
    REGISTRY.with(|reg| reg.abort(id, reason));
}

/// Open one connection. `tag` is the JS handle it belongs to, which the driver
/// keeps only so a debugger can tell the two apart.
fn open(config: &MySqlConfig, tag: u64) -> Result<i64, String> {
    let core = MysqlCore::new(config)?;
    REGISTRY.with(|reg| reg.connect(&config.host, config.port, core, tag))
}

/// Hand one command to a live connection.
///
/// The command travels through an `Option` so a `with_core` that never runs its
/// closure — the entry went away between the handle lookup and here — gives it
/// back instead of dropping it. A dropped `JsPromise` never settles, which is
/// the one outcome a caller cannot recover from; every caller of this must do
/// something with what comes back.
#[must_use = "an undelivered command still owns its promise"]
fn submit(id: i64, command: Command) -> Option<Command> {
    let mut slot = Some(command);
    let delivered = with_core(id, |core| {
        core.enqueue(slot.take().expect("the closure runs at most once"));
    });
    if delivered.is_some() {
        None
    } else {
        slot
    }
}

/// Queue one request on `id`, handing the promise back if it cannot be
/// delivered.
#[must_use = "an undelivered request still owns its promise"]
fn submit_request(
    id: i64,
    request: QueryRequest,
    promise: JsPromise,
    context: &'static str,
    answer: Answer,
) -> Option<JsPromise> {
    let command = Command::Request(Box::new(Request {
        request,
        promise,
        deadline: connection::query_deadline(),
        context,
        answer,
    }));
    match submit(id, command) {
        Some(Command::Request(request)) => Some(request.promise),
        // `submit` gives back exactly what it was handed.
        Some(_) | None => None,
    }
}

/// Queue a `COM_QUIT`. `promise`, when present, is `connection.end()`'s.
fn quit(id: i64, promise: Option<JsPromise>) {
    if let Some(Command::Quit(Some(promise))) = submit(id, Command::Quit(promise)) {
        // The connection was already gone, which is what `end()` wanted.
        promise.resolve_undefined();
    }
}

// ── Connection ────────────────────────────────────────────────────

/// `mysql.createConnection(config)` on this transport.
///
/// The JS handle is registered **before** the socket is opened so the handshake
/// has something to resolve with, and the promise is parked on the core until
/// `Connected` fires. That reproduces the eager `createConnection` the sqlx
/// path had: the promise settles when the server has accepted the credentials,
/// not when the TCP connect completes.
pub(crate) fn create_connection(config: MySqlConfig) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    // The socket is opened first so the handle can be registered already
    // carrying its driver id: a handle that is briefly registered without one
    // would answer `connection_id` with `None` and route a call that arrived in
    // that window to the sqlx path, on a connection that does not exist there.
    // The driver tag is 0 because the JS handle does not exist yet and this
    // binding never reads a tag back.
    let id = match open(&config, 0) {
        Ok(id) => id,
        Err(message) => {
            MysqlPromiseError::message(format!("Failed to connect: {message}")).reject(promise);
            return raw;
        }
    };
    let handle = register_handle(crate::MysqlConnectionHandle::on_turnloop(id));
    let mut slot = Some(promise);
    let parked = with_core(id, |core| {
        core.park_ready(slot.take().expect("the closure runs at most once"), handle)
    });
    if parked.is_none() {
        take_handle::<crate::MysqlConnectionHandle>(handle);
        if let Some(promise) = slot {
            MysqlPromiseError::message("Failed to connect: connection closed").reject(promise);
        }
    }
    raw
}

/// The driver id behind a connection handle, and what a closed one is called.
///
/// Both connection families answer here, which is what lets the per-handle
/// entry points branch on transport without the dispatch tables in `lib.rs`
/// learning about turnloop at all.
fn connection_id(handle: Handle) -> Option<(i64, &'static str)> {
    if let Some(Some(id)) =
        with_handle::<crate::MysqlConnectionHandle, _, _>(handle, |wrapper| wrapper.turnloop)
    {
        return Some((id, "Connection already closed"));
    }
    if let Some(Some((_, id))) =
        with_handle::<crate::MysqlPoolConnectionHandle, _, _>(handle, |wrapper| wrapper.turnloop)
    {
        return Some((id, "Pool connection released"));
    }
    None
}

/// Whether `handle` names a connection on this transport.
///
/// Asked before the request is built so a handle that belongs to the sqlx path
/// keeps it — [`connection_request`] consumes the request, and handing it to
/// the wrong transport would have to give it back.
pub(crate) fn owns_connection(handle: Handle) -> bool {
    connection_id(handle).is_some()
}

/// `connection.query()` / `.execute()`, and the same on a pooled connection.
///
/// Both go to the one connection the handle names, which is what keeps a
/// transaction's statements together.
pub(crate) fn connection_request(handle: Handle, request: QueryRequest) -> *mut Promise {
    let Some((id, closed)) = connection_id(handle) else {
        return rejected("Invalid connection handle");
    };
    start(id, request, "Query failed", Answer::ResultTuple, closed)
}

/// A promise that is already rejected, for a handle that went away between the
/// caller's check and here.
fn rejected(message: &str) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    MysqlPromiseError::message(message).reject(promise);
    raw
}

/// `beginTransaction()` / `commit()` / `rollback()`.
///
/// Plain SQL on the connection the handle names. A `Connection` owns one
/// loop-driven connection for life and a pooled one is pinned until
/// `release()`, so the three statements of a transaction cannot land on
/// different connections.
pub(crate) fn simple_command(handle: Handle, sql: &'static str) -> *mut Promise {
    let Some((id, closed)) = connection_id(handle) else {
        return rejected("Invalid connection handle");
    };
    let request = QueryRequest::new(sql.to_string(), Vec::new(), false, false);
    start(id, request, sql, Answer::Undefined, closed)
}

fn start(
    id: i64,
    request: QueryRequest,
    context: &'static str,
    answer: Answer,
    closed: &'static str,
) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    if let Some(promise) = submit_request(id, request, promise, context, answer) {
        MysqlPromiseError::message(closed).reject(promise);
    }
    raw
}

/// `connection.end()`.
pub(crate) fn connection_end(handle: Handle) -> Option<*mut Promise> {
    let wrapper =
        with_handle::<crate::MysqlConnectionHandle, _, _>(handle, |wrapper| wrapper.turnloop)?;
    let id = wrapper?;
    take_handle::<crate::MysqlConnectionHandle>(handle);
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    // Queued rather than closed outright: `COM_QUIT` runs behind whatever the
    // connection is still carrying, so an in-flight query answers its caller
    // before the socket goes away.
    quit(id, Some(promise));
    Some(raw)
}

/// `connection.release()` on a pooled connection.
pub(crate) fn pool_connection_release(handle: Handle) -> bool {
    let Some(Some((pool_handle, id))) =
        with_handle::<crate::MysqlPoolConnectionHandle, _, _>(handle, |wrapper| wrapper.turnloop)
    else {
        return false;
    };
    take_handle::<crate::MysqlPoolConnectionHandle>(handle);
    pool::release(pool_handle, id);
    true
}
