//! Teeth for whole-block in-place promotion (#7742).
//!
//! Two obligations, and the second is the one that matters:
//!
//! 1. The policy classifies the measured workload population correctly, and
//!    every knob that can turn the path off is asserted in BOTH states (the GC
//!    knob kill-policy in CLAUDE.md).
//! 2. A cycle that takes the path actually promotes something, and the
//!    promoted object comes out the far side at the SAME address, in old-gen,
//!    `GC_FLAG_TENURED`, and findable through the old-gen page index. A test
//!    that merely observes "nothing threw" would pass against a promotion that
//!    silently indexed nothing — which is precisely the shape that turns into a
//!    swept-live-object crash one cycle later.

use super::super::promote_in_place::{
    clear_young_survival_for_tests, note_untraced_promotion, parse_promote_in_place,
    promoted_dead_bytes_since_full, seed_promoted_dead_bytes_for_tests,
    seed_untraced_promoted_bytes_for_tests, seed_young_survival_for_tests,
    InPlacePromotionTestGuard, PROMOTED_DEAD_BUDGET_BYTES, PROMOTE_SURVIVAL_THRESHOLD_PERMILLE,
    UNTRACED_PROMOTION_SURVIVAL_PERMILLE,
};
use super::super::*;
use super::support::*;

// ---------------------------------------------------------------------------
// Knob + policy (pure)
// ---------------------------------------------------------------------------

#[test]
fn promote_in_place_knob_parses_both_states() {
    // ON is the default and every unrecognised spelling — a typo must not
    // silently change which collector a bisect is measuring.
    for raw in [None, Some("1"), Some("on"), Some("true"), Some("banana")] {
        assert!(
            parse_promote_in_place(raw),
            "{raw:?} must leave in-place promotion ON"
        );
    }
    for raw in ["0", "off", "false", "OFF", " false "] {
        assert!(
            !parse_promote_in_place(Some(raw)),
            "{raw} must turn in-place promotion OFF"
        );
    }
}

#[test]
fn threshold_separates_the_measured_workload_population() {
    // The ratios actually measured on gc-handoff/bench (see the module docs on
    // gc/promote_in_place.rs). This is the claim the constant rests on: the
    // population is bimodal, so the threshold is not a tuning dial.
    let fully_live = [999u64, 1000, 1000, 1000];
    let churny = [0u64, 1, 2, 3, 4];
    for r in fully_live {
        assert!(
            r >= PROMOTE_SURVIVAL_THRESHOLD_PERMILLE,
            "retain/deeplist-shaped ratio {r} must promote in place"
        );
    }
    for r in churny {
        assert!(
            r < PROMOTE_SURVIVAL_THRESHOLD_PERMILLE,
            "churn-shaped ratio {r} must NOT promote in place"
        );
    }
}

#[test]
fn a_promoting_cycle_still_measures_so_the_predictor_cannot_go_stale() {
    let _guard = InPlacePromotionTestGuard::enabled(1000);
    assert!(should_promote_young_in_place());

    // The promoting cycle traces, so it measures. A workload that flips to
    // garbage turns the policy off on the very next decision — one nursery of
    // retained garbage, not an unbounded run of them.
    note_young_survival(16 * 1024 * 1024, 4 * 1024);
    assert!(
        !should_promote_young_in_place(),
        "a measured collapse in survival must disable in-place promotion immediately"
    );

    note_young_survival(16 * 1024 * 1024, 16 * 1024 * 1024);
    assert!(should_promote_young_in_place());
}

