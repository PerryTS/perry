#[cfg(any(feature = "crypto", feature = "http-client"))]
use super::super::handle::with_handle;
#[cfg(feature = "external-zlib-pump")]
use super::nanbox_handle_value;
use crate::common::feature_hooks::{Hook, PropertyArm};

// One slot per optional-feature position in `js_handle_property_dispatch`, in
// hub order; see `method_dispatch.rs` for the scheme.
static PROP_TLS: Hook<PropertyArm> = Hook::empty();
static PROP_STREAMS: Hook<PropertyArm> = Hook::empty();
static PROP_ZLIB: Hook<PropertyArm> = Hook::empty();
static PROP_EXTERNAL_ZLIB: Hook<PropertyArm> = Hook::empty();
static PROP_HTTP_AGENT: Hook<PropertyArm> = Hook::empty();
static PROP_SQLITE: Hook<PropertyArm> = Hook::empty();
static PROP_HTTP_SERVER: Hook<PropertyArm> = Hook::empty();
static PROP_HTTP_CLIENT: Hook<PropertyArm> = Hook::empty();
static PROP_FETCH: Hook<PropertyArm> = Hook::empty();
static PROP_CRYPTO: Hook<PropertyArm> = Hook::empty();

/// Dispatch a property access on a handle-based object.
#[no_mangle]
pub unsafe extern "C" fn js_handle_property_dispatch(
    handle: i64,
    property_name_ptr: *const u8,
    property_name_len: usize,
) -> f64 {
    let property_name = if property_name_ptr.is_null() || property_name_len == 0 {
        ""
    } else {
        std::str::from_utf8(std::slice::from_raw_parts(
            property_name_ptr,
            property_name_len,
        ))
        .unwrap_or("")
    };
    let _ = property_name;
    let _ = handle;

    try_arm!(PROP_TLS, handle, property_name);

    try_arm!(PROP_STREAMS, handle, property_name);

    // #9324: `WebSocketServer.clients` on the dynamic path is served by
    // perry-ext-ws's own handle-property extension (`dispatch.rs`); the
    // bundled `ws` arm that sat here was deleted in tokio lane L4.
    if let Some(value) =
        super::super::net_socket_bridge::bind_net_socket_property(handle, property_name)
    {
        return value;
    }

    try_arm!(PROP_ZLIB, handle, property_name);

    try_arm!(PROP_EXTERNAL_ZLIB, handle, property_name);

    try_arm!(PROP_HTTP_AGENT, handle, property_name);

    if let Some(v) = crate::common::net_method_values::dispatch_property(handle, property_name) {
        return v;
    }

    try_arm!(PROP_SQLITE, handle, property_name);

    try_arm!(PROP_HTTP_SERVER, handle, property_name);

    try_arm!(PROP_HTTP_CLIENT, handle, property_name);

    try_arm!(PROP_FETCH, handle, property_name);

    // Issue #848: StringDecoder reads — state getters `lastNeed` /
    // `lastTotal` / `lastChar`, the canonical `encoding` property,
    // and the method-as-value reads `write` /
    // `end` (the latter return a bound-method closure so
    // `typeof dec.write === "function"` and `const w = dec.write; w(buf)`
    // both work; see `dispatch_string_decoder_property`). Same disjoint-
    // property gate as the method-dispatch arm above.
    if matches!(
        property_name,
        "lastNeed"
            | "lastTotal"
            | "lastChar"
            | "encoding"
            | "constructor"
            | "write"
            | "end"
            | "text"
    ) && crate::string_decoder::is_string_decoder_handle(handle)
    {
        return crate::string_decoder::dispatch_string_decoder_property(handle, property_name);
    }

    try_arm!(PROP_CRYPTO, handle, property_name);

    // Generic per-handle expando read: an arbitrary user-assigned own property
    // (`handle.colors = [...]`) stored by the set-dispatch fallback below. This
    // is the read half that makes native HANDLE values (Blob / fetch Response /
    // Web-Streams readers) freely extensible like Node's, so the `debug`
    // package's `createDebug.colors[...]` reads back the array it assigned
    // instead of `undefined`. Specific typed properties were all tried above, so
    // a hit here is always a genuine user expando.
    if let Some(v) =
        perry_runtime::object::handle_expando::handle_expando_get(handle, property_name)
    {
        return v;
    }

    // Unknown handle type - return undefined
    f64::from_bits(0x7FFC_0000_0000_0001)
}

