//! The outbound WebSocket client, on turnloop.
//!
//! # What moved, and what did not
//!
//! Nothing about the *protocol* moved: [`crate::handshake::ClientUpgrade`] and
//! [`crate::codec::Codec`] are `turnloop_websocket`'s sans-I/O state machines
//! over byte slices, and they were already driving both of this crate's
//! transports. What this module replaces is the four things
//! `tokio_tungstenite::connect_async` did around them — resolve the name, open
//! the socket, negotiate TLS, and read until the `101` — each of which now has
//! a turnloop primitive:
//!
//! | was | is |
//! |---|---|
//! | `tokio::net::TcpStream::connect((host, port))` (which also resolved) | `tl::tcp_connect`, whose lookup runs on the shared blocking pool |
//! | `perry_ext_net::connect_tls_client` → `tokio_rustls` | [`perry_tls_session::TlsClientSession`] above the same handle |
//! | a `tokio::spawn`ed read/write task per connection | one multishot `tl::read_start` and direct `tl::write` submissions |
//! | `tokio::sync::mpsc` carrying `send`/`close`/`terminate` | the FFI call submits where it happens |
//!
//! So a `new WebSocket(url)` no longer needs a runtime, a task or a channel.
//!
//! # Where the state lives
//!
//! Only until the `101`. Once the upgrade completes the connection is handed
//! to [`crate::turnloop_link`] — the same module `perry-ext-http` hands its
//! accepted connections to — with a [`Transport`](crate::turnloop_link::Transport)
//! that writes through this module's TLS layer. After that this module owns
//! exactly one thing per connection: the optional `TlsClientSession`. A
//! cleartext `ws://` connection keeps no state here at all once it is adopted.
//!
//! # Threading and the GC
//!
//! The sink runs on the agent thread, from the loop's own turn, so it may
//! touch this crate's registries directly. It **does not run JS**: decoded
//! frames go through [`crate::emit_incoming`] onto `WS_PENDING_EVENTS` and are
//! dispatched by `js_ws_process_pending` on its own tick, exactly as the tokio
//! task's did.
//!
//! The one JS-visible thing here is the `new WebSocket(url)` promise, held as
//! a [`JsNativeAsyncCompletion`] — the runtime's pinned, root-scanned handle
//! (#9552) — and not as a bare `*mut Promise` in a side table, which is the
//! shape `scripts/gc_runtime_root_holders.py` exists to catch. Read bytes are
//! copied out of turnloop's pooled lease before the sink returns.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use perry_ffi::turnloop_net as tl;
use perry_ffi::JsNativeAsyncCompletion;

use crate::codec::Role;
use crate::handshake::ClientUpgrade;

/// This binding's slot in the runtime's completion-sink registry.
///
/// Separate from [`crate::server::SERVER_SUBSYSTEM`] because they are two sink
/// *functions* in one binary — this module's, and `perry-http-server`'s — and
/// a slot holds one function pointer.
pub(crate) const SUBSYSTEM: u8 = 7;

/// Node's `ws` sets `TCP_NODELAY` on its sockets; a handshake that sat in
/// Nagle's queue would add a round trip to every connect.
const NODELAY: bool = true;

/// One outbound connection, from the `tcp_connect` submission to the `101`.
struct Client {
    /// The JS-visible id, allocated before the connect so `ws.on(...)` can be
    /// registered against it while it is still connecting.
    ws_id: usize,
    /// `wss://`: the TLS session this connection's bytes pass through. `None`
    /// for `ws://`.
    tls: Option<perry_tls_session::TlsClientSession>,
    /// The upgrade request, and the reader waiting for its `101`. Taken when
    /// the handshake completes.
    upgrade: Option<ClientUpgrade>,
    /// The request bytes, held until there is something to write them through
    /// — for `wss://` that is the completed TLS handshake.
    request: Vec<u8>,
    /// `new WebSocket(url)`'s promise. `None` for `js_ws_connect_start`, whose
    /// JS surface learns the outcome from `'open'` / `'error'` instead.
    token: Option<JsNativeAsyncCompletion>,
    /// The handshake finished and [`crate::turnloop_link`] owns the protocol.
    /// The entry stays for its TLS layer and for the terminal completions.
    adopted: bool,
    /// An error has been reported for this connection; the terminal close must
    /// not report a second one.
    failed: bool,
}

