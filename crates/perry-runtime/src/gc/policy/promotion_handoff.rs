//! The copying minor's promotion handoff: when survivors waiting for promotion
//! make a full collection due, and how promoted bytes credit the
//! old-generation baseline.

use super::*;

pub(in crate::gc) const GC_COPY_PROMOTION_HANDOFF_MIN_BYTES: usize = 24 * 1024 * 1024;

/// Whether an imminent promotion justifies a full old reclaim FIRST.
///
/// #7592: the pressure test is on the CURRENT old-gen occupancy only. It used
/// to be `old_reclaim_pressure_due(old_in_use + promotable_bytes, _)` — a
/// *prediction* of where old-gen would land after the promotion — which is
/// the #7594 mistake in another coat: the promotable bytes are in the
/// survivor space, where a full mark-sweep can neither reclaim them (they are
/// live) nor reclaim the old-gen space they have not yet occupied. A handoff
/// scheduled on predicted pressure over a near-empty old-gen is guaranteed
/// futile — measured on #7592's `json_pipeline` at 500k records as a 1,015 ms
/// full over 4.2 MB of old-gen that freed nothing. The only useful work a
/// handoff can do is clear CURRENT old garbage so the promotion lands in
/// reused holes; when old-gen has no reclaimable pressure of its own, the
/// promotion should simply proceed and grow it.
///
/// `promotable_bytes` remains the reason to *bother* checking: below the
/// handoff minimum the upcoming promotion is too small to be worth a full
/// collection under any old-gen state.
#[inline]
pub(in crate::gc) fn copied_minor_promotion_handoff_pressure_due(
    promotable_bytes: usize,
    old_in_use: usize,
    baseline: usize,
) -> bool {
    promotable_bytes >= gc_copy_promotion_handoff_min_dyn_bytes()
        && old_reclaim_pressure_due(old_in_use, baseline)
}

pub(in crate::gc) fn copied_minor_promotable_active_survivor_bytes() -> usize {
    // Pure measurement pass over the active survivor semispace only. Use the
    // block-filtered walk so blocks outside the active range are skipped in
    // O(n_blocks) instead of iterating every object in Eden/longlived/old-gen
    // just to discard it (#6181) — both walkers visit the regions in the same
    // order with the same global block-index bases, so the filter is exactly
    // equivalent to the previous in-callback range check.
    let active_range = crate::arena::active_survivor_block_index_range();
    // Match the per-type, per-cycle policy the copier will snapshot.
    let promotion_policy = super::tenuring::PromotionPolicy::snapshot();
    let mut promotable = 0usize;
    crate::arena::arena_walk_objects_filtered(
        |block_idx| active_range.contains(&block_idx),
        |header_ptr, _block_idx| {
            let header = header_ptr as *mut GcHeader;
            unsafe {
                let flags = (*header).gc_flags;
                if flags & GC_FLAG_FORWARDED != 0 {
                    return;
                }
                let prior_age = copied_survival_age((*header)._reserved, flags);
                let next_age = prior_age.saturating_add(1);
                // Mirror move_young's promotion predicate, including the
                // adaptive threshold (gc/tenuring.rs), so this pacing estimate
                // matches what the next copying minor will actually promote.
                let threshold = promotion_policy.for_type((*header).obj_type);
                if flags & GC_FLAG_TENURED != 0 || next_age >= threshold {
                    promotable = promotable.saturating_add((*header).size as usize);
                }
            }
        },
    );
    promotable
}

/// #7592: whether a survivor-promotion handoff full has run without the copying
/// minor it exists to enable having run since.
///
/// The handoff replaces a minor with a full mark-sweep to make room in old-gen
/// for survivors that are about to be promoted. But a full mark-sweep is
/// **non-moving — it promotes nothing**, so it cannot itself relieve the
/// pressure it was scheduled for: the survivor space still holds the same
/// bytes, the reclaim baseline it resets does not count them, and the predicate
/// is immediately true again. Without this latch the next minor is intercepted
/// too, and the collector livelocks on full collections that free nothing —
/// measured on #7592's `json_pipeline` as 19 consecutive fulls, each freeing
/// 0.0 MB at ~400 ms, which was 7.6 s of an 8.6 s phase.
///
/// One handoff per copying minor is the invariant: the handoff makes room, the
/// minor does the promotion that consumes it.
pub(in crate::gc) fn survivor_promotion_handoff_awaiting_minor() -> bool {
    SURVIVOR_HANDOFF_AWAITING_MINOR.with(Cell::get)
}

