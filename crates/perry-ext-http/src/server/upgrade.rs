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

use perry_ffi::{JsClosure, RawClosureHeader};

use crate::server::request::handle_to_pointer_f64;
use crate::server::server::HttpServer;
use crate::server::types::{js_promise_run_microtasks, POINTER_TAG, PTR_MASK};

fn upgrade_head_arg(head_data: &[u8]) -> f64 {
    let head = perry_ffi::alloc_buffer(head_data);
    f64::from_bits(POINTER_TAG | (head as u64 & PTR_MASK))
}

/// Fire the `'upgrade'` event listeners with `(im, wsId, head)`.
/// Called from the main-thread event loop after the upgrade pending
/// has been dispatched. Returns whether a listener received ownership.
pub(crate) fn fire_upgrade_listeners(
    server_handle: i64,
    im_handle: i64,
    ws_id: i64,
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
    // Encode ws_id as NaN-boxed POINTER_TAG so `unbox_to_i64` (the
    // codegen helper used at every NATIVE_MODULE_TABLE receiver
    // call site — `wsId.send(...)` / `wsId.on(...)`) extracts the
    // low-48 bits as the original ws_id. A plain `ws_id as f64`
    // (1.0_f64) would have bits 0x3FF0_…, which `unbox_to_i64`
    // AND-masks to 0, missing the WS_CONNECTIONS lookup entirely.
    let ws_id_f64 = f64::from_bits(POINTER_TAG | (ws_id as u64 & PTR_MASK));
    // Node always supplies a Buffer, including for a zero-length head. Public
    // `ws` reads `head.length` before deciding whether to call `unshift`, and
    // upgrade bytes are arbitrary protocol data rather than UTF-8 text.
    let head_arg = scope.root_nanbox(upgrade_head_arg(&head_data));

    let mut delivered = false;
    for cb in listeners {
        if cb.get() == 0 {
            continue;
        }
        unsafe {
            let raw = cb.get() as *const RawClosureHeader;
            let closure = JsClosure::from_raw(raw);
            if !closure.is_null() {
                delivered = true;
                let _ = closure.call3(req_f64, ws_id_f64, head_arg.get());
            }
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

    fn head_bytes(data: &[u8]) -> &'static [u8] {
        let arg = upgrade_head_arg(data);
        assert_eq!(arg.to_bits() & !PTR_MASK, POINTER_TAG);
        let ptr = (arg.to_bits() & PTR_MASK) as *const perry_ffi::BufferHeader;
        perry_ffi::read_buffer_bytes(ptr).expect("upgrade head buffer")
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
