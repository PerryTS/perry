//! Incremental-mark write-barrier tests: stores into an already-marked
//! parent (object field, array element, closure capture/prototype, Map/Set
//! external slots) must mark the child, plus the mark-invariant verifier and
//! the barrier-inactive fallback. Split out of `gc/tests/barrier.rs` to stay
//! under the 2,000-line cap (#10750).

use super::*;

fn assert_marked_user_ptr(ptr: usize, label: &str) {
    unsafe {
        let header = header_from_user_ptr(ptr as *const u8);
        assert_ne!(
            (*header).gc_flags & GC_FLAG_MARKED,
            0,
            "{label} should be marked by the active incremental barrier"
        );
    }
}

fn mark_user_ptr(ptr: usize) {
    unsafe {
        let header = header_from_user_ptr(ptr as *const u8);
        (*header).gc_flags |= GC_FLAG_MARKED;
    }
}

fn clear_mark_user_ptr(ptr: usize) {
    unsafe {
        let header = header_from_user_ptr(ptr as *const u8);
        (*header).gc_flags &= !GC_FLAG_MARKED;
    }
}

#[test]
fn test_incremental_barrier_marks_object_field_store() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    let child = unsafe { alloc_nursery_test_object(0).0 as usize };
    let (obj, fields) = unsafe { alloc_old_test_object(1) };
    mark_user_ptr(obj as usize);
    let valid_ptrs = build_valid_pointer_set();
    let _barrier = IncrementalMarkBarrierTestGuard::new(&valid_ptrs);

    runtime_store_jsvalue_slot(obj as usize, fields as usize, 0, ptr_bits(child));
    drain_incremental_mark_barrier_seeds(&valid_ptrs);

    assert_marked_user_ptr(child, "object field child");
    let stats = verify_marked_heap_no_unmarked_children();
    assert_eq!(stats.missing_edges, 0);
    clear_mark_user_ptr(obj as usize);
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_incremental_barrier_marks_array_element_store() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    let child = unsafe { alloc_nursery_test_object(0).0 as usize };
    let (arr, elements) = unsafe { alloc_old_test_array(1) };
    mark_user_ptr(arr as usize);
    let valid_ptrs = build_valid_pointer_set();
    let _barrier = IncrementalMarkBarrierTestGuard::new(&valid_ptrs);

    runtime_store_jsvalue_slot(arr as usize, elements as usize, 0, ptr_bits(child));
    drain_incremental_mark_barrier_seeds(&valid_ptrs);

    assert_marked_user_ptr(child, "array element child");
    let stats = verify_marked_heap_no_unmarked_children();
    assert_eq!(stats.missing_edges, 0);
    clear_mark_user_ptr(arr as usize);
    clear_marks();
    remembered_set_clear();
}

