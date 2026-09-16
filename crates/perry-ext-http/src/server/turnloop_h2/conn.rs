//! One turnloop-backed HTTP/2 connection — server or client.
//!
//! The whole session lives on the loop-owning thread: bytes arrive as a
//! `NET_DATA` completion, `turnloop_http::http2::Connection` turns them into
//! events, each event is applied to per-stream state, and the frames the core
//! produced are written back. There is no task, no channel and no cross-thread
//! notify anywhere on that path.
//!
//! ## The receive loop, and its sharp edge
//!
//! `Connection::receive` returns a `Step { consumed, event }` and **both halves
//! can be zero-ish independently**:
//!
//! * `consumed == 0, event == None` — a partial client preface or a partial
//!   frame. The only correct response is to stop and wait for more bytes;
//!   looping on "there is still input" spins forever.
//! * `consumed > 0, event == None` — a SETTINGS **ack**, a PRIORITY frame, an
//!   unknown frame type, or the preface itself. Real progress with nothing to
//!   report, and a host that stops here stalls the connection.
//!
//! So the loop condition is `consumed > 0 || event.is_some()`, which is what
//! `turnloop_http::asynchronous`'s own driver uses. (This is the HTTP/2
//! analogue of the `http1::Decoder` zero-consume `Event::End` trap
//! PerryTS/turnloop#50 records; the shape differs, the lesson does not.)
//!
//! The second edge is `Event::Data { bytes }`, which **borrows the input
//! buffer**. Every event is therefore copied into an owned [`Owned`] before the
//! buffer is drained — which the GC rule wanted anyway, since the bytes have to
//! become a JS `Buffer` eventually.
//!
//! The third is that a `receive` that **errors** has already queued a GOAWAY
//! into `core.output()`. Returning without flushing sends a peer nothing at
//! all, and h2spec asks for that frame by error code on ~60 of its tests.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use perry_ffi::turnloop_net as tl;
use turnloop_http::http1::Header;
use turnloop_http::http2::{self, Event, Role};

use super::stream::{self, H2Stream};

/// Node's `settingsTimeout`: how long a peer has to acknowledge our SETTINGS.
const SETTINGS_TIMEOUT_MS: u64 = 10_000;

/// What the connection's single turnloop deadline currently means.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Timer {
    None,
    /// Waiting for the peer's SETTINGS acknowledgement.
    Settings,
}

/// An [`Event`] with its borrowed bytes copied out, so the input buffer can be
/// drained before the event is applied.
pub(crate) enum Owned {
    Settings,
    Headers {
        stream: u32,
        headers: Vec<Header>,
        end_stream: bool,
    },
    Data {
        stream: u32,
        bytes: Vec<u8>,
        end_stream: bool,
    },
    Reset {
        stream: u32,
        code: u32,
    },
    Goaway {
        last_stream: u32,
        code: u32,
    },
    Ping {
        ack: bool,
        data: [u8; 8],
    },
    WindowUpdate {
        stream: u32,
    },
}

fn own_event(event: Event<'_>) -> Owned {
    match event {
        Event::Settings => Owned::Settings,
        Event::Headers {
            stream,
            headers,
            end_stream,
        } => Owned::Headers {
            stream,
            headers,
            end_stream,
        },
        Event::Data {
            stream,
            bytes,
            end_stream,
        } => Owned::Data {
            stream,
            bytes: bytes.to_vec(),
            end_stream,
        },
        Event::Reset { stream, code } => Owned::Reset { stream, code },
        Event::Goaway { last_stream, code } => Owned::Goaway { last_stream, code },
        Event::Ping { ack, data } => Owned::Ping { ack, data },
        Event::WindowUpdate { stream } => Owned::WindowUpdate { stream },
    }
}

