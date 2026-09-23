//! A host-driven TLS session for **either** side of a connection, over a
//! caller-built `rustls` configuration.
//!
//! [`crate::TlsClientSession`] is the outbound-client shape: its config comes
//! from [`turnloop_tls::ClientConfig`], which fixes the verifier and the
//! crypto provider. Node's `tls` module needs more than that on both sides —
//! a server with a dynamic certificate resolver, SNI contexts and an optional
//! client-certificate verifier, and a client with a Node-configured CA
//! verifier or `rejectUnauthorized: false` — so this type takes an
//! already-built `Arc<rustls::ServerConfig>` / `Arc<rustls::ClientConfig>`
//! instead. The record processing is the same `rustls::unbuffered` core
//! `turnloop_tls` wraps (and re-exports), driven the same way: ciphertext in
//! with [`TlsSession::receive`], ciphertext out with
//! [`TlsSession::take_output`], plaintext in with [`TlsSession::write`] and out
//! with [`TlsSession::take_plaintext`], and [`TlsSession::pump`] between them.
//!
//! It has no socket and no I/O: the caller owns the transport. That is what
//! lets `perry-stdlib`'s `node:tls` server and bundled `net` / `ws` clients use
//! it over the sockets they already have, instead of `tokio_rustls`.
//!
//! Two differences from the client session, both for parity with the
//! `tokio_rustls` streams this replaces:
//!
//! * **Fatal alerts are flushed.** When rustls rejects the peer it queues an
//!   alert *and* returns the error; `tokio_rustls` writes that alert before
//!   surfacing the error, so the peer sees `alert: …` rather than a bare EOF.
//!   [`TlsSession::pump`] drains the alert into the output on failure.
//! * **The failure keeps rustls's own text** ([`TlsSession::failure`]), which is
//!   exactly what `tokio_rustls` surfaced as its `io::Error`'s message; the
//!   Node cause code is kept beside it.
//!
//! # GC
//!
//! Only owned `Vec<u8>`s — no JS value, no heap pointer, no GC root.

use std::sync::Arc;

use turnloop_tls::rustls::{
    self,
    pki_types::CertificateDer,
    unbuffered::{ConnectionState, EncodeError, UnbufferedStatus},
};

/// Retained ciphertext scratch. One TLS record is at most ~16 KiB plus
/// overhead; 64 KiB covers a handshake flight without reallocating.
const SCRATCH_CAPACITY: usize = 64 * 1024;
/// Hard cap on unparsed ciphertext, so a peer that never completes a record
/// cannot grow the buffer without bound.
const INPUT_LIMIT: usize = 1024 * 1024;
/// Hard cap on decrypted plaintext the caller has not taken yet.
const PLAINTEXT_LIMIT: usize = 8 * 1024 * 1024;
/// Ceiling on growing the scratch for one oversized handshake flight.
const SCRATCH_LIMIT: usize = 4 * 1024 * 1024;
/// Most records rustls queues after a fatal error (the alert, and in principle
/// anything encoded but not yet taken). A bound, not an expectation.
const ALERT_DRAIN_LIMIT: usize = 16;

/// What the caller must know after [`TlsSession::pump`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    /// The handshake completed during this pump. Reported exactly once.
    pub handshake_done: bool,
    /// The peer sent `close_notify`; no more plaintext will arrive.
    pub peer_closed: bool,
}

/// A terminal session failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    /// Node's cause code (`turnloop_tls::node_error_code`).
    pub code: &'static str,
    /// rustls's own text for the error — the message `tokio_rustls` reported.
    pub message: String,
}

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
    grow_scratch: Option<usize>,
    failed: Option<Failure>,
    alert_drained: bool,
}

enum Action {
    Progress,
    Blocked,
}

trait Endpoint {
    type Data;
    fn process<'c, 'i>(&'c mut self, input: &'i mut [u8]) -> UnbufferedStatus<'c, 'i, Self::Data>;
}

impl Endpoint for rustls::client::UnbufferedClientConnection {
    type Data = rustls::client::ClientConnectionData;
    fn process<'c, 'i>(&'c mut self, input: &'i mut [u8]) -> UnbufferedStatus<'c, 'i, Self::Data> {
        self.process_tls_records(input)
    }
}

impl Endpoint for rustls::server::UnbufferedServerConnection {
    type Data = rustls::server::ServerConnectionData;
    fn process<'c, 'i>(&'c mut self, input: &'i mut [u8]) -> UnbufferedStatus<'c, 'i, Self::Data> {
        self.process_tls_records(input)
    }
}

enum Side {
    Client(Box<rustls::client::UnbufferedClientConnection>),
    Server(Box<rustls::server::UnbufferedServerConnection>),
}

