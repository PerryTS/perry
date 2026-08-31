//! WebSocket (`ws`) native-dispatch rows. Split out of `net_events.rs`
//! to keep that file under the file-size gate, mirroring the earlier
//! `tls_events.rs` split (#3196-#3200). Assembled into
//! `NATIVE_MODULE_TABLE` by `mod.rs` immediately BEFORE the
//! `NET_EVENTS_ROWS` slice, so dispatch order matches the original
//! layout in which these rows led the table.

use super::*;

pub(super) const WS_EVENTS_ROWS: &[NativeModSig] = &[
    // ========== WebSocket (ws) ==========
    NativeModSig {
        module: "ws",
        has_receiver: false,
        method: "Server",
        class_filter: None,
        runtime: "js_ws_server_new",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "ws",
        has_receiver: false,
        method: "WebSocket",
        class_filter: None,
        runtime: "js_ws_connect",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "ws",
        has_receiver: true,
        method: "on",
        class_filter: None,
        runtime: "js_ws_on",
        args: &[NA_STR, NA_PTR],
        ret: NR_I32,
    },
    NativeModSig {
        module: "ws",
        has_receiver: true,
        method: "send",
        class_filter: None,
        runtime: "js_ws_send",
        args: &[NA_STR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "ws",
        has_receiver: true,
        method: "close",
        class_filter: None,
        runtime: "js_ws_close",
        args: &[],
        ret: NR_VOID,
    },
    // #6117 — `ws.readyState` data getter (CONNECTING=0 / OPEN=1 /
    // CLOSING=2 / CLOSED=3). Reached as a 0-arg NativeMethodCall via the
    // bare-member-read reroute in perry-hir's `is_native_dispatch_member`.
    NativeModSig {
        module: "ws",
        has_receiver: true,
        method: "readyState",
        class_filter: None,
        runtime: "js_ws_ready_state",
        args: &[],
        ret: NR_F64,
    },
    // #9325 — `WebSocketServer.clients` is a persistent JS Set. HIR only
    // routes this getter for the WebSocketServer/Server classes, so the table
    // can use the same generic ws receiver convention as `readyState`.
    NativeModSig {
        module: "ws",
        has_receiver: true,
        method: "clients",
        class_filter: None,
        runtime: "js_ws_server_clients",
        args: &[],
        ret: NR_F64,
    },
    // Issue #577 Phase 4 — `("ws", "Client")` instance methods.
    // The wsId delivered to `Server.on('upgrade', (req, wsId, head) => …)`
    // is NaN-boxed POINTER_TAG so unbox_to_i64 (called by the dispatch
    // helper) extracts the original integer ws_id; user code writing
    // `wsId.send("…")` / `wsId.on("message", cb)` / `wsId.close()`
    // dispatches via these class-filtered entries to the dedicated
    // i64-taking Client variants.
    NativeModSig {
        module: "ws",
        has_receiver: true,
        method: "send",
        class_filter: Some("Client"),
        runtime: "js_ws_send_client_i64",
        args: &[NA_STR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "ws",
        has_receiver: true,
        method: "close",
        class_filter: Some("Client"),
        runtime: "js_ws_close_client_i64",
        args: &[],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "ws",
        has_receiver: true,
        method: "on",
        class_filter: Some("Client"),
        runtime: "js_ws_on_client_i64",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "ws",
        has_receiver: true,
        method: "addListener",
        class_filter: Some("Client"),
        runtime: "js_ws_on_client_i64",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    // Server-side helpers — the user receives a client handle as a plain
    // f64 number from `wss.on('connection', (handle) => …)`, then passes
    // it back to these free functions to write/close that specific peer.
    // Without these entries the receiver-less call falls through to the
    // silent stub a few hundred lines down, evaluates the args for side
    // effects, and returns TAG_UNDEFINED — so frames silently never ship
    // (issue #136).
    NativeModSig {
        module: "ws",
        has_receiver: false,
        method: "sendToClient",
        class_filter: None,
        runtime: "js_ws_send_to_client",
        args: &[NA_F64, NA_STR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "ws",
        has_receiver: false,
        method: "closeClient",
        class_filter: None,
        runtime: "js_ws_close_client",
        args: &[NA_F64],
        ret: NR_VOID,
    },
];
