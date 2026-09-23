//! The `node:tls` server's sockets on turnloop handles (turnloop P8 lane L).
//!
//! What this replaced, one for one:
//!
//! | tokio | turnloop |
//! |---|---|
//! | a spawned accept loop per `listen()`, `select!`ing on a shutdown `oneshot` | `tcp_listen` + one multishot `accept_start`; `close()` closes the listener |
//! | a `tokio::spawn` per connection running `TlsStream::accept` | a [`TlsSession`] per accepted handle, fed from its multishot read |
//! | `run_tls_socket_task`, `select!`ing on a read and a per-socket `mpsc` | the same session, driven from this module's completion sink; `write` / `end` / `destroy` submit where the JS call happens |
//! | a 1 ms `tokio::time::sleep` before `'close'` | an unreferenced `timer_arm` deadline |
//!
//! Everything above the transport is untouched: the same [`PendingTlsEvent`]s
//! in the same order, into the same queue, drained by the same
//! `js_tls_process_pending`, against the same registries. The handshake
//! outcomes keep the texts the tokio path produced (`"tls handshake: …"`,
//! rustls's own failure message, `"tls handshake eof"`, and rustls's
//! unexpected-EOF message for TCP EOF without `close_notify`), because they
//! reach JS as `'tlsClientError'` / `'error'` messages.
//!
//! # Threads
//!
//! Every function here that touches the loop runs on the thread that owns
//! this agent's loop — inline when that is the caller (every JS thread, since
//! turnloop P9), otherwise posted to the owner through
//! `perry_ffi::agent_post`, which serves the same JS heap (the P1 rule
//! perry-ext-net's `turnloop_io::on_loop` follows). Connection state is
//! therefore a thread-local of the owner, and the sink runs no JS: it only
//! pushes events, exactly as the tokio tasks did.
//!
//! # Ids
//!
//! turnloop names every socket, listener and deadline by one id per thread,
//! across every subsystem, so these cannot reuse the JS-visible TLS handle ids
//! (`0x70000…`), which overlap other bindings' handle spaces. A handle maps
//! into a band of its own instead ([`ID_BAND`], above the SMTP client's), and
//! back again by subtraction: no side table.

use std::cell::RefCell;
use std::collections::HashMap;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use perry_ffi::agent_post::{self, AgentJob};
use perry_runtime::turnloop_net as tl;
use perry_tls_session::TlsSession;

use super::{
    listeners, liveness, next_tls_handle_id, push_tls_event, servers, socket_api, sockets,
    tls_server_connection_finished, tls_server_connection_started, DynamicCertResolver,
    PendingTlsEvent, TlsSocketState,
};

/// This module's completion-sink slot. perry-ext-net owns 0, perry-ext-http
/// 1 and 6, this crate's HTTP client 2, SMTP client 3 and framework server 5,
/// perry-ext-ws 7 and 8, the database bindings 9..=12
/// (`perry-db-turnloop::subsystem`), and perry-runtime's own tests use 13.
pub(super) const SUBSYSTEM: u8 = 14;

/// Base of the turnloop id band for listeners and connections: `ID_BAND +
/// handle`. Above the SMTP client's `[1 << 45, 1 << 50)`.
const ID_BAND: i64 = 1 << 50;
/// Base of the band for the per-socket `'close'` deadlines.
const TIMER_BAND: i64 = ID_BAND + (1 << 48);
/// End of both bands; `turnloop_net` tokens carry 56 bits of id.
const BAND_END: i64 = ID_BAND + (1 << 49);

/// rustls's message for TCP EOF without `close_notify` — what the tokio path's
/// `TlsStream` read reported, and so what `'error'` carried.
const UNEXPECTED_EOF_MESSAGE: &str = "peer closed connection without sending TLS close_notify: \
https://docs.rs/rustls/latest/rustls/manual/_03_howto/index.html#unexpected-eof";

/// Node's backlog default (`net.Server.listen`).
const BACKLOG: u32 = 511;

pub(super) fn tl_id(handle: i64) -> i64 {
    ID_BAND + handle
}

