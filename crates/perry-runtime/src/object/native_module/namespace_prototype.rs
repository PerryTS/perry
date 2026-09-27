//! #11542: a native-module namespace object is an ordinary object whose own
//! properties are its module's exports. A read or `in` that misses them
//! continues at its `[[Prototype]]`, the same one `Object.getPrototypeOf(ns)`
//! reports. Before this, the export resolver's `undefined` for an unknown name
//! was the WHOLE answer: `ns.constructor`, `ns.hasOwnProperty` and `ns.toString`
//! all read `undefined`, and so did every read that inherits THROUGH a
//! namespace (`Object.create(require("process")).constructor`).

use super::*;

/// The default `[[Prototype]]` of a native-module namespace object:
/// `%Object.prototype%` (`null` only before that intrinsic exists). A
/// namespace is an ordinary object, not its own prototype.
pub(crate) fn native_module_namespace_default_prototype() -> f64 {
    let proto = crate::object::builtin_prototype_value("Object");
    if proto.to_bits() != crate::value::TAG_UNDEFINED {
        proto
    } else {
        f64::from_bits(crate::value::TAG_NULL)
    }
}

/// `[[Prototype]]` of the namespace object `obj`: the one recorded for it
/// (`Object.setPrototypeOf(ns, p)`), else the default. `[[Get]]`,
/// `[[HasProperty]]` and `Object.getPrototypeOf` all read it from here, so
/// they cannot disagree about what a namespace inherits.
///
/// # Safety
/// `obj` must point to a live namespace `ObjectHeader`. Resolving the default
/// may allocate: a caller holding raw pointers must root them first.
pub(crate) unsafe fn native_module_namespace_prototype_bits(obj: *const ObjectHeader) -> u64 {
    match crate::object::prototype_chain::object_static_prototype(obj as usize) {
        Some(bits) => bits,
        None => native_module_namespace_default_prototype().to_bits(),
    }
}

/// `[[Get]]` of a name outside the namespace's export surface. The resolver
/// still gets the first word (it knows names the export list does not, such
/// as sub-namespaces); only its `undefined` falls through to the prototype.
///
/// # Safety
/// `obj` must point to a live namespace `ObjectHeader`, `key` to its live key
/// string whose bytes are `key_ptr[..key_len]`.
pub(super) unsafe fn non_own_field(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
    module_name: &str,
    key_ptr: *const u8,
    key_len: usize,
) -> JSValue {
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_h = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(obj as i64));
    let key_h = scope.root_nanbox_f64(crate::value::nanbox_string_key(key));
    let resolved = js_native_module_property_by_name(
        module_name.as_ptr(),
        module_name.len(),
        key_ptr,
        key_len,
    );
    if resolved.to_bits() != crate::value::TAG_UNDEFINED {
        return JSValue::from_bits(resolved.to_bits());
    }
    let obj = crate::value::js_nanbox_get_pointer(obj_h.get_nanbox_f64()) as *const ObjectHeader;
    let proto_bits = native_module_namespace_prototype_bits(obj);
    // Re-read both: resolving the default prototype may allocate.
    let obj = crate::value::js_nanbox_get_pointer(obj_h.get_nanbox_f64()) as usize;
    let key =
        crate::value::js_nanbox_get_pointer(key_h.get_nanbox_f64()) as *const crate::StringHeader;
    crate::object::prototype_chain::resolve_inherited_field_from_prototype(obj, proto_bits, key)
        .unwrap_or_else(JSValue::undefined)
}
