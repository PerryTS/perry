//! #10057: indexed operations must preserve weak identity and moving-GC safety.
use super::*;
use crate::weakref::{
    js_weakmap_delete, js_weakmap_get, js_weakmap_has, js_weakmap_new, js_weakmap_set,
    js_weakset_add, js_weakset_new,
};

fn rooted(slot: u32) -> f64 {
    f64::from_bits(js_shadow_slot_get(slot))
}

fn root_object(slot: u32) {
    let object = crate::object::js_object_alloc(0, 0);
    js_shadow_slot_set(slot, ptr_bits(object as usize));
}

fn root_map() {
    let map = js_weakmap_new();
    js_shadow_slot_set(0, ptr_bits(map as usize));
}

fn entry_count() -> usize {
    let map = (js_shadow_slot_get(0) & POINTER_MASK) as *const crate::ObjectHeader;
    crate::weakref::weak_collection_entries(map).len()
}

#[test]
fn weakmap_index_identity_overwrite_delete_and_reuse() {
    let _guard = CopyingNurseryTestGuard::new(4);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    root_map();
    // Distinct objects with identical (empty) fields must be distinct keys.
    root_object(1);
    root_object(2);
    root_object(3);
    assert_eq!(
        js_weakmap_set(rooted(0), rooted(1), 11.0).to_bits(),
        rooted(0).to_bits()
    );
    js_weakmap_set(rooted(0), rooted(2), 22.0);
    assert_eq!(js_weakmap_get(rooted(0), rooted(1)), 11.0);
    assert_eq!(js_weakmap_get(rooted(0), rooted(2)), 22.0);
    js_weakmap_set(rooted(0), rooted(1), 33.0);
    assert_eq!(js_weakmap_get(rooted(0), rooted(1)), 33.0);
    assert_eq!(entry_count(), 2);
    assert_eq!(
        js_weakmap_delete(rooted(0), rooted(1)).to_bits(),
        crate::value::TAG_TRUE
    );
    assert_eq!(
        js_weakmap_delete(rooted(0), rooted(1)).to_bits(),
        crate::value::TAG_FALSE
    );
    assert_eq!(
        js_weakmap_has(rooted(0), rooted(1)).to_bits(),
        crate::value::TAG_FALSE
    );
    assert_eq!(
        js_weakmap_get(rooted(0), rooted(1)).to_bits(),
        crate::value::TAG_UNDEFINED
    );
    let extent = crate::weakref::test_support::weak_entry_extent(rooted(0));
    js_weakmap_set(rooted(0), rooted(3), 44.0);
    assert_eq!(
        crate::weakref::test_support::weak_entry_extent(rooted(0)),
        extent
    );
    assert_eq!(js_weakmap_get(rooted(0), rooted(3)), 44.0);
    assert_eq!(js_weakmap_get(rooted(0), rooted(2)), 22.0);
    assert_eq!(
        js_weakmap_has(rooted(0), rooted(1)).to_bits(),
        crate::value::TAG_FALSE
    );
}