/// One HTTP/2 connection, server-side or client-side.
pub(crate) struct H2Conn {
    pub(crate) id: i64,
    pub(crate) role: Role,
    /// The `Http2SecureServer` handle; zero on a client session.
    pub(crate) server_handle: i64,
    /// The `Http2SessionHandle` this connection is the transport for.
    pub(crate) session_handle: i64,
    /// `None` until the transport is ready (a client before `NET_CONNECT`, or
    /// either side before a TLS handshake completes) and after a fatal error.
    pub(crate) core: Option<http2::Connection>,
    pub(crate) input: Vec<u8>,
    pub(crate) streams: Vec<H2Stream>,
    pub(crate) secure: bool,
    pub(crate) handshaking: bool,
    pub(crate) connecting: bool,
    pub(crate) alpn: Option<Vec<u8>>,
    pub(crate) peer_address: String,
    pub(crate) peer_port: u16,
    /// Undispatched inbound body bytes held by this connection — the quantity
    /// `maxSessionMemory` bounds, and the only thing that stops the receive
    /// window from being reopened eagerly.
    pub(crate) buffered: usize,
    pub(crate) max_session_memory: usize,
    pub(crate) timer: Timer,
    pub(crate) draining: bool,
    pub(crate) closing: bool,
    pub(crate) read_eof: bool,
    pub(crate) destroyed: bool,
    /// A client's requests issued before the transport was ready.
    pub(crate) queued_opens: Vec<stream::QueuedOpen>,
    /// `allowHTTP1` for a server connection that negotiates `http/1.1`.
    pub(crate) allow_http1: bool,
    pub(crate) settings: crate::server::http2_session_settings::Http2SettingsState,
}

fn conns() -> &'static Mutex<HashMap<i64, H2Conn>> {
    static CONNS: OnceLock<Mutex<HashMap<i64, H2Conn>>> = OnceLock::new();
    CONNS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Ids this module owns, kept separately from [`conns`] so that [`owns`] stays
/// truthful while a record is checked out by [`with_owned`].
fn owned_ids() -> &'static Mutex<std::collections::HashSet<i64>> {
    static IDS: OnceLock<Mutex<std::collections::HashSet<i64>>> = OnceLock::new();
    IDS.get_or_init(|| Mutex::new(std::collections::HashSet::new()))
}

pub(crate) fn owns(id: i64) -> bool {
    owned_ids()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(&id)
}

pub(crate) fn insert(conn: H2Conn) {
    owned_ids()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(conn.id);
    conns()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(conn.id, conn);
}

fn forget(id: i64) -> Option<H2Conn> {
    owned_ids()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&id);
    conns()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&id)
}

/// Run `f` over a checked-out connection record.
///
/// The record is **removed** for the duration and put back afterwards, so `f`
/// may call anything — including `write_raw`, which re-enters the table — with
/// no risk of the non-reentrant mutex deadlocking. [`owns`] keeps answering
/// true meanwhile, so a completion that arrives in the middle (it cannot: the
/// sink is not re-entrant) would still route here rather than to P5.
pub(crate) fn with_owned<R>(id: i64, f: impl FnOnce(&mut H2Conn) -> R) -> Option<R> {
    let mut conn = conns()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&id)?;
    let result = f(&mut conn);
    let gone = conn.destroyed;
    let mut map = conns().lock().unwrap_or_else(|e| e.into_inner());
    if !gone {
        map.insert(id, conn);
    } else {
        drop(map);
        owned_ids()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
    }
    Some(result)
}

/// Read one field of a connection without checking it out.
pub(crate) fn peek<R>(id: i64, f: impl FnOnce(&H2Conn) -> R) -> Option<R> {
    let map = conns().lock().unwrap_or_else(|e| e.into_inner());
    map.get(&id).map(f)
}

/// Every live HTTP/2 connection of one JS server handle.
pub(crate) fn connections_of(server_handle: i64) -> Vec<i64> {
    conns()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|(_, c)| c.server_handle == server_handle)
        .map(|(id, _)| *id)
        .collect()
}

/// The connection carrying a session handle, if it is on turnloop.
pub(crate) fn connection_of_session(session_handle: i64) -> Option<i64> {
    conns()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|(_, c)| c.session_handle == session_handle)
        .map(|(id, _)| *id)
}

