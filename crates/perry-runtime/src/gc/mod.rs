//! Mark-sweep garbage collector for Perry
//!
//! Design:
//! - 8-byte GcHeader prepended to every heap allocation (invisible to callers)
//! - Arena objects (arrays/objects): discovered by walking arena blocks linearly (zero per-alloc tracking cost)
//! - Explicit malloc objects (promises/maps/errors, large closures, and compatibility residents): tracked in MALLOC_STATE
//! - Mark phase: precise thread-local roots + optional conservative stack scan + type-specific tracing
//! - Sweep phase: free malloc objects; arena objects added to free list for reuse
//! - Trigger: only checked on new arena block allocation or explicit gc() call
//!
//! Low-pause contract:
//! - Normal automatic GC work and mutator assists must eventually advance in
//!   bounded work-unit steps, independent of heap size.
//! - Explicit `gc()` calls may synchronously run the configured collection
//!   because the caller requested that pause; traces distinguish manual minor
//!   work from explicit full collection.
//! - Emergency full collections are reserved for allocation failure recovery,
//!   only outside suppressed, reentrant, or unsafe regions, and must be
//!   reported separately.
//!
//! Threshold-triggered work in `gc_check_trigger()` is debt-paced: heap goals
//! start or resume a budgeted cycle and allocation-side checks spend bounded
//! mutator-assist work instead of running a whole automatic collection.

use std::alloc::{alloc, dealloc, realloc, Layout};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::marker::PhantomData;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex, MutexGuard, OnceLock,
};
use std::time::{Duration, Instant};

mod types;
pub use types::*;
mod policy;
pub(crate) use policy::gc_runtime_safepoint;
/// The one writer of `GC_SAFEPOINT_PENDING` — it also keeps the poll's global
/// arming shadow in step. See `gc/poll_arm.rs`.
pub(crate) use policy::set_safepoint_pending;
pub use policy::*;
mod progress;
pub use progress::*;
mod heap_budget;
pub(crate) use heap_budget::*;
mod pressure;
pub use pressure::*;
mod telemetry;
pub use telemetry::*;
mod malloc;
pub use malloc::*;
/// #7469: the `gc` half of the hot-thread-local address cache. Split out of
/// `barrier.rs` / `layout.rs` / `malloc.rs` so each stays under the repo's
/// 2000-line-per-file cap.
mod hot_tls;
pub(crate) use hot_tls::*;
mod roots;
pub use roots::*;
#[cfg(test)]
/// Rewrite runtime-handle roots only; this deliberately does not rewrite the
/// installed `INLINE_TRAP`, whose scanner is exercised separately.
pub(crate) fn test_rewrite_runtime_handles_for_forwarded_objects() {
    let valid_ptrs = build_valid_pointer_set();
    let mut visitor = RuntimeRootVisitor::for_rewrite(&valid_ptrs);
    scan_runtime_handle_roots_mut(&mut visitor);
}
/// #7148: the census of conservative-scan fallbacks and the precise-safepoint
/// drains that replace them. Declared next to `roots` because
/// `ManualGcScanGuard` is what records into it.
mod scan_fallback;
pub(crate) use scan_fallback::*;
// The one decoder shared by the mark, rewrite and incremental-barrier paths
// for words that may hold a heap reference (#6910). Declared before its
// consumers for readability only — Rust module order is irrelevant.
mod root_words;
use root_words::*;
mod layout;
mod layout_slot_visit;
pub(crate) use layout_slot_visit::*;
/// #7510: the per-object slot-layout side tables and the emptiness flag that
/// keeps them off the allocation, store, death and trace paths. Split out of
/// `layout.rs` so it stays under the repo's 2000-line-per-file cap.
mod layout_tables;
// The immortal-object construction window and the table-occupancy readout, both
// consumed from OUTSIDE `gc`: `object::global_this` opens the window around the
// `globalThis` bootstrap and prints the residue under `PERRY_GC_DIAG`.
pub(crate) use layout_tables::per_object_layout_table_sizes;
pub use layout_tables::ImmortalLayoutScope;
/// #7510 item 1: the construction-side memo that turns an already-installed
/// typed shape into two header bit-writes instead of a descriptor build plus a
/// `SHAPE_LAYOUTS` round-trip.
mod shape_install;
pub use layout::*;
pub(crate) use shape_install::shape_install_memo_hot_addr;
mod trace;
pub(crate) use trace::*;
mod barrier;
pub use barrier::*;
/// #7630: the runtime slot-store helpers, split from `barrier.rs` (2000-line cap).
mod barrier_store;
pub use barrier_store::*;
mod dirty_page_cache;
// #7187 Phase B: `crate::arena`'s page-metadata module invalidates the
// barrier's "already dirty" page cache when it un-stamps or discards a page.
// Re-exported under an unambiguous name — `arena` cannot see `gc`'s privates.
pub(crate) use dirty_page_cache::invalidate as dirty_page_cache_invalidate;
mod barrier_arming;
// #7277: every item in `barrier_arming` is `pub(super)` (i.e. `pub(in gc)`),
// which is narrower than `pub(crate)` — so the glob re-exported nothing and
// rustc warned. A plain `use` brings them into `gc`'s namespace, which is all
// the in-module callers (`telemetry.rs`, `cycle.rs`) actually need.
use barrier_arming::*;
/// #7645: `GC_FLAG_PINNED` custody + the young-pin latch the copying minor's
/// eligibility preflight is skipped on. Every write of the bit goes through
/// `pin::pin_object`; `scripts/gc_pin_sites.py` enforces that in `lint`.
mod pin;
#[cfg(test)]
pub(crate) use pin::test_reset_young_pin_latch;
pub use pin::{
    copied_minor_preflight_skips, copied_minor_preflight_walks, pin_object, pin_object_non_young,
    unpin_object,
};
use pin::{note_preflight_skipped, note_preflight_walked, young_pin_latch_armed};
/// Software prefetch helpers for the collector's pointer-chasing loops
/// (drain, `clear_marks`, the remembered-set dirty scan).
mod prefetch;

mod copying;
mod copying_first_cycle;
/// Per-scanner root attribution for the copied-minor root scan (#7915).
mod scanner_profile;
mod sticky_remembered;
use copying::*;
use copying_first_cycle::*;
use sticky_remembered::*;
// The copied-minor pointer classifier is consumed by the weak-holder registry
// pass in `crate::weakref` (#6182), which lives outside the gc module.
pub(crate) use copying::CopyingPointerSet;
// The hard ceiling every birth-generation threshold in `gc::types` must stay
// under; asserted by `arena::tests::pointer_bearing_large_object_threshold_is_movable`.
pub(crate) use copying::MAX_YOUNG_MOVE_BYTES;
mod dead_owner;
mod old_free;
use old_free::*;
pub(crate) use old_free::{old_free_bytes, old_free_filter_range, old_free_take_exact};
mod tenuring;
use tenuring::*;
mod oldgen;
use oldgen::*;
mod oldgen_defrag;
use oldgen_defrag::*;
mod cycle;
use cycle::*;
mod verify;

/// #7035: whole-heap from-space scan — verification that does NOT depend on
/// the rewrite pass own root enumeration. Debug-only
/// (`PERRY_GC_FROMSPACE_SCAN=1`).
mod fromspace_scan;
/// #7742: the measured policy behind whole-block in-place promotion. The
/// mechanism is `arena/promote.rs`; this decides when to use it.
mod promote_in_place;
use promote_in_place::*;
pub use promote_in_place::{
    first_cycle_promotion_attempts, first_cycle_promotion_rollbacks, in_place_promoted_objects,
    in_place_promotion_cycles, untraced_promoted_objects, untraced_promotion_cycles,
};
/// Instrument-liveness counters (#7604): copying minors completed, objects
/// relocated, loop back-edge polls reached. Mode-independent — they count what
/// the COLLECTOR did, not what forced it, so they outlive any one stress knob.
pub(crate) mod instruments;
/// The loop back-edge poll's arming word: the one load that decides whether
/// `js_gc_loop_safepoint` is worth calling at all. Not debug-only — it is on
/// the hot path of every allocating loop.
pub(crate) mod poll_arm;
/// #7154 tooling: collect on a deterministic pseudo-random schedule derived from
/// a seed, at a density `PERRY_GC_SCHEDULE_RATE` tunes from "never" up to every
/// handled safepoint, so a failing seed is a reproducer. Debug-only
/// (`PERRY_GC_SCHEDULE_SEED=<u64>`).
pub(crate) mod schedule;
pub(crate) use instruments::note_loop_poll_reached;
pub use instruments::{copying_minor_cycles, loop_polls_reached, moved_objects_total};
pub use poll_arm::PERRY_GC_POLL_ARMED;
pub(crate) use poll_arm::{arm_poll, disarm_poll, poll_armed, resolve_poll_seed};
pub use schedule::{
    gc_schedule_forced_collections, gc_schedule_safepoints, schedule_liveness_report,
    schedule_polls_paced,
};
pub use verify::*;
#[cfg(feature = "diagnostics")]
mod heap_snapshot;
#[cfg(feature = "diagnostics")]
pub use heap_snapshot::gc_build_v8_heap_snapshot_json;

