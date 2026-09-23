//! TLS option parsing and rustls `ClientConfig`/`TlsConnector` construction
//! for the bundled `net`/`tls` socket surface.
//!
//! Split out of `net/mod.rs` (2000-line file cap). Pure move — the
//! functions keep their names, signatures, `#[cfg(feature = "tls")]`
//! gating and behaviour; only the ones `net` still calls widened to
//! `pub(super)`.

use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};

use perry_runtime::{JSValue, StringHeader};

use super::tls_verifier::NodeConfiguredCaVerifier;
use super::value_helpers::{get_object_value_field, unbox_pointer};
use super::TlsClientConfigData;

/// What `build_tls_connector` produces: the client config a handshake runs
/// with (formerly wrapped in a `tokio_rustls::TlsConnector`).
type TlsConnector = Arc<rustls::ClientConfig>;

#[cfg(feature = "tls")]
unsafe fn tls_value_bytes(value: f64) -> Option<Vec<u8>> {
    let mut len = 0u32;
    let data = perry_runtime::buffer::js_value_buffer_or_typedarray_data(value, &mut len);
    if !data.is_null() {
        return Some(std::slice::from_raw_parts(data, len as usize).to_vec());
    }
    // `js_get_string_pointer_unified` deliberately returns the raw pointer for
    // any POINTER_TAG value. Probe Buffer/TypedArray values first so their
    // headers are never interpreted as StringHeaders.
    let string_ptr = perry_runtime::js_get_string_pointer_unified(value);
    (string_ptr != 0)
        .then(|| crate::common::string_from_header(string_ptr as *const StringHeader))
        .flatten()
        .map(String::into_bytes)
}

#[cfg(feature = "tls")]
unsafe fn tls_material_list(value: f64) -> Option<Vec<Vec<u8>>> {
    let js = JSValue::from_bits(value.to_bits());
    if js.is_undefined() || js.is_null() {
        return Some(Vec::new());
    }
    if JSValue::from_bits(perry_runtime::js_array_is_array(value).to_bits()).as_bool() {
        let array = unbox_pointer(value) as *const perry_runtime::ArrayHeader;
        let mut out = Vec::new();
        for index in 0..perry_runtime::js_array_length(array) {
            out.extend(tls_material_list(perry_runtime::array::js_array_get_f64(
                array, index,
            ))?);
        }
        return Some(out);
    }
    tls_value_bytes(value).map(|bytes| vec![bytes])
}

#[cfg(feature = "tls")]
unsafe fn tls_option_value(options: f64, secure_context: f64, name: &str) -> Option<f64> {
    get_object_value_field(options, name).and_then(|value| {
        if JSValue::from_bits(value.to_bits()).is_undefined() {
            get_object_value_field(secure_context, name)
        } else {
            Some(value)
        }
    })
}

#[cfg(feature = "tls")]
unsafe fn tls_parse_alpn(value: f64) -> Vec<Vec<u8>> {
    if JSValue::from_bits(perry_runtime::js_array_is_array(value).to_bits()).as_bool() {
        let array = unbox_pointer(value) as *const perry_runtime::ArrayHeader;
        return (0..perry_runtime::js_array_length(array))
            .filter_map(|index| {
                tls_value_bytes(perry_runtime::array::js_array_get_f64(array, index))
            })
            .collect();
    }
    let Some(encoded) = tls_value_bytes(value) else {
        return Vec::new();
    };
    let mut offset = 0usize;
    let mut out = Vec::new();
    while offset < encoded.len() {
        let len = encoded[offset] as usize;
        offset += 1;
        if len == 0 || offset + len > encoded.len() {
            break;
        }
        out.push(encoded[offset..offset + len].to_vec());
        offset += len;
    }
    out
}

#[cfg(feature = "tls")]
pub(super) unsafe fn tls_client_config_data(options: f64) -> TlsClientConfigData {
    let secure_context = get_object_value_field(options, "secureContext")
        .unwrap_or_else(|| f64::from_bits(0x7FFC_0000_0000_0001));
    let mut ca =
        tls_option_value(options, secure_context, "ca").and_then(|value| tls_material_list(value));
    if ca.is_none() && perry_runtime::tls::js_tls_default_ca_is_configured() != 0 {
        ca = tls_material_list(perry_runtime::tls::js_tls_get_ca_certificates(
            f64::from_bits(0x7FFC_0000_0000_0001),
        ));
    }
    TlsClientConfigData {
        ca,
        cert: tls_option_value(options, secure_context, "cert")
            .and_then(|value| tls_value_bytes(value))
            .unwrap_or_default(),
        key: tls_option_value(options, secure_context, "key")
            .and_then(|value| tls_value_bytes(value))
            .unwrap_or_default(),
        alpn_protocols: tls_option_value(options, secure_context, "ALPNProtocols")
            .map(|value| tls_parse_alpn(value))
            .unwrap_or_default(),
        version_mask: perry_runtime::tls::js_tls_effective_version_mask(options),
        custom_identity: tls_option_value(options, secure_context, "checkServerIdentity")
            .is_some_and(|value| {
                let js = JSValue::from_bits(value.to_bits());
                !js.is_undefined() && !js.is_null()
            }),
    }
}

