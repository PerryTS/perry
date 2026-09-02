//! Tombstoned ordered deletes (#2831 semantics, O(1) cost).
//!
//! A delete no longer shifts survivors: the entry is holed in place, raw
//! entry indices stay stable, and compaction runs only when tombstones
//! outnumber live entries or the array must grow. These tests pin the
//! observable contract — insertion order, delete-then-re-add, lookup
//! correctness across holes, iterator hole-skips, the no-compaction
//! contract of the raw-indexed readers, and the exact epoch-based cursor
//! rebase across squeezes (single-hole, multi-hole, successive, and `clear`).

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
fn raw_indexed_reads_never_compact_and_the_cursor_steps_over_holes() {
    let map = js_map_alloc(8);
    for k in [1.0f64, 2.0, 3.0] {
        js_map_set(map, k, k);
    }
    assert_eq!(js_map_delete(map, 2.0), 1);
    unsafe {
        assert_ne!((*map).used, (*map).size, "a hole is present");
    }
    // The RAW twins the for-of walkers use are plain bounded reads: raw
    // index 2 is still the THIRD key, the hole at 1 stays, and the layout is
    // untouched — the walkers' reads used to compact the whole map once per
    // observed hole.
    assert_eq!(js_map_entry_key_raw_at(map, 2), 3.0);
    assert_eq!(js_map_entry_value_raw_at(map, 2), 3.0);
    assert_eq!(
        js_map_entry_key_raw_at(map, 1).to_bits(),
        MAP_HOLE_KEY_BITS,
        "the raw twin exposes the hole — only the cursor ever reads it"
    );
    unsafe {
        assert_ne!(
            (*map).used,
            (*map).size,
            "the raw read left the layout alone"
        );
        assert_eq!(
            crate::map::map_compaction_epoch(map),
            0,
            "no squeeze happened"
        );
        // The hole is visible only to the cursor walker, which steps over it.
        assert_eq!(crate::map::map_cursor_next_raw(map, 0, 0), Some(0));
        assert_eq!(
            crate::map::map_cursor_next_raw(map, 1, 0),
            Some(2),
            "hole at 1 skipped"
        );
        assert_eq!(
            crate::map::map_cursor_next_raw(map, 3, 0),
            None,
            "extent exhausted"
        );
    }
    // The LIVE-index accessors (#9462 / #9504 — the array-like `map[i]` read,
    // console.table, collection equality) squeeze first so that live index 1
    // IS the third key and never a hole…
    assert_eq!(
        js_map_entry_key_at(map, 1),
        3.0,
        "live index 1 is the third key"
    );
    assert_eq!(js_map_entry_value_at(map, 1), 3.0);
    assert_eq!(
        js_map_entry_key_at(map, 2).to_bits(),
        crate::value::TAG_UNDEFINED,
        "past the live size is undefined, not a hole"
    );
    unsafe {
        assert_eq!(
            (*map).used,
            (*map).size,
            "the live accessor squeezed the hole"
        );
        assert_eq!(
            crate::map::map_compaction_epoch(map),
            1,
            "…and recorded it, so a cursor that was past the hole (raw 2, synced \
             at epoch 0) rebases onto the third key's new raw index 1"
        );
        assert_eq!(crate::map::map_cursor_next_raw(map, 2, 0), Some(1));
        assert_eq!(js_map_entry_key_raw_at(map, 1), 3.0);
    }
}

#[test]
fn cursor_rebases_exactly_across_a_multi_hole_compaction() {
    // 40 keys. A walk has yielded k0..k20 (cursor = 21) when the body deletes
    // exactly those 21 — holes now outnumber the 19 live entries, so the
    // delete path squeezes 21 holes below the cursor in ONE compaction. The
    // old key-based recovery read `cursor-1` and skipped 19 entries; the
    // rebase moves the cursor down by the removed count below it: 21 → 0.
    let map = js_map_alloc(64);
    for i in 0..40 {
        js_map_set(map, i as f64, i as f64);
    }
    let epoch0 = crate::map::map_compaction_epoch(map);
    for i in 0..=20 {
        assert_eq!(js_map_delete(map, i as f64), 1);
    }
    unsafe {
        assert_eq!((*map).used, (*map).size, "the delete path compacted");
    }
    assert_ne!(crate::map::map_compaction_epoch(map), epoch0);
    let next = unsafe { crate::map::map_cursor_next_raw(map, 21, epoch0) };
    assert_eq!(
        next,
        Some(0),
        "cursor 21 minus the 21 slots removed below it"
    );
    assert_eq!(
        js_map_entry_key_at(map, 0),
        21.0,
        "which is the true next key"
    );
    // A cursor already synchronised with the new epoch is not rebased again.
    let epoch1 = crate::map::map_compaction_epoch(map);
    assert_eq!(
        unsafe { crate::map::map_cursor_next_raw(map, 3, epoch1) },
        Some(3)
    );
}

