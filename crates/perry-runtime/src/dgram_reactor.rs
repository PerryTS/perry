//! UDP transport for the real `node:dgram` sockets (#4911), on turnloop since
//! P2 (`docs/turnloop/p2-report.md`, DESIGN §12 "P2").
//!
//! # What this used to be
//!
//! Every bound socket got a background thread that blocked in `recv_from`
//! with a 250 ms read timeout — the timeout existing only so the thread could
//! periodically recheck a `closing` flag — pushed `(id, bytes, src)` onto a
//! global queue and called `js_notify_main_thread()`. `close()` then had to
//! send the socket an empty datagram to unblock its own reader and `join()`
//! the thread. Sends went straight out of the main thread on a blocking
//! socket.
//!
//! # What it is now
//!
//! The socket is still created and bound by [`crate::dgram::net`] with
//! `socket2`, because that is where Node's bind-time `SO_REUSEADDR` /
//! `SO_REUSEPORT` / `IPV6_V6ONLY` decisions live and where every post-bind
//! option setter (`addMembership`, `setMulticastInterface`, `setTTL`, …)
//! still acts. What changed is who waits: a **duplicate** of the descriptor
//! is adopted by this agent's `turnloop::Loop`, which carries the receives and
//! the sends as operations, and the thread is gone.
//!
//! `dup(2)` shares one open file description, so the retained `Arc<UdpSocket>`
//! names the same socket the driver is receiving on — `setsockopt` through it
//! is the same `setsockopt`, and `getsockname` answers about the same binding.
//! It is deliberately **never** read or written: turnloop sets `O_NONBLOCK` on
//! the description it adopts and the duplicate sees that too, so a blocking
//! `send_to` there would have become a silent `EWOULDBLOCK`. That is precisely
//! why sends moved to the driver as well, rather than only receives.
//!
//! The thread path survives as a fallback for an agent with no loop (a
//! `worker_threads` agent before P3/P4, or a host where loop creation
//! failed) — the P1 coexistence rule, unchanged.
//! [`uses_turnloop`] reports which path a socket actually took, so a test can
//! assert its subject ran instead of passing vacuously.
//!
//! # Ordering
//!
//! Received datagrams still land on [`QUEUE`] and are still drained by
//! [`pump`] from `js_run_stdlib_pump`. Only the producer changed — from a
//! thread to a completion delivered on the owning thread — so the tick at
//! which a `'message'` event fires, the AsyncLocalStorage context it restores
//! and the order of datagrams within a socket are all exactly what they were.
//!
//! # GC
//!
//! [`scan_roots_mut`] roots the socket object and the bind-time async context,
//! as before, and now also every **pending send callback**: a `send(msg, cb)`
//! whose completion has not arrived holds `cb` in [`PendingSend`], which is a
//! JS value this registry owns from submit to completion (DESIGN D3/D4). The
//! datagram bytes themselves are never JS memory — they are copied out of
//! turnloop's pooled lease inside dispatch, into the same `Vec<u8>` the thread
//! used to push.

use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use crate::turnloop_proc::{Owner, StreamEvent};

/// Poll cadence for the fallback recv loop. Only the thread path uses it.
const RECV_POLL: Duration = Duration::from_millis(250);
const RECV_CAP: usize = 65536;

/// A `send()` handed to the driver and not yet completed.
struct PendingSend {
    /// NaN-boxed completion callback — a GC root until the completion fires
    /// (see [`scan_roots_mut`]). Zero when `send()` was called without one.
    callback_bits: u64,
    len: usize,
}

struct LiveSocket {
    /// NaN-boxed dgram Socket object — a GC root (see [`scan_roots_mut`]).
    socket_bits: u64,
    /// The socket as Perry keeps it. On the turnloop path this is the
    /// duplicate retained for `setsockopt`/`getsockname` only; on the fallback
    /// path it is the socket the reader thread blocks on.
    udp: Arc<UdpSocket>,
    /// Entry id in [`crate::turnloop_proc`], or `None` on the thread path.
    proc_id: Option<u64>,
    closing: Arc<AtomicBool>,
    recv_thread: Option<std::thread::JoinHandle<()>>,
    /// AsyncLocalStorage context active when the UDP handle was bound.
    context: crate::async_context::AsyncContextSnapshot,
    /// Whether this socket holds the event loop open (`ref`'d). `unref()`
    /// clears it; `ref()` sets it.
    refed: bool,
    /// In-flight sends, keyed by the token echoed back on the completion.
    sends: HashMap<u64, PendingSend>,
    next_send: u64,
}

