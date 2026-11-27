//! Old-generation reclaim: its pressure band and backoff, the allocation-point
//! valve, and the baseline a full or promoting minor leaves behind.

use super::*;

/// The quantity old-reclaim pressure is measured in: reclaimable old-gen bytes
/// plus external side-buffer bytes (#6010), as `gc_budgeted_due_trigger_probe`
/// and `finish_full_old_reclaim_baseline` compute it.
fn old_reclaim_in_use_bytes() -> usize {
    old_gen_reclaimable_pressure_bytes().saturating_add(external_side_old_reclaim_pressure_bytes())
}

/// #11873: may an owed old reclaim wait for the next precise safepoint?
///
/// Yes until old pressure has grown one more growth band past the point the
/// deferral started — the same band that made the reclaim due, so a program
/// that never reaches a safepoint holds at most about two bands of old garbage.
pub(in crate::gc) fn old_reclaim_defer_within_slack(
    old_in_use: usize,
    deferred_at: Option<usize>,
    band: usize,
) -> bool {
    match deferred_at {
        None => true,
        Some(base) => old_in_use < base.saturating_add(band),
    }
}

/// #11873: defer an owed old reclaim from this allocation point to the next
/// precise safepoint. Returns false when the slack is spent and the caller must
/// collect here instead (the valve).
fn defer_old_reclaim_to_precise_safepoint() -> bool {
    if GC_OLD_RECLAIM_VALVE_TEST_OVERRIDE.with(|forced| forced.replace(false)) {
        return false;
    }
    let old_in_use = old_reclaim_in_use_bytes();
    let deferred_at = GC_OLD_RECLAIM_DEFER_BASE.with(Cell::get);
    let band =
        gc_old_reclaim_growth_band_bytes(GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(TriggerInput::get));
    if !old_reclaim_defer_within_slack(old_in_use, deferred_at, band) {
        return false;
    }
    if deferred_at.is_none() {
        GC_OLD_RECLAIM_DEFER_BASE.with(|base| base.set(Some(old_in_use)));
        super::diagnostic_hooks::site_trigger_decision("alloc_point_defer", "OldReclaim");
    }
    // Sticky: the safepoint must run the full even if the pressure reading it
    // makes is answered by an earlier arm first.
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(true));
    if !GC_SAFEPOINT_PENDING.with(Cell::get) {
        // The nursery arm measures ITS slack from this base whenever the poll
        // is armed, so arming without it would hand that arm a stale base.
        GC_SAFEPOINT_DEFER_ARENA_BASE.with(|base| base.set(crate::arena::arena_total_bytes()));
        set_safepoint_pending(true);
    }
    true
}

/// The allocation-point arm of an owed old reclaim (#11873): defer it to the
/// next precise safepoint, or past the slack run it here as the valve, behind
/// the conservative scan. Out of line and cold, so the trigger evaluation every
/// allocation slow path runs keeps its pre-#11873 shape.
#[cold]
#[inline(never)]
pub(super) fn old_reclaim_at_allocation_point() {
    if gc_moving_loop_polls_enabled() && defer_old_reclaim_to_precise_safepoint() {
        return;
    }
    let _reentry = OldReclaimReentryGuard::enter();
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(false));
    GC_OLD_RECLAIM_DEFER_BASE.with(|base| base.set(None));
    super::diagnostic_hooks::site_trigger_decision("alloc_point", "OldReclaim");
    super::diagnostic_hooks::site_set_full_site("alloc_point_old_reclaim");
    let probe = super::diagnostic_hooks::CollectionCharge::begin();
    let _scan = super::roots::ManualGcScanGuard::force_full_scan(
        super::ConservativeScanSite::OldReclaimAllocPoint,
    );
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::OldGenBytes))
        .emit_after_current();
    probe.end(0, super::diag_sites::ChargeKind::SyncFull);
}

/// Test-only control surface for separately compiled extension-crate tests
/// (#11873): the next allocation point that finds an old reclaim owed runs it
/// there, through the valve, instead of deferring it to the next safepoint.
/// For tests of native code that must survive a full trace inside one of its
/// own allocations; thread-local and consumed by that allocation point.
#[doc(hidden)]
pub fn js_gc_old_reclaim_valve_test_override() {
    GC_OLD_RECLAIM_VALVE_TEST_OVERRIDE.with(|forced| forced.set(true));
}

