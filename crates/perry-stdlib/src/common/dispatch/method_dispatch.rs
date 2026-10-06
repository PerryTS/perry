#[cfg(feature = "crypto")]
use super::super::handle::with_handle;
use super::*;
use crate::common::feature_hooks::{Hook, MethodArm, RawMethodArm};

// One slot per optional-feature position in `js_handle_method_dispatch`, in
// hub order. Filled by the owning feature's install (see `feature_hooks`); an
// empty slot is skipped, which is exactly what the `#[cfg]` that used to gate
// the arm did in a build without that feature.
static RAW_EXTERNAL_HTTP_CLIENT: Hook<RawMethodArm> = Hook::empty();
static ARM_STREAMS: Hook<MethodArm> = Hook::empty();
static ARM_NODEMAILER: Hook<MethodArm> = Hook::empty();
static ARM_NODE_SQLITE: Hook<MethodArm> = Hook::empty();
static ARM_CRYPTO: Hook<MethodArm> = Hook::empty();
static ARM_TLS: Hook<MethodArm> = Hook::empty();
static ARM_SQLITE: Hook<MethodArm> = Hook::empty();
static ARM_HTTP_CLIENT: Hook<MethodArm> = Hook::empty();
static ARM_HTTP_SERVER: Hook<MethodArm> = Hook::empty();
static ARM_HTTP_CLIENT_PAUSE_RESUME: Hook<MethodArm> = Hook::empty();
static ARM_EXTERNAL_NET: Hook<MethodArm> = Hook::empty();
static ARM_FETCH: Hook<MethodArm> = Hook::empty();

/// Route external `Agent` and client-side `IncomingMessage`
/// methods before this dispatcher creates owned copies of the method name and
/// arguments. Well-known wrapper archives carry a private allocator shim;
/// returning from an external HTTP call and then dropping those temporary
/// copies can otherwise free them through the wrapper's allocator copy instead
/// of the stdlib's (#4975).
///
/// These methods either consume their arguments synchronously or only return
/// the receiver, so borrowing the caller-provided slices for the duration of
/// the call is sufficient.
#[cfg(feature = "external-http-client-pump")]
unsafe fn try_dispatch_external_http_client(
    handle: i64,
    method_name_ptr: *const u8,
    method_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if method_name_ptr.is_null() || method_name_len == 0 {
        return None;
    }
    let method_name =
        std::str::from_utf8(std::slice::from_raw_parts(method_name_ptr, method_name_len)).ok()?;

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
        "getName" | "destroy" | "keepSocketAlive" | "reuseSocket" | "createConnection"
    ) && js_ext_http_agent_is_handle(handle) != 0
    {
        return Some(js_ext_http_agent_dispatch_method(
            handle,
            method_name_ptr,
            method_name_len,
            args_ptr,
            args_len,
        ));
    }

    let args = if args_len > 0 && !args_ptr.is_null() {
        std::slice::from_raw_parts(args_ptr, args_len)
    } else {
        &[]
    };
    if !matches!(
        method_name,
        "setEncoding" | "on" | "once" | "addListener" | "pipe" | "pause" | "resume"
    ) {
        return None;
    }

    extern "C" {
        fn js_ext_http_client_incoming_message_is_handle(handle: i64) -> i32;
    }
    if js_ext_http_client_incoming_message_is_handle(handle) == 0 {
        return None;
    }

    if matches!(method_name, "pause" | "resume") {
        return Some(nanbox_handle_value(handle));
    }

    super::super::dispatch_http::dispatch_client_incoming_method(handle, method_name, args)
}

/// Select a GC-stable spelling for known methods in the native stream id band.
#[cfg(feature = "bundled-streams")]
fn static_stream_method_name(handle: i64, name: &[u8]) -> Option<&'static str> {
    if handle < crate::streams::STREAM_HANDLE_ID_START as i64
        || handle >= crate::streams::STREAM_HANDLE_ID_END as i64
    {
        return None;
    }
    match name {
        b"read" => Some("read"),
        b"releaseLock" => Some("releaseLock"),
        b"cancel" => Some("cancel"),
        b"write" => Some("write"),
        b"close" => Some("close"),
        b"abort" => Some("abort"),
        b"getReader" => Some("getReader"),
        b"values" => Some("values"),
        b"@@asyncIterator" => Some("@@asyncIterator"),
        b"tee" => Some("tee"),
        b"pipeTo" => Some("pipeTo"),
        b"pipeThrough" => Some("pipeThrough"),
        b"enqueue" => Some("enqueue"),
        b"terminate" => Some("terminate"),
        b"error" => Some("error"),
        b"getWriter" => Some("getWriter"),
        _ => None,
    }
}

