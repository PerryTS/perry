use super::{
    closure_from, coerce_trap_bool, extract_pointer, handler_trap, is_callable_function,
    js_closure_call0, js_closure_call2, js_proxy_delete, js_proxy_get, js_proxy_has, lookup,
    nanbox_bool, reflect_non_object_typeerror, reflect_ordinary_delete_property_key,
    reflect_ordinary_set_with_receiver, reflect_value_is_object, revoked_return,
    target_get_property_key, throw_type_error, PROXIES, TAG_NULL, TAG_TRUE, TAG_UNDEFINED,
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

/// `Reflect.set(target, key, value, receiver?)` - returns the boolean result
/// of the `[[Set]]` operation (#2756): `false` for a non-writable property or
/// a new key on a non-extensible object, and the coerced trap result for a
/// proxy. An absent/`undefined` `receiver` defaults to `target`; a distinct
/// receiver redirects the eventual data write per OrdinarySet (observable for
/// Integer-Indexed exotic targets — test262 internals/Set/*-reflect-set).
#[no_mangle]
pub extern "C" fn js_reflect_set(target: f64, key: f64, value: f64, receiver: f64) -> f64 {
    // Reflect.set on a non-object target must throw TypeError (spec step 1),
    // matching Reflect.has/get/etc. Pre-fix it silently returned false.
    if !reflect_value_is_object(target) {
        return reflect_non_object_typeerror("set");
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let target_handle = scope.root_nanbox_f64(target);
    let key_handle = scope.root_nanbox_f64(key);
    let value_handle = scope.root_nanbox_f64(value);
    let receiver_handle = scope.root_nanbox_f64(receiver);
    let property_key_handle = scope
        .root_nanbox_f64(unsafe { crate::object::js_to_property_key(key_handle.get_nanbox_f64()) });
    let target = target_handle.get_nanbox_f64();
    let property_key = property_key_handle.get_nanbox_f64();
    let value = value_handle.get_nanbox_f64();
    let receiver = {
        let r = receiver_handle.get_nanbox_f64();
        if r.to_bits() == crate::value::TAG_UNDEFINED {
            target
        } else {
            r
        }
    };
    if lookup(target).is_some() {
        return super::proxy_set_with_receiver(target, property_key, value, receiver);
    }
    reflect_ordinary_set_with_receiver(target, property_key, value, receiver)
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
    // Private names are never reflectable via `Reflect.has` / `in` through the
    // prototype chain: a `#name`-prefixed string key models a private element
    // (method/accessor) installed on the prototype's internal slot, invisible to
    // ordinary [[HasProperty]]. The own-key probe above already hid it on the
    // instance (`js_object_has_property` gates `#`-hiding on `class_id != 0`),
    // but the inherited field-read below walks the prototype chain and FINDS the
    // private method there, leaking `true` for `Reflect.has(c, '#m')`. Suppress
    // only the inherited probe — a genuine OWN string property literally named
    // `"#x"` (set via `obj["#x"] = …` on a plain object) was already returned
    // above. The real brand check (`#name in obj`) routes through
    // `js_private_brand_check`, not here.
    {
        let kv = crate::value::JSValue::from_bits(property_key.to_bits());
        if kv.is_any_string() {
            let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            if let Some(bytes) = unsafe { crate::string::js_string_key_bytes(kv, &mut sso) } {
                if bytes.first() == Some(&b'#') {
                    return nanbox_bool(false);
                }
            }
        }
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

fn proxy_entry(proxy_boxed: f64) -> Option<(f64, f64, bool)> {
    let id = lookup(proxy_boxed)?;
    PROXIES.with(|p| {
        p.borrow()
            .get(id as usize)
            .and_then(|o| o.as_ref())
            .map(|e| (e.target, e.handler, e.revoked))
    })
}

fn descriptor_key(name: &[u8]) -> (*const crate::StringHeader, f64) {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let key_value = crate::value::js_nanbox_string(key as i64);
    (key, key_value)
}

pub(super) unsafe fn descriptor_field_present(desc: f64, name: &[u8]) -> bool {
    let (key, key_value) = descriptor_key(name);
    if lookup(desc).is_some() {
        let scope = crate::gc::RuntimeHandleScope::new();
        let desc_handle = scope.root_nanbox_f64(desc);
        let key_handle = scope.root_nanbox_f64(key_value);
        return crate::value::js_is_truthy(js_proxy_has(
            desc_handle.get_nanbox_f64(),
            key_handle.get_nanbox_f64(),
        )) != 0;
    }
    let ptr = extract_pointer(desc.to_bits()) as *mut crate::object::ObjectHeader;
    crate::object::own_key_present(ptr, key)
}

unsafe fn descriptor_field(desc: f64, name: &[u8]) -> f64 {
    let (key, key_value) = descriptor_key(name);
    if lookup(desc).is_some() {
        let scope = crate::gc::RuntimeHandleScope::new();
        let desc_handle = scope.root_nanbox_f64(desc);
        let key_handle = scope.root_nanbox_f64(key_value);
        return js_proxy_get(desc_handle.get_nanbox_f64(), key_handle.get_nanbox_f64());
    }
    let ptr = extract_pointer(desc.to_bits()) as *const crate::object::ObjectHeader;
    if ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    f64::from_bits(crate::object::js_object_get_field_by_name(ptr, key).bits())
}

pub(super) unsafe fn descriptor_bool_field(desc: f64, name: &[u8]) -> Option<bool> {
    if !descriptor_field_present(desc, name) {
        return None;
    }
    Some(crate::value::js_is_truthy(descriptor_field(desc, name)) != 0)
}

unsafe fn complete_proxy_descriptor_result(desc: f64) -> f64 {
    if !reflect_value_is_object(desc) {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let desc_handle = scope.root_nanbox_f64(desc);
    let desc = desc_handle.get_nanbox_f64();

    let has_enumerable = descriptor_field_present(desc, b"enumerable");
    let has_configurable = descriptor_field_present(desc, b"configurable");
    let has_value = descriptor_field_present(desc, b"value");
    let has_writable = descriptor_field_present(desc, b"writable");
    let has_get = descriptor_field_present(desc, b"get");
    let has_set = descriptor_field_present(desc, b"set");

    let enumerable =
        has_enumerable && crate::value::js_is_truthy(descriptor_field(desc, b"enumerable")) != 0;
    let configurable = has_configurable
        && crate::value::js_is_truthy(descriptor_field(desc, b"configurable")) != 0;
    let value = if has_value {
        descriptor_field(desc, b"value")
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let value_handle = scope.root_nanbox_f64(value);
    let writable =
        has_writable && crate::value::js_is_truthy(descriptor_field(desc, b"writable")) != 0;

    let getter = if has_get {
        descriptor_field(desc, b"get")
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let getter_handle = scope.root_nanbox_f64(getter);
    let setter = if has_set {
        descriptor_field(desc, b"set")
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let setter_handle = scope.root_nanbox_f64(setter);

    let getter = getter_handle.get_nanbox_f64();
    if getter.to_bits() != TAG_UNDEFINED && !is_callable_function(getter) {
        throw_type_error("Getter must be a function");
    }
    let setter = setter_handle.get_nanbox_f64();
    if setter.to_bits() != TAG_UNDEFINED && !is_callable_function(setter) {
        throw_type_error("Setter must be a function");
    }
    if (has_get || has_set) && (has_value || has_writable) {
        throw_type_error("Invalid property descriptor");
    }

    if has_get || has_set {
        crate::object::build_accessor_descriptor(getter, setter, enumerable, configurable)
    } else {
        crate::object::build_data_descriptor(
            value_handle.get_nanbox_f64(),
            writable,
            enumerable,
            configurable,
        )
    }
}

/// `Reflect.getOwnPropertyDescriptor(target, key)` — Reflect-specific
/// `[[GetOwnProperty]]` entry point: non-object targets throw, property keys
/// are normalized before dispatch, and proxy traps are observed.
#[no_mangle]
pub extern "C" fn js_reflect_get_own_property_descriptor(target: f64, key: f64) -> f64 {
    if !reflect_value_is_object(target) {
        return reflect_non_object_typeerror("getOwnPropertyDescriptor");
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let target_handle = scope.root_nanbox_f64(target);
    let key_handle = scope.root_nanbox_f64(key);
    let property_key_handle = scope
        .root_nanbox_f64(unsafe { crate::object::js_to_property_key(key_handle.get_nanbox_f64()) });
    let target = target_handle.get_nanbox_f64();
    let property_key = property_key_handle.get_nanbox_f64();

    let Some((inner, handler, revoked)) = proxy_entry(target) else {
        return crate::object::js_object_get_own_property_descriptor(target, property_key);
    };
    if revoked {
        return revoked_return();
    }

    let trap = handler_trap(handler, "getOwnPropertyDescriptor");
    let trap_bits = trap.to_bits();
    if trap_bits == TAG_UNDEFINED || trap_bits == TAG_NULL {
        // No trap — forward to the target's [[GetOwnProperty]]. When the target
        // is itself a Proxy, recurse through the Reflect entry point rather than
        // the ordinary object path, which would deref the fake proxy pointer.
        if lookup(inner).is_some() {
            return js_reflect_get_own_property_descriptor(inner, property_key);
        }
        return crate::object::js_object_get_own_property_descriptor(inner, property_key);
    }
    if !is_callable_function(trap) {
        return throw_type_error("proxy getOwnPropertyDescriptor trap is not a function");
    }

    let rebound = crate::closure::clone_closure_rebind_this(trap_bits, handler);
    let closure = closure_from(f64::from_bits(rebound));
    if closure.is_null() {
        return throw_type_error("proxy getOwnPropertyDescriptor trap is not a function");
    }
    let prev = crate::object::js_implicit_this_set(handler);
    let result = js_closure_call2(closure, inner, property_key);
    crate::object::js_implicit_this_set(prev);
    let result_handle = scope.root_nanbox_f64(result);

    let target_desc = crate::object::js_object_get_own_property_descriptor(inner, property_key);
    let target_desc_handle = scope.root_nanbox_f64(target_desc);
    let result = result_handle.get_nanbox_f64();
    let target_desc = target_desc_handle.get_nanbox_f64();
    if result.to_bits() == TAG_UNDEFINED {
        if target_desc.to_bits() != TAG_UNDEFINED
            && (crate::object::obj_value_no_extend(inner)
                || unsafe { descriptor_bool_field(target_desc, b"configurable") } == Some(false))
        {
            return throw_type_error(
                "proxy getOwnPropertyDescriptor trap cannot hide target property",
            );
        }
        return result;
    }

    if !reflect_value_is_object(result) {
        return throw_type_error("proxy getOwnPropertyDescriptor trap returned non-object");
    }
    let result = unsafe { complete_proxy_descriptor_result(result) };
    let result_handle = scope.root_nanbox_f64(result);
    let result = result_handle.get_nanbox_f64();

    if target_desc.to_bits() == TAG_UNDEFINED {
        if crate::object::obj_value_no_extend(inner) {
            return throw_type_error(
                "proxy getOwnPropertyDescriptor trap reports new property on non-extensible target",
            );
        }
    } else if unsafe { descriptor_bool_field(target_desc, b"configurable") } == Some(false)
        && unsafe { descriptor_bool_field(result, b"configurable") } == Some(true)
    {
        return throw_type_error(
            "proxy getOwnPropertyDescriptor trap reports incompatible descriptor",
        );
    }

    // [[GetOwnProperty]] step 21.a: a non-configurable result descriptor is only
    // valid for a non-configurable existing target property.
    if unsafe { descriptor_bool_field(result, b"configurable") } == Some(false) {
        let target_configurable = target_desc.to_bits() == TAG_UNDEFINED
            || unsafe { descriptor_bool_field(target_desc, b"configurable") } != Some(false);
        if target_configurable {
            return throw_type_error(
                "proxy getOwnPropertyDescriptor trap reports a non-configurable descriptor for a configurable or absent target property",
            );
        }
    }

    result
}
