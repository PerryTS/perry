//! Regression coverage for #9371: a large `new Array(length)` keeps a small
//! initial backing store, but writes at the dense frontier must grow that
//! store instead of accumulating numeric keys in the named-property table.

use super::*;

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
