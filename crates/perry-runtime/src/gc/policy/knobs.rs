//! Collector mode knobs read from the environment (moving safepoints,
//! incremental marking, moving loop polls, GC tracing) and the test guards
//! that pin them.

use super::*;

#[inline]
/// The moving (copying) minor at the precise-root event-loop safepoint —
/// **Perry's default GC.** At the outermost microtask-pump boundary the JS stack
/// has unwound, so the copying minor runs with precise, rewritable roots and
/// moves survivors (compacting, O(survivors), no sweep). `PERRY_GC_MOVING_SAFEPOINT=0`
/// is a kill switch that reverts to the non-moving path (for bisecting a
/// regression while we harden). Making the moving minor primary INSIDE loops
/// (back-edge polls + alloc-point deferral) is the separate opt-in
/// `PERRY_GC_MOVING_LOOP_POLLS` — off by default because the poll defeats loop
/// vectorization until it is emitted only for allocating loops.
pub(crate) fn gc_moving_safepoint_enabled() -> bool {
    static CACHED: OnceLock<bool> = OnceLock::new();
    // Default ON; the kill switch is an explicit `=0`/`off`/`false`.
    *crate::once_init::get_or_init(&CACHED, || {
        super::env_default_on_enabled("PERRY_GC_MOVING_SAFEPOINT")
    })
}

/// Phase 4 of the moving-GC project: gate the INCREMENTAL old-gen collector (the
/// budgeted stepper). **DEFAULT ON since the #6180 flip**; `PERRY_GC_INCREMENTAL=0`
/// (or `off`/`false`) is the kill switch. Perry has a full budgeted
/// mark/sweep stepper which, before that flip, never ran: every compiled program
/// registers unbudgeted mutable root scanners and
/// `registered_root_scanners_block_budgeted_gc()` blocked the cycle from ever
/// starting. When this is on, the stepper is allowed to start and runs those
/// unbudgeted scanners SYNCHRONOUSLY in its initial root-scan step (a bounded
/// initial-mark pause), then marks/sweeps the old gen incrementally across
/// safepoints — the standard "initial-mark + incremental-mark" design. Off ⇒
/// exactly the non-incremental GC (the whole path is skipped). Independent
/// of `PERRY_GC_MOVING_SAFEPOINT`; this is the concurrency layer that reduces
/// old-gen pause time.
///
/// ★ This default is load-bearing for rooting arguments, not just for pause
/// times: with the stepper on, budgeted cycles skip the conservative
/// stack-scan subphase *structurally* (`gc/cycle.rs`, classifier mode), so a
/// compiled program completes precise-roots-only cycles in its shipped
/// configuration. Reasoning that assumes "the automatic arms force a
/// conservative scan" is therefore wrong by default — that mistake was made in
/// #6972 while this doc comment still claimed the gate was off (#6987).
pub(crate) fn gc_incremental_enabled() -> bool {
    static CACHED: OnceLock<bool> = OnceLock::new();
    *crate::once_init::get_or_init(&CACHED, || {
        // DEFAULT ON (#6180 flip): ordinary allocation pressure is collected
        // by the budgeted incremental stepper — debt-paced assists, sound
        // across mutator windows (mark barrier + allocate-black + final
        // remark + drain-before-synchronous), census-free, RSS-parity on
        // realistic workloads with ~5x lower worst pause. Bounded pauses by
        // default; the synchronous collector remains for manual gc(),
        // emergency reclaim, and as the PERRY_GC_INCREMENTAL=0 escape hatch
        // (bisection / max-throughput batch workloads).
        super::env_default_on_enabled("PERRY_GC_INCREMENTAL")
    })
}

