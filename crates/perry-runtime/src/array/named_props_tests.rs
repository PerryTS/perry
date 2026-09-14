//! Named properties stored WITH the array (#10166 brief 4): the reserved
//! front slot, the three ways an existing array gains it, growth, the
//! empty-queue reset, enumeration order, and the sparse-index alias.

use super::*;

unsafe fn key(name: &str) -> *const crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

unsafe fn set(arr: *mut ArrayHeader, name: &str, value: f64) -> *mut ArrayHeader {
    let head = array_named_property_set(arr, key(name), value);
    assert!(!head.is_null(), "setting {name} must return the live head");
    head
}

unsafe fn get(arr: *const ArrayHeader, name: &str) -> Option<f64> {
    array_named_property_get_by_name(arr, name)
}

unsafe fn names(arr: *const ArrayHeader) -> Vec<String> {
    array_named_property_names(arr, false)
}

unsafe fn physical_capacity(arr: *const ArrayHeader) -> usize {
    array_physical_capacity(arr)
}

fn pushed(values: &[f64]) -> *mut ArrayHeader {
    let mut arr = js_array_alloc(values.len() as u32);
    for &v in values {
        arr = js_array_push_f64(arr, v);
    }
    arr
}

unsafe fn assert_elements(arr: *const ArrayHeader, expected: &[f64]) {
    assert_eq!(js_array_length(arr), expected.len() as u32);
    for (i, &v) in expected.iter().enumerate() {
        assert_eq!(js_array_get_f64(arr, i as u32), v, "element {i}");
    }
}

#[test]
fn set_get_has_delete_keep_insertion_order() {
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let arr = pushed(&[1.0, 2.0, 3.0]);
        assert_eq!(test_named_props_state(arr), (false, 0, 0));
        assert!(!array_has_named_properties_resolved(arr));

        let arr = set(arr, "a", 10.0);
        let arr = set(arr, "b", 20.0);
        let arr = set(arr, "c", 30.0);
        assert_eq!(names(arr), ["a", "b", "c"]);
        assert!(array_has_named_properties_resolved(arr));
        let (flagged, pairs, count) = test_named_props_state(arr);
        assert!(flagged);
        assert_ne!(pairs, 0);
        assert_eq!(count, 3);

        // Overwrite in place: no new pair, same pairs array.
        let same = set(arr, "b", 21.0);
        assert_eq!(same, arr, "an overwrite never moves the array");
        assert_eq!(test_named_props_state(arr), (true, pairs, 3));
        assert_eq!(get(arr, "b"), Some(21.0));

        assert!(array_named_property_has(arr, key("c")));
        assert!(!array_named_property_has(arr, key("zz")));
        assert_eq!(get(arr, "zz"), None);

        assert!(array_named_property_delete_by_name(arr, "b"));
        assert!(!array_named_property_delete_by_name(arr, "b"));
        assert_eq!(names(arr), ["a", "c"]);
        assert_eq!(get(arr, "a"), Some(10.0));
        assert_eq!(get(arr, "c"), Some(30.0));

        let arr = set(arr, "d", 40.0);
        assert_eq!(
            names(arr),
            ["a", "c", "d"],
            "a re-added key enumerates last"
        );
        assert_elements(arr, &[1.0, 2.0, 3.0]);
    }
}

#[test]
fn first_expando_on_an_array_with_slack_keeps_its_address() {
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let arr = pushed(&[1.0, 2.0, 3.0]);
        let capacity = (*arr).capacity;
        assert!(capacity > 3, "test premise: the literal has tail slack");
        assert_eq!(array_front_offset(arr), 0);
        let physical = physical_capacity(arr);

        let head = set(arr, "tag", 7.0);
        assert_eq!(
            head, arr,
            "slack is taken in place: the array does not move"
        );
        assert_eq!(
            array_front_offset(arr),
            1,
            "physical slot 0 is now the reserve"
        );
        assert_eq!((*arr).capacity, capacity - 1);
        assert_eq!(physical_capacity(arr), physical, "same allocation");
        assert_elements(arr, &[1.0, 2.0, 3.0]);
        assert_eq!(get(arr, "tag"), Some(7.0));

        // Pushing into the remaining slack never touches the reserve.
        let mut arr = arr;
        for v in 4..=(capacity as i32) {
            arr = js_array_push_f64(arr, v as f64);
        }
        assert_eq!(get(arr, "tag"), Some(7.0));
        assert_eq!(js_array_get_f64(arr, 0), 1.0);
    }
}

