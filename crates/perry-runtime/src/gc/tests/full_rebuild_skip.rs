//! #10182: a synchronous full replaces its old→young remembered-set rebuild by
//! an exact clear when the rebuild could only produce an empty set — no marked
//! or pinned young object, and no malloc object.
//!
//! Each case runs on a fresh thread (empty arenas, empty malloc registry). The
//! protective cases plant an old→young or old→malloc edge that ONLY the rebuild
//! can recover — a raw store with no write barrier, so the pre-cycle dirty
//! snapshot does not cover it — and check the remembered set covers it after
//! the full. Their sabotaged twins force the skip and show the edge is lost.

use super::super::*;
use super::support::*;
use crate::gc::trace::block_skip::sabotage;

fn run_isolated(test: fn()) {
    std::thread::spawn(move || {
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _scan = ConservativeScanDisabledGuard::new();
        let _barriers = GeneratedWriteBarrierTestGuard::active();
        reset_global_roots();
        reset_remembered_set();
        let _roots = ShadowAndGlobalRootResetGuard;
        test();
    })
    .join()
    .expect("full-rebuild-skip test thread must not panic");
}

fn synchronous_full() -> GcCycleTrace {
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot {
        kind: GcTriggerKind::OldGenBytes,
        steps_before: Some(GcStepSnapshot::current()),
    })
    .trace
    .expect("full GC trace requested")
}

/// A rooted old parent with one field. The root cell is leaked so its address
/// stays valid for the thread's lifetime.
fn rooted_old_parent() -> (usize, *mut u64) {
    let (parent, fields) = unsafe { alloc_old_test_object(1) };
    let root: &'static mut u64 = Box::leak(Box::new(ptr_bits(parent as usize)));
    js_gc_register_global_root(root as *mut u64 as i64);
    (parent as usize, fields)
}

/// The old→young edge verifier checks a parent only while it is marked or
/// pinned (or already remembered); after a completed full no mark is left, so
/// check the known-live parent explicitly.
fn verify_live_parent(parent: usize) -> OldYoungEdgeVerifyStats {
    let header = unsafe { header_from_user_ptr(parent as *const u8) };
    unsafe {
        (*header).gc_flags |= GC_FLAG_MARKED;
    }
    let stats = verify_old_to_young_edges_collect();
    unsafe {
        (*header).gc_flags &= !GC_FLAG_MARKED;
    }
    stats
}

/// Store `child_bits` into the parent's field WITHOUT a write barrier.
fn raw_store(parent: usize, fields: *mut u64, child_bits: u64) {
    unsafe {
        *fields = child_bits;
        layout_note_slot(parent, 0, child_bits);
    }
}

#[test]
fn a_full_with_no_live_young_object_skips_the_rebuild() {
    run_isolated(|| {
        let (parent, fields) = rooted_old_parent();
        let (old_child, _) = unsafe { alloc_old_test_object(0) };
        raw_store(parent, fields, ptr_bits(old_child as usize));
        // Young garbage: the young generation is in use but nothing in it lives.
        for _ in 0..512 {
            let _ = unsafe { alloc_nursery_test_object(2) };
        }
        let skips = crate::gc::full_remembered_rebuilds_skipped();

        let trace = synchronous_full();

        assert_eq!(
            crate::gc::full_remembered_rebuilds_skipped(),
            skips + 1,
            "a full with an unmarked young generation must skip the rebuild"
        );
        assert_eq!(trace.old_to_young_rebuild_objects_scanned, 0);
        assert_eq!(
            remembered_set_size(),
            0,
            "the skipped rebuild leaves an empty set"
        );
        let verify = verify_live_parent(parent);
        assert_eq!(verify.missing_edges, 0, "{verify:?}");

        // The mutator's next old→young store is still remembered and survives
        // a copying minor.
        let child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
        assert!(crate::arena::pointer_in_nursery(child));
        raw_store(parent, fields, ptr_bits(child));
        js_write_barrier_slot(ptr_bits(parent), fields as u64, ptr_bits(child));
        let _ = collect_minor_trace(GcTriggerKind::Direct);
        let slot_child = (unsafe { *fields } & POINTER_MASK) as usize;
        assert_eq!(
            unsafe { (*header_from_user_ptr(slot_child as *const u8)).obj_type },
            GC_TYPE_OBJECT,
            "the barrier-recorded young child must survive the minor"
        );
    });
}