#[cfg(all(
    feature = "tls-runtime",
    not(target_os = "ios"),
    not(target_os = "android")
))]
unsafe fn prop_tls(handle: i64, property_name: &str) -> Option<f64> {
    if let Some(value) = crate::tls::dispatch_tls_property(handle, property_name) {
        return Some(value);
    }
    None
}

#[cfg(feature = "bundled-streams")]
unsafe fn prop_streams(handle: i64, property_name: &str) -> Option<f64> {
    // #1670: Web Streams handle property reads. A numeric stream id reaches
    // here via `js_object_get_field_by_name`'s stream probe (inline
    // `res.body.locked`). Route getter properties to their accessors, return
    // a bound-method closure for callable members, and undefined for anything
    // else — never a deref of the float id as a pointer. Gated on stream
    // id-range + registry membership so unrelated small-handle reads are
    // untouched.
    if (crate::streams::STREAM_HANDLE_ID_START..crate::streams::STREAM_HANDLE_ID_END)
        .contains(&(handle as usize))
        && crate::streams::js_stream_handle_is_registered(handle as usize)
    {
        return Some(crate::streams::dispatch_stream_property(
            handle as f64,
            property_name,
        ));
    }
    None
}

#[cfg(feature = "compression-gzip")]
unsafe fn prop_zlib(handle: i64, property_name: &str) -> Option<f64> {
    // zlib Transform streams: `typeof createGzip().write` must read
    // "function". The actual call dispatch is HANDLE_METHOD_DISPATCH
    // (above), but feature-checks read through the property table — we
    // bind a closure here so the typeof short-circuit sees "function".
    if crate::zlib::is_zlib_stream_handle(handle) {
        if property_name == "bytesWritten" {
            return Some(crate::zlib::zlib_stream_bytes_written(handle));
        }
        let method: Option<&'static [u8]> = match property_name {
            "write" => Some(b"write"),
            "end" => Some(b"end"),
            "on" => Some(b"on"),
            "once" => Some(b"once"),
            "emit" => Some(b"emit"),
            "pipe" => Some(b"pipe"),
            "flush" => Some(b"flush"),
            "close" => Some(b"close"),
            "destroy" => Some(b"destroy"),
            "params" => Some(b"params"),
            "reset" => Some(b"reset"),
            "removeListener" => Some(b"removeListener"),
            "removeAllListeners" => Some(b"removeAllListeners"),
            _ => None,
        };
        if let Some(name_bytes) = method {
            extern "C" {
                fn js_class_method_bind(
                    instance: f64,
                    method_name_ptr: *const u8,
                    method_name_len: usize,
                ) -> f64;
            }
            return Some(js_class_method_bind(
                f64::from_bits(handle as u64),
                name_bytes.as_ptr(),
                name_bytes.len(),
            ));
        }
    }
    None
}

#[cfg(feature = "external-zlib-pump")]
unsafe fn prop_external_zlib(handle: i64, property_name: &str) -> Option<f64> {
    {
        extern "C" {
            fn js_ext_zlib_is_stream_handle(handle: i64) -> i32;
            fn js_ext_zlib_stream_bytes_written(handle: i64) -> f64;
            fn js_ext_zlib_stream_property(handle: i64, which: i32) -> f64;
            fn js_class_method_bind(
                instance: f64,
                method_name_ptr: *const u8,
                method_name_len: usize,
            ) -> f64;
        }

        if js_ext_zlib_is_stream_handle(handle) != 0 {
            if property_name == "bytesWritten" {
                return Some(js_ext_zlib_stream_bytes_written(handle));
            }
            let property = match property_name {
                "readableLength" => Some(0),
                "readableHighWaterMark" => Some(1),
                "writableLength" => Some(2),
                "writableHighWaterMark" => Some(3),
                "destroyed" => Some(4),
                "readableEnded" => Some(5),
                "writableFinished" => Some(6),
                _ => None,
            };
            if let Some(which) = property {
                return Some(js_ext_zlib_stream_property(handle, which));
            }
            let method: Option<&'static [u8]> = match property_name {
                "write" => Some(b"write"),
                "end" => Some(b"end"),
                "on" => Some(b"on"),
                "once" => Some(b"once"),
                "addListener" => Some(b"addListener"),
                "pipe" => Some(b"pipe"),
                "iterator" => Some(b"iterator"),
                "@@asyncIterator" => Some(b"@@asyncIterator"),
                "flush" => Some(b"flush"),
                "close" => Some(b"close"),
                "destroy" => Some(b"destroy"),
                "params" => Some(b"params"),
                "reset" => Some(b"reset"),
                "pause" => Some(b"pause"),
                "resume" => Some(b"resume"),
                "off" => Some(b"off"),
                "removeListener" => Some(b"removeListener"),
                _ => None,
            };
            if let Some(name_bytes) = method {
                return Some(js_class_method_bind(
                    nanbox_handle_value(handle),
                    name_bytes.as_ptr(),
                    name_bytes.len(),
                ));
            }
        }
    }
    None
}

