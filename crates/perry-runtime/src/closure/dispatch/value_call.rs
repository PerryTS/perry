//! Dynamic value-call entry points, each taking the receiver after the
//! function: `js_native_call_value` (the generic NaN-boxed callee
//! dispatcher), `js_closure_call_array` (any argument count) and the
//! spread-apply bridge `js_closure_call_apply_with_spread`; plus the V8
//! trampoline adapter `js_closure_v8_callback`.

use super::*;

/// Call a JavaScript function value with variable arguments
/// This is the native implementation for dynamic function dispatch.
/// func_value: NaN-boxed f64 containing a closure pointer
/// args_ptr: pointer to array of f64 arguments
/// args_len: number of arguments
/// Returns the result as f64
///
/// NOTE: This function is named js_native_call_value to avoid symbol collision
/// with js_call_value in perry-jsruntime which handles V8 JavaScript values.
// A dynamically-dispatched closure can throw into a generated caller's catch
// landing pad. Keep this value-call bridge unwind-capable just like
// `js_native_call_method`; otherwise debug/static runtime builds install an
// abort-on-unwind guard here and Linux aborts before the landing pad is reached.
// #8479: NOT `C-unwind`. The runtime is built `panic=abort` and JS throws
// travel as a raw Itanium `_Unwind_Exception` that must step THROUGH these
// frames untouched (see `crate::eh` and the panic=abort rationale in the
// workspace Cargo.toml). Marking a frame `extern "C-unwind"` in a
// panic=abort crate does not enable that — it makes rustc wrap the call in
// an abort-on-unwind landing pad, which is exactly the RFC-2945 guard a JS
// throw trips ("panic in a function that cannot unwind"). #8416 introduced
// the first two such guards here; #8464 added ~40 more and measurably
// regressed main (+20 gap crashes, gc-stress) before being reverted.
/// Call function value `func_value` with receiver `this`
/// (`JsThis::UNDEFINED` for a plain call) and `args_len` arguments at
/// `args_ptr`: what `func.call(this, ...args)` does.
#[cfg(panic = "abort")]
#[no_mangle]
pub unsafe extern "C" fn js_native_call_value(
    func_value: f64,
    this: crate::closure::JsThis,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    unsafe { native_call_value_this_impl(func_value, this, args_ptr, args_len) }
}

// Debug/test static archives transport Perry exceptions with Rust unwinding,
// so this outer value-call boundary must permit them to reach an interpreted
// caller's catch handler. Production keeps the plain-C wrapper above (#8479).
#[cfg(not(panic = "abort"))]
#[no_mangle]
pub unsafe extern "C-unwind" fn js_native_call_value(
    func_value: f64,
    this: crate::closure::JsThis,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    unsafe { native_call_value_this_impl(func_value, this, args_ptr, args_len) }
}

/// Call function value `func_value` with receiver `this` and `args_len`
/// arguments at `args_ptr`: the one value-call dispatcher, for runtime callers.
///
/// This calls the exported `js_native_call_value` instead of inlining the
/// dispatcher: the runtime has over a hundred callers, and inlining the whole
/// dispatch into each of them grew every linked program by ~190 KB.
#[inline(always)]
pub(crate) unsafe fn native_call_value_this(
    func_value: f64,
    this: crate::closure::JsThis,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    unsafe { js_native_call_value(func_value, this, args_ptr, args_len) }
}

/// The dispatcher body, inlined only into the two `js_native_call_value`
/// definitions (one per panic strategy).
#[inline(always)]
unsafe fn native_call_value_this_impl(
    func_value: f64,
    this: crate::closure::JsThis,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let args = if args_ptr.is_null() || args_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(args_ptr, args_len)
    };
    super::explicit_this::call_with_explicit_this(
        func_value,
        this.as_f64(),
        args,
        super::explicit_this::ReceiverBinding::AsGiven,
    )
}

