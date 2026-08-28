//! The strict element-store fast lane's unit tests, split out of `tests.rs`
//! to keep it under the repo's 2000-line cap. Pure move.

use super::*;

/// The pointer-overwrite lane answers exactly an in-range pointer-for-pointer
/// store on a plain array and declines every other shape: a non-pointer
/// value, a slot that does not already hold a pointer (a hole, a number),
/// an out-of-range index, a non-array receiver.
#[test]
fn strict_dense_pointer_overwrite_lane_matches_the_general_path() {
    use super::indexing::test_strict_dense_pointer_overwrite as lane;
    // The helpers below are already `unsafe fn`s called from an unsafe context
    // higher up, so this block is redundant and `-D unused-unsafe` rejects it.
    {
        let objects: Vec<f64> = (0..4)
            .map(|_| {
                let obj = crate::arena::arena_alloc_gc(40, 8, crate::gc::GC_TYPE_OBJECT);
                crate::value::js_nanbox_pointer(obj as i64)
            })
            .collect();
        let mut arr = js_array_alloc(4);
        for value in &objects[..3] {
            arr = js_array_push_f64(arr, *value);
        }
        let boxed = crate::value::js_nanbox_pointer(arr as i64).to_bits() as *mut ArrayHeader;

        assert!(
            lane(boxed, 1, objects[3]),
            "boxed receiver, pointer over pointer, in range"
        );
        assert_eq!(js_array_get_f64(arr, 1).to_bits(), objects[3].to_bits());
        assert!(lane(arr, 2, objects[0]), "raw receiver");
        assert_eq!(js_array_get_f64(arr, 2).to_bits(), objects[0].to_bits());

        assert!(!lane(arr, 0, 1.5), "a number is not this lane's");
        assert!(!lane(arr, 3, objects[0]), "index == length is an extension");
        assert!(!lane(std::ptr::null_mut(), 0, objects[0]), "null receiver");
        assert!(
            !lane(
                f64::from_bits(crate::value::TAG_UNDEFINED).to_bits() as *mut ArrayHeader,
                0,
                objects[0]
            ),
            "non-pointer receiver"
        );
        // A hole is not a pointer slot: the extension left one at index 3.
        js_array_set_length(arr, 5.0);
        assert!(!lane(arr, 3, objects[0]), "hole slot declines");
        assert_eq!(
            js_array_get_f64(arr, 0).to_bits(),
            objects[0].to_bits(),
            "declined stores leave slots alone"
        );

        // A number slot declines too (the value would change the layout claim).
        let mut nums = js_array_alloc(4);
        nums = js_array_push_f64(nums, 1.0);
        assert!(!lane(nums, 0, objects[1]), "number slot declines");
        assert_eq!(js_array_get_f64(nums, 0), 1.0);

        // The public strict entry answers the same shape through the lane and
        // still returns the live head.
        let out = js_array_set_f64_extend_strict(boxed, 0, objects[2]);
        assert_eq!(out, arr);
        assert_eq!(js_array_get_f64(arr, 0).to_bits(), objects[2].to_bits());
    }
}

/// The strict element-store fast lane (`try_strict_dense_number_store`) must
/// store exactly what the general path stores, for both the NaN-boxed
/// receiver codegen passes and a raw head, and must decline every shape it
/// cannot prove: out-of-range indices, tagged or NaN values.
#[test]
fn strict_dense_number_store_fast_lane_matches_the_general_path() {
    use super::indexing::test_strict_dense_number_store as lane;
    unsafe {
        let mut arr = js_array_alloc(4);
        for i in 0..3 {
            arr = js_array_push_f64(arr, i as f64);
        }
        let boxed = crate::value::js_nanbox_pointer(arr as i64).to_bits() as *mut ArrayHeader;

        assert!(
            lane(boxed, 1, 41.5),
            "boxed receiver, plain number, in range"
        );
        assert_eq!(js_array_get_f64(arr, 1), 41.5);
        assert!(lane(arr, 2, -7.0), "raw receiver");
        assert_eq!(js_array_get_f64(arr, 2), -7.0);

        // An INT32 box stores its canonical double on this raw-f64 layout.
        let boxed_int = f64::from_bits(crate::value::INT32_TAG | 12);
        assert!(lane(boxed, 1, boxed_int), "INT32 box is a number");
        assert_eq!(js_array_get_f64(arr, 1).to_bits(), 12.0f64.to_bits());
        assert!(!lane(arr, 3, 1.0), "index == length is an extension");
        assert!(
            !lane(arr, 0, f64::from_bits(crate::value::TAG_UNDEFINED)),
            "tagged value"
        );
        assert!(
            !lane(arr, 0, f64::NAN),
            "NaN keeps canonicalization on the general path"
        );
        assert!(!lane(std::ptr::null_mut(), 0, 1.0), "null receiver");
        assert!(
            !lane(
                f64::from_bits(crate::value::TAG_UNDEFINED).to_bits() as *mut ArrayHeader,
                0,
                1.0
            ),
            "non-pointer receiver"
        );
        assert_eq!(
            js_array_get_f64(arr, 0),
            0.0,
            "declined stores leave the slot alone"
        );
        assert_eq!((*arr).length, 3);

        // The public strict entry answers the same shape through the lane and
        // still returns the live head.
        let out = js_array_set_f64_extend_strict(boxed, 0, 9.0);
        assert_eq!(out, arr);
        assert_eq!(js_array_get_f64(arr, 0), 9.0);
        // …and extension still goes through the general path.
        let out = js_array_set_f64_extend_strict(boxed, 3, 3.0);
        assert_eq!((*out).length, 4);
        assert_eq!(js_array_get_f64(out, 3), 3.0);
    }
}
