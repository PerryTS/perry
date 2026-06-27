//! Runtime handle-dispatch EXTENSION registration for HTTP-server handles.
//!
//! ## Why this exists
//!
//! perry-stdlib's `js_handle_property_dispatch` / `js_handle_method_dispatch`
//! (and the property-SET twin) carry the HTTP-server handle arms behind the
//! `external-http-server-pump` Cargo feature (see
//! `perry-stdlib/src/common/dispatch/{property,method}_dispatch.rs`). That
//! feature is only compiled in when the *workspace* auto-optimize rebuilds
//! perry-stdlib for a program that imports `node:http`.
//!
//! An out-of-tree install — and `PERRY_NO_AUTO_OPTIMIZE=1` — instead links the
//! prebuilt `full` `libperry_stdlib.a`, which is built **without**
//! `external-http-server-pump`. In that build those dispatch arms are compiled
//! OUT, so an erased-receiver `req.url` / `res.end(...)` (the handler params are
//! `any`, so codegen emits a generic property-get / method-call that routes
//! through the runtime's `HANDLE_*_DISPATCH` slow path) finds no HTTP-server arm
//! and silently reads `undefined` / no-ops. The server binds and the handler
//! fires (the pump is already kept alive out-of-tree via the
//! `js_register_aux_pump` mechanism, #2532), but the request object is empty and
//! the response never flushes — the Wall-10 symptom.
//!
//! ## The fix
//!
//! perry-runtime exposes `js_register_handle_{property,method,property_set}_dispatch_extension`
//! (the same mechanism perry-ext-net uses, see `perry-ext-net/src/dispatch.rs`).
//! Registered extensions are consulted by the runtime's composite dispatcher
//! *before* the stdlib primary, regardless of which perry-stdlib features were
//! compiled. We register one extension per dispatch kind here; each probes the
//! handle against our registry and, for a name that is a genuine native member
//! of that handle type, forwards to the existing `js_ext_http_*_dispatch_*`
//! entry points.
//!
//! ## CRITICAL: gate on the native member-name lists
//!
//! A server-side `ServerResponse` / `IncomingMessage` handle is routinely
//! wrapped by a user prototype that ADDS methods — Express augments the response
//! prototype with `res.send` / `res.json` / `res.status` / … and the request
//! with `req.fresh` / `req.accepts` / …. Those methods live on a JS prototype
//! object in the chain above the native handle. If this extension claimed EVERY
//! name once the handle type matched, it would shadow `res.send` (returning
//! `undefined` instead of letting the prototype's real `send` run) and Express's
//! `res.send('...')` would silently no-op — exactly the bug this caused on the
//! first cut. So each name is matched against the SAME `matches!` vocabularies
//! perry-stdlib's gated arms use; an unrecognised name returns "not claimed" (0)
//! so the runtime falls through to the prototype-chain / user-method resolution.
//! Keep these lists in sync with
//! `perry-stdlib/src/common/dispatch/{method,property}_dispatch.rs`.

use std::sync::Once;

extern "C" {
    fn js_register_handle_method_dispatch_extension(
        f: unsafe extern "C" fn(i64, *const u8, usize, *const f64, usize, *mut f64) -> i32,
    );
    fn js_register_handle_property_dispatch_extension(
        f: unsafe extern "C" fn(i64, *const u8, usize, *mut f64) -> i32,
    );
    fn js_register_handle_property_set_dispatch_extension(
        f: unsafe extern "C" fn(i64, *const u8, usize, f64) -> i32,
    );
}

/// Register the three HTTP-server handle-dispatch extensions with perry-runtime.
/// Idempotent (`Once` here + the runtime de-dupes by fn pointer). Called from
/// `ensure_gc_scanner_registered`, so it runs the first time any HTTP/HTTPS/HTTP2
/// server is created — before any request handler can fire.
pub(crate) fn ensure_dispatch_extensions_registered() {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| unsafe {
        js_register_handle_method_dispatch_extension(http_server_method_dispatch_ext);
        js_register_handle_property_dispatch_extension(http_server_property_dispatch_ext);
        js_register_handle_property_set_dispatch_extension(http_server_property_set_dispatch_ext);
    });
}

