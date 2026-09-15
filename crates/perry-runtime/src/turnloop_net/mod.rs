//! turnloop P1: Perry's stream networking on turnloop handles
//! (DESIGN §12 "P1", §5a.5, and the P1 row of the migration audit).
//!
//! P0 handed turnloop only the *wait*. P1 hands it the sockets: a TCP or local
//! listener, its multishot accept, every accepted connection, every client
//! connect, the reads, the writes, the write-side shutdown and the close all
//! become operations on the primary agent's `turnloop::Loop`. What that
//! deletes on the caller's side is one tokio task per listener, one per
//! connection, and the per-socket `mpsc` command channel those tasks selected
//! on (`perry-ext-net/src/lib.rs`'s `run_socket_task`).
//!
//! # Why the core lives here and not in the `net` binding
//!
//! The loop is per agent and thread-local, and perry-runtime owns it
//! (`event_pump/agent_loop.rs`). A binding crate is a separately linked
//! `staticlib` with no Cargo edge to perry-runtime, so it cannot hold a
//! `&mut Loop`. Everything that must touch the driver therefore lives here,
//! and the bindings reach it through the C ABI in [`abi`] (perry-ffi wraps
//! that for ext crates the same way it wraps the event pump).
//!
//! # Completion routing
//!
//! turnloop is pull-based (DESIGN D1): nothing is called from inside the
//! driver. `agent_loop` turns the loop, drains the completion buffer, and only
//! then calls [`dispatch`], which routes each completion to the *subsystem*
//! that submitted it. A subsystem registers one sink plus one id allocator
//! ([`register_sink`]); the allocator exists because an accepted connection is
//! a resource turnloop creates, and the id space it must be named in belongs
//! to the binding (`perry_ffi::reserve_handle_id_in_domain`).
//!
//! Routing needs no side table: the submission `Token` *is* the route. Its top
//! 8 bits are the operation class and its low 56 bits are the Perry-side id,
//! so a completion identifies its socket and its syscall without a lookup, and
//! a stale token from a closed socket finds no entry and is dropped.
//!
//! # GC
//!
//! **No JS heap memory is ever handed to the driver.** Reads land in
//! turnloop's own pooled buffers and are copied into JS values by the sink,
//! on the owning thread, inside the dispatch call; writes arrive as an owned
//! `Vec<u8>` the caller already copied out of the JS value (which is what
//! `perry-ext-net`'s `jsvalue_to_socket_bytes` has always done). So there is
//! no buffer to root across a collection and no pointer for a moving
//! collector to invalidate — strictly stronger than the "root from submit to
//! completion" rule in DESIGN D3, and it is the reason this module registers
//! no GC root scanner. The JS-side records (listener closures, completion
//! callbacks) stay where they are, under the binding's existing scanner.
//!
//! Exactly-once release (DESIGN D4) is what makes that safe to *state*: every
//! accepted operation ends in exactly one terminal completion, and the entry —
//! with its queued writes — is dropped only when the handle's final `Closed`
//! arrives.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use turnloop::{
    Completion, Error, ErrorKind, Handle, ListenOpts, OpId, OpResult, PipeName, TcpOpts, Token,
    WriteBuf,
};

pub mod abi;
pub(crate) mod errors;
mod sink;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

pub use errors::{map_error, NodeError};
pub use sink::{register_sink, sink_installed, NetCompletion, SinkFn, MAX_SUBSYSTEMS};

// ── Operation classes, carried in the top 8 bits of every submission token ──
const OP_ACCEPT: u64 = 1;
const OP_READ: u64 = 2;
const OP_WRITE: u64 = 3;
const OP_SHUTDOWN: u64 = 4;
const OP_CONNECT: u64 = 5;
const OP_CLOSE: u64 = 6;
const OP_RESOLVE: u64 = 7;
/// P5: a subsystem-owned one-shot deadline (server timeouts).
const OP_TIMER: u64 = 8;

/// The low 56 bits of a token hold the Perry-side id.
const ID_BITS: u32 = 56;
const ID_MASK: u64 = (1 << ID_BITS) - 1;

fn token(op: u64, id: i64) -> Token {
    debug_assert!(id > 0 && (id as u64) <= ID_MASK, "id {id} fits a token");
    Token((op << ID_BITS) | (id as u64 & ID_MASK))
}

fn token_parts(t: Token) -> (u64, i64) {
    ((t.0 >> ID_BITS), (t.0 & ID_MASK) as i64)
}

