//! Proxy `[[HasProperty]]` and `[[Delete]]` (ECMA-262 §10.5.7 / §10.5.10) plus
//! the ordinary (non-proxy) `[[Delete]]` helper they forward to. Split out of
//! `proxy.rs` to keep that file under the 2000-line size gate — pure code motion,
//! no behavior change.

use super::{
    call_trap, handler_trap, invariants, is_callable, is_non_configurable_exotic_own, lookup,
    nanbox_bool, revoked_return, PROXIES, TAG_FALSE, TAG_UNDEFINED,
};

/// `key in proxy` — if `handler.has` exists, call it; else forward to the
/// target's `[[HasProperty]]` (recursing through a proxy target).
#[no_mangle]
pub extern "C" fn js_proxy_has(proxy_boxed: f64, key: f64) -> f64 {
    let id = match lookup(proxy_boxed) {
        Some(id) => id,
        None => return f64::from_bits(TAG_FALSE),
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
    let trap = handler_trap(handler, "has");
    if is_callable(trap) {
        let scope = crate::gc::RuntimeHandleScope::new();
        let target_h = scope.root_nanbox_f64(target);
        let key_h = scope.root_nanbox_f64(key);
        let trap_result = call_trap(
            handler,
            trap,
            &[target_h.get_nanbox_f64(), key_h.get_nanbox_f64()],
        );
        // [[HasProperty]] invariant: a `false` trap result is rejected when the
        // target owns the key non-configurably, or the target is non-extensible
        // and owns the key.
        if crate::value::js_is_truthy(trap_result) == 0 {
            invariants::enforce_has_false_invariant(
                target_h.get_nanbox_f64(),
                key_h.get_nanbox_f64(),
            );
            return nanbox_bool(false);
        }
        return nanbox_bool(true);
    }
    // No has trap — forward to the target's `[[HasProperty]]`, recursing through
    // a proxy target.
    if lookup(target).is_some() {
        return js_proxy_has(target, key);
    }
    crate::object::js_object_has_property(target, key)
}

/// `delete proxy[key]` — if handler.deleteProperty exists, call it; else
/// delegate to `js_object_delete_field` on the target.
#[no_mangle]
pub extern "C" fn js_proxy_delete(proxy_boxed: f64, key: f64) -> f64 {
    let id = match lookup(proxy_boxed) {
        Some(id) => id,
        None => return f64::from_bits(TAG_FALSE),
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
    let trap = handler_trap(handler, "deleteProperty");
    if is_callable(trap) {
        // #2760: the `deleteProperty` trap's boolean result is observable
        // through `Reflect.deleteProperty(proxy, …)`.
        let scope = crate::gc::RuntimeHandleScope::new();
        let target_h = scope.root_nanbox_f64(target);
        let key_h = scope.root_nanbox_f64(key);
        let trap_result = call_trap(
            handler,
            trap,
            &[target_h.get_nanbox_f64(), key_h.get_nanbox_f64()],
        );
        if crate::value::js_is_truthy(trap_result) == 0 {
            return nanbox_bool(false);
        }
        // [[Delete]] invariant: a `true` result is rejected when the target owns
        // the key non-configurably, or owns it and is non-extensible.
        invariants::enforce_delete_invariant(target_h.get_nanbox_f64(), key_h.get_nanbox_f64());
        return nanbox_bool(true);
    }
    // No trap — forward to the target's `[[Delete]]`, recursing through a proxy
    // target.
    if lookup(target).is_some() {
        return js_proxy_delete(target, key);
    }
    reflect_ordinary_delete(target, key)
}

/// Perform an ordinary (non-proxy) `[[Delete]]` and report the result as a
/// NaN-boxed boolean. Returns `false` for a non-configurable property (#2760),
/// matching `Reflect.deleteProperty` rather than the silent-success behavior of
/// the `delete` operator.
pub(crate) fn reflect_ordinary_delete_property_key(target: f64, property_key: f64) -> f64 {
    if unsafe { crate::symbol::js_is_symbol(property_key) } != 0 {
        let deleted =
            unsafe { crate::symbol::js_object_delete_symbol_property(target, property_key) };
        return nanbox_bool(deleted != 0);
    }
    if let Some((_writable, configurable)) = crate::object::obj_value_attrs(target, property_key) {
        if !configurable {
            return nanbox_bool(false);
        }
    }
    // Non-configurable exotic own properties (an Array's `length`, a plain
    // function's `prototype`) have no entry in the ordinary descriptor table, so
    // `obj_value_attrs` above returns `None`. `Reflect.deleteProperty` / `delete`
    // must report `false` for them (test262
    // Proxy/deleteProperty/trap-is-{undefined,missing,null}-target-is-proxy forward
    // through to a real array/function). Without this the delete silently
    // succeeded and reported `true`.
    if is_non_configurable_exotic_own(target, property_key) {
        return nanbox_bool(false);
    }
    let obj_ptr = super::extract_pointer(target.to_bits()) as *mut crate::ObjectHeader;
    let key_ptr =
        crate::value::js_get_string_pointer_unified(property_key) as *const crate::StringHeader;
    if !obj_ptr.is_null() && !key_ptr.is_null() {
        let deleted = crate::object::js_object_delete_field(obj_ptr, key_ptr);
        return nanbox_bool(deleted != 0);
    }
    nanbox_bool(true)
}

fn reflect_ordinary_delete(target: f64, key: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let target_handle = scope.root_nanbox_f64(target);
    let key_handle = scope.root_nanbox_f64(key);
    let property_key_handle = scope
        .root_nanbox_f64(unsafe { crate::object::js_to_property_key(key_handle.get_nanbox_f64()) });
    reflect_ordinary_delete_property_key(
        target_handle.get_nanbox_f64(),
        property_key_handle.get_nanbox_f64(),
    )
}