/// Dispatch a call whose receiver and capture binding have already been
/// prepared by the single explicit-this forwarder.
pub(super) unsafe fn dispatch_explicit_this_call(
    func_value: f64,
    this: crate::closure::JsThis,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    use crate::value::JSValue;

    let jsval = JSValue::from_bits(func_value.to_bits());

    // ES class constructors have [[Construct]] but no [[Call]]. ClassRefs use
    // Perry's INT32-tagged constructor representation, so letting one fall
    // into the legacy raw-pointer path below reinterprets the tag bits as a
    // ClosureHeader address. A direct `C()` must instead throw TypeError.
    if (func_value.to_bits() >> 48) == 0x7FFE {
        throw_not_callable();
    }

    // The body's own record excludes Proxy, native-export, no-op prototype
    // and constructor forwarding before any of those classifications.
    let value_info = if jsval.is_pointer() {
        crate::closure::closure_info(jsval.as_pointer::<ClosureHeader>())
    } else {
        None
    };
    if let Some(info) = value_info {
        if crate::closure::has_direct_call_body(info) {
            return call_closure_body(
                jsval.as_pointer(),
                Some(info),
                info.code,
                this,
                args_ptr,
                args_len,
            );
        }
    }

    // #3656: a Proxy value invoked as a function dispatches through its `apply`
    // trap (or, absent a trap, forwards to the target). The compiler emits a
    // `ProxyApply` node when it can statically prove the callee is a proxy, but
    // indirect callees (e.g. `record.proxy()` off a `Proxy.revocable` result)
    // reach this generic value-call path with no static hint. Proxy ids encode
    // to small pointers, so real heap closures early-out of `js_proxy_is_proxy`.
    if crate::proxy::js_proxy_is_proxy(func_value) == 1 {
        let arr = crate::array::js_array_alloc(0);
        let mut a = arr;
        if !args_ptr.is_null() {
            for i in 0..args_len {
                a = crate::array::js_array_push_f64(a, unsafe { *args_ptr.add(i) });
            }
        }
        let arr_box = f64::from_bits(0x7FFD_0000_0000_0000 | (a as u64 & 0x0000_FFFF_FFFF_FFFF));
        return crate::proxy::js_proxy_apply(func_value, this.as_f64(), arr_box);
    }

    // Dynamic `super()` for `class X extends <runtime value holding
    // events.EventEmitter>` (an import alias `import { EventEmitter as E }` or a
    // local `const E = EventEmitter`): the parent is a bound-native EventEmitter
    // export reached through a runtime value, so codegen's compile-time
    // extends-NAME machinery — which emits `js_event_emitter_subclass_init` for
    // the direct `class X extends EventEmitter` form (#5137) — never fires, and
    // `js_register_class_parent_dynamic` early-returns for bound native parents.
    // The dynamic super lowering (expr/this_super_call.rs) dispatches the parent
    // VALUE here with `this` = the fresh subclass instance. Run node's
    // `EventEmitter.init` on that instance, exactly as the direct form does;
    // `this.setMaxListeners(…)`/`.on`/`.emit` resolve on the shared prototype.
    // Routed through the armed ops table (see `nm_namespace_hooks`): the
    // probe can only match a bound native callable, which exists only once
    // `callable_exports` minted one (arming the table).
    if let Some(ops) = crate::object::nm_ee_ops() {
        if let Some(result) =
            unsafe { (ops.ee_dynamic_super)(func_value, this, args_ptr, args_len) }
        {
            return result;
        }
    }

    // Get the closure pointer from the value
    // For native compilation, function values are stored as NaN-boxed pointers
    let closure: *const ClosureHeader = if jsval.is_pointer() {
        jsval.as_pointer()
    } else if jsval.is_undefined() || jsval.is_null() || func_value.is_nan() {
        // TAG_UNDEFINED, TAG_NULL, or other NaN values are not callable
        return f64::from_bits(JSValue::undefined().bits());
    } else {
        // A genuine double (bits outside the NaN-box tag space), a string, or
        // a boolean is never callable — `fn.length()` must throw a TypeError,
        // not get reinterpreted as a raw pointer. Raw-i64 heap pointers
        // (top 16 bits zero) and INT32/class-ref/bigint tags keep the legacy
        // pointer treatment below.
        let bits = func_value.to_bits();
        let top = (bits >> 48) & 0x7FFF;
        if (top != 0 && (top & 0x7FF8) != 0x7FF8) || top == 0x7FFF || top == 0x7FFC {
            throw_not_callable();
        }
        // Try treating the value directly as a pointer (for i64 representation)
        func_value.to_bits() as *const ClosureHeader
    };

    if closure.is_null() {
        // Return undefined for null/invalid closures
        return f64::from_bits(JSValue::undefined().bits());
    }

    // #3716: a built-in prototype method invoked *as a value* (the uncurry-this
    // idiom `Function.prototype.call.bind(method)`) lands here as a no-op-backed
    // closure that would just return `undefined`. Re-dispatch it by name through
    // `js_native_call_method`, with the call's receiver.
    if let Some(result) =
        crate::object::try_dispatch_value_called_proto_method(closure, this, args_ptr, args_len)
    {
        return result;
    }

    // Refs #421: when the closure body declares more params than the call site
    // provides, pad with TAG_UNDEFINED before dispatch. Without this, the
    // dispatch transmutes func_ptr to a lower-arity signature and the closure
    // body reads garbage for the missing slots — `c.text('hi')` (1 arg)
    // dispatching to a `(text, arg, headers)` arrow read the `headers` slot
    // from random stack memory, which evaluated truthy and fell into the
    // slow-path `#newResponse` chain that ended in `(number).set is not a
    // function`. Closures with rest params (`(a, ...rest) => …`) have their
    // own path (`dispatch_rest_bundled`) which already pads.
    let info = if jsval.is_pointer() {
        value_info
    } else {
        crate::closure::closure_info(closure)
    };
    let func_ptr = info.map_or(std::ptr::null(), |info| info.code);
    // %Function.prototype% is itself callable: it accepts any arguments and
    // returns `undefined` (ECMA-262 20.2.3). It is stored as a plain object,
    // so it lands here with no valid func_ptr — short-circuit before the
    // not-callable throw.
    if func_ptr.is_null() && crate::object::is_function_prototype_object_value(func_value) {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    // W2 (Next.js app-page-turbo): a class-object (OBJECT_TYPE_CLASS) can reach
    // the value-call path — e.g. `new s.RequestCookies(headers)` where the
    // dynamic callee `s.RequestCookies` resolves (through a webpack lazy-export
    // getter) to a class object, but the construct site lowered to a call rather
    // than routing to `js_new_function_construct`. Calling a class object has
    // exactly one sensible meaning — construct it — so do that here instead of
    // `throw_not_callable` (which surfaces as "value is not a function").
    if func_ptr.is_null() && crate::object::is_class_object_value(func_value) {
        // W4 experiment: a 0-arg call of a class object is most likely a
        // new-expression CALLEE RESOLUTION (`new s.RequestCookies(headers)` whose
        // member callee eval'd as a 0-arg call). Returning the class object lets
        // the OUTER `new` construct it with the real args. A call WITH args is a
        // direct construct.
        if args_len == 0 {
            return f64::from_bits(func_value.to_bits());
        }
        return crate::object::js_new_function_construct(func_value, args_ptr, args_len);
    }
    call_closure_body(closure, info, func_ptr, this, args_ptr, args_len)
}

/// Call `closure` — a compiled ordinary function body (`FN_COMPILED_BODY`),
/// which none of the exotic callees [`js_native_call_value`] tests first
/// (class refs, proxies, bound native exports, no-op-backed built-ins,
/// class objects) can carry — with receiver `this` (#10507).
///
/// # Safety
/// `closure` is a live closure whose info carries `FN_COMPILED_BODY`;
/// `args_ptr` holds `args_len` values.
pub(crate) unsafe fn call_compiled_closure_this(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let info = crate::closure::closure_info(closure);
    let func_ptr = info.map_or(std::ptr::null(), |info| info.code);
    call_closure_body(closure, info, func_ptr, this, args_ptr, args_len)
}

/// [`call_compiled_closure_this`] for a caller that already holds the
/// closure's body `info` (a shape's ConstFn lane names it): no info read or
/// validation of the closure cell.
///
/// # Safety
/// `closure` is a live closure of `info`, with a compiled body or a real
/// non-constructor builtin body (see [`crate::closure::has_direct_call_body`]);
/// `args_ptr` holds `args_len` values.
#[inline]
pub(crate) unsafe fn call_compiled_body_this(
    closure: *const ClosureHeader,
    info: &'static crate::closure::JsFunctionInfo,
    this: crate::closure::JsThis,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    call_closure_body(closure, Some(info), info.code, this, args_ptr, args_len)
}

/// The arity-padding / rest-bundling tail of a value call, once the callee is
/// known to be a closure with a body.
#[inline(always)]
unsafe fn call_closure_body(
    closure: *const ClosureHeader,
    info: Option<&crate::closure::JsFunctionInfo>,
    func_ptr: *const u8,
    this: crate::closure::JsThis,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let arg_at = |i: usize| -> f64 {
        if i < args_len && !args_ptr.is_null() {
            *args_ptr.add(i)
        } else {
            undef
        }
    };

    if func_ptr == crate::object::global_this_array_thunk as *const u8 {
        if args_len == 1 {
            let arr = crate::array::js_array_constructor_single(arg_at(0));
            return crate::value::js_nanbox_pointer(arr as i64);
        }
        let arr = crate::array::js_array_alloc(args_len as u32);
        (*arr).length = args_len as u32;
        for i in 0..args_len {
            crate::array::js_array_set_f64(arr, i as u32, arg_at(i));
        }
        return crate::value::js_nanbox_pointer(arr as i64);
    }

    // Retain the legacy null-buffer padding contract. Ordinary callers hand
    // us a valid slice; that slice goes directly to the body's one dispatcher.
    if args_ptr.is_null() && args_len != 0 {
        let args = vec![undef; args_len];
        return dispatch_body_args(closure, info, this, &args);
    }
    let args = if args_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(args_ptr, args_len)
    };
    dispatch_body_args(closure, info, this, args)
}

