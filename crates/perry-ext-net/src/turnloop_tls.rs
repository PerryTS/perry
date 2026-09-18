//! A completion-driven TLS session over a turnloop socket (P5).
//!
//! `tokio_rustls` owns its transport and its own task; a turnloop socket has
//! neither. This is the same rustls state machine driven from the *outside*:
//! the caller feeds it ciphertext as `NET_DATA` completions arrive, takes the
//! ciphertext it wants written, and reads back plaintext — all on the loop
//! thread, inside the dispatch call.
//!
//! The record processing is `turnloop_tls`'s unbuffered core
//! (`rustls::unbuffered`, re-exported by that crate along with the Node
//! error-code mapping). `turnloop_tls::asynchronous::TlsStream` is the
//! futures-io shape of the identical state machine; it is not used here
//! because it needs a `turnloop_io::ExecutorHandle`, and Perry drives its own
//! `turnloop::Loop` (see `docs/turnloop/p5-report.md`, "Why sans-I/O").
//!
//! # Why this unblocks `socket.upgradeToTLS`
//!
//! P1 left every TLS-upgradable socket on tokio because `upgradeToTLS` hands a
//! live `TcpStream` to `tokio_rustls` mid-stream and turnloop does not expose
//! its descriptor. With TLS running *above* the turnloop socket rather than
//! beside it, no descriptor has to move: the same handle keeps carrying bytes
//! and a [`TlsSession`] is installed on top of it. turnloop's descriptor
//! handoff (`Detached::into_fd`, issue #35) would solve the same problem by
//! moving the socket out to tokio; this solves it by never leaving.
//!
//! # GC
//!
//! A session holds only owned `Vec<u8>`s — no JS value, no heap pointer, no GC
//! root. Plaintext is copied into a JS value by the caller's sink, on the
//! owning thread, exactly as a plaintext read already was (P1's rule).

use std::sync::Arc;

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
/// Ceiling on growing the scratch for one oversized handshake flight. A
/// certificate chain larger than this is not a chain worth completing.
const SCRATCH_LIMIT: usize = 4 * 1024 * 1024;

/// What the caller must know after [`TlsSession::pump`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    /// The handshake completed during this pump. Reported exactly once.
    pub handshake_done: bool,
    /// The peer sent `close_notify`; no more plaintext will arrive.
    pub peer_closed: bool,
}

/// Retained buffers, separate from the rustls connection so the state returned
/// by `process_tls_records` (which borrows the connection) and the buffers can
/// be held at once.
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
    failed: Option<String>,
}

/// One step's outcome.
enum Action {
    /// Call again.
    Progress,
    /// Nothing more can happen until more ciphertext arrives.
    Blocked,
}

trait Endpoint {
    type Data;
    fn process<'c, 'i>(&'c mut self, input: &'i mut [u8]) -> UnbufferedStatus<'c, 'i, Self::Data>;
    fn handshaking(&self) -> bool;
    fn alpn(&self) -> Option<&[u8]>;
    fn certificates(&self) -> Option<&[rustls::pki_types::CertificateDer<'static>]>;
}

impl Endpoint for rustls::client::UnbufferedClientConnection {
    type Data = rustls::client::ClientConnectionData;
    fn process<'c, 'i>(&'c mut self, input: &'i mut [u8]) -> UnbufferedStatus<'c, 'i, Self::Data> {
        self.process_tls_records(input)
    }
    fn handshaking(&self) -> bool {
        self.is_handshaking()
    }
    fn alpn(&self) -> Option<&[u8]> {
        self.alpn_protocol()
    }
    fn certificates(&self) -> Option<&[rustls::pki_types::CertificateDer<'static>]> {
        self.peer_certificates()
    }
}

impl Endpoint for rustls::server::UnbufferedServerConnection {
    type Data = rustls::server::ServerConnectionData;
    fn process<'c, 'i>(&'c mut self, input: &'i mut [u8]) -> UnbufferedStatus<'c, 'i, Self::Data> {
        self.process_tls_records(input)
    }
    fn handshaking(&self) -> bool {
        self.is_handshaking()
    }
    fn alpn(&self) -> Option<&[u8]> {
        self.alpn_protocol()
    }
    fn certificates(&self) -> Option<&[rustls::pki_types::CertificateDer<'static>]> {
        self.peer_certificates()
    }
}

enum Session {
    Client(Box<rustls::client::UnbufferedClientConnection>),
    Server(Box<rustls::server::UnbufferedServerConnection>),
}

/// One TLS connection's state, driven by the host.
pub struct TlsSession {
    session: Session,
    buffers: Buffers,
    handshaking: bool,
    handshake_reported: bool,
}

impl TlsSession {
    /// A client session for `socket.upgradeToTLS` / `tls.connect`.
    pub fn client(
        config: Arc<rustls::ClientConfig>,
        server_name: rustls::pki_types::ServerName<'static>,
    ) -> Result<Self, String> {
        let conn = rustls::client::UnbufferedClientConnection::new(config, server_name)
            .map_err(|e| node_message(&e))?;
        Ok(Self::with(Session::Client(Box::new(conn))))
    }

