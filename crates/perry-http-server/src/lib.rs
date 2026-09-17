//! The HTTP/1.1 server core on turnloop, for bindings that are not
//! `perry-ext-http`.
//!
//! # Why this crate exists
//!
//! turnloop P5 migrated `node:http` off hyper, and the machine it built —
//! one multishot `accept_start`, one multishot `read_start`, a sans-I/O
//! `turnloop_http::http1` codec, Node's framing rules and its idle-close
//! arithmetic — lives inside `perry-ext-http`'s private
//! `server::turnloop_serve` module. Two other servers in the tree need exactly
//! that machine and cannot reach it:
//!
//! * **`perry-ext-fastify`** carries its own hyper accept loop. It has no
//!   dependency edge to `perry-ext-http`, and adding one would be worse than
//!   the duplication it removes: `perry-ext-http` still needs hyper (worker
//!   agents, cluster workers, an attached `WebSocketServer`, `reqwest`), so the
//!   edge would put hyper straight back into fastify's tree — transitively
//!   instead of directly. `scripts/tokio_inventory.py` gates *manifest* edges,
//!   so that swap would turn the gate green while every fastify program still
//!   linked hyper. It would also make a framework binding depend on a Node
//!   builtin binding, and — because both are `staticlib`s — bundle all of
//!   `perry-ext-http`'s objects (hyper, h2, reqwest, rustls) into
//!   `libperry_ext_fastify.a`.
//! * **`perry-stdlib`'s `framework/server.rs`**, the bundled server the
//!   well-known flip replaces. It must not depend on the wrapper it is the
//!   fallback for, and it cannot reach a private module of it either.
//!
//! So the core moved down instead of sideways. This crate is the server
//! counterpart of `perry-http-client`: a small, tokio-free crate that several
//! bindings depend on, rather than a binding other bindings depend on.
//!
//! # What a consumer implements
//!
//! One [`Host`]. Its methods run inside the completion sink, which means they
//! run on the loop-owning thread **after** a turn has returned — so a host may
//! allocate Rust state and register handles, but it must **not run JS**. The
//! shape every consumer uses is P5's: [`Host::on_request`] pushes the decoded
//! request onto a queue, and the binding's existing main-thread pump dispatches
//! it on its own tick, exactly where an `mpsc` used to deliver it. The
//! event-loop phase order does not move; what disappears is the thread hop, the
//! channel and the cross-thread notify.
//!
//! A response is submitted with [`respond`] (buffered) or
//! [`stream_begin`] / [`stream_body`] / [`stream_end`].
//!
//! # What this crate does not do
//!
//! * **TLS.** This crate serves cleartext HTTP/1.1, and there is deliberately
//!   no seam for anything else. An earlier draft carried a `TlsLayer` trait so
//!   `perry-ext-http`'s later migration would find one ready — six methods and
//!   a branch on every read, write and close, that **no consumer implements**.
//!   CLAUDE.md's rule for exactly that shape ("a mode that still exists is a
//!   decision that hasn't been made… the losing mode should stop compiling")
//!   applies to a server core as much as to a GC knob, so it is gone. The
//!   migration that needs TLS adds the seam against its real caller, which is
//!   a better design than a guess nothing exercises.
//! * **HTTP/2.** `perry-ext-http`'s `turnloop_h2` is a second, larger surface
//!   (its own stream handles, settings, ALPN and flow control) and is not part
//!   of this core. Neither consumer here serves HTTP/2: fastify declares
//!   hyper's `http2` feature but has only ever built an `http1::Builder`.
//! * **HTTP/2** — see above. A protocol *upgrade*, on the other hand, is now
//!   here: [`Host::takes_upgrades`] / [`Host::on_upgrade`] / [`Host::on_upgraded`]
//!   hand the connection over. The hook was withheld while its only possible
//!   caller could not use it — `perry-ext-ws` needed an owned
//!   `AsyncRead + AsyncWrite` stream a turnloop connection cannot produce — and
//!   it went in with that caller, not before: `perry-ext-ws`'s standalone
//!   `WebSocketServer({ port })` is a `Host` whose `on_upgrade` answers the
//!   `101` and adopts the connection into `turnloop_websocket`'s sans-I/O
//!   codec. A host that leaves [`Host::takes_upgrades`] false is unaffected:
//!   an upgrade request is still served as an ordinary request, which is what
//!   Node does when no `'upgrade'` listener exists (#4973), and
//!   [`Request::upgrade`] still says which it was.
//!
//! # GC
//!
//! Nothing here holds a JS value or a heap pointer: a connection holds owned
//! `Vec<u8>`s and integer ids. This crate therefore registers no root scanner,
//! which is the same rule P1 set for the net layer and P5 kept for the HTTP
//! one. A consumer that carries handle *ids* through [`Request`] keeps that
//! property; one that cached a `*mut` here would not, and would be invisible to
//! `scripts/gc_root_dominance_check.py` (see
//! `docs/src/internals/gc-rooting-invariant.md`).