#[inline]
unsafe fn dispatch_body_args(
    closure: *const ClosureHeader,
    info: Option<&crate::closure::JsFunctionInfo>,
    this: crate::closure::JsThis,
    args: &[f64],
) -> f64 {
    match info {
        Some(info) => super::calln::dispatch_body_slice(closure, info, this, args),
        None => dispatch_proxy_callee_or_throw(closure, this, args),
    }
}

/// Call a closure with receiver `this` (`JsThis::UNDEFINED` for a plain
/// call) and `args_len` arguments at `args_ptr` — any count, including more
/// than the 16 `js_closure_callN` covers. Takes the closure pointer already
/// unboxed (an integer register), unlike [`js_native_call_value`].
#[no_mangle]
pub unsafe extern "C" fn js_closure_call_array(
    closure_env: i64,
    this: crate::closure::JsThis,
    args_ptr: *const f64,
    args_len: i64,
) -> f64 {
    call_array_this(
        closure_env as *const ClosureHeader,
        this,
        args_ptr,
        args_len,
    )
}

/// Adapter for V8's `native_callback_trampoline` (perry-jsruntime).
///
/// `js_create_callback(func_ptr, closure_env, param_count)` registers a JS
/// callable whose trampoline invokes `func_ptr(closure_env, args_ptr,
/// args_len)` — a contract with no receiver. The codegen arm for
/// `Expr::JsCreateCallback` (issue #248 Phase 2B) passes THIS function as
/// `func_ptr` and the raw `*const ClosureHeader` (NaN-boxing stripped) as
/// `closure_env`; it calls the closure as a plain call.
#[no_mangle]
pub unsafe extern "C" fn js_closure_v8_callback(
    closure_env: i64,
    args_ptr: *const f64,
    args_len: i64,
) -> f64 {
    call_array_this(
        closure_env as *const ClosureHeader,
        crate::closure::JsThis::UNDEFINED,
        args_ptr,
        args_len,
    )
}

