//! Handle-based method dispatch for perry-stdlib
//!
//! When native modules (Fastify, ioredis, etc.) use handle-based objects,
//! and those handles are passed to functions as generic parameters,
//! the codegen can't statically determine the type. This module provides
//! runtime dispatch by checking the handle type in the registry.

use super::handle::*;

type EventEmitterOn = unsafe extern "C" fn(i64, i64, i64) -> i64;

/// Dispatch a method call on a handle-based object.
/// Called from perry-runtime's js_native_call_method when it detects a handle
/// (pointer value < 0x100000, indicating an integer handle, not a real heap pointer).
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

    // Fastify app: routes for HTTP verbs + lifecycle methods.
    // #1113 adds `"on"` here — `app.server.on(event, cb)` dispatches
    // against the same FastifyApp handle the user code holds (the
    // `app.server` getter returns the app handle pointer-tagged).
    #[cfg(feature = "http-server")]
    if matches!(
        method_name,
        "get"
            | "post"
            | "put"
            | "delete"
            | "patch"
            | "head"
            | "options"
            | "all"
            | "addHook"
            | "setErrorHandler"
            | "register"
            | "listen"
            | "close"
            | "on"
    ) && with_handle::<crate::fastify::FastifyApp, bool, _>(handle, |_| true).unwrap_or(false)
    {
        return dispatch_fastify_app(handle, method_name, &args);
    }

    // Fastify request/reply context.
    #[cfg(feature = "http-server")]
    if matches!(
        method_name,
        "send"
            | "status"
            | "code"
            | "header"
            | "type"
            | "method"
            | "url"
            | "body"
            | "json"
            | "params"
            | "headers"
    ) && with_handle::<crate::fastify::FastifyContext, bool, _>(handle, |_| true)
        .unwrap_or(false)
    {
        return dispatch_fastify_context(handle, method_name, &args);
    }

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
        return super::dispatch_ioredis::dispatch_ioredis(handle, method_name, &args);
    }

    // crypto Hash handle: createHash(...).update(...).digest().
    // The order vs. net (below) does not matter once method-gated, but we
    // keep hash before net to avoid changing the priority of in-registry
    // matches relative to the v0.5.98/#88 ordering.
    #[cfg(feature = "crypto")]
    if matches!(method_name, "update" | "digest" | "copy")
        && with_handle::<crate::crypto::HashHandle, bool, _>(handle, |_| true).unwrap_or(false)
    {
        return crate::crypto::dispatch_hash(handle, method_name, &args);
    }

    // crypto Hmac handle: createHmac(alg, key).update(...).digest(). Routes
    // the runtime path the codegen falls back to whenever `alg` isn't a
    // literal `"sha256"`. See #1076 for the silent-empty bug this closes.
    #[cfg(feature = "crypto")]
    if matches!(method_name, "update" | "digest")
        && with_handle::<crate::crypto::HmacHandle, bool, _>(handle, |_| true).unwrap_or(false)
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
    // in the 0x60000+ range; their `.write`/`.end`/`.on`/`.pipe`/`.flush`/
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
    // to perry-ext-zlib and strips `compression`, the stream handle + dispatch
    // live in perry-ext-zlib. Same registry-gated contract; the per-method
    // match runs inside `js_ext_zlib_dispatch_method`.
    #[cfg(all(feature = "external-zlib-pump", not(feature = "compression")))]
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
    if let Some(value) =
        unsafe { super::dispatch_http::dispatch_client_incoming_method(handle, method_name, &args) }
    {
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
        }

        let is_http_server_method =
            matches!(
                method_name,
                "listen" | "close" | "address" | "on" | "addListener"
            ) || matches!(method_name, "closeAllConnections" | "closeIdleConnections");
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
            "on" | "addListener" | "setEncoding" | "pause" | "resume" | "destroy" | "read"
        ) || matches!(
            method_name,
            "method" | "url" | "httpVersion" | "headers" | "rawHeaders"
        ) || matches!(
            method_name,
            "__get_method" | "__get_url" | "__get_httpVersion" | "__get_headers"
        ) || matches!(
            method_name,
            "__get_rawHeaders" | "__get_complete" | "__get_aborted" | "__get_destroyed"
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
            "setHeader" | "getHeader" | "removeHeader" | "hasHeader" | "writeHead" | "write"
        ) || matches!(
            method_name,
            "addTrailers" | "end" | "flushHeaders" | "writeContinue" | "writeProcessing"
        ) || matches!(
            method_name,
            "on" | "addListener" | "setStatus" | "getStatus"
        ) || matches!(
            method_name,
            "__get_statusCode" | "__set_statusCode" | "__set_statusMessage"
        ) || matches!(
            method_name,
            "__get_headersSent" | "__get_writableEnded" | "__get_writableFinished"
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
    }

    // Web Fetch method dispatch (refs #421 — Phase 1 of the handle-NaN-boxing
    // unification). When user code does `res.text()` / `res.json()` / etc. on
    // an any-typed Response handle (typical of npm packages with stripped TS
    // types — hono's `await app.fetch(req)` returns an any-typed value;
    // user-side `await res.text()` ends up here), the call lands in
    // `js_native_call_method` → small-handle range check → here. Each helper
    // does its own registry-membership + property-name gate; `None` means
    // "not us, try the next dispatcher or return undefined".
    #[cfg(feature = "http-client")]
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
    f64::from_bits(0x7FF8_0000_0000_0001)
}

