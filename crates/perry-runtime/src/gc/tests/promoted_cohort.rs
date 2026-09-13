//! #10182: old-reclaim is due once the promoted-but-unverified cohort exceeds
//! `max(floor, 2 × old live at last full)`, independently of the growth band
//! (which promotion credits blind) and of the retaining exemption.

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
fn promoted_cohort_makes_old_reclaim_due_even_when_growth_reads_zero_and_the_heap_retains() {
    let _isolation = GcTestIsolationGuard::new();
    let _pacing = crate::gc::policy::force_moving_gc_pacing();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let previous_retaining = GC_MAJOR_PACING_RETAINING.with(std::cell::Cell::get);
    let floor = gc_promoted_cohort_floor_dyn_bytes();
    // Growth reads zero (old_in_use == baseline) and the heap is classified
    // retaining, so neither existing arm can fire: only the cohort can.
    GC_MAJOR_PACING_RETAINING.with(|c| c.set(true));
    let occupancy = 10 * floor;

    seed_promoted_cohort_for_tests(floor - 1, 0);
    assert!(
        !old_reclaim_pressure_due(occupancy, occupancy),
        "below the floor: not due"
    );
    seed_promoted_cohort_for_tests(floor, 0);
    assert!(
        old_reclaim_pressure_due(occupancy, occupancy),
        "at the floor: due"
    );
    seed_promoted_cohort_for_tests(floor, floor);
    assert!(
        !old_reclaim_pressure_due(occupancy, occupancy),
        "below 2 × live: not due"
    );
    seed_promoted_cohort_for_tests(2 * floor, floor);
    assert!(
        old_reclaim_pressure_due(occupancy, occupancy),
        "at 2 × live: due"
    );

    seed_promoted_cohort_for_tests(0, 0);
    GC_MAJOR_PACING_RETAINING.with(|c| c.set(previous_retaining));
}
