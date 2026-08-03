//! #7187 Phase B — the write barrier's "this page is already dirty" cache.
//!
//! Kept in its own file rather than appended to `barrier.rs`, which is already
//! at the 2 000-line cap `scripts/check_file_size.sh` enforces.
//!
//! Four tests, shaped so that each one **asserts its own subject was live**
//! (CLAUDE.md failure mode #4). A cache that never populated would make every
//! "nothing broke" assertion here pass while proving nothing, which is exactly
//! how `PERRY_GC_FORCE_EVACUATE` stayed inert for months (#6942/#6946). So the
//! counters are read under a forced trace guard — not `if tracing { … }`, which
//! skips the assertion entirely in the default `cargo test` run — and every
//! test states a non-zero `dirty_page_cache_hits`.
//!
//! What is under test, in one line each:
//!
//!   1. the fast path fires, and a genuinely NEW page is never swallowed by it,
//!   2. the clear invalidates it — the sabotage-sensitive one: without this the
//!      page is lost from the modbuf forever, which is a missed old→young edge,
//!   3. a real minor collection whose stores were overwhelmingly cache hits
//!      still has complete old→young coverage and does not free the child,
//!   4. the arena's per-page `dirty` stamp never drifts behind the modbuf.

use super::super::*;
use super::support::*;

/// An old parent whose field array spans at least two 4 KiB generation pages,
/// plus two field indices that land on different pages. The cache is a
/// *page* cache, so a single-page fixture could not distinguish "the fast path
/// works" from "the fast path swallows everything".
unsafe fn old_parent_spanning_two_pages() -> (usize, *mut u64, usize, usize) {
    const FIELDS: u32 = 2048; // 16 KiB of slots: ≥ 4 generation pages.
    let (old_obj, fields) = alloc_old_test_object(FIELDS);
    let first_page = crate::arena::generation_page_for_addr(fields as usize);
    let mut other = None;
    for i in 0..FIELDS as usize {
        if crate::arena::generation_page_for_addr(fields.add(i) as usize) != first_page {
            other = Some(i);
            break;
        }
    }
    let other = other.expect("test object did not span two generation pages");
    (old_obj as usize, fields, 0, other)
}

/// Store a fresh nursery pointer into `fields[index]` through the real barrier
/// entry point, and return the page that store should have dirtied.
unsafe fn barriered_young_store(parent: usize, fields: *mut u64, index: usize) -> usize {
    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let slot = fields.add(index);
    *slot = ptr_bits(young);
    js_write_barrier_slot(ptr_bits(parent), slot as u64, ptr_bits(young));
    crate::arena::generation_page_for_addr(slot as usize)
}

#[test]
fn test_7187b_repeat_marks_hit_the_cache_and_a_new_page_still_gets_recorded() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    let _trace = TestGcTraceCaptureGuard::force_enabled();
    let _ = take_write_barrier_trace_counters();

    // The barrier must be ARMED, or this test measures Phase A's unarmed skip
    // and says nothing about Phase B.
    assert!(
        barrier_remembering_armed(),
        "Phase B only exists in the armed state — an unarmed barrier never \
         reaches mark_dirty_old_page at all"
    );

    let (parent, fields, first_index, other_index) = unsafe { old_parent_spanning_two_pages() };

    const REPEATS: usize = 64;
    let mut page_a = 0usize;
    for _ in 0..REPEATS {
        page_a = unsafe { barriered_young_store(parent, fields, first_index) };
    }

    let counters = take_write_barrier_trace_counters();
    assert_eq!(
        counters.dirty_page_mark_attempts, REPEATS as u64,
        "every store must still ATTEMPT a page mark — the counter keeps meaning \
         'calls', so it stays comparable with the pre-Phase-B measurement"
    );
    assert_eq!(
        counters.dirty_page_cache_hits,
        REPEATS as u64 - 1,
        "all but the first repeat must be answered by the cache — a zero here \
         is an inert fast path, and every other assertion in this file would \
         still pass"
    );
    assert_eq!(
        counters.new_dirty_pages, 1,
        "the page is recorded exactly once, which is the point"
    );

    // …and it really is recorded, in BOTH places a recording lives.
    assert_eq!(remembered_dirty_page_count(), 1);
    assert!(
        old_page_dirty_for(page_a),
        "the arena page metadata must mirror the modbuf entry"
    );

    // A genuinely NEW page must not be swallowed. This is the completeness
    // half: the cache may only suppress a repeat, never a first recording.
    let page_b = unsafe { barriered_young_store(parent, fields, other_index) };
    assert_ne!(page_a, page_b, "fixture must produce two distinct pages");
    let counters = take_write_barrier_trace_counters();
    assert_eq!(
        counters.dirty_page_cache_hits, 0,
        "a different page must MISS the cache"
    );
    assert_eq!(counters.new_dirty_pages, 1);
    assert_eq!(remembered_dirty_page_count(), 2);
    assert!(old_page_dirty_for(page_a) && old_page_dirty_for(page_b));

    // Returning to the first page misses (one entry), re-marks, and — this is
    // the property that matters — does not lose or duplicate anything.
    let back = unsafe { barriered_young_store(parent, fields, first_index) };
    assert_eq!(back, page_a);
    let counters = take_write_barrier_trace_counters();
    assert_eq!(counters.dirty_page_cache_hits, 0);
    assert_eq!(counters.new_dirty_pages, 0, "page A was already recorded");
    assert_eq!(remembered_dirty_page_count(), 2);

    reset_remembered_set();
    clear_marks();
}

