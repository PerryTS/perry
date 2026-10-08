//! Owned Map storage must die on each sweep path, including active blocks,
//! and iterator history must follow a header without address re-keying.
use super::super::*;
use super::support::*;
use crate::map::*;

#[test]
fn map_growth_arms_a_deferred_safepoint_with_a_current_arena_base() {
    on_fresh_thread(|| {
        let _guard = CopyingNurseryTestGuard::new(1);
        let triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        policy::set_safepoint_pending(false);
        policy::GC_SAFEPOINT_DEFER_ARENA_BASE.with(|base| base.set(0));
        let before = gc_collection_count();
        let map = js_map_alloc(4);
        js_shadow_slot_set(0, ptr_bits(map as usize));
        let mut entries = 0;
        while !policy::GC_SAFEPOINT_PENDING.with(std::cell::Cell::get) {
            js_map_set(map, entries as f64, entries as f64);
            entries += 1;
            assert!(
                entries <= 1_100_000,
                "Map growth must cross the external allocation step"
            );
        }
        assert!(
            unsafe { (*map).capacity } >= 524288,
            "the external growth subject must be live"
        );
        assert_eq!(
            gc_collection_count(),
            before,
            "external growth must defer collection"
        );
        let current = crate::arena::arena_total_bytes();
        assert!(current > 0, "zero cannot hide a stale base");
        assert_eq!(
            policy::GC_SAFEPOINT_DEFER_ARENA_BASE.with(std::cell::Cell::get),
            current
        );
        // A further external pulse must preserve the first deferral base.
        policy::gc_note_external_side_alloc(16 * 1024 * 1024);
        assert_eq!(
            policy::GC_SAFEPOINT_DEFER_ARENA_BASE.with(std::cell::Cell::get),
            current
        );
        policy::gc_note_external_side_free(16 * 1024 * 1024);
        triggers.make_arena_trigger_due();
        js_gc_loop_safepoint();
        assert!(
            gc_collection_count() > before,
            "the precise safepoint must consume pressure"
        );
        assert!(!policy::GC_SAFEPOINT_PENDING.with(std::cell::Cell::get));
        let moved = ptr_from_slot(0);
        assert_eq!(
            js_map_get(moved, (entries - 1) as f64),
            (entries - 1) as f64
        );
    });
}

