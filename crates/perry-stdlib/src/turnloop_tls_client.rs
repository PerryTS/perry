//! A completion-driven TLS **client** session over a turnloop socket (P6).
//!
//! This is the outbound twin of `perry-ext-net`'s `turnloop_tls.rs`: the same
//! `rustls` unbuffered state machine driven from the outside — ciphertext in as
//! `NET_DATA` arrives, ciphertext out through `turnloop_net::write`, plaintext
//! back to the caller — all on the loop thread, inside the dispatch call.
//!
//! Two differences from P5's copy, both deliberate:
//!
//! * **Client only.** A fetch never accepts, so there is no server endpoint and
//!   no `Endpoint` trait to abstract over one.
//! * **The config comes from [`turnloop_tls::ClientConfig`]** rather than a
//!   hand-built `rustls::ClientConfig`. Its `ClientOptions` — ALPN, explicit
//!   `ca`, `extra_ca_pem`, `reject_unauthorized`, `enable_sni` — is exactly the
//!   surface `fetch`'s `apply_node_tls_environment` maps from
//!   `perry_ffi::node_tls_client_environment()`, and it names the crypto
//!   provider explicitly, so it is unaffected by the ring/aws-lc-rs ambiguity
//!   the `tls` / `bundled-ws` paths install a default for (#6117).
//!
//! It lives in perry-stdlib rather than being shared with P5's copy because a
//! `staticlib` binding and the stdlib rlib have no dependency edge; see the
//! P6 report's "What P6 did not do" for the consolidation that would fix it.
//! Within perry-stdlib it IS shared: the HTTP client engine
//! (`turnloop_client`) and the SMTP client engine (`turnloop_smtp`) drive the
//! same session type.
//!
//! # GC
//!
//! A session holds only owned `Vec<u8>`s — no JS value, no heap pointer, no GC
//! root. Plaintext is copied into a JS value by the engine's completion
//! handling, on the owning thread (P1's rule, unchanged).

use std::time::{SystemTime, UNIX_EPOCH};

use turnloop_tls::rustls::{
    self,
    unbuffered::{ConnectionState, EncodeError, UnbufferedStatus},
};

/// Retained ciphertext scratch. One TLS record is at most ~16 KiB plus
/// overhead; 64 KiB covers a handshake flight without reallocating.
const SCRATCH_CAPACITY: usize = 64 * 1024;
/// Hard cap on unparsed ciphertext, so a peer that never completes a record
/// cannot grow the buffer without bound.
const INPUT_LIMIT: usize = 1024 * 1024;
/// Hard cap on decrypted plaintext the caller has not taken yet. The caller
/// drains it inside the same dispatch, so this only bounds a pathological turn.
const PLAINTEXT_LIMIT: usize = 8 * 1024 * 1024;
/// Ceiling on growing the scratch for one oversized handshake flight.
const SCRATCH_LIMIT: usize = 4 * 1024 * 1024;

/// What the caller must know after [`TlsClientSession::pump`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Progress {
    /// The handshake completed during this pump. Reported exactly once.
    pub(crate) handshake_done: bool,
    /// The peer sent `close_notify`; no more plaintext will arrive.
    pub(crate) peer_closed: bool,
}

