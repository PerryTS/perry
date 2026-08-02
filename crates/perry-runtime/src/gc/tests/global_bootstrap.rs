//! #7217: the lazy `globalThis` bootstrap must run in a NO-MOVE WINDOW.
//!
//! `populate_global_this_builtins` constructs an IMMORTAL object graph —
//! everything it allocates is reachable from `globalThis` for the life of the
//! thread. It constructs it by threading raw `*mut ObjectHeader` /
//! `*mut ClosureHeader` locals through several hundred installs in a dozen
//! installer modules, each of which allocates. Those locals are slots the
//! collector does not rewrite, so a *relocating* collection inside the window
//! leaves the rest of the bootstrap writing into from-space.
//!
//! This was invisible on the safepoint route (`PERRY_GC_MOVING_LOOP_POLLS=1`):
//! a back-edge poll only fires while user JS runs, and the bootstrap runs no
//! user JS. It is reachable on the allocation-point route, where the
//! bootstrap's own block allocations are the collection points — which is why
//! `test_gap_gc_spread_accessor_rooting` still SIGSEGV'd 10/10 under
//! `PERRY_GC_HEAP_LIMIT=8 PERRY_GC_INCREMENTAL=0
//! PERRY_CONSERVATIVE_STACK_SCAN=off` after three rooting fixes that were all
//! green at safepoints.
//!
//! ***BOTH HALVES ARE ASSERTED*** (CLAUDE.md's fourth way a gate cannot fail).
//! A test that only checked "no collection ran during the bootstrap" would pass
//! on a tree where nothing was ever due. The test below therefore arms ONE
//! pending collection, shows the bootstrap did not service it, and then shows
//! that the SAME armed request is serviced by ordinary allocation once the
//! window has closed. Same thread, same lever, same magnitude — the only
//! variable is whether the allocations happened inside the window.
//!
//! SCOPE. Only `populate_global_this_builtins` is covered, and deliberately so.
//! `ensure_generator_intrinsics` / `ensure_typed_array_intrinsic` build the same
//! shape of immortal graph through the same kind of raw locals and are ALSO
//! reachable lazily, ahead of the bootstrap — but a tower is three orders of
//! magnitude smaller than the bootstrap, fits inside one arena block's tail, and
//! so may reach no `gc_check_trigger` at all. A version of this file that armed a
//! collection around them PASSED with their windows deleted. Rather than ship a
//! GC-trigger change with no test that can fail without it — the exact thing
//! CLAUDE.md's knob-kill policy exists to stop — those two windows were dropped
//! and the exposure is tracked separately.

use super::super::*;
use super::support::*;

/// Run `body` on a thread that has never touched `globalThis`, so the lazy
/// bootstrap really runs instead of returning the per-thread cache hit.
/// `THREAD_GLOBAL_THIS`, the arena, `GC_STATS` and `GC_OLD_RECLAIM_PENDING` are
/// all thread-local, so the arming and the measurement stay on this thread.
fn on_a_fresh_thread(body: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(body)
        .expect("spawn bootstrap test thread")
        .join()
        .expect("bootstrap test thread panicked");
}

/// Make one collection due at the very next `gc_check_trigger()` — which
/// `arena_alloc_gc` calls every time the current block fills. This is the same
/// lever `scan_fallback.rs` uses, and it completes synchronously on the
/// allocation-point arm rather than deferring to a safepoint.
fn arm_one_pending_collection() {
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(true));
}

fn pending_collection_still_owed() -> bool {
    GC_OLD_RECLAIM_PENDING.with(std::cell::Cell::get)
}

fn clear_pending_collection() {
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(false));
    GC_SAFEPOINT_PENDING.with(|pending| pending.set(false));
    let old_in_use = crate::arena::old_gen_in_use_bytes();
    GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.set(old_in_use));
}

/// THE CONTROL. Allocate ordinary young objects — nothing rooted, nothing
/// exotic — until either the armed collection is serviced or two arena blocks
/// have been consumed without it. Returns whether it was serviced.
///
/// Two blocks is deliberately more than the bootstrap window spans (measured:
/// ~1.15 MB, i.e. just over one 1 MB block), so a `false` here means the
/// arming is inert on this thread and the subject assertion above it proved
/// nothing.
fn ordinary_allocation_services_the_armed_collection(collections_before: u64) -> bool {
    let arena_before = crate::arena::arena_total_bytes();
    for _ in 0..500_000 {
        let _ = young_leaf();
        if gc_collection_count() > collections_before {
            return true;
        }
        if crate::arena::arena_total_bytes() >= arena_before + (2 << 20) {
            return false;
        }
    }
    false
}

#[test]
fn global_this_bootstrap_runs_in_a_no_move_window() {
    on_a_fresh_thread(|| {
        // Pin the shipped pacing (#7161 flipped `PERRY_GC_MOVING_LOOP_POLLS`
        // off) so this asserts against a declared mode rather than whatever the
        // process-wide OnceLock happened to resolve to.
        let _pacing = crate::gc::policy::force_legacy_gc_pacing();
        crate::gc::ensure_gc_initialized();
        clear_pending_collection();

        arm_one_pending_collection();
        let arena_before = crate::arena::arena_total_bytes();
        let collections_before = gc_collection_count();

        // THE SUBJECT: the one-shot realm bootstrap.
        let global = crate::object::js_get_global_this();
        assert!(
            crate::value::JSValue::from_bits(global.to_bits()).is_pointer(),
            "js_get_global_this must return a real singleton, else the \
             bootstrap never ran and this test measured nothing"
        );

        let arena_after = crate::arena::arena_total_bytes();
        let collections_after = gc_collection_count();

        // LIVE SUBJECT, half 1: the window really did span a block boundary, so
        // `arena_alloc_gc` really did reach `gc_check_trigger()` inside it.
        assert!(
            arena_after >= arena_before + (1 << 20),
            "the bootstrap must consume at least one arena block for this test \
             to say anything (before={arena_before} after={arena_after})"
        );
        // THE INVARIANT: nothing collected, and therefore nothing moved, while
        // the installers held raw pointers.
        assert_eq!(
            collections_after, collections_before,
            "a collection ran inside the globalThis bootstrap — every installer \
             local (`ctor`, `proto`, `ns_obj`, …) is now a from-space address"
        );
        assert!(
            pending_collection_still_owed(),
            "the window must DEFER the request, not drop it: leaving it \
             unserviced-and-unset would disable the trigger for the rest of \
             the thread"
        );
        assert!(
            !crate::gc::gc_is_suppressed(),
            "the no-move window must close when the bootstrap returns"
        );

        // LIVE SUBJECT, half 2 — THE CONTROL. Same thread, same armed request,
        // ordinary allocation. If this does not collect, the arming was inert
        // and the assertion above is vacuous.
        assert!(
            ordinary_allocation_services_the_armed_collection(collections_after),
            "the armed collection was never serviceable on this thread, so \
             'the bootstrap did not collect' proved nothing"
        );

        clear_pending_collection();
    });
}
