use super::super::*;
use super::support::*;

#[test]
fn test_gc_collect_minor_runs_without_panic() {
    // Smoke test: minor GC over an arena with a mix of nursery
    // and old-gen objects must complete without panic. Real
    // correctness is checked by the broader regression suite
    // (test_json_*.ts under PERRY_GEN_GC=1).
    let _y1 = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let _y2 = crate::arena::arena_alloc_gc(32, 8, GC_TYPE_STRING);
    let _o1 = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    let _o2 = crate::arena::arena_alloc_gc_old(48, 8, GC_TYPE_ARRAY);
    let _ = gc_collect_minor();
    // Following collection runs interleave nicely (cleared marks).
    let _ = gc_collect_minor();
    let _ = gc_collect_minor();
}

#[test]
fn test_remembered_set_restores_live_old_young_after_full_gc() {
    reset_remembered_set();
    // Set up an old→young edge to populate the RS.
    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (old, fields) = unsafe { alloc_old_test_object(1) };
    unsafe {
        *fields = POINTER_TAG | young as u64;
    }
    js_write_barrier_slot(
        POINTER_TAG | old as u64,
        fields as u64,
        POINTER_TAG | young as u64,
    );
    assert_eq!(remembered_set_size(), 1);
    // Run a full collection.
    let _freed = gc_collect_inner();
    assert!(
        remembered_set_size() > 0,
        "live old-to-young edges should be re-remembered after gc_collect_inner"
    );
    let stats = verify_old_to_young_edges_covered();
    assert_eq!(stats.missing_edges, 0);
}

#[test]
fn test_clear_marks_resets_all() {
    // Allocate and mark some objects
    let ptr1 = gc_malloc(32, GC_TYPE_STRING);
    let ptr2 = gc_malloc(64, GC_TYPE_CLOSURE);

    unsafe {
        init_test_closure(ptr2);
        (*header_from_user_ptr(ptr1)).gc_flags |= GC_FLAG_MARKED;
        (*header_from_user_ptr(ptr2)).gc_flags |= GC_FLAG_MARKED;
    }

    clear_marks();

    unsafe {
        assert_eq!(
            (*header_from_user_ptr(ptr1)).gc_flags & GC_FLAG_MARKED,
            0,
            "mark should be cleared on ptr1"
        );
        assert_eq!(
            (*header_from_user_ptr(ptr2)).gc_flags & GC_FLAG_MARKED,
            0,
            "mark should be cleared on ptr2"
        );
    }
}

/// Issue #856 regression: `mark_stack_roots` performs a `setjmp`
/// into a `u64` register-snapshot buffer, and `promise.rs` does a
/// `setjmp` into an `i32` trap buffer. Both used to declare their
/// own conflicting `extern "C" fn setjmp(...)` — the Rust compiler
/// emitted `clashing_extern_declarations`, and on platforms where
/// the ABI didn't happen to round-trip the bits the behaviour was
/// UB. The fix routes both through `crate::ffi::setjmp::setjmp`
/// with a libc-matching `*mut c_int` signature; this test exists
/// to make sure the GC stack-scan path keeps running without
/// crashing now that the extern is shared.
///
/// `gc_collect_inner` invokes `mark_stack_roots`, which is the
/// real production setjmp call site. The matching promise.rs
/// trap path is exercised by `crate::ffi::setjmp::tests` and by
/// any test that drains microtasks; the regression here is
/// specifically the GC half of the pair.
#[test]
fn test_issue_856_setjmp_stack_scan_does_not_crash() {
    // A few allocations so `mark_stack_roots` actually has
    // pointers to consider; the test is about the setjmp not
    // crashing, not about a specific mark outcome.
    let _ptr1 = gc_malloc(32, GC_TYPE_STRING);
    let ptr2 = gc_malloc(48, GC_TYPE_CLOSURE);
    let _ptr3 = gc_malloc(16, GC_TYPE_BIGINT);
    unsafe {
        init_test_closure(ptr2);
    }

    // Should complete cleanly. If the shared `_setjmp` extern is
    // mis-sized, libc will scribble past the 256-byte buffer in
    // `mark_stack_roots` and corrupt this frame's stack — the
    // test would crash long before reaching the assert.
    gc_collect_inner();

    // Sanity: GC ran (count advanced). We don't assert anything
    // about WHICH allocations survived — that's covered by other
    // tests.
    let count = GC_STATS.with(|s| s.borrow().collection_count);
    assert!(count > 0, "gc_collect_inner should bump collection_count");
}