#[test]
fn an_unmeasured_thread_never_promotes() {
    // Both no-promote arms, and they are genuinely different states: `None` is
    // "no copying minor has run on this thread", 0 permille is a MEASUREMENT of
    // "almost nothing survived". The first copying minor of a process is in the
    // former, and it must evacuate and measure rather than promote on no
    // evidence — so the `None` arm needs asserting in its own right.
    let _guard = InPlacePromotionTestGuard::enabled(1000);
    clear_young_survival_for_tests();
    assert!(
        !should_promote_young_in_place(),
        "an unmeasured thread has no basis for promoting"
    );
    seed_young_survival_for_tests(0);
    assert!(
        !should_promote_young_in_place(),
        "a measured 0 permille must not promote either"
    );
}

#[test]
fn dead_byte_budget_stops_promotion_until_a_full_reclaims() {
    let _guard = InPlacePromotionTestGuard::enabled(1000);
    assert!(should_promote_young_in_place());

    seed_promoted_dead_bytes_for_tests(PROMOTED_DEAD_BUDGET_BYTES);
    assert!(
        !should_promote_young_in_place(),
        "the running dead-byte budget is the bound on the steady state the \
         per-cycle re-measurement does NOT cover"
    );

    note_full_collection_reclaimed_old_gen();
    assert!(
        should_promote_young_in_place(),
        "a full collection reclaimed the parked garbage, so the budget resets"
    );
}

// ---------------------------------------------------------------------------
// End to end: the object does not move, and it is a first-class old-gen object
// afterwards.
// ---------------------------------------------------------------------------

#[test]
fn in_place_promotion_leaves_the_object_at_its_address_in_old_gen() {
    // NOTE: `CopyingNurseryTestGuard::new` takes the copying-nursery isolation
    // lock itself — taking it again here is a self-deadlock.
    let _guard = CopyingNurseryTestGuard::new(4);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _promote = InPlacePromotionTestGuard::enabled(1000);
    // NO `reset_shadow_stack()` here: the guard has already pushed the frame
    // whose slot 0 is written below, and resetting would drop it — the object
    // would then have no root, die, and the test would read as "the in-place
    // path promoted nothing".
    let child = young_leaf();
    js_shadow_slot_set(0, ptr_bits(child));
    assert!(crate::arena::pointer_in_nursery(child));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);

    // Live subject: a green row that promoted nothing proves nothing.
    assert!(
        trace.copying_nursery.in_place_promotion,
        "the cycle must have taken the in-place path"
    );
    assert!(
        trace.copying_nursery.in_place_promoted_objects > 0,
        "the in-place path must have promoted at least one object"
    );
    assert!(
        trace.copying_nursery.in_place_promoted_blocks > 0,
        "the in-place path must have taken at least one block"
    );
    assert_eq!(
        trace.copying_nursery.copied_objects, 0,
        "an in-place promotion copies nothing"
    );

    // The whole point: the address is unchanged, and every slot that pointed
    // at it is therefore still correct without any rewrite.
    let after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_eq!(after, child, "in-place promotion must not move the object");
    assert!(
        crate::arena::pointer_in_old_gen(child),
        "the promoted object must classify as old-gen afterwards"
    );

    // `Old ⟹ TENURED` (#7511): the generated write barrier's fast path skips
    // the remembering call outright when this bit is missing.
    let header = unsafe { header_from_user_ptr(child as *const u8) };
    assert_ne!(
        unsafe { (*header).gc_flags } & GC_FLAG_TENURED,
        0,
        "a promoted object must carry GC_FLAG_TENURED"
    );

    // Findable through the old-gen page index — this is what the remembered-set
    // dirty scan uses to reach it, so an unindexed promoted object is a
    // missed old→young edge waiting to happen.
    let page = crate::arena::generation_page_for_addr(header as usize);
    let meta = crate::arena::old_page_meta_for_tests(page)
        .expect("the promoted object's page must have old-gen metadata");
    assert!(
        meta.live_object_count > 0 && meta.live_bytes > 0,
        "the promoted object must be indexed as live on its old page, got {meta:?}"
    );
}

// ---------------------------------------------------------------------------
// #7888: promoting WITHOUT tracing
// ---------------------------------------------------------------------------

