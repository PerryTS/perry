use super::super::global_this_tables::BuiltinConstructorIdentity;
use super::super::native_module::worker_threads_construct;
use super::*;
use crate::JSValue;

crate::perry_thread_local! {
    /// `new.target` for the construction currently on this thread's stack.
    ///
    /// **This is a GC root, and must stay one (#7231).** It holds a NaN-boxed
    /// closure/class value for the whole constructor body, and a constructor
    /// body runs arbitrary user code. `this_binding.rs`'s `NEW_TARGET` holds
    /// the same value under `scan_dispatch_binding_roots_mut`; this is a second
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

mod compiled_function;
pub(crate) use compiled_function::{
    forget_birth_record_of_class, ordinary_compiled_function_has_instance, OrdinaryInstanceof,
};
mod builtin;
pub(in crate::object) use builtin::{
    web_text_construct, web_url_construct, web_webcrypto_construct, web_webfetch_construct,
};
mod collections;
mod native_receiver;
mod rooted_arguments;
pub(crate) use rooted_arguments::construct_rooted_arguments;
#[cfg(feature = "regex-engine")]
pub(crate) use rooted_arguments::construct_two_rooted;

mod nm_ctors;
pub(crate) use nm_ctors::{
    nm_ctor_child_process, nm_ctor_cluster, nm_ctor_events, nm_ctor_fs, nm_ctor_readline,
    nm_ctor_repl, nm_ctor_stream, nm_ctor_tls, nm_ctor_tty, nm_ctor_vm, nm_ctor_wasi,
};

/// Issue #838 followup (b): construct an instance from a function value.
/// Function prototype methods live on the function's actual prototype,
/// which is also the fresh instance's recorded prototype.
#[no_mangle]
// This is a generated-code boundary whose TypeError paths unwind to the
// caller's JavaScript catch landing pad. A plain `extern "C"` installs an
// abort-on-unwind guard in the debug/static runtime, so `new <primitive>()`
// aborts instead of remaining catchable.
pub unsafe extern "C-unwind" fn js_new_function_construct(
    func_value: f64,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    construct_function_impl(func_value, args_ptr, args_len, std::ptr::null())
}

unsafe fn construct_function_impl(
    func_value: f64,
    args_ptr: *const f64,
    args_len: usize,
    site: *const std::sync::atomic::AtomicU64,
) -> f64 {
    if let Some(identity) = super::super::global_this_tables::constructor_identity(func_value) {
        let args = if args_ptr.is_null() {
            &[][..]
        } else {
            std::slice::from_raw_parts(args_ptr, args_len)
        };
        if let Some(result) = builtin::construct(identity, func_value, args) {
            return result;
        }
    }
    if let Some(entry) = site::validated_class_entry(site, func_value) {
        return construct_class_object_entry(
            func_value,
            args_ptr,
            args_len,
            func_value,
            Some(entry),
            site::capture_slot(site),
        );
    }
    // #10507: an ordinary compiled function is none of the exotic callees
    // below — a fact of its body, read once from its info.
    if let Some(closure) = compiled_function::ordinary_compiled_function_at_site(func_value, site) {
        if let Some(result) = compiled_function::construct_ordinary_compiled_function(
            func_value, closure, args_ptr, args_len, site,
        ) {
            return result;
        }
    }
    // A class value (its function object, or the legacy immediate) constructs
    // its class: decided first, one closure probe, before the exotic arms.
    if let Some(class_cid) = constructor_class_ref_id(func_value) {
        let entry = site::resolved_site_constructor(site, u64::MAX - 1, class_cid);
        return construct_registered_class_ref_entry(
            class_cid, class_cid, func_value, args_ptr, args_len, entry,
        );
    }
    if is_class_object_value(func_value) {
        let object = JSValue::from_bits(func_value.to_bits()).as_pointer::<ObjectHeader>();
        let entry = site::site_constructor(site, object);
        let capture_slot = site::capture_slot(site);
        return construct_class_object_entry(
            func_value,
            args_ptr,
            args_len,
            func_value,
            entry,
            capture_slot,
        );
    }
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
    // #3656: `new p()` where `p` is a Proxy dispatches through its `construct`
    // trap (or forwards to the target). Reached when the compiler can't prove
    // the callee is a proxy statically (e.g. `new record.proxy()`). newTarget
    // for a plain `new` is the constructor being invoked — the proxy itself.
    if crate::proxy_hooks::proxy_is_proxy(func_value) == 1 {
        let arr = crate::array::js_array_alloc(0);
        let mut a = arr;
        if !args_ptr.is_null() {
            for i in 0..args_len {
                a = crate::array::js_array_push_f64(a, *args_ptr.add(i));
            }
        }
        let arr_box = f64::from_bits(0x7FFD_0000_0000_0000 | (a as u64 & 0x0000_FFFF_FFFF_FFFF));
        return crate::proxy_hooks::proxy_construct(func_value, arr_box, func_value);
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
        if let Some(value) =
            super::super::native_module::static_export_construct(func_value, args_ptr, args_len)
        {
            return value;
        }
        if module == "perf_hooks" {
            if let Some(result) =
                crate::perf_hook_slots::construct_perf_hooks_class(&method, args_ptr, args_len)
            {
                return result;
            }
        }
        if module == "perf_histogram"
            && matches!(method.as_str(), "RecordableHistogram" | "ELDHistogram")
        {
            return crate::perf_hook_slots::js_perf_illegal_constructor();
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
        // `new <bound async_hooks.AsyncLocalStorage>()` / `<...AsyncResource>()`.
        // Next.js stores the native ctor on `globalThis.AsyncLocalStorage` and
        // later does `new maybeGlobalAsyncLocalStorage()` (a dynamic callee), so
        // the static `new AsyncLocalStorage()` codegen arm never fires. Without
        // this the instance was a class_id=0 empty object whose `.getStore` read
        // back `undefined` -> "getStore is not a function" at server startup.
        // Route to the stdlib handle constructor via the registered dispatcher.
        if (module == "async_hooks"
            && matches!(method.as_str(), "AsyncLocalStorage" | "AsyncResource"))
            || (module == "string_decoder" && method == "StringDecoder")
        {
            let ptr = crate::value::JS_NATIVE_ASYNC_HOOKS_CONSTRUCT
                .load(std::sync::atomic::Ordering::SeqCst);
            if !ptr.is_null() {
                let dispatch: crate::value::JsNativeEventsConstructFn = std::mem::transmute(ptr);
                return dispatch(method.as_ptr(), method.len(), args_ptr, args_len);
            }
        }
        // `new ns.Worker(...)` on the worker_threads namespace as a value.
        if let Some(worker) = worker_threads_construct(&module, &method, args_ptr, args_len) {
            return worker;
        }
        // `new dns.Resolver()` / `new dp.Resolver()` where the constructor was read
        // off the namespace as a value (`const dp = require("dns/promises")`).
        if matches!(module.as_str(), "dns" | "dns/promises") && method == "Resolver" {
            let mut arr = crate::array::js_array_alloc(args_len as u32);
            if !args_ptr.is_null() {
                for i in 0..args_len {
                    arr = crate::array::js_array_push_f64(arr, *args_ptr.add(i));
                }
            }
            if let Some(resolver) =
                crate::dns::dispatch_export(module == "dns/promises", "Resolver", arr as i64)
            {
                return resolver;
            }
        }
        if module == "zlib"
            && matches!(
                method.as_str(),
                "Gzip"
                    | "Gunzip"
                    | "Deflate"
                    | "Inflate"
                    | "DeflateRaw"
                    | "InflateRaw"
                    | "Unzip"
                    | "BrotliCompress"
                    | "BrotliDecompress"
                    | "ZstdCompress"
                    | "ZstdDecompress"
            )
        {
            let ptr =
                crate::value::JS_NATIVE_ZLIB_DISPATCH.load(std::sync::atomic::Ordering::SeqCst);
            if !ptr.is_null() {
                let dispatch: unsafe extern "C" fn(*const u8, usize, *const f64, usize) -> f64 =
                    std::mem::transmute(ptr);
                return dispatch(method.as_ptr(), method.len(), args_ptr, args_len);
            }
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
    // All class/builtin/proxy constructors returned above. The remaining
    // path constructs callable function values; ordinary instances have no
    // [[Construct]], regardless of their prototype or private fields.
    if !is_callable_function_value(func_value) || extends_target_must_throw(func_value) {
        super::super::object_ops::throw_object_type_error(b"is not a constructor");
    }
    // Both owner and instance cross a collecting Get of function.prototype.
    // Ordinary functions have no class-table identity.
    let func_scope = crate::gc::RuntimeHandleScope::new();
    let func_handle = func_scope.root_nanbox_f64(func_value);
    let obj = func_scope.root_raw_mut_ptr(js_object_alloc(0, 0));
    let proto_bits = constructor_prototype_bits(func_handle.get_nanbox_f64());
    if let Some(proto_bits) = proto_bits {
        obj.with_mut_ptr::<ObjectHeader, _>(|obj| {
            super::super::prototype_chain::object_link_class_default_prototype(
                obj as usize,
                proto_bits,
            )
        });
    }
    let func_value = func_handle.get_nanbox_f64();
    let nan_boxed =
        obj.with_mut_ptr::<ObjectHeader, _>(|obj| crate::value::js_nanbox_pointer(obj as i64));
    // Only run the constructor body when the callee is recognised as
    // a closure shape. The codegen LocalGet path widens the route to
    // any local-resolved callee, so we have to gate the
    // `js_native_call_value` dispatch on a verified closure pointer
    // here — otherwise `new <non-callable>()` would dereference an
    // arbitrary pointer as a `ClosureHeader` and crash.
    if is_callable_function_value(func_value) {
        // Pass the new instance as `this` and dispatch the constructor.
        // The dispatch
        // result is discarded — JS `new` semantics use the receiver,
        // not the returned value (object returns would override, but
        // dayjs and siblings rely on the receiver mutation pattern).
        // Reproduced unrooted by `new inst.ctor(x)` where `inst.ctor` is a
        // plain function, 200/200 iterations wrong under
        // `PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_GC_SCHEDULE_SEED=1 PERRY_GC_SCHEDULE_RATE=1`
        // (#7280): the helper roots the instance across the body.
        return compiled_function::run_constructor_body(
            func_value, nan_boxed, args_ptr, args_len, false,
        );
    }
    // Ordinary objects, symbols and every other non-callable heap value do not
    // have [[Construct]]. The historical placeholder return made `new
    // Reflect()`, `new Error.prototype()` and `new sym()` silently succeed.
    super::super::object_ops::throw_object_type_error(b"is not a constructor")
}

const _: unsafe extern "C-unwind" fn(f64, *const f64, usize) -> f64 = js_new_function_construct;

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
    super::super::class_value::class_value_id(value)
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
    if constructor_class_ref_id(value).is_some() || is_class_object_value(value) {
        return true;
    }
    if crate::proxy_hooks::proxy_is_proxy(value) == 1 {
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
    if crate::proxy_hooks::proxy_is_proxy(value) == 1 {
        return extends_target_must_throw(crate::proxy_hooks::proxy_target(value));
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
            if crate::closure::closure_is_bound_method(ptr)
                || crate::closure::closure_body_is_non_constructor(ptr)
            {
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
            if crate::closure::closure_info(ptr).is_some_and(|info| {
                info.flags
                    & (crate::closure::FN_ARROW
                        | crate::closure::FN_ASYNC
                        | crate::closure::FN_GENERATOR
                        | crate::closure::FN_ASYNC_GENERATOR)
                    != 0
            }) {
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
        let bound_args = crate::closure::bound_function_args(ptr);
        if !bound_args.is_empty() {
            let mut prefix = Vec::with_capacity(bound_args.len() + args.len());
            prefix.extend_from_slice(bound_args);
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
mod site;
pub use site::{js_class_value_super_construct_site, js_new_function_construct_site};
include!("construct/promise_subclass.rs");

unsafe fn construct_registered_class_ref(
    target_cid: u32,
    instance_cid: u32,
    new_target: f64,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    construct_registered_class_ref_entry(
        target_cid,
        instance_cid,
        new_target,
        args_ptr,
        args_len,
        None,
    )
}

unsafe fn construct_registered_class_ref_entry(
    target_cid: u32,
    instance_cid: u32,
    new_target: f64,
    args_ptr: *const f64,
    args_len: usize,
    entry: Option<(usize, u32, u32)>,
) -> f64 {
    let birth_args = if args_ptr.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(args_ptr, args_len)
    };
    let inst = allocate_class_instance(instance_cid, birth_args).0;
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
    let prev_new_target_handle = scope.root_new_target_save(prev_new_target);
    crate::object::js_new_target_set(new_target);
    let prev_current_new_target =
        CURRENT_NEW_TARGET.with(|value| value.replace(new_target.to_bits()));
    let prev_current_new_target_handle = scope.root_nanbox_u64(prev_current_new_target);
    let ctor_result = super::super::class_constructors::construct_registered_class_resolved(
        target_cid, inst, args_ptr, args_len, entry,
    );
    let inst: *mut ObjectHeader = inst_handle.get_raw_mut_ptr();
    CURRENT_NEW_TARGET.with(|value| value.set(prev_current_new_target_handle.get_nanbox_u64()));
    crate::object::js_new_target_set(prev_new_target_handle.get_nanbox_f64());
    // A replayed standalone constructor can bind a replacement `this` from
    // its dynamic super-constructor. Preserve that result across the runtime
    // ClassRef construction boundary instead of publishing the provisional
    // allocation whose fields the constructor stopped updating (#9503).
    let current_inst = crate::value::js_nanbox_pointer(inst as i64);
    if ctor_result.to_bits() != current_inst.to_bits()
        && constructor_return_overrides_this(ctor_result)
    {
        return ctor_result;
    }
    // Every derived constructor reaches its actual superclass through super;
    // native state belongs to that call, never to definition-time ancestry.
    current_inst
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
    let proto = new_target_prototype_get(new_target)?;
    if unsafe { super::super::value_is_object_like(proto) }
        || super::super::class_ref_id(proto).is_some()
    {
        Some(proto.to_bits())
    } else {
        None
    }
}

/// `Get(newTarget, "prototype")` for an object or Proxy `newTarget`.
fn new_target_prototype_get(new_target: f64) -> Option<f64> {
    let bits = new_target.to_bits();
    let proxy = crate::proxy_hooks::proxy_is_proxy(new_target) != 0;
    if !proxy && (bits >> 48) != 0x7FFD {
        return None;
    }
    let raw = (bits & crate::value::POINTER_MASK) as usize;
    if raw == 0 {
        return None;
    }
    let key = crate::string::js_string_from_bytes(b"prototype".as_ptr(), b"prototype".len() as u32);
    if proxy {
        let key = crate::value::js_nanbox_string(key as i64);
        return Some(crate::proxy_hooks::proxy_get(new_target, key));
    }
    Some(js_object_get_field_by_name_f64(
        raw as *const ObjectHeader,
        key,
    ))
}

fn constructor_prototype_bits(new_target: f64) -> Option<u64> {
    let Some(proto) = new_target_prototype_get(new_target) else {
        return global_object_prototype_bits();
    };
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
    if crate::proxy_hooks::proxy_is_proxy(func_value) == 1 {
        let arr = crate::array::js_array_alloc(0);
        let mut a = arr;
        if !args_ptr.is_null() {
            for i in 0..args_len {
                a = crate::array::js_array_push_f64(a, *args_ptr.add(i));
            }
        }
        let arr_box = f64::from_bits(0x7FFD_0000_0000_0000 | (a as u64 & 0x0000_FFFF_FFFF_FFFF));
        return crate::proxy_hooks::proxy_construct(func_value, arr_box, nt);
    }
    if let Some(target_cid) = constructor_class_ref_id(func_value) {
        let instance_cid = new_target_class_id(nt).unwrap_or(target_cid);
        return construct_registered_class_ref(target_cid, instance_cid, nt, args_ptr, args_len);
    }
    // #11229: a per-evaluation class object with a distinct newTarget.
    if is_class_object_value(func_value) {
        return construct_class_object_with_new_target(func_value, args_ptr, args_len, nt);
    }
    if let Some(identity) = super::super::global_this_tables::constructor_identity(func_value) {
        let args = if args_ptr.is_null() {
            &[][..]
        } else {
            std::slice::from_raw_parts(args_ptr, args_len)
        };
        if collections::handles(identity) {
            return collections::construct(identity, func_value, args, nt);
        }
        if identity == BuiltinConstructorIdentity::Object {
            return construct_object_with_new_target(nt);
        }
        // These factories throw or return a proxy of the caller's target;
        // they do not allocate an ordinary instance with new.target.prototype.
        if matches!(
            identity,
            BuiltinConstructorIdentity::Symbol
                | BuiltinConstructorIdentity::BigInt
                | BuiltinConstructorIdentity::Proxy
                | BuiltinConstructorIdentity::Crypto
                | BuiltinConstructorIdentity::CryptoKey
                | BuiltinConstructorIdentity::SubtleCrypto
                | BuiltinConstructorIdentity::Navigator
                | BuiltinConstructorIdentity::MessagePort
                | BuiltinConstructorIdentity::Storage
        ) {
            if let Some(result) = builtin::construct(identity, func_value, args) {
                return result;
            }
        } else {
            let scope = crate::gc::RuntimeHandleScope::new();
            let nt = scope.root_nanbox_f64(nt);
            let func = scope.root_nanbox_f64(func_value);
            let args: Vec<_> = args.iter().map(|&v| scope.root_nanbox_f64(v)).collect();
            // A typed-array content conversion is observed before the custom
            // prototype Get; preserve the existing ordering for that family.
            let typed = matches!(
                identity,
                BuiltinConstructorIdentity::Int8Array
                    | BuiltinConstructorIdentity::Uint8Array
                    | BuiltinConstructorIdentity::Uint8ClampedArray
                    | BuiltinConstructorIdentity::Int16Array
                    | BuiltinConstructorIdentity::Uint16Array
                    | BuiltinConstructorIdentity::Int32Array
                    | BuiltinConstructorIdentity::Uint32Array
                    | BuiltinConstructorIdentity::Float16Array
                    | BuiltinConstructorIdentity::Float32Array
                    | BuiltinConstructorIdentity::Float64Array
                    | BuiltinConstructorIdentity::BigInt64Array
                    | BuiltinConstructorIdentity::BigUint64Array
            );
            let proto = if typed {
                None
            } else {
                new_target_custom_object_prototype(nt.get_nanbox_f64())
                    .map(|bits| scope.root_nanbox_u64(bits))
            };
            let values: Vec<_> = args.iter().map(|h| h.get_nanbox_f64()).collect();
            if let Some(result) = builtin::construct(identity, func.get_nanbox_f64(), &values) {
                let result = scope.root_nanbox_f64(result);
                let proto = if typed {
                    new_target_custom_object_prototype(nt.get_nanbox_f64())
                        .map(|bits| scope.root_nanbox_u64(bits))
                } else {
                    proto
                };
                if identity == BuiltinConstructorIdentity::RegExp {
                    return native_receiver::finish_native_object(
                        &scope,
                        result.get_nanbox_f64(),
                        &nt,
                        proto,
                    );
                }
                native_receiver::apply_prototype(result.get_nanbox_f64(), proto);
                return result.get_nanbox_f64();
            }
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

    // Declared newTargets retain their class identity; ordinary functions
    // select the actual prototype through GetPrototypeFromConstructor.
    let cid = new_target_class_id(nt).unwrap_or(0);
    let nt_scope = crate::gc::RuntimeHandleScope::new();
    let nt_handle = nt_scope.root_nanbox_f64(nt);
    let func_handle = nt_scope.root_nanbox_f64(func_value);
    let args = nt_scope.root_nanbox_f64_slice(if args_len == 0 || args_ptr.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(args_ptr, args_len)
    });
    let obj = nt_scope.root_raw_mut_ptr(js_object_alloc(
        cid,
        crate::object::learned_inline_field_count(cid),
    ));
    if let Some(proto_bits) = constructor_prototype_bits(nt_handle.get_nanbox_f64()) {
        obj.with_mut_ptr::<ObjectHeader, _>(|obj| {
            super::super::prototype_chain::object_set_static_prototype(obj as usize, proto_bits)
        });
    }
    let nt = nt_handle.get_nanbox_f64();
    let nan_boxed =
        obj.with_mut_ptr::<ObjectHeader, _>(|obj| crate::value::js_nanbox_pointer(obj as i64));

    // #7280: same unrooted-receiver shape as the plain-`new` tail above —
    // `nan_boxed` and the two displaced new.target values cross a user
    // constructor body. Reproduced by
    // `Reflect.construct(plainFn, [x], otherFn)`, 200/200 iterations wrong
    // under `PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_GC_SCHEDULE_SEED=1 PERRY_GC_SCHEDULE_RATE=1`.
    let scope = crate::gc::RuntimeHandleScope::new();
    let inst_handle = scope.root_nanbox_f64(nan_boxed);
    let prev_new_target = crate::object::js_new_target_get();
    let prev_new_target_handle = scope.root_new_target_save(prev_new_target);
    crate::object::js_new_target_set(nt);
    let prev_current_new_target = CURRENT_NEW_TARGET.with(|value| value.replace(nt.to_bits()));
    let prev_current_new_target_handle = scope.root_nanbox_u64(prev_current_new_target);
    let args = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&args);
    let result = crate::closure::native_call_value_this(
        func_handle.get_nanbox_f64(),
        crate::closure::JsThis::from_f64(inst_handle.get_nanbox_f64()),
        args.as_ptr(),
        args.len(),
    );
    CURRENT_NEW_TARGET.with(|value| value.set(prev_current_new_target_handle.get_nanbox_u64()));
    crate::object::js_new_target_set(prev_new_target_handle.get_nanbox_f64());
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
/// (`new someObject()`, `new someStringVar()`, etc.). The kind is the GC
/// header's type byte (`closure_kind_probe`), not a payload magic word.
pub(crate) fn is_callable_function_value(value: f64) -> bool {
    use crate::value::JSValue;
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    let ptr = jv.as_pointer() as *const crate::closure::ClosureHeader;
    crate::closure::is_closure_ptr(ptr as usize)
}

pub(super) fn is_arrow_function_value(value: f64) -> bool {
    use crate::value::JSValue;
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    let ptr = jv.as_pointer() as *const crate::closure::ClosureHeader;
    if !is_callable_function_value(value) {
        return false;
    }
    crate::closure::closure_is_arrow(ptr)
}

#[cfg(test)]
mod callable_probe_tests {
    use super::*;
    use crate::closure::{ClosureHeader, JsFunctionInfo};

    extern "C" fn test_body(_closure: *const ClosureHeader, _this: crate::closure::JsThis) -> f64 {
        0.0
    }

    static ORDINARY_INFO: JsFunctionInfo =
        JsFunctionInfo::of(test_body as crate::codegen_abi::JsBody0<ClosureHeader>);
    static ARROW_INFO: JsFunctionInfo =
        JsFunctionInfo::of(test_body as crate::codegen_abi::JsBody0<ClosureHeader>)
            .with_flags(crate::closure::FN_ARROW);

    fn pointer_value(ptr: usize) -> f64 {
        f64::from_bits(crate::value::POINTER_TAG | ptr as u64)
    }

    #[test]
    fn callable_probes_reject_unowned_words_and_preserve_live_closure_kinds() {
        let _global = crate::gc::global_side_table_test_lock();
        let _threshold = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();

        for handle in [0x40000usize, 0xF0000, 0x100000] {
            let value = pointer_value(handle);
            assert!(!is_callable_function_value(value));
            assert!(!is_arrow_function_value(value));
        }

        #[cfg(unix)]
        {
            use std::ptr;
            let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize };
            let mapping = unsafe {
                libc::mmap(
                    ptr::null_mut(),
                    page,
                    libc::PROT_NONE,
                    libc::MAP_PRIVATE | libc::MAP_ANON,
                    -1,
                    0,
                )
            };
            assert_ne!(mapping, libc::MAP_FAILED, "mmap failed");
            struct Mapping(*mut libc::c_void, usize);
            impl Drop for Mapping {
                fn drop(&mut self) {
                    unsafe { libc::munmap(self.0, self.1) };
                }
            }
            let _mapping = Mapping(mapping, page);
            let ptr = mapping as usize + crate::gc::GC_HEADER_SIZE;
            assert!(crate::value::addr_class::is_above_handle_band(ptr));
            assert!(ptr.is_multiple_of(std::mem::align_of::<ClosureHeader>()));
            assert!(crate::value::addr_class::is_valid_obj_ptr(ptr as *const u8));
            assert_eq!(
                crate::arena::classify_heap_generation(ptr),
                crate::arena::HeapGeneration::Unknown
            );
            assert!(unsafe { crate::value::addr_class::try_read_tracked_gc_header(ptr) }.is_none());
            let value = pointer_value(ptr);
            assert!(!is_callable_function_value(value));
            assert!(!is_arrow_function_value(value));
        }

        let ordinary = crate::closure::js_closure_alloc(&ORDINARY_INFO, 0);
        let ordinary_value = pointer_value(ordinary as usize);
        assert!(is_callable_function_value(ordinary_value));
        assert!(!is_arrow_function_value(ordinary_value));

        let arrow = crate::closure::js_closure_alloc(&ARROW_INFO, 0);
        let arrow_value = pointer_value(arrow as usize);
        assert!(is_callable_function_value(arrow_value));
        assert!(is_arrow_function_value(arrow_value));

        let object = crate::object::js_object_alloc(0, 0);
        let object_value = pointer_value(object as usize);
        assert!(!is_callable_function_value(object_value));
        assert!(!is_arrow_function_value(object_value));
    }
}