struct Datagram {
    id: u64,
    data: Vec<u8>,
    src: SocketAddr,
}

static LIVE: Mutex<Option<HashMap<u64, LiveSocket>>> = Mutex::new(None);
static QUEUE: Mutex<Vec<Datagram>> = Mutex::new(Vec::new());
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
/// Number of bound + `ref`'d sockets — lock-free fast path for [`has_active`].
static REFED_COUNT: AtomicU64 = AtomicU64::new(0);
/// Number of registered sockets (any ref state) — fast path for [`pump`] /
/// [`scan_roots_mut`].
static LIVE_COUNT: AtomicU64 = AtomicU64::new(0);
/// Sockets currently receiving on turnloop rather than on a thread. The
/// "subject ran" counter: a dgram assertion about turnloop is only worth
/// making if this was nonzero (DESIGN §11).
static TURNLOOP_COUNT: AtomicU64 = AtomicU64::new(0);

#[inline]
fn live_lock() -> std::sync::MutexGuard<'static, Option<HashMap<u64, LiveSocket>>> {
    LIVE.lock().unwrap_or_else(PoisonError::into_inner)
}

#[inline]
fn queue_lock() -> std::sync::MutexGuard<'static, Vec<Datagram>> {
    QUEUE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Register a freshly-bound socket: assign an id, adopt a duplicate of its
/// descriptor onto this agent's loop (falling back to a reader thread when
/// there is no loop), and return the id — which `dgram.rs` stashes on the JS
/// object so later method calls can recover the socket. The socket starts
/// `ref`'d.
pub(crate) fn register(socket_bits: u64, udp: Arc<UdpSocket>) -> u64 {
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let closing = Arc::new(AtomicBool::new(false));
    let context = crate::async_context::capture_context();

    let proc_id = adopt_on_loop(id, &udp);
    let recv_thread = if proc_id.is_some() {
        None
    } else {
        // No loop on this thread: keep the pre-P2 transport verbatim.
        let _ = udp.set_read_timeout(Some(RECV_POLL));
        Some(spawn_recv(id, udp.clone(), closing.clone()))
    };

    {
        let mut guard = live_lock();
        guard.get_or_insert_with(HashMap::new).insert(
            id,
            LiveSocket {
                socket_bits,
                udp: udp.clone(),
                proc_id,
                closing: closing.clone(),
                recv_thread,
                context,
                refed: true,
                sends: HashMap::new(),
                next_send: 0,
            },
        );
    }
    LIVE_COUNT.fetch_add(1, Ordering::SeqCst);
    REFED_COUNT.fetch_add(1, Ordering::SeqCst);
    if proc_id.is_some() {
        TURNLOOP_COUNT.fetch_add(1, Ordering::SeqCst);
    }
    id
}

/// Adopt a duplicate of the bound socket onto the loop and arm its receive.
///
/// Returns `None` — meaning "use the thread" — when this agent has no loop, or
/// when duplicating or attaching failed. A failure here must not fail the
/// bind: the socket is already bound and the fallback transport is still
/// correct, just thread-backed.
fn adopt_on_loop(id: u64, udp: &UdpSocket) -> Option<u64> {
    if !crate::turnloop_proc::available() {
        return None;
    }
    #[cfg(unix)]
    let transport = {
        use std::os::fd::AsFd;
        crate::turnloop_proc::adopt::duplicate_fd(udp.as_fd()).ok()?
    };
    #[cfg(windows)]
    let transport = {
        use std::os::windows::io::AsSocket;
        crate::turnloop_proc::adopt::duplicate_socket(udp.as_socket()).ok()?
    };
    #[cfg(not(any(unix, windows)))]
    let transport = {
        let _ = udp;
        return None;
    };

    let proc_id =
        crate::turnloop_proc::adopt_stream(transport, Owner::Dgram { socket: id }).ok()?;
    if crate::turnloop_proc::recv_start(proc_id).is_err() {
        crate::turnloop_proc::close(proc_id);
        return None;
    }
    Some(proc_id)
}

fn spawn_recv(
    id: u64,
    udp: Arc<UdpSocket>,
    closing: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buf = [0u8; RECV_CAP];
        loop {
            if closing.load(Ordering::Acquire) {
                break;
            }
            match udp.recv_from(&mut buf) {
                Ok((n, src)) => {
                    queue_lock().push(Datagram {
                        id,
                        data: buf[..n].to_vec(),
                        src,
                    });
                    crate::event_pump::js_notify_main_thread();
                }
                Err(err) => match err.kind() {
                    // Read-timeout tick: loop back and recheck `closing`.
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => continue,
                    // Transient interruption — keep going.
                    std::io::ErrorKind::Interrupted => continue,
                    // Socket torn down / fatal — exit the thread.
                    _ => break,
                },
            }
        }
    })
}

