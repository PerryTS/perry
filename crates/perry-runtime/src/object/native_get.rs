//! Callback-free positive Get for ordinary data properties.
//!
//! Every pointer and key borrow below is confined to a lookup that cannot
//! allocate in the JS heap, collect, or invoke user code. A miss is NOT an
//! absent-property verdict: the caller must run the existing generic Get.

use super::{shapes, ObjectHeader};
use crate::value::JSValue;

/// Read using an already materialized property key. String keys, including
/// inline strings, require neither ToPropertyKey nor another key allocation.
#[inline]
pub(crate) unsafe fn try_data_get(receiver: JSValue, key: JSValue) -> Option<JSValue> {
    if !receiver.is_pointer() || !key.is_any_string() {
        return None;
    }
    let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
    let bytes = crate::string::js_string_key_bytes(key, &mut scratch)?;
    try_data_get_bytes(receiver, bytes)
}

/// Raw-pointer sibling for the named-field ABI, which accepts both raw and
/// NaN-boxed object pointers. Reject other encodings before touching memory.
#[inline]
pub(crate) unsafe fn try_data_get_by_name(
    object: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<JSValue> {
    if key.is_null() || !crate::value::addr_class::is_plausible_heap_addr(key as usize) {
        return None;
    }
    if (*key).byte_len > (*key).capacity || (*key).byte_len >= 1 << 28 {
        return None;
    }
    let bits = object as u64;
    let receiver = match bits >> 48 {
        0 => JSValue::pointer(object as *mut u8),
        0x7FFD => JSValue::from_bits(bits),
        _ => return None,
    };
    let bytes =
        std::slice::from_raw_parts(crate::string::string_data(key), (*key).byte_len as usize);
    try_data_get_bytes(receiver, bytes)
}

/// Borrowed-name API for native callers. The keys lookup is consult-only:
/// it never builds a shape index, materializes a key, or resolves a builtin.
#[inline]
pub(crate) unsafe fn try_data_get_bytes(receiver: JSValue, key: &[u8]) -> Option<JSValue> {
    #[cfg(test)]
    if FORCE_SLOW.with(|value| value.get()) {
        return None;
    }
    if !receiver.is_pointer() || key.first() == Some(&b'#') || key == b"constructor" {
        return None;
    }
    // Descriptor summaries use the same byte hash. Invalid UTF-8 stays on
    // the WTF-8-aware slow path; no String is allocated for ordinary keys.
    std::str::from_utf8(key).ok()?;
    let accessor_bit = 1u64 << (super::key_bytes_hash(key.as_ptr(), key.len()) & 63);
    let mut object = receiver.as_pointer::<ObjectHeader>();
    let mut inherited = false;
    for _ in 0..32 {
        let addr = object as usize;
        // Headerless Buffer/TypedArray/foreign handles must never be read as
        // GcHeader-bearing objects. As in the existing own-field fast lane,
        // require positive arena membership before inspecting the header.
        if !crate::value::addr_class::is_plausible_heap_addr(addr)
            || crate::arena::classify_heap_generation(addr) == crate::arena::HeapGeneration::Unknown
        {
            return None;
        }
        let header = crate::value::addr_class::try_read_gc_header(addr)?;
        if header.obj_type != crate::gc::GC_TYPE_OBJECT
            || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
            || header._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0
        {
            return None;
        }
        let descriptor = shapes::object_shape_descriptor(object)?;
        if descriptor.object_kind != shapes::ShapeObjectKind::Ordinary {
            return None;
        }
        let class_id = (*object).class_id;
        // Positive membership, rather than a blacklist of native class ids.
        // Synthetic function/Object.create ids occupy the allocated prefix
        // of this counter's range; reserved native ids are outside it.
        let synthetic = class_id >= 0x8000_0000
            && class_id < super::NEXT_SYNTHETIC_CLASS_ID.load(std::sync::atomic::Ordering::Relaxed);
        if class_id != 0 && !synthetic && !super::is_anon_shape_class_id(class_id) {
            return None;
        }
        if crate::process::is_process_env_ptr(addr)
            || super::is_arguments_object(object)
            || (key == b"toJSON" && crate::perf_hooks::is_perf_entry_object(object))
        {
            return None;
        }
        let meta = (*object).meta;
        if !meta.is_null() && (*meta).elements != 0 {
            return None;
        }
        // A data slot can remain underneath an accessor. A clear Bloom bit
        // proves no accessor for THIS key; collisions conservatively decline.
        // Same summary as descriptor_state::may_have_descriptor_entry; the
        // receiver has already been classified, so do not classify it again.
        if meta.is_null() || (*meta).accessor_key_bits & accessor_bit == 0 {
            let keys = descriptor.keys as usize as *const crate::array::ArrayHeader;
            if !keys.is_null() {
                if let Some(slot) =
                    // `keys` came straight out of `descriptor` above with no
                    // allocation in between, and the collector maintains that
                    // field — so the resolved entry skips a `clean_arr_ptr`
                    // that re-derives it.
                    super::keys_find_slot_by_bytes_resolved(
                        keys,
                        descriptor.logical_key_count,
                        key,
                    )
                {
                    let value = super::field_get_set::object_field_at_with_live(
                        object,
                        slot,
                        descriptor.live_inline_slot_count,
                    );
                    // Legacy inherited resolution treats undefined/null as
                    // misses at some class edges. Preserve that fallback, and
                    // the f64 entry's native-handle alias on undefined reads.
                    if (inherited && (value.is_undefined() || value.is_null()))
                        || (value.is_undefined() && super::native_this_alias::alias_active())
                    {
                        return None;
                    }
                    return Some(value);
                }
            }
        } else {
            return None;
        }
        if !meta.is_null() && (*meta).prototype != 0 {
            let prototype = JSValue::from_bits((*meta).prototype);
            if !prototype.is_pointer() {
                return None;
            }
            object = prototype.as_pointer();
        } else if synthetic {
            // These are already-rooted per-class pointers, with no builtin
            // name lookup or lazy construction. Declared prototype metadata
            // has separate precedence and stays on the generic path.
            if !super::class_decl_prototype_object(class_id).is_null() {
                return None;
            }
            object = super::class_prototype_object(class_id);
        } else {
            // Default builtin prototypes may need initialization and can be
            // replaced through globalThis. Do not resolve them under a borrow.
            return None;
        }
        inherited = true;
    }
    None
}

/// Spec Get for a canonical (pre-interned) string key and the same receiver.
/// The caller must keep both inputs live until entry. A fast hit is a leaf;
/// the existing Reflect implementation owns all handles on a slow read.
#[inline]
pub(crate) unsafe fn get_by_canonical_key(receiver: f64, key: *const crate::StringHeader) -> f64 {
    let property_key = JSValue::from_bits(crate::value::nanbox_string_key(key).to_bits());
    crate::proxy::js_reflect_get(receiver, f64::from_bits(property_key.bits()), receiver)
}

#[cfg(test)]
thread_local! {
    static FORCE_SLOW: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
mod tests;
