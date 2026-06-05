use super::{
    closure_from, coerce_trap_bool, js_closure_call0, js_proxy_delete, js_proxy_get, js_proxy_has,
    js_proxy_set, lookup, nanbox_bool, reflect_non_object_typeerror,
    reflect_ordinary_delete_property_key, reflect_ordinary_set_property_key,
    reflect_value_is_object, target_get_property_key, TAG_TRUE, TAG_UNDEFINED,
};

/// `Reflect.get(target, key, receiver)` (#2766).
///
/// - throws `TypeError` for a non-object target,
/// - uses `receiver` as the `this` binding for accessor getters,
/// - dispatches proxy `get` traps (forwarding `(target, key)` to the existing
///   proxy path; the three-argument trap receiver is out of scope - Perry's
///   proxy traps are two-argument).
///
/// `receiver` is the optional third argument; codegen passes `target` when the
/// call site omits it (matching the spec default), and `undefined` is treated
/// as "use target".
#[no_mangle]
pub extern "C" fn js_reflect_get(target: f64, key: f64, receiver: f64) -> f64 {
    if !reflect_value_is_object(target) {
        return reflect_non_object_typeerror("get");
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let target_handle = scope.root_nanbox_f64(target);
    let key_handle = scope.root_nanbox_f64(key);
    let receiver_handle = scope.root_nanbox_f64(receiver);
    let property_key_handle = scope
        .root_nanbox_f64(unsafe { crate::object::js_to_property_key(key_handle.get_nanbox_f64()) });
    let target = target_handle.get_nanbox_f64();
    let property_key = property_key_handle.get_nanbox_f64();
    if lookup(target).is_some() {
        return js_proxy_get(target, property_key);
    }
    // Default receiver to target when undefined.
    let receiver = receiver_handle.get_nanbox_f64();
    let recv = if receiver.to_bits() == TAG_UNDEFINED {
        target
    } else {
        receiver
    };
    // #2766: if `key` resolves to an accessor *getter* on `target`, rebind its
    // `this` to the receiver and invoke it - object-literal getters capture
    // `this` in a reserved closure slot (not `IMPLICIT_THIS`), so plain
    // forwarding would read the target's fields, not the receiver's. When the
    // receiver equals the target we can skip the clone and use the ordinary
    // read.
    if recv.to_bits() != target.to_bits() {
        let getter_bits = if unsafe { crate::symbol::js_is_symbol(property_key) } != 0 {
            unsafe { crate::symbol::reflect_symbol_getter_closure_bits(target, property_key) }
        } else {
            crate::object::reflect_getter_closure_bits(target, property_key)
        };
        if let Some(getter_bits) = getter_bits {
            if getter_bits == 0 {
                // Accessor with no getter -> undefined.
                return f64::from_bits(TAG_UNDEFINED);
            }
            let rebound = crate::closure::clone_closure_rebind_this(getter_bits, recv);
            let closure = closure_from(f64::from_bits(rebound));
            if !closure.is_null() {
                // Also set IMPLICIT_THIS for free-function getters that read
                // `this` from the implicit-this fallback rather than a slot.
                let prev = crate::object::js_implicit_this_set(recv);
                let result = js_closure_call0(closure);
                crate::object::js_implicit_this_set(prev);
                return result;
            }
        }
    }
    let prev = crate::object::js_implicit_this_set(recv);
    let result = target_get_property_key(target, property_key);
    crate::object::js_implicit_this_set(prev);
    result
}

/// `Reflect.set(target, key, value)` - returns the boolean result of the
/// `[[Set]]` operation (#2756): `false` for a non-writable property or a new
/// key on a non-extensible object, and the coerced trap result for a proxy.
#[no_mangle]
pub extern "C" fn js_reflect_set(target: f64, key: f64, value: f64) -> f64 {
    // Reflect.set on a non-object target must throw TypeError (spec step 1),
    // matching Reflect.has/get/etc. Pre-fix it silently returned false.
    if !reflect_value_is_object(target) {
        return reflect_non_object_typeerror("set");
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let target_handle = scope.root_nanbox_f64(target);
    let key_handle = scope.root_nanbox_f64(key);
    let value_handle = scope.root_nanbox_f64(value);
    let property_key_handle = scope
        .root_nanbox_f64(unsafe { crate::object::js_to_property_key(key_handle.get_nanbox_f64()) });
    let target = target_handle.get_nanbox_f64();
    let property_key = property_key_handle.get_nanbox_f64();
    let value = value_handle.get_nanbox_f64();
    if lookup(target).is_some() {
        return js_proxy_set(target, property_key, value);
    }
    reflect_ordinary_set_property_key(target, property_key, value)
}

/// `Reflect.has(target, key)` (#2764) - `[[HasProperty]]` semantics:
///
/// - throws `TypeError` for a non-object target,
/// - walks the recorded ordinary prototype chain (e.g. `Object.create(proto)`),
/// - dispatches to a proxy `has` trap (with `ToBoolean` coercion).
#[no_mangle]
pub extern "C" fn js_reflect_has(target: f64, key: f64) -> f64 {
    if !reflect_value_is_object(target) {
        return reflect_non_object_typeerror("has");
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let target_handle = scope.root_nanbox_f64(target);
    let key_handle = scope.root_nanbox_f64(key);
    let property_key_handle = scope
        .root_nanbox_f64(unsafe { crate::object::js_to_property_key(key_handle.get_nanbox_f64()) });
    let target = target_handle.get_nanbox_f64();
    let property_key = property_key_handle.get_nanbox_f64();
    if lookup(target).is_some() {
        let trap_result = js_proxy_has(target, property_key);
        // #2764: normalize the trap result with ToBoolean.
        return coerce_trap_bool(trap_result);
    }
    if unsafe { crate::symbol::js_is_symbol(property_key) } != 0
        && unsafe { crate::symbol::js_object_has_own_symbol_property(target, property_key) }
    {
        return nanbox_bool(true);
    }
    // Own + (for class refs / closures) internal lookup.
    let own = crate::object::js_object_has_property(target, property_key);
    if own.to_bits() == TAG_TRUE {
        return own;
    }
    // #2764: `[[HasProperty]]` must also see inherited properties. Perry's
    // `js_object_has_property` only checks own keys, but the ordinary field
    // getter DOES walk the (Object.create / setPrototypeOf-recorded) prototype
    // chain. So probe via a field read: a non-`undefined` result means the
    // property resolves somewhere on the chain. (A genuinely
    // present-but-`undefined` inherited value is indistinguishable here, which
    // matches the own-undefined behavior of `js_object_has_property` and is
    // acceptable for the inherited case.)
    let inherited = target_get_property_key(target, property_key);
    if inherited.to_bits() != TAG_UNDEFINED {
        return nanbox_bool(true);
    }
    nanbox_bool(false)
}

/// `Reflect.deleteProperty(target, key)` - returns the boolean delete result
/// (#2760): `false` for a non-configurable property, and the coerced trap
/// result for a proxy.
#[no_mangle]
pub extern "C" fn js_reflect_delete(target: f64, key: f64) -> f64 {
    // Reflect.deleteProperty on a non-object target must throw TypeError (spec
    // step 1). Pre-fix it silently returned true.
    if !reflect_value_is_object(target) {
        return reflect_non_object_typeerror("deleteProperty");
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let target_handle = scope.root_nanbox_f64(target);
    let key_handle = scope.root_nanbox_f64(key);
    let property_key_handle = scope
        .root_nanbox_f64(unsafe { crate::object::js_to_property_key(key_handle.get_nanbox_f64()) });
    let target = target_handle.get_nanbox_f64();
    let property_key = property_key_handle.get_nanbox_f64();
    if lookup(target).is_some() {
        return js_proxy_delete(target, property_key);
    }
    reflect_ordinary_delete_property_key(target, property_key)
}
