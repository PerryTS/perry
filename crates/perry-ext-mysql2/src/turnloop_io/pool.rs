//! `mysql.createPool()` on loop-driven connections.
//!
//! # What this is, exactly
//!
//! A **bounded FIFO pool of up to [`POOL_MAX`] loop-driven connections**, with
//! the same three properties the sqlx `MySqlPool` it replaces had:
//!
//! * at most 10 connections (`MySqlPoolOptions::max_connections(10)`);
//! * a 10-second acquire deadline, after which a waiter is rejected;
//! * **one connection for the whole request** — a `pool.query()` takes a
//!   connection, runs its prepare and its execute on that one connection, and
//!   only then returns it. The sqlx path did this with an explicit
//!   `pool.acquire()` rather than `pool.execute()`, for the same reason.
//!
//! Waiters are served strictly in arrival order; idle connections are reused
//! most-recently-freed first, which keeps a quiet pool from touching
//! connections the server is about to time out.
//!
//! # What it is NOT
//!
//! No idle reaper, no `maxIdle`, no `idleTimeout`, no connection lifetime, no
//! `queueLimit`: a connection this pool opens lives until `pool.end()` or until
//! the server drops it. The sqlx pool had a reaper; this does not, so a pool
//! that peaks at ten connections keeps ten sockets open. That is a real
//! difference and it is not claimed otherwise. `turnloop_mysql::pool` does
//! implement those policies, but it is built around host connect/close
//! *requests* that the `perry_db_turnloop` driver does not expose, so this
//! keeps its own bookkeeping instead.
//!
//! # Where the acquire deadline lives
//!
//! A pool owns no turnloop handle, so it cannot arm a deadline of its own. The
//! earliest waiter's deadline is instead pushed onto one of the pool's
//! connections through `MysqlCore::request_kick`; when that fires, the sink's
//! `pump` sees the expired waiter. A pool with waiters always has at least one
//! connection, so there is always somewhere to put it.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};

use perry_ffi::{register_handle, take_handle, Handle, JsPromise, Promise};
use turnloop_mysql::Instant;

use super::connection::{Answer, MysqlCore};
use crate::{MySqlConfig, MysqlPromiseError, QueryRequest, DEFAULT_ACQUIRE_TIMEOUT_SECS};

/// `MySqlPoolOptions::new().max_connections(10)`, which is what
/// `js_mysql2_create_pool` configured.
pub(crate) const POOL_MAX: usize = 10;

thread_local! {
    /// JS pool handle → its connections. Thread-local for the same reason the
    /// connection registry is: a turnloop handle belongs to the loop that
    /// created it.
    static POOLS: RefCell<HashMap<Handle, Pool>> = RefCell::new(HashMap::new());
}

struct Pool {
    config: MySqlConfig,
    members: Vec<Member>,
    waiters: VecDeque<Waiter>,
}

struct Member {
    id: i64,
    state: MemberState,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MemberState {
    /// Free for the next request.
    Idle,
    /// Carrying one `pool.query()` / `pool.execute()` request.
    Busy,
    /// Checked out by `getConnection()`. Only `release()` frees it, which is
    /// what makes a pool transaction stay on one connection.
    Pinned,
    /// `release()` arrived while work was still outstanding. Returns to `Idle`
    /// once that work finishes, so a release never cuts a query short.
    Releasing,
}

enum Waiter {
    Request {
        request: QueryRequest,
        promise: JsPromise,
        deadline: Instant,
    },
    Checkout {
        promise: JsPromise,
        deadline: Instant,
    },
}

impl Waiter {
    fn deadline(&self) -> Instant {
        match self {
            Waiter::Request { deadline, .. } | Waiter::Checkout { deadline, .. } => *deadline,
        }
    }

