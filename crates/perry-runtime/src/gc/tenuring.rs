//! Adaptive tenuring threshold for the copying (scavenging) minor collector.
//!
//! The copying minor ages nursery survivors through the survivor semispaces
//! and promotes them to old-gen after a fixed number of copies
//! (`GC_COPY_PROMOTION_SURVIVALS`, 4). That fixed age is correct for the
//! generational-hypothesis workloads the scavenge was tuned on (tiny live
//! sets, near-empty survivor spaces) and pathological for large live sets:
//! a program whose survivors essentially never die in the survivor space
//! pays `age × influx` bytes of pure re-copying — measured 4.39 GB / 61 M
//! object-copies on a binary-tree benchmark whose entire live set was
//! ~35 MB, because the same ~3.15 MB survivor cohort ping-ponged between
//! the semispaces on every one of 1427 collections.
//!
//! This module is the survivor-occupancy feedback loop (HotSpot's adaptive
//! `TenuringThreshold`, restated for elastic semispaces). Per copying cycle
//! the collector reports the **Eden survivor influx** — bytes moved out of
//! Eden that were live (copied to a survivor space or promoted). At a
//! threshold of S survivals, steady-state survivor occupancy is
//! `(S−1) × influx` (S−1 resident age cohorts), so the next cycle's
//! threshold is the largest S whose projected occupancy fits the desired
//! survivor size:
//!
//! ```text
//! S = min(4, 1 + desired / influx)
//! ```
//!
//! `desired` is 1/16 of the scavenge nursery cap (1 MB at the default
//! 16 MB cap) — the same effective ratio HotSpot's defaults produce
//! (SurvivorRatio=8, TargetSurvivorRatio=50% ⇒ Eden/16).
//!
//! The influx signal is deliberately *threshold-invariant*: live Eden bytes
//! get moved somewhere at any S, so the feedback loop has a fixed point
//! instead of the oscillation that a post-hoc survivor-occupancy signal
//! produces (at S=1 the survivor space is empty, which would read as "no
//! pressure" and snap the threshold straight back up).
//!
//! Asymmetric response: the threshold *drops* to the computed target
//! immediately (every extra cycle at a too-high threshold re-copies the
//! whole resident cohort), but *rises* one step at a time and only after
//! the target has been above the current value for two consecutive cycles
//! (a single quiet cycle — e.g. a malloc-count trigger firing before Eden
//! filled — must not flush the aging pipeline into a copy burst).
//!
//! The occupancy rule alone is not sufficient: it optimises survivor
//! *space*, not copy *work*. A workload whose influx sits just under
//! `desired` (tree.ts measures 1,048,536 B against the 1,048,576 B
//! default — 40 bytes under) settles at S=2 and still copies every
//! surviving byte exactly once for nothing, because 100% of each cohort
//! survives its survivor round and gets promoted a cycle later anyway.
//! The **survival-rate lock** closes that: when last cycle's survivor
//! intake (`copied_bytes`) was substantial and ≥90% of it came back out
//! alive this cycle (`survivor_live_bytes`), the aging round demonstrably
//! filters nothing, so the threshold locks to 1 (promote on first copy)
//! until the influx goes quiet. The lock's exit signal — influx below
//! `desired/4` for two consecutive cycles — stays measurable while
//! locked, unlike survivor occupancy, which is zero at S=1 and would
//! leave the loop blind.
//!
//! There is no env knob here (see CLAUDE.md's GC knob kill-policy): the
//! loop is always on, and its neutral state — influx below `desired`,
//! survivor cohorts that die in place — computes S=4, which is
//! bit-for-bit the previous fixed behaviour.
//!
//! ## Seeding the lock from a non-copying collection (#7598)
//!
//! The survival-rate lock above is correct but **one cycle late by
//! construction**: it keys on `prev_copied`, so a *previous copying minor*
//! must already have filled the survivor space. On a workload with one
//! long-lived burst (`json_pipeline`: `out.push({…})` 500k times) the first
//! copying minor therefore always pays the wasted copy — measured 268 MB
//! Eden→survivor on cycle 3 and the same 268 MB survivor→old on cycle 4,
//! ~3.9 s of a 5.1 s phase spent copying one cohort twice.
//!
//! Both blind spots named on #7592 are the same shape: `retune_after_scavenge`
//! is fed **only** by copying minors, so full mark-sweeps and non-copying
//! minor fallbacks feed the loop nothing, and once a workload escalates to
//! those the loop goes blind exactly when it is needed.
//!
//! [`seed_promote_lock_from_sweep`] closes that. Every cycle that reaches the
//! mark-sweep path — a full, or a non-copying minor fallback — already walks
//! every Eden header and classifies it live or dead. That walk yields the two
//! numbers the lock wants, one collection *earlier* than the survivor
//! round-trip can produce them.
//!
//! ### Why this signal is not a fixed point of the policy reading it
//!
//! This issue has already produced three self-referential signals, so the
//! argument is spelled out rather than assumed:
//!
//! * The Eden-side probe reached for `promoted_bytes`, which is **zero by
//!   construction at S=4** — "was the promotion rate high?" can never answer
//!   yes while S=4 holds, so it could never leave S=4.
//! * #7596's first nursery cap gated from-space occupancy on a total that
//!   *included* from-space, so the cap was a bound from-space could never
//!   cross and scavenging stopped entirely.
//! * #7594's handoff scheduled a *non-moving* full to relieve pressure only a
//!   *moving* cycle can relieve, so the predicate was true again next cycle.
//!
//! The Eden live/dead split has none of that structure. It is produced by the
//! mark-sweep's own arena walk: marks come from reachability from roots, and
//! neither the mark phase nor the walk reads `tenuring_survivals()` — the
//! threshold is consulted only by `copying.rs`'s per-object move, which this
//! path does not run. Concretely, at both endpoints the signal stays
//! measurable and keeps its meaning:
//!
//! | state | what Eden holds at the sweep | signal |
//! |---|---|---|
//! | S=4 (state to leave) | everything that has not aged out | high live fraction on a retaining workload ⇒ says "lock" |
//! | S=1 (state to stay in) | only Eden allocated since the last promotion | still the mutator's retention of recent Eden ⇒ still measurable |
//!
//! Exit is deliberately **not** this signal. Entering hands over to the
//! existing `PROMOTE_LOCK`, whose unlock condition (Eden influx below
//! `desired/4` for two consecutive copying minors) is already
//! threshold-invariant and already tested — so the seed cannot introduce an
//! enter/exit oscillation of its own.
//!
//! ### Determinism (#7432)
//!
//! #7432 forbids re-deciding S *while objects are being moved*: the
//! copied/promoted split would then depend on root traversal order. The seed
//! is written at the END of a completed sweep and read at the ENTRY of a later
//! copying minor (`CopyingNurseryCollector::new` snapshots it once), so every
//! object in a cycle still sees exactly one threshold and the counters stay a
//! pure function of (heap state at entry, threshold at entry).
//!
//! Two exclusions at the callsite keep the *input* deterministic too, and both
//! are refusals rather than tuning:
//!
//! * **Budgeted cycles.** Whole-cycle allocate-black marks every mid-cycle
//!   birth, and this walk reads MARKED as live — a churn workload's births
//!   would read as a ~100% live Eden. Same reason the age-bump is suppressed
//!   there (`cycle.rs`).
//! * **Cycles that ran the conservative native-stack scan.** That scan retains
//!   whatever the stack happens to look like a pointer to, by an amount that
//!   varies run to run (`benchmarks/gc_ratchet/README.md`). A liveness
//!   *measurement* taken under it is not sound, and feeding it into a policy
//!   would make the gated copy/promote counters non-deterministic.

