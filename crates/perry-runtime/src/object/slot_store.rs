//! The two runtime funnels that store a JS value into an object's inline
//! slot (split out of `object/mod.rs`, which is at the file-size cap).
//! Every one runs the field-representation store check
//! (`field_rep_store::checked_slot_bits`) before the value reaches the slot.
use super::ObjectHeader;

#[inline]
pub(crate) unsafe fn store_object_field_slot(
    obj: *mut ObjectHeader,
    field_index: usize,
    value_bits: u64,
) {
    note_prototype_field_store(obj, field_index);
    let value_bits = super::field_rep_store::checked_slot_bits(obj, field_index, value_bits);
    let fields_ptr = (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut u64;
    let slot = fields_ptr.add(field_index);
    crate::gc::runtime_store_object_jsvalue_slot(
        obj as usize,
        slot as usize,
        field_index,
        value_bits,
    );
}

/// Newborn JSON materializer store with the same string alias and write
/// barrier work as ordinary object stores. The shape owns its layout.
#[inline]
pub(crate) unsafe fn store_object_field_slot_layout_deferred(
    obj: *mut ObjectHeader,
    field_index: usize,
    value_bits: u64,
) -> bool {
    let value_bits = super::field_rep_store::checked_slot_bits(obj, field_index, value_bits);
    let fields_ptr = (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut u64;
    let slot = fields_ptr.add(field_index);
    crate::gc::runtime_store_jsvalue_slot_layout_deferred(
        obj as usize,
        slot as usize,
        field_index,
        value_bits,
    )
}

/// Ordinary prototype writes retire current main's receiver-only direct-call
/// guards (S5), including the checked stores a packed site's first miss uses.
/// The prototype's private shape lineage ensures a generated inline hit was
/// first learned on this holder, after that miss retired the guard.
/// Nothing here allocates on the managed heap or runs JavaScript.
#[inline]
pub(crate) unsafe fn note_prototype_field_store(obj: *mut ObjectHeader, field_index: usize) {
    let meta = (*obj).meta;
    if meta.is_null() || (*meta).flags & super::OBJECT_META_FLAG_IS_PROTOTYPE == 0 {
        return;
    }
    let keys = super::object_keys(obj);
    if field_index as u32 >= keys.count() {
        return;
    }
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    if let Some(bytes) =
        crate::string::js_string_key_bytes(keys.get(field_index as u32), &mut scratch)
    {
        if let Ok(name) = std::str::from_utf8(bytes) {
            super::descriptor_state::invalidate_prototype_descriptor_guards(obj as usize, name);
        }
    }
}
