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
//!
//! ## The pre-scan, and the three things it recovers
//!
//! Every frame is decoded **twice**: once by [`peek_frame`] here, once by the
//! core. The second decode is the authoritative one; the first exists because
//! three facts a `node:http2` session has to surface did not leave
//! `Connection` when this was written:
//!
//! * **a SETTINGS acknowledgement** — consumed with `event: None`, so
//!   `session.settings(obj, cb)` had nothing to fire its callback on;
//! * **GOAWAY's opaque data** — `Event::Goaway` carried `last_stream` and
//!   `code` only, and Node's `'goaway'` listener receives a third argument;
//! * **the peer's SETTINGS values** — `Event::Settings` was a unit variant, so
//!   `session.remoteSettings` would stay at its defaults forever.
//!
//! **turnloop-http 0.1.0-alpha.7 now carries all three** — `Event::SettingsAck`
//! is an event, `Event::Goaway` has a `debug` field, and `Event::Settings`
//! carries a `SettingsFrame`. The pre-scan was deliberately left in place at
//! that bump rather than unwound: it is also what withholds the frame class
//! described next, and moving three `node:http2`-visible surfaces onto a new
//! source is a behaviour change that wants its own commit and its own h2spec
//! run. Treat the list above as the reason the pre-scan EXISTS, not as a
//! current statement about the core's API.
//!
//! The pre-scan also *withholds* one frame class from the core. `Connection`
//! tracks exactly one outstanding SETTINGS (its own, from the constructor) and
//! answers a second acknowledgement with `protocol("unsolicited SETTINGS
//! ack")`, which is a **connection** error. A `session.settings()` frame is
//! therefore acknowledged by the peer into a core that would kill the session
//! for it, so this module counts the SETTINGS frames it sent out of band and
//! eats exactly that many acks before the core sees them.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use perry_ffi::turnloop_net as tl;
use turnloop_http::http1::Header;
use turnloop_http::http2::{self, Event, HeadersKind, Role};

use crate::server::http2_session_settings::Http2SettingsState;

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
        /// Which of the three header blocks this is. The core enforces the
        /// distinction, so this is carried rather than re-derived here.
        kind: HeadersKind,
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
    /// A stream not processed after a graceful GOAWAY. Carried so the match is
    /// exhaustive and the decision is visible; answering it would diverge from
    /// Node, which sends nothing.
    Unprocessed,
    /// The peer acknowledged our SETTINGS. Before turnloop-http 0.1.0-alpha.7
    /// this was a silent step inside the decoder rather than an event, so
    /// nothing here ever observed it; it is carried (and ignored) to keep that
    /// behaviour explicit rather than to start acting on it. The round-trip
    /// timing the ack now makes measurable is unused — `session.ping()` is the
    /// surface Node exposes for that.
    SettingsAck,
    WindowUpdate,
}

