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
    // IntegerIndexedExotic [[Get]] is answered by the holder's bytes before
    // ordinary expandos, including canonical-invalid indices.
    if let Some(value) =
        unsafe { crate::typedarray_props::byte_buffer_index_get_by_name(obj as usize, name) }
    {
        return Some(JSValue::from_bits(value.to_bits()));
    }
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

/// The intrinsic constructor name of an ArrayBuffer / SharedArrayBuffer /
/// DataView cell (the kinds `buffer::is_non_indexed_buffer_view` admits).
pub(crate) fn view_kind_name(addr: usize) -> &'static str {
    if crate::buffer::is_data_view(addr) {
        "DataView"
    } else if crate::buffer::is_shared_array_buffer(addr) {
        "SharedArrayBuffer"
    } else {
        "ArrayBuffer"
    }
}

/// [[Prototype]] of an ArrayBuffer / SharedArrayBuffer / DataView: the recorded
/// one (`Object.setPrototypeOf`, a subclass), else its kind's intrinsic.
pub(crate) fn view_prototype_bits(addr: usize) -> u64 {
    super::super::prototype_chain::object_static_prototype(addr)
        .unwrap_or_else(|| crate::object::builtin_prototype_value(view_kind_name(addr)).to_bits())
}

/// A getter of the view kind's own prototype, answered from the cell. Only the
/// accessors node puts on that kind's prototype: `ArrayBuffer.prototype`
/// {resizable, maxByteLength, detached}, `SharedArrayBuffer.prototype`
/// {growable, maxByteLength}, `DataView.prototype` {buffer, byteOffset}
/// (`byteLength` is answered before this for every byte cell).
fn view_kind_getter(addr: usize, key_bytes: &[u8]) -> Option<JSValue> {
    let len = || crate::buffer::js_buffer_length(addr as *const crate::buffer::BufferHeader);
    if crate::buffer::is_data_view(addr) {
        return match key_bytes {
            b"buffer" => Some(JSValue::from_bits(
                crate::value::js_nanbox_pointer(
                    crate::buffer::buffer_backing_array_buffer(addr) as i64
                )
                .to_bits(),
            )),
            b"byteOffset" => Some(JSValue::number(crate::buffer::buffer_byte_offset(addr) as f64)),
            _ => None,
        };
    }
    if crate::buffer::is_shared_array_buffer(addr) {
        return match key_bytes {
            b"growable" => Some(JSValue::bool(crate::buffer::is_resizable_buffer(addr))),
            b"maxByteLength" => Some(JSValue::number(
                crate::buffer::resizable_max_byte_length(addr).unwrap_or_else(|| len() as u32)
                    as f64,
            )),
            _ => None,
        };
    }
    match key_bytes {
        b"resizable" => Some(JSValue::bool(crate::buffer::is_resizable_buffer(addr))),
        b"detached" => Some(JSValue::bool(crate::buffer::is_detached_buffer(addr))),
        b"maxByteLength" => {
            if crate::buffer::is_detached_buffer(addr) {
                return Some(JSValue::number(0.0));
            }
            Some(JSValue::number(
                crate::buffer::resizable_max_byte_length(addr).unwrap_or_else(|| len() as u32)
                    as f64,
            ))
        }
        _ => None,
    }
}

/// [[Get]] of a named key on an ArrayBuffer / SharedArrayBuffer / DataView.
/// They are ordinary objects: an own property, else the receiver's
/// [[Prototype]] chain, whose intrinsic holds exactly node's members. The
/// kind's own accessors are answered from the cell (the values the prototype
/// getters compute). Never the Node `Buffer` member table.
pub(crate) fn view_get(obj: *const ObjectHeader, key: *const crate::StringHeader) -> JSValue {
    let addr = obj as usize;
    if !key.is_null() {
        let key_bytes = unsafe {
            std::slice::from_raw_parts(
                (key as *const u8).add(std::mem::size_of::<crate::StringHeader>()),
                (*key).byte_len as usize,
            )
        };
        if let Ok(name) = std::str::from_utf8(key_bytes) {
            if let Some(v) = crate::buffer::buffer_read_own_prop(addr, name) {
                return JSValue::from_bits(v.to_bits());
            }
        }
        if let Some(v) = view_kind_getter(addr, key_bytes) {
            return v;
        }
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver_h = scope.root_raw_const_ptr(obj);
    let key_h = scope.root_raw_const_ptr(key);
    let proto_bits = receiver_h
        .with_const_ptr(|receiver: *const ObjectHeader| view_prototype_bits(receiver as usize));
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

#[cfg(test)]
#[path = "buffer_proto_index_tests.rs"]
mod buffer_proto_index_tests;
