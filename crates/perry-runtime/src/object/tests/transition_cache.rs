//! Shape transition-cache lookup/prune tests (split out of `object/tests.rs`
//! to keep it under the 2,000-line cap, #10750).

use super::*;

#[test]
fn transition_cache_lookup_rejects_mutated_edge_target() {
    let key = crate::string::js_string_from_bytes(b"id".as_ptr(), 2);
    let keys = crate::array::js_array_alloc(4);
    let keys = crate::array::js_array_push(keys, JSValue::string_ptr(key));
    let keys = crate::array::js_array_push(keys, JSValue::string_ptr(key));

    transition_cache_insert(std::ptr::null(), 0, key, keys as usize, 0, 0);

    // A slot-0 edge's target is the array's FIRST key, whatever the array's
    // length: the edge carries the count. What #6006 forbids is adopting the
    // longer list, so the hit must name exactly one key.
    let (target, slot_idx, _) =
        transition_cache_lookup(0, key).expect("the edge's target prefix is intact");
    assert_eq!((target.arr(), target.count(), slot_idx), (keys, 1, 0));

    let slot = transition_cache_slot(0, key as usize);
    with_transition_cache(|t| unsafe {
        // GC_STORE_AUDIT(ROOT): test cleanup writes non-pointer sentinels into scanned TRANSITION_CACHE_GLOBAL roots.
        (*t)[slot] = TransitionEntry {
            key_ptr: 0,
            next_keys: 0,
            prev_shape_id: 0,
            target_shape_id: 0,
            slot_idx: 0,
            target_len: 0,
        };
    });
}

#[test]
fn transition_cache_requires_exact_predecessor_shape_id() {
    let key = crate::string::js_string_from_bytes(b"shape-key".as_ptr(), 9);
    let keys = crate::array::js_array_alloc(4);
    let keys = crate::array::js_array_push(keys, JSValue::string_ptr(key));
    const PREDECESSOR: u32 = 101;
    const OTHER_PREDECESSOR: u32 = 102;
    const TARGET: u32 = 201;

    transition_cache_insert(std::ptr::null(), PREDECESSOR, key, keys as usize, 0, TARGET);
    assert!(
        transition_cache_lookup(OTHER_PREDECESSOR, key).is_none(),
        "equal keys edges with different semantic ShapeIds must not alias"
    );
    assert_eq!(
        transition_cache_lookup(PREDECESSOR, key),
        Some((
            crate::object::ObjectKeys::new(keys as *mut ArrayHeader, 1),
            0,
            TARGET
        ))
    );

    let slot = transition_cache_slot(PREDECESSOR, key as usize);
    with_transition_cache(|table| unsafe {
        (*table)[slot] = TransitionEntry {
            key_ptr: 0,
            next_keys: 0,
            prev_shape_id: 0,
            target_shape_id: 0,
            slot_idx: 0,
            target_len: 0,
        };
    });
}

#[test]
fn transition_cache_prunes_a_descriptorless_target_shape() {
    let _lock = crate::gc::global_side_table_test_lock();
    let next_keys = crate::array::js_array_alloc(0);
    let predecessor = crate::object::shapes::shape_id_for_keys_ensure(std::ptr::null(), 0);
    let target = crate::object::shapes::shape_descriptor_ensure(next_keys, 0, 0)
        .expect("shape range unexpectedly exhausted");
    let occupancy_before = test_transition_cache_occupancy();
    transition_cache_insert(
        std::ptr::null(),
        predecessor,
        std::ptr::null(),
        next_keys as usize,
        0,
        target,
    );
    assert_eq!(test_transition_cache_occupancy(), occupancy_before + 1);

    crate::object::shapes::test_drop_shape_descriptors(next_keys as usize);
    assert!(crate::object::shapes::shape_descriptor_by_id(target).is_none());
    prune_dead_transition_cache_entries(&|_| false);
    assert_eq!(
        test_transition_cache_occupancy(),
        occupancy_before,
        "a descriptorless target must release its rooted transition edge"
    );
}

