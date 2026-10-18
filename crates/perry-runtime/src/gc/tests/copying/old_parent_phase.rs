//! Root-first ordering (`gc/copying/old_parent_phase.rs`): a young object
//! reached only through an old parent is copied into survivor space instead
//! of being promoted by the adaptive threshold, because a minor cannot tell a
//! live old parent from a dead one.
//!
//! Every test pins the threshold to 1 (promote on first copy). That is the
//! schedule effect's bad minors ran at, and it makes the discriminating
//! assertion one bit: under the old order the object is in old-gen after one
//! minor, under the new order it is still in the nursery. Each test also
//! asserts a root-reached control IS promoted at the same threshold, so a
//! threshold change cannot make the tests pass vacuously.

use super::*;

fn bits_ptr(bits: u64) -> usize {
    (bits & POINTER_MASK) as usize
}

fn leaf_bytes(user: usize) -> Vec<u8> {
    unsafe {
        let s = user as *const crate::StringHeader;
        let data = (s as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        std::slice::from_raw_parts(data, (*s).byte_len as usize).to_vec()
    }
}

/// An old object with one field holding a fresh young string, stored through
/// the runtime barrier so the parent's page is remembered.
fn old_parent_of_one_young_leaf() -> (usize, *mut u64, Vec<u8>) {
    let (parent, slots) = unsafe { alloc_old_test_object(1) };
    let leaf = young_leaf();
    let bytes = leaf_bytes(leaf);
    let bits = string_bits(leaf);
    unsafe {
        *slots = bits;
    }
    runtime_write_barrier_slot(parent as usize, slots as usize, bits);
    (parent as usize, slots, bytes)
}

fn minor() -> GcCycleTrace {
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert!(
        !trace.copying_nursery.in_place_promotion,
        "premise: the minor copied; in-place promotion never reaches move_young"
    );
    trace
}

/// effect's shape: a dead old container holds young data. The minor keeps the
/// data young, and once the next full has freed the container the data is
/// never reached again: it died young.
#[test]
fn a_dead_old_parent_keeps_its_young_child_young_until_the_full_frees_it() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _stack = ConservativeScanDisabledGuard::new();
    let _tenuring = crate::gc::tenuring::set_survivals_for_test(1);

    let control = young_leaf();
    js_shadow_slot_set(0, string_bits(control));
    let (_parent, slot, bytes) = old_parent_of_one_young_leaf();

    let trace = minor();
    assert!(
        !crate::arena::pointer_in_nursery(bits_ptr(js_shadow_slot_get(0))),
        "control: a root-reached object is promoted at threshold 1"
    );
    let child = bits_ptr(unsafe { *slot });
    assert!(
        crate::arena::pointer_in_nursery(child),
        "the old-parent-only child must stay young, not be promoted"
    );
    unsafe { assert_string_bytes(child as *const crate::StringHeader, &bytes) };
    assert_eq!(trace.copying_nursery.old_parent_only_objects, 1);
    assert_eq!(trace.copying_nursery.old_parent_kept_young_objects, 1);
    assert_eq!(trace.copying_nursery.old_parent_overflow_promoted_objects, 0);

    // The parent was never rooted: the full frees it, and with it the only
    // edge to the child. The parent's memory is not read after this point.
    js_shadow_slot_set(0, crate::value::TAG_UNDEFINED);
    js_gc_collect();
    let after = minor();
    assert_eq!(
        after.copying_nursery.old_parent_only_objects, 0,
        "the freed parent must not seed the next minor"
    );
    assert_eq!(
        after.copying_nursery.copied_objects + after.copying_nursery.promoted_objects,
        0,
        "nothing young is reachable any more: the child died young"
    );
}