#[cfg(feature = "tls")]
fn tls_protocol_versions(mask: i32) -> Vec<&'static rustls::SupportedProtocolVersion> {
    let mask = if mask == 0 { 0b11 } else { mask };
    let mut versions = Vec::new();
    if mask & 0b10 != 0 {
        versions.push(&rustls::version::TLS13);
    }
    if mask & 0b01 != 0 {
        versions.push(&rustls::version::TLS12);
    }
    versions
}

#[cfg(feature = "tls")]
pub(super) unsafe fn tls_signal_is_pre_aborted(options: f64) -> bool {
    let Some(signal) = get_object_value_field(options, "signal") else {
        return false;
    };
    let signal = perry_runtime::url::js_abort_signal_resolve_ptr(signal);
    if signal.is_null() {
        return false;
    }
    perry_runtime::url::js_abort_signal_is_aborted(signal) != 0
}

#[cfg(feature = "tls")]
pub(super) unsafe fn tls_preflight(port: u16, servername: &str, options: f64) -> i32 {
    crate::tls::js_tls_client_preflight(port as f64, servername.as_ptr(), servername.len(), options)
}

#[cfg(feature = "tls")]
pub(super) fn tls_preflight_error(code: i32) -> &'static str {
    match code {
        1 => "ERR_TLS_ALPN_CALLBACK_INVALID_RESULT",
        2 => "ERR_SSL_TLSV1_ALERT_NO_APPLICATION_PROTOCOL",
        3 => "ERR_TLS_SNI_CALLBACK_FAILED",
        _ => "ERR_TLS_HANDSHAKE_FAILED",
    }
}

// ─── rustls config (TLS feature only) ────────────────────────────────────────

#[cfg(feature = "tls")]
pub(super) fn build_tls_connector(
    verify: bool,
    data: Option<&TlsClientConfigData>,
) -> Result<TlsConnector, String> {
    // rustls panics resolving the process-level CryptoProvider when both
    // `ring` and `aws-lc-rs` end up in the dep graph. Server paths install
    // one before their first handshake; a client-only program (no tls/https
    // server) reached `ClientConfig::builder()` with none installed once
    // #4971 made `tls.connect` actually resolve its host. Idempotent —
    // `install_default` errors (ignored) if a provider is already set.
    let _ = rustls::crypto::ring::default_provider().install_default();
    if !verify {
        return build_tls_connector_insecure(data);
    }
    // System trust store. Aligns with Perry's broader rustls-only stance
    // (reqwest / tokio-tungstenite / mongodb all use rustls) — no OpenSSL.
    let mut root_store = rustls::RootCertStore::empty();
    // rustls-native-certs 0.8 returns a CertificateResult with separate
    // `.certs` and `.errors` fields; we accept per-cert failures rather
    // than bail, matching the crate's own documented pattern.
    if let Some(ca) = data.and_then(|data| data.ca.as_ref()) {
        add_pem_roots(&mut root_store, ca);
    } else {
        let native = rustls_native_certs::load_native_certs();
        for cert in native.certs {
            let _ = root_store.add(cert);
        }
    }
    let configured = configured_ca_certificates(data);
    let custom_identity = data.is_some_and(|data| data.custom_identity);
    let node_verifier = if configured.is_empty() && !custom_identity {
        None
    } else {
        Some(NodeConfiguredCaVerifier {
            inner: rustls::client::WebPkiServerVerifier::builder(Arc::new(root_store.clone()))
                .build()
                .map_err(|error| format!("tls certificate verifier: {error}"))?,
            roots: root_store.clone(),
            configured,
            custom_identity,
        })
    };
    let versions = tls_protocol_versions(data.map_or(0b11, |data| data.version_mask));
    let builder = rustls::ClientConfig::builder_with_provider(
        rustls::crypto::ring::default_provider().into(),
    )
    .with_protocol_versions(&versions)
    .map_err(|error| format!("tls protocol versions: {error}"))?
    .with_root_certificates(root_store);
    let mut config = if let Some((certs, key)) = data.and_then(client_auth_material) {
        builder
            .with_client_auth_cert(certs, key)
            .map_err(|error| format!("tls client certificate: {error}"))?
    } else {
        builder.with_no_client_auth()
    };
    if let Some(data) = data {
        config.alpn_protocols = data.alpn_protocols.clone();
    }
    if let Some(verifier) = node_verifier {
        config
            .dangerous()
            .set_certificate_verifier(Arc::new(verifier));
    }
    Ok(Arc::new(config))
}

