//! Shared ordinary property operations. Proxy dispatch uses installer slots.
use super::*;

/// `Reflect.setPrototypeOf(target, proto)` (#2761).
///
/// Returns a boolean: `true` when the prototype change is applied, `false`
/// when it is rejected (target is non-extensible and the proto actually
/// changes). Throws `TypeError` for a non-object target or a proto that is
/// neither an object nor `null`. For a proxy, dispatches the `setPrototypeOf`
/// trap.
#[no_mangle]
pub extern "C" fn js_reflect_set_prototype_of(target: f64, proto: f64) -> f64 {
    // Target must be an object (a proxy qualifies).
    if !reflect_value_is_object(target) {
        return reflect_non_object_typeerror("setPrototypeOf");
    }

    // Proto must be an object or null.
    let proto_bits = proto.to_bits();
    let proto_ok = proto_bits == TAG_NULL || reflect_value_is_object(proto);
    if !proto_ok {
        return throw_type_error("Object prototype may only be an Object or null");
    }

    // Proxy `[[SetPrototypeOf]]`: dispatch the trap (or forward through the
    // target chain) before the ordinary path, which would deref the fake
    // proxy pointer.
    if lookup(target).is_some() {
        return proxy_set_prototype_of(target, proto);
    }

    // #2761: a non-extensible target rejects a *changing* prototype. If the
    // current prototype already equals `proto`, the no-op set still succeeds.
    if crate::object::obj_value_no_extend(target) {
        let current = crate::object::js_object_get_prototype_of(target);
        if current.to_bits() != proto_bits {
            return nanbox_bool(false);
        }
        return nanbox_bool(true);
    }

    // Apply via the shared Object-side helper (records in the side-table).
    crate::object::js_object_set_prototype_of(target, proto);
    nanbox_bool(true)
}