pub fn gc_collect_minor() -> u64 {
    if defer_gc_request(DeferredGcRequest::DirectMinor) {
        return 0;
    }
    gc_collect_minor_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct))
        .emit_after_current()
}

pub(super) fn gc_collect_minor_with_trigger(trigger: GcTriggerSnapshot) -> GcCollectOutcome {
    gc_collect_minor_with_trigger_inner(trigger, FullEscalation::Allowed)
}

/// May this minor be escalated to a full mark-sweep by the two THROUGHPUT
/// PACING predicates (`copied_minor_promotion_handoff_due`,
/// `arena_growth_full_escalation_due`)?
///
/// Both exist so a long-running mutator does not accumulate array-growth stubs
/// the non-moving minor cannot reclaim, and on every automatic path the answer
/// is `Allowed`. `Refused` exists for exactly one caller: the explicit `gc()`
/// under `PERRY_GC_FORCE_EVACUATE`, which asked for a *moving* collection and
/// is followed immediately by a full mark-sweep anyway (#6946). A full sweep
/// moves nothing, so an escalation there hands the caller a non-moving
/// collection under a knob whose whole name is about relocation — which is
/// precisely how that knob came to be inert for every `gc()`-driven test.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FullEscalation {
    Allowed,
    Refused,
}

/// The minor an explicit `gc()` runs first when forced evacuation is on
/// (#6946). Refuses the pacing escalation, so the caller gets the moving
/// collection the knob promises rather than a full sweep that moves nothing.
pub(super) fn gc_collect_forced_evacuating_minor(trigger: GcTriggerSnapshot) -> GcCollectOutcome {
    gc_collect_minor_with_trigger_inner(trigger, FullEscalation::Refused)
}

fn gc_collect_minor_with_trigger_inner(
    trigger: GcTriggerSnapshot,
    escalation: FullEscalation,
) -> GcCollectOutcome {
    // PERRY_GC_SAFEPOINT_ONLY: held for the whole collection so every
    // consumer of the scan decision (root scan, copying eligibility,
    // evacuation pinning, verifier) sees the same healed answer.
    let _contract_heal = policy::contract_scan_heal_guard();
    gc_drain_active_budgeted_cycle();
    // Barriers-off ⇒ the remembered set is not being maintained, and a
    // minor's black-leafed old parents would hide live children. Route
    // every caller (direct arm, moving-safepoint arm, public FFI) to the
    // full collection instead of trusting an empty RS.
    if !gen_gc_enabled() {
        return gc_collect_full_mark_sweep_with_trigger(trigger);
    }
    // Phase C4b-γ-3: re-entrancy guard. Without this, the evacuation
    // pass's `arena_alloc_gc_old` can trigger `gc_check_trigger` (via
    // `arena.alloc`'s slow-path block-fill) DURING the outer collection
    // cycle. The outer cycle's MARK_SEEDS, CONS_PINNED, and valid_ptrs
    // are all in indeterminate states mid-evac; a recursive
    // `gc_collect_minor` clears them, runs its own mark phase from a
    // mostly-empty C-stack snapshot (we're deep inside the runtime,
    // very few user pointers reachable), evacuates whatever it can find,
    // then returns to the outer cycle which proceeds with corrupt
    // pinning + corrupt seed list. Symptom: bench_evac_heavy's `cache`
    // local gets evacuated by the inner cycle (un-pinned because the
    // inner mark_stack_roots can't see it through the deep-runtime
    // stack), and the outer rewrite walk doesn't update the user's
    // shadow stack slot to point at the new copy → cache.length reads
    // garbage from the FORWARDED slot's first 8 bytes thereafter.
    //
    // Fix: set GC_FLAG_IN_ALLOC for the entire duration of
    // gc_collect_minor. `gc_check_trigger` already early-returns when
    // this bit is set. Any recursive `gc_check_trigger` call from
    // arena_alloc_gc_old / arena_alloc_gc / gc_malloc inside the
    // collection sees the bit and bails. The outer cycle's bookkeeping
    // stays intact.
    let prev_in_alloc = GC_FLAGS.with(|f| {
        let prev = f.get();
        f.set(prev | GC_FLAG_IN_ALLOC);
        prev & GC_FLAG_IN_ALLOC
    });
    let may_escalate = escalation == FullEscalation::Allowed;
    if may_escalate && copied_minor_promotion_handoff_due(trigger.kind) {
        // #7592: latch before running it. This full is non-moving and promotes
        // nothing, so it cannot relieve the survivor pressure that scheduled
        // it; without the latch the predicate is still true at the next minor
        // and the collector livelocks on fulls that free nothing.
        note_survivor_promotion_handoff_full();
        let outcome = gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(
            GcTriggerKind::SurvivorPromotionBytes,
        ));
        restore_minor_in_alloc(prev_in_alloc);
        return outcome;
    }
    // #6893-followup: major-GC pacing. A non-moving minor can't free array-growth
    // forwarding stubs, so reallocation-heavy churn grows the arena unbounded —
    // only a full mark-sweep reclaims stubs. Escalate to a full once the arena's
    // live bytes exceed K× the last full's live set (belt-and-suspenders for
    // callers that reach a minor outside the budgeted pressure path).
    if may_escalate && arena_growth_full_escalation_due() {
        let outcome =
            gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(trigger.kind));
        restore_minor_in_alloc(prev_in_alloc);
        return outcome;
    }
    let mut trace = GcCycleTrace::new(GcCollectionKind::Minor, trigger);
    let start = Instant::now();
    crate::arena::old_pages_begin_gc_cycle();
    let previous_pause_us = gc_last_pause_us();
    let current_rss_bytes = crate::process::get_rss_bytes();
    // Not budgeted, so the low-pause veto does not apply and the policy is
    // always allowed to run here (#7611 deleted the env veto that used to sit
    // in this slot). The variable stays rather than being folded away: it is
    // recorded in the cycle trace and read back by the evacuation-policy tests.
    let evacuation_policy_allowed = true;
    let force_evacuation = gc_force_evacuate_enabled();
    // MARK_SEEDS persists across GC cycles. Clear before any try_mark
    // call so trace sees only this cycle's freshly-marked headers.
    clear_mark_seeds();
    if let Some(fast_path) = gc_collect_minor_copying_fast_path(&mut trace, start, trigger.kind) {
        let freed_bytes = fast_path.freed_bytes;
        let elapsed_us = start.elapsed().as_micros() as u64;
        GC_STATS.with(|stats| {
            stats
                .borrow_mut()
                .record_collection(freed_bytes, elapsed_us);
        });
        restore_minor_in_alloc(prev_in_alloc);
        if let Some(trace) = trace.as_mut() {
            trace.pause_us = elapsed_us;
            trace.capture_layout_scans();
        }
        return GcCollectOutcome {
            freed_bytes,
            malloc_swept: fast_path.malloc_swept,
            trace,
        };
    }
    clear_mark_seeds();
    // Old-page defrag belongs to the non-copying fallback below. Snapshotting
    // and sorting all old-page metadata before trying the copying fast path
    // charged every ordinary minor an O(old pages) cost even though that path
    // cannot consume the selection. Defer both selection and source-block
    // expansion until the fast path has declined the collection.
    let old_page_selection = if old_to_young_tracking_complete() {
        select_old_page_defrag_pages(force_evacuation)
    } else {
        OldPageDefragSelection::default()
    };
    let old_page_source_blocks =
        crate::arena::old_arena_source_blocks_for_pages(&old_page_selection.pages);
    GcCycleState::new_minor_fallback(
        trigger,
        trace,
        start,
        trigger.kind.progress_kind(GcCollectionKind::Minor),
        prev_in_alloc,
        previous_pause_us,
        current_rss_bytes,
        evacuation_policy_allowed,
        force_evacuation,
        EVACUATION_POLICY_DISABLED_REASON,
        old_page_selection,
        old_page_source_blocks,
    )
    .run_to_completion()
}

#[inline]

pub fn gen_gc_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        // Generational minors are only sound with runtime write barriers:
        // minors black-leaf old parents and trust the remembered set for
        // every old→young/old→malloc edge. `PERRY_WRITE_BARRIERS=0` used
        // to disable only evacuation gating while minors kept running —
        // the remembered set stayed empty, so a born-old (>16 KB) parent's
        // nursery children were swept on the first minor and the
        // "bisection" mode crashed for reasons unrelated to what was being
        // bisected. Barriers off now means full mark-sweep only.
        if !write_barriers_enabled() {
            return false;
        }
        env_default_on_enabled("PERRY_GEN_GC")
    })
}

