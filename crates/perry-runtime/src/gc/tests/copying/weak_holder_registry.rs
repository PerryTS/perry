//! Copied-minor weak processing scoped to the live-holder registry (#6182).
//!
//! These exercise `process_weak_targets_from_registry` on a REAL moving
//! copied-minor (`collect_minor_trace` + `assert_copied_minor_trace(..true..)`):
//! the pass walks only the registered holders — following each through
//! evacuation and dropping dead ones — and classifies weak targets with the
//! copy's O(1) page-metadata classifier instead of a full-heap valid-pointer
//! set. Correctness parity with the old whole-arena pass, plus the incidental
//! handle-band fix and the registry-currency guarantees the optimization rests
//! on.
//!
//! Note: the GC test harness clears `MUTABLE_ROOT_SCANNERS`, so the registered
//! `scan_weak_holders_roots_mut` does NOT run here — these tests prove the
//! copied-minor pass keeps holder addresses current by ITSELF (follow-forward +
//! in-place rekey), which is the robust production-and-test path.

use super::*;

fn obj_bits(obj: *mut crate::ObjectHeader) -> u64 {
    ptr_bits(obj as usize)
}

/// (1) A WeakMap with a live key and a dead key across a moving minor: the dead
/// entry is tombstoned (dropped from the enumeration) while the live entry
/// survives with BOTH its key and its (object) value repaired to their
/// evacuated addresses.
#[test]
fn test_copied_minor_weakmap_dead_key_tombstoned_live_entry_rewritten() {
    let _guard = CopyingNurseryTestGuard::new(2);

    let map = crate::weakref::js_weakmap_new();
    let live_key = crate::object::js_object_alloc(0, 0);
    // Value is an object referenced ONLY through the entry's (strong) value
    // slot, so observing its rewrite proves the live entry was evacuated.
    let live_val = crate::object::js_object_alloc(0, 0);
    js_shadow_slot_set(0, obj_bits(map));
    js_shadow_slot_set(1, obj_bits(live_key));

    {
        // Dead key: reachable only from this scope's un-rooted local.
        let dead_key = crate::object::js_object_alloc(0, 0);
        let map_v = f64::from_bits(js_shadow_slot_get(0));
        crate::weakref::js_weakmap_set(
            map_v,
            f64::from_bits(obj_bits(dead_key)),
            f64::from_bits(crate::value::TAG_TRUE),
        );
        let live_v = f64::from_bits(js_shadow_slot_get(1));
        crate::weakref::js_weakmap_set(map_v, live_v, f64::from_bits(obj_bits(live_val)));
    }

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);

    let map_after = (js_shadow_slot_get(0) & POINTER_MASK) as *const crate::ObjectHeader;
    let live_key_after = js_shadow_slot_get(1) & POINTER_MASK;

    let entries = crate::weakref::weak_collection_entries(map_after);
    assert_eq!(
        entries.len(),
        1,
        "dead-key entry must be tombstoned; the live entry must remain"
    );
    let (k, v) = entries[0];
    assert_eq!(
        k.to_bits() & POINTER_MASK,
        live_key_after,
        "live entry's key must be repaired to the key's evacuated address"
    );
    let value_after = (v.to_bits() & POINTER_MASK) as usize;
    assert_ne!(
        value_after, live_val as usize,
        "test premise: the (strongly-held) value object must actually move"
    );
    assert!(crate::arena::pointer_in_nursery(value_after));

    // Behavioral: the map still answers `has(liveKey)` after the move.
    let map_v = f64::from_bits(js_shadow_slot_get(0));
    let live_v = f64::from_bits(js_shadow_slot_get(1));
    assert_eq!(
        crate::weakref::js_weakmap_has(map_v, live_v).to_bits(),
        crate::value::TAG_TRUE
    );
}