#[test]
fn transition_cache_lookup_rejects_slot_key_mismatch() {
    // The target bytes remain independently validated even though predecessor
    // identity now uses a stable ShapeId. Adopting a mismatched target would
    // store the value at the wrong slot.
    let want = crate::string::js_string_from_bytes(b"alpha".as_ptr(), 5);
    let other = crate::string::js_string_from_bytes(b"beta".as_ptr(), 4);

    // A target shape whose slot 0 holds `beta`, not `alpha`.
    let keys = crate::array::js_array_alloc(4);
    let keys = crate::array::js_array_push(keys, JSValue::string_ptr(other));

    // Insert an edge keyed on (prev=0, `alpha`) but targeting the `beta` shape,
    // mirroring a recycled-address false match (target_len is set because the
    // length matches slot_idx+1, so only the content check can catch it).
    transition_cache_insert(std::ptr::null(), 0, want, keys as usize, 0, 0);

    assert!(
        transition_cache_lookup(0, want).is_none(),
        "a cache edge whose target slot holds a different key must be rejected (#6006)"
    );

    // Sanity: an edge whose target slot DOES hold the key still hits.
    let good_keys = crate::array::js_array_alloc(4);
    let good_keys = crate::array::js_array_push(good_keys, JSValue::string_ptr(want));
    transition_cache_insert(std::ptr::null(), 0, want, good_keys as usize, 0, 0);
    assert!(
        transition_cache_lookup(0, want).is_some(),
        "a genuine edge (target slot holds the key) must still hit (#6006)"
    );

    let slot = transition_cache_slot(0, want as usize);
    with_transition_cache(|t| unsafe {
        // GC_STORE_AUDIT(ROOT): test cleanup writes non-pointer sentinels into scanned TRANSITION_CACHE_GLOBAL roots.
        (*t)[slot] = TransitionEntry {
            key_ptr: 0,
            next_keys: 0,
            prev_shape_id: 0,
            target_shape_id: 0,
            slot_idx: 0,
            target_len: 0,
        };
    });
}

#[test]
fn transition_cache_lookup_rejects_grown_shared_target() {
    // #6006: a cached edge's target is a snapshot. A shared target array
    // grows IN PLACE after caching when it is a canonical backing whose tip
    // is extended, so the array is now longer than the edge's list. Adopting
    // the ARRAY'S length would give the object more keys than it has values —
    // keys present, values undefined. The edge's count is what the hit must
    // carry: the grown backing still holds the target as its prefix.
    let key = crate::string::js_string_from_bytes(b"gamma".as_ptr(), 5);
    let extra = crate::string::js_string_from_bytes(b"delta".as_ptr(), 5);

    // A 1-key target with spare capacity, cached as a slot-0 edge (target_len=1).
    let keys = crate::array::js_array_alloc(4);
    let keys = crate::array::js_array_push(keys, JSValue::string_ptr(key));
    transition_cache_insert(std::ptr::null(), 0, key, keys as usize, 0, 0);
    assert!(
        transition_cache_lookup(0, key).is_some(),
        "sanity: a genuine 1-key edge hits before the target grows (#6006)"
    );

    // Grow the SAME array in place to length 2 (as a sibling object would).
    let keys2 = crate::array::js_array_push(keys, JSValue::string_ptr(extra));
    // `js_array_push` grows in place when capacity allows (cap was 4), so the
    // cached `next_keys` pointer still points at the now-length-2 array.
    assert_eq!(
        keys2, keys,
        "test setup: push must grow in place, not realloc"
    );

    let (target, _, _) = transition_cache_lookup(0, key)
        .expect("a grown shared target still holds the edge's list as its prefix");
    assert_eq!(
        (target.arr(), target.count()),
        (keys, 1),
        "the hit must carry the edge's count, not the grown array's length (#6006)"
    );

    let slot = transition_cache_slot(0, key as usize);
    with_transition_cache(|t| unsafe {
        // GC_STORE_AUDIT(ROOT): test cleanup writes non-pointer sentinels into scanned TRANSITION_CACHE_GLOBAL roots.
        (*t)[slot] = TransitionEntry {
            key_ptr: 0,
            next_keys: 0,
            prev_shape_id: 0,
            target_shape_id: 0,
            slot_idx: 0,
            target_len: 0,
        };
    });
}
