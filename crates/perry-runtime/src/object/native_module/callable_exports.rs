use super::*;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicPtr, Ordering};

pub(crate) fn bound_native_callable_export_value(module_name: &str, property_name: &str) -> f64 {
    // Bound-native closures carry (module, method) metadata that the
    // generic property/call paths resolve through the vtable — and they
    // can be minted via the codegen NativeModuleRef fast path without any
    // namespace object existing. Install here too.
    install_native_module_vtable();
    let module_name = cjs_default_base_module(module_name).unwrap_or(module_name);
    let module_name = assert_instance_base_module(module_name).unwrap_or(module_name);
    let property_name = canonical_native_callable_property(module_name, property_name);
    let export_module_name = if property_name == "Assert" && module_name == "assert/strict" {
        "assert"
    } else {
        module_name
    };
    let callable_module_name = if export_module_name == "util.types" {
        "util/types"
    } else {
        export_module_name
    };
    let key = format!("{callable_module_name}\0{property_name}");
    if let Some(bits) = NATIVE_CALLABLE_EXPORTS.with(|c| c.borrow().get(&key).copied()) {
        return f64::from_bits(bits);
    }

    let method_bytes: &'static [u8] = property_name.as_bytes().to_vec().leak();
    let ns = js_create_native_module_namespace(
        callable_module_name.as_ptr(),
        callable_module_name.len(),
    );
    let closure = crate::closure::js_closure_alloc(crate::closure::BOUND_METHOD_FUNC_PTR, 3);
    crate::closure::js_closure_set_capture_f64(closure, 0, ns);
    crate::closure::js_closure_set_capture_ptr(closure, 1, method_bytes.as_ptr() as i64);
    crate::closure::js_closure_set_capture_ptr(closure, 2, method_bytes.len() as i64);
    let exposed_name = if export_module_name == "fs" {
        native_callable_export_display_name(export_module_name, property_name)
    } else if export_module_name == "url" && property_name == "resolveObject" {
        "urlResolveObject"
    } else if export_module_name == "http" && property_name == "_connectionListener" {
        "connectionListener"
    } else if export_module_name == "fs" && property_name == "_toUnixTimestamp" {
        "toUnixTimestamp"
    } else {
        property_name
    };
    set_bound_native_closure_name(closure, exposed_name);
    if let Some(length) = native_callable_export_arity(export_module_name, property_name) {
        set_builtin_closure_length(closure as usize, length);
    }
    let mut value = crate::value::js_nanbox_pointer(closure as i64);
    let closure_addr = closure as usize;

    if export_module_name == "module" && property_name == "Module" {
        attach_module_cjs_constructor_statics(closure_addr);
    }
    if export_module_name == "tty" && matches!(property_name, "ReadStream" | "WriteStream") {
        attach_tty_stream_prototype(value, property_name);
    }
    if export_module_name == "tls" && property_name == "SecureContext" {
        attach_tls_secure_context_prototype(value);
    }
    if export_module_name == "wasi" && property_name == "WASI" {
        crate::wasi::attach_wasi_constructor_prototype(value);
    }
    if export_module_name == "stream" && property_name == "Stream" {
        attach_stream_legacy_prototype(value);
    }
    if export_module_name == "stream"
        && matches!(
            property_name,
            "Readable" | "Writable" | "Duplex" | "Transform" | "PassThrough"
        )
    {
        attach_stream_constructor_prototype(value, property_name);
    }
    // #6692: Node defines `stream.pipeline[util.promisify.custom]` and
    // `stream.finished[util.promisify.custom]` pointing at the promise-based
    // `stream/promises` implementations, so `promisify(stream.pipeline)` returns
    // that impl rather than the generic callback-appending wrapper. Wire the
    // same hooks so `custom_promisified_value` (util_promisify.rs) honors them.
    if export_module_name == "stream" && matches!(property_name, "pipeline" | "finished") {
        // Reassign: the attach helper roots `value` and allocates (which may
        // evacuate the closure), so it returns the possibly-relocated pointer.
        value = attach_stream_promisify_custom(value, property_name);
    }
    if export_module_name == "sqlite" && property_name == "DatabaseSync" {
        attach_sqlite_database_sync_prototype(value);
    }
    if export_module_name == "sqlite" && property_name == "Session" {
        attach_sqlite_session_prototype(value);
    }
    if export_module_name == "assert" && property_name == "Assert" {
        attach_assert_prototype(value);
    }
    if export_module_name == "crypto" && property_name == "KeyObject" {
        attach_crypto_key_object_shape(closure_addr, value);
    }
    if export_module_name == "crypto" && property_name == "X509Certificate" {
        attach_crypto_x509_certificate_shape(closure_addr, value);
    }

    // `PerformanceObserver.supportedEntryTypes` is a static array on the
    // constructor. `PerformanceObserver` is a function value (a bound-method
    // closure), so hang the array off it as a dynamic property — keeps
    // `typeof PerformanceObserver === "function"` while the static read works.
    if export_module_name == "perf_hooks" && property_name == "PerformanceObserver" {
        let arr = crate::perf_hooks::js_perf_supported_entry_types();
        crate::closure::closure_set_dynamic_prop(closure_addr, "supportedEntryTypes", arr);
    }

    if export_module_name == "async_hooks" && property_name == "AsyncLocalStorage" {
        crate::closure::closure_set_dynamic_prop(
            closure_addr,
            "bind",
            async_hooks_static_method_value(
                crate::async_hooks::js_async_local_storage_static_bind_method as *const u8,
                "bind",
                1,
                1,
            ),
        );
        crate::closure::closure_set_dynamic_prop(
            closure_addr,
            "snapshot",
            async_hooks_static_method_value(
                crate::async_hooks::js_async_local_storage_static_snapshot_method as *const u8,
                "snapshot",
                0,
                0,
            ),
        );
    }

    if export_module_name == "async_hooks" && property_name == "AsyncResource" {
        crate::closure::closure_set_dynamic_prop(
            closure_addr,
            "bind",
            async_hooks_static_method_value(
                crate::async_hooks::js_async_resource_static_bind_method as *const u8,
                "bind",
                3,
                3,
            ),
        );
    }

    if export_module_name == "events" && property_name == "EventEmitter" {
        let async_resource_ctor =
            bound_native_callable_export_value("events", "EventEmitterAsyncResource");
        for method in [
            "addAbortListener",
            "once",
            "on",
            "getEventListeners",
            "getMaxListeners",
            "listenerCount",
            "setMaxListeners",
        ] {
            let method_value = bound_native_callable_export_value("events", method);
            crate::closure::closure_set_dynamic_prop(closure_addr, method, method_value);
        }
        crate::closure::closure_set_dynamic_prop(closure_addr, "EventEmitter", value);
        crate::closure::closure_set_dynamic_prop(
            closure_addr,
            "EventEmitterAsyncResource",
            async_resource_ctor,
        );
        crate::closure::closure_set_dynamic_prop(closure_addr, "defaultMaxListeners", 10.0);
        crate::closure::closure_set_dynamic_prop(
            closure_addr,
            "usingDomains",
            f64::from_bits(JSValue::bool(false).bits()),
        );
        crate::closure::closure_set_dynamic_prop(
            closure_addr,
            "captureRejections",
            f64::from_bits(JSValue::bool(false).bits()),
        );
        crate::closure::closure_set_dynamic_prop(closure_addr, "captureRejectionSymbol", {
            let name = "nodejs.rejection";
            let ptr = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            unsafe { crate::symbol::js_symbol_for(f64::from_bits(JSValue::string_ptr(ptr).bits())) }
        });
        crate::closure::closure_set_dynamic_prop(closure_addr, "errorMonitor", {
            let name = "events.errorMonitor";
            let ptr = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            unsafe { crate::symbol::js_symbol_for(f64::from_bits(JSValue::string_ptr(ptr).bits())) }
        });
        crate::closure::closure_set_dynamic_prop(
            closure_addr,
            "init",
            bound_native_callable_export_value("events", "init"),
        );
    }

    if export_module_name == "util" && property_name == "promisify" {
        crate::closure::closure_set_dynamic_prop(
            closure_addr,
            "custom",
            crate::util_promisify::promisify_custom_symbol(),
        );
    }
    if export_module_name == "util" && property_name == "inspect" {
        crate::closure::closure_set_dynamic_prop(
            closure_addr,
            "custom",
            util_inspect_custom_symbol(),
        );
        crate::closure::closure_set_dynamic_prop(
            closure_addr,
            "defaultOptions",
            util_inspect_default_options_value(),
        );
        crate::closure::closure_set_dynamic_prop(closure_addr, "styles", util_inspect_styles());
        crate::closure::closure_set_dynamic_prop(closure_addr, "colors", util_inspect_colors());
    }

    NATIVE_CALLABLE_EXPORTS.with(|c| {
        c.borrow_mut().insert(key, value.to_bits());
        crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
    });
    value
}