    /// A server session for an accepted `https` / `tls` connection.
    pub fn server(config: Arc<rustls::ServerConfig>) -> Result<Self, String> {
        let conn = rustls::server::UnbufferedServerConnection::new(config)
            .map_err(|e| node_message(&e))?;
        Ok(Self::with(Session::Server(Box::new(conn))))
    }

    fn with(session: Session) -> Self {
        Self {
            session,
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
        }
    }

    /// The negotiated ALPN protocol, once the handshake has completed.
    pub fn alpn_protocol(&self) -> Option<Vec<u8>> {
        match &self.session {
            Session::Client(c) => c.alpn().map(<[u8]>::to_vec),
            Session::Server(s) => s.alpn().map(<[u8]>::to_vec),
        }
    }

    /// The peer's certificate chain, leaf first, DER-encoded.
    pub fn peer_certificates(&self) -> Option<Vec<Vec<u8>>> {
        let chain = match &self.session {
            Session::Client(c) => c.certificates(),
            Session::Server(s) => s.certificates(),
        }?;
        Some(chain.iter().map(|c| c.as_ref().to_vec()).collect())
    }

    pub fn is_handshaking(&self) -> bool {
        self.handshaking
    }

    /// `"TLSv1.2"` / `"TLSv1.3"`, or `""` before the version is known. The
    /// string Node reports from `socket.getProtocol()`.
    pub fn protocol_version(&self) -> &'static str {
        let version = match &self.session {
            Session::Client(c) => c.protocol_version(),
            Session::Server(s) => s.protocol_version(),
        };
        match version {
            Some(rustls::ProtocolVersion::TLSv1_2) => "TLSv1.2",
            Some(rustls::ProtocolVersion::TLSv1_3) => "TLSv1.3",
            _ => "",
        }
    }

    pub fn peer_closed(&self) -> bool {
        self.buffers.peer_closed
    }

    /// The terminal failure, if the session has one. A failed session produces
    /// no further plaintext and refuses writes. The string is Node's cause
    /// code followed by rustls's own text.
    pub fn failure(&self) -> Option<&str> {
        self.buffers.failed.as_deref()
    }

    /// Hand ciphertext that arrived on the socket to the session.
    pub fn receive(&mut self, ciphertext: &[u8]) {
        let b = &mut self.buffers;
        if b.failed.is_some() {
            return;
        }
        if b.input.len() + ciphertext.len() > INPUT_LIMIT {
            b.failed = Some("ERR_SSL_PROTOCOL_ERROR: TLS input limit".to_string());
            return;
        }
        b.input.extend_from_slice(ciphertext);
    }

    /// Queue application data. It is encrypted as soon as the handshake allows,
    /// so a `socket.write()` issued during the handshake is not lost.
    pub fn write(&mut self, plaintext: &[u8]) {
        let b = &mut self.buffers;
        if b.failed.is_some() || b.want_close || b.close_sent {
            return;
        }
        b.deferred.extend_from_slice(plaintext);
    }

    /// Ask for `close_notify` to be sent once queued writes have been encrypted.
    pub fn close_notify(&mut self) {
        if self.buffers.failed.is_none() {
            self.buffers.want_close = true;
        }
    }

    /// Whether `close_notify` has been encoded into the output.
    pub fn close_sent(&self) -> bool {
        self.buffers.close_sent
    }

