//! #7251: the two lazy intrinsic-tower builders need #7217's NO-MOVE WINDOW,
//! and #7217 itself could not gate them.
//!
//! `object::global_this::generator::build_generator_tower` and
//! `object::global_this::typed_array::ensure_typed_array_intrinsic` have the
//! identical shape #7217 fixed for `populate_global_this_builtins`: each
//! builds an IMMORTAL object graph (hanging off a process-global intrinsic
//! slot for the life of the thread) by threading raw `*mut ObjectHeader` /
//! `*mut ClosureHeader` locals across a dozen-plus allocating installs. A
//! relocating — or even a non-relocating, freeing — collection reached from
//! one of THEIR OWN allocations leaves the rest of the build writing through
//! a dangling or from-space address: none of those locals is a root the
//! collector knows about, so a mark-sweep that runs before they are stored
//! into the (rooted) AtomicI64 slots considers them garbage. Same invariant,
//! same fix (`crate::gc::GcSuppressScope`), applied at the top of each.
//!
//! Both are reachable ahead of `populate_global_this_builtins` on the
//! allocation-point route: `generator_prototype_ptr` /
//! `generator_function_prototype_of` are called from the codegen-emitted
//! `js_generator_attach_prototype` / `js_generator_attach_closure_prototype`
//! helpers on literally the first `gen()` call in a program that has not yet
//! touched `globalThis`, and `ensure_typed_array_intrinsic` is reachable the
//! same way from `typedarray_props.rs:812`.
//!
//! # Why #7217 could not gate this directly, and what changed
//!
//! Filed rather than shipped ungated, per CLAUDE.md's knob-kill policy. Four
//! attempts at a gate — the three the issue records, plus one made here —
//! all passed with the window deleted before this shape was found.
//!
//! 1. **Arm a collection and let ordinary allocation reach it**, the way
//!    `global_bootstrap.rs` does for the ~1.15 MB `populate_global_this_builtins`
//!    bootstrap. A tower is three orders of magnitude smaller and fits inside
//!    one arena block's tail, so it can reach `gc_check_trigger()` not at all —
//!    the test measured nothing. FIXED HERE by
//!    [`force_next_general_arena_alloc_slow`]: pre-fill the current block to
//!    (very nearly) full before calling the builder, so the builder's own
//!    first allocation unconditionally takes the slow path that calls
//!    `gc_check_trigger()`, regardless of how small the tower is. This is the
//!    same lever `gc::tests::runtime_roots::generator_attach_prototype`'s
//!    #7577 gate uses to land a collection inside a specific callee's own
//!    allocation.
//!
//! 2. **Re-exec the test binary so the child is the first to touch a tower.**
//!    The six intrinsic-tower statics were plain process-global `AtomicI64`s,
//!    built exactly once per *process* — so whichever test happened to run
//!    first on that binary (libtest's ordering is not something a single test
//!    controls) built them for every other test, including a freshly
//!    re-exec'd child, because OTHER tests in the SAME binary run before this
//!    one and touch `globalThis`. FIXED HERE by converting the six statics
//!    (`TYPED_ARRAY_INTRINSIC_PTR`, `TYPED_ARRAY_INTRINSIC_PROTO_PTR`,
//!    `GENERATOR_FUNCTION_INTRINSIC_PTR`, `GENERATOR_INTRINSIC_PROTO_PTR`,
//!    `GENERATOR_PROTOTYPE_PTR`, `ASYNC_GENERATOR_FUNCTION_INTRINSIC_PTR`,
//!    `ASYNC_GENERATOR_INTRINSIC_PROTO_PTR`, `ASYNC_GENERATOR_PROTOTYPE_PTR`)
//!    in `object/mod.rs` from a bare `static` to `per_test_global!` — the
//!    exact mechanism #7672 built for this class of hazard (see
//!    `per_test_global.rs`'s module docs). In a test build each libtest
//!    THREAD gets its own zeroed instance, and libtest runs one thread per
//!    test, so "per thread" and "per test" coincide: this test's first read
//!    of `GENERATOR_FUNCTION_INTRINSIC_PTR` is guaranteed `0` regardless of
//!    what any other test in the binary did. Non-test builds expand to the
//!    identical plain `static` — zero behavioural change in shipped binaries.
//!
//! 3. **Record `gc_is_suppressed()` from inside the builder under
//!    `#[cfg(test)]`.** Reported *suppressed* even with the scope removed,
//!    which turned out to be attempt 2's ordering hazard again in a different
//!    guise: `populate_global_this_builtins` itself opens a `GcSuppressScope`
//!    and calls both builders INSIDE it, so a probe that let ANY earlier test
//!    reach `js_get_global_this()` first would forever after read `true`.
//!    This test structurally avoids that by calling the tower builder
//!    directly, never through `populate_global_this_builtins` /
//!    `js_get_global_this()` — there is no outer window to be confused with.
//!
//! 4. **Arm the ArenaBytes/malloc byte-threshold trigger** (`GC_NEXT_TRIGGER_BYTES`
//!    via `GcTriggerThresholdTestGuard::make_arena_trigger_due`, the lever
//!    `gc::tests::runtime_roots::generator_attach_prototype` uses) under
//!    `force_legacy_gc_pacing`. **This PASSED with the scope removed, on the
//!    first draft of this very file, and would have shipped a vacuous gate.**
//!    The reason: `gc_check_trigger`'s nursery-churn branch only runs a
//!    synchronous, completes-on-the-spot collection when
//!    `gc_scavenge_enabled() || gc_moving_loop_polls_enabled() ||
//!    registered_root_scanners_block_budgeted_gc()` — with legacy pacing
//!    (scavenge and polls both off) and no scanner registered, an ArenaBytes
//!    trigger being "due" just starts a *budgeted* cycle that needs an
//!    explicit `gc_runtime_safepoint()` pump to advance. Nothing pumps one
//!    inline, so `gc_collection_count()` never moves whether or not the
//!    builder is suppressed — the assertion below would have been comparing
//!    two numbers that could never differ. FIXED HERE by arming
//!    `GC_OLD_RECLAIM_PENDING` instead (exactly `global_bootstrap.rs`'s own
//!    lever): the `OldReclaim` branch in `gc_check_trigger` has no such
//!    gating — `!gc_budgeted_cycle_active() &&
//!    matches!(gc_budgeted_due_trigger(), Some(OldReclaim)) &&
//!    !GC_OLD_RECLAIM_IN_PROGRESS` — and calls
//!    `gc_collect_full_mark_sweep_with_trigger` SYNCHRONOUSLY the moment it is
//!    reached, under any pacing. Combined with attempt 1's block-filling (so
//!    the tiny tower's first allocation reaches `gc_check_trigger` AT ALL),
//!    this is the first version verified — by actually deleting the fix and
//!    watching the test go red — to test anything.
//!
//! # Both halves are asserted (CLAUDE.md's fourth way a gate cannot fail)
//!
//! Each test below arms ONE pending collection, shows the builder does not
//! service it, and then shows the SAME armed request IS serviced by ordinary
//! allocation once the builder has returned — proving the arming was live
//! rather than merely absent, exactly as `global_bootstrap.rs` does for
//! `populate_global_this_builtins`.
//!
//! **SABOTAGE, run by hand and not merely asserted:** commenting out either
//! `let _no_move = crate::gc::GcSuppressScope::new();` line (generator.rs:602,
//! typed_array.rs:436) turns its test red —
//! `a collection ran inside build_generator_tower / ensure_typed_array_intrinsic`
//! — confirming attempt 4's mechanism actually observes the fix, unlike
//! attempts 1-4's own false starts above.

