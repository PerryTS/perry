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
                // `None` = turnloop-tls's default provider, `ring`.
                provider: None,
            };
            turnloop_tls::ClientConfig::new(options, unix_seconds()).ok()
        })
        .as_ref()
}
