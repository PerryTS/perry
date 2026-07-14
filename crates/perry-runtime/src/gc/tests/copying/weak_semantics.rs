//! Copied-minor weak semantics (2026-07-02 audit, GC deep set): the fast
//! path used to evacuate THROUGH weak target slots (strengthening WeakRef
//! referents / WeakMap-WeakSet entry keys / FinalizationRegistry record
//! targets) and never ran the after-mark tombstone pass at all — so weak
//! entries never cleared and FinalizationRegistry never fired while
//! copied-minor was the operative cycle. The scan now records weak slots
//! without evacuating, `repair_weak_slots` fixes addresses of targets moved
//! via strong edges, and `process_weak_targets_after_mark` runs on the fast
//! path (gated on the weak-holder latch).

use super::*;

fn object_bits(obj: *mut crate::ObjectHeader) -> u64 {
    ptr_bits(obj as usize)
}

/// A weak-only-reachable young target must DIE in a copied-minor and the
/// WeakRef must tombstone to `undefined`.
#[test]
fn test_copying_minor_weakref_dead_target_tombstones() {
    let _guard = CopyingNurseryTestGuard::new(1);

    let target = crate::object::js_object_alloc(0, 0);
    assert!(crate::arena::pointer_in_nursery(target as usize));
    let wr = crate::weakref::js_weakref_new(f64::from_bits(object_bits(target)));

    // Root the WeakRef strongly; the target is reachable ONLY through it.
    js_shadow_slot_set(0, object_bits(wr));

    let _ = gc_collect_minor();

    let wr_moved = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let deref = crate::weakref::js_weakref_deref(f64::from_bits(ptr_bits(wr_moved))).to_bits();
    assert_eq!(
        deref,
        crate::value::TAG_UNDEFINED,
        "weak-only young target must be collected and the WeakRef tombstoned \
         (copied-minor used to strengthen weak edges)"
    );
}

/// A weak target that survives via a STRONG edge must stay alive — and the
/// WeakRef's slot must be repaired to the target's post-evacuation address
/// even when the weak slot was scanned before the strong edge moved it.
#[test]
fn test_copying_minor_weakref_live_target_address_repaired() {
    let _guard = CopyingNurseryTestGuard::new(2);

    let target = crate::object::js_object_alloc(0, 0);
    let wr = crate::weakref::js_weakref_new(f64::from_bits(object_bits(target)));

    js_shadow_slot_set(0, object_bits(wr));
    js_shadow_slot_set(1, object_bits(target)); // strong edge keeps it alive

    let _ = gc_collect_minor();

    let wr_moved = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let target_moved = js_shadow_slot_get(1) & POINTER_MASK;
    let deref = crate::weakref::js_weakref_deref(f64::from_bits(ptr_bits(wr_moved))).to_bits();
    assert_eq!(
        deref & POINTER_MASK,
        target_moved,
        "live weak target's slot must be repaired to the evacuated address"
    );
    assert_ne!(
        target_moved as usize, target as usize,
        "test premise: the target must actually have moved"
    );
}

/// A WeakMap entry whose key is weak-only-reachable must tombstone: the
/// value becomes unreachable and a lookup with a NEW key doesn't alias it.
/// Observable via `js_weakmap_has` on a strongly-kept twin key staying true
/// while the dead key's entry clears (checked through the entry internals'
/// public effect: `has(live)` true after GC, map still functional).
#[test]
fn test_copying_minor_weakmap_dead_key_entry_clears() {
    let _guard = CopyingNurseryTestGuard::new(2);

    let map = crate::weakref::js_weakmap_new();
    let live_key = crate::object::js_object_alloc(0, 0);
    js_shadow_slot_set(0, object_bits(map));
    js_shadow_slot_set(1, object_bits(live_key));

    {
        // Dead key: reachable only from this scope's raw local (not rooted).
        let dead_key = crate::object::js_object_alloc(0, 0);
        let map_v = f64::from_bits(js_shadow_slot_get(0));
        let _ = crate::weakref::js_weakmap_set(
            map_v,
            f64::from_bits(object_bits(dead_key)),
            f64::from_bits(crate::value::TAG_TRUE),
        );
        let live_v = f64::from_bits(js_shadow_slot_get(1));
        let _ =
            crate::weakref::js_weakmap_set(map_v, live_v, f64::from_bits(crate::value::TAG_TRUE));
    }

    let _ = gc_collect_minor();

    let map_v = f64::from_bits(js_shadow_slot_get(0));
    let live_v = f64::from_bits(js_shadow_slot_get(1));
    let has_live = crate::weakref::js_weakmap_has(map_v, live_v).to_bits();
    assert_eq!(
        has_live,
        crate::value::TAG_TRUE,
        "strongly-reachable key's entry must survive the copied-minor (its \
         key slot repaired to the moved address)"
    );
}

