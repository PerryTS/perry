//! #10182: the promoted-cohort full (`gc::promoted_cohort`).
//!
//! The arithmetic, the futility backoff that keeps it from charging a
//! retaining heap a full per cohort, the proof that it is not an old-reclaim
//! arm (the two baseline tests #10204 broke keep their meaning), and one real
//! collection: a promoting minor credits the cohort, and the cohort full that
//! follows it reclaims a promoted object that died.
//!
//! #10241: the cohort full measures the survival of what the minor at its own
//! safepoint promoted. A dead same-safepoint cohort turns the next minor back
//! into an evacuating one (sabotaged twin: the predictor is not fed), a live
//! one leaves the predictor alone, and a dead remembered parent referring into
//! the cohort — the parse-loop shape, where a minor would have measured the
//! cohort live — feeds nothing (sabotaged twin: the parent check is skipped).

use super::super::policy::{
    credit_promoted_bytes_to_old_baseline, old_reclaim_pressure_due,
    run_promoted_cohort_full_if_due, GC_LAST_OLD_RECLAIM_IN_USE_BYTES, GC_MAJOR_PACING_RETAINING,
};
use super::super::promoted_cohort as cohort;
use super::super::promoted_cohort::survival;
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
    cohort::note_promoted(270 * MB);
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

/// #10241: rooted young strings survive evacuating minors until they tenure,
/// with the cohort one byte short of its bound. Returns the bytes tenured, the
/// cohort growth, and whether the cohort full then ran.
fn tenure_into_a_cohort_one_byte_short(count_tenured: bool) -> (usize, usize, bool) {
    let _guard = CopyingNurseryTestGuard::new(4);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _tenuring = crate::gc::tenuring::set_survivals_for_test(1);
    let _sabotage = count_tenured.then(cohort::sabotage::CountTenuredGuard::arm);
    rooted_young_strings(0, 4000);
    let short = cohort::bound_bytes() - 1;
    cohort::seed_for_tests(short, 0, 0);
    let mut tenured = 0usize;
    for _ in 0..3 {
        let trace = collect_minor_trace(GcTriggerKind::Direct);
        assert!(
            !trace.copying_nursery.in_place_promotion,
            "premise: evacuating minors, nothing promoted in place"
        );
        tenured +=
            trace.copying_nursery.promoted_bytes - trace.copying_nursery.in_place_promoted_bytes;
    }
    let grown = cohort::promoted_since_full() - short;
    let ran = run_promoted_cohort_full_if_due();
    js_shadow_slot_set(0, crate::value::TAG_UNDEFINED);
    cohort::seed_for_tests(0, 0, 0);
    (tenured, grown, ran)
}

#[test]
fn copy_tenured_bytes_do_not_bring_the_cohort_to_its_bound() {
    let (tenured, grown, ran) = tenure_into_a_cohort_one_byte_short(false);
    assert!(
        tenured > 0,
        "premise: the minors tenured the rooted strings"
    );
    assert_eq!(grown, 0, "the cohort counts in-place promotions only");
    assert!(
        !ran,
        "tenured bytes survived a minor already: no full is scheduled for them \
         (12_large_live_set's futile cohort full)"
    );
}

#[test]
fn sabotaged_tenured_accounting_schedules_a_full_for_live_survivors() {
    let (tenured, grown, ran) = tenure_into_a_cohort_one_byte_short(true);
    assert!(
        tenured > 0,
        "premise: the minors tenured the rooted strings"
    );
    assert_eq!(
        grown, tenured,
        "sabotaged, the cohort takes the tenured bytes"
    );
    assert!(
        ran,
        "and the byte short of the bound is made up by live survivors"
    );
}

/// What the young population promoted at the cohort's safepoint looks like at
/// the full.
#[derive(Clone, Copy, PartialEq)]
enum CohortShape {
    /// Still rooted.
    Live,
    /// Dropped; a rooted old parent keeps one young string on a dirty page, so
    /// the full checks a remembered parent and finds it live.
    DeadWithLiveParent,
    /// Dropped; an unrooted old parent keeps young strings on a dirty page —
    /// the born-old array of a parsed tree that died.
    DeadWithDeadParent,
}

#[derive(Clone, Copy, PartialEq)]
enum Sabotage {
    None,
    Unfed,
    SkipParentCheck,
}

