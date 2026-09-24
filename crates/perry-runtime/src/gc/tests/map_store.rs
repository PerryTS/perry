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
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    let moved = (js_shadow_slot_get(0) & crate::value::POINTER_MASK) as *mut MapHeader;
    assert_ne!(map, moved, "the test must actually move its owner");
    let raw = unsafe { map_cursor_next_raw(moved, 21, epoch) }.unwrap();
    assert_eq!(raw, 0);
    assert_eq!(js_map_entry_key_at(moved, raw), 21.0);
}
