//! Indexed and named field get/set: the inline-cache hot path
//! (`js_object_get_field_by_name`, `js_object_get_field_ic_miss`,
//! `js_object_set_field_by_name`), plus keys/values/entries/has_property
//! and the polymorphic index accessors.
//!
//! Split out of `object.rs` (issue #1103). Pure relocation — no logic
//! changes.

use super::*;

/// Property-key spelling retained while call sites migrate to the sanctioned
/// string-payload API. See [`crate::string::OwnedStringBytes`] for the GC
/// safety contract shared by all string consumers.
pub(crate) type HeapKeyBytes = crate::string::OwnedStringBytes;

/// Hidden own-field name under which a `class X extends Request/Response`
/// instance stashes the id of its underlying native Web-Fetch handle. Written
/// by the `js_request_subclass_init` / `js_response_subclass_init` super-init
/// shims (global_this.rs); read here (property forward), in
/// `native_call_method.rs` (body-method forward), and in `instanceof.rs`
/// (`x instanceof Request/Response`). A Request/Response is a registry-backed
/// native handle, not a heap object whose methods live on the JS prototype
/// chain, so a subclass instance can only reach those members via the handle.
pub(crate) const FETCH_SUBCLASS_HANDLE_FIELD: &[u8] = b"__perry_fetch_handle__";

/// Has any fetch-subclass instance EVER stashed a native handle in this
/// process? `fetch_subclass_handle_id` costs a key-string alloc + a full
/// property read per call; the `in`-operator fast path (#6748) gates on this
/// flag so the overwhelmingly-common no-fetch-subclass program never pays it.
pub(crate) static FETCH_SUBCLASS_EVER: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// If `obj` (a raw heap object address) is a `class X extends Request/Response`
/// instance, return the id of its underlying native fetch handle. Returns
/// `None` for any non-object / non-subclass receiver, so callers can fall
/// through to their normal dispatch unchanged.
pub(crate) unsafe fn fetch_subclass_handle_id(obj: usize) -> Option<i64> {
    // #7795: nothing has ever stashed a fetch handle, so this probe cannot
    // return `Some` — answer from the monotone flag instead of interning a key
    // string and running a full recursive `js_object_get_field_by_name`. This
    // sits on the ORDINARY-OBJECT PROPERTY-MISS path, which every `await` of a
    // plain object reaches through the spec thenable check (`Get(v, "then")`),
    // so an un-gated probe is a per-miss allocation in programs that never
    // subclass `Request`/`Response`. `FETCH_SUBCLASS_EVER` already existed for
    // the `in`-operator fast path (#6748) and is set at the single stash site
    // (`attach_fetch_handle_to_this`); this just consults it here too.
    if !FETCH_SUBCLASS_EVER.load(std::sync::atomic::Ordering::Relaxed) {
        return None;
    }
    // #7526: classify by BAND, not by magnitude. The old floor
    // (`GC_HEADER_SIZE + 0x1000`) plus `is_valid_obj_ptr` is a magnitude test
    // only — `is_valid_obj_ptr`'s `HEAP_MIN` is 0x1000 — so every Web Fetch
    // handle id sailed through it: they live in `[0x40000, 0xE0000)`, well
    // above the floor and well below `HANDLE_BAND_MAX` (0x100000). `new
    // Response(...)` yields id 0x40000 exactly, and `r instanceof Request`
    // reaches here, so the very first fetch handle a program allocates
    // dereferenced 0x3fff8 and took a SIGSEGV. `is_plausible_heap_addr` is the
    // canonical pairing (`is_above_handle_band && is_valid_obj_ptr`) and
    // rejects every band without touching memory.
    if !crate::value::addr_class::is_plausible_heap_addr(obj) {
        return None;
    }
    let Some(gc_header) = crate::value::addr_class::try_read_gc_header(obj) else {
        return None;
    };
    if gc_header.obj_type != crate::gc::GC_TYPE_OBJECT {
        return None;
    }
    // #7498: the key allocation below can trigger a copying minor, which moves
    // `obj` and rewrites only the slots it can see — a bare `usize` is not one.
    // This frame is on the `[...obj.arr]` prototype-walk stack that
    // `PERRY_GC_PROTECT_FROMSPACE=1` faults in. Root the receiver first, then
    // read its post-collection address for the field read.
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_h = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(obj as i64));
    let key = crate::string::js_string_from_bytes(
        FETCH_SUBCLASS_HANDLE_FIELD.as_ptr(),
        FETCH_SUBCLASS_HANDLE_FIELD.len() as u32,
    );
    let obj = crate::value::js_nanbox_get_pointer(obj_h.get_nanbox_f64()) as *const ObjectHeader;
    let v = js_object_get_field_by_name(obj, key);
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