/// A live old container: its young child is copied (aging) while it is
/// reached only through the container, and promoted at the maximum young
/// age, with the container's slot naming the live bytes throughout.
#[test]
fn a_live_old_parent_gets_its_child_promoted_at_the_maximum_age() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _tenuring = crate::gc::tenuring::set_survivals_for_test(1);

    let (parent, slot, bytes) = old_parent_of_one_young_leaf();
    js_shadow_slot_set(0, ptr_bits(parent));

    for round in 1..GC_COPY_PROMOTION_SURVIVALS {
        let trace = minor();
        let child = bits_ptr(unsafe { *slot });
        assert!(
            crate::arena::pointer_in_nursery(child),
            "round {round}: below the maximum age the child stays young"
        );
        unsafe { assert_string_bytes(child as *const crate::StringHeader, &bytes) };
        assert_eq!(trace.copying_nursery.old_parent_kept_young_objects, 1);
    }
    let trace = minor();
    let child = bits_ptr(unsafe { *slot });
    assert!(
        !crate::arena::pointer_in_nursery(child),
        "at the maximum young age the child must be promoted"
    );
    unsafe { assert_string_bytes(child as *const crate::StringHeader, &bytes) };
    assert_eq!(trace.copying_nursery.old_parent_only_objects, 1);
    assert_eq!(
        trace.copying_nursery.old_parent_kept_young_objects, 0,
        "the max-age promotion is an obligation, not a kept-young decision"
    );
    assert_eq!(trace.copying_nursery.promoted_objects, 1);
}

/// Survivor space full: the old-parent-only child falls back to promotion,
/// exactly as before root-first ordering, and the fallback is counted.
#[test]
fn survivor_overflow_falls_back_to_promotion() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _tenuring = crate::gc::tenuring::set_survivals_for_test(1);
    let _budget = crate::gc::copying::set_survivor_budget_for_test(0);

    let (parent, slot, bytes) = old_parent_of_one_young_leaf();
    js_shadow_slot_set(0, ptr_bits(parent));

    let trace = minor();
    let child = bits_ptr(unsafe { *slot });
    assert!(
        !crate::arena::pointer_in_nursery(child),
        "an exhausted survivor budget must promote the child"
    );
    unsafe { assert_string_bytes(child as *const crate::StringHeader, &bytes) };
    assert_eq!(trace.copying_nursery.old_parent_only_objects, 1);
    assert_eq!(trace.copying_nursery.old_parent_kept_young_objects, 0);
    assert_eq!(trace.copying_nursery.old_parent_overflow_promoted_objects, 1);
}

/// A WeakMap key reached only through an old parent is live to the minor:
/// the ephemeron fixed point runs after both phases, so the value the root
/// phase could not yet shade is kept and the entry still answers.
#[test]
fn a_weakmap_key_reached_only_through_an_old_parent_keeps_its_value() {
    let _guard = CopyingNurseryTestGuard::new(2);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _tenuring = crate::gc::tenuring::set_survivals_for_test(1);

    let map = crate::weakref::js_weakmap_new();
    js_shadow_slot_set(0, ptr_bits(map as usize));
    let key = crate::object::js_object_alloc(0, 0);
    let value = young_leaf();
    let value_bytes = leaf_bytes(value);
    let map_v = f64::from_bits(js_shadow_slot_get(0));
    let _ = crate::weakref::js_weakmap_set(
        map_v,
        f64::from_bits(ptr_bits(key as usize)),
        f64::from_bits(string_bits(value)),
    );
    let (parent, slots) = unsafe { alloc_old_test_object(1) };
    let key_bits = ptr_bits(key as usize);
    unsafe {
        *slots = key_bits;
    }
    runtime_write_barrier_slot(parent as usize, slots as usize, key_bits);
    js_shadow_slot_set(1, ptr_bits(parent as usize));

    let trace = minor();
    assert!(trace.copying_nursery.old_parent_only_objects >= 1);
    let key_now = unsafe { *slots };
    assert!(
        crate::arena::pointer_in_nursery(bits_ptr(key_now)),
        "the key is old-parent-only: it stays young"
    );
    let map_v = f64::from_bits(js_shadow_slot_get(0));
    let got = crate::weakref::js_weakmap_get(map_v, f64::from_bits(key_now)).to_bits();
    assert_ne!(got, crate::value::TAG_UNDEFINED, "the entry must survive");
    unsafe { assert_string_bytes(bits_ptr(got) as *const crate::StringHeader, &value_bytes) };
}