// ★ `PERRY_GEN_GC_EVACUATE` was DELETED here (#7611). Read this before adding
// an "escape hatch" back.
//
// It used to gate `evacuation_policy_allowed` — the C4b tenured→old-gen policy
// evacuation and the old-page defrag selection — and to veto
// `gc_force_evacuate_enabled()` below.
//
// **It was measured inert where anyone was looking.** On the pinned quiet host,
// identical binaries and protocol, the only difference being the knob, a
// cell-by-cell diff over all 12 gc-ratchet probes × 8 counters
// (`minor_cycles`, `step_cycles`, `copied_objects`, `copied_bytes`,
// `promoted_objects`, `promoted_bytes`, `freed_bytes`, `heap_used_bytes`)
// reported **0 of 96 cells moved** — bit-identical medians, and
// `gc_ratchet.py check` exit 0 with the knob set. For contrast the same
// procedure with `PERRY_GEN_GC=0` moved 79 cells and returned 90 findings, so
// the harness was sensitive and this knob specifically was not. The mechanism:
// the counters the ratchet reads come from the COPYING minor
// (`gc_collect_minor_copying_fast_path`), which this knob never gated; what it
// gated is the non-copying fallback's policy evacuation, which those probes do
// not reach.
//
// **Its one unique live effect was a footgun.** Vetoing
// `gc_force_evacuate_enabled()` meant an ambient `PERRY_GEN_GC_EVACUATE=0`
// silently disarmed the #7154 stress instrument (`PERRY_GC_SCHEDULE_SEED`),
// so a stress run could report "clean" having moved nothing. CLAUDE.md documented that as a
// caveat rather than treating it as the defect it is. Deleting the knob deletes
// the way to disarm the instrument by accident.
//
// **The branch it gated is NOT deleted with it, because the branch has another
// controller that IS exercised.** `evacuation_policy_allowed` is still false on
// every budgeted low-pause cycle (`low_pause_non_moving` in
// `gc_start_budgeted_minor_fallback_cycle_with_snapshot`), and
// `budgeted_low_pause_minor_does_not_evacuate` asserts that arm behaviourally —
// nothing moved, no forwarding stub, old-page selection skipped, and
// `trace.evacuation_policy.reason == "low_pause_non_moving"`. So the losing
// mode still compiles and still has a test; what stopped existing is the
// untested *configuration*.
//
// Per CLAUDE.md's binding GC knob kill-policy: "a mode that still exists is a
// decision that hasn't been made".

fn gc_force_evacuate_enabled() -> bool {
    #[cfg(test)]
    if let Some(forced) = knob_overrides::FORCE_EVACUATE_TEST_OVERRIDE.with(std::cell::Cell::get) {
        return forced;
    }
    // `PERRY_GC_SCHEDULE_SEED` implies forced evacuation (#7154 tooling): a
    // scheduled minor that leaves survivors in place would move nothing, and
    // "an unrooted value moves on its first exposure" is the entire contract of
    // the mode — without this it would be a knob whose name promises relocation
    // stress and whose effect is sweep pressure. Unconditional, per #7611's
    // deletion note above.
    schedule::gc_schedule_enabled() || env_flag_enabled("PERRY_GC_FORCE_EVACUATE")
}

fn gc_verify_evacuation_enabled() -> bool {
    #[cfg(test)]
    if let Some(forced) = knob_overrides::VERIFY_EVACUATION_TEST_OVERRIDE.with(std::cell::Cell::get)
    {
        return forced;
    }
    env_flag_enabled("PERRY_GC_VERIFY_EVACUATION")
}

/// Per-thread test overrides for the two collector knobs the unit suite needs
/// to turn ON mid-run (#7946).
///
/// **A test may not reach for the process environment to do this.** `set_var`
/// is process-wide, so a `PERRY_GC_FORCE_EVACUATE=1` held for one test's
/// duration is read by every other libtest thread — and `gc_force_evacuate_
/// enabled()` is an input to `should_promote_young_in_place()`, so it silently
/// turned in-place promotion OFF underneath `gc::tests::promote_in_place`'s
/// policy cases. Measured at 5 failed runs in 100 across three of them
/// (`a_promoting_cycle_still_measures_so_the_predictor_cannot_go_stale`,
/// `dead_byte_budget_stops_promotion_until_a_full_reclaims`,
/// `untraced_budget_forces_a_measuring_cycle_and_a_measurement_clears_it`); the
/// arm that skipped the env-setting tests dropped that family to zero.
///
/// The old `gc::tests::support::EnvVarGuard` took a mutex, which serialized the
/// *setters* against each other and did nothing at all for the ~2 200 readers.
/// That is the opt-in-defence shape `per_test_global!`'s module docs argue
/// against; per-thread storage is the same answer in a different place.
///
/// `ScheduleGuard` (thread-local) was already doing this for forced evacuation
/// via `PERRY_GC_SCHEDULE_SEED` — see
/// `gc::tests::evacuation::explicit_gc_under_forced_evacuation_runs_a_moving_minor`,
/// whose comment says in as many words that "an `EnvVarGuard` would set a
/// process-global every other test in this crate shares".
#[cfg(test)]
pub(super) mod knob_overrides {
    use std::cell::Cell;

    thread_local! {
        pub(super) static FORCE_EVACUATE_TEST_OVERRIDE: Cell<Option<bool>> =
            const { Cell::new(None) };
        pub(super) static VERIFY_EVACUATION_TEST_OVERRIDE: Cell<Option<bool>> =
            const { Cell::new(None) };
    }

    /// Pin `gc_force_evacuate_enabled()` for this thread only.
    pub(crate) struct ForcedEvacuationTestGuard(Option<bool>);

    impl ForcedEvacuationTestGuard {
        pub(crate) fn on() -> Self {
            Self(FORCE_EVACUATE_TEST_OVERRIDE.with(|c| c.replace(Some(true))))
        }
    }

    impl Drop for ForcedEvacuationTestGuard {
        fn drop(&mut self) {
            FORCE_EVACUATE_TEST_OVERRIDE.with(|c| c.set(self.0));
        }
    }

    /// Pin `gc_verify_evacuation_enabled()` for this thread only.
    pub(crate) struct VerifyEvacuationTestGuard(Option<bool>);

    impl VerifyEvacuationTestGuard {
        pub(crate) fn on() -> Self {
            Self(VERIFY_EVACUATION_TEST_OVERRIDE.with(|c| c.replace(Some(true))))
        }
    }

    impl Drop for VerifyEvacuationTestGuard {
        fn drop(&mut self) {
            VERIFY_EVACUATION_TEST_OVERRIDE.with(|c| c.set(self.0));
        }
    }
}

/// `PERRY_GC_SCAVENGE` — **ON by default since #7056**, kill switch
/// `PERRY_GC_SCAVENGE=0`/`off`/`false`. It is a PACING knob: it routes
/// nursery-churn triggers to the direct minor in `gc_check_trigger` instead of
/// the budgeted non-moving stepper, which on a reallocation-heavy loop frees
/// nothing. Paired with the nursery cap in `policy::effective_next_arena_trigger`
/// that is the -69% RSS result quoted on the getter below.
///
/// It does **not** decide whether the alloc-point minor may move, and #7682 is
/// what that confusion cost. The flag used to gate the `force_full_scan()` on
/// that arm off, so the shipped default ran an EVACUATING minor at an arbitrary
/// allocation point — a program point neither root lowering describes — and
/// values held only in registers were relocated behind their holders' backs.
/// The guard is now unconditional; see the comment at its site in
/// `policy::gc_check_trigger` for why no pacing knob can answer the question it
/// asks.
///
/// This doc comment previously read "Phase-1 de-risking flag (OFF by default)
/// … NOT sound as a production default yet". Both halves were false for two
/// hundred releases, eight lines above a body comment saying "ON BY DEFAULT" —
/// the #6987 shape CLAUDE.md warns about, and this time the stale half was the
/// one carrying the soundness argument.
///
/// **Kill-policy disposition, stated rather than left implicit — and stated
/// for the configuration that now ships.** The flag's only production reader is
/// the arm condition in `gc_check_trigger`, a three-way disjunction:
/// `gc_scavenge_enabled() || gc_moving_loop_polls_enabled() ||
/// registered_root_scanners_block_budgeted_gc()`.
///
///  * **With polls ON (the default since #7682's follow-up)** the second
///    disjunct carries the arm, and this flag decides nothing. It is redundant,
///    not load-bearing.
///  * **With `PERRY_GC_MOVING_LOOP_POLLS=0`** it is the only thing holding the
///    arm open, and dropping it would route nursery pressure to the budgeted
///    stepper, which is non-moving *and* reclaims almost nothing on a
///    reallocation loop. The third disjunct does NOT rescue that case: under
///    `gc_incremental_enabled()` (the default) it reduces to "any COPY-ONLY
///    scanner", and a compiled program has none — the reasoning
///    `test-parity/gc_matrix_inert_arms.txt` recorded for the `cons_scan_off`
///    arm, and the thing that made a first attempt at repairing
///    `generator_attach_prototype` fail for a third distinct reason.
///
/// So the honest summary is: this knob is now a modifier on the kill switch,
/// not a mode of its own. By CLAUDE.md's rule that is a candidate for deletion —
/// fold its behaviour into the polls-off path and stop having two flags whose
/// interaction nobody exercises. Deliberately NOT done here: this PR already
/// changes two defaults, and a third would make one bisect answer three
/// questions. The decision wants the arm condition's three disjuncts measured
/// on real programs, which is a separate change with a separate A/B.
///
/// An earlier draft of this comment claimed the knob was "very close to inert
/// for a compiled binary" on the strength of the third disjunct holding for
/// every compiled program. That is wrong under the default incremental stepper,
/// for the reason above. It is recorded rather than quietly deleted because
/// this whole PR exists because a stale half of a doc comment kept carrying a
/// soundness argument after it stopped being true.
#[cfg(test)]
thread_local! {
    /// Test-only override, consulted BEFORE the process-wide OnceLock so a
    /// single test can pin a pacing mode even though the process default is on.
    /// Same discipline as `GC_MOVING_LOOP_POLLS_TEST_OVERRIDE`.
    pub(super) static GC_SCAVENGE_TEST_OVERRIDE: std::cell::Cell<Option<bool>> =
        const { std::cell::Cell::new(None) };
}