#[cfg(feature = "tls")]
fn add_pem_roots(store: &mut rustls::RootCertStore, materials: &[Vec<u8>]) {
    for material in materials {
        let mut cursor = std::io::Cursor::new(material);
        for cert in rustls_pemfile::certs(&mut cursor).flatten() {
            let _ = store.add(cert);
        }
    }
}

#[cfg(feature = "tls")]
fn configured_ca_certificates(data: Option<&TlsClientConfigData>) -> Vec<Vec<u8>> {
    data.and_then(|data| data.ca.as_ref())
        .into_iter()
        .flatten()
        .flat_map(|material| {
            let mut cursor = std::io::Cursor::new(material);
            rustls_pemfile::certs(&mut cursor)
                .flatten()
                .map(|cert| cert.as_ref().to_vec())
                .collect::<Vec<_>>()
        })
        .collect()
}

#[cfg(feature = "tls")]
fn client_auth_material(
    data: &TlsClientConfigData,
) -> Option<(
    Vec<rustls::pki_types::CertificateDer<'static>>,
    rustls::pki_types::PrivateKeyDer<'static>,
)> {
    let mut cert_cursor = std::io::Cursor::new(&data.cert);
    let certs: Vec<_> = rustls_pemfile::certs(&mut cert_cursor).flatten().collect();
    if certs.is_empty() {
        return None;
    }
    let mut key_cursor = std::io::Cursor::new(&data.key);
    let key = rustls_pemfile::private_key(&mut key_cursor)
        .ok()
        .flatten()?;
    Some((certs, key))
}

/// Insecure TLS — accept any server cert without verifying chain or hostname.
/// Maps to Postgres `sslmode=require` (encryption without auth) and is the
/// right default for local dev against self-signed certs. Real deployments
/// should pass `verify: true` (the default) so the system trust store and
/// hostname validation apply.
#[cfg(feature = "tls")]
fn build_tls_connector_insecure(
    data: Option<&TlsClientConfigData>,
) -> Result<TlsConnector, String> {
    use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
    use rustls::{DigitallySignedStruct, SignatureScheme};

    #[derive(Debug)]
    struct NoVerify;

    impl ServerCertVerifier for NoVerify {
        fn verify_server_cert(
            &self,
            _end_entity: &CertificateDer<'_>,
            _intermediates: &[CertificateDer<'_>],
            _server_name: &ServerName<'_>,
            _ocsp: &[u8],
            _now: UnixTime,
        ) -> Result<ServerCertVerified, rustls::Error> {
            Ok(ServerCertVerified::assertion())
        }
        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer<'_>,
            _dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }
        fn verify_tls13_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer<'_>,
            _dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }
        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            vec![
                SignatureScheme::RSA_PKCS1_SHA256,
                SignatureScheme::RSA_PKCS1_SHA384,
                SignatureScheme::RSA_PKCS1_SHA512,
                SignatureScheme::ECDSA_NISTP256_SHA256,
                SignatureScheme::ECDSA_NISTP384_SHA384,
                SignatureScheme::RSA_PSS_SHA256,
                SignatureScheme::RSA_PSS_SHA384,
                SignatureScheme::RSA_PSS_SHA512,
                SignatureScheme::ED25519,
            ]
        }
    }

    let versions = tls_protocol_versions(data.map_or(0b11, |data| data.version_mask));
    let builder = rustls::ClientConfig::builder_with_provider(
        rustls::crypto::ring::default_provider().into(),
    )
    .with_protocol_versions(&versions)
    .map_err(|error| format!("tls protocol versions: {error}"))?
    .dangerous()
    .with_custom_certificate_verifier(Arc::new(NoVerify));
    let mut config = if let Some((certs, key)) = data.and_then(client_auth_material) {
        builder
            .with_client_auth_cert(certs, key)
            .map_err(|error| format!("tls client certificate: {error}"))?
    } else {
        builder.with_no_client_auth()
    };
    if let Some(data) = data {
        config.alpn_protocols = data.alpn_protocols.clone();
    }
    Ok(Arc::new(config))
}