/// Outcome of one untraced promotion, a cohort full at the same safepoint, and
/// the minor after it.
struct SurvivalOutcome {
    /// `(blocks, promoted bytes, live bytes, minor view)` the full measured.
    measured: Option<(usize, usize, usize, survival::MinorView)>,
    predictor_after_full: Option<u64>,
    next_minor_in_place: bool,
    next_minor_untraced: bool,
}

/// Root an array of `count` young strings in `slot`; returns the first string.
fn rooted_young_strings(slot: u32, count: usize) -> usize {
    let mut array = crate::array::js_array_alloc(count as u32);
    let first = young_leaf();
    array = crate::array::js_array_push_jsvalue(array, string_bits(first));
    for _ in 1..count {
        array = crate::array::js_array_push_jsvalue(array, string_bits(young_leaf()));
    }
    js_shadow_slot_set(slot, ptr_bits(array as usize));
    first
}

/// An old object whose `fields` young strings are stored through the runtime
/// barrier, so its page is in the remembered set the next minor snapshots.
fn old_parent_of_young_strings(fields: u32) -> usize {
    let (parent, slots) = unsafe { alloc_old_test_object(fields) };
    for i in 0..fields as usize {
        let bits = string_bits(young_leaf());
        unsafe {
            *slots.add(i) = bits;
        }
        runtime_write_barrier_slot(parent as usize, unsafe { slots.add(i) } as usize, bits);
    }
    parent as usize
}

/// A young population promoted whole and untraced by a minor that records its
/// blocks, then the cohort full at the same safepoint in `shape`, then a minor
/// over a fresh rooted population.
fn promote_then_cohort_full_then_minor(shape: CohortShape, sabotage: Sabotage) -> SurvivalOutcome {
    use super::super::trace::adopt_census;
    let _guard = CopyingNurseryTestGuard::new(4);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _promote = super::super::InPlacePromotionTestGuard::untraced();
    cohort::seed_for_tests(0, 0, 0);

    const COUNT: usize = 6000;
    let probe_leaf = rooted_young_strings(0, COUNT);
    assert!(
        crate::arena::pointer_in_nursery(probe_leaf),
        "premise: young population"
    );
    match shape {
        CohortShape::Live => {}
        CohortShape::DeadWithLiveParent => {
            js_shadow_slot_set(2, ptr_bits(old_parent_of_young_strings(1)));
        }
        CohortShape::DeadWithDeadParent => {
            old_parent_of_young_strings(64);
        }
    }
    if shape != CohortShape::Live {
        assert!(
            super::super::barrier::remembered_dirty_page_count() > 0,
            "premise: the old parent's page is remembered"
        );
    }
    let untraced_before = untraced_promotion_cycles();
    adopt_census::begin_recording();
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    adopt_census::finish_recording();
    assert!(
        trace.copying_nursery.in_place_promotion,
        "premise: the minor promoted in place"
    );
    assert_eq!(
        untraced_promotion_cycles() - untraced_before,
        1,
        "premise: the promotion skipped the trace"
    );
    if shape != CohortShape::Live {
        js_shadow_slot_set(0, crate::value::TAG_UNDEFINED);
    }
    cohort::seed_for_tests(cohort::bound_bytes(), 0, 0);
    let ran = {
        let _sabotage = match sabotage {
            Sabotage::None => None,
            Sabotage::Unfed => Some(survival::sabotage::Guard::unfed()),
            Sabotage::SkipParentCheck => Some(survival::sabotage::Guard::skip_parent_check()),
        };
        run_promoted_cohort_full_if_due()
    };
    adopt_census::discard();
    assert!(ran, "premise: the cohort full ran");
    let measured = survival::last_survival_for_tests();
    let predictor_after_full = super::super::last_young_survival_permille();

    rooted_young_strings(1, COUNT);
    let untraced_before = untraced_promotion_cycles();
    let next = collect_minor_trace(GcTriggerKind::Direct);
    let outcome = SurvivalOutcome {
        measured,
        predictor_after_full,
        next_minor_in_place: next.copying_nursery.in_place_promotion,
        next_minor_untraced: untraced_promotion_cycles() - untraced_before == 1,
    };
    for slot in 0..3 {
        js_shadow_slot_set(slot, crate::value::TAG_UNDEFINED);
    }
    cohort::seed_for_tests(0, 0, 0);
    outcome
}

fn permille((_, promoted, live, _): (usize, usize, usize, survival::MinorView)) -> u64 {
    (live as u64 * 1000 / promoted as u64).min(1000)
}