fn clients() -> &'static Mutex<HashMap<i64, Client>> {
    static CLIENTS: OnceLock<Mutex<HashMap<i64, Client>>> = OnceLock::new();
    CLIENTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn with_client<R>(id: i64, f: impl FnOnce(&mut Client) -> R) -> Option<R> {
    let mut map = clients().lock().unwrap_or_else(|e| e.into_inner());
    map.get_mut(&id).map(f)
}

fn forget(id: i64) -> Option<Client> {
    clients()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&id)
}

// ── Ids ─────────────────────────────────────────────────────────────────────

/// One authoritative id domain for the connections this module opens. The
/// runtime keys its handle table by this id across every subsystem, so it has
/// to be globally unique — which is also why this is a reserved domain rather
/// than a private counter.
fn registry_domain() -> perry_ffi::NativeRegistryDomain {
    static DOMAIN: OnceLock<perry_ffi::NativeRegistryDomain> = OnceLock::new();
    *DOMAIN.get_or_init(|| {
        perry_ffi::NativeRegistryDomain::new().expect("ws registry domains exhausted")
    })
}

fn next_id() -> i64 {
    perry_ffi::reserve_handle_id_in_domain(registry_domain())
}

/// This subsystem accepts nothing — an inbound connection belongs to the
/// standalone server's listener, which is `perry-http-server`'s slot. Returning
/// zero refuses, which is the right answer for an accept that cannot happen.
extern "C" fn alloc_id() -> i64 {
    0
}

// ── Availability ────────────────────────────────────────────────────────────

/// Whether a client opened *now, on this thread* can live on turnloop.
///
/// Deliberately not cached: availability is a property of the calling agent,
/// and `register_sink` is refused outright if the runtime's completion layout
/// does not match this crate's — which leaves this false rather than letting
/// the caller submit work whose completions nothing would deliver.
pub(crate) fn available() -> bool {
    static REGISTERED: std::sync::Once = std::sync::Once::new();
    REGISTERED.call_once(|| {
        tl::register_sink(SUBSYSTEM, sink, alloc_id);
    });
    tl::available(SUBSYSTEM)
}

// ── The completion sink ─────────────────────────────────────────────────────

extern "C" fn sink(completion: *const tl::NetCompletion) {
    if completion.is_null() {
        return;
    }
    // SAFETY: the runtime passes a live completion for the duration of the
    // call, which is this function's body.
    let c = unsafe { &*completion };
    match c.kind {
        tl::NET_CONNECT => on_connect(c.id),
        // SAFETY: same call; the pooled lease outlives it.
        tl::NET_DATA => on_data(c.id, unsafe { c.bytes() }),
        tl::NET_EOF => on_eof(c.id),
        tl::NET_SHUTDOWN => on_shutdown(c.id),
        tl::NET_CLOSED => on_closed(c.id),
        tl::NET_ERROR => {
            // SAFETY: the runtime builds these from `&'static str`s.
            let code = unsafe { c.code() }.unwrap_or("EPIPE");
            let syscall = unsafe { c.syscall() }.unwrap_or("");
            on_error(c.id, &message_for(code, syscall));
        }
        _ => {}
    }
}

/// Put whatever the TLS session has produced on the wire.
///
/// At connect time that is the ClientHello, built by `new_tls_session` before
/// the socket existed; later flights go out through [`decrypt`], which pumps
/// the session as each record arrives.
fn pump_tls(id: i64) {
    let out = with_client(id, |client| {
        let session = client.tls.as_mut()?;
        session.pump();
        Some(session.take_output())
    });
    if let Some(Some(out)) = out {
        write_wire(id, &out);
    }
}

fn message_for(code: &str, syscall: &str) -> String {
    if syscall.is_empty() {
        code.to_string()
    } else {
        format!("{syscall} {code}")
    }
}

// ── The handshake, one completion at a time ─────────────────────────────────

fn on_connect(id: i64) {
    // Start reading before the first byte goes out: the `101` can be in flight
    // before this submission returns.
    if let Err(e) = tl::read_start(id) {
        fail(id, &e.message());
        return;
    }
    let secure = with_client(id, |client| client.tls.is_some());
    match secure {
        Some(true) => pump_tls(id),
        // Cleartext: the upgrade request goes out immediately.
        Some(false) => {
            let request = with_client(id, |client| std::mem::take(&mut client.request));
            if let Some(request) = request {
                write_wire(id, &request);
            }
        }
        None => {
            let _ = tl::close(id);
        }
    }
}

