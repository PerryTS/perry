//! Shared ordinary property operations. Proxy dispatch uses installer slots.
use super::*;

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

pub(crate) fn reflect_ordinary_delete(target: f64, key: f64) -> f64 {
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