/// #6692: install `stream.pipeline[util.promisify.custom]` (or `.finished`'s)
/// pointing at the promise-based `stream/promises` export, matching Node. With
/// the hook present, `promisify(stream.pipeline)` resolves through
/// `custom_promisified_value` to the promise implementation instead of the
/// generic wrapper (whose appended callback the `promisify.custom`-aware caller
/// in `pi`'s bundled node-fetch never provides). `property_name` is `"pipeline"`
/// or `"finished"` — the matching `stream/promises` export name.
///
/// Returns the (possibly relocated) receiver value: the allocations below can
/// trigger a GC that evacuates the closure, and only the `scope` handle tracks
/// the move, so the caller must adopt the returned pointer.
fn attach_stream_promisify_custom(pipeline_value: f64, property_name: &str) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(pipeline_value);
    let promise_impl = crate::node_submodules::stream_promises_export_callable(property_name);
    // The submodule may be unavailable (returns the `TAG_TRUE` sentinel); only
    // wire the hook when it resolved to a real callable closure, otherwise leave
    // the generic promisify fallback in place.
    let impl_bits = promise_impl.to_bits();
    let impl_addr = (impl_bits & crate::value::POINTER_MASK) as usize;
    if (impl_bits & crate::value::TAG_MASK) == crate::value::POINTER_TAG
        && crate::closure::is_closure_ptr(impl_addr)
    {
        let impl_handle = scope.root_nanbox_f64(promise_impl);
        let custom_symbol = crate::util_promisify::promisify_custom_symbol();
        let symbol_handle = scope.root_nanbox_f64(custom_symbol);
        unsafe {
            crate::symbol::js_object_set_symbol_property(
                target.get_nanbox_f64(),
                symbol_handle.get_nanbox_f64(),
                impl_handle.get_nanbox_f64(),
            );
        }
    }
    target.get_nanbox_f64()
}

fn async_hooks_static_method_value(
    func_ptr: *const u8,
    name: &str,
    fixed_arity: u32,
    length: u32,
) -> f64 {
    crate::closure::js_register_closure_rest(func_ptr, fixed_arity);
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    set_bound_native_closure_name(closure, name);
    set_builtin_closure_length(closure as usize, length);
    crate::value::js_nanbox_pointer(closure as i64)
}

extern "C" fn fs_namespace_descriptor_getter_thunk(
    closure: *const crate::closure::ClosureHeader,
) -> f64 {
    unsafe {
        let property_ptr = crate::closure::js_closure_get_capture_ptr(closure, 0) as *const u8;
        let property_len = crate::closure::js_closure_get_capture_ptr(closure, 1) as usize;
        js_native_module_property_by_name(b"fs".as_ptr(), 2, property_ptr, property_len)
    }
}

extern "C" fn fs_namespace_descriptor_setter_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _value: f64,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

pub(crate) fn fs_namespace_descriptor_getter_value(property_name: &str) -> f64 {
    let key = format!("fs\0get\0{property_name}");
    if let Some(bits) = NATIVE_MODULE_ACCESSOR_EXPORTS.with(|c| c.borrow().get(&key).copied()) {
        return f64::from_bits(bits);
    }

    let property_bytes: &'static [u8] = property_name.as_bytes().to_vec().leak();
    let func_ptr = fs_namespace_descriptor_getter_thunk as *const u8;
    crate::closure::js_register_closure_arity(func_ptr, 0);
    let closure = crate::closure::js_closure_alloc(func_ptr, 2);
    crate::closure::js_closure_set_capture_ptr(closure, 0, property_bytes.as_ptr() as i64);
    crate::closure::js_closure_set_capture_ptr(closure, 1, property_bytes.len() as i64);
    let name = if property_name == "promises" {
        "get".to_string()
    } else {
        format!("get {property_name}")
    };
    set_bound_native_closure_name(closure, &name);
    let value = crate::value::js_nanbox_pointer(closure as i64);

    NATIVE_MODULE_ACCESSOR_EXPORTS.with(|c| {
        c.borrow_mut().insert(key, value.to_bits());
        crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
    });
    value
}

pub(crate) fn fs_namespace_descriptor_setter_value(property_name: &str) -> f64 {
    let key = format!("fs\0set\0{property_name}");
    if let Some(bits) = NATIVE_MODULE_ACCESSOR_EXPORTS.with(|c| c.borrow().get(&key).copied()) {
        return f64::from_bits(bits);
    }

    let func_ptr = fs_namespace_descriptor_setter_thunk as *const u8;
    crate::closure::js_register_closure_arity(func_ptr, 1);
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    let name = format!("set {property_name}");
    set_bound_native_closure_name(closure, &name);
    let value = crate::value::js_nanbox_pointer(closure as i64);

    NATIVE_MODULE_ACCESSOR_EXPORTS.with(|c| {
        c.borrow_mut().insert(key, value.to_bits());
        crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
    });
    value
}

/// The EventEmitter method names `node:cluster`'s default import exposes
/// (#3687). Kept narrow so a typo'd `cluster.foo` still reads `undefined`.
pub(crate) fn is_cluster_emitter_method(prop: &str) -> bool {
    matches!(
        prop,
        "on" | "addListener"
            | "once"
            | "prependListener"
            | "prependOnceListener"
            | "off"
            | "removeListener"
            | "removeAllListeners"
            | "emit"
            | "eventNames"
            | "listenerCount"
    )
}

