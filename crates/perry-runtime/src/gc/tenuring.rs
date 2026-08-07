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
    let influx_driven =
        gc_scavenge_nursery_cap_bytes().saturating_mul(NURSERY_CAP_SCALE.with(Cell::get) as usize);
    let tenured_proportional = old_gen_reclaimable_pressure_bytes() / TENURED_EDEN_DIVISOR;
    influx_driven.max(tenured_proportional)
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
}
