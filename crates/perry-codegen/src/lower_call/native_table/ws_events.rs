//! WebSocket native-dispatch rows: the global `WebSocket` client and the
//! connection handles perry-ext-ws hands out, served by perry-ext-ws. npm
//! `ws` compiles from source, so the `WebSocketServer` rows are gone. The
//! module key is the internal `"__websocket__"` (no JS import surface, like
//! `__disposable__`): HIR tags the global client with it.
//! Split out of `net_events.rs` to keep that file under the file-size gate, mirroring the earlier
//! `tls_events.rs` split (#3196-#3200). Assembled into
//! `NATIVE_MODULE_TABLE` by `mod.rs` immediately BEFORE the
//! `NET_EVENTS_ROWS` slice, so dispatch order matches the original
//! layout in which these rows led the table.

use super::*;

pub(super) const WS_EVENTS_ROWS: &[NativeModSig] = &[
    NativeModSig {
        module: "__websocket__",
        has_receiver: false,
        method: "WebSocket",
        class_filter: None,
        runtime: "js_ws_connect",
        args: &[NA_STR],
        ret: NR_GCPTR,
    },
    NativeModSig {
        module: "__websocket__",
        has_receiver: true,
        method: "on",
        class_filter: None,
        runtime: "js_ws_on",
        args: &[NA_STR, NA_PTR],
        ret: NR_HANDLE_ID,
    },
    // `ws.send(data)` takes the VALUE, not a string: `ws` frames a string as
    // text and anything buffer-shaped as binary, and `NA_STR` could express
    // only the first — a `Buffer` argument could not be sent at all.
    NativeModSig {
        module: "__websocket__",
        has_receiver: true,
        method: "send",
        class_filter: None,
        runtime: "js_ws_send_value",
        args: &[NA_F64, NA_F64],
        ret: NR_VOID,
    },
    // `ws.close([code[, reason]])`. Both arguments used to be dropped on the
    // floor — the signature took none and the frame was always `Close(None)`,
    // so a peer could never observe an application close code. A missing arg
    // is padded with `TAG_UNDEFINED`, which `js_ws_close_with` reads as "no
    // code", so a bare `close()` is unchanged.
    NativeModSig {
        module: "__websocket__",
        has_receiver: true,
        method: "close",
        class_filter: None,
        runtime: "js_ws_close_with",
        args: &[NA_F64, NA_F64],
        ret: NR_VOID,
    },
    // `ws.ping([data])` / `ws.pong([data])` — previously absent entirely.
    NativeModSig {
        module: "__websocket__",
        has_receiver: true,
        method: "ping",
        class_filter: None,
        runtime: "js_ws_ping",
        args: &[NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "__websocket__",
        has_receiver: true,
        method: "pong",
        class_filter: None,
        runtime: "js_ws_pong",
        args: &[NA_F64],
        ret: NR_VOID,
    },
    // `ws.terminate()` — close with no closing handshake.
    NativeModSig {
        module: "__websocket__",
        has_receiver: true,
        method: "terminate",
        class_filter: None,
        runtime: "js_ws_terminate",
        args: &[],
        ret: NR_VOID,
    },
    // #6117 — `ws.readyState` data getter (CONNECTING=0 / OPEN=1 /
    // CLOSING=2 / CLOSED=3). Reached as a 0-arg NativeMethodCall via the
    // bare-member-read reroute in perry-hir's `is_native_dispatch_member`.
    NativeModSig {
        module: "__websocket__",
        has_receiver: true,
        method: "readyState",
        class_filter: None,
        runtime: "js_ws_ready_state",
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
        module: "__websocket__",
        has_receiver: true,
        method: "send",
        class_filter: Some("Client"),
        runtime: "js_ws_send_value_client_i64",
        args: &[NA_F64, NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "__websocket__",
        has_receiver: true,
        method: "close",
        class_filter: Some("Client"),
        runtime: "js_ws_close_with_client_i64",
        args: &[NA_F64, NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "__websocket__",
        has_receiver: true,
        method: "ping",
        class_filter: Some("Client"),
        runtime: "js_ws_ping",
        args: &[NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "__websocket__",
        has_receiver: true,
        method: "pong",
        class_filter: Some("Client"),
        runtime: "js_ws_pong",
        args: &[NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "__websocket__",
        has_receiver: true,
        method: "terminate",
        class_filter: Some("Client"),
        runtime: "js_ws_terminate",
        args: &[],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "__websocket__",
        has_receiver: true,
        method: "on",
        class_filter: Some("Client"),
        runtime: "js_ws_on_client_i64",
        args: &[NA_STR, NA_PTR],
        ret: NR_HANDLE_ID,
    },
    NativeModSig {
        module: "__websocket__",
        has_receiver: true,
        method: "addListener",
        class_filter: Some("Client"),
        runtime: "js_ws_on_client_i64",
        args: &[NA_STR, NA_PTR],
        ret: NR_HANDLE_ID,
    },
];
