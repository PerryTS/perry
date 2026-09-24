use super::*;

#[test]
fn string_collisions_survive_deletion_of_first_and_overflow_entries() {
    let mut index = StringIndex::default();
    for i in 0..10_000 {
        index.insert(i, i as u32);
    }
    assert_eq!(
        index.collisions.capacity(),
        0,
        "ordinary keys need no collision allocation"
    );
    index.insert(42, 10_001);
    index.insert(42, 10_002);
    index.insert(42, 10_003);
    index.remove(42, 10_002);
    index.remove(42, 42);
    let mut candidates: Vec<_> = index.candidates(42).collect();
    candidates.sort_unstable();
    assert_eq!(candidates, vec![10_001, 10_003]);
    index.remove(42, 10_001);
    index.remove(42, 10_003);
    assert!(index.candidates(42).next().is_none());
    assert!(index.collisions.is_empty());
    assert_eq!(index.candidates(43).collect::<Vec<_>>(), vec![43]);
    index.clear();
    assert!(index.first.is_empty());
}

#[test]
fn map_brand_rejects_foreign_and_interior_headers_before_reading_store() {
    assert!(!is_live_map(0));
    assert!(!is_live_map(crate::value::addr_class::HANDLE_BAND_MAX));
    assert!(!is_live_map(usize::MAX));
    let map = js_map_alloc(4);
    assert!(is_live_map(map as usize));
    assert!(!is_live_map(map as usize + 1));
    let array = crate::arena::arena_alloc_gc(128, 8, crate::gc::GC_TYPE_ARRAY);
    unsafe {
        // GC_STORE_AUDIT(INIT): plant header-shaped bytes in a test allocation to prove interior rejection.
        ptr::write(
            array.cast::<crate::gc::GcHeader>(),
            crate::gc::GcHeader {
                obj_type: crate::gc::GC_TYPE_MAP,
                gc_flags: crate::gc::GC_FLAG_ARENA,
                _reserved: 0,
                size: (crate::gc::GC_HEADER_SIZE + std::mem::size_of::<MapHeader>()) as u32,
            },
        );
        assert!(!is_live_map(array.add(crate::gc::GC_HEADER_SIZE) as usize));
        finalize_map_side_allocation_for_gc(map);
        assert!(!is_live_map(map as usize));
    }
}

#[test]
fn value_only_rewrites_skip_pointer_index_rebuild() {
    let map = js_map_alloc(16);
    let key = crate::object::js_object_alloc(0, 0);
    let key_bits = crate::value::POINTER_TAG | key as u64;
    js_map_set(map, f64::from_bits(key_bits), 1.0);
    for i in 0..10 {
        js_map_set(map, i as f64, i as f64);
    }
    unsafe {
        let before = (*(*map).store).pointer_rebuilds;
        js_map_set(map, f64::from_bits(key_bits), 2.0);
        rebuild_map_ptr_index_for_gc(map);
        assert_eq!((*(*map).store).pointer_rebuilds, before);
        // Model a GC rewrite of the key itself. The stale key is never
        // dereferenced by the decision to rebuild.
        let moved_key = crate::object::js_object_alloc(0, 0);
        let moved_bits = crate::value::POINTER_TAG | moved_key as u64;
        // Install the rewritten key with the external-slot barrier, then
        // exercise the same index hook the collector runs after a rewrite.
        crate::gc::runtime_store_external_jsvalue_slot(
            map as usize,
            (*map).entries as usize,
            moved_bits,
        );
        rebuild_map_ptr_index_for_gc(map);
        assert_eq!((*(*map).store).pointer_rebuilds, before + 1);
        assert_eq!(js_map_get(map, f64::from_bits(moved_bits)), 2.0);
        assert_eq!(js_map_has(map, f64::from_bits(key_bits)), 0);
    }
}
