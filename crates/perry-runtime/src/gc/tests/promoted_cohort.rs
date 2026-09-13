//! #10182: the promoted-cohort full (`gc::promoted_cohort`).
//!
//! The arithmetic, the futility backoff that keeps it from charging a
//! retaining heap a full per cohort, the proof that it is not an old-reclaim
//! arm (the two baseline tests #10204 broke keep their meaning), and one real
//! collection: a promoting minor credits the cohort, and the cohort full that
//! follows it reclaims a promoted object that died.

use super::super::policy::{
    credit_promoted_bytes_to_old_baseline, old_reclaim_pressure_due,
    run_promoted_cohort_full_if_due, GC_LAST_OLD_RECLAIM_IN_USE_BYTES, GC_MAJOR_PACING_RETAINING,
};
use super::super::promoted_cohort as cohort;
use super::super::*;
use super::support::*;

const MB: usize = 1024 * 1024;

#[test]
fn the_bound_is_one_nursery_or_the_verified_live_set_doubled_per_futile_full() {
    assert_eq!(cohort::bound_from(16 * MB, 0, 0), 16 * MB);
    assert_eq!(cohort::bound_from(16 * MB, 8 * MB, 0), 16 * MB);
    assert_eq!(cohort::bound_from(16 * MB, 49 * MB, 0), 49 * MB);
    assert_eq!(cohort::bound_from(16 * MB, 49 * MB, 2), 196 * MB);
    assert_eq!(
        cohort::bound_from(16 * MB, usize::MAX / 2, 3),
        usize::MAX,
        "the shifted live set saturates instead of wrapping"
    );
}

#[test]
fn a_futile_cohort_full_doubles_the_bound_and_a_productive_one_restores_it() {
    let _iso = GcTestIsolationGuard::new();
    cohort::seed_for_tests(0, 49 * MB, 0);
    assert!(
        !cohort::record_full_yield(58 * MB, 28 * MB),
        "48% is futile"
    );
    assert_eq!(cohort::backoff_shift(), 1);
    for _ in 0..8 {
        cohort::record_full_yield(58 * MB, 0);
    }
    assert_eq!(cohort::backoff_shift(), 3, "the backoff is capped");
    assert!(
        cohort::record_full_yield(58 * MB, 29 * MB),
        "50% is productive"
    );
    assert_eq!(cohort::backoff_shift(), 0);
    cohort::seed_for_tests(0, 0, 0);
}

/// Replay `retain`'s measured untraced promotion schedule (the one
/// `an_untraced_promotion_credits_the_old_reclaim_baseline` uses) through the
/// cohort arm on a heap where every promoted byte stays live, so every cohort
/// full is futile. Returns how many cohort fulls the schedule paid for.
fn retain_schedule_cohort_fulls() -> usize {
    const RETAIN_UNTRACED_PROMOTION_BYTES: [usize; 4] =
        [18_742_816, 26_213_656, 35_650_552, 37_747_640];
    cohort::seed_for_tests(0, 0, 0);
    let mut old_live = 0usize;
    let mut fulls = 0usize;
    for step in RETAIN_UNTRACED_PROMOTION_BYTES.iter().cycle().take(64) {
        old_live += step;
        cohort::note_promoted(*step);
        if cohort::full_due() {
            let promoted = cohort::promoted_since_full();
            cohort::note_full_finished(old_live);
            cohort::record_full_yield(promoted, 0);
            fulls += 1;
        }
    }
    cohort::seed_for_tests(0, 0, 0);
    fulls
}

#[test]
fn a_retaining_promotion_schedule_pays_a_bounded_number_of_futile_cohort_fulls() {
    let _iso = GcTestIsolationGuard::new();
    let fulls = retain_schedule_cohort_fulls();
    assert!(
        fulls <= 4,
        "64 promotions of live data (~1.9 GB) may cost at most a handful of futile \
         fulls, each O(live): {fulls}"
    );
}

#[test]
fn sabotaged_backoff_charges_a_retaining_schedule_a_full_per_live_set() {
    let _iso = GcTestIsolationGuard::new();
    let fulls = {
        let _sabotage = cohort::sabotage::Guard::arm();
        retain_schedule_cohort_fulls()
    };
    assert!(
        fulls > 4,
        "without the backoff the same schedule pays a full each time the cohort \
         reaches the live set: {fulls}"
    );
}

