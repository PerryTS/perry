//! rustls client config + handshake for `tls.connect` and the
//! `socket.upgradeToTLS` mid-stream upgrade. Split out of `lib.rs` (#1852)
//! to keep that file under the 2000-line gate; the logic is unchanged.

use std::sync::{Arc, Mutex, OnceLock};

use perry_ffi::{js_array_get, js_array_length, ArrayHeader, JsValue};
use tokio::net::TcpStream;
use tokio_rustls::rustls::client::danger::{
    HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier,
};
use tokio_rustls::{client::TlsStream, rustls, TlsConnector};

#[derive(Clone, Default)]
pub(crate) struct TlsClientConfigData {
    ca: Option<Vec<Vec<u8>>>,
    cert: Vec<u8>,
    key: Vec<u8>,
    alpn_protocols: Vec<Vec<u8>>,
    version_mask: i32,
    custom_identity: bool,
}

fn pending_tls_aborts() -> &'static Mutex<std::collections::HashSet<i64>> {
    static ABORTS: OnceLock<Mutex<std::collections::HashSet<i64>>> = OnceLock::new();
    ABORTS.get_or_init(|| Mutex::new(std::collections::HashSet::new()))
}

pub(crate) fn fire_pending_tls_abort(handle: i64) {
    if pending_tls_aborts().lock().unwrap().remove(&handle) {
        crate::push_event(crate::PendingNetEvent::AbortError(handle));
        crate::push_event(crate::PendingNetEvent::Close(handle));
    }
}

/// Node reports an already-aborted connect asynchronously, after callers have
/// had a chance to attach `error` and `close` listeners to the returned socket.
unsafe fn schedule_tls_abort(handle: i64) {
    // An alloc-only SocketState normally has `pending_rx: Some` and is not
    // considered live by ext-net until connect() consumes that receiver. This
    // TLS fast path never starts connect(), so mark the synthetic socket live
    // until its deferred Close event removes it from the registry.
    if let Some(socket) = crate::statics::sockets().lock().unwrap().get_mut(&handle) {
        socket.is_open = true;
    }
    pending_tls_aborts().lock().unwrap().insert(handle);
    perry_ffi::spawn_async(async move {
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        fire_pending_tls_abort(handle);
    });
}

pub(crate) fn begin_tls_upgrade(
    handle: i64,
    servername: String,
    verify: bool,
    config: TlsClientConfigData,
) -> Result<(), String> {
    let cmd_tx = crate::statics::sockets()
        .lock()
        .unwrap()
        .get(&handle)
        .map(|socket| socket.cmd_tx.clone())
        .ok_or_else(|| "socket is closed".to_string())?;
    let (reply, _reply_rx) = tokio::sync::oneshot::channel();
    cmd_tx
        .send(crate::SocketCommand::UpgradeTls {
            servername,
            verify,
            config,
            reply,
        })
        .map_err(|_| "socket task is gone".to_string())
}

unsafe fn is_array(value: f64) -> bool {
    extern "C" {
        fn js_array_is_array(value: f64) -> f64;
    }
    JsValue::from_bits(js_array_is_array(value).to_bits()).to_bool()
}

unsafe fn value_bytes(value: f64) -> Option<Vec<u8>> {
    let js = JsValue::from_bits(value.to_bits());
    if js.is_any_string() {
        return crate::jsvalue_to_socket_bytes(value);
    }
    // Read through the canonical runtime registry.  `perry-ext-net` is a
    // separately linked archive, so perry-ffi's local Buffer registry cannot
    // see Buffers allocated by the program runtime (notably `ca`, `cert`, and
    // `key` values returned by fs.readFileSync).
    extern "C" {
        fn js_value_buffer_or_typedarray_data(value: f64, out_len: *mut u32) -> *const u8;
    }
    let mut len = 0u32;
    let data = js_value_buffer_or_typedarray_data(value, &mut len);
    if data.is_null() {
        None
    } else {
        Some(std::slice::from_raw_parts(data, len as usize).to_vec())
    }
}

