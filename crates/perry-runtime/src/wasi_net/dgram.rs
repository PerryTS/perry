//! UDP transport for the real `node:dgram` sockets on WASI
//! (`crate::dgram_reactor` there; the native one is `dgram_reactor.rs`).
//!
//! A WASI guest has no threads and no turnloop, so the native transport (a
//! duplicate descriptor adopted by the agent's loop, with a reader thread as
//! fallback) has nothing to run on. This reactor follows the WASI TCP model
//! instead (`wasi_net/mod.rs`): the socket is a non-blocking wasi:sockets UDP
//! socket opened through std by [`crate::dgram::net`], and the event-loop pump
//! polls it. The JS-facing layer (`dgram.rs`, `dgram/*.rs`) is the same on
//! every target and calls this module through the native reactor's API.
//!
//! # Per pump
//!
//! [`pump`] (driven from `js_run_stdlib_pump` through `module_hooks`) first,
//! with the reactor state borrowed and no JS running: flushes each socket's
//! queued sends with `send_to` until one would block, then drains `recv_from`
//! until it would block. Every outcome becomes an event. Then it delivers the
//! events in order, each taken off the queue with the borrow released: a `'message'` (Buffer + rinfo)
//! in the bind-time async context, a send callback through
//! [`crate::dgram::complete_send`], a receive failure as `'error'`. JS that
//! runs from a delivery may send, close or bind again; whatever that produces
//! is delivered by the next pump. Microtasks run after each delivery, as
//! Node runs them after each callback.
//!
//! [`send_on_loop`] queues the datagram and flushes at once, so a send that
//! the host accepts leaves on the call; only its callback waits for the pump,
//! as it waits for the completion on the native loop.
//!
//! # Waking
//!
//! The event-loop park (`wasi_net::park`) waits on every open socket's
//! descriptor ([`interest`]: readable always, writable while sends queue), so
//! a datagram or a drained send buffer wakes the guest; [`next_wake_ms`] only
//! asks for no park while outcomes wait for delivery.
//!
//! # GC
//!
//! [`scan_roots_mut`] roots each socket object, its bind-time context, every
//! queued send's callback and context, and every undelivered send outcome's
//! callback and context. State is per thread, as `wasi_net`'s is: values never
//! cross agents, and an agent's sockets die with its thread.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::io::ErrorKind;
use std::net::{SocketAddr, UdpSocket};
use std::os::fd::AsRawFd;
use std::sync::Arc;

use crate::async_context::AsyncContextSnapshot;
use crate::turnloop_net::{errors, NodeError};

const RECV_CAP: usize = 65536;
/// Datagrams taken from one socket per pump, so a flooded socket cannot hold
/// the event loop inside one tick.
const RECV_BURST: usize = 1024;

/// A `send()` the host has not taken yet.
struct QueuedSend {
    bytes: Vec<u8>,
    dest: SocketAddr,
    /// NaN-boxed callback, zero without one. A GC root until delivered.
    callback_bits: u64,
    context: Option<AsyncContextSnapshot>,
}

struct LiveSocket {
    /// NaN-boxed dgram Socket object — a GC root.
    socket_bits: u64,
    udp: Arc<UdpSocket>,
    /// AsyncLocalStorage context active when the socket was bound.
    context: AsyncContextSnapshot,
    refed: bool,
    sends: VecDeque<QueuedSend>,
}

enum Event {
    Message {
        id: u64,
        data: Vec<u8>,
        src: SocketAddr,
    },
    Sent {
        id: u64,
        callback_bits: u64,
        context: Option<AsyncContextSnapshot>,
        outcome: Result<usize, NodeError>,
    },
    RecvError {
        id: u64,
        error: NodeError,
    },
}

#[derive(Default)]
struct State {
    sockets: HashMap<u64, LiveSocket>,
    events: VecDeque<Event>,
    next_id: u64,
    buf: Vec<u8>,
}

crate::perry_thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }

fn node_error(err: &std::io::Error, syscall: &'static str) -> NodeError {
    match err.raw_os_error() {
        Some(os) => errors::from_os(os, syscall),
        None => NodeError {
            code: crate::io_error::io_error_code(err),
            errno: 0,
            syscall,
        },
    }
}

fn would_block(err: &std::io::Error) -> bool {
    matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted)
}