use super::*;

/// Ceiling and power-on value: the previous fixed threshold.
pub(super) const GC_TENURING_SURVIVALS_MAX: u8 = GC_COPY_PROMOTION_SURVIVALS;

/// Consecutive cycles the computed target must exceed the current threshold
/// before it is raised (by one step).
const RAISE_DEBOUNCE_CYCLES: u8 = 2;

/// Ceiling for the influx-driven nursery cap scale: 16 MB × 4 = 64 MB.
/// Bounds the young-gen RSS contribution on live-set-bound workloads while
/// still cutting their collection count 4× (each collection carries a fixed
/// root-scan/remembered-set/eligibility cost that dominates once the
/// adaptive threshold has eliminated the re-copying).
const NURSERY_CAP_SCALE_MAX: u8 = 4;

thread_local! {
    static TENURING_SURVIVALS: Cell<u8> = const { Cell::new(GC_TENURING_SURVIVALS_MAX) };
    static RAISE_STREAK: Cell<u8> = const { Cell::new(0) };
    /// Survival-rate lock: promote-on-first-copy until influx goes quiet.
    static PROMOTE_LOCK: Cell<bool> = const { Cell::new(false) };
    static UNLOCK_STREAK: Cell<u8> = const { Cell::new(0) };
    /// Bytes the previous copying minor put into the to-survivor space —
    /// the denominator of this cycle's survival rate.
    static PREV_COPIED_BYTES: Cell<usize> = const { Cell::new(0) };
    /// Influx-driven multiplier (1, 2, or 4) applied to the scavenge nursery
    /// cap. Power of two; grows/shrinks one step at a time, debounced.
    static NURSERY_CAP_SCALE: Cell<u8> = const { Cell::new(1) };
    static CAP_GROW_STREAK: Cell<u8> = const { Cell::new(0) };
    static CAP_SHRINK_STREAK: Cell<u8> = const { Cell::new(0) };
}

