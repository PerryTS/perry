//! Phase 4 — `Server.on('upgrade', (req, socket, head) => …)` for
//! HTTP Upgrade requests (WebSocket handshakes, primarily).
//!
//! # Design
//!
//! The turnloop connection layer (`turnloop_serve::conn`) recognises an
//! Upgrade request and hands the connection over — to perry-ext-ws for a
//! WebSocket handshake answered by an attached `WebSocketServer`, or to
//! perry-ext-net as a raw `net.Socket` for JS `'upgrade'` listeners — then
//! queues an `HttpPendingUpgrade`. The main-thread pump fires the server's
//! `'upgrade'` listeners here with `(req, socket, head)`.
//!
//! Attached WebSocket servers are native observers registered by perry-ext-ws.

use crate::server::request::handle_to_pointer_f64;
use crate::server::server::HttpServer;
use crate::server::types::{js_promise_run_microtasks, POINTER_TAG, PTR_MASK};

fn upgrade_head_arg(head_data: &[u8]) -> f64 {
    let head = perry_ffi::alloc_buffer(head_data);
    f64::from_bits(perry_ffi::JsValue::from_object_ptr(head as usize as *mut u8).bits())
}

/// Fire the `'upgrade'` event listeners with `(im, wsId, head)`.
/// Called from the main-thread event loop after the upgrade pending
/// has been dispatched. Returns whether a listener received ownership.
pub(crate) fn fire_upgrade_listeners(
    server_handle: i64,
    im_handle: i64,
    socket: f64,
    head_data: Vec<u8>,
) -> bool {
    let listeners = crate::server::server::with_base_server_mut(server_handle, |server| {
        crate::server::server::take_server_event_listeners(server, "upgrade")
    })
    .unwrap_or_default();
    if listeners.is_empty() {
        return false;
    }
    let scope = perry_ffi::TransientRootScope::enter();
    let listeners = scope.root_addrs(&listeners);

    let req_f64 = handle_to_pointer_f64(im_handle);
    let socket = scope.root_nanbox(socket);
    // Node always supplies a Buffer, including for a zero-length head. Public
    // `ws` reads `head.length` before deciding whether to call `unshift`, and
    // upgrade bytes are arbitrary protocol data rather than UTF-8 text.
    let head_arg = scope.root_nanbox(upgrade_head_arg(&head_data));

    let mut delivered = false;
    for cb in listeners {
        if cb.get() == 0 {
            continue;
        }
        delivered = true;
        perry_ext_net::native_transport::call(
            f64::from_bits(perry_ffi::JsValue::from_object_ptr(cb.get() as *mut u8).bits()),
            perry_ffi::JsThis::from_f64(handle_to_pointer_f64(server_handle)),
            &[req_f64, socket.get(), head_arg.get()],
        );
        unsafe {
            js_promise_run_microtasks();
        }
    }
    delivered
}

#[allow(dead_code)]
// The `& 0` is deliberate: the value is the canonical null-pointer NaN-box
// (POINTER_TAG with an all-zero payload), spelled out so both constants stay
// referenced by this linker anchor.
#[allow(clippy::erasing_op)]
fn _force_link() -> u64 {
    POINTER_TAG | (PTR_MASK & 0)
}

#[cfg(test)]
mod tests {
    use super::{upgrade_head_arg, POINTER_TAG, PTR_MASK};

    fn head_bytes(data: &[u8]) -> Vec<u8> {
        let arg = upgrade_head_arg(data);
        assert_eq!(arg.to_bits() & !PTR_MASK, POINTER_TAG);
        let ptr = (arg.to_bits() & PTR_MASK) as *const perry_ffi::BufferHeader;
        perry_ffi::bytes::no_gc(|scope| {
            perry_ffi::read_buffer_bytes(ptr, scope)
                .expect("upgrade head buffer")
                .to_vec()
        })
    }

    #[test]
    fn upgrade_request_reads_the_native_registry_tag() {
        let request = crate::server::request::IncomingMessage::new(
            "GET".into(),
            "/socket?token=1".into(),
            Default::default(),
            vec![("Upgrade".into(), "websocket".into())],
            vec![],
            "127.0.0.1".into(),
            1234,
        );
        let handle = perry_ffi::register_handle(request);
        let value = super::handle_to_pointer_f64(handle);
        assert!(perry_ffi::JsValue::from_bits(value.to_bits()).is_handle());
        let head =
            super::upgrade_request(value).expect("native request must survive the upgrade bridge");
        assert_eq!(head.method, "GET");
        assert_eq!(head.target, "/socket?token=1");
        assert_eq!(head.version, 1);
        assert_eq!(head.get("upgrade"), Some(b"websocket".as_slice()));
        assert!(super::upgrade_request(f64::from_bits(POINTER_TAG | handle as u64)).is_none());
        assert!(super::upgrade_request(1.0).is_none());
        perry_ffi::drop_handle(handle);
        assert!(super::upgrade_request(value).is_none());
    }

    #[test]
    fn empty_upgrade_head_is_an_empty_buffer() {
        assert_eq!(head_bytes(&[]), &[] as &[u8]);
    }

    #[test]
    fn upgrade_head_preserves_binary_bytes() {
        assert_eq!(head_bytes(&[0xff, 0x00, 0x80]), &[0xff, 0x00, 0x80]);
    }
}

/// Read owned address metadata without allocating JS objects or introducing a
/// reverse dependency from ws to HTTP.
pub(crate) fn attached_address(handle: i64) -> Option<(String, u16)> {
    perry_ffi::get_handle::<HttpServer>(handle)
        .and_then(|s| s.listening.then(|| (s.bound_host.clone(), s.bound_port)))
        .or_else(|| {
            perry_ffi::get_handle::<crate::server::https_server::HttpsServer>(handle).and_then(
                |s| {
                    s.base
                        .listening
                        .then(|| (s.base.bound_host.clone(), s.base.bound_port))
                },
            )
        })
}

/// Copy the upgrade request while keeping the ws → HTTP dependency one-way.
pub(crate) fn upgrade_request(value: f64) -> Option<turnloop_http::http1::Head> {
    let value = perry_ffi::JsValue::from_bits(value.to_bits());
    if !value.is_handle() {
        return None;
    }
    let handle = (value.bits() & PTR_MASK) as i64;
    let request = perry_ffi::get_handle::<crate::server::request::IncomingMessage>(handle)?;
    Some(perry_ext_ws::turnloop_link::request_head(
        &request.method,
        &request.url,
        u8::from(request.http_version != "1.0"),
        &request.raw_headers,
    ))
}