/// Dispatch a method call on a handle-based object.
#[no_mangle]
pub unsafe extern "C" fn js_handle_method_dispatch(
    handle: i64,
    method_name_ptr: *const u8,
    method_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    #[cfg(feature = "bundled-streams")]
    let static_name = if method_name_ptr.is_null() || method_name_len == 0 {
        None
    } else {
        static_stream_method_name(
            handle,
            std::slice::from_raw_parts(method_name_ptr, method_name_len),
        )
    };
    #[cfg(not(feature = "bundled-streams"))]
    let static_name: Option<&'static str> = None;

    // No known stream spelling is an external HTTP client method.
    if static_name.is_none() {
        try_arm!(
            RAW_EXTERNAL_HTTP_CLIENT,
            handle,
            method_name_ptr,
            method_name_len,
            args_ptr,
            args_len,
        );
    }

    let method_name_owned;
    let method_name = if method_name_ptr.is_null() || method_name_len == 0 {
        method_name_owned = String::new();
        method_name_owned.as_str()
    } else {
        match static_name {
            // Static spellings remain valid across moving GC; retain no
            // reference to the caller's name buffer during dispatch.
            Some(name) => name,
            None => {
                let method_bytes = std::slice::from_raw_parts(method_name_ptr, method_name_len);
                method_name_owned = String::from_utf8_lossy(method_bytes).into_owned();
                method_name_owned.as_str()
            }
        }
    };
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    // The receiver's id must stay reachable while its method runs: a
    // GC-reclaimable common handle (#11453) is released at a full trace that
    // finds no word naming it, and a chained temporary receiver
    // (`createHash(a).update(b)`) lives in no JS slot.
    let _receiver =
        scope.root_nanbox_u64(0x7FFD_0000_0000_0000 | (handle as u64 & 0x0000_FFFF_FFFF_FFFF));
    let arg_handles = {
        // Root registration does not run Perry GC. Borrow the caller's buffer
        // only while registering roots, then refresh into owned arguments
        // before any dispatcher can invoke JavaScript or trigger collection.
        let original_args = if args_len > 0 && !args_ptr.is_null() {
            std::slice::from_raw_parts(args_ptr, args_len)
        } else {
            &[]
        };
        scope.root_nanbox_f64_slice(original_args)
    };
    let args = perry_runtime::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
    let _ = method_name;
    let _ = args;
    let _ = handle;

    try_arm!(ARM_STREAMS, handle, method_name, &args);

    // Dispatchers below gate on registry membership plus method vocabulary
    // because native handle id spaces are not unified (#91).

    try_arm!(ARM_NODEMAILER, handle, method_name, &args);

    try_arm!(ARM_NODE_SQLITE, handle, method_name, &args);

    // Fastify app + request/reply context method dispatch lived here when the
    // bundled adapter was compiled into perry-stdlib. fastify now routes entirely
    // through the external perry-ext-fastify crate (well-known flip), whose
    // `app.get(...)` / `reply.send(...)` calls lower via the static
    // NATIVE_MODULE_TABLE rather than this dynamic-handle dispatcher — so no
    // fastify arm is needed here.

    try_arm!(ARM_CRYPTO, handle, method_name, &args);

    try_arm!(ARM_TLS, handle, method_name, &args);

    try_arm!(ARM_SQLITE, handle, method_name, &args);

    try_arm!(ARM_HTTP_CLIENT, handle, method_name, &args);

    try_arm!(ARM_HTTP_SERVER, handle, method_name, &args);

    try_arm!(ARM_HTTP_CLIENT_PAUSE_RESUME, handle, method_name, &args);

    try_arm!(ARM_EXTERNAL_NET, handle, method_name, &args);

    try_arm!(ARM_FETCH, handle, method_name, &args);

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

#[cfg(feature = "bundled-streams")]
unsafe fn arm_streams(handle: i64, method_name: &str, args: &[f64]) -> Option<f64> {
    // #1545: Web Streams handles (readable/writable/transform/reader/writer)
    // live in a dedicated high id range, so this never claims another
    // subsystem's handle. Routes method calls on receivers whose static stream
    // type the codegen lost (`src.pipeThrough(ts).getReader()`, `ts.readable
    // .getReader()`, `const r = rs.getReader(); r.read()`, …).
    if let Some(v) = crate::streams::dispatch_stream_method(handle as f64, method_name, &args) {
        return Some(v);
    }
    None
}

#[cfg(feature = "bundled-nodemailer")]
unsafe fn arm_nodemailer(handle: i64, method_name: &str, args: &[f64]) -> Option<f64> {
    // turnloop P6: `nodemailer.createTransport(...)` returns a bare handle
    // NUMBER, so `transporter.sendMail(...)` / `.verify()` are lowered as
    // generic calls on an untyped receiver and land here. No arm claimed them,
    // so the whole surface answered `TypeError: (number).sendMail is not a
    // function` — on the base commit too, in every call shape. This is the
    // bundled-surface half of the fix; `perry-ext-nodemailer` registers a
    // dispatch EXTENSION for the well-known-flip half, because its handles live
    // in perry-ffi's registry rather than this one.
    if let Some(value) = crate::nodemailer::dispatch_transporter_method(handle, method_name, &args)
    {
        return Some(value);
    }
    None
}

#[cfg(feature = "database-sqlite")]
unsafe fn arm_node_sqlite(handle: i64, method_name: &str, args: &[f64]) -> Option<f64> {
    // bun:sqlite `Database` / `Statement` registry handles (also Bun.SQL's
    // SQLite adapter). Keep these before the better-sqlite3 fallbacks:
    // prepare/exec/close/run/get/all overlap with different semantics.
    // node:sqlite objects never reach this tower: they are ordinary objects
    // whose methods resolve from their prototypes.
    if let Some(result) =
        crate::sqlite::dispatch_bun_sqlite_database_method(handle, method_name, args)
    {
        return Some(result);
    }
    crate::sqlite::dispatch_bun_sqlite_statement_method(handle, method_name, args)
}

#[cfg(feature = "crypto")]
unsafe fn arm_crypto(handle: i64, method_name: &str, args: &[f64]) -> Option<f64> {
    if matches!(method_name, "update" | "sign")
        && with_handle::<crate::crypto::SignHandle, bool, _>(handle, |_| true).unwrap_or(false)
    {
        return Some(crate::crypto::dispatch_sign(handle, method_name, &args));
    }

    if matches!(method_name, "update" | "verify")
        && with_handle::<crate::crypto::VerifyHandle, bool, _>(handle, |_| true).unwrap_or(false)
    {
        return Some(crate::crypto::dispatch_verify(handle, method_name, &args));
    }

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
        return Some(crate::crypto::dispatch_ecdh(handle, method_name, &args));
    }

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
        return Some(crate::crypto::dispatch_diffie_hellman(
            handle,
            method_name,
            &args,
        ));
    }

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
        return Some(crate::crypto::dispatch_x509_method(
            handle,
            method_name,
            &args,
        ));
    }

    // crypto Sign/Verify handle: createSign(alg)/createVerify(alg) followed by
    // .update(...).sign(key) / .verify(key, sig) — issue #1364. Method-gated
    // like the Hash/Cipher handles. `sign`/`verify` are distinctive enough to
    // disambiguate from other registries sharing a handle id.
    if matches!(method_name, "update" | "sign" | "verify")
        && with_handle::<crate::crypto::SignHandle, bool, _>(handle, |_| true).unwrap_or(false)
    {
        return Some(crate::crypto::dispatch_sign(handle, method_name, &args));
    }
    None
}