/// The survivals threshold the next copying minor should promote at:
/// `next_age >= tenuring_survivals()` tenures. In `1..=4`; 4 is the
/// original fixed policy, 1 promotes every live nursery object on first
/// copy.
pub(super) fn tenuring_survivals() -> u8 {
    TENURING_SURVIVALS.with(Cell::get)
}

/// The effective scavenge nursery cap: the configured base
/// (`PERRY_GC_SCAVENGE_NURSERY_MB`, default 16 MB) times the influx-driven
/// scale. A fixed cap sets collection frequency independently of how much
/// survives; on live-set-bound workloads (tree/retain/deeplist shapes) that
/// multiplies the per-collection fixed cost by an enormous collection
/// count AND promotes objects that a larger Eden would have let die young.
/// The scale grows only while survivor influx stays a heavy fraction of
/// Eden, so the small-live-set workloads #7377 fixed never leave 16 MB.
///
/// #7592: the influx-driven product is the floor, not the whole answer — it
/// is bounded by `base × NURSERY_CAP_SCALE_MAX` (64 MB), and any *constant*
/// cap sets collection cadence independently of how much is live while each
/// collection's fixed cost is O(old-gen) (#6181: full-region sweep walk,
/// whole-heap remembered-set rebuild). Total young-GC work is then
/// `(alloc / cap) × O(live)` — quadratic in the live set. The effective cap
/// is therefore also proportional to the TENURED live set, which keeps work
/// per allocated byte bounded as the heap grows.
///
/// The proportional term deliberately keys on old-gen occupancy, NOT total
/// arena in-use: the cap gates `young_scavenge_cap_due()` against from-space
/// occupancy, and from-space is part of arena in-use — a cap defined by a
/// total that includes the young generation is a fixed point from-space can
/// never cross, and scavenging stops entirely (measured on #7592: 0 copying
/// minors at 200k records, the nursery never evacuated). Old-gen is outside
/// the young generation, so no self-reference. Reclaimable pressure (in-use
/// minus free-list holes, #7437) rather than raw in-use, so dead-but-swept
/// old bytes do not inflate Eden.
pub(super) fn scavenge_nursery_cap_effective_bytes() -> usize {
    scavenge_nursery_cap_from(
        influx_driven_nursery_cap_bytes(),
        old_gen_reclaimable_pressure_bytes(),
    )
}

/// The influx-driven half of the cap on its own: the configured base times
/// the debounced `NURSERY_CAP_SCALE`. Named so the composition below reads as
/// the two-term policy it is.
fn influx_driven_nursery_cap_bytes() -> usize {
    gc_scavenge_nursery_cap_bytes().saturating_mul(NURSERY_CAP_SCALE.with(Cell::get) as usize)
}

/// The cap policy as a **pure function of its two inputs**, so it is testable
/// without arranging a heap state.
///
/// Splitting this out is not cosmetic. `old_gen_reclaimable_pressure_bytes()`
/// reads live arena state, and in a unit-test thread old-gen is ~empty — so
/// every assertion made against `scavenge_nursery_cap_effective_bytes()`
/// directly would pass with the proportional term deleted. That is the #7024
/// shape: a green test whose subject never ran. The two terms are exercised
/// here explicitly instead.
pub(super) fn scavenge_nursery_cap_from(influx_driven: usize, tenured_reclaimable: usize) -> usize {
    influx_driven.max(tenured_reclaimable / TENURED_EDEN_DIVISOR)
}

/// #7592: divisor for the tenured-proportional nursery cap — Eden may grow to
/// half the tenured live set before a scavenge is forced. Peak young-gen RSS
/// contribution is therefore bounded at `tenured / 2`; the young collection
/// count is logarithmic in heap growth on promote-heavy workloads
/// (`old_{n+1} ≈ old_n × (1 + 1/2)`) instead of linear in bytes allocated.
const TENURED_EDEN_DIVISOR: usize = 2;

/// Target steady-state survivor occupancy: 1/16 of the effective nursery
/// cap, so the tenuring dials track both the configured base and the
/// influx-driven scale.
pub(super) fn desired_survivor_bytes() -> usize {
    scavenge_nursery_cap_effective_bytes() / 16
}

pub(super) fn compute_target_survivals(eden_live_bytes: usize, desired_bytes: usize) -> u8 {
    if eden_live_bytes == 0 {
        return GC_TENURING_SURVIVALS_MAX;
    }
    let target = 1 + desired_bytes / eden_live_bytes;
    target.min(GC_TENURING_SURVIVALS_MAX as usize) as u8
}

