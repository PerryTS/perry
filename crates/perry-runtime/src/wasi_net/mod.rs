//! WASIp2 stream transport for the shared Perry net ABI.
//!
//! wasi-sdk owns socket layouts and WASI imports. This module owns nonblocking
//! handles and native byte queues; the existing extension sink owns JS values.
//! Completions are delivered after releasing every state borrow, so sinks can
//! submit more work or close handles safely.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::ffi::{CStr, CString};
use std::net::SocketAddr;
use std::path::Path;
use std::time::{Duration, Instant};

#[path = "../turnloop_net/abi.rs"]
pub mod abi;
#[path = "../turnloop_net/errors.rs"]
pub(crate) mod errors;
#[path = "../turnloop_net/sink.rs"]
mod sink;
pub use errors::NodeError;
pub use sink::{register_sink, sink_installed, NetCompletion, MAX_SUBSYSTEMS};
type NetResult<T> = Result<T, NodeError>;

extern "C" {
    fn perry_wasi_tcp_open(
        host: *const i8,
        port: u16,
        listening: i32,
        backlog: i32,
        reuse_port: i32,
        nodelay: i32,
        pending: *mut i32,
    ) -> i32;
    fn perry_wasi_socket_nonblock(fd: i32) -> i32;
    fn perry_wasi_socket_connect_ready(fd: i32) -> i32;
    fn perry_wasi_socket_accept(fd: i32, nodelay: i32) -> i32;
    fn perry_wasi_socket_read(fd: i32, bytes: *mut u8, len: u32) -> i32;
    fn perry_wasi_socket_write(fd: i32, bytes: *const u8, len: u32) -> i32;
    fn perry_wasi_socket_shutdown(fd: i32) -> i32;
    fn perry_wasi_socket_close(fd: i32) -> i32;
    fn perry_wasi_socket_address(
        fd: i32,
        peer: i32,
        text: *mut i8,
        cap: u32,
        port: *mut u16,
    ) -> i32;
}

struct Write {
    bytes: Vec<u8>,
    offset: usize,
    user: u64,
}
struct Handle {
    fd: i32,
    subsystem: u8,
    listener: bool,
    accepting: bool,
    connecting: bool,
    connected_event: bool,
    reading: bool,
    nodelay: bool,
    referenced: bool,
    writes: VecDeque<Write>,
    shutdown: Option<u64>,
    write_closed: bool,
}
impl Handle {
    fn new(fd: i32, subsystem: u8) -> Self {
        Self {
            fd,
            subsystem,
            listener: false,
            accepting: false,
            connecting: false,
            connected_event: false,
            reading: false,
            nodelay: false,
            referenced: true,
            writes: VecDeque::new(),
            shutdown: None,
            write_closed: false,
        }
    }
    fn queued(&self) -> usize {
        self.writes.iter().map(|w| w.bytes.len() - w.offset).sum()
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            perry_wasi_socket_close(self.fd);
        }
    }
}
struct Timer {
    subsystem: u8,
    deadline: Option<Instant>,
}
enum Event {
    Completion(u8, NetCompletion, Vec<u8>),
    Accepted(u8, i64, i32),
}
#[derive(Default)]
struct State {
    handles: HashMap<i64, Handle>,
    timers: HashMap<i64, Timer>,
    events: VecDeque<Event>,
}
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }

fn would_block(result: i32) -> bool {
    result == -libc::EAGAIN || result == -libc::EWOULDBLOCK || result == -libc::EINTR
}
fn valid_registration(
    state: &State,
    id: i64,
    subsystem: u8,
    syscall: &'static str,
) -> NetResult<()> {
    if id <= 0
        || state.handles.contains_key(&id)
        || state.timers.contains_key(&id)
        || !sink_installed(subsystem)
    {
        return Err(errors::invalid_input(syscall));
    }
    Ok(())
}
fn initialize() {
    crate::stdlib_pump::js_register_aux_pump(pump);
    crate::stdlib_pump::js_register_aux_has_active(has_active);
}
pub fn available() -> bool {
    initialize();
    true
}