fn native_callable_export_arity(module: &str, prop: &str) -> Option<u32> {
    match (module, prop) {
        // bun:ffi (#6562).
        ("bun:ffi", "dlopen") => Some(2),
        ("bun:ffi", "ptr" | "CString" | "JSCallback" | "CFunction" | "linkSymbols") => Some(1),
        ("bun:ffi", "toArrayBuffer" | "toBuffer") => Some(3),
        ("bun:ffi", "viewSource") => Some(2),
        ("bun:ffi", "read") => Some(0),
        // #3687: node:cluster — module-method `.length` matches Node.
        ("cluster", "fork" | "disconnect" | "setupPrimary" | "setupMaster" | "Worker") => Some(1),
        ("cluster", "emit") => Some(1),
        ("cluster", "eventNames") => Some(0),
        (
            "cluster",
            "on"
            | "addListener"
            | "once"
            | "prependListener"
            | "prependOnceListener"
            | "removeListener"
            | "off"
            | "listenerCount",
        ) => Some(2),
        ("cluster", "removeAllListeners") => Some(1),
        // #6563: node-pty `spawn(file, args, options)`.
        ("node-pty", "spawn") => Some(3),
        ("events", "EventEmitter") => Some(1),
        ("events", "EventEmitterAsyncResource") => Some(0),
        ("events", "addAbortListener") => Some(2),
        ("events", "once") => Some(2),
        ("events", "on") => Some(2),
        ("events", "getEventListeners") => Some(2),
        ("events", "getMaxListeners") => Some(1),
        ("events", "listenerCount") => Some(2),
        ("events", "setMaxListeners") => Some(0),
        ("querystring", "unescapeBuffer" | "unescape") => Some(2),
        ("querystring", "escape") => Some(1),
        ("querystring", "stringify" | "parse") => Some(4),
        ("async_hooks", "AsyncLocalStorage") => Some(0),
        ("async_hooks", "AsyncResource") => Some(2),
        ("async_hooks", "createHook") => Some(1),
        ("async_hooks", "executionAsyncId") => Some(0),
        ("async_hooks", "triggerAsyncId") => Some(0),
        ("async_hooks", "executionAsyncResource") => Some(0),
        ("url", "URL") => Some(1),
        ("url", "URLPattern") => Some(0),
        ("tls", "getCiphers") => Some(0),
        ("tls", "getCACertificates" | "setDefaultCACertificates" | "createSecureContext") => {
            Some(1)
        }
        ("tls", "checkServerIdentity") => Some(2),
        ("tls", "SecureContext") => Some(1),
        // #3726: `crypto.Cipheriv` / `crypto.Decipheriv` constructor exports —
        // `(cipher, key, iv, options)` arity matches Node's length 4.
        ("crypto", "Cipheriv" | "Decipheriv") => Some(4),
        ("crypto", "X509Certificate") => Some(1),
        ("crypto", "KeyObject") => Some(2),
        ("crypto.KeyObject", "from") => Some(1),
        // #2706/#2716 and #2694: crypto module-level callable exports.
        ("crypto", "DiffieHellman") => Some(4),
        ("crypto", "DiffieHellmanGroup") => Some(1),
        ("crypto", "diffieHellman") => Some(2),
        ("crypto", "encapsulate") => Some(2),
        ("crypto", "decapsulate") => Some(3),
        ("crypto", "generateKey" | "generateKeyPair" | "generatePrime") => Some(3),
        ("crypto", "generateKeySync" | "generateKeyPairSync") => Some(2),
        ("crypto", "generatePrimeSync" | "checkPrime" | "checkPrimeSync" | "setFips") => Some(1),
        ("crypto", "secureHeapUsed") => Some(0),
        ("crypto", "hkdf") => Some(6),
        ("crypto", "hkdfSync") => Some(5),
        ("crypto", "scrypt") => Some(4),
        ("crypto", "scryptSync") => Some(3),
        ("crypto", "argon2") => Some(3),
        ("crypto", "argon2Sync") => Some(2),
        ("url", "Url") => Some(0),
        ("url", "resolveObject") => Some(2),
        ("process", "binding" | "_linkedBinding") => Some(1),
        (
            "process",
            "dlopen"
            | "_rawDebug"
            | "_debugProcess"
            | "_debugEnd"
            | "_startProfilerIdleNotifier"
            | "_stopProfilerIdleNotifier"
            | "reallyExit"
            | "_tickCallback"
            | "_getActiveHandles"
            | "_getActiveRequests"
            | "openStdin"
            | "_kill",
        ) => Some(0),
        ("process", "_fatalException") => Some(2),
        ("process", "execve") => Some(1),
        ("process", "ref" | "unref") => Some(1),
        ("process", "setSourceMapsEnabled") => Some(1),
        (
            "inspector.Network",
            "requestWillBeSent"
            | "responseReceived"
            | "loadingFinished"
            | "loadingFailed"
            | "dataSent"
            | "dataReceived"
            | "webSocketCreated"
            | "webSocketClosed"
            | "webSocketHandshakeResponseReceived",
        ) => Some(1),
        (
            "process",
            "setUncaughtExceptionCaptureCallback" | "addUncaughtExceptionCaptureCallback",
        ) => Some(1),
        ("process", "hasUncaughtExceptionCaptureCallback") => Some(0),
        ("fs", "_toUnixTimestamp") => Some(1),
        ("util", "debug" | "debuglog" | "inherits") => Some(2),
        ("console", "context") => Some(1),
        ("console", "createTask") => Some(0),
        ("util", "MIMEParams") => Some(0),
        ("util", "MIMEType") => Some(1),
        ("sea", "isSea" | "getAssetKeys") => Some(0),
        ("sea", "getRawAsset") => Some(1),
        ("sea", "getAsset" | "getAssetAsBlob") => Some(2),
        ("stream", "pipeline" | "compose") => Some(0),
        ("stream", "finished") => Some(3),
        (
            "stream",
            "duplexPair"
            | "isDisturbed"
            | "isErrored"
            | "isReadable"
            | "isWritable"
            | "getDefaultHighWaterMark"
            | "_isArrayBufferView"
            | "_isUint8Array"
            | "_uint8ArrayToBuffer"
            | "isDestroyed",
        ) => Some(1),
        ("stream", "setDefaultHighWaterMark" | "addAbortSignal") => Some(2),
        ("net", "connect" | "createConnection") => Some(3),
        ("net", "createServer" | "Server") => Some(2),
        ("net", "Socket") => Some(1),
        ("net", "BlockList" | "SocketAddress") => Some(0),
        // #3720: `http2.performServerHandshake(socket[, options])` — length 1.
        ("http2", "performServerHandshake") => Some(1),
        ("http2", "getDefaultSettings") => Some(0),
        ("http2", "getPackedSettings" | "getUnpackedSettings") => Some(1),
        // #3905: Node `.length` — connect(authority,options,listener)=3,
        // createServer(options,handler)=2.
        ("http2", "connect") => Some(3),
        ("http2", "createServer" | "createSecureServer") => Some(2),
        ("http", "OutgoingMessage") => Some(1),
        // #4904: Node `.length` — Agent(options)=1, ClientRequest(input,
        // options, cb)=3, IncomingMessage(socket)=1, ServerResponse(req)=1.
        ("http", "Agent" | "IncomingMessage" | "ServerResponse") => Some(1),
        ("http", "ClientRequest") => Some(3),
        // #3697: node:https module-level exports (Node `.length`).
        ("https", "request") => Some(0),
        ("https", "get") => Some(3),
        ("https", "Agent") => Some(1),
        // #4904: http twins of the https entries above.
        ("http", "request") => Some(0),
        ("http", "get") => Some(3),
        (
            "stream",
            "isDestroyed"
            | "isDisturbed"
            | "isErrored"
            | "isReadable"
            | "isWritable"
            | "getDefaultHighWaterMark"
            | "_isArrayBufferView"
            | "_isUint8Array"
            | "_uint8ArrayToBuffer",
        ) => Some(1),
        ("stream", "finished") => Some(3),
        ("stream", "addAbortSignal" | "destroy" | "setDefaultHighWaterMark") => Some(2),
        ("stream", "compose" | "pipeline") => Some(0),
        ("stream", "duplexPair") => Some(1),
        // #3712: node:http module-level helper exports.
        ("http", "validateHeaderName" | "validateHeaderValue") => Some(2),
        ("http", "setMaxIdleHTTPParsers" | "setGlobalProxyFromEnv") => Some(1),
        ("http", "_connectionListener") => Some(1),
        ("module", "register" | "registerHooks") => Some(1),
        // #3904: modern V8 diagnostics/profiler exports (Node .length values).
        ("v8", "getCppHeapStatistics") => Some(0),
        (
            "v8",
            "getHeapSnapshot"
            | "isStringOneByteRepresentation"
            | "queryObjects"
            | "startCpuProfile",
        ) => Some(1),
        ("v8", "writeHeapSnapshot") => Some(2),
        // #3906: implemented top-level v8 helpers reachable as bound callables.
        ("v8", "serialize" | "deserialize") => Some(1),
        (
            "v8",
            "getHeapStatistics"
            | "getHeapSpaceStatistics"
            | "getHeapCodeStatistics"
            | "cachedDataVersionTag"
            | "GCProfiler",
        ) => Some(0),
        // #3127/#3128/#3130/#3284: node:vm no-flag export lengths.
        ("vm", "Script") => Some(1),
        ("vm", "Module") => Some(1),
        ("vm", "SourceTextModule") => Some(1),
        ("vm", "SyntheticModule") => Some(2),
        ("vm", "createContext" | "measureMemory") => Some(0),
        ("vm", "createScript" | "runInThisContext" | "compileFunction") => Some(2),
        ("vm", "runInContext" | "runInNewContext") => Some(3),
        ("vm", "isContext") => Some(1),
        ("net", "_normalizeArgs") => Some(1),
        ("net", "_createServerHandle") => Some(5),
        ("domain", "Domain" | "createDomain" | "create") => Some(0),
        ("util", "diff") => Some(2),
        ("dns" | "dns/promises", "Resolver") => Some(0),
        ("fs", "ReadStream" | "WriteStream") => Some(2),
        ("fs", "Utf8Stream") => Some(0),
        ("fs", "Dir" | "Dirent") => Some(3),
        ("fs", "Stats") => Some(18),
        ("fs", "mkdtempDisposableSync") => Some(2),
        ("fs", "openAsBlob") => Some(1),
        ("fs", "_toUnixTimestamp") => Some(1),
        ("events", "init") => Some(1),
        ("repl", "Recoverable") => Some(1),
        ("repl", "REPLServer" | "start") => Some(6),
        ("wasi", "WASI") => Some(0),
        ("perf_hooks", "Performance") => Some(0),
        ("perf_hooks", "PerformanceEntry") => Some(0),
        ("perf_hooks", "PerformanceMark") => Some(1),
        ("perf_hooks", "PerformanceMeasure") => Some(0),
        ("perf_hooks", "PerformanceObserver") => Some(1),
        ("perf_hooks", "PerformanceObserverEntryList") => Some(0),
        ("perf_hooks", "PerformanceResourceTiming") => Some(0),
        // #3119/#3126/#3263 node:module helpers.
        ("module", "createRequire") => Some(1),
        ("module", "Module") => Some(0),
        ("module", "enableCompileCache") => Some(1),
        ("module", "flushCompileCache") => Some(0),
        ("module", "getCompileCacheDir") => Some(0),
        ("module", "getSourceMapsSupport") => Some(0),
        ("module", "Module") => Some(0),
        ("module", "_findPath") => Some(3),
        ("module", "_initPaths") => Some(0),
        ("module", "_load") => Some(3),
        ("module", "_nodeModulePaths") => Some(1),
        ("module", "_preloadModules") => Some(1),
        ("module", "_resolveFilename") => Some(4),
        ("module", "_resolveLookupPaths") => Some(2),
        ("module", "setSourceMapsSupport") => Some(1),
        ("module", "stripTypeScriptTypes") => Some(1),
        ("module", "syncBuiltinESMExports") => Some(0),
        ("module", "runMain") => Some(0),
        ("tls", "connect") => Some(4),
        ("tls", "createServer" | "Server") => Some(2),
        ("tls", "TLSSocket") => Some(2),
        ("child_process", "_forkChild") => Some(2),
        _ => None,
    }
}