// ── Completion routing ──────────────────────────────────────────────────────

/// Called first from P5's sink. Returns true when this completion was HTTP/2's.
pub(crate) fn intercept(c: &tl::NetCompletion) -> bool {
    match c.kind {
        tl::NET_ACCEPT => {
            if !super::is_listener(c.id) {
                return false;
            }
            on_accept(c.id, c.conn);
            true
        }
        _ => {
            if !owns(c.id) {
                return false;
            }
            match c.kind {
                tl::NET_CONNECT => on_connect(c.id),
                // SAFETY: valid for the duration of this sink call.
                tl::NET_DATA => on_data(c.id, unsafe { c.bytes() }),
                tl::NET_EOF => on_eof(c.id),
                tl::NET_WROTE => {}
                tl::NET_SHUTDOWN => on_shutdown(c.id),
                tl::NET_CLOSED => on_closed(c.id),
                tl::NET_TIMER => on_timer(c.id),
                tl::NET_ERROR => {
                    // SAFETY: same call; both point at `'static` string data.
                    let code = unsafe { c.code() };
                    on_error(c.id, code);
                }
                _ => {}
            }
            true
        }
    }
}

/// A listener error. P5's sink owns listener ids it knows; this answers for
/// HTTP/2's, which P5's `on_error` would otherwise treat as a connection.
pub(crate) fn intercept_listener_error(id: i64, terminal: bool) -> bool {
    if !super::is_listener(id) {
        return false;
    }
    if terminal {
        super::close_listener(id);
    }
    true
}

// ── Server accept ───────────────────────────────────────────────────────────

fn on_accept(listener_id: i64, conn_id: i64) {
    if conn_id == 0 {
        return;
    }
    let Some((server_handle, tls, allow_http1, settings, max_session_memory)) =
        super::with_listener(listener_id, |l| {
            (
                l.server_handle,
                l.tls.clone(),
                l.allow_http1,
                l.settings.clone(),
                l.max_session_memory,
            )
        })
    else {
        let _ = tl::close(conn_id);
        return;
    };
    let secure = tls.is_some();
    if let Some(config) = tls {
        if perry_ext_net::turnloop_tls_io::install_server_session(conn_id, config).is_err() {
            let _ = tl::close(conn_id);
            return;
        }
    }
    let peer = tl::peer_address(conn_id);
    let session_handle = crate::server::http2_server::register_turnloop_server_session(
        server_handle,
        peer.as_ref().map(|e| e.port).unwrap_or(0),
        secure,
        if secure { "h2" } else { "h2c" },
    );
    let mut conn = H2Conn {
        id: conn_id,
        role: Role::Server,
        server_handle,
        session_handle,
        core: None,
        input: Vec::with_capacity(16 * 1024),
        streams: Vec::new(),
        secure,
        handshaking: secure,
        connecting: false,
        alpn: None,
        peer_address: peer.as_ref().map(|e| e.address.clone()).unwrap_or_default(),
        peer_port: peer.as_ref().map(|e| e.port).unwrap_or(0),
        buffered: 0,
        max_session_memory,
        timer: Timer::None,
        draining: false,
        closing: false,
        read_eof: false,
        destroyed: false,
        queued_opens: Vec::new(),
        allow_http1,
        settings,
    };
    if !secure {
        // h2c with prior knowledge: the core starts immediately and the client
        // preface is the first thing it will be fed.
        if !start_core(&mut conn) {
            let _ = tl::close(conn_id);
            return;
        }
    }
    insert(conn);
    crate::server::server::queue_turnloop_connection_event(server_handle);
    if let Err(_err) = tl::read_start(conn_id) {
        destroy_connection(conn_id);
        return;
    }
    flush_id(conn_id);
}

