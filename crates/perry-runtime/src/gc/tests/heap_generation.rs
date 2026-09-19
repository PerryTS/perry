//! The heap generation advances across every kind of event that frees or moves
//! heap memory (#10164's cross-call search position depends on it).
//!
//! Each test drives one kind through its production entry point and asserts two
//! things: the generation advanced, and a scope of that kind opened. The second
//! is what makes the test fail when that kind's `HeapChange::begin` is removed,
//! even where an enclosing scope would still advance the generation.

use super::super::heap_generation::{heap_changes_of_kind, heap_generation, HeapChangeKind};
use super::super::promote_in_place::InPlacePromotionTestGuard;
use super::super::*;
use super::support::*;

fn assert_advanced(kind: HeapChangeKind, generation_before: u64, kind_before: u64) {
    assert!(
        heap_generation() > generation_before,
        "{kind:?} must advance the heap generation"
    );
    assert!(
        heap_changes_of_kind(kind) > kind_before,
        "{kind:?} must open its own HeapChange scope"
    );
}

fn full_collection(kind: GcTriggerKind) {
    let _ = gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot {
        kind,
        steps_before: Some(GcStepSnapshot::current()),
    });
}

#[test]
fn a_copying_minor_advances_the_heap_generation() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = young_leaf();
    js_shadow_slot_set(0, ptr_bits(child));
    let (generation, kind) = (
        heap_generation(),
        heap_changes_of_kind(HeapChangeKind::CopyingMinor),
    );

    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_ne!(
        (js_shadow_slot_get(0) & POINTER_MASK) as usize,
        child,
        "the witness must actually have moved"
    );
    assert_advanced(HeapChangeKind::CopyingMinor, generation, kind);
}

#[test]
fn an_in_place_promotion_advances_the_heap_generation() {
    let _guard = CopyingNurseryTestGuard::new(4);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _promote = InPlacePromotionTestGuard::enabled(1000);
    let child = young_leaf();
    js_shadow_slot_set(0, ptr_bits(child));
    let (generation, kind) = (
        heap_generation(),
        heap_changes_of_kind(HeapChangeKind::Promotion),
    );

    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert!(
        trace.copying_nursery.in_place_promoted_objects > 0,
        "the cycle must have promoted in place"
    );
    assert_advanced(HeapChangeKind::Promotion, generation, kind);
}

#[test]
fn a_full_sweep_advances_the_heap_generation() {
    let _isolation = copying_nursery_isolation_lock();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _scan = ConservativeScanDisabledGuard::new();
    let _dead = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_STRING);
    let (generation, kind) = (
        heap_generation(),
        heap_changes_of_kind(HeapChangeKind::Sweep),
    );

    full_collection(GcTriggerKind::Direct);

    assert_advanced(HeapChangeKind::Sweep, generation, kind);
}

#[test]
fn an_emergency_reclaim_advances_the_heap_generation() {
    let _isolation = copying_nursery_isolation_lock();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _scan = ConservativeScanDisabledGuard::new();
    let _dead = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_STRING);
    let (generation, kind) = (
        heap_generation(),
        heap_changes_of_kind(HeapChangeKind::Sweep),
    );

    // The emergency full is the ordinary full cycle under another trigger; it
    // frees through the same Sweep scope.
    full_collection(GcTriggerKind::Emergency);

    assert_advanced(HeapChangeKind::Sweep, generation, kind);
}

#[test]
fn freeing_a_malloc_object_advances_the_heap_generation() {
    let _isolation = copying_nursery_isolation_lock();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _scan = ConservativeScanDisabledGuard::new();
    let dead = gc_malloc(256, GC_TYPE_STRING);
    let header = unsafe { header_from_user_ptr(dead as *const u8) };
    assert!(gc_malloc_header_is_tracked(header));
    let (generation, kind) = (
        heap_generation(),
        heap_changes_of_kind(HeapChangeKind::Sweep),
    );

    full_collection(GcTriggerKind::Direct);

    assert!(
        !gc_malloc_header_is_tracked(header),
        "the unreachable malloc object must have been freed"
    );
    assert_advanced(HeapChangeKind::Sweep, generation, kind);
}

#[test]
fn an_incremental_reclaim_step_advances_the_heap_generation() {
    let _legacy_pacing = crate::gc::policy::force_legacy_gc_pacing();
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.set(crate::arena::old_gen_in_use_bytes()));
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(false));
    let live = young_leaf();
    js_shadow_slot_set(0, ptr_bits(live));
    let _dead = allocate_dead_malloc_churn_headers(8);
    GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.set(malloc_object_count().saturating_sub(1)));
    gc_check_trigger();

    let mut status = JsGcStepResult::default();
    let mut reached_reclaim = false;
    for _ in 0..500_000 {
        js_gc_step_work_units(1, &mut status);
        if status.status == JS_GC_STEP_STATUS_ACTIVE
            && status.phase == GcCyclePhase::Reclaim.ffi_code()
        {
            reached_reclaim = true;
            break;
        }
        assert_eq!(
            status.status, JS_GC_STEP_STATUS_ACTIVE,
            "the cycle ended before Reclaim"
        );
    }
    assert!(reached_reclaim, "the budgeted cycle must reach Reclaim");
    let (generation, kind) = (
        heap_generation(),
        heap_changes_of_kind(HeapChangeKind::Reclaim),
    );

    let completed = complete_budgeted_gc_cycle();

    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);
    assert_advanced(HeapChangeKind::Reclaim, generation, kind);
}