pub(super) fn gc_scavenge_enabled() -> bool {
    #[cfg(test)]
    if let Some(forced) = GC_SCAVENGE_TEST_OVERRIDE.with(std::cell::Cell::get) {
        return forced;
    }
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        // ON BY DEFAULT (#7056). `PERRY_GC_SCAVENGE=0`/`off`/`false` reverts.
        //
        // This pairs with the nursery cap in `policy::effective_next_arena_trigger`
        // and the two are only worth anything TOGETHER. Measured as a 2x2 over
        // the 8 gc_ratchet probes, RSS and wall:
        //
        //     arm                        RSS      wall
        //     no cap, no scavenge     (base)    (base)
        //     cap only                 -33%      +23%
        //     CAP + SCAVENGE           -69%       +3%
        //     scavenge only             +0%       +2%
        //
        // Scavenge alone moves nothing, and the cap alone trades a third of the
        // footprint for a quarter of the wall time. Together they are -69% RSS
        // for +3%, because the cap makes collections frequent and scavenge makes
        // them evacuating (O(live) copying) rather than O(heap) sweeps — so the
        // frequency is cheap instead of expensive.
        //
        // What this does NOT do, despite what this comment used to claim: it
        // does not defer alloc-point collections to a precise safepoint. That
        // deferral is gated on `gc_moving_loop_polls_enabled()`, a DIFFERENT
        // flag, and for the whole #7161 stopgap the two disagreed — the
        // deferral was dead and the alloc-point minor ran right there, moving
        // objects at a register-imprecise point. That is #7682.
        //
        // The deferral is live again now that polls default ON, so the shipped
        // default does reach a precise safepoint — but not because of THIS
        // flag, and the alloc-point minor is sound on its own terms either way,
        // by being non-moving (`force_full_scan`).
        !matches!(
            std::env::var("PERRY_GC_SCAVENGE").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

#[cfg(test)]
fn gc_collect_inner() -> u64 {
    if defer_gc_request(DeferredGcRequest::Collect(GcTriggerKind::Direct)) {
        return 0;
    }
    gc_collect_inner_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct))
        .emit_after_current()
}

fn gc_collect_inner_with_trigger(trigger: GcTriggerSnapshot) -> GcCollectOutcome {
    // Issue #745: clear the per-cycle bytes-bump flag so the next
    // gc-suppressed parse can rebaseline the trigger again. Done at
    // the top so all entry points — full GC, minor GC, manual
    // `gc()`, the malloc-count trigger path — keep the flag in sync.
    GC_TRIGGER_BUMPED.with(|c| c.set(false));
    if gen_gc_enabled() {
        return gc_collect_minor_with_trigger(trigger);
    }
    gc_collect_full_mark_sweep_with_trigger(trigger)
}

fn gc_collect_full_mark_sweep_with_trigger(trigger: GcTriggerSnapshot) -> GcCollectOutcome {
    // PERRY_GC_SAFEPOINT_ONLY: see gc_collect_minor_with_trigger. Manual
    // gc() engages its own force_full_scan first, which this detects as
    // already-Scan and no-ops.
    let _contract_heal = policy::contract_scan_heal_guard();
    gc_drain_active_budgeted_cycle();
    GC_TRIGGER_BUMPED.with(|c| c.set(false));
    GcCycleState::new_full(trigger).run_to_completion()
}

fn gc_collect_emergency_full() -> GcCollectOutcome {
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Emergency))
}

/// Last-ditch recovery for a failed heap allocation (2026-07-09 audit):
/// run one synchronous full mark-sweep and let the caller retry the
/// allocation once. Returns false (caller proceeds straight to its panic)
/// when collecting here would be unsound: re-entrant emergency, inside a
/// collection/allocation bookkeeping window, or mid-budgeted-cycle.
///
/// The workspace builds with `panic = "unwind"`, and these OOM panics
/// cross `extern "C"` frames into aborts — on a memory-limited process
/// (cgroup `memory.max`, jetsam) dying without even attempting a
/// collection wasted the one chance to shed a heap full of garbage.
///
/// The conservative stack scan is forced for the same reason the
/// alloc-point direct arm forces it: this runs at an arbitrary allocation
/// site where locals of the current call chain may not be spilled to
/// shadow slots.
///
/// ★ #7148 disposition: **keep, justified, observable.** This is the one site
/// that provably cannot defer to a precise safepoint. Deferral trades a
/// collection now for a collection at the next safepoint, and this path is
/// entered only after a heap allocation has *already failed*: the caller's
/// next act is to panic, so there is no "next safepoint" to defer to — the
/// program does not survive to reach one. The pressure-spike question the
/// other sites must answer is therefore vacuous here; the spike has already
/// happened and this is the response to it.
///
/// It is instead made *measurable* (`ConservativeScanSite::EmergencyReclaim`
/// + a `PERRY_GC_DIAG` line), so "emergency reclaim never fires in practice"
/// stops being an assumption. The long-term plan for this site is the
/// statepoint work (`docs/statepoint-gc-experiment.md` on branch
/// `exp/stackmap-viability`, not on `main`): with native stack
/// maps a precise root set exists at *any* mapped PC, so an OOM-time
/// collection would not need the scan at all. That is the only mechanism that
/// removes this site, and it is not a deferral.
pub(crate) fn gc_try_emergency_reclaim() -> bool {
    thread_local! {
        static IN_EMERGENCY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    if IN_EMERGENCY.with(|c| c.get()) {
        return false;
    }
    if GC_FLAGS.with(|f| f.get()) & GC_FLAG_IN_ALLOC != 0 || gc_budgeted_cycle_active() {
        return false;
    }
    IN_EMERGENCY.with(|c| c.set(true));
    // A failed reservation can coexist with differently-sized pooled blocks.
    // Make the emergency full return those mappings before the one retry.
    crate::arena::request_block_pool_drain();
    let _scan = roots::ManualGcScanGuard::force_full_scan(ConservativeScanSite::EmergencyReclaim);
    let _ = gc_collect_emergency_full();
    IN_EMERGENCY.with(|c| c.set(false));
    true
}

#[cfg(test)]
pub(super) fn test_gc_collect_emergency_full_trace_json() -> serde_json::Value {
    let outcome = gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot {
        kind: GcTriggerKind::Emergency,
        steps_before: Some(GcStepSnapshot::current()),
    });
    outcome
        .trace
        .expect("test requested emergency full GC trace capture")
        .into_json(GcStepSnapshot::current())
}