/// Register a freshly bound non-blocking socket and return its id. The socket
/// starts `ref`'d.
pub(crate) fn register(socket_bits: u64, udp: Arc<UdpSocket>) -> u64 {
    // Captured before the borrow: capturing may allocate.
    let context = crate::async_context::capture_context();
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.next_id += 1;
        let id = s.next_id;
        s.sockets.insert(
            id,
            LiveSocket {
                socket_bits,
                udp,
                context,
                refed: true,
                sends: VecDeque::new(),
            },
        );
        id
    })
}

/// Send `socket`'s queued datagrams in order until the host would block.
fn flush(id: u64, socket: &mut LiveSocket, events: &mut VecDeque<Event>) {
    while let Some(send) = socket.sends.front() {
        let outcome = match socket.udp.send_to(&send.bytes, send.dest) {
            Ok(_) => Ok(send.bytes.len()),
            Err(err) if would_block(&err) => return,
            Err(err) => Err(node_error(&err, "send")),
        };
        let send = socket.sends.pop_front().expect("front exists");
        events.push_back(Event::Sent {
            id,
            callback_bits: send.callback_bits,
            context: send.context,
            outcome,
        });
    }
}

/// Queue one datagram and send what the host takes now. The callback (or the
/// `'error'` event of a failed send without one) is delivered by [`pump`].
pub(crate) fn send_on_loop(
    id: u64,
    bytes: Vec<u8>,
    dest: SocketAddr,
    callback_bits: u64,
) -> Result<(), SendRefusal> {
    let context = (callback_bits != 0).then(crate::async_context::capture_context);
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let State {
            sockets, events, ..
        } = &mut *s;
        let Some(socket) = sockets.get_mut(&id) else {
            return Err(SendRefusal::NotOnLoop(bytes));
        };
        socket.sends.push_back(QueuedSend {
            bytes,
            dest,
            callback_bits,
            context,
        });
        flush(id, socket, events);
        Ok(())
    })
}

/// Why [`send_on_loop`] did not take a datagram (the native reactor's type;
/// this reactor refuses only a socket it does not know).
pub(crate) enum SendRefusal {
    NotOnLoop(Vec<u8>),
    #[allow(dead_code)]
    Refused,
}

/// The socket behind `id`, for `address()` and the option setters.
pub(crate) fn udp_for(id: u64) -> Option<Arc<UdpSocket>> {
    STATE.with(|s| s.borrow().sockets.get(&id).map(|ls| ls.udp.clone()))
}

/// Sockets on this reactor: the "subject ran" counter of the native one.
pub fn turnloop_sockets() -> u64 {
    STATE.with(|s| s.borrow().sockets.len() as u64)
}

/// `close()`: the socket is closed and its port released now, as Node's
/// `close()` releases it before the next statement. Queued sends and
/// undelivered events for it are dropped: Node delivers nothing after
/// `'close'`.
pub(crate) fn unregister(id: u64) {
    let removed = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let socket = s.sockets.remove(&id);
        let (kept, dropped): (VecDeque<Event>, VecDeque<Event>) = std::mem::take(&mut s.events)
            .into_iter()
            .partition(|event| match event {
                Event::Message { id: e, .. }
                | Event::Sent { id: e, .. }
                | Event::RecvError { id: e, .. } => *e != id,
            });
        s.events = kept;
        (socket, dropped)
    });
    // Context snapshots are released outside the borrow.
    drop(removed);
}

/// `socket.ref()` / `socket.unref()`.
pub(crate) fn set_refed(id: u64, refed: bool) {
    STATE.with(|s| {
        if let Some(ls) = s.borrow_mut().sockets.get_mut(&id) {
            ls.refed = refed;
        }
    });
}

/// Poll every socket, then deliver what the poll produced (module note).
///
/// Events stay on the rooted queue until each is delivered, so a collection
/// run by one delivery still sees the callbacks of the next. Only the events
/// queued when the poll ended are delivered here: a callback that sends again
/// gets its own callback on the next pump, not in an unbounded loop in this one.
pub(crate) fn pump() {
    let due = STATE.with(|s| {
        let mut s = s.borrow_mut();
        if s.sockets.is_empty() {
            return s.events.len();
        }
        let State {
            sockets,
            events,
            buf,
            ..
        } = &mut *s;
        if buf.len() != RECV_CAP {
            buf.resize(RECV_CAP, 0);
        }
        for (&id, socket) in sockets.iter_mut() {
            flush(id, socket, events);
            for _ in 0..RECV_BURST {
                match socket.udp.recv_from(buf) {
                    Ok((n, src)) => events.push_back(Event::Message {
                        id,
                        data: buf[..n].to_vec(),
                        src,
                    }),
                    Err(err) if would_block(&err) => break,
                    Err(err) => {
                        events.push_back(Event::RecvError {
                            id,
                            error: node_error(&err, "recvmsg"),
                        });
                        break;
                    }
                }
            }
        }
        events.len()
    });
    for _ in 0..due {
        let Some(event) = STATE.with(|s| s.borrow_mut().events.pop_front()) else {
            break;
        };
        deliver(event);
        // Node drains microtasks after every callback. A send callback is
        // itself a microtask (`dgram::net::defer_send_callback`), so without
        // this checkpoint it would run after a `'message'` delivered later in
        // this pump: the peer's reply to a datagram would be handled before
        // the callback of the send that carried it.
        crate::promise::microtasks::js_promise_run_microtasks_checkpoint();
    }
}