fn timer_id(handle: i64) -> i64 {
    TIMER_BAND + handle
}

fn handle_of(id: i64) -> Option<i64> {
    (ID_BAND..TIMER_BAND).contains(&id).then(|| id - ID_BAND)
}

fn timer_handle_of(id: i64) -> Option<i64> {
    (TIMER_BAND..BAND_END)
        .contains(&id)
        .then(|| id - TIMER_BAND)
}

/// What an accepted connection is handshaken with — captured at `listen()`,
/// as the tokio accept loop captured it.
pub(super) struct Acceptor {
    pub(super) config: Arc<rustls::ServerConfig>,
    pub(super) cert_resolver: Option<Arc<DynamicCertResolver>>,
    pub(super) allow_half_open: bool,
}

#[derive(PartialEq, Eq)]
enum Phase {
    /// Accepted; the server handshake is still running.
    Handshaking,
    /// Handshake done: the socket is registered and delivers data.
    Open,
    /// Terminal events have been pushed; only the transport's own teardown
    /// (the shutdown, then the close) is left.
    Ended,
}

struct Conn {
    server_id: i64,
    session: TlsSession,
    phase: Phase,
    original_servername: Option<String>,
    cert_resolver: Option<Arc<DynamicCertResolver>>,
    allow_half_open: bool,
    local_addr: Option<SocketAddr>,
    peer_addr: Option<SocketAddr>,
    /// `close_notify` has been queued and the write side shut down.
    write_closed: bool,
    /// Close the handle once the write-side shutdown completes, so a queued
    /// `close_notify` or alert is not cancelled by the close.
    close_after_shutdown: bool,
}

thread_local! {
    /// Per loop-owning thread: the connections and the listeners' acceptors.
    static CONNS: RefCell<HashMap<i64, Conn>> = RefCell::new(HashMap::new());
    static ACCEPTORS: RefCell<HashMap<i64, Acceptor>> = RefCell::new(HashMap::new());
}

// ── Which thread submits ────────────────────────────────────────────────────

static REGISTERED: AtomicBool = AtomicBool::new(false);

/// Whether this thread owns its agent's loop and the sink is installed.
fn enabled() -> bool {
    if !REGISTERED.load(Ordering::Acquire) && tl::register_sink(SUBSYSTEM, sink, alloc_id) {
        REGISTERED.store(true, Ordering::Release);
    }
    REGISTERED.load(Ordering::Acquire) && tl::available()
}

/// How many times a transiently refused post is retried (perry-ext-net's
/// `POST_ATTEMPTS`: a refusal is transient only while the owner is publishing
/// its loop or draining a full postbox).
const POST_ATTEMPTS: usize = 64;

struct LoopJob(Box<dyn FnOnce() + Send>);

impl AgentJob for LoopJob {
    fn run(self: Box<Self>) {
        (self.0)();
    }
}

/// Run `op` on the thread that owns this agent's loop: inline when that is
/// this thread, otherwise posted to the owner. `false` — with `op` dropped
/// unrun — only when this agent has no loop anywhere.
fn on_loop(op: impl FnOnce() + Send + 'static) -> bool {
    if enabled() {
        op();
        return true;
    }
    let mut job = Box::new(LoopJob(Box::new(op)));
    for _ in 0..POST_ATTEMPTS {
        match agent_post::post_job(job) {
            Ok(()) => return true,
            Err(rejected) if rejected.is_permanent() => return false,
            Err(rejected) => {
                job = rejected.into_job();
                std::thread::yield_now();
            }
        }
    }
    false
}

/// Allocate the handle for a connection turnloop just accepted.
extern "C" fn alloc_id() -> i64 {
    tl_id(next_tls_handle_id())
}

// ── Listener ────────────────────────────────────────────────────────────────

/// The text the tokio path's `format!("bind {bind}: {e}")` carried for `e`.
fn os_error_text(error: &tl::NodeError) -> String {
    if error.errno != 0 {
        std::io::Error::from_raw_os_error(error.errno.abs()).to_string()
    } else {
        error.code.to_string()
    }
}