unsafe fn call_array_this(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    args_ptr: *const f64,
    args_len: i64,
) -> f64 {
    if closure.is_null() {
        throw_not_callable();
    }
    let n = if args_len < 0 { 0 } else { args_len as usize };

    let info = crate::closure::closure_info(closure);
    if args_ptr.is_null() {
        if n == 0 || info.and_then(crate::closure::info_rest).is_some() {
            return dispatch_body_args(closure, info, this, &[]);
        }
        // Preserve the legacy array bridge's null-buffer zero slots.
        return dispatch_body_args(closure, info, this, &vec![0.0; n]);
    }
    let args = std::slice::from_raw_parts(args_ptr, n);
    // V8 supplies INT32-tagged numbers; compiled code supplies doubles. Keep
    // conversion at this bridge, and leave class references (the same tag)
    // intact. Only a slice containing a numeric INT32 needs scratch storage.
    let is_int = |raw: f64| {
        raw.to_bits() & 0xFFFF_0000_0000_0000 == crate::value::INT32_TAG
            && crate::object::class_ref_id(raw).is_none()
    };
    if !args.iter().copied().any(is_int) {
        return dispatch_body_args(closure, info, this, args);
    }
    let unbox = |raw: f64| {
        if is_int(raw) {
            (raw.to_bits() as u32 as i32) as f64
        } else {
            raw
        }
    };
    if n <= 16 {
        let mut converted = [0.0; 16];
        for (slot, raw) in converted.iter_mut().zip(args) {
            *slot = unbox(*raw);
        }
        return dispatch_body_args(closure, info, this, &converted[..n]);
    }
    let converted: Vec<f64> = args.iter().copied().map(unbox).collect();
    dispatch_body_args(closure, info, this, &converted)
}