thread_local! {
    /// Whether `gc_init` has registered this thread's root scanners yet. The
    /// scanner list (`MUTABLE_ROOT_SCANNERS`) is thread-local, so soundness
    /// requires every thread that can trigger a collection to register
    /// independently — not just the main thread that runs `js_gc_init()`.
    static GC_INIT_DONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// When set, `ensure_gc_initialized` is a no-op. The GC unit tests take
    /// manual control of the thread's scanner registry (see
    /// `ScopedRootScannerRegistryGuard`) and must collect with exactly the
    /// roots they install — lazy auto-init would pollute that controlled set.
    static AUTO_GC_INIT_SUPPRESSED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Suppress (or re-enable) lazy `ensure_gc_initialized` on this thread, returning
/// the previous value. Used by the GC tests' `ScopedRootScannerRegistryGuard` to
/// run collections against a hand-controlled root set.
#[allow(dead_code)] // test scaffolding: used only by ScopedRootScannerRegistryGuard under cfg(test)
pub(crate) fn set_auto_gc_init_suppressed(suppressed: bool) -> bool {
    AUTO_GC_INIT_SUPPRESSED.with(|c| c.replace(suppressed))
}

/// Register the runtime root scanners on the current thread if they haven't been
/// registered yet. Idempotent per thread; a no-op while auto-init is suppressed.
///
/// `js_gc_init()` runs this at the production entrypoint, but spawned worker
/// threads and the unit-test harness never call it — so without this a collection
/// on those threads runs with an empty scanner set and reclaims live objects
/// reachable only through a registered root (most importantly the realm global at
/// `GLOBAL_THIS_PTR` and the `Array`/`Object` intrinsics it holds). Called from
/// `js_get_global_this` before the global is created, so the global is born under
/// a registered scanner and survives later collections on this thread.
pub(crate) fn ensure_gc_initialized() {
    if AUTO_GC_INIT_SUPPRESSED.with(|c| c.get()) {
        return;
    }
    if !GC_INIT_DONE.with(|c| c.get()) {
        gc_init();
    }
}

/// Register a mutable root scanner, tagging it with its own path as the
/// attribution name (`gc/scanner_profile.rs`, #7915). Root-scan cost on
/// promise-heavy workloads is per registered root, so "which registry" is the
/// first question any investigation asks; deriving the name from the
/// registration site keeps the answer from drifting away from the list.
macro_rules! reg_scanner {
    ($scanner:expr $(,)?) => {
        gc_register_named_mutable_root_scanner(stringify!($scanner), $scanner)
    };
}

macro_rules! reg_budgeted_scanner {
    ($scanner:expr, $step:expr, $state:expr, $source:expr $(,)?) => {
        gc_register_budgeted_named_mutable_root_scanner_with_source(
            stringify!($scanner),
            $scanner,
            $step,
            $state,
            $source,
        )
    };
}

pub fn gc_init() {
    // Idempotent per thread: production calls this at startup, and
    // `ensure_gc_initialized` calls it lazily on threads that don't. Latch the
    // flag before any registration so a re-entrant call can't double-register
    // the thread-local scanner list.
    if GC_INIT_DONE.with(|c| c.replace(true)) {
        return;
    }
    crate::perf_hooks::init_time_origin();
    reg_budgeted_scanner!(
        scan_runtime_handle_roots_mut,
        scan_runtime_handle_roots_mut_step,
        new_runtime_handle_root_scan_state,
        MutableRootScannerSource::RuntimeHandles,
    );
    // #6951: expression temporaries generated code is holding in SSA registers
    // across a collection point. Same standing as the shadow stack — a precise
    // mutable root that is marked AND rewritten — and, like the shadow stack,
    // load-bearing the moment the conservative native-stack scan is off.
    reg_budgeted_scanner!(
        scan_temp_roots_mut,
        scan_temp_roots_mut_step,
        new_temp_root_scan_state,
        MutableRootScannerSource::RuntimeMutableScanner,
    );
    reg_scanner!(crate::promise::scan_native_async_completion_roots_mut);
    reg_budgeted_scanner!(
        promise_mutable_root_scanner,
        crate::promise::scan_promise_roots_mut_step,
        crate::promise::new_promise_root_scan_state,
        MutableRootScannerSource::RuntimeMutableScanner,
    );
    reg_budgeted_scanner!(
        timer_mutable_root_scanner,
        crate::timer::scan_timer_roots_mut_step,
        crate::timer::new_timer_root_scan_state,
        MutableRootScannerSource::RuntimeMutableScanner,
    );
    // 2026-07-02 audit P0 (ported from be73b4f8d): string-keyed descriptor
    // tables (defineProperty accessors/attrs) and the proxy registry +
    // reflect-metadata store were invisible to GC — values swept/moved under
    // live references, owner keys stale after evacuation.
    reg_scanner!(crate::object::descriptor_state::scan_descriptor_roots_mut);
    // #6759 Phase C3a: shape records follow their keys array across
    // evacuation (metadata-rewrite rekey only; the records hold no heap
    // references and mark nothing).
    reg_scanner!(crate::object::shapes::scan_shape_table_rekey_mut);
    reg_scanner!(crate::proxy::scan_proxy_roots_mut);
    // Object/string-valued `err.<prop> = v` user props live as raw bits in
    // ERROR_USER_PROPS — invisible to GC without this scanner (collectable
    // while reachable; stale addresses after a move). The address KEYS are
    // maintained by the ErrorSideTables move/finalize hooks.
    reg_scanner!(crate::node_submodules::diagnostics_gc::scan_error_user_props_roots_mut,);
    reg_scanner!(exception_mutable_root_scanner);
    reg_scanner!(async_context_mutable_root_scanner);
    reg_scanner!(async_hooks_mutable_root_scanner);
    reg_scanner!(shape_cache_mutable_root_scanner);
    reg_scanner!(crate::regex::scan_last_exec_groups_root_mut);
    // #7211: the eight interned `typeof` result strings, and JSON.rawJSON's
    // interned `"rawJSON"` key. Both are thread-local caches of a RAW
    // `StringHeader*` allocated in the nursery and referenced by nothing else,
    // so before this registration the FIRST minor collection sweeps or
    // evacuates them and the cached pointer names abandoned memory forever
    // after. Not a timing-dependent stale register: a permanently wrong cache,
    // which is why `sfw-registry --help` under a
    // `PERRY_GC_MOVING_LOOP_POLLS=1` build failed 10/10 rather than
    // intermittently, and why the from-space reporter blamed
    // `retired_by_minor=#0`.
    reg_scanner!(crate::builtins::arithmetic::scan_typeof_string_roots_mut);
    reg_scanner!(crate::json::raw_json::scan_raw_json_key_root_mut);
    reg_scanner!(crate::object::scan_exotic_expando_roots_mut);
    reg_scanner!(crate::array::scan_template_raw_roots_mut);
    // #6981: the memoized `Array.prototype` / `Object.prototype` addresses in
    // `array::indexing`. Raw addresses of movable objects — a relocating cycle
    // that does not rewrite them leaves the hole/OOB read fallback comparing a
    // stale address against a forwarding-resolved receiver, which defeats its
    // own self-recursion guard and drives the mutator into unbounded recursion.
    reg_scanner!(crate::array::scan_prototype_addr_cache_roots_mut);
    // #6763: inherited-property resolution retains an owner while an accessor
    // or Proxy trap can re-enter after moving GC. Rewrite that temporary
    // identity so malformed prototype cycles remain bounded.
    reg_scanner!(crate::object::prototype_chain::scan_prototype_resolution_stack_roots_mut,);
    reg_scanner!(crate::map::scan_map_iterator_array_roots_mut);
    reg_scanner!(crate::set::scan_set_iterator_array_roots_mut);
    reg_scanner!(crate::perf_hooks::scan_perf_entries_roots_mut);
    reg_scanner!(crate::v8::scan_v8_promise_hook_roots_mut);
    reg_scanner!(crate::typed_feedback::scan_typed_feedback_roots_mut);
    reg_scanner!(crate::typedarray_props::scan_typed_array_own_props_roots_mut);
    // A typed array's materialized backing ArrayBuffer lives only as a raw
    // address in TYPED_ARRAY_VIEW_META — collectable/stale under a live typed
    // array, which made `subarray` hand back a garbage-length view.
    reg_scanner!(crate::typedarray_view::scan_typed_array_view_meta_roots_mut);
    reg_scanner!(transition_cache_mutable_root_scanner);
    reg_scanner!(crate::object::scan_object_cache_roots_mut);
    reg_scanner!(crate::object::scan_arguments_object_roots_mut);
    // bun:ffi (#6562): the cached FFIType enum object.
    reg_scanner!(crate::bun_ffi::scan_bun_ffi_roots_mut);
    reg_budgeted_scanner!(
        crate::object::scan_class_side_table_roots_mut,
        crate::object::scan_class_side_table_roots_mut_step,
        crate::object::new_class_side_table_root_scan_state,
        MutableRootScannerSource::RuntimeMutableScanner,
    );
    reg_budgeted_scanner!(
        crate::symbol::scan_symbol_side_table_roots_mut,
        crate::symbol::scan_symbol_side_table_roots_mut_step,
        crate::symbol::new_symbol_side_table_root_scan_state,
        MutableRootScannerSource::RuntimeMutableScanner,
    );
    // Issue #1813: the implicit-`this` cell holds the live receiver across a
    // dynamically-dispatched method body. A moving GC triggered from inside
    // that body (e.g. @perryts/mysql Pool.acquire → handshake → nativeScramble
    // under concurrent load) must rewrite the cell, or the body's next
    // `this`-derived dispatch derefs a relocated receiver → SIGSEGV.
    reg_scanner!(crate::object::scan_implicit_this_roots_mut);
    // Connected inspector sessions are retained only by the inspector's
    // thread-local registry while they receive protocol notifications.
    reg_scanner!(crate::node_inspector::scan_inspector_roots_mut);
    // Issue #1790 (epic #1785 class-object dispatch / design #1772): the class
    // static-inheritance side-tables CLASS_PROTOTYPE_OBJECTS and
    // CLASS_PARENT_CLOSURES hold the heap parent (`class Sub extends make(...)`
    // / `extends Context.Tag(..)()`) as a raw `usize` pointer. Root + rewrite
    // them so a parent reachable only through the table survives collection and
    // its address is fixed up after a copying-nursery / evacuation move,
    // keeping `Sub.ast` and inherited static methods resolvable.
    reg_scanner!(crate::object::scan_class_inheritance_roots_mut);
    // #1934: live `child_process.spawn` ChildProcess objects are reachable only
    // from the reactor's registry (the event loop holds no JSValue root for a
    // fire-and-forget spawn). Scan + rewrite them so a GC between ticks doesn't
    // reclaim the object whose `data`/`exit` handlers are still pending.
    reg_scanner!(crate::child_process::reactor::cp_reactor_scan_roots_mut);
    // #6563: live node-pty IPty objects are likewise reachable only from the
    // pty reactor's registry while their onData/onExit handlers are pending.
    #[cfg(unix)]
    reg_scanner!(crate::pty::reactor::pty_reactor_scan_roots_mut);
    // #4911: a bound node:dgram socket is reachable only from the dgram
    // reactor's registry while its recv thread runs; scan + rewrite it so a GC
    // between ticks doesn't reclaim the object whose `message` handlers fire.
    #[cfg(feature = "mod-dgram")]
    reg_scanner!(crate::dgram_reactor::scan_roots_mut);
    reg_scanner!(json_parse_mutable_root_scanner);
    reg_scanner!(intern_table_mutable_root_scanner);
    // #7564: the per-thread `{ value, done }` / `{ done, value }` keys arrays
    // shared by every iterator result the runtime builds. Nothing else in the
    // heap references them — the result objects that use them are short-lived
    // while the cache outlives them — so without this scanner they would be
    // swept and the next `.next()` would install a freed keys array. It also
    // REWRITES: an evacuating collection moves them like any other array, and
    // the thread-local slot is the only place the new address can be recorded.
    reg_scanner!(crate::iter_result::scan_iter_result_keys_roots_mut);
    reg_scanner!(small_int_cache_mutable_root_scanner);
    reg_scanner!(crate::builtins::scan_console_log_singleton_roots_mut);
    reg_scanner!(crate::builtins::scan_boxed_primitive_payload_roots_mut);
    reg_scanner!(crate::weakref::scan_pending_finalization_jobs_roots_mut);
    // #6182: keep the weak-holder registry's stored holder ADDRESSES current
    // across evacuation. Metadata-only (non-rooting) — it rewrites forwarded
    // addresses in rewrite phases and emits nothing during mark, so it never
    // keeps a dead holder alive. Copied-minor liveness/prune is driven by
    // `process_weak_targets_from_registry`; this covers full-cycle currency.
    reg_scanner!(crate::weakref::scan_weak_holders_roots_mut);
    // Issue #841: GC roots for the per-(submodule, export) function
    // singletons + per-submodule namespace stub objects allocated by
    // `node_submodules.rs`. Without this scanner the next GC cycle
    // after first import-binding use would reclaim the singletons
    // (nothing else holds them — they live for the program's lifetime
    // via codegen `getter` calls, not via a user-visible JSValue root).
    reg_scanner!(crate::node_submodules::scan_node_submodule_singleton_roots_mut,);
    // Box-capture root scanner (mutable closure captures, esp. the
    // generator state-machine's `__iter` and `__step` boxes that hold
    // the iter object + step closure across awaits).
    reg_scanner!(crate::r#box::scan_box_roots_mut);
    // Iter-result scratch slot — the async-step fast path stows the
    // generator's most recent yield value here; it stays live until
    // the step driver reads it back.
    reg_scanner!(crate::promise::scan_iter_result_root_mut);
    // Async-step thunk single-slot cache (build_async_step_thunks).
    reg_scanner!(crate::promise::scan_async_step_thunk_cache_mut);
    // Closure singleton caches. Captured-closure cache keys mirror closure
    // capture heap words, so copied-minor must rewrite them after moving
    // captured young values or future cache hits miss on stale addresses.
    reg_scanner!(crate::closure::scan_singleton_closure_roots_mut);
    reg_scanner!(crate::closure::scan_closure_dynamic_props_roots_mut);
    reg_scanner!(crate::buffer::scan_buffer_own_props_roots_mut);
    // Generic per-handle expando properties (`blob.colors = [...]` and other
    // arbitrary own props on native HANDLE values). Keys are stable small handle
    // ids; only the stored VALUES are JS references that must be traced.
    reg_scanner!(crate::object::handle_expando::scan_handle_expando_roots_mut);
    // Native-module callable export singletons and process stdio stream
    // singletons store heap pointers in TLS caches; keep them live and rewrite
    // them if a copying collection moves their backing allocations.
    reg_scanner!(crate::object::scan_native_callable_export_roots_mut);
    reg_scanner!(crate::object::scan_class_capture_value_roots_mut);
    reg_scanner!(crate::node_vm::scan_vm_roots_mut);
    // #6559: the dyn-eval interpreter's rooted value stack (environments,
    // temporaries, arguments of in-flight interpreted frames). Mark +
    // REWRITE — interpreter state must survive moving collections triggered
    // from inside interpreted code.
    #[cfg(feature = "dyn-eval")]
    reg_scanner!(crate::dyn_eval::scan_dyn_eval_roots_mut);
    reg_scanner!(crate::tls::scan_tls_roots_mut);
    reg_scanner!(crate::process::scan_process_finalization_roots_mut);
    reg_scanner!(crate::process::scan_process_module_loader_roots_mut);
    // #7231: the materialize-once `process.*` caches. Each is a thread-local
    // cell holding a NURSERY-allocated object that nothing else refers to —
    // `process.env` / `.permission` / `.report` are getter CALLS, not fields
    // of the `process` object, so the cache is the whole reference graph.
    // `scan_process_finalization_roots_mut` above is the identical idiom and
    // was already registered; these three were an omission, not a design.
    // `CACHED_ENV` is the load-bearing one: `process.env` is touched by nearly
    // every real Node program, and every `process.env.X = v` after the first
    // collection wrote through a dangling pointer.
    reg_scanner!(crate::process::scan_process_env_cache_roots_mut);
    reg_scanner!(crate::process::scan_permission_cache_roots_mut);
    reg_scanner!(crate::process::scan_report_cache_roots_mut);
    // #7231: the raw `Error` constructor address behind
    // `Error.prepareStackTrace`. The closure is reachable through `globalThis`
    // so it is not swept, but this duplicate lives outside the object graph
    // and goes stale on a move.
    reg_scanner!(crate::object::scan_error_constructor_root_mut);
    // #7231: native callback slots that bypass their rooted sibling
    // structures. `RESIZE_CALLBACK` bypasses the EventEmitter listener array;
    // `FRAME_CALLBACKS` is rooted only transiently by a `RuntimeHandleScope`
    // during registration; `INPUT_HANDLER` holds the `useInput` arrow, which
    // in idiomatic inline form has no other reference at all.
    reg_scanner!(crate::tty::scan_tty_resize_callback_root_mut);
    reg_scanner!(crate::frame::scan_frame_callback_roots_mut);
    reg_scanner!(crate::tui::input::scan_tui_input_handler_root_mut);
    // #7231: three in-flight cells that hold a NaN-boxed heap value across a
    // window in which user code can run. Each is a second copy of a value
    // whose original is rooted elsewhere, or the only copy for the length of
    // the window; both shapes are the #7226 `prev_this` family. Rooting the
    // CELL is the half a scanner can close — the displaced value each
    // save/restore idiom parks in a bare Rust local is noted at each
    // declaration and needs `RuntimeHandleScope` plumbing, not a scanner.
    reg_scanner!(crate::object::scan_current_new_target_root_mut);
    reg_scanner!(crate::object::scan_accessor_receiver_override_root_mut);
    reg_scanner!(crate::object::scan_pending_fetch_signal_root_mut);
    reg_scanner!(crate::os::scan_process_event_listener_roots_mut);
    // #6077: keep promises tracked for an unhandled rejection alive + address-
    // stable until reported, so the program-end report is not a stale/UAF read.
    reg_scanner!(crate::promise::scan_unhandled_rejection_roots_mut);
    reg_scanner!(crate::os::scan_process_stream_singleton_roots_mut);
    reg_scanner!(crate::fs::scan_fs_handle_roots_mut);
    reg_scanner!(crate::fs::scan_fs_stream_roots_mut);
    reg_scanner!(crate::fs::scan_fs_watcher_roots_mut);
    #[cfg(feature = "full")]
    reg_scanner!(crate::plugin::scan_plugin_roots_mut);
    reg_scanner!(crate::geisterhand_registry::scan_geisterhand_roots_mut);
    reg_scanner!(crate::ui_text_registry::scan_ui_text_registry_roots_mut);
    // perry/tui hook + state slot pools — they store raw NaN-boxed
    // value bits but the GC has no other way to know which slots hold
    // heap pointers (arrays/objects/strings stashed via setState /
    // useState / useRef). #679 follow-up: pre-fix, an Enter-press in
    // the perry-code demo stored a freshly-concat'd messages array,
    // the next allocation triggered minor GC, and the array was
    // reclaimed because nothing else held it — `messages.map(…)` on
    // the stale pointer produced an empty render.
    reg_budgeted_scanner!(
        crate::tui::hooks::scan_hook_slot_roots_mut,
        crate::tui::hooks::scan_hook_slot_roots_mut_step,
        crate::tui::hooks::new_hook_slot_root_scan_state,
        MutableRootScannerSource::RuntimeMutableScanner,
    );
    reg_budgeted_scanner!(
        crate::tui::state::scan_state_slot_roots_mut,
        crate::tui::state::scan_state_slot_roots_mut_step,
        crate::tui::state::new_state_slot_root_scan_state,
        MutableRootScannerSource::RuntimeMutableScanner,
    );
    #[cfg(feature = "ohos-napi")]
    reg_scanner!(crate::arkts_callbacks::arkts_callbacks_root_scanner_mut);
}

#[no_mangle]
pub extern "C" fn js_gc_init() {
    // Parse LLVM stack-map metadata before the first collection. The parser
    // allocates its immutable index once; root scans themselves must remain
    // allocation-free while the collector owns the heap.
    initialize_stack_maps();
    // Windows: opt console stdout/stderr into VT/ANSI escape processing
    // once at program start so runtime-emitted escapes (console.clear, tty
    // cursor ops, color output keyed off isTTY) render instead of printing
    // literally. No-op for piped/redirected streams; a failing
    // SetConsoleMode is ignored — this never fails startup. Idempotent, so
    // a second js_gc_init on another thread is harmless.
    #[cfg(windows)]
    crate::win_console::enable_vt_output();
    // #6882/#7450: macOS decodes mimalloc's default VM tag (100) as
    // `IOAccelerator`, so the whole JS heap renders as GPU-driver memory in
    // vmmap/Instruments/`footprint`. The retag to tag 240 that fixes this
    // cannot live here — mimalloc reserves its 1 GiB arena during Rust's
    // pre-`main` startup and every later allocation just commits pages inside
    // that already-tagged mapping, so a retag at `js_gc_init` time reaches
    // nothing (#7450). It now runs from a `__DATA,__mod_init_func`
    // constructor; this call keeps that constructor in the link and re-applies
    // the option idempotently. See `crate::mimalloc_os_tag`.
    crate::mimalloc_os_tag::ensure_mimalloc_os_tag_applied();
    crate::node_submodules::diagnostics_channel_init_main_thread();
    crate::node_submodules::init_trace_events_runtime();
    // #5093: force every class-field access back through the full guard call —
    // i.e. disable the codegen-inlined fast path — when:
    //   - typed-feedback tracing is on (the guard observes every access), or
    //   - the intact-bit verifier is on (`PERRY_VERIFY_TYPED_INTACT`): the
    //     verifier lives in the guard's fast contract, so inline hits would skip
    //     it; disabling the inline path routes every access through it, or
    //   - the explicit escape hatch `PERRY_DISABLE_CLASS_FIELD_INLINE` is set to
    //     a truthy value (perf bisection / A-B measurement). `=0`/`=false`/`=off`
    //     leave the fast path enabled.
    if crate::typed_feedback::typed_feedback_active()
        || env_flag_enabled("PERRY_VERIFY_TYPED_INTACT")
        || env_flag_enabled("PERRY_DISABLE_CLASS_FIELD_INLINE")
    {
        crate::object::disable_class_field_inline_guard();
    }
    gc_init();
}

/// Release external Map/Set/JSON-tape storage owned by the current thread.
///
/// This is intentionally narrower than a general heap teardown: the arena
/// headers remain owned by the arena, while the side-allocation registries own
/// the separately allocated buffers. The operation is idempotent and is called
/// only once no more JavaScript work can run on this thread.
///
/// ★ It is also where the **schedule liveness verdict** is emitted (#7604).
/// Codegen calls this exactly once, at the real process-exit boundary after
/// every exit callback (`codegen/entry.rs`), which is the one point in a
/// compiled program where "what did this run actually exercise" is answerable.
/// See `emit_schedule_liveness_verdict`.
#[no_mangle]
pub extern "C" fn js_gc_release_current_thread_collection_side_allocations() {
    crate::map::release_current_thread_map_side_allocations();
    crate::json_tape_store::release_current_thread_lazy_tapes();
    crate::set::release_current_thread_set_side_allocations();
    // Every process-exit path funnels through here — the generated exit
    // epilogue, `js_process_exit`, and the fatal-path teardown — and perry's own
    // exits call `_exit`, so `atexit` alone would not see them. Print the seeded
    // GC-schedule summary here so a *passing* run still reports how many
    // safepoints the schedule actually saw. Inert (one cached-`Option` load) and
    // once-only when the mode is off.
    schedule::report_exit_summary();
    emit_incremental_liveness_diag();
    emit_schedule_liveness_verdict();
}

/// `PERRY_GC_DIAG=1`: what the INCREMENTAL collector charged this run, whether
/// or not any cycle completed (#7909).
///
/// Every other GC diagnostic is emitted per completed cycle, so a run that
/// starts a budgeted cycle and never finishes it prints nothing at all — the
/// `asyncpipe` shape, where the collector's own output is empty while a third
/// of the leaf profile is collector machinery. `cycle_starts > completions`
/// with a large `steps` is exactly that state, and it is only visible here.
fn emit_incremental_liveness_diag() {
    if !telemetry::gc_diag_enabled() {
        return;
    }
    if !crate::native_handle::is_main_thread_or_unrecorded() {
        return;
    }
    static EMITTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if EMITTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let (reentrant, no_trigger, start_blocked, resume_blocked) = instruments::budgeted_step_skips();
    let (blocked_alloc, blocked_unsafe_zone, blocked_root_lock) =
        instruments::moving_safepoints_blocked_by_other_guards();
    eprintln!(
        "[gc-incremental] cycle_starts={} steps={} completions={} active_at_exit={} \
         mark_barrier_arms={} mark_barrier_armed_us={} \
         skips(reentrant={reentrant} no_trigger={no_trigger} start_blocked={start_blocked} \
         resume_blocked={resume_blocked} nursery_cap_deferred={}) \
         safepoints_blocked_by_budgeted={} \
         safepoints_blocked(in_alloc={blocked_alloc} unsafe_zone={blocked_unsafe_zone} \
         root_lock={blocked_root_lock}) \
         copying_minors={} loop_polls={} poll_arm_events={} \
         poll_armed_at_exit={}",
        instruments::incremental_cycle_starts(),
        instruments::incremental_steps(),
        instruments::incremental_completions(),
        policy::gc_budgeted_cycle_active(),
        instruments::mark_barrier_arm_events(),
        instruments::mark_barrier_armed_us(),
        instruments::budgeted_step_nursery_cap_deferrals(),
        instruments::moving_safepoints_blocked_by_budgeted(),
        instruments::copying_minor_cycles(),
        instruments::loop_polls_reached(),
        poll_arm::poll_arm_events(),
        poll_arm::poll_armed_count(),
    );
    emit_step_bounds_diag();
}

/// What the "time-budgeted" collector actually cost, as opposed to what it was
/// asked to cost (#7903).
///
/// `js_gc_step_us` and mutator assist can only consult the clock BETWEEN work
/// units, so a budget is only as good as the largest single unit. These are the
/// measured maxima plus the liveness counters for the sliced weak path:
///
/// * `step_max_us` — longest single budgeted step.
/// * `final_remark_max_us` / `final_remarks` — the deliberately ATOMIC phase,
///   reported separately so a heap-sized pause cannot hide inside the general
///   maximum.
/// * `weak_records` / `weak_max_records_per_step` — FinalizationRegistry
///   records scanned, and the worst single step's share of them. Before #7903
///   one registry was one work unit, so this maximum was the whole registry.
/// * `weak_steps_sliced` — steps that ended PARTWAY THROUGH a registry. **This
///   is the subject-was-live counter**: a run reporting zero has not exercised
///   the sliced path, whatever else it reports. A NONZERO value proves less
///   than it looks — a step can end mid-registry at the entry park, before any
///   record is scanned — so pair it with `weak_max_records_per_step`, which is
///   what actually distinguishes a sliced array from a swallowed one.
/// * `weak_registry_restarts` / `weak_registry_atomic_finishes` — cursors
///   invalidated by mutator restructuring, and the bounded fallback taken when
///   one registry exhausted its restart budget.
fn emit_step_bounds_diag() {
    eprintln!(
        "[gc-step-bounds] step_max_us={} final_remark_max_us={} final_remarks={} \
         weak_records={} weak_max_records_per_step={} weak_steps_sliced={} \
         weak_registry_restarts={} weak_registry_atomic_finishes={}",
        instruments::step_max_us(),
        instruments::final_remark_max_us(),
        instruments::final_remark_count(),
        instruments::weak_records_scanned(),
        instruments::weak_max_records_per_step(),
        instruments::weak_steps_sliced(),
        instruments::weak_registry_restarts(),
        instruments::weak_registry_atomic_finishes(),
    );
}

/// Print what the rate-1 schedule endpoint actually did, and **fail the
/// process** when the answer is "nothing" (#7604).
///
/// This is the "assert the subject was live" rule turned on the instrument
/// itself. A rate-1 run that forced zero collections, or whose every forced
/// collection was escalated to a non-moving full mark-sweep, has exercised
/// nothing — and before #7604 it exited 0 and looked exactly like a run that
/// had. That is the fourth way a gate cannot fail, applied to a debug knob
/// whose entire purpose is to make a class of bug reproducible.
///
/// Exiting non-zero rather than warning is deliberate. The schedule is never on
/// in production — the whole knob is debug-only, off by default, and set by
/// hand or by a CI stress arm. In both of those contexts a vacuous run is a
/// result the operator must not be allowed to read as a pass. Sub-endpoint
/// rates get no verdict (a sparse sample legitimately forcing nothing is not a
/// broken instrument); their liveness counters are in the exit-summary line
/// above.
///
/// Known limitation, stated rather than hidden: `process.exit()` terminates via
/// `libc::_exit` and never reaches this boundary, so a run that ends that way
/// gets no verdict. An uncaught throw is the same. Both already bypass every
/// other exit callback.
fn emit_schedule_liveness_verdict() {
    // Same discipline as `report_exit_summary`, for the same reason: every
    // thread routes through the teardown funnel, the counters are
    // process-global, and a worker tearing down first would judge — and at
    // rate 1 `exit(70)` on — counts that are not yet final. Main-thread-only
    // (via the TLS-free OS-id compare) and once-only.
    if !crate::native_handle::is_main_thread_or_unrecorded() {
        return;
    }
    static VERDICT_EMITTED: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);
    if VERDICT_EMITTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    match schedule_liveness_report() {
        None => {}
        Some(Ok(summary)) => eprintln!("{summary}"),
        Some(Err(complaint)) => {
            eprintln!("{complaint}");
            std::process::exit(70);
        }
    }
}

/// #5093 semantics as a **pure** function of the raw value, so both directions
/// can be pinned by a test without touching the process environment (the live
/// readers cache in a `OnceLock`; a test that called `set_var` would be at the
/// mercy of which test ran first, and `set_var` is process-wide — see the
/// `knob_overrides` note above for what that cost us once already).
///
/// True for `1`/`true`/`on`/`yes` (case-insensitive, surrounding whitespace
/// ignored). False for unset, `0`/`false`/`off`/`no`, the empty string, **and
/// anything unrecognised** — a typo must not silently arm an instrument.
///
/// #7991: this is the single definition of "boolean-ish GC knob". Every GC knob
/// that is a boolean must route through it. `scripts/check_gc_env_knobs.py`
/// enforces that by rejecting presence-only reads (`var_os(..).is_some()`) of
/// GC-family names in production code.
pub(crate) fn env_flag_from_value(raw: Option<&str>) -> bool {
    match raw {
        Some(v) => matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "on" | "yes"
        ),
        None => false,
    }
}