/// Build the protocol core and queue our own preface + SETTINGS.
fn start_core(c: &mut H2Conn) -> bool {
    let mut limits = http2::Limits::default();
    // `Limits::streams` is both the SETTINGS_MAX_CONCURRENT_STREAMS we
    // advertise and the size of the core's stream table, so it is clamped to
    // something a connection can actually hold rather than Node's `u32::MAX`
    // default. 128 is Node's own effective server default.
    limits.streams = clamp_streams(c.settings.max_concurrent_streams);
    limits.frame_size = c.settings.max_frame_size.clamp(16_384, 0xff_ffff) as usize;
    limits.header_list = c.settings.max_header_list_size.max(4_096) as usize;
    match http2::Connection::new(c.role, limits) {
        Ok(core) => {
            c.core = Some(core);
            arm_settings_timeout(c);
            true
        }
        Err(_) => false,
    }
}

fn clamp_streams(requested: u32) -> usize {
    if requested == 0 || requested == u32::MAX {
        128
    } else {
        requested.min(10_000) as usize
    }
}

fn arm_settings_timeout(c: &mut H2Conn) {
    let Some(core) = c.core.as_mut() else { return };
    let Some(deadline) = Instant::now().checked_add(Duration::from_millis(SETTINGS_TIMEOUT_MS))
    else {
        return;
    };
    core.set_settings_deadline(Some(deadline));
    if tl::timer_arm(c.id, super::SUBSYSTEM, SETTINGS_TIMEOUT_MS).is_ok() {
        c.timer = Timer::Settings;
    }
}

// ── Client connect ──────────────────────────────────────────────────────────

fn on_connect(id: i64) {
    let ready = with_owned(id, |c| {
        c.connecting = false;
        c.peer_address = tl::peer_address(id).map(|e| e.address).unwrap_or_default();
        if c.secure {
            // The TLS handshake starts now; the core waits for ALPN.
            return false;
        }
        start_core(c)
    });
    match ready {
        Some(true) => {
            if tl::read_start(id).is_err() {
                destroy_connection(id);
                return;
            }
            client_transport_ready(id);
        }
        Some(false) => {
            if tl::read_start(id).is_err() {
                destroy_connection(id);
            }
        }
        None => {}
    }
}

/// The transport is up and the core exists: announce `'connect'` and release
/// any `session.request()` calls JS made before this point.
fn client_transport_ready(id: i64) {
    let session = peek(id, |c| c.session_handle).unwrap_or(0);
    let alpn = peek(id, |c| c.alpn.clone()).flatten();
    if session != 0 {
        let protocol = match alpn.as_deref() {
            Some(b"h2") => "h2",
            Some(other) => std::str::from_utf8(other).unwrap_or("h2"),
            None => "h2c",
        };
        crate::server::http2_server::mark_turnloop_client_connected(session, protocol);
    }
    with_owned(id, |c| {
        let queued = std::mem::take(&mut c.queued_opens);
        for open in queued {
            stream::open_client_stream(c, open);
        }
    });
    flush_id(id);
}

// ── Data ────────────────────────────────────────────────────────────────────

fn on_data(id: i64, bytes: &[u8]) {
    let plaintext: Option<Vec<u8>> = if peek(id, |c| c.secure).unwrap_or(false) {
        match perry_ext_net::turnloop_tls_io::receive(id, bytes) {
            Some(received) => {
                if received.peer_closed {
                    let text = received.plaintext;
                    if !text.is_empty() {
                        feed(id, &text);
                    }
                    on_eof(id);
                    return;
                }
                Some(received.plaintext)
            }
            None => return,
        }
    } else {
        None
    };
    if peek(id, |c| c.handshaking).unwrap_or(false)
        && perry_ext_net::turnloop_tls_io::handshake_done(id)
    {
        if !finish_handshake(id) {
            return;
        }
    }
    match plaintext {
        Some(text) if !text.is_empty() => feed(id, &text),
        Some(_) => {}
        None => feed(id, bytes),
    }
}