/// The socket object and bind-time context of a socket still open.
fn live(id: u64) -> Option<(u64, AsyncContextSnapshot)> {
    STATE.with(|s| {
        s.borrow()
            .sockets
            .get(&id)
            .map(|ls| (ls.socket_bits, ls.context.clone()))
    })
}

fn deliver(event: Event) {
    match event {
        Event::Message { id, data, src } => {
            // Closed by an earlier delivery of this pump.
            let Some((socket_bits, context)) = live(id) else {
                return;
            };
            let previous = crate::async_context::enter_context(&context);
            crate::async_context::push_context_guard(
                crate::async_context::ContextGuardAction::RestoreSnapshot(previous),
            );
            crate::dgram::dgram_emit_message(socket_bits, &data, src);
            if let Some(action) = crate::async_context::pop_context_guard() {
                crate::async_context::apply_context_guard(action);
            }
        }
        Event::Sent {
            id,
            callback_bits,
            context,
            outcome,
        } => {
            let Some((socket_bits, _)) = live(id) else {
                return;
            };
            crate::dgram::complete_send(
                f64::from_bits(socket_bits),
                callback_bits,
                outcome,
                context,
            );
        }
        Event::RecvError { id, error } => {
            let Some((socket_bits, _)) = live(id) else {
                return;
            };
            crate::dgram::emit_socket_error(f64::from_bits(socket_bits), error);
        }
    }
}

/// Whether a bound + `ref`'d socket keeps the event loop alive.
pub(crate) fn has_active() -> bool {
    STATE.with(|s| s.borrow().sockets.values().any(|ls| ls.refed))
}

/// The park bound this reactor puts on the event loop: now while outcomes
/// wait for delivery, none otherwise (its sockets wake the park, [`interest`]).
pub(crate) fn next_wake_ms() -> f64 {
    STATE.with(|s| {
        if s.borrow().events.is_empty() {
            -1.0
        } else {
            0.0
        }
    })
}

/// Add every open socket to the park's wait set: readable always (the pump
/// drains every socket), writable while sends wait for the host.
pub(crate) fn interest(fds: &mut Vec<i32>, kinds: &mut Vec<u8>) {
    STATE.with(|s| {
        for ls in s.borrow().sockets.values() {
            fds.push(ls.udp.as_raw_fd());
            let write = if ls.sends.is_empty() {
                0
            } else {
                crate::turnloop_net::WRITE
            };
            kinds.push(crate::turnloop_net::READ | write);
        }
    })
}

/// Test probe: is socket `id` still registered?
#[doc(hidden)]
pub fn live_socket_registered_for_test(id: u64) -> bool {
    STATE.with(|s| s.borrow().sockets.contains_key(&id))
}

/// GC mutable-root scanner (module note).
pub(crate) fn scan_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let State {
            sockets, events, ..
        } = &mut *s;
        for ls in sockets.values_mut() {
            visitor.visit_nanbox_u64_slot(&mut ls.socket_bits);
            crate::async_context::scan_snapshot_roots_mut(&mut ls.context, visitor);
            for send in ls.sends.iter_mut() {
                scan_send_roots(&mut send.callback_bits, &mut send.context, visitor);
            }
        }
        for event in events.iter_mut() {
            if let Event::Sent {
                callback_bits,
                context,
                ..
            } = event
            {
                scan_send_roots(callback_bits, context, visitor);
            }
        }
    });
}

fn scan_send_roots(
    callback_bits: &mut u64,
    context: &mut Option<AsyncContextSnapshot>,
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
) {
    if *callback_bits != 0 {
        visitor.visit_nanbox_u64_slot(callback_bits);
    }
    if let Some(context) = context.as_mut() {
        crate::async_context::scan_snapshot_roots_mut(context, visitor);
    }
}
