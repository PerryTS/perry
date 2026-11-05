//! Proxy `[[Call]]` / `[[Construct]]` exotic behavior: the `apply` and
//! `construct` traps, their default-forwarding paths, and the
//! callable/constructor classification helpers shared by the Reflect trap
//! validators. Extracted from `proxy.rs` to keep it under the 2000-line gate
//! (split-large-files recipe). Proxy calls preserve creation-time callability.

use super::{
    closure_from, create_list_from_array_like, entry, handler_trap, lookup,
    pin_proxy_for_native_call, reflect_value_is_object, revoked_return, throw_type_error,
    value_display_string, POINTER_MASK, POINTER_TAG, TAG_NULL, TAG_UNDEFINED,
};
use crate::closure::js_closure_call3;

/// Forward a `[[Call]]` to `target` (the default behavior when a proxy has no
/// `apply` trap). If `target` is itself a proxy, recurse so its own trap chain
/// runs; otherwise invoke the target through the canonical value-call path with
/// `this_arg` as its receiver. Routing through the value-call path (rather than
/// calling the closure directly) also recovers built-in prototype methods
/// invoked as values — e.g. forwarding to `Object.prototype.hasOwnProperty`
/// re-dispatches by name with `this_arg` as the receiver.
fn forward_apply(target: f64, this_arg: f64, args_array: f64) -> f64 {
    if lookup(target).is_some() {
        return js_proxy_apply(target, this_arg, args_array);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(target);
    let this_arg = scope.root_nanbox_f64(this_arg);
    let args_array = scope.root_nanbox_f64(args_array);
    let args_bits = args_array.get_nanbox_f64().to_bits();
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
    // Keep the native argument owner above the trap. A JS longjmp would
    // otherwise bypass Vec::drop on every throwing forwarded proxy call.
    let result = crate::exception::catch_js_throw(|| unsafe {
        crate::closure::native_call_value_this(
            target.get_nanbox_f64(),
            crate::closure::JsThis::from_f64(this_arg.get_nanbox_f64()),
            ptr,
            n,
        )
    });
    drop(buf);
    match result {
        Ok(value) => value,
        Err(error) => crate::exception::js_throw(error),
    }
}

/// Call a Proxy VALUE as a function with an explicit `this` and a plain
/// argument slice — the `Call(F, V, args)` shape that runtime sites use when a
/// proxy turns up where a raw `ClosureHeader*` was expected (#6320: a Proxy
/// stored at `obj[Symbol.toPrimitive]`, or used as a JSX function component).
///
/// [`js_proxy_apply`] takes the spec `argArray`; this builds it, keeping every
/// operand rooted across the allocations so a GC in `js_array_push_f64` cannot
/// invalidate an already-boxed argument.
pub fn call_proxy_value_with_this(proxy: f64, this_arg: f64, args: &[f64]) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let proxy_h = scope.root_nanbox_f64(proxy);
    let this_h = scope.root_nanbox_f64(this_arg);
    let arg_handles: Vec<_> = args.iter().map(|a| scope.root_nanbox_f64(*a)).collect();
    let result = crate::exception::catch_js_throw(|| {
        let arr_h = scope
            .root_nanbox_u64(POINTER_TAG | (crate::array::js_array_alloc(0) as u64 & POINTER_MASK));
        for h in &arg_handles {
            let arr = (arr_h.get_nanbox_u64() & POINTER_MASK) as *mut crate::ArrayHeader;
            let grown = crate::array::js_array_push_f64(arr, h.get_nanbox_f64());
            arr_h.set_nanbox_u64(POINTER_TAG | (grown as u64 & POINTER_MASK));
        }
        js_proxy_apply(
            proxy_h.get_nanbox_f64(),
            this_h.get_nanbox_f64(),
            f64::from_bits(arr_h.get_nanbox_u64()),
        )
    });
    drop(arg_handles);
    match result {
        Ok(value) => value,
        Err(error) => crate::exception::js_throw(error),
    }
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
    let _proxy_pin = pin_proxy_for_native_call(proxy_boxed);
    let scope = crate::gc::RuntimeHandleScope::new();
    let this_arg = scope.root_nanbox_f64(this_arg);
    let args_array = scope.root_nanbox_f64(args_array);
    let id = match lookup(proxy_boxed) {
        Some(id) => id,
        None => return f64::from_bits(TAG_UNDEFINED),
    };
    // A Proxy has [[Call]] only when its target was callable at creation.
    // An apply trap cannot make an ordinary object callable.
    if !super::proxy_wraps_callable(proxy_boxed) {
        return throw_type_error("value is not a function");
    }
    let (target, handler, revoked) = entry(id)
        .map(|e| (e.target, e.handler, e.revoked))
        .unwrap_or((
            f64::from_bits(TAG_UNDEFINED),
            f64::from_bits(TAG_UNDEFINED),
            false,
        ));
    if revoked {
        return revoked_return();
    }
    let target = scope.root_nanbox_f64(target);
    let handler = scope.root_nanbox_f64(handler);
    let trap = scope.root_nanbox_f64(handler_trap(handler.get_nanbox_f64(), "apply"));
    let trap_bits = trap.get_nanbox_f64().to_bits();
    // GetMethod: a missing / undefined / null trap means "use the default" —
    // forward the call to the target's [[Call]].
    if trap_bits == TAG_UNDEFINED || trap_bits == TAG_NULL {
        return forward_apply(
            target.get_nanbox_f64(),
            this_arg.get_nanbox_f64(),
            args_array.get_nanbox_f64(),
        );
    }
    // Present-but-not-callable trap → TypeError.
    if !is_callable_function(trap.get_nanbox_f64()) {
        return throw_type_error("proxy apply trap is not a function");
    }
    if lookup(trap.get_nanbox_f64()).is_some() {
        return call_proxy_value_with_this(
            trap.get_nanbox_f64(),
            handler.get_nanbox_f64(),
            &[
                target.get_nanbox_f64(),
                this_arg.get_nanbox_f64(),
                args_array.get_nanbox_f64(),
            ],
        );
    }
    // Invoke the trap with the handler bound as `this` and the spec argument
    // list (target, thisArgument, argArray). Object-literal/free-function traps
    // read `this` from a closure slot and/or the `this` parameter, so we
    // supply both — mirroring the `Reflect.get` accessor path.
    let rebound = crate::closure::clone_closure_rebind_this(trap_bits, handler.get_nanbox_f64());
    let closure = closure_from(f64::from_bits(rebound));
    if closure.is_null() {
        return throw_type_error("proxy apply trap is not a function");
    }
    let result = crate::exception::catch_js_throw(|| {
        js_closure_call3(
            closure,
            crate::closure::JsThis::from_f64(handler.get_nanbox_f64()),
            target.get_nanbox_f64(),
            this_arg.get_nanbox_f64(),
            args_array.get_nanbox_f64(),
        )
    });
    match result {
        Ok(value) => value,
        Err(error) => crate::exception::js_throw(error),
    }
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
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(target);
    let new_target = scope.root_nanbox_f64(new_target);
    let args_array = scope.root_nanbox_f64(args_array);
    let buf = create_list_from_array_like(args_array.get_nanbox_f64());
    let result = crate::exception::catch_js_throw(|| {
        crate::object::construct_rooted_arguments(
            target.get_nanbox_f64(),
            &buf,
            new_target.get_nanbox_f64(),
        )
    });
    drop(buf);
    match result {
        Ok(value) => value,
        Err(error) => crate::exception::js_throw(error),
    }
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
    let _proxy_pin = pin_proxy_for_native_call(proxy_boxed);
    let id = match lookup(proxy_boxed) {
        Some(id) => id,
        None => return f64::from_bits(TAG_UNDEFINED),
    };
    let (target, handler, revoked) = entry(id)
        .map(|e| (e.target, e.handler, e.revoked))
        .unwrap_or((
            f64::from_bits(TAG_UNDEFINED),
            f64::from_bits(TAG_UNDEFINED),
            false,
        ));
    if revoked {
        return revoked_return();
    }
    // Default newTarget to the proxy itself (spec: `new P(...)` passes the
    // constructor being invoked, which is the proxy).
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(target);
    let handler = scope.root_nanbox_f64(handler);
    let args_array = scope.root_nanbox_f64(args_array);
    let nt = scope.root_nanbox_f64(if new_target.to_bits() == TAG_UNDEFINED {
        proxy_boxed
    } else {
        new_target
    });
    let trap = scope.root_nanbox_f64(handler_trap(handler.get_nanbox_f64(), "construct"));
    if matches!(trap.get_nanbox_f64().to_bits(), TAG_UNDEFINED | TAG_NULL) {
        return forward_construct(
            target.get_nanbox_f64(),
            args_array.get_nanbox_f64(),
            nt.get_nanbox_f64(),
        );
    }
    if !is_callable_function(trap.get_nanbox_f64()) {
        return throw_type_error("proxy construct trap is not a function");
    }
    let result = if lookup(trap.get_nanbox_f64()).is_some() {
        call_proxy_value_with_this(
            trap.get_nanbox_f64(),
            handler.get_nanbox_f64(),
            &[
                target.get_nanbox_f64(),
                args_array.get_nanbox_f64(),
                nt.get_nanbox_f64(),
            ],
        )
    } else {
        let rebound = crate::closure::clone_closure_rebind_this(
            trap.get_nanbox_f64().to_bits(),
            handler.get_nanbox_f64(),
        );
        let closure = closure_from(f64::from_bits(rebound));
        if closure.is_null() {
            return throw_type_error("proxy construct trap is not a function");
        }
        let result = crate::exception::catch_js_throw(|| {
            js_closure_call3(
                closure,
                crate::closure::JsThis::from_f64(handler.get_nanbox_f64()),
                target.get_nanbox_f64(),
                args_array.get_nanbox_f64(),
                nt.get_nanbox_f64(),
            )
        });
        match result {
            Ok(value) => value,
            Err(error) => crate::exception::js_throw(error),
        }
    };
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

#[cfg(test)]
mod callability_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    per_test_global! {
        static APPLY_CALLS: AtomicUsize = AtomicUsize::new(0);
    }

    extern "C" fn apply_trap(
        _: *const crate::closure::ClosureHeader,
        _: crate::closure::JsThis,
        _: f64,
        _: f64,
        _: f64,
    ) -> f64 {
        APPLY_CALLS.fetch_add(1, Ordering::Relaxed);
        999.0
    }

    #[test]
    fn value_call_bridges_reject_a_noncallable_proxy_before_invoking_its_apply_trap() {
        crate::gc::initialize_runtime_fixture();
        APPLY_CALLS.store(0, Ordering::Relaxed);
        let scope = crate::gc::RuntimeHandleScope::new();
        let target = scope.root_nanbox_u64(
            crate::value::JSValue::pointer(crate::object::js_object_alloc(0, 0).cast()).bits(),
        );
        let handler = scope.root_nanbox_u64(
            crate::value::JSValue::pointer(crate::object::js_object_alloc(0, 0).cast()).bits(),
        );
        let trap = scope.root_nanbox_u64(
            crate::value::JSValue::pointer(crate::closure::js_closure_alloc(
                crate::fn_info!(apply_trap, 3),
                0,
            ) as *const u8)
            .bits(),
        );
        let key = scope.root_nanbox_u64(
            crate::value::STRING_TAG
                | (crate::string::js_string_from_bytes(b"apply".as_ptr(), 5) as u64
                    & crate::value::POINTER_MASK),
        );
        crate::object::js_object_set_field_by_name(
            (handler.get_nanbox_u64() & crate::value::POINTER_MASK) as *mut crate::ObjectHeader,
            (key.get_nanbox_u64() & crate::value::POINTER_MASK) as *const crate::StringHeader,
            trap.get_nanbox_f64(),
        );
        assert_eq!(
            handler_trap(handler.get_nanbox_f64(), "apply").to_bits(),
            trap.get_nanbox_u64(),
            "the fixture must install its callable apply trap",
        );
        let proxy = scope.root_nanbox_f64(crate::proxy::js_proxy_new(
            target.get_nanbox_f64(),
            handler.get_nanbox_f64(),
        ));
        assert!(!crate::proxy::proxy_wraps_callable(proxy.get_nanbox_f64()));
        let result = crate::exception::catch_js_throw(|| {
            call_proxy_value_with_this(proxy.get_nanbox_f64(), f64::from_bits(TAG_UNDEFINED), &[])
        });
        assert!(result.is_err(), "an object Proxy must throw when called");
        let result = crate::exception::catch_js_throw(|| unsafe {
            crate::closure::native_call_value_this(
                proxy.get_nanbox_f64(),
                crate::closure::JsThis::from_f64(f64::from_bits(TAG_UNDEFINED)),
                std::ptr::null(),
                0,
            )
        });
        assert!(
            result.is_err(),
            "generic calls must also reject an object Proxy"
        );
        assert_eq!(APPLY_CALLS.load(Ordering::Relaxed), 0);
    }
}

pub(crate) use crate::object::property_ops::callable::{
    is_callable_function, is_constructor_function,
};
