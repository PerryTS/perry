//! #1113 — `app.server.on("upgrade", (req, socket, head) => …)` for
//! HTTP Upgrade requests (WebSocket handshakes) on a fastify app.
//!
//! Mirrors perry-ext-http-server's `upgrade.rs` (issue #577 Phase 4),
//! the proven template for bidirectional WebSocket upgrade dispatch.
//!
//! # Design
//!
//! When the hyper service fn in `server.rs` sees a request with
//! `Connection: Upgrade` + `Upgrade: websocket`, fastify diverges
//! from the normal route flow:
//!
//! 1. The accepting tokio task awaits `hyper::upgrade::on(&mut req)`,
//!    yielding an `Upgraded` stream after hyper sends the 101.
//! 2. It builds a `tokio_tungstenite::WebSocketStream` from that raw
//!    socket with `Role::Server` (the handshake bytes were already
//!    exchanged via the 101 response we returned synchronously).
//! 3. The resulting stream is registered in perry-ext-ws's connection
//!    registry through `perry_ext_ws::register_external_ws_stream`,
//!    yielding the standard `ws_id`.
//! 4. The fastify `upgrade_handlers` (registered via
//!    `app.server.on("upgrade", cb)`) are fired with
//!    `(req, ws_id, head)`. `ws_id` is the same integer id the
//!    standalone `WebSocketServer({port})` path produces, so
//!    `wss.handleUpgrade(req, socket, head, cb)` re-dispatches it
//!    through perry-ext-ws's `js_ws_handle_upgrade`.

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

/// Test whether a request looks like a WebSocket upgrade — checks
/// `Connection: Upgrade` (case-insensitive contains) and
/// `Upgrade: websocket` (case-insensitive). Hyper's `headers()`
/// already lowercases names, so we only normalize values. Identical
/// to perry-ext-http-server's `is_websocket_upgrade`.
pub(crate) fn is_websocket_upgrade(req: &hyper::Request<hyper::body::Incoming>) -> bool {
    let h = req.headers();
    let connection_ok = h
        .get("connection")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_ascii_lowercase().contains("upgrade"))
        .unwrap_or(false);
    let upgrade_ok = h
        .get("upgrade")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false);
    connection_ok && upgrade_ok
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
