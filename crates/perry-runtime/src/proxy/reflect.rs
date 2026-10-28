use super::{
    closure_from, entry, handler_trap, is_callable_function, lookup, reflect_value_is_object,
    revoked_return, throw_type_error, TAG_NULL, TAG_UNDEFINED,
};

fn proxy_entry(proxy_boxed: f64) -> Option<(f64, f64, bool)> {
    let id = lookup(proxy_boxed)?;
    entry(id).map(|e| (e.target, e.handler, e.revoked))
}

unsafe fn complete_proxy_descriptor_result(desc: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let desc = scope.root_nanbox_f64(crate::object::object_ops::normalize_descriptor_operand(
        desc,
    ));
    let view = crate::object::object_ops::decode_property_descriptor(&scope, &desc);
    let enumerable = view.flag(b"enumerable").unwrap_or(false);
    let configurable = view.flag(b"configurable").unwrap_or(false);
    if view.has_named(b"get") || view.has_named(b"set") {
        crate::object::build_accessor_descriptor(
            f64::from_bits(view.read_named(b"get").bits()),
            f64::from_bits(view.read_named(b"set").bits()),
            enumerable,
            configurable,
        )
    } else {
        crate::object::build_data_descriptor(
            f64::from_bits(view.read_named(b"value").bits()),
            view.flag(b"writable").unwrap_or(false),
            enumerable,
            configurable,
        )
    }
}

pub(crate) fn proxy_get_own_property_descriptor(
    scope: &crate::gc::RuntimeHandleScope,
    target_handle: &crate::gc::RuntimeHandle<'_>,
    property_key_handle: &crate::gc::RuntimeHandle<'_>,
) -> f64 {
    let target = target_handle.get_nanbox_f64();
    let property_key = property_key_handle.get_nanbox_f64();
    let Some((inner, handler, revoked)) = proxy_entry(target) else {
        return crate::object::js_object_get_own_property_descriptor(target, property_key);
    };
    if revoked {
        return revoked_return();
    }

    // The handler lookup, trap, and descriptor field getters can all run JS.
    // A root is useful only if each post-callback read reloads its current value.
    let inner_handle = scope.root_nanbox_f64(inner);
    let handler_handle = scope.root_nanbox_f64(handler);
    let trap = handler_trap(handler_handle.get_nanbox_f64(), "getOwnPropertyDescriptor");
    let trap_bits = trap.to_bits();
    if trap_bits == TAG_UNDEFINED || trap_bits == TAG_NULL {
        // No trap — forward to the target's [[GetOwnProperty]]. When the target
        // is itself a Proxy, recurse through the Reflect entry point rather than
        // the ordinary object path, which would deref the fake proxy pointer.
        if lookup(inner_handle.get_nanbox_f64()).is_some() {
            return js_reflect_get_own_property_descriptor(
                inner_handle.get_nanbox_f64(),
                property_key_handle.get_nanbox_f64(),
            );
        }
        return crate::object::js_object_get_own_property_descriptor(
            inner_handle.get_nanbox_f64(),
            property_key_handle.get_nanbox_f64(),
        );
    }
    if !is_callable_function(trap) {
        return throw_type_error("proxy getOwnPropertyDescriptor trap is not a function");
    }

    let rebound =
        crate::closure::clone_closure_rebind_this(trap_bits, handler_handle.get_nanbox_f64());
    let closure = closure_from(f64::from_bits(rebound));
    if closure.is_null() {
        return throw_type_error("proxy getOwnPropertyDescriptor trap is not a function");
    }
    let result = crate::closure::js_closure_call2(
        closure,
        crate::closure::JsThis::from_f64(handler_handle.get_nanbox_f64()),
        inner_handle.get_nanbox_f64(),
        property_key_handle.get_nanbox_f64(),
    );
    let result_handle = scope.root_nanbox_f64(result);

    let target_desc = crate::object::js_object_get_own_property_descriptor(
        inner_handle.get_nanbox_f64(),
        property_key_handle.get_nanbox_f64(),
    );
    let target_desc_handle = scope.root_nanbox_f64(target_desc);
    let result = result_handle.get_nanbox_f64();
    if result.to_bits() == TAG_UNDEFINED {
        if target_desc_handle.get_nanbox_u64() != TAG_UNDEFINED
            && (crate::object::obj_value_no_extend(inner_handle.get_nanbox_f64())
                || unsafe {
                    descriptor_bool_field(target_desc_handle.get_nanbox_f64(), b"configurable")
                } == Some(false))
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

    if target_desc_handle.get_nanbox_u64() == TAG_UNDEFINED {
        if crate::object::obj_value_no_extend(inner_handle.get_nanbox_f64()) {
            return throw_type_error(
                "proxy getOwnPropertyDescriptor trap reports new property on non-extensible target",
            );
        }
    } else if unsafe { descriptor_bool_field(target_desc_handle.get_nanbox_f64(), b"configurable") }
        == Some(false)
        && unsafe { descriptor_bool_field(result_handle.get_nanbox_f64(), b"configurable") }
            == Some(true)
    {
        return throw_type_error(
            "proxy getOwnPropertyDescriptor trap reports incompatible descriptor",
        );
    }

    // [[GetOwnProperty]] step 21.a: a non-configurable result descriptor is only
    // valid for a non-configurable existing target property.
    if unsafe { descriptor_bool_field(result_handle.get_nanbox_f64(), b"configurable") }
        == Some(false)
    {
        let target_configurable = target_desc_handle.get_nanbox_u64() == TAG_UNDEFINED
            || unsafe {
                descriptor_bool_field(target_desc_handle.get_nanbox_f64(), b"configurable")
            } != Some(false);
        if target_configurable {
            return throw_type_error(
                "proxy getOwnPropertyDescriptor trap reports a non-configurable descriptor for a configurable or absent target property",
            );
        }
    }

    result_handle.get_nanbox_f64()
}

pub use crate::object::property_ops::reflect::{
    js_reflect_delete, js_reflect_get, js_reflect_get_own_property_descriptor, js_reflect_has,
    js_reflect_set,
};

pub(crate) use crate::object::property_ops::reflect::{
    descriptor_bool_field, js_reflect_get_with_explicit_receiver,
};
