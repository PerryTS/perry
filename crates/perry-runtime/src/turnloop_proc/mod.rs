//! turnloop P2: child processes, their pipes, `process.stdin`, datagram
//! sockets and OS signals on turnloop handles (DESIGN §12 "P2", §5a.4).
//!
//! P1 moved `node:net`'s sockets onto the loop. P2 moves everything the
//! migration audit lists under "many ad-hoc threads": the per-child stdout and
//! stderr readers, the child waiter, the IPC reader, the `process.stdin`
//! reader, the per-dgram-socket `recv_from` poller and the signal wake thread.
//! Each was a thread whose only job was to turn a blocking syscall into a
//! queue push plus a `js_notify_main_thread()`; each becomes an operation on
//! this agent's `turnloop::Loop`, completing on the thread that owns the heap.
//!
//! # Why this is not `turnloop_net`
//!
//! [`crate::turnloop_net`] exists because `perry-ext-net` is a separately
//! linked `staticlib` with no Cargo edge to perry-runtime, so its completions
//! have to cross a C ABI. Every P2 subsystem already lives *in* perry-runtime,
//! so there is no ABI here at all: [`dispatch`] calls the owning module
//! directly through [`Owner`]. That is the whole reason this is a second
//! module rather than a fifth subsystem slot in the P1 sink registry.
//!
//! # Adoption, not re-implementation
//!
//! Perry creates most of these descriptors itself and must keep doing so: a
//! dgram socket carries Node's bind-time `SO_REUSEADDR` and its post-bind
//! multicast state, and a child is launched through `std::process::Command`
//! with `pre_exec` hooks that turnloop's `ProcessSpec` has no equivalent for
//! (fd 3 `NODE_CHANNEL_FD` for `fork()`, arbitrary `stdio` fd maps, `setsid`).
//! So P2 *adopts*: the descriptor is created by the existing code and handed
//! to the loop with `Detached::from_fd` / `from_socket` / `from_handle` plus
//! [`turnloop::Loop::attach`]. What moves is the **wait**, which is the thread
//! this phase deletes; what stays is every syscall Perry already got right.
//! See `docs/turnloop/p2-report.md` for the per-subsystem table.
//!
//! # Completion routing
//!
//! One token space, disjoint from P1's: the top 8 bits are the operation
//! class (`0x10`–`0x1F`, versus P1's `1`–`7`), the low 56 bits the Perry-side
//! id. [`crate::event_pump::agent_loop`] routes a staged completion here when
//! [`owns`] recognises the class, so neither module can be handed the other's
//! completion, and a stale token from a closed handle finds no entry and is
//! dropped.
//!
//! # GC
//!
//! **No JS heap memory reaches the driver.** Reads land in turnloop's pooled
//! buffers and are copied out inside the dispatch call, on the owning thread,
//! into the very same `Vec<u8>`-carrying queue entries the deleted threads
//! pushed; writes arrive as an owned `Vec<u8>` the caller already copied out
//! of the JS value. So there is nothing to root from submit to completion and
//! nothing for a moving collector to invalidate — the same property P1
//! established, and the reason this module registers no root scanner.
//!
//! The JS-side records are untouched: `dgram_reactor::scan_roots_mut` and
//! `child_process::reactor::cp_reactor_scan_roots_mut` still own the socket
//! and ChildProcess values, still through `gc_register_mutable_root_scanner`.
//! That is deliberate — moving the *producer* off a thread must not move the
//! *roots*, or the phase would be two changes at once.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;

use turnloop::{Completion, Error, ErrorKind, Handle, OpId, OpResult, Token, WriteBuf};

pub(crate) mod adopt;
mod registry;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

pub use crate::turnloop_net::{map_error, NodeError};
pub(crate) use registry::{Owner, StreamEvent};