// ---- native member-name vocabularies (mirror perry-stdlib's gated arms) ----

fn is_http_server_method(name: &str) -> bool {
    matches!(
        name,
        "listen"
            | "close"
            | "address"
            | "on"
            | "addListener"
            | "setTimeout"
            | "closeAllConnections"
            | "closeIdleConnections"
            | "removeAllListeners"
            | "removeListener"
            | "off"
            | "ref"
            | "unref"
            | "@@__perry_wk_asyncDispose"
    )
}

fn is_http_server_property(name: &str) -> bool {
    is_http_server_method(name)
        || matches!(
            name,
            "@@kConnectionsCheckingInterval"
                | "listening"
                | "headersTimeout"
                | "keepAliveTimeout"
                | "keepAliveTimeoutBuffer"
                | "requestTimeout"
                | "timeout"
                | "maxHeadersCount"
                | "maxRequestsPerSocket"
        )
}

fn is_incoming_message_member(name: &str) -> bool {
    matches!(
        name,
        "on" | "addListener"
            | "setEncoding"
            | "setTimeout"
            | "pause"
            | "resume"
            | "destroy"
            | "read"
            | "_addHeaderLine"
            | "__set_socket"
            | "__set_connection"
    ) || matches!(
        name,
        "method"
            | "url"
            | "rawBody"
            | "httpVersion"
            | "httpVersionMajor"
            | "httpVersionMinor"
            | "headers"
            | "rawHeaders"
            | "headersDistinct"
            | "trailers"
            | "rawTrailers"
            | "trailersDistinct"
            | "complete"
            | "aborted"
            | "destroyed"
            | "readable"
            | "readableEnded"
            | "socket"
            | "connection"
            | "signal"
            | "remoteAddress"
            | "remotePort"
    ) || matches!(
        name,
        "__get_method"
            | "__get_url"
            | "__get_httpVersion"
            | "__get_headers"
            | "__get_headersDistinct"
            | "__get_trailers"
            | "__get_rawHeaders"
            | "__get_rawTrailers"
            | "__get_trailersDistinct"
            | "__get_complete"
            | "__get_aborted"
            | "__get_destroyed"
            | "__get_socket"
            | "__get_connection"
            | "__get_signal"
            | "__get_remoteAddress"
            | "__get_remotePort"
            | "constructor"
    )
}

fn is_server_response_member(name: &str) -> bool {
    matches!(
        name,
        "setHeader"
            | "getHeader"
            | "removeHeader"
            | "hasHeader"
            | "getHeaders"
            | "getHeaderNames"
            | "appendHeader"
            | "setHeaders"
            | "writeHead"
            | "write"
            | "addTrailers"
            | "end"
            | "flushHeaders"
            | "cork"
            | "uncork"
            | "destroy"
            | "pipe"
            | "setTimeout"
            | "writeEarlyHints"
            | "writeContinue"
            | "writeProcessing"
            | "assignSocket"
            | "detachSocket"
            | "on"
            | "addListener"
            | "setStatus"
            | "getStatus"
    ) || matches!(
        name,
        "statusCode"
            | "statusMessage"
            | "headersSent"
            | "writableEnded"
            | "writableFinished"
            | "finished"
            | "writableCorked"
            | "writableHighWaterMark"
            | "writableLength"
            | "writableObjectMode"
            | "writableNeedDrain"
            | "sendDate"
            | "strictContentLength"
            | "req"
            | "socket"
            | "connection"
            | "constructor"
    ) || matches!(
        name,
        "__get_statusCode"
            | "__get_statusMessage"
            | "__set_statusCode"
            | "__set_statusMessage"
            | "__get_headersSent"
            | "__get_writableEnded"
            | "__get_writableFinished"
            | "__get_finished"
            | "__get_sendDate"
            | "__set_sendDate"
            | "__get_strictContentLength"
            | "__set_strictContentLength"
            | "__get_req"
            | "__get_socket"
            | "__get_connection"
    )
}