fn bind_failed(server_id: i64, message: String) {
    push_tls_event(PendingTlsEvent::ServerError(server_id, message));
    push_tls_event(PendingTlsEvent::ServerClose(server_id));
    if let Some(server) = servers().lock().unwrap().get_mut(&server_id) {
        liveness::update_server(server, |server| server.listening = false);
    }
}

/// `server.listen(port, host)`: bind, report `'listening'`, start accepting.
pub(super) fn listen(server_id: i64, host: String, port: u16, acceptor: Acceptor) {
    let bind = format!("{host}:{port}");
    let refused = bind.clone();
    let posted = on_loop(move || {
        // A `close()` that overtook a posted listen: never open the listener.
        if servers()
            .lock()
            .unwrap()
            .get(&server_id)
            .is_none_or(|server| server.closing)
        {
            return;
        }
        let addrs: Vec<SocketAddr> = match (host.as_str(), port).to_socket_addrs() {
            Ok(addrs) => addrs.collect(),
            Err(error) => return bind_failed(server_id, format!("bind {bind}: {error}")),
        };
        // `TcpListener::bind(&str)` semantics: the first address that binds.
        let mut last_error = None;
        let mut bound = None;
        for addr in addrs {
            match tl::tcp_listen(tl_id(server_id), SUBSYSTEM, addr, BACKLOG, false, false) {
                Ok(local) => {
                    bound = Some(local);
                    break;
                }
                Err(error) => last_error = Some(os_error_text(&error)),
            }
        }
        let Some(local) = bound else {
            let reason =
                last_error.unwrap_or_else(|| "could not resolve to any address".to_string());
            return bind_failed(server_id, format!("bind {bind}: {reason}"));
        };
        if let Some(server) = servers().lock().unwrap().get_mut(&server_id) {
            server.bound_port = local.port();
            server.bound_host = local.ip().to_string();
            server.listener_open = true;
        }
        ACCEPTORS.with(|acceptors| acceptors.borrow_mut().insert(server_id, acceptor));
        push_tls_event(PendingTlsEvent::ServerListening(server_id));
        if let Err(error) = tl::accept_start(tl_id(server_id)) {
            push_tls_event(PendingTlsEvent::ServerError(
                server_id,
                format!("accept: {}", os_error_text(&error)),
            ));
        }
    });
    if !posted {
        // This agent has no loop anywhere (a host where `Loop::new` failed):
        // report it through the listener's own error path instead of running
        // a second event loop.
        bind_failed(server_id, format!("bind {refused}: ENOTSUP"));
    }
}

/// `server.close()`: stop accepting. Established connections stay open, as
/// in Node.
pub(super) fn close_listener(server_id: i64) {
    on_loop(move || {
        ACCEPTORS.with(|acceptors| acceptors.borrow_mut().remove(&server_id));
        let _ = tl::close(tl_id(server_id));
    });
}

fn on_accept(server_id: i64, socket_id: i64) {
    let conn_id = tl_id(socket_id);
    if !tls_server_connection_started(server_id) {
        let _ = tl::close(conn_id);
        return;
    }
    let acceptor = ACCEPTORS.with(|acceptors| {
        acceptors.borrow().get(&server_id).map(|acceptor| {
            (
                acceptor.config.clone(),
                acceptor.cert_resolver.clone(),
                acceptor.allow_half_open,
            )
        })
    });
    let Some((config, cert_resolver, allow_half_open)) = acceptor else {
        // The listener closed between the accept and its completion.
        let _ = tl::close(conn_id);
        tls_server_connection_finished(server_id);
        return;
    };
    let local_addr = tl::local_addr(conn_id);
    let peer_addr = tl::peer_addr(conn_id);
    let original_servername = socket_api::take_original_servername(server_id);
    let session = match TlsSession::server(config) {
        Ok(session) => session,
        Err(error) => {
            handshake_failed_record(
                server_id,
                socket_id,
                local_addr,
                peer_addr,
                allow_half_open,
                error.to_string(),
            );
            let _ = tl::close(conn_id);
            return;
        }
    };
    CONNS.with(|conns| {
        conns.borrow_mut().insert(
            socket_id,
            Conn {
                server_id,
                session,
                phase: Phase::Handshaking,
                original_servername,
                cert_resolver,
                allow_half_open,
                local_addr,
                peer_addr,
                write_closed: false,
                close_after_shutdown: false,
            },
        )
    });
    if let Err(error) = tl::read_start(conn_id) {
        with_conn(socket_id, |conn| {
            fail(socket_id, conn, os_error_text(&error));
        });
    }
}