// ── Operation classes, in the top 8 bits of every submission token ──────────
//
// Disjoint from `turnloop_net`'s 1..=7 by construction: `owns()` is the only
// thing that decides which module a completion reaches, and it tests exactly
// this range.

/// Lowest operation class this module claims.
const CLASS_MIN: u64 = 0x10;
/// Highest operation class this module claims.
const CLASS_MAX: u64 = 0x1F;

const OP_READ: u64 = 0x11;
const OP_CLOSE: u64 = 0x13;
const OP_RECV: u64 = 0x14;
const OP_SEND: u64 = 0x15;
const OP_SIGNAL: u64 = 0x16;

/// The low 56 bits of a token hold the Perry-side id.
const ID_BITS: u32 = 56;
const ID_MASK: u64 = (1 << ID_BITS) - 1;

/// Bits of the 56-bit token id reserved for the minting agent.
///
/// The tables that hold these ids are thread-local, so before P9 — when only
/// the primary agent could own a loop — a plain per-thread counter was enough:
/// there was one minter. Now every JS agent can own a loop, and two agents
/// counting from 1 would both own an id `1`. That is harmless while every
/// lookup is same-thread (each finds its own entry), and a **silent misroute**
/// the moment one is not.
///
/// So the id carries its agent: agent N mints from `(N & 0xFFFF) << ID_AGENT_SHIFT`,
/// leaving each agent 2^40 ids inside the 56-bit field. A foreign id then MISSES
/// the table rather than aliasing an entry, which turns a misroute into an
/// error the caller can see. The primary agent is unchanged (band 0, ids from
/// 1), so nothing about a single-agent program moves.
const ID_AGENT_SHIFT: u32 = 40;
const ID_AGENT_MASK: u64 = 0xFFFF;

/// The first id this agent may mint, minus one.
fn agent_id_band() -> u64 {
    (crate::agent::current_agent() & ID_AGENT_MASK) << ID_AGENT_SHIFT
}

/// Take the next id for this thread's agent, seeding the band on first use.
fn mint_id(state: &mut ProcState) -> u64 {
    if state.next_id == 0 {
        state.next_id = agent_id_band();
    }
    state.next_id += 1;
    debug_assert!(state.next_id <= ID_MASK, "agent id band overflowed a token");
    state.next_id
}

fn token(op: u64, id: u64) -> Token {
    debug_assert!(id > 0 && id <= ID_MASK, "id {id} fits a token");
    debug_assert!((CLASS_MIN..=CLASS_MAX).contains(&op), "class {op} is P2's");
    Token((op << ID_BITS) | (id & ID_MASK))
}

fn token_parts(t: Token) -> (u64, u64) {
    (t.0 >> ID_BITS, t.0 & ID_MASK)
}

/// Whether this completion belongs to P2 rather than to P1's net subsystems.
///
/// The router in `agent_loop::dispatch_staged` asks this and nothing else, so
/// the two token spaces cannot overlap by accident: a class outside the P2
/// range is P1's by definition.
#[inline]
pub fn owns(t: Token) -> bool {
    (CLASS_MIN..=CLASS_MAX).contains(&(t.0 >> ID_BITS))
}

/// The `syscall` string Node reports for a failure of each operation class.
fn syscall_for(op: u64) -> &'static str {
    match op {
        OP_READ | OP_RECV => "read",
        OP_SEND => "write",
        OP_CLOSE => "close",
        OP_SIGNAL => "sigaction",
        _ => "",
    }
}

/// One write the caller handed over, still owned by the driver.
struct PendingWrite {
    /// Echoed back on the completion so the caller can fire its JS callback.
    /// Zero means "no callback"; it is never used for routing.
    user: u64,
    len: usize,
}

