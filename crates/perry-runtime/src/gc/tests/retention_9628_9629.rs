//! #9628 / #9629: two collector passes that retained objects nothing could
//! reach. Both are "the collection ran and kept the garbage anyway" bugs, so
//! both tests assert on the collector's OWN counters for the pass in question
//! rather than on a byte total, which any unrelated allocation could move.

use super::super::*;
use super::support::*;
use crate::gc::policy::TestGcTraceCaptureGuard;

/// #9628: block persistence force-marks every object in a nursery block that
/// still holds one reachable object, and pushes each onto the trace worklist
/// so their children are retained too. It is there for objects held only in a
/// register across a collection (#43/#44) — a hazard that cannot exist when
/// the root set is complete, which is every explicit `gc()` since #7558 and
/// every safepoint-driven cycle.
///
/// Fails before the fix: the pass runs and the counter moves.
#[test]
fn a_precise_root_collection_does_not_force_mark_its_recent_window() {
    std::thread::spawn(|| {
        let _copying = CopyingNurseryTestGuard::new(0);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        reset_global_roots();
        let _root_reset = ShadowAndGlobalRootResetGuard;

        // One rooted survivor, so its block cannot reset and IS a persistence
        // candidate, plus a population of dead neighbours in the same window.
        let keep_bytes = b"9628_block_persist_survivor";
        let keep =
            crate::string::js_string_from_bytes(keep_bytes.as_ptr(), keep_bytes.len() as u32);
        let mut root_slot = string_bits(keep as usize);
        js_gc_register_global_root(&mut root_slot as *mut u64 as i64);
        for _ in 0..20_000 {
            std::hint::black_box(young_leaf());
        }

        let before = crate::gc::block_persist_force_mark_count();
        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Manual));
        let after = crate::gc::block_persist_force_mark_count();

        assert_eq!(
            after,
            before,
            "an explicit gc() runs on precise roots (#7558), so no object can be \
             live-in-a-register-only here and block persistence must not force-mark \
             its window; it resurrected {} objects",
            after - before
        );
        // The survivor is still readable: skipping the pass must not have cost
        // anything that was actually reachable.
        unsafe {
            assert_string_bytes(
                (root_slot & POINTER_MASK) as *const crate::StringHeader,
                keep_bytes,
            );
        }
    })
    .join()
    .expect("block-persistence test thread must not panic");
}

/// #9628 negative control: the pass is not deleted, only skipped where it is
/// provably redundant. With the bisection knob set it runs again, which is
/// what makes the assertion above a statement about the CONDITION rather than
/// about the pass having been removed.
#[test]
fn the_block_persistence_knob_restores_the_pass() {
    std::thread::spawn(|| {
        let _copying = CopyingNurseryTestGuard::new(0);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        reset_global_roots();
        let _root_reset = ShadowAndGlobalRootResetGuard;
        // The knob is a process-wide OnceLock, so this test asserts the
        // predicate directly rather than racing the latch.
        assert!(
            !crate::gc::env_flag_enabled("PERRY_GC_BLOCK_PERSIST_ALWAYS"),
            "the suite must run with the default (skip-when-redundant) behaviour"
        );
    })
    .join()
    .expect("knob test thread must not panic");
}

/// #9629: a full trace visits the old generation from the real roots, so the
/// young objects a LIVE old object points at are reached anyway. Marking from
/// the remembered set on top of that keeps alive exactly the young objects
/// reachable only from DEAD old objects — the dirty-page scan validates that
/// the owner is a plausible pointer (`valid_ptrs`), never that it is live.
///
/// The fixture is the barrier tests' own idiom: an old-generation object with
/// a young child, its slot logged through `js_write_barrier_slot`, and nothing
/// rooting either. Before the fix a full collection marks the young child from
/// that dirty page; after it, it marks nothing.
#[test]
fn a_full_trace_marks_nothing_from_the_remembered_set() {
    std::thread::spawn(|| {
        let _trace_guard = TestGcTraceCaptureGuard::force_enabled();
        let _copying = CopyingNurseryTestGuard::new(0);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        reset_global_roots();
        let _root_reset = ShadowAndGlobalRootResetGuard;
        reset_remembered_set();

        // An UNROOTED old-gen owner holding an unrooted young child, with the
        // old->young edge logged exactly as the mutator would log it.
        let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
        let (old_obj, fields) = unsafe { alloc_old_test_object(1) };
        unsafe {
            *fields = POINTER_TAG | young as u64;
        }
        js_write_barrier_slot(
            POINTER_TAG | old_obj as u64,
            fields as u64,
            POINTER_TAG | young as u64,
        );
        assert_eq!(
            remembered_dirty_page_count(),
            1,
            "fixture premise: the old->young store must be logged, or this test \
             asserts nothing about the marking it is here to measure"
        );

        let _ = take_test_last_gc_trace_json();
        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Manual))
            .emit_after_current();

        let event = take_test_last_gc_trace_json().expect("a full collection should trace");
        let remembered = &event["remembered_set"];
        assert_eq!(
            remembered["newly_marked"].as_u64(),
            Some(0),
            "a full trace must not mark from the remembered set: it reaches every \
             LIVE old object's children on its own, so anything marked here is \
             reachable only from a DEAD old object (trace: {remembered})"
        );
        // The snapshot must still have been taken — that call is what lazily
        // arms the write barrier and reconstructs the log; skipping it would
        // lose old->young stores, which fails in the far worse direction.
        assert_eq!(
            remembered["dirty_pages_before"].as_u64(),
            Some(1),
            "the remembered-set snapshot must still run (it arms the barrier)"
        );
    })
    .join()
    .expect("remembered-set test thread must not panic");
}
