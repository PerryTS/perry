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
    clear_young_survival_for_tests, first_cycle_promotion_holds_up, note_untraced_promotion,
    parse_promote_in_place, promoted_dead_bytes_since_full, seed_promoted_dead_bytes_for_tests,
    seed_untraced_promoted_bytes_for_tests, seed_young_survival_for_tests,
    should_attempt_first_cycle_promotion, untraced_promotion_budget_with,
    InPlacePromotionTestGuard, PROMOTED_DEAD_BUDGET_BYTES, PROMOTE_SURVIVAL_THRESHOLD_PERMILLE,
    UNTRACED_PROMOTION_CEILING_BYTES, UNTRACED_PROMOTION_FLOOR_BYTES,
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
    //
    // The cap is read by `should_promote_young_in_place` alone.
    // `should_promote_young_untraced` is a SUB-decision — `copying.rs`
    // evaluates it only after the promoting decision has said yes AND the
    // retag came back non-empty — so what the cap closes is the composite, and
    // that is what this asserts. Asserting the sub-decision on its own would
    // read as a stronger statement than the code makes; it passed before only
    // because the seeded ratio was under the then-999 untraced threshold, i.e.
    // for a reason that had nothing to do with the cap.
    seed_promoted_dead_bytes_for_tests(PROMOTED_DEAD_BUDGET_BYTES);
    assert!(!should_promote_young_in_place());
    assert!(
        !(should_promote_young_in_place() && should_promote_young_untraced()),
        "an exhausted dead-bytes budget must close the untraced path too"
    );
}

/// #7902: the extrapolation must not be taken from a stationary 1000‰ reading.
///
/// The predictor is by construction the PREVIOUS cycle's answer, so a fully-live
/// measurement says nothing about the cohort being promoted now. Charging
/// `1000 − 1000 = 0` disarmed `PROMOTED_DEAD_BUDGET_BYTES` on exactly the
/// workloads that reach this path — and contradicted
/// `UNTRACED_PROMOTION_SURVIVAL_PERMILLE`'s own doc, which derives its 1.28 MB
/// bound from the threshold rather than from the measurement.
#[test]
fn a_stationary_fully_live_predictor_still_charges_implied_dead_bytes() {
    let _guard = InPlacePromotionTestGuard::untraced();
    seed_young_survival_for_tests(1000);
    let before = promoted_dead_bytes_since_full();
    note_untraced_promotion(100 * 1000, 1);
    let charged = promoted_dead_bytes_since_full() - before;
    assert_eq!(
        charged, 1000,
        "a 1000 permille predictor must still be charged at the threshold the \
         decision admits ({UNTRACED_PROMOTION_SURVIVAL_PERMILLE} permille), not \
         at its own optimism"
    );

    // ...and the charge is enough to close the composite decision once the
    // footprint cap is spent, so the untraced path cannot bleed indefinitely
    // while every individual cycle looks fine.
    seed_promoted_dead_bytes_for_tests(PROMOTED_DEAD_BUDGET_BYTES);
    assert!(!should_promote_young_in_place());
}

/// #7902: the untraced budget IS the worst-case retained-garbage bound (every
/// byte it admits is assumed live), so it must be statable — bounded above, and
/// scaled to a configured heap budget rather than parking a flat 128 MB floor
/// on a device heap smaller than that.
#[test]
fn the_untraced_promotion_budget_is_bounded_and_scales_to_the_heap_budget() {
    // Unconstrained: the historical floor, growing with old-gen, capped.
    assert_eq!(
        untraced_promotion_budget_with(None, 0),
        UNTRACED_PROMOTION_FLOOR_BYTES
    );
    assert_eq!(
        untraced_promotion_budget_with(None, 256 * 1024 * 1024),
        256 * 1024 * 1024,
        "the relative half must still track a genuinely-live old heap"
    );
    assert_eq!(
        untraced_promotion_budget_with(None, 64 * 1024 * 1024 * 1024),
        UNTRACED_PROMOTION_CEILING_BYTES,
        "#7902: an unbounded relative half lets a phase change park a whole \
         old-heap's worth of assumed-live garbage"
    );

    // Constrained: a quarter of the budget, and never more than half of it even
    // against an old generation that fills the budget.
    let budget = 64 * 1024 * 1024;
    assert_eq!(
        untraced_promotion_budget_with(Some(budget), 0),
        budget / 4,
        "#7902: a 128 MB floor is larger than this whole configured heap"
    );
    assert!(
        untraced_promotion_budget_with(Some(budget), budget) <= budget / 2,
        "a constrained process must not admit more assumed-live retention than \
         half its own heap budget"
    );
    assert!(
        untraced_promotion_budget_with(Some(budget), usize::MAX) < UNTRACED_PROMOTION_FLOOR_BYTES
    );
}

