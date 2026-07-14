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
    key_ptr: *const u8,
    key_len: usize,
) -> Option<JSValue> {
    let name = std::str::from_utf8(key_bytes).ok()?;
    if let Some(v) = crate::buffer::buffer_get_own_prop(obj as usize, name) {
        return Some(JSValue::from_bits(v.to_bits()));
    }
    if crate::object::buffer_dispatch::is_buffer_method_name(name) {
        let bound = crate::object::js_class_method_bind(
            crate::value::js_nanbox_pointer(obj as i64),
            key_ptr,
            key_len,
        );
        return Some(JSValue::from_bits(bound.to_bits()));
    }
    None
}
