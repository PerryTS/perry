//! Tombstoned ordered Set deletes — the Set twin of `map_tombstone_tests`.

use super::*;

#[test]
fn ordered_delete_preserves_order_and_lookup_across_holes() {
    let set = js_set_alloc(8);
    for v in [10.0f64, 20.0, 30.0, 40.0, 50.0] {
        js_set_add(set, v);
    }
    assert_eq!(js_set_delete(set, 30.0), 1, "middle");
    assert_eq!(js_set_delete(set, 10.0), 1, "front");
    assert_eq!(js_set_delete(set, 50.0), 1, "back");
    unsafe {
        assert_eq!((*set).size, 2);
    }
    assert_eq!(js_set_has(set, 20.0), 1);
    assert_eq!(js_set_has(set, 40.0), 1);
    for gone in [10.0f64, 30.0, 50.0] {
        assert_eq!(js_set_has(set, gone), 0, "{gone} was deleted");
    }
    // delete-then-re-add appends (#2831)
    js_set_add(set, 30.0);
    unsafe { compact_if_holey_set(set) };
    unsafe {
        let elements = elements_ptr(set);
        assert_eq!(ptr::read(elements), 20.0);
        assert_eq!(ptr::read(elements.add(1)), 40.0);
        assert_eq!(ptr::read(elements.add(2)), 30.0);
    }
}

#[test]
fn emptying_a_set_stays_consistent_and_compacts() {
    let set = js_set_alloc(16);
    for i in 0..64 {
        js_set_add(set, i as f64);
    }
    for i in 0..64 {
        assert_eq!(js_set_delete(set, i as f64), 1, "element {i} deletes once");
        assert_eq!(js_set_delete(set, i as f64), 0, "and only once");
    }
    unsafe {
        assert_eq!((*set).size, 0);
        assert!(
            (*set).used < 64,
            "the tombstone threshold must have compacted (used = {})",
            (*set).used
        );
    }
    js_set_add(set, 7.0);
    assert_eq!(js_set_has(set, 7.0), 1);
}

#[test]
fn raw_indexed_access_self_heals_by_compacting() {
    let set = js_set_alloc(8);
    for v in [1.0f64, 2.0, 3.0] {
        js_set_add(set, v);
    }
    assert_eq!(js_set_delete(set, 2.0), 1);
    unsafe {
        assert_ne!((*set).used, (*set).size, "a hole is present");
    }
    assert_eq!(js_set_value_at(set, 1), 3.0, "extern read compacts first");
    unsafe {
        assert_eq!((*set).used, (*set).size, "access healed the layout");
    }
}

#[test]
fn iterator_skips_holes_and_survives_deleting_the_last_returned_value() {
    unsafe {
        let set = js_set_alloc(8);
        for v in [1.0f64, 2.0, 3.0, 4.0] {
            js_set_add(set, v);
        }
        let iter = crate::value::js_nanbox_pointer(
            crate::collection_iter_object::js_set_values_iter_obj(set),
        );
        let val = |r: f64| {
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
        let next = |it: f64| crate::collection_iter_object::js_for_of_next(it);

        let r = next(iter);
        assert_eq!(val(r), 1.0);
        js_set_delete(set, 1.0); // the last-returned value
        js_set_delete(set, 3.0); // one ahead of the cursor
        assert_eq!(val(next(iter)), 2.0, "hole at the resume point is skipped");
        assert_eq!(val(next(iter)), 4.0, "hole ahead of the cursor is skipped");
        assert!(done(next(iter)), "then exhausted");
    }
}

#[test]
fn clear_resets_the_extent_and_walkers_compact() {
    let set = js_set_alloc(4);
    js_set_add(set, 1.0);
    js_set_add(set, 2.0);
    js_set_delete(set, 1.0);
    js_set_clear(set);
    unsafe {
        assert_eq!((*set).size, 0);
        assert_eq!((*set).used, 0);
    }
    // subset walker over a holey set must not see the holes
    let a = js_set_alloc(4);
    for v in [1.0f64, 2.0, 3.0] {
        js_set_add(a, v);
    }
    js_set_delete(a, 2.0);
    let b = js_set_alloc(4);
    js_set_add(b, 1.0);
    js_set_add(b, 3.0);
    assert_eq!(
        js_set_is_subset_of(
            a,
            f64::from_bits(crate::value::JSValue::pointer(b as *const u8).bits())
        ),
        1,
        "the hole must not defeat the subset walk"
    );
}