/// #5093: parse a boolean-ish env var by value (not mere presence).
/// See [`env_flag_from_value`] for the exact contract.
pub(crate) fn env_flag_enabled(name: &str) -> bool {
    env_flag_from_value(std::env::var(name).ok().as_deref())
}

/// The mirror of [`env_flag_from_value`] for a **default-ON kill switch**:
/// the feature is ON for unset, for the empty string, and for anything
/// unrecognised; OFF only for an explicit `0`/`off`/`false`/`no`
/// (case-insensitive, surrounding whitespace ignored).
///
/// This is deliberately **not** `!env_flag_from_value(..)`. Both helpers fail
/// toward their knob's documented default, which is the opposite direction in
/// each case: a typo must neither arm an instrument that is off by default nor
/// disable a collector feature that ships on.
pub(crate) fn env_default_on_from_value(raw: Option<&str>) -> bool {
    match raw {
        Some(v) => !matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "0" | "off" | "false" | "no"
        ),
        None => true,
    }
}

/// Read a default-ON kill switch from the environment.
/// See [`env_default_on_from_value`] for the exact contract.
pub(crate) fn env_default_on_enabled(name: &str) -> bool {
    env_default_on_from_value(std::env::var(name).ok().as_deref())
}

/// FFI: get GC stats
#[no_mangle]
pub extern "C" fn js_gc_stats(
    out_collections: *mut u64,
    out_freed: *mut u64,
    out_pause_us: *mut u64,
) {
    GC_STATS.with(|stats| {
        let stats = stats.borrow();
        unsafe {
            if !out_collections.is_null() {
                *out_collections = stats.collection_count;
            }
            if !out_freed.is_null() {
                *out_freed = stats.total_freed_bytes;
            }
            if !out_pause_us.is_null() {
                *out_pause_us = stats.last_pause_us;
            }
        }
    });
}

