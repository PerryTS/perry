//! A public, binding-agnostic TLS **client** installed on a turnloop socket.
//!
//! # What was missing, and for whom
//!
//! P5 put TLS *above* the turnloop socket rather than beside it, which is what
//! made `socket.upgradeToTLS` possible without a descriptor handoff. But the
//! client half of that work stayed inside `perry-ext-net`:
//! `turnloop_tls_io::install_server_session` is `pub`, while
//! `begin_client_upgrade` is `pub(crate)`, takes that crate's own
//! `TlsClientConfigData` (built by reading JS values) and settles a
//! `JsNativeAsyncCompletion`. None of that is reachable from — or appropriate
//! for — a caller that has no JS promise at the point of upgrade.
//!
//! Two such callers exist and both were stuck:
//!
//! * the four database bindings on `perry-db-turnloop`. Every one of their
//!   protocol cores already *asks* for the upgrade (`Event::UpgradeTls`) and
//!   already has the acknowledgement (`tls_established`); the host simply had
//!   no TLS to give them, so all four answered "TLS is not available on the
//!   turnloop transport" and the client fell back to its tokio driver.
//! * `http2.connect('https://…')`, which needs **ALPN** negotiated before it
//!   knows whether it may speak HTTP/2 at all.
//!
//! # ALPN is part of the contract
//!
//! [`TlsClientOptions::alpn`] is a required part of the options rather than an
//! afterthought, and [`TlsFacts::alpn`] reports what the server actually
//! selected. The three callers want three different things — `http2.connect`
//! offers `h2` alone and must refuse a server that declines it, `fetch` offers
//! `h2,http/1.1` and switches on the answer, a database client offers nothing
//! — and an installer that could not express all three would have to be
//! wrapped differently by each of them, which is how the `pub(crate)` version
//! came to be shaped for exactly one caller.
//!
//! # Channel binding
//!
//! [`TlsFacts::channel_binding`] carries the RFC 5929 `tls-server-end-point`
//! digest of the **verified leaf**, which is what PostgreSQL's
//! SCRAM-SHA-256-PLUS needs. It is derived here rather than left to the caller
//! because the leaf certificate is only reachable through the session, and a
//! caller that has to reach for the chain itself is a caller that can reach for
//! an *unverified* chain. `None` means the leaf's signature algorithm has no
//! defined binding (Ed25519, notably) — the honest answer, and the one that
//! makes `turnloop-postgres` fall back to plain SCRAM rather than authenticate
//! with a bogus binding.
//!
//! # GC
//!
//! A transport holds owned `Vec<u8>`s and nothing else — no JS value, no heap
//! pointer, no promise token, so no GC root and no entry in
//! `scripts/gc_runtime_root_holders.json`. Plaintext is copied into a JS value
//! by the caller, on the owning thread (P1's rule, unchanged). Settling a
//! promise on the outcome is the *caller's* job, which is the whole difference
//! between this and `begin_client_upgrade`.

use std::time::{SystemTime, UNIX_EPOCH};

use perry_ffi::turnloop_net as tl;
use perry_tls_session::{server_name, TlsClientSession};

/// What a caller must say to put TLS on a socket.
///
/// Deliberately free of both JS values and rustls types: `perry-db-turnloop`
/// has neither, and a binding that had to build a `rustls::ClientConfig` would
/// need rustls in its own manifest.
#[derive(Clone, Debug)]
pub struct TlsClientOptions {
    /// SNI name and the name the certificate is verified against.
    pub servername: String,
    /// ALPN protocols to offer, in preference order. Empty offers no ALPN
    /// extension at all, which is what a database client wants.
    pub alpn: Vec<Vec<u8>>,
    /// Node's `rejectUnauthorized`. `false` disables certificate verification
    /// entirely — the caller is responsible for having been asked.
    pub reject_unauthorized: bool,
    /// Explicit trust roots, PEM. Non-empty **replaces** the default root set,
    /// which is Node's `ca` semantics rather than an addition.
    pub ca_pem: Vec<u8>,
    /// Extra roots on top of the default set — the `NODE_EXTRA_CA_CERTS` shape.
    /// Ignored when `ca_pem` is non-empty, exactly as Node ignores it then.
    pub extra_ca_pem: Vec<u8>,
    /// Send SNI. Off only for a caller that deliberately wants it off; an IP
    /// literal is handled by rustls itself and needs no flag.
    pub enable_sni: bool,
}