#[cfg(all(
    feature = "tls-runtime",
    not(target_os = "ios"),
    not(target_os = "android")
))]
unsafe fn arm_tls(handle: i64, method_name: &str, args: &[f64]) -> Option<f64> {
    if crate::tls::should_dispatch_tls_handle(handle, method_name) {
        return Some(crate::tls::dispatch_tls_handle(handle, method_name, &args));
    }
    None
}

#[cfg(feature = "database-sqlite")]
unsafe fn arm_sqlite(handle: i64, method_name: &str, args: &[f64]) -> Option<f64> {
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
    // Handle id spaces are not unified, so a `get` on any other handle (a
    // fetch Response's `headers`, …) reaches this arm too. Claim only handles
    // the sqlite registry owns: since #11919 the sqlite entry points throw
    // "The database connection is not open" for a handle they do not know.
    extern "C" {
        fn js_sqlite_is_stmt_handle(handle: i64) -> i32;
        fn js_sqlite_is_db_handle(handle: i64) -> i32;
    }
    if matches!(method_name, "raw" | "all" | "get" | "run") && js_sqlite_is_stmt_handle(handle) != 0
    {
        let result = dispatch_sqlite_stmt(handle, method_name, &args);
        if result.to_bits() != perry_runtime::JSValue::undefined().bits() {
            return Some(result);
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
    // by other registries (SignHandle, FastifyApp, etc.).
    if matches!(method_name, "prepare" | "exec" | "close") && js_sqlite_is_db_handle(handle) != 0 {
        let result = dispatch_sqlite_db(handle, method_name, &args);
        if result.to_bits() != perry_runtime::JSValue::undefined().bits() {
            return Some(result);
        }
    }
    None
}

#[cfg(feature = "external-http-client-pump")]
unsafe fn arm_http_client(handle: i64, method_name: &str, args: &[f64]) -> Option<f64> {
    if let Some(value) = unsafe {
        super::super::dispatch_http::dispatch_client_request_method(handle, method_name, &args)
    } {
        return Some(value);
    }

    if let Some(value) = unsafe {
        super::super::dispatch_http::dispatch_client_incoming_method(handle, method_name, &args)
    } {
        return Some(value);
    }
    None
}

#[cfg(feature = "external-http-server-pump")]
unsafe fn arm_http_server(handle: i64, method_name: &str, args: &[f64]) -> Option<f64> {
    // External http-server path (#2153): when `node:http` / `node:https` /
    // `node:http2` routes through perry-ext-http, the HttpServer handle
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
    // Method-gated so a handle id reused by another registry (SignHandle,
    // FastifyApp, …) doesn't misroute. The list mirrors the
    // `class_filter: Some("HttpServer")` rows in http.rs.
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
            return Some(unsafe {
                js_ext_http_server_dispatch_method(
                    handle,
                    method_name.as_ptr(),
                    method_name.len(),
                    args.as_ptr(),
                    args.len(),
                )
            });
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
            return Some(unsafe {
                js_ext_http_incoming_message_dispatch_method(
                    handle,
                    method_name.as_ptr(),
                    method_name.len(),
                    args.as_ptr(),
                    args.len(),
                )
            });
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
                | "__get_destroyed"
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
            return Some(unsafe {
                js_ext_http_server_response_dispatch_method(
                    handle,
                    method_name.as_ptr(),
                    method_name.len(),
                    args.as_ptr(),
                    args.len(),
                )
            });
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
            return Some(unsafe {
                js_ext_http2_session_dispatch_method(
                    handle,
                    method_name.as_ptr(),
                    method_name.len(),
                    args.as_ptr(),
                    args.len(),
                )
            });
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
            return Some(unsafe {
                js_ext_http2_stream_dispatch_method(
                    handle,
                    method_name.as_ptr(),
                    method_name.len(),
                    args.as_ptr(),
                    args.len(),
                )
            });
        }
    }
    None
}