/// One turnloop completion for a dgram socket, delivered on the owning thread
/// by `turnloop_proc::dispatch`.
///
/// A received datagram goes onto the same [`QUEUE`] the thread pushed to and
/// is emitted by [`pump`] on the next tick — the producer moved, the delivery
/// point did not. A completed send fires its JS callback the same way the
/// synchronous path did, through a microtask, so only *when the outcome is
/// known* changed, not how it is reported.
pub(crate) fn on_completion(id: u64, event: StreamEvent) {
    match event {
        StreamEvent::Datagram { bytes, from } => {
            // A datagram the driver had already received when `close()` ran.
            // Node delivers nothing after `'close'`.
            if live_lock()
                .as_ref()
                .and_then(|map| map.get(&id))
                .is_none_or(is_closing)
            {
                return;
            }
            queue_lock().push(Datagram {
                id,
                data: bytes,
                src: from,
            });
            // Preserve the pre-P2 contract exactly: the producer used to be a
            // thread that had to wake the loop, and the waiter's fast path is
            // still what carries this tick into `js_run_stdlib_pump`.
            crate::event_pump::js_notify_main_thread();
        }
        StreamEvent::Wrote { user, len, .. } => {
            let Some(pending) = take_send(id, user) else {
                return;
            };
            let socket_bits = match socket_bits_for(id) {
                Some(bits) => bits,
                None => return,
            };
            crate::dgram::complete_send(
                f64::from_bits(socket_bits),
                pending.callback_bits,
                Ok(pending.len.max(len)),
            );
        }
        StreamEvent::Error { user, error, .. } => {
            let pending = take_send(id, user);
            let Some(socket_bits) = socket_bits_for(id) else {
                return;
            };
            let socket = f64::from_bits(socket_bits);
            match pending {
                // A failed send: report it exactly where a synchronous failure
                // was reported — the callback if there is one, otherwise an
                // `'error'` event.
                Some(pending) => {
                    crate::dgram::complete_send(socket, pending.callback_bits, Err(error))
                }
                // A receive-side failure. Node surfaces these on the socket.
                None => crate::dgram::emit_socket_error(socket, error),
            }
        }
        StreamEvent::Closed => {
            drop_entry(id);
        }
        // A dgram socket has no stream half-close and no EOF; the remaining
        // variants cannot name one.
        StreamEvent::Data(_) | StreamEvent::Eof | StreamEvent::Signal => {}
    }
}

fn take_send(id: u64, user: u64) -> Option<PendingSend> {
    if user == 0 {
        return None;
    }
    live_lock()
        .as_mut()
        .and_then(|map| map.get_mut(&id))
        .and_then(|ls| ls.sends.remove(&user))
}

fn socket_bits_for(id: u64) -> Option<u64> {
    live_lock()
        .as_ref()
        .and_then(|map| map.get(&id).map(|ls| ls.socket_bits))
}

/// Queue one datagram on the loop.
///
/// Returns [`SendRefusal::NotOnLoop`] — carrying the bytes back — when this
/// socket is thread-backed, so the caller falls through to the synchronous
/// send it used before P2. Handing the buffer back rather than reporting a
/// failure is the whole point: a thread-backed socket must still be able to
/// send, and a dropped `Vec` here would silently lose the datagram. The two
/// refusals are separate variants rather than an empty-buffer sentinel,
/// because a zero-length datagram is a real datagram Node can send.
pub(crate) fn send_on_loop(
    id: u64,
    bytes: Vec<u8>,
    dest: SocketAddr,
    callback_bits: u64,
) -> Result<(), SendRefusal> {
    let (proc_id, user) = {
        let mut guard = live_lock();
        let Some(ls) = guard.as_mut().and_then(|map| map.get_mut(&id)) else {
            return Err(SendRefusal::NotOnLoop(bytes));
        };
        let Some(proc_id) = ls.proc_id else {
            return Err(SendRefusal::NotOnLoop(bytes));
        };
        ls.next_send += 1;
        let user = ls.next_send;
        ls.sends.insert(
            user,
            PendingSend {
                callback_bits,
                len: bytes.len(),
            },
        );
        (proc_id, user)
    };
    // Always `send_to`, never a connected-socket `write`: Node's
    // `socket.connect()` is bookkeeping in Perry (`dgram/ops.rs` sets hidden
    // fields and never calls `connect(2)`), so the descriptor has no default
    // peer and a write would fail with EDESTADDRREQ.
    match crate::turnloop_proc::send_to(proc_id, bytes, Some(dest), user) {
        Ok(_) => Ok(()),
        Err(_) => {
            // The submission never reached the driver, so no completion will
            // name this token; release the rooted callback here rather than
            // leaking it. The bytes are gone with the refused submission, so
            // the caller is told the send failed, not asked to retry.
            let _ = take_send(id, user);
            Err(SendRefusal::Refused)
        }
    }
}