extern "C" fn sqlite_statement_sync_constructor_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    crate::fs::validate::throw_error_with_code("Illegal constructor", "ERR_ILLEGAL_CONSTRUCTOR")
}

extern "C" fn sqlite_session_constructor_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    crate::fs::validate::throw_error_with_code("Illegal constructor", "ERR_ILLEGAL_CONSTRUCTOR")
}

pub(crate) fn sqlite_statement_sync_constructor_value() -> f64 {
    SQLITE_STATEMENT_SYNC_CONSTRUCTOR_VALUE.with(|slot| {
        let cached = slot.get();
        if cached != 0 {
            return f64::from_bits(cached);
        }

        let func_ptr = sqlite_statement_sync_constructor_thunk as *const u8;
        crate::closure::js_register_closure_arity(func_ptr, 0);
        let closure = crate::closure::js_closure_alloc_singleton(func_ptr);
        if closure.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        set_bound_native_closure_name(closure, "StatementSync");
        let value = crate::value::js_nanbox_pointer(closure as i64);
        slot.set(value.to_bits());
        value
    })
}

pub(crate) fn sqlite_session_constructor_value() -> f64 {
    SQLITE_SESSION_CONSTRUCTOR_VALUE.with(|slot| {
        let cached = slot.get();
        if cached != 0 {
            return f64::from_bits(cached);
        }

        let func_ptr = sqlite_session_constructor_thunk as *const u8;
        crate::closure::js_register_closure_arity(func_ptr, 0);
        let closure = crate::closure::js_closure_alloc_singleton(func_ptr);
        if closure.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        set_bound_native_closure_name(closure, "Session");
        let value = crate::value::js_nanbox_pointer(closure as i64);
        attach_sqlite_session_prototype(value);
        slot.set(value.to_bits());
        value
    })
}

fn native_callable_export_display_name<'a>(module: &str, prop: &'a str) -> &'a str {
    if module == "fs" {
        match prop {
            "_toUnixTimestamp" => "toUnixTimestamp",
            "Stats" => "deprecated",
            _ => prop,
        }
    } else {
        prop
    }
}

extern "C" fn buffer_constructor_thunk(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
    encoding_or_offset: f64,
    length: f64,
) -> f64 {
    let value_js = crate::value::JSValue::from_bits(value.to_bits());
    let buf = if value_js.is_undefined() || value_js.is_null() {
        crate::buffer::js_buffer_alloc(0, 0)
    } else if value_js.is_int32() || value_js.is_number() {
        let size = if value_js.is_int32() {
            value_js.as_int32()
        } else {
            value as i32
        };
        crate::buffer::js_buffer_alloc_unsafe(size)
    } else {
        let second = crate::value::JSValue::from_bits(encoding_or_offset.to_bits());
        let third = crate::value::JSValue::from_bits(length.to_bits());
        let second_is_offset =
            !second.is_undefined() && !second.is_null() && !second.is_any_string();
        if !third.is_undefined() || second_is_offset {
            let len = if third.is_undefined() {
                -1
            } else if third.is_int32() {
                third.as_int32()
            } else {
                length as i32
            };
            let offset = if second.is_int32() {
                second.as_int32()
            } else {
                encoding_or_offset as i32
            };
            crate::buffer::js_buffer_from_arraybuffer_slice(value.to_bits() as i64, offset, len)
        } else {
            let enc = if second.is_undefined() {
                0
            } else {
                crate::buffer::js_encoding_tag_from_value(encoding_or_offset)
            };
            crate::buffer::js_buffer_from_value(value.to_bits() as i64, enc)
        }
    };
    crate::value::js_nanbox_pointer(buf as i64)
}

extern "C" fn buffer_prototype_method_thunk(_closure: *const crate::closure::ClosureHeader) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

const BUFFER_STATIC_METHODS: &[&str] = &[
    "from",
    "alloc",
    "allocUnsafe",
    "allocUnsafeSlow",
    "concat",
    "of",
    "isBuffer",
    "isEncoding",
    "byteLength",
    "compare",
    "copyBytesFrom",
];

/// Node exposes the WHOLE Buffer method surface on `Buffer.prototype`, and it is
/// enumerable — `for (const k in Buffer.prototype)` yields ~93 names there.
/// Perry used to install ELEVEN, which quietly broke any code that walks the
/// prototype: mysql2 sizes every outgoing packet by no-op'ing the write methods
/// of a zero-length Buffer
/// (`for (const k in Buffer.prototype) if (typeof mock[k] === "function") mock[k] = noop`),
/// so `writeUInt32LE` — absent from the stub list — stayed live, wrote into the
/// empty measuring buffer, and killed the MySQL handshake with
/// RangeError [ERR_OUT_OF_RANGE]. Generated from the dispatcher's own
/// `is_buffer_method_name` table so the two can't drift.
const BUFFER_PROTOTYPE_METHODS: &[&str] = &[
    "toString",
    "inspect",
    "slice",
    "subarray",
    "set",
    "copy",
    "write",
    "toJSON",
    "fill",
    "equals",
    "compare",
    "indexOf",
    "lastIndexOf",
    "includes",
    "at",
    "swap16",
    "swap32",
    "swap64",
    "values",
    "keys",
    "entries",
    "undefined",
    "hasOwnProperty",
    "propertyIsEnumerable",
    "valueOf",
    "isPrototypeOf",
    "toLocaleString",
    "readUInt8",
    "readUint8",
    "readInt8",
    "readUInt16BE",
    "readUint16BE",
    "readUInt16LE",
    "readUint16LE",
    "readInt16BE",
    "readInt16LE",
    "readUInt32BE",
    "readUint32BE",
    "readUInt32LE",
    "readUint32LE",
    "readInt32BE",
    "readInt32LE",
    "readFloatBE",
    "readFloatLE",
    "readDoubleBE",
    "readDoubleLE",
    "readBigInt64BE",
    "readBigInt64LE",
    "readBigUInt64BE",
    "readBigUint64BE",
    "readBigUInt64LE",
    "readBigUint64LE",
    "readUIntBE",
    "readUintBE",
    "readUIntLE",
    "readUintLE",
    "readIntBE",
    "readIntLE",
    "writeUInt8",
    "writeUint8",
    "writeInt8",
    "writeUInt16BE",
    "writeUint16BE",
    "writeUInt16LE",
    "writeUint16LE",
    "writeInt16BE",
    "writeInt16LE",
    "writeUInt32BE",
    "writeUint32BE",
    "writeUInt32LE",
    "writeUint32LE",
    "writeInt32BE",
    "writeInt32LE",
    "writeFloatBE",
    "writeFloatLE",
    "writeDoubleBE",
    "writeDoubleLE",
    "writeBigInt64BE",
    "writeBigInt64LE",
    "writeBigUInt64BE",
    "writeBigUint64BE",
    "writeBigUInt64LE",
    "writeBigUint64LE",
    "writeUIntBE",
    "writeUintBE",
    "writeUIntLE",
    "writeUintLE",
    "writeIntBE",
    "writeIntLE",
    "toBase64",
    "toHex",
    "setFromBase64",
    "setFromHex",
    "copyWithin",
    "function",
    "getInt8",
    "getUint8",
    "getInt16",
    "getUint16",
    "getInt32",
    "getUint32",
    "getFloat32",
    "getFloat64",
    "setInt8",
    "setUint8",
    "setInt16",
    "setUint16",
    "setInt32",
    "setUint32",
    "setFloat32",
    "setFloat64",
    "getBigInt64",
    "getBigUint64",
    "setBigInt64",
    "setBigUint64",
];

