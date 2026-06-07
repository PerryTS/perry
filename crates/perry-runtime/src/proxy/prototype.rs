//! Proxy `[[SetPrototypeOf]]` (ECMA-262 §10.5.2) and the `Reflect.setPrototypeOf`
//! entry point. Split out of `proxy.rs` to keep that file under the size gate.

use super::{
    call_trap, handler_trap, is_callable_function, lookup, nanbox_bool,
    reflect_non_object_typeerror, reflect_target_get_prototype_of, reflect_value_is_object,
    revoked_return, throw_type_error, PROXIES, TAG_NULL, TAG_UNDEFINED,
};

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

/// Proxy `[[SetPrototypeOf]]` (ECMA-262 §10.5.2): invoke the `setPrototypeOf`
/// trap (bound to the handler) when present, otherwise forward to the target's
/// `[[SetPrototypeOf]]` (recursing through proxy targets). Enforces the
/// non-extensible-target invariant: a `true` result requires the new proto to
/// SameValue the target's current proto.
fn proxy_set_prototype_of(proxy_boxed: f64, proto: f64) -> f64 {
    let id = match lookup(proxy_boxed) {
        Some(id) => id,
        None => return nanbox_bool(false),
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
    let trap = handler_trap(handler, "setPrototypeOf");
    let trap_bits = trap.to_bits();
    if trap_bits == TAG_UNDEFINED || trap_bits == TAG_NULL {
        // No trap — forward to the target's [[SetPrototypeOf]].
        return js_reflect_set_prototype_of(target, proto);
    }
    if !is_callable_function(trap) {
        return throw_type_error("proxy setPrototypeOf trap is not a function");
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let target_h = scope.root_nanbox_f64(target);
    let proto_h = scope.root_nanbox_f64(proto);
    let trap_result = call_trap(
        handler,
        trap,
        &[target_h.get_nanbox_f64(), proto_h.get_nanbox_f64()],
    );
    if crate::value::js_is_truthy(trap_result) == 0 {
        return nanbox_bool(false);
    }
    // Invariant: if the target is non-extensible, the proto must not change.
    let target = target_h.get_nanbox_f64();
    let proto = proto_h.get_nanbox_f64();
    if crate::object::obj_value_no_extend(target) {
        let current = reflect_target_get_prototype_of(target);
        if current.to_bits() != proto.to_bits() {
            return throw_type_error(
                "proxy setPrototypeOf trap violates non-extensible target invariant",
            );
        }
    }
    nanbox_bool(true)
}