#[test]
fn weakmap_index_entry_visits_are_linear() {
    // Counts the actual entry-array reads, including index rebuilds and
    // validation. This fails deterministically for the old n(n +/- 1)/2 scans.
    for n in [1_000u32, 10_000, 100_000] {
        let _guard = CopyingNurseryTestGuard::new(n + 1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        root_map();
        for slot in 1..=n {
            root_object(slot);
        }
        crate::weakref::test_support::reset_weak_entry_visits();
        for slot in 1..=n {
            js_weakmap_set(rooted(0), rooted(slot), slot as f64);
        }
        let inserts = crate::weakref::test_support::weak_entry_visits();
        assert!(
            inserts <= 4 * n as usize,
            "{n} inserts visited {inserts} entries"
        );
        crate::weakref::test_support::reset_weak_entry_visits();
        for slot in 1..=n {
            assert_eq!(js_weakmap_get(rooted(0), rooted(slot)), slot as f64);
        }
        let reads = crate::weakref::test_support::weak_entry_visits();
        assert!(reads >= n as usize, "counter must observe actual reads");
        assert!(reads <= 3 * n as usize, "{n} reads visited {reads} entries");
        crate::weakref::test_support::reset_weak_entry_visits();
        for slot in 1..=n {
            assert_eq!(
                js_weakmap_delete(rooted(0), rooted(slot)).to_bits(),
                crate::value::TAG_TRUE
            );
        }
        let deletes = crate::weakref::test_support::weak_entry_visits();
        eprintln!(
            "WeakMap n={n}: insert visits={inserts}, get visits={reads}, delete visits={deletes}"
        );
        assert!(
            deletes <= 3 * n as usize,
            "{n} deletes visited {deletes} entries"
        );
    }
}

#[test]
fn weakmap_index_rebuilds_after_moving_minors_and_full_collection() {
    let _guard = CopyingNurseryTestGuard::new(4);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _tenuring = crate::gc::tenuring::set_survivals_for_test(2);
    root_map();
    root_object(1);
    root_object(2);
    js_weakmap_set(rooted(0), rooted(1), rooted(2));
    for cycle in 0..3 {
        let old_key = rooted(1).to_bits();
        // Populate the cache BEFORE each collection, then force its reuse.
        assert_eq!(
            js_weakmap_get(rooted(0), rooted(1)).to_bits(),
            rooted(2).to_bits()
        );
        assert!(crate::weakref::test_support::cached_weak_collections() > 0);
        // Keep a young witness so even the post-promotion cycle must copy.
        root_object(3);
        let trace = collect_minor_trace(GcTriggerKind::Direct);
        assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
        assert_eq!(crate::weakref::test_support::cached_weak_collections(), 0);
        if cycle == 0 {
            assert_ne!(rooted(1).to_bits(), old_key);
        }
        assert_eq!(
            js_weakmap_get(rooted(0), rooted(1)).to_bits(),
            rooted(2).to_bits()
        );
        assert_eq!(
            js_weakmap_has(rooted(0), rooted(1)).to_bits(),
            crate::value::TAG_TRUE
        );
    }
    assert!(crate::arena::pointer_in_old_gen(
        (rooted(1).to_bits() & POINTER_MASK) as usize
    ));
    // An overwrite on the promoted entry must remember its new young value.
    root_object(2);
    js_weakmap_set(rooted(0), rooted(1), rooted(2));
    let before_value = rooted(2).to_bits();
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_ne!(rooted(2).to_bits(), before_value);
    assert_eq!(
        js_weakmap_get(rooted(0), rooted(1)).to_bits(),
        rooted(2).to_bits()
    );
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    assert_eq!(crate::weakref::test_support::cached_weak_collections(), 0);
    assert_eq!(
        js_weakmap_get(rooted(0), rooted(1)).to_bits(),
        rooted(2).to_bits()
    );
    assert_eq!(
        js_weakmap_delete(rooted(0), rooted(1)).to_bits(),
        crate::value::TAG_TRUE
    );
}

#[test]
fn weakmap_index_does_not_root_keys_or_alias_recycled_addresses() {
    let _guard = CopyingNurseryTestGuard::new(2);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    root_map();
    // Only the map/index sees this key. Assert tombstoning directly, with no
    // dependency on when a finalization callback happens to run.
    root_object(1);
    let dead_bits = rooted(1).to_bits();
    js_weakmap_set(rooted(0), rooted(1), 91.0);
    js_shadow_slot_set(1, 0);
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(entry_count(), 0);
    // Pointer-bit probes never dereference the old key. Even an exact reuse
    // of its address must not retrieve the dead key's value.
    assert_eq!(
        js_weakmap_get(rooted(0), f64::from_bits(dead_bits)).to_bits(),
        crate::value::TAG_UNDEFINED
    );
    let extent = crate::weakref::test_support::weak_entry_extent(rooted(0));
    for _ in 0..128 {
        root_object(1);
        assert_eq!(
            js_weakmap_has(rooted(0), rooted(1)).to_bits(),
            crate::value::TAG_FALSE
        );
        js_weakmap_set(rooted(0), rooted(1), 42.0);
        assert_eq!(js_weakmap_get(rooted(0), rooted(1)), 42.0);
        assert_eq!(
            js_weakmap_delete(rooted(0), rooted(1)).to_bits(),
            crate::value::TAG_TRUE
        );
    }
    assert_eq!(
        crate::weakref::test_support::weak_entry_extent(rooted(0)),
        extent
    );
    js_shadow_slot_set(0, 0);
    js_shadow_slot_set(1, 0);
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    assert_eq!(crate::weakref::test_support::cached_weak_collections(), 0);
    assert!(!crate::weakref::weak_target_holders_allocated());
}

#[test]
fn weakset_uses_the_same_index_after_collection_and_reuse() {
    let _guard = CopyingNurseryTestGuard::new(2);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let set = js_weakset_new();
    js_shadow_slot_set(0, ptr_bits(set as usize));
    root_object(1);
    assert_eq!(
        js_weakset_add(rooted(0), rooted(1)).to_bits(),
        rooted(0).to_bits()
    );
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(
        js_weakmap_has(rooted(0), rooted(1)).to_bits(),
        crate::value::TAG_TRUE
    );
    assert_eq!(
        js_weakmap_delete(rooted(0), rooted(1)).to_bits(),
        crate::value::TAG_TRUE
    );
    assert_eq!(
        js_weakmap_has(rooted(0), rooted(1)).to_bits(),
        crate::value::TAG_FALSE
    );
    assert_eq!(
        js_weakset_add(rooted(0), rooted(1)).to_bits(),
        rooted(0).to_bits()
    );
    assert_eq!(
        js_weakmap_has(rooted(0), rooted(1)).to_bits(),
        crate::value::TAG_TRUE
    );
}

#[test]
fn weakmap_index_revalidates_a_cached_slot_tombstoned_between_slices() {
    let _guard = CopyingNurseryTestGuard::new(3);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::weakref::test_support::clear_weak_holders();
    root_map();
    root_object(1);
    root_object(2);
    js_weakmap_set(rooted(0), rooted(1), 11.0);
    assert_eq!(js_weakmap_get(rooted(0), rooted(1)), 11.0);
    let holders = crate::weakref::test_support::weak_holder_addresses();
    assert_eq!(holders.len(), 1, "one registered entry");
    let entry = holders[0] as *mut crate::ObjectHeader;
    // Model a weak slice clearing the two fields while its mutator-side
    // cache survives. Actual collection/weakness is tested separately above;
    // this isolates stale-hit validation from the GC's cache-discard hook.
    crate::object::js_object_set_field(entry, 0, crate::JSValue::undefined());
    crate::object::js_object_set_field(entry, 1, crate::JSValue::undefined());
    assert_eq!(crate::weakref::test_support::cached_weak_collections(), 1);
    assert_eq!(
        js_weakmap_get(rooted(0), rooted(1)).to_bits(),
        crate::value::TAG_UNDEFINED
    );
    assert_eq!(
        js_weakmap_delete(rooted(0), rooted(1)).to_bits(),
        crate::value::TAG_FALSE
    );
    js_weakmap_set(rooted(0), rooted(2), 22.0);
    assert_eq!(
        crate::weakref::test_support::weak_entry_extent(rooted(0)),
        1
    );
    assert_eq!(js_weakmap_get(rooted(0), rooted(2)), 22.0);
    assert_eq!(
        js_weakmap_has(rooted(0), rooted(1)).to_bits(),
        crate::value::TAG_FALSE
    );
}

#[test]
fn weakmap_index_survives_explicit_full_gc_with_forced_movement() {
    let _guard = CopyingNurseryTestGuard::new(3);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _schedule =
        crate::gc::schedule::ScheduleGuard::set(7, crate::gc::schedule::rate_threshold(1.0));
    root_map();
    root_object(1);
    root_object(2);
    js_weakmap_set(rooted(0), rooted(1), rooted(2));
    assert_eq!(
        js_weakmap_get(rooted(0), rooted(1)).to_bits(),
        rooted(2).to_bits()
    );
    let key_before = rooted(1).to_bits();
    let value_before = rooted(2).to_bits();
    let copies_before = crate::gc::copying_minor_cycles();
    js_gc_collect();
    assert!(crate::gc::copying_minor_cycles() > copies_before);
    assert_ne!(rooted(1).to_bits(), key_before);
    assert_ne!(rooted(2).to_bits(), value_before);
    assert_eq!(
        js_weakmap_get(rooted(0), rooted(1)).to_bits(),
        rooted(2).to_bits()
    );
    assert_eq!(
        js_weakmap_has(rooted(0), rooted(1)).to_bits(),
        crate::value::TAG_TRUE
    );
}