fn on_data(id: i64, bytes: &[u8]) {
    // An adopted connection's plaintext belongs to the codec. The TLS layer
    // still sits in between, which is why this is not simply
    // `turnloop_link::on_data`.
    let plaintext = match decrypt(id, bytes) {
        Some(plaintext) => plaintext,
        // The connection is gone, or TLS failed and already reported it.
        None => return,
    };
    if plaintext.is_empty() {
        return;
    }
    if with_client(id, |client| client.adopted) == Some(true) {
        crate::turnloop_link::on_data(id, &plaintext);
        return;
    }
    receive_upgrade(id, &plaintext);
}

/// Feed ciphertext through the TLS layer, or pass bytes straight through on a
/// cleartext connection. `None` means this connection is finished.
fn decrypt(id: i64, bytes: &[u8]) -> Option<Vec<u8>> {
    let has_tls = with_client(id, |client| client.tls.is_some())?;
    if !has_tls {
        return Some(bytes.to_vec());
    }
    let progress = with_client(id, |client| {
        let session = client.tls.as_mut()?;
        session.receive(bytes);
        let progress = session.pump();
        let out = session.take_output();
        let plain = session.take_plaintext();
        let failure = session.failure().map(|(_, message)| message.to_string());
        Some((progress, out, plain, failure))
    })??;
    let (progress, out, plain, failure) = progress;
    if !out.is_empty() {
        write_wire(id, &out);
    }
    if let Some(message) = failure {
        fail(id, &format!("TLS handshake failed: {message}"));
        return None;
    }
    if progress.handshake_done {
        // The request was withheld until there was an encrypted channel to put
        // it on; send it now, in the same turn the handshake completed.
        let request = with_client(id, |client| std::mem::take(&mut client.request))?;
        if !request.is_empty() {
            write_plain(id, &request);
        }
    }
    Some(plain)
}

/// Feed the `101` reader, and adopt the connection once it is satisfied.
fn receive_upgrade(id: i64, plaintext: &[u8]) {
    let outcome = with_client(id, |client| {
        let Some(upgrade) = client.upgrade.as_mut() else {
            return Ok(None);
        };
        upgrade
            .receive(plaintext)
            .map(|done| done.map(|u| u.leftover))
    });
    match outcome {
        Some(Ok(Some(leftover))) => adopt(id, leftover),
        Some(Ok(None)) => {}
        Some(Err(e)) => fail(id, &e.message),
        None => {}
    }
}

/// The handshake succeeded: hand the connection to the protocol layer.
fn adopt(id: i64, leftover: Vec<u8>) {
    let ws_id = with_client(id, |client| {
        client.upgrade = None;
        client.adopted = true;
        client.ws_id
    });
    let Some(ws_id) = ws_id else { return };
    // `register_turnloop_client` would allocate a *second* JS id; this
    // connection already has one, handed out synchronously by `connect` so
    // listeners could be registered while it was still connecting.
    let queued = crate::attach_turnloop_client(ws_id, id);
    // `'open'` is queued BEFORE the leftover is decoded: a message the server
    // pipelined behind its `101` would otherwise reach the pump ahead of the
    // event that says the socket is open.
    crate::connection_opened(ws_id);
    crate::turnloop_link::adopt_existing(
        id,
        ws_id,
        crate::turnloop_link::Transport {
            write: write_plain,
            finish,
            destroy,
        },
        Role::Client,
        &leftover,
    );
    // Whatever `ws.send(...)` queued while this was CONNECTING, in order.
    for command in queued {
        crate::replay_command(id, command);
    }
    let token = with_client(id, |client| client.token.take()).flatten();
    if let Some(token) = token {
        token.resolve_number(ws_id as f64);
    }
}

// ── Writing ─────────────────────────────────────────────────────────────────

