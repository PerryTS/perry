//! The two runtime funnels that store a JS value into an object's inline
//! slot (split out of `object/mod.rs`, which is at the file-size cap).
//! Every one also tells `proto_validity` about writes to prototype-marked
//! objects (owner decision D3(b)), and runs the field-representation store
//! check (charter step 5, `field_rep_store::checked_slot_bits`) before the
//! value reaches the slot.
use super::ObjectHeader;

#[inline]
pub(crate) unsafe fn store_object_field_slot(
    obj: *mut ObjectHeader,
    field_index: usize,
    value_bits: u64,
) {
    super::proto_validity::note_marked_value_write(obj);
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

/// #7630: `store_object_field_slot` without the per-slot layout note, for the
/// JSON materialiser's construction loops. Returns whether the value carries a
/// heap pointer; the caller accumulates that and settles the object's layout
/// state once via `layout_finish_deferred_boxed_object`.
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