fn own_event(event: Event<'_>) -> Owned {
    match event {
        // alpha.7 gave `Settings` the peer's actual parameters
        // (`SettingsFrame`). Still discarded here: `session.remoteSettings` is
        // served from `stream::on_peer_settings`'s own view of the applied
        // settings, so reading them off the frame would be a second source of
        // the same truth. See the `remoteSettings` note below.
        Event::Settings(_) => Owned::Settings,
        Event::SettingsAck(_) => Owned::SettingsAck,
        Event::Headers {
            stream,
            headers,
            end_stream,
            kind,
        } => Owned::Headers {
            stream,
            headers,
            end_stream,
            kind,
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
        // A stream the peer opened after our graceful GOAWAY, above the last
        // id that GOAWAY named, with nothing sent for it. RFC 9113 §6.8 calls
        // such a stream "not processed" and expects the peer to retry it on a
        // new connection.
        //
        // Node emits NO FRAME AT ALL here — measured against Node 26.5.1 with
        // a raw peer: no RST_STREAM, no GOAWAY, the session stays alive and the
        // request never reaches the application. turnloop-http deliberately
        // does not answer for us, because a frame it emitted could not be
        // un-emitted. So matching Node means doing nothing, and the event is
        // dropped rather than turned into a reset.
        //
        // This is worth stating because the opposite was believed here first:
        // a REFUSED_STREAM reset was proposed on the assumption Node sends one,
        // and measurement against a raw peer showed it does not.
        Event::Unprocessed { .. } => Owned::Unprocessed,
        Event::Reset { stream, code } => Owned::Reset { stream, code },
        // `..` drops alpha.7's new `debug` (RFC 9113 §6.8 Additional Debug
        // Data). Node surfaces it as the `'goaway'` handler's third argument;
        // wiring it through is a behaviour ADDITION, so it is deliberately not
        // done in a dependency bump. See the `Additional Debug Data` note below.
        Event::Goaway {
            last_stream, code, ..
        } => Owned::Goaway { last_stream, code },
        Event::Ping { ack, data } => Owned::Ping { ack, data },
        Event::WindowUpdate { .. } => Owned::WindowUpdate,
    }
}

/// One HTTP/2 connection, server-side or client-side.
/// The TLS a `http2.connect('https://…')` session needs.
///
/// Deliberately not `rustls` types: the configuration this asks for is
/// "verify this name, offer `h2`", and `perry_ext_net::turnloop_tls_io`'s
/// public client installer is the one place that turns it into a session.
#[derive(Clone, Debug)]
pub(crate) struct ClientTls {
    /// The name verified against the certificate and sent as SNI.
    pub(crate) servername: String,
    /// Node's `rejectUnauthorized`. `false` accepts any certificate.
    pub(crate) verify: bool,
    /// Node's `ca`, as PEM blobs. Empty keeps the platform roots.
    pub(crate) ca: Vec<Vec<u8>>,
}

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
    /// What a `https://` client session installs once the socket is up. Taken
    /// by `on_connect`; `None` on a server connection, whose TLS configuration
    /// belongs to the listener and is installed at accept time instead.
    pub(crate) client_tls: Option<ClientTls>,
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
    pub(crate) settings: Http2SettingsState,
    /// Whether the client preface has been consumed, so [`peek_frame`] may
    /// start reading frame headers out of the buffer. Always true for a client,
    /// which never receives one.
    pub(crate) preface_done: bool,
    /// Whether the core's own constructor SETTINGS has been acknowledged. Until
    /// it has, an ack belongs to the core and must not be eaten.
    pub(crate) core_settings_acked: bool,
    /// SETTINGS frames this module wrote out of band (`session.settings()`)
    /// whose acknowledgement has not arrived. See the module docs.
    pub(crate) owed_settings_acks: u32,
    /// GOAWAY opaque data captured by the pre-scan, for the `'goaway'` event.
    /// `Event::Goaway` gained a `debug` field in turnloop-http alpha.7; this
    /// remains the source (see the module docs' pre-scan note).
    pub(crate) goaway_opaque: Vec<u8>,
    /// The peer's SETTINGS values captured by the pre-scan, for the
    /// `'remoteSettings'` event. `Event::Settings` carries a `SettingsFrame`
    /// since turnloop-http alpha.7; this remains the source (see the module
    /// docs' pre-scan note).
    pub(crate) peer_settings: Option<Http2SettingsState>,
    /// Connection-level frames JS asked for before the transport was ready.
    ///
    /// `http2.connect()` returns a session object synchronously and Node lets
    /// `settings()` / `ping()` / `goaway()` be called on it immediately — which
    /// is what `test_gap_gc_http2_pending_event_callback_rooting.ts` does on its
    /// first tick. Writing such a frame straight to a socket that has not
    /// finished connecting puts it on the wire **before the client preface**,
    /// and the peer answers a connection error.
    pub(crate) pending_controls: Vec<PendingControl>,
}

/// A connection-level frame queued until `NET_CONNECT` (and, on a secure
/// client, until ALPN) has produced a core to encode it with.
pub(crate) enum PendingControl {
    Settings(Http2SettingsState),
    Ping([u8; 8]),
    Goaway {
        code: u32,
        last_stream: u32,
        opaque: Vec<u8>,
    },
    Close,
}

