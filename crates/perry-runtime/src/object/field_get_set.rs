//! Indexed and named field get/set: the inline-cache hot path
//! (`js_object_get_field_by_name`, `js_object_get_field_ic_miss`,
//! `js_object_set_field_by_name`), plus keys/values/entries/has_property
//! and the polymorphic index accessors.
//!
//! Split out of `object.rs` (issue #1103). Pure relocation — no logic
//! changes.

use super::*;

/// Hidden own-field name under which a `class X extends Request/Response`
/// instance stashes the id of its underlying native Web-Fetch handle. Written
/// by the `js_request_subclass_init` / `js_response_subclass_init` super-init
/// shims (global_this.rs); read here (property forward), in
/// `native_call_method.rs` (body-method forward), and in `instanceof.rs`
/// (`x instanceof Request/Response`). A Request/Response is a registry-backed
/// native handle, not a heap object whose methods live on the JS prototype
/// chain, so a subclass instance can only reach those members via the handle.
pub(crate) const FETCH_SUBCLASS_HANDLE_FIELD: &[u8] = b"__perry_fetch_handle__";

/// If `obj` (a raw heap object address) is a `class X extends Request/Response`
/// instance, return the id of its underlying native fetch handle. Returns
/// `None` for any non-object / non-subclass receiver, so callers can fall
/// through to their normal dispatch unchanged.
pub(crate) unsafe fn fetch_subclass_handle_id(obj: usize) -> Option<i64> {
    if obj < crate::gc::GC_HEADER_SIZE + 0x1000 || !is_valid_obj_ptr(obj as *const u8) {
        return None;
    }
    let gc_header = (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT {
        return None;
    }
    let key = crate::string::js_string_from_bytes(
        FETCH_SUBCLASS_HANDLE_FIELD.as_ptr(),
        FETCH_SUBCLASS_HANDLE_FIELD.len() as u32,
    );
    let v = js_object_get_field_by_name(obj as *const ObjectHeader, key);
    if v.is_undefined() {
        return None;
    }
    let id = f64::from_bits(v.bits());
    if id.is_finite() && id > 0.0 && id.fract() == 0.0 {
        Some(id as i64)
    } else {
        None
    }
}

/// Hidden own-field name under which a `class X extends Temporal.<Type>`
/// instance stashes the NaN-boxed pointer to its underlying Temporal cell.
/// Written by `js_fetch_or_value_super` (the runtime-value super dispatcher,
/// global_this/fetch_globals.rs) when the resolved parent is a Temporal
/// constructor; read here (getter forward), in `native_call_method.rs`
/// (method forward), and in `instanceof.rs`. A Temporal value is a NaN-boxed
/// cell that dispatches via brand arms, not a JS prototype chain, so a subclass
/// instance (a plain heap object) can only reach its members through this
/// stashed cell. Stored as a real pointer-valued field so GC keeps the cell
/// alive and rewrites the slot on evacuation. (#5587)
#[cfg(feature = "temporal")]
pub(crate) const TEMPORAL_SUBCLASS_CELL_FIELD: &[u8] = b"__perry_temporal_cell__";

/// If `obj` (a raw heap object address) is a `class X extends Temporal.<Type>`
/// instance, return the NaN-boxed value of its stashed Temporal cell. Returns
/// `None` for any non-object / non-subclass receiver (so callers fall through
/// to their normal dispatch unchanged) or if the stashed value is somehow no
/// longer a live Temporal cell.
#[cfg(feature = "temporal")]
pub(crate) unsafe fn temporal_subclass_cell(obj: usize) -> Option<f64> {
    // Reject any address that isn't a plausible heap pointer.  Proxy ids live
    // in [0xF0000, 0x100000) — they pass a naïve `>= GC_HEADER_SIZE + 0x1000`
    // check but are NOT heap pointers.  On Linux (HEAP_MIN = 0x1000) the old
    // `is_valid_obj_ptr` guard passed them too, causing a SIGSEGV when the
    // GC header was read at (proxy_id − 8).  `is_plausible_heap_addr` rejects
    // the entire handle band [0, 0x100000) unconditionally.
    if !crate::value::addr_class::is_plausible_heap_addr(obj) {
        return None;
    }
    let gc_header = (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT {
        return None;
    }
    let key = crate::string::js_string_from_bytes(
        TEMPORAL_SUBCLASS_CELL_FIELD.as_ptr(),
        TEMPORAL_SUBCLASS_CELL_FIELD.len() as u32,
    );
    let v = js_object_get_field_by_name(obj as *const ObjectHeader, key);
    if v.is_undefined() {
        return None;
    }
    let boxed = f64::from_bits(v.bits());
    if crate::temporal::is_temporal_value(boxed) {
        Some(boxed)
    } else {
        None
    }
}

/// The Web-Fetch body-reading methods (`text`/`json`/`arrayBuffer`/`blob`/
/// `bytes`/`formData`/`clone`). On a `class X extends Request/Response`
/// instance these live on the underlying native handle, not the JS prototype
/// chain, so they must be made readable as callable VALUES (see the property
/// forward in `js_object_get_field_by_name`). Mirrors the set in
/// `native_call_method.rs` (the fused-call body-method forward).
pub(crate) fn is_fetch_subclass_body_method(name: &[u8]) -> bool {
    matches!(
        name,
        b"text" | b"json" | b"arrayBuffer" | b"blob" | b"bytes" | b"formData" | b"clone"
    )
}

// ── Topical sub-modules (issue #1103: keep every file < 2000 lines) ──
mod accessors;
mod buffer_own_prop;
mod class_object_props;
mod crypto_key;
mod enumeration;
mod field_ops;
mod get_field_by_name;
mod get_field_by_name_tail;
mod has_property;
mod ic_miss;
mod map_set_receiver;

// Explicit named re-exports so existing `crate::object::…` / `super::…`
// paths keep resolving (a glob re-export does not reliably propagate through
// `object/mod.rs`'s `pub use field_get_set::*`), and so sibling modules can
// reach the cross-module helpers via their own `use super::*;`.
pub use accessors::js_object_get_field;
pub(crate) use accessors::{
    accessor_receiver_override_begin, accessor_receiver_override_end,
    array_prototype_property_value, builtin_reflection_accessor_read, class_getter_this,
    invoke_accessor_getter, invoke_accessor_setter, is_typed_array_prototype,
    ordinary_object_prototype_property_value, own_data_field_by_name,
    primitive_builtin_prototype_property, primitive_object_prototype_accessor, string_index_value,
};
pub(crate) use crypto_key::{
    crypto_key_property_value, CLASS_ID_BOXED_BIGINT, CLASS_ID_BOXED_BOOLEAN,
    CLASS_ID_BOXED_NUMBER, CLASS_ID_BOXED_STRING, CLASS_ID_BOXED_SYMBOL,
};
pub(crate) use enumeration::{
    canonical_array_index, descriptor_marks_non_enumerable, ecma_own_key_order,
    instance_private_key_hidden, is_internal_runtime_key, is_internal_runtime_key_bytes,
    keys_contain_array_index,
};
pub use enumeration::{
    js_for_in_keys_value, js_object_entries, js_object_entries_value, js_object_keys,
    js_object_keys_value, js_object_values, js_object_values_value,
};
pub use field_ops::{
    js_object_free, js_object_get_class_id, js_object_get_field_f64,
    js_object_get_unboxed_f64_field, js_object_set_field, js_object_set_field_by_index,
    js_object_set_field_f64, js_object_set_keys, js_object_set_unboxed_f64_field,
    js_object_to_value, js_value_to_object,
};
pub use get_field_by_name::js_object_get_field_by_name;
pub(crate) use get_field_by_name_tail::get_field_by_name_object_tail;
pub(super) use has_property::native_module_own_field_by_key;
pub(crate) use has_property::{
    closure_dynamic_prop_by_key, reified_function_method_name, wide_key_index_lookup,
    wide_key_index_note_hit, WIDE_KEY_INDEX_MIN_KEYS,
};
pub use has_property::{js_in_operator, js_object_has_property};
pub(crate) use ic_miss::{
    is_array_method_value_name, is_primitive_proto_method, is_timer_handle_method_key,
    set_method_value_name,
};
pub use ic_miss::{
    js_object_get_field_by_name_f64, js_object_get_field_by_property_id_f64,
    js_object_get_field_ic_miss, js_object_set_field_by_property_id, js_private_brand_check,
    js_private_guard,
};

#[cfg(test)]
mod buffer_ic_miss_tests {
    use super::*;

    unsafe fn key(bytes: &[u8]) -> *const crate::StringHeader {
        crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
    }

    unsafe fn string_value_bytes(value: f64) -> Vec<u8> {
        let bits = value.to_bits();
        assert_eq!((bits >> 48) as u16, 0x7fff);
        let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::StringHeader;
        let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        std::slice::from_raw_parts(data, (*ptr).byte_len as usize).to_vec()
    }

    unsafe fn secret_buffer(len: usize) -> *mut crate::buffer::BufferHeader {
        let buf = crate::buffer::buffer_alloc(len as u32);
        (*buf).length = len as u32;
        crate::buffer::mark_as_uint8array(buf as usize);
        crate::buffer::mark_as_secret_key(buf as usize);
        buf
    }

    #[test]
    fn secret_key_buffer_metadata_survives_ic_miss_for_aes_sizes() {
        unsafe {
            for len in [16usize, 24, 32] {
                let buf = secret_buffer(len);
                let mut cache = [0i64; 2];

                let ty = js_object_get_field_ic_miss(
                    buf as *const ObjectHeader,
                    key(b"type"),
                    &mut cache,
                );
                assert_eq!(string_value_bytes(ty), b"secret");

                let size = js_object_get_field_ic_miss(
                    buf as *const ObjectHeader,
                    key(b"symmetricKeySize"),
                    &mut cache,
                );
                assert_eq!(size, len as f64);

                let raw = dispatch_buffer_method(buf as usize, "export", std::ptr::null(), 0);
                let raw_addr = (raw.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const ObjectHeader;
                let raw_len = js_object_get_field_ic_miss(raw_addr, key(b"length"), &mut cache);
                assert_eq!(raw_len, len as f64);
            }
        }
    }
}
