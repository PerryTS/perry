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
//! pending collection, shows that no collection had run when the window
//! closed (read at the window's exit, before the check it owes), and then shows
//! that the SAME armed request was serviced by the check the window turned
//! away and ran as it closed. Same thread, same lever, same magnitude — the
//! only variable is whether the window was still open.
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
    GC_OLD_RECLAIM_PENDING.with(TriggerInput::get)
}

fn clear_pending_collection() {
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(false));
    crate::gc::set_safepoint_pending(false);
    let old_in_use = crate::arena::old_gen_in_use_bytes();
    GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.set(old_in_use));
}

/// Placement changes can make the bootstrap fit inside the current block.
/// Start it at a nearly full block so its first allocation window must reach
/// the slow allocation path, independently of the bootstrap's total footprint.
fn prime_current_block_for_bootstrap() {
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    for _ in 0..100_000 {
        let _ = young_leaf();
        let state = crate::arena::js_inline_arena_state();
        let remaining = unsafe { (*state).size.saturating_sub((*state).offset) };
        if remaining < 256 {
            return;
        }
    }
    panic!("bootstrap fixture could not position the current block near its end");
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
        prime_current_block_for_bootstrap();

        arm_one_pending_collection();
        let arena_before = crate::arena::arena_total_bytes();
        let collections_before = gc_collection_count();
        let exit_probe = &crate::gc::policy::window_exit_probe::COLLECTIONS_AT_WINDOW_EXIT;
        exit_probe.with(|c| c.set(None));

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
        // the installers held raw pointers. Read as the bootstrap's window
        // closed, before the check it owes ran.
        assert_eq!(
            exit_probe.with(std::cell::Cell::get),
            Some(collections_before),
            "a collection ran inside the globalThis bootstrap — every installer \
             local (`ctor`, `proto`, `ns_obj`, …) is now a from-space address"
        );
        // LIVE SUBJECT, half 2: the window DEFERRED the request, it did not
        // drop it. The trigger check it turned away ran as it closed and
        // serviced the armed collection; dropping it would disable the trigger
        // until the next block boundary.
        assert_eq!(
            collections_after,
            collections_before + 1,
            "the check the window owed must run as it closes"
        );
        assert!(
            !pending_collection_still_owed(),
            "the armed collection must be serviced at the window's exit"
        );
        assert!(
            !crate::gc::gc_is_suppressed(),
            "the no-move window must close when the bootstrap returns"
        );

        clear_pending_collection();
    });
}
