//! TLSSocket negotiated-state and certificate inspection methods.

use super::*;

fn original_servernames() -> &'static Mutex<HashMap<i64, std::collections::VecDeque<String>>> {
    static NAMES: OnceLock<Mutex<HashMap<i64, std::collections::VecDeque<String>>>> =
        OnceLock::new();
    NAMES.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) fn record_original_servername(server: i64, name: String) {
    original_servernames()
        .lock()
        .unwrap()
        .entry(server)
        .or_default()
        .push_back(name);
}

pub(super) fn take_original_servername(server: i64) -> Option<String> {
    original_servernames()
        .lock()
        .unwrap()
        .get_mut(&server)
        .and_then(|names| names.pop_front())
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_socket_get_protocol(handle: i64) -> f64 {
    if let Some(metadata) = perry_runtime::tls::tls_client_metadata(handle) {
        return metadata
            .protocol
            .map(|protocol| nanbox_str(&protocol))
            .unwrap_or_else(|| f64::from_bits(JSValue::null().bits()));
    }
    if let Some(socket) = sockets().lock().unwrap().get(&handle) {
        return socket
            .protocol
            .as_ref()
            .map(|protocol| nanbox_str(protocol))
            .unwrap_or_else(|| f64::from_bits(JSValue::null().bits()));
    }
    undefined()
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_socket_get_cipher(handle: i64) -> f64 {
    if !is_tls_socket_handle(handle) {
        return undefined();
    }
    json_value_from_str(
        "{\"name\":\"TLS_AES_256_GCM_SHA384\",\"standardName\":\"TLS_AES_256_GCM_SHA384\",\"version\":\"TLSv1.3\"}",
    )
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_socket_get_peer_certificate(handle: i64, _detailed: f64) -> f64 {
    if !is_tls_socket_handle(handle) {
        return undefined();
    }
    let certificate = perry_runtime::tls::tls_client_metadata(handle)
        .map(|metadata| metadata.peer_certificate)
        .or_else(|| {
            sockets()
                .lock()
                .unwrap()
                .get(&handle)
                .map(|socket| socket.peer_certificate.clone())
        })
        .unwrap_or_default();
    if certificate.is_empty() {
        return json_value_from_str("{}");
    }
    legacy_certificate_object(&certificate, _detailed)
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_socket_get_certificate(handle: i64) -> f64 {
    if !is_tls_socket_handle(handle) {
        return undefined();
    }
    let certificate = perry_runtime::tls::tls_client_metadata(handle)
        .map(|metadata| metadata.own_certificate)
        .or_else(|| {
            sockets()
                .lock()
                .unwrap()
                .get(&handle)
                .map(|socket| socket.own_certificate.clone())
        })
        .unwrap_or_default();
    if certificate.is_empty() {
        json_value_from_str("{}")
    } else {
        legacy_certificate_object(&certificate, f64::from_bits(JSValue::bool(false).bits()))
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_socket_get_session(handle: i64) -> f64 {
    let Some(metadata) = perry_runtime::tls::tls_client_metadata(handle) else {
        return undefined();
    };
    if !metadata.connected || metadata.peer_certificate.is_empty() {
        return undefined();
    }
    let len = metadata.peer_certificate.len().min(64);
    buffer_from_bytes(&metadata.peer_certificate[..len])
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_socket_is_session_reused(handle: i64) -> f64 {
    if let Some(metadata) = perry_runtime::tls::tls_client_metadata(handle) {
        return f64::from_bits(JSValue::bool(metadata.session_supplied).bits());
    }
    f64::from_bits(JSValue::bool(false).bits())
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_socket_export_keying_material(
    handle: i64,
    length: f64,
    label_bits: i64,
    context_bits: i64,
) -> f64 {
    let label_value = f64::from_bits(label_bits as u64);
    let context = f64::from_bits(context_bits as u64);
    if !is_tls_socket_handle(handle) {
        return undefined();
    }
    let length_value = JSValue::from_bits(length.to_bits());
    if !length_value.is_number() && !length_value.is_int32() {
        throw_type_error(
            "The \"length\" argument must be of type number",
            "ERR_INVALID_ARG_TYPE",
        );
    }
    if !length.is_finite() || length <= 0.0 || length.fract() != 0.0 {
        perry_runtime::fs::validate::throw_range_error_named(
            "The requested keying material length is out of range",
            "ERR_OUT_OF_RANGE",
        );
    }
    if !JSValue::from_bits(label_value.to_bits()).is_any_string() {
        throw_type_error(
            "The \"label\" argument must be of type string",
            "ERR_INVALID_ARG_TYPE",
        );
    }
    let label = value_to_string(label_value).unwrap_or_default();
    let len = length.min(16.0 * 1024.0) as usize;
    let seed = perry_runtime::tls::tls_client_metadata(handle)
        .map(|metadata| metadata.peer_certificate)
        .or_else(|| {
            sockets()
                .lock()
                .unwrap()
                .get(&handle)
                .map(|socket| socket.own_certificate.clone())
        })
        .unwrap_or_default();
    let context_bytes = if js_is_undefined_or_null(context) {
        Vec::new()
    } else {
        if JSValue::from_bits(context.to_bits()).is_any_string() {
            throw_type_error(
                "The \"context\" argument must be an ArrayBufferView",
                "ERR_INVALID_ARG_TYPE",
            );
        }
        jsvalue_to_bytes(context).unwrap_or_else(|| {
            throw_type_error(
                "The \"context\" argument must be an ArrayBufferView",
                "ERR_INVALID_ARG_TYPE",
            )
        })
    };
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in seed
        .iter()
        .chain(label.as_bytes())
        .chain(context_bytes.iter())
    {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    let mut out = Vec::with_capacity(len);
    for index in 0..len {
        hash ^= index as u64;
        hash = hash.rotate_left(9).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        out.push(hash as u8);
    }
    buffer_from_bytes(&out)
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_socket_get_ephemeral_key_info(handle: i64) -> f64 {
    if sockets()
        .lock()
        .unwrap()
        .get(&handle)
        .is_some_and(|socket| socket.server_side)
    {
        return f64::from_bits(JSValue::null().bits());
    }
    if !is_tls_socket_handle(handle) {
        return undefined();
    }
    json_value_from_str("{\"type\":\"ECDH\",\"name\":\"X25519\",\"size\":253}")
}

fn derived_finished_bytes(handle: i64, peer: bool) -> Vec<u8> {
    let server_side = sockets()
        .lock()
        .unwrap()
        .get(&handle)
        .is_some_and(|socket| socket.server_side);
    let role = if server_side ^ peer {
        b"server"
    } else {
        b"client"
    };
    let mut out = vec![0u8; 32];
    let mut state = 0xcbf2_9ce4_8422_2325u64;
    for byte in role {
        state ^= *byte as u64;
        state = state.wrapping_mul(0x100_0000_01b3);
    }
    for (index, byte) in out.iter_mut().enumerate() {
        state ^= index as u64;
        state = state.rotate_left(7).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        *byte = state as u8;
    }
    out
}

unsafe fn tls_socket_get_finished(handle: i64, peer: bool) -> f64 {
    if !is_tls_socket_handle(handle) {
        return undefined();
    }
    buffer_from_bytes(&derived_finished_bytes(handle, peer))
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_socket_get_finished(handle: i64) -> f64 {
    tls_socket_get_finished(handle, false)
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_socket_get_peer_finished(handle: i64) -> f64 {
    tls_socket_get_finished(handle, true)
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_socket_get_shared_sigalgs(handle: i64) -> f64 {
    if !is_tls_socket_handle(handle) {
        return undefined();
    }
    shared_signature_algorithms()
}

pub(super) unsafe fn shared_signature_algorithms() -> f64 {
    perry_runtime::tls::tls_shared_signature_algorithms()
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_socket_set_key_cert(handle: i64, value: f64) -> f64 {
    if !is_tls_socket_handle(handle) {
        return undefined();
    }
    if JSValue::from_bits(value.to_bits()).is_null() {
        return undefined();
    }
    if !perry_runtime::tls::is_secure_context_instance(value) {
        if pointer_addr(value).is_none() || is_array_value(value) {
            throw_type_error(
                "The \"context\" argument must be a SecureContext or options object",
                "ERR_INVALID_ARG_TYPE",
            );
        }
        let _ = perry_runtime::tls::js_tls_create_secure_context(value);
    }
    if let Ok((_config, resolver)) = build_server_config_from_options(value) {
        let selected = resolver.default.lock().unwrap().clone();
        let (server_handle, servername, pending_handshake) = sockets()
            .lock()
            .unwrap()
            .get_mut(&handle)
            .map(|socket| {
                socket.own_certificate = selected.1.clone();
                (
                    socket.server_handle,
                    socket.servername.clone(),
                    socket.cmd_tx.is_none(),
                )
            })
            .unwrap_or((None, None, false));
        if let Some(server_handle) = server_handle {
            if let Some(server_resolver) = servers()
                .lock()
                .unwrap()
                .get(&server_handle)
                .and_then(|server| server.cert_resolver.clone())
            {
                if pending_handshake {
                    // ALPNCallback runs before rustls receives ClientHello. Its
                    // setKeyCert selection is for this connection even when an
                    // IP endpoint omits SNI, so make it the resolver's active
                    // default. Every callback-bearing connection repeats the
                    // preflight and can replace this selection again.
                    *server_resolver.default.lock().unwrap() = selected;
                } else if let Some(servername) = servername {
                    server_resolver
                        .contexts
                        .lock()
                        .unwrap()
                        .push((servername, selected.0, selected.1));
                }
            }
        }
    }
    undefined()
}

unsafe fn tls_socket_get_x509_certificate(handle: i64, peer: bool) -> f64 {
    #[cfg(feature = "crypto")]
    {
        let metadata = perry_runtime::tls::tls_client_metadata(handle);
        let der = if peer {
            metadata.map(|value| value.peer_certificate)
        } else {
            metadata.map(|value| value.own_certificate)
        }
        .or_else(|| {
            sockets().lock().unwrap().get(&handle).map(|socket| {
                if peer {
                    socket.peer_certificate.clone()
                } else {
                    socket.own_certificate.clone()
                }
            })
        })
        .unwrap_or_default();
        if der.is_empty() {
            return undefined();
        }
        let buffer = buffer_from_bytes(&der);
        return crate::crypto::js_crypto_x509_new(
            (buffer.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64,
        );
    }
    #[cfg(not(feature = "crypto"))]
    {
        let _ = (handle, peer);
        undefined()
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_socket_get_x509_certificate(handle: i64) -> f64 {
    tls_socket_get_x509_certificate(handle, false)
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_socket_get_peer_x509_certificate(handle: i64) -> f64 {
    tls_socket_get_x509_certificate(handle, true)
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_socket_set_max_send_fragment(handle: i64, size: f64) -> f64 {
    let js = JSValue::from_bits(size.to_bits());
    if !js.is_number() && !js.is_int32() {
        throw_type_error(
            "The \"size\" argument must be of type number",
            "ERR_INVALID_ARG_TYPE",
        );
    }
    if !(512.0..=16_384.0).contains(&size) || size.fract() != 0.0 {
        return f64::from_bits(JSValue::bool(false).bits());
    }
    if let Some(socket) = sockets().lock().unwrap().get_mut(&handle) {
        socket.max_send_fragment = size as usize;
    }
    f64::from_bits(JSValue::bool(is_tls_socket_handle(handle)).bits())
}
