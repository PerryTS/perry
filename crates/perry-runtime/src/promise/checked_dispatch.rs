//! Codegen entries for `.then`/`.catch`/`.finally` calls whose receiver is
//! STATICALLY known to be a Promise (`is_promise_expr` in perry-codegen).
//! Split out of `then.rs` to stay under the file-size cap (#5849).
//!
//! Mirrors the own-property guard the dynamic dispatch path already applies
//! in `object/native_call_method/primitive_methods.rs::dispatch_primitive`:
//! an own "then" expando must be invoked directly (`Invoke(promise, "then",
//! args)` per spec — the call site resolves the method via `Get` before
//! `Promise.prototype.then`'s own algorithm ever runs), an own "constructor"
//! expando must route through the full SpeciesConstructor-aware thunk, and
//! everything else stays on the native fast path (test262
//! then/ctor-access-count, resolve/resolve-prms-cstm-then). `.catch`/
//! `.finally` always end up invoking "then" (per spec, `catch(r)` is sugar
//! for `then(undefined, r)`), so either own-property hit routes them
//! through the existing thunks, which already funnel through
//! `call_receiver_then`.
//!
//! These also replace codegen's blind `unbox_to_i64` of the handler
//! arguments: `undefined`/`null`/numbers must resolve to "no handler" (the
//! IsCallable-false default), not a garbage low address reinterpreted as a
//! `ClosureHeader*` (test262 then/S25.4.5.3_A4.1_T1/T2) — `arg_to_closure`
//! tag-checks before treating the bits as a pointer.

use super::*;
use then::arg_to_closure;

#[no_mangle]
pub extern "C" fn js_promise_then_checked(
    promise_val: f64,
    on_fulfilled: f64,
    on_rejected: f64,
) -> f64 {
    let promise_addr = (promise_val.to_bits() & crate::value::POINTER_MASK) as usize;
    if promise_has_own_property(promise_addr, "then") {
        let own_then = unsafe {
            crate::object::exotic_expando::exotic_get_own_property(
                promise_addr,
                crate::object::exotic_expando::ExoticKind::Promise,
                "then",
                promise_val,
            )
        }
        .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
        if !spec_combinators::is_callable_value(own_then) {
            let msg = b"'then' property on Promise is not callable";
            let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
            let err = crate::error::js_typeerror_new(msg_str);
            crate::exception::js_throw(f64::from_bits(
                crate::value::JSValue::pointer(err as *const u8).bits(),
            ));
        }
        let args = [on_fulfilled, on_rejected];
        let prev = crate::object::js_implicit_this_set(promise_val);
        let result =
            unsafe { crate::closure::js_native_call_value(own_then, args.as_ptr(), args.len()) };
        crate::object::js_implicit_this_set(prev);
        return result;
    }
    if promise_has_own_constructor(promise_addr)
        || subclass::subclass_backing_promise(promise_val).is_some()
    {
        // Own `constructor` override OR a `class X extends Promise` instance:
        // route through the SpeciesConstructor-aware thunk, which unwraps the
        // backing cell and reads `this.constructor` for species chaining.
        let prev = crate::object::js_implicit_this_set(promise_val);
        let result = promise_prototype_then_thunk(std::ptr::null(), on_fulfilled, on_rejected);
        crate::object::js_implicit_this_set(prev);
        return result;
    }
    let promise = promise_addr as *mut Promise;
    box_promise_ptr(js_promise_then(
        promise,
        arg_to_closure(on_fulfilled),
        arg_to_closure(on_rejected),
    ))
}

#[no_mangle]
pub extern "C" fn js_promise_catch_checked(promise_val: f64, on_rejected: f64) -> f64 {
    let promise_addr = (promise_val.to_bits() & crate::value::POINTER_MASK) as usize;
    if promise_has_own_property(promise_addr, "then")
        || promise_has_own_constructor(promise_addr)
        || subclass::subclass_backing_promise(promise_val).is_some()
    {
        let prev = crate::object::js_implicit_this_set(promise_val);
        let result = promise_prototype_catch_thunk(std::ptr::null(), on_rejected);
        crate::object::js_implicit_this_set(prev);
        return result;
    }
    let promise = promise_addr as *mut Promise;
    box_promise_ptr(js_promise_catch(promise, arg_to_closure(on_rejected)))
}

#[no_mangle]
pub extern "C" fn js_promise_finally_checked(promise_val: f64, on_finally: f64) -> f64 {
    let promise_addr = (promise_val.to_bits() & crate::value::POINTER_MASK) as usize;
    if promise_has_own_property(promise_addr, "then")
        || promise_has_own_constructor(promise_addr)
        || subclass::subclass_backing_promise(promise_val).is_some()
    {
        let prev = crate::object::js_implicit_this_set(promise_val);
        let result = promise_prototype_finally_thunk(std::ptr::null(), on_finally);
        crate::object::js_implicit_this_set(prev);
        return result;
    }
    let promise = promise_addr as *mut Promise;
    box_promise_ptr(js_promise_finally(promise, arg_to_closure(on_finally)))
}

/// Codegen-safe conversion of a `.then`/`.catch`/`.finally` handler argument
/// (still boxed) to a `ClosurePtr`, exposed for the codegen fused
/// `Promise.resolve(x).then(cb_f, cb_e)` fast path
/// (`js_promise_resolved_then`), which — unlike the entries above — takes
/// already-unboxed `ClosurePtr` args. Delegates to `arg_to_closure`'s tag
/// check: `undefined`/`null`/numbers become `0` (null / "no handler"), never
/// a garbage low address reinterpreted as a `ClosureHeader*` (the naive
/// `unbox_to_i64` codegen helper this replaces masked the low 48 bits
/// unconditionally, turning e.g. `undefined`'s `0x7FFC_0000_0000_0001` into
/// address `1`).
#[no_mangle]
pub extern "C" fn js_promise_closure_arg(value: f64) -> i64 {
    arg_to_closure(value) as i64
}
