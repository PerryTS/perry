//! Owned Map storage must die on each sweep path, including active blocks,
//! and iterator history must follow a header without address re-keying.
use super::super::*;
use super::support::*;
use crate::map::*;

#[test]
fn map_store_full_sweep_reclaims_dead_active_block_and_preserves_live_owner() {
    for stepped in [false, true] {
        std::thread::spawn(move || {
            let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
            let _scan = ConservativeScanDisabledGuard::new();
            reset_global_roots();
            let _roots = ShadowAndGlobalRootResetGuard;
            let live = js_map_alloc(8);
            js_map_set(live, 42.0, 99.0);
            let mut root = ptr_bits(live as usize);
            js_gc_register_global_root(&mut root as *mut u64 as i64);
            let dead = js_map_alloc(8);
            js_map_set(dead, 1.0, 2.0);
            let before = test_map_side_deallocation_snapshot();
            let mut cycle = GcCycleState::new_full(GcTriggerSnapshot {
                kind: GcTriggerKind::Manual,
                steps_before: Some(GcStepSnapshot::current()),
            });
            if stepped {
                while !cycle.step(GcWorkBudget::bounded(1)).completed {}
            } else {
                cycle.run_to_completion();
            }
            let after = test_map_side_deallocation_snapshot();
            assert_eq!((after.0 - before.0, after.1 - before.1), (1, 128));
            let live = (root & crate::value::POINTER_MASK) as *mut MapHeader;
            assert!(is_registered_map(live as usize));
            assert_eq!(js_map_get(live, 42.0), 99.0);
            let before = test_map_side_deallocation_snapshot();
            release_current_thread_map_side_allocations();
            release_current_thread_map_side_allocations();
            let after = test_map_side_deallocation_snapshot();
            assert_eq!((after.0 - before.0, after.1 - before.1), (1, 128));
        })
        .join()
        .unwrap();
    }
}

#[test]
fn map_store_compaction_history_survives_actual_copying_collection() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let map = js_map_alloc(40);
    for i in 0..40 {
        js_map_set(map, i as f64, i as f64);
    }
    let epoch = map_compaction_epoch(map);
    for i in 0..21 {
        js_map_delete(map, i as f64);
    }
    assert_ne!(map_compaction_epoch(map), epoch);
    js_shadow_slot_set(0, ptr_bits(map as usize));
    // A dead Map in from-space: only the copying minor's from-space
    // Map-start walk can free its store, since the block is reset in bulk.
    let dead = js_map_alloc(8);
    js_map_set(dead, 1.0, 2.0);
    let from_space_before = test_from_space_map_finalizations();
    let dealloc_before = test_map_side_deallocation_snapshot();
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(
        test_from_space_map_finalizations() - from_space_before,
        1,
        "the from-space walk must finalize exactly the dead Map"
    );
    assert_eq!(dealloc_delta(dealloc_before), (1, 128));
    let moved = (js_shadow_slot_get(0) & crate::value::POINTER_MASK) as *mut MapHeader;
    assert_ne!(map, moved, "the test must actually move its owner");
    let raw = unsafe { map_cursor_next_raw(moved, 21, epoch) }.unwrap();
    assert_eq!(raw, 0);
    assert_eq!(js_map_entry_key_at(moved, raw), 21.0);
}

/// Map side-allocation deallocations `(count, bytes)` since `before`.
fn dealloc_delta(before: (u64, u64)) -> (u64, u64) {
    let after = test_map_side_deallocation_snapshot();
    (after.0 - before.0, after.1 - before.1)
}

fn ptr_from_slot(slot: u32) -> *mut MapHeader {
    (js_shadow_slot_get(slot) & crate::value::POINTER_MASK) as *mut MapHeader
}

/// Run `body` on a fresh thread, so the exit walk it ends with sees only the
/// Maps this test allocated.
fn on_fresh_thread(body: impl FnOnce() + Send + 'static) {
    std::thread::spawn(body).join().unwrap();
}

