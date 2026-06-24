use super::super::handle::*;
use super::*;

/// Dispatch method calls on Fastify app handles
#[cfg(feature = "http-server")]
pub(crate) unsafe fn dispatch_fastify_app(handle: i64, method: &str, args: &[f64]) -> f64 {
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
                TAG_UNDEFINED_F64
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
            TAG_UNDEFINED_F64 // undefined (void)
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
            TAG_UNDEFINED_F64 // undefined (void)
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
            TAG_UNDEFINED_F64
        }
    }
}

/// Dispatch method calls on Fastify context handles (request/reply)
#[cfg(feature = "http-server")]
pub(crate) unsafe fn dispatch_fastify_context(handle: i64, method: &str, args: &[f64]) -> f64 {
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
            TAG_UNDEFINED_F64
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
pub(crate) unsafe fn dispatch_net_socket(handle: i64, method: &str, args: &[f64]) -> f64 {
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
        "getTypeOfService" => crate::net::js_net_socket_get_type_of_service(handle),
        "setTypeOfService" => {
            let value = args
                .first()
                .copied()
                .unwrap_or(f64::from_bits(0x7FFC_0000_0000_0001));
            crate::net::js_net_socket_set_type_of_service(handle, value);
            f64::from_bits(0x7FFD_0000_0000_0000u64 | (handle as u64 & 0x0000_FFFF_FFFF_FFFF))
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
pub(crate) unsafe fn dispatch_zlib_stream(handle: i64, method: &str, args: &[f64]) -> f64 {
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
pub(crate) unsafe fn dispatch_external_net_socket(handle: i64, method: &str, args: &[f64]) -> f64 {
    fn unbox_to_i64(v: f64) -> i64 {
        (v.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64
    }
    fn nanbox_handle(h: i64) -> f64 {
        f64::from_bits(0x7FFD_0000_0000_0000u64 | (h as u64 & 0x0000_FFFF_FFFF_FFFF))
    }
    extern "C" {
        // #5021 — route write/end/destroy through perry-ext-net's DISTINCT
        // `js_ext_net_*` symbols, NOT the shared `js_net_socket_*` names. The
        // bundled stdlib net exports same-named twins, so in a workspace /
        // jsruntime build the shared names bind to the bundled twin's EMPTY
        // socket registry and the command (write bytes / FIN / teardown) is
        // silently dropped — no `write()` syscall ever fires. The distinct
        // symbols have no twin and always reach ext-net's own registry.
        // Mirrors how `js_ext_net_destroy_socket` was already split out (#5010).
        fn js_ext_net_socket_write(handle: i64, buf_ptr: i64);
        // Issue #1852 — `js_ext_net_socket_end` takes the optional final
        // chunk (NA_JSV bits) so `socket.end(data)` writes before FIN.
        fn js_ext_net_socket_end(handle: i64, chunk_bits: i64);
        fn js_ext_net_destroy_socket(handle: i64);
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
        fn js_net_socket_get_type_of_service(handle: i64) -> f64;
        fn js_net_socket_set_type_of_service(handle: i64, value: f64) -> i64;
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
            // pre-stripped pointer. ext-net's write probes Buffer-vs-string
            // itself. #5021 — distinct symbol so the bytes can't be dropped
            // into the bundled twin's empty registry.
            js_ext_net_socket_write(handle, args[0].to_bits() as i64);
            f64::from_bits(0x7FFC_0000_0000_0001)
        }
        "end" => {
            // Issue #1852 — forward the optional `socket.end(data)` chunk;
            // pad with `undefined` for the no-arg `socket.end()` form.
            let chunk = args
                .first()
                .copied()
                .unwrap_or(f64::from_bits(0x7FFC_0000_0000_0001));
            js_ext_net_socket_end(handle, chunk.to_bits() as i64);
            f64::from_bits(0x7FFC_0000_0000_0001)
        }
        "destroy" | "destroySoon" => {
            js_ext_net_destroy_socket(handle);
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
        "getTypeOfService" => js_net_socket_get_type_of_service(handle),
        "setTypeOfService" => {
            let value = args
                .first()
                .copied()
                .unwrap_or(f64::from_bits(0x7FFC_0000_0000_0001));
            js_net_socket_set_type_of_service(handle, value);
            nanbox_handle(handle)
        }
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