/// Put plaintext on the connection, through TLS when it carries any.
///
/// This is the [`crate::turnloop_link::Transport`] writer for every connection
/// this module opens, which is what makes the link layer TLS-transparent — the
/// codec hands it frame bytes and never learns whether they were encrypted.
fn write_plain(id: i64, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    let encrypted = with_client(id, |client| {
        let session = client.tls.as_mut()?;
        session.write(bytes);
        session.pump();
        Some(session.take_output())
    });
    match encrypted {
        // TLS: write the ciphertext the session produced.
        Some(Some(out)) => write_wire(id, &out),
        // Cleartext, or a connection this module has already forgotten — the
        // latter still submits, because turnloop answers a dead handle with an
        // error completion rather than misdelivering.
        _ => write_wire(id, bytes),
    }
}

/// Put bytes on the wire exactly as given.
fn write_wire(id: i64, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    if tl::write(id, bytes, 0).is_err() {
        // `on_error`, not `fail`: an adopted connection's failure belongs to
        // the protocol layer, which owes JS an `'error'` and a `'close'`.
        // `fail` only settles the pre-`101` promise, so routing every write
        // failure through it would silently drop the close event for a live
        // `ws` object. Never called with the links lock held — every
        // `turnloop_link` writer releases it first.
        on_error(id, "write EPIPE");
    }
}

/// `finish` in the transport's sense: everything queued goes out, then FIN.
///
/// Not `close`: turnloop's `close` cancels the connection's outstanding
/// operations, including the close frame the codec has just queued, and a peer
/// that sees the reset instead reports 1006 rather than the code it was sent.
fn finish(id: i64) {
    if let Some(mut client) = forget(id) {
        // A graceful WebSocket close is also a graceful TLS close.
        if let Some(session) = client.tls.as_mut() {
            session.close_notify();
            session.pump();
            let out = session.take_output();
            if !out.is_empty() {
                let _ = tl::write(id, &out, 0);
            }
        }
        settle_pending(&mut client, "socket hang up");
    }
    if tl::shutdown(id, 0).is_err() {
        let _ = tl::close(id);
    }
}

/// `ws.terminate()` and the error paths: drop the connection now.
fn destroy(id: i64) {
    if let Some(mut client) = forget(id) {
        settle_pending(&mut client, "socket hang up");
    }
    let _ = tl::close(id);
}

// ── Terminal completions ────────────────────────────────────────────────────

fn on_eof(id: i64) {
    if with_client(id, |client| client.adopted) == Some(true) {
        if crate::turnloop_link::on_eof(id) {
            // The protocol layer is done with it; end our side too.
            forget(id);
            if tl::shutdown(id, 0).is_err() {
                let _ = tl::close(id);
            }
        }
        return;
    }
    fail(id, "socket hang up before the upgrade completed");
}

/// The write-side shutdown submitted by [`finish`] completed: every byte queued
/// ahead of it has left, because turnloop orders a handle's writes before its
/// shutdown. Close now, which is what reclaims the runtime entry and hands the
/// id back.
///
/// Without this arm a connection **this side** ends — every `ws.close()` whose
/// handshake completes — is shut down and then never closed, so no terminal
/// `NET_CLOSED` arrives and `free_handle_id` never runs. That is the #6441
/// id-exhaustion shape, one id per closed WebSocket; `perry-http-server` and
/// `perry-ext-net` each carry the same arm for the same reason.
fn on_shutdown(id: i64) {
    let _ = tl::close(id);
}

fn on_closed(id: i64) {
    let adopted = with_client(id, |client| client.adopted) == Some(true);
    if adopted {
        crate::turnloop_link::on_closed(id);
    }
    if let Some(mut client) = forget(id) {
        if !adopted {
            settle_pending(&mut client, "socket hang up");
        }
    }
    // The terminal completion: nothing can name this id again, and no JS
    // object holds it, so the id goes back to the shared band rather than
    // leaking one per connection (the #6441 exhaustion class).
    perry_ffi::free_handle_id(id);
}

fn on_error(id: i64, message: &str) {
    if with_client(id, |client| client.adopted) == Some(true) {
        if crate::turnloop_link::on_error(id, message) {
            forget(id);
            let _ = tl::close(id);
        }
        return;
    }
    fail(id, message);
}