pub fn tcp_listen(
    id: i64,
    subsystem: u8,
    addr: SocketAddr,
    backlog: u32,
    reuse_port: bool,
    nodelay: bool,
) -> NetResult<()> {
    initialize();
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        valid_registration(&state, id, subsystem, "listen")?;
        let host = CString::new(addr.ip().to_string()).unwrap();
        let mut pending = 0;
        let fd = unsafe {
            perry_wasi_tcp_open(
                host.as_ptr(),
                addr.port(),
                1,
                backlog.min(i32::MAX as u32) as i32,
                reuse_port as i32,
                nodelay as i32,
                &mut pending,
            )
        };
        if fd < 0 {
            return Err(errors::from_os(-fd, "listen"));
        }
        let mut handle = Handle::new(fd, subsystem);
        handle.listener = true;
        handle.nodelay = nodelay;
        state.handles.insert(id, handle);
        Ok(())
    })
}
pub fn tcp_connect_host(
    id: i64,
    subsystem: u8,
    host: &str,
    port: u16,
    nodelay: bool,
) -> NetResult<()> {
    initialize();
    let host = CString::new(host).map_err(|_| errors::invalid_input("connect"))?;
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        valid_registration(&state, id, subsystem, "connect")?;
        let mut pending = 0;
        let fd = unsafe {
            perry_wasi_tcp_open(host.as_ptr(), port, 0, 0, 0, nodelay as i32, &mut pending)
        };
        if fd < 0 {
            return Err(errors::from_os(-fd, "connect"));
        }
        let mut handle = Handle::new(fd, subsystem);
        handle.connecting = pending != 0;
        handle.connected_event = true;
        state.handles.insert(id, handle);
        Ok(())
    })
}
pub fn pipe_listen(_: i64, _: u8, _: &Path, _: u32) -> NetResult<()> {
    Err(errors::unsupported("listen"))
}
pub fn pipe_connect(_: i64, _: u8, _: &Path) -> NetResult<()> {
    Err(errors::unsupported("connect"))
}
pub fn adopt_fd(id: i64, subsystem: u8, fd: i32) -> NetResult<()> {
    initialize();
    // Ownership is consumed even on registration failure.
    let handle = Handle::new(fd, subsystem);
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        valid_registration(&state, id, subsystem, "adopt")?;
        let result = unsafe { perry_wasi_socket_nonblock(fd) };
        if result < 0 {
            return Err(errors::from_os(-result, "adopt"));
        }
        state.handles.insert(id, handle);
        Ok(())
    })
}
fn with_handle<T>(
    id: i64,
    syscall: &'static str,
    f: impl FnOnce(&mut Handle) -> NetResult<T>,
) -> NetResult<T> {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        f(state
            .handles
            .get_mut(&id)
            .ok_or_else(|| errors::from_os(libc::EBADF, syscall))?)
    })
}
pub fn accept_start(id: i64) -> NetResult<()> {
    with_handle(id, "accept", |h| {
        if !h.listener {
            return Err(errors::invalid_input("accept"));
        }
        h.accepting = true;
        Ok(())
    })
}
pub fn read_start(id: i64) -> NetResult<()> {
    with_handle(id, "read", |h| {
        h.reading = true;
        Ok(())
    })
}
pub fn write(id: i64, bytes: Vec<u8>, user: u64) -> NetResult<usize> {
    with_handle(id, "write", |h| {
        if h.listener || h.write_closed {
            return Err(errors::from_os(libc::EPIPE, "write"));
        }
        h.writes.push_back(Write {
            bytes,
            offset: 0,
            user,
        });
        Ok(h.queued())
    })
}
pub fn shutdown(id: i64, user: u64) -> NetResult<()> {
    with_handle(id, "shutdown", |h| {
        if h.listener {
            return Err(errors::invalid_input("shutdown"));
        }
        if h.write_closed {
            return Err(errors::from_os(libc::EPIPE, "shutdown"));
        }
        h.write_closed = true;
        h.shutdown = Some(user);
        Ok(())
    })
}
pub fn close(id: i64) -> NetResult<()> {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if let Some(h) = state.handles.remove(&id) {
            state.events.push_back(Event::Completion(
                h.subsystem,
                NetCompletion::closed(id),
                Vec::new(),
            ));
        }
        state.timers.remove(&id);
        Ok(())
    })
}
pub fn set_ref(id: i64, referenced: bool) -> NetResult<()> {
    with_handle(id, "ref", |h| {
        h.referenced = referenced;
        Ok(())
    })
}
pub fn queued_bytes(id: i64) -> usize {
    STATE.with(|s| s.borrow().handles.get(&id).map_or(0, Handle::queued))
}
pub fn is_live(id: i64) -> bool {
    STATE.with(|s| s.borrow().handles.contains_key(&id))
}
pub fn live_handles() -> usize {
    STATE.with(|s| s.borrow().handles.len())
}
pub fn transfer(id: i64, subsystem: u8) -> NetResult<()> {
    if !sink_installed(subsystem) {
        return Err(errors::invalid_input("transfer"));
    }
    with_handle(id, "transfer", |h| {
        h.subsystem = subsystem;
        Ok(())
    })
}
pub fn timer_arm(id: i64, subsystem: u8, delay_ms: u64) -> NetResult<()> {
    initialize();
    if !sink_installed(subsystem) {
        return Err(errors::invalid_input("timer"));
    }
    let deadline = Instant::now()
        .checked_add(Duration::from_millis(delay_ms))
        .ok_or_else(|| errors::invalid_input("timer"))?;
    STATE.with(|s| {
        s.borrow_mut().timers.insert(
            id,
            Timer {
                subsystem,
                deadline: Some(deadline),
            },
        )
    });
    Ok(())
}
pub fn timer_cancel(id: i64) -> NetResult<()> {
    STATE.with(|s| s.borrow_mut().timers.remove(&id));
    Ok(())
}
pub fn timer_park(id: i64) -> NetResult<()> {
    STATE.with(|s| {
        if let Some(t) = s.borrow_mut().timers.get_mut(&id) {
            t.deadline = None;
        }
    });
    Ok(())
}
fn address(id: i64, peer: bool) -> Option<SocketAddr> {
    STATE.with(|s| {
        let s = s.borrow();
        let h = s.handles.get(&id)?;
        let mut text = [0i8; 64];
        let mut port = 0;
        let result = unsafe {
            perry_wasi_socket_address(
                h.fd,
                peer as i32,
                text.as_mut_ptr(),
                text.len() as u32,
                &mut port,
            )
        };
        if result < 0 {
            return None;
        }
        let ip = unsafe { CStr::from_ptr(text.as_ptr()) }
            .to_str()
            .ok()?
            .parse()
            .ok()?;
        Some(SocketAddr::new(ip, port))
    })
}
pub fn local_addr(id: i64) -> Option<SocketAddr> {
    address(id, false)
}
pub fn peer_addr(id: i64) -> Option<SocketAddr> {
    address(id, true)
}