const SQLITE_DATABASE_SYNC_PROTOTYPE_METHODS: &[&str] = &[
    "open",
    "close",
    "exec",
    "prepare",
    "function",
    "aggregate",
    "enableDefensive",
    "setAuthorizer",
    "createTagStore",
    "createSession",
    "applyChangeset",
    "enableLoadExtension",
    "loadExtension",
    "location",
];

const SQLITE_SESSION_PROTOTYPE_METHODS: &[&str] = &["changeset", "patchset", "close"];

const ASSERT_PROTOTYPE_METHODS: &[&str] = &[
    "fail",
    "ok",
    "equal",
    "notEqual",
    "deepEqual",
    "notDeepEqual",
    "deepStrictEqual",
    "notDeepStrictEqual",
    "strictEqual",
    "notStrictEqual",
    "partialDeepStrictEqual",
    "throws",
    "rejects",
    "doesNotThrow",
    "doesNotReject",
    "ifError",
    "match",
    "doesNotMatch",
];

fn attach_assert_prototype(constructor_value: f64) {
    let constructor_js = JSValue::from_bits(constructor_value.to_bits());
    if !constructor_js.is_pointer() {
        return;
    }
    let closure = constructor_js.as_pointer::<crate::closure::ClosureHeader>() as usize;
    if closure == 0 {
        return;
    }

    let proto = js_object_alloc(0, 0);
    if proto.is_null() {
        return;
    }

    let constructor = "constructor";
    let constructor_key =
        crate::string::js_string_from_bytes(constructor.as_ptr(), constructor.len() as u32);
    js_object_set_field_by_name(proto, constructor_key, constructor_value);
    super::set_builtin_property_attrs(
        proto as usize,
        constructor.to_string(),
        super::PropertyAttrs::new(true, false, true),
    );

    for method in ASSERT_PROTOTYPE_METHODS {
        let method_value = bound_native_callable_export_value("assert", method);
        let key = crate::string::js_string_from_bytes(method.as_ptr(), method.len() as u32);
        js_object_set_field_by_name(proto, key, method_value);
        super::set_builtin_property_attrs(
            proto as usize,
            (*method).to_string(),
            super::PropertyAttrs::new(true, false, true),
        );
    }

    let proto_value = crate::value::js_nanbox_pointer(proto as i64);
    crate::closure::closure_set_dynamic_prop(closure, "prototype", proto_value);
    super::set_builtin_property_attrs(
        closure,
        "prototype".to_string(),
        super::PropertyAttrs::new(true, false, false),
    );
}

extern "C" fn sqlite_database_sync_prototype_method_thunk(
    closure: *const crate::closure::ClosureHeader,
    arg0: f64,
    arg1: f64,
    arg2: f64,
) -> f64 {
    unsafe {
        let method_name_ptr = crate::closure::js_closure_get_capture_ptr(closure, 0) as *const i8;
        let method_name_len = crate::closure::js_closure_get_capture_ptr(closure, 1) as usize;
        let receiver = crate::object::js_implicit_this_get();
        let args = [arg0, arg1, arg2];
        crate::object::js_native_call_method(
            receiver,
            method_name_ptr,
            method_name_len,
            args.as_ptr(),
            args.len(),
        )
    }
}

fn attach_sqlite_database_sync_prototype(constructor_value: f64) {
    let constructor_js = JSValue::from_bits(constructor_value.to_bits());
    if !constructor_js.is_pointer() {
        return;
    }
    let closure = constructor_js.as_pointer::<crate::closure::ClosureHeader>() as usize;
    if closure == 0 {
        return;
    }

    let proto = js_object_alloc(0, 0);
    if proto.is_null() {
        return;
    }

    let constructor = "constructor";
    let constructor_key =
        crate::string::js_string_from_bytes(constructor.as_ptr(), constructor.len() as u32);
    js_object_set_field_by_name(proto, constructor_key, constructor_value);
    super::set_builtin_property_attrs(
        proto as usize,
        constructor.to_string(),
        super::PropertyAttrs::new(true, false, true),
    );

    let func_ptr = sqlite_database_sync_prototype_method_thunk as *const u8;
    crate::closure::js_register_closure_arity(func_ptr, 3);
    for method in SQLITE_DATABASE_SYNC_PROTOTYPE_METHODS {
        let leaked: &'static [u8] = method.as_bytes().to_vec().leak();
        let method_closure = crate::closure::js_closure_alloc(func_ptr, 2);
        if method_closure.is_null() {
            continue;
        }
        crate::closure::js_closure_set_capture_ptr(method_closure, 0, leaked.as_ptr() as i64);
        crate::closure::js_closure_set_capture_ptr(method_closure, 1, leaked.len() as i64);
        set_bound_native_closure_name(method_closure, method);
        set_builtin_closure_length(method_closure as usize, 0);
        let key = crate::string::js_string_from_bytes(method.as_ptr(), method.len() as u32);
        let method_value = crate::value::js_nanbox_pointer(method_closure as i64);
        js_object_set_field_by_name(proto, key, method_value);
        super::set_builtin_property_attrs(
            proto as usize,
            (*method).to_string(),
            super::PropertyAttrs::new(true, false, true),
        );
    }

    let proto_value = crate::value::js_nanbox_pointer(proto as i64);
    crate::closure::closure_set_dynamic_prop(closure, "prototype", proto_value);
    super::set_builtin_property_attrs(
        closure,
        "prototype".to_string(),
        super::PropertyAttrs::new(true, false, false),
    );
}

