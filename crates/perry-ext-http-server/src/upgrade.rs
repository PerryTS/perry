//! Phase 4 — `Server.on('upgrade', (req, socket, head) => …)` for
//! HTTP Upgrade requests (WebSocket handshakes, primarily).
//!
//! # Design
//!
//! When a hyper service fn sees a request with `Connection: Upgrade`
//! + `Upgrade: websocket`, perry-ext-http-server diverges from the
//! Phase 1 (req, res) flow. Instead:
//!
//! 1. The accepting tokio task awaits `hyper::upgrade::on(&mut req)`,
//!    yielding an `Upgraded` stream after hyper sends a 101.
//! 2. It runs `tokio_tungstenite::accept_async` on the upgraded
//!    stream to complete the WebSocket handshake server-side.
//! 3. The resulting `WebSocketStream<Upgraded>` is registered in
//!    perry-ext-ws's connection registry through
//!    `perry_ext_ws::register_external_ws_stream`, yielding the
//!    standard `ws_id` that the rest of perry-ext-ws's surface
//!    consumes.
//! 4. The `'upgrade'` listeners on the HTTP server are fired with
//!    `(im_f64, ws_id_f64, head_str_f64)`. `ws_id_f64` is the same
//!    integer id as standalone `WebSocketServer({port})` connections,
//!    so user code can interact with it through `ws.on('message',…)`,
//!    `ws.send(…)`, `ws.close(…)` unchanged.
//!
//! The TS-side wrapper for `import { WebSocketServer } from 'ws'`
//! when constructed with `{ server }` simply registers an
//! `'upgrade'` listener that re-dispatches to its own `'connection'`
//! event:
//!
//! ```ts
//! const wss = new WebSocketServer({ server: httpServer });
//! // wss internally:
//! //   server.on('upgrade', (req, wsId, head) => {
//! //     wss.emit('connection', wsId, req);
//! //   });
//! ```

use perry_ffi::{alloc_string, get_handle_mut, JsClosure, RawClosureHeader};

use crate::request::handle_to_pointer_f64;
use crate::server::HttpServer;
use crate::types::{js_promise_run_microtasks, POINTER_TAG, PTR_MASK, STRING_TAG, TAG_UNDEFINED};

/// Test whether a request looks like a WebSocket upgrade — checks
/// `Connection: Upgrade` (case-insensitive contains) and
/// `Upgrade: websocket` (case-insensitive). Hyper's `headers()`
/// already lowercases names, so we only normalize values.
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

/// Fire the `'upgrade'` event listeners with `(im, wsId, head)`.
/// Called from the main-thread event loop after the upgrade pending
/// has been dispatched.
pub(crate) fn fire_upgrade_listeners(
    server_handle: i64,
    im_handle: i64,
    ws_id: i64,
    head_data: Vec<u8>,
) {
    let listeners = if let Some(s) = get_handle_mut::<HttpServer>(server_handle) {
        s.listeners.get("upgrade").cloned().unwrap_or_default()
    } else {
        return;
    };
    if listeners.is_empty() {
        return;
    }

    let req_f64 = handle_to_pointer_f64(im_handle);
    // Encode ws_id as NaN-boxed POINTER_TAG so `unbox_to_i64` (the
    // codegen helper used at every NATIVE_MODULE_TABLE receiver
    // call site — `wsId.send(...)` / `wsId.on(...)`) extracts the
    // low-48 bits as the original ws_id. A plain `ws_id as f64`
    // (1.0_f64) would have bits 0x3FF0_…, which `unbox_to_i64`
    // AND-masks to 0, missing the WS_CONNECTIONS lookup entirely.
    let ws_id_f64 = f64::from_bits(POINTER_TAG | (ws_id as u64 & PTR_MASK));
    let head_str = if head_data.is_empty() {
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
                let _ = closure.call3(req_f64, ws_id_f64, head_str);
            }
            js_promise_run_microtasks();
        }
    }
}

#[allow(dead_code)]
// The `& 0` is deliberate: the value is the canonical null-pointer NaN-box
// (POINTER_TAG with an all-zero payload), spelled out so both constants stay
// referenced by this linker anchor.
#[allow(clippy::erasing_op)]
fn _force_link() -> u64 {
    POINTER_TAG | (PTR_MASK & 0)
}