unsafe fn material_list(value: f64) -> Option<Vec<Vec<u8>>> {
    let js = JsValue::from_bits(value.to_bits());
    if js.is_undefined() || js.is_null() {
        return Some(Vec::new());
    }
    if is_array(value) {
        let array = crate::unbox_pointer(value) as *const ArrayHeader;
        let mut out = Vec::new();
        for index in 0..js_array_length(array) {
            out.extend(material_list(f64::from_bits(
                js_array_get(array, index).bits(),
            ))?);
        }
        return Some(out);
    }
    value_bytes(value).map(|bytes| vec![bytes])
}

unsafe fn option_value(options: f64, secure_context: f64, name: &str) -> Option<f64> {
    crate::get_object_value_field(options, name).and_then(|value| {
        let js = JsValue::from_bits(value.to_bits());
        if js.is_undefined() {
            crate::get_object_value_field(secure_context, name)
        } else {
            Some(value)
        }
    })
}

unsafe fn parse_alpn(value: f64) -> Vec<Vec<u8>> {
    if is_array(value) {
        let array = crate::unbox_pointer(value) as *const ArrayHeader;
        return (0..js_array_length(array))
            .filter_map(|index| {
                let item = f64::from_bits(js_array_get(array, index).bits());
                crate::jsvalue_to_socket_bytes(item)
            })
            .collect();
    }
    let Some(encoded) = value_bytes(value) else {
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

unsafe fn tls_client_config_data(options: f64) -> TlsClientConfigData {
    let secure_context = crate::get_object_value_field(options, "secureContext")
        .unwrap_or_else(|| f64::from_bits(JsValue::UNDEFINED.bits()));
    let mut ca = option_value(options, secure_context, "ca").and_then(|value| material_list(value));
    if ca.is_none() {
        extern "C" {
            fn js_tls_default_ca_is_configured() -> i32;
            fn js_tls_get_ca_certificates(ca_type: f64) -> f64;
        }
        if js_tls_default_ca_is_configured() != 0 {
            ca = material_list(js_tls_get_ca_certificates(f64::from_bits(
                JsValue::UNDEFINED.bits(),
            )));
        }
    }
    let cert = option_value(options, secure_context, "cert")
        .and_then(|value| value_bytes(value))
        .unwrap_or_default();
    let key = option_value(options, secure_context, "key")
        .and_then(|value| value_bytes(value))
        .unwrap_or_default();
    let alpn_protocols = option_value(options, secure_context, "ALPNProtocols")
        .map(|value| parse_alpn(value))
        .unwrap_or_default();
    extern "C" {
        fn js_tls_effective_version_mask(options: f64) -> i32;
    }
    TlsClientConfigData {
        ca,
        cert,
        key,
        alpn_protocols,
        version_mask: js_tls_effective_version_mask(options),
        custom_identity: option_value(options, secure_context, "checkServerIdentity").is_some_and(
            |value| {
                let js = JsValue::from_bits(value.to_bits());
                !js.is_undefined() && !js.is_null()
            },
        ),
    }
}

fn protocol_versions(mask: i32) -> Vec<&'static rustls::SupportedProtocolVersion> {
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

unsafe fn signal_is_pre_aborted(options: f64) -> bool {
    let Some(signal) = crate::get_object_value_field(options, "signal") else {
        return false;
    };
    extern "C" {
        fn js_abort_signal_resolve_ptr(value: f64) -> *mut u8;
        fn js_abort_signal_is_aborted(signal: *mut u8) -> i32;
    }
    let signal = js_abort_signal_resolve_ptr(signal);
    !signal.is_null() && js_abort_signal_is_aborted(signal) != 0
}

unsafe fn tls_preflight(port: u16, servername: &str, options: f64) -> i32 {
    extern "C" {
        fn js_tls_client_preflight(
            port: f64,
            servername_ptr: *const u8,
            servername_len: usize,
            options: f64,
        ) -> i32;
    }
    js_tls_client_preflight(port as f64, servername.as_ptr(), servername.len(), options)
}

fn preflight_error(code: i32) -> &'static str {
    match code {
        1 => "ERR_TLS_ALPN_CALLBACK_INVALID_RESULT",
        2 => "ERR_SSL_TLSV1_ALERT_NO_APPLICATION_PROTOCOL",
        3 => "ERR_TLS_SNI_CALLBACK_FAILED",
        _ => "ERR_TLS_HANDSHAKE_FAILED",
    }
}

fn add_pem_roots(store: &mut rustls::RootCertStore, materials: &[Vec<u8>]) {
    for material in materials {
        let mut cursor = std::io::Cursor::new(material);
        for cert in rustls_pemfile::certs(&mut cursor).flatten() {
            let _ = store.add(cert);
        }
    }
}

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

/// Node accepts an explicitly trusted self-signed certificate as a server
/// leaf even when its BasicConstraints extension also marks it as a CA.
/// rustls-webpki rejects that narrow shape as `CaUsedAsEndEntity`. Delegate
/// every normal check to rustls and recover only when the presented leaf is
/// byte-for-byte one of the configured CA certificates, retaining hostname
/// validation and rustls's TLS handshake-signature checks.
#[derive(Debug)]
struct NodeConfiguredCaVerifier {
    inner: Arc<rustls::client::WebPkiServerVerifier>,
    roots: rustls::RootCertStore,
    configured: Vec<Vec<u8>>,
    custom_identity: bool,
}

fn is_ca_used_as_end_entity(error: &rustls::Error) -> bool {
    let rustls::Error::InvalidCertificate(rustls::CertificateError::Other(other)) = error else {
        return false;
    };
    other.0.to_string() == "CaUsedAsEndEntity"
}

impl ServerCertVerifier for NodeConfiguredCaVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        intermediates: &[rustls::pki_types::CertificateDer<'_>],
        server_name: &rustls::pki_types::ServerName<'_>,
        ocsp_response: &[u8],
        now: rustls::pki_types::UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if self.custom_identity {
            let parsed = rustls::server::ParsedCertificate::try_from(end_entity)?;
            let provider = rustls::crypto::aws_lc_rs::default_provider();
            match rustls::client::verify_server_cert_signed_by_trust_anchor(
                &parsed,
                &self.roots,
                intermediates,
                now,
                provider.signature_verification_algorithms.all,
            ) {
                Ok(()) => return Ok(ServerCertVerified::assertion()),
                Err(error)
                    if is_ca_used_as_end_entity(&error)
                        && self
                            .configured
                            .iter()
                            .any(|cert| cert.as_slice() == end_entity.as_ref()) =>
                {
                    return Ok(ServerCertVerified::assertion());
                }
                Err(error) => return Err(error),
            }
        }
        match self.inner.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        ) {
            Err(error)
                if is_ca_used_as_end_entity(&error)
                    && self
                        .configured
                        .iter()
                        .any(|cert| cert.as_slice() == end_entity.as_ref()) =>
            {
                let parsed = rustls::server::ParsedCertificate::try_from(end_entity)?;
                rustls::client::verify_server_name(&parsed, server_name)?;
                Ok(ServerCertVerified::assertion())
            }
            result => result,
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

fn build_tls_connector(
    verify: bool,
    data: Option<&TlsClientConfigData>,
) -> Result<TlsConnector, String> {
    // rustls panics resolving the process-level CryptoProvider when both
    // `ring` and `aws-lc-rs` end up in the dep graph. Server paths install
    // one before their first handshake; a client-only program (no tls/https
    // server) reached `ClientConfig::builder()` with none installed once
    // #4971 made `tls.connect` actually resolve its host. Idempotent —
    // `install_default` errors (ignored) if a provider is already set.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    if !verify {
        return build_tls_connector_insecure(data);
    }
    let mut root_store = rustls::RootCertStore::empty();
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
    let versions = protocol_versions(data.map_or(0b11, |data| data.version_mask));
    let builder = rustls::ClientConfig::builder_with_provider(
        rustls::crypto::aws_lc_rs::default_provider().into(),
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
    Ok(TlsConnector::from(Arc::new(config)))
}

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

    let versions = protocol_versions(data.map_or(0b11, |data| data.version_mask));
    let builder = rustls::ClientConfig::builder_with_provider(
        rustls::crypto::aws_lc_rs::default_provider().into(),
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
    Ok(TlsConnector::from(Arc::new(config)))
}

pub(crate) async fn do_tls_handshake(
    tcp: TcpStream,
    servername: &str,
    verify: bool,
    data: Option<&TlsClientConfigData>,
) -> Result<TlsStream<TcpStream>, String> {
    let connector = if verify {
        build_tls_connector(true, data)?
    } else {
        build_tls_connector_insecure(data)?
    };
    let server_name = rustls::pki_types::ServerName::try_from(servername.to_string())
        .map_err(|e| format!("invalid servername '{}': {}", servername, e))?;
    connector
        .connect(server_name, tcp)
        .await
        .map_err(|e| format!("tls handshake: {}", e))
}

pub(crate) fn record_tls_handshake(
    handle: i64,
    stream: &TlsStream<TcpStream>,
    servername: &str,
    verify: bool,
    data: Option<&TlsClientConfigData>,
) {
    let connection = stream.get_ref().1;
    let protocol = match connection.protocol_version() {
        Some(rustls::ProtocolVersion::TLSv1_2) => "TLSv1.2",
        Some(rustls::ProtocolVersion::TLSv1_3) => "TLSv1.3",
        _ => "",
    };
    let alpn = connection.alpn_protocol().unwrap_or_default();
    let peer = connection
        .peer_certificates()
        .and_then(|certs| certs.first())
        .map(|cert| cert.as_ref())
        .unwrap_or_default();
    let trusted_by_configured_ca =
        data.and_then(|data| data.ca.as_ref())
            .is_some_and(|materials| {
                materials.iter().any(|material| {
                    let mut cursor = std::io::Cursor::new(material);
                    let trusted = rustls_pemfile::certs(&mut cursor)
                        .flatten()
                        .any(|cert| cert.as_ref() == peer);
                    trusted
                })
            });
    let authorized = verify || trusted_by_configured_ca;
    let authorization_error = if authorized {
        ""
    } else {
        "DEPTH_ZERO_SELF_SIGNED_CERT"
    };
    if let Some(socket) = crate::statics::sockets().lock().unwrap().get_mut(&handle) {
        socket.tls.encrypted = true;
        socket.tls.authorized = authorized;
        socket.tls.servername = Some(servername.to_string());
    }
    let own_certificate = data
        .map(|data| {
            let mut cursor = std::io::Cursor::new(&data.cert);
            let certificate = rustls_pemfile::certs(&mut cursor)
                .flatten()
                .next()
                .map(|cert| cert.as_ref().to_vec())
                .unwrap_or_default();
            certificate
        })
        .unwrap_or_default();
    extern "C" {
        fn js_tls_client_record_connected(
            handle: i64,
            authorized: i32,
            authorization_error_ptr: *const u8,
            authorization_error_len: usize,
            protocol_ptr: *const u8,
            protocol_len: usize,
            alpn_ptr: *const u8,
            alpn_len: usize,
            peer_cert_ptr: *const u8,
            peer_cert_len: usize,
            own_cert_ptr: *const u8,
            own_cert_len: usize,
        );
    }
    unsafe {
        js_tls_client_record_connected(
            handle,
            authorized as i32,
            authorization_error.as_ptr(),
            authorization_error.len(),
            protocol.as_ptr(),
            protocol.len(),
            alpn.as_ptr(),
            alpn.len(),
            peer.as_ptr(),
            peer.len(),
            own_certificate.as_ptr(),
            own_certificate.len(),
        );
    }
}

// ─── FFI: tls.connect ────────────────────────────────────────────────────────

/// `tls.connect(...)` — opens a plain TCP socket and runs the TLS handshake
/// before firing `'connect'`/`'secureConnect'`. Use this for HTTPS-style
/// protocols that start TLS from byte 0.
///
/// Resolves Node's overloads plus Perry's legacy positional form (#4971 —
/// pre-fix only the legacy form existed; `tls.connect({ port })` had its
/// options object string-coerced by the NA_STR table row, returned handle 0,
/// and every method call on the "socket" hit the runtime's null-pointer
/// guard):
///
/// - `tls.connect(options[, callback])` — `port` required; `host`/`hostname`
///   default `"localhost"`; `servername` defaults to the host;
///   `rejectUnauthorized: false` disables cert verification.
/// - `tls.connect(port[, host][, options][, callback])`
/// - Legacy Perry positional: `tls.connect(host, port, servername?, verify?)`.
///
/// The `callback` (whichever slot it lands in) registers as a
/// `'secureConnect'` listener, matching the Node spec.
///
/// # Safety
///
/// All four args must be raw NaN-boxed Perry-runtime values per the codegen
/// ABI — see `NA_F64` lowering in perry-codegen.
#[no_mangle]
pub unsafe extern "C" fn js_tls_connect(arg1: f64, arg2: f64, arg3: f64, arg4: f64) -> i64 {
    // `js_tls_prepare_connect` may invoke a user-replaced createSecureContext
    // before overload resolution. Keep every incoming value in the runtime's
    // moving-GC root stack, and re-read the selected options/callback values
    // after every later callback-capable runtime call.
    let root_scope = perry_ffi::TransientRootScope::enter();
    let rooted_args = [
        root_scope.root_nanbox(arg1),
        root_scope.root_nanbox(arg2),
        root_scope.root_nanbox(arg3),
        root_scope.root_nanbox(arg4),
    ];
    extern "C" {
        fn js_tls_prepare_connect();
    }
    js_tls_prepare_connect();
    use crate::option_setters::js_net_validate_connect_port;
    use crate::{
        get_object_bool_field, get_object_number_field, get_object_string_field,
        is_nanboxed_pointer, spawn_socket_task_initialized, statics, string_from_header_i64,
        unbox_pointer,
    };
    use perry_ffi::JsValue;

    extern "C" {
        fn js_value_is_closure(value_bits: i64) -> i32;
        fn js_get_string_pointer_unified(value: f64) -> i64;
    }
    let is_closure =
        |v: f64| is_nanboxed_pointer(v) && js_value_is_closure(v.to_bits() as i64) != 0;
    let as_string = |v: f64| -> Option<String> {
        // #1781: accept BOTH heap (STRING_TAG) and inline SSO
        // (SHORT_STRING_TAG) strings. The strict `is_string()` matched
        // STRING_TAG only, so a short tls arg (`tls.connect("h", …)`)
        // was dropped before `js_get_string_pointer_unified` — which
        // already materializes either repr — ever ran.
        if !JsValue::from_bits(v.to_bits()).is_any_string() {
            return None;
        }
        string_from_header_i64(js_get_string_pointer_unified(v))
    };
    // Cert verification only goes off when the caller says so explicitly —
    // a missing/undefined flag keeps it on.
    let explicitly_off = |v: f64| -> bool {
        let j = JsValue::from_bits(v.to_bits());
        (j.is_bool() && !j.to_bool()) || (j.is_number() && j.to_number() == 0.0)
    };

    let (host, port, servername, verify, callback_arg, metadata_options_arg);
    if let Some(h) = as_string(rooted_args[0].get()) {
        // Legacy Perry positional: (host, port, servername?, verify?).
        let p = JsValue::from_bits(rooted_args[1].get().to_bits());
        if !p.is_number() && !p.is_int32() {
            return 0;
        }
        port = p.to_number() as u16;
        servername = as_string(rooted_args[2].get()).unwrap_or_else(|| h.clone());
        host = h;
        verify = !explicitly_off(rooted_args[3].get());
        callback_arg = None;
        metadata_options_arg = None;
    } else if JsValue::from_bits(rooted_args[0].get().to_bits()).is_number()
        || JsValue::from_bits(rooted_args[0].get().to_bits()).is_int32()
    {
        // Node positional form: tls.connect(port[, host][, options][, cb]).
        js_net_validate_connect_port(rooted_args[0].get());
        port = JsValue::from_bits(rooted_args[0].get().to_bits()).to_number() as u16;
        let mut opt_host: Option<String> = None;
        let mut opts_arg: Option<usize> = None;
        let mut cb_arg: Option<usize> = None;
        for index in [1, 2, 3] {
            let v = rooted_args[index].get();
            if opt_host.is_none() {
                if let Some(h) = as_string(v) {
                    opt_host = Some(h);
                    continue;
                }
            }
            if is_closure(v) {
                cb_arg = cb_arg.or(Some(index));
            } else if is_nanboxed_pointer(v) {
                opts_arg = opts_arg.or(Some(index));
            }
        }
        if let Some(index) = opts_arg {
            extern "C" {
                fn js_tls_validate_positional_connect_options(options: f64);
            }
            js_tls_validate_positional_connect_options(rooted_args[index].get());
        }
        host = opt_host
            .or_else(|| {
                opts_arg.and_then(|index| {
                    let options = rooted_args[index].get();
                    get_object_string_field(options, "host")
                        .or_else(|| get_object_string_field(rooted_args[index].get(), "hostname"))
                })
            })
            .filter(|h| !h.is_empty())
            .unwrap_or_else(|| "localhost".to_string());
        servername = opts_arg
            .and_then(|index| get_object_string_field(rooted_args[index].get(), "servername"))
            .unwrap_or_else(|| host.clone());
        verify = opts_arg
            .and_then(|index| get_object_bool_field(rooted_args[index].get(), "rejectUnauthorized"))
            .unwrap_or(true);
        callback_arg = cb_arg;
        metadata_options_arg = opts_arg;
    } else if is_nanboxed_pointer(rooted_args[0].get()) && !is_closure(rooted_args[0].get()) {
        // Node options form: tls.connect(options[, callback]).
        extern "C" {
            fn js_tls_validate_connect_options(options: f64);
        }
        js_tls_validate_connect_options(rooted_args[0].get());
        if let Some(socket_value) = crate::get_object_value_field(rooted_args[0].get(), "socket") {
            let socket_js = JsValue::from_bits(socket_value.to_bits());
            let handle = if socket_js.is_pointer() {
                crate::unbox_pointer(socket_value) as i64
            } else {
                0
            };
            if handle != 0 {
                host = get_object_string_field(rooted_args[0].get(), "host")
                    .or_else(|| get_object_string_field(rooted_args[0].get(), "hostname"))
                    .unwrap_or_else(|| "localhost".to_string());
                servername = get_object_string_field(rooted_args[0].get(), "servername")
                    .unwrap_or_else(|| host.clone());
                verify = get_object_bool_field(rooted_args[0].get(), "rejectUnauthorized")
                    .unwrap_or(true);
                callback_arg = is_closure(rooted_args[1].get()).then_some(1);
                let config = tls_client_config_data(rooted_args[0].get());
                extern "C" {
                    fn js_tls_client_record_start(
                        handle: i64,
                        options: f64,
                        servername_ptr: *const u8,
                        servername_len: usize,
                    );
                }
                js_tls_client_record_start(
                    handle,
                    rooted_args[0].get(),
                    servername.as_ptr(),
                    servername.len(),
                );
                if let Some(index) = callback_arg {
                    let cb_ptr = unbox_pointer(rooted_args[index].get()) as i64;
                    if cb_ptr != 0 {
                        statics::listeners()
                            .lock()
                            .unwrap()
                            .entry(handle)
                            .or_default()
                            .entry("secureConnect".to_string())
                            .or_default()
                            .push(cb_ptr);
                    }
                }
                let preflight = tls_preflight(0, &servername, rooted_args[0].get());
                if preflight != 0 {
                    crate::push_event(crate::PendingNetEvent::Error(
                        handle,
                        preflight_error(preflight).to_string(),
                    ));
                    crate::push_event(crate::PendingNetEvent::Close(handle));
                } else if let Err(error) = begin_tls_upgrade(handle, servername, verify, config) {
                    crate::push_event(crate::PendingNetEvent::Error(handle, error));
                    crate::push_event(crate::PendingNetEvent::Close(handle));
                }
                return handle;
            }
        }
        port = match get_object_number_field(rooted_args[0].get(), "port") {
            Some(p) => {
                js_net_validate_connect_port(p);
                p as u16
            }
            None => return 0,
        };
        host = match get_object_string_field(rooted_args[0].get(), "host")
            .or_else(|| get_object_string_field(rooted_args[0].get(), "hostname"))
        {
            Some(h) if !h.is_empty() => h,
            _ => "localhost".to_string(),
        };
        servername = get_object_string_field(rooted_args[0].get(), "servername")
            .unwrap_or_else(|| host.clone());
        verify = get_object_bool_field(rooted_args[0].get(), "rejectUnauthorized").unwrap_or(true);
        callback_arg = is_closure(rooted_args[1].get()).then_some(1);
        metadata_options_arg = Some(0);
    } else {
        return 0;
    }

    let metadata_options = || {
        metadata_options_arg
            .map(|index| rooted_args[index].get())
            .unwrap_or_else(|| f64::from_bits(0x7FFC_0000_0000_0001))
    };
    let config = tls_client_config_data(metadata_options());
    if signal_is_pre_aborted(metadata_options()) {
        let handle = crate::js_net_socket_alloc();
        extern "C" {
            fn js_tls_client_record_start(
                handle: i64,
                options: f64,
                servername_ptr: *const u8,
                servername_len: usize,
            );
        }
        js_tls_client_record_start(
            handle,
            metadata_options(),
            servername.as_ptr(),
            servername.len(),
        );
        schedule_tls_abort(handle);
        return handle;
    }
    let preflight = tls_preflight(port, &servername, metadata_options());
    if preflight != 0 {
        let handle = crate::js_net_socket_alloc();
        extern "C" {
            fn js_tls_client_record_start(
                handle: i64,
                options: f64,
                servername_ptr: *const u8,
                servername_len: usize,
            );
        }
        js_tls_client_record_start(
            handle,
            metadata_options(),
            servername.as_ptr(),
            servername.len(),
        );
        crate::push_event(crate::PendingNetEvent::Error(
            handle,
            preflight_error(preflight).to_string(),
        ));
        crate::push_event(crate::PendingNetEvent::Close(handle));
        return handle;
    }
    let metadata_servername = servername.clone();
    let handle =
        spawn_socket_task_initialized(host, port, Some((servername, verify, config)), |handle| {
            extern "C" {
                fn js_tls_client_record_start(
                    handle: i64,
                    options: f64,
                    servername_ptr: *const u8,
                    servername_len: usize,
                );
            }
            js_tls_client_record_start(
                handle,
                metadata_options(),
                metadata_servername.as_ptr(),
                metadata_servername.len(),
            );
        });
    if let Some(index) = callback_arg {
        if handle != 0 {
            let cb_ptr = unbox_pointer(rooted_args[index].get()) as i64;
            if cb_ptr != 0 {
                statics::listeners()
                    .lock()
                    .unwrap()
                    .entry(handle)
                    .or_default()
                    .entry("secureConnect".to_string())
                    .or_default()
                    .push(cb_ptr);
            }
        }
    }
    handle
}

/// Collision-proof entry point for AOT calls. Both bundled stdlib net and
/// perry-ext-net export `js_tls_connect`; auto-optimized binaries may bind the
/// shared name to the bundled registry while their event pump owns ext-net
/// sockets. Keep generated TLS clients in the same backend as the pump.
#[no_mangle]
pub unsafe extern "C" fn js_ext_tls_connect(arg1: f64, arg2: f64, arg3: f64, arg4: f64) -> i64 {
    js_tls_connect(arg1, arg2, arg3, arg4)
}