/// The young child is reachable only through an old parent, stored without a
/// barrier: the rebuild is the only thing that can remember it.
fn plant_unbarriered_young_edge() -> (usize, *mut u64, usize) {
    let (parent, fields) = rooted_old_parent();
    let child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    assert!(crate::arena::pointer_in_nursery(child));
    raw_store(parent, fields, ptr_bits(child));
    assert_eq!(
        remembered_set_size(),
        0,
        "premise: no barrier recorded the edge"
    );
    (parent, fields, child)
}

#[test]
fn a_live_young_child_keeps_the_rebuild_and_its_edge() {
    run_isolated(|| {
        let (parent, fields, child) = plant_unbarriered_young_edge();
        let skips = crate::gc::full_remembered_rebuilds_skipped();

        let trace = synchronous_full();

        assert_eq!(crate::gc::full_remembered_rebuilds_skipped(), skips);
        assert!(trace.old_to_young_rebuild_objects_scanned > 0);
        let verify = verify_live_parent(parent);
        assert!(verify.checked_old_to_young_edges > 0, "premise: {verify:?}");
        assert_eq!(
            verify.missing_edges, 0,
            "the rebuild must remember the edge"
        );
        assert_eq!(unsafe { *fields } & POINTER_MASK, child as u64);
    });
}

#[test]
fn sabotaged_skip_loses_an_unbarriered_young_edge() {
    run_isolated(|| {
        let (parent, _, _) = plant_unbarriered_young_edge();
        {
            let _sabotage = sabotage::Guard::arm(sabotage::FORCE_REBUILD_SKIP);
            let _ = synchronous_full();
        }
        let verify = verify_live_parent(parent);
        assert!(
            verify.missing_edges > 0,
            "a wrongly skipped rebuild must leave the young edge unremembered: {verify:?}"
        );
    });
}

/// A malloc-registry child of an old parent, stored without a barrier, with an
/// empty young generation: only the malloc guard stops the skip.
fn plant_unbarriered_malloc_edge() -> usize {
    activate_malloc_registry_for_tests();
    let (parent, fields) = rooted_old_parent();
    let symbol = alloc_tracked_test_symbol() as usize;
    assert!(malloc_user_ptr_tracked(symbol as *mut u8));
    raw_store(parent, fields, ptr_bits(symbol));
    assert_eq!(
        remembered_set_size(),
        0,
        "premise: no barrier recorded the edge"
    );
    parent
}

#[test]
fn a_live_malloc_child_keeps_the_rebuild_and_its_edge() {
    run_isolated(|| {
        let parent = plant_unbarriered_malloc_edge();
        let skips = crate::gc::full_remembered_rebuilds_skipped();

        let _ = synchronous_full();

        assert_eq!(crate::gc::full_remembered_rebuilds_skipped(), skips);
        let verify = verify_live_parent(parent);
        assert!(verify.checked_old_to_young_edges > 0, "premise: {verify:?}");
        assert_eq!(
            verify.missing_edges, 0,
            "the rebuild must remember the malloc edge"
        );
    });
}

#[test]
fn sabotaged_skip_loses_an_unbarriered_malloc_edge() {
    run_isolated(|| {
        let parent = plant_unbarriered_malloc_edge();
        {
            let _sabotage = sabotage::Guard::arm(sabotage::FORCE_REBUILD_SKIP);
            let _ = synchronous_full();
        }
        let verify = verify_live_parent(parent);
        assert!(
            verify.missing_edges > 0,
            "a wrongly skipped rebuild must leave the malloc edge unremembered: {verify:?}"
        );
    });
}