/// One TLS connection's state — client or server — driven by the host.
pub struct TlsSession {
    side: Side,
    /// The client's SNI, for a server session. rustls 0.23's unbuffered server
    /// connection does not expose the name it parsed (`ServerConnection::
    /// server_name` has no unbuffered counterpart), so it is read from the
    /// ClientHello as it arrives; see [`sni`].
    sni: sni::Capture,
    buffers: Buffers,
    handshaking: bool,
    handshake_reported: bool,
}

impl TlsSession {
    /// A client session over a caller-built rustls config.
    pub fn client(
        config: Arc<rustls::ClientConfig>,
        server_name: rustls::pki_types::ServerName<'static>,
    ) -> Result<Self, rustls::Error> {
        let conn = rustls::client::UnbufferedClientConnection::new(config, server_name)?;
        Ok(Self::with(
            Side::Client(Box::new(conn)),
            sni::Capture::Done(None),
        ))
    }

    /// A server session for one accepted connection.
    pub fn server(config: Arc<rustls::ServerConfig>) -> Result<Self, rustls::Error> {
        let conn = rustls::server::UnbufferedServerConnection::new(config)?;
        Ok(Self::with(
            Side::Server(Box::new(conn)),
            sni::Capture::default(),
        ))
    }

    fn with(side: Side, sni: sni::Capture) -> Self {
        Self {
            side,
            sni,
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
                alert_drained: false,
            },
            handshaking: true,
            handshake_reported: false,
        }
    }

    /// The negotiated ALPN protocol, once the handshake has completed.
    pub fn alpn_protocol(&self) -> Option<&[u8]> {
        match &self.side {
            Side::Client(c) => c.alpn_protocol(),
            Side::Server(s) => s.alpn_protocol(),
        }
    }

    /// The peer's certificate chain, leaf first. Verification has already been
    /// decided by the configured verifier by the time this returns `Some`.
    pub fn peer_certificates(&self) -> Option<&[CertificateDer<'static>]> {
        match &self.side {
            Side::Client(c) => c.peer_certificates(),
            Side::Server(s) => s.peer_certificates(),
        }
    }

    /// The negotiated protocol version, once known.
    pub fn protocol_version(&self) -> Option<rustls::ProtocolVersion> {
        match &self.side {
            Side::Client(c) => c.protocol_version(),
            Side::Server(s) => s.protocol_version(),
        }
    }

    /// The SNI host name the client sent, normalised as rustls normalises it
    /// (lower-cased, trailing dot removed; an IP literal is not a name). Always
    /// `None` on a client session.
    pub fn server_name(&self) -> Option<&str> {
        self.sni.name()
    }

    pub fn is_handshaking(&self) -> bool {
        self.handshaking
    }

    /// The peer sent `close_notify`.
    pub fn peer_closed(&self) -> bool {
        self.buffers.peer_closed
    }

    /// Whether `close_notify` has been encoded into the output.
    pub fn close_sent(&self) -> bool {
        self.buffers.close_sent
    }

    /// The terminal failure, if any. A failed session produces no further
    /// plaintext and refuses writes; any alert it queued is in the output.
    pub fn failure(&self) -> Option<&Failure> {
        self.buffers.failed.as_ref()
    }

    /// Hand ciphertext that arrived on the transport to the session.
    pub fn receive(&mut self, ciphertext: &[u8]) {
        let b = &mut self.buffers;
        if b.failed.is_some() {
            return;
        }
        if b.input.len() + ciphertext.len() > INPUT_LIMIT {
            b.failed = Some(Failure {
                code: "ERR_SSL_PROTOCOL_ERROR",
                message: "TLS input limit".to_string(),
            });
            return;
        }
        b.input.extend_from_slice(ciphertext);
        self.sni.feed(ciphertext);
    }

    /// Queue application data; it is encrypted as soon as the handshake allows.
    pub fn write(&mut self, plaintext: &[u8]) {
        let b = &mut self.buffers;
        if b.failed.is_some() || b.want_close || b.close_sent {
            return;
        }
        b.deferred.extend_from_slice(plaintext);
    }

    /// Plaintext queued with [`TlsSession::write`] that is not encrypted yet.
    pub fn pending_plaintext(&self) -> usize {
        self.buffers.deferred.len()
    }

    /// Ask for `close_notify` once queued writes have been encrypted.
    pub fn close_notify(&mut self) {
        if self.buffers.failed.is_none() {
            self.buffers.want_close = true;
        }
    }

    /// Take the ciphertext that must be written to the transport.
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
    /// consumes input, produces output, or reaches a terminal/blocked state.
    pub fn pump(&mut self) -> Progress {
        let mut progress = Progress::default();
        if self.buffers.failed.is_none() {
            for _ in 0..4096 {
                let action = match &mut self.side {
                    Side::Client(c) => step(c.as_mut(), &mut self.buffers),
                    Side::Server(s) => step(s.as_mut(), &mut self.buffers),
                };
                if self.buffers.failed.is_some() || matches!(action, Action::Blocked) {
                    break;
                }
            }
        }
        if self.buffers.failed.is_some() && !self.buffers.alert_drained {
            self.buffers.alert_drained = true;
            match &mut self.side {
                Side::Client(c) => drain_alert(c.as_mut(), &mut self.buffers),
                Side::Server(s) => drain_alert(s.as_mut(), &mut self.buffers),
            }
        }
        progress.peer_closed = self.buffers.peer_closed;
        let still = match &self.side {
            Side::Client(c) => c.is_handshaking(),
            Side::Server(s) => s.is_handshaking(),
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

fn failure_of(error: &rustls::Error) -> Failure {
    Failure {
        code: turnloop_tls::node_error_code(error),
        message: error.to_string(),
    }
}

fn internal_failure(message: String) -> Failure {
    Failure {
        code: "ERR_SSL_PROTOCOL_ERROR",
        message,
    }
}

/// rustls queues its fatal alert in the same call that returns the error; the
/// records come back as `EncodeTlsData` from the next `process`. Move them into
/// the output so the peer is told why, as `tokio_rustls` did.
fn drain_alert<E: Endpoint>(tls: &mut E, b: &mut Buffers) {
    flush_scratch(b);
    for _ in 0..ALERT_DRAIN_LIMIT {
        // The same input buffer, not an empty one: rustls's handshake
        // deframer holds positions into it across calls.
        let input_len = b.input.len();
        let UnbufferedStatus { discard, state } = tls.process(&mut b.input);
        let discard = discard.min(input_len);
        match state {
            Ok(ConnectionState::EncodeTlsData(mut encode)) => {
                match encode.encode(&mut b.scratch[..]) {
                    Ok(n) => {
                        b.out.extend_from_slice(&b.scratch[..n]);
                    }
                    Err(_) => break,
                }
            }
            Ok(ConnectionState::TransmitTlsData(transmit)) => transmit.done(),
            _ => break,
        }
        b.input.drain(..discard);
    }
}

fn step<E: Endpoint>(tls: &mut E, b: &mut Buffers) -> Action {
    if let Some(required) = b.grow_scratch.take() {
        flush_scratch(b);
        b.scratch.resize(required, 0);
    }
    let UnbufferedStatus { discard, state } = tls.process(&mut b.input);
    let mut discard = discard;
    let action = match state {
        Err(error) => {
            b.failed = Some(failure_of(&error));
            Action::Blocked
        }
        Ok(ConnectionState::EncodeTlsData(mut encode)) => {
            match encode.encode(&mut b.scratch[b.scratch_len..]) {
                Ok(n) => {
                    b.scratch_len += n;
                    Action::Progress
                }
                Err(EncodeError::InsufficientSize(required)) => {
                    if required.required_size > SCRATCH_LIMIT {
                        b.failed = Some(internal_failure("TLS output limit".to_string()));
                        Action::Blocked
                    } else {
                        b.grow_scratch = Some(required.required_size);
                        Action::Progress
                    }
                }
                Err(e) => {
                    b.failed = Some(internal_failure(format!("{e:?}")));
                    Action::Blocked
                }
            }
        }
        Ok(ConnectionState::TransmitTlsData(transmit)) => {
            if b.transmitted {
                transmit.done();
                b.transmitted = false;
            } else {
                // The caller writes `take_output()` in order before anything
                // encrypted later, so queuing the bytes is the transmission as
                // far as ordering goes (the same deviation P5 documented).
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
                            b.failed = Some(internal_failure("TLS plaintext limit".to_string()));
                        } else {
                            b.plain.extend_from_slice(record.payload);
                        }
                    }
                    Err(e) => b.failed = Some(failure_of(&e)),
                }
            }
            Action::Progress
        }
        Ok(ConnectionState::WriteTraffic(mut write)) => {
            if !b.deferred.is_empty() {
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
                        b.failed = Some(internal_failure(format!("{e:?}")));
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
                        b.failed = Some(internal_failure(format!("{e:?}")));
                        Action::Blocked
                    }
                }
            } else {
                Action::Blocked
            }
        }
        Ok(ConnectionState::BlockedHandshake) => Action::Blocked,
        // Emitted exactly once; the next `process` moves on to `WriteTraffic`
        // (a half-open connection may still write) or `Closed`.
        Ok(ConnectionState::PeerClosed) => {
            b.peer_closed = true;
            Action::Progress
        }
        Ok(ConnectionState::Closed) => {
            b.peer_closed = true;
            Action::Blocked
        }
        Ok(_) => {
            // `ReadEarlyData` and any later state: early data is never enabled
            // by Perry's configs, so fail rather than spin.
            b.failed = Some(internal_failure("unsupported TLS state".to_string()));
            Action::Blocked
        }
    };
    if discard > 0 {
        b.input.drain(..discard.min(b.input.len()));
    }
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

mod sni;

#[cfg(test)]
mod tests;
