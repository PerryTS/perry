//! #10182: a full collection paced by the bytes promoted into old-gen since the
//! last full — the *promoted cohort* — rather than by old-gen growth.
//!
//! # Why the growth band cannot see this garbage
//!
//! Old-reclaim pacing measures `old_in_use - baseline`, and every promotion
//! credits the baseline (`credit_promoted_bytes_to_old_baseline`, #7592/#7965):
//! bytes a minor just moved into old-gen are growth the pacing decision has
//! already seen, and withholding the credit degenerates the band into a
//! constant on every retaining workload. So a promoted tree that dies after its
//! minor is invisible to old-reclaim. A document parse/scan loop lives in
//! exactly that blind spot: each parse result's top-level array is born old,
//! its young contents stay reachable through that array's remembered slots
//! until a full proves the array dead, so every nursery minor promotes the
//! previous (dead) tree together with the current one. On
//! `records_array_20m:parse` that is two trees per minor, one of them dead, and
//! old-gen holding every tree ever parsed until something else forces a full.
//!
//! # The bound
//!
//! A full is due when the cohort reaches `max(floor, live << backoff)`, where
//! `live` is the old-gen occupancy the last full verified and `floor` is one
//! base nursery. Each full costs O(live), so one full per `live` promoted bytes
//! keeps collector work proportional to allocation (the #7592 argument, applied
//! to promotions instead of arena growth) while capping how many dead cohorts
//! old-gen can hold.
//!
//! # Why this is not an old-reclaim arm
//!
//! #10204 measured the bound as a disjunct of `old_reclaim_pressure_due`. That
//! reintroduced the futile full `test_old_reclaim_band_is_proportional_and_promotion_credits_baseline`
//! and `an_untraced_promotion_credits_the_old_reclaim_baseline` pin: on a heap
//! whose promoted bytes are live (`retain`), a full scheduled because promotion
//! moved them frees nothing, and an allocation-point arm fires it behind a
//! forced conservative scan. Here, instead:
//!
//! * the bound is consulted in exactly one place, at a precise safepoint
//!   right after the nursery minor it applies to (`gc_safepoint_moving_minor`),
//!   so the full runs with precise roots and, after an in-place promotion, an
//!   empty young generation (its remembered-set rebuild is provably empty);
//! * `old_reclaim_pressure_due` and the credited baseline are untouched;
//! * a cohort full that reclaims less than half the cohort doubles the bound
//!   (up to `BACKOFF_SHIFT_MAX`), so a retaining heap pays a logarithmic number
//!   of futile fulls, each O(live), and a productive full restores it.

use std::cell::Cell;

/// A cohort full is productive when it reclaims at least this percentage of
/// the cohort it was scheduled for.
const PRODUCTIVE_PERCENT: usize = 50;
/// The bound doubles at most this many times on consecutive futile fulls.
const BACKOFF_SHIFT_MAX: u32 = 3;

crate::perry_thread_local! {
    /// Bytes promoted into old-gen since the last full collection.
    static PROMOTED_SINCE_FULL: Cell<usize> = const { Cell::new(0) };
    /// Old-gen occupancy (reclaimable pressure plus external side bytes) the
    /// last full collection left behind.
    static OLD_LIVE_AT_LAST_FULL: Cell<usize> = const { Cell::new(0) };
    /// Consecutive futile cohort fulls, capped at `BACKOFF_SHIFT_MAX`.
    static BACKOFF_SHIFT: Cell<u32> = const { Cell::new(0) };
    /// Cohort fulls run on this thread (live-subject counter).
    static COHORT_FULLS: Cell<u64> = const { Cell::new(0) };
}

/// A minor moved `bytes` into old-gen.
pub(super) fn note_promoted(bytes: usize) {
    PROMOTED_SINCE_FULL.with(|c| c.set(c.get().saturating_add(bytes)));
}

/// A full collection finished and verified `old_live` bytes of old-gen.
pub(super) fn note_full_finished(old_live: usize) {
    OLD_LIVE_AT_LAST_FULL.with(|c| c.set(old_live));
    PROMOTED_SINCE_FULL.with(|c| c.set(0));
}

pub(super) fn promoted_since_full() -> usize {
    PROMOTED_SINCE_FULL.with(Cell::get)
}

/// The cohort size at which a full becomes due.
pub(super) fn bound_bytes() -> usize {
    bound_from(
        super::policy::gc_scavenge_nursery_cap_bytes(),
        OLD_LIVE_AT_LAST_FULL.with(Cell::get),
        BACKOFF_SHIFT.with(Cell::get),
    )
}

