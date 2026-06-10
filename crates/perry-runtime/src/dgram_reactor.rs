//! Async UDP reactor backing the real `node:dgram` sockets (#4911).
//!
//! Mirrors the `child_process` spawn reactor (`child_process::reactor`): each
//! bound socket gets a background thread that blocks on `recv_from` and pushes
//! raw `(id, bytes, src)` datagrams into a queue, calling
//! [`crate::event_pump::js_notify_main_thread`] so the event loop wakes
//! promptly. The main-thread [`pump`] (driven from `js_run_stdlib_pump`) drains
//! the queue, and `dgram.rs` turns each datagram into a `Buffer` + `rinfo` and
//! emits `'message'`. Background threads never touch JSValues — those live in
//! the main thread's arena — so they move only `Vec<u8>` + `SocketAddr`.
//!
//! The socket JSValue is kept reachable across ticks by [`scan_roots_mut`], a
//! registered GC mutable-root scanner, and a bound+`ref`'d socket keeps the
//! loop alive via [`has_active`] (matching Node, where an open socket holds the
//! process open until `close()`/`unref()`).

use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// Poll cadence for the recv loop. The socket carries a read timeout this long
/// so the thread periodically rechecks its `closing` flag and exits promptly
/// after `close()` without needing a platform-specific socket shutdown.
const RECV_POLL: Duration = Duration::from_millis(250);
const RECV_CAP: usize = 65536;

struct LiveSocket {
    /// NaN-boxed dgram Socket object — a GC root (see [`scan_roots_mut`]).
    socket_bits: u64,
    udp: Arc<UdpSocket>,
    closing: Arc<AtomicBool>,
    /// Whether this socket holds the event loop open (`ref`'d). `unref()`
    /// clears it; `ref()` sets it.
    refed: bool,
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

#[inline]
fn live_lock() -> std::sync::MutexGuard<'static, Option<HashMap<u64, LiveSocket>>> {
    LIVE.lock().unwrap_or_else(PoisonError::into_inner)
}

#[inline]
fn queue_lock() -> std::sync::MutexGuard<'static, Vec<Datagram>> {
    QUEUE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Register a freshly-bound socket: assign an id, store its `UdpSocket`, start
/// the recv thread, and return the id (stashed on the JS object so later method
/// calls can recover the socket). The socket starts `ref`'d.
pub(crate) fn register(socket_bits: u64, udp: Arc<UdpSocket>) -> u64 {
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let closing = Arc::new(AtomicBool::new(false));
    let _ = udp.set_read_timeout(Some(RECV_POLL));
    {
        let mut guard = live_lock();
        guard.get_or_insert_with(HashMap::new).insert(
            id,
            LiveSocket {
                socket_bits,
                udp: udp.clone(),
                closing: closing.clone(),
                refed: true,
            },
        );
    }
    LIVE_COUNT.fetch_add(1, Ordering::SeqCst);
    REFED_COUNT.fetch_add(1, Ordering::SeqCst);
    spawn_recv(id, udp, closing);
    id
}

fn spawn_recv(id: u64, udp: Arc<UdpSocket>, closing: Arc<AtomicBool>) {
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
    });
}

/// Recover the live `UdpSocket` for `id` (set/used by `dgram.rs` methods).
pub(crate) fn udp_for(id: u64) -> Option<Arc<UdpSocket>> {
    live_lock()
        .as_ref()
        .and_then(|map| map.get(&id).map(|ls| ls.udp.clone()))
}

/// Close + deregister a socket: signal its recv thread to exit and drop the
/// registry entry (the last `Arc<UdpSocket>` drop closes the OS socket).
pub(crate) fn unregister(id: u64) {
    let removed = {
        let mut guard = live_lock();
        guard.as_mut().and_then(|map| map.remove(&id))
    };
    if let Some(ls) = removed {
        ls.closing.store(true, Ordering::Release);
        LIVE_COUNT.fetch_sub(1, Ordering::SeqCst);
        if ls.refed {
            REFED_COUNT.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

/// `socket.ref()` / `socket.unref()` — toggle whether this socket holds the
/// event loop open.
pub(crate) fn set_refed(id: u64, refed: bool) {
    let mut guard = live_lock();
    if let Some(ls) = guard.as_mut().and_then(|map| map.get_mut(&id)) {
        if ls.refed != refed {
            ls.refed = refed;
            if refed {
                REFED_COUNT.fetch_add(1, Ordering::SeqCst);
            } else {
                REFED_COUNT.fetch_sub(1, Ordering::SeqCst);
            }
        }
    }
}

/// Drain queued datagrams and deliver each as a `'message'` event. Driven from
/// `js_run_stdlib_pump` every event-loop tick.
pub(crate) fn pump() {
    if LIVE_COUNT.load(Ordering::Relaxed) == 0 {
        return;
    }
    let datagrams = std::mem::take(&mut *queue_lock());
    for datagram in datagrams {
        // The socket may have been closed between recv and pump; skip if gone.
        let socket_bits = {
            let guard = live_lock();
            guard
                .as_ref()
                .and_then(|map| map.get(&datagram.id).map(|ls| ls.socket_bits))
        };
        let Some(socket_bits) = socket_bits else {
            continue;
        };
        let family = if datagram.src.is_ipv4() {
            "IPv4"
        } else {
            "IPv6"
        };
        crate::dgram::dgram_emit_message(
            socket_bits,
            &datagram.data,
            &datagram.src.ip().to_string(),
            datagram.src.port(),
            family,
        );
    }
}

/// Whether any bound + `ref`'d socket should keep the event loop alive — OR'd
/// into `js_stdlib_has_active_handles`.
pub(crate) fn has_active() -> bool {
    REFED_COUNT.load(Ordering::Relaxed) > 0
}

/// GC mutable-root scanner: keep every live socket object reachable across
/// collections and rewrite the stored pointer on evacuation.
pub(crate) fn scan_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    if LIVE_COUNT.load(Ordering::Relaxed) == 0 {
        return;
    }
    if let Some(map) = live_lock().as_mut() {
        for ls in map.values_mut() {
            visitor.visit_nanbox_u64_slot(&mut ls.socket_bits);
        }
    }
}
