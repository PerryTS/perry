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
//!
//! # Only in-place promotions count (#10241)
//!
//! A copying minor tenures an object only after it has already survived a
//! minor, so tenured bytes are live by the one measurement a minor has, and a
//! full scheduled for them is futile by the same measure. The blind spot above
//! is the in-place promotion's alone: it moves a young generation wholesale,
//! dead trees included. Measured: `12_large_live_set`'s only cohort full was
//! reached by 21.2 MB of copy-tenured bytes over a 16 MB bound and reclaimed
//! 8.3 MB (futile; wall 0.25 s → 0.28 s against base), and so was
//! `14_grow_then_churn`'s first (2.1 MB tenured, reclaimed 0), while every
//! cohort full on the JSON rows was reached by in-place promotions alone. The
//! old-reclaim baseline credit is unchanged: it still takes every promoted
//! byte.

use std::cell::Cell;

/// What the cohort full's own mark says about the blocks the minor at its
/// safepoint promoted (#10241).
pub(super) mod survival;

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

/// A minor moved `bytes` into old-gen by promoting its blocks in place.
pub(super) fn note_promoted(bytes: usize) {
    PROMOTED_SINCE_FULL.with(|c| c.set(c.get().saturating_add(bytes)));
}

/// A copying minor promoted `promoted` bytes, `in_place` of them by promoting
/// blocks in place and the rest by tenuring copies. The cohort takes the
/// in-place share.
pub(super) fn note_minor_promotion(promoted: usize, in_place: usize) {
    #[cfg(test)]
    let in_place = if sabotage::counting_tenured() {
        promoted
    } else {
        in_place
    };
    let _ = promoted;
    note_promoted(in_place);
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

/// Sabotage switch for the cohort tests: every cohort full counts as
/// productive, so the bound never backs off. Test builds only.
#[cfg(test)]
pub(super) mod sabotage {
    use std::cell::Cell;

    thread_local! {
        static NEVER_BACK_OFF: Cell<bool> = const { Cell::new(false) };
        static COUNT_TENURED: Cell<bool> = const { Cell::new(false) };
    }

    pub(super) fn never_back_off() -> bool {
        NEVER_BACK_OFF.with(Cell::get)
    }

    /// The cohort takes copy-tenured bytes too, as it did before #10241.
    pub(super) fn counting_tenured() -> bool {
        COUNT_TENURED.with(Cell::get)
    }

    pub(crate) struct CountTenuredGuard(bool);

    impl CountTenuredGuard {
        pub(crate) fn arm() -> Self {
            Self(COUNT_TENURED.with(|s| s.replace(true)))
        }
    }

    impl Drop for CountTenuredGuard {
        fn drop(&mut self) {
            COUNT_TENURED.with(|s| s.set(self.0));
        }
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
