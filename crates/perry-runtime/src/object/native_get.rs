//! Callback-free positive Get for ordinary data properties.
//!
//! Every pointer and key borrow below is confined to a lookup that cannot
//! allocate in the JS heap, collect, or invoke user code. A miss is NOT an
//! absent-property verdict: the caller must run the existing generic Get.

use super::{shapes, ObjectHeader};
use crate::codegen_abi::LayoutWord;
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
/// The named reads settle through [`try_settled_get_by_name`]; this positive-only
/// form is kept for the lookup tests.
#[cfg(test)]
#[inline]
pub(crate) unsafe fn try_data_get_by_name(
    object: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<JSValue> {
    try_data_lookup_by_name(object, key).flatten()
}

/// The named-field ABI's receiver, key text and key bits, or `None` for an
/// encoding or key header this lane does not read.
#[inline]
unsafe fn named_lookup_inputs<'a>(
    object: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<(JSValue, &'a [u8], u64)> {
    if key.is_null() || !crate::value::addr_class::is_plausible_heap_addr(key as usize) {
        return None;
    }
    if (*key).byte_len > (*key).capacity || (*key).byte_len >= 1 << 28 {
        return None;
    }
    let bits = object as u64;
    let receiver = match bits >> 48 {
        // A raw receiver in the id band is a native registry id (a `Headers`
        // handle): the unknown-word classification makes it a `HANDLE_TAG`
        // value, which has no shape, so this lane declines it before any
        // boxing. Every other raw receiver is a heap address.
        0 if crate::value::addr_class::is_handle_band(bits as usize) => return None,
        0 => JSValue::pointer(object as *mut u8),
        0x7FFD => JSValue::from_bits(bits),
        _ => return None,
    };
    let bytes =
        std::slice::from_raw_parts(crate::string::string_data(key), (*key).byte_len as usize);
    Some((
        receiver,
        bytes,
        crate::value::nanbox_string_key(key).to_bits(),
    ))
}

/// [`try_data_get_by_name`] that also answers a definite miss, with the
/// contract of [`try_data_lookup_bytes`].
#[inline]
pub(crate) unsafe fn try_data_lookup_by_name(
    object: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<Option<JSValue>> {
    let (receiver, bytes, key_bits) = named_lookup_inputs(object, key)?;
    try_data_lookup_key(receiver, key_bits, bytes)
}

/// The settled `[[Get]]` of a named read on an ordinary receiver: its data
/// value, or `undefined` once the shapes prove the key is on no link of the
/// chain AND the receiver is one whose reads the shapes alone decide (see
/// [`shapes::plain_receiver_miss_is_final`]), or, for an instance of a declared
/// class, what its class chain's shapes answer
/// ([`super::native_call_method::class_holder::class_instance_inherited_read`]).
/// `None` leaves the read to the
/// generic path, which owns every other receiver and every accessor.
#[inline]
pub(crate) unsafe fn try_settled_get_by_name(
    object: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<JSValue> {
    let (receiver, bytes, key_bits) = named_lookup_inputs(object, key)?;
    match try_data_lookup_key(receiver, key_bits, bytes) {
        Some(Some(value)) => Some(value),
        Some(None) => {
            shapes::plain_receiver_miss_is_final(receiver.as_pointer::<ObjectHeader>(), bytes)
                .then(JSValue::undefined)
        }
        // The data walk declines at a declared class's prototype hop: an
        // instance of one is settled by its class chain's shapes instead.
        None => {
            #[cfg(test)]
            if FORCE_SLOW.with(|value| value.get()) {
                return None;
            }
            super::native_call_method::class_holder::class_instance_inherited_read(receiver, bytes)
        }
    }
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
    let mut own_absent = false;
    // A POINTER_TAG receiver names a GC-headed object (honest pointers).
    if crate::value::addr_class::is_above_handle_band(addr)
        && addr.is_multiple_of(std::mem::align_of::<ObjectHeader>())
    {
        // Bit 30 (`layout_desc::LAYOUT_SHADOW_BIT`) clear proves there is no
        // overflow. With an overflow installed, it is the order authority and
        // answers the own lookup; a bit set by a dynamic write or relink alone
        // (object model step 3) leaves the layout complete for own keys.
        let shadowed = !super::overflow::table(object).is_null();
        let shape_id = super::layout_desc::layout_id_of_word((*object).parent_class_id);
        // Non-object cells, unstamped receivers and dictionary/exotic shapes
        // cannot supply this proof. Decline from the owner word before paying
        // for the agent directory; no registry determines this classification.
        let ordinary_id = shape_id.wrapping_sub(shapes::SHAPE_ID_BASE)
            < crate::codegen_abi::LAYOUT_ORDINARY_END - crate::codegen_abi::LAYOUT_TAG;
        if !ordinary_id {
            // The data walk needs an ordinary-band descriptor. Unstamped
            // cells have none; dictionary and function namespaces never
            // describe this layout. Their generic callers still own Get.
            return None;
        }
        let own_shape = shapes::own_data_shape(shapes::ordinary_dir_addr(), LayoutWord(shape_id));
        if let Some(own_shape) = own_shape {
            // A live nonordinary shape cannot use this data lane. Its caller
            // still handles class getters and other special receiver reads.
            let shape = own_shape?;
            first_shape = Some(shape);
            if shadowed {
                let table = super::overflow::table(object);
                if table.is_null() {
                    return None;
                }
                // Every own key is an entry of the table, so a miss proves the
                // key is not own: the prototype walk below continues from the
                // receiver's link (object model §3.7 case 3).
                if let Some(entry) = super::overflow::find_key(table, key_bits, key) {
                    let attrs = *super::overflow::attrs_at(table, entry) as u8;
                    let header = &*crate::gc::header_from_trusted_user_ptr(object.cast());
                    // A1_UNPROVEN: record-first: class not yet known (kind-A/B question for S0f).
                    let meta =
                        crate::object::header::MetaObj::runtime_born(object as *mut _).meta();
                    if attrs & super::key_attrs::ENTRY_ACCESSOR != 0
                        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
                        || header._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0
                        || (*meta).elements != 0
                    {
                        return None;
                    }
                    let value =
                        JSValue::from_bits(super::overflow::get_from(table, object, entry)?);
                    if value.bits() == crate::value::TAG_HOLE
                        || (value.is_undefined()
                            && super::native_this_alias::object_alias(object).is_some())
                    {
                        return None;
                    }
                    return Some(Some(value));
                }
                own_absent = true;
            }
            let lookup = if shadowed {
                None
            } else {
                shape.plain_lookup(key_bits, key)
            };
            own_absent |= lookup == Some(None);
            if let Some(Some((slot, live))) = lookup {
                // The live shape proves a shaped ObjectHeader. Owner-local state
                // still controls forwarding and indexed/exotic receiver layouts.
                let header = &*crate::gc::header_from_trusted_user_ptr(object.cast());
                // A1_UNPROVEN: record-first: class not yet known (kind-A/B question for S0f).
                let meta = crate::object::header::MetaObj::runtime_born(object as *mut _).meta();
                if header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
                    && header._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO == 0
                    && (meta.is_null() || (*meta).elements == 0)
                {
                    let value = super::field_get_set::object_field_at_with_live(object, slot, live);
                    if value.bits() != crate::value::TAG_HOLE
                        && !(value.is_undefined()
                            && super::native_this_alias::object_alias(object).is_some())
                    {
                        return Some(Some(value));
                    }
                }
            }
        }
    }
    // Descriptor summaries use the same byte hash. Invalid UTF-8 stays on
    // the WTF-8-aware slow path; no String is allocated for ordinary keys.
    let key_value = JSValue::from_bits(key_bits);
    if key_value.is_string() {
        crate::string::header_str_checked(key_value.as_string_ptr())?;
    } else {
        // Inline keys and the borrowed-byte ABI have no header proof.
        std::str::from_utf8(key).ok()?;
    }
    let key_hash = super::key_bytes_hash(key.as_ptr(), key.len());
    let mut object = receiver.as_pointer::<ObjectHeader>();
    let mut inherited = false;
    for _ in 0..32 {
        let addr = object as usize;
        let (header, keys, key_count, live, summary, dictionary, proto_id) =
            if let Some(shape) = first_shape.take() {
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
                    shape.proto_id,
                )
            } else {
                // The receiver and every hop are POINTER_TAG words or the realm's
                // resolved prototype: honest pointers, so the header needs no
                // admission. The arena flag keeps the malloc-backed exotics out.
                if !crate::value::addr_class::is_above_handle_band(addr) {
                    return None;
                }
                let header = &*crate::gc::header_from_trusted_user_ptr(addr as *const u8);
                if header.gc_flags & crate::gc::GC_FLAG_ARENA == 0
                    || !crate::gc::gc_type_is_object(header.obj_type)
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
                    super::overflow::has_overflow(object),
                    descriptor.proto_id,
                )
            };
        if header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
            || header._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0
        {
            return None;
        }
        // The live shape is the read authority. A declared class or exotic
        // receiver still needs its generic semantics; ordinary serial links
        // (including ES5 constructors) need no class-id classification.
        if (shapes::PROTO_ID_CLASS..shapes::PROTO_ID_UNIQUE).contains(&proto_id)
            || proto_id == shapes::PROTO_ID_PER_OBJECT
        {
            return None;
        }
        if key == b"toJSON" && crate::perf_hook_slots::is_perf_entry_object(object) {
            return None;
        }
        // A1_UNPROVEN: record-first: class not yet known (kind-A/B question for S0f).
        let meta = crate::object::header::MetaObj::runtime_born(object as *mut _).meta();
        if !meta.is_null() && (*meta).elements != 0 {
            return None;
        }
        // The shape entry decides whether this own slot contains data.
        if own_absent {
            // The positional shape lookup already proved absence. Do not
            // search the same key list again before following its link.
            own_absent = false;
        } else if dictionary {
            // A hop with an overflow: its table holds every own key (a fixed
            // key as a slot reference), so one atom-keyed probe answers it.
            let table = super::overflow::table(object);
            if let Some(entry) = super::overflow::find_key(table, key_bits, key) {
                let attrs = *super::overflow::attrs_at(table, entry) as u8;
                if attrs & super::key_attrs::ENTRY_ACCESSOR != 0 {
                    return None;
                }
                let value = JSValue::from_bits(super::overflow::get_from(table, object, entry)?);
                if value.bits() == crate::value::TAG_HOLE
                    || (inherited && (value.is_undefined() || value.is_null()))
                    || (value.is_undefined()
                        && super::native_this_alias::object_alias(object).is_some())
                {
                    return None;
                }
                return Some(Some(value));
            }
        } else {
            let keys = keys as usize as *const crate::array::ArrayHeader;
            if !keys.is_null() {
                // Public reads skip intrinsic and class-private entries.
                // These facts come from the live owner proof without another
                // shape lookup or allocation.
                let slot = if summary & super::key_attrs::SUMMARY_PRIVATE == 0 {
                    super::keys_lookup::keys_find_slot_by_bytes_resolved_hashed(
                        crate::object::ObjectKeys::new(keys as *mut _, key_count),
                        key_count,
                        key,
                        key_hash,
                    )
                } else {
                    super::keys_lookup::keys_find_property_slot_by_bytes_resolved_hashed(
                        crate::object::ObjectKeys::new(keys as *mut _, key_count),
                        key_count,
                        key,
                        key_hash,
                    )
                };
                if let Some(position) = slot {
                    // The key's attribute word answers the accessor question
                    // and holds its storage slot: one lookup, one load.
                    let word = super::key_attrs::key_word(keys, position);
                    if word as u8 & super::key_attrs::ENTRY_ACCESSOR != 0 {
                        return None;
                    }
                    let slot = super::key_attrs::slot_of_word(word, position);
                    let value = super::field_get_set::object_field_at_with_live(object, slot, live);
                    // Legacy inherited resolution treats undefined/null as
                    // misses at some class edges. Preserve that fallback, and
                    // the f64 entry's native-handle alias on undefined reads.
                    // A slot holding the hole is an absent optional key
                    // (`key_view`): the generic read continues the chain.
                    if value.bits() == crate::value::TAG_HOLE
                        || (inherited && (value.is_undefined() || value.is_null()))
                        || (value.is_undefined()
                            && super::native_this_alias::object_alias(object).is_some())
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
        }
        if proto_id == shapes::PROTO_ID_NULL {
            // The chain ends here, and no hop listed the key.
            return Some(None);
        }
        let recorded = crate::object::shapes::object_prototype_word(object);
        if recorded != 0 {
            let prototype = JSValue::from_bits(recorded);
            if !prototype.is_pointer() {
                return None;
            }
            object = prototype.as_pointer();
        } else if proto_id == shapes::PROTO_ID_DEFAULT {
            // A resolved realm prototype is an existing shape-held link's
            // default endpoint. The borrowed walk never initializes it.
            let prototype = crate::array::object_prototype_addr_if_resolved();
            if prototype == 0 {
                return None;
            }
            if prototype == object as usize {
                return Some(None);
            }
            object = prototype as *const ObjectHeader;
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