/// `max(floor, live << shift)`, saturating.
pub(super) fn bound_from(floor: usize, old_live: usize, shift: u32) -> usize {
    floor.max(
        old_live
            .checked_shl(shift)
            .filter(|v| v >> shift == old_live)
            .unwrap_or(usize::MAX),
    )
}

/// Could a promotion of `young_bytes` bring the cohort to its bound?
pub(super) fn promotion_may_reach_bound(young_bytes: usize) -> bool {
    promoted_since_full().saturating_add(young_bytes) >= bound_bytes()
}

pub(super) fn full_due() -> bool {
    promoted_since_full() >= bound_bytes()
}

/// Price a finished cohort full: `reclaimed` old-gen bytes against the
/// `cohort` it was scheduled for.
pub(super) fn record_full_yield(cohort: usize, reclaimed: usize) -> bool {
    let productive = reclaimed.saturating_mul(100) >= cohort.saturating_mul(PRODUCTIVE_PERCENT);
    #[cfg(test)]
    let productive = productive || sabotage::never_back_off();
    BACKOFF_SHIFT.with(|shift| {
        if productive {
            shift.set(0);
        } else {
            shift.set(shift.get().saturating_add(1).min(BACKOFF_SHIFT_MAX));
        }
    });
    COHORT_FULLS.with(|c| c.set(c.get().saturating_add(1)));
    productive
}

pub(super) fn backoff_shift() -> u32 {
    BACKOFF_SHIFT.with(Cell::get)
}

#[cfg(test)]
pub(super) fn cohort_fulls() -> u64 {
    COHORT_FULLS.with(Cell::get)
}

/// What a promoted-cohort full's own mark says about the blocks the nursery
/// minor at the same safepoint promoted (#10241).
///
/// # Why this measurement, and why the cohort's yield is not enough
///
/// A minor promotes in place, and may skip its trace, on the strength of the
/// PREVIOUS minor's young-survival ratio. An untraced run re-measures only when
/// its byte budget runs out, and every full resets that budget
/// (`note_full_collection_reclaimed_old_gen`). A cohort full every ~`live`
/// promoted bytes therefore keeps a workload that turned from building a live
/// set to churning on the untraced path indefinitely: nothing ever measures the
/// churn, every minor promotes it, and every cohort full marks the whole live
/// set to reclaim it (`14_grow_then_churn`: 13 cohort fulls, 0 copied objects).
///
/// The cohort's yield cannot tell that apart from a parse loop whose promoted
/// trees die one tree later: both reclaim about the whole cohort. What differs
/// is the survival of the blocks the LAST minor promoted. On a parse loop they
/// hold the tree still being built and the tail of the previous one, both
/// reachable; on a churn phase they are the churn, dead by the full. The full's
/// mark is exact (full reachability, no remembered-set conservatism), and the
/// sweep reads it block by block anyway, so the figure costs one map lookup per
/// swept block of the cohort full and nothing anywhere else.
///
/// # Exact or nothing
///
/// The blocks are the ones the minor's promotion walk recorded for census
/// adoption (`trace::adopt_census`), keyed by data address with the bump extent
/// and header bytes they held at promotion. Each is accounted when the full's
/// sweep either walks it whole at the same extent (its live bytes are the
/// sweep's own `arena_live_bytes` delta) or reclaims it unwalked as a dead
/// block (live 0). A block the sweep reaches any other way, at another extent,
/// or not at all leaves the measurement unset.
pub(super) struct PromotedSurvival {
    pub(super) blocks: usize,
    pub(super) promoted_bytes: usize,
    pub(super) live_bytes: usize,
}

impl PromotedSurvival {
    pub(super) fn permille(&self) -> Option<u64> {
        (self.promoted_bytes > 0).then(|| {
            (self.live_bytes as u64)
                .saturating_mul(1000)
                .checked_div(self.promoted_bytes as u64)
                .unwrap_or(0)
                .min(1000)
        })
    }
}

struct SurvivalProbe {
    /// Recorded blocks not yet accounted: data address -> (extent, bytes).
    pending: crate::fast_hash::PtrHashMap<usize, (usize, u64)>,
    blocks: usize,
    promoted_bytes: u64,
    live_bytes: u64,
    exact: bool,
}

crate::perry_thread_local! {
    /// The armed probe of the cohort full in progress, if any. Holds arena
    /// block data addresses only as identity keys for the sweep's snapshot;
    /// nothing is read through them.
    static SURVIVAL_PROBE: std::cell::RefCell<Option<SurvivalProbe>> =
        const { std::cell::RefCell::new(None) };
}

