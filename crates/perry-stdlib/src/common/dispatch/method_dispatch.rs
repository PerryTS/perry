use super::super::handle::*;
use super::*;

/// Dispatch a method call on a handle-based object.
#[no_mangle]
pub unsafe extern "C" fn js_handle_method_dispatch(
    handle: i64,
    method_name_ptr: *const u8,
    method_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let method_name_owned = if method_name_ptr.is_null() || method_name_len == 0 {
        String::new()
    } else {
        String::from_utf8_lossy(std::slice::from_raw_parts(method_name_ptr, method_name_len))
            .into_owned()
    };
    let method_name = method_name_owned.as_str();
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let original_args: Vec<f64> = if args_len > 0 && !args_ptr.is_null() {
        std::slice::from_raw_parts(args_ptr, args_len).to_vec()
    } else {
        Vec::new()
    };
    let arg_handles = scope.root_nanbox_f64_slice(&original_args);
    let args = perry_runtime::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
    let _ = method_name;
    let _ = args;
    let _ = handle;

    if let Some(v) = crate::domain::dispatch_domain_method(handle, method_name, &args) {
        return v;
    }

    // #1545: Web Streams handles (readable/writable/transform/reader/writer)
    // live in a dedicated high id range, so this never claims another
    // subsystem's handle. Routes method calls on receivers whose static stream
    // type the codegen lost (`src.pipeThrough(ts).getReader()`, `ts.readable
    // .getReader()`, `const r = rs.getReader(); r.read()`, …).
    #[cfg(feature = "bundled-streams")]
    if let Some(v) = crate::streams::dispatch_stream_method(handle as f64, method_name, &args) {
        return v;
    }

    // Dispatchers below gate on registry membership plus method vocabulary
    // because native handle id spaces are not unified (#91).

    #[cfg(any(feature = "bundled-events", feature = "external-events-construct"))]
    if let Some(value) = dispatch_event_emitter_method(handle, method_name, &args) {
        return value;
    }

    if let Some(value) = dispatch_async_local_storage_method(handle, method_name, &args) {
        return value;
    }

    #[cfg(feature = "http-client")]
    if let Some(value) = unsafe { crate::http::dispatch_agent_method(handle, method_name, &args) } {
        return value;
    }

    #[cfg(feature = "external-http-client-pump")]
    {
        extern "C" {
            fn js_ext_http_agent_is_handle(handle: i64) -> i32;
            fn js_ext_http_agent_dispatch_method(
                handle: i64,
                method_ptr: *const u8,
                method_len: usize,
                args_ptr: *const f64,
                args_len: usize,
            ) -> f64;
        }

        if matches!(
            method_name,
            "getName" | "destroy" | "keepSocketAlive" | "reuseSocket"
        ) && js_ext_http_agent_is_handle(handle) != 0
        {
            let args_ptr = if args.is_empty() {
                std::ptr::null()
            } else {
                args.as_ptr()
            };
            return js_ext_http_agent_dispatch_method(
                handle,
                method_name.as_ptr(),
                method_name.len(),
                args_ptr,
                args.len(),
            );
        }
    }

    #[cfg(feature = "http-client")]
    if let Some(value) = crate::http::dispatch_client_request_method(handle, method_name, &args) {
        return value;
    }

    // node:sqlite DatabaseSync handle. Keep this before the better-sqlite3
    // SQLite fallbacks because method names like prepare/exec/close overlap
    // but the lifecycle/error semantics are intentionally different.
    #[cfg(feature = "database-sqlite")]
    if matches!(
        method_name,
        "open"
            | "close"
            | "exec"
            | "prepare"
            | "createTagStore"
            | "createSession"
            | "applyChangeset"
            | "enableLoadExtension"
            | "loadExtension"
            | "location"
            | "__perry_dispose__"
            | "@@__perry_wk_dispose"
    ) {
        if let Some(result) =
            crate::sqlite::dispatch_node_sqlite_database_method(handle, method_name, &args)
        {
            return result;
        }
    }

    // node:sqlite SQLTagStore handle. Keep this before StatementSync because
    // the query execution method names overlap but tag stores consume tagged
    // template arguments and bind them positionally.
    #[cfg(feature = "database-sqlite")]
    if matches!(method_name, "run" | "get" | "all" | "iterate" | "clear") {
        if let Some(result) =
            crate::sqlite::dispatch_node_sqlite_tag_store_method(handle, method_name, &args)
        {
            return result;
        }
    }

    // node:sqlite StatementSync handle. Keep this before the better-sqlite3
    // statement fallback because run/get/all overlap but Node's parameter and
    // result semantics are different.
    #[cfg(feature = "database-sqlite")]
    if matches!(
        method_name,
        "run"
            | "get"
            | "all"
            | "iterate"
            | "columns"
            | "setReadBigInts"
            | "setReturnArrays"
            | "setAllowBareNamedParameters"
            | "setAllowUnknownNamedParameters"
    ) {
        if let Some(result) =
            crate::sqlite::dispatch_node_sqlite_statement_method(handle, method_name, &args)
        {
            return result;
        }
    }

    // node:sqlite Session handle. This follows DatabaseSync dispatch because
    // `close` overlaps and the database lifecycle rules should win for DBs.
    #[cfg(feature = "database-sqlite")]
    if matches!(
        method_name,
        "changeset" | "patchset" | "close" | "__perry_dispose__" | "@@__perry_wk_dispose"
    ) {
        if let Some(result) =
            crate::sqlite::dispatch_node_sqlite_session_method(handle, method_name, &args)
        {
            return result;
        }
    }

    // Fastify app + request/reply context method dispatch lived here when the
    // bundled adapter was compiled into perry-stdlib. fastify now routes entirely
    // through the external perry-ext-fastify crate (well-known flip), whose
    // `app.get(...)` / `reply.send(...)` calls lower via the static
    // NATIVE_MODULE_TABLE rather than this dynamic-handle dispatcher — so no
    // fastify arm is needed here.

    // ioredis client.
    #[cfg(feature = "database-redis")]
    if matches!(
        method_name,
        "connect"
            | "get"
            | "set"
            | "setex"
            | "del"
            | "exists"
            | "incr"
            | "decr"
            | "expire"
            | "ping"
            | "quit"
            | "disconnect"
    ) && with_handle::<crate::ioredis::RedisClient, bool, _>(handle, |_| true).unwrap_or(false)
    {
        return super::super::dispatch_ioredis::dispatch_ioredis(handle, method_name, &args);
    }

    // crypto Hash handle: createHash(...).update(...).digest().
    // The order vs. net (below) does not matter once method-gated, but we
    // keep hash before net to avoid changing the priority of in-registry
    // matches relative to the v0.5.98/#88 ordering.
    #[cfg(feature = "crypto")]
    if matches!(
        method_name,
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
        return crate::crypto::dispatch_hash(handle, method_name, &args);
    }

    // crypto Hmac handle: createHmac(alg, key).update(...).digest(). Routes
    // the runtime path the codegen falls back to whenever `alg` isn't a
    // literal `"sha256"`. See #1076 for the silent-empty bug this closes.
    #[cfg(feature = "crypto")]
    if matches!(
        method_name,
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
        return crate::crypto::dispatch_hmac(handle, method_name, &args);
    }

    #[cfg(feature = "crypto")]
    if matches!(method_name, "update" | "sign")
        && with_handle::<crate::crypto::SignHandle, bool, _>(handle, |_| true).unwrap_or(false)
    {
        return crate::crypto::dispatch_sign(handle, method_name, &args);
    }

    #[cfg(feature = "crypto")]
    if matches!(method_name, "update" | "verify")
        && with_handle::<crate::crypto::VerifyHandle, bool, _>(handle, |_| true).unwrap_or(false)
    {
        return crate::crypto::dispatch_verify(handle, method_name, &args);
    }

    #[cfg(feature = "crypto")]
    if matches!(
        method_name,
        "generateKeys"
            | "getPublicKey"
            | "getPrivateKey"
            | "dhGetPrivateKey"
            | "setPrivateKey"
            | "setPublicKey"
            | "computeSecret"
            | "dhComputeSecret"
    ) && with_handle::<crate::crypto::EcdhHandle, bool, _>(handle, |_| true).unwrap_or(false)
    {
        return crate::crypto::dispatch_ecdh(handle, method_name, &args);
    }

    #[cfg(feature = "crypto")]
    if matches!(
        method_name,
        "generateKeys"
            | "dhGenerateKeys"
            | "computeSecret"
            | "dhComputeSecret"
            | "getPrime"
            | "dhGetPrime"
            | "getGenerator"
            | "dhGetGenerator"
            | "getPublicKey"
            | "dhGetPublicKey"
            | "getPrivateKey"
            | "dhGetPrivateKey"
            | "setPublicKey"
            | "setPrivateKey"
            | "verifyError"
    ) && with_handle::<crate::crypto::DiffieHellmanHandle, bool, _>(handle, |_| true)
        .unwrap_or(false)
    {
        return crate::crypto::dispatch_diffie_hellman(handle, method_name, &args);
    }

    #[cfg(feature = "crypto")]
    if matches!(
        method_name,
        "toString"
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
        return crate::crypto::dispatch_x509_method(handle, method_name, &args);
    }

    // crypto Cipher handle: createCipheriv(...) / createDecipheriv(...)
    // followed by .update(...).final() / .getAuthTag() / .setAuthTag() —
    // issue #1075. Method-gated like the Hash handle above so handle id
    // collisions across registries (net.Socket id=1 vs CipherHandle id=1)
    // don't accidentally route a socket method here.
    #[cfg(feature = "crypto")]
    if matches!(
        method_name,
        "update" | "final" | "getAuthTag" | "setAuthTag" | "setAAD" | "setAutoPadding"
    ) && with_handle::<crate::crypto::CipherHandle, bool, _>(handle, |_| true).unwrap_or(false)
    {
        return crate::crypto::dispatch_cipher(handle, method_name, &args);
    }

    // crypto Sign/Verify handle: createSign(alg)/createVerify(alg) followed by
    // .update(...).sign(key) / .verify(key, sig) — issue #1364. Method-gated
    // like the Hash/Cipher handles. `sign`/`verify` are distinctive enough to
    // disambiguate from other registries sharing a handle id.
    #[cfg(feature = "crypto")]
    if matches!(method_name, "update" | "sign" | "verify")
        && with_handle::<crate::crypto::SignHandle, bool, _>(handle, |_| true).unwrap_or(false)
    {
        return crate::crypto::dispatch_sign(handle, method_name, &args);
    }

    #[cfg(all(feature = "tls", not(target_os = "ios"), not(target_os = "android")))]
    if crate::tls::should_dispatch_tls_handle(handle, method_name) {
        return crate::tls::dispatch_tls_handle(handle, method_name, &args);
    }

    // SQLite Statement handle: stmt.raw() / .all() / .get() / .run() —
    // routes the dynamic-receiver path used by drizzle's
    // `this.stmt.raw().all(...params)` chain (where `this.stmt` is
    // any-typed because drizzle's PreparedQuery is a JS file with no
    // type annotations). Without this, the call falls through to the
    // generic dispatcher which doesn't know about sqlite stmts and
    // returns null/undefined sentinels — `(number).all is not a
    // function` then surfaces deeper down. Refs #643.
    //
    // Gated on `database-sqlite` so the dispatch fn (and its extern
    // refs to `js_sqlite_stmt_*`) are only emitted when sqlite is in
    // the build. The well-known flip used to strip this feature when
    // `better-sqlite3` routed to perry-ext-better-sqlite3, which
    // would have left this arm cfg'd out of every actually-using
    // binary — `optimized_libs.rs` now keeps `database-sqlite` for
    // exactly this reason (the duplicate `js_sqlite_*` symbols are
    // resolved by the linker to a single impl).
    #[cfg(feature = "database-sqlite")]
    if matches!(method_name, "raw" | "all" | "get" | "run") {
        let result = dispatch_sqlite_stmt(handle, method_name, &args);
        if result.to_bits() != perry_runtime::JSValue::undefined().bits() {
            return result;
        }
    }

    // SQLite Database handle: db.prepare(sql) / .exec(sql) / .close() —
    // routes the dynamic-receiver path used by drizzle's
    // `BetterSQLiteSession.prepareQuery` body, where
    // `const stmt = this.client.prepare(query.sql)` reads `this.client`
    // off a class instance field whose declared type is `any`. Pre-fix
    // the call fell through every dispatcher (the existing sqlite arm
    // only handles Statement methods, not Database methods) and the
    // catch-all returned NULL_OBJECT_BYTES — chained `stmt.run(...)` /
    // `stmt.raw().all(...)` then collapsed to a number receiver and
    // crashed with `(number).<method> is not a function` (the surface
    // symptom of #645). The static dispatch-table path (#465) covers
    // typed receivers; this arm is the runtime fallback for Any-typed
    // class fields the codegen can't statically resolve. Refs #645 /
    // #488 / #643. Method-gated to avoid claiming small handles owned
    // by other registries (HashHandle, FastifyApp, etc.).
    #[cfg(feature = "database-sqlite")]
    if matches!(method_name, "prepare" | "exec" | "close") {
        let result = dispatch_sqlite_db(handle, method_name, &args);
        if result.to_bits() != perry_runtime::JSValue::undefined().bits() {
            return result;
        }
    }

    // net.Socket: covers wrapper-function, struct-field, and Map.get
    // receivers where codegen lost the static type. Static NATIVE_MODULE_TABLE
    // path is still preferred when types are visible.
    #[cfg(all(
        feature = "bundled-net",
        not(target_os = "ios"),
        not(target_os = "android")
    ))]
    if crate::net::is_net_socket_handle(handle) {
        return dispatch_net_socket(handle, method_name, &args);
    }

    // zlib Transform streams (#1843): `zlib.createGzip()` etc. return handles
    // in the zlib small-handle range; their `.write`/`.end`/`.on`/`.pipe`/`.flush`/
    // `.params`/`.reset`/`.close` calls lose their static type and route here.
    // Gated on the registry AND the method vocabulary so a handle-id reused
    // across another subsystem's registry can't misroute (handle id-spaces
    // aren't unified — see the long comment above).
    #[cfg(feature = "compression")]
    if matches!(
        method_name,
        "write"
            | "end"
            | "on"
            | "once"
            | "pipe"
            | "flush"
            | "params"
            | "reset"
            | "close"
            | "destroy"
    ) && crate::zlib::is_zlib_stream_handle(handle)
    {
        // zlib streams are synchronous, so nothing else triggers the pump
        // registration that async ops (spawn/queue) normally do. Register here
        // so the event loop's `has_active` gate + pump drain the deferred
        // 'data'/'end' events instead of exiting before they fire (#1843).
        crate::common::async_bridge::ensure_pump_registered();
        return dispatch_zlib_stream(handle, method_name, &args);
    }

    // External zlib path (#1843): when the well-known flip routes `node:zlib`
    // to perry-ext-zlib, the stream handle + dispatch live in perry-ext-zlib.
    // Same registry-gated contract; the per-method match runs inside
    // `js_ext_zlib_dispatch_method`. This may coexist with `compression` in
    // no-auto test builds that use the full stdlib plus external archives.
    #[cfg(feature = "external-zlib-pump")]
    if matches!(
        method_name,
        "write"
            | "end"
            | "on"
            | "once"
            | "addListener"
            | "pipe"
            | "flush"
            | "params"
            | "reset"
            | "close"
            | "destroy"
    ) {
        extern "C" {
            fn js_ext_zlib_is_stream_handle(handle: i64) -> i32;
            fn js_ext_zlib_dispatch_method(
                handle: i64,
                method_ptr: *const u8,
                method_len: usize,
                args_ptr: *const f64,
                args_len: usize,
            ) -> f64;
        }
        if unsafe { js_ext_zlib_is_stream_handle(handle) } != 0 {
            // Register the stdlib pump (#1843) — see the bundled arm above.
            crate::common::async_bridge::ensure_pump_registered();
            return unsafe {
                js_ext_zlib_dispatch_method(
                    handle,
                    method_name.as_ptr(),
                    method_name.len(),
                    args.as_ptr(),
                    args.len(),
                )
            };
        }
    }

    #[cfg(feature = "external-http-client-pump")]
    if let Some(value) = unsafe {
        super::super::dispatch_http::dispatch_client_request_method(handle, method_name, &args)
    } {
        return value;
    }

    #[cfg(feature = "external-http-client-pump")]
    if let Some(value) = unsafe {
        super::super::dispatch_http::dispatch_client_incoming_method(handle, method_name, &args)
    } {
        return value;
    }

    // External http-server path (#2153): when `node:http` / `node:https` /
    // `node:http2` routes through perry-ext-http-server, the HttpServer handle
    // returned by `http.createServer(...)` reaches `js_native_call_method` via
    // the small-handle range check above whenever the receiver's static type
    // is `any` (e.g. `const s: any = http.createServer(...); s.listen(0)` or
    // any `.js` source — both are common in the node-test radar). Without
    // this arm `server.listen / .close / .on / .address / ...` resolved to
    // undefined-or-NaN even though the `("http", "HttpServer", ...)` rows in
    // `crates/perry-codegen/src/lower_call/native_table/http.rs` describe a
    // valid dispatch — the typed-feedback emit site doesn't consult the
    // native_table, and the runtime had no `HttpServer` arm.
    //
    // Method-gated so a handle id reused by another registry (HashHandle,
    // FastifyApp, …) doesn't misroute. The list mirrors the
    // `class_filter: Some("HttpServer")` rows in http.rs.
    #[cfg(feature = "external-http-server-pump")]
    {
        extern "C" {
            fn js_ext_http_server_is_handle(handle: i64) -> i32;
            fn js_ext_http_incoming_message_is_handle(handle: i64) -> i32;
            fn js_ext_http_server_response_is_handle(handle: i64) -> i32;
            fn js_ext_http2_session_is_handle(handle: i64) -> i32;
            fn js_ext_http2_stream_is_handle(handle: i64) -> i32;
            fn js_ext_http_server_dispatch_method(
                handle: i64,
                method_ptr: *const u8,
                method_len: usize,
                args_ptr: *const f64,
                args_len: usize,
            ) -> f64;
            fn js_ext_http_incoming_message_dispatch_method(
                handle: i64,
                method_ptr: *const u8,
                method_len: usize,
                args_ptr: *const f64,
                args_len: usize,
            ) -> f64;
            fn js_ext_http_server_response_dispatch_method(
                handle: i64,
                method_ptr: *const u8,
                method_len: usize,
                args_ptr: *const f64,
                args_len: usize,
            ) -> f64;
            fn js_ext_http2_session_dispatch_method(
                handle: i64,
                method_ptr: *const u8,
                method_len: usize,
                args_ptr: *const f64,
                args_len: usize,
            ) -> f64;
            fn js_ext_http2_stream_dispatch_method(
                handle: i64,
                method_ptr: *const u8,
                method_len: usize,
                args_ptr: *const f64,
                args_len: usize,
            ) -> f64;
        }

        let is_http_server_method = matches!(
            method_name,
            "listen" | "close" | "address" | "on" | "addListener" | "setTimeout"
        ) || matches!(
            method_name,
            "closeAllConnections"
                | "closeIdleConnections"
                | "removeAllListeners"
                | "removeListener"
                | "off"
                | "@@__perry_wk_asyncDispose"
        );
        if is_http_server_method && unsafe { js_ext_http_server_is_handle(handle) } != 0 {
            return unsafe {
                js_ext_http_server_dispatch_method(
                    handle,
                    method_name.as_ptr(),
                    method_name.len(),
                    args.as_ptr(),
                    args.len(),
                )
            };
        }

        let is_incoming_message_method = matches!(
            method_name,
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
            method_name,
            "method"
                | "url"
                | "httpVersion"
                | "headers"
                | "rawHeaders"
                | "headersDistinct"
                | "trailers"
                | "rawTrailers"
                | "trailersDistinct"
                | "socket"
                | "connection"
                | "signal"
                | "remoteAddress"
                | "remotePort"
        ) || matches!(
            method_name,
            "__get_method"
                | "__get_url"
                | "__get_httpVersion"
                | "__get_headers"
                | "__get_headersDistinct"
                | "__get_trailers"
        ) || matches!(
            method_name,
            "__get_rawHeaders"
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
        );
        if is_incoming_message_method
            && unsafe { js_ext_http_incoming_message_is_handle(handle) } != 0
        {
            return unsafe {
                js_ext_http_incoming_message_dispatch_method(
                    handle,
                    method_name.as_ptr(),
                    method_name.len(),
                    args.as_ptr(),
                    args.len(),
                )
            };
        }

        let is_server_response_method = matches!(
            method_name,
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
        ) || matches!(
            method_name,
            "addTrailers"
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
        ) || matches!(
            method_name,
            "on" | "addListener" | "setStatus" | "getStatus"
        ) || matches!(
            method_name,
            "__get_statusCode" | "__get_statusMessage" | "__set_statusCode" | "__set_statusMessage"
        ) || matches!(
            method_name,
            "__get_headersSent"
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
        );
        if is_server_response_method
            && unsafe { js_ext_http_server_response_is_handle(handle) } != 0
        {
            return unsafe {
                js_ext_http_server_response_dispatch_method(
                    handle,
                    method_name.as_ptr(),
                    method_name.len(),
                    args.as_ptr(),
                    args.len(),
                )
            };
        }

        let is_h2_session_method = matches!(
            method_name,
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
        );
        if is_h2_session_method && unsafe { js_ext_http2_session_is_handle(handle) } != 0 {
            return unsafe {
                js_ext_http2_session_dispatch_method(
                    handle,
                    method_name.as_ptr(),
                    method_name.len(),
                    args.as_ptr(),
                    args.len(),
                )
            };
        }

        let is_h2_stream_method = matches!(
            method_name,
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
        );
        if is_h2_stream_method && unsafe { js_ext_http2_stream_is_handle(handle) } != 0 {
            return unsafe {
                js_ext_http2_stream_dispatch_method(
                    handle,
                    method_name.as_ptr(),
                    method_name.len(),
                    args.as_ptr(),
                    args.len(),
                )
            };
        }
    }

    // #4975: client-side response (`http.get`/`ClientRequest` `'response'`
    // callback) is a *distinct* IncomingMessage handle from the server's, and
    // is registered as an EventEmitter — so `res.on(...)` already routes
    // through the EventEmitter arm above. But `Readable.pause()`/`.resume()`
    // aren't EventEmitter methods, the server-IM check above rejects the
    // client handle, and they fell through to the unknown-handle catch-all
    // which returns a NaN (`typeof` number). That broke the canonical
    // `res.resume().on('end', …)` body-drain chain with
    // `(number).on is not a function` (test-http-write-head-2). Node's
    // `Readable.pause()/resume()` return `this`; the buffered body already
    // drains when an `'end'`/`'data'` listener attaches, so returning the
    // receiver is the whole fix here.
    #[cfg(feature = "external-http-client-pump")]
    {
        extern "C" {
            fn js_ext_http_client_incoming_message_is_handle(handle: i64) -> i32;
        }
        if matches!(method_name, "pause" | "resume")
            && unsafe { js_ext_http_client_incoming_message_is_handle(handle) } != 0
        {
            return nanbox_handle_value(handle);
        }
    }

    // External net path (v0.5.581): perry-ext-net registers itself when
    // the well-known flip strips bundled-net. Same dispatch contract,
    // but routes through extern "C" symbols perry-ext-net provides.
    #[cfg(all(
        not(feature = "bundled-net"),
        feature = "external-net-pump",
        not(target_os = "ios"),
        not(target_os = "android")
    ))]
    {
        extern "C" {
            fn js_ext_net_is_socket_handle(handle: i64) -> i32;
        }
        if unsafe { js_ext_net_is_socket_handle(handle) } != 0 {
            return dispatch_external_net_socket(handle, method_name, &args);
        }
        if let Some(v) = crate::common::net_method_values::dispatch_external_server_method(
            handle,
            method_name,
            &args,
        ) {
            return v;
        }
        if let Some(v) = crate::common::net_method_values::dispatch_external_block_list_method(
            handle,
            method_name,
            &args,
        ) {
            return v;
        }
    }

    // Web Fetch method dispatch (refs #421 — Phase 1 of the handle-NaN-boxing
    // unification). When user code does `res.text()` / `res.json()` / etc. on
    // an any-typed Response handle (typical of npm packages with stripped TS
    // types — hono's `await app.fetch(req)` returns an any-typed value;
    // user-side `await res.text()` ends up here), the call lands in
    // `js_native_call_method` → small-handle range check → here. Each helper
    // does its own registry-membership + property-name gate; `None` means
    // "not us, try the next dispatcher or return undefined".
    #[cfg(feature = "web-fetch")]
    {
        // #1698: Request body methods (`req.json()`/`.text()`/`.arrayBuffer()`)
        // on an any-typed / computed-key receiver. Hono's `HonoRequest.#cachedBody`
        // does `raw[key]()` (computed key) on the underlying Request, which loses
        // the static type and lands here. Fetch-family ids are unified, so the
        // registry-membership gate inside cleanly distinguishes a Request from a
        // Response with the (formerly colliding) same id.
        if let Some(v) = crate::fetch::dispatch_request_method(handle as usize, method_name, &args)
        {
            return v;
        }
        if let Some(v) = crate::fetch::dispatch_response_method(handle as usize, method_name, &args)
        {
            return v;
        }
        if let Some(v) =
            crate::fetch::dispatch_form_data_method(handle as usize, method_name, &args)
        {
            return v;
        }
        if let Some(v) = crate::fetch::dispatch_blob_method(handle as usize, method_name, &args) {
            return v;
        }
        if let Some(v) = crate::fetch::dispatch_headers_method(handle as usize, method_name, &args)
        {
            return v;
        }
    }

    // Issue #848: StringDecoder write / end. The any-typed receiver path
    // (`const dec = new StringDecoder("utf8"); dec.write(buf)` where
    // `dec`'s declared type vanishes after TS stripping in libraries that
    // re-export it) lands here. Method-name gated to avoid claiming
    // colliding handle ids whose owners have disjoint method sets.
    if matches!(method_name, "write" | "end")
        && crate::string_decoder::is_string_decoder_handle(handle)
    {
        return crate::string_decoder::dispatch_string_decoder(handle, method_name, &args);
    }

    // Unknown handle type - return undefined
    TAG_UNDEFINED_F64
}