/// (2) One WeakRef whose target dies across a moving minor derefs to
/// `undefined`; another whose target lives derefs to the target's repaired
/// (evacuated) address.
#[test]
fn test_copied_minor_weakref_dead_and_live_via_registry() {
    let _guard = CopyingNurseryTestGuard::new(3);

    let dead_target = crate::object::js_object_alloc(0, 0);
    let wr_dead = crate::weakref::js_weakref_new(f64::from_bits(obj_bits(dead_target)));
    let live_target = crate::object::js_object_alloc(0, 0);
    let wr_live = crate::weakref::js_weakref_new(f64::from_bits(obj_bits(live_target)));

    js_shadow_slot_set(0, obj_bits(wr_dead));
    js_shadow_slot_set(1, obj_bits(wr_live));
    js_shadow_slot_set(2, obj_bits(live_target)); // strong edge keeps it alive

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);

    let wr_dead_after = ptr_bits((js_shadow_slot_get(0) & POINTER_MASK) as usize);
    let wr_live_after = ptr_bits((js_shadow_slot_get(1) & POINTER_MASK) as usize);
    let live_target_after = js_shadow_slot_get(2) & POINTER_MASK;

    assert_eq!(
        crate::weakref::js_weakref_deref(f64::from_bits(wr_dead_after)).to_bits(),
        crate::value::TAG_UNDEFINED,
        "weak-only young target must be collected and its WeakRef tombstoned"
    );
    let live_deref = crate::weakref::js_weakref_deref(f64::from_bits(wr_live_after)).to_bits();
    assert_eq!(
        live_deref & POINTER_MASK,
        live_target_after,
        "live target's WeakRef slot must be repaired to the evacuated address"
    );
    assert_ne!(
        (live_deref & POINTER_MASK) as usize,
        live_target as usize,
        "test premise: the live target must actually move"
    );
}

/// (3) Band-key correctness — the incidental fix. A WeakMap keyed by a
/// POINTER_TAG value whose address lands in the handle band
/// `[0x1000, HANDLE_BAND_MAX)` (here a fabricated 0x40000, not a real heap
/// object) must NOT be tombstoned by the copied-minor pass. The old
/// `valid_ptrs`-based predicate false-tombstoned any such key on the first GC
/// (it read "not in valid_ptrs" as "dead"); the classifier reads a miss as
/// "not a collectible heap object" and keeps it.
#[test]
fn test_copied_minor_handle_band_key_not_false_tombstoned() {
    let _guard = CopyingNurseryTestGuard::new(1);

    let map = crate::weakref::js_weakmap_new();
    js_shadow_slot_set(0, obj_bits(map));

    // A fabricated handle-band key: POINTER_TAG over 0x40000 (a Proxy/fetch/http
    // id band address), which is NOT a heap allocation.
    let band_key = f64::from_bits(ptr_bits(0x40000));
    let map_v = f64::from_bits(js_shadow_slot_get(0));
    crate::weakref::js_weakmap_set(map_v, band_key, f64::from_bits(crate::value::TAG_TRUE));
    assert_eq!(
        crate::weakref::js_weakmap_has(map_v, band_key).to_bits(),
        crate::value::TAG_TRUE
    );

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);

    let map_v = f64::from_bits(js_shadow_slot_get(0));
    assert_eq!(
        crate::weakref::js_weakmap_has(map_v, band_key).to_bits(),
        crate::value::TAG_TRUE,
        "a handle-band weak key (non-heap) must NOT be false-tombstoned"
    );
    let map_after = (js_shadow_slot_get(0) & POINTER_MASK) as *const crate::ObjectHeader;
    assert_eq!(
        crate::weakref::weak_collection_entries(map_after).len(),
        1,
        "the band-key entry must survive the copied-minor pass"
    );
}

/// (4) Latch clears: a transient WeakMap whose wrapper AND entries all die is
/// pruned from the registry by a full collection, so
/// `weak_target_holders_allocated()` (registry non-empty) returns to false —
/// the transient-WeakMap copied-minor cost returns to zero (the old bool latch
/// stayed set forever).
#[test]
fn test_weak_holder_latch_clears_after_transient_weakmap_dies() {
    let _guard = GcTestIsolationGuard::new();
    assert!(
        !crate::weakref::weak_target_holders_allocated(),
        "the registry starts empty for this isolated test"
    );

    {
        // Un-rooted WeakMap + one entry: both are dead at the full trace.
        let map = crate::weakref::js_weakmap_new();
        let map_v = f64::from_bits(ptr_bits(map as usize));
        let key = crate::object::js_object_alloc(0, 0);
        crate::weakref::js_weakmap_set(
            map_v,
            f64::from_bits(ptr_bits(key as usize)),
            f64::from_bits(crate::value::TAG_TRUE),
        );
    }
    assert!(
        crate::weakref::weak_target_holders_allocated(),
        "the WeakMap entry must register a holder"
    );

    // Full (non-moving) mark-sweep → dead holders pruned via
    // `prune_dead_weak_holders` (dead_owner post-trace hook).
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));

    assert!(
        !crate::weakref::weak_target_holders_allocated(),
        "a transient WeakMap that died must return the weak-processing latch to zero"
    );
}