/// Feed one finished copying-minor cycle into the feedback loop.
/// `eden_live_bytes` is the cycle's Eden survivor influx (bytes moved out
/// of Eden, whether copied to a survivor space or promoted);
/// `copied_bytes` is what this cycle put into the to-survivor space;
/// `survivor_live_bytes` is what came back out of the from-survivor space
/// alive (numerator of the survival rate against the *previous* cycle's
/// `copied_bytes`).
pub(super) fn retune_after_scavenge(
    eden_live_bytes: usize,
    copied_bytes: usize,
    survivor_live_bytes: usize,
) {
    retune_nursery_cap_scale(eden_live_bytes);
    let desired = desired_survivor_bytes();
    let substantial = desired / 4;
    let prev_copied = PREV_COPIED_BYTES.with(|c| c.replace(copied_bytes));
    let current = TENURING_SURVIVALS.with(Cell::get);

    if PROMOTE_LOCK.with(Cell::get) {
        // Locked: stay at 1 while the influx stays substantial. The exit
        // signal is the influx itself — measurable every cycle, unlike
        // survivor occupancy, which is identically zero at S=1.
        if eden_live_bytes < substantial {
            let streak = UNLOCK_STREAK.with(|s| s.get()).saturating_add(1);
            if streak >= RAISE_DEBOUNCE_CYCLES {
                PROMOTE_LOCK.with(|l| l.set(false));
                UNLOCK_STREAK.with(|s| s.set(0));
                RAISE_STREAK.with(|s| s.set(0));
                // Resume the ladder one step up rather than snapping to the
                // ceiling; the normal debounced rise takes it the rest of
                // the way if the workload stays quiet.
                set_survivals(current, 2, eden_live_bytes, "unlock");
            } else {
                UNLOCK_STREAK.with(|s| s.set(streak));
            }
        } else {
            UNLOCK_STREAK.with(|s| s.set(0));
        }
        return;
    }

    // Survival-rate lock: last cycle's survivor intake was substantial and
    // (nearly) all of it came back out alive, so the aging round filters
    // nothing — every copied byte is a byte that will be promoted anyway.
    if prev_copied >= substantial && survivor_live_bytes.saturating_mul(10) >= prev_copied * 9 {
        PROMOTE_LOCK.with(|l| l.set(true));
        UNLOCK_STREAK.with(|s| s.set(0));
        RAISE_STREAK.with(|s| s.set(0));
        set_survivals(current, 1, eden_live_bytes, "lock");
        return;
    }

    let target = compute_target_survivals(eden_live_bytes, desired);
    let next = if target < current {
        RAISE_STREAK.with(|s| s.set(0));
        target
    } else if target > current {
        let streak = RAISE_STREAK.with(|s| s.get()).saturating_add(1);
        if streak >= RAISE_DEBOUNCE_CYCLES {
            RAISE_STREAK.with(|s| s.set(0));
            current + 1
        } else {
            RAISE_STREAK.with(|s| s.set(streak));
            current
        }
    } else {
        RAISE_STREAK.with(|s| s.set(0));
        current
    };
    set_survivals(current, next, eden_live_bytes, "occupancy");
}

/// Grow the nursery cap one ×2 step (to at most ×4) when survivor influx
/// exceeds 4% of the current effective cap for two consecutive cycles —
/// objects are surviving because they aren't getting time to die, so a
/// bigger Eden both cuts the collection count and lets them die young.
/// Shrink one step when influx falls below 1% for two consecutive cycles.
/// The 4%/1% band is wide enough that the scale cannot oscillate on a
/// steady workload (growing halves the observed ratio, 4%/2 = 2% > 1%).
fn retune_nursery_cap_scale(eden_live_bytes: usize) {
    let cap = scavenge_nursery_cap_effective_bytes();
    let scale = NURSERY_CAP_SCALE.with(Cell::get);
    if eden_live_bytes > cap / 25 {
        CAP_SHRINK_STREAK.with(|s| s.set(0));
        if scale < NURSERY_CAP_SCALE_MAX {
            let streak = CAP_GROW_STREAK.with(|s| s.get()).saturating_add(1);
            if streak >= RAISE_DEBOUNCE_CYCLES {
                CAP_GROW_STREAK.with(|s| s.set(0));
                NURSERY_CAP_SCALE.with(|s| s.set(scale * 2));
                diag_cap_scale(scale, scale * 2, eden_live_bytes);
            } else {
                CAP_GROW_STREAK.with(|s| s.set(streak));
            }
        }
    } else if eden_live_bytes < cap / 100 {
        CAP_GROW_STREAK.with(|s| s.set(0));
        if scale > 1 {
            let streak = CAP_SHRINK_STREAK.with(|s| s.get()).saturating_add(1);
            if streak >= RAISE_DEBOUNCE_CYCLES {
                CAP_SHRINK_STREAK.with(|s| s.set(0));
                NURSERY_CAP_SCALE.with(|s| s.set(scale / 2));
                diag_cap_scale(scale, scale / 2, eden_live_bytes);
            } else {
                CAP_SHRINK_STREAK.with(|s| s.set(streak));
            }
        }
    } else {
        CAP_GROW_STREAK.with(|s| s.set(0));
        CAP_SHRINK_STREAK.with(|s| s.set(0));
    }
}