/// Closure call with regular + spread args: `cb(reg0, reg1, ..., ...spread_arr)`.
///
/// Codegen lowers `closure(...args)` (or `closure(a, b, ...rest)`) at the
/// CallSpread arm by collecting regular arg slots into a stack buffer,
/// unboxing the spread source to an array handle, and calling this helper.
/// We concatenate `regular_args[0..regular_count]` with the array's
/// elements into a scratch buffer, then dispatch through
/// `js_closure_call_array`.
///
/// `closure_box` is a NaN-boxed closure value (the same shape that
/// `lower_expr` produces for a closure-typed expression). A null/undefined
/// box returns TAG_UNDEFINED.
#[no_mangle]
pub unsafe extern "C" fn js_closure_call_apply_with_spread(
    closure_box: f64,
    this: crate::closure::JsThis,
    regular_args: *const f64,
    regular_count: i64,
    spread_arr_handle: i64,
) -> f64 {
    use crate::array::ArrayHeader;

    let bits = closure_box.to_bits();
    let closure_ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const ClosureHeader;
    if closure_ptr.is_null() {
        throw_not_callable();
    }

    let reg_n = if regular_count < 0 {
        0
    } else {
        regular_count as usize
    };

    // #6518: resolve a push-grown array's forwarding stub (#233, the #6486
    // family) before reading length. In-tree codegen callsites pre-resolve
    // the spread source through `js_array_like_to_array` (whose real-Array
    // arm runs `clean_arr_ptr`), but this helper is `#[no_mangle]` and
    // declared to stdlib FFI — a caller passing a raw handle to a grown
    // array would read the forwarding pointer's bytes as the spread length.
    // Don't lean on upstream cleaning for memory safety here; the re-clean
    // on an already-resolved pointer is cheap.
    let arr = crate::array::clean_arr_ptr(spread_arr_handle as *const ArrayHeader);
    let spread_n: usize = if arr.is_null() {
        0
    } else {
        (*arr).length as usize
    };

    let total = reg_n + spread_n;

    // Small fast path: stack buffer for up to 16 args (matches js_closure_call16).
    let mut stack_buf: [f64; 16] = [0.0; 16];
    let mut heap_buf: Vec<f64>;
    // Spread slots are read per element via `js_array_get_f64`, not a raw
    // memcpy: a sparse array (length > capacity, far slots in
    // the named properties) legally passes `clean_arr_ptr`, so copying `length`
    // raw slots reads out of bounds (same rule as #6517's from-array
    // constructors). The accessor resolves far-index slots and reads holes
    // as undefined.
    let buf_ptr: *const f64 = if total <= 16 {
        if !regular_args.is_null() && reg_n > 0 {
            // GC_STORE_AUDIT(STACK): spread-call regular args copy into a temporary stack buffer.
            std::ptr::copy_nonoverlapping(regular_args, stack_buf.as_mut_ptr(), reg_n);
        }
        for i in 0..spread_n {
            // GC_STORE_AUDIT(STACK): spread args copy into a temporary stack buffer.
            stack_buf[reg_n + i] = crate::array::js_array_get_f64(arr, i as u32);
        }
        stack_buf.as_ptr()
    } else {
        heap_buf = vec![0.0; total];
        if !regular_args.is_null() && reg_n > 0 {
            // GC_STORE_AUDIT(STACK): regular args copy into a temporary native Vec buffer.
            std::ptr::copy_nonoverlapping(regular_args, heap_buf.as_mut_ptr(), reg_n);
        }
        for i in 0..spread_n {
            // GC_STORE_AUDIT(STACK): spread args copy into a temporary native Vec buffer.
            heap_buf[reg_n + i] = crate::array::js_array_get_f64(arr, i as u32);
        }
        heap_buf.as_ptr()
    };

    js_closure_call_array(closure_ptr as i64, this, buf_ptr, total as i64)
}

