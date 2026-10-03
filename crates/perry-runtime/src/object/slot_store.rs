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