#[test]
fn test_7187b_clearing_the_remembered_set_invalidates_the_cache() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    let _trace = TestGcTraceCaptureGuard::force_enabled();
    let _ = take_write_barrier_trace_counters();

    let (parent, fields, first_index, _other) = unsafe { old_parent_spanning_two_pages() };
    let page = unsafe { barriered_young_store(parent, fields, first_index) };
    // Subject live: the cache is populated, so there is something to invalidate.
    let _ = unsafe { barriered_young_store(parent, fields, first_index) };
    assert!(
        take_write_barrier_trace_counters().dirty_page_cache_hits > 0,
        "the cache never populated — this test's subject does not exist"
    );
    assert!(!crate::gc::dirty_page_cache::is_empty_for_tests());
    assert_eq!(remembered_dirty_page_count(), 1);

    remembered_set_clear();

    assert_eq!(remembered_dirty_page_count(), 0);
    assert!(
        !old_page_dirty_for(page),
        "the clear must un-stamp the arena page metadata too"
    );
    assert!(
        crate::gc::dirty_page_cache::is_empty_for_tests(),
        "the clear removed the page from the modbuf but left the cache claiming \
         it is recorded — the next store to that page would be dropped and the \
         old→young edge lost for good"
    );

    // The consequence, stated as behaviour rather than as internal state: after
    // a clear, a store to the SAME page must be recorded again.
    let _ = take_write_barrier_trace_counters();
    let again = unsafe { barriered_young_store(parent, fields, first_index) };
    assert_eq!(again, page);
    let counters = take_write_barrier_trace_counters();
    assert_eq!(
        counters.dirty_page_cache_hits, 0,
        "the first store after a clear must not be answered from the cache"
    );
    assert_eq!(counters.new_dirty_pages, 1);
    assert_eq!(remembered_dirty_page_count(), 1);
    assert!(old_page_dirty_for(page));

    reset_remembered_set();
    clear_marks();
}

#[test]
fn test_7187b_minor_after_cache_heavy_stores_keeps_the_young_child_alive() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    let _trace = TestGcTraceCaptureGuard::force_enabled();
    let _ = take_write_barrier_trace_counters();

    let (parent, fields, first_index, _other) = unsafe { old_parent_spanning_two_pages() };
    let slot = unsafe { fields.add(first_index) };
    let page = crate::arena::generation_page_for_addr(slot as usize);

    // Hammer the same slot so that the LAST surviving child's edge is recorded
    // through a run that is overwhelmingly cache hits. If the cache could lose
    // the page, this is the shape that loses it.
    let mut young = 0usize;
    for _ in 0..256 {
        young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
        unsafe {
            *slot = ptr_bits(young);
        }
        js_write_barrier_slot(ptr_bits(parent), slot as u64, ptr_bits(young));
    }
    let counters = take_write_barrier_trace_counters();
    assert!(
        counters.dirty_page_cache_hits >= 255,
        "the run must have been served by the cache (hits={}) or it is not \
         exercising Phase B",
        counters.dirty_page_cache_hits
    );
    assert!(old_page_dirty_for(page));

    // The old parent is a live root for the purposes of this check.
    let old_header = unsafe { header_from_user_ptr(parent as *const u8) };
    unsafe {
        (*old_header).gc_flags |= GC_FLAG_MARKED;
    }
    let stats = verify_old_to_young_edges_covered();
    assert_eq!(
        stats.missing_edges, 0,
        "a cache-served store run left an old→young edge uncovered — that is a \
         swept-live-object bug, not a slow program"
    );
    assert!(stats.checked_old_to_young_edges >= 1);

    // And the collector really finds the child through the remembered set.
    let valid_ptrs = build_valid_pointer_set();
    let marked = mark_remembered_set_roots(&valid_ptrs);
    assert!(
        marked.newly_marked >= 1,
        "remembered-set root marking found no young child"
    );
    unsafe {
        let child_header = header_from_user_ptr(young as *const u8);
        assert_ne!(
            (*child_header).gc_flags & GC_FLAG_MARKED,
            0,
            "the surviving young child must be marked through the dirty page"
        );
        (*old_header).gc_flags &= !GC_FLAG_MARKED;
    }

    reset_remembered_set();
    clear_marks();
}

#[test]
fn test_7187b_cache_never_outruns_the_arena_dirty_stamp() {
    // The cache remembers "recorded", and a recording lives in TWO places: the
    // modbuf and `OldPageMeta.dirty`. `old_page_mark_dirty` silently does
    // nothing for a page with no metadata entry, so caching on the strength of
    // the modbuf alone would let the metadata drift behind. Assert the pairing
    // directly: every page the cache is willing to answer for is stamped.
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    let _trace = TestGcTraceCaptureGuard::force_enabled();
    let _ = take_write_barrier_trace_counters();

    let (parent, fields, first_index, other_index) = unsafe { old_parent_spanning_two_pages() };
    for index in [first_index, other_index, first_index, other_index] {
        let page = unsafe { barriered_young_store(parent, fields, index) };
        assert!(
            old_page_dirty_for(page),
            "page {page} answered by the barrier is not stamped dirty"
        );
        assert!(
            !crate::gc::dirty_page_cache::is_empty_for_tests(),
            "a completed recording must populate the cache"
        );
    }
    assert_eq!(remembered_dirty_page_count(), 2);

    reset_remembered_set();
    clear_marks();
}
