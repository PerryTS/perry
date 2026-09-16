//! TLS on a turnloop socket: the per-socket layer between the P1 completion
//! sink and [`crate::turnloop_tls::TlsSession`] (P5).
//!
//! P1 kept every TLS-upgradable socket on tokio because `socket.upgradeToTLS`
//! hands a live `TcpStream` to `tokio_rustls`. Running the rustls state
//! machine *above* the turnloop handle removes the need to move a descriptor
//! at all: the handle keeps carrying bytes and a session is installed on it
//! mid-stream, which is exactly PostgreSQL's `SSLRequest` shape
//! (`test-files/test_net_upgrade_tls.ts`).
//!
//! # Write accounting
//!
//! A caller writes *plaintext*; turnloop reports *ciphertext* written. Those
//! are different byte counts and the mapping is not one-to-one — a write
//! issued during the handshake is buffered by rustls and encrypted later, and
//! one flush can carry several application writes plus handshake records.
//!
//! So each application write records the total ciphertext offset at which its
//! plaintext had been encrypted (its *mark*), and a `NET_WROTE` completion
//! advances a running acknowledged-ciphertext counter. A write's callback
//! fires when the counter reaches its mark. Writes that the handshake has not
//! encrypted yet carry no mark and simply wait. This is what makes
//! `socket.write(chunk, cb)` on an upgraded socket keep Node's "cb fires when
//! the bytes have left" contract.
//!
//! # GC
//!
//! The layer holds owned `Vec<u8>`s and one [`JsNativeAsyncCompletion`] token.
//! The token is the *only* JS-visible thing here, and it is the runtime's own
//! pinned, root-scanned handle to the promise (#9552) rather than a bare
//! `*mut Promise` cached in a side table — which is the shape
//! `scripts/gc_runtime_root_holders.py` exists to catch.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};

use perry_ffi::turnloop_net as tl;
use perry_ffi::JsNativeAsyncCompletion;

use crate::turnloop_tls::TlsSession;
use crate::{push_event, PendingNetEvent, TlsClientConfigData};

/// One application write awaiting its ciphertext acknowledgement.
struct PendingWrite {
    /// The caller's completion token; zero means no callback.
    user: u64,
    /// Plaintext bytes the caller handed over, for `bytesWritten`.
    plain_len: usize,
    /// Total ciphertext offset after this write's plaintext was encrypted.
    /// `None` while the handshake has not encrypted it yet.
    mark: Option<u64>,
}

/// The TLS layer installed on one turnloop socket.
struct Layer {
    session: TlsSession,
    /// Pending `socket.upgradeToTLS()` promise, settled on handshake outcome.
    token: Option<JsNativeAsyncCompletion>,
    servername: String,
    verify: bool,
    config: TlsClientConfigData,
    cipher_written: u64,
    cipher_acked: u64,
    pending: VecDeque<PendingWrite>,
    /// A `socket.end()` seen before the handshake finished; the write-side
    /// shutdown runs once `close_notify` has been encrypted.
    pending_shutdown: Option<u64>,
    secure_emitted: bool,
    /// The application asked to close, or the peer already did. A rustls
    /// failure *after* that is teardown noise — the records that follow a
    /// `close_notify` on a socket nobody is reading any more — and Node emits
    /// no `'error'` for it. Before it, a failure is the real thing.
    closing: bool,
}

fn layers() -> &'static Mutex<HashMap<i64, Layer>> {
    static LAYERS: OnceLock<Mutex<HashMap<i64, Layer>>> = OnceLock::new();
    LAYERS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn with_layer<R>(id: i64, f: impl FnOnce(&mut Layer) -> R) -> Option<R> {
    let mut map = layers().lock().unwrap_or_else(|e| e.into_inner());
    map.get_mut(&id).map(f)
}

/// Whether this socket carries TLS.
pub fn installed(id: i64) -> bool {
    layers()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains_key(&id)
}