fn is_h2_session_member(name: &str) -> bool {
    matches!(
        name,
        "request"
            | "on"
            | "addListener"
            | "close"
            | "destroy"
            | "ref"
            | "unref"
            | "setTimeout"
            | "setLocalWindowSize"
            | "ping"
            | "settings"
            | "goaway"
            | "type"
            | "encrypted"
            | "connecting"
            | "closed"
            | "destroyed"
            | "alpnProtocol"
            | "pendingSettingsAck"
            | "localSettings"
            | "remoteSettings"
            | "state"
            | "socket"
    )
}

fn is_h2_stream_member(name: &str) -> bool {
    matches!(
        name,
        "on" | "addListener"
            | "setEncoding"
            | "respond"
            | "end"
            | "close"
            | "setTimeout"
            | "priority"
            | "additionalHeaders"
            | "pushStream"
            | "respondWithFD"
            | "respondWithFile"
            | "sendTrailers"
            | "id"
            | "pending"
            | "closed"
            | "destroyed"
            | "aborted"
            | "rstCode"
            | "headersSent"
            | "sentHeaders"
            | "session"
            | "state"
            | "bufferSize"
            | "endAfterHeaders"
    )
}

#[inline]
unsafe fn name_str<'a>(ptr: *const u8, len: usize) -> &'a str {
    if ptr.is_null() || len == 0 {
        ""
    } else {
        std::str::from_utf8(std::slice::from_raw_parts(ptr, len)).unwrap_or("")
    }
}

/// Method-call extension. Claims (returns 1, sets `*out`) ONLY when `handle`
/// belongs to one of our registries AND `method` is a native member of that
/// type. Returns 0 for any other name so user-prototype-augmented methods
/// (Express `res.send`, etc.) resolve through the prototype chain.
unsafe extern "C" fn http_server_method_dispatch_ext(
    handle: i64,
    method_ptr: *const u8,
    method_len: usize,
    args_ptr: *const f64,
    args_len: usize,
    out: *mut f64,
) -> i32 {
    let name = name_str(method_ptr, method_len);
    if name.is_empty() {
        return 0;
    }
    let value = if is_http_server_method(name)
        && crate::handle_dispatch::js_ext_http_server_is_handle(handle) != 0
    {
        Some(crate::handle_dispatch::js_ext_http_server_dispatch_method(
            handle, method_ptr, method_len, args_ptr, args_len,
        ))
    } else if is_incoming_message_member(name)
        && crate::handle_dispatch::js_ext_http_incoming_message_is_handle(handle) != 0
    {
        Some(
            crate::handle_dispatch::js_ext_http_incoming_message_dispatch_method(
                handle, method_ptr, method_len, args_ptr, args_len,
            ),
        )
    } else if is_server_response_member(name)
        && crate::handle_dispatch::js_ext_http_server_response_is_handle(handle) != 0
    {
        Some(
            crate::handle_dispatch::js_ext_http_server_response_dispatch_method(
                handle, method_ptr, method_len, args_ptr, args_len,
            ),
        )
    } else if is_h2_session_member(name)
        && crate::http2_server::dispatch::js_ext_http2_session_is_handle(handle) != 0
    {
        Some(
            crate::http2_server::dispatch::js_ext_http2_session_dispatch_method(
                handle, method_ptr, method_len, args_ptr, args_len,
            ),
        )
    } else if is_h2_stream_member(name)
        && crate::http2_server::dispatch::js_ext_http2_stream_is_handle(handle) != 0
    {
        Some(
            crate::http2_server::dispatch::js_ext_http2_stream_dispatch_method(
                handle, method_ptr, method_len, args_ptr, args_len,
            ),
        )
    } else {
        None
    };
    match value {
        Some(v) => {
            if !out.is_null() {
                *out = v;
            }
            1
        }
        None => 0,
    }
}