impl Default for TlsClientOptions {
    fn default() -> Self {
        Self {
            servername: String::new(),
            alpn: Vec::new(),
            reject_unauthorized: true,
            ca_pem: Vec::new(),
            extra_ca_pem: Vec::new(),
            enable_sni: true,
        }
    }
}

impl TlsClientOptions {
    /// Options carrying Perry's process-wide TLS environment
    /// (`NODE_TLS_REJECT_UNAUTHORIZED`, `SSL_CERT_FILE`, `NODE_EXTRA_CA_CERTS`),
    /// resolved through `perry_ffi::node_tls_client_environment` so this path
    /// and `node:https` answer the same way.
    ///
    /// A caller then overrides whatever its own option surface names — a `ca`
    /// on a `pg` client, a `rejectUnauthorized: false` on a `mysql2` one.
    pub fn from_node_environment(servername: impl Into<String>) -> Self {
        let environment = perry_ffi::node_tls_client_environment();
        let mut extra_ca_pem = Vec::new();
        for pem in environment.ca_pems() {
            extra_ca_pem.extend_from_slice(pem);
            if !pem.ends_with(b"\n") {
                extra_ca_pem.push(b'\n');
            }
        }
        Self {
            servername: servername.into(),
            alpn: Vec::new(),
            reject_unauthorized: !environment.accepts_invalid_certificates(),
            ca_pem: Vec::new(),
            extra_ca_pem,
            enable_sni: true,
        }
    }

    /// Offer these ALPN protocols, in preference order.
    #[must_use]
    pub fn with_alpn(mut self, protocols: &[&[u8]]) -> Self {
        self.alpn = protocols.iter().map(|p| p.to_vec()).collect();
        self
    }
}

/// What the handshake negotiated. Produced once, when it completes.
#[derive(Clone, Debug, Default)]
pub struct TlsFacts {
    /// The protocol the server selected from [`TlsClientOptions::alpn`], or
    /// `None` when none was offered or the server selected none.
    pub alpn: Option<Vec<u8>>,
    /// The peer's chain, leaf first, DER-encoded.
    pub peer_certificates: Vec<Vec<u8>>,
    /// RFC 5929 `tls-server-end-point` over the verified leaf — the digest
    /// PostgreSQL's SCRAM-SHA-256-PLUS binds to. `None` when the leaf's
    /// signature algorithm has no defined binding.
    pub channel_binding: Option<Vec<u8>>,
}

impl TlsFacts {
    /// The negotiated protocol as a string, for a diagnostic or a JS-visible
    /// `alpnProtocol`. Empty when nothing was negotiated.
    pub fn alpn_str(&self) -> &str {
        self.alpn
            .as_deref()
            .and_then(|a| std::str::from_utf8(a).ok())
            .unwrap_or("")
    }
}

/// What one [`TlsClientTransport::pump`] produced.
#[derive(Debug, Default)]
pub struct TlsProgress {
    /// Decrypted application data. Feed it to the protocol core.
    pub plaintext: Vec<u8>,
    /// The handshake completed during this pump. Reported exactly once; the
    /// facts are in [`TlsClientTransport::facts`].
    pub handshake_done: bool,
    /// The peer sent `close_notify`; treat it as readable EOF.
    pub peer_closed: bool,
    /// The session failed terminally. Node's cause code, then rustls's text.
    pub failure: Option<String>,
}

/// One TLS client session bound to one turnloop handle.
///
/// The caller owns it — there is no process-global table keyed by handle id,
/// because a turnloop handle belongs to the loop that created it and a
/// `Mutex<HashMap<i64, _>>` would be a claim that it can be driven from another
/// thread. That is the same reasoning `perry-db-turnloop` records for keeping
/// its connection table thread-local.
pub struct TlsClientTransport {
    session: TlsClientSession,
    facts: Option<TlsFacts>,
    /// Ciphertext bytes handed to turnloop since installation. Only a
    /// diagnostic: a caller that never sees this move has an installer that
    /// ran but a subject that did not.
    cipher_written: u64,
}

impl TlsClientTransport {
    /// Build a client session for `options`. No socket is touched yet — call
    /// [`Self::pump`] to send the ClientHello.
    pub fn connect(options: &TlsClientOptions) -> Result<Self, String> {
        let config = client_config(options)?;
        let name = server_name(&options.servername)?;
        Ok(Self {
            session: TlsClientSession::new(&config, name)?,
            facts: None,
            cipher_written: 0,
        })
    }