/// Drop the layer; called from the socket's terminal path.
pub fn forget(id: i64) {
    let layer = layers()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&id);
    if let Some(mut layer) = layer {
        // An upgrade that never completed must not leave its promise pending
        // for the life of the process.
        if let Some(token) = layer.token.take() {
            token.reject_string("socket closed before the TLS handshake completed");
        }
    }
}

/// Install a client session and send the first handshake flight.
///
/// `token` settles when the handshake does; `None` is the `tls.connect` shape,
/// where JS learns the outcome from `'secureConnect'` / `'error'`.
pub(crate) fn begin_client_upgrade(
    id: i64,
    servername: String,
    verify: bool,
    config: TlsClientConfigData,
    token: Option<JsNativeAsyncCompletion>,
) -> Result<(), String> {
    // Every early return settles `token`, so a caller that handed one over
    // never has to take it back to reject it.
    macro_rules! refuse {
        ($message:expr) => {{
            let message: String = $message;
            if let Some(token) = token {
                token.reject_string(&message);
            }
            return Err(message);
        }};
    }
    if installed(id) {
        refuse!("socket is already TLS".to_string());
    }
    if !tl::is_live(id) {
        refuse!("socket is closed".to_string());
    }
    let client_config = match crate::tls::build_client_config(verify, Some(&config)) {
        Ok(config) => config,
        Err(message) => refuse!(message),
    };
    let name = match crate::turnloop_tls::server_name(&servername) {
        Ok(name) => name,
        Err(message) => refuse!(message),
    };
    let session = match TlsSession::client(client_config, name) {
        Ok(session) => session,
        Err(message) => refuse!(message),
    };
    layers().lock().unwrap_or_else(|e| e.into_inner()).insert(
        id,
        Layer {
            session,
            token,
            servername,
            verify,
            config,
            cipher_written: 0,
            cipher_acked: 0,
            pending: VecDeque::new(),
            pending_shutdown: None,
            secure_emitted: false,
            closing: false,
        },
    );
    // Produce and send the ClientHello.
    pump_session(id);
    Ok(())
}

/// Install a client session on a turnloop socket that is already connected.
///
/// The client twin of [`install_server_session`], and the reason it exists
/// separately from [`begin_client_upgrade`]: that one takes this crate's own
/// `TlsClientConfigData` (built by reading JS values) and settles a
/// `JsNativeAsyncCompletion`, so it is reachable only from `net.Socket`'s
/// `upgradeToTLS`. A caller that owns a raw turnloop handle and has no promise
/// to settle — `http2.connect('https://…')`, which needs **ALPN** decided
/// before it knows whether it may speak HTTP/2 at all — could not use it.
///
/// `alpn` is the protocol list to offer, in preference order; read the result
/// back with [`alpn_protocol`] once [`handshake_done`] is true. The outcome is
/// reported the way `tls.connect` reports it — a `'secureConnect'` event on
/// success, an `'error'` and a destroyed socket on failure — because that is
/// what this layer already does for a socket with no upgrade promise.
///
/// The ClientHello goes out before this returns.
pub fn install_client_session(
    id: i64,
    servername: String,
    verify: bool,
    alpn: Vec<Vec<u8>>,
    ca: Vec<Vec<u8>>,
) -> Result<(), String> {
    begin_client_upgrade(
        id,
        servername,
        verify,
        crate::tls::TlsClientConfigData::for_alpn(alpn, ca),
        None,
    )
}

/// Install an already-built server session on an accepted connection.
///
/// Used by the `https` / `http2` server paths, which build their
/// `rustls::ServerConfig` from Node's own option surface.
pub fn install_server_session(
    id: i64,
    config: Arc<turnloop_tls::rustls::ServerConfig>,
) -> Result<(), String> {
    if installed(id) {
        return Err("socket is already TLS".to_string());
    }
    let session = TlsSession::server(config)?;
    layers().lock().unwrap_or_else(|e| e.into_inner()).insert(
        id,
        Layer {
            session,
            token: None,
            servername: String::new(),
            verify: true,
            config: TlsClientConfigData::default(),
            cipher_written: 0,
            cipher_acked: 0,
            pending: VecDeque::new(),
            pending_shutdown: None,
            secure_emitted: false,
            closing: false,
        },
    );
    Ok(())
}