/// ALPN has been decided. Either start the HTTP/2 core, hand the whole
/// connection to P5's HTTP/1.1 server, or refuse it.
///
/// Returns false when the connection is no longer ours.
fn finish_handshake(id: i64) -> bool {
    let alpn = perry_ext_net::turnloop_tls_io::alpn_protocol(id);
    let decision = with_owned(id, |c| {
        c.handshaking = false;
        c.alpn = alpn.clone();
        match alpn.as_deref() {
            // Node's `createSecureServer` speaks HTTP/2 to a peer that asked
            // for it, and so does a peer that offered no ALPN at all on a
            // cleartext-equivalent connection.
            Some(b"h2") | None => {
                if start_core(c) {
                    Handshake::Http2
                } else {
                    Handshake::Refuse
                }
            }
            Some(b"http/1.1") | Some(b"http/1.0") => {
                if c.allow_http1 {
                    Handshake::Http1
                } else {
                    Handshake::Refuse
                }
            }
            Some(_) => Handshake::Refuse,
        }
    });
    match decision {
        Some(Handshake::Http2) => {
            if peek(id, |conn| conn.role == Role::Client).unwrap_or(false) {
                client_transport_ready(id);
            }
            flush_id(id);
            true
        }
        Some(Handshake::Http1) => {
            hand_to_http1(id);
            false
        }
        Some(Handshake::Refuse) => {
            destroy_connection(id);
            false
        }
        None => false,
    }
}

enum Handshake {
    Http2,
    Http1,
    Refuse,
}

/// ALPN chose `http/1.1` on an `http2.createSecureServer({ allowHTTP1: true })`
/// listener. The socket keeps its id, its TLS layer and its outstanding
/// multishot read; only the owning table changes, because both halves are the
/// same subsystem. That is the whole reason this module shares slot 1.
fn hand_to_http1(id: i64) {
    let Some(conn) = forget(id) else { return };
    let leftover = conn.input;
    if !crate::server::turnloop_serve::adopt_alpn_http1(
        id,
        conn.server_handle,
        conn.peer_address,
        conn.peer_port,
        leftover,
    ) {
        let _ = tl::close(id);
        return;
    }
    crate::server::http2_server::mark_turnloop_session_closed(conn.session_handle);
}

fn feed(id: i64, bytes: &[u8]) {
    let ready = with_owned(id, |conn| {
        conn.input.extend_from_slice(bytes);
        conn.core.is_some() && !conn.destroyed
    });
    if ready == Some(true) {
        pump(id);
    }
}

// ── The receive loop ────────────────────────────────────────────────────────

fn pump(id: i64) {
    let outcome = with_owned(id, |conn| {
        let mut fatal = None;
        loop {
            if conn.destroyed {
                return Outcome::Gone;
            }
            let (consumed, event) = {
                let H2Conn { core, input, .. } = &mut *conn;
                let Some(core) = core.as_mut() else {
                    return Outcome::Gone;
                };
                match core.receive(input) {
                    Ok(step) => (step.consumed, step.event.map(own_event)),
                    Err(err) => {
                        // `receive` has already queued the GOAWAY carrying this
                        // error's code. Consume nothing, stop, and let the
                        // caller flush before the connection goes down.
                        fatal = Some(err.code);
                        (0, None)
                    }
                }
            };
            if fatal.is_some() {
                break;
            }
            if consumed > 0 {
                conn.input.drain(..consumed);
            }
            let progressed = consumed > 0 || event.is_some();
            if let Some(event) = event {
                apply(conn, event);
            }
            if !progressed {
                break;
            }
        }
        stream::pump_outbox(conn);
        match fatal {
            Some(code) => Outcome::Fatal(code),
            None => Outcome::Ok,
        }
    });
    match outcome {
        Some(Outcome::Fatal(code)) => {
            flush_id(id);
            fail_connection(id, code);
        }
        Some(Outcome::Ok) => {
            flush_id(id);
            settle(id);
        }
        _ => {}
    }
}