/// The `syscall` string Node reports for a failure of each operation class.
fn syscall_for(op: u64) -> &'static str {
    match op {
        OP_ACCEPT => "accept",
        OP_READ => "read",
        OP_WRITE => "write",
        OP_SHUTDOWN => "shutdown",
        OP_CONNECT => "connect",
        OP_CLOSE => "close",
        OP_RESOLVE => "getaddrinfo",
        OP_TIMER => "timer",
        _ => "",
    }
}

/// One write the caller handed over, still owned by the driver.
struct PendingWrite {
    /// The caller's completion token, echoed back on the `Wrote` completion.
    /// Zero means "no callback"; it is never used for routing.
    user: u64,
    len: usize,
}

/// Everything Perry knows about one turnloop-backed socket or listener.
///
/// Deliberately holds no JS value and no GC pointer (see the module note): a
/// binding keeps its own JS-side record keyed by the same id.
struct Entry {
    handle: Handle,
    subsystem: u8,
    /// A listener answers `accept`, never `read`/`write`. Kept so a misrouted
    /// submission is rejected here instead of by the backend.
    listener: bool,
    accept_op: Option<OpId>,
    read_op: Option<OpId>,
    writes: VecDeque<PendingWrite>,
    /// Bytes handed to the driver and not yet reported written — Node's
    /// `socket.writableLength`, and the input to its `write()` return value.
    queued: usize,
    /// `close` was submitted; the entry survives until its `Closed` arrives.
    closing: bool,
    referenced: bool,
    local: Option<SocketAddr>,
    peer: Option<SocketAddr>,
    /// Bound path of a local listener, so the caller can unlink it on close.
    path: Option<PathBuf>,
}

impl Entry {
    fn new(handle: Handle, subsystem: u8, listener: bool) -> Self {
        Self {
            handle,
            subsystem,
            listener,
            accept_op: None,
            read_op: None,
            writes: VecDeque::new(),
            queued: 0,
            closing: false,
            referenced: true,
            local: None,
            peer: None,
            path: None,
        }
    }
}

/// A client connect that is still choosing an address.
///
/// It starts with no handle at all (the hostname is still resolving), and
/// afterwards outlives each failed attempt: a name that resolves to both
/// families — `localhost` on any dual-stack host — must not fail because the
/// first family's listener does not exist. Node calls this `autoSelectFamily`
/// and has it on by default since v20; the tokio path got it free from
/// `TcpStream::connect(&str)`, which walks the whole address list.
///
/// This is the sequential form of that walk: one attempt at a time, each
/// failure closing its handle before the next is created, and the *last*
/// error reported if every address fails. Node additionally races the
/// families on a 250 ms head start; the outcome only differs in how fast a
/// dead family is abandoned, never in which connection is established.
struct ConnectPlan {
    subsystem: u8,
    nodelay: bool,
    /// Addresses not yet attempted, in resolver order.
    remaining: std::collections::VecDeque<SocketAddr>,
    /// Set while the failed attempt's handle is being closed; its `Closed`
    /// starts the next attempt instead of reaching the binding.
    retrying: bool,
    /// The most recent failure, reported if the list runs out.
    last_error: Option<NodeError>,
}

/// A subsystem-owned one-shot deadline (P5).
///
/// Perry's server timeouts — `keepAliveTimeout`, `headersTimeout`,
/// `requestTimeout`, a TLS handshake deadline, a lingering close — are
/// deadlines on a *connection*, not JS timers, and a binding has no way to
/// create a JS timer. Arming them here puts them in `Loop::next_deadline()`,
/// so a park that has nothing but an idle keep-alive connection still ends on
/// time instead of blocking until the peer does something.
///
/// Deliberately **unreferenced**, like the agent's JS-timer deadline: a
/// pending deadline must never keep the process alive on its own. An idle
/// connection is kept alive by its own read operation, which is the thing the
/// deadline is there to end.
struct TimerEntry {
    handle: Handle,
    subsystem: u8,
}

#[derive(Default)]
struct NetState {
    entries: HashMap<i64, Entry>,
    plans: HashMap<i64, ConnectPlan>,
    timers: HashMap<i64, TimerEntry>,
}

thread_local! {
    /// Per agent, like the loop itself. A socket belongs to the thread that
    /// created it; there is no cross-thread map to race on.
    static NET: RefCell<NetState> = RefCell::new(NetState::default());
}

