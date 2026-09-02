//! Tombstoned ordered Set deletes — the Set twin of `map_tombstone_tests`.

use super::*;

crate::perry_thread_local! {
    static FOREACH_DELETE_VISITS: std::cell::RefCell<Vec<u64>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

extern "C" fn delete_current_set_value(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
    _value_again: f64,
    collection: f64,
) -> f64 {
    FOREACH_DELETE_VISITS.with(|visits| visits.borrow_mut().push(value.to_bits()));
    let set = crate::value::js_nanbox_get_pointer(collection) as *mut SetHeader;
    js_set_delete(set, value);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

extern "C" fn delete_earlier_set_value(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
    _value_again: f64,
    collection: f64,
) -> f64 {
    FOREACH_DELETE_VISITS.with(|visits| visits.borrow_mut().push(value.to_bits()));
    if value > 0.0 {
        let set = crate::value::js_nanbox_get_pointer(collection) as *mut SetHeader;
        js_set_delete(set, value - 1.0);
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn take_foreach_delete_visits() -> Vec<f64> {
    FOREACH_DELETE_VISITS.with(|visits| visits.borrow_mut().drain(..).map(f64::from_bits).collect())
}

fn foreach_callback(func: *const u8) -> f64 {
    FOREACH_DELETE_VISITS.with(|visits| visits.borrow_mut().clear());
    let callback = crate::closure::js_closure_alloc(func, 0);
    crate::value::js_nanbox_pointer(callback as i64)
}

#[test]
fn foreach_survives_delete_compaction_threshold() {
    let expected = (0..20).map(|value| value as f64).collect::<Vec<_>>();
    let set = js_set_alloc(4);
    for &value in &expected {
        js_set_add(set, value);
    }
    js_set_foreach(
        set,
        foreach_callback(delete_current_set_value as *const u8),
        f64::from_bits(crate::value::TAG_UNDEFINED),
    );
    assert_eq!(take_foreach_delete_visits(), expected);
    assert_eq!(js_set_size(set), 0);
    unsafe {
        assert_eq!(
            (*set).used,
            0,
            "the completed walk runs deferred compaction"
        )
    };

    let set = js_set_alloc(4);
    for value in 0..20 {
        js_set_add(set, value as f64);
    }
    js_set_foreach(
        set,
        foreach_callback(delete_earlier_set_value as *const u8),
        f64::from_bits(crate::value::TAG_UNDEFINED),
    );
    assert_eq!(take_foreach_delete_visits(), expected);
    assert_eq!(js_set_size(set), 1);
}

#[test]
fn caught_throw_restores_set_foreach_compaction_state() {
    let set = js_set_alloc(4);
    let base = set_foreach_stack_savepoint();
    let _ = crate::exception::js_try_push();
    set_foreach_enter(set);
    assert!(set_foreach_is_active(set));
    crate::exception::test_unwind_innermost_shadow_restore();
    assert_eq!(set_foreach_stack_savepoint(), base);
    assert!(!set_foreach_is_active(set));
    crate::exception::js_try_end();
}

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
fn raw_indexed_reads_never_compact_and_the_cursor_steps_over_holes() {
    let set = js_set_alloc(8);
    for v in [1.0f64, 2.0, 3.0] {
        js_set_add(set, v);
    }
    assert_eq!(js_set_delete(set, 2.0), 1);
    unsafe {
        assert_ne!((*set).used, (*set).size, "a hole is present");
    }
    // The RAW twin the for-of walker uses is a plain bounded read: raw index
    // 2 is still the THIRD value, the hole at 1 stays, the layout is untouched
    // (the walker's reads used to compact the whole set once per observed
    // hole).
    assert_eq!(js_set_value_raw_at(set, 2), 3.0);
    assert_eq!(
        js_set_value_raw_at(set, 1).to_bits(),
        SET_HOLE_VALUE_BITS,
        "the raw twin exposes the hole — only the cursor ever reads it"
    );
    unsafe {
        assert_ne!(
            (*set).used,
            (*set).size,
            "the raw read left the layout alone"
        );
        assert_eq!(
            crate::set::set_compaction_epoch(set),
            0,
            "no squeeze happened"
        );
        assert_eq!(crate::set::set_cursor_next_raw(set, 0, 0), Some(0));
        assert_eq!(
            crate::set::set_cursor_next_raw(set, 1, 0),
            Some(2),
            "hole at 1 skipped"
        );
        assert_eq!(
            crate::set::set_cursor_next_raw(set, 3, 0),
            None,
            "extent exhausted"
        );
    }
    // The LIVE-index accessor (#9462 / #9504 — the array-like `set[i]` read)
    // squeezes first: live index 1 IS the third value, never a hole, and the
    // squeeze is recorded so a cursor past the hole rebases exactly.
    assert_eq!(
        js_set_value_at(set, 1),
        3.0,
        "live index 1 is the third value"
    );
    assert_eq!(
        js_set_value_at(set, 2).to_bits(),
        crate::value::TAG_UNDEFINED,
        "past the live size is undefined, not a hole"
    );
    unsafe {
        assert_eq!(
            (*set).used,
            (*set).size,
            "the live accessor squeezed the hole"
        );
        assert_eq!(crate::set::set_compaction_epoch(set), 1, "…and recorded it");
        assert_eq!(crate::set::set_cursor_next_raw(set, 2, 0), Some(1));
        assert_eq!(js_set_value_raw_at(set, 1), 3.0);
    }
}

#[test]
fn cursor_rebases_exactly_across_a_multi_hole_compaction() {
    // 40 values; a walk at cursor 21 while the body deletes v0..v20: holes
    // outnumber the 19 live values, one compaction squeezes all 21 below the
    // cursor. The rebase moves the cursor down by exactly that count.
    let set = js_set_alloc(64);
    for i in 0..40 {
        js_set_add(set, i as f64);
    }
    let epoch0 = crate::set::set_compaction_epoch(set);
    for i in 0..=20 {
        assert_eq!(js_set_delete(set, i as f64), 1);
    }
    unsafe {
        assert_eq!((*set).used, (*set).size, "the delete path compacted");
    }
    assert_ne!(crate::set::set_compaction_epoch(set), epoch0);
    assert_eq!(
        unsafe { crate::set::set_cursor_next_raw(set, 21, epoch0) },
        Some(0)
    );
    assert_eq!(js_set_value_at(set, 0), 21.0, "the true next value");
    let epoch1 = crate::set::set_compaction_epoch(set);
    assert_eq!(
        unsafe { crate::set::set_cursor_next_raw(set, 3, epoch1) },
        Some(3)
    );
}

#[test]
fn cursor_rebases_through_successive_squeezes_and_clear() {
    let set = js_set_alloc(64);
    for i in 0..40 {
        js_set_add(set, i as f64);
    }
    let epoch0 = crate::set::set_compaction_epoch(set);
    for i in 0..=31 {
        assert_eq!(js_set_delete(set, i as f64), 1);
    }
    unsafe {
        assert_eq!((*set).used, (*set).size);
        assert_eq!((*set).size, 8);
    }
    assert_eq!(
        unsafe { crate::set::set_cursor_next_raw(set, 21, epoch0) },
        Some(0)
    );
    assert_eq!(js_set_value_at(set, 0), 32.0);
    let epoch1 = crate::set::set_compaction_epoch(set);
    js_set_clear(set);
    js_set_add(set, 100.0);
    assert_eq!(
        unsafe { crate::set::set_cursor_next_raw(set, 5, epoch1) },
        Some(0)
    );
    assert_eq!(js_set_value_at(set, 0), 100.0);
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

#[test]
fn cursor_stays_exact_across_forty_squeezes_in_one_body() {
    // See the Map twin: a set at full capacity squeezes once per delete+re-add
    // pair on the grow path; the removed-index budget keeps every record.
    let set = js_set_alloc(64);
    for i in 0..64 {
        js_set_add(set, i as f64);
    }
    unsafe {
        assert_eq!((*set).used, (*set).capacity, "premise: at capacity");
    }
    let epoch0 = crate::set::set_compaction_epoch(set);
    for i in 0..40 {
        assert_eq!(js_set_delete(set, i as f64), 1);
        js_set_add(set, i as f64);
    }
    let squeezes = crate::set::set_compaction_epoch(set).wrapping_sub(epoch0);
    assert!(
        squeezes >= 33,
        "premise: {squeezes} squeezes happened, need > 32"
    );
    assert_eq!(
        unsafe { crate::set::set_cursor_next_raw(set, 10, epoch0) },
        Some(0)
    );
    assert_eq!(js_set_value_raw_at(set, 0), 40.0);
    let mut order = Vec::new();
    let mut idx = 0u32;
    let epoch_now = crate::set::set_compaction_epoch(set);
    while let Some(i) = unsafe { crate::set::set_cursor_next_raw(set, idx, epoch_now) } {
        order.push(js_set_value_raw_at(set, i));
        idx = i + 1;
    }
    let expected: Vec<f64> = (40..64).chain(0..40).map(|k| k as f64).collect();
    assert_eq!(order, expected);
}

#[test]
fn clear_truncates_the_squeeze_history() {
    let set = js_set_alloc(64);
    for i in 0..64 {
        js_set_add(set, i as f64);
    }
    let epoch0 = crate::set::set_compaction_epoch(set);
    for i in 0..40 {
        js_set_delete(set, i as f64);
        js_set_add(set, i as f64);
    }
    js_set_clear(set);
    js_set_add(set, 7.0);
    assert_eq!(
        unsafe { crate::set::set_cursor_next_raw(set, 10, epoch0) },
        Some(0)
    );
    assert_eq!(js_set_value_raw_at(set, 0), 7.0);
}
