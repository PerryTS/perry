//! Regression coverage for #9371: a large `new Array(length)` keeps a small
//! initial backing store, but writes at the dense frontier must grow that
//! store instead of accumulating numeric keys in the named-property table.

use super::*;

// #9201: named properties live in a side table keyed by the array allocation.
// A grow replaces that allocation, so both the values and the owner key must
// move before the old header becomes a forwarding stub.
#[test]
fn growth_rekeys_named_property_owner() {
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        const NAME: &str = "__perry_test_9201_expando__";
        let key = crate::string::js_string_from_bytes(NAME.as_ptr(), NAME.len() as u32);
        let mut arr = js_array_alloc(3);
        arr = js_array_push_f64(arr, 1.0);
        arr = js_array_push_f64(arr, 2.0);
        arr = js_array_push_f64(arr, 3.0);

        array_named_property_set(arr, key, 42.0);
        let old_owner = arr as usize;
        let old_capacity = (*arr).capacity;
        assert_eq!(
            array_named_property_get_by_name(arr, NAME),
            Some(42.0),
            "the expando must exist before growth"
        );

        arr = js_array_set_f64_extend(arr, old_capacity, 99.0);

        assert_ne!(arr as usize, old_owner, "the fixture must grow the array");
        assert_eq!(
            array_named_property_get_by_name(arr, NAME),
            Some(42.0),
            "#9201: growth must preserve the named property"
        );
        assert!(test_array_named_property_owner_exists(arr as usize));
        assert!(
            !test_array_named_property_owner_exists(old_owner),
            "the side table must no longer be keyed by the forwarding stub"
        );
    }
}

#[test]
fn large_presized_array_grows_its_dense_frontier() {
    unsafe {
        const LENGTH: u32 = 1_200_000;
        let mut arr = js_array_constructor_single(LENGTH as f64);
        assert_eq!((*arr).length, LENGTH);
        assert_eq!((*arr).capacity, MIN_ARRAY_CAPACITY);

        for index in 0..LENGTH {
            let old_capacity = (*arr).capacity;
            arr = js_array_set_f64_extend(arr, index, index as f64 + 0.25);
            if (*arr).capacity != old_capacity {
                assert_eq!(
                    js_array_get_f64(arr, 0),
                    0.25,
                    "growth at index {index} lost the first value (capacity {old_capacity} -> {})",
                    (*arr).capacity
                );
                assert_eq!(
                    js_array_get_f64(arr, index),
                    index as f64 + 0.25,
                    "growth at index {index} lost the current value"
                );
            }
        }

        assert_eq!((*arr).length, LENGTH);
        assert!(
            (*arr).capacity >= LENGTH,
            "sequential in-bounds writes must grow dense storage"
        );
        for index in 0..LENGTH {
            assert_eq!(js_array_get_f64(arr, index), index as f64 + 0.25);
        }
    }
}

#[test]
fn existing_sparse_indices_prevent_dense_growth_from_hiding_them() {
    unsafe {
        let mut arr = js_array_constructor_single(1_000_001.0);
        arr = js_array_set_f64_extend(arr, 500_000, 7.0);
        let capacity = (*arr).capacity;
        assert!(500_000 >= capacity, "fixture must take sparse storage");

        arr = js_array_set_f64_extend(arr, capacity, 9.0);

        assert_eq!(
            (*arr).capacity,
            capacity,
            "growth must not cover an existing sparse numeric property"
        );
        assert_eq!(js_array_get_f64(arr, capacity), 9.0);
        assert_eq!(js_array_get_f64(arr, 500_000), 7.0);
    }
}