#[test]
fn stale_external_deferral_base_turns_the_map_growth_witness_red() {
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "gc::tests::map_store::map_growth_arms_a_deferred_safepoint_with_a_current_arena_base",
            "--nocapture",
        ])
        .env("PERRY_B4_SABOTAGE", "external_base")
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&child.stdout).contains("running 1 test"));
    assert!(
        !child.status.success(),
        "the stale nursery base must be detected"
    );
}

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
            let before = test_thread_map_side_deallocation_snapshot();
            let mut cycle = GcCycleState::new_full(GcTriggerSnapshot {
                kind: GcTriggerKind::Manual,
                steps_before: Some(GcStepSnapshot::current()),
            });
            if stepped {
                while !cycle.step(GcWorkBudget::bounded(1)).completed {}
            } else {
                cycle.run_to_completion();
            }
            let after = test_thread_map_side_deallocation_snapshot();
            assert_eq!((after.0 - before.0, after.1 - before.1), (1, 128));
            let live = (root & crate::value::POINTER_MASK) as *mut MapHeader;
            assert!(is_registered_map(live as usize));
            assert_eq!(js_map_get(live, 42.0), 99.0);
            let before = test_thread_map_side_deallocation_snapshot();
            release_current_thread_map_side_allocations();
            release_current_thread_map_side_allocations();
            let after = test_thread_map_side_deallocation_snapshot();
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
    let dealloc_before = test_thread_map_side_deallocation_snapshot();
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
    let after = test_thread_map_side_deallocation_snapshot();
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

        let before = test_thread_map_side_deallocation_snapshot();
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
        assert_eq!(
            dealloc_delta(before),
            (0, 0),
            "a later sweep freed the live store"
        );
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
/// bit the same way; its store must stay with the copy too. Defrag is opt-in
/// on the allocation path and armed by idle compaction; the test enables it.
#[test]
fn map_store_survives_old_page_defrag_sweeps_and_exit_walk() {
    on_fresh_thread(|| {
        let _isolation = copying_nursery_isolation_lock();
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _barriers = GeneratedWriteBarrierTestGuard::active();
        let _force = ForcedEvacuationTestGuard::on();
        let _defrag = super::super::oldgen_defrag::OldDefragTestEnable::new();
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

        let before = test_thread_map_side_deallocation_snapshot();
        // Defrag runs in the non-copying minor's evacuation phase.
        let trace = {
            let _copy_only = TemporaryCopyOnlyRootScanner::rust_bits(&[]);
            collect_minor_trace(GcTriggerKind::Direct)
        };
        let moved = ptr_from_slot(0);
        assert_ne!(moved, old_map, "old-page defrag must actually move the Map");
        assert!(trace.evacuation.released_original_objects > 0);
        assert!(trace.evacuation.old_page_moved_objects > 0);
        assert_eq!(
            dealloc_delta(before),
            (0, 0),
            "the defrag cycle must not free the live store"
        );
        assert_eq!(js_map_get(moved, 42.0), 99.0);

        full_collection();
        let moved = ptr_from_slot(0);
        assert_eq!(
            dealloc_delta(before),
            (0, 0),
            "a later sweep freed the live store"
        );
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
/// minor's from-space walk. The Map is alone in its block: a reachable
/// neighbour in a recent block would make block persistence force-mark it,
/// and a force-marked Map is live, not leaked.
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
            let dead = js_map_alloc(8);
            js_map_set(dead, 1.0, 2.0);
            let from_space_before = test_from_space_map_finalizations();
            let force_marks_before = crate::gc::block_persist_force_mark_count();

            let before = test_thread_map_side_deallocation_snapshot();
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
                crate::gc::block_persist_force_mark_count(),
                force_marks_before,
                "block persistence kept the Map alive; the fixture proves nothing"
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
            release_current_thread_map_side_allocations();
            assert_eq!(dealloc_delta(before), (1, 128), "and never freed again");
        });
    }
}
/// Growth, retained clear/delete capacity, and real GC finalization must all
/// agree with the reserved index payload, including the dense numeric table.
#[test]
fn map_index_external_bytes_balance_after_growth_clear_delete_and_collection() {
    for clear in [false, true] {
        std::thread::spawn(move || {
            let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
            let _scan = ConservativeScanDisabledGuard::new();
            reset_global_roots();
            let _roots = ShadowAndGlobalRootResetGuard;
            let baseline = policy::external_side_live_bytes();
            let map = js_map_alloc(4);
            let mut keys = Vec::new();
            for i in 0..512 {
                let text = format!("map-accounting-key-{i}");
                let string = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
                let object = crate::object::js_object_alloc(0, 0);
                let string_key = f64::from_bits(crate::value::STRING_TAG | string as u64);
                let object_key = f64::from_bits(ptr_bits(object as usize));
                for key in [i as f64, -(i as f64) - 0.5, string_key, object_key] {
                    js_map_set(map, key, i as f64);
                    keys.push(key);
                }
                unsafe {
                    let entries = (*map).capacity as usize * 2 * std::mem::size_of::<f64>();
                    let indexes = test_map_index_bytes(map);
                    assert!(indexes > 0, "fixture must allocate all key indexes");
                    assert_eq!(
                        policy::external_side_live_bytes(),
                        baseline + entries + indexes
                    );
                }
            }
            // Model a relocated pointer key so the GC hook must rebuild,
            // rather than taking its unchanged-keys fast path.
            let replacement = crate::object::js_object_alloc(0, 0);
            let replacement_bits = ptr_bits(replacement as usize);
            let raw = keys.len() - 1;
            unsafe {
                crate::gc::runtime_store_external_jsvalue_slot(
                    map as usize,
                    (*map).entries.add(raw * 2) as usize,
                    replacement_bits,
                );
            }
            keys[raw] = f64::from_bits(replacement_bits);
            let before = policy::external_side_live_bytes();
            rebuild_map_ptr_index_for_gc(map);
            assert_eq!(policy::external_side_live_bytes(), before);
            assert_eq!(js_map_get(map, keys[raw]), 511.0);
            if clear {
                js_map_clear(map);
            } else {
                for key in keys {
                    assert_eq!(js_map_delete(map, key), 1);
                }
            }
            assert_eq!(js_map_size(map), 0);
            unsafe {
                let entries = (*map).capacity as usize * 2 * std::mem::size_of::<f64>();
                let indexes = test_map_index_bytes(map);
                assert!(indexes > 0, "empty tables retain allocated capacity");
                assert_eq!(
                    policy::external_side_live_bytes(),
                    baseline + entries + indexes
                );
            }
            // The pointer above is deliberately not a root. A real full sweep
            // must reach the Map finalizer even after its keys were removed.
            let before = test_thread_map_side_deallocation_snapshot();
            let mut cycle = GcCycleState::new_full(GcTriggerSnapshot {
                kind: GcTriggerKind::Manual,
                steps_before: Some(GcStepSnapshot::current()),
            });
            cycle.run_to_completion();
            assert_eq!(test_thread_map_side_deallocation_snapshot().0 - before.0, 1);
            assert_eq!(policy::external_side_live_bytes(), baseline);
        })
        .join()
        .unwrap();
    }
}