/// RAII guard that marks a #5476 direct old-gen reclaim in progress so a nested
/// `gc_check_trigger` can't re-enter it. See `GC_OLD_RECLAIM_IN_PROGRESS`.
pub(super) struct OldReclaimReentryGuard;

impl OldReclaimReentryGuard {
    pub(super) fn enter() -> Self {
        GC_OLD_RECLAIM_IN_PROGRESS.with(|p| p.set(true));
        Self
    }
}

impl Drop for OldReclaimReentryGuard {
    fn drop(&mut self) {
        GC_OLD_RECLAIM_IN_PROGRESS.with(|p| p.set(false));
    }
}

/// #10928: DIAGNOSTIC ONLY - this no longer gates any collection.
///
/// It was the #7937 absolute first-crossing threshold. That arm is deleted
/// (see `old_reclaim_pressure_due`), so the sole live old-reclaim input is
/// `gc_old_reclaim_growth_band_bytes`. What remains here is the value the
/// `[gc-trigger]` line prints as `old_threshold=`, kept so the parsers three
/// lanes point at this trace keep working; `old_threshold_governs=0` is
/// printed next to it. Delete both after the campaign.
///
/// Do not reach for this constant in new code: an orphaned tunable that
/// still prints is worse than an inert arm, because it looks live.
pub(in crate::gc) const GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES: usize = 48 * 1024 * 1024;
pub(in crate::gc) const GC_OLD_GEN_RECLAIM_GROWTH_BYTES: usize = 32 * 1024 * 1024;
/// Old-gen pressure the reclaim arms act on: block-offset in-use minus the
/// swept holes the free list can already hand back (#7437). Before hole
/// reuse existed, dead-but-unreclaimable bytes counted as pressure, so
/// old-reclaim kept re-firing full collections that could not actually
/// lower the number they were watching (probe 12: 49/50 blocks pinned by
/// scattered survivors, in-use immovable at ~105 MB).
pub(in crate::gc) fn old_gen_reclaimable_pressure_bytes() -> usize {
    crate::arena::old_gen_in_use_bytes().saturating_sub(super::old_free_bytes())
}

/// #7592: divisor for the proportional old-reclaim growth band — the next
/// full reclaim fires when old-gen has grown `baseline / 2` (50%) past the
/// post-reclaim baseline, Go's GOGC shape.
const OLD_RECLAIM_GROWTH_DIVISOR: usize = 2;

