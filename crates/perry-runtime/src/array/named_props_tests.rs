//! Arrays carry ordinary named holders independently of element storage.
//! Cover full/slack/queue births, growth, deletion order, sparse indices,
//! key ownership and regex producer layouts.

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
    crate::gc::initialize_runtime_fixture();
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
fn named_properties_do_not_move_elements_or_arm_element_descriptors() {
    let _global = crate::gc::global_side_table_test_lock();
    crate::gc::initialize_runtime_fixture();
    unsafe {
        let arr = js_array_alloc_literal(3);
        js_array_set_f64(arr, 0, 1.0);
        js_array_set_f64(arr, 1, 2.0);
        js_array_set_f64(arr, 2, 3.0);
        let elements = array_elements_ptr(arr);
        let capacity = (*arr).capacity;
        let named = set(arr, "pos", 1.0);
        let named = set(named, "end", 2.0);
        assert_eq!(arr, named);
        assert_eq!(elements, array_elements_ptr(named));
        assert_eq!(capacity, (*named).capacity);
        assert!(
            !crate::array::array_has_element_descriptors(named),
            "default-attribute named keys leave the elements plain"
        );
        assert_eq!(names(named), ["pos", "end"]);
        let holder = (*named).named;
        let grown = js_array_grow(named, 100);
        assert_eq!((*grown).named, holder);
        assert_eq!(get(grown, "pos"), Some(1.0));
        assert_eq!(get(grown, "end"), Some(2.0));
        assert_elements(grown, &[1.0, 2.0, 3.0]);
    }
}

#[test]
fn named_holder_is_independent_of_slack_fullness_and_queue_front() {
    let _global = crate::gc::global_side_table_test_lock();
    crate::gc::initialize_runtime_fixture();
    unsafe {
        for (length, capacity, shift) in [(3, 8, false), (3, 3, false), (4, 4, true)] {
            let arr = js_array_alloc_with_length_exact(capacity);
            (*arr).length = length;
            for i in 0..length {
                store_array_slot(arr, i as usize, (i as f64 + 1.0).to_bits());
            }
            if shift {
                assert_eq!(js_array_shift_f64(arr), 1.0);
            }
            let elements = array_elements_ptr(arr);
            let capacity = (*arr).capacity;
            let front = array_front_offset(arr);
            let physical = physical_capacity(arr);
            let named = set(arr, "tag", 7.0);
            assert_eq!(named, arr);
            assert_eq!(array_elements_ptr(named), elements);
            assert_eq!((*named).capacity, capacity);
            assert_eq!(array_front_offset(named), front);
            assert_eq!(physical_capacity(named), physical);
            assert!(!(*named).named.is_null());
            assert_eq!(get(named, "tag"), Some(7.0));
            let first = if shift { 2.0 } else { 1.0 };
            assert_eq!(js_array_shift_f64(named), first);
            assert_eq!(get(named, "tag"), Some(7.0));
            let grown = js_array_grow(named, 100);
            assert_eq!(get(grown, "tag"), Some(7.0));
            assert_eq!(
                get(named, "tag"),
                Some(7.0),
                "growth alias resolves the same holder"
            );
        }
    }
}

#[test]
fn empty_queue_reset_keeps_named_properties_out_of_capacity() {
    let _global = crate::gc::global_side_table_test_lock();
    crate::gc::initialize_runtime_fixture();
    unsafe {
        let arr = set(pushed(&[1.0, 2.0]), "tag", 7.0);
        let physical = physical_capacity(arr);
        assert_eq!(super::storage::shift_dense(arr), 1.0);
        assert_eq!(super::storage::shift_dense(arr), 2.0);
        assert_eq!((*arr).length, 0);
        assert_eq!((*arr).capacity as usize, physical);
        assert_eq!(array_front_offset(arr), 0);
        let arr = js_array_push_f64(arr, 5.0);
        assert_elements(arr, &[5.0]);
        assert_eq!(get(arr, "tag"), Some(7.0));
    }
}

#[test]
fn sparse_indices_use_the_holder_and_keep_the_dense_frontier() {
    let _global = crate::gc::global_side_table_test_lock();
    crate::gc::initialize_runtime_fixture();
    unsafe {
        let arr = js_array_alloc_with_length_exact(3);
        (*arr).length = 2;
        store_array_slot(arr, 0, 1.0f64.to_bits());
        store_array_slot(arr, 1, 2.0f64.to_bits());
        let elements = array_elements_ptr(arr);
        let head = js_array_set_f64_extend(arr, 5_000_000, 9.0);
        assert_eq!(head, arr);
        assert_eq!(array_elements_ptr(head), elements);
        assert_eq!(js_array_length(head), 5_000_001);
        assert_eq!(js_array_get_f64(head, 5_000_000), 9.0);
        assert_eq!(js_array_get_f64(head, 0), 1.0);
        assert!(array_has_sparse_index_properties_resolved(head));
        assert_eq!(get(head, "5000000"), Some(9.0));
        assert_eq!(names(head), ["5000000"]);
    }
}

