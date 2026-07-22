//! `Object.*` static methods and descriptor machinery:
//! `Object.fromEntries`/`groupBy`/`is`/`hasOwn`/`create`/`freeze`/`seal`/
//! `defineProperty`/`getOwnPropertyDescriptor`/`getPrototypeOf`/... plus
//! the `js_object_*` helpers backing them.
//!
//! The bulk of this module was split out into sibling files for size; this
//! trunk keeps the two shared pointer helpers (`extract_obj_ptr` /
//! `gc_header_for`) and re-exports the moved items so existing call paths like
//! `crate::object::object_ops::<Item>` keep resolving.
use super::*;

mod accessors;
mod define_properties;
mod define_property;
mod descriptor_helpers;
mod from_entries;
mod has_own;
mod keys_array;
mod prototype;

// `#[no_mangle] pub extern "C"` FFI entry points keep their `pub` visibility so
// the parent's `pub use object_ops::*` glob re-exports them crate-publicly.
pub use accessors::{
    js_object_define_getter, js_object_define_setter, js_object_get_own_field_or_undef,
    js_object_lookup_getter, js_object_lookup_setter,
};
pub use define_properties::{js_object_define_properties, js_object_set_prototype_of};
pub use define_property::js_object_define_property;
pub use from_entries::js_object_from_entries;
pub use has_own::{js_object_has_own, js_object_is, js_object_property_is_enumerable};
pub use prototype::{
    js_get_global_this_builtin_value, js_object_create, js_object_get_prototype_of,
};

// Internal `pub(crate)` helpers shared between siblings / the rest of the crate.
pub(crate) use descriptor_helpers::{
    define_property_force_store_value, desc_has_field, desc_read_field,
    describe_value_for_type_error, descriptor_enumerable, enforce_define_property_invariants,
    registered_buffer_index_own_property_present, throw_object_type_error,
    throw_object_type_error_with_suffix, validate_nonconfigurable_redefine,
    validate_property_descriptor, value_is_object_like,
};
// Module-private `unsafe fn value_is_callable` (descriptor_helpers): used by the
// object_ops children (`accessors.rs`, `descriptor_helpers.rs`) but NOT
// re-exported, so `crate::object::value_is_callable` resolves uniquely to the
// `instanceof.rs` definition (preserves the pre-split resolution).
pub(crate) use keys_array::{
    ensure_key_in_keys_array, install_builtin_getter, own_key_present, own_key_present_via_index,
};

/// Helper: extract object pointer from NaN-boxed f64. Returns null on failure.
pub(crate) unsafe fn extract_obj_ptr(value: f64) -> *mut ObjectHeader {
    let jsval = crate::JSValue::from_bits(value.to_bits());
    if jsval.is_pointer() {
        let ptr = jsval.as_pointer::<ObjectHeader>() as *mut ObjectHeader;
        // A POINTER_TAG payload is not always an address. zlib streams, fetch
        // Request/Response/Headers/Blob, net.Socket, crypto hashes and revocable
        // Proxies all smuggle small registry *handles* under the same tag (see
        // `value::addr_class` for the band map). A handle must never be
        // dereferenced. Return null so every caller takes the `is_null()` path
        // it already has, instead of reading unmapped low memory.
        //
        // This is the central fix for the #6271 crash class: `is_valid_obj_ptr`
        // does NOT reject the handle band on Linux/Windows/Android/iOS (heap
        // floor 0x1000, far below HANDLE_BAND_MAX), so callers that guard with
        // it alone deref the handle and segfault there — while macOS silently
        // takes the null path behind its 2 TB heap floor. Rejecting here makes
        // every platform agree on the already-correct macOS behaviour.
        if !crate::value::addr_class::is_above_handle_band(ptr as usize) {
            return ptr::null_mut();
        }
        ptr
    } else {
        let bits = value.to_bits();
        // Raw-I64-pointer fallback (module-level array/object vars store the
        // untagged pointer directly). Every GC allocation is `align.max(8)`-
        // aligned, so a real object pointer always has its low 3 bits clear.
        // Requiring alignment here rejects non-object values whose raw bits
        // merely *land* in the address range — e.g. a native-module namespace
        // sentinel (`require('buffer')`) reaching a generic object op like
        // `hasOwnProperty`. Without it, callers deref `[ptr-8]` for the
        // GcHeader on a misaligned garbage address → SIGBUS (#3527).
        if bits != 0
            && bits <= 0x0000_FFFF_FFFF_FFFF
            && crate::value::addr_class::is_above_handle_band(bits as usize)
            && bits & 0x7 == 0
        {
            bits as *mut ObjectHeader
        } else {
            ptr::null_mut()
        }
    }
}

/// Helper: get GcHeader for an object pointer
pub(super) unsafe fn gc_header_for(obj: *const ObjectHeader) -> *mut crate::gc::GcHeader {
    (obj as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader
}
