//! Ordinary Socket TLS compatibility facts and loop reference state.

use super::*;

fn owner(handle: i64) -> f64 {
    crate::payload_transport::boxed_addr(handle)
}
fn tls_field(handle: i64, key: &str) -> f64 {
    let scope = perry_ffi::TransientRootScope::enter();
    let owner = scope.root_nanbox(owner(handle));
    let state = scope.root_nanbox(crate::payload_socket::state(owner.get()));
    crate::payload_transport::own_get(state.get(), key)
}

/// TLS compatibility facts are ordinary Socket JS state, never an id record.
/// # Safety
/// Call on the owning runtime thread. Heap-address carriers must name live,
/// rooted allocations in that realm; NaN-boxed arguments must be valid JS values.
/// Non-null byte pointers must be readable for their paired length.
#[no_mangle]
pub unsafe extern "C" fn js_ext_net_set_tls_metadata(
    handle: i64,
    authorized: i32,
    servername_ptr: *const u8,
    servername_len: usize,
    peer_certificate_cn_ptr: *const u8,
    peer_certificate_cn_len: usize,
    session_id: u64,
    session_reused: i32,
) {
    let scope = perry_ffi::TransientRootScope::enter();
    let owner = scope.root_nanbox(owner(handle));
    if crate::payload_transport::socket_link(owner.get()).is_err() {
        return;
    }
    let state = scope.root_nanbox(crate::payload_socket::state(owner.get()));
    let string = |ptr: *const u8, len: usize| {
        if ptr.is_null() {
            crate::payload_transport::undefined()
        } else {
            crate::payload_events::string(&String::from_utf8_lossy(std::slice::from_raw_parts(
                ptr, len,
            )))
        }
    };
    for (key, value) in [
        ("encrypted", true),
        ("authorized", authorized != 0),
        ("sessionReused", session_reused != 0),
    ] {
        crate::payload_transport::own_set(
            state.get(),
            key,
            f64::from_bits(JsValue::from_bool(value).bits()),
        );
    }
    crate::payload_transport::own_set(
        state.get(),
        "servername",
        string(servername_ptr, servername_len),
    );
    crate::payload_transport::own_set(
        state.get(),
        "peerCertificateCn",
        string(peer_certificate_cn_ptr, peer_certificate_cn_len),
    );
    crate::payload_transport::own_set(
        state.get(),
        "session",
        f64::from_bits(JsValue::from_object_ptr(alloc_buffer(&session_id.to_be_bytes())).bits()),
    );
}

/// # Safety
/// Call on the owning runtime thread. Heap-address carriers must name live,
/// rooted allocations in that realm; NaN-boxed arguments must be valid JS values.
/// Non-null byte pointers must be readable for their paired length.
#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_tls_encrypted(handle: i64) -> f64 {
    tls_field(handle, "encrypted")
}
/// # Safety
/// Call on the owning runtime thread. Heap-address carriers must name live,
/// rooted allocations in that realm; NaN-boxed arguments must be valid JS values.
/// Non-null byte pointers must be readable for their paired length.
#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_tls_authorized(handle: i64) -> f64 {
    tls_field(handle, "authorized")
}
/// # Safety
/// Call on the owning runtime thread. Heap-address carriers must name live,
/// rooted allocations in that realm; NaN-boxed arguments must be valid JS values.
/// Non-null byte pointers must be readable for their paired length.
#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_tls_servername(handle: i64) -> f64 {
    tls_field(handle, "servername")
}
/// # Safety
/// Call on the owning runtime thread. Heap-address carriers must name live,
/// rooted allocations in that realm; NaN-boxed arguments must be valid JS values.
/// Non-null byte pointers must be readable for their paired length.
#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_tls_session(
    handle: i64,
) -> *mut perry_ffi::BufferHeader {
    let bytes = crate::jsvalue_to_socket_bytes(tls_field(handle, "session")).unwrap_or_default();
    alloc_buffer(&bytes)
}
/// # Safety
/// Call on the owning runtime thread. Heap-address carriers must name live,
/// rooted allocations in that realm; NaN-boxed arguments must be valid JS values.
/// Non-null byte pointers must be readable for their paired length.
#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_tls_session_reused(handle: i64) -> f64 {
    tls_field(handle, "sessionReused")
}
/// # Safety
/// Call on the owning runtime thread. Heap-address carriers must name live,
/// rooted allocations in that realm; NaN-boxed arguments must be valid JS values.
/// Non-null byte pointers must be readable for their paired length.
#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_peer_certificate_json(handle: i64) -> *mut StringHeader {
    let cn = crate::jsvalue_to_owned_string(tls_field(handle, "peerCertificateCn"));
    let value = cn
        .map(|cn| serde_json::json!({"subject":{"CN":cn}}))
        .unwrap_or_else(|| serde_json::json!({}));
    alloc_string(&value.to_string()).as_raw()
}
#[no_mangle]
pub extern "C" fn js_ext_net_is_socket_handle(handle: i64) -> i32 {
    crate::payload_transport::socket_link(owner(handle)).is_ok() as i32
}
#[no_mangle]
pub extern "C" fn js_ext_net_socket_set_ref(handle: i64, refed: i32) {
    crate::payload_socket::set_ref(owner(handle), refed != 0);
}
#[no_mangle]
pub extern "C" fn js_ext_net_socket_has_ref(handle: i64) -> i32 {
    crate::native_transport::has_ref(owner(handle)) as i32
}
