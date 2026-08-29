//! Tombstoned ordered deletes (#2831 semantics, O(1) cost).
//!
//! A delete no longer shifts survivors: the entry is holed in place, raw
//! entry indices stay stable, and compaction runs only when tombstones
//! outnumber live entries or the array must grow. These tests pin the
//! observable contract — insertion order, delete-then-re-add, lookup
//! correctness across holes, iterator hole-skips, and the self-healing
//! compaction under raw-indexed access.

use super::*;

crate::perry_thread_local! {
    static FOREACH_DELETE_VISITS: std::cell::RefCell<Vec<(u64, u64)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

extern "C" fn delete_current_map_entry(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
    key: f64,
    collection: f64,
) -> f64 {
    FOREACH_DELETE_VISITS.with(|visits| visits.borrow_mut().push((key.to_bits(), value.to_bits())));
    let map = crate::value::js_nanbox_get_pointer(collection) as *mut MapHeader;
    js_map_delete(map, key);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

extern "C" fn delete_earlier_map_entry(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
    key: f64,
    collection: f64,
) -> f64 {
    FOREACH_DELETE_VISITS.with(|visits| visits.borrow_mut().push((key.to_bits(), value.to_bits())));
    if key > 0.0 {
        let map = crate::value::js_nanbox_get_pointer(collection) as *mut MapHeader;
        js_map_delete(map, key - 1.0);
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn take_foreach_delete_visits() -> Vec<(f64, f64)> {
    FOREACH_DELETE_VISITS.with(|visits| {
        visits
            .borrow_mut()
            .drain(..)
            .map(|(key, value)| (f64::from_bits(key), f64::from_bits(value)))
            .collect()
    })
}

fn foreach_callback(func: *const u8) -> f64 {
    FOREACH_DELETE_VISITS.with(|visits| visits.borrow_mut().clear());
    let callback = crate::closure::js_closure_alloc(func, 0);
    crate::value::js_nanbox_pointer(callback as i64)
}

#[test]
fn foreach_survives_delete_compaction_threshold() {
    let map = js_map_alloc(4);
    let expected = (0..20)
        .map(|key| (key as f64, (key * 10) as f64))
        .collect::<Vec<_>>();
    for &(key, value) in &expected {
        js_map_set(map, key, value);
    }
    js_map_foreach(
        map,
        foreach_callback(delete_current_map_entry as *const u8),
        f64::from_bits(crate::value::TAG_UNDEFINED),
    );
    assert_eq!(take_foreach_delete_visits(), expected);
    assert_eq!(js_map_size(map), 0);
    unsafe {
        assert_eq!(
            (*map).used,
            0,
            "the completed walk runs deferred compaction"
        )
    };

    let map = js_map_alloc(4);
    for &(key, value) in &expected {
        js_map_set(map, key, value);
    }
    js_map_foreach(
        map,
        foreach_callback(delete_earlier_map_entry as *const u8),
        f64::from_bits(crate::value::TAG_UNDEFINED),
    );
    assert_eq!(take_foreach_delete_visits(), expected);
    assert_eq!(js_map_size(map), 1);
}

#[test]
fn caught_throw_restores_map_foreach_compaction_state() {
    let map = js_map_alloc(4);
    let base = map_foreach_stack_savepoint();
    let _ = crate::exception::js_try_push();
    map_foreach_enter(map);
    assert!(map_foreach_is_active(map));
    crate::exception::test_unwind_innermost_shadow_restore();
    assert_eq!(map_foreach_stack_savepoint(), base);
    assert!(!map_foreach_is_active(map));
    crate::exception::js_try_end();
}

#[test]
fn ordered_delete_preserves_order_and_lookup_across_holes() {
    let map = js_map_alloc(8);
    for k in [10.0f64, 20.0, 30.0, 40.0, 50.0] {
        js_map_set(map, k, k * 10.0);
    }

    assert_eq!(js_map_delete(map, 30.0), 1, "middle");
    assert_eq!(js_map_delete(map, 10.0), 1, "front");
    assert_eq!(js_map_delete(map, 50.0), 1, "back");
    unsafe {
        assert_eq!((*map).size, 2);
        assert!((*map).used >= 2, "holes may remain before compaction");
    }

    // Survivors resolve, deleted keys do not — through every lookup lane.
    assert_eq!(js_map_get(map, 20.0), 200.0);
    assert_eq!(js_map_get(map, 40.0), 400.0);
    for gone in [10.0f64, 30.0, 50.0] {
        assert_eq!(js_map_has(map, gone), 0, "{gone} was deleted");
    }

    // Delete-then-re-add appends at the end (#2831): iteration order is
    // 20, 40, 30 after re-adding 30.
    js_map_set(map, 30.0, 999.0);
    unsafe { compact_if_holey(map) };
    unsafe {
        let entries = entries_ptr(map);
        assert_eq!(ptr::read(entries), 20.0);
        assert_eq!(ptr::read(entries.add(2)), 40.0);
        assert_eq!(ptr::read(entries.add(4)), 30.0);
    }
    assert_eq!(js_map_get(map, 30.0), 999.0);
}

#[test]
fn emptying_a_map_stays_consistent_and_compacts() {
    let map = js_map_alloc(16);
    for i in 0..64 {
        js_map_set(map, i as f64, (i * 2) as f64);
    }
    for i in 0..64 {
        assert_eq!(js_map_delete(map, i as f64), 1, "key {i} deletes once");
        assert_eq!(js_map_delete(map, i as f64), 0, "and only once");
    }
    unsafe {
        assert_eq!((*map).size, 0);
        assert!(
            (*map).used < 64,
            "the tombstone threshold must have compacted at least once (used = {})",
            (*map).used
        );
    }
    for i in 0..64 {
        assert_eq!(js_map_has(map, i as f64), 0);
    }
    js_map_set(map, 7.0, 70.0);
    assert_eq!(
        js_map_get(map, 7.0),
        70.0,
        "the emptied map still accepts inserts"
    );
}

#[test]
fn raw_indexed_access_self_heals_by_compacting() {
    let map = js_map_alloc(8);
    for k in [1.0f64, 2.0, 3.0] {
        js_map_set(map, k, k);
    }
    assert_eq!(js_map_delete(map, 2.0), 1);
    unsafe {
        assert_ne!((*map).used, (*map).size, "a hole is present");
    }
    // The raw-indexed extern compacts first, so entry 1 is the THIRD key —
    // exactly what the typed for-of lane's fallback needs for raw == live.
    assert_eq!(js_map_entry_key_at(map, 1), 3.0);
    unsafe {
        assert_eq!((*map).used, (*map).size, "access healed the layout");
    }
}

#[test]
fn iterator_skips_holes_and_survives_deleting_the_last_returned_key() {
    unsafe {
        let iter = crate::value::js_nanbox_pointer(
            crate::collection_iter_object::js_map_keys_iter_obj(map_with(&[1.0, 2.0, 3.0, 4.0])),
        );
        let key = |r: f64| {
            f64::from_bits(
                crate::object::js_object_get_field(
                    crate::value::js_nanbox_get_pointer(r) as *mut crate::object::ObjectHeader,
                    0,
                )
                .bits(),
            )
        };
        let done = |r: f64| {
            crate::value::JSValue::from_bits(
                crate::object::js_object_get_field(
                    crate::value::js_nanbox_get_pointer(r) as *mut crate::object::ObjectHeader,
                    1,
                )
                .bits(),
            )
            .as_bool()
        };
        let next = |iter: f64| crate::collection_iter_object::js_for_of_next(iter);

        let backing = iter_backing(iter);
        let r = next(iter);
        assert_eq!(key(r), 1.0);
        // Delete the key we just returned, and one ahead of the cursor.
        js_map_delete(backing, 1.0);
        js_map_delete(backing, 3.0);
        let r = next(iter);
        assert_eq!(key(r), 2.0, "hole at the cursor's resume point is skipped");
        let r = next(iter);
        assert_eq!(key(r), 4.0, "hole ahead of the cursor is skipped");
        assert!(done(next(iter)), "then exhausted");
    }
}

#[test]
fn clear_resets_the_extent() {
    let map = js_map_alloc(4);
    js_map_set(map, 1.0, 1.0);
    js_map_set(map, 2.0, 2.0);
    js_map_delete(map, 1.0);
    js_map_clear(map);
    unsafe {
        assert_eq!((*map).size, 0);
        assert_eq!((*map).used, 0);
    }
    js_map_set(map, 9.0, 90.0);
    assert_eq!(js_map_get(map, 9.0), 90.0);
}

fn map_with(keys: &[f64]) -> *mut MapHeader {
    let map = js_map_alloc(8);
    for &k in keys {
        js_map_set(map, k, k * 100.0);
    }
    map
}

unsafe fn iter_backing(iter: f64) -> *mut MapHeader {
    let obj = crate::value::js_nanbox_get_pointer(iter) as *mut crate::object::ObjectHeader;
    crate::value::js_nanbox_get_pointer(f64::from_bits(
        crate::object::js_object_get_field(obj, 0).bits(),
    )) as *mut MapHeader
}