/// #7592: how much old-gen may grow past the post-reclaim baseline before the
/// next full reclaim is due. The constant `gc_old_gen_reclaim_growth_dyn_bytes`
/// band survives as the floor, but a *constant* band cannot be the whole
/// answer: each full reclaim costs O(live), so a fixed-bytes cadence makes
/// total major-GC work quadratic in the live set — the same shape #7594
/// removed one generation down, just paced by promotion instead of
/// allocation. A band proportional to the baseline makes the major count
/// logarithmic in heap growth (`∫ dL/(L/2) = 2·ln(Lmax/L0)`), so total major
/// work stays linear in the final live set.
///
/// This is also strictly better on the #7437 failure mode (a reclaim that
/// cannot actually lower the number it watches, e.g. pinned survivors): the
/// futile reclaim resets the baseline to the still-high value, so the next
/// band is *larger*, spacing futile repeats out instead of re-firing every
/// constant step.
///
/// Shared by `old_reclaim_pressure_due` and `gc_old_reclaim_debt_bytes` so
/// the "is it due" predicate and the debt arithmetic cannot diverge (#7024's
/// two-predicates-collapse family).
pub(in crate::gc) fn gc_old_reclaim_growth_band_bytes(baseline: usize) -> usize {
    let band = gc_old_gen_reclaim_growth_dyn_bytes().max(baseline / OLD_RECLAIM_GROWTH_DIVISOR);
    // Survival-adaptive, the same signal and the same multiplier the
    // arena-growth escalation uses (`MAJOR_PACING_RETAINING_GROWTH_MULTIPLIER`).
    //
    // `credit_promoted_bytes_to_old_baseline` already exempts old-gen growth
    // that a minor PROVED live, but a large object is allocated straight into
    // old-gen and never passes through promotion, so its bytes are uncredited
    // growth even when they are the program's live data. On `retain.ts` that is
    // the element array itself: with the arena-growth escalation correctly
    // declining, this band became the binding constraint and fired a 452 ms
    // full that reclaimed 7.6% — the same futile-full shape one trigger over,
    // reached by the same route. While the young generation is not dying, old
    // growth is priced as live here too.
    let band = if GC_MAJOR_PACING_RETAINING.with(|c| c.get()) {
        band.saturating_mul(MAJOR_PACING_RETAINING_GROWTH_MULTIPLIER)
    } else {
        band
    };
    // #10928: the adaptive input. `update_old_reclaim_backoff` shifts this left
    // once per old-reclaim full that freed less than
    // `MAJOR_PACING_PRODUCTIVE_YIELD_PCT` of the live set it traced, and resets
    // it to zero on the first productive one.
    //
    // The signal already existed (`GC_MAJOR_PACING_BACKOFF_SHIFT`) and could
    // never see an old-reclaim full: `note_full_cycle_started` has exactly one
    // production caller, inside `arena_growth_full_escalation_due`, so
    // `update_major_pacing_backoff` returned at `pre_in_use == 0` on every one
    // of the 412 fulls a tsc transpile runs. Recording the pre-full reading
    // inside the PREDICATE is the same structural fix
    // `arena_growth_full_escalation_due` already carries, for the same reason
    // its comment gives: #7726 wired call sites by hand and missed the one the
    // shipped path actually takes.
    //
    // Quiescent on tsc by measurement - its fulls free a median 21.5 MB against
    // a 65.1 MB arena, comfortably past the 20% yield test - so the shift stays
    // 0 there and the band is unchanged. It engages on the futile-full shape
    // the yield test was written for.
    let shift = GC_OLD_RECLAIM_BACKOFF_SHIFT.with(TriggerInput::get);
    band.checked_shl(shift).unwrap_or(usize::MAX)
}

/// #10928: the single live old-reclaim pacing rule.
///
/// The #7937 absolute first-crossing arm used to sit here and was DELETED. It
/// fired on `old_in_use >= 48 MiB && baseline < 48 MiB`, which is not a
/// property of the heap: every reclaim resets the baseline BELOW the
/// threshold, so the "first crossing" re-arms and fires again on the next
/// ~18 MiB of growth. Evaluating both arms per trigger on `tscwork.ts` x3
/// measured the absolute arm true in 422/438 observations (96.3%) on
/// v0.5.1633 and 428/444 (96.4%) on PR #10931s head, the proportional arm
/// alone in 5 and 3, and BOTH TOGETHER IN ZERO. One arm did all the work.
///
/// The other could not bind. `gc_old_reclaim_growth_band_bytes` is
/// `max(32 MiB floor, baseline/2)`, and `baseline/2` passes the floor only
/// above a 64 MiB baseline; tsc peaks at 57.6 MiB before #10931 and 49.3 MiB
/// after it, so the band read exactly 32 MiB in 438/438 and 444/444
/// observations. Shrinking the live set moved the heap FURTHER from the
/// proportional regime, so the defect deepens as the object-model work
/// succeeds - which is why this is a rewrite and not a retuned constant.
///
/// What is left is one proportional rule with one adaptive input: the
/// productivity of the preceding old-reclaim full.
#[inline]
pub(in crate::gc) fn old_reclaim_pressure_due(old_in_use: usize, baseline: usize) -> bool {
    let due = old_reclaim_pressure_due_inner(old_in_use, baseline);
    if due {
        // Recorded HERE, not at the call sites, so a future arm that decides
        // to run a reclaim cannot forget to price it - the rule
        // `arena_growth_full_escalation_due` already follows, and the one
        // #7726 learned by wiring two sites and missing the third.
        note_old_reclaim_cycle_started();
    }
    due
}