    /// Hand ciphertext from a `NET_DATA` completion to the session.
    pub fn receive(&mut self, ciphertext: &[u8]) {
        self.session.receive(ciphertext);
    }

    /// Queue application plaintext. It is encrypted as soon as the handshake
    /// allows, so a write issued during the handshake is not lost.
    pub fn write(&mut self, plaintext: &[u8]) {
        self.session.write(plaintext);
    }

    /// Ask for `close_notify` once queued writes have been encrypted.
    pub fn close_notify(&mut self) {
        self.session.close_notify();
    }

    /// Whether the handshake is still in flight.
    pub fn is_handshaking(&self) -> bool {
        self.session.is_handshaking()
    }

    /// What the handshake negotiated, once it has completed.
    pub fn facts(&self) -> Option<&TlsFacts> {
        self.facts.as_ref()
    }

    /// Total ciphertext bytes submitted to turnloop on this session.
    ///
    /// A liveness counter, for the reason CLAUDE.md's "four ways a gate can be
    /// unable to fail" gives: a fixture that asserts "TLS was installed" proves
    /// nothing if no byte was ever encrypted through it.
    pub fn cipher_written(&self) -> u64 {
        self.cipher_written
    }

    /// Run the state machine, submit whatever ciphertext it produced on `id`,
    /// and report what happened.
    ///
    /// The single point where this transport touches the socket. A submission
    /// failure is reported as a session failure rather than returned
    /// separately, so a caller has one terminal path rather than two.
    pub fn pump(&mut self, id: i64) -> TlsProgress {
        let mut out = TlsProgress::default();
        let progress = self.session.pump();
        out.peer_closed = progress.peer_closed;
        out.plaintext = self.session.take_plaintext();

        let ciphertext = self.session.take_output();
        if !ciphertext.is_empty() {
            // `user` is zero: the ciphertext write is not an application write.
            // A caller that needs per-write acknowledgement accounts for it
            // itself — plaintext and ciphertext byte counts are not the same
            // number and the mapping is not one-to-one.
            if let Err(err) = tl::write(id, &ciphertext, 0) {
                out.failure = Some(err.message());
                return out;
            }
            self.cipher_written = self.cipher_written.saturating_add(ciphertext.len() as u64);
        }

        if let Some((code, text)) = self.session.failure() {
            out.failure = Some(format!("{code}: {text}"));
            return out;
        }
        if progress.handshake_done {
            out.handshake_done = true;
            self.facts = Some(TlsFacts {
                alpn: self.session.alpn_protocol(),
                peer_certificates: self.session.peer_certificates().unwrap_or_default(),
                channel_binding: self.session.tls_server_end_point(),
            });
        }
        out
    }
}

/// `turnloop_tls` takes wall time from the host rather than reading a clock
/// itself. One read per configuration, not per record.
fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Build the rustls configuration for `options`.
///
/// Public because a caller that opens many connections to the same endpoint
/// should build it once and share the session cache, which is what
/// `turnloop_tls::ClientConfig` is for.
pub fn client_config(options: &TlsClientOptions) -> Result<turnloop_tls::ClientConfig, String> {
    use turnloop_tls::rustls::pki_types::{pem::PemObject, CertificateDer};

    let ca = if options.ca_pem.is_empty() {
        None
    } else {
        let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(&options.ca_pem)
            .collect::<Result<_, _>>()
            .map_err(|error| format!("ERR_TLS_CERT_ALTNAME_INVALID: invalid ca: {error}"))?;
        if certs.is_empty() {
            return Err("ERR_TLS_CERT_ALTNAME_INVALID: ca contained no certificate".to_string());
        }
        Some(certs)
    };
    turnloop_tls::ClientConfig::new(
        turnloop_tls::ClientOptions {
            alpn: options.alpn.clone(),
            ca,
            extra_ca_pem: options.extra_ca_pem.clone(),
            reject_unauthorized: options.reject_unauthorized,
            enable_sni: options.enable_sni,
            // `None` selects turnloop-tls's own default, which is `ring` —
            // the provider this workspace pins the `ring` feature for, and the
            // one alpha.6 used unconditionally. Not a new choice.
            provider: None,
        },
        unix_seconds(),
    )
    .map_err(|error| format!("{}: {error}", turnloop_tls::node_error_code(&error)))
}

#[cfg(test)]
mod tests;