pub(in crate::gc) fn note_survivor_promotion_handoff_full() {
    SURVIVOR_HANDOFF_AWAITING_MINOR.with(|flag| flag.set(true));
}

/// How many handoffs the latch has suppressed. Every one of these was a full
/// mark-sweep that would have freed nothing — on `main` this counter would have
/// read 18 for #7592's 200k `json_pipeline` run. It exists so the suppression
/// itself is observable: the latch short-circuits before the arena inspection,
/// so a test with an empty heap cannot otherwise distinguish "suppressed" from
/// "there was no pressure anyway".
#[cfg(test)]
pub(in crate::gc) fn survivor_promotion_handoff_suppressions() -> u64 {
    SURVIVOR_HANDOFF_SUPPRESSIONS.with(Cell::get)
}

/// Clear the latch — called only when a *copying* minor completes, since only
/// that collector promotes. A non-moving minor fallback promotes nothing, so
/// re-arming on one would reinstate the livelock at half rate.
pub(in crate::gc) fn note_copying_minor_completed() {
    SURVIVOR_HANDOFF_AWAITING_MINOR.with(|flag| flag.set(false));
}

pub(in crate::gc) fn copied_minor_promotion_handoff_due(trigger_kind: GcTriggerKind) -> bool {
    if !matches!(
        trigger_kind,
        GcTriggerKind::ArenaBytes | GcTriggerKind::MallocCount
    ) {
        return false;
    }
    // A handoff full has already run and promoted nothing; let the copying
    // minor it was scheduled for actually happen (#7592). Placed before the
    // survivor walk below so a suppressed handoff also skips that O(n) pass.
    if survivor_promotion_handoff_awaiting_minor() {
        SURVIVOR_HANDOFF_SUPPRESSIONS.with(|n| n.set(n.get().saturating_add(1)));
        return false;
    }
    if crate::arena::copying_active_survivor_in_use_bytes()
        < gc_copy_promotion_handoff_min_dyn_bytes()
    {
        return false;
    }
    let promotable = copied_minor_promotable_active_survivor_bytes();
    let old_in_use = old_gen_reclaimable_pressure_bytes()
        .saturating_add(external_side_old_reclaim_pressure_bytes());
    let baseline = GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.get());
    copied_minor_promotion_handoff_pressure_due(promotable, old_in_use, baseline)
}