/// Number of live turnloop-backed sockets and listeners on this thread.
///
/// The keep-alive answer a binding needs, and the assertion a test needs: a
/// "turnloop drove this workload" claim is only worth making if this was ever
/// nonzero (DESIGN §11, "a benchmark must assert its subject ran").
pub fn live_handles() -> usize {
    NET.with(|net| {
        let net = net.borrow();
        net.entries.len() + net.plans.len() + net.timers.len()
    })
}

/// Whether this thread can take the turnloop net path at all.
///
/// False on a worker agent (no loop before P3/P4), in the `tokio-wait-driver`
/// A/B arm, and on a host where loop creation failed. A caller that gets
/// `false` must keep its existing transport — that is the P1 coexistence rule,
/// and it is why the tokio socket task is not deleted outright.
pub fn available() -> bool {
    crate::event_pump::net_loop_available()
}

/// Errors this module reports to its callers, before any completion exists.
pub type NetResult<T> = Result<T, NodeError>;

fn no_loop() -> NodeError {
    NodeError {
        code: "ENOTSUP",
        errno: 0,
        syscall: "",
    }
}

fn not_found(syscall: &'static str) -> NodeError {
    map_error(Error::new(ErrorKind::NotFound), syscall)
}

/// Run `f` against this agent's driver, creating a net-sized loop first.
fn with_driver<R>(f: impl FnOnce(&mut turnloop::Loop) -> R) -> Option<R> {
    crate::event_pump::with_net_driver(f)
}

// ── Submission ──────────────────────────────────────────────────────────────

/// Bind and listen on a TCP address. Synchronous, like `bind(2)`: a failure
/// here is the `EADDRINUSE` / `EACCES` the caller must surface as `'error'`.
///
/// Returns the *actual* local address, which is what `server.address()` must
/// report after a `listen(0)` ephemeral bind.
pub fn tcp_listen(
    id: i64,
    subsystem: u8,
    addr: SocketAddr,
    backlog: u32,
    reuse_port: bool,
) -> NetResult<SocketAddr> {
    with_driver(|driver| {
        let opts = ListenOpts {
            reuse_port,
            backlog,
        };
        let handle = driver
            .tcp_listen(addr, &opts)
            .map_err(|e| map_error(e, "listen"))?;
        let local = driver.local_addr(handle).unwrap_or(addr);
        let mut entry = Entry::new(handle, subsystem, true);
        entry.local = Some(local);
        NET.with(|net| net.borrow_mut().entries.insert(id, entry));
        Ok(local)
    })
    .unwrap_or_else(|| Err(no_loop()))
}

/// Bind and listen on a local endpoint: a Unix-domain socket path, or a
/// Windows named pipe (`\\.\pipe\...`). Removing a stale socket file is the
/// caller's job, as it is in Node.
pub fn pipe_listen(id: i64, subsystem: u8, path: &Path, backlog: u32) -> NetResult<()> {
    with_driver(|driver| {
        let opts = ListenOpts {
            reuse_port: false,
            backlog,
        };
        let name = PipeName(path.to_path_buf());
        let handle = driver
            .pipe_listen(&name, &opts)
            .map_err(|e| map_error(e, "listen"))?;
        let mut entry = Entry::new(handle, subsystem, true);
        entry.path = Some(path.to_path_buf());
        NET.with(|net| net.borrow_mut().entries.insert(id, entry));
        Ok(())
    })
    .unwrap_or_else(|| Err(no_loop()))
}

/// Start accepting. Multishot (DESIGN D4): one submission yields a completion
/// per connection until it is stopped, cancelled or errors — no resubmission
/// per accept, and no task to hold the runtime open between them.
pub fn accept_start(id: i64) -> NetResult<()> {
    with_driver(|driver| {
        NET.with(|net| {
            let mut net = net.borrow_mut();
            let entry = net
                .entries
                .get_mut(&id)
                .ok_or_else(|| not_found("accept"))?;
            if entry.accept_op.is_some() {
                return Ok(());
            }
            let op = driver
                .accept_start(entry.handle, token(OP_ACCEPT, id))
                .map_err(|e| map_error(e, "accept"))?;
            entry.accept_op = Some(op);
            Ok(())
        })
    })
    .unwrap_or_else(|| Err(no_loop()))
}

/// Connect a TCP client socket. The `Connected` (or error) completion arrives
/// on a later turn; nothing blocks here.
pub fn tcp_connect(id: i64, subsystem: u8, addr: SocketAddr, nodelay: bool) -> NetResult<()> {
    with_driver(|driver| {
        let handle = driver
            .tcp_connect(addr, &TcpOpts { nodelay }, token(OP_CONNECT, id))
            .map_err(|e| map_error(e, "connect"))?;
        let mut entry = Entry::new(handle, subsystem, false);
        entry.peer = Some(addr);
        NET.with(|net| net.borrow_mut().entries.insert(id, entry));
        Ok(())
    })
    .unwrap_or_else(|| Err(no_loop()))
}

