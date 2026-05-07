//! Handle-based method dispatch for perry-stdlib
//!
//! When native modules (Fastify, ioredis, etc.) use handle-based objects,
//! and those handles are passed to functions as generic parameters,
//! the codegen can't statically determine the type. This module provides
//! runtime dispatch by checking the handle type in the registry.

use super::handle::*;

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
    let method_name = if method_name_ptr.is_null() || method_name_len == 0 {
        ""
    } else {
        std::str::from_utf8(std::slice::from_raw_parts(method_name_ptr, method_name_len))
            .unwrap_or("")
    };
    let args: &[f64] = if args_len > 0 && !args_ptr.is_null() {
        std::slice::from_raw_parts(args_ptr, args_len)
    } else {
        &[]
    };
    // `_` prefixes silence unused-variable warnings when every dispatch
    // arm below is compiled out (e.g. minimal-stdlib without http-server
    // / database-redis).
    let _ = method_name;
    let _ = args;
    let _ = handle;

    // Each dispatcher below is gated on TWO conditions: (a) its registry
    // currently holds this handle id, AND (b) the method name is one this
    // dispatcher actually handles. Both are required because handle id
    // namespaces are not unified — `net.createConnection` uses its own
    // `NEXT_NET_ID` counter, separate from the common HANDLES registry that
    // backs Fastify/ioredis/HashHandle. A net.Socket at id=1 always
    // collides with the first object created in the common registry. If we
    // claimed a handle on registry match alone, calling `socket.write(b)` on
    // a socket whose id collided with a HashHandle would route to
    // `dispatch_hash` (registry says yes), find no `write` arm, and silently
    // return undefined — the bytes never reach the wire (#91). Gating on
    // method-name vocabulary lets the call fall through to the next
    // dispatcher when a handle id is reused across registries with disjoint
    // method sets. The proper long-term fix is a single unified id space;
    // this is the surgical version.

    // Fastify app: routes for HTTP verbs + lifecycle methods.
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
    ) && with_handle::<crate::fastify::FastifyApp, bool, _>(handle, |_| true).unwrap_or(false)
    {
        return dispatch_fastify_app(handle, method_name, args);
    }

    // Fastify request/reply context.
    #[cfg(feature = "http-server")]
    if matches!(
        method_name,
        "send"
            | "status"
            | "code"
            | "header"
            | "method"
            | "url"
            | "body"
            | "json"
            | "params"
            | "headers"
    ) && with_handle::<crate::fastify::FastifyContext, bool, _>(handle, |_| true)
        .unwrap_or(false)
    {
        return dispatch_fastify_context(handle, method_name, args);
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
        return dispatch_ioredis(handle, method_name, args);
    }

    // crypto Hash handle: createHash(...).update(...).digest().
    // The order vs. net (below) does not matter once method-gated, but we
    // keep hash before net to avoid changing the priority of in-registry
    // matches relative to the v0.5.98/#88 ordering.
    #[cfg(feature = "crypto")]
    if matches!(method_name, "update" | "digest")
        && with_handle::<crate::crypto::HashHandle, bool, _>(handle, |_| true).unwrap_or(false)
    {
        return crate::crypto::dispatch_hash(handle, method_name, args);
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
        return dispatch_net_socket(handle, method_name, args);
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
            return dispatch_external_net_socket(handle, method_name, args);
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
        if let Some(v) =
            crate::fetch::dispatch_response_method(handle as usize, method_name, args)
        {
            return v;
        }
        if let Some(v) =
            crate::fetch::dispatch_blob_method(handle as usize, method_name, args)
        {
            return v;
        }
        if let Some(v) =
            crate::fetch::dispatch_headers_method(handle as usize, method_name, args)
        {
            return v;
        }
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
            crate::net::js_net_socket_write(handle, unbox_to_i64(args[0]));
            f64::from_bits(0x7FFC_0000_0000_0001) // undefined
        }
        "end" => {
            crate::net::js_net_socket_end(handle);
            f64::from_bits(0x7FFC_0000_0000_0001)
        }
        "destroy" => {
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
    extern "C" {
        fn js_net_socket_write(handle: i64, buf_ptr: i64);
        fn js_net_socket_end(handle: i64);
        fn js_net_socket_destroy(handle: i64);
        fn js_net_socket_on(handle: i64, event_ptr: i64, cb_ptr: i64);
        fn js_net_socket_method_connect(handle: i64, port: f64, host_ptr: i64);
        fn js_net_socket_upgrade_tls(
            handle: i64,
            servername_ptr: i64,
            verify: f64,
        ) -> *mut perry_runtime::Promise;
    }

    match method {
        "write" if !args.is_empty() => {
            js_net_socket_write(handle, unbox_to_i64(args[0]));
            f64::from_bits(0x7FFC_0000_0000_0001)
        }
        "end" => {
            js_net_socket_end(handle);
            f64::from_bits(0x7FFC_0000_0000_0001)
        }
        "destroy" => {
            js_net_socket_destroy(handle);
            f64::from_bits(0x7FFC_0000_0000_0001)
        }
        "on" if args.len() >= 2 => {
            let event_ptr = unbox_to_i64(args[0]);
            let cb_ptr = unbox_to_i64(args[1]);
            js_net_socket_on(handle, event_ptr, cb_ptr);
            f64::from_bits(0x7FFC_0000_0000_0001)
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

    // Unknown handle type - return undefined
    f64::from_bits(0x7FFC_0000_0000_0001)
}

/// Dispatch method calls on ioredis Redis client handles
#[cfg(feature = "database-redis")]
unsafe fn dispatch_ioredis(handle: i64, method: &str, args: &[f64]) -> f64 {
    // Helper: extract raw StringHeader pointer from NaN-boxed f64
    fn get_string_ptr(val: f64) -> *const perry_runtime::StringHeader {
        let bits = val.to_bits();
        // Strip STRING_TAG (0x7FFF) to get raw pointer
        (bits & 0x0000_FFFF_FFFF_FFFF) as *const perry_runtime::StringHeader
    }

    // Helper: NaN-box a Promise pointer with POINTER_TAG for return
    fn nanbox_promise(promise: *mut perry_runtime::Promise) -> f64 {
        let bits = (promise as u64) | 0x7FFD_0000_0000_0000;
        f64::from_bits(bits)
    }

    match method {
        "connect" => {
            let promise = crate::ioredis::js_ioredis_connect(handle);
            nanbox_promise(promise)
        }
        "get" if !args.is_empty() => {
            let key_ptr = get_string_ptr(args[0]);
            let promise = crate::ioredis::js_ioredis_get(handle, key_ptr);
            nanbox_promise(promise)
        }
        "set" if args.len() >= 2 => {
            let key_ptr = get_string_ptr(args[0]);
            let value_ptr = get_string_ptr(args[1]);
            let promise = crate::ioredis::js_ioredis_set(handle, key_ptr, value_ptr);
            nanbox_promise(promise)
        }
        "setex" if args.len() >= 3 => {
            let key_ptr = get_string_ptr(args[0]);
            let seconds = args[1];
            let value_ptr = get_string_ptr(args[2]);
            let promise = crate::ioredis::js_ioredis_setex(handle, key_ptr, seconds, value_ptr);
            nanbox_promise(promise)
        }
        "del" if !args.is_empty() => {
            let key_ptr = get_string_ptr(args[0]);
            let promise = crate::ioredis::js_ioredis_del(handle, key_ptr);
            nanbox_promise(promise)
        }
        "exists" if !args.is_empty() => {
            let key_ptr = get_string_ptr(args[0]);
            let promise = crate::ioredis::js_ioredis_exists(handle, key_ptr);
            nanbox_promise(promise)
        }
        "incr" if !args.is_empty() => {
            let key_ptr = get_string_ptr(args[0]);
            let promise = crate::ioredis::js_ioredis_incr(handle, key_ptr);
            nanbox_promise(promise)
        }
        "decr" if !args.is_empty() => {
            let key_ptr = get_string_ptr(args[0]);
            let promise = crate::ioredis::js_ioredis_decr(handle, key_ptr);
            nanbox_promise(promise)
        }
        "expire" if args.len() >= 2 => {
            let key_ptr = get_string_ptr(args[0]);
            let seconds = args[1];
            let promise = crate::ioredis::js_ioredis_expire(handle, key_ptr, seconds);
            nanbox_promise(promise)
        }
        "ping" => {
            let promise = crate::ioredis::js_ioredis_ping(handle);
            nanbox_promise(promise)
        }
        "quit" => {
            let promise = crate::ioredis::js_ioredis_quit(handle);
            nanbox_promise(promise)
        }
        "disconnect" => {
            crate::ioredis::js_ioredis_disconnect(handle);
            f64::from_bits(0x7FFC_0000_0000_0001) // undefined
        }
        _ => {
            f64::from_bits(0x7FFC_0000_0000_0001) // undefined
        }
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
    }
    js_register_handle_method_dispatch(js_handle_method_dispatch);
    js_register_handle_property_dispatch(js_handle_property_dispatch);
    js_register_handle_property_set_dispatch(js_handle_property_set_dispatch);
}