/// Normalize a Fetch `Request`/`Response` value for a direct stdlib FFI call.
///
/// Bare Fetch values are already registry handles and pass through unchanged.
/// A userland subclass such as Next.js's `NextResponse` is a GC object whose
/// native handle lives in [`FETCH_SUBCLASS_HANDLE_FIELD`]; typed lowering used
/// to pass that object address to `js_fetch_response_*`, where it could never
/// resolve in the stdlib registry.  Recover and re-box the backing handle so
/// typed and dynamic property access share the same record.
#[no_mangle]
pub extern "C" fn js_fetch_unwrap_handle(value: f64) -> f64 {
    let js_value = crate::value::JSValue::from_bits(value.to_bits());
    if !js_value.is_pointer() {
        return value;
    }
    let raw = crate::value::js_nanbox_get_pointer(value) as usize;
    match unsafe { fetch_subclass_handle_id(raw) } {
        Some(id) => crate::value::js_nanbox_pointer(id),
        None => value,
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

/// Has any `class X extends Temporal.<Type>` instance EVER stashed a cell in
/// this process? The sibling of [`FETCH_SUBCLASS_EVER`], for the same reason:
/// `temporal_subclass_cell` costs a key-string alloc plus a full recursive
/// property read per call, and it is consulted on the ordinary-object
/// property-MISS path. Set at the single stash site
/// (`attach_temporal_cell_to_this`, `global_this/fetch_globals.rs`), which is
/// the only writer of `TEMPORAL_SUBCLASS_CELL_FIELD`. (#7795)
#[cfg(feature = "temporal")]
pub(crate) static TEMPORAL_SUBCLASS_EVER: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// If `obj` (a raw heap object address) is a `class X extends Temporal.<Type>`
/// instance, return the NaN-boxed value of its stashed Temporal cell. Returns
/// `None` for any non-object / non-subclass receiver (so callers fall through
/// to their normal dispatch unchanged) or if the stashed value is somehow no
/// longer a live Temporal cell.
#[cfg(feature = "temporal")]
pub(crate) unsafe fn temporal_subclass_cell(obj: usize) -> Option<f64> {
    // #7795: see `fetch_subclass_handle_id`. No Temporal subclass instance has
    // ever stashed a cell, so this probe cannot return `Some`.
    if !TEMPORAL_SUBCLASS_EVER.load(std::sync::atomic::Ordering::Relaxed) {
        return None;
    }
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
    // #7498: same shape as `fetch_subclass_handle_id` above — `obj` must not
    // ride the key allocation as a bare `usize`.
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_h = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(obj as i64));
    let key = crate::string::js_string_from_bytes(
        TEMPORAL_SUBCLASS_CELL_FIELD.as_ptr(),
        TEMPORAL_SUBCLASS_CELL_FIELD.len() as u32,
    );
    let obj = crate::value::js_nanbox_get_pointer(obj_h.get_nanbox_f64()) as *const ObjectHeader;
    let v = js_object_get_field_by_name(obj, key);
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
pub(crate) use accessors::scan_accessor_receiver_override_root_mut;
mod buffer_own_prop;
mod class_object_props;
mod crypto_key;
pub(crate) mod enumeration;
mod field_ops;
mod get_field_by_name;
#[cfg(test)]
mod get_field_by_name_probe_tests;
mod get_field_by_name_tail;
mod has_property;
mod ic_miss;
mod map_set_receiver;
mod probe_dispatch;

/// Size of the direct-mapped `(keys_ptr, key_hash, field_index)` inline
/// cache backing `js_object_get_field_by_name`'s slow tail.
pub(crate) const FIELD_CACHE_SIZE: usize = 1024;

/// #6759 Phase A: property-lookup inline caches, grouped as the
/// `field_lookup` field of [`crate::state::RuntimeState`]. Previously a
/// function-local `thread_local!` in `get_field_by_name_tail` plus a module
/// `thread_local!` in `has_property`; reach them via
/// `crate::state::state().field_lookup`.
pub(crate) struct FieldLookupCaches {
    /// Fixed-size direct-mapped cache (no allocation, no HashMap): each
    /// entry stores `(keys_ptr, key_hash, field_index)`. Copied-minor
    /// nursery reset can reuse a keys-array address, so cache hits still
    /// validate the key slot before returning a field.
    pub(crate) field_cache: std::cell::UnsafeCell<[(usize, u32, u32); FIELD_CACHE_SIZE]>,
}

impl FieldLookupCaches {
    pub(crate) fn new() -> Self {
        FieldLookupCaches {
            field_cache: std::cell::UnsafeCell::new([(0usize, 0u32, 0u32); FIELD_CACHE_SIZE]),
        }
    }
}

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
pub(crate) use class_object_props::class_object_prototype_value;
pub(crate) use crypto_key::{
    crypto_key_property_value, CLASS_ID_BOXED_BIGINT, CLASS_ID_BOXED_BOOLEAN,
    CLASS_ID_BOXED_NUMBER, CLASS_ID_BOXED_STRING, CLASS_ID_BOXED_SYMBOL,
};
pub(crate) use enumeration::{
    canonical_array_index, ecma_own_key_order, instance_private_key_hidden,
    is_internal_runtime_key, is_internal_runtime_key_bytes, keys_contain_array_index,
};
pub use enumeration::{
    js_for_in_keys_value, js_object_entries, js_object_entries_value, js_object_keys,
    js_object_keys_value, js_object_values, js_object_values_value,
};
pub use field_ops::{
    js_object_free, js_object_get_class_id, js_object_get_field_f64, js_object_set_field,
    js_object_set_field_by_index, js_object_set_field_f64, js_object_set_keys, js_object_to_value,
    js_value_to_object,
};
pub use get_field_by_name::js_object_get_field_by_name;
pub(crate) use get_field_by_name_tail::get_field_by_name_object_tail;
pub(super) use has_property::native_module_own_field_by_key;
pub(crate) use has_property::{
    closure_dynamic_prop_by_key, reified_function_method_name, wide_key_index_lookup,
    wide_key_index_note_hit, WIDE_KEY_INDEX_MIN_KEYS,
};
pub use has_property::{js_in_operator, js_object_has_property};
#[cfg(test)]
pub(crate) use ic_miss::primitive_proto_method_name_static;
pub(crate) use ic_miss::{
    bind_primitive_proto_method_static, is_array_method_value_name, private_evaluation_brand_value,
    private_lexical_brand_pop, private_lexical_brand_push, private_lexical_brand_stack_restore,
    private_lexical_brand_stack_savepoint, private_member_access_hints_restore,
    private_member_access_hints_savepoint, private_member_call_by_name, private_member_get_by_name,
    private_member_set_by_name, scan_private_lexical_brand_roots_mut, set_method_value_name,
    stamp_private_evaluation_brand, take_private_method_call_hint, take_private_method_owner_hint,
    timer_handle_method_name_static,
};
pub use ic_miss::{
    js_class_field_add, js_object_get_field_by_name_f64, js_object_get_field_by_property_id_f64,
    js_object_get_field_ic_miss, js_object_set_field_by_property_id, js_private_brand_add,
    js_private_brand_check, js_private_field_add, js_private_guard, PicCache, PIC_CACHE_WORDS,
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
                let mut cache = [0i64; crate::object::PIC_CACHE_WORDS];

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

    /// #7526: a Web Fetch handle id is NOT a heap address, and the subclass
    /// probe must reject it by BAND before dereferencing `addr - 8`.
    ///
    /// `new Response(...)` yields handle id `FETCH_HANDLE_BAND_START` (0x40000)
    /// exactly, so `r instanceof Request` — which reaches
    /// `fetch_subclass_handle_id` — took a SIGSEGV on the first fetch handle a
    /// program allocated. The old guard was a magnitude floor plus
    /// `is_valid_obj_ptr`, whose own `HEAP_MIN` is 0x1000; every band sits
    /// above that and below `HANDLE_BAND_MAX`, so none of them were excluded.
    ///
    /// This walks the band boundaries rather than one value, so a future band
    /// added to `addr_class` without a matching guard here fails the test.
    #[test]
    fn fetch_subclass_probe_rejects_every_handle_band_without_dereferencing() {
        use crate::value::addr_class::*;
        let probes = [
            (COMMON_HANDLE_BAND_END, "fetch band start (the #7526 crash)"),
            (FETCH_HANDLE_BAND_START, "fetch band start"),
            (FETCH_HANDLE_BAND_START + 1, "inside the fetch band"),
            (FETCH_HANDLE_BAND_END - 1, "fetch band end"),
            (PROXY_ID_BAND_START, "proxy id band"),
            (HANDLE_BAND_MAX - 1, "last handle-band address"),
            (1, "smallest handle"),
        ];
        for (addr, what) in probes {
            assert!(
                !is_plausible_heap_addr(addr),
                "{what} ({addr:#x}) must not be classified as a heap address"
            );
            // The probe must return None WITHOUT faulting; reaching the
            // assertion at all is half the test.
            let got = unsafe { fetch_subclass_handle_id(addr) };
            assert!(
                got.is_none(),
                "{what} ({addr:#x}) must not probe as a subclass handle"
            );
        }
    }
}