/// Negotiated ALPN protocol, once the handshake has completed.
pub fn alpn_protocol(id: i64) -> Option<Vec<u8>> {
    with_layer(id, |l| l.session.alpn_protocol()).flatten()
}

/// Whether the handshake on this socket has completed.
pub fn handshake_done(id: i64) -> bool {
    with_layer(id, |l| !l.session.is_handshaking()).unwrap_or(false)
}

/// Decrypted application data produced by one ciphertext delivery.
pub struct Received {
    pub plaintext: Vec<u8>,
    /// The peer sent `close_notify`: treat it as readable EOF.
    pub peer_closed: bool,
}

/// Feed ciphertext from a `NET_DATA` completion and take back plaintext.
///
/// Returns `None` for a socket with no TLS layer, so the caller can keep its
/// plaintext path unchanged.
pub fn receive(id: i64, ciphertext: &[u8]) -> Option<Received> {
    let installed = with_layer(id, |l| l.session.receive(ciphertext)).is_some();
    if !installed {
        return None;
    }
    let out = pump_session(id);
    Some(Received {
        plaintext: out.plaintext,
        peer_closed: out.peer_closed,
    })
}

/// Encrypt and submit one application write. Returns the socket's queued
/// ciphertext byte count, as `SocketState::command` reports it.
pub fn write(id: i64, bytes: &[u8], user: u64) -> Result<usize, String> {
    let known = with_layer(id, |l| {
        l.session.write(bytes);
        l.pending.push_back(PendingWrite {
            user,
            plain_len: bytes.len(),
            mark: None,
        });
    })
    .is_some();
    if !known {
        return Err("socket is closed".to_string());
    }
    pump_session(id);
    Ok(tl::queued_bytes(id))
}

/// `socket.end()` on a TLS socket: send `close_notify`, then shut the write
/// side down once it has been encrypted and queued.
pub fn shutdown(id: i64, user: u64) -> Result<(), String> {
    let known = with_layer(id, |l| {
        l.session.close_notify();
        l.pending_shutdown = Some(user);
        l.closing = true;
    })
    .is_some();
    if !known {
        return Err("socket is closed".to_string());
    }
    pump_session(id);
    Ok(())
}

/// Account a `NET_WROTE` completion and return the application writes it
/// completed, as `(user token, plaintext length)` pairs in submission order.
pub fn wrote(id: i64, len: usize) -> Option<Vec<(u64, usize)>> {
    with_layer(id, |l| {
        l.cipher_acked = l.cipher_acked.saturating_add(len as u64);
        let mut done = Vec::new();
        while let Some(front) = l.pending.front() {
            match front.mark {
                Some(mark) if mark <= l.cipher_acked => {
                    let w = l.pending.pop_front().expect("checked");
                    done.push((w.user, w.plain_len));
                }
                _ => break,
            }
        }
        done
    })
}

struct Driven {
    plaintext: Vec<u8>,
    peer_closed: bool,
}