/// Regression: a uniquely-owned (refcount==1) string stored into an object
/// field or array element must be demoted to shared (refcount==0) by the
/// write-barrier choke point, so a later `js_string_append` on the original
/// local allocates fresh instead of mutating the buffer in place and
/// corrupting the stored alias. The manifesting shape is a heap-stored snapshot
/// (`slot = s`) whose source is then grown (`s += chunk`): without the demote,
/// the append rewrites the slot the snapshot still points at, so a later
/// equality check against the snapshot wrongly sees the two as identical.
#[test]
fn test_store_demotes_unique_string_to_shared() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();

    // Build a uniquely-owned string via append (refcount becomes 1).
    let a = crate::string::js_string_from_bytes(b"john".as_ptr(), 4);
    let b = crate::string::js_string_from_bytes(b".".as_ptr(), 1);
    let unique = crate::string::js_string_append(a, b);
    assert_eq!(
        unsafe { (*unique).refcount },
        1,
        "js_string_append yields a uniquely-owned string"
    );

    // Store it into an object field slot via the choke point under test.
    let (obj, fields) = unsafe { alloc_old_test_object(1) };
    mark_user_ptr(obj as usize);
    runtime_store_jsvalue_slot(
        obj as usize,
        fields as usize,
        0,
        string_bits(unique as usize),
    );

    // The fix demotes the stored string to shared (refcount==0).
    assert_eq!(
        unsafe { (*unique).refcount },
        0,
        "string stored into an object field is demoted to shared"
    );

    // A subsequent append must allocate fresh and leave the stored buffer intact.
    let c = crate::string::js_string_from_bytes(b"doe".as_ptr(), 3);
    let grown = crate::string::js_string_append(unique, c);
    assert_ne!(
        grown, unique,
        "append after store allocates fresh — no in-place mutation of the aliased buffer"
    );
    assert_eq!(
        unsafe { (*unique).byte_len },
        5,
        "the stored buffer ('john.') was not grown in place"
    );
    let slot_bits = unsafe { std::ptr::read(fields as *const u64) };
    assert_eq!(
        slot_bits,
        string_bits(unique as usize),
        "the object field still references the original, unmutated string"
    );

    // Same guarantee for an array element store (shares the choke point).
    let unique2 = crate::string::js_string_append(
        crate::string::js_string_from_bytes(b"a".as_ptr(), 1),
        crate::string::js_string_from_bytes(b"b".as_ptr(), 1),
    );
    assert_eq!(unsafe { (*unique2).refcount }, 1);
    let (arr, elements) = unsafe { alloc_old_test_array(1) };
    mark_user_ptr(arr as usize);
    runtime_store_jsvalue_slot(
        arr as usize,
        elements as usize,
        0,
        string_bits(unique2 as usize),
    );
    assert_eq!(
        unsafe { (*unique2).refcount },
        0,
        "string stored into an array element is demoted to shared"
    );

    // ...and the same in-place-mutation guarantee as the object case above.
    let d = crate::string::js_string_from_bytes(b"c".as_ptr(), 1);
    let grown2 = crate::string::js_string_append(unique2, d);
    assert_ne!(
        grown2, unique2,
        "append after array-element store allocates fresh — no in-place mutation of the aliased buffer"
    );
    assert_eq!(
        unsafe { (*unique2).byte_len },
        2,
        "the stored array-element buffer ('ab') was not grown in place"
    );
    let elem_bits = unsafe { std::ptr::read(elements as *const u64) };
    assert_eq!(
        elem_bits,
        string_bits(unique2 as usize),
        "the array element still references the original, unmutated string"
    );

    clear_mark_user_ptr(obj as usize);
    clear_mark_user_ptr(arr as usize);
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_incremental_barrier_marks_closure_capture_store() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    let child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let closure = js_closure_alloc(crate::fn_info!(test_captured_singleton_func, 0), 1);
    mark_user_ptr(closure as usize);
    let valid_ptrs = build_valid_pointer_set();
    let _barrier = IncrementalMarkBarrierTestGuard::new(&valid_ptrs);

    crate::closure::js_closure_set_capture_ptr(closure, 0, child as i64);
    drain_incremental_mark_barrier_seeds(&valid_ptrs);

    assert_marked_user_ptr(child, "closure capture child");
    let stats = verify_marked_heap_no_unmarked_children();
    assert_eq!(stats.missing_edges, 0);
    clear_mark_user_ptr(closure as usize);
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_incremental_barrier_marks_closure_static_prototype_store() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    let proto = unsafe { alloc_nursery_test_object(0).0 as usize };
    let closure = js_closure_alloc(crate::fn_info!(test_no_capture_singleton_func, 0), 0);
    mark_user_ptr(closure as usize);
    let valid_ptrs = build_valid_pointer_set();
    let _barrier = IncrementalMarkBarrierTestGuard::new(&valid_ptrs);

    crate::closure::closure_set_static_prototype(closure as usize, ptr_bits(proto));
    drain_incremental_mark_barrier_seeds(&valid_ptrs);

    assert_marked_user_ptr(proto, "closure static prototype");
    // Reached via closure -> bag -> meta -> state record -> proto: no edge missing.
    let stats = verify_marked_heap_no_unmarked_children();
    assert!(stats.checked_edges >= 1);
    assert_eq!(stats.missing_edges, 0);
    clear_mark_user_ptr(closure as usize);
    crate::closure::test_clear_closure_side_tables();
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_incremental_barrier_marks_external_map_and_set_slots() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    let map_child = unsafe { alloc_nursery_test_object(0).0 as usize };
    let set_child = unsafe { alloc_nursery_test_object(0).0 as usize };
    let (map, entries, map_layout) = unsafe { alloc_old_test_map(1) };
    let (set, elements, set_layout) = unsafe { alloc_old_test_set(1) };
    unsafe {
        (*map).size = 1;
        (*map).used = 1;
        (*set).size = 1;
        (*set).used = 1;
    }
    mark_user_ptr(map as usize);
    mark_user_ptr(set as usize);
    let valid_ptrs = build_valid_pointer_set();
    let _barrier = IncrementalMarkBarrierTestGuard::new(&valid_ptrs);

    runtime_store_external_jsvalue_slot(map as usize, entries as usize, ptr_bits(map_child));
    runtime_store_external_jsvalue_slot(set as usize, elements as usize, ptr_bits(set_child));
    drain_incremental_mark_barrier_seeds(&valid_ptrs);

    assert_marked_user_ptr(map_child, "map external slot child");
    assert_marked_user_ptr(set_child, "set external slot child");
    let stats = verify_marked_heap_no_unmarked_children();
    assert_eq!(stats.missing_edges, 0);
    unsafe {
        retire_old_test_map(map, entries, map_layout);
        retire_old_test_set(set, elements, set_layout);
    }
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_mark_invariant_verifier_rejects_incremental_barrier_bypass() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    let child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (obj, fields) = unsafe { alloc_old_test_object(1) };
    mark_user_ptr(obj as usize);
    unsafe {
        *fields = ptr_bits(child);
    }

    let result = std::panic::catch_unwind(verify_marked_heap_no_unmarked_children);

    assert!(
        result.is_err(),
        "raw pointer-capable stores into marked parents must fail the mark verifier"
    );
    clear_mark_user_ptr(obj as usize);
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_store_outside_incremental_mark_keeps_generational_behavior_only() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    assert!(!incremental_mark_barrier_active());
    let child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (obj, fields) = unsafe { alloc_old_test_object(1) };

    runtime_store_jsvalue_slot(obj as usize, fields as usize, 0, ptr_bits(child));

    unsafe {
        let child_header = header_from_user_ptr(child as *const u8);
        assert_eq!(
            (*child_header).gc_flags & GC_FLAG_MARKED,
            0,
            "inactive incremental barrier must not mark the stored child"
        );
    }
    assert!(
        remembered_set_size() > 0,
        "inactive incremental barrier must leave old-to-young remembered-set behavior intact"
    );
    clear_marks();
    remembered_set_clear();
}