    /// Reject this waiter. Called when the acquire deadline expires or the
    /// pool is closed underneath it — never dropped silently, because a dropped
    /// `JsPromise` never settles.
    fn reject(self, message: &str) {
        match self {
            Waiter::Request { promise, .. } | Waiter::Checkout { promise, .. } => {
                MysqlPromiseError::message(message).reject(promise)
            }
        }
    }
}

fn acquire_deadline() -> Instant {
    Instant::now()
        .checked_add(std::time::Duration::from_secs(DEFAULT_ACQUIRE_TIMEOUT_SECS))
        .unwrap_or_else(Instant::now)
}

/// `mysql.createPool(config)` — synchronous and lazy, opening no connection.
///
/// mysql2's `createPool` is synchronous and connects on first use; the sqlx
/// path matched that with `connect_lazy` after an eager version returned handle
/// 0 for an unreachable database and every `pool.constructor` read crashed.
pub(crate) fn create(config: MySqlConfig) -> Handle {
    let handle = register_handle(crate::MysqlPoolHandle::on_turnloop());
    POOLS.with(|pools| {
        pools.borrow_mut().insert(
            handle,
            Pool {
                config,
                members: Vec::new(),
                waiters: VecDeque::new(),
            },
        )
    });
    handle
}

/// Whether `handle` names a pool on this transport.
pub(crate) fn is_turnloop_pool(handle: Handle) -> bool {
    POOLS.with(|pools| pools.borrow().contains_key(&handle))
}

/// `pool.query(sql, params)` / `pool.execute(...)`.
pub(crate) fn query(handle: Handle, request: QueryRequest) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    // The promise travels in an `Option` so every early return below hands it
    // back rather than dropping it.
    let mut slot = Some(promise);
    // What a promise still sitting in `slot` afterwards means. It starts as the
    // missing-pool answer and becomes the lost-connection one once the pool has
    // been found, so a leftover is never reported as the wrong failure.
    let mut undelivered = "Invalid pool handle";
    POOLS.with(|pools| {
        let mut pools = pools.borrow_mut();
        let Some(pool) = pools.get_mut(&handle) else {
            return;
        };
        undelivered = "Pool acquire failed: connection closed";
        let waiter = Waiter::Request {
            request,
            promise: slot.take().expect("the pool is live"),
            deadline: acquire_deadline(),
        };
        admit(handle, pool, waiter, &mut slot);
    });
    if let Some(promise) = slot {
        MysqlPromiseError::message(undelivered).reject(promise);
    }
    raw
}

/// `pool.getConnection()` — pins one connection until `release()`.
pub(crate) fn get_connection(handle: Handle) -> Option<*mut Promise> {
    if !is_turnloop_pool(handle) {
        return None;
    }
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    let mut slot = Some(promise);
    let mut undelivered = "Invalid pool handle";
    POOLS.with(|pools| {
        let mut pools = pools.borrow_mut();
        let Some(pool) = pools.get_mut(&handle) else {
            return;
        };
        undelivered = "Pool acquire failed: connection closed";
        let waiter = Waiter::Checkout {
            promise: slot.take().expect("the pool is live"),
            deadline: acquire_deadline(),
        };
        admit(handle, pool, waiter, &mut slot);
    });
    if let Some(promise) = slot {
        MysqlPromiseError::message(undelivered).reject(promise);
    }
    Some(raw)
}

/// Serve `waiter` now if the pool can, otherwise queue it.
///
/// `slot` is the caller's escape hatch: anything that leaves without settling
/// the promise puts it back there.
fn admit(handle: Handle, pool: &mut Pool, waiter: Waiter, slot: &mut Option<JsPromise>) {
    // A member whose socket the driver has already retired is not a member any
    // more. Dropped here as well as in `pump` so a request arriving between two
    // completions cannot be handed a connection that no longer exists — and so
    // a pool that lost a connection can open a replacement instead of counting
    // the dead one against `POOL_MAX`.
    pool.members.retain(|member| super::is_live(member.id));
    // Most-recently-freed first: a quiet pool then keeps reusing one connection
    // rather than cycling through ten the server is about to time out.
    if let Some(index) = pool
        .members
        .iter()
        .rposition(|member| member.state == MemberState::Idle)
    {
        assign(handle, pool, index, waiter, slot);
        return;
    }
    if pool.members.len() < POOL_MAX {
        match open(&pool.config, handle) {
            Ok(id) => {
                pool.members.push(Member {
                    id,
                    state: MemberState::Idle,
                });
                let index = pool.members.len() - 1;
                assign(handle, pool, index, waiter, slot);
            }
            Err(message) => match waiter {
                Waiter::Request { promise, .. } | Waiter::Checkout { promise, .. } => {
                    MysqlPromiseError::message(format!("Pool acquire failed: {message}"))
                        .reject(promise)
                }
            },
        }
        return;
    }
    // Every connection is busy. Wait in arrival order, and make sure something
    // will wake this pool when the acquire deadline expires.
    let at = waiter.deadline();
    pool.waiters.push_back(waiter);
    arm_acquire_deadline(pool, at);
}

/// Hand the idle member at `index` to `waiter`.
fn assign(
    handle: Handle,
    pool: &mut Pool,
    index: usize,
    waiter: Waiter,
    slot: &mut Option<JsPromise>,
) {
    let id = pool.members[index].id;
    match waiter {
        Waiter::Request {
            request, promise, ..
        } => {
            // Marked before the submission, so a `pump` that runs later cannot
            // hand this connection to a second caller.
            pool.members[index].state = MemberState::Busy;
            if let Some(promise) =
                super::submit_request(id, request, promise, "Query failed", Answer::ResultTuple)
            {
                pool.members[index].state = MemberState::Idle;
                *slot = Some(promise);
            }
        }
        Waiter::Checkout { promise, .. } => {
            pool.members[index].state = MemberState::Pinned;
            // The JS handle is registered before the promise is parked so the
            // handshake has something to resolve with, exactly as
            // `createConnection` does.
            let conn_handle =
                register_handle(crate::MysqlPoolConnectionHandle::on_turnloop(handle, id));
            let mut parked = Some(promise);
            let delivered = super::with_core(id, |core| {
                core.park_ready(parked.take().expect("the closure runs once"), conn_handle)
            });
            if delivered.is_none() {
                take_handle::<crate::MysqlPoolConnectionHandle>(conn_handle);
                pool.members[index].state = MemberState::Idle;
                *slot = parked;
            }
        }
    }
}

