use super::super::handle::*;
use super::*;

/// Dispatch a property access on a handle-based object.
#[no_mangle]
pub unsafe extern "C" fn js_handle_property_dispatch(
    handle: i64,
    property_name_ptr: *const u8,
    property_name_len: usize,
) -> f64 {
    #[cfg(feature = "external-fastify-pump")]
    use perry_runtime::JSValue;

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

    if let Some(v) = crate::domain::dispatch_domain_property(handle, property_name) {
        return v;
    }

    #[cfg(any(feature = "bundled-events", feature = "external-events-construct"))]
    if let Some(value) = dispatch_event_emitter_property(handle, property_name) {
        return value;
    }

    if let Some(value) = dispatch_async_local_storage_property(handle, property_name) {
        return value;
    }

    #[cfg(feature = "http-client")]
    if let Some(value) = crate::http::dispatch_agent_property(handle, property_name) {
        return value;
    }

    #[cfg(feature = "http-client")]
    if let Some(value) = crate::http::dispatch_client_request_property(handle, property_name) {
        return value;
    }

    #[cfg(all(feature = "tls", not(target_os = "ios"), not(target_os = "android")))]
    if let Some(value) = crate::tls::dispatch_tls_property(handle, property_name) {
        return value;
    }

    // #1670: Web Streams handle property reads. A numeric stream id reaches
    // here via `js_object_get_field_by_name`'s stream probe (inline
    // `res.body.locked`). Route getter properties to their accessors, return
    // a bound-method closure for callable members, and undefined for anything
    // else — never a deref of the float id as a pointer. Gated on stream
    // id-range + registry membership so unrelated small-handle reads are
    // untouched.
    #[cfg(feature = "bundled-streams")]
    if (crate::streams::STREAM_HANDLE_ID_START..crate::streams::STREAM_HANDLE_ID_END)
        .contains(&(handle as usize))
        && crate::streams::js_stream_handle_is_registered(handle as usize)
    {
        return crate::streams::dispatch_stream_property(handle as f64, property_name);
    }

    if let Some(value) =
        super::super::net_socket_bridge::bind_net_socket_property(handle, property_name)
    {
        return value;
    }

    // zlib Transform streams: `typeof createGzip().write` must read
    // "function". The actual call dispatch is HANDLE_METHOD_DISPATCH
    // (above), but feature-checks read through the property table — we
    // bind a closure here so the typeof short-circuit sees "function".
    #[cfg(feature = "compression")]
    if crate::zlib::is_zlib_stream_handle(handle) {
        if property_name == "bytesWritten" {
            return crate::zlib::zlib_stream_bytes_written(handle);
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
            return js_class_method_bind(
                f64::from_bits(handle as u64),
                name_bytes.as_ptr(),
                name_bytes.len(),
            );
        }
    }

    #[cfg(feature = "external-zlib-pump")]
    {
        extern "C" {
            fn js_ext_zlib_is_stream_handle(handle: i64) -> i32;
            fn js_ext_zlib_stream_bytes_written(handle: i64) -> f64;
            fn js_class_method_bind(
                instance: f64,
                method_name_ptr: *const u8,
                method_name_len: usize,
            ) -> f64;
        }

        if js_ext_zlib_is_stream_handle(handle) != 0 {
            if property_name == "bytesWritten" {
                return js_ext_zlib_stream_bytes_written(handle);
            }
            let method: Option<&'static [u8]> = match property_name {
                "write" => Some(b"write"),
                "end" => Some(b"end"),
                "on" => Some(b"on"),
                "once" => Some(b"once"),
                "addListener" => Some(b"addListener"),
                "pipe" => Some(b"pipe"),
                "flush" => Some(b"flush"),
                "close" => Some(b"close"),
                "destroy" => Some(b"destroy"),
                "params" => Some(b"params"),
                "reset" => Some(b"reset"),
                _ => None,
            };
            if let Some(name_bytes) = method {
                return js_class_method_bind(
                    f64::from_bits(handle as u64),
                    name_bytes.as_ptr(),
                    name_bytes.len(),
                );
            }
        }
    }

    #[cfg(feature = "external-http-client-pump")]
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
                | "keepAliveMsecs"
                | "keepAlive"
                | "destroyed"
                | "defaultPort"
                | "protocol"
                | "sockets"
                | "freeSockets"
                | "requests"
        ) && unsafe { js_ext_http_agent_is_handle(handle) } != 0
        {
            return unsafe {
                js_ext_http_agent_dispatch_property(
                    handle,
                    property_name.as_ptr(),
                    property_name.len(),
                )
            };
        }
    }

    if let Some(v) = crate::common::net_method_values::dispatch_property(handle, property_name) {
        return v;
    }

    #[cfg(feature = "database-sqlite")]
    {
        if let Some(v) =
            crate::sqlite::dispatch_node_sqlite_database_property(handle, property_name)
        {
            return v;
        }
        if let Some(v) =
            crate::sqlite::dispatch_node_sqlite_tag_store_property(handle, property_name)
        {
            return v;
        }
        if let Some(v) =
            crate::sqlite::dispatch_node_sqlite_statement_property(handle, property_name)
        {
            return v;
        }
        if let Some(v) = crate::sqlite::dispatch_node_sqlite_limits_property(handle, property_name)
        {
            return v;
        }
        if let Some(v) = crate::sqlite::dispatch_node_sqlite_session_property(handle, property_name)
        {
            return v;
        }
    }

    // Server-side node:http request/response handles whose static
    // `HttpServer` / `IncomingMessage` / `ServerResponse` type was lost.
    #[cfg(feature = "external-http-server-pump")]
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
            return unsafe {
                js_ext_http_server_dispatch_property(
                    handle,
                    property_name.as_ptr(),
                    property_name.len(),
                )
            };
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
            return unsafe {
                js_ext_http_incoming_message_dispatch_property(
                    handle,
                    property_name.as_ptr(),
                    property_name.len(),
                )
            };
        }

        if matches!(
            property_name,
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
            return unsafe {
                js_ext_http_server_response_dispatch_property(
                    handle,
                    property_name.as_ptr(),
                    property_name.len(),
                )
            };
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
            return unsafe {
                js_ext_http2_session_dispatch_property(
                    handle,
                    property_name.as_ptr(),
                    property_name.len(),
                )
            };
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
            return unsafe {
                js_ext_http2_stream_dispatch_property(
                    handle,
                    property_name.as_ptr(),
                    property_name.len(),
                )
            };
        }
    }

    // Fastify request/reply property dispatch (#5037, #1113). fastify is served
    // exclusively by the external perry-ext-fastify crate (the bundled in-stdlib
    // adapter was removed). A `request`/`reply` handle that escaped into a user
    // helper has its static type erased, so codegen emits a generic dynamic
    // property read here rather than a `NativeMethodCall`. The handle lives in
    // perry-ext-fastify's perry-ffi registry, so probe membership via the external
    // `js_ext_fastify_is_context_handle` symbol (resolved at link time) and forward
    // to perry-ext-fastify's `js_fastify_req_*` exports. Enabled by the well-known
    // flip's `external-fastify-pump`, mirroring the `async_bridge.rs` pump.
    // (`app.server` and statically-typed inline reads dispatch via codegen's static
    // NATIVE_MODULE_TABLE; only this erased-receiver dynamic path needs an arm.)
    #[cfg(feature = "external-fastify-pump")]
    {
        extern "C" {
            fn js_ext_fastify_is_context_handle(handle: i64) -> i32;
            fn js_fastify_req_query_object(handle: i64) -> f64;
            fn js_fastify_req_params_object(handle: i64) -> f64;
            fn js_fastify_req_json(handle: i64) -> f64;
            fn js_fastify_req_body(handle: i64) -> *mut perry_runtime::StringHeader;
            fn js_fastify_req_headers(handle: i64) -> i64;
            fn js_fastify_req_method(handle: i64) -> *mut perry_runtime::StringHeader;
            fn js_fastify_req_url(handle: i64) -> *mut perry_runtime::StringHeader;
            fn js_fastify_req_get_user_data(handle: i64) -> f64;
        }
        if js_ext_fastify_is_context_handle(handle) != 0 {
            return match property_name {
                "query" => js_fastify_req_query_object(handle),
                "params" => js_fastify_req_params_object(handle),
                "body" => js_fastify_req_json(handle),
                "rawBody" | "text" => {
                    let ptr = js_fastify_req_body(handle);
                    if ptr.is_null() {
                        f64::from_bits(0x7FFC_0000_0000_0001)
                    } else {
                        f64::from_bits(perry_runtime::JSValue::string_ptr(ptr).bits())
                    }
                }
                "headers" => {
                    // Returns NaN-boxed JS object bits — use directly.
                    let bits = js_fastify_req_headers(handle);
                    f64::from_bits(bits as u64)
                }
                "method" => {
                    let ptr = js_fastify_req_method(handle);
                    if ptr.is_null() {
                        f64::from_bits(0x7FFC_0000_0000_0001)
                    } else {
                        f64::from_bits(perry_runtime::JSValue::string_ptr(ptr).bits())
                    }
                }
                "url" => {
                    let ptr = js_fastify_req_url(handle);
                    if ptr.is_null() {
                        f64::from_bits(0x7FFC_0000_0000_0001)
                    } else {
                        f64::from_bits(perry_runtime::JSValue::string_ptr(ptr).bits())
                    }
                }
                "user" => js_fastify_req_get_user_data(handle),
                _ => f64::from_bits(0x7FFC_0000_0000_0001), // undefined
            };
        }
    }

    // Issue #340: axios response — dispatch `r.status` / `r.data` /
    // `r.statusText` / `r.headers` to the AxiosResponseHandle accessor
    // shims. The handle id is registered in the common HANDLES
    // registry; gate on registry membership AND a known property
    // name so a colliding handle id doesn't silently return one of
    // these slots when the user meant something else (same disjoint
    // method-set discipline as the method dispatch above).
    #[cfg(feature = "http-client")]
    if matches!(property_name, "status" | "data" | "statusText" | "headers") {
        if with_handle::<crate::axios::AxiosResponseHandle, bool, _>(handle, |_| true)
            .unwrap_or(false)
        {
            use perry_runtime::JSValue;
            return match property_name {
                "status" => crate::axios::js_axios_response_status(handle),
                "data" => {
                    let ptr = crate::axios::js_axios_response_data(handle);
                    if ptr.is_null() {
                        f64::from_bits(0x7FFC_0000_0000_0001)
                    } else {
                        f64::from_bits(JSValue::string_ptr(ptr).bits())
                    }
                }
                "statusText" => {
                    let ptr = crate::axios::js_axios_response_status_text(handle);
                    if ptr.is_null() {
                        f64::from_bits(0x7FFC_0000_0000_0001)
                    } else {
                        f64::from_bits(JSValue::string_ptr(ptr).bits())
                    }
                }
                // headers: Vec<(String, String)> — return undefined
                // for now (header object materialisation is its own
                // follow-up; status / data cover the issue).
                _ => f64::from_bits(0x7FFC_0000_0000_0001),
            };
        }
    }

    #[cfg(feature = "external-http-client-pump")]
    if let Some(value) = unsafe {
        super::super::dispatch_http::dispatch_client_request_property(handle, property_name)
    } {
        return value;
    }

    #[cfg(feature = "external-http-client-pump")]
    if let Some(value) = unsafe {
        super::super::dispatch_http::dispatch_client_incoming_property(handle, property_name)
    } {
        return value;
    }

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
    #[cfg(feature = "web-fetch")]
    {
        if let Some(v) = crate::fetch::dispatch_request_property(handle as usize, property_name) {
            return v;
        }
        if let Some(v) = crate::fetch::dispatch_response_property(handle as usize, property_name) {
            return v;
        }
        if let Some(v) = crate::fetch::dispatch_headers_property(handle as usize, property_name) {
            return v;
        }
        if let Some(v) = crate::fetch::dispatch_form_data_property(handle as usize, property_name) {
            return v;
        }
        if let Some(v) = crate::fetch::dispatch_blob_property(handle as usize, property_name) {
            return v;
        }
    }

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

    #[cfg(feature = "crypto")]
    if matches!(
        property_name,
        "update"
            | "digest"
            | "copy"
            | "write"
            | "end"
            | "on"
            | "once"
            | "addListener"
            | "pipe"
            | "setEncoding"
            | "destroy"
            | "close"
    ) && with_handle::<crate::crypto::HashHandle, bool, _>(handle, |_| true).unwrap_or(false)
    {
        return crate::crypto::dispatch_hash_property(handle, property_name);
    }

    #[cfg(feature = "crypto")]
    if matches!(
        property_name,
        "update"
            | "digest"
            | "write"
            | "end"
            | "on"
            | "once"
            | "addListener"
            | "pipe"
            | "setEncoding"
            | "destroy"
            | "close"
    ) && with_handle::<crate::crypto::HmacHandle, bool, _>(handle, |_| true).unwrap_or(false)
    {
        return crate::crypto::dispatch_hmac_property(handle, property_name);
    }

    #[cfg(feature = "crypto")]
    if matches!(property_name, "update" | "sign")
        && with_handle::<crate::crypto::SignHandle, bool, _>(handle, |_| true).unwrap_or(false)
    {
        return crate::crypto::dispatch_sign_property(handle, property_name);
    }

    #[cfg(feature = "crypto")]
    if matches!(property_name, "update" | "verify")
        && with_handle::<crate::crypto::VerifyHandle, bool, _>(handle, |_| true).unwrap_or(false)
    {
        return crate::crypto::dispatch_verify_property(handle, property_name);
    }

    #[cfg(feature = "crypto")]
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
        return crate::crypto::dispatch_ecdh_property(handle, property_name);
    }

    #[cfg(feature = "crypto")]
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
        return crate::crypto::dispatch_diffie_hellman_property(handle, property_name);
    }

    // #1367/#2563: X509Certificate data properties plus bound conversion
    // methods.
    #[cfg(feature = "crypto")]
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
        return crate::crypto::dispatch_x509_property(handle, property_name);
    }

    // Issue #1111: CipherHandle method-as-value reads. Returns a
    // bound-method closure for `update` / `final` / `getAuthTag` /
    // `setAuthTag` / `setAAD` / `setAutoPadding` so `c.getAuthTag?.()` doesn't short-circuit
    // on the optional-chain `c.getAuthTag == null` check. Same disjoint
    // method-name gate as the method-dispatch arm above.
    #[cfg(feature = "crypto")]
    if matches!(
        property_name,
        "update" | "final" | "getAuthTag" | "setAuthTag" | "setAAD" | "setAutoPadding"
    ) && with_handle::<crate::crypto::CipherHandle, bool, _>(handle, |_| true).unwrap_or(false)
    {
        return crate::crypto::dispatch_cipher_property(handle, property_name);
    }

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