/// #7902: the forced measuring cycle is the FIRST evidence about the cohort the
/// preceding untraced cycles promoted on faith. When it contradicts the
/// predictor, that cohort must be scheduled for reclamation — a traced minor
/// measures only its own young generation and can neither identify nor free
/// bytes already sitting in old-gen.
#[test]
fn a_contradicting_measurement_schedules_the_old_reclaim_it_needs() {
    let _guard = InPlacePromotionTestGuard::untraced();
    let previous = GC_OLD_RECLAIM_PENDING.with(Cell::get);

    // A measurement that AGREES with the predictor changes nothing.
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(false));
    seed_untraced_promoted_bytes_for_tests(64 * 1024 * 1024);
    note_young_survival(16 * 1024 * 1024, 16 * 1024 * 1024);
    assert!(
        !GC_OLD_RECLAIM_PENDING.with(Cell::get),
        "a confirming measurement must not schedule a reclaim"
    );

    // A phase change does: 1000 permille promoted the cohort, 0 permille says
    // it was garbage.
    seed_untraced_promoted_bytes_for_tests(64 * 1024 * 1024);
    note_young_survival(16 * 1024 * 1024, 0);
    assert!(
        GC_OLD_RECLAIM_PENDING.with(Cell::get),
        "#7902: a contradicted predictor must schedule the old-gen reclaim that \
         can decide the cohort it already promoted"
    );

    // With no outstanding untraced run there is nothing to recover, so a low
    // measurement on its own must not force a reclaim.
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(false));
    seed_untraced_promoted_bytes_for_tests(0);
    note_young_survival(16 * 1024 * 1024, 0);
    assert!(
        !GC_OLD_RECLAIM_PENDING.with(Cell::get),
        "a low measurement with no untraced run behind it schedules nothing"
    );
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(previous));
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
        "the untraced path must have run and promoted something (cycles={}, objects={}); \
         the cycle declined it because: {}",
        untraced_promotion_cycles(),
        untraced_promoted_objects(),
        crate::gc::copying::last_untraced_decline_reason()
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

// ---------------------------------------------------------------------------
// #7937: the FIRST copying minor decides from its own trace
// ---------------------------------------------------------------------------

#[test]
fn only_an_unmeasured_thread_may_attempt_the_first_cycle_promotion() {
    let _guard = InPlacePromotionTestGuard::enabled(1000);
    clear_young_survival_for_tests();
    assert!(
        should_attempt_first_cycle_promotion(),
        "no copying minor has run, so there is nothing for the steady-state \
         policy to read and the attempt is the only way to find out"
    );
    // Once ANY measurement exists the steady-state policy owns the decision —
    // including a measurement of "almost nothing survived", which is a
    // genuinely different state from `None`.
    for measured in [0u64, 500, 1000] {
        seed_young_survival_for_tests(measured);
        assert!(
            !should_attempt_first_cycle_promotion(),
            "{measured} permille is a measurement; the first-cycle attempt must \
             not re-run on a thread that already has one"
        );
    }
}

#[test]
fn the_first_cycle_attempt_shares_every_guard_the_steady_state_policy_has() {
    // The dead-byte budget is the one that can differ between the two — it is
    // read by both, and a first-cycle attempt that ignored it would park a
    // nursery of garbage the budget exists to bound.
    let _guard = InPlacePromotionTestGuard::enabled(1000);
    clear_young_survival_for_tests();
    assert!(should_attempt_first_cycle_promotion());
    seed_promoted_dead_bytes_for_tests(PROMOTED_DEAD_BUDGET_BYTES);
    assert!(
        !should_attempt_first_cycle_promotion(),
        "the running dead-byte budget must bound the first cycle too"
    );
}