extern "C" fn has_active() -> i32 {
    STATE.with(|s| {
        let s = s.borrow();
        i32::from(!s.events.is_empty() || s.handles.values().any(|h| h.referenced))
    })
}
/// Bound the legacy WASI event-loop park while sockets await readiness.
/// This also preserves timer deadlines; idle programs without sockets sleep normally.
pub(crate) fn next_wake_ms() -> f64 {
    STATE.with(|s| {
        let s = s.borrow();
        if !s.events.is_empty() {
            return 0.0;
        }
        let mut next = if s.handles.is_empty() {
            f64::INFINITY
        } else {
            1.0
        };
        let now = Instant::now();
        for timer in s.timers.values() {
            if let Some(d) = timer.deadline {
                next = next.min(d.saturating_duration_since(now).as_secs_f64() * 1000.0);
            }
        }
        if next.is_finite() {
            next
        } else {
            -1.0
        }
    })
}
fn completion(events: &mut VecDeque<Event>, subsystem: u8, c: NetCompletion) {
    events.push_back(Event::Completion(subsystem, c, Vec::new()));
}

extern "C" fn pump() -> i32 {
    let mut events = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let State {
            handles,
            timers,
            events,
        } = &mut *s;
        for (&id, h) in handles.iter_mut() {
            if h.connecting {
                let ready = unsafe { perry_wasi_socket_connect_ready(h.fd) };
                if ready == 0 {
                    continue;
                }
                h.connecting = false;
                if ready < 0 {
                    h.connected_event = false;
                    completion(
                        events,
                        h.subsystem,
                        NetCompletion::error(
                            id,
                            0,
                            h.queued(),
                            errors::from_os(-ready, "connect"),
                            true,
                        ),
                    );
                    continue;
                }
            }
            if h.connected_event {
                h.connected_event = false;
                completion(events, h.subsystem, NetCompletion::connect(id));
            }
            if h.accepting {
                for _ in 0..32 {
                    let fd = unsafe { perry_wasi_socket_accept(h.fd, h.nodelay as i32) };
                    if would_block(fd) {
                        break;
                    }
                    if fd < 0 {
                        completion(
                            events,
                            h.subsystem,
                            NetCompletion::error(id, 0, 0, errors::from_os(-fd, "accept"), false),
                        );
                        break;
                    }
                    events.push_back(Event::Accepted(h.subsystem, id, fd));
                }
            }
            while let Some(w) = h.writes.front_mut() {
                if w.offset != w.bytes.len() {
                    let remaining = &w.bytes[w.offset..];
                    let n = unsafe {
                        perry_wasi_socket_write(
                            h.fd,
                            remaining.as_ptr(),
                            remaining.len().min(i32::MAX as usize) as u32,
                        )
                    };
                    if would_block(n) || n == 0 {
                        break;
                    }
                    if n < 0 {
                        let user = w.user;
                        h.writes.pop_front();
                        completion(
                            events,
                            h.subsystem,
                            NetCompletion::error(
                                id,
                                user,
                                h.queued(),
                                errors::from_os(-n, "write"),
                                true,
                            ),
                        );
                        break;
                    }
                    w.offset += n as usize;
                    if w.offset != w.bytes.len() {
                        break;
                    }
                }
                let w = h.writes.pop_front().unwrap();
                completion(
                    events,
                    h.subsystem,
                    NetCompletion::wrote(id, w.user, w.bytes.len(), h.queued()),
                );
            }
            if h.writes.is_empty() {
                if let Some(user) = h.shutdown.take() {
                    let result = unsafe { perry_wasi_socket_shutdown(h.fd) };
                    completion(
                        events,
                        h.subsystem,
                        if result < 0 {
                            NetCompletion::error(
                                id,
                                user,
                                0,
                                errors::from_os(-result, "shutdown"),
                                true,
                            )
                        } else {
                            NetCompletion::shutdown(id, user)
                        },
                    );
                }
            }
            if h.reading {
                for _ in 0..32 {
                    let mut bytes = vec![0u8; 16 * 1024];
                    let n = unsafe {
                        perry_wasi_socket_read(h.fd, bytes.as_mut_ptr(), bytes.len() as u32)
                    };
                    if would_block(n) {
                        break;
                    }
                    if n <= 0 {
                        h.reading = false;
                        completion(
                            events,
                            h.subsystem,
                            if n == 0 {
                                NetCompletion::eof(id)
                            } else {
                                NetCompletion::error(
                                    id,
                                    0,
                                    h.queued(),
                                    errors::from_os(-n, "read"),
                                    true,
                                )
                            },
                        );
                        break;
                    }
                    bytes.truncate(n as usize);
                    events.push_back(Event::Completion(
                        h.subsystem,
                        NetCompletion::data(id, &bytes),
                        bytes,
                    ));
                }
            }
        }
        let now = Instant::now();
        timers.retain(|&id, t| {
            if t.deadline.is_some_and(|d| d <= now) {
                completion(events, t.subsystem, NetCompletion::timer(id));
                false
            } else {
                true
            }
        });
        std::mem::take(events)
    });
    let count = events.len().min(i32::MAX as usize) as i32;
    while let Some(event) = events.pop_front() {
        match event {
            Event::Completion(subsystem, c, bytes) => {
                if c.kind == sink::NET_CLOSED || c.kind == sink::NET_TIMER || is_live(c.id) {
                    sink::emit(subsystem, c);
                }
                drop(bytes); // The read payload stays alive through the sink call.
            }
            Event::Accepted(subsystem, server, fd) => {
                let handle = Handle::new(fd, subsystem);
                if !is_live(server) {
                    continue;
                }
                let Some(id) = sink::allocate_id(subsystem) else {
                    continue;
                };
                STATE.with(|s| s.borrow_mut().handles.insert(id, handle));
                sink::emit(subsystem, NetCompletion::accept(server, id, peer_addr(id)));
            }
        }
    }
    count
}