fn attach_sqlite_session_prototype(constructor_value: f64) {
    let constructor_js = JSValue::from_bits(constructor_value.to_bits());
    if !constructor_js.is_pointer() {
        return;
    }
    let closure = constructor_js.as_pointer::<crate::closure::ClosureHeader>() as usize;
    if closure == 0 {
        return;
    }

    let proto = js_object_alloc(0, 0);
    if proto.is_null() {
        return;
    }

    let func_ptr = sqlite_database_sync_prototype_method_thunk as *const u8;
    crate::closure::js_register_closure_arity(func_ptr, 3);
    for method in SQLITE_SESSION_PROTOTYPE_METHODS {
        let leaked: &'static [u8] = method.as_bytes().to_vec().leak();
        let method_closure = crate::closure::js_closure_alloc(func_ptr, 2);
        if method_closure.is_null() {
            continue;
        }
        crate::closure::js_closure_set_capture_ptr(method_closure, 0, leaked.as_ptr() as i64);
        crate::closure::js_closure_set_capture_ptr(method_closure, 1, leaked.len() as i64);
        set_bound_native_closure_name(method_closure, method);
        set_builtin_closure_length(method_closure as usize, 0);
        let key = crate::string::js_string_from_bytes(method.as_ptr(), method.len() as u32);
        let method_value = crate::value::js_nanbox_pointer(method_closure as i64);
        js_object_set_field_by_name(proto, key, method_value);
        super::set_builtin_property_attrs(
            proto as usize,
            (*method).to_string(),
            super::PropertyAttrs::new(true, true, true),
        );
    }

    let dispose_method = "@@__perry_wk_dispose";
    let dispose_leaked: &'static [u8] = dispose_method.as_bytes().to_vec().leak();
    let dispose_closure = crate::closure::js_closure_alloc(func_ptr, 2);
    if !dispose_closure.is_null() {
        crate::closure::js_closure_set_capture_ptr(
            dispose_closure,
            0,
            dispose_leaked.as_ptr() as i64,
        );
        crate::closure::js_closure_set_capture_ptr(dispose_closure, 1, dispose_leaked.len() as i64);
        set_bound_native_closure_name(dispose_closure, "[Symbol.dispose]");
        set_builtin_closure_length(dispose_closure as usize, 0);
        let dispose_value = crate::value::js_nanbox_pointer(dispose_closure as i64);
        let dispose_sym = crate::symbol::well_known_symbol("dispose");
        if !dispose_sym.is_null() {
            let dispose_sym_value = crate::value::js_nanbox_pointer(dispose_sym as i64);
            unsafe {
                crate::symbol::js_object_set_symbol_property(
                    crate::value::js_nanbox_pointer(proto as i64),
                    dispose_sym_value,
                    dispose_value,
                );
            }
        }
    }

    let constructor = "constructor";
    let constructor_key =
        crate::string::js_string_from_bytes(constructor.as_ptr(), constructor.len() as u32);
    js_object_set_field_by_name(proto, constructor_key, constructor_value);
    super::set_builtin_property_attrs(
        proto as usize,
        constructor.to_string(),
        super::PropertyAttrs::new(true, false, true),
    );

    let proto_value = crate::value::js_nanbox_pointer(proto as i64);
    crate::closure::closure_set_dynamic_prop(closure, "prototype", proto_value);
    super::set_builtin_property_attrs(
        closure,
        "prototype".to_string(),
        super::PropertyAttrs::new(true, false, false),
    );
}

pub(crate) fn buffer_constructor_value() -> f64 {
    BUFFER_CONSTRUCTOR_VALUE.with(|slot| {
        let cached = slot.get();
        if cached != 0 {
            return f64::from_bits(cached);
        }

        let func_ptr = buffer_constructor_thunk as *const u8;
        let closure = crate::closure::js_closure_alloc(func_ptr, 0);
        if closure.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        crate::closure::js_register_closure_arity(func_ptr, 3);
        set_bound_native_closure_name(closure, "Buffer");
        let closure_addr = closure as usize;
        let value = crate::value::js_nanbox_pointer(closure as i64);

        for method in BUFFER_STATIC_METHODS {
            let method_value = bound_native_callable_export_value("buffer.Buffer", method);
            crate::closure::closure_set_dynamic_prop(closure_addr, method, method_value);
        }

        crate::closure::closure_set_dynamic_prop(closure_addr, "poolSize", buffer_pool_size());

        let proto = js_object_alloc(0, 0);
        if !proto.is_null() {
            let constructor = "constructor";
            let constructor_key =
                crate::string::js_string_from_bytes(constructor.as_ptr(), constructor.len() as u32);
            js_object_set_field_by_name(proto, constructor_key, value);
            super::set_builtin_property_attrs(
                proto as usize,
                constructor.to_string(),
                super::PropertyAttrs::new(true, false, true),
            );

            for method in BUFFER_PROTOTYPE_METHODS {
                let method_ptr = buffer_prototype_method_thunk as *const u8;
                let method_closure = crate::closure::js_closure_alloc(method_ptr, 0);
                if method_closure.is_null() {
                    continue;
                }
                set_bound_native_closure_name(method_closure, method);
                let key = crate::string::js_string_from_bytes(method.as_ptr(), method.len() as u32);
                let method_value = crate::value::js_nanbox_pointer(method_closure as i64);
                js_object_set_field_by_name(proto, key, method_value);
            }
            let proto_value = crate::value::js_nanbox_pointer(proto as i64);
            crate::closure::closure_set_dynamic_prop(closure_addr, "prototype", proto_value);
            super::set_builtin_property_attrs(
                closure_addr,
                "prototype".to_string(),
                super::PropertyAttrs::new(true, false, false),
            );
        }

        slot.set(value.to_bits());
        value
    })
}

pub(crate) fn is_buffer_constructor_value(value: f64) -> bool {
    BUFFER_CONSTRUCTOR_VALUE.with(|slot| {
        let cached = slot.get();
        cached != 0 && cached == value.to_bits()
    })
}

fn attach_crypto_key_object_shape(closure_addr: usize, constructor_value: f64) {
    let from_value = bound_native_callable_export_value("crypto.KeyObject", "from");
    crate::closure::closure_set_dynamic_prop(closure_addr, "from", from_value);
    super::set_builtin_property_attrs(
        closure_addr,
        "from".to_string(),
        super::PropertyAttrs::new(true, false, true),
    );

    let proto = js_object_alloc(0, 0);
    if proto.is_null() {
        return;
    }
    let constructor = "constructor";
    let constructor_key =
        crate::string::js_string_from_bytes(constructor.as_ptr(), constructor.len() as u32);
    js_object_set_field_by_name(proto, constructor_key, constructor_value);
    super::set_builtin_property_attrs(
        proto as usize,
        constructor.to_string(),
        super::PropertyAttrs::new(true, false, true),
    );

    let proto_value = crate::value::js_nanbox_pointer(proto as i64);
    crate::closure::closure_set_dynamic_prop(closure_addr, "prototype", proto_value);
    super::set_builtin_property_attrs(
        closure_addr,
        "prototype".to_string(),
        super::PropertyAttrs::new(true, false, false),
    );
}

extern "C" fn x509_issuer_certificate_getter_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn attach_crypto_x509_certificate_shape(closure_addr: usize, constructor_value: f64) {
    let proto = js_object_alloc(0, 0);
    if proto.is_null() {
        return;
    }
    let constructor = "constructor";
    let constructor_key =
        crate::string::js_string_from_bytes(constructor.as_ptr(), constructor.len() as u32);
    js_object_set_field_by_name(proto, constructor_key, constructor_value);
    super::set_builtin_property_attrs(
        proto as usize,
        constructor.to_string(),
        super::PropertyAttrs::new(true, false, true),
    );

    unsafe {
        crate::closure::js_register_closure_arity(
            x509_issuer_certificate_getter_thunk as *const u8,
            0,
        );
        let getter =
            crate::closure::js_closure_alloc(x509_issuer_certificate_getter_thunk as *const u8, 0);
        if !getter.is_null() {
            let getter_bits = crate::value::js_nanbox_pointer(getter as i64).to_bits();
            super::object_ops::install_builtin_getter(proto, "issuerCertificate", getter_bits);
        }
    }

    let proto_value = crate::value::js_nanbox_pointer(proto as i64);
    crate::closure::closure_set_dynamic_prop(closure_addr, "prototype", proto_value);
    super::set_builtin_property_attrs(
        closure_addr,
        "prototype".to_string(),
        super::PropertyAttrs::new(true, false, false),
    );
}

