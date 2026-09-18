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
    #[cfg(feature = "external-http-client-pump")]
    if matches!(
        property_name,
        "maxSockets"
            | "maxFreeSockets"
            | "maxTotalSockets"
            | "keepAliveMsecs"
            | "agentKeepAliveTimeoutBuffer"
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
    #[cfg(feature = "database-sqlite")]
    if let Some(names) = crate::sqlite::dispatch_node_sqlite_own_property_names(handle) {
        return names;
    }
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
    #[cfg(feature = "crypto")]
    if crate::common::handle::with_handle::<crate::crypto::X509Handle, bool, _>(handle, |_| true)
        .unwrap_or(false)
    {
        let constructor =
            perry_runtime::object::bound_native_callable_export_value("crypto", "X509Certificate");
        let constructor = perry_runtime::JSValue::from_bits(constructor.to_bits());
        if constructor.is_pointer() {
            return perry_runtime::closure::closure_get_dynamic_prop(
                constructor.as_pointer::<u8>() as usize,
                "prototype",
            );
        }
    }
    f64::from_bits(perry_runtime::JSValue::undefined().bits())
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
        fn js_register_event_emitter_async_resource_dispatch(
            f: unsafe extern "C" fn(i64, u32) -> f64,
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
        #[cfg(feature = "web-fetch")]
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
    #[cfg(feature = "web-fetch")]
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
    #[cfg(feature = "bundled-events")]
    unsafe extern "C" fn event_emitter_async_resource_dispatch(handle: i64, operation: u32) -> f64 {
        match operation {
            0 => crate::events::js_event_emitter_async_resource_async_id(handle),
            1 => crate::events::js_event_emitter_async_resource_trigger_async_id(handle),
            2 => crate::events::js_event_emitter_async_resource_async_resource(handle),
            3 => crate::events::js_event_emitter_async_resource_emit_destroy(handle),
            _ => TAG_UNDEFINED_F64,
        }
    }
    #[cfg(feature = "bundled-events")]
    js_register_event_emitter_async_resource_dispatch(event_emitter_async_resource_dispatch);
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
    // #10625: register the AsyncLocalStorage subclass-init dispatcher so
    // `class X extends <bound async_hooks.AsyncLocalStorage export>` reached
    // through a local alias, namespace member, or CJS destructured `require()`
    // reaches the real handle at `super()` time — not just the canonical bare
    // import shape codegen already routes statically. See
    // `js_fetch_or_value_super` in perry-runtime for why this indirection
    // exists (perry-runtime cannot depend on perry-stdlib, where
    // `js_async_local_storage_subclass_init` and the `Handle` registry it uses
    // live).
    perry_runtime::js_set_native_async_local_storage_subclass_init(
        crate::async_local_storage::js_async_local_storage_subclass_init,
    );
    super::super::net_socket_bridge::register_net_socket_handle_probe();
    #[cfg(feature = "external-http-client-pump")]
    {
        extern "C" {
            fn js_register_http_agent_handle_probe(f: unsafe extern "C" fn(i64) -> bool);
            fn js_ext_http_agent_is_handle(handle: i64) -> i32;
        }
        unsafe extern "C" fn http_agent_probe(handle: i64) -> bool {
            js_ext_http_agent_is_handle(handle) != 0
        }
        js_register_http_agent_handle_probe(http_agent_probe);
    }
    #[cfg(all(
        feature = "tls-runtime",
        not(target_os = "ios"),
        not(target_os = "android")
    ))]
    {
        unsafe extern "C" fn tls_handle_kind_probe(handle: i64) -> u8 {
            if crate::tls::is_tls_server_handle(handle) {
                1
            } else if crate::tls::is_tls_socket_handle(handle) {
                2
            } else {
                0
            }
        }
        perry_runtime::object::js_register_tls_handle_kind_probe(tls_handle_kind_probe);
    }
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
    #[cfg(feature = "compression-gzip")]
    perry_runtime::js_set_native_zlib_dispatch(crate::zlib::js_zlib_native_dispatch);
    // Optimized builds route `node:zlib` to perry-ext-zlib and compile the
    // bundled codec module out. Captured exports (`const gzip = zlib.gzip`) and
    // `util.promisify(zlib.gzip)` still enter the runtime's by-name dispatcher,
    // so install the external archive's mirror when it is the active backend.
    #[cfg(all(feature = "external-zlib-pump", not(feature = "compression-gzip")))]
    {
        extern "C" {
            fn js_ext_zlib_native_dispatch(
                method: *const u8,
                method_len: usize,
                args: *const f64,
                args_len: usize,
            ) -> f64;
        }
        perry_runtime::js_set_native_zlib_dispatch(js_ext_zlib_native_dispatch);
    }
    perry_runtime::js_set_native_querystring_dispatch(
        crate::querystring::js_querystring_native_dispatch,
    );
    // Module-level `events.*` helpers reached indirectly (captured value,
    // type-erased receiver, spread call) — see `js_events_native_dispatch`.
    //
    // #7764: gated to match `pub mod events`, which is `bundled-events`. #7745
    // added this line ungated, so `--no-default-features` — the configuration
    // the auto-optimize relink builds with — stopped compiling, and every
    // `perry` compile that triggers auto-optimize silently fell back to the
    // prebuilt archives. The neighbouring registrations are gated the same way
    // (`database-sqlite` on the next line), which is what makes this an
    // omission rather than a decision.
    #[cfg(feature = "bundled-events")]
    perry_runtime::js_set_native_events_dispatch(crate::events::js_events_native_dispatch);
    #[cfg(all(feature = "external-events-construct", not(feature = "bundled-events")))]
    {
        extern "C" {
            fn js_events_native_dispatch(
                method: *const u8,
                method_len: usize,
                args: *const f64,
                args_len: usize,
            ) -> f64;
        }
        perry_runtime::js_set_native_events_dispatch(js_events_native_dispatch);
    }
    #[cfg(feature = "database-sqlite")]
    perry_runtime::js_set_native_sqlite_dispatch(crate::sqlite::js_node_sqlite_native_dispatch);
    perry_runtime::js_set_native_domain_dispatch(crate::domain::js_domain_native_dispatch);
    #[cfg(all(
        feature = "tls-runtime",
        not(target_os = "ios"),
        not(target_os = "android")
    ))]
    perry_runtime::js_set_native_tls_dispatch(crate::tls::js_tls_native_dispatch);

    // #2533: route captured / aliased http/https/http2 exports back to
    // perry-ext-http. The dispatcher lives in that crate (#10428), which also
    // registers it from its namespace install so the prebuilt no-auto archives
    // work too; registering here keeps value forms that never materialize an
    // http namespace (`bun.serve`, `class extends http.Server`) covered when
    // the feature guarantees the crate is linked.
    #[cfg(feature = "external-http-server-pump")]
    {
        extern "C" {
            fn js_ext_http_native_dispatch(
                module_ptr: *const u8,
                module_len: usize,
                method_ptr: *const u8,
                method_len: usize,
                args_ptr: *const f64,
                args_len: usize,
            ) -> f64;
        }
        perry_runtime::js_set_native_http_dispatch(js_ext_http_native_dispatch);
    }

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

    // `instanceof` for WHATWG fetch handles
    // (Response/Request/Headers/Blob/File).
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