mod conn;
pub mod wire;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use perry_ffi::turnloop_net as tl;

pub use conn::{
    connections_of, destroy, finish, is_busy, respond, send_interim, stream_begin, stream_body,
    stream_end, write_raw,
};
pub use wire::{align_headers, body_forbidden, encode_head, framing_for, Framing};

/// A fully decoded request, handed to [`Host::on_request`].
///
/// `conn_id` and `seq` together name the exchange a response answers; keep
/// both and pass them to [`respond`]. A `seq` that no longer names the
/// connection's active request is ignored rather than mis-delivered, which is
/// what makes a late response from an abandoned handler harmless.
#[derive(Debug, Clone)]
pub struct Request {
    pub conn_id: i64,
    pub seq: u64,
    pub method: String,
    /// The request target exactly as sent: path plus query string.
    pub target: String,
    /// 0 for HTTP/1.0, 1 for HTTP/1.1 — `turnloop_http::http1::Head::version`.
    pub version: u8,
    /// Header names lowercased by the decoder, in arrival order.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub peer_address: String,
    pub peer_port: u16,
    /// The request carried `Expect: 100-continue`. The core has already
    /// written the interim `100 Continue` unless the host asked it not to
    /// through [`Host::intercepts_continue`].
    pub expects_continue: bool,
    /// `Connection: upgrade` with an `Upgrade` header. The request is
    /// dispatched as an ordinary one either way — Node only diverts an upgrade
    /// when an `'upgrade'` listener exists (#4973), and no consumer of this
    /// crate has one on the turnloop path — so this is how a host tells.
    pub upgrade: bool,
    /// How many requests this connection has decoded, including this one.
    pub request_number: u64,
}

impl Request {
    /// The first value of `name`, matched case-insensitively.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// A response to write. `headers` is emitted verbatim, in order, with the case
/// the caller used — Node preserves both.
#[derive(Debug, Clone, Default)]
pub struct Response {
    pub status: u16,
    /// A custom reason phrase, observable on the wire in Node.
    pub status_message: Option<String>,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub trailers: Vec<(String, String)>,
    /// True when any `Content-Length` in `headers` was synthesized by the
    /// caller rather than set by the application. Node sends none on 204, 304,
    /// 1xx, a HEAD response or a close-delimited HTTP/1.0 body, but keeps one
    /// the application set; this flag is what tells the two apart.
    pub auto_content_length: bool,
}

/// What a consumer plugs into the core. One per listening server.
///
/// Every method runs inside the completion sink: after a turn, on the
/// loop-owning thread, and **never** running JS. See the module header.
pub trait Host: Send + Sync + 'static {
    /// A fully decoded request. Queue it; do not dispatch it here.
    fn on_request(&self, request: Request);

    /// The connection carrying `seq` died before its response was written.
    /// Node raises `'aborted'` on the request; the sink cannot run JS, so a
    /// consumer queues that for its own pump.
    fn on_aborted(&self, _conn_id: i64, _seq: u64) {}

    /// A connection was accepted (Node's `'connection'`).
    fn on_connection(&self, _conn_id: i64) {}

    /// `server.close()` is in progress: answer `Connection: close` and do not
    /// reuse the connection. In-flight requests still complete, which is Node's
    /// contract.
    fn is_closing(&self) -> bool {
        false
    }

    /// Node's `server.maxRequestsPerSocket`; zero means no limit.
    fn max_requests_per_socket(&self) -> u64 {
        0
    }

    /// `server.keepAliveTimeout`, in ms — what the `Keep-Alive` response
    /// header advertises. Zero advertises nothing, which is Node's meaning.
    fn keep_alive_timeout_ms(&self) -> f64 {
        5_000.0
    }