/// `connection.release()` on a pooled connection.
///
/// Returns the connection to the pool once whatever it is carrying finishes —
/// the sqlx path waited on the connection's mutex for the same reason. A
/// release therefore never cuts a query short and never returns a connection
/// with an open transaction to another caller mid-statement.
pub(crate) fn release(pool_handle: Handle, id: i64) {
    POOLS.with(|pools| {
        let mut pools = pools.borrow_mut();
        let Some(pool) = pools.get_mut(&pool_handle) else {
            // The pool is gone (`pool.end()` ran first). Drop the connection
            // rather than leak the socket.
            super::abort(id, "Connection closed");
            return;
        };
        if let Some(member) = pool.members.iter_mut().find(|member| member.id == id) {
            member.state = if super::inspect(id, MysqlCore::is_idle).unwrap_or(false) {
                MemberState::Idle
            } else {
                MemberState::Releasing
            };
        }
    });
    pump();
}

/// `pool.end()` — stop handing out connections and close the ones it has.
pub(crate) fn end(handle: Handle) -> Option<*mut Promise> {
    let pool = POOLS.with(|pools| pools.borrow_mut().remove(&handle))?;
    take_handle::<crate::MysqlPoolHandle>(handle);
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    for waiter in pool.waiters {
        // sqlx answered an acquire on a closed pool with `PoolClosed`; keep a
        // rejection rather than a promise that can never be served.
        waiter.reject("Pool acquire failed: attempted to acquire a connection on a closed pool");
    }
    for member in pool.members {
        // Queued rather than aborted: a `COM_QUIT` runs behind whatever the
        // connection is still carrying, so an in-flight query finishes and
        // answers its caller first. That is what `MySqlPool::close` did.
        super::quit(member.id, None);
    }
    promise.resolve_undefined();
    Some(raw)
}

/// Push `at` onto a member's deadline so the loop wakes this pool by then.
///
/// Any live member will do — the kick only has to make *something* dispatch a
/// completion before `at`. Tried in turn rather than taking the first, because
/// a member whose socket died between the last pump and now would take the
/// deadline nowhere and the waiter would then never time out.
fn arm_acquire_deadline(pool: &mut Pool, at: Instant) {
    for member in &pool.members {
        if super::with_core(member.id, |core| core.request_kick(at)).is_some() {
            return;
        }
    }
}

/// Reconcile every pool with what its connections actually did.
///
/// Called from the sink after each completion has been dispatched, which is the
/// only moment a connection can have become free, died, or had its acquire
/// deadline expire.
pub(crate) fn pump() {
    let mut expired: Vec<(Waiter, &'static str)> = Vec::new();
    POOLS.with(|pools| {
        let mut pools = pools.borrow_mut();
        for (handle, pool) in pools.iter_mut() {
            let handle = *handle;
            // A connection the driver retired is no longer a member. Its
            // promises were settled by the core's own teardown.
            pool.members.retain(|member| super::is_live(member.id));
            for member in pool.members.iter_mut() {
                if matches!(member.state, MemberState::Busy | MemberState::Releasing)
                    && super::inspect(member.id, MysqlCore::is_idle).unwrap_or(false)
                {
                    member.state = MemberState::Idle;
                }
            }
            let now = Instant::now();
            let mut kept = VecDeque::with_capacity(pool.waiters.len());
            while let Some(waiter) = pool.waiters.pop_front() {
                if waiter.deadline() <= now {
                    expired.push((waiter, "Pool acquire timed out"));
                } else {
                    kept.push_back(waiter);
                }
            }
            pool.waiters = kept;
            while !pool.waiters.is_empty() {
                let Some(index) = pool
                    .members
                    .iter()
                    .rposition(|member| member.state == MemberState::Idle)
                else {
                    break;
                };
                let waiter = pool.waiters.pop_front().expect("the queue is not empty");
                let mut slot = None;
                assign(handle, pool, index, waiter, &mut slot);
                if let Some(promise) = slot {
                    MysqlPromiseError::message("Pool acquire failed: connection closed")
                        .reject(promise);
                }
            }
            if let Some(next) = pool.waiters.front().map(Waiter::deadline) {
                arm_acquire_deadline(pool, next);
            }
        }
    });
    // Settled outside the `POOLS` borrow: a rejection runs FFI, and holding a
    // `RefCell` across that is how a re-entrant call turns into a panic.
    for (waiter, message) in expired {
        waiter.reject(message);
    }
}

/// Open one more connection for `pool_handle`.
fn open(config: &MySqlConfig, pool_handle: Handle) -> Result<i64, String> {
    super::open(config, pool_handle as u64)
}