/// Evacuate the Map rooted in shadow slot 0 through old-generation
/// evacuation (the tenured-nursery path of the non-copying minor), never the
/// copying minor: a registered copy-only root scanner makes the copying minor
/// fall back. Returns the moved address.
fn evacuate_rooted_tenured_map_to_old() -> *mut MapHeader {
    let map = ptr_from_slot(0);
    let _copy_only = TemporaryCopyOnlyRootScanner::rust_bits(&[]);
    unsafe {
        (*header_from_user_ptr(map as *const u8)).gc_flags |= GC_FLAG_TENURED;
    }
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let moved = ptr_from_slot(0);
    assert!(
        !trace.copying_nursery.eligible,
        "the subject is old-generation evacuation, not the copying minor"
    );
    assert_ne!(moved, map, "the tenured Map must actually move");
    assert!(crate::arena::pointer_in_old_gen(moved as usize));
    assert!(trace.evacuation.moved_objects > 0);
    assert!(
        trace.evacuation.released_original_objects > 0,
        "the original's FORWARDED bit must be released, or the hazard is absent"
    );
    moved
}

/// Runs a full collection to completion.
fn full_collection() {
    GcCycleState::new_full(GcTriggerSnapshot {
        kind: GcTriggerKind::Manual,
        steps_before: Some(GcStepSnapshot::current()),
    })
    .run_to_completion();
}

/// Old-generation evacuation releases FORWARDED on the original header
/// before the sweep. The original must not still own the store: if it does,
/// the sweep frees the live copy's store as a dead Map, and the exit walk
/// frees it a second time.
#[test]
fn map_store_survives_tenured_evacuation_sweeps_and_exit_walk() {
    on_fresh_thread(|| {
        let _isolation = copying_nursery_isolation_lock();
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _barriers = GeneratedWriteBarrierTestGuard::active();
        let _force = ForcedEvacuationTestGuard::on();
        let _scan = ConservativeScanDisabledGuard::new();
        reset_shadow_stack();
        reset_global_roots();
        reset_remembered_set();
        let _roots = ShadowAndGlobalRootResetGuard;
        let frame = js_shadow_frame_push(1);
        let map = js_map_alloc(8);
        js_map_set(map, 42.0, 99.0);
        js_shadow_slot_set(0, ptr_bits(map as usize));

        let before = test_map_side_deallocation_snapshot();
        let moved = evacuate_rooted_tenured_map_to_old();
        assert_eq!(
            dealloc_delta(before),
            (0, 0),
            "the cycle that moved a live Map must not free its store"
        );
        assert_eq!(js_map_get(moved, 42.0), 99.0);

        full_collection();
        full_collection();
        let moved = ptr_from_slot(0);
        assert_eq!(dealloc_delta(before), (0, 0), "a later sweep freed the live store");
        assert!(is_registered_map(moved as usize));
        assert_eq!(js_map_get(moved, 42.0), 99.0);
        js_map_set(moved, 43.0, 100.0);
        assert_eq!(js_map_get(moved, 43.0), 100.0);

        release_current_thread_map_side_allocations();
        assert_eq!(
            dealloc_delta(before),
            (1, 128),
            "the exit walk must free the one live store exactly once"
        );
        js_shadow_frame_pop(frame);
    });
}