#[test]
fn property_key_storage_does_not_alias_a_mutable_input_string() {
    let _global = crate::gc::global_side_table_test_lock();
    crate::gc::initialize_runtime_fixture();
    unsafe {
        let k = key("named-key-ownership") as *mut crate::StringHeader;
        (*k).refcount = 1;
        let arr = array_named_property_set(pushed(&[1.0]), k, 3.0);
        let holder = (*arr).named;
        let keys = crate::object::object_keys(holder);
        let slot = crate::object::keys_find_property_slot_by_bytes(
            keys,
            keys.count(),
            b"named-key-ownership",
        )
        .unwrap();
        let stored = keys.get(slot);
        assert!(
            (*k).refcount == 0 || stored.as_string_ptr() != k,
            "a retained key must be immutable or independently owned"
        );
        assert_eq!(get(arr, "named-key-ownership"), Some(3.0));
    }
}

#[test]
fn long_named_arrays_keep_their_elements_and_prototype() {
    let _global = crate::gc::global_side_table_test_lock();
    crate::gc::initialize_runtime_fixture();
    unsafe {
        let mut arr = js_array_alloc(256);
        for i in 0..200 {
            arr = js_array_push_f64(arr, i as f64);
        }
        let elements = array_elements_ptr(arr);
        let capacity = (*arr).capacity;
        let head = set(arr, "tag", 1.0);
        assert_eq!(head, arr);
        assert_eq!(array_elements_ptr(head), elements);
        assert_eq!((*head).capacity, capacity);
        assert_eq!(js_array_get_f64(head, 199), 199.0);
        assert_eq!(get(head, "tag"), Some(1.0));
        let proto =
            crate::object::js_object_get_prototype_of(crate::value::js_nanbox_pointer(head as i64));
        assert_eq!(
            proto.to_bits(),
            crate::object::builtin_prototype_value("Array").to_bits()
        );
    }
}

#[cfg(feature = "regex-engine")]
mod layouts {
    use super::*;

    unsafe fn exec_result() -> *mut ArrayHeader {
        let arr = js_array_alloc_named_props_reserved(2, NamedPropertyKeys::ExecResult);
        (*arr).length = 2;
        store_array_slot(arr, 0, 1.0f64.to_bits());
        store_array_slot(arr, 1, 2.0f64.to_bits());
        array_named_props_install(arr, &[4.0, 5.0, 6.0]);
        arr
    }

    #[test]
    fn regex_birth_slots_preserve_values_elements_and_prototype() {
        let _global = crate::gc::global_side_table_test_lock();
        crate::gc::initialize_runtime_fixture();
        unsafe {
            let arr = exec_result();
            let holder = (*arr).named;
            let desc = std::ptr::addr_of!(crate::object::runtime_layouts::REGEX_EXEC_NAMED);
            assert_eq!((*holder).parent_class_id, (*desc).layout_id);
            assert_eq!(array_front_offset(arr), 0);
            assert_eq!(test_named_props_state(arr), (true, holder as usize, 3));
            assert_elements(arr, &[1.0, 2.0]);
            assert_eq!(names(arr), ["index", "input", "groups"]);
            assert_eq!(get(arr, "index"), Some(4.0));
            assert_eq!(get(arr, "groups"), Some(6.0));
            assert_eq!(get(arr, "indices"), None);
            assert!(array_named_property_has(arr, key("input")));
            assert!(!array_has_sparse_index_properties_resolved(arr));
            assert_eq!(
                crate::object::js_object_get_prototype_of(crate::value::js_nanbox_pointer(
                    arr as i64
                ))
                .to_bits(),
                crate::object::builtin_prototype_value("Array").to_bits()
            );
            assert!(!crate::symbol::has_own_symbol_property(
                crate::value::js_nanbox_pointer(arr as i64),
                crate::value::js_nanbox_pointer(crate::symbol::well_known_symbol("iterator") as i64)
            ));
        }
    }