#[test]
fn a_full_array_keeps_its_address_and_uses_the_fallback_table() {
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let arr = js_array_alloc_with_length_exact(3);
        for i in 0..3 {
            store_array_slot(arr, i, ((i + 1) as f64).to_bits());
        }
        assert_eq!(
            (*arr).length,
            (*arr).capacity,
            "test premise: no slack anywhere"
        );

        let head = set(arr, "tag", 7.0);
        let head = set(head, "other", 8.0);
        assert_eq!(head, arr, "a full array must not move to gain a property");
        assert_eq!(array_front_offset(arr), 0, "no reserve was taken");
        assert_eq!(test_named_props_state(arr), (false, 0, 0));
        assert!(test_full_array_named_property_owner_exists(arr as usize));
        assert!(array_has_named_properties_resolved(arr));
        assert_elements(arr, &[1.0, 2.0, 3.0]);
        assert_eq!(get(arr, "tag"), Some(7.0));
        assert_eq!(names(arr), ["tag", "other"]);
        assert!(array_named_property_delete_by_name(arr, "tag"));
        assert_eq!(names(arr), ["other"]);

        // Growth rekeys the fallback entry; the array never switches stores.
        let grown = js_array_grow(arr, 64);
        assert_ne!(grown, arr);
        assert!(test_full_array_named_property_owner_exists(grown as usize));
        assert!(!test_full_array_named_property_owner_exists(arr as usize));
        let grown = set(grown, "late", 9.0);
        assert_eq!(
            test_named_props_state(grown),
            (false, 0, 0),
            "still no reserve"
        );
        assert_eq!(names(grown), ["other", "late"]);
        test_clear_full_array_named_property_roots();
    }
}

#[test]
fn first_expando_on_a_shifted_queue_takes_the_dead_front_slot() {
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let arr = pushed(&[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(js_array_shift_f64(arr), 1.0);
        assert_eq!(
            array_front_offset(arr),
            1,
            "test premise: one consumed front slot"
        );
        let capacity = (*arr).capacity;

        let head = set(arr, "tag", 7.0);
        assert_eq!(head, arr);
        assert_eq!(array_front_offset(arr), 1, "the dead slot IS the reserve");
        assert_eq!(
            (*arr).capacity,
            capacity,
            "no element moved, capacity unchanged"
        );
        assert_elements(arr, &[2.0, 3.0, 4.0]);
        assert_eq!(get(arr, "tag"), Some(7.0));

        // A named property makes the array "exotic" for the dense-queue
        // shift (same `OBJ_FLAG_ARRAY_DESCRIPTORS` gate as before), so the
        // second shift takes the spec path and moves elements down instead of
        // consuming more front; either way the reserve stays in front.
        assert_eq!(js_array_shift_f64(arr), 2.0);
        assert!(array_front_offset(arr) >= 1);
        assert_elements(arr, &[3.0, 4.0]);
        assert_eq!(get(arr, "tag"), Some(7.0));
    }
}

#[test]
fn growth_carries_the_reserve_and_the_pairs() {
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let arr = set(pushed(&[1.0, 2.0]), "tag", 7.0);
        let (_, pairs, _) = test_named_props_state(arr);
        let grown = js_array_grow(arr, 100);
        assert_ne!(grown, arr);
        assert_eq!(
            array_front_offset(grown),
            1,
            "the replacement keeps the reserve"
        );
        assert_eq!(physical_capacity(grown), (*grown).capacity as usize + 1);
        assert_eq!(
            test_named_props_state(grown),
            (true, pairs, 1),
            "same pairs array"
        );
        assert_eq!(get(grown, "tag"), Some(7.0));
        assert_elements(grown, &[1.0, 2.0]);
        let mut grown = grown;
        for v in 3..=100 {
            grown = js_array_push_f64(grown, v as f64);
        }
        assert_eq!(js_array_get_f64(grown, 99), 100.0);
        assert_eq!(get(grown, "tag"), Some(7.0));
    }
}

