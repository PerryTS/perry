//! Proxy `[[Call]]` / `[[Construct]]` exotic behavior: the `apply` and
//! `construct` traps, their default-forwarding paths, and the
//! callable/constructor classification helpers shared by the Reflect trap
//! validators. Extracted from `proxy.rs` to keep it under the 2000-line gate
//! (split-large-files recipe). Pure relocation; behavior is unchanged.

use super::{
    closure_from, create_list_from_array_like, handler_trap, lookup, reflect_value_is_object,
    revoked_return, throw_type_error, value_display_string, POINTER_MASK, POINTER_TAG, PROXIES,
    TAG_NULL, TAG_UNDEFINED,
};
use crate::closure::js_closure_call3;

/// Is `value` a callable function value: a closure, a class-ref constructor, or
/// a (possibly callable) proxy? Distinct from `is_callable`, which treats *any*
/// pointer-tagged value as callable — that's too loose for trap validation,
/// where a present-but-non-callable trap (e.g. `apply: {}`) must throw a
/// `TypeError` rather than be silently invoked as a no-op.
pub(crate) fn is_callable_function(value: f64) -> bool {
    let bits = value.to_bits();
    // Class-ref constructors (INT32-tagged, top16 == 0x7FFE) are callable.
    if (bits >> 48) == 0x7FFE {
        return true;
    }
    // A proxy whose target is callable is itself callable.
    if lookup(value).is_some() {
        return true;
    }
    // A POINTER_TAG value is callable only if it points at a closure.
    if (bits & !POINTER_MASK) == POINTER_TAG {
        let raw = (bits & POINTER_MASK) as usize;
        return crate::closure::is_closure_ptr(raw);
    }
    false
}

pub(crate) fn is_constructor_function(value: f64) -> bool {
    if !is_callable_function(value) {
        return false;
    }
    if crate::object::builtin_closure_is_non_constructable_value(value) {
        return false;
    }
    // #2768: arrow functions have no [[Construct]]. The deep construct path
    // already rejects an arrow *target* ("Arrow function is not a
    // constructor"), but `Reflect.construct`'s up-front constructor checks —
    // for both the target and the `newTarget` operand — must reject them too.
    // Without this, `Reflect.construct(C, args, arrowFn)` silently proceeded
    // instead of throwing the spec TypeError (newTarget is never itself
    // constructed, so the deep path never fires for it).
    // A POINTER_TAG value is only a closure if `is_closure_ptr` confirms it —
    // a callable Proxy is also POINTER_TAG but its lower 48 bits are a proxy
    // id, not a `ClosureHeader*`, so `closure_is_arrow` (which dereferences the
    // header via `get_valid_func_ptr`) must not run on it. Mirror the guard in
    // `is_callable_function`.
    let bits = value.to_bits();
    if (bits & !POINTER_MASK) == POINTER_TAG {
        let raw = (bits & POINTER_MASK) as usize;
        if crate::closure::is_closure_ptr(raw)
            && crate::closure::closure_is_arrow(raw as *const crate::closure::ClosureHeader)
        {
            return false;
        }
    }
    true
}

/// Forward a `[[Call]]` to `target` (the default behavior when a proxy has no
/// `apply` trap). If `target` is itself a proxy, recurse so its own trap chain
/// runs; otherwise invoke the target through the canonical value-call path with
/// `this_arg` bound via `IMPLICIT_THIS`. Routing through `js_native_call_value`
/// (rather than calling the closure directly) also recovers built-in prototype
/// methods invoked as values — e.g. forwarding to `Object.prototype.hasOwnProperty`
/// re-dispatches by name with the receiver taken from `IMPLICIT_THIS`.
fn forward_apply(target: f64, this_arg: f64, args_array: f64) -> f64 {
    if lookup(target).is_some() {
        return js_proxy_apply(target, this_arg, args_array);
    }
    let args_bits = args_array.to_bits();
    let arr_ptr = (args_bits & POINTER_MASK) as *const crate::ArrayHeader;
    let len = if arr_ptr.is_null() {
        0
    } else {
        crate::array::js_array_length(arr_ptr) as usize
    };
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
    let prev = crate::object::js_implicit_this_set(this_arg);
    let result = unsafe { crate::closure::js_native_call_value(target, ptr, n) };
    crate::object::js_implicit_this_set(prev);
    result
}

