use super::*;
use crate::JSValue;

crate::perry_thread_local! {
    /// `new.target` for the construction currently on this thread's stack.
    ///
    /// **This is a GC root, and must stay one (#7231).** It holds a NaN-boxed
    /// closure/class value for the whole constructor body, and a constructor
    /// body runs arbitrary user code. `this_binding.rs`'s `NEW_TARGET` holds
    /// the same value under `scan_implicit_this_roots_mut`; this is a second
    /// copy on a different path, and a second copy of a root that is not
    /// itself a root is exactly the shape #7226 found in `prev_this`.
    ///
    /// RESIDUAL, deliberately not closed here: the save/restore idiom parks
    /// the DISPLACED value in a bare Rust local (`prev_current_new_target`)
    /// across the construction and republishes it afterwards. Runtime frames
    /// are not covered by the precise scan, so that local is #7226's
    /// `prev_this` defect in Rust rather than in codegen. Closing it means
    /// routing the three save sites through a `RuntimeHandleScope`, which
    /// wants its own before/after rather than being appended here.
    static CURRENT_NEW_TARGET: std::cell::Cell<u64> =
        const { std::cell::Cell::new(crate::value::TAG_UNDEFINED) };
}

/// Root + rewrite the in-flight `new.target`.
pub(crate) fn scan_current_new_target_root_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    CURRENT_NEW_TARGET.with(|cell| {
        let mut bits = cell.get();
        if visitor.visit_nanbox_u64_slot(&mut bits) {
            cell.set(bits);
        }
    });
}

#[no_mangle]
pub extern "C" fn js_new_target_value() -> f64 {
    f64::from_bits(CURRENT_NEW_TARGET.with(|value| value.get()))
}

/// Issue #838 followup (b): construct an instance from a function value.
/// Pairs with `js_register_function_prototype_method` — both arms route
/// through `synthetic_class_id_for_function` so the instance's
/// `class_id` matches the bucket prototype methods were registered
/// against. Allocates a fresh object stamped with the synthetic id,
/// then invokes the function as the constructor with `IMPLICIT_THIS`
/// bound to the new object so any `this.foo = …` writes in the
/// function body land on the instance. Returns the NaN-boxed new
/// instance pointer.
///
/// `func_value` must be a POINTER_TAG'd closure. `args_ptr` is a flat
/// f64 array of length `args_len`. Falls back to a class_id=0
/// empty-object allocation when the function value isn't a closure
/// (preserves the pre-fix baseline for misuse).
// ── Per-module constructor buckets (devirt phase 2) ────────────────────────
// `new <namespace>.<Ctor>()` for node-module-namespaced constructors that the
// old monolithic `js_new_function_construct` dispatched with a direct call to
// the subsystem's `*_new` — statically pinning tty/fs/vm/tls/wasi/repl/stream/
// readline handlers into every binary. Each is now a per-module fn reached only
// through NM_CTOR_REGISTRY, registered by the same `js_nm_install_<module>()`
// that codegen emits when the module is imported. `None` ⇒ not a ctor this
// module owns; caller falls through (e.g. to the http/events/zlib dynamic
// dispatchers, which already strip on their own). Helper to read arg N.
#[inline]
unsafe fn nm_ctor_arg(args_ptr: *const f64, args_len: usize, n: usize) -> f64 {
    if !args_ptr.is_null() && args_len > n {
        *args_ptr.add(n)
    } else {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    }
}

pub(crate) unsafe fn nm_ctor_tty(
    _module: &str,
    method: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if matches!(method, "ReadStream" | "WriteStream") {
        let fd = nm_ctor_arg(args_ptr, args_len, 0);
        return Some(if method == "ReadStream" {
            crate::tty::js_tty_read_stream_new(fd)
        } else {
            crate::tty::js_tty_write_stream_new(fd)
        });
    }
    None
}

pub(crate) unsafe fn nm_ctor_child_process(
    _module: &str,
    method: &str,
    _args_ptr: *const f64,
    _args_len: usize,
) -> Option<f64> {
    (method == "ChildProcess").then(crate::child_process::cp_build_unstarted_child_process)
}

pub(crate) unsafe fn nm_ctor_cluster(
    _module: &str,
    method: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    (method == "Worker")
        .then(|| crate::cluster::js_cluster_worker_new(nm_ctor_arg(args_ptr, args_len, 0)))
}

pub(crate) unsafe fn nm_ctor_fs(
    _module: &str,
    method: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if method == "Utf8Stream" {
        return Some(crate::fs::js_fs_utf8_stream_new(nm_ctor_arg(
            args_ptr, args_len, 0,
        )));
    }
    if matches!(
        method,
        "ReadStream" | "FileReadStream" | "WriteStream" | "FileWriteStream"
    ) {
        let path = nm_ctor_arg(args_ptr, args_len, 0);
        let options = nm_ctor_arg(args_ptr, args_len, 1);
        return Some(if matches!(method, "ReadStream" | "FileReadStream") {
            crate::fs::js_fs_create_read_stream(path, options)
        } else {
            crate::fs::js_fs_create_write_stream(path, options)
        });
    }
    None
}

pub(crate) unsafe fn nm_ctor_vm(
    _module: &str,
    method: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if method == "Script" {
        let code = nm_ctor_arg(args_ptr, args_len, 0);
        let options = nm_ctor_arg(args_ptr, args_len, 1);
        return Some(super::brand_vm_script_instance(
            crate::node_vm::js_vm_script_new(code, options),
        ));
    }
    None
}

pub(crate) unsafe fn nm_ctor_tls(
    _module: &str,
    method: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if method == "SecureContext" {
        return Some(crate::tls::js_tls_secure_context_new(nm_ctor_arg(
            args_ptr, args_len, 0,
        )));
    }
    None
}

pub(crate) unsafe fn nm_ctor_wasi(
    _module: &str,
    method: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if method == "WASI" {
        return Some(crate::wasi::js_wasi_new(nm_ctor_arg(args_ptr, args_len, 0)));
    }
    None
}

pub(crate) unsafe fn nm_ctor_readline(
    module: &str,
    method: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if module == "readline/promises" && method == "Readline" {
        let output = nm_ctor_arg(args_ptr, args_len, 0);
        let options = nm_ctor_arg(args_ptr, args_len, 1);
        return Some(crate::node_submodules::js_readline_promises_readline_new(
            output, options,
        ));
    }
    None
}

pub(crate) unsafe fn nm_ctor_repl(
    _module: &str,
    method: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if matches!(method, "Recoverable" | "REPLServer") {
        let first = nm_ctor_arg(args_ptr, args_len, 0);
        return Some(if method == "Recoverable" {
            crate::node_repl::js_repl_recoverable_new(first)
        } else {
            crate::node_repl::js_repl_repl_server_new(first)
        });
    }
    None
}

pub(crate) unsafe fn nm_ctor_stream(
    _module: &str,
    method: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if matches!(
        method,
        "Readable" | "Writable" | "Duplex" | "Transform" | "PassThrough"
    ) {
        let opts = nm_ctor_arg(args_ptr, args_len, 0);
        return Some(match method {
            "Readable" => crate::node_stream::js_node_stream_readable_new(opts),
            "Writable" => crate::node_stream::js_node_stream_writable_new(opts),
            "Duplex" => crate::node_stream::js_node_stream_duplex_new(opts),
            "Transform" => crate::node_stream::js_node_stream_transform_new(opts),
            "PassThrough" => crate::node_stream::js_node_stream_passthrough_new(opts),
            _ => unreachable!(),
        });
    }
    None
}