/// FFI: always-on pause observability (#6187, 2026-07-09 audit). Fills the
/// max pause since thread start, the max and mean over the recent-pause
/// ring (`GC_RECENT_PAUSE_WINDOW` samples), and how many samples the ring
/// currently holds. Cheap enough for a UI frame scheduler to poll per tick.
#[no_mangle]
pub extern "C" fn js_gc_pause_stats(
    out_max_us: *mut u64,
    out_recent_max_us: *mut u64,
    out_recent_avg_us: *mut u64,
    out_recent_count: *mut u64,
) {
    GC_STATS.with(|stats| {
        let stats = stats.borrow();
        let n = stats.recent_len as usize;
        let window = &stats.recent_pauses_us[..n.min(GC_RECENT_PAUSE_WINDOW)];
        let recent_max = window.iter().copied().max().unwrap_or(0);
        let recent_avg = if window.is_empty() {
            0
        } else {
            window.iter().copied().sum::<u64>() / window.len() as u64
        };
        unsafe {
            if !out_max_us.is_null() {
                *out_max_us = stats.max_pause_us;
            }
            if !out_recent_max_us.is_null() {
                *out_recent_max_us = recent_max;
            }
            if !out_recent_avg_us.is_null() {
                *out_recent_avg_us = recent_avg;
            }
            if !out_recent_count.is_null() {
                *out_recent_count = n as u64;
            }
        }
    });
}