/// Whether this connection can encode a frame right now.
pub(crate) fn transport_ready(c: &H2Conn) -> bool {
    c.core.is_some() && !c.connecting && !c.handshaking && !c.destroyed
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
///
/// A record that `f` marked `destroyed` is dropped — but the **id stays owned**
/// until its `NET_CLOSED`. Releasing ownership here instead would hand the
/// remaining completions of a socket this module still has open to P5's HTTP/1.1
/// sink, which has never heard of the id: the terminal completion would reach
/// nobody, the id would never go back to the shared band, and a `perry-ext-http`
/// process that failed one write would leak a handle id per connection (the
/// #6441 exhaustion class). [`forget`] is the only release, and [`on_closed`]
/// is the only caller that can reach it for a live socket.
pub(crate) fn with_owned<R>(id: i64, f: impl FnOnce(&mut H2Conn) -> R) -> Option<R> {
    let mut conn = conns()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&id)?;
    let result = f(&mut conn);
    if !conn.destroyed {
        conns()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, conn);
    }
    Some(result)
}

/// Read one field of a connection without checking it out.
pub(crate) fn peek<R>(id: i64, f: impl FnOnce(&H2Conn) -> R) -> Option<R> {
    let map = conns().lock().unwrap_or_else(|e| e.into_inner());
    map.get(&id).map(f)
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
        conn_id,
        peer.as_ref().map(|e| e.port).unwrap_or(0),
        secure,
        if secure { "h2" } else { "h2c" },
        advertised_settings(&settings),
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
        // A server connection's TLS is the listener's; it was installed at
        // accept time, before this record existed.
        client_tls: None,
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
        preface_done: false,
        core_settings_acked: false,
        owed_settings_acks: 0,
        goaway_opaque: Vec::new(),
        peer_settings: None,
        pending_controls: Vec::new(),
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
            // Only a server reads a client preface; a client goes straight to
            // frames, so the pre-scan may start immediately.
            c.preface_done = c.role == Role::Client;
            arm_settings_timeout(c);
            true
        }
        Err(_) => false,
    }
}

/// The SETTINGS this connection's core actually advertises, after `Limits`
/// clamping — what `session.localSettings` must report, rather than the
/// unclamped option object JS passed in.
pub(crate) fn advertised_settings(requested: &Http2SettingsState) -> Http2SettingsState {
    let mut out = requested.clone();
    out.max_concurrent_streams = clamp_streams(requested.max_concurrent_streams) as u32;
    out.max_frame_size = requested.max_frame_size.clamp(16_384, 0xff_ffff);
    out.max_header_list_size = requested.max_header_list_size.max(4_096);
    out.max_header_size = out.max_header_list_size;
    // The core never negotiates HPACK table size up and never offers push on a
    // server connection.
    out.header_table_size = out.header_table_size.min(4_096);
    out.enable_push = false;
    out
}

/// One frame header read out of the buffer without consuming it.
///
/// `None` for a partial frame, for input the preface has not cleared yet, and
/// for a length the core's own `Limits` would reject — in every one of those
/// the core is the authority and the pre-scan stays out of the way.
fn peek_frame(conn: &H2Conn) -> Option<(u8, u8, u32, usize)> {
    if !conn.preface_done || conn.input.len() < 9 {
        return None;
    }
    let len =
        ((conn.input[0] as usize) << 16) | ((conn.input[1] as usize) << 8) | conn.input[2] as usize;
    if conn.input.len() < 9 + len {
        return None;
    }
    let stream = u32::from_be_bytes([conn.input[5], conn.input[6], conn.input[7], conn.input[8]])
        & 0x7fff_ffff;
    Some((conn.input[3], conn.input[4], stream, len))
}

