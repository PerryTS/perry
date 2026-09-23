//! The client's TLS configuration, and the secure random the WebSocket
//! handshake needs.
//!
//! # No environment configuration, deliberately
//!
//! Perry's *runtime* builds its `ClientConfig` from Node's TLS environment
//! through `perry_ffi` — `NODE_TLS_REJECT_UNAUTHORIZED`, `NODE_EXTRA_CA_CERTS`,
//! `SSL_CERT_FILE` — because a JS program's `fetch` should answer the way
//! `node:https` does. **This crate reads none of them**, and that is a decision
//! rather than an omission.
//!
//! Its two callers are the `perry` CLI and `perry-ext-axios`, and neither
//! honoured any of those variables before: the CLI used the workspace `reqwest`
//! with `rustls-tls` and webpki roots and no environment handling at all, and
//! old axios built a bare `reqwest::Client::new()`. Honouring
//! `NODE_TLS_REJECT_UNAUTHORIZED=0` here would mean that a variable JS
//! developers set casually, for an unrelated program, silently turns off
//! certificate verification for `perry publish` — which uploads Apple signing
//! certificates, API tokens and licence keys. An earlier draft of this file did
//! exactly that, and described it as preserving behaviour it was in fact
//! introducing.
//!
//! So: webpki roots, verification always on. A corporate-CA story for the CLI
//! is a feature with its own decision and its own test, not a side effect of a
//! transport migration.

use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{Error, Result};

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The process-wide outbound TLS configuration. Only `http/1.1` is advertised
/// in ALPN — this client speaks HTTP/1.1 and nothing else, so a server that
/// could select h2 must not be allowed to.
pub fn client_config() -> Result<&'static turnloop_tls::ClientConfig> {
    static CONFIG: OnceLock<std::result::Result<turnloop_tls::ClientConfig, String>> =
        OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let options = turnloop_tls::ClientOptions {
                alpn: vec![b"http/1.1".to_vec()],
                ca: None,
                extra_ca_pem: Vec::new(),
                // Never configurable from this crate — see the module docs.
                reject_unauthorized: true,
                enable_sni: true,
                // `None` = turnloop-tls's default provider, `ring`.
                provider: None,
            };
            turnloop_tls::ClientConfig::new(options, unix_seconds()).map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(|e| Error::new(format!("TLS configuration: {e}")))
}

/// Fill `out` with cryptographically secure random bytes.
///
/// Used for the WebSocket `Sec-WebSocket-Key` nonce, which RFC 6455 §4.1
/// requires to be unpredictable — a guessable key lets an attacker who can
/// make the client issue a request convince a cache that the 101 response
/// belongs to an ordinary GET.
///
/// The source is rustls's own provider rather than a new `rand` dependency:
/// `ring`'s `SystemRandom` is already linked through `turnloop-tls`, so this
/// adds a call rather than a crate.
pub fn secure_random(out: &mut [u8]) -> Result<()> {
    use turnloop_tls::rustls::crypto::ring::default_provider;
    static PROVIDER: OnceLock<turnloop_tls::rustls::crypto::CryptoProvider> = OnceLock::new();
    let provider = PROVIDER.get_or_init(default_provider);
    provider
        .secure_random
        .fill(out)
        .map_err(|_| Error::new("no secure random source"))
}