/// Arm the probe over `(data, extent, bytes)` blocks before the cohort full.
pub(super) fn arm_survival_probe(blocks: Vec<(usize, usize, u64)>) {
    let mut pending = crate::fast_hash::new_ptr_hash_map();
    let mut promoted_bytes = 0u64;
    for (data, extent, bytes) in blocks {
        promoted_bytes = promoted_bytes.saturating_add(bytes);
        pending.insert(data, (extent, bytes));
    }
    let probe = (!pending.is_empty()).then(|| SurvivalProbe {
        blocks: pending.len(),
        pending,
        promoted_bytes,
        live_bytes: 0,
        exact: true,
    });
    SURVIVAL_PROBE.with(|p| *p.borrow_mut() = probe);
}

/// Read once per synchronous full sweep.
pub(super) fn survival_probe_armed() -> bool {
    SURVIVAL_PROBE.with(|p| p.borrow().is_some())
}

/// The sweep walked the block at `data` whole, to `extent`, and kept `live`
/// bytes of it.
pub(super) fn note_probe_block_swept(data: usize, extent: usize, live: u64) {
    account_probe_block(data, extent, live);
}

/// The sweep reclaimed the block at `data` without walking it: nothing in it
/// was reached.
pub(super) fn note_probe_block_skipped(data: usize, extent: usize) {
    account_probe_block(data, extent, 0);
}

fn account_probe_block(data: usize, extent: usize, live: u64) {
    SURVIVAL_PROBE.with(|p| {
        let mut probe = p.borrow_mut();
        let Some(probe) = probe.as_mut() else {
            return;
        };
        let Some((recorded_extent, _)) = probe.pending.remove(&data) else {
            return;
        };
        if recorded_extent != extent {
            probe.exact = false;
        }
        probe.live_bytes = probe.live_bytes.saturating_add(live);
    });
}

/// Disarm the probe; the measurement when every recorded block was accounted.
pub(super) fn take_survival_probe() -> Option<PromotedSurvival> {
    let probe = SURVIVAL_PROBE.with(|p| p.borrow_mut().take())?;
    let survival = (probe.exact && probe.pending.is_empty()).then(|| PromotedSurvival {
        blocks: probe.blocks,
        promoted_bytes: usize::try_from(probe.promoted_bytes).unwrap_or(usize::MAX),
        live_bytes: usize::try_from(probe.live_bytes).unwrap_or(usize::MAX),
    });
    #[cfg(test)]
    LAST_SURVIVAL_FOR_TESTS.with(|c| {
        c.set(
            survival
                .as_ref()
                .map(|s| (s.blocks, s.promoted_bytes, s.live_bytes)),
        )
    });
    survival
}

#[cfg(test)]
thread_local! {
    static LAST_SURVIVAL_FOR_TESTS: std::cell::Cell<Option<(usize, usize, usize)>> =
        const { std::cell::Cell::new(None) };
}

/// `(blocks, promoted bytes, live bytes)` of the last probe taken, when it
/// measured (tests only).
#[cfg(test)]
pub(super) fn last_survival_for_tests() -> Option<(usize, usize, usize)> {
    LAST_SURVIVAL_FOR_TESTS.with(std::cell::Cell::get)
}

/// Sabotage switch for the survival tests: the cohort full measures but does
/// not feed the promotion predictor. Test builds only.
#[cfg(test)]
pub(super) mod survival_sabotage {
    use std::cell::Cell;

    thread_local! {
        static UNFED: Cell<bool> = const { Cell::new(false) };
    }

    pub(in crate::gc) fn unfed() -> bool {
        UNFED.with(Cell::get)
    }

    pub(crate) struct Guard(bool);

    impl Guard {
        pub(crate) fn arm() -> Self {
            Self(UNFED.with(|s| s.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            UNFED.with(|s| s.set(self.0));
        }
    }
}

/// Sabotage switch for the cohort tests: every cohort full counts as
/// productive, so the bound never backs off. Test builds only.
#[cfg(test)]
pub(super) mod sabotage {
    use std::cell::Cell;

    thread_local! {
        static NEVER_BACK_OFF: Cell<bool> = const { Cell::new(false) };
    }

    pub(super) fn never_back_off() -> bool {
        NEVER_BACK_OFF.with(Cell::get)
    }

    pub(crate) struct Guard(bool);

    impl Guard {
        pub(crate) fn arm() -> Self {
            Self(NEVER_BACK_OFF.with(|s| s.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            NEVER_BACK_OFF.with(|s| s.set(self.0));
        }
    }
}

/// Seed the cohort state (tests only).
#[cfg(test)]
pub(super) fn seed_for_tests(promoted_since_full: usize, old_live: usize, shift: u32) {
    PROMOTED_SINCE_FULL.with(|c| c.set(promoted_since_full));
    OLD_LIVE_AT_LAST_FULL.with(|c| c.set(old_live));
    BACKOFF_SHIFT.with(|c| c.set(shift));
}
