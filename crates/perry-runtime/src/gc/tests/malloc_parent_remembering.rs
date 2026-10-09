//! A malloc parent owes the remembered set what an old parent owes it.
//!
//! A minor traces a malloc object only when it reaches it, and frees one only
//! when its malloc sweep is due. A dead malloc object therefore survives most
//! minors untraced, still registered. If a young word in it was never
//! remembered, a minor that moves or frees that child leaves the word naming
//! recycled nursery memory. A later trace through such a parent can therefore
//! interpret unrelated bytes as an object. OpenCode TUI initialization's
//! old-young-edge verifier reported a missing edge in a malloc closure
//! (`old_arena=false`, `child_type=scope`, `slot_page_ever_dirty=false`). The
//! exact connection to its intermittent gc-pin-latch abort remains unproven.
//!
//! Each test leaves the malloc parent unreachable across a minor whose malloc
//! sweep is not due, then asserts that the parent's young word names a live
//! object. The two tests cover the two places a malloc parent's young edge
//! was created without an entry:
//!
//!   * the newborn gate (`newborn_parent_needs_barrier`) on a large-capture
//!     closure birth;
//!   * a store through `js_write_barrier_slot`, the generated-code entry, which
//!     passes `external_slot = false`, behind the emitted #7511 gate.
use super::support::*;
use crate::gc::*;

fn large_capture_count() -> usize {
    LARGE_OBJECT_THRESHOLD_BYTES / 8 + 64
}

/// The parent is a malloc object, so the test exercises the malloc path.
fn assert_malloc_parent(closure: *const crate::closure::ClosureHeader) {
    unsafe {
        assert_eq!(
            (*header_from_user_ptr(closure as *const u8)).gc_flags & GC_FLAG_ARENA,
            0,
            "the fixture must be a malloc parent; a large-capture closure is born there"
        );
    }
    assert!(malloc_user_ptr_tracked(closure as *mut u8));
}

/// Collect a minor in which the malloc parent is unreachable and is neither
/// traced nor swept.
fn minor_leaving_the_parent_untraced(closure: *const crate::closure::ClosureHeader) {
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert!(
        malloc_user_ptr_tracked(closure as *mut u8),
        "the parent must survive the minor unswept, or the test checks nothing"
    );
}

/// The capture word names a live object: an address the heap walk finds an
/// object at. A word left naming the recycled nursery does not.
fn assert_capture_names_a_live_object(closure: *const crate::closure::ClosureHeader, index: u32) {
    let bits = crate::closure::js_closure_get_capture_bits(closure, index);
    let child = (bits & POINTER_MASK) as usize;
    assert!(
        build_valid_pointer_set().contains(&child),
        "capture {index} of the malloc parent names {child:#x}, which is no live object \
         after a minor that did not trace the parent: its young edge was never remembered"
    );
}

#[test]
fn a_large_capture_closure_born_with_a_young_capture_keeps_it_valid() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _scan = ConservativeScanDisabledGuard::new();
    register_runtime_handle_root_scanner_for_tests();
    assert!(
        incremental_mark_barrier_globally_idle(),
        "with a cycle live the newborn gate takes the barrier anyway; this test needs it idle"
    );

    let child = young_leaf();
    let mut captures = vec![crate::value::TAG_UNDEFINED; large_capture_count()];
    captures[1] = string_bits(child);
    let closure = crate::closure::js_closure_alloc_init(
        std::ptr::null(),
        captures.len() as u32,
        captures.as_ptr(),
    );
    assert_malloc_parent(closure);
    assert_eq!(
        crate::closure::js_closure_get_capture_bits(closure, 1),
        string_bits(child),
        "precondition: the capture still names the nursery child"
    );

    minor_leaving_the_parent_untraced(closure);
    assert_capture_names_a_live_object(closure, 1);
}

#[test]
fn a_generated_store_into_a_malloc_parent_is_remembered() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _scan = ConservativeScanDisabledGuard::new();
    register_runtime_handle_root_scanner_for_tests();

    let captures = vec![crate::value::TAG_UNDEFINED; large_capture_count()];
    let closure = crate::closure::js_closure_alloc_init(
        std::ptr::null(),
        captures.len() as u32,
        captures.as_ptr(),
    );
    assert_malloc_parent(closure);

    // What a generated store does: the raw slot write, then the #7511 gate on
    // the live header, then `js_write_barrier_slot` with the NaN-boxed parent.
    // The parent was born pointer-free; the layout is widened first, as the
    // store site's layout note would, so the collector visits the slot at all.
    let child = young_leaf();
    let child_bits = string_bits(child);
    unsafe {
        crate::gc::layout_init_unknown_fresh(closure as *mut u8);
        let slot = crate::closure::closure_capture_slots_mut(closure).add(1);
        *slot = child_bits;
        let flags = (*header_from_user_ptr(closure as *const u8)).gc_flags;
        assert!(
            parent_flags_may_need_remembering(flags),
            "the emitted gate must not skip a store into a malloc parent"
        );
        js_write_barrier_slot(ptr_bits(closure as usize), slot as u64, child_bits);
    }

    minor_leaving_the_parent_untraced(closure);
    assert_capture_names_a_live_object(closure, 1);
}