/// Old-page defrag moves an old Map and releases the original's FORWARDED
/// bit the same way; its store must stay with the copy too.
#[test]
fn map_store_survives_old_page_defrag_sweeps_and_exit_walk() {
    on_fresh_thread(|| {
        let _isolation = copying_nursery_isolation_lock();
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _barriers = GeneratedWriteBarrierTestGuard::active();
        let _force = ForcedEvacuationTestGuard::on();
        let _scan = ConservativeScanDisabledGuard::new();
        reset_shadow_stack();
        reset_global_roots();
        reset_remembered_set();
        let _roots = ShadowAndGlobalRootResetGuard;
        let frame = js_shadow_frame_push(1);

        // Fill the current old block so the evacuated Map opens a fresh one,
        // then leave a dead neighbour behind it: a fragmented page for the
        // forced defrag policy to select.
        let _filler =
            crate::arena::arena_alloc_gc_old(2 * 1024 * 1024 - GC_HEADER_SIZE, 8, GC_TYPE_STRING);
        let map = js_map_alloc(8);
        js_map_set(map, 42.0, 99.0);
        js_shadow_slot_set(0, ptr_bits(map as usize));
        let old_map = evacuate_rooted_tenured_map_to_old();
        let _dead = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_STRING);
        let old_header = unsafe { header_from_user_ptr(old_map as *const u8) };
        let total = unsafe { (*old_header).size as usize };
        let map_pages = crate::arena::old_object_page_overlaps(old_header as usize, total)
            .into_iter()
            .map(|(page, _)| page)
            .collect::<Vec<_>>();
        unsafe {
            (*old_header).gc_flags |= GC_FLAG_MARKED;
        }
        let _ = sweep_with_age_bump(false);
        let selected = select_old_page_defrag_pages(true);
        assert!(
            map_pages.iter().any(|page| selected.pages.contains(page)),
            "the test must seed an old-page defrag candidate holding the Map"
        );

        let before = test_map_side_deallocation_snapshot();
        let trace = collect_minor_trace(GcTriggerKind::Direct);
        let moved = ptr_from_slot(0);
        assert_ne!(moved, old_map, "old-page defrag must actually move the Map");
        assert!(trace.evacuation.old_page_moved_objects > 0);
        assert_eq!(
            dealloc_delta(before),
            (0, 0),
            "the defrag cycle must not free the live store"
        );
        assert_eq!(js_map_get(moved, 42.0), 99.0);

        full_collection();
        let moved = ptr_from_slot(0);
        assert_eq!(dealloc_delta(before), (0, 0), "a later sweep freed the live store");
        assert_eq!(js_map_get(moved, 42.0), 99.0);

        release_current_thread_map_side_allocations();
        assert_eq!(
            dealloc_delta(before),
            (1, 128),
            "the exit walk must free the one live store exactly once"
        );
        js_shadow_frame_pop(frame);
    });
}

/// #6010: a dead Map in the ACTIVE nursery block must be finalized by a
/// minor that does not copy, both monolithic (the copy-only-roots fallback)
/// and budgeted (always non-moving), not only by full cycles or the copying
/// minor's from-space walk. Barriers stay active: without them a minor keeps
/// every young object (the dead Map is marked and age-bumped), so nothing is
/// dead for it to finalize.
#[test]
fn map_store_non_copying_minor_reclaims_dead_active_block_map() {
    for budgeted in [false, true] {
        on_fresh_thread(move || {
            let _isolation = copying_nursery_isolation_lock();
            let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
            let _barriers = GeneratedWriteBarrierTestGuard::active();
            let _scan = ConservativeScanDisabledGuard::new();
            reset_shadow_stack();
            reset_global_roots();
            reset_remembered_set();
            let _roots = ShadowAndGlobalRootResetGuard;
            let frame = js_shadow_frame_push(1);
            let live = js_map_alloc(8);
            js_map_set(live, 42.0, 99.0);
            js_shadow_slot_set(0, ptr_bits(live as usize));
            let dead = js_map_alloc(8);
            js_map_set(dead, 1.0, 2.0);
            let from_space_before = test_from_space_map_finalizations();

            let before = test_map_side_deallocation_snapshot();
            let trace = if budgeted {
                let mut state = test_start_budgeted_minor_fallback_state_with_trace(
                    GcTriggerKind::ArenaBytes,
                    GcProgressKind::NormalIncremental,
                );
                while state.phase() != GcCyclePhase::Complete {
                    state.step(GcWorkBudget::bounded(1));
                }
                state
                    .take_outcome()
                    .expect("cycle should complete")
                    .trace
                    .expect("test requested GC trace capture")
            } else {
                let _copy_only = TemporaryCopyOnlyRootScanner::rust_bits(&[]);
                collect_minor_trace(GcTriggerKind::Direct)
            };
            assert!(
                !trace.copying_nursery.eligible,
                "the subject is a non-copying minor"
            );
            assert_eq!(
                test_from_space_map_finalizations(),
                from_space_before,
                "the copying minor's from-space walk must not be what freed it"
            );
            assert_eq!(
                dealloc_delta(before),
                (1, 128),
                "the dead Map's store must be freed by the minor that found it dead"
            );
            let live = ptr_from_slot(0);
            assert_eq!(js_map_get(live, 42.0), 99.0);
            release_current_thread_map_side_allocations();
            assert_eq!(dealloc_delta(before), (2, 256));
            js_shadow_frame_pop(frame);
        });
    }
}
