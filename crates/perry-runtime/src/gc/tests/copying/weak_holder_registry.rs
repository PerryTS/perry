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

/// Full/fallback weak processing must scale with registered weak holders, not
/// with unrelated arena population. One live WeakRef is held constant while
/// the second collection sees a much larger heap.
#[test]
fn test_full_weak_processing_work_is_independent_of_unrelated_heap_size() {
    let _guard = CopyingNurseryTestGuard::new(1_001);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::weakref::test_support::clear_weak_holders();

    let target = crate::object::js_object_alloc(0, 0);
    let weak_ref = crate::weakref::js_weakref_new(f64::from_bits(obj_bits(target)));
    js_shadow_slot_set(0, obj_bits(weak_ref));

    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    let small_heap_work = crate::weakref::test_support::full_weak_processing_work_units();
    assert_eq!(small_heap_work, 1, "exactly one holder was registered");

    for slot in 1..=1_000 {
        let unrelated = crate::object::js_object_alloc(0, 0);
        js_shadow_slot_set(slot, obj_bits(unrelated));
    }
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    let large_heap_work = crate::weakref::test_support::full_weak_processing_work_units();
    assert_eq!(
        large_heap_work, small_heap_work,
        "one-holder weak work grew with unrelated heap: small={small_heap_work}, large={large_heap_work}"
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

/// Build one holder of every kind, move all four through a copied minor, then
/// drop their weak targets and age the graph out of block persistence. Also
/// adds one dead holder and one stale registry address so the following
/// full/fallback pass must classify both before dereferencing.
fn prepare_moved_full_path_weak_holders() {
    crate::weakref::test_support::clear_weak_holders();
    assert_eq!(crate::weakref::pending_finalization_jobs_count(), 0);

    let weak_target = crate::object::js_object_alloc(0, 0);
    let weak_ref = crate::weakref::js_weakref_new(f64::from_bits(obj_bits(weak_target)));
    js_shadow_slot_set(0, obj_bits(weak_ref));
    js_shadow_slot_set(4, obj_bits(weak_target));

    let map = crate::weakref::js_weakmap_new();
    let map_key = crate::object::js_object_alloc(0, 0);
    js_shadow_slot_set(1, obj_bits(map));
    js_shadow_slot_set(5, obj_bits(map_key));
    crate::weakref::js_weakmap_set(
        f64::from_bits(js_shadow_slot_get(1)),
        f64::from_bits(js_shadow_slot_get(5)),
        f64::from_bits(crate::value::TAG_TRUE),
    );

    let set = crate::weakref::js_weakset_new();
    let set_key = crate::object::js_object_alloc(0, 0);
    js_shadow_slot_set(2, obj_bits(set));
    js_shadow_slot_set(6, obj_bits(set_key));
    crate::weakref::js_weakset_add(
        f64::from_bits(js_shadow_slot_get(2)),
        f64::from_bits(js_shadow_slot_get(6)),
    );

    let callback = crate::closure::js_closure_alloc(finreg_registry_test_callback as *const u8, 0);
    let registry = crate::weakref::js_finreg_new(f64::from_bits(ptr_bits(callback as usize)));
    let finreg_target = crate::object::js_object_alloc(0, 0);
    js_shadow_slot_set(3, obj_bits(registry));
    js_shadow_slot_set(7, obj_bits(finreg_target));
    crate::weakref::js_finreg_register(
        f64::from_bits(js_shadow_slot_get(3)),
        f64::from_bits(js_shadow_slot_get(7)),
        f64::from_bits(crate::value::TAG_TRUE),
        f64::from_bits(crate::value::TAG_UNDEFINED),
    );

    let before_move = crate::weakref::test_support::weak_holder_addresses();
    assert_eq!(
        before_move.len(),
        4,
        "one holder of every kind is registered"
    );
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    let after_move = crate::weakref::test_support::weak_holder_addresses();
    assert_eq!(after_move.len(), 4);
    assert!(
        before_move.iter().all(|addr| !after_move.contains(addr)),
        "every registered holder must be rekeyed after evacuation"
    );

    for slot in 4..8 {
        js_shadow_slot_set(slot, 0);
    }

    // A dead holder must be pruned without dispatch. The fabricated stale
    // address must be rejected by ValidPointerSet before any header read.
    let dead_target = crate::object::js_object_alloc(0, 0);
    let _dead_holder = crate::weakref::js_weakref_new(f64::from_bits(obj_bits(dead_target)));
    crate::weakref::test_support::register_weak_holder_address(0x1234_5678);
    assert_eq!(
        crate::weakref::test_support::weak_holder_addresses().len(),
        6
    );

    let aged_from = crate::arena::general_block_count();
    while crate::arena::general_block_count().saturating_sub(aged_from) < 7 {
        for _ in 0..64 {
            let _ = crate::arena::arena_alloc_gc(4096, 8, GC_TYPE_STRING);
        }
    }
}

fn assert_full_path_weak_results() {
    assert_eq!(
        crate::weakref::js_weakref_deref(f64::from_bits(js_shadow_slot_get(0))).to_bits(),
        crate::value::TAG_UNDEFINED,
        "WeakRef target must be tombstoned"
    );
    for slot in [1, 2] {
        let collection = (js_shadow_slot_get(slot) & POINTER_MASK) as *const crate::ObjectHeader;
        assert!(
            crate::weakref::weak_collection_entries(collection).is_empty(),
            "WeakMap/WeakSet dead-key entry must be tombstoned"
        );
    }
    assert_eq!(
        crate::weakref::pending_finalization_jobs_count(),
        1,
        "FinalizationRegistry target must enqueue exactly one cleanup job"
    );
    assert_eq!(
        crate::weakref::test_support::weak_holder_addresses().len(),
        4,
        "the dead holder and stale address must be pruned; live holders remain"
    );
}

#[test]
fn test_full_registry_path_handles_all_weak_kinds_moved_dead_and_stale_holders() {
    let _guard = CopyingNurseryTestGuard::new(8);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    prepare_moved_full_path_weak_holders();

    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    assert_full_path_weak_results();
}

#[test]
fn test_fallback_registry_path_handles_all_weak_kinds_moved_dead_and_stale_holders() {
    let _guard = CopyingNurseryTestGuard::new(8);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    prepare_moved_full_path_weak_holders();

    let _barrier_guard = GeneratedWriteBarrierTestGuard::inactive();
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(
        &trace,
        false,
        CopiedMinorFallbackReason::BarriersInactive,
        false,
    );
    assert_full_path_weak_results();
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

// ---------------------------------------------------------------------------
// Promoted-key generational-invariant guards.
//
// A copied minor does NOT mark the old generation (old objects are black
// leaves, not re-traced), so a weak target that survived enough minors to be
// PROMOTED to old-gen is live-but-unmarked during a copied minor. Judging its
// deadness by the mark bit would silently drop a live entry / deref / target.
// Each of these FAILS against a mark-only copied-minor predicate and PASSES
// with the `pointer_in_nursery` guard (replicating `minor_only`).
// ---------------------------------------------------------------------------

/// Drive a rooted young object to old-gen via `GC_COPY_PROMOTION_SURVIVALS`
/// (4) survivals, then return its (promoted) address. Leaves it rooted in
/// `slot`.
fn promote_rooted_to_old(slot: u32) -> usize {
    for _ in 0..4 {
        let _ = gc_collect_minor();
    }
    let addr = (js_shadow_slot_get(slot) & POINTER_MASK) as usize;
    assert!(
        crate::arena::pointer_in_old_gen(addr),
        "test premise: the object must be promoted to old-gen after 4 survivals"
    );
    addr
}

/// A WeakMap key promoted to old-gen (still strongly held) whose entry is a
/// young object must NOT be tombstoned by a further copied minor. Pre-fix
/// (`classify → !header_is_live`) the unmarked old key was judged dead and the
/// entry silently dropped from the map.
#[test]
fn test_copied_minor_promoted_weakmap_key_not_tombstoned() {
    let _guard = CopyingNurseryTestGuard::new(2);

    let key = crate::object::js_object_alloc(0, 0);
    js_shadow_slot_set(1, obj_bits(key));
    let key_old = promote_rooted_to_old(1);

    // Fresh (young) WeakMap + entry keyed by the promoted old-gen key.
    let map = crate::weakref::js_weakmap_new();
    js_shadow_slot_set(0, obj_bits(map));
    let map_v = f64::from_bits(js_shadow_slot_get(0));
    crate::weakref::js_weakmap_set(
        map_v,
        f64::from_bits(ptr_bits(key_old)),
        f64::from_bits(crate::value::TAG_TRUE),
    );

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);

    let key_now = (js_shadow_slot_get(1) & POINTER_MASK) as usize;
    let map_v = f64::from_bits(js_shadow_slot_get(0));
    assert_eq!(
        crate::weakref::js_weakmap_has(map_v, f64::from_bits(ptr_bits(key_now))).to_bits(),
        crate::value::TAG_TRUE,
        "a live WeakMap key PROMOTED to old-gen must NOT be tombstoned by a copied minor \
         (a minor does not mark old-gen)"
    );
    let map_after = (js_shadow_slot_get(0) & POINTER_MASK) as *const crate::ObjectHeader;
    assert_eq!(
        crate::weakref::weak_collection_entries(map_after).len(),
        1,
        "the promoted-key entry must survive"
    );
}

/// A WeakRef whose target is promoted to old-gen and still live must deref to
/// the target (not `undefined`) after a copied minor.
#[test]
fn test_copied_minor_weakref_promoted_target_survives() {
    let _guard = CopyingNurseryTestGuard::new(2);

    let target = crate::object::js_object_alloc(0, 0);
    js_shadow_slot_set(1, obj_bits(target));
    let target_old = promote_rooted_to_old(1);

    let wr = crate::weakref::js_weakref_new(f64::from_bits(ptr_bits(target_old)));
    js_shadow_slot_set(0, obj_bits(wr));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);

    let wr_after = ptr_bits((js_shadow_slot_get(0) & POINTER_MASK) as usize);
    let target_now = js_shadow_slot_get(1) & POINTER_MASK;
    let deref = crate::weakref::js_weakref_deref(f64::from_bits(wr_after)).to_bits();
    assert_ne!(
        deref,
        crate::value::TAG_UNDEFINED,
        "a live WeakRef target promoted to old-gen must NOT be tombstoned by a copied minor"
    );
    assert_eq!(
        deref & POINTER_MASK,
        target_now,
        "deref must return the (old-gen) target"
    );
}

/// A FinalizationRegistry target promoted to old-gen and still live must NOT be
/// reported collected by a copied minor (no cleanup job enqueued). Pre-fix the
/// unmarked old target was judged collected and its callback prematurely queued.
#[test]
fn test_copied_minor_finreg_promoted_live_target_not_collected() {
    let _guard = CopyingNurseryTestGuard::new(2);

    let target = crate::object::js_object_alloc(0, 0);
    js_shadow_slot_set(1, obj_bits(target));
    let target_old = promote_rooted_to_old(1);

    let cb = crate::closure::js_closure_alloc(finreg_registry_test_callback as *const u8, 0);
    let reg = crate::weakref::js_finreg_new(f64::from_bits(ptr_bits(cb as usize)));
    js_shadow_slot_set(0, obj_bits(reg));
    let reg_v = f64::from_bits(js_shadow_slot_get(0));
    crate::weakref::js_finreg_register(
        reg_v,
        f64::from_bits(ptr_bits(target_old)),
        f64::from_bits(crate::value::TAG_TRUE),
        f64::from_bits(crate::value::TAG_UNDEFINED),
    );
    assert_eq!(crate::weakref::pending_finalization_jobs_count(), 0);

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);

    assert_eq!(
        crate::weakref::pending_finalization_jobs_count(),
        0,
        "a FinalizationRegistry target promoted to old-gen and still live must NOT be \
         reported collected by a copied minor"
    );
}
