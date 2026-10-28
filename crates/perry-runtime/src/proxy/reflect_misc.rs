//! Reflect.* entry points + proxy [[GetPrototypeOf]] — extracted from
//! `proxy.rs` to keep it under the 2000-line gate (split-large-files
//! recipe). Pure relocation; `use super::*` preserves visibility of the
//! parent's private registry helpers.

use super::*;

pub(crate) unsafe fn proxy_define_own_property_decoded(
    scope: &crate::gc::RuntimeHandleScope,
    receiver: &crate::gc::RuntimeHandle<'_>,
    key: &crate::gc::RuntimeHandle<'_>,
    descriptor: &crate::object::object_ops::DescView<'_>,
) -> bool {
    let obj = f64::from_bits(receiver.get_nanbox_u64());
    let _proxy_pin = pin_proxy_for_native_call(obj);
    let id = lookup(obj).expect("decoded proxy receiver");
    let (target, handler, revoked) = {
        let entry = entry(id).expect("live proxy cell");
        (entry.target, entry.handler, entry.revoked)
    };
    if revoked {
        revoked_return();
        return false;
    }
    let target = scope.root_nanbox_u64(target.to_bits());
    let handler = scope.root_nanbox_f64(handler);
    let trap = scope.root_nanbox_f64(handler_trap(handler.get_nanbox_f64(), "defineProperty"));
    if trap.get_nanbox_u64() == TAG_UNDEFINED || trap.get_nanbox_u64() == TAG_NULL {
        return crate::object::object_ops::define_own_property_decoded(
            scope, &target, key, descriptor,
        );
    }
    if !is_callable(trap.get_nanbox_f64()) {
        throw_type_error("proxy defineProperty trap is not a function");
    }
    let bag = scope.root_nanbox_f64(crate::object::object_ops::descriptor_object_from_view(
        scope, descriptor,
    ));
    let result = call_trap(
        handler.get_nanbox_f64(),
        trap.get_nanbox_f64(),
        &[
            f64::from_bits(target.get_nanbox_u64()),
            key.get_nanbox_f64(),
            bag.get_nanbox_f64(),
        ],
    );
    if crate::value::js_is_truthy(result) == 0 {
        return false;
    }
    invariants::enforce_define_property_invariant(
        f64::from_bits(target.get_nanbox_u64()),
        key.get_nanbox_f64(),
        descriptor,
    );
    true
}

/// `[[GetPrototypeOf]]` for a Proxy: invoke the handler's `getPrototypeOf`
/// trap when present, otherwise forward to the TARGET's `[[Prototype]]`. A
/// Proxy itself is a small registered id (not a heap object), so the generic
/// `js_object_get_prototype_of` would mis-read it and return `null` — which
/// broke `Object.getPrototypeOf(proxy).constructor` (drizzle aliases columns as
/// `new Proxy(column, …)` and its `is(value, type)` reads
/// `getPrototypeOf(value).constructor`, crashing on `null.constructor`).
/// Callers must have already confirmed `obj` is a registered proxy.
pub(crate) fn js_proxy_get_prototype_of(obj: f64) -> f64 {
    if lookup(obj).is_none() {
        return crate::object::js_object_get_prototype_of(obj);
    }
    proxy_get_prototype_of_impl(obj)
}