#[test]
fn first_cycle_threshold_separates_the_measured_cycle0_population() {
    // Cycle-0 young survival measured over gc-handoff/bench + gc-handoff/apps
    // (gc-handoff/c0/cycles.py, 2026-08-12). This is the claim
    // FIRST_CYCLE_PROMOTE_SURVIVAL_PERMILLE rests on, and the band between the
    // two groups is empty — 25 to 770 — so the constant is not a tuning dial
    // any more than the steady-state one is.
    let young = 16 * 1024 * 1024usize;
    // churn, churn_alloc, push_cls, push_num, cycles, pipeline, tree,
    // tree_wide, shapes.
    for permille in [0u64, 1, 2, 3, 25] {
        assert!(
            !first_cycle_promotion_holds_up(young, young * permille as usize / 1000),
            "{permille} permille at cycle 0 must roll back and evacuate"
        );
    }
    // asyncpipe (770), then retain/retain1/retain_wide/retain_wide1/deeplist.
    for permille in [770u64, 992, 1000] {
        assert!(
            first_cycle_promotion_holds_up(young, young * permille as usize / 1000),
            "{permille} permille at cycle 0 must keep the promotion"
        );
    }
    // An empty young generation is not a fully-live one: 0/0 must not read as
    // 1000 permille and keep a promotion of nothing.
    assert!(!first_cycle_promotion_holds_up(0, 0));
}

/// The rollback is the half that can corrupt the heap, so it is driven end to
/// end rather than asserted about: an unmeasured thread whose nursery is mostly
/// garbage must ATTEMPT the promotion, read its own trace, undo the retag, and
/// come out the far side having EVACUATED — object relocated, still live, still
/// in the nursery.
///
/// A rollback that forgot to undo the retag would leave the survivor at its old
/// address in old-gen, so `assert_ne!` on the address is the teeth; a rollback
/// that forgot `clear_marks` would leave the second attempt's trace unable to
/// mark anything, so `copied_objects` would read 0.
#[test]
fn a_first_cycle_attempt_that_its_own_trace_refutes_rolls_back_and_evacuates() {
    let _guard = CopyingNurseryTestGuard::new(4);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _promote = InPlacePromotionTestGuard::enabled(1000);
    clear_young_survival_for_tests();

    let attempts_before = first_cycle_promotion_attempts();
    let rollbacks_before = first_cycle_promotion_rollbacks();
    let old_pages_before = crate::arena::old_page_summary().pages;

    // One live leaf in a nursery sized for many: survival is far under the
    // threshold, so the attempt's own trace refutes it.
    let child = young_leaf();
    for _ in 0..64 {
        let _garbage = young_leaf();
    }
    js_shadow_slot_set(0, ptr_bits(child));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(
        first_cycle_promotion_attempts() - attempts_before,
        1,
        "the first cycle must have ATTEMPTED the promotion — a test that never \
         entered the path proves nothing about it"
    );
    assert_eq!(
        first_cycle_promotion_rollbacks() - rollbacks_before,
        1,
        "and its own trace must have refuted it"
    );
    assert!(
        !trace.copying_nursery.in_place_promotion,
        "the cycle that actually ran is the rolled-back-to evacuation"
    );
    assert!(
        trace.copying_nursery.copied_objects >= 1,
        "the second attempt must have marked and copied the survivor; zero here \
         means the rollback left stale marks behind"
    );

    let after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(after, child, "a rolled-back cycle still evacuates");
    assert!(
        crate::arena::pointer_in_nursery(after),
        "undoing the retag must put the block back in the young generation — a \
         survivor in old-gen here means the rollback did not run"
    );
    unsafe {
        assert_ne!(
            (*header_from_user_ptr(after as *const u8)).size,
            0,
            "the relocated survivor must be a live object"
        );
    }
    // The retag is not only a relabel: it REGISTERS old-gen page metadata for
    // every page of every young block (`register_old_block_pages`). Rolling
    // back has to unregister them, or the whole young generation is left
    // carrying old-gen page entries — a lie the dirty scan and the defrag page
    // selector both read, and +4–10% peak RSS across the corpus when it was
    // missing.
    assert_eq!(
        crate::arena::old_page_summary().pages,
        old_pages_before,
        "a rolled-back attempt must leave no old-gen page metadata behind"
    );
}