/// The two baseline tests #10204 broke by adding the bound to
/// `old_reclaim_pressure_due` pin that promoted bytes alone never make old
/// reclaim due. The cohort is not consulted there, however large it is.
#[test]
fn the_cohort_never_makes_old_reclaim_due() {
    let _iso = GcTestIsolationGuard::new();
    let previous_retaining = GC_MAJOR_PACING_RETAINING.with(std::cell::Cell::get);
    let previous_baseline = GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(std::cell::Cell::get);
    GC_MAJOR_PACING_RETAINING.with(|c| c.set(true));
    GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|c| c.set(4 * MB));
    cohort::seed_for_tests(0, 0, 0);
    credit_promoted_bytes_to_old_baseline(270 * MB);
    assert!(
        cohort::full_due(),
        "premise: the cohort is far past its bound"
    );
    let baseline = GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(std::cell::Cell::get);
    assert!(!old_reclaim_pressure_due(274 * MB, baseline));
    cohort::seed_for_tests(0, 0, 0);
    GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|c| c.set(previous_baseline));
    GC_MAJOR_PACING_RETAINING.with(|c| c.set(previous_retaining));
}

/// A real untraced in-place promotion credits the cohort by exactly the bytes
/// it moved; one of the promoted leaves then dies, and the cohort full reclaims
/// it and keeps the other. Below the bound the same state runs no full.
fn promote_two_leaves_then_drop_one(cohort_due: bool) -> (bool, u8, u8, usize) {
    let _guard = CopyingNurseryTestGuard::new(4);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _promote = super::super::InPlacePromotionTestGuard::untraced();
    cohort::seed_for_tests(0, 0, 0);

    let kept = young_leaf();
    let dropped = young_leaf();
    js_shadow_slot_set(0, string_bits(kept));
    js_shadow_slot_set(1, string_bits(dropped));
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert!(
        trace.copying_nursery.in_place_promotion && trace.copying_nursery.promoted_bytes > 0,
        "premise: the minor promoted in place"
    );
    assert_eq!(
        cohort::promoted_since_full(),
        trace.copying_nursery.promoted_bytes,
        "the cohort is credited with exactly the promoted bytes"
    );
    assert!(crate::arena::pointer_in_old_gen(dropped));
    js_shadow_slot_set(1, crate::value::TAG_UNDEFINED);
    if cohort_due {
        cohort::seed_for_tests(cohort::bound_bytes(), 0, 0);
    }
    let fulls_before = cohort::cohort_fulls();
    let ran = run_promoted_cohort_full_if_due();
    assert_eq!(cohort::cohort_fulls() - fulls_before, u64::from(ran));
    let type_of = |user: usize| unsafe { (*header_from_user_ptr(user as *const u8)).obj_type };
    let result = (
        ran,
        type_of(kept),
        type_of(dropped),
        cohort::promoted_since_full(),
    );
    js_shadow_slot_set(0, crate::value::TAG_UNDEFINED);
    cohort::seed_for_tests(0, 0, 0);
    result
}

#[test]
fn a_cohort_full_after_a_promotion_reclaims_the_promoted_object_that_died() {
    let (ran, kept, dropped, cohort_after) = promote_two_leaves_then_drop_one(true);
    assert!(ran, "the cohort at its bound must run the full");
    assert_eq!(kept, GC_TYPE_STRING, "the rooted promoted leaf survives");
    assert_eq!(dropped, 0, "the promoted leaf that died is reclaimed");
    assert_eq!(cohort_after, 0, "the full restarts the cohort");
}

#[test]
fn below_the_bound_no_cohort_full_runs_and_the_dead_promoted_object_stays() {
    let (ran, kept, dropped, _) = promote_two_leaves_then_drop_one(false);
    assert!(!ran);
    assert_eq!(kept, GC_TYPE_STRING);
    assert_eq!(
        dropped, GC_TYPE_STRING,
        "only a full can reclaim a promoted object"
    );
}
