//! Proxy [[Set]] backend. Ordinary PutValue belongs to the object model.
use super::*;

/// `proxy[key] = value` — if handler.set exists, call it with
/// (target, key, value) and return TAG_TRUE (the trap's return value is
/// ignored by the default test semantics since we echo `value`). Otherwise
/// forward to the target directly.
#[no_mangle]
pub extern "C" fn js_proxy_set(proxy_boxed: f64, key: f64, value: f64) -> f64 {
    let tag = key.to_bits() & crate::value::TAG_MASK;
    if tag == crate::value::STRING_TAG
        || tag == crate::value::SHORT_STRING_TAG
        || unsafe { crate::symbol::js_is_symbol(key) } != 0
    {
        return proxy_set_with_receiver(proxy_boxed, key, value, proxy_boxed);
    }
    // Dynamic numeric-index callers pass their raw index. Proxy [[Set]]
    // traps receive only property keys, just like [[Get]]; preserve operands
    // across ToPropertyKey, which can allocate or invoke user coercion.
    let scope = crate::gc::RuntimeHandleScope::new();
    let proxy = scope.root_nanbox_f64(proxy_boxed);
    let value = scope.root_nanbox_f64(value);
    let key = scope.root_nanbox_f64(key);
    let key =
        scope.root_nanbox_f64(unsafe { crate::object::js_to_property_key(key.get_nanbox_f64()) });
    proxy_set_with_receiver(
        proxy.get_nanbox_f64(),
        key.get_nanbox_f64(),
        value.get_nanbox_f64(),
        proxy.get_nanbox_f64(),
    )
}

/// Proxy `[[Set]]` (ECMA-262 §10.5.9) with an explicit `Receiver`, distinct
/// from `proxy_boxed` itself — reached when a Proxy sits partway up another
/// object's `[[Prototype]]` chain (`OrdinarySetWithOwnDescriptor` forwards to
/// `parent.[[Set]](P, V, Receiver)` with the ORIGINAL receiver, not `parent`).
pub(crate) fn proxy_set_with_receiver(
    proxy_boxed: f64,
    key: f64,
    value: f64,
    receiver: f64,
) -> f64 {
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
    let value_h = scope.root_nanbox_f64(value);
    let receiver_h = scope.root_nanbox_f64(receiver);
    let trap = handler_trap(handler_h.get_nanbox_f64(), "set");
    let target = target_h.get_nanbox_f64();
    let key = key_h.get_nanbox_f64();
    let value = value_h.get_nanbox_f64();
    let receiver = receiver_h.get_nanbox_f64();
    if is_callable(trap) {
        // #2756: the `set` trap's boolean result is observable through
        // `Reflect.set(proxy, …)` (and strict-mode assignment). Coerce and
        // return it rather than discarding it. The trap receives the spec
        // argument list `(target, key, value, receiver)` with `this` bound to
        // the handler.
        let trap_result = call_trap(
            handler_h.get_nanbox_f64(),
            trap,
            &[
                target_h.get_nanbox_f64(),
                key_h.get_nanbox_f64(),
                value_h.get_nanbox_f64(),
                receiver_h.get_nanbox_f64(),
            ],
        );
        // A falsy trap result means the assignment failed; no invariant check.
        if crate::value::js_is_truthy(trap_result) == 0 {
            return nanbox_bool(false);
        }
        invariants::enforce_set_invariant(
            target_h.get_nanbox_f64(),
            key_h.get_nanbox_f64(),
            value_h.get_nanbox_f64(),
        );
        return nanbox_bool(true);
    }
    // No set trap — forward to the target's `[[Set]]`. When the target is
    // itself a Proxy, recurse through the proxy dispatch (its own trap or
    // target) rather than `ordinary_set`, which would deref the fake pointer.
    if lookup(target).is_some() {
        return proxy_set_with_receiver(target, key, value, receiver);
    }
    let target_handle = scope.root_nanbox_f64(target);
    let key_handle = scope.root_nanbox_f64(key);
    let value_handle = scope.root_nanbox_f64(value);
    let receiver_handle = scope.root_nanbox_f64(receiver);
    let property_key_handle = scope
        .root_nanbox_f64(unsafe { crate::object::js_to_property_key(key_handle.get_nanbox_f64()) });
    reflect_ordinary_set_with_receiver(
        target_handle.get_nanbox_f64(),
        property_key_handle.get_nanbox_f64(),
        value_handle.get_nanbox_f64(),
        receiver_handle.get_nanbox_f64(),
    )
}
#[cfg(test)]
pub(crate) use crate::object::property_ops::put_value::record_fixture;
pub(crate) use crate::object::property_ops::put_value::IC_SLOT_OVERFLOW_BIT;
pub use crate::object::property_ops::put_value::*;