// ---------------------------------------------------------------------------
// #7913 interaction: old-page relocation vs a still-DESCRIBED promoted run
// ---------------------------------------------------------------------------

/// #7914 describes a promoted page's object list by its `first_header` address
/// and expands it lazily; #7913 relocates old pages BY DEFAULT. If a page
/// carrying an unexpanded run could be relocated, the deferred expansion would
/// later parse stale addresses — the silent-corruption shape.
///
/// It cannot, and this pins the mechanism that makes it so:
/// `evacuate_selected_old_pages_collecting` snapshots its source-block
/// occupants through `old_arena_walk_objects_on_pages`, which expands every
/// pending run on every page of the source blocks BEFORE a single object is
/// forwarded. #7913 widened that enumeration from the selected pages to the
/// whole containing source blocks, so it covers strictly more than before.
///
/// The test is written so it cannot pass by accident: the objects are removed
/// from the eager index first, so a run that is NOT expanded leaves
/// `source_headers` empty, the all-or-nothing guard returns early, and
/// `old_page_moved_objects` reads 0 instead of 3. Deleting the
/// `materialize_promoted_page_runs` call in `old_arena_walk_objects_on_pages`
/// turns this red.
#[test]
fn old_page_relocation_expands_a_described_run_before_it_moves_anything() {
    let _isolation = copying_nursery_isolation_lock();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    CONS_PINNED.with(|s| s.borrow_mut().clear());

    // Three real old-gen objects, contiguous and linearly parseable — the
    // shape a promoted block hands to OLD_ARENA.
    let users: Vec<usize> = (0..3)
        .map(|_| crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT) as usize)
        .collect();
    let headers: Vec<(*mut GcHeader, usize)> =
        users.iter().map(|&u| old_test_header_and_size(u)).collect();
    let first = headers[0].0 as usize;
    let last = headers[2].0 as usize;
    let total: usize = headers.iter().map(|&(_, t)| t).sum();

    let mut selected_pages = crate::fast_hash::new_ptr_hash_set();
    for &(header, size) in &headers {
        for (page, _) in crate::arena::old_object_page_overlaps(header as usize, size) {
            selected_pages.insert(page);
        }
    }
    // All three must share one page for the run to describe them as one span;
    // 64-byte objects out of a fresh block do, but assert rather than assume.
    assert_eq!(
        selected_pages.len(),
        1,
        "test setup expects one page; a multi-page split would need one run per page"
    );
    let page = *selected_pages.iter().next().unwrap();

    // Drop the eager index built at birth, then DESCRIBE the same objects.
    // From here the run is the only record that this page has occupants.
    crate::arena::old_arena_page_index_clear_for_tests();
    crate::arena::register_promoted_page_run(page, first, last, headers.len(), total);
    assert_eq!(
        crate::arena::pending_promoted_page_runs(),
        1,
        "the page must be DESCRIBED going in, or this test proves nothing"
    );

    for &(header, _) in &headers {
        unsafe {
            (*header).gc_flags |= GC_FLAG_MARKED;
        }
    }

    let mut new_headers = Vec::new();
    let mut original_headers = Vec::new();
    let moved = evacuate_selected_old_pages_collecting(
        &selected_pages,
        &mut new_headers,
        &mut original_headers,
    );

    assert_eq!(
        moved.old_page_moved_objects,
        headers.len(),
        "relocation must see every occupant of a DESCRIBED page. Seeing fewer \
         means it relocated a block while some occupants were still only \
         described — their memory would be released with live objects in it, \
         and the deferred expansion would parse recycled addresses"
    );
    assert_eq!(
        crate::arena::pending_promoted_page_runs(),
        0,
        "the run must have been consumed by the relocation's own enumeration"
    );
    for &(header, _) in &headers {
        unsafe {
            assert_ne!(
                (*header).gc_flags & GC_FLAG_FORWARDED,
                0,
                "every described occupant must be forwarded, not just indexed"
            );
        }
    }

    release_evacuated_original_forwarding_stubs(&original_headers);
    clear_marks();
    CONS_PINNED.with(|s| s.borrow_mut().clear());
}