/// Retained buffers, separate from the rustls connection so the state returned
/// by `process` (which borrows the connection) and the buffers can be held at
/// once.
struct Buffers {
    input: Vec<u8>,
    scratch: Vec<u8>,
    scratch_len: usize,
    out: Vec<u8>,
    plain: Vec<u8>,
    deferred: Vec<u8>,
    transmitted: bool,
    want_close: bool,
    close_sent: bool,
    peer_closed: bool,
    /// rustls asked for a larger output buffer than `scratch` has (a big
    /// certificate chain). Applied before the next `process`, which then hands
    /// back the same `EncodeTlsData` state and succeeds.
    grow_scratch: Option<usize>,
    /// Node's cause code and rustls's own text. The code is `&'static str` so
    /// it can reach `register_error_code_pub`, which takes one.
    failed: Option<(&'static str, String)>,
}

/// One step's outcome.
enum Action {
    /// Call again.
    Progress,
    /// Nothing more can happen until more ciphertext arrives.
    Blocked,
}

/// One outbound TLS connection's state, driven by the host.
pub(crate) struct TlsClientSession {
    client: turnloop_tls::Client,
    buffers: Buffers,
    handshaking: bool,
    handshake_reported: bool,
}

impl TlsClientSession {
    pub(crate) fn new(
        config: &turnloop_tls::ClientConfig,
        server_name: rustls::pki_types::ServerName<'static>,
    ) -> Result<Self, String> {
        let client = config.connect(server_name).map_err(|e| node_message(&e))?;
        Ok(Self {
            client,
            buffers: Buffers {
                input: Vec::with_capacity(16 * 1024),
                scratch: vec![0; SCRATCH_CAPACITY],
                scratch_len: 0,
                out: Vec::with_capacity(8 * 1024),
                plain: Vec::with_capacity(16 * 1024),
                deferred: Vec::new(),
                transmitted: false,
                want_close: false,
                close_sent: false,
                peer_closed: false,
                grow_scratch: None,
                failed: None,
            },
            handshaking: true,
            handshake_reported: false,
        })
    }

    /// The negotiated ALPN protocol, once the handshake has completed.
    pub(crate) fn alpn_protocol(&self) -> Option<Vec<u8>> {
        self.client.alpn_protocol().map(<[u8]>::to_vec)
    }

    pub(crate) fn is_handshaking(&self) -> bool {
        self.handshaking
    }

    /// The terminal failure, if the session has one. A failed session produces
    /// no further plaintext and refuses writes.
    pub(crate) fn failure(&self) -> Option<(&'static str, &str)> {
        self.buffers
            .failed
            .as_ref()
            .map(|(code, text)| (*code, text.as_str()))
    }

    /// Hand ciphertext that arrived on the socket to the session.
    pub(crate) fn receive(&mut self, ciphertext: &[u8]) {
        let b = &mut self.buffers;
        if b.failed.is_some() {
            return;
        }
        if b.input.len() + ciphertext.len() > INPUT_LIMIT {
            b.failed = Some(("ERR_SSL_PROTOCOL_ERROR", "TLS input limit".to_string()));
            return;
        }
        b.input.extend_from_slice(ciphertext);
    }

    /// Queue application data. It is encrypted as soon as the handshake allows,
    /// so a request head written during the handshake is not lost.
    pub(crate) fn write(&mut self, plaintext: &[u8]) {
        let b = &mut self.buffers;
        if b.failed.is_some() || b.want_close || b.close_sent {
            return;
        }
        b.deferred.extend_from_slice(plaintext);
    }

    /// Ask for `close_notify` to be sent once queued writes have been encrypted.
    pub(crate) fn close_notify(&mut self) {
        if self.buffers.failed.is_none() {
            self.buffers.want_close = true;
        }
    }

    /// Take the ciphertext that must be written to the socket.
    pub(crate) fn take_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.buffers.out)
    }

    /// Take the decrypted application data received so far.
    pub(crate) fn take_plaintext(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.buffers.plain)
    }

    /// Run the state machine until it blocks. Bounded: every iteration either
    /// consumes input, produces output, or reaches a terminal/blocked state,
    /// and the two `TransmitTlsData` iterations alternate on `transmitted`.
    pub(crate) fn pump(&mut self) -> Progress {
        let mut progress = Progress::default();
        if self.buffers.failed.is_some() {
            return progress;
        }
        let now = unix_seconds();
        // A bound no correct handshake approaches. It exists so a rustls state
        // this code did not anticipate cannot spin the event loop forever.
        for _ in 0..4096 {
            let action = step(&mut self.client, &mut self.buffers, now);
            if self.buffers.failed.is_some() || matches!(action, Action::Blocked) {
                break;
            }
        }
        progress.peer_closed = self.buffers.peer_closed;
        let still = self.client.is_handshaking();
        if self.handshaking && !still && self.buffers.failed.is_none() {
            self.handshaking = false;
            if !self.handshake_reported {
                self.handshake_reported = true;
                progress.handshake_done = true;
            }
        }
        progress
    }
}