// ── Connection ──────────────────────────────────────────────────────────────

fn with_conn<R>(socket_id: i64, f: impl FnOnce(&mut Conn) -> R) -> Option<R> {
    CONNS.with(|conns| conns.borrow_mut().get_mut(&socket_id).map(f))
}

/// Run the session and write whatever it produced.
fn pump_and_flush(socket_id: i64, conn: &mut Conn) {
    conn.session.pump();
    if conn.session.has_output() {
        let bytes = conn.session.take_output();
        // A failed submission is also reported by the handle's own error or
        // close completion, which is where the socket is torn down.
        let _ = tl::write(tl_id(socket_id), bytes, 0);
    }
}

fn failure_message(conn: &Conn) -> Option<String> {
    conn.session
        .failure()
        .map(|failure| failure.message.clone())
}

/// The record a failed handshake leaves behind for `'tlsClientError'`'s
/// socket argument — the tokio path's `Err(e)` arm, verbatim.
fn handshake_failed_record(
    server_id: i64,
    socket_id: i64,
    local_addr: Option<SocketAddr>,
    peer_addr: Option<SocketAddr>,
    allow_half_open: bool,
    message: String,
) {
    sockets().lock().unwrap().insert(
        socket_id,
        TlsSocketState {
            live_transport: false,
            local_addr,
            peer_addr,
            authorized: false,
            server_side: true,
            max_send_fragment: 16 * 1024,
            allow_half_open,
            locally_constructed: false,
            authorization_error: Some(message.clone()),
            protocol: None,
            alpn_protocol: None,
            servername: None,
            peer_certificate: Vec::new(),
            own_certificate: Vec::new(),
            server_handle: Some(server_id),
        },
    );
    listeners()
        .lock()
        .unwrap()
        .insert(socket_id, HashMap::new());
    push_tls_event(PendingTlsEvent::ServerTlsClientError(
        server_id,
        socket_id,
        format!("tls handshake: {message}"),
        None,
    ));
    tls_server_connection_finished(server_id);
}

/// Shut the write side down (after anything queued, such as an alert or a
/// `close_notify`), then close the handle.
fn close_gracefully(socket_id: i64, conn: &mut Conn) {
    conn.close_after_shutdown = true;
    if conn.write_closed || tl::shutdown(tl_id(socket_id), 0).is_err() {
        let _ = tl::close(tl_id(socket_id));
    }
    conn.write_closed = true;
}

/// The socket's terminal pair after the readable side is done: `'close'` one
/// deadline later, so Node's `'end'`-then-`'close'` turn boundary holds and
/// `server.close()` can observe the count reaching zero in between.
fn finish(socket_id: i64, conn: &mut Conn) {
    conn.phase = Phase::Ended;
    tls_server_connection_finished(conn.server_id);
    if tl::timer_arm(timer_id(socket_id), SUBSYSTEM, 1).is_err() {
        push_tls_event(PendingTlsEvent::SocketClose(socket_id));
    }
}

/// A transport or TLS error: during the handshake it is `'tlsClientError'`,
/// afterwards the socket's `'error'` then `'close'`.
fn fail(socket_id: i64, conn: &mut Conn, message: String) {
    match conn.phase {
        Phase::Handshaking => {
            conn.phase = Phase::Ended;
            handshake_failed_record(
                conn.server_id,
                socket_id,
                conn.local_addr,
                conn.peer_addr,
                conn.allow_half_open,
                message,
            );
            close_gracefully(socket_id, conn);
        }
        Phase::Open => {
            push_tls_event(PendingTlsEvent::SocketError(socket_id, message));
            finish(socket_id, conn);
            let _ = tl::close(tl_id(socket_id));
        }
        Phase::Ended => {}
    }
}