/// Connect a TCP client socket to `host:port`, resolving a hostname first.
///
/// An IP literal connects immediately. A name goes to `Loop::resolve`, which
/// uses the backend's own resolver where it has one and the shared blocking
/// pool otherwise — `getaddrinfo` never runs on the event-loop thread, which
/// is the property the tokio path had and a naive `to_socket_addrs()` here
/// would have silently lost.
pub fn tcp_connect_host(
    id: i64,
    subsystem: u8,
    host: &str,
    port: u16,
    nodelay: bool,
) -> NetResult<()> {
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return tcp_connect(id, subsystem, SocketAddr::new(ip, port), nodelay);
    }
    with_driver(|driver| {
        let request = turnloop::DnsRequest {
            host: host.to_string(),
            port,
        };
        driver
            .resolve(request, token(OP_RESOLVE, id))
            .map_err(|e| map_error(e, "getaddrinfo"))?;
        NET.with(|net| {
            net.borrow_mut().plans.insert(
                id,
                ConnectPlan {
                    subsystem,
                    nodelay,
                    remaining: std::collections::VecDeque::new(),
                    retrying: false,
                    last_error: None,
                },
            )
        });
        Ok(())
    })
    .unwrap_or_else(|| Err(no_loop()))
}

/// Connect to a Unix-domain socket or a Windows named pipe.
pub fn pipe_connect(id: i64, subsystem: u8, path: &Path) -> NetResult<()> {
    with_driver(|driver| {
        let name = PipeName(path.to_path_buf());
        let handle = driver
            .pipe_connect(&name, token(OP_CONNECT, id))
            .map_err(|e| map_error(e, "connect"))?;
        let mut entry = Entry::new(handle, subsystem, false);
        entry.path = Some(path.to_path_buf());
        NET.with(|net| net.borrow_mut().entries.insert(id, entry));
        Ok(())
    })
    .unwrap_or_else(|| Err(no_loop()))
}

/// Arm — or move — a subsystem-owned one-shot deadline `delay_ms` from now.
///
/// `id` is the caller's own id for the deadline; it must not collide with a
/// socket id, which the shared handle-id allocator already guarantees. Arming
/// an id that already has a deadline moves it, so a per-connection timeout can
/// be refreshed on every read without churning handles.
pub fn timer_arm(id: i64, subsystem: u8, delay_ms: u64) -> NetResult<()> {
    with_driver(|driver| {
        let at = driver.now() + std::time::Duration::from_millis(delay_ms);
        let existing = NET.with(|net| net.borrow().timers.get(&id).map(|t| t.handle));
        if let Some(handle) = existing {
            if driver.timer_reset(handle, at) {
                return Ok(());
            }
            // The timer already fired or is closing: replace it below.
            let _ = driver.close(handle, token(OP_TIMER, id));
            NET.with(|net| net.borrow_mut().timers.remove(&id));
        }
        let handle = driver
            .timer(at, None, token(OP_TIMER, id))
            .map_err(|e| map_error(e, "timer"))?;
        // Must not hold the loop alive on its own (see `TimerEntry`).
        let _ = driver.set_ref(handle, false);
        NET.with(|net| {
            net.borrow_mut()
                .timers
                .insert(id, TimerEntry { handle, subsystem })
        });
        Ok(())
    })
    .unwrap_or_else(|| Err(no_loop()))
}

/// Hand a live socket to another subsystem, keeping its id and every
/// outstanding operation (P5).
///
/// An HTTP `'upgrade'` is exactly this: the server crate decoded the head and
/// the rest of the connection belongs to `net` as a raw `net.Socket`. The
/// multishot read is deliberately **not** cancelled — the token carries only
/// the id, and routing reads the subsystem out of the entry at dispatch time,
/// so the very next `Read` completion is delivered to the new owner with no
/// gap and no resubmission. Anything the old owner had already buffered it
/// hands over itself (Node's `'upgrade'` `head` argument).
pub fn transfer(id: i64, subsystem: u8) -> NetResult<()> {
    if subsystem as usize >= sink::MAX_SUBSYSTEMS {
        return Err(map_error(Error::new(ErrorKind::InvalidInput), "transfer"));
    }
    NET.with(|net| {
        let mut net = net.borrow_mut();
        match net.entries.get_mut(&id) {
            Some(entry) => {
                entry.subsystem = subsystem;
                Ok(())
            }
            None => Err(not_found("transfer")),
        }
    })
}

