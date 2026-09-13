//! #10182: the promoted-but-unverified cohort bound, `max(floor, 2 × old live
//! at last full)` with a floor of one base nursery cap. It is read one promotion
//! ahead by the nursery-safepoint pre-emption and is never an arm of
//! `old_reclaim_pressure_due` (which would reinstate #7592's futile full).

use super::super::policy::{
    old_reclaim_pressure_due, promoted_cohort_bound_bytes, seed_promoted_cohort_for_tests,
    GC_MAJOR_PACING_RETAINING,
};
use super::super::*;
use super::support::*;

#[test]
fn promoted_cohort_bound_is_the_floor_or_twice_the_verified_live_set() {
    let floor = gc_promoted_cohort_floor_dyn_bytes();
    assert_eq!(promoted_cohort_bound_bytes(0), floor);
    assert_eq!(promoted_cohort_bound_bytes(floor / 4), floor);
    assert_eq!(promoted_cohort_bound_bytes(floor), 2 * floor);
    assert_eq!(promoted_cohort_bound_bytes(3 * floor), 6 * floor);
}

#[test]
fn the_cohort_bound_never_makes_old_reclaim_due_on_its_own() {
    // #7592/#7965: a full that is due only because promotion moved bytes into
    // old-gen frees nothing. The cohort bound must not reintroduce that arm —
    // however large the unverified cohort, growth pacing reads zero here and
    // only the safepoint pre-emption may act on the bound.
    let _isolation = GcTestIsolationGuard::new();
    let _pacing = crate::gc::policy::force_moving_gc_pacing();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let previous_retaining = GC_MAJOR_PACING_RETAINING.with(std::cell::Cell::get);
    GC_MAJOR_PACING_RETAINING.with(|c| c.set(true));
    let floor = gc_promoted_cohort_floor_dyn_bytes();
    let occupancy = 10 * floor;
    seed_promoted_cohort_for_tests(100 * floor, 0);
    assert!(
        super::super::policy::promoted_cohort_bound_due_with(0),
        "precondition: the cohort is far past its bound"
    );
    assert!(
        !old_reclaim_pressure_due(occupancy, occupancy),
        "the cohort bound alone must not schedule an old reclaim"
    );
    seed_promoted_cohort_for_tests(0, 0);
    GC_MAJOR_PACING_RETAINING.with(|c| c.set(previous_retaining));
}

#[test]
fn the_cohort_floor_is_one_base_nursery_cap() {
    // Unconstrained process: the budget-scaled accessor returns its default.
    if gc_heap_budget_bytes().is_none() {
        assert_eq!(
            gc_promoted_cohort_floor_dyn_bytes(),
            super::super::policy::gc_scavenge_nursery_cap_bytes(),
            "the floor is one nursery quantum, not a separate constant"
        );
    }
}

#[test]
fn the_cohort_bound_can_be_asked_one_promotion_ahead() {
    // `promoting_full_preempts_nursery_minor` asks whether the promotion a
    // minor is ABOUT to perform would make the bound due. The pending bytes
    // must count exactly like already-promoted bytes, at both edges.
    let _isolation = GcTestIsolationGuard::new();
    let floor = gc_promoted_cohort_floor_dyn_bytes();
    seed_promoted_cohort_for_tests(floor / 2, 0);
    assert!(!super::super::policy::promoted_cohort_bound_due_with(0));
    assert!(!super::super::policy::promoted_cohort_bound_due_with(
        floor - floor / 2 - 1
    ));
    assert!(super::super::policy::promoted_cohort_bound_due_with(
        floor - floor / 2
    ));
    seed_promoted_cohort_for_tests(0, 0);
}
