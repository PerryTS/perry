//! Callback-free metadata reads and the ordinary Get fallback for typed views.
//!
//! The type byte proves the receiver layout; annotations do not. No address or
//! backing pointer is cached. Detach/resize update the live length field, and
//! offset reads use the existing view metadata. Native arena views keep their
//! validating generic path.

use crate::value::JSValue;
use std::sync::atomic::{AtomicU8, Ordering};

/// Sticky withdrawal of the default typed-view metadata accessor proof.
/// This carries no heap pointers and needs no GC registration. Mutations are
/// rare; conservatively withdraw for all views when any relevant owner changes.
#[no_mangle]
pub static PERRY_TYPED_NAMED_PROPS_INVALIDATED: AtomicU8 = AtomicU8::new(0);

#[inline]
pub(crate) fn is_metadata_name(name: &[u8]) -> bool {
    matches!(name, b"length" | b"byteLength" | b"byteOffset")
}

fn is_buffer_prototype(owner: usize) -> bool {
    crate::object::native_module::cached_buffer_intrinsic_prototype_value().is_some_and(|value| {
        let value = JSValue::from_bits(value.to_bits());
        value.is_pointer() && value.as_pointer::<u8>() as usize == owner
    })
}

/// Called before publication by ordinary setters, descriptors and deletion.
/// Bootstrap installs intrinsic accessors before publishing the intrinsic
/// pointer; those installs cannot invalidate a previously observed proof.
pub(crate) fn note_named_mutation(owner: usize, name: &[u8]) {
    if !is_metadata_name(name) {
        return;
    }
    unsafe {
        let Some(header) = crate::value::addr_class::try_read_gc_header(owner) else {
            return;
        };
        let prototype = header.obj_type == crate::gc::GC_TYPE_OBJECT
            && (crate::object::is_typed_array_prototype(owner) || is_buffer_prototype(owner));
        if prototype {
            PERRY_TYPED_NAMED_PROPS_INVALIDATED.store(1, Ordering::Release);
        }
    }
}

pub(crate) fn note_prototype_mutation(owner: usize, user_override: bool) {
    // Reflect.construct also links a fresh view through the runtime-wiring
    // path. That custom prototype must withdraw the default accessor proof.
    // Bootstrap links between builtin prototypes do not change a view's
    // default chain; only a user retarget of those prototypes withdraws it.
    if user_override {
        note_named_mutation(owner, b"length");
    }
}

/// A positive, noncollecting read. None means full Get, never "absent".
#[inline]
pub(crate) unsafe fn try_get(receiver: JSValue, name: &[u8]) -> Option<f64> {
    if !is_metadata_name(name)
        || !receiver.is_pointer()
        || PERRY_TYPED_NAMED_PROPS_INVALIDATED.load(Ordering::Acquire) != 0
    {
        return None;
    }
    let addr = receiver.as_pointer::<u8>() as usize;
    let header = crate::value::addr_class::try_read_gc_header(addr)?;
    if header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
        return None;
    }
    let obj_type = crate::buffer::header::byte_cell_type(addr)?;
    // Shadow guard: own properties and a custom prototype live only in the
    // bag, so a cell without one (an owner with link 0, a view linked straight
    // to its owner) has neither; that is one load of the link.
    let bag = crate::buffer::store::bag(addr);
    if !bag.is_null()
        && (crate::buffer::store::bag_holds(bag, name)
            || crate::buffer::store::bag_holds(bag, crate::buffer::store::PROTOTYPE_KEY.as_bytes()))
    {
        return None;
    }
    match obj_type & !crate::codegen_abi::BYTES_TYPE_VIEW {
        crate::gc::GC_TYPE_BUFFER | crate::gc::GC_TYPE_BUFFER_UINT8ARRAY => {
            let buf = addr as *const crate::buffer::BufferHeader;
            Some(if name == b"byteOffset" {
                crate::buffer::buffer_byte_offset(addr) as f64
            } else {
                crate::buffer::store::length(buf as usize) as f64
            })
        }
        t if crate::gc::is_typed_array_type(t) => {
            let ta = addr as *const crate::typedarray::TypedArrayHeader;
            Some(match name {
                b"byteOffset" => crate::typedarray_view::js_typed_array_byte_offset(ta) as f64,
                b"byteLength" => {
                    crate::typedarray::element_length(ta) as f64
                        * crate::typedarray::elem_size_for_kind(crate::typedarray::element_kind(ta))
                            as f64
                }
                _ => crate::typedarray::element_length(ta) as f64,
            })
        }
        _ => None,
    }
}

/// Known metadata names after the proof was withdrawn. Resolve own properties
/// and the actual prototype chain, with the original view as getter receiver.
/// Both inputs stay rooted across bootstrap and arbitrary user getters.
pub(crate) unsafe fn get(receiver: f64, key: *const crate::StringHeader) -> Option<f64> {
    let bytes =
        std::slice::from_raw_parts(crate::string::string_data(key), (*key).byte_len as usize);
    if !is_metadata_name(bytes) {
        return None;
    }
    let value = JSValue::from_bits(receiver.to_bits());
    if let Some(result) = try_get(value, bytes) {
        return Some(result);
    }
    if !value.is_pointer() {
        return None;
    }
    let addr = value.as_pointer::<u8>() as usize;
    let header = crate::value::addr_class::try_read_gc_header(addr)?;
    // Let the ordinary by-name walk clean forwarded receivers before Get.
    if header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
        return None;
    }
    let prototype_name = match header.obj_type & !crate::codegen_abi::BYTES_TYPE_VIEW {
        crate::gc::GC_TYPE_BUFFER => "Buffer",
        crate::gc::GC_TYPE_BUFFER_UINT8ARRAY => "Uint8Array",
        t if crate::gc::is_typed_array_type(t) => crate::typedarray::name_for_kind(
            crate::typedarray::element_kind(addr as *const crate::typedarray::TypedArrayHeader),
        ),
        _ => return None,
    };
    // Known ASCII name: no UTF-8 validation or CanonicalNumericIndexString work.
    let name = match bytes {
        b"length" => "length",
        b"byteLength" => "byteLength",
        _ => "byteOffset",
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let key = scope.root_raw_const_ptr(key);
    let own = if matches!(
        header.obj_type,
        crate::gc::GC_TYPE_BUFFER | crate::gc::GC_TYPE_BUFFER_UINT8ARRAY
    ) {
        crate::buffer::buffer_read_own_prop(addr, name)
    } else {
        crate::typedarray_props::typed_array_get_property_value_by_name(addr, name)
    };
    if own.is_some() {
        return own;
    }
    let proto = match crate::object::prototype_chain::object_static_prototype(addr) {
        Some(bits) => f64::from_bits(bits),
        None if prototype_name == "Buffer" => {
            crate::object::native_module::buffer_intrinsic_prototype_value()
        }
        None => crate::object::builtin_prototype_value(prototype_name),
    };
    let addr = JSValue::from_bits(receiver.get_nanbox_f64().to_bits()).as_pointer::<u8>() as usize;
    Some(
        crate::object::prototype_chain::resolve_inherited_field_from_prototype(
            addr,
            proto.to_bits(),
            key.get_raw_const_ptr(),
        )
        .map(|v| f64::from_bits(v.bits()))
        .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED)),
    )
}

#[cfg(test)]
#[path = "typedarray_named_tests.rs"]
mod tests;