/// Dispatch method calls on Fastify app handles
#[cfg(feature = "http-server")]
unsafe fn dispatch_fastify_app(handle: i64, method: &str, args: &[f64]) -> f64 {
    match method {
        "get" if args.len() >= 2 => {
            let path = args[0].to_bits() as i64;
            // Support 3-arg form: fastify.get(path, options, handler) — skip options object
            let handler = if args.len() >= 3 {
                args[2].to_bits() as i64
            } else {
                args[1].to_bits() as i64
            };
            let result = crate::fastify::js_fastify_get(handle, path, handler);
            if result {
                1.0
            } else {
                0.0
            }
        }
        "post" if args.len() >= 2 => {
            let path = args[0].to_bits() as i64;
            let handler = if args.len() >= 3 {
                args[2].to_bits() as i64
            } else {
                args[1].to_bits() as i64
            };
            let result = crate::fastify::js_fastify_post(handle, path, handler);
            if result {
                1.0
            } else {
                0.0
            }
        }
        "put" if args.len() >= 2 => {
            let path = args[0].to_bits() as i64;
            let handler = if args.len() >= 3 {
                args[2].to_bits() as i64
            } else {
                args[1].to_bits() as i64
            };
            let result = crate::fastify::js_fastify_put(handle, path, handler);
            if result {
                1.0
            } else {
                0.0
            }
        }
        "delete" if args.len() >= 2 => {
            let path = args[0].to_bits() as i64;
            let handler = if args.len() >= 3 {
                args[2].to_bits() as i64
            } else {
                args[1].to_bits() as i64
            };
            let result = crate::fastify::js_fastify_delete(handle, path, handler);
            if result {
                1.0
            } else {
                0.0
            }
        }
        "patch" if args.len() >= 2 => {
            let path = args[0].to_bits() as i64;
            let handler = if args.len() >= 3 {
                args[2].to_bits() as i64
            } else {
                args[1].to_bits() as i64
            };
            let result = crate::fastify::js_fastify_patch(handle, path, handler);
            if result {
                1.0
            } else {
                0.0
            }
        }
        "head" if args.len() >= 2 => {
            let path = args[0].to_bits() as i64;
            let handler = if args.len() >= 3 {
                args[2].to_bits() as i64
            } else {
                args[1].to_bits() as i64
            };
            let result = crate::fastify::js_fastify_head(handle, path, handler);
            if result {
                1.0
            } else {
                0.0
            }
        }
        "options" if args.len() >= 2 => {
            let path = args[0].to_bits() as i64;
            let handler = if args.len() >= 3 {
                args[2].to_bits() as i64
            } else {
                args[1].to_bits() as i64
            };
            let result = crate::fastify::js_fastify_options(handle, path, handler);
            if result {
                1.0
            } else {
                0.0
            }
        }
        "all" if args.len() >= 2 => {
            let path = args[0].to_bits() as i64;
            let handler = if args.len() >= 3 {
                args[2].to_bits() as i64
            } else {
                args[1].to_bits() as i64
            };
            let result = crate::fastify::js_fastify_all(handle, path, handler);
            if result {
                1.0
            } else {
                0.0
            }
        }
        "addHook" if args.len() >= 2 => {
            let hook_name = args[0].to_bits() as i64;
            let handler = args[1].to_bits() as i64;
            let result = crate::fastify::js_fastify_add_hook(handle, hook_name, handler);
            if result {
                1.0
            } else {
                0.0
            }
        }
        "setErrorHandler" if !args.is_empty() => {
            let handler = args[0].to_bits() as i64;
            let result = crate::fastify::js_fastify_set_error_handler(handle, handler);
            if result {
                1.0
            } else {
                0.0
            }
        }
        "register" if !args.is_empty() => {
            let plugin = args[0].to_bits() as i64;
            let opts = if args.len() >= 2 {
                args[1]
            } else {
                f64::from_bits(0x7FF8_0000_0000_0001)
            };
            let result = crate::fastify::js_fastify_register(handle, plugin, opts);
            if result {
                1.0
            } else {
                0.0
            }
        }
        "listen" if !args.is_empty() => {
            let callback = if args.len() >= 2 {
                args[1].to_bits() as i64
            } else {
                0
            };
            crate::fastify::js_fastify_listen(handle, args[0], callback);
            f64::from_bits(0x7FF8_0000_0000_0001) // undefined (void)
        }
        "close" => {
            // `app.close()` — shut down every server bound to this
            // FastifyApp. Walks the handle registry for matching
            // `FastifyServerHandle` rows and marks each as no-longer
            // listening so `js_fastify_has_active_handles` lets the
            // runtime's event loop exit. Pre-fix `close` was not
            // routed here — fell through to "unknown method" and was a
            // no-op, so the server kept the loop alive forever.
            crate::fastify::js_fastify_app_close(handle);
            f64::from_bits(0x7FF8_0000_0000_0001) // undefined (void)
        }
        "on" if args.len() >= 2 => {
            // #1113: `app.server.on(event, cb)` — see the function-level
            // doc on `js_fastify_app_server` for why `app.server`
            // shares the FastifyApp handle. Storing the callback
            // unblocks the user's boot-time
            // `app.server.on("upgrade", …)` line from throwing
            // `(number).on is not a function`. The hyper accept loop
            // doesn't yet route upgrade requests through the
            // registered handler list (full bidirectional WebSocket
            // upgrade dispatch is the tracked #1113 follow-up).
            let event_ptr = args[0].to_bits() as i64;
            let cb_ptr = args[1].to_bits() as i64;
            crate::fastify::js_fastify_app_on(handle, event_ptr, cb_ptr);
            // Mirror Node's `EventEmitter.on` contract: return the
            // emitter (the FastifyApp handle pointer-tagged) so
            // chained `app.server.on("a", …).on("b", …)` works.
            f64::from_bits(0x7FFD_0000_0000_0000 | (handle as u64 & 0x0000_FFFF_FFFF_FFFF))
        }
        _ => {
            // Unknown method - return undefined
            f64::from_bits(0x7FF8_0000_0000_0001)
        }
    }
}

/// Dispatch method calls on Fastify context handles (request/reply)
#[cfg(feature = "http-server")]
unsafe fn dispatch_fastify_context(handle: i64, method: &str, args: &[f64]) -> f64 {
    use perry_runtime::JSValue;

    match method {
        // Reply methods
        "send" if !args.is_empty() => {
            let result = crate::fastify::js_fastify_reply_send(handle, args[0]);
            if result {
                1.0
            } else {
                0.0
            }
        }
        "status" | "code" if !args.is_empty() => {
            let result = crate::fastify::js_fastify_reply_status(handle, args[0]);
            // Return the handle as NaN-boxed pointer for chaining (reply.status(200).send(...))
            f64::from_bits(0x7FFD_0000_0000_0000 | (result as u64 & 0x0000_FFFF_FFFF_FFFF))
        }
        "header" if args.len() >= 2 => {
            let name = args[0].to_bits() as i64;
            let value = args[1].to_bits() as i64;
            let result = crate::fastify::js_fastify_reply_header(handle, name, value);
            // Return the handle for chaining
            f64::from_bits(0x7FFD_0000_0000_0000 | (result as u64 & 0x0000_FFFF_FFFF_FFFF))
        }
        // `reply.type(value)` — chainable alias for setting content-type.
        // Without this arm, chained `.code().type().send()` returned
        // TAG_UNDEFINED for `.type()` and the next chain step failed with
        // `(number).send is not a function` (#1048). The chain takes this
        // path (rather than NATIVE_MODULE_TABLE static dispatch) because
        // the HIR loses the static type after the first call in the chain.
        "type" if !args.is_empty() => {
            let value = args[0].to_bits() as i64;
            let result = crate::fastify::js_fastify_reply_type(handle, value);
            f64::from_bits(0x7FFD_0000_0000_0000 | (result as u64 & 0x0000_FFFF_FFFF_FFFF))
        }
        // Request methods
        "method" => {
            let ptr = crate::fastify::js_fastify_req_method(handle);
            f64::from_bits(JSValue::string_ptr(ptr).bits())
        }
        "url" => {
            let ptr = crate::fastify::js_fastify_req_url(handle);
            f64::from_bits(JSValue::string_ptr(ptr).bits())
        }
        "body" => crate::fastify::js_fastify_req_json(handle),
        "json" => crate::fastify::js_fastify_req_json(handle),
        "params" => crate::fastify::js_fastify_req_params_object(handle),
        "headers" => {
            // Returns NaN-boxed JS object (parsed from JSON), use bits directly
            let bits = crate::fastify::js_fastify_req_headers(handle);
            f64::from_bits(bits as u64)
        }
        _ => {
            // Unknown method - return undefined
            f64::from_bits(0x7FF8_0000_0000_0001)
        }
    }
}