/// Decode a peer SETTINGS payload into the shape `session.remoteSettings`
/// reports. Unknown identifiers are ignored, exactly as the core ignores them.
fn decode_settings(payload: &[u8]) -> Http2SettingsState {
    let mut out = Http2SettingsState {
        // A peer that omits a setting is at the RFC 9113 default, which is what
        // `Http2SettingsState::default` already carries — except
        // MAX_CONCURRENT_STREAMS, whose protocol default is "unlimited".
        ..Default::default()
    };
    for chunk in payload.chunks_exact(6) {
        let id = u16::from_be_bytes([chunk[0], chunk[1]]);
        let value = u32::from_be_bytes([chunk[2], chunk[3], chunk[4], chunk[5]]);
        match id {
            1 => out.header_table_size = value,
            2 => out.enable_push = value != 0,
            3 => out.max_concurrent_streams = value,
            4 => out.initial_window_size = value,
            5 => out.max_frame_size = value,
            6 => {
                out.max_header_list_size = value;
                out.max_header_size = value;
            }
            8 => out.enable_connect_protocol = value != 0,
            _ => {}
        }
    }
    out
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

/// What `http2.connect('https://…')` offers in ALPN: `h2` alone, which is what
/// Node offers.
///
/// Offering `http/1.1` as well would let a server select it and leave this
/// connection holding a protocol its core cannot speak — there is no HTTP/1.1
/// client on this path to hand it to, the way an accepted connection hands a
/// negotiated `http/1.1` to P5.
///
/// Named rather than written inline at the install so the offer is one thing a
/// test can read; the capability behind it is
/// `perry_ext_net::turnloop_tls_io::install_client_session`, the public client
/// twin of `install_server_session`.
pub(crate) fn client_alpn() -> Vec<Vec<u8>> {
    vec![b"h2".to_vec()]
}

/// `http2.connect('http://host:port')` on the loop, in place of the private
/// `current_thread` tokio runtime the `h2` client built **per session** — and
/// the second one `start_client_request` built **per request** (perry#10327).
///
/// `tls` turns the connection into an `https://` one: the servername to verify
/// and offer as SNI, and whether to verify at all. It is `Some` for exactly the
/// authorities Node would have put TLS under, and it is what makes **ALPN**
/// reachable — `http2.connect('https://…')` may only speak HTTP/2 if the server
/// selected `h2`, and until `perry_ext_net::turnloop_tls_io` grew a public
/// client installer there was no way to ask.
///
/// Returns the connection id, or `None` when the caller must keep the `h2`
/// path. The connect itself is asynchronous: `NET_CONNECT` installs the TLS
/// session (or starts the core, for cleartext).
pub(crate) fn connect_client(
    session_handle: i64,
    host: &str,
    port: u16,
    tls: Option<ClientTls>,
) -> Option<i64> {
    let id = super::next_id();
    if id == perry_ffi::INVALID_HANDLE {
        return None;
    }
    insert(H2Conn {
        id,
        role: Role::Client,
        server_handle: 0,
        session_handle,
        core: None,
        input: Vec::with_capacity(16 * 1024),
        streams: Vec::new(),
        secure: tls.is_some(),
        // A client handshake has not started yet — `on_connect` installs the
        // session once the socket is up — but the flag has to be set here,
        // because `on_data` reads it to decide whether the first bytes are a
        // ServerHello or an HTTP/2 preface.
        handshaking: tls.is_some(),
        connecting: true,
        client_tls: tls,
        alpn: None,
        peer_address: String::new(),
        peer_port: port,
        buffered: 0,
        // Node's `maxSessionMemory` default, in bytes.
        max_session_memory: 10 * 1024 * 1024,
        timer: Timer::None,
        draining: false,
        closing: false,
        read_eof: false,
        destroyed: false,
        queued_opens: Vec::new(),
        allow_http1: false,
        // What this connection's core will actually advertise, so
        // `clamp_to_core` in `control.rs` has something truthful to clamp a
        // later `session.settings()` against.
        settings: advertised_settings(&Http2SettingsState::default()),
        preface_done: false,
        core_settings_acked: false,
        owed_settings_acks: 0,
        goaway_opaque: Vec::new(),
        peer_settings: None,
        pending_controls: Vec::new(),
    });
    // Node sets TCP_NODELAY on an HTTP/2 client socket.
    if tl::tcp_connect(id, super::SUBSYSTEM, host, port, true).is_err() {
        forget(id);
        perry_ffi::free_handle_id(id);
        return None;
    }
    Some(id)
}

fn on_connect(id: i64) {
    let local_port = tl::local_address(id).map(|e| e.port).unwrap_or(0);
    let ready = with_owned(id, |c| {
        c.connecting = false;
        c.peer_address = tl::peer_address(id).map(|e| e.address).unwrap_or_default();
        // `local_server_session_event_ready` pairs a loopback client with its
        // server session by the client's local port; without it the server's
        // `'session'` event never fires on an in-process pair.
        crate::server::http2_server::bind_turnloop_client_port(c.session_handle, local_port);
        if c.secure {
            // The TLS handshake starts now; the core waits for ALPN. The
            // configuration is taken rather than cloned — a second connect on
            // the same id cannot happen, and leaving it behind would be a
            // second place the servername could be read from.
            return Started::Tls(c.client_tls.take());
        }
        if start_core(c) {
            Started::Ready
        } else {
            Started::Waiting
        }
    });
    match ready {
        Some(Started::Ready) => {
            if tl::read_start(id).is_err() {
                destroy_connection(id);
                return;
            }
            client_transport_ready(id);
        }
        Some(Started::Tls(tls)) => {
            // Reads start BEFORE the ClientHello goes out: the installer writes
            // it inside the call below, and a ServerHello that arrived before
            // the multishot read was armed would have nowhere to land.
            if tl::read_start(id).is_err() {
                destroy_connection(id);
                return;
            }
            let Some(tls) = tls else {
                destroy_connection(id);
                return;
            };
            if perry_ext_net::turnloop_tls_io::install_client_session(
                id,
                tls.servername,
                tls.verify,
                client_alpn(),
                tls.ca,
            )
            .is_err()
            {
                destroy_connection(id);
            }
        }
        Some(Started::Waiting) => {
            if tl::read_start(id).is_err() {
                destroy_connection(id);
            }
        }
        None => {}
    }
}

/// What `on_connect` decided, so the TLS install happens outside the table
/// borrow: the installer writes to the socket and can destroy the connection.
enum Started {
    /// Cleartext, core running — announce `'connect'`.
    Ready,
    /// Install this client session; ALPN decides the rest.
    Tls(Option<ClientTls>),
    /// Cleartext, but the core could not start.
    Waiting,
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
        super::control::drain_pending(c);
        let queued = std::mem::take(&mut c.queued_opens);
        for open in queued {
            stream::open_client_stream(c, open);
        }
    });
    flush_id(id);
}

