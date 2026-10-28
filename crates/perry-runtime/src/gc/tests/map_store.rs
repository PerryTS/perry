//! Owned Map storage must die on each sweep path, including active blocks,
//! and iterator history must follow a header without address re-keying.
use super::super::*;
use super::support::*;
use crate::map::*;

#[test]
fn map_gc_store_growth_is_collected_only_at_a_rooted_safepoint() {
    on_fresh_thread(|| {
        let _guard = CopyingNurseryTestGuard::new(1);
        let triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let map = js_map_alloc(4);
        js_shadow_slot_set(0, ptr_bits(map as usize));
        let before = gc_collection_count();
        for i in 0..2000 {
            js_map_set(map, i as f64, i as f64);
        }
        assert_eq!(
            gc_collection_count(),
            before,
            "raw receiver growth must not collect"
        );
        triggers.make_arena_trigger_due();
        // Model the pending request made by the allocation-point pacing
        // probe; the subject here is collection after the raw growth window.
        policy::set_safepoint_pending(true);
        js_gc_loop_safepoint();
        assert!(
            gc_collection_count() > before,
            "the rooted safepoint must consume GC storage pressure"
        );
        assert_eq!(js_map_get(ptr_from_slot(0), 1999.0), 1999.0);
    });
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
            root = 0;
            std::hint::black_box(root);
            full_collection();
            full_collection();
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
    // store-start walk can free its young store, since the block is reset in
    // bulk.
    let dead = js_map_alloc(8);
    js_map_set(dead, 1.0, 2.0);
    let dealloc_before = test_thread_map_side_deallocation_snapshot();
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(dealloc_delta(dealloc_before), (1, 128));
    full_collection();
    assert_eq!(
        dealloc_delta(dealloc_before),
        (1, 128),
        "and never freed again"
    );
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

        js_shadow_slot_set(0, 0);
        full_collection();
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

        // Retain permanent startup shape metadata before the destructive
        // sweep, so promotion cannot reuse holes on its stationary block.
        gc_init();
        isolate_runtime_layout_blocks();

        // Fill the current old block so the evacuated Map opens a fresh one,
        // then leave a dead neighbour behind it: a fragmented page for the
        // forced defrag policy to select.
        let _filler =
            crate::arena::arena_alloc_gc_old(2 * 1024 * 1024 - GC_HEADER_SIZE, 8, GC_TYPE_STRING);
        let map = js_map_alloc(8);
        js_map_set(map, 42.0, 99.0);
        js_shadow_slot_set(0, ptr_bits(map as usize));
        // Keep the nonmoving store off the movable-header block: defrag
        // evacuates whole source blocks and refuses nonmoving neighbors.
        let _padding =
            crate::arena::arena_alloc_gc_old(2 * 1024 * 1024 - GC_HEADER_SIZE, 8, GC_TYPE_STRING);
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
            (*header_from_user_ptr(test_map_store_word(old_map) as *const u8)).gc_flags |=
                GC_FLAG_MARKED;
        }
        // This fixture drives the sweep directly, so perform the permanent
        // layout metadata root visit that a real full trace would do first.
        let valid = build_valid_pointer_set();
        crate::object::shapes::scan_shape_table_rekey_mut(&mut RuntimeRootVisitor::for_mark(
            &valid,
        ));
        let _ = sweep_with_age_bump(false);
        let selected = select_old_page_defrag_pages(true);
        assert!(
            map_pages.iter().any(|page| selected.pages.contains(page)),
            "the test must seed an old-page defrag candidate holding the Map"
        );

        let blocks = crate::arena::old_arena_source_blocks_for_pages(&selected.pages);
        let mut immovable = Vec::new();
        crate::arena::old_arena_walk_objects_on_pages(&blocks.pages, |header| {
            let header = header as *mut GcHeader;
            let flags = unsafe { (*header).gc_flags };
            if flags & (GC_FLAG_FORWARDED | GC_FLAG_PINNED | GC_FLAG_SHAPE_SHARED) != 0
                || !gc_type_is_movable(unsafe { (*header).obj_type })
                || unsafe { super::super::never_move::blocks_evacuation(header) }
            {
                immovable.push((header as usize, unsafe { (*header).obj_type }, flags));
            }
        });
        let before = test_thread_map_side_deallocation_snapshot();
        // Defrag runs in the non-copying minor's evacuation phase.
        let trace = {
            let _copy_only = TemporaryCopyOnlyRootScanner::rust_bits(&[]);
            collect_minor_trace(GcTriggerKind::Direct)
        };
        let moved = ptr_from_slot(0);
        assert_ne!(
            moved, old_map,
            "old-page defrag must actually move the Map; source refusals: {immovable:?}"
        );
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

        js_shadow_slot_set(0, 0);
        full_collection();
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
                dealloc_delta(before),
                (1, 128),
                "the minor's sweep finalizes the dead young store"
            );
            full_collection();
            assert_eq!(dealloc_delta(before), (1, 128));
            full_collection();
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
                    let indexes = test_map_index_bytes(map);
                    assert!(indexes > 0, "fixture must allocate all key indexes");
                    assert_eq!(policy::external_side_live_bytes(), baseline + indexes);
                }
            }
            // Model a relocated pointer key so the GC hook must rebuild,
            // rather than taking its unchanged-keys fast path.
            let replacement = crate::object::js_object_alloc(0, 0);
            let replacement_bits = ptr_bits(replacement as usize);
            let raw = keys.len() - 1;
            unsafe {
                crate::gc::runtime_store_gc_jsvalue_slot(
                    test_map_store_word(map),
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
                let indexes = test_map_index_bytes(map);
                assert!(indexes > 0, "empty tables retain allocated capacity");
                assert_eq!(policy::external_side_live_bytes(), baseline + indexes);
            }
            // The pointer above is deliberately not a root. A real full sweep
            // must reach the Map finalizer even after its keys were removed.
            let before = test_thread_map_side_deallocation_snapshot();
            let cycle = GcCycleState::new_full(GcTriggerSnapshot {
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

/// The public Map owns the collecting store, and the store owns its values.
/// A weak value witness prevents stale mapped test pages from faking liveness.
#[test]
fn map_store_owned_edge_retains_values_after_moving_owner() {
    owned_edge_witness(false);
}
#[test]
#[ignore = "negative control: erase the Map's only strong store edge"]
fn negative_control_objtables_map_store() {
    owned_edge_witness(true);
}
fn owned_edge_witness(erase: bool) {
    let _guard = CopyingNurseryTestGuard::new(3);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _scan = ConservativeScanDisabledGuard::new();
    crate::weakref::test_support::clear_weak_holders();
    let map = js_map_alloc(4);
    let value = unsafe { alloc_old_test_object(0).0 };
    js_map_set(map, 37.0, f64::from_bits(ptr_bits(value as usize)));
    let weak = crate::weakref::js_weakref_new(f64::from_bits(ptr_bits(value as usize)));
    js_shadow_slot_set(0, ptr_bits(map as usize));
    js_shadow_slot_set(1, ptr_bits(weak as usize));
    let store = unsafe { test_map_store_word(map) };
    assert!(
        crate::arena::pointer_in_nursery(store),
        "stores are born young"
    );
    assert!(gc_type_is_movable(GC_TYPE_MAP_STORE));
    let _ = gc_collect_minor();
    let moved = ptr_from_slot(0);
    assert_ne!(moved, map);
    assert_ne!(
        unsafe { test_map_store_word(moved) },
        store,
        "the store moves too"
    );
    if erase {
        // GC_STORE_AUDIT(TEST): intentional edge deletion is the control.
        unsafe {
            std::ptr::write((moved as *mut u8).add(16).cast::<usize>(), 0);
        }
    }
    full_collection();
    let retained = crate::weakref::js_weakref_deref(f64::from_bits(js_shadow_slot_get(1)));
    assert_ne!(
        retained.to_bits(),
        crate::value::TAG_UNDEFINED,
        "only the Map-to-store-to-value edges retain this weak target"
    );
    let moved = ptr_from_slot(0);
    assert_eq!(js_map_get(moved, 37.0).to_bits(), retained.to_bits());
    let fresh = js_map_alloc(4);
    assert_eq!(
        js_map_get(fresh, 37.0).to_bits(),
        crate::value::TAG_UNDEFINED
    );
    js_shadow_slot_set(0, 0);
    full_collection();
    assert_eq!(
        crate::weakref::js_weakref_deref(f64::from_bits(js_shadow_slot_get(1))).to_bits(),
        crate::value::TAG_UNDEFINED,
        "no allocation registry roots a dead owner"
    );
}

#[test]
fn indexed_string_candidates_survive_a_proven_copying_collection() {
    let _guard = CopyingNurseryTestGuard::new(2);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let map = js_map_alloc(32);
    js_shadow_slot_set(0, ptr_bits(map as usize));
    for i in 0..16 {
        js_map_set(map, i as f64, i as f64);
    }
    let heap_key = |name: &str| {
        let ptr = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        f64::from_bits(crate::value::STRING_TAG | ptr as u64)
    };
    for (name, value) in [("target-value", 41.0), ("", 42.0), ("short", 43.0)] {
        js_map_set(map, heap_key(name), value);
    }
    let symbol = unsafe { crate::symbol::js_symbol_new_empty() };
    js_shadow_slot_set(1, symbol.to_bits());
    js_map_set(map, symbol, 44.0);
    let stored_key_before = js_map_entry_key_at(map, 16).to_bits();
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    let moved = ptr_from_slot(0);
    assert_ne!(map, moved, "the Map must really move");
    assert_ne!(
        stored_key_before,
        js_map_entry_key_at(moved, 16).to_bits(),
        "the indexed heap key must really move"
    );
    let before = crate::map::COLD_LOOKUPS.with(std::cell::Cell::get);
    for (name, value) in [("target-value", 41.0), ("", 42.0), ("short", 43.0)] {
        let key = heap_key(name);
        assert_eq!(js_map_get(ptr_from_slot(0), key), value);
        assert_eq!(js_map_has(ptr_from_slot(0), key), 1);
    }
    for (name, value) in [("", 42.0), ("short", 43.0)] {
        let key = crate::value::JSValue::try_short_string(name.as_bytes()).unwrap();
        assert_eq!(
            js_map_get(ptr_from_slot(0), f64::from_bits(key.bits())),
            value
        );
    }
    let missing = heap_key("absent-value");
    assert_eq!(
        js_map_get(ptr_from_slot(0), missing).to_bits(),
        crate::value::TAG_UNDEFINED
    );
    assert_eq!(
        crate::map::COLD_LOOKUPS.with(std::cell::Cell::get),
        before,
        "validated indexed queries must retain their admitted path after moving"
    );
    assert_eq!(
        js_map_get(ptr_from_slot(0), f64::from_bits(js_shadow_slot_get(1))),
        44.0
    );
    let key = heap_key("target-value");
    assert_eq!(js_map_delete(ptr_from_slot(0), key), 1);
    assert_eq!(
        js_map_get(ptr_from_slot(0), key).to_bits(),
        crate::value::TAG_UNDEFINED
    );
    js_map_set(ptr_from_slot(0), key, 99.0);
    assert_eq!(js_map_get(ptr_from_slot(0), heap_key("target-value")), 99.0);
    assert_eq!(js_map_size(ptr_from_slot(0)), 20);
}