    #[test]
    fn regex_deletion_readdition_and_extra_keys_keep_property_order() {
        let _global = crate::gc::global_side_table_test_lock();
        crate::gc::initialize_runtime_fixture();
        unsafe {
            let arr = exec_result();
            let holder = (*arr).named;
            assert_eq!(set(arr, "index", 40.0), arr);
            assert_eq!((*arr).named, holder);
            assert_eq!(get(arr, "index"), Some(40.0));
            assert!(array_named_property_delete_by_name(arr, "input"));
            assert!(!array_named_property_delete_by_name(arr, "input"));
            assert_eq!(names(arr), ["index", "groups"]);
            assert_eq!(get(arr, "input"), None);
            let arr = set(arr, "extra", 9.0);
            let arr = set(arr, "input", 41.0);
            assert_eq!(names(arr), ["index", "groups", "extra", "input"]);
            assert_eq!(get(arr, "input"), Some(41.0));
            assert_elements(arr, &[1.0, 2.0]);
            assert_eq!(array_front_offset(arr), 0);
        }
    }

    #[test]
    fn regex_growth_truncation_and_queue_reset_keep_named_slots() {
        let _global = crate::gc::global_side_table_test_lock();
        crate::gc::initialize_runtime_fixture();
        unsafe {
            let arr = exec_result();
            let holder = (*arr).named;
            let grown = js_array_grow(arr, 200);
            assert_ne!(grown, arr);
            assert_eq!((*grown).named, holder);
            assert_eq!(array_front_offset(grown), 0);
            assert_eq!(physical_capacity(grown), (*grown).capacity as usize);
            assert_elements(grown, &[1.0, 2.0]);
            assert!(!array_iteration_is_exotic(grown));
            assert_eq!(super::super::storage::shift_dense(grown), 1.0);
            assert_eq!(super::super::storage::shift_dense(grown), 2.0);
            assert_eq!((*grown).capacity as usize, physical_capacity(grown));
            assert_eq!(get(grown, "input"), Some(5.0));
            js_array_set_length(grown, 0.0);
            let grown = set(grown, "user", 1.0);
            assert!(!crate::array::array_has_element_descriptors(grown));
            assert_eq!(get(grown, "groups"), Some(6.0));
        }
    }
}

#[test]
fn regex_layout_birth_installs_values_in_declared_key_order() {
    let _global = crate::gc::global_side_table_test_lock();
    crate::gc::initialize_runtime_fixture();
    unsafe {
        for kind in [
            NamedPropertyKeys::ExecResult,
            NamedPropertyKeys::ExecResultIndices,
            NamedPropertyKeys::IndicesGroups,
        ] {
            let arr = js_array_alloc_named_props_reserved(0, kind);
            let values: Vec<f64> = (0..kind.keys().len()).map(|i| (i + 3) as f64).collect();
            array_named_props_install(arr, &values);
            for (key, value) in kind.keys().iter().zip(values) {
                assert_eq!(get(arr, key), Some(value));
            }
            assert!(!array_has_element_descriptors(arr));
            let named = array_property_bag(arr);
            assert_eq!(
                crate::object::js_object_get_field(named, 0).bits(),
                crate::value::TAG_HOLE,
                "a regex birth reserves absent iterator slot zero"
            );
        }
    }
}

#[test]
fn regex_layout_overwrite_delete_and_readd_preserve_element_values() {
    let _global = crate::gc::global_side_table_test_lock();
    crate::gc::initialize_runtime_fixture();
    unsafe {
        let arr = js_array_alloc_named_props_reserved(4, NamedPropertyKeys::ExecResult);
        let arr = js_array_push_f64(arr, 17.0);
        array_named_props_install(arr, &[3.0, 4.0, 5.0]);
        let named = array_property_bag(arr);
        let arr = set(arr, "input", 9.0);
        assert_eq!(array_property_bag(arr), named);
        assert_eq!(get(arr, "input"), Some(9.0));
        assert!(array_named_property_delete_by_name(arr, "input"));
        assert_eq!(get(arr, "input"), None);
        let arr = set(arr, "input", 11.0);
        assert_eq!(get(arr, "input"), Some(11.0));
        assert_elements(arr, &[17.0]);
        assert_eq!(names(arr), ["index", "groups", "input"]);
    }
}

#[test]
fn regex_layout_growth_keeps_all_fixed_and_added_names() {
    let _global = crate::gc::global_side_table_test_lock();
    crate::gc::initialize_runtime_fixture();
    unsafe {
        let arr = js_array_alloc_named_props_reserved(2, NamedPropertyKeys::ExecResultIndices);
        array_named_props_install(arr, &[3.0, 4.0, 5.0, 6.0]);
        let arr = set(arr, "extra", 7.0);
        let grown = js_array_grow(arr, 100);
        for (name, value) in [
            ("index", 3.0),
            ("input", 4.0),
            ("groups", 5.0),
            ("indices", 6.0),
            ("extra", 7.0),
        ] {
            assert_eq!(get(grown, name), Some(value));
            assert_eq!(
                get(arr, name),
                Some(value),
                "growth aliases retain one holder"
            );
        }
        assert!(!array_has_element_descriptors(grown));
    }
}