use super::super::*;
use super::support::*;
use std::sync::atomic::Ordering as StdOrdering;

/// Run `body` on a thread that has touched neither `globalThis` nor either
/// tower, mirroring `global_bootstrap.rs::on_a_fresh_thread`. Belt-and-braces
/// alongside the `per_test_global!` conversion above: libtest already gives
/// each `#[test]` its own thread (and therefore its own instance of every
/// `per_test_global!` table), but spawning explicitly keeps this test's
/// guarantee independent of that harness detail.
fn on_a_fresh_thread(body: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(body)
        .expect("spawn tower test thread")
        .join()
        .expect("tower test thread panicked");
}

/// Force the CURRENT arena block to (very nearly) full, so the very next
/// allocation takes `arena_cell_alloc`'s slow path and reaches
/// `gc_check_trigger()`. A local copy of
/// `gc::tests::runtime_roots::force_next_general_arena_alloc_slow`: that one
/// is private to a sibling module, and the two helpers have diverged reasons
/// for existing (#7577's lands a collection at ONE specific callee's own
/// allocation deep in a call chain; this one exists so a tower three orders
/// of magnitude smaller than a block still reaches a trigger check on its
/// very FIRST allocation, covering the whole call).
fn force_next_general_arena_alloc_slow() {
    const TEST_BLOCK_SIZE: usize = 1024 * 1024;
    let _ = crate::arena::arena_alloc(TEST_BLOCK_SIZE, 8);
}

/// Make one collection due at the very next `gc_check_trigger()`, via the
/// `OldReclaim` branch — the only one `gc_check_trigger` services
/// synchronously regardless of scavenge/polls/scanner pacing (see attempt 4
/// above). Identical lever to `global_bootstrap.rs::arm_one_pending_collection`.
fn arm_one_pending_collection() {
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(true));
}

fn pending_collection_still_owed() -> bool {
    GC_OLD_RECLAIM_PENDING.with(std::cell::Cell::get)
}

/// Arm the OldReclaim request AND guarantee the very next allocation reaches
/// the check that services it, however small the allocator that follows is.
fn arm_collection_reachable_by_next_allocation() {
    force_next_general_arena_alloc_slow();
    arm_one_pending_collection();
}