/// Run the session, submit whatever ciphertext it produced, and report the
/// handshake and close transitions to JS.
///
/// Named `pump_session` rather than the obvious `drive` on purpose:
/// `scripts/gc_runtime_root_holders.py` resolves function names ACROSS crates,
/// and `perry-runtime`'s `gc/roots/stack_maps_walker_agreement.rs` has its own
/// `fn drive`. A same-named function in a scanner-registering crate pulled that
/// file's body into the reachable set and silently flipped its `PROBE` holder
/// to COVERED, invalidating someone else's frontier entry (P1's report records
/// the same coincidence from the other direction).
fn pump_session(id: i64) -> Driven {
    let mut out = Driven {
        plaintext: Vec::new(),
        peer_closed: false,
    };
    let mut handshake_done = false;
    let mut failure: Option<String> = None;
    let mut ciphertext = Vec::new();
    let mut shutdown_user: Option<u64> = None;
    let mut closing = false;

    let present = with_layer(id, |l| {
        let progress = l.session.pump();
        handshake_done = progress.handshake_done;
        out.peer_closed = progress.peer_closed;
        failure = l.session.failure().map(str::to_string);
        out.plaintext = l.session.take_plaintext();
        ciphertext = l.session.take_output();
        if !ciphertext.is_empty() {
            l.cipher_written = l.cipher_written.saturating_add(ciphertext.len() as u64);
            // Everything the session had buffered has now been encrypted into
            // this flush, in submission order, so every unmarked write is
            // covered by it.
            for pending in l.pending.iter_mut().filter(|p| p.mark.is_none()) {
                pending.mark = Some(l.cipher_written);
            }
        }
        if l.session.close_sent() {
            shutdown_user = l.pending_shutdown.take();
        }
        if out.peer_closed {
            l.closing = true;
        }
        closing = l.closing;
    })
    .is_some();
    if !present {
        return out;
    }

    if !ciphertext.is_empty() {
        // `user` is zero: the ciphertext write is not an application write.
        // Application callbacks are driven by the ciphertext acknowledgement
        // accounting in `wrote`, not by this submission's own completion.
        if let Err(err) = tl::write(id, &ciphertext, 0) {
            fail(id, err.message(), closing);
            return out;
        }
    }
    if let Some(user) = shutdown_user {
        // turnloop orders a handle's writes ahead of its shutdown, so the
        // queued close_notify is on the wire before the FIN.
        if let Err(err) = tl::shutdown(id, user) {
            fail(id, err.message(), closing);
            return out;
        }
    }
    if let Some(message) = failure {
        fail(id, message, closing);
        return out;
    }
    if handshake_done {
        finish_handshake(id);
    }
    out
}

fn finish_handshake(id: i64) {
    let (token, servername, verify, config, already) = {
        let mut map = layers().lock().unwrap_or_else(|e| e.into_inner());
        let Some(layer) = map.get_mut(&id) else {
            return;
        };
        let already = std::mem::replace(&mut layer.secure_emitted, true);
        (
            layer.token.take(),
            layer.servername.clone(),
            layer.verify,
            layer.config.clone(),
            already,
        )
    };
    if already {
        return;
    }
    // Record the handshake facts (`socket.authorized`, ALPN, peer certificate)
    // on the same JS-visible surface the tokio path wrote.
    let recorded = with_layer(id, |l| {
        crate::tls::HandshakeFacts::from_session(&l.session, &servername, verify, Some(&config))
    });
    if let Some(facts) = recorded {
        facts.publish(id);
    }
    if let Some(token) = token {
        token.resolve_undefined();
    }
    push_event(PendingNetEvent::SecureConnect(id));
}

/// Report a TLS failure and tear the socket down.
///
/// `closing` suppresses the JS `'error'`: once the application has asked to
/// close — or the peer has sent `close_notify` — a rustls failure is teardown
/// noise on a socket nobody is reading, and Node emits nothing for it. The
/// pending upgrade promise is still settled, because a caller awaiting it must
/// not be left hanging by a close that raced the handshake.
fn fail(id: i64, message: String, closing: bool) {
    let token = {
        let mut map = layers().lock().unwrap_or_else(|e| e.into_inner());
        map.get_mut(&id).and_then(|l| l.token.take())
    };
    if let Some(token) = token {
        token.reject_string(&message);
    }
    if !closing {
        push_event(PendingNetEvent::Error(id, message));
    }
    crate::turnloop_io::destroy(id);
}