fn dead_cohort_measurement(
    outcome: &SurvivalOutcome,
) -> (usize, usize, usize, survival::MinorView) {
    let measured = outcome
        .measured
        .expect("the full's sweep accounted every recorded block");
    assert!(
        measured.0 >= 1 && measured.1 > 0,
        "the probe measured the promoted blocks: {measured:?}"
    );
    assert!(
        permille(measured) < super::super::PROMOTE_SURVIVAL_THRESHOLD_PERMILLE,
        "the dropped cohort is dead by the full: {measured:?}"
    );
    measured
}

#[test]
fn a_dead_same_safepoint_cohort_turns_the_next_minor_into_an_evacuation() {
    let outcome =
        promote_then_cohort_full_then_minor(CohortShape::DeadWithLiveParent, Sabotage::None);
    let measured = dead_cohort_measurement(&outcome);
    assert_eq!(
        measured.3,
        survival::MinorView::Exact,
        "the only remembered parent is live, so a minor measures what the full did"
    );
    assert_eq!(
        outcome.predictor_after_full,
        Some(permille(measured)),
        "the full's measurement replaces the ratio the promotion was admitted on"
    );
    assert!(
        !outcome.next_minor_in_place && !outcome.next_minor_untraced,
        "the next minor evacuates and measures instead of promoting on faith"
    );
}

#[test]
fn sabotaged_unfed_predictor_promotes_the_next_minor_untraced() {
    let outcome =
        promote_then_cohort_full_then_minor(CohortShape::DeadWithLiveParent, Sabotage::Unfed);
    let measured = dead_cohort_measurement(&outcome);
    assert_eq!(
        measured.3,
        survival::MinorView::Exact,
        "premise: exact view"
    );
    assert_eq!(
        outcome.predictor_after_full,
        Some(1000),
        "unfed, the predictor keeps the ratio the dead cohort was promoted on"
    );
    assert!(
        outcome.next_minor_in_place && outcome.next_minor_untraced,
        "without the feed the next minor promotes untraced again — the \
         14_grow_then_churn regime, where no minor ever measures the churn"
    );
}

#[test]
fn a_live_same_safepoint_cohort_leaves_the_predictor_at_retained() {
    let outcome = promote_then_cohort_full_then_minor(CohortShape::Live, Sabotage::None);
    let measured = outcome
        .measured
        .expect("the full's sweep accounted every recorded block");
    assert!(
        permille(measured) >= super::super::PROMOTE_SURVIVAL_THRESHOLD_PERMILLE,
        "the rooted cohort survives the full: {measured:?}"
    );
    assert_eq!(
        outcome.predictor_after_full,
        Some(1000),
        "a confirming measurement leaves the predictor alone"
    );
    assert!(
        outcome.next_minor_in_place && outcome.next_minor_untraced,
        "a retained cohort keeps the next minor on the untraced promotion"
    );
}

#[test]
fn a_dead_remembered_parent_keeps_a_dead_cohort_from_feeding_the_predictor() {
    let outcome =
        promote_then_cohort_full_then_minor(CohortShape::DeadWithDeadParent, Sabotage::None);
    let measured = dead_cohort_measurement(&outcome);
    assert_eq!(
        measured.3,
        survival::MinorView::DeadParent,
        "the dead old parent refers into the promoted blocks: a minor would have \
         measured them live"
    );
    assert_eq!(
        outcome.predictor_after_full,
        Some(1000),
        "a survival no minor would measure is not fed"
    );
    assert!(
        outcome.next_minor_in_place && outcome.next_minor_untraced,
        "the next minor keeps the untraced promotion (records_array_20m:parse)"
    );
}

#[test]
fn sabotaged_parent_check_feeds_a_parse_loop_cohort_into_an_evacuation() {
    let outcome = promote_then_cohort_full_then_minor(
        CohortShape::DeadWithDeadParent,
        Sabotage::SkipParentCheck,
    );
    let measured = dead_cohort_measurement(&outcome);
    assert_eq!(
        measured.3,
        survival::MinorView::Exact,
        "premise: check skipped"
    );
    assert!(
        !outcome.next_minor_in_place && !outcome.next_minor_untraced,
        "without the parent check the dead-parent cohort is fed and the next \
         minor evacuates — the traced minor that cost records_array_20m:parse \
         ~95 ms per cohort full"
    );
}
