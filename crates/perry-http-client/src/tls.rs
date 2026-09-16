//! The client's TLS configuration, and the secure random the WebSocket
//! handshake needs.
//!
//! Perry's runtime builds its `ClientConfig` from Node's TLS environment
//! through `perry_ffi` (`perry_stdlib::turnloop_tls_client::client_config`).
//! This crate has no `perry_ffi` and no JS, so it reads the same two variables
//! directly. They are the ones a CI job actually sets:
//!
//! * `NODE_EXTRA_CA_CERTS` / `SSL_CERT_FILE` — a PEM bundle added to the
//!   Mozilla roots, which is how a corporate MITM proxy is trusted;
//! * `NODE_TLS_REJECT_UNAUTHORIZED=0` — verification off, honoured because
//!   `reqwest` honoured it here before this crate replaced it and a build box
//!   behind a self-signed proxy would otherwise stop being able to publish.
//!
//! The configuration is built once per process and shared, so the rustls
//! session cache is shared too.

use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{Error, Result};

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Read a PEM bundle named by the environment, if one is named and readable.
///
/// An unreadable path is ignored rather than fatal: that is what Node does
/// with `NODE_EXTRA_CA_CERTS`, and failing the whole command because a stale
/// variable points at a deleted file would be worse than using the defaults.
fn extra_ca_pem() -> Vec<u8> {
    let mut pem = Vec::new();
    for key in ["NODE_EXTRA_CA_CERTS", "SSL_CERT_FILE"] {
        let Ok(path) = std::env::var(key) else {
            continue;
        };
        if path.is_empty() {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        pem.extend_from_slice(&bytes);
        if !pem.ends_with(b"\n") {
            pem.push(b'\n');
        }
    }
    pem
}

fn reject_unauthorized() -> bool {
    !matches!(
        std::env::var("NODE_TLS_REJECT_UNAUTHORIZED").as_deref(),
        Ok("0")
    )
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
                extra_ca_pem: extra_ca_pem(),
                reject_unauthorized: reject_unauthorized(),
                enable_sni: true,
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