/// `turnloop_tls` takes wall time from the host rather than reading a clock
/// itself (its `SuppliedTime` provider). This is the only clock read on the
/// path, and it is per `pump`, not per record.
fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn step(tls: &mut turnloop_tls::Client, b: &mut Buffers, now: u64) -> Action {
    if let Some(required) = b.grow_scratch.take() {
        // Applied here rather than inside the arm below, where `b` is already
        // borrowed by the rustls state.
        flush_scratch(b);
        b.scratch.resize(required, 0);
    }
    let UnbufferedStatus { discard, state } = tls.process(&mut b.input, now);
    let mut discard = discard;
    let action = match state {
        Err(error) => {
            b.failed = Some(node_failure(&error));
            Action::Blocked
        }
        Ok(ConnectionState::EncodeTlsData(mut encode)) => {
            match encode.encode(&mut b.scratch[b.scratch_len..]) {
                Ok(n) => {
                    b.scratch_len += n;
                    Action::Progress
                }
                // A handshake flight bigger than the retained scratch — a large
                // certificate chain. Ask for the size rustls named and retry;
                // failing here instead would refuse the connection outright.
                Err(EncodeError::InsufficientSize(required)) => {
                    if required.required_size > SCRATCH_LIMIT {
                        b.failed = Some(("ERR_SSL_PROTOCOL_ERROR", "TLS output limit".to_string()));
                        Action::Blocked
                    } else {
                        b.grow_scratch = Some(required.required_size);
                        Action::Progress
                    }
                }
                Err(e) => {
                    b.failed = Some(("ERR_SSL_PROTOCOL_ERROR", format!("{e:?}")));
                    Action::Blocked
                }
            }
        }
        Ok(ConnectionState::TransmitTlsData(transmit)) => {
            if b.transmitted {
                transmit.done();
                b.transmitted = false;
            } else {
                // turnloop orders a handle's writes, so moving the encoded
                // bytes into the caller's output queue *is* the transmission as
                // far as ordering goes: nothing encrypted afterwards can
                // overtake them. The caller submits `take_output()` before the
                // next completion is processed. Same deviation P5 documented.
                flush_scratch(b);
                b.transmitted = true;
            }
            Action::Progress
        }
        Ok(ConnectionState::ReadTraffic(mut read)) => {
            if let Some(record) = read.next_record() {
                match record {
                    Ok(record) => {
                        discard += record.discard;
                        if b.plain.len() + record.payload.len() > PLAINTEXT_LIMIT {
                            b.failed =
                                Some(("ERR_SSL_PROTOCOL_ERROR", "TLS plaintext limit".to_string()));
                        } else {
                            b.plain.extend_from_slice(record.payload);
                        }
                    }
                    Err(e) => b.failed = Some(node_failure(&e)),
                }
            }
            Action::Progress
        }
        Ok(ConnectionState::WriteTraffic(mut write)) => {
            if !b.deferred.is_empty() {
                // rustls writes the whole record from offset zero, so the
                // scratch must be free first.
                flush_scratch(b);
                let n = b.deferred.len().min(16384);
                match write.encrypt(&b.deferred[..n], &mut b.scratch) {
                    Ok(len) => {
                        b.scratch_len = len;
                        b.deferred.drain(..n);
                        flush_scratch(b);
                        Action::Progress
                    }
                    Err(e) => {
                        b.failed = Some(("ERR_SSL_PROTOCOL_ERROR", format!("{e:?}")));
                        Action::Blocked
                    }
                }
            } else if b.want_close && !b.close_sent {
                flush_scratch(b);
                match write.queue_close_notify(&mut b.scratch) {
                    Ok(len) => {
                        b.scratch_len = len;
                        b.close_sent = true;
                        flush_scratch(b);
                        Action::Progress
                    }
                    Err(e) => {
                        b.failed = Some(("ERR_SSL_PROTOCOL_ERROR", format!("{e:?}")));
                        Action::Blocked
                    }
                }
            } else {
                Action::Blocked
            }
        }
        Ok(ConnectionState::BlockedHandshake) => Action::Blocked,
        Ok(ConnectionState::PeerClosed | ConnectionState::Closed) => {
            b.peer_closed = true;
            Action::Blocked
        }
        Ok(_) => {
            // `ReadEarlyData` and any state added by a later rustls. Early data
            // is not enabled on this config, so reaching one is a bug, not a
            // peer behaviour: fail the connection rather than spin.
            b.failed = Some((
                "ERR_SSL_PROTOCOL_ERROR",
                "unsupported TLS state".to_string(),
            ));
            Action::Blocked
        }
    };
    if discard > 0 {
        b.input.drain(..discard.min(b.input.len()));
    }
    // Records encoded during a handshake step are published to the caller even
    // when rustls did not ask for a transmit yet; the ordering guarantee above
    // makes that safe and it keeps the output moving in one turn.
    if b.scratch_len > 0 && !b.transmitted {
        flush_scratch(b);
    }
    action
}

