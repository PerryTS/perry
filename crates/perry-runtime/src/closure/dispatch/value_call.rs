//! Dynamic value-call entry points: `js_native_call_value` (the generic
//! NaN-boxed callee dispatcher), the V8 trampoline bridge `js_closure_call_array`,
//! and the spread-apply bridge `js_closure_call_apply_with_spread`.

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
#[no_mangle]
pub unsafe extern "C" fn js_native_call_value(
    func_value: f64,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    use crate::value::JSValue;

    let jsval = JSValue::from_bits(func_value.to_bits());

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
        let this_arg = f64::from_bits(crate::value::TAG_UNDEFINED);
        return crate::proxy::js_proxy_apply(func_value, this_arg, arr_box);
    }

    // Dynamic `super()` for `class X extends <runtime value holding
    // events.EventEmitter>` (an import alias `import { EventEmitter as E }` or a
    // local `const E = EventEmitter`): the parent is a bound-native EventEmitter
    // export reached through a runtime value, so codegen's compile-time
    // extends-NAME machinery — which emits `js_event_emitter_subclass_init` for
    // the direct `class X extends EventEmitter` form (#5137) — never fires, and
    // `js_register_class_parent_dynamic` early-returns for bound native parents.
    // The dynamic super lowering (expr/this_super_call.rs) dispatches the parent
    // VALUE here with IMPLICIT_THIS bound to the fresh subclass instance. Install
    // the EventEmitter listener/emit methods onto that instance, exactly as the
    // direct form does, so `this.setMaxListeners(…)`/`.on`/`.emit` resolve.
    if let Some((module, method)) =
        unsafe { crate::object::bound_native_callable_module_and_method(func_value) }
    {
        if module.trim_start_matches("node:") == "events"
            && (method == "EventEmitter" || method == "EventEmitterAsyncResource")
        {
            let this_val = crate::object::js_implicit_this_get();
            if JSValue::from_bits(this_val.to_bits()).is_pointer() {
                return crate::node_stream::js_event_emitter_subclass_init(this_val);
            }
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
    // `js_native_call_method`, with the receiver taken from `IMPLICIT_THIS`.
    if let Some(result) =
        crate::object::try_dispatch_value_called_proto_method(closure, args_ptr, args_len)
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
    // own registry path via `lookup_closure_rest` which already pads, so we
    // skip the arity lookup when the rest registry has an entry.
    let func_ptr = get_valid_func_ptr(closure);
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
    let dispatch_args_len = if !func_ptr.is_null() && lookup_closure_rest(func_ptr).is_none() {
        match lookup_closure_arity(func_ptr) {
            Some(declared) if (declared as usize) > args_len => declared as usize,
            _ => args_len,
        }
    } else {
        args_len
    };

    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let arg_at = |i: usize| -> f64 {
        if i < args_len && !args_ptr.is_null() {
            unsafe { *args_ptr.add(i) }
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

    // A closure with a registered rest param must bundle EVERY argument into
    // its rest array. The per-arity `match` below caps at `js_closure_call8`
    // (passing only `arg_at(0..7)`), so a rest closure invoked with >8 args
    // (e.g. `new Temporal.Duration(y,mo,w,d,h,mi,s,ms,us,ns)` — 10 positional
    // args) would silently drop the overflow. Route through the rest-bundler
    // with the full slice up front. (The arity-specific `js_closure_callN`
    // helpers do their own rest check, but only see the truncated arg list.)
    if !func_ptr.is_null() {
        if let Some((fixed_arity, synth)) = lookup_closure_rest_full(func_ptr) {
            let all: Vec<f64> = (0..args_len).map(arg_at).collect();
            return dispatch_rest_bundled(closure, func_ptr, &all, fixed_arity, synth);
        }
    }

    // Call with the appropriate arity
    match dispatch_args_len {
        0 => js_closure_call0(closure),
        1 => js_closure_call1(closure, arg_at(0)),
        2 => js_closure_call2(closure, arg_at(0), arg_at(1)),
        3 => js_closure_call3(closure, arg_at(0), arg_at(1), arg_at(2)),
        4 => js_closure_call4(closure, arg_at(0), arg_at(1), arg_at(2), arg_at(3)),
        5 => js_closure_call5(
            closure,
            arg_at(0),
            arg_at(1),
            arg_at(2),
            arg_at(3),
            arg_at(4),
        ),
        6 => js_closure_call6(
            closure,
            arg_at(0),
            arg_at(1),
            arg_at(2),
            arg_at(3),
            arg_at(4),
            arg_at(5),
        ),
        7 => js_closure_call7(
            closure,
            arg_at(0),
            arg_at(1),
            arg_at(2),
            arg_at(3),
            arg_at(4),
            arg_at(5),
            arg_at(6),
        ),
        8 => js_closure_call8(
            closure,
            arg_at(0),
            arg_at(1),
            arg_at(2),
            arg_at(3),
            arg_at(4),
            arg_at(5),
            arg_at(6),
            arg_at(7),
        ),
        // Arities 9..=16 must each dispatch through their own
        // `js_closure_call{N}` so the func-ptr is transmuted to a signature
        // with the matching number of `f64` params. Collapsing these into
        // `js_closure_call8` (the pre-fix `_` arm) silently dropped args 9+ for
        // any closure VALUE / method invoked with >8 args — the codegen-side
        // wrapper now carries up to 16 params (see artifacts.rs), so the runtime
        // dispatch must reach them. >16 args fall back to the array path.
        9 => js_closure_call9(
            closure,
            arg_at(0),
            arg_at(1),
            arg_at(2),
            arg_at(3),
            arg_at(4),
            arg_at(5),
            arg_at(6),
            arg_at(7),
            arg_at(8),
        ),
        10 => js_closure_call10(
            closure,
            arg_at(0),
            arg_at(1),
            arg_at(2),
            arg_at(3),
            arg_at(4),
            arg_at(5),
            arg_at(6),
            arg_at(7),
            arg_at(8),
            arg_at(9),
        ),
        11 => js_closure_call11(
            closure,
            arg_at(0),
            arg_at(1),
            arg_at(2),
            arg_at(3),
            arg_at(4),
            arg_at(5),
            arg_at(6),
            arg_at(7),
            arg_at(8),
            arg_at(9),
            arg_at(10),
        ),
        12 => js_closure_call12(
            closure,
            arg_at(0),
            arg_at(1),
            arg_at(2),
            arg_at(3),
            arg_at(4),
            arg_at(5),
            arg_at(6),
            arg_at(7),
            arg_at(8),
            arg_at(9),
            arg_at(10),
            arg_at(11),
        ),
        13 => js_closure_call13(
            closure,
            arg_at(0),
            arg_at(1),
            arg_at(2),
            arg_at(3),
            arg_at(4),
            arg_at(5),
            arg_at(6),
            arg_at(7),
            arg_at(8),
            arg_at(9),
            arg_at(10),
            arg_at(11),
            arg_at(12),
        ),
        14 => js_closure_call14(
            closure,
            arg_at(0),
            arg_at(1),
            arg_at(2),
            arg_at(3),
            arg_at(4),
            arg_at(5),
            arg_at(6),
            arg_at(7),
            arg_at(8),
            arg_at(9),
            arg_at(10),
            arg_at(11),
            arg_at(12),
            arg_at(13),
        ),
        15 => js_closure_call15(
            closure,
            arg_at(0),
            arg_at(1),
            arg_at(2),
            arg_at(3),
            arg_at(4),
            arg_at(5),
            arg_at(6),
            arg_at(7),
            arg_at(8),
            arg_at(9),
            arg_at(10),
            arg_at(11),
            arg_at(12),
            arg_at(13),
            arg_at(14),
        ),
        16 => js_closure_call16(
            closure,
            arg_at(0),
            arg_at(1),
            arg_at(2),
            arg_at(3),
            arg_at(4),
            arg_at(5),
            arg_at(6),
            arg_at(7),
            arg_at(8),
            arg_at(9),
            arg_at(10),
            arg_at(11),
            arg_at(12),
            arg_at(13),
            arg_at(14),
            arg_at(15),
        ),
        // >16 args: marshal into a stack buffer and dispatch via the variadic
        // array path (which itself fans back out to `js_closure_call{N}`).
        _ => {
            let mut buf: Vec<f64> = Vec::with_capacity(dispatch_args_len);
            for i in 0..dispatch_args_len {
                buf.push(arg_at(i));
            }
            js_closure_call_array(closure as i64, buf.as_ptr(), buf.len() as i64)
        }
    }
}

/// Adapter for V8's `native_callback_trampoline` (perry-jsruntime).
///
/// `js_create_callback(func_ptr, closure_env, param_count)` registers a JS
/// callable whose trampoline invokes `func_ptr(closure_env, args_ptr,
/// args_len)`. Perry closure bodies have signature
/// `(closure_ptr, arg0, arg1, ...)` per arity instead, so the codegen
/// arm for `Expr::JsCreateCallback` (issue #248 Phase 2B) passes
/// `js_closure_call_array` as the trampoline `func_ptr` and the raw
/// `*const ClosureHeader` (NaN-boxing stripped) as `closure_env`. The
/// trampoline then ends up calling THIS function, which dispatches to
/// the right `js_closure_callN` per `args_len`.
///
/// Mirrors `js_native_call_value` exactly but takes an i64 closure
/// pointer (already unboxed) instead of an f64 NaN-boxed value, so the
/// SysV-x64 / Win64 first-arg register lands in rdi/rcx (integer)
/// rather than xmm0 — matching the trampoline's `extern "C"` int-arg
/// expectation.
#[no_mangle]
pub unsafe extern "C" fn js_closure_call_array(
    closure_env: i64,
    args_ptr: *const f64,
    args_len: i64,
) -> f64 {
    let closure = closure_env as *const ClosureHeader;
    if closure.is_null() {
        throw_not_callable();
    }
    let n = if args_len < 0 { 0 } else { args_len as usize };

    // Issue #653 followup: route through `dispatch_rest_bundled` directly
    // when the closure body has a registered rest param, before falling
    // through to the per-arity `js_closure_callN` dispatchers. Pre-fix,
    // `js_closure_call7` through `js_closure_call16` skipped the
    // rest-bundling path entirely and trampolined the args list straight
    // through `mem::transmute`. With a wrapper registered for the rest
    // param at `fixed_arity = 2` (e.g. `function h(a, b, ...rest)`),
    // calling with 8 total args matched the call8 arm and called the
    // wrapper with 9 doubles when the wrapper signature is 4 doubles —
    // the receiver's `rest` parameter then read whatever happened to be
    // in the call's overflow registers, which the wrapper passed
    // through to the underlying user function as the rest array. Result:
    // `rest.length` came back as 0 because the actual rest array was
    // never built. Centralizing the dispatch here keeps the `callN`
    // arity-specific paths sound for direct-callee dispatch (which is
    // the dominant case for closure literals stored as locals) while
    // making the spread path correct for arities ≥ 7. The bound-method
    // routing has its own path inside `js_closure_callN` and isn't
    // affected here — we never see BOUND_METHOD_FUNC_PTR through this
    // entry because `js_closure_call_apply_with_spread`'s caller always
    // resolves a real closure pointer first.
    let fp_for_rest = get_valid_func_ptr(closure);
    if let Some((fixed_arity, synth)) = lookup_closure_rest_full(fp_for_rest) {
        let mut tmp: Vec<f64> = Vec::with_capacity(n);
        if !args_ptr.is_null() && n > 0 {
            for i in 0..n {
                let raw = *args_ptr.add(i);
                let bits = raw.to_bits();
                // Same INT32_TAG unboxing the per-arity dispatchers do
                // below — keep the body's `fadd` arithmetic working when
                // the args came from `v8_to_native`. EXCEPT class refs: a
                // class/class-prototype ref is ALSO 0x7FFE-tagged (the cid in
                // the low 32 bits), so unboxing it would turn an imported
                // class passed through `f(...args)` into the plain number
                // `cid` — breaking `typeof Filter === 'function'` in
                // `@nestjs` `@UseFilters`/`@UseGuards` metadata. Skip the
                // unbox for a registered class-ref (never a v8 int32).
                let unboxed = if (bits & 0xFFFF_0000_0000_0000) == 0x7FFE_0000_0000_0000
                    && crate::object::class_ref_id(raw).is_none()
                {
                    ((bits & 0xFFFF_FFFF) as i32) as f64
                } else {
                    raw
                };
                tmp.push(unboxed);
            }
        }
        return dispatch_rest_bundled(closure, fp_for_rest, &tmp, fixed_arity, synth);
    }
    // Perry's closure-body arithmetic uses plain `fadd`/`fmul`/etc on
    // f64 inputs and assumes its arguments arrive as plain doubles, not
    // NaN-boxed values. perry-jsruntime's `v8_to_native` (bridge.rs:215)
    // NaN-boxes JS integers with INT32_TAG=0x7FFE. If we passed those
    // bits straight through, the closure body's `fadd` would produce a
    // NaN (whose payload happens to look like one of the operands when
    // re-decoded by `console.log`'s tag-aware unbox — which is why
    // `(a, b) => a + b` with `cb(10, 20)` returned 10 instead of 30
    // pre-fix). Unbox at the dispatch boundary so the body sees a
    // plain `20.0` not the NaN-boxed `0x7FFE_0000_0000_0014`. JS
    // doubles (non-int32) already arrive as plain f64 from
    // `v8_to_native`; only the INT32_TAG case needs unboxing here.
    let a = |i: usize| {
        if args_ptr.is_null() {
            return 0.0;
        }
        let raw = *args_ptr.add(i);
        let bits = raw.to_bits();
        // Skip the unbox for a registered class-ref: it is 0x7FFE-tagged like
        // a v8 int32 but must stay a class ref (see the rest-bundled loop
        // above) so an imported class spread through `f(...args)` keeps
        // `typeof === 'function'`.
        if (bits & 0xFFFF_0000_0000_0000) == 0x7FFE_0000_0000_0000
            && crate::object::class_ref_id(raw).is_none()
        {
            let int_val = (bits & 0xFFFF_FFFF) as i32;
            return int_val as f64;
        }
        raw
    };
    match n {
        0 => js_closure_call0(closure),
        1 => js_closure_call1(closure, a(0)),
        2 => js_closure_call2(closure, a(0), a(1)),
        3 => js_closure_call3(closure, a(0), a(1), a(2)),
        4 => js_closure_call4(closure, a(0), a(1), a(2), a(3)),
        5 => js_closure_call5(closure, a(0), a(1), a(2), a(3), a(4)),
        6 => js_closure_call6(closure, a(0), a(1), a(2), a(3), a(4), a(5)),
        7 => js_closure_call7(closure, a(0), a(1), a(2), a(3), a(4), a(5), a(6)),
        8 => js_closure_call8(closure, a(0), a(1), a(2), a(3), a(4), a(5), a(6), a(7)),
        9 => js_closure_call9(
            closure,
            a(0),
            a(1),
            a(2),
            a(3),
            a(4),
            a(5),
            a(6),
            a(7),
            a(8),
        ),
        10 => js_closure_call10(
            closure,
            a(0),
            a(1),
            a(2),
            a(3),
            a(4),
            a(5),
            a(6),
            a(7),
            a(8),
            a(9),
        ),
        11 => js_closure_call11(
            closure,
            a(0),
            a(1),
            a(2),
            a(3),
            a(4),
            a(5),
            a(6),
            a(7),
            a(8),
            a(9),
            a(10),
        ),
        12 => js_closure_call12(
            closure,
            a(0),
            a(1),
            a(2),
            a(3),
            a(4),
            a(5),
            a(6),
            a(7),
            a(8),
            a(9),
            a(10),
            a(11),
        ),
        13 => js_closure_call13(
            closure,
            a(0),
            a(1),
            a(2),
            a(3),
            a(4),
            a(5),
            a(6),
            a(7),
            a(8),
            a(9),
            a(10),
            a(11),
            a(12),
        ),
        14 => js_closure_call14(
            closure,
            a(0),
            a(1),
            a(2),
            a(3),
            a(4),
            a(5),
            a(6),
            a(7),
            a(8),
            a(9),
            a(10),
            a(11),
            a(12),
            a(13),
        ),
        15 => js_closure_call15(
            closure,
            a(0),
            a(1),
            a(2),
            a(3),
            a(4),
            a(5),
            a(6),
            a(7),
            a(8),
            a(9),
            a(10),
            a(11),
            a(12),
            a(13),
            a(14),
        ),
        16 => js_closure_call16(
            closure,
            a(0),
            a(1),
            a(2),
            a(3),
            a(4),
            a(5),
            a(6),
            a(7),
            a(8),
            a(9),
            a(10),
            a(11),
            a(12),
            a(13),
            a(14),
            a(15),
        ),
        // #3527: arities above 16 can't go through a fixed per-arity
        // `js_closure_callN` (none exist past 16). Build the full unboxed
        // arg slice and dispatch through the strategy resolver so the
        // closure body is called with ALL its args (the old `_ =>
        // js_closure_call16(...)` silently dropped args 16.. — breaking
        // qs's recursive `stringify`, which self-calls with 18 args). For
        // a plain (Direct) closure with no registered rest/arity, dispatch
        // through `dispatch_with_arity` with the provided count so the body
        // is transmuted to its real N-arg signature.
        _ => {
            let mut full: Vec<f64> = Vec::with_capacity(n);
            for i in 0..n {
                full.push(a(i));
            }
            let func_ptr = get_valid_func_ptr(closure);
            if func_ptr.is_null() {
                throw_not_callable();
            }
            if let Some(result) = dispatch_registered_call(closure, func_ptr, &full) {
                return result;
            }
            if let Some(result) =
                dispatch_rest_or_declared_arity(closure, func_ptr, &full, n as u32)
            {
                return result;
            }
            // Direct closure: declared arity == provided count. Reuse the
            // arity dispatcher (it transmutes to the concrete N-arg fn and
            // forwards the slice unchanged when provided == declared).
            dispatch_with_arity(closure, func_ptr, &full, n as u32)
        }
    }
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
    // ARRAY_NAMED_PROPS) legally passes `clean_arr_ptr`, so copying `length`
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

    js_closure_call_array(closure_ptr as i64, buf_ptr, total as i64)
}