#[cfg(feature = "external-http-client-pump")]
unsafe fn prop_http_agent(handle: i64, property_name: &str) -> Option<f64> {
    {
        extern "C" {
            fn js_ext_http_agent_is_handle(handle: i64) -> i32;
            fn js_ext_http_agent_dispatch_property(
                handle: i64,
                property_ptr: *const u8,
                property_len: usize,
            ) -> f64;
        }

        if matches!(
            property_name,
            "createConnection"
                | "createSocket"
                | "keepSocketAlive"
                | "reuseSocket"
                | "getName"
                | "destroy"
                | "maxSockets"
                | "maxFreeSockets"
                | "maxTotalSockets"
                | "totalSocketCount"
                | "keepAliveMsecs"
                | "agentKeepAliveTimeoutBuffer"
                | "keepAlive"
                | "destroyed"
                | "defaultPort"
                | "protocol"
                | "sockets"
                | "freeSockets"
                | "requests"
                | "_sessionCache"
        ) && unsafe { js_ext_http_agent_is_handle(handle) } != 0
        {
            return Some(unsafe {
                js_ext_http_agent_dispatch_property(
                    handle,
                    property_name.as_ptr(),
                    property_name.len(),
                )
            });
        }
    }
    None
}

#[cfg(feature = "database-sqlite")]
unsafe fn prop_sqlite(handle: i64, property_name: &str) -> Option<f64> {
    {
        if let Some(v) =
            crate::sqlite::dispatch_node_sqlite_database_property(handle, property_name)
        {
            return Some(v);
        }
        if let Some(v) =
            crate::sqlite::dispatch_node_sqlite_tag_store_property(handle, property_name)
        {
            return Some(v);
        }
        if let Some(v) =
            crate::sqlite::dispatch_node_sqlite_statement_property(handle, property_name)
        {
            return Some(v);
        }
        if let Some(v) = crate::sqlite::dispatch_node_sqlite_limits_property(handle, property_name)
        {
            return Some(v);
        }
        if let Some(v) = crate::sqlite::dispatch_node_sqlite_session_property(handle, property_name)
        {
            return Some(v);
        }
    }
    None
}

