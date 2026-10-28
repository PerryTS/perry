//! Proxy `[[HasProperty]]` and `[[Delete]]` (ECMA-262 §10.5.7 / §10.5.10) plus
//! the ordinary (non-proxy) `[[Delete]]` helper they forward to. Split out of
//! `proxy.rs` to keep that file under the 2000-line size gate — pure code motion,
//! no behavior change.

use super::{
    call_trap, entry, handler_trap, invariants, is_callable, lookup, nanbox_bool,
    pin_proxy_for_native_call, revoked_return, TAG_FALSE, TAG_UNDEFINED,
};

/// `key in proxy` — if `handler.has` exists, call it; else forward to the
/// target's `[[HasProperty]]` (recursing through a proxy target).
#[no_mangle]
pub extern "C" fn js_proxy_has(proxy_boxed: f64, key: f64) -> f64 {
    let _proxy_pin = pin_proxy_for_native_call(proxy_boxed);
    let id = match lookup(proxy_boxed) {
        Some(id) => id,
        None => return f64::from_bits(TAG_FALSE),
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
    let key_h = scope.root_nanbox_f64(key);
    let trap = handler_trap(handler_h.get_nanbox_f64(), "has");
    let target = target_h.get_nanbox_f64();
    let key = key_h.get_nanbox_f64();
    if is_callable(trap) {
        let trap_result = call_trap(
            handler_h.get_nanbox_f64(),
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
    let _proxy_pin = pin_proxy_for_native_call(proxy_boxed);
    let id = match lookup(proxy_boxed) {
        Some(id) => id,
        None => return f64::from_bits(TAG_FALSE),
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
    let key_h = scope.root_nanbox_f64(key);
    let trap = handler_trap(handler_h.get_nanbox_f64(), "deleteProperty");
    let target = target_h.get_nanbox_f64();
    let key = key_h.get_nanbox_f64();
    if is_callable(trap) {
        // #2760: the `deleteProperty` trap's boolean result is observable
        // through `Reflect.deleteProperty(proxy, …)`.
        let trap_result = call_trap(
            handler_h.get_nanbox_f64(),
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

pub(crate) use crate::object::property_ops::delete::reflect_ordinary_delete;
