//! A parent must remain collectable while the first nursery-growth sample is
//! being measured. Otherwise its later-dead remembered edge keeps fresh children
//! alive. Leaves have no such edge and need no extra aging round.

use super::*;

const LEAVES: u32 = 340;
const WEAK: u32 = LEAVES + 1;
const ROOTS: u32 = WEAK + 1 + 256;

fn allocate_string(slot: Option<u32>) {
    let payload = vec![b'x'; 4096];
    let string = crate::string::js_string_from_bytes(payload.as_ptr(), payload.len() as u32);
    if let Some(slot) = slot {
        js_shadow_slot_set(slot, string_bits(string as usize));
    }
}

fn eden_allocated_bytes() -> usize {
    crate::arena::copying_from_space_in_use_bytes()
        .saturating_sub(crate::arena::copying_active_survivor_in_use_bytes())
}

#[test]
fn a_growing_nursery_does_not_promote_a_parent_before_rating_its_first_round() {
    let _guard = CopyingNurseryTestGuard::new(ROOTS);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let parent = crate::object::js_object_alloc(0, 1);
    js_shadow_slot_set(0, ptr_bits(parent as usize));
    // Carry more than one dead-string allocation's worth of small parents,
    // comfortably inside the existing survivor occupancy budget.
    for slot in (WEAK + 1)..ROOTS {
        let parent = crate::object::js_object_alloc(0, 1);
        js_shadow_slot_set(slot, ptr_bits(parent as usize));
    }
    for slot in 1..=LEAVES {
        allocate_string(Some(slot));
        allocate_string(None);
    }
    let first = collect_minor_trace(GcTriggerKind::Direct);
    assert!(first.copying_nursery.eligible);
    assert!(!first.copying_nursery.in_place_promotion);
    assert!(first.copying_nursery.eden_copied_bytes > 1024 * 1024);
    assert!(crate::gc::tenuring::PromotionPolicy::snapshot().parent_probe_active());

    // Dead new material keeps overall survival below the in-place-promotion
    // gate, so this exercises per-object age decisions on a copying minor.
    for _ in 0..LEAVES {
        allocate_string(None);
    }
    let second = collect_minor_trace(GcTriggerKind::Direct);
    assert!(!second.copying_nursery.in_place_promotion);
    let parent = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::ObjectHeader;
    assert!(
        crate::arena::pointer_in_nursery(parent as usize),
        "the first-round parent must still be young when its sample reaches policy"
    );
    for slot in 1..=LEAVES {
        let leaf = (js_shadow_slot_get(slot) & POINTER_MASK) as usize;
        assert!(
            !crate::arena::pointer_in_nursery(leaf),
            "leaf {slot} cannot retain children and must keep the ordinary promotion age"
        );
    }
    assert!(!crate::gc::tenuring::PromotionPolicy::snapshot().parent_probe_active());
    assert_eq!(crate::gc::tenuring::tenuring_survivals(), 1);

    // The parent temporarily receives fresh data, then dies before the next
    // minor. An old parent would retain that data through its remembered slot.
    let child = crate::object::js_object_alloc(0, 0);
    let weak = crate::weakref::js_weakref_new(f64::from_bits(ptr_bits(child as usize)));
    js_shadow_slot_set(WEAK, ptr_bits(weak as usize));
    crate::object::js_object_set_field(
        parent,
        0,
        crate::value::JSValue::from_bits(ptr_bits(child as usize)),
    );
    js_shadow_slot_set(0, crate::value::TAG_UNDEFINED);
    for slot in (WEAK + 1)..ROOTS {
        js_shadow_slot_set(slot, crate::value::TAG_UNDEFINED);
    }
    let carried = crate::arena::copying_active_survivor_in_use_bytes();
    let cap = crate::gc::tenuring::scavenge_nursery_cap_effective_bytes();
    assert!(carried > 8192);
    assert!(carried <= crate::gc::tenuring::desired_survivor_bytes());
    while eden_allocated_bytes() < cap - carried {
        allocate_string(None);
    }
    assert!(
        eden_allocated_bytes() < cap,
        "fixture must reject a raw-cap clock"
    );
    let third = collect_minor_trace(GcTriggerKind::Direct);
    assert!(third.copying_nursery.eligible);
    let weak = f64::from_bits(js_shadow_slot_get(WEAK));
    assert_eq!(
        crate::weakref::js_weakref_deref(weak).to_bits(),
        crate::value::TAG_UNDEFINED,
        "a dead young parent must not keep its child alive through a remembered edge"
    );
    assert_eq!(crate::gc::tenuring::tenuring_survivals(), 1);

    // Both quiet collections cover the available Eden allocation window.
    // Ordinary carried survivors must not delay the two-window unlock.
    let cap = crate::gc::tenuring::scavenge_nursery_cap_effective_bytes();
    while eden_allocated_bytes() < cap {
        allocate_string(None);
    }
    let fourth = collect_minor_trace(GcTriggerKind::Direct);
    assert!(fourth.copying_nursery.eligible);
    assert_eq!(
        crate::gc::tenuring::tenuring_survivals(),
        2,
        "bounded survivor occupancy must not turn a complete Eden window into an early one"
    );
}