/// Cancel a deadline. Idempotent: an id with no deadline is not an error,
/// because a connection cancels its timeout on every completion path.
pub fn timer_cancel(id: i64) -> NetResult<()> {
    let handle = NET.with(|net| net.borrow_mut().timers.remove(&id).map(|t| t.handle));
    let Some(handle) = handle else {
        return Ok(());
    };
    with_driver(|driver| {
        let _ = driver.close(handle, token(OP_TIMER, id));
        Ok(())
    })
    .unwrap_or_else(|| Err(no_loop()))
}

/// Start reading. Multishot into turnloop's buffer pool: the read side needs
/// no per-socket buffer and no resubmission, and pool exhaustion applies
/// backpressure by leaving the read pending rather than by allocating.
pub fn read_start(id: i64) -> NetResult<()> {
    with_driver(|driver| {
        NET.with(|net| {
            let mut net = net.borrow_mut();
            let entry = net.entries.get_mut(&id).ok_or_else(|| not_found("read"))?;
            if entry.read_op.is_some() || entry.closing {
                return Ok(());
            }
            let op = driver
                .read_start(entry.handle, token(OP_READ, id))
                .map_err(|e| map_error(e, "read"))?;
            entry.read_op = Some(op);
            Ok(())
        })
    })
    .unwrap_or_else(|| Err(no_loop()))
}

/// Hand `bytes` to the driver. Returns the number of bytes now queued on this
/// socket — everything `socket.write()` needs to decide its `false` return,
/// and everything `writableLength` reports.
///
/// Ordering is turnloop's: `write` completes the *whole* buffer, and queued
/// writes on one handle preserve submission order, so there is no partial-write
/// bookkeeping here and no per-write channel.
pub fn write(id: i64, bytes: Vec<u8>, user: u64) -> NetResult<usize> {
    with_driver(|driver| {
        NET.with(|net| {
            let mut net = net.borrow_mut();
            let entry = net.entries.get_mut(&id).ok_or_else(|| not_found("write"))?;
            if entry.listener || entry.closing {
                return Err(map_error(Error::new(ErrorKind::InvalidInput), "write"));
            }
            let len = bytes.len();
            driver
                .write(entry.handle, WriteBuf::Owned(bytes), token(OP_WRITE, id))
                .map_err(|e| map_error(e, "write"))?;
            entry.writes.push_back(PendingWrite { user, len });
            entry.queued += len;
            Ok(entry.queued)
        })
    })
    .unwrap_or_else(|| Err(no_loop()))
}

/// Half-close: shut the write side down after every queued write has gone out
/// (`socket.end()`), leaving the read side open for the peer's reply.
pub fn shutdown(id: i64, user: u64) -> NetResult<()> {
    with_driver(|driver| {
        NET.with(|net| {
            let mut net = net.borrow_mut();
            let entry = net
                .entries
                .get_mut(&id)
                .ok_or_else(|| not_found("shutdown"))?;
            if entry.listener || entry.closing {
                return Err(map_error(Error::new(ErrorKind::InvalidInput), "shutdown"));
            }
            // The user token rides the pending-write queue's tail slot so the
            // `Shutdown` completion can echo it back; a zero-length entry never
            // affects `queued`.
            driver
                .shutdown(entry.handle, token(OP_SHUTDOWN, id))
                .map_err(|e| map_error(e, "shutdown"))?;
            entry.writes.push_back(PendingWrite { user, len: 0 });
            Ok(())
        })
    })
    .unwrap_or_else(|| Err(no_loop()))
}

/// Close the handle. Outstanding operations are cancelled and the entry is
/// dropped only when the final `Closed` completion arrives (DESIGN D4), so a
/// caller never has to guess when the descriptor is really gone.
pub fn close(id: i64) -> NetResult<()> {
    with_driver(|driver| {
        NET.with(|net| {
            let mut net = net.borrow_mut();
            let entry = net.entries.get_mut(&id).ok_or_else(|| not_found("close"))?;
            if entry.closing {
                return Ok(());
            }
            entry.closing = true;
            driver
                .close(entry.handle, token(OP_CLOSE, id))
                .map_err(|e| map_error(e, "close"))
        })
    })
    .unwrap_or_else(|| Err(no_loop()))
}

