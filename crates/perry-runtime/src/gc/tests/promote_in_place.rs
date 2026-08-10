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
    parse_promote_in_place, seed_promoted_dead_bytes_for_tests, InPlacePromotionTestGuard,
    PROMOTED_DEAD_BUDGET_BYTES, PROMOTE_SURVIVAL_THRESHOLD_PERMILLE,
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
    // 0 permille is what an unmeasured thread decides as (the `None` case takes
    // the same branch): with no evidence there is no basis for promoting, so
    // the first copying minor must evacuate and measure.
    let _guard = InPlacePromotionTestGuard::enabled(0);
    assert!(!should_promote_young_in_place());
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