pub(crate) fn native_string_value(value: &str) -> f64 {
    let ptr = crate::string::js_string_from_bytes(value.as_ptr(), value.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn native_bool_value(value: bool) -> f64 {
    f64::from_bits(JSValue::bool(value).bits())
}

fn native_object_value(obj: *mut ObjectHeader) -> f64 {
    crate::value::js_nanbox_pointer(obj as i64)
}

fn native_set_field(obj: *mut ObjectHeader, name: &str, value: f64) {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(obj, key, value);
}

extern "C" fn module_cjs_extension_noop_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _module: f64,
    _filename: f64,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn module_cjs_extension_function(name: &str) -> f64 {
    let func_ptr = module_cjs_extension_noop_thunk as *const u8;
    crate::closure::js_register_closure_arity(func_ptr, 2);
    crate::closure::js_register_closure_length(func_ptr, 2);
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    set_bound_native_closure_name(closure, name);
    crate::object::set_builtin_closure_length(closure as usize, 2);
    crate::value::js_nanbox_pointer(closure as i64)
}

fn store_module_cjs_root(slot: &Cell<u64>, value: f64) -> f64 {
    slot.set(value.to_bits());
    crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
    value
}

pub(crate) fn module_cjs_cache_value() -> f64 {
    MODULE_CJS_CACHE_VALUE.with(|slot| {
        let bits = slot.get();
        if bits != 0 {
            return f64::from_bits(bits);
        }
        let obj = crate::object::js_object_alloc_null_proto(0, 0);
        store_module_cjs_root(slot, native_object_value(obj))
    })
}

pub(crate) fn module_cjs_path_cache_value() -> f64 {
    MODULE_CJS_PATH_CACHE_VALUE.with(|slot| {
        let bits = slot.get();
        if bits != 0 {
            return f64::from_bits(bits);
        }
        let obj = crate::object::js_object_alloc_null_proto(0, 0);
        store_module_cjs_root(slot, native_object_value(obj))
    })
}

pub(crate) fn module_cjs_extensions_value() -> f64 {
    MODULE_CJS_EXTENSIONS_VALUE.with(|slot| {
        let bits = slot.get();
        if bits != 0 {
            return f64::from_bits(bits);
        }
        let obj = js_object_alloc(0, 3);
        native_set_field(obj, ".js", module_cjs_extension_function(".js"));
        native_set_field(obj, ".json", module_cjs_extension_function(".json"));
        native_set_field(obj, ".node", module_cjs_extension_function(".node"));
        store_module_cjs_root(slot, native_object_value(obj))
    })
}

pub(crate) fn module_cjs_global_paths_value() -> f64 {
    MODULE_CJS_GLOBAL_PATHS_VALUE.with(|slot| {
        let bits = slot.get();
        if bits != 0 {
            return f64::from_bits(bits);
        }

        let mut paths = Vec::new();
        if let Some(home) = std::env::var_os("HOME") {
            let home = std::path::PathBuf::from(home);
            paths.push(home.join(".node_modules").to_string_lossy().into_owned());
            paths.push(home.join(".node_libraries").to_string_lossy().into_owned());
        }
        let prefix = std::env::var("PREFIX").unwrap_or_else(|_| "/usr/local".to_string());
        paths.push(format!("{prefix}/lib/node"));

        let arr = crate::array::js_array_alloc_with_length(paths.len() as u32);
        for (i, path) in paths.iter().enumerate() {
            crate::array::js_array_set_f64(arr, i as u32, native_string_value(path));
        }
        store_module_cjs_root(slot, f64::from_bits(JSValue::array_ptr(arr).bits()))
    })
}

fn attach_module_cjs_constructor_statics(closure_addr: usize) {
    crate::closure::closure_set_dynamic_prop(closure_addr, "_cache", module_cjs_cache_value());
    crate::closure::closure_set_dynamic_prop(
        closure_addr,
        "_extensions",
        module_cjs_extensions_value(),
    );
    crate::closure::closure_set_dynamic_prop(
        closure_addr,
        "_pathCache",
        module_cjs_path_cache_value(),
    );
    crate::closure::closure_set_dynamic_prop(
        closure_addr,
        "globalPaths",
        module_cjs_global_paths_value(),
    );
    for name in [
        "_findPath",
        "_initPaths",
        "_load",
        "_nodeModulePaths",
        "_preloadModules",
        "_resolveFilename",
        "_resolveLookupPaths",
    ] {
        crate::closure::closure_set_dynamic_prop(
            closure_addr,
            name,
            bound_native_callable_export_value("module", name),
        );
    }
    // `Module.prototype` — Node's require-hook pattern (Next.js):
    // `const mod = require('module'); const orig = mod.prototype.require;
    // mod.prototype.require = function(request) {…}`. Expose a plain object
    // carrying a `require` method so the read+patch round-trips; the patch
    // is inert under AOT compilation (Perry resolves modules at compile
    // time), but startup must not throw on the access.
    let proto = js_object_alloc(0, 1);
    native_set_field(
        proto,
        "require",
        bound_native_callable_export_value("module", "_load"),
    );
    crate::closure::closure_set_dynamic_prop(
        closure_addr,
        "prototype",
        crate::value::js_nanbox_pointer(proto as i64),
    );
}

fn native_color_tuple(open: i32, close: i32) -> f64 {
    let arr = crate::array::js_array_alloc_with_length(2);
    crate::array::js_array_set_f64(arr, 0, open as f64);
    crate::array::js_array_set_f64(arr, 1, close as f64);
    f64::from_bits(JSValue::array_ptr(arr).bits())
}

fn util_inspect_custom_symbol() -> f64 {
    unsafe { crate::symbol::js_symbol_for(native_string_value("nodejs.util.inspect.custom")) }
}

pub(crate) fn util_inspect_default_options_value() -> f64 {
    UTIL_INSPECT_DEFAULT_OPTIONS.with(|slot| {
        let bits = slot.get();
        if bits != 0 {
            return f64::from_bits(bits);
        }

        let obj = js_object_alloc(0, 0);
        native_set_field(obj, "showHidden", native_bool_value(false));
        native_set_field(obj, "depth", 2.0);
        native_set_field(obj, "colors", native_bool_value(false));
        native_set_field(obj, "customInspect", native_bool_value(true));
        native_set_field(obj, "showProxy", native_bool_value(false));
        native_set_field(obj, "maxArrayLength", 100.0);
        native_set_field(obj, "maxStringLength", 10000.0);
        native_set_field(obj, "breakLength", 80.0);
        native_set_field(obj, "compact", 3.0);
        native_set_field(obj, "sorted", native_bool_value(false));
        native_set_field(obj, "getters", native_bool_value(false));
        native_set_field(obj, "numericSeparator", native_bool_value(false));

        let value = native_object_value(obj);
        slot.set(value.to_bits());
        crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
        value
    })
}

fn util_inspect_styles() -> f64 {
    UTIL_INSPECT_STYLES.with(|slot| {
        let bits = slot.get();
        if bits != 0 {
            return f64::from_bits(bits);
        }

        let obj = js_object_alloc(0, 0);
        native_set_field(obj, "special", native_string_value("cyan"));
        native_set_field(obj, "number", native_string_value("yellow"));
        native_set_field(obj, "bigint", native_string_value("yellow"));
        native_set_field(obj, "boolean", native_string_value("yellow"));
        native_set_field(obj, "undefined", native_string_value("grey"));
        native_set_field(obj, "null", native_string_value("bold"));
        native_set_field(obj, "string", native_string_value("green"));
        native_set_field(obj, "symbol", native_string_value("green"));
        native_set_field(obj, "date", native_string_value("magenta"));
        native_set_field(obj, "regexp", native_string_value("red"));
        native_set_field(obj, "module", native_string_value("underline"));

        let value = native_object_value(obj);
        slot.set(value.to_bits());
        crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
        value
    })
}

fn util_inspect_colors() -> f64 {
    UTIL_INSPECT_COLORS.with(|slot| {
        let bits = slot.get();
        if bits != 0 {
            return f64::from_bits(bits);
        }

        let obj = js_object_alloc(0, 0);
        for style in crate::util_style_text::INSPECT_COLOR_STYLES {
            native_set_field(obj, style.name, native_color_tuple(style.open, style.close));
        }

        let value = native_object_value(obj);
        slot.set(value.to_bits());
        crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
        value
    })
}

pub(crate) fn zlib_codes_object() -> f64 {
    const ZLIB_RETURN_CODES: &[(&str, i32)] = &[
        ("Z_OK", 0),
        ("Z_STREAM_END", 1),
        ("Z_NEED_DICT", 2),
        ("Z_ERRNO", -1),
        ("Z_STREAM_ERROR", -2),
        ("Z_DATA_ERROR", -3),
        ("Z_MEM_ERROR", -4),
        ("Z_BUF_ERROR", -5),
        ("Z_VERSION_ERROR", -6),
    ];

    ZLIB_CODES_OBJECT.with(|slot| {
        let bits = slot.get();
        if bits != 0 {
            return f64::from_bits(bits);
        }

        let obj = js_object_alloc(0, 0);
        for (name, value) in ZLIB_RETURN_CODES.iter().take(3) {
            native_set_field(obj, &value.to_string(), native_string_value(name));
        }
        for (name, value) in ZLIB_RETURN_CODES {
            native_set_field(obj, name, *value as f64);
        }
        for (name, value) in ZLIB_RETURN_CODES.iter().skip(3) {
            native_set_field(obj, &value.to_string(), native_string_value(name));
        }

        let value = native_object_value(obj);
        slot.set(value.to_bits());
        crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
        value
    })
}

pub(crate) fn timers_promises_parent_namespace() -> f64 {
    TIMERS_PROMISES_PARENT_NAMESPACE.with(|slot| {
        let bits = slot.get();
        if bits != 0 {
            return f64::from_bits(bits);
        }

        let module_name = "timers/promises";
        let value = js_create_native_module_namespace(module_name.as_ptr(), module_name.len());
        slot.set(value.to_bits());
        crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
        value
    })
}

extern "C" fn util_debuglog_logger_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

pub(crate) fn util_debuglog_logger_value() -> f64 {
    let func_ptr = util_debuglog_logger_thunk as *const u8;
    crate::closure::js_register_closure_arity(func_ptr, 1);
    let closure = crate::closure::js_closure_alloc_singleton(func_ptr);
    set_bound_native_closure_name(closure, "debuglog");
    crate::value::js_nanbox_pointer(closure as i64)
}

fn attach_tty_stream_prototype(constructor_value: f64, name: &str) {
    crate::tty::attach_tty_constructor_prototype(constructor_value, name);
}

fn attach_tls_secure_context_prototype(constructor_value: f64) {
    crate::tls::attach_secure_context_constructor_prototype(constructor_value);
}

pub(crate) unsafe fn bound_native_callable_module_and_method(
    value: f64,
) -> Option<(String, String)> {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let closure = jv.as_pointer::<crate::closure::ClosureHeader>();
    // A POINTER-tagged value is not necessarily a heap closure: a Proxy is
    // POINTER-tagged over a SMALL encoded id that lands in the handle band
    // (`class X extends new Proxy(fn, …)`). Dereferencing its `type_tag` would
    // read offset 0xc of a bogus ~handle-sized address and segfault. Classify
    // through `is_closure_ptr`, which rejects the handle band AND every
    // non-heap address on EVERY platform (the macOS-only `is_valid_obj_ptr`
    // heap floor lets a `0xf0000`-band id through on Linux — the #5592 sweep
    // host) before probing `CLOSURE_MAGIC`. Refs test262
    // class/subclass/superclass-{arrow,async,generator,async-generator}-function.
    if !crate::closure::is_closure_ptr(closure as usize)
        || (*closure).func_ptr != crate::closure::BOUND_METHOD_FUNC_PTR
    {
        return None;
    }
    let ns = crate::closure::js_closure_get_capture_f64(closure, 0);
    let module = get_module_name_from_namespace(ns).to_string();
    let method_ptr = crate::closure::js_closure_get_capture_ptr(closure, 1) as *const u8;
    let method_len = crate::closure::js_closure_get_capture_ptr(closure, 2) as usize;
    if method_ptr.is_null() {
        return None;
    }
    let method = std::str::from_utf8(std::slice::from_raw_parts(method_ptr, method_len))
        .ok()?
        .to_string();
    Some((module, method))
}

pub(crate) unsafe fn bound_native_callable_value_arity(value: f64) -> Option<u32> {
    let (module, method) = bound_native_callable_module_and_method(value)?;
    let module = normalize_native_module_alias(&module);
    match (module, method.as_str()) {
        ("console", "Console") => Some(1),
        ("util", "isArray") => Some(1),
        ("module", "isBuiltin") => Some(1),
        ("process", "getBuiltinModule") => Some(1),
        _ => native_callable_export_arity(module, method.as_str()),
    }
}

pub(crate) fn set_bound_native_closure_name(
    closure: *mut crate::closure::ClosureHeader,
    name: &str,
) {
    let ptr = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let name_value = f64::from_bits(JSValue::string_ptr(ptr).bits());
    crate::closure::closure_set_dynamic_prop(closure as usize, "name", name_value);
    // Spec: a function's `name` property is { writable:false, enumerable:false,
    // configurable:true }. Storing it as a plain dynamic prop left it ENUMERABLE
    // by default, so `for (k in Buffer)` yielded "name" — even though
    // `getOwnPropertyDescriptor(Buffer,'name').enumerable` correctly reported
    // false via the function-name special case. The inconsistency broke
    // safe-buffer's `copyProps(Buffer, SafeBuffer)` (`for (k in Buffer)
    // SafeBuffer[k] = Buffer[k]`): it copied "name" onto SafeBuffer, whose own
    // `name` is read-only, throwing `Cannot assign to read only property 'name'`
    // in strict mode (jsonwebtoken → Next.js). Pin the proper descriptor so
    // enumeration matches reflection.
    crate::object::set_property_attrs(
        closure as usize,
        "name".to_string(),
        crate::object::PropertyAttrs::new(false, false, true),
    );
}

thread_local! {
    /// Per-closure spec `.length` for built-in *prototype methods*. Those
    /// methods all share one no-op closure thunk
    /// (`global_this_builtin_noop_thunk`), so the func-ptr-keyed
    /// `CLOSURE_ARITY_REGISTRY` can't give `Array.prototype.map.length === 1`
    /// while `Array.prototype.slice.length === 2` — the last install would
    /// win for every method. Recording the length per *closure instance* here
    /// (keyed by the closure pointer, like the user-facing dynamic-prop table
    /// but isolated from it so a user `fn.length = x` write can't perturb it)
    /// lets the `.length` value-read and `getOwnPropertyDescriptor` agree with
    /// the spec count. #3143.
    static BUILTIN_CLOSURE_LENGTH: std::cell::RefCell<std::collections::HashMap<usize, u32>> =
        std::cell::RefCell::new(std::collections::HashMap::new());

    /// Built-in method closures are callable but lack ECMAScript
    /// `[[Construct]]`. Track the installed closure values so the dynamic
    /// `new` / `Reflect.construct` paths can reject them without changing
    /// ordinary user closures or global constructor closures.
    static BUILTIN_CLOSURE_NON_CONSTRUCTABLE: std::cell::RefCell<std::collections::HashSet<usize>> =
        std::cell::RefCell::new(std::collections::HashSet::new());
}

/// Record the spec `.length` for a built-in prototype-method closure. See
/// [`BUILTIN_CLOSURE_LENGTH`].
pub(crate) fn set_builtin_closure_length(closure: usize, length: u32) {
    BUILTIN_CLOSURE_LENGTH.with(|m| {
        m.borrow_mut().insert(closure, length);
    });
}

/// Look up the recorded spec `.length` for a built-in prototype-method
/// closure, or `None` if this closure isn't one. See [`BUILTIN_CLOSURE_LENGTH`].
pub(crate) fn builtin_closure_length(closure: usize) -> Option<u32> {
    BUILTIN_CLOSURE_LENGTH.with(|m| m.borrow().get(&closure).copied())
}

pub(crate) fn set_builtin_closure_non_constructable(closure: usize) {
    BUILTIN_CLOSURE_NON_CONSTRUCTABLE.with(|m| {
        m.borrow_mut().insert(closure);
    });
}

pub(crate) fn builtin_closure_is_non_constructable(closure: usize) -> bool {
    BUILTIN_CLOSURE_NON_CONSTRUCTABLE.with(|m| m.borrow().contains(&closure))
}

pub(crate) fn builtin_closure_is_non_constructable_value(value: f64) -> bool {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    let ptr = jv.as_pointer::<crate::closure::ClosureHeader>();
    if ptr.is_null() {
        return false;
    }
    builtin_closure_is_non_constructable(ptr as usize)
}