fn forced_evacuating_minor() -> GcCycleTrace {
    let _defrag = super::super::oldgen_defrag::OldDefragTestEnable::new();
    let (parent, fields) = unsafe { alloc_old_test_object(1) };
    let parent_header = unsafe { header_from_user_ptr(parent as *const u8) };
    let _dead = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_STRING);
    unsafe {
        (*parent_header).gc_flags |= GC_FLAG_MARKED;
    }
    let _ = sweep_with_age_bump(false);
    let _frame = js_shadow_frame_push(1);
    let child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let _copy_only_root_guard = TemporaryCopyOnlyRootScanner::rust_bits(&[ptr_bits(child)]);
    unsafe {
        *fields = ptr_bits(child);
    }
    js_write_barrier_slot(ptr_bits(parent as usize), fields as u64, ptr_bits(child));
    js_shadow_slot_set(0, ptr_bits(parent as usize));
    collect_minor_trace(GcTriggerKind::Direct)
}

struct ResetGcTestState;

impl Drop for ResetGcTestState {
    fn drop(&mut self) {
        reset_shadow_stack();
        reset_global_roots();
        reset_remembered_set();
        clear_marks();
        clear_mark_seeds();
        CONS_PINNED.with(|s| s.borrow_mut().clear());
    }
}

macro_rules! evacuation_setup {
    () => {
        let _reset = ResetGcTestState;
        let _scan = ConservativeScanDisabledGuard::new();
        let _isolation = copying_nursery_isolation_lock();
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _force = ForcedEvacuationTestGuard::on();
        let _barrier_guard = GeneratedWriteBarrierTestGuard::active();
        reset_shadow_stack();
        reset_global_roots();
        reset_remembered_set();
        clear_marks();
        clear_mark_seeds();
        CONS_PINNED.with(|s| s.borrow_mut().clear());
    };
}

#[test]
fn minor_prelude_evacuation_advances_the_heap_generation() {
    evacuation_setup!();
    let (generation, kind) = (
        heap_generation(),
        heap_changes_of_kind(HeapChangeKind::Evacuation),
    );

    let trace = forced_evacuating_minor();

    assert!(
        trace.evacuation.objects > 0,
        "the minor must have evacuated"
    );
    assert_advanced(HeapChangeKind::Evacuation, generation, kind);
}

#[test]
fn old_page_compaction_advances_the_heap_generation() {
    evacuation_setup!();
    let (generation, kind) = (
        heap_generation(),
        heap_changes_of_kind(HeapChangeKind::Compaction),
    );

    let trace = forced_evacuating_minor();

    assert!(
        trace.evacuation.old_page_moved_objects >= 1,
        "the minor must have moved an object off a selected old page"
    );
    assert_advanced(HeapChangeKind::Compaction, generation, kind);
}

#[test]
fn a_moving_realloc_advances_the_heap_generation() {
    let _isolation = copying_nursery_isolation_lock();
    let mut ptr = gc_malloc(64, GC_TYPE_STRING);
    let original = unsafe { header_from_user_ptr(ptr as *const u8) };
    let (generation, kind) = (
        heap_generation(),
        heap_changes_of_kind(HeapChangeKind::Realloc),
    );

    for payload in [1024 * 1024, 4 * 1024 * 1024, 16 * 1024 * 1024] {
        ptr = gc_realloc(ptr, payload);
        if unsafe { header_from_user_ptr(ptr as *const u8) } != original {
            break;
        }
    }

    assert_ne!(
        unsafe { header_from_user_ptr(ptr as *const u8) },
        original,
        "the realloc must have moved the object"
    );
    assert_advanced(HeapChangeKind::Realloc, generation, kind);
}

#[test]
// `debug_assert_heap_change_open` is `#[cfg(debug_assertions)]`, so under a
// release profile it cannot panic and this twin cannot pass. CI's cargo-test
// builds debug and never sees it; `cargo test --release -p perry-runtime` did.
// Do NOT "fix" the test instead: it is correct, the profile changed what the
// code means. `[profile.gcaudit]` is the only profile giving release codegen
// with assertions live, and is where this should be exercised under release.
#[cfg_attr(not(debug_assertions), ignore = "asserts a debug_assert! fires")]
fn a_free_or_move_outside_every_scope_is_caught_in_debug_builds() {
    let caught = std::panic::catch_unwind(|| {
        crate::gc::heap_generation::debug_assert_heap_change_open();
    });
    assert!(
        caught.is_err(),
        "the funnel assertion must fire with no scope open"
    );
    let _scope = crate::gc::heap_generation::HeapChange::begin(HeapChangeKind::Sweep);
    crate::gc::heap_generation::debug_assert_heap_change_open();
}