#[no_mangle]
pub unsafe extern "C" fn js_new_function_construct(
    func_value: f64,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    // `new <primitive>()` is a TypeError — a primitive is never a constructor
    // (`new undefined()`, `new 5n()`, `new "s"()`, `new true()`). Checked via
    // the unambiguous NaN-box tags only (NOT `is_number`, whose f64 range
    // overlaps the raw-i64 pointer encoding of module-level objects). Without
    // this, `new x.method()` where `x.method` reads back `undefined`, and other
    // primitive callees, silently fell through to the empty-object fallback.
    {
        let jv = crate::value::JSValue::from_bits(func_value.to_bits());
        if jv.is_undefined()
            || jv.is_null()
            || jv.is_bool()
            || (jv.is_int32() && constructor_class_ref_id(func_value).is_none())
            || jv.is_any_string()
            || jv.is_bigint()
        {
            let desc =
                unsafe { super::super::object_ops::describe_value_for_type_error(func_value) };
            super::super::object_ops::throw_object_type_error_with_suffix(
                &format!("{desc} "),
                "is not a constructor",
            );
        }
    }
    // `new boundFn(...)` — delegate entirely to the newTarget-aware path,
    // which unwinds the bind chain to the real target before re-dispatching
    // (see `unwrap_bound_construct_chain`). `new`'s implicit newTarget is
    // `func_value` itself, matching spec step "If NewTarget is not present,
    // set newTarget to F" for a plain `new`. Bypasses everything below —
    // primitive/proxy/non-constructable/native-module checks all re-run on
    // the UNWRAPPED target, which is what they need to see (a bound wrapper
    // around `Date`, a bound wrapper around a non-constructor, etc.).
    if is_bound_function_closure_value(func_value) {
        return js_new_function_construct_with_new_target(
            func_value, args_ptr, args_len, func_value,
        );
    }
    // `new (new String(""))` / `new (new Number(1))` — a boxed primitive WRAPPER
    // object is an ordinary object, never a constructor, so `new` on it throws
    // `TypeError` (Test262 `S15.5.5_A2`). Without this it fell through to the
    // empty-object construction fallback and silently produced `{}`.
    if crate::builtins::boxed_primitive_payload(func_value).is_some() {
        super::super::object_ops::throw_object_type_error(b"is not a constructor");
    }
    // `new (new RegExp())` — a RegExp instance has no [[Construct]] internal
    // method (Test262 `S15.10.7_A2_T2`). Without this it fell through to the
    // empty-object construction fallback and silently produced `{}` instead
    // of throwing.
    {
        let jv = crate::value::JSValue::from_bits(func_value.to_bits());
        if jv.is_pointer() && crate::regex::is_registered_regex(jv.as_pointer::<u8>() as usize) {
            super::super::object_ops::throw_object_type_error(b"is not a constructor");
        }
    }
    // #3656: `new p()` where `p` is a Proxy dispatches through its `construct`
    // trap (or forwards to the target). Reached when the compiler can't prove
    // the callee is a proxy statically (e.g. `new record.proxy()`). newTarget
    // for a plain `new` is the constructor being invoked — the proxy itself.
    if crate::proxy::js_proxy_is_proxy(func_value) == 1 {
        let arr = crate::array::js_array_alloc(0);
        let mut a = arr;
        if !args_ptr.is_null() {
            for i in 0..args_len {
                a = crate::array::js_array_push_f64(a, *args_ptr.add(i));
            }
        }
        let arr_box = f64::from_bits(0x7FFD_0000_0000_0000 | (a as u64 & 0x0000_FFFF_FFFF_FFFF));
        return crate::proxy::js_proxy_construct(func_value, arr_box, func_value);
    }
    if is_non_constructable_builtin_function_value(func_value) {
        throw_non_constructable_builtin_function();
    }
    // `new Function.prototype` — %Function.prototype% is callable but NOT a
    // constructor (ECMA-262 20.2.3: "does not have a [[Construct]] internal
    // method").
    if super::super::global_this::is_function_prototype_object_value(func_value) {
        super::super::object_ops::throw_object_type_error(b"is not a constructor");
    }
    if let Some((module, method)) = bound_native_callable_module_and_method(func_value) {
        // Native constructors and ordinary exports share the bound-method
        // trampoline. Consult the export metadata before falling through to
        // generic closure construction; some lower-case JavaScript wrappers
        // are constructors while native functions such as path methods are not.
        if !super::super::native_module::is_native_module_constructor_export(&module, &method) {
            super::super::object_ops::throw_object_type_error(b"is not a constructor");
        }
        if module == "perf_hooks" {
            if let Some(result) =
                crate::perf_hooks::construct_perf_hooks_class(&method, args_ptr, args_len)
            {
                return result;
            }
        }
        if module == "sqlite"
            && matches!(
                method.as_str(),
                "DatabaseSync" | "Session" | "StatementSync"
            )
        {
            let ptr =
                crate::value::JS_NATIVE_SQLITE_DISPATCH.load(std::sync::atomic::Ordering::SeqCst);
            if !ptr.is_null() {
                let dispatch: crate::value::JsNativeSqliteDispatchFn = std::mem::transmute(ptr);
                return dispatch(method.as_ptr(), method.len(), args_ptr, args_len, 1);
            }
        }
        // Devirt phase 2: node-module-namespaced constructors (tty/fs/vm/tls/
        // wasi/readline/repl/stream) dispatch through the per-module ctor
        // registry, populated by `js_nm_install_<module>()` at import. Each
        // unimported module's constructors are referenced only via that install
        // symbol, so they dead-strip. `None` falls through to the dynamic-
        // dispatch ctors below (http/events/zlib) and the global-name match.
        if let Some(ctor) = crate::object::nm_ctor_lookup(&module) {
            if let Some(result) = ctor(&module, &method, args_ptr, args_len) {
                return result;
            }
        }
        // #4904: `new http.Agent(opts)` / `new http.ClientRequest(opts)` /
        // `new http.IncomingMessage(socket)` / `new http.ServerResponse(req)`
        // (and `new https.Agent(opts)`) through any value-aliasing path —
        // `const { Agent } = require('http')`, `const CR =
        // http.ClientRequest`, etc. The bound export value carries
        // (module, method); forward construction to the stdlib http
        // dispatcher exactly like `OutgoingMessage` below.
        if (module == "http"
            && matches!(
                method.as_str(),
                "OutgoingMessage"
                    | "Agent"
                    | "ClientRequest"
                    | "IncomingMessage"
                    | "ServerResponse"
            ))
            || (module == "https" && method == "Agent")
        {
            let ptr =
                crate::value::JS_NATIVE_HTTP_DISPATCH.load(std::sync::atomic::Ordering::SeqCst);
            if !ptr.is_null() {
                let dispatch: unsafe extern "C" fn(
                    *const u8,
                    usize,
                    *const u8,
                    usize,
                    *const f64,
                    usize,
                ) -> f64 = std::mem::transmute(ptr);
                return dispatch(
                    module.as_ptr(),
                    module.len(),
                    method.as_ptr(),
                    method.len(),
                    args_ptr,
                    args_len,
                );
            }
        }
        // #4995: `new EE()` where `EE = require('events')` or came in as a
        // default / namespace import (`import EE from 'events'`, `import * as
        // ev from 'events'; new ev.EventEmitter()`). The callee is the bound
        // `events.EventEmitter` export value; without this arm construction
        // fell through to the generic empty-object path, so the instance had
        // no `.on`/`.emit`/`.setMaxListeners` (signal-exit's init throws).
        // Route to the linked emitter impl (perry-stdlib `bundled-events` or
        // perry-ext-events) via the construct dispatcher registered at
        // startup — this crate can't call the constructors directly.
        if module == "events"
            && matches!(
                method.as_str(),
                "EventEmitter" | "EventEmitterAsyncResource"
            )
        {
            let ptr =
                crate::value::JS_NATIVE_EVENTS_CONSTRUCT.load(std::sync::atomic::Ordering::SeqCst);
            if !ptr.is_null() {
                let dispatch: crate::value::JsNativeEventsConstructFn = std::mem::transmute(ptr);
                return dispatch(method.as_ptr(), method.len(), args_ptr, args_len);
            }
        }
        // `new <bound async_hooks.AsyncLocalStorage>()` / `<...AsyncResource>()`.
        // Next.js stores the native ctor on `globalThis.AsyncLocalStorage` and
        // later does `new maybeGlobalAsyncLocalStorage()` (a dynamic callee), so
        // the static `new AsyncLocalStorage()` codegen arm never fires. Without
        // this the instance was a class_id=0 empty object whose `.getStore` read
        // back `undefined` -> "getStore is not a function" at server startup.
        // Route to the stdlib handle constructor via the registered dispatcher.
        if module == "async_hooks"
            && matches!(method.as_str(), "AsyncLocalStorage" | "AsyncResource")
        {
            let ptr = crate::value::JS_NATIVE_ASYNC_HOOKS_CONSTRUCT
                .load(std::sync::atomic::Ordering::SeqCst);
            if !ptr.is_null() {
                let dispatch: crate::value::JsNativeEventsConstructFn = std::mem::transmute(ptr);
                return dispatch(method.as_ptr(), method.len(), args_ptr, args_len);
            }
        }
        if module == "zlib" && matches!(method.as_str(), "ZstdCompress" | "ZstdDecompress") {
            let ptr =
                crate::value::JS_NATIVE_ZLIB_DISPATCH.load(std::sync::atomic::Ordering::SeqCst);
            if !ptr.is_null() {
                let dispatch: unsafe extern "C" fn(*const u8, usize, *const f64, usize) -> f64 =
                    std::mem::transmute(ptr);
                let factory = if method == "ZstdCompress" {
                    "createZstdCompress"
                } else {
                    "createZstdDecompress"
                };
                return dispatch(factory.as_ptr(), factory.len(), args_ptr, args_len);
            }
        }
    }

    // date-fns `constructFrom` clones a Date via
    // `new date.constructor(value)`. `date.constructor` resolves to
    // the global `Date` closure pointer (the noop thunk installed by
    // `populate_global_this_builtins`). Without this intercept the
    // call falls through to the generic empty-object path and
    // `cloned.getTime()` reads garbage. Detect the global Date /
    // Array / Object constructor pointers and dispatch into the
    // matching real factory. Refs date-fns blocker.
    if let Some(name) = identify_global_builtin_constructor(func_value) {
        let args = if args_ptr.is_null() {
            &[][..]
        } else {
            std::slice::from_raw_parts(args_ptr, args_len)
        };
        match name {
            #[cfg(feature = "global-webcrypto")]
            "Crypto" | "CryptoKey" | "SubtleCrypto" => {
                return crate::object::js_webcrypto_illegal_constructor();
            }
            "Symbol" => {
                return crate::error::js_throw_symbol_constructor_type_error();
            }
            "BigInt" => {
                return crate::error::js_throw_bigint_constructor_type_error();
            }
            "Navigator" => {
                return crate::error::js_throw_illegal_constructor_type_error();
            }
            "Date" => {
                if args.is_empty() {
                    return crate::date::js_date_new();
                }
                if args.len() == 1 {
                    return crate::date::js_date_new_from_value(args[0]);
                }
                let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
                let mut vals = [undefined, undefined, 1.0, 0.0, 0.0, 0.0, 0.0];
                for (i, slot) in vals.iter_mut().enumerate() {
                    if i < args.len() {
                        *slot = args[i];
                    }
                }
                return crate::date::js_date_new_local_components(
                    vals[0], vals[1], vals[2], vals[3], vals[4], vals[5], vals[6],
                );
            }
            "Boolean" => {
                let value = args
                    .first()
                    .copied()
                    .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
                return crate::builtins::js_boxed_boolean_new(value);
            }
            "Number" => {
                let value = args
                    .first()
                    .copied()
                    .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
                return crate::builtins::js_boxed_number_new(value);
            }
            "String" => {
                let value = args
                    .first()
                    .copied()
                    .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
                return crate::builtins::js_boxed_string_new(value, (!args.is_empty()) as i32);
            }
            "Array" => {
                if args.len() == 1 {
                    let arr = crate::array::js_array_constructor_single(args[0]);
                    return crate::value::js_nanbox_pointer(arr as i64);
                }
                // `new Array(a, b, c)`: array filled with the args.
                let len = args.len() as u32;
                let arr = crate::array::js_array_alloc(len);
                (*arr).length = len;
                for (i, &v) in args.iter().enumerate() {
                    crate::array::js_array_set_f64(arr, i as u32, v);
                }
                return crate::value::js_nanbox_pointer(arr as i64);
            }
            "Object" => {
                let value = args
                    .first()
                    .copied()
                    .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
                return crate::object::js_object_coerce(value);
            }
            #[cfg(feature = "global-webfetch")]
            "Blob" => {
                let parts = args
                    .first()
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                let options = args
                    .get(1)
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                return crate::object::global_this_blob_thunk(std::ptr::null(), parts, options);
            }
            #[cfg(feature = "global-webfetch")]
            "File" => {
                let parts = args
                    .first()
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                let name = args
                    .get(1)
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                let options = args
                    .get(2)
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                return crate::object::global_this_file_thunk(
                    std::ptr::null(),
                    parts,
                    name,
                    options,
                );
            }
            // Global builtins reached through a VALUE (alias variable,
            // intrinsic lookup, cross-module re-export) rather than by name.
            //
            // #8223: NOT feature-gated. This arm carried a
            // `#[cfg(feature = "global-webfetch")]` inherited from #7008's
            // web-platform size gating when #7779 moved the arms out — but it
            // dispatches Map/Set/WeakMap/WeakSet/WeakRef/EventTarget/
            // AbortController/TextEncoder/URLSearchParams/DisposableStack,
            // whose factories are all unconditional modules. Auto-optimize
            // builds the runtime with a minimal feature set (a bare test gets
            // `async-runtime` alone), so the arm compiled out and every
            // value-held builtin constructor fell through: `new (Map-as-value)`
            // threw "Constructor Map requires 'new'", an aliased EventTarget
            // had no surface. The prebuilt FULL stdlib (PERRY_SKIP_BUILD fast
            // mode) masked it, which is exactly the fast/full gap-suite mode
            // divergence #8223 documents.
            n if builtin_alias_construct::handles(n) => {
                return builtin_alias_construct::construct(n, args);
            }
            "Headers" => {
                let init = args
                    .first()
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                return crate::object::global_this_headers_thunk(std::ptr::null(), init);
            }
            #[cfg(feature = "global-webfetch")]
            "Request" => {
                let input = args
                    .first()
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                let init = args
                    .get(1)
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                return crate::object::global_this_request_thunk(std::ptr::null(), input, init);
            }
            #[cfg(feature = "global-webfetch")]
            "Response" => {
                let body = args
                    .first()
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                let init = args
                    .get(1)
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                return crate::object::global_this_response_thunk(std::ptr::null(), body, init);
            }
            "Event" => {
                let event_type = args
                    .first()
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                let options = args
                    .get(1)
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                let event =
                    crate::event_target::js_event_new(event_type, options, args.len() as u32);
                return crate::value::js_nanbox_pointer(event as i64);
            }
            "CustomEvent" => {
                let event_type = args
                    .first()
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                let options = args
                    .get(1)
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                let event = crate::event_target::js_custom_event_new(
                    event_type,
                    options,
                    args.len() as u32,
                );
                return crate::value::js_nanbox_pointer(event as i64);
            }
            "DOMException" => {
                let message = args
                    .first()
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                let name = args
                    .get(1)
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                let exception = crate::event_target::js_dom_exception_new(message, name);
                return crate::value::js_nanbox_pointer(exception as i64);
            }
            // #2889: `new (rebound Error subclass)(msg)` through a global
            // constructor value. Mirrors the bare `new TypeError(msg)`
            // lowering so `const E = TypeError; new E("x")` produces a real
            // error instance with the right `.name`.
            "Error" | "TypeError" | "RangeError" | "ReferenceError" | "SyntaxError"
            | "EvalError" | "URIError" => {
                let kind = match name {
                    "TypeError" => crate::error::ERROR_KIND_TYPE_ERROR,
                    "RangeError" => crate::error::ERROR_KIND_RANGE_ERROR,
                    "ReferenceError" => crate::error::ERROR_KIND_REFERENCE_ERROR,
                    "SyntaxError" => crate::error::ERROR_KIND_SYNTAX_ERROR,
                    "EvalError" => crate::error::ERROR_KIND_EVAL_ERROR,
                    "URIError" => crate::error::ERROR_KIND_URI_ERROR,
                    _ => crate::error::ERROR_KIND_ERROR,
                };
                let message = if args.is_empty() {
                    f64::from_bits(crate::value::TAG_UNDEFINED)
                } else {
                    args[0]
                };
                let error = crate::error::js_error_new_kind_from_value(kind, message);
                return crate::value::js_nanbox_pointer(error as i64);
            }
            // #2889: `new (rebound RegExp)(pattern, flags)`.
            #[cfg(feature = "regex-engine")]
            "RegExp" => {
                let flags_value = if args.len() < 2 {
                    f64::from_bits(crate::value::TAG_UNDEFINED)
                } else {
                    args[1]
                };
                let scope = crate::gc::RuntimeHandleScope::new();
                let flags_value_handle = scope.root_nanbox_f64(flags_value);
                let (pattern, flags_value) = flags_value_handle.across_nanbox(|| {
                    if args.is_empty() {
                        std::ptr::null_mut()
                    } else {
                        crate::builtins::js_string_coerce(args[0])
                    }
                });
                let pattern_handle = scope.root_string_ptr(pattern);
                let (flags, pattern) =
                    pattern_handle.across_const::<crate::StringHeader, _>(|| {
                        if flags_value.to_bits() == crate::value::TAG_UNDEFINED {
                            std::ptr::null_mut()
                        } else {
                            crate::builtins::js_string_coerce(flags_value)
                        }
                    });
                let re = crate::regex::js_regexp_new(pattern, flags);
                return crate::value::js_nanbox_pointer(re as i64);
            }
            // #2889: `new (rebound TypedArray)(lengthOrSource)`.
            "Int8Array" | "Uint8Array" | "Uint8ClampedArray" | "Int16Array" | "Uint16Array"
            | "Int32Array" | "Uint32Array" | "Float16Array" | "Float32Array" | "Float64Array"
            | "BigInt64Array" | "BigUint64Array" => {
                let kind = match name {
                    "Int8Array" => crate::typedarray::KIND_INT8,
                    "Uint8Array" => crate::typedarray::KIND_UINT8,
                    "Uint8ClampedArray" => crate::typedarray::KIND_UINT8_CLAMPED,
                    "Int16Array" => crate::typedarray::KIND_INT16,
                    "Uint16Array" => crate::typedarray::KIND_UINT16,
                    "Int32Array" => crate::typedarray::KIND_INT32,
                    "Uint32Array" => crate::typedarray::KIND_UINT32,
                    "Float16Array" => crate::typedarray::KIND_FLOAT16,
                    "Float32Array" => crate::typedarray::KIND_FLOAT32,
                    "Float64Array" => crate::typedarray::KIND_FLOAT64,
                    "BigInt64Array" => crate::typedarray::KIND_BIGINT64,
                    _ => crate::typedarray::KIND_BIGUINT64,
                } as i32;
                let arg0 = if args.is_empty() {
                    f64::from_bits(crate::value::JSValue::number(0.0).bits())
                } else {
                    args[0]
                };
                // `new TA(buffer, byteOffset, length?)` via a *dynamic* constructor
                // value (e.g. test262's `testWithTypedArrayConstructors`, where
                // `TA` is a variable) must honor the offset/length arguments. The
                // single-arg `js_typed_array_new` path dropped them, so every
                // view built this way reported `byteOffset === 0`. Route the
                // multi-arg form through the view constructor, which records the
                // backing/offset so `.byteOffset` / `.buffer` are correct and the
                // result aliases the buffer (mirrors the literal-name codegen
                // path in `lower_call::builtin`). A non-ArrayBuffer `arg0` falls
                // back to `js_typed_array_new` inside `js_typed_array_view`.
                let ta = if args.len() >= 2 {
                    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
                    crate::typedarray_view::js_typed_array_view(
                        kind,
                        arg0,
                        args[1],
                        args.get(2).copied().unwrap_or(undefined),
                    )
                } else {
                    crate::typedarray::js_typed_array_new(kind, arg0)
                };
                return crate::value::js_nanbox_pointer(ta as i64);
            }
            #[cfg(feature = "global-text")]
            "TextEncoderStream" => {
                return text_encoding_stream_new_with_constructor(
                    func_value,
                    CLASS_ID_TEXT_ENCODER_STREAM,
                );
            }
            #[cfg(feature = "global-text")]
            "TextDecoderStream" => {
                return text_encoding_stream_new_with_constructor(
                    func_value,
                    CLASS_ID_TEXT_DECODER_STREAM,
                );
            }
            "CompressionStream" => {
                let format = args
                    .first()
                    .copied()
                    .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
                validate_web_compression_stream_format(format);
                return text_encoding_stream_new_with_constructor(
                    func_value,
                    CLASS_ID_COMPRESSION_STREAM,
                );
            }
            "DecompressionStream" => {
                let format = args
                    .first()
                    .copied()
                    .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
                validate_web_compression_stream_format(format);
                return text_encoding_stream_new_with_constructor(
                    func_value,
                    CLASS_ID_DECOMPRESSION_STREAM,
                );
            }
            // #4950 (secondary note): react-reconciler captures the global
            // `AbortController` into a local (`AbortControllerLocal = typeof
            // AbortController !== "undefined" ? AbortController : <shim>`) and
            // constructs through the variable. Without this arm the dynamic
            // `new` fell through and threw "AbortController is not a function".
            "AbortController" => {
                let controller = crate::url::js_abort_controller_new();
                return crate::value::js_nanbox_pointer(controller as i64);
            }
            "MessageChannel" => {
                return crate::messaging::js_message_channel_new();
            }
            "MessagePort" => {
                return crate::messaging::js_message_port_constructor_error();
            }
            "Storage" => {
                return crate::web_storage::storage_constructor_illegal(std::ptr::null());
            }
            "BroadcastChannel" => {
                let name = args
                    .first()
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                return crate::messaging::js_broadcast_channel_new(name);
            }
            #[cfg(feature = "global-url")]
            "URL" => {
                let input = args
                    .first()
                    .copied()
                    .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
                let input_ptr = crate::url::js_url_coerce_string(input);
                let url = if let Some(base) = args.get(1).copied() {
                    let base_ptr = crate::url::js_url_coerce_string(base);
                    crate::url::js_url_new_with_base(input_ptr, base_ptr)
                } else {
                    crate::url::js_url_new(input_ptr)
                };
                return crate::value::js_nanbox_pointer(url as i64);
            }
            #[cfg(feature = "global-url")]
            "URLSearchParams" => {
                let params = if let Some(init) = args.first().copied() {
                    crate::url::js_url_search_params_new_any(init)
                } else {
                    crate::url::js_url_search_params_new_empty()
                };
                return crate::value::js_nanbox_pointer(params as i64);
            }
            #[cfg(feature = "global-text")]
            "TextEncoder" => {
                let encoder = crate::text::js_text_encoder_new();
                return crate::value::js_nanbox_pointer(encoder);
            }
            // `new P(executor)` where `P` holds the global Promise constructor
            // VALUE (`const P = Promise;` / a polyfill alias). Without this arm
            // the match fell through to the closure fallback, which CALLED
            // `promise_constructor_call_thunk` as a plain function — throwing
            // "Constructor Promise requires 'new'". Route to the same executor
            // construction as the static literal lowering.
            "Promise" => {
                let executor = args
                    .first()
                    .copied()
                    .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
                let bits = executor.to_bits();
                let exec_ptr = if (bits & crate::value::TAG_MASK) == crate::value::POINTER_TAG {
                    (bits & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader
                } else {
                    // Non-callable executor: `js_promise_new_with_executor`
                    // validates and throws the spec TypeError synchronously.
                    std::ptr::null()
                };
                let promise = crate::promise::js_promise_new_with_executor(exec_ptr);
                return crate::value::js_nanbox_pointer(promise as i64);
            }
            #[cfg(feature = "global-text")]
            "TextDecoder" => {
                let label = args
                    .first()
                    .copied()
                    .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
                let options = args
                    .get(1)
                    .copied()
                    .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
                let fatal = text_decoder_bool_option(options, "fatal");
                let ignore_bom = text_decoder_bool_option(options, "ignoreBOM");
                let decoder = crate::text::js_text_decoder_new(label, fatal, ignore_bom);
                return crate::value::js_nanbox_pointer(decoder);
            }
            // `new $ArrayBuffer(n)` / `new $DataView(buf, off?, len?)` where the
            // constructor was obtained as a VALUE (e.g. the bundle reads
            // `IN(globalThis, "DataView")` into a variable) rather than the
            // syntactic `new DataView(...)` that lower_call/builtin.rs handles.
            // Without these arms the dynamic-construct path falls through to
            // "not a function". Mirror the static lowering exactly.
            "ArrayBuffer" | "SharedArrayBuffer" => {
                let size = args.first().copied().unwrap_or(0.0);
                let buf = if name == "SharedArrayBuffer" {
                    crate::buffer::js_shared_array_buffer_new_value(size)
                } else {
                    crate::buffer::js_array_buffer_new_value(size)
                };
                return crate::value::js_nanbox_pointer(buf as i64);
            }
            "DataView" => {
                let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
                let value = args.first().copied().unwrap_or(undef);
                let offset = args.get(1).copied().unwrap_or(undef);
                let length = args.get(2).copied().unwrap_or(undef);
                return crate::buffer::js_data_view_new(value, offset, length);
            }
            _ => {}
        }
    }
    // #1789/#1787: `new (classObjectValue)(args)` — the callee is a heap
    // class object (the value a class EXPRESSION evaluates to, e.g.
    // `const C = mk(x); new C()`). Read its class_id (the compile-time
    // template) and allocate an instance stamped with it, so instance
    // methods dispatch and `x instanceof C` matches.
    //
    // #1787: then REPLAY the class's constructor on the instance. The
    // constructor can't be inlined at the `new` site — the callee is a
    // runtime value, and the class's captured environment lived where the
    // class EXPRESSION was evaluated (e.g. inside the `mk(tag)` factory),
    // not at the (possibly far-away) construction site. So the codegen
    // ClassExprFresh lowering snapshots those captures onto this class
    // object as the `__perry_ctor_caps` own array, and registers the
    // standalone `<prefix>__<class>_constructor` symbol in
    // `CLASS_CONSTRUCTORS`. Replaying it here runs the instance-field
    // initializers (literal AND captured) and the constructor body —
    // matching what the static `new ClassName()` path does inline.
    if is_class_object_value(func_value) {
        // Root the class object: its pointer is both the constructor value and
        // the private-brand identity for the instance.
        let scope = crate::gc::RuntimeHandleScope::new();
        let class_handle = scope.root_nanbox_f64(func_value);
        let obj = crate::value::JSValue::from_bits(class_handle.get_nanbox_f64().to_bits())
            .as_pointer::<ObjectHeader>();
        let class_cid = js_object_get_class_id(obj);
        if class_cid != 0 {
            let inst = js_object_alloc(
                class_cid,
                crate::object::learned_inline_field_count(class_cid),
            );
            // #7280: root the instance across the replay — see the long note
            // in `construct_registered_class_ref`. The replay runs a user
            // constructor body, so a bare `*mut ObjectHeader` held across it
            // is an unrooted receiver and this arm returns the pre-move
            // address. Reproduced by `new C()` where `C = mk()` is a class
            // EXPRESSION value.
            let inst_handle = scope.root_raw_mut_ptr(inst);
            inst_handle.with_mut_ptr::<ObjectHeader, _>(|inst| {
                link_class_object_instance_prototype(class_handle.get_nanbox_f64(), inst)
            });
            // Every evaluation gets a distinct brand despite sharing its
            // class id. Stamp it before replay, where private access may occur.
            inst_handle.with_mut_ptr::<ObjectHeader, _>(|inst| {
                super::super::field_get_set::stamp_private_evaluation_brand(
                    inst,
                    class_handle.get_nanbox_f64(),
                );
            });
            // Replay the class's registered constructor (instance-field
            // initializers + body) on the fresh instance, filling the
            // capture params from the snapshotted `__perry_ctor_caps`. The
            // mechanism lives in `class_constructors` to keep this file under
            // the 2,000-line CI gate.
            inst_handle.with_mut_ptr::<ObjectHeader, _>(|inst| {
                super::super::class_constructors::replay_class_object_constructor(
                    class_handle.get_nanbox_f64(),
                    class_cid,
                    inst,
                    args_ptr,
                    args_len,
                );
            });
            // `class X extends Request/Response {}` constructed via the dynamic
            // (class-expression value) path: the replayed ctor's `super()`
            // can't statically route an aliased parent, so attach the native
            // fetch handle here when the registered parent is a fetch builtin
            // and the instance didn't already get one. Refs `@hono/node-server`.
            if let Some(kind) = fetch_parent_kind_in_chain(class_cid) {
                let has_handle = inst_handle.with_mut_ptr::<ObjectHeader, _>(|inst| {
                    super::super::field_get_set::fetch_subclass_handle_id(inst as usize).is_some()
                });
                if !has_handle {
                    inst_handle.with_mut_ptr::<ObjectHeader, _>(|inst| {
                        super::super::attach_fetch_handle_for_construction(
                            inst, kind, args_ptr, args_len,
                        )
                    });
                }
            }
            // Class-expression values can also extend Promise and reach this
            // dynamic construct path. The synthesized default constructor does
            // not call construct-only builtins as plain functions; attach the
            // Promise backing here, matching the ClassRef path below. An
            // explicit `super(executor)` has already installed it, so avoid
            // invoking the executor twice.
            ensure_promise_subclass_backing(&inst_handle, class_cid, args_ptr, args_len);
            // Re-read: the fetch attachment and Promise executor both allocate.
            return crate::value::js_nanbox_pointer(
                inst_handle.get_raw_mut_ptr::<ObjectHeader>() as i64
            );
        }
    }

    // #321/#4530: `new C(args)` where `C` is a first-class ClassRef, including
    // proxy-forwarded construction. Allocate an instance stamped with the
    // registered class id and replay the standalone constructor so field
    // initializers and `this.foo = ...` writes match static `new ClassName()`.
    if let Some(class_cid) = constructor_class_ref_id(func_value) {
        return construct_registered_class_ref(
            class_cid, class_cid, func_value, args_ptr, args_len,
        );
    }
    if is_arrow_function_value(func_value) {
        crate::fs::validate::throw_type_error_with_code(
            "Arrow function is not a constructor",
            "ERR_INVALID_ARG_TYPE",
        );
    }
    if extends_target_must_throw(func_value) {
        super::super::object_ops::throw_object_type_error(b"is not a constructor");
    }
    let cid = synthetic_class_id_for_function(func_value);
    // Allocate the instance with the synthetic class id (or 0 if the
    // value isn't callable). The object starts with no own props; the
    // constructor body fills `this.<field>` writes through
    // PropertySet, and prototype-method dispatch consults the
    // synthetic class id's entry in CLASS_PROTOTYPE_METHODS.
    // Learned inline sizing: a class that overflowed once pre-sizes every
    // later instance so all its fields land inline (object/mod.rs).
    // #7341: `func_value` is a heap pointer (the closure) and every use of it
    // below happens AFTER this allocation. `js_object_alloc` can drive an
    // evacuating minor, which moves the closure; the decode at `let fp = ...`
    // then names from-space and `is_closure_ptr` reads `CLOSURE_MAGIC` off it
    // -- `ldr w8, [x23, #0xc]` at `js_new_function_construct + 5304`, the fault
    // in `test_gap_learned_inline_sizing` under from-space quarantine.
    //
    // A stale read here is quiet rather than fatal without the quarantine: the
    // magic check simply fails, the user-prototype link is skipped, and the
    // instance silently gets the wrong [[Prototype]].
    let func_scope = crate::gc::RuntimeHandleScope::new();
    let func_handle = func_scope.root_nanbox_f64(func_value);
    let obj_ptr = js_object_alloc(cid, crate::object::learned_inline_field_count(cid));
    // Republish IMMEDIATELY: `fp` below is derived from this value, and the
    // derivation is what must see the post-collection address.
    let func_value = func_handle.get_nanbox_f64();
    let nan_boxed = crate::value::js_nanbox_pointer(obj_ptr as i64);
    // A user-assigned `foo.prototype = <obj/array>` lives as the closure's
    // "prototype" dynamic prop; the instance's [[Prototype]] must be THAT
    // value — notably a real array (`foo.prototype = new Array(1,2,3)`),
    // which `ensure_function_prototype_object` would shadow with a fresh
    // empty object (test262 filter/15.4.4.20-6-*, some/15.4.4.17-8-*).
    let mut linked_user_proto = false;
    {
        let fp = (func_value.to_bits() & crate::value::POINTER_MASK) as usize;
        if fp != 0 && crate::closure::is_closure_ptr(fp) {
            let dyn_proto = crate::closure::closure_get_dynamic_prop(fp, "prototype");
            let dp = JSValue::from_bits(dyn_proto.to_bits());
            if dp.is_pointer() {
                let raw = dp.as_pointer::<u8>() as usize;
                // Function objects (closures) are identified by CLOSURE_MAGIC,
                // not a GC type tag — check them first.
                let is_fn = crate::closure::is_closure_ptr(raw);
                let has_user_proto = is_fn
                    || (raw >= crate::gc::GC_HEADER_SIZE + 0x1000 && {
                        let hdr = unsafe {
                            &*((raw - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader)
                        };
                        // Arrays: test262 filter/15.4.4.20-6-*, some/15.4.4.17-8-*.
                        // Objects: Intl/Temporal constructors install .prototype via
                        // closure_set_dynamic_prop (bypassing js_set_function_prototype),
                        // so CLASS_PROTOTYPE_OBJECTS has no entry; ensure_function_prototype_object
                        // would otherwise create a fresh empty proto and overwrite .prototype.
                        matches!(
                            hdr.obj_type,
                            crate::gc::GC_TYPE_ARRAY
                                | crate::gc::GC_TYPE_LAZY_ARRAY
                                | crate::gc::GC_TYPE_OBJECT
                        )
                    });
                if has_user_proto {
                    super::super::prototype_chain::object_link_class_default_prototype(
                        obj_ptr as usize,
                        dyn_proto.to_bits(),
                    );
                    linked_user_proto = true;
                }
            }
        }
    }
    if !linked_user_proto {
        let proto = ensure_function_prototype_object(func_value, cid);
        if !proto.is_null() {
            super::super::prototype_chain::object_link_class_default_prototype(
                obj_ptr as usize,
                crate::value::js_nanbox_pointer(proto as i64).to_bits(),
            );
        }
    }
    // Only run the constructor body when the callee is recognised as
    // a closure shape. The codegen LocalGet path widens the route to
    // any local-resolved callee, so we have to gate the
    // `js_native_call_value` dispatch on a verified closure pointer
    // here — otherwise `new <non-callable>()` would dereference an
    // arbitrary pointer as a `ClosureHeader` and crash.
    //
    // Reflective `Function.apply(self, scope, code)` (and `Reflect.construct`
    // on Function) reach here with `func_value` = the reified Function
    // constructor — a plain callable closure singleton, so
    // `is_callable_function_value` below reports it callable and it would be
    // CALLED as a value → "Function is not a function". The literal
    // `new Function(...)` path routes to the Function-from-strings shim in
    // codegen (`lower_call/new.rs`); route the reflective form to the SAME shim
    // here. Identify the constructor by its intrinsic closure identity
    // (`identify_global_builtin_constructor`, keyed on the builtin `func_ptr`) —
    // robust to `globalThis.Function` reassignment, unlike reading the mutable
    // global property. (User classes / other builtins / proxies were handled
    // above and don't match.)
    if matches!(
        identify_global_builtin_constructor(func_value),
        Some("Function")
    ) {
        return super::super::js_function_ctor_from_strings(args_ptr, args_len);
    }
    if is_callable_function_value(func_value) {
        // Bind `this` to the new instance, dispatch the constructor,
        // then restore the previous IMPLICIT_THIS. The dispatch
        // result is discarded — JS `new` semantics use the receiver,
        // not the returned value (object returns would override, but
        // dayjs and siblings rely on the receiver mutation pattern).
        // #7280: `nan_boxed` (the implicit `this` this call is building) and
        // the three DISPLACED cell values are held across a call that runs a
        // user constructor body — see the long note in
        // `construct_registered_class_ref`. Unrooted, the evacuating minor
        // moves the instance and this arm returns the pre-move address;
        // reproduced by `new inst.ctor(x)` where `inst.ctor` is a plain
        // function, 200/200 iterations wrong under
        // `PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_GC_SCHEDULE_SEED=1 PERRY_GC_SCHEDULE_RATE=1`.
        let scope = crate::gc::RuntimeHandleScope::new();
        let inst_handle = scope.root_nanbox_f64(nan_boxed);
        let prev_this = crate::object::js_implicit_this_get();
        let prev_this_handle = scope.root_nanbox_f64(prev_this);
        let prev_new_target = crate::object::js_new_target_get();
        let prev_new_target_handle = scope.root_nanbox_f64(prev_new_target);
        crate::object::js_implicit_this_set(nan_boxed);
        crate::object::js_new_target_set(func_value);
        let prev_current_new_target =
            CURRENT_NEW_TARGET.with(|value| value.replace(func_value.to_bits()));
        let prev_current_new_target_handle = scope.root_nanbox_u64(prev_current_new_target);
        let result = crate::closure::js_native_call_value(func_value, args_ptr, args_len);
        CURRENT_NEW_TARGET.with(|value| value.set(prev_current_new_target_handle.get_nanbox_u64()));
        crate::object::js_new_target_set(prev_new_target_handle.get_nanbox_f64());
        crate::object::js_implicit_this_set(prev_this_handle.get_nanbox_f64());
        if constructor_return_overrides_this(result) {
            return result;
        }
        return inst_handle.get_nanbox_f64();
    }
    // Ordinary objects, symbols and every other non-callable heap value do not
    // have [[Construct]]. The historical placeholder return made `new
    // Reflect()`, `new Error.prototype()` and `new sym()` silently succeed.
    super::super::object_ops::throw_object_type_error(b"is not a constructor")
}

/// `new <callee>(...spread)` — spread-bearing construction. Codegen builds a
/// single JS array containing every argument in evaluation order (regular args
/// pushed, spread sources expanded via `js_array_like_to_array` + concat), then
/// hands the array here. We materialise it into a flat `f64` buffer and forward
/// to `js_new_function_construct`, so the full callee-shape dispatch (primitive
/// → TypeError, proxy `construct` trap, boxed-wrapper TypeError, class refs,
/// closures, native module constructors) is shared with the non-spread path.
///
/// `args_array` is a NaN-boxed Array JSValue (POINTER_TAG). A null/0 handle is
/// treated as an empty argument list.
#[no_mangle]
pub unsafe extern "C" fn js_new_function_construct_apply(func_value: f64, args_array: f64) -> f64 {
    let arr_ptr = (args_array.to_bits() & crate::value::POINTER_MASK) as *const crate::ArrayHeader;
    if arr_ptr.is_null() {
        return js_new_function_construct(func_value, std::ptr::null::<f64>(), 0);
    }
    let len = crate::array::js_array_length(arr_ptr) as usize;
    let mut buf: Vec<f64> = Vec::with_capacity(len);
    for i in 0..len {
        let v = crate::array::js_array_get(arr_ptr, i as u32);
        buf.push(f64::from_bits(v.bits()));
    }
    let (ptr, n) = if buf.is_empty() {
        (std::ptr::null::<f64>(), 0usize)
    } else {
        (buf.as_ptr(), buf.len())
    };
    js_new_function_construct(func_value, ptr, n)
}

fn constructor_class_ref_id(value: f64) -> Option<u32> {
    if super::super::class_prototype_ref_id(value).is_some() {
        return None;
    }
    super::super::class_ref_id(value)
}

/// Spec `IsConstructor(value)` — used by `NewPromiseCapability` (the Promise
/// combinators) to validate the `this` constructor argument. Returns true for
/// registered class constructors, the reified builtin constructors, and plain
/// (non-arrow, non-builtin-method) function closures; false for primitives,
/// arrow functions, and non-constructable builtin functions (e.g. `eval`).
pub(crate) fn js_value_is_constructor(value: f64) -> bool {
    // A bound function is a constructor iff its ultimate target is (10.4.1.3
    // BoundFunctionExoticObjects don't have their own [[Construct]] slot
    // independent of the target's constructibility) — resolve through any
    // number of `.bind()` layers first so the checks below (class ref,
    // proxy, arrow, non-constructable builtin) see the real callee.
    let value = resolve_bound_target(value);
    if constructor_class_ref_id(value).is_some() {
        return true;
    }
    if crate::proxy::js_proxy_is_proxy(value) == 1 {
        return true;
    }
    if !is_callable_function_value(value) {
        return false;
    }
    if is_arrow_function_value(value) {
        return false;
    }
    // %Function.prototype% itself is callable but has no [[Construct]] slot
    // (ECMA-262 20.2.3) — `is_non_constructable_builtin_function_value` only
    // covers the separate "reified builtin closure" registry (`eval`, a
    // reified `.apply`/`.call`/`.bind` value, …), not this singleton, so it
    // needs its own check here too (mirrors `js_new_function_construct`'s
    // dedicated `is_function_prototype_object_value` guard). Without this, a
    // bound wrapper around it — `Function.prototype.bind()` — would resolve
    // through `resolve_bound_target` to Function.prototype and read back as
    // constructible.
    if super::super::global_this::is_function_prototype_object_value(value) {
        return false;
    }
    if is_non_constructable_builtin_function_value(value) {
        return false;
    }
    let ptr = JSValue::from_bits(value.to_bits()).as_pointer::<crate::closure::ClosureHeader>();
    if crate::closure::closure_is_bound_method(ptr) {
        return is_bound_native_constructor_closure_value(value);
    }
    true
}

/// Spec ClassDefinitionEvaluation: a non-`null` superclass that is not a
/// constructor makes `class X extends <value>` throw a TypeError before any
/// `.prototype` access. Returns true when `value` is a *definitively* invalid
/// superclass (so the caller throws). `null` is a valid superclass (creates a
/// null-`[[Prototype]]` class) and never throws. Ambiguous heap values (not
/// recognized as callable) return false so legitimate dynamic-extends shapes
/// (mixins, factory-returned classes) keep their parentless baseline rather
/// than mis-throwing. (Test262 subclass/superclass-* and definition/invalid-extends.)
pub(crate) fn extends_target_must_throw(value: f64) -> bool {
    use crate::value::JSValue;
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_null() {
        return false;
    }
    // Registered class refs / heap class objects are constructors.
    if constructor_class_ref_id(value).is_some() || is_class_object_value(value) {
        return false;
    }
    // A Proxy is a constructor iff its `[[ProxyTarget]]` is — recurse.
    if crate::proxy::js_proxy_is_proxy(value) == 1 {
        return extends_target_must_throw(crate::proxy::js_proxy_target(value));
    }
    // Non-object primitives (number, string, boolean, undefined, symbol, bigint)
    // can never be a superclass.
    if !jv.is_pointer() {
        return true;
    }
    if is_callable_function_value(value) {
        if is_arrow_function_value(value) || is_non_constructable_builtin_function_value(value) {
            return true;
        }
        // Native-module constructor exports use the same BOUND_METHOD
        // trampoline as ordinary method reads. Their module/method captures
        // are the distinguishing [[Construct]] metadata: rejecting the raw
        // trampoline here breaks dynamic aliases such as
        // `const Console = console.Console; new Console(...)` and native base
        // construction reached through an indirect user-class chain.
        if is_bound_native_constructor_closure_value(value) {
            return false;
        }
        let ptr = jv.as_pointer::<crate::closure::ClosureHeader>();
        if !ptr.is_null() && is_valid_obj_ptr(ptr as *const u8) {
            // A bound *method* (class/instance method read as a value) is never
            // a constructor.
            if crate::closure::closure_is_bound_method(ptr) {
                return true;
            }
            let fp = crate::closure::get_valid_func_ptr(ptr);
            // A bound *function* (`fn.bind(...)`) is a constructor iff its bound
            // target is — recurse on the captured target.
            if fp == crate::closure::BOUND_FUNCTION_FUNC_PTR {
                let target = crate::closure::js_closure_get_capture_f64(ptr, 0);
                return extends_target_must_throw(target);
            }
            // Arrow / async / generator / async-generator function bodies are
            // non-constructors.
            if crate::closure::is_registered_arrow_function(fp)
                || crate::closure::is_registered_async_function(fp)
                || crate::closure::is_registered_generator_function(fp)
                || crate::closure::is_registered_async_generator_function(fp)
            {
                return true;
            }
        }
        // Ordinary function — a constructor.
        return false;
    }
    // A pointer we don't recognize as callable: stay conservative (no throw).
    false
}

/// Whether `value` is itself a `Function.prototype.bind` result (not
/// recursive — a single-layer tag check).
///
/// `get_valid_func_ptr` re-validates the address range and `CLOSURE_MAGIC`
/// tag itself before touching `func_ptr`, so it's safe to call on ANY
/// pointer-shaped `JSValue` — including a small-handle-band id (Fetch/ws/
/// proxy registry ids) or an otherwise-invalid heap address — without a
/// separate `is_closure_ptr` pre-check. A prior version of this helper
/// dereferenced `(*ptr).type_tag` directly first, which is exactly the
/// unguarded-pointer-shaped-value read this codebase has been bitten by
/// before (#4740-class SIGSEGV on a mis-boxed/handle-band value).
fn bound_function_target_ptr(value: f64) -> Option<*mut crate::closure::ClosureHeader> {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let ptr = jv.as_pointer::<crate::closure::ClosureHeader>();
    if crate::closure::get_valid_func_ptr(ptr) == crate::closure::BOUND_FUNCTION_FUNC_PTR {
        Some(ptr as *mut crate::closure::ClosureHeader)
    } else {
        None
    }
}

pub(crate) fn is_bound_function_closure_value(value: f64) -> bool {
    bound_function_target_ptr(value).is_some()
}

/// Walk through any number of `Function.prototype.bind` wrapper layers to the
/// ultimate non-bound target. Returns `value` unchanged when it isn't a bound
/// closure (including when it isn't a closure at all) — cheap no-op on the
/// overwhelmingly common non-bound path.
fn resolve_bound_target(value: f64) -> f64 {
    let mut cur = value;
    loop {
        let Some(ptr) = bound_function_target_ptr(cur) else {
            return cur;
        };
        cur = crate::closure::js_closure_get_capture_f64(ptr, 0);
    }
}

/// Unwind a `Function.prototype.bind` construct chain of any depth per
/// `BoundFunctionExoticObjects.[[Construct]]` (10.4.1.2): each layer
/// prepends its own bound args to the call-time args before delegating to
/// its target, and resets `newTarget` to the unwrapped target whenever it
/// `SameValue`s the layer being unwound (so a plain `new boundFn()` — where
/// `newTarget` starts out equal to `boundFn` itself — cascades all the way
/// down to the ultimate target, while a `Reflect.construct(boundFn, args,
/// OtherCtor)` leaves `OtherCtor` untouched).
///
/// Returns `None` (no-op, no allocation) when `func_value` isn't a bound
/// closure. Otherwise returns `(target, combined_args, resolved_new_target)`
/// — the caller should re-dispatch construction on `target` with these.
unsafe fn unwrap_bound_construct_chain(
    func_value: f64,
    args_ptr: *const f64,
    args_len: usize,
    new_target: f64,
) -> Option<(f64, Vec<f64>, f64)> {
    let mut cur = func_value;
    let mut nt = new_target;
    let mut unwrapped = false;
    let mut args: Vec<f64> = if args_ptr.is_null() {
        Vec::new()
    } else {
        std::slice::from_raw_parts(args_ptr, args_len).to_vec()
    };
    loop {
        let Some(ptr) = bound_function_target_ptr(cur) else {
            break;
        };
        unwrapped = true;
        let target = crate::closure::js_closure_get_capture_f64(ptr, 0);
        if nt.to_bits() == cur.to_bits() {
            nt = target;
        }
        let bound_args_ptr =
            crate::closure::js_closure_get_capture_ptr(ptr, 2) as *const crate::array::ArrayHeader;
        if !bound_args_ptr.is_null() {
            let n = crate::array::js_array_length(bound_args_ptr) as usize;
            let mut prefix: Vec<f64> = Vec::with_capacity(n + args.len());
            for i in 0..n {
                prefix.push(crate::array::js_array_get_f64(bound_args_ptr, i as u32));
            }
            prefix.extend(args);
            args = prefix;
        }
        cur = target;
    }
    if unwrapped {
        Some((cur, args, nt))
    } else {
        None
    }
}

fn class_object_class_id(value: f64) -> Option<u32> {
    if !is_class_object_value(value) {
        return None;
    }
    let obj = crate::value::JSValue::from_bits(value.to_bits()).as_pointer::<ObjectHeader>();
    let class_id = js_object_get_class_id(obj);
    if class_id != 0 && is_class_id_registered(class_id) {
        Some(class_id)
    } else {
        None
    }
}

fn new_target_class_id(new_target: f64) -> Option<u32> {
    constructor_class_ref_id(new_target).or_else(|| class_object_class_id(new_target))
}

include!("construct/class_return.rs");
include!("construct/class_object.rs");
include!("construct/promise_subclass.rs");

unsafe fn construct_registered_class_ref(
    target_cid: u32,
    instance_cid: u32,
    new_target: f64,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let inst = if let Some((keys_array, field_count)) = registered_class_keys_array(instance_cid) {
        js_object_alloc_class_inline_keys(instance_cid, 0, field_count, keys_array)
    } else {
        js_object_alloc(
            instance_cid,
            crate::object::learned_inline_field_count(instance_cid),
        )
    };
    // #2768: a registered-class constructor reached through this path — static
    // `new ClassName()`, a first-class ClassRef `new`, or `Reflect.construct`
    // with a distinct newTarget — must observe `new.target` inside its body.
    // The function-construct paths set the NEW_TARGET cell (read by codegen's
    // `js_new_target_get`) around the call; this path replayed the constructor
    // without it, so `new.target` was `undefined` for a base class and the
    // explicit `Reflect.construct` newTarget never reached the body. Mirror the
    // other paths: set the cell to the constructor (or the Reflect newTarget)
    // around the replay, then restore.
    //
    // ponytail: the cell is process-global, so a non-constructor function called
    // synchronously from the ctor body reads it too and sees the newTarget
    // instead of `undefined`. This matches the pre-existing plain-function
    // construct paths (which already set the cell the same way) — the codegen
    // `new_target_stack` slot avoids this for fully-inlined `new`, but the
    // replayed ctor is a separate compiled function that can only read the cell.
    // Fix holistically with the slot mechanism if it ever bites.
    // #7280: `inst` — and the two DISPLACED cell values the save/restore idiom
    // parks beside it — must survive the constructor replay as ROOTS, not as
    // Rust locals. `replay_registered_class_constructor` runs a user
    // constructor body: arbitrary allocation, loop back-edge polls, and (under
    // the evacuating minor) a relocation of the very instance being built. A
    // runtime frame is not covered by the precise scan and the conservative
    // stack scan resolves to `SkipDisabled` in shipped builds, so a bare
    // `*mut ObjectHeader` held across that call is the classic unrooted
    // receiver: the collector never sees it, never rewrites it, and this
    // function returns the PRE-MOVE address. Every field the constructor wrote
    // then reads back as garbage through the stale handle — measured on
    // `new inst.ctor(x)` under
    // `PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_GC_SCHEDULE_SEED=1 PERRY_GC_SCHEDULE_RATE=1`,
    // 200/200 iterations wrong,
    // and as a `signal 10` on retired from-space under
    // `PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=800`.
    //
    // This is the `RuntimeHandleScope` routing the `CURRENT_NEW_TARGET`
    // doc-comment at the top of this file called for. Reading back through the
    // handles is not bookkeeping: an evacuating cycle rewrites the slot in
    // place, so the pre-call value is stale by construction.
    let scope = crate::gc::RuntimeHandleScope::new();
    let inst_handle = scope.root_raw_mut_ptr(inst);
    let prev_new_target = crate::object::js_new_target_get();
    let prev_new_target_handle = scope.root_nanbox_f64(prev_new_target);
    crate::object::js_new_target_set(new_target);
    let prev_current_new_target =
        CURRENT_NEW_TARGET.with(|value| value.replace(new_target.to_bits()));
    let prev_current_new_target_handle = scope.root_nanbox_u64(prev_current_new_target);
    super::super::class_constructors::replay_registered_class_constructor(
        target_cid, inst, args_ptr, args_len,
    );
    let inst: *mut ObjectHeader = inst_handle.get_raw_mut_ptr();
    CURRENT_NEW_TARGET.with(|value| value.set(prev_current_new_target_handle.get_nanbox_u64()));
    crate::object::js_new_target_set(prev_new_target_handle.get_nanbox_f64());
    // ClassRef `new` of a Request/Response subclass — attach the native fetch
    // handle on the dynamic path (mirrors the class-expression arm above).
    if let Some(kind) = fetch_parent_kind_in_chain(target_cid) {
        if super::super::field_get_set::fetch_subclass_handle_id(inst as usize).is_none() {
            super::super::attach_fetch_handle_for_construction(inst, kind, args_ptr, args_len);
        }
    }
    // `NewPromiseCapability(Subclass)` reaches this dynamic ClassRef path.
    ensure_promise_subclass_backing(&inst_handle, target_cid, args_ptr, args_len);
    // Re-read once more: the executor `js_promise_subclass_init` runs is user
    // code, so the last two blocks are both collection points.
    crate::value::js_nanbox_pointer(inst_handle.get_raw_mut_ptr::<ObjectHeader>() as i64)
}

/// `GetPrototypeFromConstructor(newTarget)` restricted to the "use it only when
/// it is an object" rule: returns `newTarget.prototype`'s bits when that value
/// is an object (so a typed-array view should adopt it as its `[[Prototype]]`),
/// or `None` when it is a primitive (so the default per-kind prototype applies).
fn new_target_custom_object_prototype(new_target: f64) -> Option<u64> {
    if let Some(class_id) = constructor_class_ref_id(new_target) {
        let declared = super::class_decl_prototype_value(class_id);
        if unsafe { super::super::value_is_object_like(declared) } {
            return Some(declared.to_bits());
        }
        return Some(super::super::class_prototype_ref_value(class_id).to_bits());
    }
    let bits = new_target.to_bits();
    if (bits >> 48) != 0x7FFD {
        return None;
    }
    let raw = (bits & crate::value::POINTER_MASK) as usize;
    if raw == 0 {
        return None;
    }
    let key = crate::string::js_string_from_bytes(b"prototype".as_ptr(), b"prototype".len() as u32);
    let proto = js_object_get_field_by_name_f64(raw as *const ObjectHeader, key);
    if unsafe { super::super::value_is_object_like(proto) }
        || super::super::class_ref_id(proto).is_some()
    {
        Some(proto.to_bits())
    } else {
        None
    }
}

fn constructor_prototype_bits(new_target: f64) -> Option<u64> {
    let bits = new_target.to_bits();
    if (bits >> 48) != 0x7FFD {
        return global_object_prototype_bits();
    }
    let raw = (bits & crate::value::POINTER_MASK) as usize;
    if raw == 0 {
        return global_object_prototype_bits();
    }
    let key = crate::string::js_string_from_bytes(b"prototype".as_ptr(), b"prototype".len() as u32);
    let proto = js_object_get_field_by_name_f64(raw as *const ObjectHeader, key);
    if unsafe { super::super::value_is_object_like(proto) }
        || super::super::class_ref_id(proto).is_some()
    {
        Some(proto.to_bits())
    } else {
        global_object_prototype_bits()
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_new_function_construct_with_new_target(
    func_value: f64,
    args_ptr: *const f64,
    args_len: usize,
    new_target: f64,
) -> f64 {
    let nt = if new_target.to_bits() == crate::value::TAG_UNDEFINED {
        func_value
    } else {
        new_target
    };
    // Unwind any `Function.prototype.bind` wrapper chain BEFORE the
    // `nt == func_value` shortcut below — a plain `new boundFn()` arrives
    // here with `nt == func_value == boundFn`, and re-dispatching that
    // through `js_new_function_construct(boundFn, ...)` would just loop back
    // into this same bound closure. Unwrapping first replaces `func_value`
    // with the real (non-bound) target and resolves `nt` per
    // `BoundFunctionExoticObjects.[[Construct]]`, so the shortcut below (and
    // everything after it) sees the real target either way.
    if let Some((target, combined_args, resolved_nt)) =
        unwrap_bound_construct_chain(func_value, args_ptr, args_len, nt)
    {
        let (ptr, len) = if combined_args.is_empty() {
            (std::ptr::null::<f64>(), 0usize)
        } else {
            (combined_args.as_ptr(), combined_args.len())
        };
        return js_new_function_construct_with_new_target(target, ptr, len, resolved_nt);
    }
    if nt.to_bits() == func_value.to_bits() {
        return js_new_function_construct(func_value, args_ptr, args_len);
    }
    if crate::proxy::js_proxy_is_proxy(func_value) == 1 {
        let arr = crate::array::js_array_alloc(0);
        let mut a = arr;
        if !args_ptr.is_null() {
            for i in 0..args_len {
                a = crate::array::js_array_push_f64(a, *args_ptr.add(i));
            }
        }
        let arr_box = f64::from_bits(0x7FFD_0000_0000_0000 | (a as u64 & 0x0000_FFFF_FFFF_FFFF));
        return crate::proxy::js_proxy_construct(func_value, arr_box, nt);
    }
    if let Some(target_cid) = constructor_class_ref_id(func_value) {
        let instance_cid = new_target_class_id(nt).unwrap_or(target_cid);
        return construct_registered_class_ref(target_cid, instance_cid, nt, args_ptr, args_len);
    }
    // `Reflect.construct(Int8Array, [len], newTarget)` — a typed-array
    // constructor invoked with a distinct newTarget. Build the typed array the
    // normal way, then honor `GetPrototypeFromConstructor(newTarget)`: when
    // `newTarget.prototype` is an object other than the default per-kind
    // prototype, record it as the instance's `[[Prototype]]` so
    // `Object.getPrototypeOf` and `.constructor` resolve through it (test262
    // `ctors*/use-custom-proto-if-object` / `use-default-proto-if-…`).
    if let Some(ta_name) = identify_global_builtin_constructor(func_value) {
        // Symbol and BigInt are callable conversion functions but have no
        // [[Construct]] slot. A distinct newTarget (the subclass case) must
        // not turn either into a generic constructable closure.
        if matches!(ta_name, "Symbol" | "BigInt") {
            return if ta_name == "Symbol" {
                crate::error::js_throw_symbol_constructor_type_error()
            } else {
                crate::error::js_throw_bigint_constructor_type_error()
            };
        }
        // `Reflect.construct(Date, args, newTarget)` (#5989) — Next.js 16's
        // cacheComponents Date extension constructs through exactly this
        // shape: its installed wrapper runs
        // `Reflect.construct(OriginalDate, arguments, new.target)`. The
        // generic tail below allocates a PLAIN object and invokes the Date
        // thunk against it, yielding an unbranded date (`getTime()` broken /
        // "Invalid Date"). Build the real branded Date, then honor
        // `GetPrototypeFromConstructor(newTarget)` like the typed-array arm
        // below so `instanceof newTarget` and subclass prototypes hold.
        if ta_name == "Date" {
            let scope = crate::gc::RuntimeHandleScope::new();
            let nt = scope.root_nanbox_f64(nt);
            let func = scope.root_nanbox_f64(func_value);
            let proto = new_target_custom_object_prototype(nt.get_nanbox_f64())
                .map(|bits| scope.root_heap_word_u64(bits));
            let result = js_new_function_construct(func.get_nanbox_f64(), args_ptr, args_len);
            if let Some(proto) = proto {
                let jv = crate::value::JSValue::from_bits(result.to_bits());
                if jv.is_pointer() {
                    let addr = (jv.bits() & crate::value::POINTER_MASK) as usize;
                    super::super::prototype_chain::object_set_static_prototype(
                        addr,
                        proto.get_heap_word_u64(),
                    );
                }
            }
            return result;
        }
        if matches!(
            ta_name,
            "Int8Array"
                | "Uint8Array"
                | "Uint8ClampedArray"
                | "Int16Array"
                | "Uint16Array"
                | "Int32Array"
                | "Uint32Array"
                | "Float16Array"
                | "Float32Array"
                | "Float64Array"
                | "BigInt64Array"
                | "BigUint64Array"
        ) {
            // Validate and initialize the typed-array contents before reading
            // a custom newTarget prototype. In particular, invalid Symbol
            // element conversion must throw TypeError without observing a
            // poisoned `newTarget.prototype` getter.
            let scope = crate::gc::RuntimeHandleScope::new();
            let nt_h = scope.root_nanbox_f64(nt);
            let result = js_new_function_construct(func_value, args_ptr, args_len);
            let result_h = scope.root_heap_word_u64(result.to_bits());
            let proto_bits = new_target_custom_object_prototype(nt_h.get_nanbox_f64());
            let result = f64::from_bits(result_h.get_heap_word_u64());
            if let Some(addr) = crate::typedarray_props::typed_array_addr_from_value(result) {
                if let Some(proto_bits) = proto_bits {
                    super::super::prototype_chain::object_set_static_prototype(addr, proto_bits);
                }
            }
            return result;
        }
        if matches!(
            ta_name,
            "ArrayBuffer"
                | "SharedArrayBuffer"
                | "DataView"
                | "Boolean"
                | "Number"
                | "String"
                | "RegExp"
                | "Function"
        ) {
            let scope = crate::gc::RuntimeHandleScope::new();
            let nt = scope.root_nanbox_f64(nt);
            let func = scope.root_nanbox_f64(func_value);
            let proto = new_target_custom_object_prototype(nt.get_nanbox_f64())
                .map(|bits| scope.root_heap_word_u64(bits));
            let result = js_new_function_construct(func.get_nanbox_f64(), args_ptr, args_len);
            if let Some(proto) = proto {
                let bits = result.to_bits();
                let addr = if (bits >> 48) == 0x7FFD {
                    (bits & crate::value::POINTER_MASK) as usize
                } else if (bits >> 48) == 0 && crate::buffer::is_registered_buffer(bits as usize) {
                    // ArrayBuffer and SharedArrayBuffer are represented by a
                    // raw BufferHeader pointer rather than a NaN-boxed object.
                    bits as usize
                } else {
                    0
                };
                if addr != 0 {
                    super::super::prototype_chain::object_set_static_prototype(
                        addr,
                        proto.get_heap_word_u64(),
                    );
                }
            }
            return result;
        }
    }
    if !is_callable_function_value(func_value) {
        return js_new_function_construct(func_value, args_ptr, args_len);
    }
    // %Function.prototype% has no [[Construct]] slot (ECMA-262 20.2.3) but
    // isn't covered by `is_non_constructable_builtin_function_value` (a
    // separate "reified builtin closure" registry) — reachable here directly
    // via `Reflect.construct(Function.prototype, …, distinctNewTarget)`, or
    // after `unwrap_bound_construct_chain` resolves a bound wrapper down to
    // it with a non-cascading newTarget.
    if super::super::global_this::is_function_prototype_object_value(func_value)
        || super::super::global_this::is_function_prototype_object_value(nt)
    {
        throw_non_constructable_builtin_function();
    }
    if is_non_constructable_builtin_function_value(func_value)
        || is_non_constructable_builtin_function_value(nt)
    {
        throw_non_constructable_builtin_function();
    }
    if is_arrow_function_value(func_value) {
        crate::fs::validate::throw_type_error_with_code(
            "Arrow function is not a constructor",
            "ERR_INVALID_ARG_TYPE",
        );
    }

    // Stamp the instance with the class id of `newTarget` (not the invoked
    // `target`). Per `OrdinaryCreateFromConstructor`, the instance's
    // `[[Prototype]]` is `newTarget.prototype`, so `obj instanceof newTarget`
    // must be true and `obj instanceof target` false. Perry models the
    // prototype chain via class ids, so allocating with `0` left
    // `Reflect.construct(Target, …, NewTarget)` instances matching neither.
    // A `newTarget` may be a *declared class* (an `Expr::ClassRef`, e.g.
    // `Reflect.construct(plainFn, [], class C {})`) — resolve its registered
    // class id first so `instanceof C` holds — or a *plain function*, for which
    // the synthetic per-function id applies. (The real `[[Prototype]]` link is
    // still set below from `newTarget.prototype`.)
    let cid = new_target_class_id(nt).unwrap_or_else(|| synthetic_class_id_for_function(nt));
    // Learned inline sizing: a class that overflowed once pre-sizes every
    // later instance so all its fields land inline (object/mod.rs).
    // #7341: same shape as the sibling below -- `nt` is a heap pointer and
    // `constructor_prototype_bits(nt)` on the next line runs AFTER this
    // allocation, which can evacuate it. Not independently reproduced (the
    // measured fault is in the plain-`new` entry point), fixed because it is
    // the identical defect one call away.
    let nt_scope = crate::gc::RuntimeHandleScope::new();
    let nt_handle = nt_scope.root_nanbox_f64(nt);
    let obj_ptr = js_object_alloc(cid, crate::object::learned_inline_field_count(cid));
    // Republish IMMEDIATELY -- `constructor_prototype_bits(nt)` on the next
    // line is the use that must see the post-collection address.
    let nt = nt_handle.get_nanbox_f64();
    let nan_boxed = crate::value::js_nanbox_pointer(obj_ptr as i64);
    if let Some(proto_bits) = constructor_prototype_bits(nt) {
        super::super::prototype_chain::object_set_static_prototype(obj_ptr as usize, proto_bits);
    }

    // #7280: same unrooted-receiver shape as the plain-`new` tail above —
    // `nan_boxed` and the three displaced cell values cross a user
    // constructor body. Reproduced by
    // `Reflect.construct(plainFn, [x], otherFn)`, 200/200 iterations wrong
    // under `PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_GC_SCHEDULE_SEED=1 PERRY_GC_SCHEDULE_RATE=1`.
    let scope = crate::gc::RuntimeHandleScope::new();
    let inst_handle = scope.root_nanbox_f64(nan_boxed);
    let prev_this = crate::object::js_implicit_this_get();
    let prev_this_handle = scope.root_nanbox_f64(prev_this);
    let prev_new_target = crate::object::js_new_target_get();
    let prev_new_target_handle = scope.root_nanbox_f64(prev_new_target);
    crate::object::js_implicit_this_set(nan_boxed);
    crate::object::js_new_target_set(nt);
    let prev_current_new_target = CURRENT_NEW_TARGET.with(|value| value.replace(nt.to_bits()));
    let prev_current_new_target_handle = scope.root_nanbox_u64(prev_current_new_target);
    let result = crate::closure::js_native_call_value(func_value, args_ptr, args_len);
    CURRENT_NEW_TARGET.with(|value| value.set(prev_current_new_target_handle.get_nanbox_u64()));
    crate::object::js_new_target_set(prev_new_target_handle.get_nanbox_f64());
    crate::object::js_implicit_this_set(prev_this_handle.get_nanbox_f64());
    if constructor_return_overrides_this(result) {
        return result;
    }
    inst_handle.get_nanbox_f64()
}

/// Verify that a JSValue is a NaN-boxed pointer to a registered
/// closure header. `js_native_call_value` itself doesn't validate the
/// pointer shape — it dereferences whatever lower-48 bits it gets — so
/// the `new <LocalGet>(args)` widened path here in
/// `js_new_function_construct` needs to gate the constructor dispatch
/// on a real closure to avoid SIGSEGV'ing on non-callable callees
/// (`new someObject()`, `new someStringVar()`, etc.). Uses the
/// `_reserved` magic word `crate::closure::CLOSURE_MAGIC` that every
/// `js_closure_alloc*` site stamps on allocation.
pub(crate) fn is_callable_function_value(value: f64) -> bool {
    use crate::value::JSValue;
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    let ptr = jv.as_pointer() as *const crate::closure::ClosureHeader;
    if ptr.is_null() {
        return false;
    }
    if !(ptr as usize).is_multiple_of(std::mem::align_of::<crate::closure::ClosureHeader>()) {
        return false;
    }
    if !is_valid_obj_ptr(ptr as *const u8) {
        return false;
    }
    unsafe { (*ptr).type_tag == crate::closure::CLOSURE_MAGIC }
}

pub(super) fn is_arrow_function_value(value: f64) -> bool {
    use crate::value::JSValue;
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    let ptr = jv.as_pointer() as *const crate::closure::ClosureHeader;
    if !(ptr as usize).is_multiple_of(std::mem::align_of::<crate::closure::ClosureHeader>()) {
        return false;
    }
    if ptr.is_null() || !is_valid_obj_ptr(ptr as *const u8) {
        return false;
    }
    unsafe {
        if (*ptr).type_tag != crate::closure::CLOSURE_MAGIC {
            return false;
        }
    }
    crate::closure::closure_is_arrow(ptr)
}

/// Lookup helper: returns the registered prototype-method value for
/// `(class_id, name)`, or None if no assignment matched. Walks the
/// parent-class chain so methods registered on a base class are found
/// via subclass instances.
pub(crate) fn lookup_prototype_method(class_id: u32, name: &str) -> Option<f64> {
    CLASS_PROTOTYPE_METHODS.with(|table| {
        let guard = table.read().ok()?;
        let map = guard.as_ref()?;
        let mut cid = class_id;
        let mut depth = 0usize;
        while depth < 32 {
            if let Some(per_class) = map.get(&cid) {
                if let Some(&bits) = per_class.get(name) {
                    return Some(f64::from_bits(bits));
                }
            }
            match crate::object::class_generic_origin(cid).or_else(|| get_parent_class_id(cid)) {
                Some(p) if p != 0 && p != cid => {
                    cid = p;
                    depth += 1;
                }
                _ => break,
            }
        }
        None
    })
}