#[test]
fn shifting_a_regex_result_to_empty_preserves_its_named_layout() {
    let _global = crate::gc::global_side_table_test_lock();
    crate::gc::initialize_runtime_fixture();
    unsafe {
        let mut arr = js_array_alloc_named_props_reserved(2, NamedPropertyKeys::ExecResult);
        array_named_props_install(arr, &[3.0, 4.0, 5.0]);
        arr = js_array_push_f64(arr, 1.0);
        arr = js_array_push_f64(arr, 2.0);
        let capacity = (*arr).capacity;
        assert_eq!(js_array_shift_f64(arr), 1.0);
        assert_eq!(js_array_shift_f64(arr), 2.0);
        assert_eq!((*arr).length, 0);
        assert_eq!((*arr).capacity, capacity);
        for (name, value) in [("index", 3.0), ("input", 4.0), ("groups", 5.0)] {
            assert_eq!(get(arr, name), Some(value));
        }
        arr = js_array_push_f64(arr, 9.0);
        assert_elements(arr, &[9.0]);
    }
}

#[test]
fn length_truncation_and_growing_a_full_array_preserve_named_layout() {
    let _global = crate::gc::global_side_table_test_lock();
    crate::gc::initialize_runtime_fixture();
    unsafe {
        let arr = js_array_alloc_with_length_exact(2);
        store_array_slot(arr, 0, 1.0f64.to_bits());
        store_array_slot(arr, 1, 2.0f64.to_bits());
        let arr = set(arr, "tag", 7.0);
        let named = array_property_bag(arr);
        js_array_set_length(arr, 1.0);
        assert_eq!(get(arr, "tag"), Some(7.0));
        let arr = js_array_grow(arr, 100);
        assert_eq!(array_property_bag(arr), named);
        assert_eq!(get(arr, "tag"), Some(7.0));
        assert_elements(arr, &[1.0]);
    }
}

#[test]
fn named_layout_and_regex_results_keep_the_array_prototype() {
    let _global = crate::gc::global_side_table_test_lock();
    crate::gc::initialize_runtime_fixture();
    unsafe {
        for kind in [
            NamedPropertyKeys::ExecResult,
            NamedPropertyKeys::ExecResultIndices,
            NamedPropertyKeys::IndicesGroups,
        ] {
            let arr = js_array_alloc_named_props_reserved(1, kind);
            let values = vec![3.0; kind.keys().len()];
            array_named_props_install(arr, &values);
            let value = crate::value::js_nanbox_pointer(arr as i64);
            assert_eq!(
                crate::object::js_object_get_prototype_of(value).to_bits(),
                crate::object::builtin_prototype_value("Array").to_bits()
            );
        }
    }
}

/// Object model step 4: once an array's named holder has an overflow, the
/// overflow is its key-order authority and lists every own key, fixed or
/// added after. The element-kind fact reads that list: named keys leave the
/// elements plain, and an element descriptor recorded only in the overflow
/// (the holder's layout never names it) arms the fact.
#[test]
fn element_descriptor_fact_reads_the_named_holders_overflow() {
    let _global = crate::gc::global_side_table_test_lock();
    crate::gc::initialize_runtime_fixture();
    unsafe {
        let arr = set(pushed(&[1.0, 2.0, 3.0]), "pos", 1.0);
        let holder = (*arr).named;
        assert!(crate::object::overflow::establish_order(holder));
        let arr = set(arr, "end", 2.0);
        assert_eq!((*arr).named, holder);
        assert!(crate::object::overflow::has_overflow(holder));
        assert!(
            !array_has_element_descriptors(arr),
            "default-attribute named keys in the overflow leave the elements plain"
        );
        crate::object::set_property_attrs(
            arr as usize,
            "0".to_string(),
            crate::object::PropertyAttrs::new(false, true, true),
        );
        let record = crate::object::shapes::object_shape_record((*arr).named).unwrap();
        assert!(
            !named_props::keys_hold_element_descriptors(
                record.keys() as usize as *const ArrayHeader,
                record.logical_key_count(),
            ),
            "premise: the holder's layout does not name the descriptor"
        );
        assert!(
            array_has_element_descriptors(arr),
            "an element descriptor in the overflow arms the fact"
        );
        assert_elements(arr, &[1.0, 2.0, 3.0]);
    }
}
