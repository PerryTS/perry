//! Phase 2 TLS scaffolding — `https.createServer({ key, cert }, ...)`
//! reads PEM-encoded key/cert pairs and builds a `rustls::ServerConfig`.
//! See `https_server::js_node_https_create_server`.

use std::sync::Arc;

use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::ServerConfig;

/// Decode a `key`/`cert`-shaped JSON value into the PEM bytes the
/// rustls parsers expect.
///
/// Node lets users pass either a PEM string OR a `Buffer` (the form
/// `fs.readFileSync('key.pem')` returns when no encoding is supplied).
/// `https.createServer` / `http2.createSecureServer` decode their
/// options object via `JSON.stringify` → `serde_json`, which
/// round-trips a `Buffer` as `{ "type": "Buffer", "data": [..] }`.
/// Without this helper, the `.as_str()` extraction silently yielded
/// an empty string for Buffer-typed PEMs and the user saw a
/// `"no recognized PEM private key"` error even with valid input
/// (#2132).
pub fn json_value_to_pem_bytes(v: Option<&serde_json::Value>) -> Vec<u8> {
    let Some(v) = v else { return Vec::new() };
    if let Some(s) = v.as_str() {
        return s.as_bytes().to_vec();
    }
    if let Some(obj) = v.as_object() {
        if obj.get("type").and_then(|t| t.as_str()) == Some("Buffer") {
            if let Some(arr) = obj.get("data").and_then(|d| d.as_array()) {
                return arr
                    .iter()
                    .filter_map(|n| n.as_u64().map(|u| u as u8))
                    .collect();
            }
        }
    }
    if let Some(arr) = v.as_array() {
        return arr
            .iter()
            .filter_map(|n| n.as_u64().map(|u| u as u8))
            .collect();
    }
    Vec::new()
}

/// True when a secure-server options object supplied key/cert material worth
/// parsing. Empty options, omitted fields, and explicitly empty strings all
/// construct quietly in Node; errors surface later when the server is used.
pub fn has_pem_material(key_pem: &[u8], cert_pem: &[u8]) -> bool {
    !key_pem.is_empty() || !cert_pem.is_empty()
}

/// Parse PEM-encoded certificate chain bytes into rustls
/// `CertificateDer`s. Returns an empty vec on parse failure (caller
/// must check for emptiness before building a ServerConfig — empty
/// cert chains fail at TLS-handshake time anyway).
pub fn parse_cert_chain(pem_bytes: &[u8]) -> Vec<CertificateDer<'static>> {
    let mut cursor = std::io::Cursor::new(pem_bytes);
    rustls_pemfile::certs(&mut cursor)
        .filter_map(|c| c.ok())
        .collect()
}

/// Parse a PEM-encoded private key (PKCS#8, RSA, or EC). Returns
/// `None` if the input doesn't yield a recognized key form.
pub fn parse_private_key(pem_bytes: &[u8]) -> Option<PrivateKeyDer<'static>> {
    let mut cursor = std::io::Cursor::new(pem_bytes);
    if let Some(Ok(k)) = rustls_pemfile::pkcs8_private_keys(&mut cursor).next() {
        return Some(PrivateKeyDer::Pkcs8(k));
    }
    let mut cursor = std::io::Cursor::new(pem_bytes);
    if let Some(Ok(k)) = rustls_pemfile::rsa_private_keys(&mut cursor).next() {
        return Some(PrivateKeyDer::Pkcs1(k));
    }
    let mut cursor = std::io::Cursor::new(pem_bytes);
    if let Some(Ok(k)) = rustls_pemfile::ec_private_keys(&mut cursor).next() {
        return Some(PrivateKeyDer::Sec1(k));
    }
    None
}

/// rustls 0.23 requires explicit selection of a CryptoProvider when
/// multiple providers are linked into the binary (perry transitively
/// pulls in both `ring` via our direct dep and `aws-lc-rs` via
/// reqwest's rustls-tls feature). Without an explicit install,
/// `ServerConfig::builder()` panics with "Could not automatically
/// determine the process-level CryptoProvider". Idempotent — the
/// `Once` makes repeated calls safe across multiple createServer
/// invocations within a single process.
fn ensure_crypto_provider_installed() {
    use std::sync::Once;
    static INSTALLED: Once = Once::new();
    INSTALLED.call_once(|| {
        // Best-effort install. If a provider was already installed
        // by another crate (or by user code), `install_default()`
        // returns Err; we ignore it because in that case the
        // existing provider is already usable.
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

#[cfg(test)]
mod tests {
    use super::json_value_to_pem_bytes;
    use serde_json::json;

    #[test]
    fn string_value_returns_utf8_bytes() {
        let v = json!("-----BEGIN RSA PRIVATE KEY-----\n");
        assert_eq!(
            json_value_to_pem_bytes(Some(&v)),
            b"-----BEGIN RSA PRIVATE KEY-----\n"
        );
    }

    #[test]
    fn node_buffer_shape_returns_data_bytes() {
        // `JSON.stringify(Buffer.from("hi"))` → `{"type":"Buffer","data":[104,105]}`.
        let v = json!({"type":"Buffer","data":[104,105]});
        assert_eq!(json_value_to_pem_bytes(Some(&v)), b"hi");
    }

    #[test]
    fn plain_numeric_array_returns_bytes() {
        let v = json!([104, 105]);
        assert_eq!(json_value_to_pem_bytes(Some(&v)), b"hi");
    }

    #[test]
    fn none_and_unknown_shapes_return_empty() {
        assert!(json_value_to_pem_bytes(None).is_empty());
        assert!(json_value_to_pem_bytes(Some(&json!(42))).is_empty());
        assert!(json_value_to_pem_bytes(Some(&json!({"foo": "bar"}))).is_empty());
    }

    #[test]
    fn pem_material_detection_matches_empty_options_behavior() {
        assert!(!super::has_pem_material(b"", b""));
        assert!(super::has_pem_material(b"not pem", b""));
        assert!(super::has_pem_material(b"", b"not cert"));
    }
}

/// Build a rustls `ServerConfig` ready for `tokio_rustls::TlsAcceptor`.
/// `alpn_protocols` is set to `[h2, http/1.1]` so an HTTP/2-aware
/// negotiator can pick the upgraded transport on the same port —
/// hooks into the Phase 3 ALPN handoff.
pub fn build_server_config(
    cert_chain: Vec<CertificateDer<'static>>,
    private_key: PrivateKeyDer<'static>,
    enable_http2: bool,
) -> Result<Arc<ServerConfig>, String> {
    if cert_chain.is_empty() {
        return Err("https.createServer: empty certificate chain".to_string());
    }
    ensure_crypto_provider_installed();
    let mut config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, private_key)
        .map_err(|e| format!("rustls: build server config: {}", e))?;
    if enable_http2 {
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    } else {
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
    }
    Ok(Arc::new(config))
}
