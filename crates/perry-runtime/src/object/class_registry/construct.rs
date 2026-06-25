use super::*;
use crate::object::*;
use crate::{ArrayHeader, JSValue};
use std::cell::{Cell, RefCell, UnsafeCell};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicU8, Ordering};
use std::sync::RwLock;

thread_local! {
    static CURRENT_NEW_TARGET: std::cell::Cell<u64> =
        const { std::cell::Cell::new(crate::value::TAG_UNDEFINED) };
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
        return Some(crate::node_vm::js_vm_script_new(code, options));
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
    // `new (new String(""))` / `new (new Number(1))` — a boxed primitive WRAPPER
    // object is an ordinary object, never a constructor, so `new` on it throws
    // `TypeError` (Test262 `S15.5.5_A2`). Without this it fell through to the
    // empty-object construction fallback and silently produced `{}`.
    if crate::builtins::boxed_primitive_payload(func_value).is_some() {
        super::super::object_ops::throw_object_type_error(b"is not a constructor");
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
                let mut vals = [f64::from_bits(crate::value::TAG_UNDEFINED); 7];
                for (i, slot) in vals.iter_mut().enumerate() {
                    if i < args.len() {
                        *slot = args[i];
                    }
                }
                return crate::date::js_date_new_local_components(
                    vals[0], vals[1], vals[2], vals[3], vals[4], vals[5], vals[6],
                );
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
            // `new $Map()` / `new $Set()` / `new $WeakMap()` / … where the
            // constructor was obtained as a value (alias variable, intrinsic
            // lookup, cross-module re-export). Mirror the static codegen
            // construction in lower_call/builtin.rs: allocate, NaN-box, then
            // initialize from the optional iterable argument.
            "Map" => {
                let map = crate::map::js_map_alloc(4);
                let boxed = crate::value::js_nanbox_pointer(map as i64);
                if let Some(&iterable) = args.first() {
                    let ij = crate::value::JSValue::from_bits(iterable.to_bits());
                    if !ij.is_undefined() && !ij.is_null() {
                        let from = crate::map::js_map_from_iterable(iterable);
                        return crate::value::js_nanbox_pointer(from as i64);
                    }
                }
                return boxed;
            }
            "Set" => {
                let set = crate::set::js_set_alloc(4);
                let boxed = crate::value::js_nanbox_pointer(set as i64);
                if let Some(&iterable) = args.first() {
                    let ij = crate::value::JSValue::from_bits(iterable.to_bits());
                    if !ij.is_undefined() && !ij.is_null() {
                        let from = crate::set::js_set_from_iterable(iterable);
                        return crate::value::js_nanbox_pointer(from as i64);
                    }
                }
                return boxed;
            }
            "WeakMap" => {
                let map = crate::weakref::js_weakmap_new();
                let boxed = crate::value::js_nanbox_pointer(map as i64);
                if let Some(&iterable) = args.first() {
                    let ij = crate::value::JSValue::from_bits(iterable.to_bits());
                    if !ij.is_undefined() && !ij.is_null() {
                        return crate::weakref::js_weakmap_init_iterable(boxed, iterable);
                    }
                }
                return boxed;
            }
            "WeakSet" => {
                let set = crate::weakref::js_weakset_new();
                let boxed = crate::value::js_nanbox_pointer(set as i64);
                if let Some(&iterable) = args.first() {
                    let ij = crate::value::JSValue::from_bits(iterable.to_bits());
                    if !ij.is_undefined() && !ij.is_null() {
                        return crate::weakref::js_weakset_init_iterable(boxed, iterable);
                    }
                }
                return boxed;
            }
            "WeakRef" => {
                let target = args
                    .first()
                    .copied()
                    .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
                let wr = crate::weakref::js_weakref_new(target);
                return crate::value::js_nanbox_pointer(wr as i64);
            }
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
            "Headers" => {
                let init = args
                    .first()
                    .copied()
                    .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                return crate::object::global_this_headers_thunk(std::ptr::null(), init);
            }
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
                let pattern = if args.is_empty() {
                    std::ptr::null_mut()
                } else {
                    crate::builtins::js_string_coerce(args[0])
                };
                let flags = if args.len() < 2 || args[1].to_bits() == crate::value::TAG_UNDEFINED {
                    std::ptr::null_mut()
                } else {
                    crate::builtins::js_string_coerce(args[1])
                };
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
            "TextEncoderStream" => {
                return text_encoding_stream_new_with_constructor(
                    func_value,
                    CLASS_ID_TEXT_ENCODER_STREAM,
                );
            }
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
            "URLSearchParams" => {
                let params = if let Some(init) = args.first().copied() {
                    crate::url::js_url_search_params_new_any(init)
                } else {
                    crate::url::js_url_search_params_new_empty()
                };
                return crate::value::js_nanbox_pointer(params as i64);
            }
            "TextEncoder" => {
                let encoder = crate::text::js_text_encoder_new();
                return crate::value::js_nanbox_pointer(encoder);
            }
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
        let obj =
            crate::value::JSValue::from_bits(func_value.to_bits()).as_pointer::<ObjectHeader>();
        let class_cid = js_object_get_class_id(obj);
        if class_cid != 0 {
            let inst = js_object_alloc(class_cid, 0);
            // Replay the class's registered constructor (instance-field
            // initializers + body) on the fresh instance, filling the
            // capture params from the snapshotted `__perry_ctor_caps`. The
            // mechanism lives in `class_constructors` to keep this file under
            // the 2,000-line CI gate.
            super::super::class_constructors::replay_class_object_constructor(
                func_value, class_cid, inst, args_ptr, args_len,
            );
            // `class X extends Request/Response {}` constructed via the dynamic
            // (class-expression value) path: the replayed ctor's `super()`
            // can't statically route an aliased parent, so attach the native
            // fetch handle here when the registered parent is a fetch builtin
            // and the instance didn't already get one. Refs `@hono/node-server`.
            if let Some(kind) = fetch_parent_kind_in_chain(class_cid) {
                if super::super::field_get_set::fetch_subclass_handle_id(inst as usize).is_none() {
                    super::super::attach_fetch_handle_for_construction(
                        inst, kind, args_ptr, args_len,
                    );
                }
            }
            return crate::value::js_nanbox_pointer(inst as i64);
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
    let cid = synthetic_class_id_for_function(func_value);
    // Allocate the instance with the synthetic class id (or 0 if the
    // value isn't callable). The object starts with no own props; the
    // constructor body fills `this.<field>` writes through
    // PropertySet, and prototype-method dispatch consults the
    // synthetic class id's entry in CLASS_PROTOTYPE_METHODS.
    let obj_ptr = js_object_alloc(cid, 0);
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
                let is_array = raw >= crate::gc::GC_HEADER_SIZE + 0x1000 && {
                    let hdr = unsafe {
                        &*((raw - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader)
                    };
                    hdr.obj_type == crate::gc::GC_TYPE_ARRAY
                        || hdr.obj_type == crate::gc::GC_TYPE_LAZY_ARRAY
                };
                if is_array {
                    super::super::prototype_chain::object_set_static_prototype(
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
            super::super::prototype_chain::object_set_static_prototype(
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
    if is_callable_function_value(func_value) {
        // Bind `this` to the new instance, dispatch the constructor,
        // then restore the previous IMPLICIT_THIS. The dispatch
        // result is discarded — JS `new` semantics use the receiver,
        // not the returned value (object returns would override, but
        // dayjs and siblings rely on the receiver mutation pattern).
        let prev_this = crate::object::js_implicit_this_get();
        let prev_new_target = crate::object::js_new_target_get();
        crate::object::js_implicit_this_set(nan_boxed);
        crate::object::js_new_target_set(func_value);
        let prev_current_new_target =
            CURRENT_NEW_TARGET.with(|value| value.replace(func_value.to_bits()));
        let result = crate::closure::js_native_call_value(func_value, args_ptr, args_len);
        CURRENT_NEW_TARGET.with(|value| value.set(prev_current_new_target));
        crate::object::js_new_target_set(prev_new_target);
        crate::object::js_implicit_this_set(prev_this);
        if constructor_return_overrides_this(result) {
            return result;
        }
    }
    nan_boxed
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
    if is_non_constructable_builtin_function_value(value) {
        return false;
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
        js_object_alloc(instance_cid, 0)
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
    let prev_new_target = crate::object::js_new_target_get();
    crate::object::js_new_target_set(new_target);
    let prev_current_new_target =
        CURRENT_NEW_TARGET.with(|value| value.replace(new_target.to_bits()));
    super::super::class_constructors::replay_registered_class_constructor(
        target_cid, inst, args_ptr, args_len,
    );
    CURRENT_NEW_TARGET.with(|value| value.set(prev_current_new_target));
    crate::object::js_new_target_set(prev_new_target);
    // ClassRef `new` of a Request/Response subclass — attach the native fetch
    // handle on the dynamic path (mirrors the class-expression arm above).
    if let Some(kind) = fetch_parent_kind_in_chain(target_cid) {
        if super::super::field_get_set::fetch_subclass_handle_id(inst as usize).is_none() {
            super::super::attach_fetch_handle_for_construction(inst, kind, args_ptr, args_len);
        }
    }
    crate::value::js_nanbox_pointer(inst as i64)
}

/// `GetPrototypeFromConstructor(newTarget)` restricted to the "use it only when
/// it is an object" rule: returns `newTarget.prototype`'s bits when that value
/// is an object (so a typed-array view should adopt it as its `[[Prototype]]`),
/// or `None` when it is a primitive (so the default per-kind prototype applies).
fn new_target_custom_object_prototype(new_target: f64) -> Option<u64> {
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
            // Read `newTarget.prototype` (GetPrototypeFromConstructor) BEFORE
            // building the view: Node evaluates the proto access as part of
            // AllocateTypedArray, so a throwing `prototype` getter must surface
            // here even when later steps would also throw (test262
            // `throw-type-error-before-custom-proto-access` agreement).
            let proto_bits = new_target_custom_object_prototype(nt);
            let result = js_new_function_construct(func_value, args_ptr, args_len);
            if let Some(addr) = crate::typedarray_props::typed_array_addr_from_value(result) {
                if let Some(proto_bits) = proto_bits {
                    super::super::prototype_chain::object_set_static_prototype(addr, proto_bits);
                }
            }
            return result;
        }
    }
    if !is_callable_function_value(func_value) {
        return js_new_function_construct(func_value, args_ptr, args_len);
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
    let obj_ptr = js_object_alloc(cid, 0);
    let nan_boxed = crate::value::js_nanbox_pointer(obj_ptr as i64);
    if let Some(proto_bits) = constructor_prototype_bits(nt) {
        super::super::prototype_chain::object_set_static_prototype(obj_ptr as usize, proto_bits);
    }

    let prev_this = crate::object::js_implicit_this_get();
    let prev_new_target = crate::object::js_new_target_get();
    crate::object::js_implicit_this_set(nan_boxed);
    crate::object::js_new_target_set(nt);
    let prev_current_new_target = CURRENT_NEW_TARGET.with(|value| value.replace(nt.to_bits()));
    let result = crate::closure::js_native_call_value(func_value, args_ptr, args_len);
    CURRENT_NEW_TARGET.with(|value| value.set(prev_current_new_target));
    crate::object::js_new_target_set(prev_new_target);
    crate::object::js_implicit_this_set(prev_this);
    if constructor_return_overrides_this(result) {
        return result;
    }
    nan_boxed
}

fn constructor_return_overrides_this(value: f64) -> bool {
    use crate::value::JSValue;
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    if is_callable_function_value(value) {
        return true;
    }
    let raw = jv.as_pointer::<u8>();
    if raw.is_null() {
        return false;
    }
    if super::super::is_arguments_object(raw as *const ObjectHeader) {
        return true;
    }
    unsafe {
        let arr = crate::array::clean_arr_ptr(raw as *const crate::array::ArrayHeader);
        if !arr.is_null() {
            return true;
        }
        if !is_valid_obj_ptr(raw as *const u8) {
            return false;
        }
        let gc_header =
            (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        matches!(
            (*gc_header).obj_type,
            // Per spec, a constructor returning ANY Object overrides the
            // implicit `this`. Promises are objects — a user constructor like
            // `function P(exec){ return new Promise(...) }` (the
            // `NewPromiseCapability` shape exercised by the Promise-combinator
            // test262 cases) must yield that Promise, not the empty default.
            // GC_TYPE_TEMPORAL: `new Temporal.Duration(...)` (and every other
            // Temporal constructor) is dispatched through this generic path —
            // the constructor thunk allocates a Temporal cell and returns it, so
            // that cell must override the empty default `this` (#4687).
            crate::gc::GC_TYPE_OBJECT
                | crate::gc::GC_TYPE_ERROR
                | crate::gc::GC_TYPE_PROMISE
                | crate::gc::GC_TYPE_TEMPORAL
        )
    }
}

/// Apply ECMAScript constructor return-override semantics for an inlined
/// constructor body's explicit `return <value>`. Given the implicit `this`
/// and the returned value:
///   - returned value is an Object  → it becomes the construction result;
///   - returned value is `undefined` → result is `this`;
///   - returned value is any other primitive → for a derived constructor
///     (`class X extends Y`) this is a TypeError; for a base constructor the
///     primitive is ignored and the result is `this`.
/// `is_derived` is 1 for a class with an `extends` clause, 0 otherwise.
/// Refs class/subclass/derived-class-return-override-*.
#[no_mangle]
pub extern "C" fn js_ctor_return_override(this_val: f64, return_val: f64, is_derived: i32) -> f64 {
    use crate::value::JSValue;
    if constructor_return_overrides_this(return_val) {
        return return_val;
    }
    let jv = JSValue::from_bits(return_val.to_bits());
    if jv.is_undefined() {
        return this_val;
    }
    if is_derived != 0 {
        crate::collection_iter::throw_type_error(
            "Derived constructors may only return object or undefined",
        );
    }
    // Base constructor: a returned primitive is ignored.
    this_val
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

fn is_arrow_function_value(value: f64) -> bool {
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

/// Predicate-only sibling of `ordinary_function_prototype_value_for_read`:
/// would this function have an own `.prototype` slot? Crucially does NOT
/// materialize the prototype object — `fn.hasOwnProperty('prototype')` must
/// not lock the slot's attributes before a later
/// `Object.defineProperty(fn, "prototype", …)` (TypedArrayConstructors
/// custom-proto tests).
pub(crate) fn function_would_have_own_prototype(func_value: f64) -> bool {
    if !is_callable_function_value(func_value) || is_arrow_function_value(func_value) {
        return false;
    }
    if super::super::native_module::builtin_closure_is_non_constructable_value(func_value) {
        return false;
    }
    synthetic_class_id_for_function(func_value) != 0
}

pub(crate) fn ordinary_function_prototype_value_for_read(func_value: f64) -> Option<f64> {
    if !is_callable_function_value(func_value) || is_arrow_function_value(func_value) {
        return None;
    }
    // Bound-method / bound-function values (class method/getter/setter reads via
    // `C.prototype.m`, instance method reads, `fn.bind(...)`) are non-constructors
    // and have NO `prototype` own property (`C.prototype.m.prototype === undefined`,
    // `'prototype' in C.prototype.m === false`). (Test262 definition method/accessor
    // prop-desc.)
    //
    // #4973 / #3527 / #5268 exception: bound NATIVE-MODULE *class* exports
    // (`http.Server`, `fs.ReadStream`, `events.EventEmitter`, …) are
    // constructors in Node, and the util.inherits / `Object.create(Ctor.
    // prototype)` / `Object.setPrototypeOf(x, Ctor.prototype)` subclass
    // pattern reads their `.prototype` as a setPrototypeOf / Object.create
    // operand. Returning None here made that read `undefined`, and
    // `Object.create(undefined)` / `Object.setPrototypeOf(x, undefined)` then
    // threw "Object prototype may only be an Object or null" — the blocker hit
    // at Express init (`express/lib/request.js`:
    // `Object.create(http.IncomingMessage.prototype)`), graceful-fs's
    // `ReadStream.prototype = Object.create(fs$ReadStream.prototype)`, and
    // pino's `Object.setPrototypeOf(prototype, EventEmitter.prototype)`.
    //
    // A bound-native export is a constructor class when its method name uses
    // Node's constructor-cased convention (a leading uppercase ASCII letter,
    // e.g. `ReadStream`/`EventEmitter`/`Server`) AND it isn't explicitly
    // marked non-constructable (built-in prototype methods like
    // `String.prototype.charAt` carry that flag). Such exports are cached
    // singleton closures (NATIVE_CALLABLE_EXPORTS), so the synthetic-class
    // path below gives them a stable `.prototype` object. Non-constructor
    // bound methods (`fs.readFile`, `path.join`, …) keep `prototype ===
    // undefined`, matching Node's built-in non-constructor functions.
    {
        let jv = crate::value::JSValue::from_bits(func_value.to_bits());
        if jv.is_pointer() {
            let cptr = jv.as_pointer::<crate::closure::ClosureHeader>();
            if !cptr.is_null()
                && is_valid_obj_ptr(cptr as *const u8)
                && crate::closure::closure_is_bound_method(cptr)
            {
                if super::super::native_module::builtin_closure_is_non_constructable_value(
                    func_value,
                ) {
                    return None;
                }
                let is_native_class_export = unsafe {
                    super::super::native_module::bound_native_callable_module_and_method(func_value)
                }
                .map(|(_module, method)| {
                    method
                        .as_bytes()
                        .first()
                        .is_some_and(|b| b.is_ascii_uppercase())
                })
                .unwrap_or(false);
                if !is_native_class_export {
                    return None;
                }
            }
        }
    }
    // Built-in methods (`String.prototype.charAt`, `Array.prototype.map`, …) are
    // not constructors and have NO `prototype` own property — `String.prototype.
    // charAt.prototype === undefined` (ECMA-262: built-in non-constructor
    // functions don't get the auto-created `.prototype`). Don't lazily synthesize
    // one for them.
    if super::super::native_module::builtin_closure_is_non_constructable_value(func_value) {
        return None;
    }
    let cid = synthetic_class_id_for_function(func_value);
    if cid == 0 {
        return None;
    }
    let proto = ensure_function_prototype_object(func_value, cid);
    if proto.is_null() {
        return None;
    }
    Some(crate::value::js_nanbox_pointer(proto as i64))
}

#[no_mangle]
pub extern "C" fn js_function_prototype_value_for_read(func_value: f64) -> f64 {
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let jv = crate::value::JSValue::from_bits(func_value.to_bits());
    if !jv.is_pointer() {
        return undef;
    }
    let ptr = jv.as_pointer() as *const crate::closure::ClosureHeader;
    if ptr.is_null() || !is_valid_obj_ptr(ptr as *const u8) {
        return undef;
    }
    unsafe {
        if (*ptr).type_tag != crate::closure::CLOSURE_MAGIC {
            return undef;
        }
    }

    let closure_addr = ptr as usize;
    if crate::closure::closure_is_key_deleted(closure_addr, "prototype") {
        return undef;
    }
    let dynamic = crate::closure::closure_get_dynamic_prop(closure_addr, "prototype");
    if dynamic.to_bits() != crate::value::TAG_UNDEFINED {
        return dynamic;
    }
    if let Some(proto) = generator_function_prototype_of(closure_addr) {
        return proto;
    }
    ordinary_function_prototype_value_for_read(func_value).unwrap_or(undef)
}

/// Lookup helper: returns the registered prototype-method value for
/// `(class_id, name)`, or None if no assignment matched. Walks the
/// parent-class chain so methods registered on a base class are found
/// via subclass instances.
pub(crate) fn lookup_prototype_method(class_id: u32, name: &str) -> Option<f64> {
    let guard = CLASS_PROTOTYPE_METHODS.read().ok()?;
    let map = guard.as_ref()?;
    let mut cid = class_id;
    let mut depth = 0usize;
    while depth < 32 {
        if let Some(per_class) = map.get(&cid) {
            if let Some(&bits) = per_class.get(name) {
                return Some(f64::from_bits(bits));
            }
        }
        match get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
    None
}