#[test]
fn untraced_promotion_needs_the_fully_live_regime_not_merely_the_promoting_one() {
    let _guard = InPlacePromotionTestGuard::untraced();
    assert!(should_promote_young_in_place());
    assert!(should_promote_young_untraced());

    // The band BETWEEN the two thresholds is the whole point of having two:
    // 96% live is worth promoting in place (5% of a nursery in footprint) and
    // is NOT worth promoting blind (the 4% this would silently keep is real
    // garbage the trace would have identified for free, since it was running
    // anyway).
    for permille in [
        PROMOTE_SURVIVAL_THRESHOLD_PERMILLE,
        UNTRACED_PROMOTION_SURVIVAL_PERMILLE - 1,
    ] {
        seed_young_survival_for_tests(permille);
        assert!(
            should_promote_young_in_place(),
            "{permille}‰ is in the in-place band"
        );
        assert!(
            !should_promote_young_untraced(),
            "{permille}‰ is NOT in the untraced band"
        );
    }
}

#[test]
fn an_untraced_cycle_charges_the_dead_bytes_its_last_measurement_implies() {
    // Charging zero is not "no garbage", it is "no answer" — and it would
    // disarm the footprint cap for the whole untraced run. The two paths share
    // one bound; they differ only in whether the dead figure is measured or
    // extrapolated.
    let _guard = InPlacePromotionTestGuard::untraced();
    seed_young_survival_for_tests(990);
    let before = promoted_dead_bytes_since_full();
    note_untraced_promotion(100 * 1000, 1);
    assert_eq!(
        promoted_dead_bytes_since_full() - before,
        1000,
        "10 permille of 100_000 bytes is the dead figure a 990 permille \
         measurement implies"
    );

    // ...and it is the SAME cap: enough implied dead bytes stop in-place
    // promotion until a full collection reclaims them, exactly as a measured
    // collapse would.
    seed_promoted_dead_bytes_for_tests(PROMOTED_DEAD_BUDGET_BYTES);
    assert!(!should_promote_young_in_place());
    assert!(!should_promote_young_untraced());
}

#[test]
fn untraced_budget_forces_a_measuring_cycle_and_a_measurement_clears_it() {
    let _guard = InPlacePromotionTestGuard::untraced();
    assert!(should_promote_young_untraced());

    seed_untraced_promoted_bytes_for_tests(usize::MAX);
    assert!(
        !should_promote_young_untraced(),
        "an exhausted budget must force the next cycle to measure"
    );
    assert!(
        should_promote_young_in_place(),
        "...but it is still a promoting cycle — the budget buys a TRACE, not an \
         evacuation"
    );

    // The measuring cycle it forced re-arms the run.
    note_young_survival(16 * 1024 * 1024, 16 * 1024 * 1024);
    assert!(should_promote_young_untraced());

    // So does a full collection: its trace is a stronger answer to the same
    // question, and it reclaimed whatever the untraced run retained.
    seed_untraced_promoted_bytes_for_tests(usize::MAX);
    assert!(!should_promote_young_untraced());
    note_full_collection_reclaimed_old_gen();
    assert!(should_promote_young_untraced());
}