/// Property-read extension. Same name-gating discipline as the method path.
unsafe extern "C" fn http_server_property_dispatch_ext(
    handle: i64,
    property_ptr: *const u8,
    property_len: usize,
    out: *mut f64,
) -> i32 {
    let name = name_str(property_ptr, property_len);
    if name.is_empty() {
        return 0;
    }
    let value = if is_http_server_property(name)
        && crate::handle_dispatch::js_ext_http_server_is_handle(handle) != 0
    {
        Some(
            crate::handle_dispatch::js_ext_http_server_dispatch_property(
                handle,
                property_ptr,
                property_len,
            ),
        )
    } else if is_incoming_message_member(name)
        && crate::handle_dispatch::js_ext_http_incoming_message_is_handle(handle) != 0
    {
        Some(
            crate::handle_dispatch::js_ext_http_incoming_message_dispatch_property(
                handle,
                property_ptr,
                property_len,
            ),
        )
    } else if is_server_response_member(name)
        && crate::handle_dispatch::js_ext_http_server_response_is_handle(handle) != 0
    {
        Some(
            crate::handle_dispatch::js_ext_http_server_response_dispatch_property(
                handle,
                property_ptr,
                property_len,
            ),
        )
    } else if is_h2_session_member(name)
        && crate::http2_server::dispatch::js_ext_http2_session_is_handle(handle) != 0
    {
        Some(
            crate::http2_server::dispatch::js_ext_http2_session_dispatch_property(
                handle,
                property_ptr,
                property_len,
            ),
        )
    } else if is_h2_stream_member(name)
        && crate::http2_server::dispatch::js_ext_http2_stream_is_handle(handle) != 0
    {
        Some(
            crate::http2_stream_props::js_ext_http2_stream_dispatch_property(
                handle,
                property_ptr,
                property_len,
            ),
        )
    } else {
        None
    };
    match value {
        Some(v) => {
            if !out.is_null() {
                *out = v;
            }
            1
        }
        None => 0,
    }
}

/// Property-write extension. Claims only the writable NATIVE keys of each handle
/// type — `res.statusCode` / `res.statusMessage` / `res.sendDate` /
/// `res.strictContentLength` / `res.socket` / `res.connection`, and
/// `req.socket` / `req.connection`. Express's `res.status(n)` lowers to
/// `this.statusCode = n`, so the response set MUST route to the native handle;
/// without it Express responses keep status 200 but, more importantly, the set
/// would silently no-op. Every OTHER set (Express stashing `res.locals`,
/// `req.app`, `req.baseUrl`, …) returns 0 so the user expando lands on the
/// object as usual. The underlying dispatch returns 1/0 itself, so an
/// unrecognised key on a matched handle still falls through.
unsafe extern "C" fn http_server_property_set_dispatch_ext(
    handle: i64,
    property_ptr: *const u8,
    property_len: usize,
    value: f64,
) -> i32 {
    let name = name_str(property_ptr, property_len);
    if matches!(
        name,
        "statusCode"
            | "statusMessage"
            | "sendDate"
            | "strictContentLength"
            | "socket"
            | "connection"
    ) && crate::handle_dispatch::js_ext_http_server_response_is_handle(handle) != 0
    {
        return crate::handle_dispatch::js_ext_http_server_response_dispatch_property_set(
            handle,
            property_ptr,
            property_len,
            value,
        );
    }
    if matches!(name, "socket" | "connection")
        && crate::handle_dispatch::js_ext_http_incoming_message_is_handle(handle) != 0
    {
        return crate::handle_dispatch::js_ext_http_incoming_message_dispatch_property_set(
            handle,
            property_ptr,
            property_len,
            value,
        );
    }
    0
}