#[test]
fn shift_to_empty_keeps_the_reserve_out_of_capacity() {
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let arr = set(pushed(&[1.0, 2.0]), "tag", 7.0);
        let physical = physical_capacity(arr);
        // A named property routes the public `shift` through the spec path
        // (see the shifted-queue case above), which never touches `capacity`.
        // The dense-queue reset is what a reserved array WITHOUT the
        // descriptors bit would take, so drive it directly: this is the case
        // that catches a reset returning the reserve to `capacity`.
        assert_eq!(super::storage::shift_dense(arr), 1.0);
        assert_eq!(super::storage::shift_dense(arr), 2.0);
        assert_eq!((*arr).length, 0);
        assert_eq!(
            (*arr).capacity as usize,
            physical - 1,
            "the empty-queue reset returns every consumed slot except the reserve"
        );
        assert_eq!(array_front_offset(arr), 1);
        assert_eq!(get(arr, "tag"), Some(7.0));
        let arr = js_array_push_f64(arr, 5.0);
        assert_elements(arr, &[5.0]);
        assert_eq!(
            get(arr, "tag"),
            Some(7.0),
            "the push did not clobber the reserve"
        );
    }
}

#[test]
fn sparse_indices_are_named_properties_and_return_the_live_head() {
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        // Slack in the allocation, so the sparse index takes the reserve path.
        let arr = js_array_alloc_with_length_exact(3);
        (*arr).length = 2;
        store_array_slot(arr, 0, 1.0f64.to_bits());
        store_array_slot(arr, 1, 2.0f64.to_bits());
        let head = js_array_set_f64_extend(arr, 5_000_000, 9.0);
        assert!(!head.is_null());
        assert_eq!(js_array_length(head), 5_000_001);
        assert_eq!(js_array_get_f64(head, 5_000_000), 9.0);
        assert_eq!(js_array_get_f64(head, 0), 1.0);
        assert!(array_has_sparse_index_properties_resolved(head));
        assert_eq!(get(head, "5000000"), Some(9.0));
        assert_eq!(names(head), ["5000000"]);
        assert_eq!(
            js_array_get_f64(arr, 5_000_000),
            9.0,
            "reads through the stub"
        );
    }
}

#[test]
fn a_moved_key_string_is_shared_not_rewritten_in_place() {
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let arr = pushed(&[1.0]);
        let k = key("name") as *mut crate::StringHeader;
        (*k).refcount = 1;
        let arr = array_named_property_set(arr, k, 3.0);
        assert!(!arr.is_null());
        assert_eq!(
            (*k).refcount,
            0,
            "the pairs array shares the key: an append must copy"
        );
        assert_eq!(get(arr, "name"), Some(3.0));
    }
}

#[test]
fn a_long_array_with_slack_does_not_shift_its_elements() {
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let mut arr = js_array_alloc(256);
        for i in 0..200 {
            arr = js_array_push_f64(arr, i as f64);
        }
        assert!(
            (*arr).capacity > (*arr).length,
            "test premise: spare capacity"
        );
        let elems = array_elements_ptr(arr);
        let head = set(arr, "tag", 1.0);
        assert_eq!(head, arr);
        assert_eq!(array_elements_ptr(arr), elems, "no element moved");
        assert_eq!(array_front_offset(arr), 0, "no in-place reserve was opened");
        assert!(test_full_array_named_property_owner_exists(arr as usize));
        assert_eq!(get(arr, "tag"), Some(1.0));
        assert_eq!(js_array_get_f64(arr, 199), 199.0);
        test_clear_full_array_named_property_roots();
    }
}