/// The peer's `close_notify` (or, for a peer that never sends one, the TCP
/// FIN): answer it, then `'end'` and `'close'`.
fn clean_eof(socket_id: i64, conn: &mut Conn) {
    if !conn.write_closed {
        // Reply to the peer's close_notify before dropping TCP; otherwise the
        // client intermittently sees UnexpectedEof.
        conn.session.close_notify();
        pump_and_flush(socket_id, conn);
    }
    push_tls_event(PendingTlsEvent::SocketEnd(socket_id));
    finish(socket_id, conn);
    close_gracefully(socket_id, conn);
}

/// The handshake just completed: register the socket and announce it.
fn established(socket_id: i64, conn: &mut Conn) {
    let session = &conn.session;
    let protocol = match session.protocol_version() {
        Some(rustls::ProtocolVersion::TLSv1_2) => Some("TLSv1.2".to_string()),
        Some(rustls::ProtocolVersion::TLSv1_3) => Some("TLSv1.3".to_string()),
        _ => None,
    };
    let alpn_protocol = session
        .alpn_protocol()
        .map(|value| String::from_utf8_lossy(value).into_owned());
    let servername = conn
        .original_servername
        .take()
        .or_else(|| session.server_name().map(str::to_string));
    let own_certificate = conn
        .cert_resolver
        .as_ref()
        .map(|resolver| resolver.selected(servername.as_deref()).1)
        .unwrap_or_default();
    let peer_certificate = session
        .peer_certificates()
        .and_then(|certificates| certificates.first())
        .map(|certificate| certificate.as_ref().to_vec())
        .unwrap_or_default();
    let authorized = !peer_certificate.is_empty();
    {
        let mut registry = sockets().lock().unwrap();
        // A server-side socket with a live transport keeps the loop alive.
        liveness::step(false, true);
        registry.insert(
            socket_id,
            TlsSocketState {
                live_transport: true,
                local_addr: conn.local_addr,
                peer_addr: conn.peer_addr,
                authorized,
                server_side: true,
                max_send_fragment: 16 * 1024,
                allow_half_open: conn.allow_half_open,
                locally_constructed: false,
                authorization_error: (!authorized).then(|| "UNABLE_TO_GET_ISSUER_CERT".to_string()),
                protocol,
                alpn_protocol,
                servername,
                peer_certificate,
                own_certificate,
                server_handle: Some(conn.server_id),
            },
        );
    }
    listeners()
        .lock()
        .unwrap()
        .insert(socket_id, HashMap::new());
    push_tls_event(PendingTlsEvent::ServerSecureConnection(
        conn.server_id,
        socket_id,
    ));
    conn.phase = Phase::Open;
}

/// Advance the session after new ciphertext (or none): flush, finish the
/// handshake, deliver plaintext, and notice failure or the peer's close.
fn advance(socket_id: i64, conn: &mut Conn) {
    pump_and_flush(socket_id, conn);
    if conn.phase == Phase::Handshaking {
        if let Some(message) = failure_message(conn) {
            return fail(socket_id, conn, message);
        }
        if conn.session.is_handshaking() {
            return;
        }
        established(socket_id, conn);
    }
    if conn.phase != Phase::Open {
        return;
    }
    let plaintext = conn.session.take_plaintext();
    if !plaintext.is_empty() {
        push_tls_event(PendingTlsEvent::SocketData(socket_id, plaintext));
    }
    if let Some(message) = failure_message(conn) {
        fail(socket_id, conn, message);
    } else if conn.session.peer_closed() {
        clean_eof(socket_id, conn);
    }
}