/// Report a connect/handshake failure and tear the connection down.
///
/// Before the `101` there is no `ws.on('close')` contract to honour — `ws`
/// raises `'error'` on a failed connect and the promise rejects — so this is
/// deliberately not `connection_closed`'s path.
fn fail(id: i64, message: &str) {
    let Some(mut client) = forget(id) else { return };
    if !client.failed {
        client.failed = true;
        settle_pending(&mut client, message);
    }
    let _ = tl::close(id);
}

/// Settle whatever the JS side is still waiting on for a connection that never
/// opened. A connection that has been adopted has already resolved.
fn settle_pending(client: &mut Client, message: &str) {
    if client.adopted {
        return;
    }
    let text = format!("WebSocket connect error: {message}");
    crate::connection_failed(client.ws_id, &text);
    if let Some(token) = client.token.take() {
        token.reject_string(&text);
    }
}

// ── Opening a connection ────────────────────────────────────────────────────

/// Open `target`, run the upgrade, and adopt the result into `ws_id`.
///
/// **Every failure path settles what the caller handed over**: `'error'` on
/// `ws_id` and a rejection on `token`. `token` is moved in, so an early return
/// that merely reported a `String` would leave a `new WebSocket(url)` promise
/// pending for the life of the process — the shape `turnloop_tls_io`'s
/// `begin_client_upgrade` guards with the same discipline.
pub(crate) fn connect(
    ws_id: usize,
    target: &crate::connect::Target,
    protocols: Vec<String>,
    headers: Vec<(String, String)>,
    mut token: Option<JsNativeAsyncCompletion>,
) -> Result<i64, String> {
    macro_rules! refuse {
        ($message:expr) => {{
            let message: String = $message;
            let text = format!("WebSocket connect error: {message}");
            crate::connection_failed(ws_id, &text);
            if let Some(token) = token.take() {
                token.reject_string(&text);
            }
            return Err(message);
        }};
    }

    let mut nonce = [0u8; 16];
    if let Err(message) = secure_random(&mut nonce) {
        refuse!(message);
    }
    let started = ClientUpgrade::start(&target.authority, &target.path, nonce, protocols, &headers);
    let (upgrade, request) = match started {
        Ok(started) => started,
        Err(e) => refuse!(e.message),
    };

    let tls = if target.secure {
        match new_tls_session(&target.host) {
            Ok(session) => Some(session),
            Err(message) => refuse!(message),
        }
    } else {
        None
    };

    let id = next_id();
    if id == perry_ffi::INVALID_HANDLE {
        refuse!("no connection ids available".to_string());
    }
    clients().lock().unwrap_or_else(|e| e.into_inner()).insert(
        id,
        Client {
            ws_id,
            tls,
            upgrade: Some(upgrade),
            request,
            token: token.take(),
            adopted: false,
            failed: false,
        },
    );
    // Submitted last: the completion can arrive before this call returns (a
    // loopback connect completes in the same turn), and it must find the
    // entry.
    if let Err(e) = tl::tcp_connect(id, SUBSYSTEM, &target.host, target.port, NODELAY) {
        // The entry owns the token now, so hand it back before reporting.
        token = forget(id).and_then(|mut client| client.token.take());
        perry_ffi::free_handle_id(id);
        refuse!(e.message());
    }
    Ok(id)
}

/// A `wss://` client session, configured the way every other Perry TLS client
/// is: Node's own environment (`NODE_TLS_REJECT_UNAUTHORIZED`,
/// `NODE_EXTRA_CA_CERTS`, `SSL_CERT_FILE`) read through `perry_ffi`.
///
/// No ALPN is offered. RFC 6455 has no ALPN identifier for WebSocket over TLS
/// and `ws` advertises none; offering `http/1.1` would let a server that
/// implements ALPN strictly select a protocol the handshake then contradicts.
fn new_tls_session(host: &str) -> Result<perry_tls_session::TlsClientSession, String> {
    let config = tls_config()?;
    let name = perry_tls_session::server_name(host)?;
    let mut session = perry_tls_session::TlsClientSession::new(config, name)?;
    // Produce the ClientHello now, so `on_connect` has something to write.
    session.pump();
    Ok(session)
}

