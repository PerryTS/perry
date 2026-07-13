use super::super::handle::*;
use super::*;

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

    #[cfg(feature = "database-sqlite")]
    if crate::sqlite::dispatch_node_sqlite_limits_set(handle, property_name, value) {
        return;
    }

    if crate::common::net_method_values::dispatch_property_set(handle, property_name, value) {
        return;
    }

    // External-fastify `request.user = …` setter. fastify is served by
    // perry-ext-fastify (the bundled in-stdlib adapter was removed); the handle
    // lives in its perry-ffi registry, so probe membership via the external symbol
    // and forward to its setter — the write counterpart of the
    // `js_fastify_req_get_user_data` read arm in `js_handle_property_dispatch`, so
    // a later `request.user` read sees the value rather than missing the generic
    // expando store. Statically-typed inline sets lower via codegen's
    // NATIVE_MODULE_TABLE; this covers the erased-receiver dynamic case.
    #[cfg(feature = "external-fastify-pump")]
    if property_name == "user" {
        extern "C" {
            fn js_ext_fastify_is_context_handle(handle: i64) -> i32;
            fn js_fastify_req_set_user_data(handle: i64, value: f64);
        }
        if unsafe { js_ext_fastify_is_context_handle(handle) } != 0 {
            unsafe { js_fastify_req_set_user_data(handle, value) };
            // Claimed by the typed setter — don't also write a stale expando copy.
            return;
        }
    }

    #[cfg(feature = "external-http-server-pump")]
    if matches!(
        property_name,
        "statusCode" | "statusMessage" | "sendDate" | "strictContentLength"
    ) {
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
            // Claimed by the typed setter — don't also write a stale expando copy.
            return;
        }
    }

    // #4904: Agent tunables (`agent.maxSockets = 4`) and the
    // `agent.createConnection = fn` monkeypatch pattern Node's tests use.
    #[cfg(feature = "http-client")]
    if crate::http::dispatch_agent_property_set(handle, property_name, value) {
        return;
    }
    #[cfg(feature = "external-http-client-pump")]
    if matches!(
        property_name,
        "maxSockets"
            | "maxFreeSockets"
            | "maxTotalSockets"
            | "keepAliveMsecs"
            | "keepAlive"
            | "createConnection"
            | "createSocket"
    ) {
        extern "C" {
            fn js_ext_http_agent_is_handle(handle: i64) -> i32;
            fn js_ext_http_agent_dispatch_property_set(
                handle: i64,
                property_ptr: *const u8,
                property_len: usize,
                value: f64,
            ) -> i32;
        }
        if unsafe { js_ext_http_agent_is_handle(handle) } != 0 {
            unsafe {
                js_ext_http_agent_dispatch_property_set(
                    handle,
                    property_name.as_ptr(),
                    property_name.len(),
                    value,
                );
            }
            return;
        }
    }

    // #4904: `req.connection = v` / `req.socket = v` on an IncomingMessage —
    // Node's `connection` accessor writes `this.socket`.
    #[cfg(feature = "external-http-server-pump")]
    if matches!(property_name, "socket" | "connection") {
        extern "C" {
            fn js_ext_http_incoming_message_is_handle(handle: i64) -> i32;
            fn js_ext_http_incoming_message_dispatch_property_set(
                handle: i64,
                property_ptr: *const u8,
                property_len: usize,
                value: f64,
            ) -> i32;
        }

        if unsafe { js_ext_http_incoming_message_is_handle(handle) } != 0 {
            unsafe {
                js_ext_http_incoming_message_dispatch_property_set(
                    handle,
                    property_name.as_ptr(),
                    property_name.len(),
                    value,
                );
            }
            return;
        }
    }

    // Generic per-handle expando store: an ARBITRARY user-assigned own property
    // (`handle.colors = [...]`) that none of the typed setters above claimed.
    // Native HANDLE values are ordinary, extensible objects in Node; this gives
    // them the same string-keyed own-property storage closures get from
    // `CLOSURE_PROPS`. The read half (`js_handle_property_dispatch`) consults
    // every typed property FIRST and only falls back to this expando table, so a
    // typed property name can never be shadowed by an expando copy. This is what
    // makes `debug`'s `createDebug.colors = [...]` persist and read back (the
    // wall: a Blob/Response-tagged `_` whose `.colors` write was silently
    // dropped, so `selectColor` read `undefined`).
    if !property_name.is_empty() {
        perry_runtime::object::handle_expando::handle_expando_set(handle, property_name, value);
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_handle_own_property_names_dispatch(handle: i64) -> f64 {
    if crate::string_decoder::is_string_decoder_handle(handle) {
        return crate::string_decoder::string_decoder_own_property_names(handle);
    }
    f64::from_bits(perry_runtime::JSValue::undefined().bits())
}

#[no_mangle]
pub unsafe extern "C" fn js_handle_prototype_dispatch(handle: i64) -> f64 {
    if crate::string_decoder::is_string_decoder_handle(handle) {
        return crate::string_decoder::string_decoder_prototype_value();
    }
    f64::from_bits(perry_runtime::JSValue::undefined().bits())
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
    method_ptr: *const u8,
    method_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    use perry_runtime::JSValue;
    extern "C" {
        fn js_node_http_create_server_with_options(first_arg: f64, second_arg: f64) -> i64;
        fn js_node_http_outgoing_message_new() -> i64;
        fn js_node_https_create_server(opts_f64: f64, handler: i64) -> i64;
        fn js_node_http2_create_server(first_arg: f64, second_arg: f64) -> i64;
        fn js_node_http2_create_secure_server(opts_f64: f64, handler: i64) -> i64;
        fn js_value_is_closure(value_bits: i64) -> i32;
    }
    let undefined = f64::from_bits(JSValue::undefined().bits());
    let module = if module_ptr.is_null() || module_len == 0 {
        ""
    } else {
        std::str::from_utf8(std::slice::from_raw_parts(module_ptr, module_len)).unwrap_or("")
    };
    let method = if method_ptr.is_null() || method_len == 0 {
        ""
    } else {
        std::str::from_utf8(std::slice::from_raw_parts(method_ptr, method_len)).unwrap_or("")
    };
    let arg = |n: usize| -> f64 {
        if n < args_len && !args_ptr.is_null() {
            *args_ptr.add(n)
        } else {
            undefined
        }
    };
    if module == "http" && method == "OutgoingMessage" {
        let handle = js_node_http_outgoing_message_new();
        return if handle == 0 {
            undefined
        } else {
            perry_runtime::js_nanbox_pointer(handle)
        };
    }
    // #4904: Node exposes Agent / ClientRequest / IncomingMessage /
    // ServerResponse as constructable classes. Construction through any
    // value/aliasing path (`const { Agent } = require('http')`,
    // `new http.IncomingMessage(socket)`, …) lands here via the
    // class_registry http construct arm.
    if module == "http" && method == "IncomingMessage" {
        extern "C" {
            fn js_node_http_incoming_message_standalone_new(socket: f64) -> i64;
        }
        let handle = js_node_http_incoming_message_standalone_new(arg(0));
        return if handle == 0 {
            undefined
        } else {
            perry_runtime::js_nanbox_pointer(handle)
        };
    }
    if module == "http" && method == "ServerResponse" {
        extern "C" {
            fn js_node_http_server_response_standalone_new(req: f64) -> i64;
        }
        let handle = js_node_http_server_response_standalone_new(arg(0));
        return if handle == 0 {
            undefined
        } else {
            perry_runtime::js_nanbox_pointer(handle)
        };
    }
    #[cfg(feature = "external-http-client-pump")]
    {
        extern "C" {
            fn js_http_agent_new(options_f64: f64) -> i64;
            fn js_https_agent_new(options_f64: f64) -> i64;
            fn js_http_client_request_standalone_new(options_f64: f64) -> i64;
            fn js_http_get(arg_f64: f64, callback_i64: i64) -> i64;
            fn js_https_get(arg_f64: f64, callback_i64: i64) -> i64;
            fn js_http_request(opts_f64: f64, callback_i64: i64) -> i64;
            fn js_https_request(opts_f64: f64, callback_i64: i64) -> i64;
        }
        // #4904: captured / aliased `get` / `request` (`const { get } =
        // require('http')`). The first non-closure arg is the options/url,
        // the first closure-valued arg is the response callback.
        if matches!(method, "get" | "request") && matches!(module, "http" | "https") {
            let mut options = undefined;
            let mut callback: i64 = 0;
            for n in 0..args_len.min(3) {
                let a = arg(n);
                if callback == 0 && js_value_is_closure(a.to_bits() as i64) != 0 {
                    callback = perry_runtime::js_nanbox_get_pointer(a);
                } else if JSValue::from_bits(a.to_bits()).is_undefined() {
                    continue;
                } else if options.to_bits() == undefined.to_bits() {
                    options = a;
                }
            }
            let handle = match (module, method) {
                ("http", "get") => js_http_get(options, callback),
                ("http", "request") => js_http_request(options, callback),
                ("https", "get") => js_https_get(options, callback),
                _ => js_https_request(options, callback),
            };
            return if handle == 0 {
                undefined
            } else {
                perry_runtime::js_nanbox_pointer(handle)
            };
        }
        if method == "Agent" && (module == "http" || module == "https") {
            let handle = if module == "https" {
                js_https_agent_new(arg(0))
            } else {
                js_http_agent_new(arg(0))
            };
            return if handle == 0 {
                undefined
            } else {
                perry_runtime::js_nanbox_pointer(handle)
            };
        }
        if module == "http" && method == "ClientRequest" {
            let handle = js_http_client_request_standalone_new(arg(0));
            return if handle == 0 {
                undefined
            } else {
                perry_runtime::js_nanbox_pointer(handle)
            };
        }
    }
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
    let handler_f64 = if handler_ptr == 0 {
        undefined
    } else {
        perry_runtime::js_nanbox_pointer(handler_ptr)
    };
    let handle = match module {
        "http" => js_node_http_create_server_with_options(options_f64, handler_f64),
        "https" => js_node_https_create_server(options_f64, handler_ptr),
        "http2" if method == "createSecureServer" => {
            js_node_http2_create_secure_server(options_f64, handler_ptr)
        }
        "http2" => js_node_http2_create_server(options_f64, handler_f64),
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
        fn js_register_handle_own_property_names_dispatch(f: unsafe extern "C" fn(i64) -> f64);
        fn js_register_handle_prototype_dispatch(f: unsafe extern "C" fn(i64) -> f64);
        fn js_register_event_emitter_handle_probe(f: unsafe extern "C" fn(i64) -> bool);
        fn js_register_event_emitter_async_resource_handle_probe(
            f: unsafe extern "C" fn(i64) -> bool,
        );
        fn js_register_event_emitter_on(f: EventEmitterOn);
        #[cfg(feature = "web-fetch")]
        fn js_register_global_fetch_with_options(
            f: unsafe extern "C" fn(
                *const perry_runtime::StringHeader,
                *const perry_runtime::StringHeader,
                *const perry_runtime::StringHeader,
                *const perry_runtime::StringHeader,
            ) -> *mut perry_runtime::Promise,
        );
        #[cfg(feature = "web-fetch")]
        fn js_register_global_fetch_constructors(
            blob_new: unsafe extern "C" fn(f64, f64) -> f64,
            file_new: unsafe extern "C" fn(f64, f64, f64, f64) -> f64,
            headers_new: extern "C" fn() -> f64,
            headers_init_from_value: unsafe extern "C" fn(f64, f64) -> f64,
            request_new: unsafe extern "C" fn(
                *const perry_runtime::StringHeader,
                *const perry_runtime::StringHeader,
                *const perry_runtime::StringHeader,
                f64,
                *const perry_runtime::StringHeader,
                *const perry_runtime::StringHeader,
                *const perry_runtime::StringHeader,
                *const perry_runtime::StringHeader,
                *const perry_runtime::StringHeader,
                *const perry_runtime::StringHeader,
                *const perry_runtime::StringHeader,
                f64,
                *const perry_runtime::StringHeader,
                f64,
            ) -> f64,
            response_new: unsafe extern "C" fn(
                *const perry_runtime::StringHeader,
                f64,
                *const perry_runtime::StringHeader,
                f64,
            ) -> f64,
            response_static_json: unsafe extern "C" fn(
                f64,
                f64,
                *const perry_runtime::StringHeader,
                f64,
            ) -> f64,
            response_static_redirect: unsafe extern "C" fn(
                *const perry_runtime::StringHeader,
                f64,
            ) -> f64,
            response_static_error: extern "C" fn() -> f64,
        );
        #[cfg(feature = "web-fetch")]
        fn js_register_global_fetch_body_init_ptr(f: extern "C" fn(f64) -> i64);
        // #4965: Headers → `res.setHeaders` entries-JSON producer.
        #[cfg(feature = "http-client")]
        fn js_register_global_headers_entries_json(
            f: extern "C" fn(f64) -> *mut perry_runtime::StringHeader,
        );
        // Headers → flat `{name:value}` object-JSON producer for the
        // `fetch(url, { headers: Headers })` request path (avoids the
        // `js_json_stringify`-on-handle SIGSEGV).
        #[cfg(feature = "web-fetch")]
        fn js_register_global_headers_object_json(
            f: extern "C" fn(f64) -> *mut perry_runtime::StringHeader,
        );
        fn js_register_worker_threads_namespace_getters(
            worker_data: extern "C" fn() -> f64,
            is_main_thread: extern "C" fn() -> f64,
            parent_port: extern "C" fn() -> f64,
            thread_name: extern "C" fn() -> f64,
            resource_limits: extern "C" fn() -> f64,
        );
        fn js_register_worker_threads_messaging_constructors(
            message_channel: extern "C" fn() -> f64,
            broadcast_channel: extern "C" fn(f64) -> f64,
        );
    }
    js_register_handle_method_dispatch(js_handle_method_dispatch);
    js_register_handle_property_dispatch(js_handle_property_dispatch);
    js_register_handle_property_set_dispatch(js_handle_property_set_dispatch);
    js_register_handle_own_property_names_dispatch(js_handle_own_property_names_dispatch);
    js_register_handle_prototype_dispatch(js_handle_prototype_dispatch);
    crate::string_decoder::string_decoder_prototype_value();
    #[cfg(feature = "web-fetch")]
    js_register_global_fetch_with_options(crate::fetch::js_fetch_with_options);
    #[cfg(feature = "web-fetch")]
    js_register_global_fetch_constructors(
        crate::fetch_blob::js_blob_new,
        crate::fetch_blob::js_file_new,
        crate::fetch::js_headers_new,
        crate::fetch::js_headers_init_from_value,
        crate::fetch::js_request_new,
        crate::fetch::js_response_new,
        crate::fetch::js_response_static_json,
        crate::fetch::js_response_static_redirect,
        crate::fetch::js_response_static_error,
    );
    #[cfg(feature = "web-fetch")]
    js_register_global_fetch_body_init_ptr(crate::fetch::js_response_body_init_ptr);
    #[cfg(feature = "http-client")]
    js_register_global_headers_entries_json(crate::fetch::js_headers_setheaders_entries_json);
    #[cfg(feature = "web-fetch")]
    js_register_global_headers_object_json(crate::fetch::js_headers_fetch_object_json);
    // Probe / `on` hook / constructor all route through the shared
    // `extern "C"` events surface declared above dispatch_event_emitter_method
    // (#4995): the linker resolves them to whichever EventEmitter impl is in
    // the binary (perry-stdlib `bundled-events` or perry-ext-events under the
    // well-known flip), so the registry these consult is always the one the
    // constructors used. Registered eagerly at startup — perry-ext-events
    // alone only registers its hooks lazily on the first *static* emitter
    // construction, which a dynamic-first program (signal-exit's
    // `new (require('events'))()`) never performs.
    #[cfg(any(feature = "bundled-events", feature = "external-events-construct"))]
    unsafe extern "C" fn event_emitter_probe(handle: i64) -> bool {
        js_event_emitter_is_handle(handle)
    }
    #[cfg(any(feature = "bundled-events", feature = "external-events-construct"))]
    js_register_event_emitter_handle_probe(event_emitter_probe);
    #[cfg(feature = "bundled-events")]
    unsafe extern "C" fn event_emitter_async_resource_probe(handle: i64) -> bool {
        crate::events::is_event_emitter_async_resource_handle(handle)
    }
    #[cfg(feature = "bundled-events")]
    js_register_event_emitter_async_resource_handle_probe(event_emitter_async_resource_probe);
    #[cfg(any(feature = "bundled-events", feature = "external-events-construct"))]
    unsafe extern "C" fn event_emitter_on_hook(
        handle: i64,
        event_bits: i64,
        listener_bits: i64,
    ) -> i64 {
        js_event_emitter_on(handle, event_bits, listener_bits)
    }
    #[cfg(any(feature = "bundled-events", feature = "external-events-construct"))]
    js_register_event_emitter_on(event_emitter_on_hook);
    // #4995: serve dynamic `new` on the bound `events.EventEmitter` /
    // `events.EventEmitterAsyncResource` export values (`require('events')`,
    // default import, namespace property read) with the same constructors the
    // named-import codegen path calls. Without this the runtime's
    // `js_new_function_construct` fell through to the generic empty-object
    // path and the instance had no `.on`/`.emit`/`.setMaxListeners`.
    // EventEmitterAsyncResource exists only in the bundled impl.
    #[cfg(any(feature = "bundled-events", feature = "external-events-construct"))]
    unsafe extern "C" fn events_native_construct(
        class_name_ptr: *const u8,
        class_name_len: usize,
        args_ptr: *const f64,
        args_len: usize,
    ) -> f64 {
        let class_name = std::slice::from_raw_parts(class_name_ptr, class_name_len);
        let options = if !args_ptr.is_null() && args_len > 0 {
            *args_ptr
        } else {
            TAG_UNDEFINED_F64
        };
        let handle = match class_name {
            b"EventEmitter" => js_event_emitter_new_with_options(options),
            #[cfg(feature = "bundled-events")]
            b"EventEmitterAsyncResource" => {
                crate::events::js_event_emitter_async_resource_new(options)
            }
            _ => return TAG_UNDEFINED_F64,
        };
        perry_runtime::js_nanbox_pointer(handle)
    }
    #[cfg(any(feature = "bundled-events", feature = "external-events-construct"))]
    perry_runtime::js_set_native_events_construct(events_native_construct);

    // Dynamic `new <bound async_hooks ctor>()` -> real handle. Next.js does
    // `globalThis.AsyncLocalStorage = AsyncLocalStorage` then
    // `new maybeGlobalAsyncLocalStorage()`; the dynamic callee misses the static
    // `new AsyncLocalStorage()` codegen arm, so the runtime construct path must
    // build the handle here (else `.getStore` is undefined at server startup).
    unsafe extern "C" fn async_hooks_native_construct(
        method_ptr: *const u8,
        method_len: usize,
        args_ptr: *const f64,
        args_len: usize,
    ) -> f64 {
        let method = std::slice::from_raw_parts(method_ptr, method_len);
        match method {
            b"AsyncLocalStorage" => {
                let handle = crate::async_local_storage::js_async_local_storage_new();
                perry_runtime::js_nanbox_pointer(handle)
            }
            b"AsyncResource" => {
                let type_value = if !args_ptr.is_null() && args_len > 0 {
                    *args_ptr
                } else {
                    TAG_UNDEFINED_F64
                };
                let options = if !args_ptr.is_null() && args_len > 1 {
                    *args_ptr.add(1)
                } else {
                    TAG_UNDEFINED_F64
                };
                let handle = perry_runtime::async_hooks::js_async_resource_new(type_value, options);
                perry_runtime::js_nanbox_pointer(handle)
            }
            _ => TAG_UNDEFINED_F64,
        }
    }
    perry_runtime::js_set_native_async_hooks_construct(async_hooks_native_construct);
    super::super::net_socket_bridge::register_net_socket_handle_probe();
    js_register_worker_threads_namespace_getters(
        crate::worker_threads::js_worker_threads_get_worker_data,
        crate::worker_threads::js_worker_threads_is_main_thread,
        crate::worker_threads::js_worker_threads_parent_port,
        crate::worker_threads::js_worker_threads_thread_name,
        crate::worker_threads::js_worker_threads_resource_limits,
    );
    js_register_worker_threads_messaging_constructors(
        crate::worker_threads::js_worker_threads_message_channel_new,
        crate::worker_threads::js_worker_threads_broadcast_channel_new,
    );
    // #1577: route captured-then-called `crypto.*` methods (which reach the
    // runtime's native-module dispatch) back to the stdlib crypto impls.
    #[cfg(feature = "crypto")]
    perry_runtime::js_set_native_crypto_dispatch(crate::crypto::js_crypto_native_dispatch);
    #[cfg(feature = "crypto")]
    perry_runtime::js_set_native_webcrypto_dispatch(crate::webcrypto::js_webcrypto_native_dispatch);
    // Prune the stdlib CryptoKey-material map when the GC sweeps a key's
    // backing buffer (otherwise it leaks an entry per key and a recycled
    // address inherits the dead key's material).
    #[cfg(feature = "crypto")]
    perry_runtime::buffer::js_set_crypto_key_death_hook(crate::webcrypto::crypto_key_buffer_died);
    #[cfg(feature = "compression")]
    perry_runtime::js_set_native_zlib_dispatch(crate::zlib::js_zlib_native_dispatch);
    perry_runtime::js_set_native_querystring_dispatch(
        crate::querystring::js_querystring_native_dispatch,
    );
    #[cfg(feature = "database-sqlite")]
    perry_runtime::js_set_native_sqlite_dispatch(crate::sqlite::js_node_sqlite_native_dispatch);
    perry_runtime::js_set_native_domain_dispatch(crate::domain::js_domain_native_dispatch);
    #[cfg(all(feature = "tls", not(target_os = "ios"), not(target_os = "android")))]
    perry_runtime::js_set_native_tls_dispatch(crate::tls::js_tls_native_dispatch);

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
        perry_runtime::fs::js_register_filehandle_readable_web_stream_factory(
            crate::streams::js_readable_stream_deferred_byte_source,
        );
        perry_runtime::node_stream::js_register_node_stream_web_adapter_callbacks(
            crate::streams::js_readable_stream_new,
            crate::streams::js_readable_stream_controller_enqueue,
            crate::streams::js_readable_stream_controller_close,
            crate::streams::js_readable_stream_controller_error,
            crate::streams::js_writable_stream_new,
            crate::streams::js_readable_stream_get_reader,
            crate::streams::js_reader_read,
            crate::streams::js_writable_stream_get_writer,
            crate::streams::js_writer_write,
            crate::streams::js_writer_close,
            crate::streams::js_writer_abort,
        );
    }

    // `instanceof` for WHATWG fetch handles (Response/Request/Headers/Blob).
    // They are pointer-tagged small-integer ids, not heap objects, so the
    // runtime can't walk a prototype chain — register a kind-probe so
    // `x instanceof Response` (Hono's route-fallback guard) resolves. Gated on
    // `web-fetch` — the feature that actually compiles the fetch module and
    // `js_fetch_handle_kind` (since #5174 split `http-client = ["web-fetch"]`,
    // auto-optimize enables `web-fetch` directly for bare `new Response()`; the
    // old `http-client` gate left the probe unregistered in that build).
    #[cfg(feature = "web-fetch")]
    {
        extern "C" {
            fn js_register_fetch_handle_kind_probe(f: unsafe extern "C" fn(usize) -> u8);
            fn js_fetch_handle_kind(id: usize) -> u8;
        }
        js_register_fetch_handle_kind_probe(js_fetch_handle_kind);
    }
}