#[cfg(feature = "external-http-client-pump")]
unsafe fn arm_http_client_pause_resume(
    handle: i64,
    method_name: &str,
    _args: &[f64],
) -> Option<f64> {
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
    {
        extern "C" {
            fn js_ext_http_client_incoming_message_is_handle(handle: i64) -> i32;
        }
        if matches!(method_name, "pause" | "resume")
            && unsafe { js_ext_http_client_incoming_message_is_handle(handle) } != 0
        {
            return Some(nanbox_handle_value(handle));
        }
    }
    None
}

#[cfg(all(
    feature = "external-net-pump",
    not(target_os = "ios"),
    not(target_os = "android")
))]
unsafe fn arm_external_net(handle: i64, method_name: &str, args: &[f64]) -> Option<f64> {
    // External net path (v0.5.581): perry-ext-net registers itself when
    // the well-known flip strips bundled-net. Same dispatch contract,
    // but routes through extern "C" symbols perry-ext-net provides.
    {
        extern "C" {
            fn js_ext_net_is_socket_handle(handle: i64) -> i32;
        }
        if unsafe { js_ext_net_is_socket_handle(handle) } != 0 {
            return Some(dispatch_external_net_socket(handle, method_name, &args));
        }
        if let Some(v) = crate::common::net_method_values::dispatch_external_server_method(
            handle,
            method_name,
            &args,
        ) {
            return Some(v);
        }
        if let Some(v) = crate::common::net_method_values::dispatch_external_block_list_method(
            handle,
            method_name,
            &args,
        ) {
            return Some(v);
        }
    }
    None
}