#[test]
fn an_untraced_promotion_indexes_the_objects_it_could_not_prove_live() {
    // The load-bearing property. An untraced cycle has no marks, so it registers
    // EVERY object on the promoted block into the old-gen page index. If it
    // registered none — or only the ones some stale mark bit happened to name —
    // the next remembered-set dirty scan could not reach a promoted parent, and
    // a young child stored into it afterwards would be swept while live.
    //
    // The second collection is deliberately an EVACUATING one (survival seeded
    // low): on a promoting cycle every young object survives whether or not
    // anything reached it, which would make this test pass against a promotion
    // that indexed nothing at all.
    let _guard = CopyingNurseryTestGuard::new(4);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _promote = InPlacePromotionTestGuard::untraced();

    let payload = std::mem::size_of::<crate::array::ArrayHeader>() + 8;
    let parent = unsafe {
        let arr = crate::arena::arena_alloc_gc(payload, 8, GC_TYPE_ARRAY)
            as *mut crate::array::ArrayHeader;
        (*arr).length = 1;
        (*arr).capacity = 1;
        let elements =
            (arr as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *mut u64;
        *elements = 0;
        (arr, elements)
    };
    let (parent_arr, elements) = parent;
    js_shadow_slot_set(0, ptr_bits(parent_arr as usize));
    assert!(crate::arena::pointer_in_nursery(parent_arr as usize));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert!(
        trace.copying_nursery.in_place_promotion,
        "the cycle must have promoted in place"
    );
    // Live subject: the counters must show the UNTRACED path ran, not merely
    // that a promoting one did.
    assert!(
        untraced_promotion_cycles() > 0 && untraced_promoted_objects() > 0,
        "the untraced path must have run and promoted something (cycles={}, objects={})",
        untraced_promotion_cycles(),
        untraced_promoted_objects()
    );
    assert_eq!(
        trace.remembered_set.dirty_objects_scanned, 0,
        "an untraced cycle must not run the dirty scan — that scan IS the \
         per-object mark pass this change removes"
    );
    assert_eq!(
        parent_arr as usize,
        (js_shadow_slot_get(0) & POINTER_MASK) as usize,
        "in-place promotion must not move the parent"
    );
    assert!(crate::arena::pointer_in_old_gen(parent_arr as usize));

    // Now the edge that only the page index can carry.
    let child = young_leaf();
    unsafe {
        *elements = ptr_bits(child);
        layout_note_slot(parent_arr as usize, 0, *elements);
        runtime_write_barrier_slot(parent_arr as usize, elements as usize, *elements);
    }
    // Drop the direct root: the child is now reachable ONLY through the
    // untraced-promoted parent.
    js_shadow_slot_set(0, ptr_bits(parent_arr as usize));

    seed_young_survival_for_tests(10);
    seed_untraced_promoted_bytes_for_tests(usize::MAX);
    assert!(!should_promote_young_in_place());
    let trace2 = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace2, true, CopiedMinorFallbackReason::None, false);
    assert!(
        !trace2.copying_nursery.in_place_promotion,
        "the second cycle must EVACUATE, or an unreached child would survive \
         anyway and this test would prove nothing"
    );
    assert_eq!(
        trace2.copying_nursery.copied_objects, 1,
        "the child must have been found through the promoted parent and copied"
    );
    let child_after = unsafe { (*elements & POINTER_MASK) as usize };
    assert_ne!(
        child_after, child,
        "an evacuating cycle relocates the child and rewrites the slot in its \
         promoted parent"
    );
    assert!(crate::arena::pointer_in_nursery(child_after));
    unsafe {
        assert_ne!(
            (*header_from_user_ptr(child_after as *const u8)).size,
            0,
            "the relocated child must be a live object"
        );
    }
}

#[test]
fn a_low_survival_cycle_still_evacuates_and_moves_the_object() {
    // NOTE: `CopyingNurseryTestGuard::new` takes the copying-nursery isolation
    // lock itself — taking it again here is a self-deadlock.
    let _guard = CopyingNurseryTestGuard::new(4);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    // Opted in, but the measurement says "mostly garbage" — the OFF arm of the
    // policy, driven through the same entry point as the ON arm above.
    let _promote = InPlacePromotionTestGuard::enabled(10);
    let child = young_leaf();
    js_shadow_slot_set(0, ptr_bits(child));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert!(
        !trace.copying_nursery.in_place_promotion,
        "a 1% survival reading must evacuate, not promote"
    );

    let after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(
        after, child,
        "the ordinary copying path must still relocate"
    );
    assert!(crate::arena::pointer_in_nursery(after));
}