/// Node's `ref()`/`unref()`: whether this handle keeps the loop alive.
pub fn set_ref(id: i64, referenced: bool) -> NetResult<()> {
    with_driver(|driver| {
        NET.with(|net| {
            let mut net = net.borrow_mut();
            let entry = net.entries.get_mut(&id).ok_or_else(|| not_found(""))?;
            if entry.referenced == referenced {
                return Ok(());
            }
            entry.referenced = referenced;
            driver
                .set_ref(entry.handle, referenced)
                .map_err(|e| map_error(e, ""))
        })
    })
    .unwrap_or_else(|| Err(no_loop()))
}

/// Whether the loop still has referenced work — the turnloop half of the
/// binding's keep-alive answer.
pub fn loop_alive() -> bool {
    with_driver(|driver| driver.alive()).unwrap_or(false)
}

/// The socket's local endpoint, as `server.address()` / `socket.localAddress`
/// report it.
pub fn local_addr(id: i64) -> Option<SocketAddr> {
    NET.with(|net| net.borrow().entries.get(&id).and_then(|e| e.local))
}

/// The socket's peer endpoint (`socket.remoteAddress`).
pub fn peer_addr(id: i64) -> Option<SocketAddr> {
    NET.with(|net| net.borrow().entries.get(&id).and_then(|e| e.peer))
}

/// Bytes handed to the driver and not yet reported written.
pub fn queued_bytes(id: i64) -> usize {
    NET.with(|net| net.borrow().entries.get(&id).map_or(0, |e| e.queued))
}

/// Whether `id` names a live turnloop-backed handle on this thread.
pub fn is_live(id: i64) -> bool {
    NET.with(|net| {
        let net = net.borrow();
        net.entries.contains_key(&id) || net.plans.contains_key(&id)
    })
}

// ── Completion dispatch ─────────────────────────────────────────────────────