#[cfg(feature = "web-fetch")]
unsafe fn arm_fetch(handle: i64, method_name: &str, args: &[f64]) -> Option<f64> {
    // Web Fetch method dispatch (refs #421 — Phase 1 of the handle-NaN-boxing
    // unification). When user code does `res.text()` / `res.json()` / etc. on
    // an any-typed Response handle (typical of npm packages with stripped TS
    // types — hono's `await app.fetch(req)` returns an any-typed value;
    // user-side `await res.text()` ends up here), the call lands in
    // `js_native_call_method` → small-handle range check → here. Each helper
    // does its own registry-membership + property-name gate; `None` means
    // "not us, try the next dispatcher or return undefined".
    {
        // #1698: Request body methods (`req.json()`/`.text()`/`.arrayBuffer()`)
        // on an any-typed / computed-key receiver. Hono's `HonoRequest.#cachedBody`
        // does `raw[key]()` (computed key) on the underlying Request, which loses
        // the static type and lands here. Fetch-family ids are unified, so the
        // registry-membership gate inside cleanly distinguishes a Request from a
        // Response with the (formerly colliding) same id.
        if let Some(v) = crate::fetch::dispatch_request_method(handle as usize, method_name, &args)
        {
            return Some(v);
        }
        if let Some(v) = crate::fetch::dispatch_response_method(handle as usize, method_name, &args)
        {
            return Some(v);
        }
        if let Some(v) =
            crate::fetch::dispatch_form_data_method(handle as usize, method_name, &args)
        {
            return Some(v);
        }
        if let Some(v) = crate::fetch::dispatch_blob_method(handle as usize, method_name, &args) {
            return Some(v);
        }
        if let Some(v) = crate::fetch::dispatch_headers_method(handle as usize, method_name, &args)
        {
            return Some(v);
        }
    }
    None
}

// Per-feature slot fills, called from the owning feature's install
// (`super::install_*`, reached from `feature_hooks`).
#[cfg(feature = "bundled-streams")]
pub(super) fn install_streams() {
    ARM_STREAMS.set(arm_streams);
}
#[cfg(feature = "bundled-nodemailer")]
pub(super) fn install_nodemailer() {
    ARM_NODEMAILER.set(arm_nodemailer);
}
#[cfg(feature = "database-sqlite")]
pub(super) fn install_sqlite() {
    ARM_NODE_SQLITE.set(arm_node_sqlite);
    ARM_SQLITE.set(arm_sqlite);
}
#[cfg(feature = "crypto")]
pub(super) fn install_crypto() {
    ARM_CRYPTO.set(arm_crypto);
}
#[cfg(all(
    feature = "tls-runtime",
    not(target_os = "ios"),
    not(target_os = "android")
))]
pub(super) fn install_tls() {
    ARM_TLS.set(arm_tls);
}

#[cfg(feature = "external-http-client-pump")]
pub(super) fn install_external_http_client() {
    RAW_EXTERNAL_HTTP_CLIENT.set(try_dispatch_external_http_client);
    ARM_HTTP_CLIENT.set(arm_http_client);
    ARM_HTTP_CLIENT_PAUSE_RESUME.set(arm_http_client_pause_resume);
}
#[cfg(feature = "external-http-server-pump")]
pub(super) fn install_external_http_server() {
    ARM_HTTP_SERVER.set(arm_http_server);
}
#[cfg(all(
    feature = "external-net-pump",
    not(target_os = "ios"),
    not(target_os = "android")
))]
pub(super) fn install_external_net() {
    ARM_EXTERNAL_NET.set(arm_external_net);
}
#[cfg(feature = "web-fetch")]
pub(super) fn install_fetch() {
    ARM_FETCH.set(arm_fetch);
}
