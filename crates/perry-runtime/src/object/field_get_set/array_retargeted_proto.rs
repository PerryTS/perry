//! #9192: array property slots that a retargeted `[[Prototype]]` changes.
//!
//! Split out of `get_field_by_name_tail.rs`, which is at the 2000-line cap.
//! Both slots answer off the static-prototype side table rather than the
//! implicit array chain, because a recorded custom `[[Prototype]]` replaces
//! that chain entirely.

use super::accessors::array_prototype_property_value;
use crate::value::JSValue;

/// `arr.__proto__` IS the array's `[[Prototype]]` — the spec models it as an
/// `Object.prototype` accessor returning `[[GetPrototypeOf]](this)`. Without
/// this a retargeted array reported the WRONG object while
/// `Object.getPrototypeOf(arr)` reported the right one.
pub(super) fn array_proto_slot(obj: *const crate::object::ObjectHeader) -> JSValue {
    // `__proto__` itself lives on `Object.prototype`, so an array whose chain
    // no longer reaches it (an explicit null prototype) has no such property.
    if crate::object::prototype_chain::object_static_prototype(obj as usize)
        == Some(crate::value::TAG_NULL)
    {
        return JSValue::undefined();
    }
    let receiver = crate::value::js_nanbox_pointer(obj as i64);
    let proto =
        crate::object::object_ops::js_object_get_prototype_of(f64::from_bits(receiver.to_bits()));
    JSValue::from_bits(proto.to_bits())
}

/// A recorded custom `[[Prototype]]` replaces the whole implicit chain, so
/// `constructor` must resolve through it — a plain `{}` prototype answers
/// `Object`, not `Array` — rather than short-circuiting to the global `Array`.
/// `None` means no retarget was recorded and the caller keeps its fast path.
pub(super) fn array_constructor_slot(obj: *const crate::object::ObjectHeader) -> Option<JSValue> {
    crate::object::prototype_chain::object_static_prototype(obj as usize)?;
    // SAFETY: the caller has already established `obj` as a live array
    // header; this reads the recorded prototype's own properties.
    Some(
        unsafe { array_prototype_property_value("constructor", obj as usize) }
            .unwrap_or_else(JSValue::undefined),
    )
}