/// Route one completion to the subsystem that submitted it.
///
/// Called by `agent_loop` *after* `turn` has returned (DESIGN D1: the driver
/// never calls host code), so a sink is free to run JS, allocate, collect, and
/// submit new operations on the same loop.
pub(crate) fn dispatch(completion: Completion) {
    let (op_class, id) = token_parts(completion.token);
    let Completion {
        result, terminal, ..
    } = completion;

    // A deadline has no `Entry`, so it is routed before the lookup below. Its
    // expiry retires the operation, and its `Closed` is the terminal the
    // cancel path produces — neither reaches the binding twice.
    if op_class == OP_TIMER {
        let fired = matches!(result, OpResult::Timer);
        let subsystem = NET.with(|net| {
            let mut net = net.borrow_mut();
            let subsystem = net.timers.get(&id).map(|t| t.subsystem);
            if fired {
                // A one-shot expiry is terminal: drop the record so a later
                // `timer_arm` for the same id creates a fresh handle.
                net.timers.remove(&id);
            }
            subsystem
        });
        if let (true, Some(subsystem)) = (fired, subsystem) {
            sink::emit(subsystem, NetCompletion::timer(id));
        }
        return;
    }

    // Everything below needs the subsystem, and most arms need to mutate the
    // entry. Take both under one short borrow and release it before calling
    // out: a sink re-enters this module (`write`, `read_start`, `close`).
    let Some(subsystem) = NET.with(|net| {
        let net = net.borrow();
        net.entries
            .get(&id)
            .map(|e| e.subsystem)
            .or_else(|| net.plans.get(&id).map(|p| p.subsystem))
    }) else {
        // A completion for an entry that is already gone. `Cancelled` results
        // after a close race here routinely; they are not errors.
        return;
    };

    if op_class == OP_RESOLVE {
        resolve_completed(subsystem, id, result);
        return;
    }

    match result {
        OpResult::Connected => {
            // The peer address was recorded at submit; the local one only
            // exists now that the connection is established.
            let local = with_driver(|driver| {
                NET.with(|net| {
                    let handle = net.borrow().entries.get(&id)?.handle;
                    driver.local_addr(handle).ok()
                })
            })
            .flatten();
            NET.with(|net| {
                let mut net = net.borrow_mut();
                if let Some(entry) = net.entries.get_mut(&id) {
                    entry.local = local;
                }
                net.plans.remove(&id);
            });
            sink::emit(subsystem, NetCompletion::connect(id));
        }
        OpResult::Accepted { conn, peer } => {
            accept_connection(subsystem, id, conn, Some(peer));
        }
        OpResult::PipeAccepted { conn } => {
            accept_connection(subsystem, id, conn, None);
        }
        OpResult::Read { n, lease } => {
            let bytes = lease.as_ref().map(|l| l.as_slice()).unwrap_or(&[]);
            debug_assert!(bytes.len() == n || lease.is_none());
            sink::emit(subsystem, NetCompletion::data(id, bytes));
            // The lease returns to turnloop's pool here, after the sink has
            // copied what it needs. Holding it would throttle reads.
            drop(lease);
        }
        OpResult::Eof => {
            NET.with(|net| {
                if let Some(entry) = net.borrow_mut().entries.get_mut(&id) {
                    entry.read_op = None;
                }
            });
            sink::emit(subsystem, NetCompletion::eof(id));
        }
        OpResult::Wrote(n) => {
            let (user, queued) = NET.with(|net| {
                let mut net = net.borrow_mut();
                let Some(entry) = net.entries.get_mut(&id) else {
                    return (0, 0);
                };
                let user = entry.writes.pop_front().map_or(0, |w| w.user);
                entry.queued = entry.queued.saturating_sub(n);
                (user, entry.queued)
            });
            sink::emit(subsystem, NetCompletion::wrote(id, user, n, queued));
        }
        OpResult::Shutdown => {
            let user = NET.with(|net| {
                let mut net = net.borrow_mut();
                net.entries
                    .get_mut(&id)
                    .and_then(|entry| entry.writes.pop_front())
                    .map_or(0, |w| w.user)
            });
            sink::emit(subsystem, NetCompletion::shutdown(id, user));
        }
        OpResult::Closed => {
            NET.with(|net| net.borrow_mut().entries.remove(&id));
            let retrying = NET.with(|net| {
                let mut net = net.borrow_mut();
                match net.plans.get_mut(&id) {
                    Some(plan) if plan.retrying => {
                        plan.retrying = false;
                        true
                    }
                    _ => false,
                }
            });
            if retrying {
                // A failed connect attempt, not the socket the caller sees.
                attempt_next_address(id);
            } else {
                sink::emit(subsystem, NetCompletion::closed(id));
            }
        }
        OpResult::Err(err) => {
            let (user, queued) = if op_class == OP_WRITE || op_class == OP_SHUTDOWN {
                NET.with(|net| {
                    let mut net = net.borrow_mut();
                    let Some(entry) = net.entries.get_mut(&id) else {
                        return (0, 0);
                    };
                    let w = entry.writes.pop_front();
                    let user = w.as_ref().map_or(0, |w| w.user);
                    entry.queued = entry.queued.saturating_sub(w.map_or(0, |w| w.len));
                    (user, entry.queued)
                })
            } else {
                (0, queued_bytes(id))
            };
            if terminal {
                clear_op(id, op_class);
            }
            let mapped = map_error(err, syscall_for(op_class));
            if op_class == OP_CONNECT && connect_failed(id, mapped) {
                // Absorbed into the next address attempt.
                return;
            }
            sink::emit(
                subsystem,
                NetCompletion::error(id, user, queued, mapped, terminal),
            );
        }
        OpResult::Cancelled | OpResult::Stopped => {
            clear_op(id, op_class);
            // A cancelled write's bytes never left; drop its accounting so a
            // socket that is closing does not report a permanently non-empty
            // write buffer to `writableLength`.
            if op_class == OP_WRITE || op_class == OP_SHUTDOWN {
                NET.with(|net| {
                    let mut net = net.borrow_mut();
                    if let Some(entry) = net.entries.get_mut(&id) {
                        if let Some(w) = entry.writes.pop_front() {
                            entry.queued = entry.queued.saturating_sub(w.len);
                        }
                    }
                });
            }
        }
        // P1 submits no timer, signal, process, datagram, blocking or posted
        // work on this token space; those belong to P2/P3/P4.
        _ => {}
    }
}

/// Promote a resolved hostname into a real connect, or report the lookup
/// failure the way Node does (`getaddrinfo ENOTFOUND <host>`).
fn resolve_completed(subsystem: u8, id: i64, result: OpResult) {
    match result {
        OpResult::Resolved(addresses) => {
            let has_plan = NET.with(|net| {
                let mut net = net.borrow_mut();
                match net.plans.get_mut(&id) {
                    Some(plan) => {
                        plan.remaining = addresses.into_iter().collect();
                        true
                    }
                    None => false,
                }
            });
            if has_plan {
                attempt_next_address(id);
            }
        }
        OpResult::Err(err) => {
            NET.with(|net| net.borrow_mut().plans.remove(&id));
            let mut mapped = map_error(err, "getaddrinfo");
            // libuv (and therefore Node) reports a failed name lookup as
            // ENOTFOUND whatever the resolver's own errno was, and Node's own
            // tests match on that string.
            mapped.code = "ENOTFOUND";
            sink::emit(subsystem, NetCompletion::error(id, 0, 0, mapped, true));
        }
        OpResult::Cancelled | OpResult::Stopped => {
            NET.with(|net| net.borrow_mut().plans.remove(&id));
        }
        _ => {}
    }
}

