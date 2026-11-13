//! `delete o[k]` for a computed string key on an ordinary bag that owns an
//! overflow table: the key is resolved as the keyed read resolves it
//! ([`computed_key`]) and the entry is deleted from the table, without the
//! by-name ladder of `js_object_delete_field`.
//!
//! Every by-address case of that ladder is ruled out by a fact of the
//! receiver read here (header type and flags, layout word, record, meta):
//!
//! * handle band, proxy cell: the address is above the handle floor and its
//!   header is a live object header (the caller answers a proxy first);
//! * typed array, buffer, array, lazy array, closure, Date/RegExp/Error:
//!   the header type is the object type and the record kind is `Ordinary`;
//! * arguments object: `OBJ_FLAG_ARGUMENTS` clear;
//! * typed-array prototype: `OBJ_FLAG_TYPED_ARRAY_PROTO` clear (the named
//!   note itself is still made, it keys on the name first);
//! * accessor or attribute descriptors, frozen, sealed:
//!   `OBJ_FLAG_HAS_DESCRIPTORS | FROZEN | SEALED` clear;
//! * stable-tombstone receivers: flag clear (they have their own lane);
//! * deferred namespace / native module: class id; native alias and exotic
//!   read receivers: meta flags;
//! * string wrapper index keys, array-index keys: a digit-led key declines;
//! * class-declaration prototypes and class-family instances: an anonymous
//!   layout band, not a family object, realm-default or null prototype link;
//! * `process.env`: its address latch.
//!
//! These are the facts the keyed store asks before an append
//! (`computed::append_admitted`) minus extensibility, which a delete does
//! not need. `None` decides nothing: the complete delete runs.
use super::*;

/// `Some(deleted)` when the receiver and key are answered here.
///
/// # Safety
/// `obj` is the payload of a POINTER-tagged receiver that is not a proxy;
/// `key_bits` any string value.
#[inline]
pub(crate) unsafe fn probe_delete_keyed(obj: *mut ObjectHeader, key_bits: u64) -> Option<bool> {
    let addr = obj as usize;
    if addr < perry_abi::RECEIVER_HANDLE_FLOOR {
        return None;
    }
    let gc = crate::value::addr_class::try_read_gc_header(addr)?;
    const DECLINE: u16 = crate::gc::OBJ_FLAG_FROZEN
        | crate::gc::OBJ_FLAG_SEALED
        | crate::gc::OBJ_FLAG_HAS_DESCRIPTORS
        | crate::gc::OBJ_FLAG_STABLE_TOMBSTONES
        | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO
        | crate::gc::OBJ_FLAG_ARGUMENTS;
    if crate::gc_type_is_not_object!(gc.obj_type)
        || gc.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || gc._reserved & DECLINE != 0
    {
        return None;
    }
    let word = (*obj).parent_class_id;
    if word & crate::object::overflow::SHADOW_BIT == 0
        || (*obj).class_id == crate::object::NATIVE_MODULE_CLASS_ID
        || crate::object::layout_desc::is_family_object(obj)
    {
        return None;
    }
    let id = crate::object::layout_desc::layout_id_of_word(word);
    if id.wrapping_sub(super::super::SHAPE_ID_BASE) >= (2 << crate::codegen_abi::LAYOUT_BAND_SHIFT)
    {
        return None;
    }
    let dir = super::super::ordinary_dir_addr();
    let r = record_of(dir, LayoutWord(word));
    if r.is_null()
        || (*r).object_kind() != super::super::ShapeObjectKind::Ordinary
        || !matches!(
            (*r).proto_id,
            super::super::PROTO_ID_DEFAULT | super::super::PROTO_ID_NULL
        )
    {
        return None;
    }
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _));
    if meta.is_null()
        || (*meta).overflow == 0
        || (*meta).elements != 0
        || (*meta).flags
            & (crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER
                | crate::object::OBJECT_META_FLAG_NATIVE_ALIAS)
            != 0
        || crate::process::is_process_env_ptr(addr)
    {
        return None;
    }
    let (key, hash) = computed_key(key_bits)?;
    if key & TAG_MASK != STRING_TAG || hash == write::PROTO_KEY_HASH {
        return None;
    }
    let text = (key & POINTER_MASK) as *const crate::StringHeader;
    let bytes = std::slice::from_raw_parts(
        crate::string::string_data(text),
        (*text).byte_len as usize,
    );
    match bytes.first() {
        Some(first) if *first != b'#' && !first.is_ascii_digit() => {}
        _ => return None,
    }
    crate::typedarray_named::note_named_mutation(addr, bytes);
    crate::object::prop_plan::prop_plan_epoch_bump_for_owner(addr);
    let t = (*meta).overflow as usize as *mut crate::object::overflow::Table;
    // The overflow owns every current own key: a key it does not hold is
    // absent, and deleting an absent key succeeds.
    if crate::object::overflow::find_hashed(t, key, hash).is_none() {
        return Some(true);
    }
    Some(crate::object::overflow::delete_key_bits(obj, key))
}