/// Everything P2 knows about one loop-owned descriptor.
///
/// Deliberately holds no JS value and no GC pointer: the owning subsystem
/// keeps its own JS-side record, under its own already-registered scanner.
struct Entry {
    handle: Handle,
    owner: Owner,
    /// A multishot `read_start`, for a stream.
    read_op: Option<OpId>,
    /// The single outstanding `recv`, for a datagram socket. UDP receive is
    /// single-shot in turnloop 0.1, so the sink rearms it per datagram.
    recv_op: Option<OpId>,
    /// Rearm the datagram receive after each completion. Cleared by `pause`
    /// and by close, so a paused socket stops consuming its pooled buffer.
    recv_armed: bool,
    writes: VecDeque<PendingWrite>,
    /// Bytes handed to the driver and not yet reported written.
    queued: usize,
    /// `close` was submitted; the entry survives until its `Closed` arrives.
    closing: bool,
    referenced: bool,
}

impl Entry {
    fn new(handle: Handle, owner: Owner) -> Self {
        Self {
            handle,
            owner,
            read_op: None,
            recv_op: None,
            recv_armed: false,
            writes: VecDeque::new(),
            queued: 0,
            closing: false,
            referenced: true,
        }
    }
}

#[derive(Default)]
struct ProcState {
    entries: HashMap<u64, Entry>,
    next_id: u64,
}

crate::perry_thread_local! {
    /// Per agent, like the loop itself. A descriptor belongs to the thread
    /// that adopted it; there is no cross-thread map to race on.
    static PROC: RefCell<ProcState> = RefCell::new(ProcState::default());
}

/// Number of live turnloop-backed P2 descriptors on this thread.
///
/// The assertion a test needs: "turnloop carried this child's stdout" is only
/// worth claiming if this was ever nonzero (DESIGN §11 — a benchmark must
/// assert its subject ran).
pub fn live_handles() -> usize {
    PROC.with(|state| state.borrow().entries.len())
}

/// Whether this thread can take the turnloop P2 path at all.
///
/// True on every thread that runs a JS agent's event loop, since turnloop P9
/// gave every agent a loop. False on a host where loop creation failed, and
/// on a second thread acting for an agent another thread already owns. A
/// caller that gets `false` keeps its existing thread-backed transport — the
/// P1 coexistence rule, unchanged.
pub fn available() -> bool {
    crate::event_pump::net_loop_available()
}

/// Errors reported to a caller before any completion exists.
pub type ProcResult<T> = Result<T, NodeError>;

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

// ── Adoption ────────────────────────────────────────────────────────────────

/// Hand an already-created descriptor to the loop and start reading it.
///
/// `owner` says who receives its completions. Returns the P2 id the caller
/// stores next to its own record; every later call names the descriptor by
/// that id, never by an fd, so nothing outside this module ever holds a raw
/// descriptor the driver owns.
///
/// On failure the transport is dropped, which closes the descriptor — the
/// caller must therefore treat a failure as "this descriptor is gone" and fall
/// back by recreating it, not by reusing the fd it handed over.
pub(crate) fn adopt_stream(transport: adopt::Transport, owner: Owner) -> ProcResult<u64> {
    let id = with_driver(|driver| {
        let detached = transport
            .into_detached()
            .map_err(|e| map_error(e, "open"))?;
        let handle = driver
            .attach(detached, Token(0))
            .map_err(|e| map_error(e, "open"))?;
        Ok(insert(handle, owner))
    })
    .unwrap_or_else(|| Err(no_loop()))?;
    Ok(id)
}

/// Descriptors this process has adopted onto a loop, over its whole life.
///
/// The "subject ran" counter (DESIGN §11): a live count answers "is turnloop
/// carrying anything *now*", which is zero by the time a program exits, so a
/// claim that a workload ran on turnloop needs the lifetime number instead.
/// It is reported on the `PERRY_LOOP_STATS=1` exit line.
static ADOPTED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Total descriptors adopted onto a loop since process start.
pub fn adopted_total() -> u64 {
    ADOPTED.load(std::sync::atomic::Ordering::Relaxed)
}

