use super::{shapes, ObjectHeader};
use crate::ArrayHeader;

/// The AUTHORITATIVE ordered-keys edge of a traced receiver (#8112).
///
/// This is the shape record's own `keys` word, not a copy of it: the record is
/// boxed (`object::shapes::ShapeRecordRef`), so its address is fixed for the
/// record's lifetime and the collector can mark through it and rewrite it in
/// place like any other child slot. The address is the record handle
/// `gc::layout::gc_child_slots` already resolved for this receiver, so the
/// edge costs no extra shape-table probe (#8122's one-probe rule) and needs no
/// post-visit write-back callback.
///
/// Siblings sharing a ShapeId hand the collector the SAME slot: one shape, one
/// edge. For a YOUNG carrier that is the whole liveness rule — the receiver is
/// traced, so the edge is emitted exactly while it lives. An OLD carrier needs
/// more, because a minor never enumerates it: the visitor also arms
/// `ShapeDescriptor::old_carrier`, and `shapes::scan_shape_table_rekey_mut`
/// roots the record for it. Emitting here regardless of generation is
/// deliberate — the alternative loses a shape whose only carrier is promoted
/// during the very drain that would have emitted it.
#[inline]
pub(crate) fn gc_shape_keys_edge_slot(record: Option<shapes::ShapeRecordRef>) -> Option<*mut u64> {
    #[cfg(test)]
    if shapes::test_keys_edge_suppressed() {
        // Sabotage arm: without this edge a keys array has no root and no
        // rewritable location at all. The fixtures use
        // it to prove their detector fires — a green protected run then means
        // the detector works, not that nothing was tried.
        return None;
    }
    let record = record?;
    if record.keys() == 0 {
        return None;
    }
    Some(record.keys_slot())
}

/// The object's inline field-slot range, given the receiver's shape record
/// resolved once by the collector.
pub(crate) unsafe fn gc_field_slot_range(
    obj: *mut ObjectHeader,
    record: Option<shapes::ShapeRecordRef>,
) -> Option<crate::gc::HeapSlotRange> {
    if obj.is_null() {
        return None;
    }
    // #8113: the descriptor is now the SOLE record of the live inline-slot
    // bound — there is no header word left to fall back to. An unstamped
    // receiver therefore traces zero payload slots, which is the fail-closed
    // answer for the only population that can be unstamped: synthetic/raw test
    // fixtures that bypass every runtime allocator, and which hold no heap
    // edges. Every runtime allocator publishes a descriptor before its header
    // escapes (`object/alloc.rs`), and every bound change is mint-then-stamp
    // (`shapes::publish_object_live_slot_count`), so a live object is never
    // observed here without one.
    let field_count = record
        .map(|record| record.live_inline_slot_count() as usize)
        .unwrap_or(0);
    if field_count > 1_000_000 {
        return None;
    }
    let fields = (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut u64;
    Some(crate::gc::HeapSlotRange::new(fields, field_count))
}

#[inline]
pub(crate) unsafe fn rebuild_object_field_layout(obj: *mut ObjectHeader, slot_count: usize) {
    let fields = (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut u64;
    crate::gc::layout_rebuild_from_slots(obj as *mut u8, fields, slot_count);
    if crate::arena::pointer_in_old_gen(obj as usize) {
        for i in 0..slot_count {
            let slot = fields.add(i);
            crate::gc::runtime_write_barrier_slot(obj as usize, slot as usize, *slot);
        }
    }
}

#[inline]
pub(crate) unsafe fn rebuild_array_layout_from_slots(arr: *mut ArrayHeader) {
    if arr.is_null() {
        return;
    }
    let len = (*arr).length as usize;
    let slots = crate::array::array_elements_ptr(arr as *const ArrayHeader) as *mut u64;
    crate::gc::layout_rebuild_from_slots(arr as *mut u8, slots, len);
    // The GC pointer bitmap is not the only per-array fact a direct slot write
    // invalidates. Every caller here reached this function because it set
    // `length` and `std::ptr::write`-d the element words itself, bypassing the
    // noting store helpers — and `js_array_alloc` births the array carrying
    // `GC_ARRAY_RAW_F64_LAYOUT` (vacuously true at length 0). Re-derive that
    // flag from the same slots this rebuild just walked, or a NaN-boxed pointer
    // sits in a slot the flag promises is a plain double. Clear-only, so an
    // array that really is all-numbers keeps its fast path.
    crate::array::reclassify_array_numeric_layout_from_slots(arr);
    if crate::arena::pointer_in_old_gen(arr as usize) {
        for i in 0..len {
            let slot = slots.add(i);
            crate::gc::runtime_write_barrier_slot(arr as usize, slot as usize, *slot);
        }
    }
}