/// Make the moving minor PRIMARY inside loops: defer the alloc-point nursery
/// collection to a codegen loop back-edge poll (`js_gc_loop_safepoint`) instead
/// of collecting non-moving mid-expression, so reallocation-heavy loops evacuate
/// (bounded RSS) instead of leaking.
///
/// **DEFAULT ON.** The kill switch is `PERRY_GC_MOVING_LOOP_POLLS=0`/`off`/`false`.
/// See [`moving_loop_polls_enabled_from_env`] for the decision and its evidence;
/// #7161's stopgap default-OFF (pending #7154) is discharged there.
///
/// MUST match codegen `moving_safepoint_polls_enabled` (same env) so the deferral
/// and the polls that drain it stay coherent — a runtime default that disagrees
/// with the codegen default would defer collections that never drain (or drain
/// collections that were never deferred). That disagreement is not hypothetical:
/// it shipped. #7690 wrote the default-ON argument into the doc below and left
/// both bodies matching `1|on|true`, so the runtime deferred nursery pressure to
/// a safepoint codegen never emitted. Combined with #7687 (the alloc-point minor
/// must not move), the shipped collector had NO nursery evacuation at all —
/// `churn_alloc` ran 13 whole-arena full collections where it had run 105 copying
/// minors, and `tree` spent 4.1 s of its 5.1 s wall in GC pause. Both predicates
/// are now pinned by tests, and `polls_default_matches_codegen_mirror` pins that
/// they agree.
pub(crate) fn gc_moving_loop_polls_enabled() -> bool {
    // Test-only mode override (see `force_legacy_gc_pacing`). Consulted BEFORE
    // the process-wide OnceLock so a single test can pin a specific pacing mode
    // for its duration even though the process default is off. Compiled out
    // entirely in release builds.
    #[cfg(test)]
    if let Some(forced) = GC_MOVING_LOOP_POLLS_TEST_OVERRIDE.with(TriggerInput::get) {
        return forced;
    }

    static CACHED: OnceLock<bool> = OnceLock::new();
    *crate::once_init::get_or_init(&CACHED, || {
        moving_loop_polls_enabled_from_env(
            std::env::var("PERRY_GC_MOVING_LOOP_POLLS").ok().as_deref(),
        )
    })
}

/// Pure env→enable decision for the moving-loop minor, factored out so the
/// default is unit-testable without touching process env / the cached `OnceLock`.
///
/// **Default ON since #7682.** The kill switch is `0`/`off`/`false`; anything
/// else, including unset, selects the moving-loop path. Codegen's
/// `moving_safepoint_polls_enabled` mirrors this exactly (same env, same
/// predicate) — they MUST agree, or a deferred collection has no drain.
///
/// #7161 flipped this OFF as a stopgap, and named both conditions for putting
/// it back. Both are met:
///
///  * **Its correctness reason is closed.** #7161's own title is "pending
///    #7154"; #7154 closed on 2026-08-01. The class it belongs to now has a
///    static gate (`gc-root-dominance.yml` over
///    `scripts/gc_root_dominance_corpus.sh`) whose allowlist is EMPTY, so a new
///    instance is a red build rather than a field report.
///  * **Its codegen-quality reason is discharged.** The other half of the
///    stopgap was that a poll at every back-edge defeats auto-vectorization;
///    `emit_gc_loop_safepoint` now emits one only where
///    `loop_purity::loop_may_allocate` says the body can allocate, so
///    numeric/vectorizable loops stay call-free. A loop that cannot allocate
///    cannot arm a trigger, so skipping it there is not a coverage hole.
///
/// And leaving it off had become the more dangerous state, which is the actual
/// reason this moves now. Nursery pressure has exactly two precise collection
/// points — this poll and the outermost microtask-pump boundary — and a
/// compute-only program reaches neither with polls off. Every nursery
/// collection therefore happened at the register-imprecise allocation point,
/// where #7682 showed it must not move. So "polls off" does not mean "collect
/// later, precisely"; it means "never collect precisely at all".
///
/// #7690 wrote every paragraph above and then did not change this line, and
/// nothing failed — the function was factored out expressly to make the default
/// "unit-testable without touching process env", and no test ever pinned it in
/// either direction. `polls_default_is_on` and its codegen mirror exist so that
/// the next edit to this predicate has to be deliberate.
pub(crate) fn moving_loop_polls_enabled_from_env(value: Option<&str>) -> bool {
    !matches!(value, Some("0") | Some("off") | Some("false"))
}

