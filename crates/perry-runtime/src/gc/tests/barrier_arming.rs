//! #7187 Phase A — lazy write-barrier arming.
//!
//! Kept in its own file rather than appended to `barrier.rs`, which is already
//! at the 2 000-line cap `scripts/check_file_size.sh` enforces.

use super::super::*;
use super::support::*;

// ── #7187 Phase A: lazy barrier arming ─────────────────────────────────────
//
// Three tests, deliberately shaped as a liveness triad rather than as "nothing
// threw". CLAUDE.md's failure mode #4 — the gate runs but its subject never did
// — is the one that has cost this repo the most (`PERRY_GC_FORCE_EVACUATE` was
// inert for every `gc()`-driven test for months, #6942/#6946). So each test
// asserts that the mechanism it covers was actually exercised:
//
//   1. the unarmed barrier really skipped (the log is empty for a real edge),
//   2. the reconstruct really ran and really recovered that edge (census
//      counters, plus the child ends up marked through it),
//   3. suppressing the reconstruct really loses the edge — the sabotage arm,
//      without which (1) and (2) would both pass on a fixture that never
//      produced an old→young edge at all.

/// The fixture the whole lever is about, and the shape of the bug it must not
/// reintroduce: a **born-old** parent (`arena_alloc_gc_old`, exactly what
/// `arena_alloc_gc` does for anything over `LARGE_OBJECT_THRESHOLD_BYTES` —
/// `batch.ts`'s 40 000-element arrays) holding a **nursery** child, stored
/// while the barrier has never been armed. `note_array_slot_layout_only`'s
/// comment records the last time this went wrong: "155 edges, all born-old
/// array→young object; the store never hit any barrier".
unsafe fn born_old_parent_with_young_child() -> (usize, *mut u64, usize) {
    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (old_obj, fields) = alloc_old_test_object(1);
    *fields = ptr_bits(young);
    js_write_barrier_slot(ptr_bits(old_obj as usize), fields as u64, ptr_bits(young));
    (old_obj as usize, fields, young)
}

#[test]
fn test_7187_unarmed_barrier_logs_nothing_and_first_snapshot_reconstructs() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    reset_barrier_arming_for_tests();

    let (_old, fields, young) = unsafe { born_old_parent_with_young_child() };
    let slot_page = crate::arena::generation_page_for_addr(fields as usize);

    // (1) The subject was live: this is a genuine old→young edge, it went
    // through the real barrier entry point, and the barrier logged NOTHING.
    assert!(
        !barrier_remembering_armed(),
        "fixture must run inside the unarmed window"
    );
    assert_eq!(
        remembered_dirty_page_count(),
        0,
        "an unarmed barrier must not maintain the remembered set — that skip \
         IS the lever, and a non-zero count here means it never engaged"
    );
    assert!(
        !old_page_dirty_for(slot_page),
        "the written slot's old page must be undirtied while unarmed"
    );
    assert_eq!(remembered_reconstruct_census().reconstructs, 0);

    // (2) The first read of the remembered set reconstructs what the barrier
    // did not log, and the collector finds the child through it.
    let valid_ptrs = build_valid_pointer_set();
    let stats = mark_remembered_set_roots(&valid_ptrs);

    let census = remembered_reconstruct_census();
    assert_eq!(
        census.reconstructs, 1,
        "the first remembered-set read must reconstruct exactly once"
    );
    assert!(
        census.recovered_old_pages >= 1,
        "the reconstruct recovered no old→young pages at all — it walked \
         nothing, or looked for the wrong thing"
    );
    assert!(
        old_page_dirty_for(slot_page),
        "the reconstruct must cover the exact page the unarmed store skipped"
    );
    assert!(
        stats.newly_marked >= 1,
        "remembered-set root marking found no young children"
    );
    assert_heap_child_marked(young as *const u8, "young child of born-old parent");

    // (3) Arming went live: the next store is logged by the barrier itself,
    // with no reconstruct involved.
    assert!(
        barrier_remembering_armed(),
        "the first remembered-set read must arm the barrier"
    );
    reset_remembered_set();
    clear_marks();
    let young2 = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    unsafe {
        *fields = ptr_bits(young2);
    }
    js_write_barrier_slot(ptr_bits(_old), fields as u64, ptr_bits(young2));
    assert!(
        old_page_dirty_for(slot_page),
        "once armed, the barrier must log the edge without a reconstruct"
    );
    assert_eq!(
        remembered_reconstruct_census().reconstructs,
        1,
        "arming must not re-run the reconstruct"
    );

    reset_remembered_set();
    clear_marks();
}

/// Sabotage arm. Same fixture, reconstruct suppressed — the edge must be LOST.
/// A version of the test above that passes with and without the reconstruct
/// would be testing nothing; this is what makes it testing something.
#[test]
fn test_7187_without_the_reconstruct_the_unarmed_edge_is_uncovered() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    reset_barrier_arming_for_tests();

    let (_old, fields, young) = unsafe { born_old_parent_with_young_child() };
    let slot_page = crate::arena::generation_page_for_addr(fields as usize);
    assert_eq!(remembered_dirty_page_count(), 0);

    // Suppress the reconstruct by marking this thread as already-reconstructed
    // WITHOUT having reconstructed — the one thing the production path can
    // never do, and precisely the mistake a future refactor could make.
    arm_barrier_for_tests();

    let valid_ptrs = build_valid_pointer_set();
    let stats = mark_remembered_set_roots(&valid_ptrs);

    assert_eq!(
        remembered_reconstruct_census().reconstructs,
        0,
        "sabotage arm must actually suppress the reconstruct"
    );
    assert!(
        !old_page_dirty_for(slot_page),
        "with the reconstruct suppressed the unarmed edge must stay uncovered \
         — if this page is dirty, something else is covering it and the \
         positive test above proves nothing"
    );
    assert_eq!(
        stats.newly_marked, 0,
        "no young child should be reachable through an empty remembered set"
    );
    unsafe {
        let child_header = header_from_user_ptr(young as *const u8);
        assert_eq!(
            (*child_header).gc_flags & GC_FLAG_MARKED,
            0,
            "the young child must be UNMARKED without the reconstruct — this \
             is the sweep-a-live-object bug the reconstruct prevents"
        );
    }

    reset_remembered_set();
    clear_marks();
}

/// End-to-end through the real collection entry point rather than
/// `mark_remembered_set_roots` directly: a minor that begins in the unarmed
/// window must still see complete old→young coverage.
#[test]
fn test_7187_minor_in_the_unarmed_window_has_complete_old_young_coverage() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    reset_barrier_arming_for_tests();

    let (_old, _fields, _young) = unsafe { born_old_parent_with_young_child() };
    assert_eq!(
        remembered_dirty_page_count(),
        0,
        "the minor must start from an EMPTY log for this to be a real test"
    );

    let _freed = gc_collect_minor();

    let census = remembered_reconstruct_census();
    assert_eq!(census.reconstructs, 1, "the minor must have reconstructed");
    assert!(census.recovered_old_pages >= 1);
    assert!(
        barrier_remembering_armed(),
        "a collection must leave the barrier armed"
    );
    let stats = verify_old_to_young_edges_covered();
    assert_eq!(
        stats.missing_edges, 0,
        "a minor started in the unarmed window left old→young edges uncovered"
    );

    reset_remembered_set();
    clear_marks();
}