#[test]
fn named_property_arrays_keep_the_array_prototype() {
    let _global = crate::gc::global_side_table_test_lock();
    let array_proto = crate::object::builtin_prototype_value("Array");
    assert!(
        crate::value::JSValue::from_bits(array_proto.to_bits()).is_pointer(),
        "test premise: the realm has an Array.prototype"
    );
    let proto_of = |arr: *mut ArrayHeader| {
        crate::object::js_object_get_prototype_of(crate::value::js_nanbox_pointer(arr as i64))
            .to_bits()
    };
    unsafe {
        // Pairs reserve (spare capacity), fallback table (full), shifted queue.
        let pairs = set(pushed(&[1.0]), "tag", 1.0);
        assert_eq!(
            test_named_props_state(pairs).0,
            true,
            "premise: reserve taken"
        );
        assert_eq!(proto_of(pairs), array_proto.to_bits());

        let full = js_array_alloc_with_length_exact(1);
        store_array_slot(full, 0, 1.0f64.to_bits());
        let full = set(full, "tag", 1.0);
        assert_eq!(proto_of(full), array_proto.to_bits());

        let queue = pushed(&[1.0, 2.0]);
        js_array_shift_f64(queue);
        let queue = set(queue, "tag", 1.0);
        assert_eq!(proto_of(queue), array_proto.to_bits());
        test_clear_full_array_named_property_roots();
    }
}

#[cfg(feature = "regex-engine")]
mod inline {
    use super::*;
    use crate::array::InlineKeySet;

    unsafe fn exec_result(values: [f64; 3]) -> *mut ArrayHeader {
        let arr = js_array_alloc_named_props_reserved(2, InlineKeySet::ExecResult);
        (*arr).length = 2;
        store_array_slot(arr, 0, 1.0f64.to_bits());
        store_array_slot(arr, 1, 2.0f64.to_bits());
        array_named_props_install_inline(arr, &values);
        arr
    }

    #[test]
    fn install_is_inline_and_reads_back_in_key_order() {
        let _global = crate::gc::global_side_table_test_lock();
        unsafe {
            let arr = exec_result([4.0, 5.0, 6.0]);
            assert_eq!(
                test_named_props_inline_set(arr),
                Some(InlineKeySet::ExecResult)
            );
            assert_eq!(test_named_props_state(arr), (true, 0, 3), "no pairs array");
            assert_eq!(array_front_offset(arr), 4, "header word + three values");
            assert_elements(arr, &[1.0, 2.0]);
            assert_eq!(names(arr), ["index", "input", "groups"]);
            assert_eq!(get(arr, "index"), Some(4.0));
            assert_eq!(get(arr, "groups"), Some(6.0));
            assert_eq!(get(arr, "indices"), None, "not in this set");
            assert!(array_named_property_has(arr, key("input")));
            assert!(array_has_named_properties_resolved(arr));
            assert!(!array_has_sparse_index_properties_resolved(arr));
        }
    }

    #[test]
    fn overwrite_stays_inline_and_delete_never_allocates() {
        let _global = crate::gc::global_side_table_test_lock();
        unsafe {
            let arr = exec_result([4.0, 5.0, 6.0]);
            assert_eq!(set(arr, "index", 40.0), arr);
            assert_eq!(
                test_named_props_inline_set(arr),
                Some(InlineKeySet::ExecResult)
            );
            assert_eq!(get(arr, "index"), Some(40.0));

            assert!(array_named_property_delete_by_name(arr, "input"));
            assert!(!array_named_property_delete_by_name(arr, "input"));
            assert_eq!(
                test_named_props_inline_set(arr),
                Some(InlineKeySet::ExecResult)
            );
            assert_eq!(names(arr), ["index", "groups"]);
            assert_eq!(get(arr, "input"), None);
            assert_eq!(
                array_named_props_reserve(arr),
                4,
                "the reserve size is unchanged"
            );
        }
    }