#[cfg(test)]
thread_local! {
    /// Test-only override for [`gc_moving_loop_polls_enabled`]. When `Some(v)`,
    /// the getter returns `v` before consulting the process-wide OnceLock. This
    /// is the ONLY way a unit test can select GC pacing mode per-test: the
    /// OnceLock caches the env-derived default once for the whole process, so
    /// the entire test binary otherwise runs in a single mode. Because the
    /// nursery-cap in `effective_next_arena_trigger`, the alloc-point routing in
    /// `gc_check_trigger`, and the eager malloc-registry build in
    /// `CopyingPointerSet::new` all consult `gc_moving_loop_polls_enabled()`
    /// (and `gc_scavenge_enabled()` is env-gated OFF by default in tests), this
    /// single override flips all of the moving-mode behavior coherently.
    static GC_MOVING_LOOP_POLLS_TEST_OVERRIDE: TriggerInput<Option<bool>> = const { TriggerInput::new(None) };
}

/// RAII guard that pins LEGACY (non-moving, budgeted/direct, 128 MiB-ceiling) GC
/// pacing for the tests that assert the budgeted/direct pacer + trigger
/// arithmetic — the mechanism that, since the moving-nursery default-on flip,
/// lives behind the `PERRY_GC_MOVING_LOOP_POLLS=0` kill switch rather than the
/// default path. Restores the previous override state on drop. Test-only.
#[cfg(test)]
pub(in crate::gc) struct LegacyGcPacingGuard {
    previous: Option<bool>,
    cap_previous: bool,
    scavenge_previous: Option<bool>,
}

#[cfg(test)]
impl Drop for LegacyGcPacingGuard {
    fn drop(&mut self) {
        GC_MOVING_LOOP_POLLS_TEST_OVERRIDE.with(|cell| cell.set(self.previous));
        GC_NURSERY_CAP_TEST_SUPPRESSED.with(|cell| cell.set(self.cap_previous));
        super::GC_SCAVENGE_TEST_OVERRIDE.with(|cell| cell.set(self.scavenge_previous));
    }
}

/// Pin moving-loop polls ON for the duration of the returned guard, so a test
/// can drive `js_gc_loop_safepoint` without depending on the process-wide
/// default (which the `OnceLock` fixes from the environment once per test
/// binary — exactly the ambient dependency #7728's own regression hid behind).
#[cfg(test)]
pub(in crate::gc) struct MovingLoopPollsGuard(Option<bool>);

#[cfg(test)]
impl MovingLoopPollsGuard {
    pub(in crate::gc) fn on() -> Self {
        Self(GC_MOVING_LOOP_POLLS_TEST_OVERRIDE.with(|cell| cell.replace(Some(true))))
    }
}

#[cfg(test)]
impl Drop for MovingLoopPollsGuard {
    fn drop(&mut self) {
        GC_MOVING_LOOP_POLLS_TEST_OVERRIDE.with(|cell| cell.set(self.0));
    }
}

/// Pin legacy GC pacing (moving-loop polls OFF) for the duration of the returned
/// guard. See [`LegacyGcPacingGuard`] and [`gc_moving_loop_polls_enabled`].
#[cfg(test)]
pub(in crate::gc) fn force_legacy_gc_pacing() -> LegacyGcPacingGuard {
    let previous = GC_MOVING_LOOP_POLLS_TEST_OVERRIDE.with(|cell| {
        let previous = cell.get();
        cell.set(Some(false));
        previous
    });
    // Legacy pacing now means THREE things, not one. Pinning the polls flag
    // used to be sufficient because both the nursery cap and the deferral
    // branch hung off it; #7056 made the cap unconditional and scavenge
    // default-on, so a guard that only touched the polls flag silently stopped
    // pinning anything. That is what broke 23 gc:: tests — they were correct,
    // and their guard had quietly become a no-op.
    let cap_previous = GC_NURSERY_CAP_TEST_SUPPRESSED.with(|cell| cell.replace(true));
    let scavenge_previous = super::GC_SCAVENGE_TEST_OVERRIDE.with(|cell| cell.replace(Some(false)));
    LegacyGcPacingGuard {
        previous,
        cap_previous,
        scavenge_previous,
    }
}