/// The second, independent line of defence, stated as a test rather than a
/// comment: a page that has only just been promoted is not defrag-eligible at
/// all, because `register_promoted_page_run` records live bytes and never dead
/// ones, and `old_page_defrag_eligible` requires `dead_bytes > 0`. Dead bytes
/// come only from the old-gen sweep, and the sweep's cycle constructor
/// (`GcCycleState::new_full`) expands every pending run before it starts.
#[test]
fn a_freshly_described_page_is_not_defrag_eligible() {
    let _isolation = copying_nursery_isolation_lock();
    let user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT) as usize;
    let (header, size) = old_test_header_and_size(user);
    let page = crate::arena::old_object_page_overlaps(header as usize, size)[0].0;
    crate::arena::old_arena_page_index_clear_for_tests();
    crate::arena::register_promoted_page_run(page, header as usize, header as usize, 1, size);

    let meta = crate::arena::old_page_meta_for_tests(page).expect("promotion records page meta");
    assert_eq!(
        meta.dead_bytes, 0,
        "a promotion records live bytes only; dead bytes are the sweep's to record"
    );
    assert!(
        !super::super::oldgen_defrag::old_page_defrag_eligible(meta),
        "a page whose run is still described must not be a defrag candidate"
    );
}

/// #7946: **another agent's** incremental cycle must not veto this thread's
/// untraced promotion.
///
/// `PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT` is a process-global armed by
/// whichever thread has a cycle running — deliberately conservative, because on
/// the write barrier's fast path a false positive costs one call that returns.
/// The untraced-promotion veto read it directly, so one agent's cycle turned
/// another agent's promotion policy off, and under `cargo test` "another agent"
/// means "any of the other 2 200 tests":
/// `an_untraced_promotion_indexes_the_objects_it_could_not_prove_live` failed 25
/// runs in 200 with `cycles=0, objects=0` for exactly this reason.
///
/// Both directions, because a veto that never fires is as wrong as one that
/// always does:
///
/// * count armed WITHOUT this thread's `valid_ptrs` pointer — the state another
///   agent's cycle creates — must NOT veto;
/// * this thread's own barrier armed MUST veto, or #7888's stale-mark hazard is
///   back.
#[test]
fn another_agents_incremental_cycle_does_not_veto_this_threads_untraced_promotion() {
    use std::sync::atomic::Ordering;

    let _guard = GcTestIsolationGuard::new();

    /// Restores the count even if an assertion unwinds — leaking an arm would
    /// pin every later test into the conservative arm.
    struct ForeignArm;
    impl ForeignArm {
        fn new() -> Self {
            PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT.fetch_add(1, Ordering::Release);
            Self
        }
    }
    impl Drop for ForeignArm {
        fn drop(&mut self) {
            PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT.fetch_sub(1, Ordering::Release);
        }
    }

    let veto_before = crate::gc::copying::test_untraced_promotion_instrument_veto();
    assert_eq!(
        veto_before, None,
        "test premise: nothing else may be vetoing before the arm"
    );

    {
        let _foreign = ForeignArm::new();
        // The subject has to be live: if the count were still idle this test
        // would pass having armed nothing.
        assert!(
            !crate::gc::incremental_mark_barrier_globally_idle(),
            "test premise: the global count must read as armed"
        );
        assert_eq!(
            crate::gc::copying::test_untraced_promotion_instrument_veto(),
            None,
            "a cycle on ANOTHER thread must not veto this thread's untraced \
             promotion — its marks cannot reach this thread's nursery"
        );
    }

    // The local direction. `GcTestIsolationGuard` above owns the root registry,
    // so the barrier sees exactly this scope's pointers.
    clear_marks();
    let valid_ptrs = build_valid_pointer_set();
    {
        let _barrier = IncrementalMarkBarrierTestGuard::new(&valid_ptrs);
        assert_eq!(
            crate::gc::copying::test_untraced_promotion_instrument_veto(),
            Some("incremental_mark_in_progress"),
            "this thread's own incremental cycle MUST veto: an allocate-black \
             birth would carry GC_FLAG_MARKED into old-gen (#7888)"
        );
    }
    clear_marks();

    assert_eq!(
        crate::gc::copying::test_untraced_promotion_instrument_veto(),
        None,
        "and the veto must lift when this thread's cycle ends"
    );
}