fn on_eof(socket_id: i64, conn: &mut Conn) {
    match conn.phase {
        Phase::Handshaking => fail(socket_id, conn, "tls handshake eof".to_string()),
        Phase::Open if conn.session.peer_closed() => clean_eof(socket_id, conn),
        Phase::Open => fail(socket_id, conn, UNEXPECTED_EOF_MESSAGE.to_string()),
        Phase::Ended => {}
    }
}

// ── JS-side commands ────────────────────────────────────────────────────────

/// `socket.write(data)`.
pub(super) fn write(socket_id: i64, bytes: Vec<u8>) {
    on_loop(move || {
        with_conn(socket_id, |conn| {
            if conn.phase != Phase::Open || conn.write_closed {
                return;
            }
            conn.session.write(&bytes);
            pump_and_flush(socket_id, conn);
            if let Some(message) = failure_message(conn) {
                fail(socket_id, conn, message);
            }
        });
    });
}

/// `socket.end([data])`: flush, send `close_notify`, shut the write side;
/// the read side stays open for the peer's reply.
pub(super) fn end(socket_id: i64, bytes: Option<Vec<u8>>) {
    on_loop(move || {
        with_conn(socket_id, |conn| {
            if conn.phase != Phase::Open || conn.write_closed {
                return;
            }
            if let Some(bytes) = bytes {
                conn.session.write(&bytes);
            }
            conn.session.close_notify();
            pump_and_flush(socket_id, conn);
            conn.write_closed = true;
            let _ = tl::shutdown(tl_id(socket_id), 0);
        });
    });
}

/// `socket.destroy()`.
pub(super) fn destroy(socket_id: i64) {
    on_loop(move || {
        with_conn(socket_id, |conn| {
            if conn.phase == Phase::Ended {
                return;
            }
            finish(socket_id, conn);
            let _ = tl::close(tl_id(socket_id));
        });
    });
}

// ── Completion sink ─────────────────────────────────────────────────────────

extern "C" fn sink(completion: *const tl::NetCompletion) {
    // SAFETY: `turnloop_net::dispatch` borrows a live completion for the
    // duration of this call; nothing here retains it.
    let c = unsafe { &*completion };
    if c.kind == tl::NET_TIMER {
        if let Some(socket_id) = timer_handle_of(c.id) {
            push_tls_event(PendingTlsEvent::SocketClose(socket_id));
        }
        return;
    }
    let Some(handle) = handle_of(c.id) else {
        return;
    };
    match c.kind {
        tl::NET_ACCEPT => {
            if let Some(socket_id) = handle_of(c.conn) {
                on_accept(handle, socket_id);
            }
        }
        tl::NET_DATA => {
            // SAFETY: only read inside this call, which is the documented lease.
            let bytes = unsafe { c.bytes() };
            with_conn(handle, |conn| {
                if conn.phase == Phase::Ended {
                    return;
                }
                conn.session.receive(bytes);
                advance(handle, conn);
            });
        }
        tl::NET_EOF => {
            with_conn(handle, |conn| on_eof(handle, conn));
        }
        tl::NET_SHUTDOWN => {
            with_conn(handle, |conn| {
                if conn.close_after_shutdown {
                    let _ = tl::close(c.id);
                }
            });
        }
        tl::NET_ERROR => {
            let message = if c.errno != 0 {
                std::io::Error::from_raw_os_error(c.errno.abs()).to_string()
            } else {
                // SAFETY: `code` points at static string data in the runtime.
                unsafe { c.code_str() }.unwrap_or("ECONNRESET").to_string()
            };
            let is_conn = with_conn(handle, |conn| {
                fail(handle, conn, message.clone());
                // A terminal error ends the handle's operations; close it so
                // the entry is released.
                if c.terminal != 0 {
                    let _ = tl::close(c.id);
                }
            })
            .is_some();
            if !is_conn && servers().lock().unwrap().contains_key(&handle) {
                push_tls_event(PendingTlsEvent::ServerError(
                    handle,
                    format!("accept: {message}"),
                ));
            }
        }
        tl::NET_CLOSED => {
            CONNS.with(|conns| conns.borrow_mut().remove(&handle));
        }
        _ => {}
    }
}