    /// Take over the automatic `100 Continue`. Node sends it unless a
    /// `'checkContinue'` listener exists; a consumer with no such surface
    /// leaves this false and the core writes it.
    fn intercepts_continue(&self) -> bool {
        false
    }

    /// Divert `Connection: upgrade` requests to [`Host::on_upgrade`] instead of
    /// [`Host::on_request`].
    ///
    /// A listener-level decision, not a per-request one, because that is the
    /// shape of the thing it models: Node diverts an upgrade only when the
    /// server has an `'upgrade'` listener, and both consumers here answer the
    /// same way for every upgrade on a given server. It is read once per
    /// decoded upgrade request, the same way [`Host::intercepts_continue`] is
    /// read once per `Expect: 100-continue`.
    fn takes_upgrades(&self) -> bool {
        false
    }

    /// An upgrade request on a host that asked for them.
    ///
    /// The core has already stopped decoding HTTP on `request.conn_id`: it
    /// will not parse another request, arm another idle deadline or answer
    /// anything on that connection. The host writes its own `101` (or its
    /// refusal) with [`write_raw`], and from then on every byte, half-close,
    /// error and terminal close arrives at [`Host::on_upgraded`].
    ///
    /// `leftover` is whatever followed the request head in the same read —
    /// bytes the peer pipelined behind its handshake, which belong to the
    /// upgraded protocol and would otherwise be parsed as a second HTTP
    /// request.
    ///
    /// Like every other method here this runs inside the completion sink, so
    /// it must not run JS.
    fn on_upgrade(&self, _request: Request, _leftover: Vec<u8>) {}