fn tls_config() -> Result<&'static turnloop_tls::ClientConfig, String> {
    static CONFIG: OnceLock<Result<turnloop_tls::ClientConfig, String>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let environment = perry_ffi::node_tls_client_environment();
            let mut extra_ca_pem = Vec::new();
            for pem in environment.ca_pems() {
                extra_ca_pem.extend_from_slice(pem);
                if !pem.ends_with(b"\n") {
                    extra_ca_pem.push(b'\n');
                }
            }
            let options = turnloop_tls::ClientOptions {
                alpn: Vec::new(),
                ca: None,
                extra_ca_pem,
                reject_unauthorized: !environment.accepts_invalid_certificates(),
                enable_sni: true,
                // `None` = turnloop-tls's default provider, `ring`.
                provider: None,
            };
            turnloop_tls::ClientConfig::new(options, unix_seconds()).map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(|e| format!("TLS configuration: {e}"))
}

fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// RFC 6455 §4.1's nonce, which must be unpredictable rather than merely
/// unique: a guessable key lets an attacker who can make the client issue a
/// request convince a cache that the `101` belongs to an ordinary GET.
///
/// The source is rustls's own provider, already linked through `turnloop-tls`,
/// so this adds a call rather than a crate.
fn secure_random(out: &mut [u8]) -> Result<(), String> {
    use turnloop_tls::rustls::crypto::ring::default_provider;
    static PROVIDER: OnceLock<turnloop_tls::rustls::crypto::CryptoProvider> = OnceLock::new();
    let provider = PROVIDER.get_or_init(default_provider);
    provider
        .secure_random
        .fill(out)
        .map_err(|_| "no secure random source".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two slots this crate claims must be distinct, because they hold two
    /// different sink functions in the same binary: this module's and
    /// `perry-http-server`'s. Folding them onto one slot would leave whichever
    /// registered last delivering the other's completions.
    #[test]
    fn the_client_and_server_slots_are_distinct() {
        assert_ne!(SUBSYSTEM, crate::server::SERVER_SUBSYSTEM);
    }

    /// Both transports are actually *installed* in the linked runtime's sink
    /// registry — not merely compiled.
    ///
    /// This is the non-vacuous half. `register_sink` refuses outright when the
    /// runtime's `NetCompletion` layout does not match this crate's, and it
    /// refuses a slot at or above the runtime's `MAX_SUBSYSTEMS` — which the
    /// server's slot 8 was, before this lane raised the ceiling from 8 to 16.
    /// Either refusal leaves `available()` false and every `new WebSocket(url)`
    /// / `new WebSocketServer({port})` declining to a transport that no longer
    /// exists, which is a silent no-WebSockets build rather than a failure.
    ///
    /// `sink_installed` is the discriminating quantity: it is false for a slot
    /// nothing registered, so this cannot pass with nothing listening.
    #[test]
    fn both_slots_register_a_live_sink_in_the_runtime() {
        // `available()` performs this module's registration as a side effect;
        // its own answer depends on the calling thread owning a loop, which a
        // test thread does not, so the registration is what is asserted.
        let _ = available();
        assert!(
            tl::sink_installed(SUBSYSTEM),
            "the outbound client's sink must be installed in slot {SUBSYSTEM}"
        );

        let _ = perry_http_server::available(crate::server::SERVER_SUBSYSTEM);
        assert!(
            tl::sink_installed(crate::server::SERVER_SUBSYSTEM),
            "the standalone server's sink must be installed in slot {} — a \
             refusal here means the runtime's MAX_SUBSYSTEMS is back below it",
            crate::server::SERVER_SUBSYSTEM
        );
    }

    /// Every terminal entry point must tolerate an id it has never seen: a
    /// completion can arrive for a connection `fail` has already forgotten.
    #[test]
    fn an_unknown_connection_is_inert_rather_than_a_panic() {
        on_data(-7, b"\x81\x00");
        on_eof(-7);
        on_error(-7, "gone");
        fail(-7, "gone");
        assert!(with_client(-7, |_| ()).is_none());
    }

    /// The nonce is 16 unpredictable bytes, not 16 zeroes — the failure a
    /// missing crypto provider would produce, and one an all-zero buffer would
    /// hide.
    #[test]
    fn the_handshake_nonce_comes_from_a_real_random_source() {
        let mut a = [0u8; 16];
        let mut b = [0u8; 16];
        secure_random(&mut a).expect("a random source");
        secure_random(&mut b).expect("a random source");
        assert_ne!(a, [0u8; 16]);
        assert_ne!(a, b);
    }
}
