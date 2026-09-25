//! Test-only accessors over the slot-layout machinery (pointer-slot counts,
//! rewrite-slot enumeration, and the probe counters). Split out of
//! `gc/layout.rs` to keep it under the 2,000-line cap (#10750); the probe
//! thread-locals themselves stay in `layout.rs`.

use super::*;

#[inline]
pub(crate) fn test_layout_pointer_slot_count(user_ptr: usize, slot_count: usize) -> Option<usize> {
    let mut count = 0usize;
    if layout_visit_pointer_slots(user_ptr, slot_count, |_| count += 1) {
        Some(count)
    } else {
        None
    }
}

pub(crate) fn test_gc_rewrite_slot_count(user_ptr: usize) -> Option<usize> {
    if user_ptr < GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let header = unsafe { header_from_user_ptr(user_ptr as *const u8) };
    let mut count = 0usize;
    unsafe {
        visit_gc_rewrite_slot_descriptors(header, |descriptor| {
            let mut visit_slot = |_| {
                count += 1;
            };
            descriptor.visit_slots(&mut visit_slot);
        });
    }
    Some(count)
}

pub(crate) fn test_gc_rewrite_slot_addresses(user_ptr: usize) -> Option<Vec<usize>> {
    if user_ptr < GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let header = unsafe { header_from_user_ptr(user_ptr as *const u8) };
    let mut slots = Vec::new();
    unsafe {
        visit_gc_rewrite_slot_descriptors(header, |descriptor| {
            descriptor.visit_slots(&mut |slot| slots.push(slot.slot as usize));
        });
    }
    Some(slots)
}

pub(in crate::gc) fn test_reset_trace_slot_reads() {
    TRACE_SLOT_READS.with(|c| c.set(0));
}

pub(in crate::gc) fn test_trace_slot_reads() -> usize {
    TRACE_SLOT_READS.with(|c| c.get())
}

pub(in crate::gc) fn test_reset_typed_slot_descriptor_probes() {
    TYPED_SLOT_DESCRIPTOR_PROBES.with(|c| c.set(0));
}

pub(in crate::gc) fn test_typed_slot_descriptor_probes() -> usize {
    TYPED_SLOT_DESCRIPTOR_PROBES.with(Cell::get)
}

pub(crate) fn test_reset_typed_raw_f64_descriptor_queries() {
    TYPED_RAW_F64_DESCRIPTOR_QUERIES.with(|c| c.set(0));
}

pub(crate) fn test_typed_raw_f64_descriptor_queries() -> usize {
    TYPED_RAW_F64_DESCRIPTOR_QUERIES.with(Cell::get)
}