#[cfg(feature = "external-http-server-pump")]
unsafe fn prop_http_server(handle: i64, property_name: &str) -> Option<f64> {
    // Server-side node:http request/response handles whose static
    // `HttpServer` / `IncomingMessage` / `ServerResponse` type was lost.
    {
        extern "C" {
            fn js_ext_http_server_is_handle(handle: i64) -> i32;
            fn js_ext_http_incoming_message_is_handle(handle: i64) -> i32;
            fn js_ext_http_server_response_is_handle(handle: i64) -> i32;
            fn js_ext_http2_session_is_handle(handle: i64) -> i32;
            fn js_ext_http2_stream_is_handle(handle: i64) -> i32;
            fn js_ext_http_server_dispatch_property(
                handle: i64,
                property_ptr: *const u8,
                property_len: usize,
            ) -> f64;
            fn js_ext_http_incoming_message_dispatch_property(
                handle: i64,
                property_ptr: *const u8,
                property_len: usize,
            ) -> f64;
            fn js_ext_http_server_response_dispatch_property(
                handle: i64,
                property_ptr: *const u8,
                property_len: usize,
            ) -> f64;
            fn js_ext_http2_session_dispatch_property(
                handle: i64,
                property_ptr: *const u8,
                property_len: usize,
            ) -> f64;
            fn js_ext_http2_stream_dispatch_property(
                handle: i64,
                property_ptr: *const u8,
                property_len: usize,
            ) -> f64;
        }

        if matches!(
            property_name,
            "listen"
                | "close"
                | "closeAllConnections"
                | "closeIdleConnections"
                | "address"
                | "on"
                | "addListener"
                | "setTimeout"
                | "@@__perry_wk_asyncDispose"
                | "@@kConnectionsCheckingInterval"
                | "listening"
                | "headersTimeout"
                | "keepAliveTimeout"
                | "keepAliveTimeoutBuffer"
                | "requestTimeout"
                | "timeout"
                | "maxHeadersCount"
                | "maxRequestsPerSocket"
        ) && unsafe { js_ext_http_server_is_handle(handle) } != 0
        {
            return Some(unsafe {
                js_ext_http_server_dispatch_property(
                    handle,
                    property_name.as_ptr(),
                    property_name.len(),
                )
            });
        }

        if matches!(
            property_name,
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
                | "socket"
                | "connection"
                | "signal"
                | "remoteAddress"
                | "remotePort"
                | "on"
                | "addListener"
                | "setEncoding"
                | "setTimeout"
                | "pause"
                | "resume"
                | "destroy"
                | "read"
                | "constructor"
        ) && unsafe { js_ext_http_incoming_message_is_handle(handle) } != 0
        {
            return Some(unsafe {
                js_ext_http_incoming_message_dispatch_property(
                    handle,
                    property_name.as_ptr(),
                    property_name.len(),
                )
            });
        }

        if matches!(
            property_name,
            "statusCode"
                | "statusMessage"
                | "headersSent"
                | "writableEnded"
                | "writableFinished"
                | "finished"
                | "destroyed"
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
                | "setHeader"
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
                | "on"
                | "addListener"
                | "constructor"
        ) && unsafe { js_ext_http_server_response_is_handle(handle) } != 0
        {
            return Some(unsafe {
                js_ext_http_server_response_dispatch_property(
                    handle,
                    property_name.as_ptr(),
                    property_name.len(),
                )
            });
        }

        if matches!(
            property_name,
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
        ) && unsafe { js_ext_http2_session_is_handle(handle) } != 0
        {
            return Some(unsafe {
                js_ext_http2_session_dispatch_property(
                    handle,
                    property_name.as_ptr(),
                    property_name.len(),
                )
            });
        }

        if matches!(
            property_name,
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
        ) && unsafe { js_ext_http2_stream_is_handle(handle) } != 0
        {
            return Some(unsafe {
                js_ext_http2_stream_dispatch_property(
                    handle,
                    property_name.as_ptr(),
                    property_name.len(),
                )
            });
        }
    }
    None
}

#[cfg(feature = "external-http-client-pump")]
unsafe fn prop_http_client(handle: i64, property_name: &str) -> Option<f64> {
    if let Some(value) = unsafe {
        super::super::dispatch_http::dispatch_client_request_property(handle, property_name)
    } {
        return Some(value);
    }

    if let Some(value) = unsafe {
        super::super::dispatch_http::dispatch_client_incoming_property(handle, property_name)
    } {
        return Some(value);
    }
    None
}

#[cfg(feature = "web-fetch")]
unsafe fn prop_fetch(handle: i64, property_name: &str) -> Option<f64> {
    // Web Fetch property dispatch (refs #421 — Phase 1 of the handle-NaN-boxing
    // unification). When user code accesses a property on a Request / Response /
    // Headers / Blob handle in untyped position (`(r) => r.url` where the static
    // type is `any` — typical of npm packages whose TS sources have been
    // type-stripped, like hono's compiled JS), codegen falls through to
    // `js_object_get_field_by_name` which strips POINTER_TAG and routes here.
    // Each helper does its own registry-membership check; the order matches the
    // observed property-name disjointness (`url` / `method` only on Request,
    // `status` / `ok` only on Response, etc.). First match wins.
    // Gated on `web-fetch` because fetch.rs itself is gated on that feature (#5174).
    {
        if let Some(v) = crate::fetch::dispatch_request_property(handle as usize, property_name) {
            return Some(v);
        }
        if let Some(v) = crate::fetch::dispatch_response_property(handle as usize, property_name) {
            return Some(v);
        }
        if let Some(v) = crate::fetch::dispatch_headers_property(handle as usize, property_name) {
            return Some(v);
        }
        if let Some(v) = crate::fetch::dispatch_form_data_property(handle as usize, property_name) {
            return Some(v);
        }
        if let Some(v) = crate::fetch::dispatch_blob_property(handle as usize, property_name) {
            return Some(v);
        }
    }
    None
}

