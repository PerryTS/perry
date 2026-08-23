//! Phase 2 TLS scaffolding — `https.createServer({ key, cert }, ...)`
//! reads PEM-encoded key/cert pairs and builds a `rustls::ServerConfig`.
//! See `https_server::js_node_https_create_server`.

use std::fmt;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex, RwLock};

use ring::aead::{self, Aad, LessSafeKey, Nonce, UnboundKey};
use ring::digest::{digest, SHA256};
use ring::rand::{SecureRandom, SystemRandom};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::ProducesTickets;
use rustls::{KeyLog, ServerConfig};

/// Per-connection rustls key-log sink. The TLS worker records NSS-format
/// lines here and the main-thread HTTP pump drains them into Node's `keylog`
/// event without invoking JS from a worker thread.
#[derive(Debug, Default)]
pub struct ConnectionKeyLog {
    lines: Mutex<Vec<Vec<u8>>>,
}

impl ConnectionKeyLog {
    pub fn drain(&self) -> Vec<Vec<u8>> {
        self.lines
            .lock()
            .map(|mut lines| lines.drain(..).collect())
            .unwrap_or_default()
    }
}

impl KeyLog for ConnectionKeyLog {
    fn log(&self, label: &str, client_random: &[u8], secret: &[u8]) {
        fn append_hex(out: &mut String, bytes: &[u8]) {
            for byte in bytes {
                let _ = write!(out, "{byte:02x}");
            }
        }

        let mut line =
            String::with_capacity(label.len() + 2 + (client_random.len() + secret.len()) * 2 + 1);
        line.push_str(label);
        line.push(' ');
        append_hex(&mut line, client_random);
        line.push(' ');
        append_hex(&mut line, secret);
        line.push('\n');
        if let Ok(mut lines) = self.lines.lock() {
            lines.push(line.into_bytes());
        }
    }
}

struct TicketCipher {
    key_name: [u8; 16],
    key: LessSafeKey,
}

impl fmt::Debug for TicketCipher {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TicketCipher")
            .finish_non_exhaustive()
    }
}

/// Mutable server-scoped session-ticket provider backing
/// `server.setTicketKeys()`. Node's public 48-byte key blob is treated as key
/// material for a stable AEAD ticket key; replacement affects new handshakes
/// on this server only and never clears unrelated client Agent caches.
pub struct NodeTicketKey {
    cipher: RwLock<TicketCipher>,
}

impl fmt::Debug for NodeTicketKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NodeTicketKey")
            .finish_non_exhaustive()
    }
}

impl NodeTicketKey {
    pub fn random() -> Result<Arc<Self>, String> {
        let mut keys = [0_u8; 48];
        SystemRandom::new()
            .fill(&mut keys)
            .map_err(|_| "rustls: unable to generate session ticket keys".to_string())?;
        Self::from_keys(&keys).map(Arc::new)
    }

    fn from_keys(keys: &[u8; 48]) -> Result<Self, String> {
        let mut key_name = [0_u8; 16];
        key_name.copy_from_slice(&keys[..16]);
        let material = digest(&SHA256, keys);
        let key = UnboundKey::new(&aead::CHACHA20_POLY1305, material.as_ref())
            .map_err(|_| "rustls: invalid session ticket key material".to_string())?;
        Ok(Self {
            cipher: RwLock::new(TicketCipher {
                key_name,
                key: LessSafeKey::new(key),
            }),
        })
    }

    pub fn set_keys(&self, keys: &[u8; 48]) -> Result<(), String> {
        let replacement = Self::from_keys(keys)?;
        let replacement = replacement
            .cipher
            .into_inner()
            .map_err(|_| "rustls: session ticket key lock poisoned".to_string())?;
        *self
            .cipher
            .write()
            .map_err(|_| "rustls: session ticket key lock poisoned".to_string())? = replacement;
        Ok(())
    }
}

impl ProducesTickets for NodeTicketKey {
    fn enabled(&self) -> bool {
        true
    }

    fn lifetime(&self) -> u32 {
        12 * 60 * 60
    }

    fn encrypt(&self, plain: &[u8]) -> Option<Vec<u8>> {
        let cipher = self.cipher.read().ok()?;
        let mut nonce_bytes = [0_u8; 12];
        SystemRandom::new().fill(&mut nonce_bytes).ok()?;
        let nonce = Nonce::assume_unique_for_key(nonce_bytes);
        let mut sealed = plain.to_vec();
        cipher
            .key
            .seal_in_place_append_tag(nonce, Aad::from(&cipher.key_name), &mut sealed)
            .ok()?;
        let mut ticket = Vec::with_capacity(16 + 12 + sealed.len());
        ticket.extend_from_slice(&cipher.key_name);
        ticket.extend_from_slice(&nonce_bytes);
        ticket.extend_from_slice(&sealed);
        Some(ticket)
    }

    fn decrypt(&self, ticket: &[u8]) -> Option<Vec<u8>> {
        if ticket.len() < 16 + 12 + aead::CHACHA20_POLY1305.tag_len() {
            return None;
        }
        let cipher = self.cipher.read().ok()?;
        // The ticket key name is sent in cleartext and is not secret, so an
        // ordinary equality check is sufficient here.
        if ticket[..16] != cipher.key_name {
            return None;
        }
        let nonce_bytes: [u8; 12] = ticket[16..28].try_into().ok()?;
        let nonce = Nonce::assume_unique_for_key(nonce_bytes);
        let mut sealed = ticket[28..].to_vec();
        let plain = cipher
            .key
            .open_in_place(nonce, Aad::from(&cipher.key_name), &mut sealed)
            .ok()?;
        Some(plain.to_vec())
    }
}