// The owner-authorized alternative to an extra young round is a complete
// speculative trace, with the ordinary 950-permille criterion and rollback.
fn run_current_parent_probe(commit: bool) {
    use crate::gc::promote_in_place::{parent_probe_promotion_holds_up, InPlacePromotionTestGuard};
    let _guard = CopyingNurseryTestGuard::new(ROOTS);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _promote = InPlacePromotionTestGuard::enabled(333);
    let parent = crate::object::js_object_alloc(0, 1);
    js_shadow_slot_set(0, ptr_bits(parent as usize));
    for slot in 1..=LEAVES {
        allocate_string(Some(slot));
        allocate_string(None);
    }
    let first = collect_minor_trace(GcTriggerKind::Direct);
    assert!(first.copying_nursery.eligible);
    assert!(!first.copying_nursery.in_place_promotion);
    assert!(crate::gc::tenuring::PromotionPolicy::snapshot().parent_probe_active());
    let policy = crate::gc::tenuring::PromotionPolicy::snapshot();
    assert!(crate::gc::promote_in_place::should_attempt_parent_probe_promotion(policy));
    crate::gc::promote_in_place::seed_promoted_dead_bytes_for_tests(
        crate::gc::promote_in_place::PROMOTED_DEAD_BUDGET_BYTES,
    );
    assert!(!crate::gc::promote_in_place::should_attempt_parent_probe_promotion(policy));
    crate::gc::promote_in_place::seed_promoted_dead_bytes_for_tests(0);
    let before = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    if !commit {
        for _ in 0..LEAVES {
            allocate_string(None);
        }
    }
    let untraced_before = crate::gc::untraced_promotion_cycles();
    let second = collect_minor_trace(GcTriggerKind::Direct);
    assert!(second.copying_nursery.eligible);
    assert_eq!(second.copying_nursery.in_place_promotion, commit);
    assert!(!second.copying_nursery.first_cycle_promotion_attempted);
    assert!(!second.copying_nursery.first_cycle_promotion_rolled_back);
    assert_eq!(crate::gc::untraced_promotion_cycles(), untraced_before);
    let after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    if commit {
        assert_eq!(
            after, before,
            "current high survival commits without copying"
        );
        assert!(!crate::arena::pointer_in_nursery(after));
        assert!(second.copying_nursery.in_place_promoted_blocks > 0);
    } else {
        assert_ne!(
            after, before,
            "refuted speculation must undo retag and evacuate"
        );
        assert!(crate::arena::pointer_in_nursery(after));
        assert!(
            second.copying_nursery.copied_objects > 0,
            "rollback must clear marks"
        );
    }
    assert!(!parent_probe_promotion_holds_up(0, 0));
    assert!(!parent_probe_promotion_holds_up(1000, 949));
    assert!(parent_probe_promotion_holds_up(1000, 950));
}

#[test]
fn parent_probe_commits_from_current_survival() {
    std::thread::spawn(|| run_current_parent_probe(true))
        .join()
        .unwrap();
}

#[test]
fn parent_probe_refuted_by_current_survival_rolls_back() {
    std::thread::spawn(|| run_current_parent_probe(false))
        .join()
        .unwrap();
}