/// Run one copying minor that promotes the young generation in place, and
/// assert it took the path asked for.
fn promote_young_in_place(untraced: bool) {
    let cycles = untraced_promotion_cycles();
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert!(
        trace.copying_nursery.in_place_promotion,
        "the cycle must promote in place"
    );
    assert_eq!(
        untraced_promotion_cycles() > cycles,
        untraced,
        "untraced path taken: {} (decline reason: {})",
        untraced_promotion_cycles() > cycles,
        crate::gc::copying::last_untraced_decline_reason()
    );
}

/// Fill a Map with `n` numeric entries, `key -> key * 2`.
fn filled_map(n: usize) -> *mut MapHeader {
    let map = js_map_alloc(8);
    for i in 0..n {
        js_map_set(map, i as f64, (i * 2) as f64);
    }
    map
}

/// #12141: an untraced in-place promotion marks nothing, so its unmarked
/// Maps are live. The copying minor's from-space Map walk ran before the
/// promoted blocks left the young arenas and freed the store of every Map on
/// them, including a rooted one; the next `set` wrote through a null store.
#[test]
fn an_untraced_promotion_keeps_the_store_of_a_live_map() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _promote = super::super::promote_in_place::InPlacePromotionTestGuard::untraced();
    let live = filled_map(1000);
    js_shadow_slot_set(0, ptr_bits(live as usize));
    // The dropped cohort, past the 131k entries the crashing probe needed.
    let _dropped = filled_map(131_100);

    let before = test_thread_map_side_deallocation_snapshot();
    promote_young_in_place(true);
    assert_eq!(
        ptr_from_slot(0),
        live,
        "in-place promotion must not move it"
    );
    assert_eq!(
        dealloc_delta(before),
        (0, 0),
        "an untraced promotion proves nothing dead, so it must free no store"
    );
    assert!(
        test_map_side_allocation(live as usize).is_some(),
        "the live Map lost its store"
    );
    unsafe {
        assert_eq!((*live).size, 1000);
        assert!(!(*live).entries.is_null());
    }
    assert_eq!(js_map_get(live, 999.0), 1998.0);
    // Growing it goes through the store the walk used to null.
    for i in 1000..2000 {
        js_map_set(live, i as f64, (i * 2) as f64);
    }
    assert_eq!(js_map_get(live, 1999.0), 3998.0);
    unsafe {
        assert_eq!((*live).size, 2000);
    }
}

/// The traced half of #12141: a traced in-place promotion's marks are real,
/// so the live Map keeps its store and the dead one is reclaimed, no later
/// than the next full collection.
#[test]
fn a_traced_promotion_keeps_the_live_map_and_reclaims_the_dead_one() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _promote = super::super::promote_in_place::InPlacePromotionTestGuard::enabled(1000);
    let live = filled_map(1000);
    js_shadow_slot_set(0, ptr_bits(live as usize));
    let _dead = filled_map(64);

    let before = test_thread_map_side_deallocation_snapshot();
    promote_young_in_place(false);
    assert_eq!(ptr_from_slot(0), live);
    assert!(test_map_side_allocation(live as usize).is_some());
    full_collection();
    let live = ptr_from_slot(0);
    assert!(test_map_side_allocation(live as usize).is_some());
    assert_eq!(js_map_get(live, 999.0), 1998.0);
    assert_eq!(
        dealloc_delta(before).0,
        1,
        "the dead Map's store must be freed by the promotion or the next full"
    );
}
