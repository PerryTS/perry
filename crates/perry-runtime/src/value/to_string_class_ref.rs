//! `ToPrimitive` for INT32-tagged constructor ClassRefs (#9125).
//!
//! Split out of `to_string.rs`, which sits at the 2000-line cap. ClassRefs are
//! objects at the language level but are not pointer-tagged in Perry, so the
//! generic pointer-only coercion ladders cannot discover them; this module is
//! the one place that representation exception lives.

use super::to_string::{
    is_primitive_value, ordinary_to_primitive_for_toprimitive, throw_cannot_convert_to_primitive,
};
use super::*;

pub(crate) enum CustomToPrimitiveOutcome {
    Absent,
    Primitive(f64),
    TypeError,
}
pub(crate) unsafe fn custom_to_primitive(value: f64, hint: &[u8]) -> CustomToPrimitiveOutcome {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_handle = scope.root_nanbox_f64(value);
    let to_primitive = crate::symbol::well_known_symbol("toPrimitive");
    let sym_value = f64::from_bits(POINTER_TAG | (to_primitive as u64 & POINTER_MASK));
    let method =
        crate::symbol::js_object_get_symbol_property(value_handle.get_nanbox_f64(), sym_value);
    let method_jsv = JSValue::from_bits(method.to_bits());
    if method_jsv.is_undefined() || method_jsv.is_null() {
        return CustomToPrimitiveOutcome::Absent;
    }

    let method_bits = method.to_bits();
    if (method_bits & 0xFFFF_0000_0000_0000) != POINTER_TAG {
        return CustomToPrimitiveOutcome::TypeError;
    }
    let method_handle = scope.root_nanbox_f64(method);
    let hint_ptr = crate::string::js_string_from_bytes(hint.as_ptr(), hint.len() as u32);
    let hint_handle = scope.root_string_ptr(hint_ptr);
    // Scoped: the tag combine below allocates nothing, so the pointer cannot
    // go stale inside the closure (#7341).
    let hint = hint_handle.with_const_ptr(|p: *const crate::string::StringHeader| {
        f64::from_bits(STRING_TAG | (p as u64 & POINTER_MASK))
    });
    let receiver = value_handle.get_nanbox_f64();
    let method = method_handle.get_nanbox_f64();
    let result = if crate::proxy::js_proxy_is_proxy(method) == 1 {
        if !crate::proxy::proxy_wraps_callable(method) {
            return CustomToPrimitiveOutcome::TypeError;
        }
        crate::proxy::call_proxy_value_with_this(method, receiver, &[hint])
    } else {
        let method_ptr = (method.to_bits() & POINTER_MASK) as usize;
        if !crate::closure::is_closure_ptr(method_ptr) {
            return CustomToPrimitiveOutcome::TypeError;
        }
        let prev_this = crate::object::js_implicit_this_set(receiver);
        let result = crate::closure::js_native_call_value(method, &hint, 1);
        crate::object::js_implicit_this_set(prev_this);
        result
    };

    if is_primitive_value(result) {
        CustomToPrimitiveOutcome::Primitive(result)
    } else {
        CustomToPrimitiveOutcome::TypeError
    }
}
/// Run full `ToPrimitive` for an INT32-tagged constructor ClassRef.
///
/// ClassRefs are objects at the language level but are not pointer-tagged in
/// Perry, so the generic pointer-only coercion ladders cannot safely discover
/// them. Keep the representation exception in one place: consult the static
/// `Symbol.toPrimitive` hook first, then perform the ordinary Function-object
/// `valueOf`/`toString` sequence with the hint-mandated ordering.
pub(crate) unsafe fn class_ref_to_primitive(value: f64, hint: i32) -> f64 {
    let (hint_name, string_first): (&[u8], bool) = match hint {
        1 => (b"number", false),
        2 => (b"string", true),
        _ => (b"default", false),
    };
    match custom_to_primitive(value, hint_name) {
        CustomToPrimitiveOutcome::Absent => {
            ordinary_to_primitive_for_toprimitive(value, string_first)
        }
        CustomToPrimitiveOutcome::Primitive(p) => p,
        CustomToPrimitiveOutcome::TypeError => throw_cannot_convert_to_primitive(),
    }
}
