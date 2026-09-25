//! Buffer own-property / method-value reads for the object-deref tail.
//!
//! Split out of `get_field_by_name_tail.rs` to keep that file under the
//! 2000-line budget (it is a single very large function).

use super::*;

/// Node's Buffer IS an object (a Uint8Array), so user code can store properties
/// on one — and an own key SHADOWS the prototype method of the same name. Perry
/// keeps buffers outside the object model, so both halves were missing:
/// `buf.foo = v` was dropped and `typeof buf.writeInt8` read `undefined`
/// (methods dispatched on CALL only).
///
/// mysql2 sizes every outgoing packet with exactly that idiom (`MockBuffer`):
/// it walks `Packet.prototype`, replaces the matching write methods on a
/// ZERO-LENGTH Buffer with a no-op, serializes once to MEASURE, then allocates
/// for real. With the `typeof mock[k] === "function"` probe false, nothing was
/// replaced, the measuring pass wrote into the empty Buffer, and the MySQL
/// handshake died with RangeError [ERR_OUT_OF_RANGE].
///
/// Returns `None` when the key names neither an own property nor a Buffer
/// method, so the caller falls through to the rest of the property walk.
pub(super) fn buffer_own_prop_or_method(
    obj: *const ObjectHeader,
    key_bytes: &[u8],
) -> Option<JSValue> {
    let name = std::str::from_utf8(key_bytes).ok()?;
    if let Some(accessor) = super::get_accessor_descriptor(obj as usize, name) {
        if accessor.get == 0 {
            return Some(JSValue::undefined());
        }
        let receiver = crate::value::js_nanbox_pointer(obj as i64);
        return Some(unsafe { super::invoke_accessor_getter(accessor.get, receiver) });
    }
    if let Some(v) = crate::buffer::buffer_get_own_prop(obj as usize, name) {
        return Some(JSValue::from_bits(v.to_bits()));
    }
    // The bound closure keeps the name POINTER and re-reads it at call time
    // (`dispatch_bound_method`), so it must not point into the key string:
    // that is a movable GC heap allocation, and `key_bytes` borrows its
    // interior. Bind the `'static` literal instead.
    if let Some(method) = crate::object::buffer_dispatch::buffer_method_name_static(name) {
        let bound = crate::object::js_class_method_bind(
            crate::value::js_nanbox_pointer(obj as i64),
            method.as_ptr(),
            method.len(),
        );
        return Some(JSValue::from_bits(bound.to_bits()));
    }
    None
}

/// Resolve a DataView constructor after the caller has checked own properties.
/// An explicit prototype (even null or one without a constructor) replaces the
/// intrinsic chain. The shared walker preserves the view as an accessor receiver.
pub(super) fn data_view_constructor(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> JSValue {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver_h = scope.root_raw_const_ptr(obj);
    let key_h = scope.root_raw_const_ptr(key);
    let proto_bits = receiver_h
        .with_const_ptr(|receiver: *const ObjectHeader| {
            super::super::prototype_chain::object_static_prototype(receiver as usize)
        })
        .unwrap_or_else(|| crate::object::builtin_prototype_value("DataView").to_bits());
    // Re-read both pointers after materializing the intrinsic prototype, which
    // can allocate. Neither raw pointer is retained across that operation.
    receiver_h.with_const_ptr(|receiver: *const ObjectHeader| {
        key_h.with_const_ptr(|key: *const crate::StringHeader| {
            super::super::prototype_chain::resolve_inherited_field_from_prototype(
                receiver as usize,
                proto_bits,
                key,
            )
            .unwrap_or_else(JSValue::undefined)
        })
    })
}

#[cfg(test)]
#[path = "data_view_constructor_tests.rs"]
mod data_view_constructor_tests;