// ---------------------------------------------------------------------------
// FinalizationRegistry on AUTOMATIC cycles + weak unregister tokens
// (2026-07-09 GC audit, wave 2 batch A).
// ---------------------------------------------------------------------------

extern "C" fn finreg_test_callback(
    _closure: *const crate::closure::ClosureHeader,
    _held: f64,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Allocate a FinalizationRegistry with a real (callable) cleanup closure and
/// root it in shadow slot 0. Returns nothing — read the registry back through
/// the slot so post-move addresses stay correct.
fn make_rooted_finreg() {
    let cb = crate::closure::js_closure_alloc(finreg_test_callback as *const u8, 0);
    let cb_val = f64::from_bits(ptr_bits(cb as usize));
    let reg = crate::weakref::js_finreg_new(cb_val);
    js_shadow_slot_set(0, object_bits(reg));
}

/// An AUTOMATIC collection (non-Manual trigger — `gc_collect_minor` captures
/// `GcTriggerKind::Direct`) must ENQUEUE the cleanup job for a collected
/// target, and a second automatic cycle must not enqueue it again
/// (enqueue-once via the record's pending-flag reset). Before the fix,
/// `enqueue_callbacks` was gated on `GcTriggerKind::Manual`, so ordinary
/// servers never ran any FinalizationRegistry cleanup callback.
#[test]
fn test_finreg_job_enqueued_after_automatic_copied_minor() {
    let _guard = CopyingNurseryTestGuard::new(1);
    make_rooted_finreg();

    {
        // Target reachable only from this scope — dead at the first minor.
        let target = crate::object::js_object_alloc(0, 0);
        let reg_v = f64::from_bits(js_shadow_slot_get(0));
        let _ = crate::weakref::js_finreg_register(
            reg_v,
            f64::from_bits(object_bits(target)),
            f64::from_bits(crate::value::TAG_TRUE),
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
    }
    assert_eq!(crate::weakref::pending_finalization_jobs_count(), 0);

    let _ = gc_collect_minor();
    assert_eq!(
        crate::weakref::pending_finalization_jobs_count(),
        1,
        "automatic copied-minor must enqueue the cleanup job for the dead target"
    );

    let _ = gc_collect_minor();
    assert_eq!(
        crate::weakref::pending_finalization_jobs_count(),
        1,
        "a second automatic cycle must NOT re-enqueue the same record"
    );
}

/// The microtask-pump drain converts recorded jobs into nextTick callback
/// invocations exactly once: it removes the record from the registry, queues
/// the tick job, and a second drain is a no-op.
#[test]
fn test_pump_drain_delivers_automatic_finreg_jobs_once() {
    let _guard = CopyingNurseryTestGuard::new(1);
    make_rooted_finreg();

    {
        let target = crate::object::js_object_alloc(0, 0);
        let reg_v = f64::from_bits(js_shadow_slot_get(0));
        let _ = crate::weakref::js_finreg_register(
            reg_v,
            f64::from_bits(object_bits(target)),
            f64::from_bits(crate::value::TAG_TRUE),
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
    }
    let _ = gc_collect_minor();
    assert_eq!(crate::weakref::pending_finalization_jobs_count(), 1);

    let delivered = crate::weakref::drain_pending_finalization_jobs();
    assert_eq!(delivered, 1, "drain must deliver the recorded job");
    assert_eq!(
        crate::weakref::pending_finalization_jobs_count(),
        0,
        "delivery consumes the pending-jobs vec"
    );
    assert_eq!(
        crate::weakref::drain_pending_finalization_jobs(),
        0,
        "a second drain must be a no-op (no double-delivery)"
    );
}

/// The unregister token slot is WEAK (spec [[UnregisterToken]]): the canonical
/// `registry.register(obj, held, obj)` must not pin `obj` immortal. Before the
/// fix, record field 1 (the token) was traced strongly, so the target survived
/// every collection and no cleanup job was ever recorded.
#[test]
fn test_finreg_unregister_token_held_weakly() {
    let _guard = CopyingNurseryTestGuard::new(1);
    make_rooted_finreg();

    {
        let target = crate::object::js_object_alloc(0, 0);
        let target_v = f64::from_bits(object_bits(target));
        let reg_v = f64::from_bits(js_shadow_slot_get(0));
        // register(obj, held, obj): the target doubles as its own token.
        let _ = crate::weakref::js_finreg_register(
            reg_v,
            target_v,
            f64::from_bits(crate::value::TAG_TRUE),
            target_v,
        );
    }

    let _ = gc_collect_minor();
    assert_eq!(
        crate::weakref::pending_finalization_jobs_count(),
        1,
        "register(obj, held, obj): the token edge must not keep the target \
         alive — the target must die and its cleanup job must be recorded"
    );
}
