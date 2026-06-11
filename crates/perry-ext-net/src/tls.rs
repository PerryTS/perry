//! rustls client config + handshake for `tls.connect` and the
//! `socket.upgradeToTLS` mid-stream upgrade. Split out of `lib.rs` (#1852)
//! to keep that file under the 2000-line gate; the logic is unchanged.

use std::sync::Arc;

use tokio::net::TcpStream;
use tokio_rustls::{client::TlsStream, rustls, TlsConnector};

fn build_tls_connector(verify: bool) -> Result<TlsConnector, String> {
    // rustls panics resolving the process-level CryptoProvider when both
    // `ring` and `aws-lc-rs` end up in the dep graph. Server paths install
    // one before their first handshake; a client-only program (no tls/https
    // server) reached `ClientConfig::builder()` with none installed once
    // #4971 made `tls.connect` actually resolve its host. Idempotent —
    // `install_default` errors (ignored) if a provider is already set.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    if !verify {
        return build_tls_connector_insecure();
    }
    let mut root_store = rustls::RootCertStore::empty();
    let native = rustls_native_certs::load_native_certs();
    for cert in native.certs {
        let _ = root_store.add(cert);
    }
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    Ok(TlsConnector::from(Arc::new(config)))
}

fn build_tls_connector_insecure() -> Result<TlsConnector, String> {
    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
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

    let config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoVerify))
        .with_no_client_auth();
    Ok(TlsConnector::from(Arc::new(config)))
}

pub(crate) async fn do_tls_handshake(
    tcp: TcpStream,
    servername: &str,
    verify: bool,
) -> Result<TlsStream<TcpStream>, String> {
    let connector = build_tls_connector(verify)?;
    let server_name = rustls::pki_types::ServerName::try_from(servername.to_string())
        .map_err(|e| format!("invalid servername '{}': {}", servername, e))?;
    connector
        .connect(server_name, tcp)
        .await
        .map_err(|e| format!("tls handshake: {}", e))
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
    use crate::option_setters::js_net_validate_connect_port;
    use crate::{
        get_object_bool_field, get_object_number_field, get_object_string_field,
        is_nanboxed_pointer, spawn_socket_task, statics, string_from_header_i64, unbox_pointer,
    };
    use perry_ffi::JsValue;

    extern "C" {
        fn js_value_is_closure(value_bits: i64) -> i32;
        fn js_get_string_pointer_unified(value: f64) -> i64;
    }
    let is_closure =
        |v: f64| is_nanboxed_pointer(v) && js_value_is_closure(v.to_bits() as i64) != 0;
    let as_string = |v: f64| -> Option<String> {
        if !JsValue::from_bits(v.to_bits()).is_string() {
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

    let (host, port, servername, verify, cb_f64);
    if let Some(h) = as_string(arg1) {
        // Legacy Perry positional: (host, port, servername?, verify?).
        let p = JsValue::from_bits(arg2.to_bits());
        if !p.is_number() {
            return 0;
        }
        port = p.to_number() as u16;
        servername = as_string(arg3).unwrap_or_else(|| h.clone());
        host = h;
        verify = !explicitly_off(arg4);
        cb_f64 = None;
    } else if is_nanboxed_pointer(arg1) && !is_closure(arg1) {
        // Node options form: tls.connect(options[, callback]).
        port = match get_object_number_field(arg1, "port") {
            Some(p) => {
                js_net_validate_connect_port(p);
                p as u16
            }
            None => return 0,
        };
        host = match get_object_string_field(arg1, "host")
            .or_else(|| get_object_string_field(arg1, "hostname"))
        {
            Some(h) if !h.is_empty() => h,
            _ => "localhost".to_string(),
        };
        servername = get_object_string_field(arg1, "servername").unwrap_or_else(|| host.clone());
        verify = get_object_bool_field(arg1, "rejectUnauthorized").unwrap_or(true);
        cb_f64 = is_closure(arg2).then_some(arg2);
    } else if JsValue::from_bits(arg1.to_bits()).is_number() {
        // Node positional form: tls.connect(port[, host][, options][, cb]).
        js_net_validate_connect_port(arg1);
        port = arg1 as u16;
        let mut opt_host: Option<String> = None;
        let mut opts: Option<f64> = None;
        let mut cb: Option<f64> = None;
        for v in [arg2, arg3, arg4] {
            if opt_host.is_none() {
                if let Some(h) = as_string(v) {
                    opt_host = Some(h);
                    continue;
                }
            }
            if is_closure(v) {
                cb = cb.or(Some(v));
            } else if is_nanboxed_pointer(v) {
                opts = opts.or(Some(v));
            }
        }
        host = opt_host
            .or_else(|| {
                opts.and_then(|o| {
                    get_object_string_field(o, "host")
                        .or_else(|| get_object_string_field(o, "hostname"))
                })
            })
            .filter(|h| !h.is_empty())
            .unwrap_or_else(|| "localhost".to_string());
        servername = opts
            .and_then(|o| get_object_string_field(o, "servername"))
            .unwrap_or_else(|| host.clone());
        verify = opts
            .and_then(|o| get_object_bool_field(o, "rejectUnauthorized"))
            .unwrap_or(true);
        cb_f64 = cb;
    } else {
        return 0;
    }

    let handle = spawn_socket_task(host, port, Some((servername, verify)));
    if let Some(cb) = cb_f64 {
        if handle != 0 {
            let cb_ptr = unbox_pointer(cb) as i64;
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