    /// Take the ciphertext that must be written to the socket.
    pub fn take_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.buffers.out)
    }

    /// Whether any ciphertext is waiting to be written.
    pub fn has_output(&self) -> bool {
        !self.buffers.out.is_empty()
    }

    /// Take the decrypted application data received so far.
    pub fn take_plaintext(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.buffers.plain)
    }

    /// Run the state machine until it blocks. Bounded: every iteration either
    /// consumes input, produces output, or reaches a terminal/blocked state,
    /// and the two `TransmitTlsData` iterations alternate on `transmitted`.
    pub fn pump(&mut self) -> Progress {
        let mut progress = Progress::default();
        if self.buffers.failed.is_some() {
            return progress;
        }
        // A bound no correct handshake approaches. It exists so a rustls state
        // this code did not anticipate cannot spin the event loop forever.
        for _ in 0..4096 {
            let action = match &mut self.session {
                Session::Client(c) => step(c.as_mut(), &mut self.buffers),
                Session::Server(s) => step(s.as_mut(), &mut self.buffers),
            };
            if self.buffers.failed.is_some() || matches!(action, Action::Blocked) {
                break;
            }
        }
        progress.peer_closed = self.buffers.peer_closed;
        let still = match &self.session {
            Session::Client(c) => c.handshaking(),
            Session::Server(s) => s.handshaking(),
        };
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

fn step<E: Endpoint>(tls: &mut E, b: &mut Buffers) -> Action {
    if let Some(required) = b.grow_scratch.take() {
        // Applied here rather than inside the arm below, where `b` is already
        // borrowed by the rustls state.
        flush_scratch(b);
        b.scratch.resize(required, 0);
    }
    let UnbufferedStatus { discard, state } = tls.process(&mut b.input);
    let mut discard = discard;
    let action = match state {
        Err(error) => {
            b.failed = Some(node_message(&error));
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
                        b.failed = Some("ERR_SSL_PROTOCOL_ERROR: TLS output limit".to_string());
                        Action::Blocked
                    } else {
                        b.grow_scratch = Some(required.required_size);
                        Action::Progress
                    }
                }
                Err(e) => {
                    b.failed = Some(format!("ERR_SSL_PROTOCOL_ERROR: {e:?}"));
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
                // bytes into the caller's output queue *is* the transmission
                // as far as ordering goes: nothing encrypted afterwards can
                // overtake them. The caller submits `take_output()` before the
                // next completion is processed.
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
                                Some("ERR_SSL_PROTOCOL_ERROR: TLS plaintext limit".to_string());
                        } else {
                            b.plain.extend_from_slice(record.payload);
                        }
                    }
                    Err(e) => b.failed = Some(node_message(&e)),
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
                        b.failed = Some(format!("ERR_SSL_PROTOCOL_ERROR: {e:?}"));
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
                        b.failed = Some(format!("ERR_SSL_PROTOCOL_ERROR: {e:?}"));
                        Action::Blocked
                    }
                }
            } else {
                Action::Blocked
            }
        }
        Ok(ConnectionState::BlockedHandshake) => Action::Blocked,
        Ok(ConnectionState::PeerClosed) => {
            b.peer_closed = true;
            Action::Blocked
        }
        Ok(ConnectionState::Closed) => {
            b.peer_closed = true;
            Action::Blocked
        }
        Ok(_) => {
            // `ReadEarlyData` and any state added by a later rustls. Early data
            // is not enabled on either config, so reaching one is a bug, not a
            // peer behaviour: fail the connection rather than spin.
            b.failed = Some("ERR_SSL_PROTOCOL_ERROR: unsupported TLS state".to_string());
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

/// Node's cause code plus rustls's own text, the shape `net`/`tls` already
/// reports for a handshake failure.
pub fn node_message(error: &rustls::Error) -> String {
    format!("{}: {error}", turnloop_tls::node_error_code(error))
}

/// Parse a Node `servername` into the rustls type, keeping Node's error text.
///
/// An IP literal is a valid `ServerName` but must not be sent as SNI; rustls
/// handles that itself once the name is built from the address.
pub fn server_name(name: &str) -> Result<rustls::pki_types::ServerName<'static>, String> {
    rustls::pki_types::ServerName::try_from(name.to_string())
        .map_err(|_| format!("ERR_TLS_CERT_ALTNAME_INVALID: invalid servername {name:?}"))
}

/// Node's cause code alone, for `err.code`.
pub fn node_code(error: &rustls::Error) -> &'static str {
    turnloop_tls::node_error_code(error)
}
