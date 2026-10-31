//! Perry's outbound TLS client configuration, and the re-export of the session
//! state machine that uses it.
//!
//! The state machine itself lives in [`perry_tls_session`] — one copy, shared
//! with the CLI's blocking HTTP client (P11). What stays here is the one thing
//! that cannot: the configuration is read through
//! `perry_ffi::node_tls_client_environment()`, so that `node:https` and this
//! path answer the same way about `NODE_TLS_REJECT_UNAUTHORIZED`,
//! `SSL_CERT_FILE` and `NODE_EXTRA_CA_CERTS`.
//!
//! Callers inside perry-stdlib keep using `crate::turnloop_tls_client::…`;
//! the re-exports below make that path mean the shared crate's types.

pub(crate) use perry_tls_session::{server_name, TlsClientSession};

mod ca_leaf;

use std::time::{SystemTime, UNIX_EPOCH};

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
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
        .get_or_init(|| build_config(&TlsOverride::default()))
        .as_ref()
}

/// Build the outbound configuration: the process environment's trust settings
/// with `over` layered on top.
fn build_config(over: &TlsOverride) -> Option<turnloop_tls::ClientConfig> {
    let environment = perry_ffi::node_tls_client_environment();
    let mut extra_ca_pem = Vec::new();
    for pem in environment.ca_pems() {
        extra_ca_pem.extend_from_slice(pem);
        if !pem.ends_with(b"\n") {
            extra_ca_pem.push(b'\n');
        }
    }
    extra_ca_pem.extend_from_slice(&over.ca_pem);
    let options = turnloop_tls::ClientOptions {
        alpn: vec![b"http/1.1".to_vec()],
        ca: None,
        extra_ca_pem,
        reject_unauthorized: over
            .reject_unauthorized
            .unwrap_or(!environment.accepts_invalid_certificates()),
        enable_sni: true,
        // `None` = turnloop-tls's default provider, `ring`.
        provider: None,
    };
    ca_leaf::client_config(options, unix_seconds()).ok()
}

/// Per-request trust settings carried by an undici `Agent`'s `connect` options.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct TlsOverride {
    /// Extra trust roots, PEM, added to the default set.
    pub(crate) ca_pem: Vec<u8>,
    /// `connect.rejectUnauthorized`; `None` keeps the process-wide answer.
    pub(crate) reject_unauthorized: Option<bool>,
}

/// A configuration built for one distinct [`TlsOverride`]. `id` separates the
/// connections it verified from every other configuration's in the pool.
pub(crate) struct TlsProfile {
    pub(crate) id: u64,
    pub(crate) config: turnloop_tls::ClientConfig,
}

/// The profile for `over`, built on first use and shared by every request that
/// carries an equal override. `None` when rustls refused the PEM.
pub(crate) fn profile_for(over: &TlsOverride) -> Option<&'static TlsProfile> {
    static PROFILES: std::sync::Mutex<
        Option<std::collections::HashMap<TlsOverride, Option<&'static TlsProfile>>>,
    > = std::sync::Mutex::new(None);
    let mut guard = PROFILES.lock().ok()?;
    let profiles = guard.get_or_insert_with(Default::default);
    if let Some(profile) = profiles.get(over) {
        return *profile;
    }
    // Ids start at 1; a default-configuration connection has no profile.
    let id = profiles.len() as u64 + 1;
    let profile = build_config(over).map(|config| &*Box::leak(Box::new(TlsProfile { id, config })));
    profiles.insert(over.clone(), profile);
    profile
}