pub fn install_ticket_key(config: &mut Arc<ServerConfig>, ticket_key: Arc<NodeTicketKey>) {
    Arc::get_mut(config)
        .expect("newly built ServerConfig must be uniquely owned")
        .ticketer = ticket_key;
}

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

/// Build a `ServerConfig` for an `https.Server` constructed without
/// key/cert material — `https.createServer()` with empty (or omitted)
/// options. Node constructs and `listen()`s such a server fine; the
/// missing credentials only surface per-connection, as a TLS alert
/// during the handshake. The always-`None` cert resolver reproduces
/// that: rustls accepts the TCP connection, then aborts the handshake
/// with a fatal alert when no certificate resolves (#4974).
pub fn build_certless_server_config(enable_http2: bool) -> Arc<ServerConfig> {
    let provider = crypto_provider();

    #[derive(Debug)]
    struct NoCert;
    impl rustls::server::ResolvesServerCert for NoCert {
        fn resolve(
            &self,
            _client_hello: rustls::server::ClientHello<'_>,
        ) -> Option<Arc<rustls::sign::CertifiedKey>> {
            None
        }
    }

    let mut config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("ring provider must support rustls default protocol versions")
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(NoCert));
    if enable_http2 {
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    } else {
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
    }
    Arc::new(config)
}

/// rustls 0.23 requires explicit selection of a CryptoProvider when
/// multiple providers are linked into the binary (perry transitively
/// pulls in both `ring` via our direct dep and `aws-lc-rs` via
/// reqwest's rustls-tls feature). Without an explicit install,
/// `ServerConfig::builder()` panics with "Could not automatically
/// determine the process-level CryptoProvider". Keep one explicit ring
/// provider so key loading and every server builder use the same backend.
fn crypto_provider() -> Arc<rustls::crypto::CryptoProvider> {
    use std::sync::OnceLock;
    static PROVIDER: OnceLock<Arc<rustls::crypto::CryptoProvider>> = OnceLock::new();
    PROVIDER
        .get_or_init(|| {
            let provider = rustls::crypto::ring::default_provider();
            let _ = provider.clone().install_default();
            Arc::new(provider)
        })
        .clone()
}

#[cfg(test)]
mod tests {
    use super::{json_value_to_pem_bytes, ConnectionKeyLog, NodeTicketKey};
    use rustls::{server::ProducesTickets, KeyLog};
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

    #[test]
    fn keylog_records_are_nss_formatted_and_drained_once() {
        let log = ConnectionKeyLog::default();
        log.log("CLIENT_RANDOM", &[0xab, 0xcd], &[0x01, 0x23]);
        assert_eq!(log.drain(), vec![b"CLIENT_RANDOM abcd 0123\n".to_vec()]);
        assert!(log.drain().is_empty());
    }

    #[test]
    fn rotating_ticket_keys_invalidates_only_old_tickets() {
        let initial = [0x11; 48];
        let rotated = [0x22; 48];
        let ticket_key = NodeTicketKey::from_keys(&initial).expect("initial ticket key");
        let old_ticket = ticket_key.encrypt(b"session").expect("encrypted ticket");
        assert_eq!(
            ticket_key.decrypt(&old_ticket).as_deref(),
            Some(b"session".as_slice())
        );

        ticket_key.set_keys(&rotated).expect("rotate ticket key");
        assert!(ticket_key.decrypt(&old_ticket).is_none());
        let new_ticket = ticket_key.encrypt(b"new session").expect("new ticket");
        assert_eq!(
            ticket_key.decrypt(&new_ticket).as_deref(),
            Some(b"new session".as_slice())
        );
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
    let provider = crypto_provider();

    // #4906: don't route through `ServerConfig::with_single_cert` — it
    // parses the leaf with webpki, which rejects the X.509 **v1** certs in
    // Node's `test/fixtures/keys` (`agent2`/`agent3`) outright
    // (`UnsupportedCertVersion`). Node serves whatever cert/key pair the
    // user supplies without re-validating the leaf, so we mirror that by
    // loading the signing key directly and installing a fixed-cert
    // resolver. The client is the party that validates the served cert.
    let signing_key = provider
        .key_provider
        .load_private_key(private_key)
        .map_err(|e| format!("rustls: build server config: {}", e))?;
    let certified_key = Arc::new(rustls::sign::CertifiedKey::new(cert_chain, signing_key));

    #[derive(Debug)]
    struct FixedCert(Arc<rustls::sign::CertifiedKey>);
    impl rustls::server::ResolvesServerCert for FixedCert {
        fn resolve(
            &self,
            _client_hello: rustls::server::ClientHello<'_>,
        ) -> Option<Arc<rustls::sign::CertifiedKey>> {
            Some(self.0.clone())
        }
    }

    let mut config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("rustls: build server config: {}", e))?
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(FixedCert(certified_key)));
    if enable_http2 {
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    } else {
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
    }
    Ok(Arc::new(config))
}