/// A WeakRef whose target is reached only through an old parent: the target
/// is live to the minor and the weak slot is repaired to its new address.
#[test]
fn a_weakref_target_reached_only_through_an_old_parent_is_repaired() {
    let _guard = CopyingNurseryTestGuard::new(2);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _tenuring = crate::gc::tenuring::set_survivals_for_test(1);

    let target = crate::object::js_object_alloc(0, 0);
    let wr = crate::weakref::js_weakref_new(f64::from_bits(ptr_bits(target as usize)));
    js_shadow_slot_set(0, ptr_bits(wr as usize));
    let (parent, slots) = unsafe { alloc_old_test_object(1) };
    let target_bits = ptr_bits(target as usize);
    unsafe {
        *slots = target_bits;
    }
    runtime_write_barrier_slot(parent as usize, slots as usize, target_bits);
    js_shadow_slot_set(1, ptr_bits(parent as usize));

    let _ = minor();
    let target_now = bits_ptr(unsafe { *slots });
    assert_ne!(target_now, target as usize, "premise: the target moved");
    assert!(crate::arena::pointer_in_nursery(target_now));
    let wr_now = bits_ptr(js_shadow_slot_get(0));
    let deref = crate::weakref::js_weakref_deref(f64::from_bits(ptr_bits(wr_now))).to_bits();
    assert_eq!(
        bits_ptr(deref),
        target_now,
        "the weak slot must name the target's survivor-space copy"
    );
}

/// Stage 2: the copy work a kept-young cohort is owed outprices the full, so
/// the precise full at the minor's safepoint is due; it frees the dead parent,
/// the cohort dies in survivor space, and the full is priced productive.
#[test]
fn an_owed_old_parent_cohort_schedules_a_productive_full() {
    use crate::gc::promoted_cohort as cohort;
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _stack = ConservativeScanDisabledGuard::new();
    let _tenuring = crate::gc::tenuring::set_survivals_for_test(1);
    cohort::seed_for_tests(0, 0, 0);
    let _bound = cohort::sabotage::OldParentBoundGuard::pin(1);

    let _ = old_parent_of_one_young_leaf();
    let trace = minor();
    assert_eq!(trace.copying_nursery.old_parent_kept_young_objects, 1);
    assert!(
        trace.copying_nursery.old_parent_owed_copy_bytes
            >= 3 * trace.copying_nursery.old_parent_kept_young_bytes,
        "a first-round survivor owes three more copies"
    );
    assert!(cohort::old_parent_full_due(), "the owed copy work reaches the pinned price");
    assert!(crate::gc::policy::run_promoted_cohort_full_if_due(), "the full ran");
    assert_eq!(cohort::backoff_shift(), 0, "freeing the cohort is productive");
    assert!(!cohort::old_parent_full_due(), "the full settles the cohort");
    let after = minor();
    assert_eq!(after.copying_nursery.old_parent_only_objects, 0);
    assert_eq!(after.copying_nursery.copied_objects + after.copying_nursery.promoted_objects, 0);
    cohort::seed_for_tests(0, 0, 0);
}

/// A live old parent: the scheduled full proves it live, so its child
/// survives the full intact and the next minor still finds it.
#[test]
fn a_live_old_parent_cohort_survives_the_scheduled_full() {
    use crate::gc::promoted_cohort as cohort;
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _tenuring = crate::gc::tenuring::set_survivals_for_test(1);
    cohort::seed_for_tests(0, 0, 0);
    let _bound = cohort::sabotage::OldParentBoundGuard::pin(1);

    let (parent, slot, bytes) = old_parent_of_one_young_leaf();
    js_shadow_slot_set(0, ptr_bits(parent));
    let _ = minor();
    assert!(cohort::old_parent_full_due());
    assert!(crate::gc::policy::run_promoted_cohort_full_if_due());
    assert!(!cohort::old_parent_full_due(), "the full settles the cohort");
    let child = bits_ptr(unsafe { *slot });
    unsafe { assert_string_bytes(child as *const crate::StringHeader, &bytes) };
    let _ = minor();
    let child = bits_ptr(unsafe { *slot });
    unsafe { assert_string_bytes(child as *const crate::StringHeader, &bytes) };
    cohort::seed_for_tests(0, 0, 0);
}