/// The pure predicate: no pacing side effect, so the debt arithmetic and the
/// unit tests can ask the question without answering it.
#[inline]
pub(in crate::gc) fn old_reclaim_pressure_due_inner(old_in_use: usize, baseline: usize) -> bool {
    old_in_use.saturating_sub(baseline) >= gc_old_reclaim_growth_band_bytes(baseline)
}

/// #10928: snapshot the live set this old-reclaim full is about to trace, for
/// `update_old_reclaim_backoff` to price the result against. Same metric on
/// both sides (`pacing_arena_in_use_bytes`) so the two cannot disagree about
/// whether the full helped.
fn note_old_reclaim_cycle_started() {
    GC_OLD_RECLAIM_PRE_IN_USE_BYTES.with(|bytes| bytes.set(pacing_arena_in_use_bytes()));
}

/// Cap on the old-reclaim backoff, deliberately tighter than
/// `MAJOR_PACING_BACKOFF_SHIFT_MAX`. Each shift doubles how much old-gen may
/// grow between fulls, and that growth is peak RSS: measured on tsc, the band
/// governs a ~47 MB live set inside a ~300 MB peak, so one doubling of the
/// 32 MiB band is already most of the campaigns +10% peak-RSS budget. Raise
/// this only with an RSS measurement that says the budget holds.
const OLD_RECLAIM_BACKOFF_SHIFT_MAX: u32 = 1;

/// Price what the just-finished old-reclaim full actually reclaimed.
///
/// Mirrors `update_major_pacing_backoff` on what "productive" means, against
/// its own cell: the two triggers pace different things and must not share a
/// homeostat.
///
/// #10960: an unproductive full widens the band ONLY when the live set did
/// not grow since the previous full. The backoff exists for the futile-full
/// shape - repeated traces of the same live set that free nothing - and a
/// full that reclaimed little because the program was still BUILDING its live
/// set is not that shape: the proportional band already follows the baseline
/// up, so widening on top of it double-counts the growth. Measured on
/// `gc_reclaim.ts 64 6` (64 MiB live, replaced continuously): the first full
/// runs at 33.5 MB while the heap is still ramping and frees nothing, the old
/// rule doubled the band to 64 MiB, and the next full slid out to
/// `old_in_use=135 MB` with `arena_total=281 MB` - a +29% peak-RSS excursion
/// (median 150 -> 193 MB, 15/15 interleaved pairs) that steady state never
/// needed. A growing live set leaves the shift where it is: it is evidence
/// neither that fulls are futile nor that they have become productive.
fn update_old_reclaim_backoff(post_in_use: usize) {
    let prev_post = GC_OLD_RECLAIM_LAST_POST_IN_USE_BYTES.with(|bytes| bytes.replace(post_in_use));
    let pre_in_use = GC_OLD_RECLAIM_PRE_IN_USE_BYTES.with(|bytes| bytes.get());
    if pre_in_use == 0 {
        return;
    }
    GC_OLD_RECLAIM_PRE_IN_USE_BYTES.with(|bytes| bytes.set(0));
    let reclaimed = pre_in_use.saturating_sub(post_in_use);
    let productive =
        reclaimed.saturating_mul(100) / pre_in_use >= MAJOR_PACING_PRODUCTIVE_YIELD_PCT;
    // Same yardstick as `productive`: at least that share of what this full
    // traced is live data the previous full never saw.
    let grew = post_in_use.saturating_sub(prev_post).saturating_mul(100) / pre_in_use
        >= MAJOR_PACING_PRODUCTIVE_YIELD_PCT;
    GC_OLD_RECLAIM_BACKOFF_SHIFT.with(|shift| {
        if productive {
            shift.set(0);
        } else if !grew {
            shift.set(
                shift
                    .get()
                    .saturating_add(1)
                    .min(OLD_RECLAIM_BACKOFF_SHIFT_MAX),
            );
        }
    });
}

#[cfg(test)]
pub(in crate::gc) fn test_set_old_reclaim_last_post_in_use(bytes: usize) -> usize {
    GC_OLD_RECLAIM_LAST_POST_IN_USE_BYTES.with(|c| c.replace(bytes))
}