/// Minimum Eden survival rate, in tenths, for a mark-sweep to seed the
/// promote-on-first-copy lock: ≥90% of the Eden bytes the sweep classified
/// must have been live. That is the "the aging round would filter nothing"
/// proof, measured directly instead of inferred from a survivor round-trip.
const FULL_SEED_LIVE_TENTHS: usize = 9;

/// Would a completed mark-sweep's Eden census justify promote-on-first-copy?
///
/// Pure function of the census and the survivor target so the policy is
/// testable without arranging a heap (the #7024 shape: a green test whose
/// subject never ran — see the sibling `scavenge_nursery_cap_from`).
///
/// Two independent conditions, both required, each answering a different
/// question:
///
/// 1. **Occupancy** — `compute_target_survivals(...) == 1`, i.e. the surviving
///    cohort alone already exceeds the desired survivor occupancy, so at any
///    S ≥ 2 it cannot fit and would be re-copied. This is deliberately the
///    module's *existing* rule; the only new thing is where the number is read
///    from.
/// 2. **Survival rate** — nearly nothing in Eden died, so a survivor round
///    would filter nothing. Without this an Eden that merely happens to be
///    large would promote its garbage too.
///
/// The stated failure mode (design note on #7592): a normally churn-heavy
/// program whose nursery is atypically mostly-live at one sweep promotes one
/// Eden's worth of short-lived objects and pays an old-gen reclaim to get them
/// back. Exposure is bounded by one nursery cap and by the existing unlock
/// path; requiring BOTH conditions is what keeps it narrow.
pub(super) fn full_seed_promotes_on_first_copy(
    eden_live_bytes: usize,
    eden_dead_bytes: usize,
    desired_bytes: usize,
) -> bool {
    if compute_target_survivals(eden_live_bytes, desired_bytes) != 1 {
        return false;
    }
    let classified = eden_live_bytes.saturating_add(eden_dead_bytes);
    classified > 0
        && eden_live_bytes.saturating_mul(10) >= classified.saturating_mul(FULL_SEED_LIVE_TENTHS)
}

/// Feed one finished mark-sweep's Eden census into the loop. `eden_live_bytes`
/// and `eden_dead_bytes` are the bytes the sweep walk classified live and dead
/// in the general (Eden) blocks.
///
/// Callers must exclude budgeted cycles and cycles that ran the conservative
/// native-stack scan — see the module header for why those two inputs are not
/// sound liveness measurements.
pub(super) fn seed_promote_lock_from_sweep(eden_live_bytes: usize, eden_dead_bytes: usize) {
    if PROMOTE_LOCK.with(Cell::get) {
        return;
    }
    if !full_seed_promotes_on_first_copy(
        eden_live_bytes,
        eden_dead_bytes,
        desired_survivor_bytes(),
    ) {
        return;
    }
    let current = TENURING_SURVIVALS.with(Cell::get);
    PROMOTE_LOCK.with(|l| l.set(true));
    UNLOCK_STREAK.with(|s| s.set(0));
    RAISE_STREAK.with(|s| s.set(0));
    // PREV_COPIED_BYTES is deliberately untouched: it is the survival-rate
    // lock's denominator, owned by the copying path.
    set_survivals(current, 1, eden_live_bytes, "sweep-seed");
}

fn diag_cap_scale(from: u8, to: u8, eden_live_bytes: usize) {
    if std::env::var_os("PERRY_GC_DIAG").is_some() {
        eprintln!(
            "[gc-tenuring] nursery cap scale {from}x -> {to}x (eden_live_bytes={eden_live_bytes})"
        );
    }
}

fn set_survivals(current: u8, next: u8, eden_live_bytes: usize, why: &str) {
    if next == current {
        return;
    }
    TENURING_SURVIVALS.with(|s| s.set(next));
    if std::env::var_os("PERRY_GC_DIAG").is_some() {
        eprintln!(
            "[gc-tenuring] survivals {} -> {} ({why}, eden_live_bytes={} desired={})",
            current,
            next,
            eden_live_bytes,
            desired_survivor_bytes()
        );
    }
}

