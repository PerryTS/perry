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
    try_data_lookup_key(receiver, key.bits(), bytes).flatten()
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
    let key_bits = crate::value::nanbox_string_key(key).to_bits();
    try_data_lookup_key(receiver, key_bits, bytes).flatten()
}

/// Borrowed-name API for native callers. The keys lookup is consult-only:
/// it never builds a shape index, materializes a key, or resolves a builtin.
#[inline]
pub(crate) unsafe fn try_data_get_bytes(receiver: JSValue, key: &[u8]) -> Option<JSValue> {
    try_data_lookup_bytes(receiver, key).flatten()
}

/// [`try_data_get_bytes`] that also answers a definite miss: `Some(None)`
/// when the chain's shapes end at a null `[[Prototype]]` without the key (a
/// `{ __proto__: null }` dictionary such as an emitter's `_events`). `None`
/// when the shapes cannot answer and the caller must take the full `[[Get]]`.
#[inline]
pub(crate) unsafe fn try_data_lookup_bytes(
    receiver: JSValue,
    key: &[u8],
) -> Option<Option<JSValue>> {
    try_data_lookup_key(receiver, 0, key)
}

#[inline]
unsafe fn try_data_lookup_key(
    receiver: JSValue,
    key_bits: u64,
    key: &[u8],
) -> Option<Option<JSValue>> {
    #[cfg(test)]
    if FORCE_SLOW.with(|value| value.get()) {
        return None;
    }
    if !receiver.is_pointer() || key.first() == Some(&b'#') || key == b"constructor" {
        return None;
    }
    let object = receiver.as_pointer::<ObjectHeader>();
    let addr = object as usize;
    let mut first_shape = None;
    if crate::value::addr_class::is_plausible_heap_addr(addr)
        && addr.is_multiple_of(std::mem::align_of::<ObjectHeader>())
    {
        let shape_id = (*object).parent_class_id;
        // Non-object cells, unstamped receivers and dictionary/exotic shapes
        // cannot supply this proof. Decline from the owner word before paying
        // for the agent directory; no registry determines this classification.
        let ordinary_id = shape_id.wrapping_sub(shapes::SHAPE_ID_BASE)
            < shapes::DICTIONARY_SHAPE_ID_BASE - shapes::SHAPE_ID_BASE;
        if !ordinary_id {
            // The data walk needs an ordinary-band descriptor. Unstamped
            // cells have none; dictionary and function namespaces never
            // describe this layout. Their generic callers still own Get.
            return None;
        }
        let own_shape = shapes::own_data_shape(shapes::ordinary_dir_addr(), shape_id);
        if let Some(own_shape) = own_shape {
            // A live nonordinary shape cannot use this data lane. Its caller
            // still handles class getters and other special receiver reads.
            let shape = own_shape?;
            first_shape = Some(shape);
            if let Some((slot, live)) = shape.plain_slot(key_bits, key) {
                // The live shape proves a shaped ObjectHeader. Owner-local state
                // still controls forwarding and indexed/exotic receiver layouts.
                let header = &*crate::gc::header_from_trusted_user_ptr(object.cast());
                let meta = (*object).meta;
                if header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
                    && header._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO == 0
                    && (meta.is_null() || (*meta).elements == 0)
                {
                    let value = super::field_get_set::object_field_at_with_live(object, slot, live);
                    if value.bits() != crate::value::TAG_HOLE
                        && !(value.is_undefined() && super::native_this_alias::alias_active())
                    {
                        return Some(Some(value));
                    }
                }
            }
        }
    }
    // Descriptor summaries use the same byte hash. Invalid UTF-8 stays on
    // the WTF-8-aware slow path; no String is allocated for ordinary keys.
    std::str::from_utf8(key).ok()?;
    let key_hash = super::key_bytes_hash(key.as_ptr(), key.len());
    let accessor_bit = 1u64 << (key_hash & 63);
    let mut object = receiver.as_pointer::<ObjectHeader>();
    let mut inherited = false;
    for _ in 0..32 {
        let addr = object as usize;
        let (header, keys, key_count, live, summary, dictionary) = if let Some(shape) =
            first_shape.take()
        {
            // The receiver's live ordinary shape already proved ObjectHeader
            // layout. Reuse it on wide, semantic and absent-key reads too;
            // these facts are consumed without allocation or user code.
            (
                &*crate::gc::header_from_trusted_user_ptr(object.cast()),
                shape.keys,
                shape.logical_key_count,
                shape.live_inline_slot_count,
                shape.summary,
                false, // The retained proof came from the ordinary ShapeId band.
            )
        } else {
            // An inherited hop or unproved cell still needs positive arena
            // membership before inspecting a possibly headerless handle.
            if !crate::value::addr_class::is_plausible_heap_addr(addr)
                || crate::arena::classify_heap_generation(addr)
                    == crate::arena::HeapGeneration::Unknown
            {
                return None;
            }
            let header = crate::value::addr_class::try_read_gc_header_known_plausible(addr)?;
            if header.obj_type != crate::gc::GC_TYPE_OBJECT
                || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
                || header._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0
            {
                return None;
            }
            let descriptor = shapes::object_shape_descriptor(object)?;
            if !descriptor.object_kind.is_ordinary_layout() {
                return None;
            }
            (
                header,
                descriptor.keys,
                descriptor.logical_key_count,
                descriptor.live_inline_slot_count,
                descriptor.summary,
                descriptor.semantic_generation & super::dictionary::DICTIONARY_GENERATION_TAG != 0,
            )
        };
        if header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
            || header._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0
        {
            return None;
        }
        let class_id = (*object).class_id;
        // Positive membership, rather than a blacklist of native class ids.
        // Synthetic function/Object.create ids occupy the allocated prefix
        // of this counter's range; reserved native ids are outside it.
        let synthetic = class_id >= super::class_registry::SYNTHETIC_CLASS_ID_BASE
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
            let keys = keys as usize as *const crate::array::ArrayHeader;
            if !keys.is_null() {
                // Public reads skip intrinsic and class-private entries.
                // These facts come from the live owner proof without another
                // shape lookup or allocation.
                let slot = if summary & super::key_attrs::SUMMARY_PRIVATE == 0 {
                    super::keys_lookup::keys_find_slot_by_bytes_resolved_hashed(
                        keys, key_count, key, key_hash,
                    )
                } else {
                    super::keys_lookup::keys_find_property_slot_by_bytes_resolved_hashed(
                        keys, key_count, key, key_hash,
                    )
                };
                if let Some(slot) = slot {
                    let value = super::field_get_set::object_field_at_with_live(object, slot, live);
                    // Legacy inherited resolution treats undefined/null as
                    // misses at some class edges. Preserve that fallback, and
                    // the f64 entry's native-handle alias on undefined reads.
                    if (inherited && (value.is_undefined() || value.is_null()))
                        || (value.is_undefined() && super::native_this_alias::alias_active())
                    {
                        return None;
                    }
                    return Some(Some(value));
                }
            } else if dictionary {
                // A keyless dictionary shape cannot prove an own-key miss.
                // Its mutable list is read by the ordinary generic lookup.
                return None;
            }
        } else {
            return None;
        }
        let recorded = crate::object::shapes::object_prototype_word(object);
        if recorded == crate::value::TAG_NULL
            || (recorded == 0 && header._reserved & crate::gc::OBJ_FLAG_NULL_PROTO != 0)
        {
            // The chain ends here, and no hop listed the key.
            return Some(None);
        }
        if recorded != 0 {
            let prototype = JSValue::from_bits(recorded);
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