/// `proxy(arg0, arg1)` / `p.call(thisArg, …)` / `Reflect.apply(p, thisArg, …)`.
///
/// Implements the Proxy `[[Call]]` exotic behavior (#3656):
///   * trap absent / `undefined` / `null` → forward `[[Call]]` to the target,
///     binding `thisArg`;
///   * trap present but not callable → `TypeError`;
///   * trap present → `Call(trap, handler, «target, thisArg, argArray»)` — the
///     handler is the trap's `this`, and the trap's return value is returned
///     verbatim (no fallback to the target).
///
/// `args_array` is an already-constructed Array JSValue (NaN-boxed).
#[no_mangle]
pub extern "C" fn js_proxy_apply(proxy_boxed: f64, this_arg: f64, args_array: f64) -> f64 {
    let id = match lookup(proxy_boxed) {
        Some(id) => id,
        None => return f64::from_bits(TAG_UNDEFINED),
    };
    let (target, handler, revoked) = PROXIES.with(|p| {
        p.borrow()
            .get(id as usize)
            .and_then(|o| o.as_ref())
            .map(|e| (e.target, e.handler, e.revoked))
            .unwrap_or((
                f64::from_bits(TAG_UNDEFINED),
                f64::from_bits(TAG_UNDEFINED),
                false,
            ))
    });
    if revoked {
        return revoked_return();
    }
    let trap = handler_trap(handler, "apply");
    let trap_bits = trap.to_bits();
    // GetMethod: a missing / undefined / null trap means "use the default" —
    // forward the call to the target's [[Call]].
    if trap_bits == TAG_UNDEFINED || trap_bits == TAG_NULL {
        return forward_apply(target, this_arg, args_array);
    }
    // Present-but-not-callable trap → TypeError.
    if !is_callable_function(trap) {
        return throw_type_error("proxy apply trap is not a function");
    }
    // Invoke the trap with the handler bound as `this` and the spec argument
    // list (target, thisArgument, argArray). Object-literal/free-function traps
    // read `this` from a closure slot and/or the IMPLICIT_THIS fallback, so we
    // set both — mirroring the `Reflect.get` accessor path.
    let rebound = crate::closure::clone_closure_rebind_this(trap_bits, handler);
    let closure = closure_from(f64::from_bits(rebound));
    if closure.is_null() {
        return throw_type_error("proxy apply trap is not a function");
    }
    let prev = crate::object::js_implicit_this_set(handler);
    let result = js_closure_call3(closure, target, this_arg, args_array);
    crate::object::js_implicit_this_set(prev);
    result
}

/// Forward a `[[Construct]]` to `target` (the default behavior when a proxy has
/// no `construct` trap). Recurses through proxy targets; otherwise constructs a
/// fresh instance from the target function value.
fn forward_construct(target: f64, args_array: f64, new_target: f64) -> f64 {
    if lookup(target).is_some() {
        return js_proxy_construct(target, args_array, new_target);
    }
    if !is_constructor_function(target) {
        return throw_type_error("target is not a constructor");
    }
    let buf = create_list_from_array_like(args_array);
    let (ptr, n) = if buf.is_empty() {
        (std::ptr::null::<f64>(), 0usize)
    } else {
        (buf.as_ptr(), buf.len())
    };
    unsafe { crate::object::js_new_function_construct_with_new_target(target, ptr, n, new_target) }
}

/// `new Proxy(...)` / `Reflect.construct(p, args, newTarget)`.
///
/// Implements the Proxy `[[Construct]]` exotic behavior (#3656):
///   * trap absent / `undefined` / `null` → forward `[[Construct]]` to the
///     target (recursing through proxy targets), threading `newTarget`;
///   * trap present but not callable → `TypeError`;
///   * trap present → `Call(trap, handler, «target, argArray, newTarget»)` with
///     the handler bound as `this`. The trap's result must be an Object, else
///     `TypeError`.
///
/// `new_target` defaults to the proxy itself when the caller passes
/// `undefined` (the `new Proxy(...)` path).
#[no_mangle]
pub extern "C" fn js_proxy_construct(proxy_boxed: f64, args_array: f64, new_target: f64) -> f64 {
    let id = match lookup(proxy_boxed) {
        Some(id) => id,
        None => return f64::from_bits(TAG_UNDEFINED),
    };
    let (target, handler, revoked) = PROXIES.with(|p| {
        p.borrow()
            .get(id as usize)
            .and_then(|o| o.as_ref())
            .map(|e| (e.target, e.handler, e.revoked))
            .unwrap_or((
                f64::from_bits(TAG_UNDEFINED),
                f64::from_bits(TAG_UNDEFINED),
                false,
            ))
    });
    if revoked {
        return revoked_return();
    }
    // Default newTarget to the proxy itself (spec: `new P(...)` passes the
    // constructor being invoked, which is the proxy).
    let nt = if new_target.to_bits() == TAG_UNDEFINED {
        proxy_boxed
    } else {
        new_target
    };
    let trap = handler_trap(handler, "construct");
    let trap_bits = trap.to_bits();
    if trap_bits == TAG_UNDEFINED || trap_bits == TAG_NULL {
        return forward_construct(target, args_array, nt);
    }
    if !is_callable_function(trap) {
        return throw_type_error("proxy construct trap is not a function");
    }
    let rebound = crate::closure::clone_closure_rebind_this(trap_bits, handler);
    let closure = closure_from(f64::from_bits(rebound));
    if closure.is_null() {
        return throw_type_error("proxy construct trap is not a function");
    }
    let prev = crate::object::js_implicit_this_set(handler);
    let result = js_closure_call3(closure, target, args_array, nt);
    crate::object::js_implicit_this_set(prev);
    // [[Construct]] must return an Object (spec step 9 of the construct trap).
    if !reflect_value_is_object(result) {
        // Node/V8 wording: `'construct' on proxy: trap returned non-object ('1')`.
        return throw_type_error(&format!(
            "'construct' on proxy: trap returned non-object ('{}')",
            value_display_string(result)
        ));
    }
    result
}
