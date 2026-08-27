//! Array element-slot GC bookkeeping: slot ranges, write-barrier notes, and
//! layout rebuild/replay for `ArrayHeader`.
//!
//! Split out of `header.rs` to keep it under the 2,000-line file gate.

use super::header::*;
use super::ArrayHeader;

pub(crate) unsafe fn gc_element_slot_range(
    arr: *mut ArrayHeader,
) -> Option<crate::gc::HeapSlotRange> {
    if arr.is_null() {
        return None;
    }
    let length = (*arr).length as usize;
    let capacity = (*arr).capacity as usize;
    if length > capacity || length > 16_000_000 {
        return None;
    }
    Some(crate::gc::HeapSlotRange::new(
        array_elements_ptr(arr),
        length,
    ))
}

#[inline]
pub(crate) unsafe fn note_array_slot(arr: *mut ArrayHeader, index: usize, value_bits: u64) {
    let value_bits = canonicalize_array_numeric_store_bits(arr, value_bits);
    // GC_STORE_AUDIT(BARRIERED): shared helper notes layout and emits the array slot barrier below.
    std::ptr::write(array_elements_ptr(arr).add(index), value_bits);
    note_array_numeric_index_write(arr, index, value_bits);
    crate::gc::layout_note_slot(arr as usize, index, value_bits);
    let slot = array_elements_ptr(arr).add(index) as usize;
    crate::gc::runtime_write_barrier_slot(arr as usize, slot, value_bits);
}

/// [`note_array_slot`] for a live plain Array whose flag word was already
/// read by the caller's receiver guard. This preserves the exact store,
/// numeric-layout, element-shape, per-slot-layout, and barrier sequence
/// without redispatching through `array_numeric_layout` merely to recover the
/// same raw-f64 bits.
///
/// # Safety
///
/// `arr` must be a live, forwarding-resolved `GC_TYPE_ARRAY`; `index` must be
/// inside its allocation, and `flags` must be the current preceding
/// `GcHeader::_reserved` word with no intervening safepoint.
#[inline]
pub(crate) unsafe fn note_array_slot_resolved_flags(
    arr: *mut ArrayHeader,
    index: usize,
    value: f64,
    flags: u16,
) {
    let value = canonicalize_array_numeric_store_value_from_flags(flags, value);
    let mut value_bits = value.to_bits();
    let slot_ptr = array_elements_ptr(arr).add(index);
    let old_bits = std::ptr::read(slot_ptr);
    // GC_STORE_AUDIT(BARRIERED): resolved-flags helper notes layout and emits the array slot barrier below.
    std::ptr::write(slot_ptr, value_bits);
    value_bits = note_array_numeric_index_write(arr, index, value_bits);
    crate::gc::layout_note_slot_aware(arr as usize, index, value_bits, old_bits);
    let slot = slot_ptr as usize;
    crate::gc::runtime_write_barrier_slot(arr as usize, slot, value_bits);
}

#[inline]
pub(crate) unsafe fn note_array_slot_layout_only(
    arr: *mut ArrayHeader,
    index: usize,
    value_bits: u64,
) {
    let value_bits = canonicalize_array_numeric_store_bits(arr, value_bits);
    // GC_STORE_AUDIT(INIT): layout-only helper is restricted to fresh/suppressed caller sites.
    std::ptr::write(array_elements_ptr(arr).add(index), value_bits);
    note_array_numeric_index_write(arr, index, value_bits);
    crate::gc::layout_note_slot(arr as usize, index, value_bits);
    // "Fresh/suppressed caller" does NOT imply barrier-free: a BORN-OLD array
    // (>16KB, e.g. a >2048-element JSON.parse result) is old-gen from birth, so
    // storing a young child creates an old→young edge that later minors need in
    // the remembered set — GC suppression only protects DURING the caller's
    // fill, not after it returns. This was the missing-edge bug behind the
    // old-young-edge-verifier failures (155 edges, all born-old array→young
    // object; slot_page_ever_dirty=false = the store never hit any barrier):
    // JSON.parse filled born-old arrays through this helper, the children were
    // swept live on a later minor → "value is not a function". The old-gen
    // check hits the page-generation cache (same array → same cached range), so
    // young arrays pay ~one cached compare.
    if crate::arena::pointer_in_old_gen(arr as usize) {
        let slot = array_elements_ptr(arr).add(index) as usize;
        crate::gc::runtime_write_barrier_slot(arr as usize, slot, value_bits);
    }
}

