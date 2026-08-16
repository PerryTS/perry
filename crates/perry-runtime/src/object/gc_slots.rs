use super::{shapes, ObjectHeader};
use crate::ArrayHeader;

/// The object's keys-array child slot, given the receiver's `ShapeDescriptor`.
///
/// #8122: the collector resolves the receiver's `ShapeDescriptor` ONCE per
/// traced object (`gc::layout::gc_child_slots`) and threads it through the
/// keys-slot, field-range, payload-mask and slot-visit steps, which used to
/// probe the shape table independently — five `shape_descriptor_by_id`
/// lookups per traced object, the top leaf of a traced in-place-promotion
/// cycle's profile. Callers therefore pass the descriptor rather than the
/// function re-deriving it.
pub(crate) unsafe fn gc_keys_array_slot(
    obj: *mut ObjectHeader,
    descriptor: Option<shapes::ShapeDescriptor>,
) -> Option<*mut u64> {
    if obj.is_null() {
        return None;
    }
    if let Some(descriptor) = descriptor {
        // Compatibility scratch slot: GC obtains the authoritative edge from
        // the ShapeId descriptor, then lets the existing slot visitor rewrite
        // it in place. #8047 can replace this scratch with a descriptor-table
        // rewrite without changing the source of the edge.
        //
        // The descriptor lookup immediately precedes this collector-side
        // materialization: no allocation or callback can change its `keys`
        // edge before the visitor receives the slot. That ordering is what
        // makes the descriptor authoritative while this legacy field remains
        // only the mutable scratch location used for pointer rewriting.
        // GC_STORE_AUDIT(ROOT): collector materializes the authoritative descriptor root into its compatibility rewrite slot.
        (*obj).keys_array = descriptor.keys as usize as *mut ArrayHeader;
    }
    if (*obj).keys_array.is_null() {
        return None;
    }
    Some(&mut (*obj).keys_array as *mut _ as *mut u64)
}

/// The object's inline field-slot range, given the receiver's `ShapeDescriptor`
/// (see [`gc_keys_array_slot`]).
pub(crate) unsafe fn gc_field_slot_range(
    obj: *mut ObjectHeader,
    descriptor: Option<shapes::ShapeDescriptor>,
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
    let field_count = descriptor
        .map(|descriptor| descriptor.live_inline_slot_count as usize)
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
    let slots = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut u64;
    crate::gc::layout_rebuild_from_slots(arr as *mut u8, slots, len);
    if crate::arena::pointer_in_old_gen(arr as usize) {
        for i in 0..len {
            let slot = slots.add(i);
            crate::gc::runtime_write_barrier_slot(arr as usize, slot as usize, *slot);
        }
    }
}