#[cfg(test)]
mod slice_body_tests {
    use super::*;

    extern "C" fn probe(_: *const ClosureHeader, this: JsThis, a: f64, b: f64) -> f64 {
        assert_eq!(this.as_f64(), 42.0);
        assert_eq!(a, 3.0);
        b
    }

    #[test]
    fn slice_body_padding_and_surplus_use_the_declared_signature() {
        let info = crate::fn_info!(probe, 2; plain());
        let closure = js_closure_alloc(info, 0);
        let this = JsThis::from_f64(42.0);
        unsafe {
            assert_eq!(
                call_compiled_body_this(closure, &*info, this, [3.0].as_ptr(), 1).to_bits(),
                crate::value::TAG_UNDEFINED
            );
            let args = [3.0, 5.0, 9.0];
            for n in [2, 3] {
                assert_eq!(
                    call_compiled_body_this(closure, &*info, this, args.as_ptr(), n),
                    5.0
                );
            }
            let args = [3.0; 2048];
            assert_eq!(
                call_compiled_body_this(closure, &*info, this, args.as_ptr(), args.len()),
                3.0
            );
        }
    }

    #[test]
    fn array_bridge_keeps_legacy_int32_conversion() {
        let info = crate::fn_info!(probe, 2; plain());
        let closure = js_closure_alloc(info, 0);
        let args = [
            f64::from_bits(crate::value::JSValue::int32(3).bits()),
            f64::from_bits(crate::value::JSValue::int32(5).bits()),
        ];
        assert_eq!(
            unsafe {
                js_closure_call_array(closure as i64, JsThis::from_f64(42.0), args.as_ptr(), 2)
            },
            5.0
        );
    }

    #[test]
    fn native_non_constructor_body_keeps_this_padding_and_surplus() {
        let info = crate::fn_info!(probe, 2; with_declared(2), with_flags(crate::closure::FN_BUILTIN | crate::closure::FN_NON_CONSTRUCTOR));
        let closure = js_closure_alloc(info, 0);
        let value = crate::value::js_nanbox_pointer(closure as i64);
        let args = [3.0, 5.0, 9.0];
        unsafe {
            assert_eq!(
                native_call_value_this(value, JsThis::from_f64(42.0), args.as_ptr(), 1).to_bits(),
                crate::value::TAG_UNDEFINED
            );
            assert_eq!(
                native_call_value_this(value, JsThis::from_f64(42.0), args.as_ptr(), 3),
                5.0
            );
        }
    }
}