/// Stamp a freshly built, runtime-owned keys array as copy-on-write shared.
///
/// The `*mut GcHeader` cast lives HERE, in `gc/`, rather than at the call site.
/// `scripts/addr_class_inventory.py` refuses a bare `as *mut GcHeader` outside
/// `gc/` and `value/addr_class.rs`, and every one of the ~126 grandfathered
/// entries in `scripts/addr_class_allowlist.txt` carries the same promise —
/// "migrate to a helper in a follow-up". This is that helper for the one thing
/// those call sites actually do: set a flag.
///
/// `addr_class::try_read_gc_header` cannot serve them, and that is not an
/// oversight — it returns `&'static GcHeader`, a SHARED reference, precisely so
/// that a probe of an untrusted address can never write through it. A flag
/// write needs `*mut`, so it needs a separate, narrower entry point with a
/// stronger precondition, which is what this is.
///
/// # Safety
/// `user_ptr` must be the user pointer of a live GC object this thread has just
/// allocated — never an address decoded from a NaN-box payload. That is the
/// same discipline the arena walkers are allowlisted under: an address obtained
/// from allocation or block iteration cannot be in the handle band, so there is
/// nothing for `try_read_gc_header`'s band check to reject.
#[inline]
pub(crate) unsafe fn mark_shape_shared(user_ptr: *mut u8) {
    let header = layout::header_from_user_ptr(user_ptr);
    (*header).gc_flags |= GC_FLAG_SHAPE_SHARED;
}

#[cfg(test)]
mod tests;

/// Crate-wide handle on the GC test-isolation lock — see
/// `tests::support::copying_nursery_isolation_lock`. Any test OUTSIDE the gc
/// module that populates-then-asserts a process-global side table (e.g.
/// `CLOSURE_PROPS`) must hold this, or the gc test guards' global state reset
/// on a parallel test thread can wipe its entries mid-test.
#[cfg(test)]
pub(crate) use tests::support::copying_nursery_isolation_lock as global_side_table_test_lock;