#[cfg(test)]
pub(super) fn reset_for_test() {
    TENURING_SURVIVALS.with(|s| s.set(GC_TENURING_SURVIVALS_MAX));
    RAISE_STREAK.with(|s| s.set(0));
    PROMOTE_LOCK.with(|l| l.set(false));
    UNLOCK_STREAK.with(|s| s.set(0));
    PREV_COPIED_BYTES.with(|c| c.set(0));
    NURSERY_CAP_SCALE.with(|s| s.set(1));
    CAP_GROW_STREAK.with(|s| s.set(0));
    CAP_SHRINK_STREAK.with(|s| s.set(0));
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: usize = 1024 * 1024;

    #[test]
    fn target_formula_matches_projected_occupancy() {
        // Largest S with (S-1) * influx <= desired.
        assert_eq!(compute_target_survivals(0, MB), 4);
        assert_eq!(compute_target_survivals(20 * 1024, MB), 4); // churn-like
        assert_eq!(compute_target_survivals(MB / 3 - 1, MB), 4);
        assert_eq!(compute_target_survivals(MB / 2, MB), 3);
        assert_eq!(compute_target_survivals(MB, MB), 2);
        assert_eq!(compute_target_survivals(MB + MB / 20, MB), 1); // tree-like
        assert_eq!(compute_target_survivals(16 * MB, MB), 1); // retain-like
    }

    #[test]
    fn drops_immediately_and_rises_debounced() {
        reset_for_test();
        let desired = desired_survivor_bytes();
        assert_eq!(tenuring_survivals(), 4);

        // Heavy influx: instant drop to 1.
        retune_after_scavenge(desired * 2, 0, 0);
        assert_eq!(tenuring_survivals(), 1);

        // One quiet cycle: no rise yet (debounce).
        retune_after_scavenge(0, 0, 0);
        assert_eq!(tenuring_survivals(), 1);
        // Second quiet cycle: rise by exactly one step, not to the target.
        retune_after_scavenge(0, 0, 0);
        assert_eq!(tenuring_survivals(), 2);

        // Heavy again: streak resets and threshold drops straight back.
        retune_after_scavenge(desired * 2, 0, 0);
        assert_eq!(tenuring_survivals(), 1);

        // Sustained quiet recovers to the ceiling two cycles per step.
        for _ in 0..6 {
            retune_after_scavenge(0, 0, 0);
        }
        assert_eq!(tenuring_survivals(), 4);
        reset_for_test();
    }

    #[test]
    fn steady_heavy_influx_is_a_fixed_point() {
        reset_for_test();
        // Heavy relative to the cap-scale CEILING (desired is cap/16, so at
        // the ×4 ceiling desired is base/4): the threshold must pin at 1 on
        // every cycle even while the cap scale walks up underneath it. An
        // influx only marginally above the base desired is a different case:
        // the growing cap re-classifies it as moderate, which is correct.
        let heavy = gc_scavenge_nursery_cap_bytes();
        for _ in 0..10 {
            retune_after_scavenge(heavy, 0, 0);
            assert_eq!(tenuring_survivals(), 1);
        }
        assert_eq!(
            scavenge_nursery_cap_effective_bytes(),
            gc_scavenge_nursery_cap_bytes() * NURSERY_CAP_SCALE_MAX as usize,
            "sustained heavy influx must also walk the cap to its ceiling"
        );
        reset_for_test();
    }

    #[test]
    fn survival_rate_lock_breaks_a_saturated_pipeline() {
        reset_for_test();
        let d = desired_survivor_bytes();
        // tree.ts steady state: influx sits JUST under desired (occupancy
        // alone settles at S=2), the survivor space holds 3 cohorts, and
        // 100% of every intake comes back out alive.
        let influx = d - 64;
        retune_after_scavenge(influx, 3 * d, 3 * d);
        assert_eq!(
            tenuring_survivals(),
            2,
            "first cycle has no prior intake to rate, so occupancy decides"
        );
        retune_after_scavenge(influx, 3 * d, 3 * d);
        assert_eq!(
            tenuring_survivals(),
            1,
            "a substantial intake that fully survives its round must lock promote-on-first-copy"
        );
        // The lock holds through occupancy readings that would say S=2.
        for _ in 0..5 {
            retune_after_scavenge(influx, 0, 0);
            assert_eq!(tenuring_survivals(), 1);
        }
        // Quiet influx exits the lock after the debounce, resuming the
        // ladder one step up rather than snapping to the ceiling.
        retune_after_scavenge(0, 0, 0);
        assert_eq!(tenuring_survivals(), 1);
        retune_after_scavenge(0, 0, 0);
        assert_eq!(tenuring_survivals(), 2);
        for _ in 0..4 {
            retune_after_scavenge(0, 0, 0);
        }
        assert_eq!(tenuring_survivals(), 4);
        reset_for_test();
    }

    #[test]
    fn cap_scale_grows_on_heavy_influx_and_shrinks_when_quiet() {
        reset_for_test();
        let base = gc_scavenge_nursery_cap_bytes();
        assert_eq!(scavenge_nursery_cap_effective_bytes(), base);
        // Influx above 4% of the cap: one debounce cycle, then a ×2 step.
        retune_after_scavenge(base / 15, 0, 0);
        assert_eq!(scavenge_nursery_cap_effective_bytes(), base);
        retune_after_scavenge(base / 15, 0, 0);
        assert_eq!(scavenge_nursery_cap_effective_bytes(), base * 2);
        // Growth halves the observed ratio into the dead band: stable.
        retune_after_scavenge(base / 15, 0, 0);
        retune_after_scavenge(base / 15, 0, 0);
        assert_eq!(scavenge_nursery_cap_effective_bytes(), base * 2);
        // Heavier influx reaches the ×4 ceiling and stops there.
        for _ in 0..4 {
            retune_after_scavenge(base, 0, 0);
        }
        assert_eq!(scavenge_nursery_cap_effective_bytes(), base * 4);
        // Quiet influx walks back one step at a time.
        for _ in 0..2 {
            retune_after_scavenge(0, 0, 0);
        }
        assert_eq!(scavenge_nursery_cap_effective_bytes(), base * 2);
        for _ in 0..2 {
            retune_after_scavenge(0, 0, 0);
        }
        assert_eq!(scavenge_nursery_cap_effective_bytes(), base);
        reset_for_test();
    }

    #[test]
    fn dying_survivor_cohorts_do_not_lock() {
        reset_for_test();
        let d = desired_survivor_bytes();
        // Medium-lived objects: a substantial intake of which only half
        // survives its survivor round. Aging is filtering — the lock must
        // stay out and the occupancy ladder must decide.
        for _ in 0..6 {
            retune_after_scavenge(d / 2, d / 2, d / 4);
            assert!(
                tenuring_survivals() >= 3,
                "a cohort that dies in the survivor space must keep aging (got {})",
                tenuring_survivals()
            );
        }
        reset_for_test();
    }

    // ── #7596's tenured-proportional cap term ───────────────────────────
    //
    // #7596 added `max(influx_driven, old_gen_reclaimable / 2)` to
    // `scavenge_nursery_cap_effective_bytes` with no test of its own. The
    // sibling `cap_scale_grows_on_heavy_influx_and_shrinks_when_quiet` looks
    // like coverage but is not: it asserts against the effective cap in a
    // unit-test thread whose old-gen is ~empty, so the proportional term
    // contributes 0 and the test stays green with that term deleted. These
    // two exercise the policy directly.

    #[test]
    fn nursery_cap_keeps_the_influx_floor_below_the_crossover() {
        // #7377's guarantee: a program whose tenured live set is small keeps
        // the configured 16 MB base. If the `.max()` were dropped — leaving
        // only `tenured / 2` — a small-live-set program would collect on a
        // near-zero cap, which is the regression #7377 fixed.
        let base = gc_scavenge_nursery_cap_bytes();
        assert_eq!(scavenge_nursery_cap_from(base, 0), base);
        assert_eq!(scavenge_nursery_cap_from(base, 1), base);
        // Just below the crossover the floor still decides.
        assert_eq!(
            scavenge_nursery_cap_from(base, base * TENURED_EDEN_DIVISOR - 1),
            base
        );
        // Exactly at it the two terms coincide.
        assert_eq!(
            scavenge_nursery_cap_from(base, base * TENURED_EDEN_DIVISOR),
            base
        );
        // The floor tracks the influx-driven scale, not the raw base: at the
        // ×4 ceiling a 3×base tenured set is still below the crossover.
        assert_eq!(scavenge_nursery_cap_from(base * 4, base * 3), base * 4);
    }

    #[test]
    fn nursery_cap_becomes_tenured_proportional_above_the_crossover() {
        // The #7592 shape: `json_pipeline` at 500k promotes ~400 MB. With a
        // constant cap the young collection count is linear in bytes
        // allocated and each collection is O(live) — quadratic total. Above
        // the crossover the cap must BE `tenured / 2`, so `old_{n+1} ≈
        // old_n × 1.5` and the count is logarithmic instead.
        //
        // If the proportional term were deleted, every assertion here would
        // read `base` and fail.
        let base = gc_scavenge_nursery_cap_bytes();
        let tenured = base * 50;
        assert_eq!(
            scavenge_nursery_cap_from(base, tenured),
            tenured / TENURED_EDEN_DIVISOR
        );
        // Strictly above the floor, and exactly the divisor — not merely
        // "bigger than base", which a wrong divisor would also satisfy.
        assert!(scavenge_nursery_cap_from(base, tenured) > base);
        assert_eq!(
            scavenge_nursery_cap_from(base, 400 * 1024 * 1024),
            200 * 1024 * 1024
        );
        // A larger influx-driven term still wins when it is the bigger of
        // the two: the policy is a max, not a takeover.
        assert_eq!(
            scavenge_nursery_cap_from(tenured, tenured),
            tenured,
            "the proportional term must never LOWER the cap"
        );
    }

    // ── #7598's mark-sweep seed ─────────────────────────────────────────
    //
    // The lock above cannot engage before the SECOND copying minor. These
    // exercise the seed that reads the same proof off a completed mark-sweep,
    // one collection earlier.

    #[test]
    fn sweep_seed_decides_before_the_first_copying_minor_snapshots_the_threshold() {
        // `copying.rs` snapshots `tenuring_survivals()` in
        // `CopyingNurseryCollector::new`, so the only value that can change
        // what the first big minor does is the one standing BEFORE any
        // `retune_after_scavenge` for that cycle has run. That is precisely
        // what the survival-rate lock cannot reach and this seed can.
        reset_for_test();
        let d = desired_survivor_bytes();
        let eden_live = d * 4;
        assert_eq!(
            tenuring_survivals(),
            4,
            "with no input the loop is at the ceiling: the wasted copy state"
        );

        seed_promote_lock_from_sweep(eden_live, eden_live / 50);
        assert_eq!(
            tenuring_survivals(),
            1,
            "the copying minor must ENTER at S=1, not be retuned to it afterwards"
        );
        reset_for_test();
    }

    #[test]
    fn sweep_seed_refuses_a_churn_eden() {
        // The stated failure mode, guarded: a big Eden is not a survival
        // signal. #7592 recorded exactly this trap ("Eden in-use at cycle
        // start is not a survival signal — churn workloads also overshoot
        // Eden, with ~0% survival"). Occupancy alone would say S=1 here.
        reset_for_test();
        let d = desired_survivor_bytes();
        let eden_live = d * 4;
        let eden_dead = eden_live * 9;
        assert_eq!(
            compute_target_survivals(eden_live, d),
            1,
            "precondition: occupancy alone says lock here, so this test is \
             exercising the survival-rate half and not passing vacuously"
        );
        seed_promote_lock_from_sweep(eden_live, eden_dead);
        assert_eq!(
            tenuring_survivals(),
            4,
            "10% Eden survival must not seed promote-on-first-copy"
        );
        reset_for_test();
    }

    #[test]
    fn sweep_seed_refuses_a_small_fully_live_eden() {
        // The other half: a nursery that is 100% live but far under the
        // survivor target fits the survivor space, so aging still filters and
        // the ladder must decide. Right after a scavenge this is the NORMAL
        // reading, and seeding off it would lock every program at S=1.
        reset_for_test();
        let d = desired_survivor_bytes();
        seed_promote_lock_from_sweep(d / 8, 0);
        assert_eq!(tenuring_survivals(), 4);
        reset_for_test();
    }

    #[test]
    fn sweep_seed_rule_is_a_pure_function_of_the_census() {
        // Table over the policy itself, with no heap state involved — the
        // sibling `scavenge_nursery_cap_from` exists for the same reason
        // (#7024: a test whose subject never ran).
        let d = 1024 * 1024;
        // Both conditions met.
        assert!(full_seed_promotes_on_first_copy(4 * d, d / 10, d));
        // Exactly at the 90% survival boundary: 9 live, 1 dead.
        assert!(full_seed_promotes_on_first_copy(9 * d, d, d));
        // One byte under it.
        assert!(!full_seed_promotes_on_first_copy(9 * d - 1, d + 1, d));
        // Occupancy says the cohort fits the survivor space.
        assert!(!full_seed_promotes_on_first_copy(d / 2, 0, d));
        // An empty census decides nothing (and must not divide by zero).
        assert!(!full_seed_promotes_on_first_copy(0, 0, d));
        assert!(!full_seed_promotes_on_first_copy(0, 4 * d, d));
    }

    #[test]
    fn sweep_seed_hands_over_to_the_existing_unlock_path() {
        // The seed sets the lock and nothing else: exit stays the influx
        // signal, which is measurable at S=1 (survivor occupancy is not).
        // Without this the seed would need an exit condition of its own, and
        // the obvious one — "Eden stopped being mostly live" — reads
        // differently at S=1 than at S=4 and would oscillate.
        reset_for_test();
        let d = desired_survivor_bytes();
        seed_promote_lock_from_sweep(d * 4, 0);
        assert_eq!(tenuring_survivals(), 1);

        // Substantial influx holds the lock, exactly as if it had been set by
        // the survivor round-trip.
        for _ in 0..4 {
            retune_after_scavenge(d * 4, 0, 0);
            assert_eq!(tenuring_survivals(), 1);
        }
        // Quiet influx exits after the same debounce, to the same S=2.
        retune_after_scavenge(0, 0, 0);
        assert_eq!(tenuring_survivals(), 1);
        retune_after_scavenge(0, 0, 0);
        assert_eq!(tenuring_survivals(), 2);
        reset_for_test();
    }

    #[test]
    fn effective_nursery_cap_is_the_two_term_policy() {
        // Wiring pin: the effective accessor must be the composition, so a
        // future edit that inlines one term and drops the other cannot pass
        // the two policy tests above while shipping a different cap.
        //
        // Honest about its limits: on a quiescent test thread old-gen is
        // ~empty, so this cannot distinguish the two terms by value — it only
        // proves the accessor and the policy agree on the live inputs, and
        // that the #7377 floor survives whatever old-gen happens to be.
        reset_for_test();
        let expected = scavenge_nursery_cap_from(
            influx_driven_nursery_cap_bytes(),
            old_gen_reclaimable_pressure_bytes(),
        );
        assert_eq!(scavenge_nursery_cap_effective_bytes(), expected);
        assert!(scavenge_nursery_cap_effective_bytes() >= gc_scavenge_nursery_cap_bytes());
        reset_for_test();
    }
}