/// Start the next address in a connect plan, or report the final failure.
fn attempt_next_address(id: i64) {
    let Some((subsystem, nodelay, next, last_error)) = NET.with(|net| {
        let mut net = net.borrow_mut();
        let plan = net.plans.get_mut(&id)?;
        Some((
            plan.subsystem,
            plan.nodelay,
            plan.remaining.pop_front(),
            plan.last_error,
        ))
    }) else {
        return;
    };
    let Some(addr) = next else {
        NET.with(|net| net.borrow_mut().plans.remove(&id));
        let err = last_error.unwrap_or(NodeError {
            code: "ECONNREFUSED",
            errno: 0,
            syscall: "connect",
        });
        sink::emit(subsystem, NetCompletion::error(id, 0, 0, err, true));
        return;
    };
    if let Err(err) = tcp_connect(id, subsystem, addr, nodelay) {
        // Submission itself failed (descriptor exhaustion, a full operation
        // table): record it and move on, so one bad family cannot mask a
        // working one.
        NET.with(|net| {
            if let Some(plan) = net.borrow_mut().plans.get_mut(&id) {
                plan.last_error = Some(err);
            }
        });
        attempt_next_address(id);
    }
}

/// A connect attempt failed. Returns true when the failure was absorbed into
/// a retry (the caller must not report it).
fn connect_failed(id: i64, err: NodeError) -> bool {
    let retry = NET.with(|net| {
        let mut net = net.borrow_mut();
        let Some(plan) = net.plans.get_mut(&id) else {
            return false;
        };
        plan.last_error = Some(err);
        if plan.remaining.is_empty() {
            return false;
        }
        plan.retrying = true;
        true
    });
    if !retry {
        // Either there was no plan (a direct-address connect) or the list is
        // exhausted; drop the plan so its `Closed` is reported normally.
        NET.with(|net| net.borrow_mut().plans.remove(&id));
        return false;
    }
    // Release this attempt's handle first: `Closed` is what starts the next.
    let _ = close(id);
    true
}

fn clear_op(id: i64, op_class: u64) {
    NET.with(|net| {
        let mut net = net.borrow_mut();
        if let Some(entry) = net.entries.get_mut(&id) {
            match op_class {
                OP_ACCEPT => entry.accept_op = None,
                OP_READ => entry.read_op = None,
                _ => {}
            }
        }
    });
}

/// Register a connection turnloop just accepted, under an id the *subsystem*
/// allocates (its id space is JS-visible; ours is not).
fn accept_connection(subsystem: u8, server: i64, conn: Handle, peer: Option<SocketAddr>) {
    let Some(conn_id) = sink::allocate_id(subsystem) else {
        // No allocator, or the binding refused an id: the connection would be
        // unreachable, so close it rather than leaking the descriptor.
        let _ = with_driver(|driver| driver.close(conn, Token(0)));
        return;
    };
    let local = with_driver(|driver| driver.local_addr(conn).ok()).flatten();
    let mut entry = Entry::new(conn, subsystem, false);
    entry.local = local;
    entry.peer = peer;
    NET.with(|net| net.borrow_mut().entries.insert(conn_id, entry));
    sink::emit(subsystem, NetCompletion::accept(server, conn_id, peer));
}

/// Test-only: forget this thread's entries without touching the driver, for a
/// test that is about to drop the loop itself.
#[cfg(test)]
pub(crate) fn reset_for_test() {
    NET.with(|net| net.borrow_mut().timers.clear());
    NET.with(|net| {
        let mut net = net.borrow_mut();
        net.entries.clear();
        net.plans.clear();
    });
}

/// Close every handle this thread still owns, for agent teardown.
///
/// The loop is dropped right after, and `Loop::drop` quiesces native I/O, so
/// this exists to run the sink's `Closed` bookkeeping rather than to release
/// descriptors.
pub(crate) fn shutdown_current_thread() {
    let ids: Vec<i64> = NET.with(|net| net.borrow().entries.keys().copied().collect());
    for id in ids {
        let _ = close(id);
    }
    NET.with(|net| net.borrow_mut().plans.clear());
}