#[inline]
pub(crate) unsafe fn store_array_slot(arr: *mut ArrayHeader, index: usize, value_bits: u64) {
    let value_bits = canonicalize_array_numeric_store_bits(arr, value_bits);
    note_array_numeric_index_write(arr, index, value_bits);
    let slot = array_elements_ptr(arr).add(index) as usize;
    let stored_bits = if array_has_raw_f64_layout_flag(arr) {
        match value_bits_to_number(value_bits) {
            Some(number) => number.to_bits(),
            None => {
                clear_array_numeric_layout(arr);
                value_bits
            }
        }
    } else {
        value_bits
    };
    crate::gc::runtime_store_jsvalue_slot(arr as usize, slot, index, stored_bits);
}

#[inline]
pub(crate) unsafe fn rebuild_array_layout(arr: *mut ArrayHeader) {
    if arr.is_null() {
        return;
    }
    // #7480: this is the post-hoc funnel most bulk element mutators use —
    // `shift`, `unshift`, `splice`, `fill`, `copyWithin`, and `reverse` all
    // mutate slots with bare `ptr::write` / `ptr::copy` and then land here.
    // NOT `sort`: its default path writes the rank permutation back through
    // `RootedArrayElems::set`, so it revokes through the STORE funnel
    // (`layout_note_slot`) instead — established by sabotage in #7608's
    // matrix (removing the revoke here leaves the sort test green). They are permutations or arbitrary
    // rewrites, so the element-shape proof is dropped conservatively; a
    // still-homogeneous array re-earns it on the next `ensure`.
    super::element_shape::clear_element_shape(arr);
    let length = (*arr).length as usize;
    let capacity = (*arr).capacity as usize;
    if length > capacity || length > 16_000_000 {
        clear_array_numeric_layout(arr);
        crate::gc::layout_mark_unknown(arr as *mut u8);
        return;
    }
    crate::gc::layout_rebuild_from_slots(arr as *mut u8, array_elements_ptr(arr), length);
    refresh_array_numeric_layout(arr);
    if crate::arena::pointer_in_old_gen(arr as usize) {
        let slots = array_elements_ptr(arr);
        for i in 0..length {
            let slot = slots.add(i);
            crate::gc::runtime_write_barrier_slot(arr as usize, slot as usize, *slot);
        }
    }
}

#[inline]
pub(crate) unsafe fn rebuild_array_layout_exact(arr: *mut ArrayHeader) {
    if arr.is_null() {
        return;
    }
    // #7480: same conservative drop as `rebuild_array_layout` — see there.
    super::element_shape::clear_element_shape(arr);
    let length = (*arr).length as usize;
    let capacity = (*arr).capacity as usize;
    if length > capacity || length > 16_000_000 {
        clear_array_numeric_layout(arr);
        crate::gc::layout_mark_unknown(arr as *mut u8);
        return;
    }
    crate::gc::layout_rebuild_exact_from_slots(arr as *mut u8, array_elements_ptr(arr), length);
    refresh_array_numeric_layout(arr);
    if crate::arena::pointer_in_old_gen(arr as usize) {
        let slots = array_elements_ptr(arr);
        for i in 0..length {
            let slot = slots.add(i);
            crate::gc::runtime_write_barrier_slot(arr as usize, slot as usize, *slot);
        }
    }
}

#[inline]
pub(crate) unsafe fn replay_array_growth_write_barriers(arr: *mut ArrayHeader) {
    if arr.is_null() || !crate::arena::pointer_in_old_gen(arr as usize) {
        return;
    }

    let length = (*arr).length as usize;
    if length == 0 || length > 16_000_000 {
        return;
    }

    let slots = array_elements_ptr(arr);
    if crate::gc::layout_visit_pointer_slots_for_user(arr as usize, length, |index| {
        let slot = slots.add(index);
        crate::gc::runtime_write_barrier_slot(arr as usize, slot as usize, *slot);
    }) {
        return;
    }

    // One parent, one contiguous slot run — the loop form of the barrier, whose
    // per-store entry point would re-derive the parent classification `length`
    // times and re-assert a page-granular fact ~512 times per page. See
    // `gc::barrier::replay_old_parent_slot_range`.
    crate::gc::replay_old_parent_slot_range_barriers(arr as usize, slots, length);
}

#[inline]
pub(crate) unsafe fn mark_array_layout_unknown(arr: *mut ArrayHeader) {
    clear_array_numeric_layout(arr);
    crate::gc::layout_mark_unknown(arr as *mut u8);
}

/// Minimum initial capacity for arrays to reduce reallocations
pub(crate) const MIN_ARRAY_CAPACITY: u32 = 16;