/// Dispatch method calls on net.Socket handles when codegen couldn't tag
/// the receiver type. Mirrors the static NATIVE_MODULE_TABLE entries for
/// the same methods (write/end/destroy/on/upgradeToTLS).
///
/// Args arrive as NaN-boxed `f64`s: BufferHeader / StringHeader / Closure
/// pointers in the low 48 bits with POINTER_TAG / STRING_TAG in the top.
/// We strip the tag and pass the raw `i64` to the FFI — same shape the
/// codegen path produces.
#[cfg(all(
    feature = "bundled-net",
    not(target_os = "ios"),
    not(target_os = "android")
))]
unsafe fn dispatch_net_socket(handle: i64, method: &str, args: &[f64]) -> f64 {
    /// Strip a NaN-box tag (POINTER / STRING / BIGINT) to get the raw 48-bit pointer.
    fn unbox_to_i64(v: f64) -> i64 {
        (v.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64
    }

    match method {
        "write" if !args.is_empty() => {
            // Issue #1131 — pass the full NaN-box bits; the runtime
            // probes Buffer-vs-string and reads the correct layout.
            crate::net::js_net_socket_write(handle, args[0].to_bits() as i64);
            f64::from_bits(0x7FFC_0000_0000_0001) // undefined
        }
        "end" => {
            // Issue #1852 — forward the optional `socket.end(data)` chunk.
            let chunk = args
                .first()
                .copied()
                .unwrap_or(f64::from_bits(0x7FFC_0000_0000_0001));
            crate::net::js_net_socket_end(handle, chunk.to_bits() as i64);
            f64::from_bits(0x7FFC_0000_0000_0001)
        }
        "destroy" | "destroySoon" => {
            crate::net::js_net_socket_destroy(handle);
            f64::from_bits(0x7FFC_0000_0000_0001)
        }
        "on" if args.len() >= 2 => {
            let event_ptr = unbox_to_i64(args[0]);
            let cb_ptr = unbox_to_i64(args[1]);
            crate::net::js_net_socket_on(handle, event_ptr, cb_ptr);
            f64::from_bits(0x7FFC_0000_0000_0001)
        }
        // Issue #422: `sock.connect(port, host)` for the deferred-connect
        // shape (`new net.Socket()` then `.connect(...)`). The first arg
        // is the port (raw f64); the second is a string handle (NaN-boxed
        // STRING_TAG'd f64) that we strip back to the StringHeader pointer.
        "connect" if args.len() >= 2 => {
            let port = args[0];
            let host_ptr = unbox_to_i64(args[1]);
            crate::net::js_net_socket_method_connect(handle, port, host_ptr);
            f64::from_bits(0x7FFC_0000_0000_0001)
        }
        "upgradeToTLS" if !args.is_empty() => {
            // upgradeToTLS(servername, verify) → Promise. Default verify=1
            // when omitted, mirroring the safer default in the static table.
            let servername_ptr = unbox_to_i64(args[0]);
            let verify = if args.len() >= 2 { args[1] } else { 1.0 };
            let promise = crate::net::js_net_socket_upgrade_tls(handle, servername_ptr, verify);
            f64::from_bits(0x7FFD_0000_0000_0000u64 | (promise as u64 & 0x0000_FFFF_FFFF_FFFF))
        }
        _ => f64::from_bits(0x7FFC_0000_0000_0001),
    }
}

/// Dispatch a method call on a zlib Transform-stream handle (#1843).
///
/// `createGzip()` / `createDeflate()` / `createBrotliCompress()` / … return
/// handles whose `.write`/`.end`/`.on`/`.pipe`/`.flush`/`.params`/`.reset`/
/// `.close` lose their static type and arrive here. Compression is synchronous
/// and buffered in the runtime: `.write()` accumulates input, `.end()` runs the
/// codec and queues 'data'/'end' onto the deferred-event pump.
#[cfg(feature = "compression")]
unsafe fn dispatch_zlib_stream(handle: i64, method: &str, args: &[f64]) -> f64 {
    fn unbox_to_i64(v: f64) -> i64 {
        (v.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64
    }
    const UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TRUE: u64 = 0x7FFC_0000_0000_0004;
    // The stream itself, re-boxed as a POINTER_TAG handle (for `.on()` chaining
    // `s.on('data', …).on('end', …)`).
    let self_ref =
        f64::from_bits(0x7FFD_0000_0000_0000u64 | (handle as u64 & 0x0000_FFFF_FFFF_FFFF));
    match method {
        "write" if !args.is_empty() => {
            crate::zlib::zlib_stream_write(handle, args[0]);
            f64::from_bits(TRUE) // Node's writable.write() returns a boolean
        }
        "end" => {
            let chunk = args.first().copied().unwrap_or(f64::from_bits(UNDEFINED));
            crate::zlib::zlib_stream_end(handle, chunk);
            self_ref
        }
        "on" | "once" if args.len() >= 2 => {
            // `args[0]` is the full NaN-boxed event name (SSO-safe extraction
            // happens inside zlib_stream_on); `args[1]` is the closure pointer.
            crate::zlib::zlib_stream_on(handle, args[0], unbox_to_i64(args[1]));
            self_ref
        }
        "pipe" if !args.is_empty() => {
            crate::zlib::zlib_stream_pipe(handle, args[0]);
            args[0] // Node's `.pipe(dest)` returns `dest` for chaining
        }
        "close" | "destroy" => {
            // Force the codec to run (so 'end' fires) if it hasn't already.
            crate::zlib::zlib_stream_end(handle, f64::from_bits(UNDEFINED));
            f64::from_bits(UNDEFINED)
        }
        // `.flush([kind], cb?)` — emit a Z_SYNC_FLUSH block, then run the
        // callback. `kind` is numeric; the callback is the POINTER_TAG arg.
        "flush" => {
            let cb = args
                .iter()
                .rev()
                .find(|a| (a.to_bits() >> 48) == 0x7FFD)
                .map(|a| unbox_to_i64(*a))
                .unwrap_or(0);
            crate::zlib::zlib_stream_flush(handle, cb);
            f64::from_bits(UNDEFINED)
        }
        "params" => {
            let cb = args
                .iter()
                .rev()
                .find(|a| (a.to_bits() >> 48) == 0x7FFD)
                .map(|a| unbox_to_i64(*a))
                .unwrap_or(0);
            crate::zlib::zlib_stream_params(handle, cb);
            f64::from_bits(UNDEFINED)
        }
        "reset" => {
            crate::zlib::zlib_stream_reset(handle);
            f64::from_bits(UNDEFINED)
        }
        _ => f64::from_bits(UNDEFINED),
    }
}

/// Dispatch a method call on a perry-ext-net Socket handle via
/// extern "C" symbols. Same shape as `dispatch_net_socket` above
/// but the per-method functions resolve to perry-ext-net's archive
/// at link time, not perry-stdlib's `crate::net::*`.
///
/// Closes issue #91 regression for the well-known-flipped path:
/// Map.get'd / struct-field / wrapper-function receivers where
/// the static type was lost get caught by HANDLE_METHOD_DISPATCH
/// and routed here.
#[cfg(all(
    not(feature = "bundled-net"),
    feature = "external-net-pump",
    not(target_os = "ios"),
    not(target_os = "android")
))]
unsafe fn dispatch_external_net_socket(handle: i64, method: &str, args: &[f64]) -> f64 {
    fn unbox_to_i64(v: f64) -> i64 {
        (v.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64
    }
    // Pack a raw i64 handle back as a NaN-boxed POINTER_TAG f64 — the
    // shape every chainable Socket method returns so subsequent
    // `.on(...)` / `.write(...)` calls dispatch through the same
    // small-handle range check at the top of `js_native_call_method`.
    fn nanbox_handle(h: i64) -> f64 {
        f64::from_bits(0x7FFD_0000_0000_0000u64 | (h as u64 & 0x0000_FFFF_FFFF_FFFF))
    }
    extern "C" {
        fn js_net_socket_write(handle: i64, buf_ptr: i64);
        // Issue #1852 — `js_net_socket_end` now takes the optional final
        // chunk (NA_JSV bits) so `socket.end(data)` writes before FIN.
        fn js_net_socket_end(handle: i64, chunk_bits: i64);
        fn js_net_socket_destroy(handle: i64);
        fn js_net_socket_on(handle: i64, event_ptr: i64, cb_ptr: i64);
        fn js_net_socket_method_connect(handle: i64, port: f64, host_ptr: i64);
        fn js_net_socket_upgrade_tls(
            handle: i64,
            servername_ptr: i64,
            verify: f64,
        ) -> *mut perry_runtime::Promise;
        // Issue #2131 — lifecycle + EventEmitter surface beyond `on`.
        // Same FFIs the NATIVE_MODULE_TABLE typed path uses; the
        // dispatch arms below route any-typed receivers (e.g. the
        // socket arg of `server.on('connection', sock => …)` after
        // codegen loses the static class) to them.
        fn js_net_socket_address(handle: i64) -> *mut perry_runtime::StringHeader;
        fn js_net_socket_once(handle: i64, event_ptr: i64, cb_ptr: i64) -> i64;
        fn js_net_socket_remove_listener(handle: i64, event_ptr: i64, cb_ptr: i64) -> i64;
        fn js_net_socket_remove_all_listeners(handle: i64, event_ptr: i64) -> i64;
        fn js_net_socket_listener_count(handle: i64, event_ptr: i64) -> f64;
        fn js_net_socket_event_names(handle: i64) -> *mut perry_runtime::StringHeader;
        fn js_net_socket_reset_and_destroy(handle: i64) -> i64;
        // Issue #2211 — listeners()/rawListeners() return a *mut ArrayHeader
        // cast to i64; NaN-box with POINTER_TAG to surface as a real JS array.
        fn js_net_socket_listeners(handle: i64, event_ptr: i64) -> i64;
        fn js_net_socket_raw_listeners(handle: i64, event_ptr: i64) -> i64;
    }

    // Parse a runtime StringHeader pointer (`address` / `eventNames`
    // return value) into a NaN-boxed JS value via `js_json_parse_or_null`.
    // Mirrors the codegen's NR_OBJ_FROM_JSON_STR lowering so the
    // typed-path and any-typed-path return shapes match byte-for-byte.
    fn json_str_to_value(s: *mut perry_runtime::StringHeader) -> f64 {
        if s.is_null() {
            return f64::from_bits(0x7FFC_0000_0000_0002); // null
        }
        f64::from_bits(unsafe { perry_runtime::json::js_json_parse_or_null(s).bits() })
    }

    match method {
        "write" if !args.is_empty() => {
            // Issue #1131 — pass the full NaN-box bits, not the
            // pre-stripped pointer. perry-ext-net's js_net_socket_write
            // now probes Buffer-vs-string itself.
            js_net_socket_write(handle, args[0].to_bits() as i64);
            f64::from_bits(0x7FFC_0000_0000_0001)
        }
        "end" => {
            // Issue #1852 — forward the optional `socket.end(data)` chunk;
            // pad with `undefined` for the no-arg `socket.end()` form.
            let chunk = args
                .first()
                .copied()
                .unwrap_or(f64::from_bits(0x7FFC_0000_0000_0001));
            js_net_socket_end(handle, chunk.to_bits() as i64);
            f64::from_bits(0x7FFC_0000_0000_0001)
        }
        "destroy" | "destroySoon" => {
            js_net_socket_destroy(handle);
            f64::from_bits(0x7FFC_0000_0000_0001)
        }
        "on" | "addListener" if args.len() >= 2 => {
            let event_ptr = unbox_to_i64(args[0]);
            let cb_ptr = unbox_to_i64(args[1]);
            js_net_socket_on(handle, event_ptr, cb_ptr);
            nanbox_handle(handle)
        }
        "connect" if args.len() >= 2 => {
            let port = args[0];
            let host_ptr = unbox_to_i64(args[1]);
            js_net_socket_method_connect(handle, port, host_ptr);
            f64::from_bits(0x7FFC_0000_0000_0001)
        }
        "upgradeToTLS" if !args.is_empty() => {
            let servername_ptr = unbox_to_i64(args[0]);
            let verify = if args.len() >= 2 { args[1] } else { 1.0 };
            let promise = js_net_socket_upgrade_tls(handle, servername_ptr, verify);
            f64::from_bits(0x7FFD_0000_0000_0000u64 | (promise as u64 & 0x0000_FFFF_FFFF_FFFF))
        }
        // Issue #2131 — EventEmitter surface on any-typed receivers
        // (the accepted-socket arg of `server.on('connection', s => …)`
        // is the dominant case; the static class info is lost between
        // the connection event push and the user callback).
        "once" if args.len() >= 2 => {
            let event_ptr = unbox_to_i64(args[0]);
            let cb_ptr = unbox_to_i64(args[1]);
            js_net_socket_once(handle, event_ptr, cb_ptr);
            nanbox_handle(handle)
        }
        "off" | "removeListener" if args.len() >= 2 => {
            let event_ptr = unbox_to_i64(args[0]);
            let cb_ptr = unbox_to_i64(args[1]);
            js_net_socket_remove_listener(handle, event_ptr, cb_ptr);
            nanbox_handle(handle)
        }
        "removeAllListeners" => {
            // Bare `removeAllListeners()` passes no event, padded as
            // `undefined`; the FFI treats a null/non-string ptr as
            // "drain every event".
            let event_ptr = args.first().copied().map(unbox_to_i64).unwrap_or(0);
            js_net_socket_remove_all_listeners(handle, event_ptr);
            nanbox_handle(handle)
        }
        "listenerCount" if !args.is_empty() => {
            let event_ptr = unbox_to_i64(args[0]);
            js_net_socket_listener_count(handle, event_ptr)
        }
        "eventNames" => json_str_to_value(js_net_socket_event_names(handle)),
        // Issue #2211 — `socket.listeners(event)` / `socket.rawListeners(event)`
        // for any-typed receivers. FFI returns a *mut ArrayHeader cast to i64;
        // NaN-box with POINTER_TAG (0x7FFD) so callers see a real JS array.
        "listeners" if !args.is_empty() => {
            let event_ptr = unbox_to_i64(args[0]);
            let arr = js_net_socket_listeners(handle, event_ptr);
            f64::from_bits(0x7FFD_0000_0000_0000u64 | (arr as u64 & 0x0000_FFFF_FFFF_FFFF))
        }
        "rawListeners" if !args.is_empty() => {
            let event_ptr = unbox_to_i64(args[0]);
            let arr = js_net_socket_raw_listeners(handle, event_ptr);
            f64::from_bits(0x7FFD_0000_0000_0000u64 | (arr as u64 & 0x0000_FFFF_FFFF_FFFF))
        }
        "address" => json_str_to_value(js_net_socket_address(handle)),
        "resetAndDestroy" => {
            js_net_socket_reset_and_destroy(handle);
            nanbox_handle(handle)
        }
        // Chainable Socket option setters — Node returns `this` from each
        // so feature-detect-and-call sites stay flowing on any-typed
        // receivers. Pre-#2131 these returned `undefined` here and the
        // very next `.write(...)` lost its handle.
        "setNoDelay" | "setKeepAlive" | "setTimeout" | "setEncoding" | "pause" | "resume"
        | "ref" | "unref" | "cork" | "uncork" | "setDefaultEncoding" => nanbox_handle(handle),
        _ => f64::from_bits(0x7FFC_0000_0000_0001),
    }
}

/// Dispatch a property access on a handle-based object.
/// Called from perry-runtime's js_dynamic_object_get_property when it detects a handle.
#[no_mangle]
pub unsafe extern "C" fn js_handle_property_dispatch(
    handle: i64,
    property_name_ptr: *const u8,
    property_name_len: usize,
) -> f64 {
    #[cfg(feature = "http-server")]
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

    // #1670: Web Streams handle property reads. A numeric stream id reaches
    // here via `js_object_get_field_by_name`'s stream probe (inline
    // `res.body.locked`). Route getter properties to their accessors, return
    // a bound-method closure for callable members, and undefined for anything
    // else — never a deref of the float id as a pointer. Gated on stream
    // id-range + registry membership so unrelated small-handle reads are
    // untouched.
    #[cfg(feature = "bundled-streams")]
    if (0x40000..0x100000).contains(&(handle as usize))
        && crate::streams::js_stream_handle_is_registered(handle as usize)
    {
        return crate::streams::dispatch_stream_property(handle as f64, property_name);
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
            return js_class_method_bind(handle as f64, name_bytes.as_ptr(), name_bytes.len());
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
                | "close"
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

    // Server-side node:http request/response handles whose static
    // `HttpServer` / `IncomingMessage` / `ServerResponse` type was lost.
    #[cfg(feature = "external-http-server-pump")]
    {
        extern "C" {
            fn js_ext_http_server_is_handle(handle: i64) -> i32;
            fn js_ext_http_incoming_message_is_handle(handle: i64) -> i32;
            fn js_ext_http_server_response_is_handle(handle: i64) -> i32;
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
                | "httpVersion"
                | "headers"
                | "rawHeaders"
                | "complete"
                | "aborted"
                | "destroyed"
                | "on"
                | "addListener"
                | "setEncoding"
                | "pause"
                | "resume"
                | "destroy"
                | "read"
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
                | "headersSent"
                | "writableEnded"
                | "writableFinished"
                | "setHeader"
                | "getHeader"
                | "removeHeader"
                | "hasHeader"
                | "writeHead"
                | "write"
                | "addTrailers"
                | "end"
                | "flushHeaders"
                | "writeContinue"
                | "writeProcessing"
                | "on"
                | "addListener"
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
    }

    // #1113: `app.server` — return the FastifyApp handle pointer-tagged
    // so `typeof app.server === "object"` and `.on("upgrade", …)`
    // routes through HANDLE_METHOD_DISPATCH back into the FastifyApp
    // arm (see `js_fastify_app_server` for full rationale). Gated on
    // membership in the FastifyApp registry AND the literal `"server"`
    // property name so unrelated handle ids that happen to land on
    // `.server` access don't accidentally claim the path.
    #[cfg(feature = "http-server")]
    if property_name == "server"
        && with_handle::<crate::fastify::FastifyApp, bool, _>(handle, |_| true).unwrap_or(false)
    {
        // `js_fastify_app_server` returns the bare i64 handle; the
        // codegen-side NATIVE_MODULE_TABLE arm NaN-boxes it via
        // `NR_PTR`. The property-dispatch path lives below that
        // (handles dynamic small-handle `.server` reads when codegen
        // didn't recognise the receiver), so we tag the handle
        // inline here to keep the JS-visible shape consistent.
        let h = crate::fastify::js_fastify_app_server(handle);
        return f64::from_bits(0x7FFD_0000_0000_0000u64 | ((h as u64) & 0x0000_FFFF_FFFF_FFFF));
    }

    // Try Fastify context dispatch (request/reply properties)
    #[cfg(feature = "http-server")]
    if with_handle::<crate::fastify::FastifyContext, bool, _>(handle, |_| true).unwrap_or(false) {
        return match property_name {
            "query" => {
                // Return a real JavaScript object, not a JSON string
                crate::fastify::js_fastify_req_query_object(handle)
            }
            "params" => crate::fastify::js_fastify_req_params_object(handle),
            "body" => crate::fastify::js_fastify_req_json(handle),
            "rawBody" | "text" => {
                let ptr = crate::fastify::js_fastify_req_body(handle);
                if ptr.is_null() {
                    f64::from_bits(0x7FFC_0000_0000_0001)
                } else {
                    f64::from_bits(JSValue::string_ptr(ptr).bits())
                }
            }
            "headers" => {
                // Returns NaN-boxed JS object (parsed from JSON), use bits directly
                let bits = crate::fastify::js_fastify_req_headers(handle);
                f64::from_bits(bits as u64)
            }
            "method" => {
                let ptr = crate::fastify::js_fastify_req_method(handle);
                if ptr.is_null() {
                    f64::from_bits(0x7FFC_0000_0000_0001)
                } else {
                    f64::from_bits(JSValue::string_ptr(ptr).bits())
                }
            }
            "url" => {
                let ptr = crate::fastify::js_fastify_req_url(handle);
                if ptr.is_null() {
                    f64::from_bits(0x7FFC_0000_0000_0001)
                } else {
                    f64::from_bits(JSValue::string_ptr(ptr).bits())
                }
            }
            "user" => {
                // Return user data set by auth middleware
                crate::fastify::js_fastify_req_get_user_data(handle)
            }
            _ => f64::from_bits(0x7FFC_0000_0000_0001), // undefined
        };
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
    if let Some(value) =
        unsafe { super::dispatch_http::dispatch_client_incoming_property(handle, property_name) }
    {
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
    // Gated on `http-client` because fetch.rs itself is gated on that feature.
    #[cfg(feature = "http-client")]
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
        "lastNeed" | "lastTotal" | "lastChar" | "encoding" | "write" | "end"
    ) && crate::string_decoder::is_string_decoder_handle(handle)
    {
        return crate::string_decoder::dispatch_string_decoder_property(handle, property_name);
    }

    #[cfg(feature = "crypto")]
    if matches!(property_name, "update" | "digest" | "copy")
        && with_handle::<crate::crypto::HashHandle, bool, _>(handle, |_| true).unwrap_or(false)
    {
        return crate::crypto::dispatch_hash_property(handle, property_name);
    }

    #[cfg(feature = "crypto")]
    if matches!(property_name, "update" | "digest")
        && with_handle::<crate::crypto::HmacHandle, bool, _>(handle, |_| true).unwrap_or(false)
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

    // #1367: X509Certificate read-only properties (data values, not
    // bound-method closures).
    #[cfg(feature = "crypto")]
    if matches!(
        property_name,
        "subject"
            | "issuer"
            | "validFrom"
            | "validTo"
            | "serialNumber"
            | "fingerprint"
            | "fingerprint256"
            | "ca"
            | "raw"
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

    // Unknown handle type - return undefined
    f64::from_bits(0x7FFC_0000_0000_0001)
}

/// Dispatch method calls on SQLite Statement handles. Routes the
/// dynamic-receiver chain `this.stmt.raw().all(...params)` (drizzle's
/// PreparedQuery.values()) and similar shapes where the codegen
/// can't see the static stmt type. The runtime paths
/// (`js_sqlite_stmt_*`) take a pre-packed args array, so this
/// function repacks the f64 slice into a fresh JS array via
/// `js_array_alloc` + `js_array_push` before delegating.
///
/// Gated on `database-sqlite` — symbol/feature reasoning lives at
/// the caller arm in `js_handle_method_dispatch`. The extern
/// `js_sqlite_stmt_*` declarations resolve to whichever crate's impl
/// the linker picked (perry-stdlib's vs perry-ext-better-sqlite3's),
/// so this dispatch routes to the same impl that `js_sqlite_prepare`
/// used to allocate the handle. Refs #643.
#[cfg(feature = "database-sqlite")]
unsafe fn dispatch_sqlite_stmt(handle: i64, method: &str, args: &[f64]) -> f64 {
    use perry_runtime::js_nanbox_pointer;
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let arg_handles = scope.root_nanbox_f64_slice(args);
    // Pack args into a fresh JS array. Each `f64` is already a
    // NaN-boxed value as the codegen produces. js_array_push takes a
    // perry_ffi::JsValue (NaN-boxed), but the runtime helpers in
    // perry-stdlib accept JSValue::from_bits — convert via raw bits.
    let arr = perry_runtime::js_array_alloc(0);
    let arr_handle = scope.root_raw_mut_ptr(arr);
    for handle in &arg_handles {
        let v = handle.get_nanbox_f64();
        let arr = perry_runtime::js_array_push(
            arr_handle.get_raw_mut_ptr(),
            perry_runtime::JSValue::from_bits(v.to_bits()),
        );
        arr_handle.set_raw_mut_ptr(arr);
    }
    let arr_handle = arr_handle.get_raw_mut_ptr::<perry_runtime::ArrayHeader>();

    // Route through extern "C" so we hit the *linked* impl
    // (perry-stdlib's vs perry-ext-better-sqlite3's — only one wins
    // the link race when both crates expose `js_sqlite_*`). Calling
    // `crate::sqlite::js_sqlite_stmt_*` directly would always invoke
    // perry-stdlib's local impl, so handles registered by perry-ext's
    // `js_sqlite_prepare` (different TypeId) wouldn't downcast in
    // perry-stdlib's get_handle. The extern path delegates to whichever
    // crate's `js_sqlite_prepare` actually ran, keeping handle and
    // lookup TypeIds consistent. Refs #643.
    extern "C" {
        fn js_sqlite_stmt_raw(stmt_handle: i64) -> i64;
        fn js_sqlite_stmt_all(
            stmt_handle: i64,
            params_arr: *const perry_runtime::ArrayHeader,
        ) -> *mut perry_runtime::ArrayHeader;
        fn js_sqlite_stmt_get(
            stmt_handle: i64,
            params_arr: *const perry_runtime::ArrayHeader,
        ) -> f64;
        fn js_sqlite_stmt_run(
            stmt_handle: i64,
            params_arr: *const perry_runtime::ArrayHeader,
        ) -> *mut perry_runtime::ObjectHeader;
    }

    match method {
        "raw" => {
            let new_handle = js_sqlite_stmt_raw(handle);
            // NaN-box as a pointer so subsequent dynamic dispatch sees
            // it as a heap-pointer-shaped value (the runtime detects
            // small-handle range and routes back here).
            js_nanbox_pointer(new_handle)
        }
        "all" => {
            let arr_ptr = js_sqlite_stmt_all(handle, arr_handle);
            js_nanbox_pointer(arr_ptr as i64)
        }
        "get" => {
            // Already returns f64 (NaN-boxed bits).
            js_sqlite_stmt_get(handle, arr_handle)
        }
        "run" => {
            let obj_ptr = js_sqlite_stmt_run(handle, arr_handle);
            if obj_ptr.is_null() {
                f64::from_bits(perry_runtime::JSValue::undefined().bits())
            } else {
                js_nanbox_pointer(obj_ptr as i64)
            }
        }
        _ => f64::from_bits(perry_runtime::JSValue::undefined().bits()),
    }
}

/// Dispatch method calls on a SQLite Database handle (`db.prepare(sql)`,
/// `db.exec(sql)`, `db.close()`) — the Database counterpart to
/// `dispatch_sqlite_stmt`. Reached when codegen lost the static type
/// through a class field (e.g. drizzle's
/// `BetterSQLiteSession.prepareQuery` reads `this.client` typed as
/// `any` and calls `.prepare(query.sql)`). The static NATIVE_MODULE
/// dispatch-table path (#465) covers typed receivers; this arm is
/// the runtime fallback. Returns `JSValue::undefined()` if the handle
/// isn't a SqliteDb — the caller falls through to the next
/// dispatcher.
///
/// Like `dispatch_sqlite_stmt`, we route through `extern "C"` so the
/// linked impl wins (perry-stdlib's vs perry-ext-better-sqlite3's),
/// keeping handle and lookup TypeIds consistent regardless of which
/// crate registered the Database handle.
#[cfg(feature = "database-sqlite")]
unsafe fn dispatch_sqlite_db(handle: i64, method: &str, args: &[f64]) -> f64 {
    use perry_runtime::js_nanbox_pointer;

    extern "C" {
        fn js_sqlite_prepare(db_handle: i64, sql_ptr: *const perry_runtime::StringHeader) -> i64;
        fn js_sqlite_exec(db_handle: i64, sql_ptr: *const perry_runtime::StringHeader) -> i32;
        fn js_sqlite_close(db_handle: i64) -> i32;
    }

    // Helper: extract a raw StringHeader pointer from a NaN-boxed f64.
    // STRING_TAG (0x7FFF) carries a 48-bit pointer in the lower bits.
    let arg_str_ptr = |idx: usize| -> *const perry_runtime::StringHeader {
        if idx >= args.len() {
            return std::ptr::null();
        }
        let bits = args[idx].to_bits();
        let tag = bits >> 48;
        if tag == 0x7FFF {
            (bits & 0x0000_FFFF_FFFF_FFFF) as *const perry_runtime::StringHeader
        } else {
            std::ptr::null()
        }
    };

    match method {
        "prepare" => {
            let sql_ptr = arg_str_ptr(0);
            if sql_ptr.is_null() {
                return f64::from_bits(perry_runtime::JSValue::undefined().bits());
            }
            let stmt_handle = js_sqlite_prepare(handle, sql_ptr);
            // -1 means prepare failed (invalid SQL or not-a-Database
            // handle — the registry lookup inside `js_sqlite_prepare`
            // returns None for the latter). Returning undefined lets
            // the outer dispatcher fall through to other arms (e.g.
            // when the handle is actually a HashHandle or FastifyApp
            // with a coincidentally-named "prepare" method).
            if stmt_handle < 0 {
                return f64::from_bits(perry_runtime::JSValue::undefined().bits());
            }
            // NaN-box as POINTER so subsequent `.run(...)` / `.all(...)`
            // / `.get(...)` calls re-enter the small-handle dispatch
            // path and route to `dispatch_sqlite_stmt`.
            js_nanbox_pointer(stmt_handle)
        }
        "exec" => {
            let sql_ptr = arg_str_ptr(0);
            if sql_ptr.is_null() {
                return f64::from_bits(perry_runtime::JSValue::undefined().bits());
            }
            let _ = js_sqlite_exec(handle, sql_ptr);
            // better-sqlite3 returns the Database for chaining; mirror
            // that so `db.exec("...").exec("...")` chains.
            js_nanbox_pointer(handle)
        }
        "close" => {
            let _ = js_sqlite_close(handle);
            f64::from_bits(perry_runtime::JSValue::undefined().bits())
        }
        _ => f64::from_bits(perry_runtime::JSValue::undefined().bits()),
    }
}

/// Dispatch property set on a handle-based object.
/// Called from perry-runtime's js_object_set_field_by_name when it detects a handle.
#[no_mangle]
pub unsafe extern "C" fn js_handle_property_set_dispatch(
    handle: i64,
    property_name_ptr: *const u8,
    property_name_len: usize,
    value: f64,
) {
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
    let _ = value;

    // Try Fastify context dispatch (request/reply properties)
    #[cfg(feature = "http-server")]
    if with_handle::<crate::fastify::FastifyContext, bool, _>(handle, |_| true).unwrap_or(false) {
        if property_name == "user" {
            crate::fastify::js_fastify_req_set_user_data(handle, value);
        }
    }

    #[cfg(feature = "external-http-server-pump")]
    if matches!(property_name, "statusCode" | "statusMessage") {
        extern "C" {
            fn js_ext_http_server_response_is_handle(handle: i64) -> i32;
            fn js_ext_http_server_response_dispatch_property_set(
                handle: i64,
                property_ptr: *const u8,
                property_len: usize,
                value: f64,
            ) -> i32;
        }

        if unsafe { js_ext_http_server_response_is_handle(handle) } != 0 {
            unsafe {
                js_ext_http_server_response_dispatch_property_set(
                    handle,
                    property_name.as_ptr(),
                    property_name.len(),
                    value,
                );
            }
        }
    }
}

/// #2533: route a captured / aliased `http`/`https`/`http2` `createServer`
/// (or the `Server` / `createSecureServer` aliases) back to the
/// perry-ext-http-server factories. Registered with the runtime via
/// `js_set_native_http_dispatch` under `external-http-server-pump` (enabled
/// whenever the program imports one of those modules), so we can safely
/// `extern "C"`-reference the ext-crate symbols — they're guaranteed linked.
///
/// The method-call form (`http.createServer(...)`) already lowers through the
/// codegen NATIVE_MODULE_TABLE; this only serves the value-read form, where the
/// factory reaches the runtime as a bound-method closure (see
/// `is_native_module_callable_export`) and lands here when invoked.
///
/// Node's overloads are `createServer([options][, requestListener])`, while
/// `@hono/node-server` calls `createServer(serverOptions, requestListener)`. We
/// classify each arg by type rather than position — the function/closure arg is
/// the handler, the remaining object arg is the options — so both orders work.
#[cfg(feature = "external-http-server-pump")]
unsafe extern "C" fn js_node_http_native_dispatch(
    module_ptr: *const u8,
    module_len: usize,
    _method_ptr: *const u8,
    _method_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    use perry_runtime::JSValue;
    extern "C" {
        fn js_node_http_create_server_with_options(handler: i64, options_f64: f64) -> i64;
        fn js_node_https_create_server(opts_f64: f64, handler: i64) -> i64;
        fn js_node_http2_create_secure_server(opts_f64: f64, handler: i64) -> i64;
        fn js_value_is_closure(value_bits: i64) -> i32;
    }
    let undefined = f64::from_bits(JSValue::undefined().bits());
    let module = if module_ptr.is_null() || module_len == 0 {
        ""
    } else {
        std::str::from_utf8(std::slice::from_raw_parts(module_ptr, module_len)).unwrap_or("")
    };
    let arg = |n: usize| -> f64 {
        if n < args_len && !args_ptr.is_null() {
            *args_ptr.add(n)
        } else {
            undefined
        }
    };
    // Disambiguate handler (function/closure) from options (object),
    // independent of argument order.
    let mut handler_ptr: i64 = 0;
    let mut options_f64 = undefined;
    for n in 0..args_len.min(2) {
        let a = arg(n);
        if js_value_is_closure(a.to_bits() as i64) != 0 {
            handler_ptr = perry_runtime::js_nanbox_get_pointer(a);
        } else if JSValue::from_bits(a.to_bits()).is_pointer() {
            options_f64 = a;
        }
    }
    let handle = match module {
        "http" => js_node_http_create_server_with_options(handler_ptr, options_f64),
        "https" => js_node_https_create_server(options_f64, handler_ptr),
        "http2" => js_node_http2_create_secure_server(options_f64, handler_ptr),
        _ => return undefined,
    };
    if handle == 0 {
        undefined
    } else {
        perry_runtime::js_nanbox_pointer(handle)
    }
}

/// Initialize the handle method and property dispatch systems.
/// This registers our dispatch functions with perry-runtime.
/// Must be called before any user code runs.
#[no_mangle]
pub unsafe extern "C" fn js_stdlib_init_dispatch() {
    extern "C" {
        fn js_register_handle_method_dispatch(
            f: unsafe extern "C" fn(i64, *const u8, usize, *const f64, usize) -> f64,
        );
        fn js_register_handle_property_dispatch(
            f: unsafe extern "C" fn(i64, *const u8, usize) -> f64,
        );
        fn js_register_handle_property_set_dispatch(
            f: unsafe extern "C" fn(i64, *const u8, usize, f64),
        );
        fn js_register_event_emitter_handle_probe(f: unsafe extern "C" fn(i64) -> bool);
        fn js_register_event_emitter_on(f: EventEmitterOn);
    }
    js_register_handle_method_dispatch(js_handle_method_dispatch);
    js_register_handle_property_dispatch(js_handle_property_dispatch);
    js_register_handle_property_set_dispatch(js_handle_property_set_dispatch);
    #[cfg(feature = "bundled-events")]
    unsafe extern "C" fn event_emitter_probe(handle: i64) -> bool {
        crate::events::is_event_emitter_handle(handle)
    }
    #[cfg(feature = "bundled-events")]
    js_register_event_emitter_handle_probe(event_emitter_probe);
    #[cfg(feature = "bundled-events")]
    js_register_event_emitter_on(crate::events::js_event_emitter_on);
    perry_runtime::js_set_native_crypto_dispatch(crate::crypto::js_crypto_native_dispatch);
    #[cfg(feature = "compression")]
    perry_runtime::js_set_native_zlib_dispatch(crate::zlib::js_zlib_native_dispatch);
    perry_runtime::js_set_native_querystring_dispatch(
        crate::querystring::js_querystring_native_dispatch,
    );
    perry_runtime::js_set_native_domain_dispatch(crate::domain::js_domain_native_dispatch);

    // #2533: route captured / aliased http/https/http2 `createServer` back to
    // the perry-ext-http-server factories. Only registered when the http ext
    // crate is linked (its symbols are referenced by the dispatcher), so the
    // runtime arm stays null-and-undefined for non-http programs.
    #[cfg(feature = "external-http-server-pump")]
    perry_runtime::js_set_native_http_dispatch(js_node_http_native_dispatch);

    // #1545: register the Web Streams numeric-handle probe so method calls on
    // stream handles whose static type the codegen lost route to the stream
    // dispatch arms in `js_handle_method_dispatch`.
    #[cfg(feature = "bundled-streams")]
    {
        extern "C" {
            fn js_register_stream_handle_probe(f: unsafe extern "C" fn(usize) -> bool);
            fn js_register_stream_handle_kind_probe(f: unsafe extern "C" fn(usize) -> u8);
        }
        unsafe extern "C" fn stream_probe(id: usize) -> bool {
            crate::streams::js_stream_handle_is_registered(id)
        }
        unsafe extern "C" fn stream_kind_probe(id: usize) -> u8 {
            crate::streams::js_stream_handle_kind(id)
        }
        js_register_stream_handle_probe(stream_probe);
        js_register_stream_handle_kind_probe(stream_kind_probe);
        // #1671: back `hono/jsx/streaming`'s `renderToReadableStream` with a
        // real single-chunk Web stream when streams are linked.
        perry_runtime::node_submodules::js_register_jsx_render_stream(
            crate::streams::js_jsx_render_stream_from_value,
        );
    }
}