/// Pin moving GC pacing (moving-loop polls ON) for the duration of the returned
/// guard — the shipped default, pinned explicitly so a #7148 deferral test
/// asserts against a *declared* pacing mode instead of inheriting whatever the
/// process-wide `PERRY_GC_MOVING_LOOP_POLLS` OnceLock resolved to. A test that
/// silently ran under legacy pacing would find the deferral branch dead and
/// pass for the wrong reason.
#[cfg(test)]
pub(in crate::gc) fn force_moving_gc_pacing() -> LegacyGcPacingGuard {
    let previous = GC_MOVING_LOOP_POLLS_TEST_OVERRIDE.with(|cell| {
        let previous = cell.get();
        cell.set(Some(true));
        previous
    });
    let cap_previous = GC_NURSERY_CAP_TEST_SUPPRESSED.with(|cell| cell.replace(false));
    let scavenge_previous = super::GC_SCAVENGE_TEST_OVERRIDE.with(|cell| cell.replace(Some(true)));
    LegacyGcPacingGuard {
        previous,
        cap_previous,
        scavenge_previous,
    }
}

/// Pin the one pacing combination in which nursery pressure reaches the DIRECT
/// allocation-point minor: moving-loop polls OFF (so nothing defers) and
/// scavenge ON (so the arm in `gc_check_trigger` is open at all).
///
/// **This is the `PERRY_GC_MOVING_LOOP_POLLS=0` kill-switch configuration, and
/// it is deliberately NOT called "shipped default" any more.** It *was* the
/// shipped default — polls OFF since #7161, scavenge ON since #7056 — and it is
/// the combination #7682 was found in. The follow-up that turned polls back ON
/// made that name a lie in the same PR that introduced it, which is the kind of
/// stale claim this whole line of work is about. A test naming this guard is
/// asserting something about the kill switch; a test that wants the default
/// must take no pacing guard at all.
///
/// The third combination is what needed a guard in the first place.
/// [`force_legacy_gc_pacing`] pins polls OFF *and* scavenge OFF;
/// [`force_moving_gc_pacing`] pins both ON. Every test in this crate therefore
/// declared a pacing mode in which the two flags AGREED — and the
/// alloc-point/deferral interaction that broke is precisely the one where they
/// disagree: scavenge routes nursery pressure to the direct alloc-point minor,
/// while the deferral that was supposed to move that collection to a precise
/// safepoint is gated on the polls flag.
#[cfg(test)]
pub(in crate::gc) fn force_alloc_point_minor_pacing() -> LegacyGcPacingGuard {
    let previous = GC_MOVING_LOOP_POLLS_TEST_OVERRIDE.with(|cell| cell.replace(Some(false)));
    let cap_previous = GC_NURSERY_CAP_TEST_SUPPRESSED.with(|cell| cell.replace(false));
    let scavenge_previous = super::GC_SCAVENGE_TEST_OVERRIDE.with(|cell| cell.replace(Some(true)));
    LegacyGcPacingGuard {
        previous,
        cap_previous,
        scavenge_previous,
    }
}

pub(in crate::gc) fn gc_trace_enabled() -> bool {
    #[cfg(test)]
    if GC_TRACE_TEST_FORCE.with(Cell::get) {
        return true;
    }

    static CACHED: OnceLock<bool> = OnceLock::new();
    *crate::once_init::get_or_init(&CACHED, || super::env_flag_enabled("PERRY_GC_TRACE"))
}

#[cfg(test)]
thread_local! {
    static GC_TRACE_TEST_FORCE: Cell<bool> = const { Cell::new(false) };
}

#[cfg(test)]
pub(in crate::gc) struct TestGcTraceCaptureGuard {
    previous: bool,
}

#[cfg(test)]
impl TestGcTraceCaptureGuard {
    pub(in crate::gc) fn force_enabled() -> Self {
        let previous = GC_TRACE_TEST_FORCE.with(|force| {
            let previous = force.get();
            force.set(true);
            previous
        });
        clear_test_last_gc_trace_json();
        Self { previous }
    }
}

#[cfg(test)]
impl Drop for TestGcTraceCaptureGuard {
    fn drop(&mut self) {
        GC_TRACE_TEST_FORCE.with(|force| force.set(self.previous));
        clear_test_last_gc_trace_json();
    }
}