#[test]
fn cursor_rebases_through_successive_squeezes_and_clear() {
    let map = js_map_alloc(64);
    for i in 0..40 {
        js_map_set(map, i as f64, i as f64);
    }
    let epoch0 = crate::map::map_compaction_epoch(map);
    // Squeeze 1 at the 21st delete (k0..k20 gone), squeeze 2 at the 32nd
    // (k21..k31 gone from the compacted layout: 8 live < 19 / 2). The cursor,
    // still at raw 21 with epoch0, must rebase through BOTH records in order.
    for i in 0..=31 {
        assert_eq!(js_map_delete(map, i as f64), 1);
    }
    unsafe {
        assert_eq!((*map).used, (*map).size);
        assert_eq!((*map).size, 8);
    }
    assert_eq!(
        unsafe { crate::map::map_cursor_next_raw(map, 21, epoch0) },
        Some(0)
    );
    assert_eq!(js_map_entry_key_at(map, 0), 32.0);
    // clear() during a walk discards the extent; a cursor then resumes at 0
    // and visits whatever is appended afterwards (the spec empties the
    // [[MapData]] list in place, so later adds are visited).
    let epoch1 = crate::map::map_compaction_epoch(map);
    js_map_clear(map);
    js_map_set(map, 100.0, 1.0);
    assert_eq!(
        unsafe { crate::map::map_cursor_next_raw(map, 5, epoch1) },
        Some(0)
    );
    assert_eq!(js_map_entry_key_at(map, 0), 100.0);
}

#[test]
fn a_fresh_map_at_a_reused_address_starts_without_history() {
    // A previous tenant's squeeze log must not rebase a new Map's cursor.
    let map = js_map_alloc(64);
    for i in 0..40 {
        js_map_set(map, i as f64, i as f64);
    }
    for i in 0..=20 {
        js_map_delete(map, i as f64);
    }
    assert_ne!(crate::map::map_compaction_epoch(map), 0);
    // Simulate address reuse: re-run the allocation-time reset on this
    // header and check a stale-epoch cursor is left alone.
    crate::map::test_reset_compaction_log_for(map);
    assert_eq!(
        unsafe { crate::map::map_cursor_next_raw(map, 5, 0) },
        Some(5)
    );
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

#[test]
fn cursor_stays_exact_across_forty_squeezes_in_one_body() {
    // A map at FULL capacity squeezes once per delete+re-add pair on the grow
    // path (`ensure_capacity`: used == capacity with a hole → compact), so one
    // loop body can force dozens of squeezes between two reads of a cursor.
    // History is budgeted by removed-index count, not record count, so all of
    // them are retained and the rebase stays exact.
    let map = js_map_alloc(64);
    for i in 0..64 {
        js_map_set(map, i as f64, i as f64);
    }
    unsafe {
        assert_eq!((*map).used, (*map).capacity, "premise: at capacity");
    }
    let epoch0 = crate::map::map_compaction_epoch(map);
    // The walk has yielded k0..k9 (cursor = 10). The body then deletes and
    // re-adds k0..k39: ten holes BELOW the cursor, thirty above, forty
    // squeezes in total.
    for i in 0..40 {
        assert_eq!(js_map_delete(map, i as f64), 1);
        js_map_set(map, i as f64, (i * 100) as f64);
    }
    let squeezes = crate::map::map_compaction_epoch(map).wrapping_sub(epoch0);
    assert!(
        squeezes >= 33,
        "premise: {squeezes} squeezes happened, need > 32"
    );
    unsafe {
        assert_eq!((*map).used, (*map).size);
    }
    // Every entry yielded so far (k0..k9) was deleted, so no live entry
    // remains below the old cursor: it rebases to 0, where the first
    // not-yet-visited live entry now sits — k40, because k10..k39 were
    // re-appended behind k40..k63 and behind the re-added k0..k9.
    assert_eq!(
        unsafe { crate::map::map_cursor_next_raw(map, 10, epoch0) },
        Some(0)
    );
    assert_eq!(js_map_entry_key_raw_at(map, 0), 40.0);
    // Live order is k40..k63, k0..k9, k10..k39 — the raw walk from the
    // rebased cursor sees exactly that.
    let mut order = Vec::new();
    let mut idx = 0u32;
    let epoch_now = crate::map::map_compaction_epoch(map);
    while let Some(i) = unsafe { crate::map::map_cursor_next_raw(map, idx, epoch_now) } {
        order.push(js_map_entry_key_raw_at(map, i));
        idx = i + 1;
    }
    let expected: Vec<f64> = (40..64).chain(0..40).map(|k| k as f64).collect();
    assert_eq!(order, expected);
}

#[test]
fn clear_truncates_the_squeeze_history() {
    let map = js_map_alloc(64);
    for i in 0..64 {
        js_map_set(map, i as f64, i as f64);
    }
    let epoch0 = crate::map::map_compaction_epoch(map);
    for i in 0..40 {
        js_map_delete(map, i as f64);
        js_map_set(map, i as f64, 0.0);
    }
    js_map_clear(map);
    js_map_set(map, 7.0, 7.0);
    // Whatever the cursor was, a clear rebases it to 0 — and the log behind
    // the clear record is gone, so this holds however many squeezes preceded
    // it.
    assert_eq!(
        unsafe { crate::map::map_cursor_next_raw(map, 10, epoch0) },
        Some(0)
    );
    assert_eq!(js_map_entry_key_raw_at(map, 0), 7.0);
}