    #[test]
    fn inline_results_keep_the_array_prototype() {
        let _global = crate::gc::global_side_table_test_lock();
        let array_proto = crate::object::builtin_prototype_value("Array");
        unsafe {
            let arr = exec_result([4.0, 5.0, 6.0]);
            let proto = crate::object::js_object_get_prototype_of(crate::value::js_nanbox_pointer(
                arr as i64,
            ));
            assert_eq!(proto.to_bits(), array_proto.to_bits());
        }
    }

    #[test]
    fn inline_results_keep_element_fast_paths_until_a_user_key_arrives() {
        let _global = crate::gc::global_side_table_test_lock();
        unsafe {
            let arr = exec_result([4.0, 5.0, 6.0]);
            assert_eq!(
                array_object_flags(arr) & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS,
                0,
                "an exec result must not set the index-descriptor gate"
            );
            assert!(!array_iteration_is_exotic(arr));
            // Named-property readers do not depend on the gate.
            assert_eq!(names(arr), ["index", "input", "groups"]);
            assert!(array_has_named_properties_resolved(arr));
            // Truncation keeps named properties (they are not indices).
            js_array_set_length(arr, 0.0);
            assert_eq!(js_array_length(arr), 0);
            assert_eq!(get(arr, "groups"), Some(6.0));

            let arr = set(arr, "user", 1.0);
            assert_ne!(
                array_object_flags(arr) & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS,
                0,
                "a key outside the inline set materializes and sets the gate"
            );
        }
    }

    #[test]
    fn a_new_key_materializes_pairs_in_insertion_order() {
        let _global = crate::gc::global_side_table_test_lock();
        unsafe {
            let arr = exec_result([4.0, 5.0, 6.0]);
            assert!(array_named_property_delete_by_name(arr, "index"));
            let arr = set(arr, "extra", 9.0);
            let arr = set(arr, "index", 41.0);
            assert_eq!(test_named_props_inline_set(arr), None, "now pairs mode");
            let (flagged, pairs, count) = test_named_props_state(arr);
            assert!(flagged && pairs != 0);
            assert_eq!(count, 4);
            assert_eq!(
                names(arr),
                ["input", "groups", "extra", "index"],
                "surviving inline keys, then keys in the order they were added"
            );
            assert_eq!(get(arr, "input"), Some(5.0));
            assert_eq!(get(arr, "index"), Some(41.0));
            assert_eq!(array_named_props_reserve(arr), 1);
            assert_elements(arr, &[1.0, 2.0]);
        }
    }

    #[test]
    fn growth_carries_every_inline_slot() {
        let _global = crate::gc::global_side_table_test_lock();
        unsafe {
            let arr = exec_result([4.0, 5.0, 6.0]);
            let grown = js_array_grow(arr, 200);
            assert_ne!(grown, arr);
            assert_eq!(array_front_offset(grown), 4);
            assert_eq!(physical_capacity(grown), (*grown).capacity as usize + 4);
            assert_eq!(names(grown), ["index", "input", "groups"]);
            assert_eq!(get(grown, "groups"), Some(6.0));
            assert_elements(grown, &[1.0, 2.0]);
        }
    }

    #[test]
    fn shift_to_empty_keeps_the_whole_inline_reserve() {
        let _global = crate::gc::global_side_table_test_lock();
        unsafe {
            let arr = exec_result([4.0, 5.0, 6.0]);
            let physical = physical_capacity(arr);
            assert_eq!(super::super::storage::shift_dense(arr), 1.0);
            assert_eq!(super::super::storage::shift_dense(arr), 2.0);
            assert_eq!((*arr).capacity as usize, physical - 4);
            assert_eq!(get(arr, "input"), Some(5.0));
        }
    }
}