#[cfg(test)]
pub(in crate::gc) fn test_set_old_reclaim_backoff_shift(shift: u32) -> u32 {
    GC_OLD_RECLAIM_BACKOFF_SHIFT.with(|c| c.replace(shift))
}

#[cfg(test)]
pub(in crate::gc) fn test_old_reclaim_backoff_shift() -> u32 {
    GC_OLD_RECLAIM_BACKOFF_SHIFT.with(TriggerInput::get)
}

#[cfg(test)]
pub(in crate::gc) fn test_old_reclaim_pre_in_use_bytes() -> usize {
    GC_OLD_RECLAIM_PRE_IN_USE_BYTES.with(Cell::get)
}

#[cfg(test)]
pub(in crate::gc) fn test_clear_old_reclaim_pre_in_use_bytes() {
    GC_OLD_RECLAIM_PRE_IN_USE_BYTES.with(|b| b.set(0));
}

#[cfg(test)]
pub(in crate::gc) fn test_price_old_reclaim_full(pre_in_use: usize, post_in_use: usize) {
    GC_OLD_RECLAIM_PRE_IN_USE_BYTES.with(|b| b.set(pre_in_use));
    update_old_reclaim_backoff(post_in_use);
}

pub(in crate::gc) fn maybe_schedule_old_reclaim_after_copied_minor() {
    // #6010: external Map/Set side buffers count toward old-gen pressure —
    // a tenured-then-dead Map holds its multi-MB buffer until a full
    // reclaim's old-gen sweep finalizes it, so the buffer bytes must be
    // able to escalate that reclaim.
    let old_in_use = old_gen_reclaimable_pressure_bytes()
        .saturating_add(external_side_old_reclaim_pressure_bytes());
    let baseline = GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.get());
    if old_reclaim_pressure_due(old_in_use, baseline) {
        GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(true));
    }
}

/// #7902: a traced cycle contradicted the predictor that admitted `bytes` of
/// untraced (assumed-live) promotion, so schedule the old-gen reclaim that can
/// actually decide their liveness.
///
/// Nothing else will: the traced cycle measures only its own young generation,
/// so it can neither identify nor reclaim a cohort the preceding untraced
/// cycles already moved into old-gen. Left alone the bytes sit there until
/// growth pressure fires — which it may not, because a phase-changed program's
/// heap has stopped growing.
pub(in crate::gc) fn request_old_reclaim_for_untraced_promotions(bytes: usize) {
    if bytes == 0 {
        return;
    }
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(true));
}

pub(in crate::gc) fn finish_full_old_reclaim_baseline() {
    // Medium-parse pacing (2026-09-14): the full this baseline records is the
    // collection the drained bytes were being held for, so the debt is paid
    // here — before the baseline is read, or the baseline would carry it into
    // the next band incorrectly including already released bytes in the following growth band.
    GC_EXTERNAL_SIDE_DRAINED_SINCE_FULL.with(|c| c.set(0));
    // Baseline includes external side-buffer bytes (#6010) so the growth
    // delta in `old_reclaim_pressure_due` stays unit-consistent.
    let old_in_use = old_gen_reclaimable_pressure_bytes()
        .saturating_add(external_side_old_reclaim_pressure_bytes());
    GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.set(old_in_use));
    // #10182: this full verified everything old; the promoted cohort starts over.
    super::promoted_cohort::note_full_finished(old_in_use);
    // Price escalation against old objects on both sides of the full.
    // Keep the whole-arena census only for the existing old-reclaim yield.
    let post_in_use = crate::arena::arena_live_allocated_bytes();
    let post_old = pacing_old_in_use_bytes();
    GC_LAST_FULL_OLD_IN_USE_BYTES.with(|bytes| bytes.set(post_old));
    update_major_pacing_backoff(post_old);
    // #10928: and price THIS full against the old-reclaim band that scheduled it.
    update_old_reclaim_backoff(post_in_use);
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(false));
    GC_OLD_RECLAIM_DEFER_BASE.with(|base| base.set(None));
    // #7742: the dead bytes that whole-block promotion parked in old-gen are
    // exactly what this collection just reclaimed, so the running budget that
    // caps them starts over.
    super::note_full_collection_reclaimed_old_gen();
}
