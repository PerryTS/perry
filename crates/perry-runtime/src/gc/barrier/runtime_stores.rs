//! Runtime-side barriered store helpers, split from `barrier/mod.rs`
//! for the 2000-line file cap.

use super::*;

#[inline]
pub(crate) fn runtime_store_gc_heap_word_slot(
    parent_user: usize,
    slot_addr: usize,
    value_bits: u64,
) {
    unsafe {
        std::ptr::write(slot_addr as *mut u64, value_bits);
    }
    runtime_write_barrier_gc_slot(parent_user, slot_addr, value_bits);
}

#[inline]
pub(crate) fn runtime_store_gc_jsvalue_slot(parent_user: usize, slot_addr: usize, value_bits: u64) {
    runtime_store_gc_heap_word_slot(parent_user, slot_addr, value_bits);
}

#[inline]
pub(crate) fn runtime_store_external_heap_word_slot(
    parent_user: usize,
    slot_addr: usize,
    value_bits: u64,
) {
    unsafe {
        std::ptr::write(slot_addr as *mut u64, value_bits);
    }
    runtime_write_barrier_external_slot(parent_user, slot_addr, value_bits);
}

#[inline]
pub(crate) fn runtime_store_external_jsvalue_slot(
    parent_user: usize,
    slot_addr: usize,
    value_bits: u64,
) {
    runtime_store_external_heap_word_slot(parent_user, slot_addr, value_bits);
}

// #854: GC write-barrier external-slot store-with-layout path
#[allow(dead_code)]
#[inline]
pub(crate) fn runtime_store_external_jsvalue_slot_with_layout(
    parent_user: usize,
    slot_addr: usize,
    slot_index: usize,
    value_bits: u64,
) {
    unsafe {
        std::ptr::write(slot_addr as *mut u64, value_bits);
    }
    layout_note_slot(parent_user, slot_index, value_bits);
    runtime_write_barrier_external_slot(parent_user, slot_addr, value_bits);
}

pub(crate) fn runtime_write_barrier_external_slot_span(
    parent_addr: usize,
    first_slot_addr: usize,
    slot_count: usize,
) {
    if !write_barriers_enabled() {
        return;
    }
    dirty_external_slot_span(parent_addr, first_slot_addr, slot_count);
}

pub(crate) fn dirty_external_slot_span(
    parent_addr: usize,
    first_slot_addr: usize,
    slot_count: usize,
) {
    if parent_addr < GC_HEADER_SIZE || first_slot_addr == 0 || slot_count == 0 {
        return;
    }
    if !barrier_parent_needs_remembering(parent_addr, true) {
        return;
    }
    let Some(bytes) = slot_count.checked_mul(std::mem::size_of::<u64>()) else {
        return;
    };
    let Some(last_byte) = first_slot_addr.checked_add(bytes.saturating_sub(1)) else {
        return;
    };
    bump_write_barrier_trace_counter(BarrierTraceCounter::ConservativeParentSpanMarks);
    let header_addr = parent_addr - GC_HEADER_SIZE;
    let first_page = crate::arena::generation_page_for_addr(first_slot_addr);
    let last_page = crate::arena::generation_page_for_addr(last_byte);
    for page in first_page..=last_page {
        mark_dirty_external_slot_page(header_addr, page);
    }
}

pub(crate) fn remembered_dirty_page_count() -> usize {
    DIRTY_OLD_PAGES.with(|old| {
        let old = old.borrow();
        EXTERNAL_DIRTY_SLOT_PAGES.with(|external| {
            let external = external.borrow();
            if external.is_empty() {
                return old.len();
            }
            let mut pages = crate::fast_hash::new_ptr_hash_set();
            for &page in old.iter() {
                pages.insert(page);
            }
            for &page in external.keys() {
                pages.insert(page);
            }
            pages.len()
        })
    })
}