fn flush_scratch(b: &mut Buffers) {
    if b.scratch_len == 0 {
        return;
    }
    b.out.extend_from_slice(&b.scratch[..b.scratch_len]);
    b.scratch_len = 0;
}

/// Node's cause code plus rustls's own text — the shape `net` / `tls` already
/// reports for a handshake failure.
pub(crate) fn node_message(error: &rustls::Error) -> String {
    format!("{}: {error}", turnloop_tls::node_error_code(error))
}

/// The same pair, kept apart so the code reaches `register_error_code_pub`
/// (which takes a `&'static str`) without being re-parsed out of a message.
fn node_failure(error: &rustls::Error) -> (&'static str, String) {
    (turnloop_tls::node_error_code(error), error.to_string())
}

/// Parse a host into the rustls type, keeping Node's error text. An IP literal
/// is a valid `ServerName`; rustls declines to send it as SNI itself.
pub(crate) fn server_name(name: &str) -> Result<rustls::pki_types::ServerName<'static>, String> {
    rustls::pki_types::ServerName::try_from(name.to_string())
        .map_err(|_| format!("ERR_TLS_CERT_ALTNAME_INVALID: invalid servername {name:?}"))
}

/// The process-wide outbound TLS configuration, built once from Node's TLS
/// environment (`NODE_TLS_REJECT_UNAUTHORIZED`, `SSL_CERT_FILE`,
/// `NODE_EXTRA_CA_CERTS`, resolved by `perry_ffi::node_tls_client_environment`
/// so `node:https` and this path answer the same way).
///
/// `None` when rustls refused the configuration, which makes a `https:` or
/// `smtps:` submission DECLINE to its existing transport rather than fail.
///
/// Only `http/1.1` is advertised in ALPN. Perry's turnloop client speaks
/// HTTP/1.1 and nothing else, so a server can never select h2 here — see the
/// P6 report's "HTTP/2" section for why that is a decision rather than an
/// omission. SMTP ignores ALPN entirely.
pub(crate) fn client_config() -> Option<&'static turnloop_tls::ClientConfig> {
    static CONFIG: std::sync::OnceLock<Option<turnloop_tls::ClientConfig>> =
        std::sync::OnceLock::new();
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
                alpn: vec![b"http/1.1".to_vec()],
                ca: None,
                extra_ca_pem,
                reject_unauthorized: !environment.accepts_invalid_certificates(),
                enable_sni: true,
            };
            turnloop_tls::ClientConfig::new(options, unix_seconds()).ok()
        })
        .as_ref()
}