// ── Data ────────────────────────────────────────────────────────────────────

fn on_data(id: i64, bytes: &[u8]) {
    if !peek(id, |c| c.secure).unwrap_or(false) {
        feed(id, bytes);
        return;
    }
    let Some(received) = perry_ext_net::turnloop_tls_io::receive(id, bytes) else {
        // The layer is gone (the handshake failed and destroyed the
        // connection); there is nothing to decode.
        return;
    };
    let text = received.plaintext;
    if received.peer_closed {
        // A TLS close_notify is the readable EOF.
        if !text.is_empty() {
            feed(id, &text);
        }
        on_eof(id);
        return;
    }
    if peek(id, |c| c.handshaking).unwrap_or(false)
        && perry_ext_net::turnloop_tls_io::handshake_done(id)
    {
        // `text` is handed over, not dropped. The handshake's last flight and
        // the peer's first application bytes routinely arrive in one read — a
        // TLS 1.3 client sends `Finished` and its request back to back — and an
        // ALPN handoff that kept only `conn.input` would lose the request that
        // decided the handoff. Measured: `curl --http1.1` against
        // `createSecureServer({ allowHTTP1: true })` hung, every time.
        if !finish_handshake(id, &text) {
            return;
        }
    }
    if !text.is_empty() {
        feed(id, &text);
    }
}