enum Outcome {
    Ok,
    Fatal(&'static str),
    Gone,
}

fn apply(conn: &mut H2Conn, event: Owned) {
    match event {
        Owned::Settings => stream::on_peer_settings(conn),
        Owned::Headers {
            stream: id,
            headers,
            end_stream,
        } => stream::on_headers(conn, id, headers, end_stream),
        Owned::Data {
            stream: id,
            bytes,
            end_stream,
        } => stream::on_data(conn, id, bytes, end_stream),
        Owned::Reset { stream: id, code } => stream::on_reset(conn, id, code),
        Owned::Goaway { last_stream, code } => {
            conn.draining = true;
            stream::on_goaway(conn, last_stream, code);
        }
        Owned::Ping { ack, data } => stream::on_ping(conn, ack, data),
        // A peer window opened: retry whatever stalled.
        Owned::WindowUpdate { .. } => stream::pump_outbox(conn),
    }
}

/// Post-pump bookkeeping: cancel a satisfied SETTINGS deadline and close a
/// drained connection.
fn settle(id: i64) {
    let action = with_owned(id, |conn| {
        if conn.timer == Timer::Settings
            && conn
                .core
                .as_ref()
                .is_some_and(|core| core.next_timeout().is_none())
        {
            conn.timer = Timer::None;
            let _ = tl::timer_cancel(conn.id);
        }
        conn.core.as_ref().is_some_and(|core| core.is_drained())
    });
    if action == Some(true) {
        graceful_close(id);
    }
}

// ── Writing ─────────────────────────────────────────────────────────────────

/// Hand `core.output()` to the transport and acknowledge it.
///
/// turnloop's `write` copies and queues the whole slice, so a successful
/// submission is a complete one and the acknowledgement is unconditional —
/// which is what lets `consume_output` take the whole buffer in one step.
pub(crate) fn flush(conn: &mut H2Conn) {
    loop {
        let bytes = match conn.core.as_ref() {
            Some(core) => core.output().to_vec(),
            None => return,
        };
        if bytes.is_empty() {
            return;
        }
        let written = if conn.secure {
            perry_ext_net::turnloop_tls_io::write(conn.id, &bytes, 0)
                .map(|_| bytes.len())
                .map_err(|_| ())
        } else {
            tl::write(conn.id, &bytes, 0)
                .map(|_| bytes.len())
                .map_err(|_| ())
        };
        match written {
            Ok(n) => {
                if let Some(core) = conn.core.as_mut() {
                    if core.consume_output(n).is_err() {
                        return;
                    }
                }
            }
            Err(()) => {
                conn.destroyed = true;
                let _ = tl::close(conn.id);
                return;
            }
        }
    }
}

pub(crate) fn flush_id(id: i64) {
    with_owned(id, flush);
}

// ── Terminal paths ──────────────────────────────────────────────────────────

/// A connection-level protocol failure. The GOAWAY is already on the wire;
/// every still-open stream now gets exactly one terminal event, and the
/// connection closes.
fn fail_connection(id: i64, code: &'static str) {
    let terminated = with_owned(id, |conn| {
        if let Some(core) = conn.core.as_mut() {
            core.eof();
        }
        let mut ids = Vec::new();
        while let Some(stream_id) = conn
            .core
            .as_mut()
            .and_then(|core| core.poll_failed_stream())
        {
            ids.push(stream_id);
        }
        ids
    })
    .unwrap_or_default();
    for stream_id in terminated {
        with_owned(id, |conn| {
            stream::terminate(conn, stream_id, Some(code));
        });
    }
    with_owned(id, |conn| {
        crate::server::http2_server::mark_turnloop_session_closed(conn.session_handle);
    });
    finish_and_close(id);
}

/// Node's `session.close()` and the drained end of `session.goaway()`: the
/// GOAWAY has gone, every stream has finished, so end the write side.
pub(crate) fn graceful_close(id: i64) {
    with_owned(id, |conn| {
        crate::server::http2_server::mark_turnloop_session_closed(conn.session_handle);
    });
    finish_and_close(id);
}

/// `session.destroy()` / a transport error: no GOAWAY, no drain.
pub(crate) fn destroy_connection(id: i64) {
    let existed = with_owned(id, |conn| {
        conn.closing = true;
        conn.destroyed = true;
        let session = conn.session_handle;
        let live: Vec<u32> = conn.streams.iter().map(|s| s.h2_id).collect();
        (session, live)
    });
    if let Some((session, live)) = existed {
        for stream_id in live {
            with_owned(id, |conn| {
                stream::terminate(conn, stream_id, Some("ECONNRESET"));
            });
        }
        crate::server::http2_server::mark_turnloop_session_closed(session);
    }
    let _ = tl::timer_cancel(id);
    let _ = tl::close(id);
}

/// End the write side and close once it has drained. turnloop orders a
/// handle's writes ahead of its shutdown, so a completed shutdown means every
/// queued byte — the GOAWAY included — left the process.
fn finish_and_close(id: i64) {
    let secure = with_owned(id, |conn| {
        conn.closing = true;
        conn.secure
    });
    let _ = tl::timer_cancel(id);
    match secure {
        Some(true) => {
            let _ = perry_ext_net::turnloop_tls_io::shutdown(id, 0);
        }
        Some(false) => {
            if tl::shutdown(id, 0).is_err() {
                let _ = tl::close(id);
            }
        }
        None => {
            let _ = tl::close(id);
        }
    }
}

fn on_shutdown(id: i64) {
    // Every queued byte has left; the handle may go.
    let _ = tl::close(id);
}

fn on_eof(id: i64) {
    let already = with_owned(id, |conn| {
        std::mem::replace(&mut conn.read_eof, true) || conn.closing
    });
    if already != Some(false) {
        return;
    }
    // The transport is gone: the core produces one terminal per open stream.
    with_owned(id, |conn| {
        if let Some(core) = conn.core.as_mut() {
            core.eof();
        }
    });
    let terminated = with_owned(id, |conn| {
        let mut ids = Vec::new();
        while let Some(stream_id) = conn
            .core
            .as_mut()
            .and_then(|core| core.poll_failed_stream())
        {
            ids.push(stream_id);
        }
        ids
    })
    .unwrap_or_default();
    for stream_id in terminated {
        with_owned(id, |conn| {
            stream::terminate(conn, stream_id, Some("ECONNRESET"));
        });
    }
    with_owned(id, |conn| {
        crate::server::http2_server::mark_turnloop_session_closed(conn.session_handle);
    });
    finish_and_close(id);
}

fn on_closed(id: i64) {
    let Some(mut conn) = forget(id) else {
        return;
    };
    conn.destroyed = true;
    let live: Vec<u32> = conn.streams.iter().map(|s| s.h2_id).collect();
    for stream_id in live {
        stream::terminate(&mut conn, stream_id, Some("ECONNRESET"));
    }
    crate::server::http2_server::mark_turnloop_session_closed(conn.session_handle);
    if conn.server_handle != 0 {
        crate::server::server::turnloop_connection_closed(id);
    }
    perry_ext_net::turnloop_tls_io::forget(id);
    // The terminal completion: no completion can name this id again, so it goes
    // back to the shared band rather than leaking one per connection.
    perry_ffi::free_handle_id(id);
}

fn on_timer(id: i64) {
    let expired = with_owned(id, |conn| {
        if conn.timer != Timer::Settings {
            return false;
        }
        conn.timer = Timer::None;
        conn.core
            .as_mut()
            .and_then(|core| core.handle_timeout(Instant::now()))
            .is_some()
    });
    if expired == Some(true) {
        // `handle_timeout` queued the SETTINGS_TIMEOUT GOAWAY.
        flush_id(id);
        fail_connection(id, "SETTINGS_TIMEOUT");
    }
}

fn on_error(id: i64, code: Option<&str>) {
    let session = peek(id, |conn| conn.session_handle).unwrap_or(0);
    if session != 0 {
        crate::server::http2_server::queue_turnloop_session_error(
            session,
            code.unwrap_or("ECONNRESET"),
        );
    }
    destroy_connection(id);
}
