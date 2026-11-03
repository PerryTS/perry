//! #10182: a promoted-cohort full adopts the census record the in-place
//! promotion walk made of each block it promoted, instead of walking it again.
//!
//! A young population spanning multiple blocks — a rooted young array of young
//! arrays holding young strings, with dead strings between them — is promoted
//! whole by an untraced minor while recording is armed. Half the held strings
//! then die, and the cohort full runs. Every adopted block is re-walked by the
//! census in test builds (`adopt_census::verify_adopted_block`), the held
//! strings survive and the dropped ones are reclaimed. The sabotaged twin
//! records blocks without start bits: the census then cannot recognise any
//! object in them, and the rooted strings are swept.

use super::super::mark::adopt_census;
use super::super::policy::run_promoted_cohort_full_if_due;
use super::super::promoted_cohort as cohort;
use super::super::*;
use super::support::*;

struct Outcome {
    adopted: u64,
    kept_alive: usize,
    kept: usize,
    dropped_reclaimed: usize,
    dropped: usize,
}

fn promote_then_collect_cohort(sabotaged: bool, traced: bool) -> Outcome {
    let _guard = CopyingNurseryTestGuard::new(4);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _promote = if traced {
        super::super::InPlacePromotionTestGuard::enabled(1000)
    } else {
        super::super::InPlacePromotionTestGuard::untraced()
    };
    cohort::seed_for_tests(0, 0, 0);

    const INNER: usize = 64;
    const OUTER: usize = crate::arena::BLOCK_SIZE / (16 * 1024);
    let mut outer = crate::array::js_array_alloc(OUTER as u32);
    let mut held: Vec<(usize, usize, usize)> = Vec::new();
    for o in 0..OUTER {
        let mut inner = crate::array::js_array_alloc(INNER as u32);
        for i in 0..INNER {
            let leaf = young_leaf();
            inner = crate::array::js_array_push_jsvalue(inner, string_bits(leaf));
            held.push((o, i, leaf));
            for _ in 0..8 {
                young_leaf();
            }
        }
        outer = crate::array::js_array_push_jsvalue(outer, ptr_bits(inner as usize));
    }
    js_shadow_slot_set(0, ptr_bits(outer as usize));
    assert!(
        crate::arena::pointer_in_nursery(held[0].2),
        "premise: young population"
    );

    let untraced_before = untraced_promotion_cycles();
    adopt_census::begin_recording();
    let trace = {
        let _sabotage = sabotaged.then(adopt_census::sabotage::Guard::arm);
        collect_minor_trace(GcTriggerKind::Direct)
    };
    adopt_census::finish_recording();
    assert!(
        trace.copying_nursery.in_place_promotion,
        "premise: the minor promoted in place"
    );
    assert!(crate::arena::pointer_in_old_gen(held[0].2));
    if traced {
        assert_eq!(
            untraced_promotion_cycles(),
            untraced_before,
            "the traced census witness must execute the real trace"
        );
    }

    // Drop every other held string.
    let mut kept = Vec::new();
    let mut dropped = Vec::new();
    for &(o, i, leaf) in &held {
        if (o + i) % 2 == 0 {
            kept.push(leaf);
        } else {
            let inner = crate::array::js_array_get_jsvalue(outer, o as u32);
            crate::array::js_array_set_jsvalue(
                ((inner & POINTER_MASK) as usize) as *mut crate::array::ArrayHeader,
                i as u32,
                crate::value::TAG_UNDEFINED,
            );
            dropped.push(leaf);
        }
    }

    cohort::seed_for_tests(cohort::bound_bytes(), 0, 0);
    let adopted_before = adopt_census::adopted_blocks();
    let survival_before_full = last_young_survival_permille();
    if traced {
        assert!(
            adopt_census::ready_blocks().is_empty(),
            "traced census records must not arm the untraced survival predictor"
        );
    }
    let ran = {
        let _sabotage = sabotaged.then(adopt_census::sabotage::Guard::arm);
        run_promoted_cohort_full_if_due()
    };
    assert!(ran, "premise: the cohort full ran");
    let adopted = adopt_census::adopted_blocks() - adopted_before;
    if traced {
        assert_eq!(
            last_young_survival_permille(),
            survival_before_full,
            "census reuse must leave the traced minor's predictor unchanged"
        );
    }
    let alive = |user: usize| {
        crate::arena::pointer_in_old_gen(user)
            && unsafe { (*header_from_user_ptr(user as *const u8)).obj_type } == GC_TYPE_STRING
    };
    let outcome = Outcome {
        adopted,
        kept_alive: kept.iter().filter(|&&u| alive(u)).count(),
        kept: kept.len(),
        dropped_reclaimed: dropped.iter().filter(|&&u| !alive(u)).count(),
        dropped: dropped.len(),
    };
    js_shadow_slot_set(0, crate::value::TAG_UNDEFINED);
    cohort::seed_for_tests(0, 0, 0);
    adopt_census::discard();
    outcome
}

#[test]
fn a_cohort_full_adopts_the_promotion_census_and_collects_exactly() {
    let outcome = promote_then_collect_cohort(false, false);
    assert!(
        outcome.adopted >= 2,
        "the full must adopt the promoted blocks' records: {}",
        outcome.adopted
    );
    assert_eq!(
        outcome.kept_alive, outcome.kept,
        "every held string survives"
    );
    assert_eq!(
        outcome.dropped_reclaimed, outcome.dropped,
        "every dropped string is reclaimed"
    );
}

#[test]
fn sabotaged_promotion_census_loses_rooted_objects() {
    let outcome = promote_then_collect_cohort(true, false);
    assert!(
        outcome.adopted >= 2,
        "premise: the sabotaged records were adopted"
    );
    assert!(
        outcome.kept_alive < outcome.kept,
        "records without start bits hide every object in their blocks from the \
         mark, so rooted strings are swept: {} of {} survived",
        outcome.kept_alive,
        outcome.kept
    );
}

#[test]
fn a_traced_promotion_census_is_adopted_and_preserves_exact_liveness() {
    let outcome = promote_then_collect_cohort(false, true);
    assert!(
        outcome.adopted >= 2,
        "traced records must actually be adopted"
    );
    assert_eq!(outcome.kept_alive, outcome.kept);
    assert_eq!(outcome.dropped_reclaimed, outcome.dropped);
}

#[test]
fn a_corrupted_traced_promotion_census_is_detected_by_liveness() {
    let outcome = promote_then_collect_cohort(true, true);
    assert!(outcome.adopted >= 2);
    assert!(
        outcome.kept_alive < outcome.kept,
        "removing recorded start bits must make the live-subject witness fail"
    );
}