    /// A transport event on a connection this host took over.
    fn on_upgraded(&self, _conn_id: i64, _event: Upgraded<'_>) {}
}

/// What happened on a connection a host took over through
/// [`Host::on_upgrade`].
///
/// One enum rather than four trait methods: a host that takes upgrades must
/// handle all four, and a default-empty method per event is four places for
/// one to be forgotten silently.
#[derive(Debug)]
pub enum Upgraded<'a> {
    /// Bytes arrived. Borrowed for the duration of the call only — they live
    /// in turnloop's pooled read buffer.
    Data(&'a [u8]),
    /// The peer closed its write side.
    Eof,
    /// A transport error. The connection is being torn down.
    Error(&'a str),
    /// The connection's final completion; no event can name it again.
    Closed,
}

/// A bound listener and the host it serves.
pub(crate) struct Listener {
    pub(crate) host: Arc<dyn Host>,
    /// The completion-sink slot this listener's connections were opened on.
    /// A `NET_TIMER` has to be armed on the same slot the connection lives in,
    /// and the connection only knows its listener.
    pub(crate) subsystem: u8,
    /// Node's idle close: `keepAliveTimeout + keepAliveTimeoutBuffer`, in ms,
    /// with **zero meaning never** (Node 26.5.1, measured — see
    /// `docs/turnloop/p5-report.md`).
    pub(crate) idle_close_ms: u64,
}

fn listeners() -> &'static Mutex<HashMap<i64, Listener>> {
    static LISTENERS: OnceLock<Mutex<HashMap<i64, Listener>>> = OnceLock::new();
    LISTENERS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn with_listener<R>(id: i64, f: impl FnOnce(&Listener) -> R) -> Option<R> {
    let map = listeners().lock().unwrap_or_else(|e| e.into_inner());
    map.get(&id).map(f)
}

/// The sink slot a listener's connections live on. A listener that has already
/// been closed while one of its connections is still draining reports slot 0,
/// which owns no timer of this crate's — an unarmed deadline on a dying
/// connection, rather than one armed on someone else's slot.
pub(crate) fn subsystem_of_listener(id: i64) -> u8 {
    with_listener(id, |l| l.subsystem).unwrap_or(0)
}

/// One authoritative id domain for the listeners and connections this crate
/// allocates. The runtime keys its `Entry` map by this id across every
/// subsystem, so it has to be globally unique.
fn registry_domain() -> perry_ffi::NativeRegistryDomain {
    static DOMAIN: OnceLock<perry_ffi::NativeRegistryDomain> = OnceLock::new();
    *DOMAIN.get_or_init(|| {
        perry_ffi::NativeRegistryDomain::new().expect("http server registry domains exhausted")
    })
}

pub(crate) fn next_id() -> i64 {
    perry_ffi::reserve_handle_id_in_domain(registry_domain())
}

extern "C" fn alloc_id() -> i64 {
    let id = next_id();
    if id == perry_ffi::INVALID_HANDLE {
        0
    } else {
        id
    }
}

/// Whether a server created *now, on this thread* can live on turnloop.
///
/// Deliberately not cached: availability is a property of the calling agent,
/// and the sink registration is refused outright if the runtime's completion
/// layout does not match this crate's — which leaves `available` false and
/// keeps the consumer on its own fallback rather than submitting work nothing
/// can deliver.
pub fn available(subsystem: u8) -> bool {
    registered_subsystems(subsystem);
    tl::available(subsystem)
}

/// Install this crate's sink for `subsystem`, once per subsystem.
///
/// One sink function serves every subsystem: it routes by connection id, and
/// ids come from a single global domain, so two consumers on two slots never
/// see each other's completions.
fn registered_subsystems(subsystem: u8) {
    // Width must be at least the runtime's `MAX_SUBSYSTEMS`; a slot past the
    // end is simply never remembered as registered, so it would re-register on
    // every `listen()` rather than mis-route.
    static REGISTERED: Mutex<[bool; 16]> = Mutex::new([false; 16]);
    let slot = subsystem as usize;
    let mut guard = REGISTERED.lock().unwrap_or_else(|e| e.into_inner());
    if slot >= guard.len() || guard[slot] {
        return;
    }
    guard[slot] = true;
    tl::register_sink(subsystem, conn::sink, alloc_id);
}

/// Where a listener bound, as `server.address()` must report it after an
/// ephemeral `listen(0)`.
#[derive(Debug, Clone)]
pub struct Bound {
    pub listener_id: i64,
    pub port: u16,
    pub address: String,
}

/// Bind and start accepting.
///
/// The bind is synchronous, so the bound port is correct inside a
/// `listen(0, cb)` callback.
///
/// `reuse_port` sets `SO_REUSEPORT` — what a `cluster.fork()` worker and an
/// explicit `{ reusePort: true }` need. `no_delay` disables Nagle on every
/// accepted connection, which is where Node applies `noDelay` (a server option,
/// not a per-socket one) and what `http.createServer` defaults to true.
///
/// These two are separate arguments for a reason: P5's listen path had
/// `no_delay` sitting in `reuse_port`'s slot for its whole life, so every
/// turnloop HTTP listener bound with `SO_REUSEPORT` on and Nagle on — a second
/// `listen()` on the same port quietly succeeded where Node answers
/// `EADDRINUSE`, and no connection ever got `TCP_NODELAY`. Pass them by name.
#[allow(clippy::too_many_arguments)]
pub fn listen(
    subsystem: u8,
    host: Arc<dyn Host>,
    bind_host: &str,
    port: u16,
    backlog: u32,
    reuse_port: bool,
    no_delay: bool,
    idle_close_ms: u64,
) -> Result<Bound, tl::NetError> {
    registered_subsystems(subsystem);
    let id = next_id();
    if id == perry_ffi::INVALID_HANDLE {
        return Err(tl::error_from_os(None, "listen"));
    }
    tl::tcp_listen(
        id, subsystem, bind_host, port, backlog, reuse_port, no_delay,
    )?;
    tl::accept_start(id)?;
    let bound = tl::local_address(id);
    let bound_port = bound.as_ref().map(|e| e.port).unwrap_or(port);
    let address = bound
        .as_ref()
        .map(|e| e.address.clone())
        .unwrap_or_else(|| bind_host.to_string());
    listeners()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(
            id,
            Listener {
                host,
                subsystem,
                idle_close_ms,
            },
        );
    Ok(Bound {
        listener_id: id,
        port: bound_port,
        address,
    })
}

/// Stop accepting. In-flight connections finish, which is Node's contract for
/// `server.close()`; tearing those down is `closeAllConnections`.
pub fn close_listener(id: i64) {
    listeners()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&id);
    let _ = tl::close(id);
}

/// Node's `Connection` / `Keep-Alive` response headers, for a response that has
/// not set `Connection` itself.
///
/// Returns whether the connection is to be reused. The two decisions are
/// **separate**, which is the correction P5 measured on Node 26.5.1: reuse
/// comes from the protocol version and the request's `Connection` tokens alone,
/// while `keep_alive_timeout_ms` only decides whether a `Keep-Alive: timeout=N`
/// advertises one. Folding them together — as Perry did — makes a server with
/// `keepAliveTimeout = 0` answer `Connection: close` on every response and get
/// no reuse at all, where Node keeps the connection open forever.
pub fn connection_headers(
    headers: &mut Vec<(String, String)>,
    request_version: u8,
    request_connection: Option<&str>,
    keep_alive_timeout_ms: f64,
    force_close: bool,
) -> bool {
    if let Some((_, value)) = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("connection"))
    {
        return value.eq_ignore_ascii_case("keep-alive");
    }

