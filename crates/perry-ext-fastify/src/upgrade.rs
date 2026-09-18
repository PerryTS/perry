//! #1113 — `app.server.on("upgrade", (req, socket, head) => …)` for
//! HTTP Upgrade requests (WebSocket handshakes) on a fastify app.
//!
//! Mirrors perry-ext-http's `upgrade.rs` (issue #577 Phase 4),
//! the proven template for bidirectional WebSocket upgrade dispatch.
//!
//! # Design
//!
//! `perry_http_server` decodes the request head, sees `Connection: upgrade`
//! and asks `FastifyHost::takes_upgrades`. For an app that registered
//! `app.server.on('upgrade', …)` handlers the answer is yes, and the core
//! stops being an HTTP connection:
//!
//! 1. `FastifyHost::on_upgrade` validates the handshake and writes the `101`
//!    through [`perry_ext_ws::accept_http_upgrade`], which then installs
//!    `turnloop_websocket`'s sans-I/O codec on the connection in place.
//! 2. That yields the standard `ws_id` — the same integer the standalone
//!    `WebSocketServer({port})` path produces, so
//!    `wss.handleUpgrade(req, socket, head, cb)` re-dispatches it through
//!    perry-ext-ws's `js_ws_handle_upgrade`.
//! 3. A `FastifyPendingUpgrade` is queued, because `on_upgrade` runs in the
//!    completion sink and must not run JS.
//! 4. `js_fastify_process_pending` fires the `upgrade_handlers` with
//!    `(req, ws_id, head)` on the main thread's own tick.
//!
//! What this replaced: a synchronous hand-built `101` so hyper would switch
//! protocols, a `tokio::spawn`ed task awaiting `hyper::upgrade::on`, a
//! `tokio_tungstenite::WebSocketStream::from_raw_socket`, and a
//! worker-to-main channel hop. Steps 3 and 4 are the only ones that survive.

use perry_ffi::{
    alloc_string, build_object_shape, get_handle, js_object_alloc_with_shape, js_object_set_field,
    Handle, JsClosure, JsValue, ObjectHeader, RawClosureHeader,
};
use std::collections::HashMap;

use crate::app::FastifyApp;

const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const PTR_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

extern "C" {
    fn js_promise_run_microtasks() -> i32;
}

/// Build a minimal pointer-tagged request object exposing
/// `{ method, url, headers }`. `headers` is a nested object of
/// lowercased name → value. Returns the NaN-boxed (POINTER_TAG) bits
/// as f64, or `undefined` on allocation failure.
///
/// This must run from the main thread. The hyper worker queues the raw
/// method/url/headers in `FastifyPendingUpgrade`; allocating the JS object
/// here avoids holding unscannable JS heap pointers across the worker-to-main
/// handoff.
///
/// Fastify's per-handler request is backed by a `FastifyContext`
/// handle dispatched via the `request.*` codegen arm — that path is
/// tightly coupled to the route dispatcher and oneshot response
/// channel, so reusing it for an upgrade (which has no reply) would
/// require threading a no-op response. We instead allocate a plain
/// object with the request-shaped fields the `'upgrade'` handler
/// reads (`req.headers`, `req.url`, `req.method`). It's a real
/// pointer-tagged object so `typeof req === "object"`.
pub(crate) unsafe fn build_request_object(
    method: &str,
    url: &str,
    headers: &HashMap<String, String>,
) -> f64 {
    let undefined = f64::from_bits(TAG_UNDEFINED);

    let headers_obj = build_string_map_object(headers).unwrap_or(undefined);

    let keys = ["method", "url", "headers"];
    let (packed, shape_id) = build_object_shape(&keys);
    let obj: *mut ObjectHeader = js_object_alloc_with_shape(
        shape_id,
        keys.len() as u32,
        packed.as_ptr(),
        packed.len() as u32,
    );
    if obj.is_null() {
        return undefined;
    }
    let method_s = alloc_string(method);
    js_object_set_field(obj, 0, JsValue::from_string_ptr(method_s.as_raw()));
    let url_s = alloc_string(url);
    js_object_set_field(obj, 1, JsValue::from_string_ptr(url_s.as_raw()));
    js_object_set_field(obj, 2, JsValue::from_bits(headers_obj.to_bits()));

    let v = JsValue::from_object_ptr(obj);
    f64::from_bits(v.bits())
}

unsafe fn build_string_map_object(map: &HashMap<String, String>) -> Option<f64> {
    let keys: Vec<&str> = map.keys().map(|s| s.as_str()).collect();
    let (packed, shape_id) = build_object_shape(&keys);
    let count = keys.len() as u32;
    let obj: *mut ObjectHeader =
        js_object_alloc_with_shape(shape_id, count, packed.as_ptr(), packed.len() as u32);
    if obj.is_null() {
        return None;
    }
    for (i, key) in keys.iter().enumerate() {
        if let Some(val) = map.get(*key) {
            let s = alloc_string(val);
            js_object_set_field(obj, i as u32, JsValue::from_string_ptr(s.as_raw()));
        }
    }
    let v = JsValue::from_object_ptr(obj);
    Some(f64::from_bits(v.bits()))
}

/// Fire the `app.server.on("upgrade", …)` handlers with
/// `(req, ws_id, head)`. Called from the main-thread pump after the
/// upgrade has been registered with perry-ext-ws.
///
/// NaN-boxing mirrors the http template exactly:
///   - `req` is a pointer-tagged minimal request object.
///   - `ws_id` is encoded as `POINTER_TAG | (ws_id & PTR_MASK)` so
///     the codegen `unbox_to_i64` at every `wss.handleUpgrade(...)` /
///     `wsId.send(...)` callsite extracts the original integer id.
///   - `head` is a STRING_TAG string when non-empty, else undefined.
pub(crate) fn fire_fastify_upgrade_listeners(
    app_handle: Handle,
    req_handle_bits: i64,
    ws_id: i64,
    head_data: Vec<u8>,
) {
    let listeners = match get_handle::<FastifyApp>(app_handle) {
        Some(app) => app.upgrade_handlers.clone(),
        None => return,
    };
    if listeners.is_empty() {
        return;
    }

    let req_f64 = f64::from_bits(req_handle_bits as u64);
    let ws_id_f64 = f64::from_bits(POINTER_TAG | (ws_id as u64 & PTR_MASK));
    let head_f64 = if head_data.is_empty() {
        f64::from_bits(TAG_UNDEFINED)
    } else {
        let s = String::from_utf8_lossy(&head_data).into_owned();
        let header = alloc_string(&s);
        f64::from_bits(STRING_TAG | (header.as_raw() as u64 & PTR_MASK))
    };

    for cb in &listeners {
        if *cb == 0 {
            continue;
        }
        unsafe {
            let raw = *cb as *const RawClosureHeader;
            let closure = JsClosure::from_raw(raw);
            if !closure.is_null() {
                let _ = closure.call3(req_f64, ws_id_f64, head_f64);
            }
            js_promise_run_microtasks();
        }
    }
}

#[allow(dead_code)]
// The `& 0` is deliberate: this linker anchor only exists to keep the tag
// constants referenced; the composed value itself is meaningless.
#[allow(clippy::erasing_op)]
fn _force_link() -> u64 {
    POINTER_TAG | (PTR_MASK & 0) | STRING_TAG | TAG_UNDEFINED
}