#[cfg(feature = "crypto")]
unsafe fn prop_crypto(handle: i64, property_name: &str) -> Option<f64> {
    if matches!(property_name, "update" | "sign")
        && with_handle::<crate::crypto::SignHandle, bool, _>(handle, |_| true).unwrap_or(false)
    {
        return Some(crate::crypto::dispatch_sign_property(handle, property_name));
    }

    if matches!(property_name, "update" | "verify")
        && with_handle::<crate::crypto::VerifyHandle, bool, _>(handle, |_| true).unwrap_or(false)
    {
        return Some(crate::crypto::dispatch_verify_property(
            handle,
            property_name,
        ));
    }

    if matches!(
        property_name,
        "generateKeys"
            | "getPublicKey"
            | "getPrivateKey"
            | "setPrivateKey"
            | "setPublicKey"
            | "computeSecret"
    ) && with_handle::<crate::crypto::EcdhHandle, bool, _>(handle, |_| true).unwrap_or(false)
    {
        return Some(crate::crypto::dispatch_ecdh_property(handle, property_name));
    }

    if matches!(
        property_name,
        "generateKeys"
            | "computeSecret"
            | "getPrime"
            | "getGenerator"
            | "getPublicKey"
            | "getPrivateKey"
            | "setPublicKey"
            | "setPrivateKey"
            | "verifyError"
    ) && with_handle::<crate::crypto::DiffieHellmanHandle, bool, _>(handle, |_| true)
        .unwrap_or(false)
    {
        return Some(crate::crypto::dispatch_diffie_hellman_property(
            handle,
            property_name,
        ));
    }

    // #1367/#2563: X509Certificate data properties plus bound conversion
    // methods.
    if matches!(
        property_name,
        "subject"
            | "issuer"
            | "validFrom"
            | "validFromDate"
            | "validTo"
            | "validToDate"
            | "serialNumber"
            | "signatureAlgorithm"
            | "signatureAlgorithmOid"
            | "fingerprint"
            | "fingerprint256"
            | "fingerprint512"
            | "subjectAltName"
            | "keyUsage"
            | "infoAccess"
            | "ca"
            | "raw"
            | "publicKey"
            | "issuerCertificate"
            | "toString"
            | "toJSON"
            | "toLegacyObject"
            | "checkHost"
            | "checkEmail"
            | "checkIP"
            | "verify"
            | "checkPrivateKey"
            | "checkIssued"
    ) && with_handle::<crate::crypto::X509Handle, bool, _>(handle, |_| true).unwrap_or(false)
    {
        return Some(crate::crypto::dispatch_x509_property(handle, property_name));
    }

    None
}

// Per-feature slot fills, called from the owning feature's install.
#[cfg(all(
    feature = "tls-runtime",
    not(target_os = "ios"),
    not(target_os = "android")
))]
pub(super) fn install_tls() {
    PROP_TLS.set(prop_tls);
}
#[cfg(feature = "bundled-streams")]
pub(super) fn install_streams() {
    PROP_STREAMS.set(prop_streams);
}
#[cfg(feature = "compression-gzip")]
pub(super) fn install_zlib() {
    PROP_ZLIB.set(prop_zlib);
}
#[cfg(feature = "external-zlib-pump")]
pub(super) fn install_external_zlib() {
    PROP_EXTERNAL_ZLIB.set(prop_external_zlib);
}
#[cfg(feature = "external-http-client-pump")]
pub(super) fn install_external_http_client() {
    PROP_HTTP_AGENT.set(prop_http_agent);
    PROP_HTTP_CLIENT.set(prop_http_client);
}
#[cfg(feature = "database-sqlite")]
pub(super) fn install_sqlite() {
    PROP_SQLITE.set(prop_sqlite);
}
#[cfg(feature = "external-http-server-pump")]
pub(super) fn install_external_http_server() {
    PROP_HTTP_SERVER.set(prop_http_server);
}
#[cfg(feature = "web-fetch")]
pub(super) fn install_fetch() {
    PROP_FETCH.set(prop_fetch);
}
#[cfg(feature = "crypto")]
pub(super) fn install_crypto() {
    PROP_CRYPTO.set(prop_crypto);
}