/// Shared Proxy `[[GetPrototypeOf]]` (ECMA-262 §10.5.1): dispatch the trap
/// bound to the handler, validate the result is an Object or `null`, and (when
/// the target is non-extensible) enforce that the trap result matches the
/// target's actual prototype. Used by both `Object.getPrototypeOf(proxy)` and
/// `Reflect.getPrototypeOf(proxy)` so they validate identically.
pub(crate) fn proxy_get_prototype_of_impl(obj: f64) -> f64 {
    let _proxy_pin = pin_proxy_for_native_call(obj);
    let Some(id) = lookup(obj) else {
        return crate::object::js_object_get_prototype_of(obj);
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
    let scope = crate::gc::RuntimeHandleScope::new();
    let target_h = scope.root_nanbox_f64(target);
    let handler_h = scope.root_nanbox_f64(handler);
    let trap = handler_trap(handler_h.get_nanbox_f64(), "getPrototypeOf");
    let target = target_h.get_nanbox_f64();
    let trap_bits = trap.to_bits();
    if trap_bits == TAG_UNDEFINED || trap_bits == TAG_NULL {
        return reflect_target_get_prototype_of(target);
    }
    if !is_callable_function(trap) {
        return throw_type_error("proxy getPrototypeOf trap is not a function");
    }
    let result = call_trap(
        handler_h.get_nanbox_f64(),
        trap,
        &[target_h.get_nanbox_f64()],
    );
    let result_bits = result.to_bits();
    if result_bits != TAG_NULL && !reflect_value_is_object(result) {
        return throw_type_error("proxy getPrototypeOf trap returned non-object");
    }
    let target = target_h.get_nanbox_f64();
    if crate::object::obj_value_no_extend(target) {
        let actual = reflect_target_get_prototype_of(target);
        if actual.to_bits() != result_bits {
            return throw_type_error("proxy getPrototypeOf trap violates target invariant");
        }
    }
    result
}

/// Native trampoline backing the `revoke` function returned by
/// `Proxy.revocable`. The closure captures the proxy value in capture slot 0;
/// invoking it revokes that specific proxy. Idempotent — revoking an
/// already-revoked proxy is a no-op (Node's `revoke()` is idempotent). (#2846)
pub(super) extern "C" fn proxy_revoke_trampoline(
    closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    let proxy = crate::closure::js_closure_get_capture_f64(closure, 0);
    js_proxy_revoke(proxy);
    f64::from_bits(TAG_UNDEFINED)
}

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn weighted_six(
        _: *const crate::closure::ClosureHeader,
        _this: crate::closure::JsThis,
        a0: f64,
        a1: f64,
        a2: f64,
        a3: f64,
        a4: f64,
        a5: f64,
    ) -> f64 {
        a0 + 10.0 * a1 + 100.0 * a2 + 1_000.0 * a3 + 10_000.0 * a4 + 100_000.0 * a5
    }

    /// #10425: `Reflect.apply` dispatched every list of four or more arguments
    /// through `js_closure_call4`, so the body read whatever the fifth and sixth
    /// argument registers held.
    #[test]
    fn reflect_apply_forwards_every_argument() {
        let closure = crate::closure::js_closure_alloc(crate::fn_info!(weighted_six, 6), 0);
        let f = crate::value::js_nanbox_pointer(closure as i64);
        let args = array_from_args(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        let result = js_reflect_apply(f, f64::from_bits(TAG_UNDEFINED), args);
        assert_eq!(result, 654_321.0);
    }
}

pub(crate) fn proxy_is_extensible(target: f64) -> f64 {
    let id = lookup(target).expect("Proxy receiver");

    let _proxy_pin = pin_proxy_for_native_call(target);
    let (inner, handler, revoked) = entry(id)
        .map(|e| (e.target, e.handler, e.revoked))
        .unwrap_or((
            f64::from_bits(TAG_UNDEFINED),
            f64::from_bits(TAG_UNDEFINED),
            false,
        ));
    if revoked {
        return revoked_return();
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let inner_h = scope.root_nanbox_f64(inner);
    let handler_h = scope.root_nanbox_f64(handler);
    let trap = handler_trap(handler_h.get_nanbox_f64(), "isExtensible");
    let inner = inner_h.get_nanbox_f64();
    if is_callable(trap) {
        let trap_result = call_trap(
            handler_h.get_nanbox_f64(),
            trap,
            &[inner_h.get_nanbox_f64()],
        );
        let booleanized = crate::value::js_is_truthy(trap_result) != 0;
        // Invariant: the trap result must equal the target's actual
        // extensibility.
        let target_ext = crate::value::js_is_truthy(crate::object::js_object_is_extensible(
            inner_h.get_nanbox_f64(),
        )) != 0;
        if booleanized != target_ext {
            return throw_type_error(
                "proxy isExtensible trap result does not match target extensibility",
            );
        }
        return nanbox_bool(booleanized);
    }
    return crate::object::js_object_is_extensible(inner);
}

pub(crate) fn proxy_prevent_extensions(target: f64) -> f64 {
    let id = lookup(target).expect("Proxy receiver");

    let _proxy_pin = pin_proxy_for_native_call(target);
    let (inner, handler, revoked) = entry(id)
        .map(|e| (e.target, e.handler, e.revoked))
        .unwrap_or((
            f64::from_bits(TAG_UNDEFINED),
            f64::from_bits(TAG_UNDEFINED),
            false,
        ));
    if revoked {
        return revoked_return();
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let inner_h = scope.root_nanbox_f64(inner);
    let handler_h = scope.root_nanbox_f64(handler);
    let trap = handler_trap(handler_h.get_nanbox_f64(), "preventExtensions");
    let inner = inner_h.get_nanbox_f64();
    if is_callable(trap) {
        let trap_result = call_trap(
            handler_h.get_nanbox_f64(),
            trap,
            &[inner_h.get_nanbox_f64()],
        );
        let booleanized = crate::value::js_is_truthy(trap_result) != 0;
        // Invariant: a `true` result requires the target to be non-extensible.
        if booleanized {
            let target_ext = crate::value::js_is_truthy(crate::object::js_object_is_extensible(
                inner_h.get_nanbox_f64(),
            )) != 0;
            if target_ext {
                return throw_type_error(
                    "proxy preventExtensions trap returned true but target is extensible",
                );
            }
        }
        return nanbox_bool(booleanized);
    }
    crate::object::js_object_prevent_extensions(inner);
    return nanbox_bool(true);
}

pub use crate::object::property_ops::reflect_misc::{
    js_reflect_apply, js_reflect_construct, js_reflect_define_property,
    js_reflect_get_prototype_of, js_reflect_is_extensible, js_reflect_own_keys,
    js_reflect_prevent_extensions,
};

pub(crate) use crate::object::property_ops::reflect_misc::array_from_args;