/// ALPN has been decided. Either start the HTTP/2 core, hand the whole
/// connection to P5's HTTP/1.1 server, or refuse it.
///
/// `pending` is the plaintext decrypted by the same `NET_DATA` that completed
/// the handshake; a handoff takes it with the connection, and an HTTP/2
/// connection leaves it to the caller to feed.
///
/// Returns false when the connection is no longer ours.
fn finish_handshake(id: i64, pending: &[u8]) -> bool {
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
            hand_to_http1(id, pending);
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
fn hand_to_http1(id: i64, pending: &[u8]) {
    let Some(conn) = forget(id) else { return };
    let mut leftover = conn.input;
    leftover.extend_from_slice(pending);
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

/// What the pre-scan did with the frame at the head of the input buffer.
#[derive(PartialEq, Eq, Debug)]
enum Prescan {
    /// The frame was taken out of the stream entirely; the core never sees it.
    Consumed,
    /// Nothing was consumed; the core decodes the frame next.
    Pass,
}

/// Read the frame at the head of the buffer for the three facts `Connection`
/// does not surface, and withhold a SETTINGS acknowledgement that belongs to a
/// `session.settings()` frame this module wrote out of band.
///
/// See the module docs for why the second decode is not redundant.
fn prescan(conn: &mut H2Conn) -> Prescan {
    let Some((kind, flags, _stream, len)) = peek_frame(conn) else {
        return Prescan::Pass;
    };
    match kind {
        // SETTINGS
        4 if flags & 1 != 0 => {
            if len == 0 && conn.core_settings_acked && conn.owed_settings_acks > 0 {
                conn.owed_settings_acks -= 1;
                conn.input.drain(..9);
                crate::server::http2_server::complete_turnloop_settings(conn.session_handle);
                return Prescan::Consumed;
            }
        }
        4 => {
            conn.peer_settings = Some(decode_settings(&conn.input[9..9 + len]));
        }
        // GOAWAY: everything past the 8-byte header is Node's `opaqueData`.
        7 if len > 8 => {
            conn.goaway_opaque = conn.input[9 + 8..9 + len].to_vec();
        }
        7 => conn.goaway_opaque.clear(),
        _ => {}
    }
    Prescan::Pass
}

fn pump(id: i64) {
    let outcome = with_owned(id, |conn| {
        let mut fatal = None;
        loop {
            if conn.destroyed {
                return Outcome::Gone;
            }
            if prescan(conn) == Prescan::Consumed {
                continue;
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
                if !conn.preface_done && conn.role == Role::Server {
                    // The preface step is the only one that consumes with no
                    // event before any frame has been read.
                    conn.preface_done = true;
                }
            }
            if !conn.core_settings_acked
                && conn
                    .core
                    .as_ref()
                    .is_some_and(|core| core.next_timeout().is_none())
            {
                conn.core_settings_acked = true;
                crate::server::http2_server::mark_turnloop_settings_acked(conn.session_handle);
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
            kind,
        } => stream::on_headers(conn, id, headers, end_stream, kind),
        Owned::Data {
            stream: id,
            bytes,
            end_stream,
        } => stream::on_data(conn, id, bytes, end_stream),
        Owned::Reset { stream: id, code } => stream::on_reset(conn, id, code),
        Owned::Goaway { last_stream, code } => {
            conn.draining = true;
            let opaque = std::mem::take(&mut conn.goaway_opaque);
            stream::on_goaway(conn, last_stream, code, opaque);
        }
        Owned::Ping { ack, data } => stream::on_ping(conn, ack, data),
        // Deliberately nothing: see `Event::Unprocessed` above. Node sends no
        // frame and never surfaces the request, and so do we.
        Owned::Unprocessed => {}
        // Deliberately nothing: turnloop-http applies the acknowledged settings
        // itself, and before alpha.7 made this an event the decoder stepped
        // over it silently. Ignoring it keeps that exact behaviour.
        Owned::SettingsAck => {}
        // A peer window opened: retry whatever stalled. Which window — the
        // connection's or one stream's — does not matter, because
        // `pump_outbox` walks every stream and `send_data` answers zero for
        // any that is still shut.
        Owned::WindowUpdate => stream::pump_outbox(conn),
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

/// Write a frame this module encoded itself, **after** everything the core has
/// already queued.
///
/// `Connection` has no API for a second SETTINGS, for
/// `goaway(code, last, opaque)`, or for a WINDOW_UPDATE the host chose, so
/// those frames are hand-encoded (see `control.rs`). Order is the whole point:
/// a raw frame written while `core.output()` still holds bytes would arrive
/// *before* them and interleave two frame streams, which is a protocol error on
/// the peer's side rather than a Perry-side bug that anything here would catch.
pub(crate) fn write_raw(conn: &mut H2Conn, frame: &[u8]) {
    if frame.is_empty() || conn.destroyed {
        return;
    }
    flush(conn);
    let written = if conn.secure {
        perry_ext_net::turnloop_tls_io::write(conn.id, frame, 0)
            .map(|_| ())
            .map_err(|_| ())
    } else {
        tl::write(conn.id, frame, 0).map(|_| ()).map_err(|_| ())
    };
    if written.is_err() {
        conn.destroyed = true;
        let _ = tl::close(conn.id);
    }
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
    // `forget` may find nothing — an earlier failed write dropped the record
    // and left only the ownership entry — and the cleanup below still has to
    // run, because it is the ownership entry and the id itself that leak.
    if let Some(mut conn) = forget(id) {
        conn.destroyed = true;
        let live: Vec<u32> = conn.streams.iter().map(|s| s.h2_id).collect();
        for stream_id in live {
            stream::terminate(&mut conn, stream_id, Some("ECONNRESET"));
        }
        crate::server::http2_server::mark_turnloop_session_closed(conn.session_handle);
        if conn.server_handle != 0 {
            crate::server::server::turnloop_connection_closed(id);
        }
    } else {
        owned_ids()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
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

#[cfg(test)]
mod prescan_tests {
    //! The pre-scan is the subtlest thing in this module and h2spec cannot
    //! reach it: h2spec never makes Perry send a second SETTINGS, so nothing in
    //! the conformance run exercises the ack-withholding at all. Getting it
    //! wrong in either direction is a **connection** error — stealing the
    //! core's own acknowledgement leaves `settings_awaiting_ack` set until the
    //! SETTINGS deadline kills the session, and failing to steal ours lets the
    //! core answer `protocol("unsolicited SETTINGS ack")`.

    use super::*;

    fn conn(owed: u32, core_acked: bool) -> H2Conn {
        H2Conn {
            id: 0,
            role: Role::Server,
            server_handle: 0,
            // Zero: every glue call this module makes returns early on it, so a
            // pre-scan test touches no handle registry.
            session_handle: 0,
            core: None,
            input: Vec::new(),
            streams: Vec::new(),
            secure: false,
            handshaking: false,
            connecting: false,
            client_tls: None,
            alpn: None,
            peer_address: String::new(),
            peer_port: 0,
            buffered: 0,
            max_session_memory: 10 * 1024 * 1024,
            timer: Timer::None,
            draining: false,
            closing: false,
            read_eof: false,
            destroyed: false,
            queued_opens: Vec::new(),
            allow_http1: false,
            settings: Http2SettingsState::default(),
            preface_done: true,
            core_settings_acked: core_acked,
            owed_settings_acks: owed,
            goaway_opaque: Vec::new(),
            peer_settings: None,
            pending_controls: Vec::new(),
        }
    }

    fn frame(kind: u8, flags: u8, stream: u32, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        http2::encode_frame(kind, flags, stream, payload, &mut out).expect("encode");
        out
    }

    /// The handshake's own acknowledgement belongs to the core. Eating it would
    /// leave `settings_awaiting_ack` set forever and the SETTINGS deadline
    /// would fail the connection ten seconds later, somewhere else entirely.
    #[test]
    fn the_cores_own_ack_is_never_stolen() {
        let mut c = conn(1, false);
        c.input = frame(4, 1, 0, &[]);
        assert!(prescan(&mut c) == Prescan::Pass);
        assert_eq!(c.input.len(), 9, "the ack must still be there for the core");
        assert_eq!(c.owed_settings_acks, 1, "and it must not have been counted");
    }

    /// Exactly as many acknowledgements are withheld as this module sent
    /// SETTINGS frames; the next one belongs to the core again.
    #[test]
    fn exactly_the_owed_acks_are_withheld() {
        let mut c = conn(2, true);
        let ack = frame(4, 1, 0, &[]);
        c.input.extend_from_slice(&ack);
        c.input.extend_from_slice(&ack);
        c.input.extend_from_slice(&ack);
        assert!(prescan(&mut c) == Prescan::Consumed);
        assert_eq!(c.owed_settings_acks, 1);
        assert!(prescan(&mut c) == Prescan::Consumed);
        assert_eq!(c.owed_settings_acks, 0);
        assert!(prescan(&mut c) == Prescan::Pass);
        assert_eq!(c.input.len(), 9, "the third ack is the core's");
    }

    /// A SETTINGS ack with a payload is a FRAME_SIZE_ERROR, and only the core
    /// can raise it. Swallowing the frame would turn a protocol violation into
    /// silence.
    #[test]
    fn a_malformed_ack_is_left_for_the_core() {
        let mut c = conn(1, true);
        c.input = frame(4, 1, 0, &[0; 6]);
        assert!(prescan(&mut c) == Prescan::Pass);
        assert_eq!(c.input.len(), 15);
        assert_eq!(c.owed_settings_acks, 1);
    }

    /// `session.remoteSettings` comes from the pre-scan, not from the core's
    /// `Event::Settings` (see the module docs' pre-scan note).
    #[test]
    fn peer_settings_values_are_captured() {
        let mut c = conn(0, true);
        let mut payload = Vec::new();
        for (id, value) in [(3u16, 7u32), (4, 1 << 20), (6, 9)] {
            payload.extend_from_slice(&id.to_be_bytes());
            payload.extend_from_slice(&value.to_be_bytes());
        }
        c.input = frame(4, 0, 0, &payload);
        assert!(prescan(&mut c) == Prescan::Pass);
        let seen = c.peer_settings.expect("captured");
        assert_eq!(seen.max_concurrent_streams, 7);
        assert_eq!(seen.initial_window_size, 1 << 20);
        assert_eq!(seen.max_header_list_size, 9);
        // An identifier the peer did not send stays at the protocol default.
        assert_eq!(seen.max_frame_size, 16_384);
        // And the frame is still there for the core, which has its own work to
        // do with these values.
        assert_eq!(c.input.len(), 9 + payload.len());
    }

    /// RFC 9113 §6.8's Additional Debug Data, which this module takes from the
    /// pre-scan rather than from `Event::Goaway`'s alpha.7 `debug` field.
    #[test]
    fn goaway_opaque_data_is_captured_and_cleared() {
        let mut c = conn(0, true);
        let mut payload = vec![0, 0, 0, 5, 0, 0, 0, 2];
        payload.extend_from_slice(b"why");
        c.input = frame(7, 0, 0, &payload);
        assert!(prescan(&mut c) == Prescan::Pass);
        assert_eq!(c.goaway_opaque, b"why");

        // A second GOAWAY with no debug data must not inherit the first's.
        let mut c2 = conn(0, true);
        c2.goaway_opaque = b"stale".to_vec();
        c2.input = frame(7, 0, 0, &[0, 0, 0, 5, 0, 0, 0, 2]);
        assert!(prescan(&mut c2) == Prescan::Pass);
        assert!(c2.goaway_opaque.is_empty());
    }

    /// Before the client preface has been consumed the buffer starts with
    /// `PRI * HTTP/2.0…`, which decodes as a frame header of some absurd kind.
    /// Reading it would be reading noise.
    #[test]
    fn nothing_is_peeked_before_the_preface() {
        let mut c = conn(4, true);
        c.preface_done = false;
        c.input = http2::PREFACE.to_vec();
        c.input.extend_from_slice(&frame(4, 1, 0, &[]));
        assert!(prescan(&mut c) == Prescan::Pass);
        assert_eq!(c.owed_settings_acks, 4);
        assert_eq!(c.input.len(), http2::PREFACE.len() + 9);
    }

    /// A partial frame is nobody's: the host must wait for the rest rather than
    /// act on a length it has not received.
    #[test]
    fn a_partial_frame_is_not_peeked() {
        let mut c = conn(1, true);
        let full = frame(4, 0, 0, &[0, 3, 0, 0, 0, 7]);
        c.input = full[..full.len() - 1].to_vec();
        assert!(prescan(&mut c) == Prescan::Pass);
        assert!(c.peer_settings.is_none());
    }
}