/// #7592: credit a copying minor's promoted bytes to the old-reclaim
/// baseline.
///
/// Promoted bytes are live *by construction* — only marked-live objects get
/// copied — so a full mark-sweep fired the moment promotion pushes old-gen
/// over a threshold is guaranteed to find them all live and free nothing
/// (measured: a 2,100 ms full over 274 MB of just-promoted objects, 0.0 MB
/// freed). Treating the promotion delta as part of the "clean" baseline means
/// the next reclaim fires only after old-gen has *grown or churned* past it —
/// the proportional band (`gc_old_reclaim_growth_band_bytes`) then prices the
/// reclaim off the real live set. Pre-existing old garbage is unaffected: the
/// credit is exactly the promoted delta, never a resync to current in-use.
///
/// The trade is the standard GOGC one: bytes that are promoted and then die
/// quickly now wait for the growth band instead of the next threshold
/// crossing. That is deliberate — a promoted-then-dead cohort big enough to
/// matter moves the band by its own size.
///
/// # ★ Every promotion is credited, including an UNTRACED one (#7965)
///
/// #7902 made the call site skip this for a `PromotionLiveness::AssumeAllLive`
/// promotion, reasoning that "live by construction" is a marked-liveness claim
/// an untraced cycle does not make. The premise is right and the conclusion
/// does not follow, because **this baseline is not a liveness claim**. It is
/// the base of a growth measurement: `old_in_use - baseline` is meant to read
/// "how much has old-gen grown since the last reclaim decision", and bytes a
/// minor has just relocated there are growth that decision has already seen.
///
/// Withholding it does not defer a little reclamation, it degenerates the
/// predicate. A fully-live young generation promotes untraced on *every*
/// cycle, so on exactly the workloads that reach the untraced path nothing
/// else credits the baseline and it stays pinned at 0. Then
/// `old_in_use - baseline` collapses into `old_in_use` — absolute occupancy,
/// not growth — and [`gc_old_reclaim_growth_band_bytes`]'s proportional half
/// (`baseline / OLD_RECLAIM_GROWTH_DIVISOR`) collapses with it, leaving the
/// constant floor. **A constant band pacing a collector whose per-cycle cost is
/// O(live)** is the quadratic shape #7592 removed here and #7594 removed one
/// generation down. Measured on `retain` (#7965): 0 fulls → 1–2 fulls,
/// 2 841 M → 8 237 M instructions retired, +25% peak RSS, and the same on
/// `retain1` / `retain_wide` / `retain_wide1` / `deeplist`.
///
/// The uncertain cohort #7902 is right to worry about — assumed-live bytes
/// parked in old-gen by a predictor that has since been contradicted — is
/// bounded by the three instruments #7902 itself added, none of which paces on
/// this quantity: `untraced_promotion_budget_bytes` forces a measuring cycle,
/// `implied_dead_bytes` charges that run against `PROMOTED_DEAD_BUDGET_BYTES`,
/// and [`request_old_reclaim_for_untraced_promotions`] schedules the reclaim
/// outright when the measurement contradicts the predictor. Those act on
/// evidence about the cohort; a pinned pacing base acts on every program that
/// retains, whether or not anything about it is uncertain.
pub(in crate::gc) fn credit_promoted_bytes_to_old_baseline(promoted_bytes: usize) {
    if promoted_bytes == 0 {
        return;
    }
    GC_LAST_OLD_RECLAIM_IN_USE_BYTES
        .with(|bytes| bytes.set(bytes.get().saturating_add(promoted_bytes)));
    // #10182: the credit hides these bytes from the growth band by design; the
    // promoted-cohort bound is what still counts the in-place-promoted share of
    // them (`promoted_cohort::note_minor_promotion`, called by the minor).
}

/// Feed a copying minor's measured young-survival ratio to arena-growth pacing.
///
/// Two effects, both gated on the same measurement:
///
/// * It arms/disarms `GC_MAJOR_PACING_RETAINING`, which widens the escalation
///   growth band (see `MAJOR_PACING_RETAINING_GROWTH_MULTIPLIER`).
/// * While retaining, it re-baselines arena-growth pacing on the occupancy that
///   *survived this collection*. Without that the band has nothing to scale:
///   before the first full the baseline is 0, so the boundary degenerates to
///   the absolute `PERRY_GC_MAJOR_PACING_FLOOR_MB` and **any** program that
///   retains more than 32 MB pays a whole-heap mark-sweep for doing so.
///
/// The re-baseline is a ratchet (`max`), never a decrease, so a minor cannot
/// pull the boundary in below what the last full established — and it is
/// skipped entirely when the heap is not retaining, which is what keeps
/// `churn`/`cycles`/`push_cls` (0–4 permille survival) bit-identical to the
/// previous policy: their baseline stays 0 and their boundary stays the floor.
///
/// Note the direction of the whole change: because the baseline only ever
/// ratchets UP and the multiplier is ≥ 1, the escalation boundary is never
/// *lower* than it was before. This can only make fulls rarer, never more
/// frequent — the exposure is deferred reclamation (RSS), not extra pauses.
pub(in crate::gc) fn note_copying_minor_young_survival(survival_permille: u64) {
    let retaining = survival_permille >= MAJOR_PACING_RETAINING_SURVIVAL_PERMILLE;
    GC_MAJOR_PACING_RETAINING.with(|c| c.set(retaining));
    if !retaining {
        return;
    }
    let survived = pacing_old_in_use_bytes();
    GC_LAST_FULL_OLD_IN_USE_BYTES.with(|bytes| bytes.set(bytes.get().max(survived)));
}