    let conn_lower = request_connection.map(str::to_ascii_lowercase);
    let has_token = |tok: &str| {
        conn_lower
            .as_deref()
            .map(|c| c.split(',').any(|t| t.trim() == tok))
            .unwrap_or(false)
    };
    // HTTP/1.0 defaults to close (keep-alive only when explicitly requested);
    // HTTP/1.1 defaults to keep-alive unless asked to close.
    let should_keep_alive = !force_close
        && if request_version == 0 {
            has_token("keep-alive")
        } else {
            !has_token("close")
        };

    if !should_keep_alive {
        headers.push(("Connection".to_string(), "close".to_string()));
        return false;
    }
    headers.push(("Connection".to_string(), "keep-alive".to_string()));
    if keep_alive_timeout_ms > 0.0 {
        let secs = (keep_alive_timeout_ms / 1000.0).floor().max(0.0) as u64;
        headers.push(("Keep-Alive".to_string(), format!("timeout={secs}")));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn_value(headers: &[(String, String)]) -> Option<&str> {
        headers
            .iter()
            .find(|(k, _)| k == "Connection")
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn http11_defaults_to_keep_alive_and_advertises_the_timeout() {
        let mut headers = Vec::new();
        assert!(connection_headers(&mut headers, 1, None, 5_000.0, false));
        assert_eq!(conn_value(&headers), Some("keep-alive"));
        assert_eq!(
            headers
                .iter()
                .find(|(k, _)| k == "Keep-Alive")
                .map(|(_, v)| v.as_str()),
            Some("timeout=5")
        );
    }

    #[test]
    fn a_zero_timeout_keeps_the_connection_and_advertises_nothing() {
        // The P5 correction: zero means "no timeout", not "no keep-alive".
        let mut headers = Vec::new();
        assert!(connection_headers(&mut headers, 1, None, 0.0, false));
        assert_eq!(conn_value(&headers), Some("keep-alive"));
        assert!(headers.iter().all(|(k, _)| k != "Keep-Alive"));
    }

    #[test]
    fn a_sub_second_timeout_advertises_zero_seconds() {
        let mut headers = Vec::new();
        assert!(connection_headers(&mut headers, 1, None, 300.0, false));
        assert_eq!(
            headers
                .iter()
                .find(|(k, _)| k == "Keep-Alive")
                .map(|(_, v)| v.as_str()),
            Some("timeout=0")
        );
    }

    #[test]
    fn http10_needs_an_explicit_keep_alive_token() {
        let mut implicit = Vec::new();
        assert!(!connection_headers(&mut implicit, 0, None, 5_000.0, false));
        assert_eq!(conn_value(&implicit), Some("close"));

        let mut explicit = Vec::new();
        assert!(connection_headers(
            &mut explicit,
            0,
            Some("keep-alive"),
            5_000.0,
            false
        ));
        assert_eq!(conn_value(&explicit), Some("keep-alive"));
    }

    #[test]
    fn a_close_token_and_a_forced_close_both_end_the_connection() {
        let mut asked = Vec::new();
        assert!(!connection_headers(
            &mut asked,
            1,
            Some("close"),
            5_000.0,
            false
        ));
        assert_eq!(conn_value(&asked), Some("close"));

        let mut forced = Vec::new();
        assert!(!connection_headers(&mut forced, 1, None, 5_000.0, true));
        assert_eq!(conn_value(&forced), Some("close"));
    }

    #[test]
    fn a_header_the_caller_set_is_never_overridden() {
        let mut headers = vec![("connection".to_string(), "keep-alive".to_string())];
        assert!(connection_headers(
            &mut headers,
            1,
            Some("close"),
            5_000.0,
            true
        ));
        assert_eq!(headers.len(), 1, "no second Connection header");
    }
}
