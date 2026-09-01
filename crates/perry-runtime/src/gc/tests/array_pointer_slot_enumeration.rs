//! #9261: the collector's slot enumeration must reach every array element that
//! holds a heap reference.

use super::super::*;
use super::support::*;

use crate::gc::verify::{
    verify_array_pointer_slots_enumerated, verify_array_pointer_slots_enumerated_for,
    ArraySlotEnumerationStats,
};

/// Build a mask-described array: one numeric element, then one pointer.
///
/// The numeric element first is load-bearing. An EMPTY array's first pointer
/// append publishes `GC_LAYOUT_ALL_POINTERS` ("its sole element is the pointer
/// we just classified"), and under that state every element is enumerated by
/// construction — the omission this file is about cannot be expressed. With a
/// numeric prefix the append takes the per-object mask instead, which is the
/// state that can under-report.
fn mask_described_array() -> *mut crate::array::ArrayHeader {
    let arr = crate::array::js_array_alloc(8);
    let arr = crate::array::js_array_push_f64(arr, 1.0);
    let child = young_leaf();
    let arr = crate::array::js_array_push_f64(arr, f64::from_bits(ptr_bits(child)));
    let header = unsafe { header_from_user_ptr(arr as *const u8) };
    assert_eq!(
        unsafe { (*header)._reserved } & crate::gc::GC_LAYOUT_STATE_MASK,
        crate::gc::GC_LAYOUT_SIDE_MASK,
        "fixture must be described by a per-object pointer mask, or the \
         sabotage below is not expressible and every verdict here is vacuous"
    );
    arr
}

unsafe fn stats_for(arr: *mut crate::array::ArrayHeader) -> ArraySlotEnumerationStats {
    let mut stats = ArraySlotEnumerationStats::default();
    verify_array_pointer_slots_enumerated_for(&mut stats, header_from_user_ptr(arr as *const u8));
    stats
}

/// Publish a pointer at the append position WITHOUT the layout note every
/// store path performs. This is the state #9261 found in the wild on an
/// object's spill buffer: `mask=0xc7fc live=0xfffc`, three live `STRING_TAG`
/// elements the mask omitted. The check must SEE it — a checker that cannot
/// fail is documentation.
#[test]
fn array_slot_enumeration_reports_a_pointer_element_the_layout_omits() {
    let _isolation = copying_nursery_isolation_lock();
    let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let arr = mask_described_array();

    // The fixture itself must be clean, and must have LOOKED at something.
    let clean = unsafe { stats_for(arr) };
    assert_eq!(
        clean.unenumerated_slots, 0,
        "a correctly-noted array must not be reported"
    );
    assert_eq!(clean.checked_arrays, 1);
    assert!(
        clean.checked_pointer_slots >= 1,
        "no pointer element was examined, so the clean verdict above is vacuous"
    );

    let planted = young_leaf();
    let index = unsafe { (*arr).length } as usize;
    unsafe {
        let elements =
            (arr as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *mut u64;
        std::ptr::write(elements.add(index), ptr_bits(planted));
        (*arr).length = index as u32 + 1;
    }

    let broken = unsafe { stats_for(arr) };
    assert_eq!(
        broken.unenumerated_slots, 1,
        "the planted element is a live heap edge the collector cannot reach"
    );
    let missing = broken
        .first
        .expect("the first offending element is recorded");
    assert_eq!(missing.index, index);
    assert_eq!(missing.child, planted);
    assert_eq!(missing.array, arr as usize);

    clear_marks();
    remembered_set_clear();
}

/// The whole-heap form is what the copied minor calls, so it must actually
/// walk arrays rather than return an empty verdict.
#[test]
fn array_slot_enumeration_walks_the_heap() {
    let _isolation = copying_nursery_isolation_lock();
    let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let arr = mask_described_array();
    // The walk skips unmarked nursery objects (they are this cycle's garbage,
    // and their elements are the previous tenant's bytes). Nothing has marked
    // anything here, so pin the fixture to make it a live subject.
    let header = unsafe { header_from_user_ptr(arr as *const u8) };
    unsafe {
        crate::gc::pin_object(header);
    }

    let stats = verify_array_pointer_slots_enumerated();
    assert!(
        stats.checked_arrays >= 1 && stats.checked_pointer_slots >= 1,
        "the heap walk examined no array element ({stats:?}), so a clean \
         verdict from it would mean nothing"
    );

    unsafe {
        crate::gc::unpin_object(header);
    }
    clear_marks();
    remembered_set_clear();
}