fn insert(handle: Handle, owner: Owner) -> u64 {
    ADOPTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    PROC.with(|state| {
        let mut state = state.borrow_mut();
        let id = mint_id(&mut state);
        state.entries.insert(id, Entry::new(handle, owner));
        id
    })
}

// ── Submission ──────────────────────────────────────────────────────────────

/// Start streaming a descriptor. Multishot (DESIGN D4): one submission yields
/// a completion per chunk until EOF, stop, cancel or error — which is exactly
/// what the deleted reader thread's `loop { pipe.read(&mut buf) }` was.
pub(crate) fn read_start(id: u64) -> ProcResult<()> {
    with_driver(|driver| {
        PROC.with(|state| {
            let mut state = state.borrow_mut();
            let entry = state
                .entries
                .get_mut(&id)
                .ok_or_else(|| not_found("read"))?;
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

/// Arm one datagram receive. UDP receive is single-shot in turnloop 0.1, so
/// the sink rearms after each datagram while `recv_armed` holds.
// Datagram-side surface of the P2 handle table: real, exercised by
// `turnloop_proc::tests`, and consumed in production only by
// `dgram_reactor`, which is `#[cfg(feature = "mod-dgram")]`. The gate
// stays LIVE in the configuration that has the consumer -- if
// `dgram_reactor` ever stops calling this, a `mod-dgram` build goes red.
#[cfg_attr(not(feature = "mod-dgram"), allow(dead_code))]
pub(crate) fn recv_start(id: u64) -> ProcResult<()> {
    with_driver(|driver| {
        PROC.with(|state| {
            let mut state = state.borrow_mut();
            let entry = state
                .entries
                .get_mut(&id)
                .ok_or_else(|| not_found("recv"))?;
            entry.recv_armed = true;
            arm_recv(driver, id, entry)
        })
    })
    .unwrap_or_else(|| Err(no_loop()))
}

// Datagram-side surface of the P2 handle table: real, exercised by
// `turnloop_proc::tests`, and consumed in production only by
// `dgram_reactor`, which is `#[cfg(feature = "mod-dgram")]`. The gate
// stays LIVE in the configuration that has the consumer -- if
// `dgram_reactor` ever stops calling this, a `mod-dgram` build goes red.
#[cfg_attr(not(feature = "mod-dgram"), allow(dead_code))]
fn arm_recv(driver: &mut turnloop::Loop, id: u64, entry: &mut Entry) -> ProcResult<()> {
    if entry.recv_op.is_some() || entry.closing || !entry.recv_armed {
        return Ok(());
    }
    let op = driver
        .recv(entry.handle, turnloop::ReadBuf::Pooled, token(OP_RECV, id))
        .map_err(|e| map_error(e, "recv"))?;
    entry.recv_op = Some(op);
    Ok(())
}

/// Send one datagram. `to` is `None` for a connected socket.
// Datagram-side surface of the P2 handle table: real, exercised by
// `turnloop_proc::tests`, and consumed in production only by
// `dgram_reactor`, which is `#[cfg(feature = "mod-dgram")]`. The gate
// stays LIVE in the configuration that has the consumer -- if
// `dgram_reactor` ever stops calling this, a `mod-dgram` build goes red.
#[cfg_attr(not(feature = "mod-dgram"), allow(dead_code))]
pub(crate) fn send_to(
    id: u64,
    bytes: Vec<u8>,
    to: Option<SocketAddr>,
    user: u64,
) -> ProcResult<usize> {
    with_driver(|driver| {
        PROC.with(|state| {
            let mut state = state.borrow_mut();
            let entry = state
                .entries
                .get_mut(&id)
                .ok_or_else(|| not_found("send"))?;
            if entry.closing {
                return Err(map_error(Error::new(ErrorKind::BrokenPipe), "send"));
            }
            let len = bytes.len();
            let buf = WriteBuf::Owned(bytes);
            let result = match to {
                Some(addr) => driver.send_to(entry.handle, buf, addr, token(OP_SEND, id)),
                None => driver.write(entry.handle, buf, token(OP_SEND, id)),
            };
            result.map_err(|e| map_error(e, "send"))?;
            entry.writes.push_back(PendingWrite { user, len });
            entry.queued += len;
            Ok(entry.queued)
        })
    })
    .unwrap_or_else(|| Err(no_loop()))
}

/// Subscribe this agent's loop to an OS signal. Returns the entry id the
/// caller stores and later passes to [`signal_stop`].
///
/// The id comes from the same monotonic allocator adopted descriptors use, and
/// deliberately *not* from the signal number: an `off()` immediately followed
/// by an `on()` for the same signal would otherwise reuse the id while the
/// first subscription's terminal completion is still in flight, and that
/// completion would then release the new entry instead of the old one.
pub(crate) fn signal_start(signal: turnloop::Signal, owner: Owner) -> ProcResult<u64> {
    with_driver(|driver| {
        let id = PROC.with(|state| mint_id(&mut state.borrow_mut()));
        let handle = driver
            .signal_start(signal, token(OP_SIGNAL, id))
            .map_err(|e| map_error(e, "sigaction"))?;
        PROC.with(|state| {
            state
                .borrow_mut()
                .entries
                .insert(id, Entry::new(handle, owner))
        });
        ADOPTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(id)
    })
    .unwrap_or_else(|| Err(no_loop()))
}

/// Unsubscribe from a signal. The handle survives until its terminal
/// completion, exactly like every other close here.
pub(crate) fn signal_stop(id: u64) {
    let _ = with_driver(|driver| {
        PROC.with(|state| {
            let mut state = state.borrow_mut();
            if let Some(entry) = state.entries.get_mut(&id) {
                if entry.closing {
                    return;
                }
                entry.closing = true;
                let _ = driver.signal_stop(entry.handle, token(OP_CLOSE, id));
            }
        })
    });
}

/// Include or exclude this descriptor from the loop's keep-alive count:
/// Node's `ref()` / `unref()`, on turnloop's own O(1) counter (DESIGN §8).
pub(crate) fn set_ref(id: u64, referenced: bool) {
    let _ = with_driver(|driver| {
        PROC.with(|state| {
            let mut state = state.borrow_mut();
            if let Some(entry) = state.entries.get_mut(&id) {
                if entry.referenced != referenced {
                    entry.referenced = referenced;
                    let _ = driver.set_ref(entry.handle, referenced);
                }
            }
        })
    });
}

/// Close a descriptor. The entry survives until the driver's final `Closed`
/// completion, which is the exactly-once release point (DESIGN D4) — the
/// owning subsystem is told then, and not before.
pub(crate) fn close(id: u64) {
    let submitted = with_driver(|driver| {
        PROC.with(|state| {
            let mut state = state.borrow_mut();
            let Some(entry) = state.entries.get_mut(&id) else {
                return None;
            };
            if entry.closing {
                return None;
            }
            entry.closing = true;
            entry.recv_armed = false;
            let owner = entry.owner;
            match driver.close(entry.handle, token(OP_CLOSE, id)) {
                Ok(()) => None,
                // The handle is already gone, so no completion can arrive.
                // Release here instead, outside the borrow, so the owner is
                // never left waiting for one — exactly-once, still once.
                Err(_) => {
                    state.entries.remove(&id);
                    Some(owner)
                }
            }
        })
    });
    if let Some(Some(owner)) = submitted {
        registry::deliver(owner, id, StreamEvent::Closed);
    }
}

/// Collect whatever the driver has ready for this thread, without blocking.
///
/// A subsystem's pump used to be self-sufficient: a thread had already pushed
/// the bytes onto the queue, so draining the queue was the whole job. A
/// completion-shaped transport is not like that — the bytes exist only once
/// the loop has been turned. A caller that drives a pump in a loop *without*
/// parking (the `await` poll loop, and the child-process lifecycle tests,
/// which is where this was caught) would otherwise spin against a queue
/// nothing can fill.
///
/// Costs nothing at all when this thread has adopted no descriptor: one
/// thread-local length read, no syscall.
#[inline]
pub(crate) fn drain_pending() {
    if PROC.with(|state| state.borrow().entries.is_empty()) {
        return;
    }
    crate::event_pump::settle_loop_once();
}

/// Close a descriptor and drive the loop until the driver has acknowledged it.
///
/// [`close`] alone is asynchronous, which is right for everything whose release
/// nothing observes — a child's pipe has already delivered EOF by the time it
/// is closed. A dgram socket is not that: `socket.close()` must leave the port
/// free, because the very next statement may bind it. The descriptor is
/// released only when the final `Closed` lands, so the close is driven to
/// completion here rather than left for whenever the loop next turns.
///
/// Bounded, and deliberately so: a turn that cannot run — re-entry from inside
/// a dispatch pass, or a thread with no loop — must not spin, and a completion
/// that never arrives must not hang a `close()`.
// Datagram-side surface of the P2 handle table: real, exercised by
// `turnloop_proc::tests`, and consumed in production only by
// `dgram_reactor`, which is `#[cfg(feature = "mod-dgram")]`. The gate
// stays LIVE in the configuration that has the consumer -- if
// `dgram_reactor` ever stops calling this, a `mod-dgram` build goes red.
#[cfg_attr(not(feature = "mod-dgram"), allow(dead_code))]
pub(crate) fn close_and_settle(id: u64) {
    close(id);
    for _ in 0..64 {
        if !PROC.with(|state| state.borrow().entries.contains_key(&id)) {
            return;
        }
        crate::event_pump::settle_loop_once();
    }
}

// ── Dispatch ────────────────────────────────────────────────────────────────

/// Route one completion to the subsystem that submitted it.
///
/// Called by `agent_loop::dispatch_staged` **after** `turn` has returned, out
/// of a staging buffer with no borrow held on the loop (DESIGN D1), so a
/// handler may run JS, allocate, collect and submit new work.
pub fn dispatch(completion: Completion) {
    let (op, id) = token_parts(completion.token);
    let Some((owner, event)) = translate(op, id, completion) else {
        return;
    };
    registry::deliver(owner, id, event);
}

/// Turn one driver completion into the owning subsystem's event, updating the
/// entry's bookkeeping on the way. `None` means "nothing to deliver": a stale
/// token, or a completion whose only effect is internal.
///
/// Anything that must touch the entry table *again* (releasing the entry on
/// `Closed`, rearming a datagram receive) is deferred to [`After`] and run
/// once the borrow is gone — a rearm has to call back into the driver, and
/// doing that under a live `RefCell` borrow is the re-entrancy bug DESIGN D1
/// exists to avoid.
fn translate(op: u64, id: u64, completion: Completion) -> Option<(Owner, StreamEvent)> {
    /// What still has to happen after the entry borrow is released.
    enum After {
        Nothing,
        /// The handle produced its final completion: drop the entry.
        Release,
        /// A single-shot datagram receive completed and should be rearmed.
        Rearm(Handle),
    }

    let terminal = completion.terminal;
    let (owner, event, after) = PROC.with(|state| {
        let mut state = state.borrow_mut();
        let entry = state.entries.get_mut(&id)?;
        let owner = entry.owner;
        let mut after = After::Nothing;
        let event = match completion.result {
            OpResult::Read { n, lease } => {
                if terminal {
                    entry.read_op = None;
                }
                let bytes = read_bytes(lease, n);
                if bytes.is_empty() {
                    return None;
                }
                StreamEvent::Data(bytes)
            }
            OpResult::RecvFrom { n, from, lease } => {
                entry.recv_op = None;
                if entry.recv_armed && !entry.closing {
                    after = After::Rearm(entry.handle);
                }
                StreamEvent::Datagram {
                    bytes: read_bytes(lease, n),
                    from,
                }
            }
            OpResult::Eof => {
                entry.read_op = None;
                StreamEvent::Eof
            }
            OpResult::Wrote(n) => {
                let pending = entry.writes.pop_front();
                let user = pending.as_ref().map(|w| w.user).unwrap_or(0);
                let len = pending.map(|w| w.len).unwrap_or(n);
                entry.queued = entry.queued.saturating_sub(len);
                StreamEvent::Wrote { user, len }
            }
            OpResult::Signal(_) => StreamEvent::Signal,
            OpResult::Closed => {
                after = After::Release;
                StreamEvent::Closed
            }
            OpResult::Stopped | OpResult::Cancelled => {
                // A stopped multishot read (`pause()`), or an operation the
                // close path already accounted for. Clearing the slot here is
                // what lets `resume()` rearm. A signal subscription's `Stopped`
                // *is* its terminal completion, so that one releases.
                match op {
                    OP_READ => entry.read_op = None,
                    OP_RECV => entry.recv_op = None,
                    OP_SIGNAL | OP_CLOSE => after = After::Release,
                    _ => {}
                }
                if matches!(after, After::Release) {
                    StreamEvent::Closed
                } else {
                    return None;
                }
            }
            OpResult::Err(err) => {
                let mut user = 0;
                match op {
                    OP_READ if terminal => entry.read_op = None,
                    OP_RECV => {
                        entry.recv_op = None;
                        if entry.recv_armed && !entry.closing {
                            after = After::Rearm(entry.handle);
                        }
                    }
                    OP_SEND => {
                        if let Some(pending) = entry.writes.pop_front() {
                            entry.queued = entry.queued.saturating_sub(pending.len);
                            user = pending.user;
                        }
                    }
                    _ => {}
                }
                StreamEvent::Error {
                    user,
                    error: map_error(err, syscall_for(op)),
                    terminal,
                }
            }
            _ => return None,
        };
        Some((owner, event, after))
    })?;

    match after {
        After::Nothing => {}
        After::Release => {
            PROC.with(|state| state.borrow_mut().entries.remove(&id));
        }
        After::Rearm(handle) => {
            let rearm = with_driver(|driver| {
                driver.recv(handle, turnloop::ReadBuf::Pooled, token(OP_RECV, id))
            });
            if let Some(Ok(op_id)) = rearm {
                PROC.with(|state| {
                    if let Some(entry) = state.borrow_mut().entries.get_mut(&id) {
                        entry.recv_op = Some(op_id);
                    }
                });
            }
        }
    }
    Some((owner, event))
}

/// Copy a pooled lease's bytes out before it returns to the driver's pool.
///
/// This copy is the whole GC story for P2 reads: the bytes the owner receives
/// are its own, so nothing borrows driver memory across a collection and there
/// is no buffer to root (see the module note).
fn read_bytes(lease: Option<turnloop::BufLease>, n: usize) -> Vec<u8> {
    let Some(lease) = lease else {
        return Vec::new();
    };
    let slice = lease.as_slice();
    let bytes = slice[..n.min(slice.len())].to_vec();
    lease.release();
    bytes
}

// ── Lifecycle ───────────────────────────────────────────────────────────────

/// Drop every entry without submitting anything: the loop is going away, so
/// there is nothing left to complete. Called from
/// `event_pump::agent_loop::shutdown_current_thread`.
pub fn shutdown_current_thread() {
    PROC.with(|state| state.borrow_mut().entries.clear());
}

#[cfg(test)]
pub(crate) fn reset_for_test() {
    PROC.with(|state| {
        let mut state = state.borrow_mut();
        state.entries.clear();
        state.next_id = 0;
    });
}