/// Why [`send_on_loop`] did not take a datagram.
pub(crate) enum SendRefusal {
    /// This socket is thread-backed; send synchronously with these bytes.
    NotOnLoop(Vec<u8>),
    /// The driver refused the submission and the datagram went with it.
    Refused,
}

/// Recover the live `UdpSocket` for `id` (set/used by `dgram.rs` methods).
///
/// On the turnloop path this is the retained duplicate: correct for
/// `setsockopt` and `getsockname`, never to be read or written (see the module
/// note on the shared `O_NONBLOCK`).
pub(crate) fn udp_for(id: u64) -> Option<Arc<UdpSocket>> {
    live_lock()
        .as_ref()
        .and_then(|map| map.get(&id).map(|ls| ls.udp.clone()))
}

/// Number of dgram sockets currently receiving on turnloop rather than on a
/// thread. The "subject ran" counter for a dgram claim, reported on the
/// `PERRY_LOOP_STATS=1` exit line.
pub fn turnloop_sockets() -> u64 {
    TURNLOOP_COUNT.load(Ordering::Relaxed)
}

/// Close + deregister a socket.
///
/// On the turnloop path the close is submitted and the registry entry survives
/// until the driver's final `Closed` completion, which is the exactly-once
/// release point (DESIGN D4) — so the retained duplicate is dropped there, in
/// [`drop_entry`], not here. On the thread path this still signals the reader
/// and joins it, as before.
pub(crate) fn unregister(id: u64) {
    let proc_id = {
        let guard = live_lock();
        guard
            .as_ref()
            .and_then(|map| map.get(&id).and_then(|ls| ls.proc_id))
    };
    if let Some(proc_id) = proc_id {
        // Stop keeping the loop alive immediately — `close()` must not hold
        // the process open for the length of its own teardown — but leave the
        // entry in place for the completion.
        //
        // Marking it closing is not bookkeeping: the entry now outlives
        // `close()` by however long the driver takes to acknowledge, and the
        // pre-P2 code relied on the entry *vanishing* here to stop delivery.
        // Without this flag a datagram queued before `close()` would reach
        // `pump` afterwards and emit `'message'` on a socket JS has already
        // seen `'close'` for.
        mark_closing(id);
        release_refcount(id);
        // Settled, not merely submitted: Node's `close()` releases the port,
        // and `node-suite/dgram/multicast/reuse-address-cleanup` binds a fresh
        // socket to it on the next statement. An asynchronous release turns
        // that into EADDRINUSE.
        crate::turnloop_proc::close_and_settle(proc_id);
        return;
    }
    let removed = {
        let mut guard = live_lock();
        guard.as_mut().and_then(|map| map.remove(&id))
    };
    if let Some(mut ls) = removed {
        ls.closing.store(true, Ordering::Release);
        wake_receiver(&ls.udp);
        if let Some(thread) = ls.recv_thread.take() {
            let _ = thread.join();
        }
        LIVE_COUNT.fetch_sub(1, Ordering::SeqCst);
        if ls.refed {
            REFED_COUNT.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

/// Mark a socket as closing so [`pump`] stops delivering for it, matching the
/// pre-P2 behaviour where `close()` removed the registry entry outright.
fn mark_closing(id: u64) {
    let guard = live_lock();
    if let Some(ls) = guard.as_ref().and_then(|map| map.get(&id)) {
        ls.closing.store(true, Ordering::Release);
    }
}

fn is_closing(ls: &LiveSocket) -> bool {
    ls.closing.load(Ordering::Acquire)
}

/// Stop a closing socket from holding the loop open, without removing it.
fn release_refcount(id: u64) {
    let mut guard = live_lock();
    if let Some(ls) = guard.as_mut().and_then(|map| map.get_mut(&id)) {
        if ls.refed {
            ls.refed = false;
            REFED_COUNT.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

/// Drop the registry entry after the driver's final completion. Releases the
/// retained duplicate descriptor and any send callback that will now never
/// complete.
fn drop_entry(id: u64) {
    let removed = {
        let mut guard = live_lock();
        guard.as_mut().and_then(|map| map.remove(&id))
    };
    if let Some(ls) = removed {
        LIVE_COUNT.fetch_sub(1, Ordering::SeqCst);
        if ls.refed {
            REFED_COUNT.fetch_sub(1, Ordering::SeqCst);
        }
        if ls.proc_id.is_some() {
            TURNLOOP_COUNT.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

fn wake_receiver(udp: &UdpSocket) {
    let Ok(local) = udp.local_addr() else {
        return;
    };
    let wake = if local.is_ipv4() {
        SocketAddr::from(([127, 0, 0, 1], local.port()))
    } else {
        SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], local.port()))
    };
    let _ = udp.send_to(&[], wake);
}

/// `socket.ref()` / `socket.unref()` — toggle whether this socket holds the
/// event loop open. Mirrored into turnloop's own O(1) keep-alive counter so
/// `Loop::alive()` agrees with `js_stdlib_has_active_handles` (DESIGN §8).
pub(crate) fn set_refed(id: u64, refed: bool) {
    let proc_id = {
        let mut guard = live_lock();
        let Some(ls) = guard.as_mut().and_then(|map| map.get_mut(&id)) else {
            return;
        };
        if ls.refed != refed {
            ls.refed = refed;
            if refed {
                REFED_COUNT.fetch_add(1, Ordering::SeqCst);
            } else {
                REFED_COUNT.fetch_sub(1, Ordering::SeqCst);
            }
        }
        ls.proc_id
    };
    if let Some(proc_id) = proc_id {
        crate::turnloop_proc::set_ref(proc_id, refed);
    }
}

/// Drain queued datagrams and deliver each as a `'message'` event. Driven from
/// `js_run_stdlib_pump` every event-loop tick.
pub(crate) fn pump() {
    if LIVE_COUNT.load(Ordering::Relaxed) == 0 {
        return;
    }
    // A datagram exists on the queue only once the loop has been turned; a
    // caller that drives this pump without parking must not spin against a
    // queue nothing can fill (see `turnloop_proc::drain_pending`).
    crate::turnloop_proc::drain_pending();
    let datagrams = std::mem::take(&mut *queue_lock());
    for datagram in datagrams {
        // The socket may have been closed between recv and pump; skip if gone.
        let live = {
            let guard = live_lock();
            guard.as_ref().and_then(|map| {
                map.get(&datagram.id)
                    .filter(|ls| !is_closing(ls))
                    .map(|ls| (ls.socket_bits, ls.context.clone()))
            })
        };
        let Some((socket_bits, context)) = live else {
            continue;
        };
        let family = if datagram.src.is_ipv4() {
            "IPv4"
        } else {
            "IPv6"
        };
        let previous = crate::async_context::enter_context(&context);
        crate::async_context::push_context_guard(
            crate::async_context::ContextGuardAction::RestoreSnapshot(previous),
        );
        crate::dgram::dgram_emit_message(
            socket_bits,
            &datagram.data,
            &datagram.src.ip().to_string(),
            datagram.src.port(),
            family,
        );
        if let Some(action) = crate::async_context::pop_context_guard() {
            crate::async_context::apply_context_guard(action);
        }
    }
}

/// Whether any bound + `ref`'d socket should keep the event loop alive — OR'd
/// into `js_stdlib_has_active_handles`.
pub(crate) fn has_active() -> bool {
    REFED_COUNT.load(Ordering::Relaxed) > 0
}

/// GC mutable-root scanner: keep every live socket object, its bind-time async
/// context and every in-flight send callback reachable across collections, and
/// rewrite the stored pointers on evacuation.
pub(crate) fn scan_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    if LIVE_COUNT.load(Ordering::Relaxed) == 0 {
        return;
    }
    if let Some(map) = live_lock().as_mut() {
        for ls in map.values_mut() {
            visitor.visit_nanbox_u64_slot(&mut ls.socket_bits);
            crate::async_context::scan_snapshot_roots_mut(&mut ls.context, visitor);
            for pending in ls.sends.values_mut() {
                if pending.callback_bits != 0 {
                    visitor.visit_nanbox_u64_slot(&mut pending.callback_bits);
                }
            }
        }
    }
}
