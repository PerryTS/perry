//! TLS metadata and event-loop reference state for socket facades.

use super::*;

#[derive(Clone, Default)]
pub(crate) struct TlsSocketMetadata {
    pub(crate) encrypted: bool,
    pub(crate) authorized: bool,
    pub(crate) servername: Option<String>,
    pub(crate) session: Vec<u8>,
    pub(crate) session_reused: bool,
    pub(crate) peer_certificate_cn: Option<String>,
}

/// Attach TLS-observable state to a socket facade owned by another native
/// extension (notably the reqwest-backed HTTPS client).
#[no_mangle]
pub unsafe extern "C" fn js_ext_net_set_tls_metadata(
    handle: i64,
    authorized: i32,
    servername_ptr: *const u8,
    servername_len: usize,
    session_id: u64,
    session_reused: i32,
) {
    if let Some(socket) = statics::sockets().lock().unwrap().get_mut(&handle) {
        socket.tls.encrypted = true;
        socket.tls.authorized = authorized != 0;
        socket.tls.servername = if servername_ptr.is_null() {
            None
        } else {
            Some(
                String::from_utf8_lossy(std::slice::from_raw_parts(servername_ptr, servername_len))
                    .into_owned(),
            )
        };
        socket.tls.session = session_id.to_be_bytes().to_vec();
        socket.tls.session_reused = session_reused != 0;
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_tls_encrypted(handle: i64) -> f64 {
    let value = statics::sockets()
        .lock()
        .unwrap()
        .get(&handle)
        .is_some_and(|socket| socket.tls.encrypted);
    f64::from_bits(JsValue::from_bool(value).bits())
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_tls_authorized(handle: i64) -> f64 {
    let value = statics::sockets()
        .lock()
        .unwrap()
        .get(&handle)
        .is_some_and(|socket| socket.tls.authorized);
    f64::from_bits(JsValue::from_bool(value).bits())
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_tls_servername(handle: i64) -> f64 {
    match statics::sockets()
        .lock()
        .unwrap()
        .get(&handle)
        .and_then(|socket| socket.tls.servername.clone())
    {
        Some(value) if !value.is_empty() => {
            f64::from_bits(JsValue::from_string_ptr(alloc_string(&value).as_raw()).bits())
        }
        _ => f64::from_bits(JsValue::from_bool(false).bits()),
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_tls_session(
    handle: i64,
) -> *mut perry_ffi::BufferHeader {
    let bytes = statics::sockets()
        .lock()
        .unwrap()
        .get(&handle)
        .map(|socket| socket.tls.session.clone())
        .unwrap_or_default();
    alloc_buffer(&bytes)
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_tls_session_reused(handle: i64) -> f64 {
    let value = statics::sockets()
        .lock()
        .unwrap()
        .get(&handle)
        .is_some_and(|socket| socket.tls.session_reused);
    f64::from_bits(JsValue::from_bool(value).bits())
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_peer_certificate_json(handle: i64) -> *mut StringHeader {
    let cn = statics::sockets()
        .lock()
        .unwrap()
        .get(&handle)
        .and_then(|socket| socket.tls.peer_certificate_cn.clone());
    let value = cn
        .map(|cn| format!(r#"{{"subject":{{"CN":{cn:?}}}}}"#))
        .unwrap_or_else(|| "{}".to_string());
    alloc_string(&value).as_raw()
}

#[no_mangle]
pub extern "C" fn js_ext_net_is_socket_handle(handle: i64) -> i32 {
    let owned = is_net_socket_handle(handle);
    if owned {
        1
    } else {
        0
    }
}

/// Update whether a socket participates in event-loop liveness.
#[no_mangle]
pub extern "C" fn js_ext_net_socket_set_ref(handle: i64, refed: i32) {
    if let Some(socket) = statics::sockets().lock().unwrap().get_mut(&handle) {
        socket.refed = refed != 0;
    }
    perry_ffi::notify_main_thread();
}

/// Return nonzero when a socket is still referenced by the event loop.
#[no_mangle]
pub extern "C" fn js_ext_net_socket_has_ref(handle: i64) -> i32 {
    statics::sockets()
        .lock()
        .unwrap()
        .get(&handle)
        .is_none_or(|socket| socket.refed) as i32
}

/// Auxiliary liveness hook registered with the runtime for mixed stdlib links.
#[no_mangle]
pub extern "C" fn js_ext_net_has_active_handles() -> i32 {
    server_state::has_active_handles() as i32
}
