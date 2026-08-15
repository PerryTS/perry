use super::{shapes, ObjectHeader};
use crate::ArrayHeader;

pub(crate) unsafe fn gc_keys_array_slot(obj: *mut ObjectHeader) -> Option<*mut u64> {
    if obj.is_null() {
        return None;
    }
    if let Some(descriptor) = shapes::object_shape_descriptor(obj) {
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

pub(crate) unsafe fn gc_field_slot_range(
    obj: *mut ObjectHeader,
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
    let field_count = shapes::object_shape_descriptor(obj)
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