/// THE CONTROL, shared by both towers below — identical shape to
/// `global_bootstrap.rs`'s `ordinary_allocation_services_the_armed_collection`.
/// Forces one more slow-path allocation (so the still-armed `OldReclaim`
/// request is reached immediately rather than waiting on however much of the
/// current block happens to be free) and asserts it was serviced. `false`
/// means the arming was inert on this thread, and the "did not collect"
/// assertion above it proved nothing.
fn armed_collection_is_serviced_by_the_next_allocation(collections_before: u64) -> bool {
    force_next_general_arena_alloc_slow();
    gc_collection_count() > collections_before
}

#[test]
fn generator_tower_runs_in_a_no_move_window() {
    on_a_fresh_thread(|| {
        let _pacing = crate::gc::policy::force_legacy_gc_pacing();
        crate::gc::ensure_gc_initialized();
        // LIVE SUBJECT, precondition: this really is the first touch. Without
        // this a stray earlier build (attempts 2/3 above) would silently
        // early-return from `ensure_generator_intrinsics` and this test would
        // measure a no-op.
        assert_eq!(
            crate::object::GENERATOR_FUNCTION_INTRINSIC_PTR.load(StdOrdering::Acquire),
            0,
            "GENERATOR_FUNCTION_INTRINSIC_PTR was already built before this \
             test's first touch — the per-test-global isolation (#7251/#7672) \
             did not give this thread a fresh tower, so this test proved \
             nothing"
        );

        arm_collection_reachable_by_next_allocation();
        let collections_before = gc_collection_count();

        // THE SUBJECT: the lazy generator/async-generator tower build.
        crate::object::ensure_generator_intrinsics();

        // LIVE SUBJECT, half 1: the tower really built (both towers — sync
        // and async each call `build_generator_tower` once).
        assert_ne!(
            crate::object::GENERATOR_FUNCTION_INTRINSIC_PTR.load(StdOrdering::Acquire),
            0,
            "the tower did not build at all, so this test measured nothing"
        );
        assert_ne!(
            crate::object::ASYNC_GENERATOR_FUNCTION_INTRINSIC_PTR.load(StdOrdering::Acquire),
            0,
            "the async tower did not build at all, so this test measured \
             nothing"
        );

        let collections_after = gc_collection_count();
        // THE INVARIANT: nothing collected while `ctor`/`proto`/`gen_proto`
        // were raw, un-rewritable locals.
        assert_eq!(
            collections_after, collections_before,
            "a collection ran inside `build_generator_tower` — its `ctor` / \
             `proto` / `gen_proto` locals are now dangling or from-space"
        );
        assert!(
            pending_collection_still_owed(),
            "the window must DEFER the request, not drop it: leaving it \
             unserviced-and-unset would disable the trigger for the rest of \
             the thread"
        );
        assert!(
            !crate::gc::gc_is_suppressed(),
            "the no-move window must close when the tower builder returns"
        );

        // LIVE SUBJECT, half 2 — THE CONTROL.
        assert!(
            armed_collection_is_serviced_by_the_next_allocation(collections_after),
            "the armed collection was never serviceable on this thread, so \
             'the tower build did not collect' proved nothing"
        );
    });
}

#[test]
fn typed_array_intrinsic_tower_runs_in_a_no_move_window() {
    on_a_fresh_thread(|| {
        let _pacing = crate::gc::policy::force_legacy_gc_pacing();
        crate::gc::ensure_gc_initialized();
        assert_eq!(
            crate::object::TYPED_ARRAY_INTRINSIC_PTR.load(StdOrdering::Acquire),
            0,
            "TYPED_ARRAY_INTRINSIC_PTR was already built before this test's \
             first touch — the per-test-global isolation (#7251/#7672) did \
             not give this thread a fresh tower, so this test proved nothing"
        );

        arm_collection_reachable_by_next_allocation();
        let collections_before = gc_collection_count();

        // THE SUBJECT.
        let (ctor, proto) = crate::object::ensure_typed_array_intrinsic();

        assert!(
            !ctor.is_null() && !proto.is_null(),
            "the intrinsic did not build at all, so this test measured \
             nothing"
        );

        let collections_after = gc_collection_count();
        assert_eq!(
            collections_after, collections_before,
            "a collection ran inside `ensure_typed_array_intrinsic` — its \
             `ctor` / `proto` locals are now dangling or from-space"
        );
        assert!(
            pending_collection_still_owed(),
            "the window must DEFER the request, not drop it"
        );
        assert!(
            !crate::gc::gc_is_suppressed(),
            "the no-move window must close when the builder returns"
        );

        assert!(
            armed_collection_is_serviced_by_the_next_allocation(collections_after),
            "the armed collection was never serviceable on this thread, so \
             'the builder did not collect' proved nothing"
        );
    });
}