/// (5) Cross-cycle registry currency: a WeakMap entry survives three
/// consecutive moving minors with its holder evacuated (address changing) each
/// time; the registry tracks the moved holder so a key that dies on cycle 3 is
/// still tombstoned while a permanently-live key is retained. If the registry
/// lost track of the moved entry, the dead key's entry would never be processed
/// (enumeration would still show it).
#[test]
fn test_registry_tracks_holder_across_three_moving_minors() {
    let _guard = CopyingNurseryTestGuard::new(3);

    let map = crate::weakref::js_weakmap_new();
    let live_key = crate::object::js_object_alloc(0, 0);
    let temp_key = crate::object::js_object_alloc(0, 0);
    js_shadow_slot_set(0, obj_bits(map));
    js_shadow_slot_set(1, obj_bits(live_key));
    js_shadow_slot_set(2, obj_bits(temp_key));

    let map_v = f64::from_bits(js_shadow_slot_get(0));
    crate::weakref::js_weakmap_set(
        map_v,
        f64::from_bits(js_shadow_slot_get(1)),
        f64::from_bits(crate::value::TAG_TRUE),
    );
    crate::weakref::js_weakmap_set(
        map_v,
        f64::from_bits(js_shadow_slot_get(2)),
        f64::from_bits(crate::value::TAG_TRUE),
    );

    let mut prev_map = map as usize;
    for cycle in 0..3 {
        if cycle == 2 {
            // Drop the strong root: temp_key dies going into the third minor.
            js_shadow_slot_set(2, 0);
        }
        let trace = collect_minor_trace(GcTriggerKind::Direct);
        assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
        let map_now = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
        assert_ne!(
            map_now, prev_map,
            "the holder graph must be evacuated (addresses change) on each moving minor"
        );
        prev_map = map_now;
    }

    let map_after = (js_shadow_slot_get(0) & POINTER_MASK) as *const crate::ObjectHeader;
    let entries = crate::weakref::weak_collection_entries(map_after);
    assert_eq!(
        entries.len(),
        1,
        "the key that died on cycle 3 must be tombstoned; the live entry retained \
         (proves the registry tracked the entry through all three evacuations)"
    );
    let live_key_after = js_shadow_slot_get(1) & POINTER_MASK;
    assert_eq!(
        entries[0].0.to_bits() & POINTER_MASK,
        live_key_after,
        "the surviving entry's key must track the live key's final address"
    );
    let map_v = f64::from_bits(js_shadow_slot_get(0));
    assert_eq!(
        crate::weakref::js_weakmap_has(map_v, f64::from_bits(js_shadow_slot_get(1))).to_bits(),
        crate::value::TAG_TRUE
    );
}

extern "C" fn finreg_registry_test_callback(
    _closure: *const crate::closure::ClosureHeader,
    _held: f64,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// (6) FinalizationRegistry: a registered target that dies across a moving minor
/// enqueues its cleanup job through the registry-based pass (the #6192
/// automatic-cycle delivery must be preserved — the registry holder is the
/// FinalizationRegistry itself, which carries the callback), and a second cycle
/// must not re-enqueue it.
#[test]
fn test_copied_minor_finreg_enqueues_cleanup_via_registry() {
    let _guard = CopyingNurseryTestGuard::new(1);

    let cb = crate::closure::js_closure_alloc(finreg_registry_test_callback as *const u8, 0);
    let reg = crate::weakref::js_finreg_new(f64::from_bits(ptr_bits(cb as usize)));
    js_shadow_slot_set(0, obj_bits(reg));

    {
        // Target reachable only from this scope — dead at the first minor.
        let target = crate::object::js_object_alloc(0, 0);
        let reg_v = f64::from_bits(js_shadow_slot_get(0));
        crate::weakref::js_finreg_register(
            reg_v,
            f64::from_bits(obj_bits(target)),
            f64::from_bits(crate::value::TAG_TRUE),
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
    }
    assert_eq!(crate::weakref::pending_finalization_jobs_count(), 0);

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(
        crate::weakref::pending_finalization_jobs_count(),
        1,
        "the registry-based copied-minor pass must enqueue the cleanup job for the dead target"
    );

    let _ = gc_collect_minor();
    assert_eq!(
        crate::weakref::pending_finalization_jobs_count(),
        1,
        "a second automatic cycle must NOT re-enqueue the same record"
    );
}
