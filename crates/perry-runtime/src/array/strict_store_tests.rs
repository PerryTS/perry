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

        // #9220: an in-bounds hole is not an own property. The number lane
        // must decline it so the strict entry can consult an inherited index
        // setter / non-writable data descriptor before creating an element —
        // but ONLY once some array has been retargeted. With the process latch
        // clear (the overwhelmingly common case, including every `new Array(n)`
        // fill) the lane keeps filling holes exactly as it did before #9220.
        //
        // Indices 4 and 5 are the SAME shape — two in-bounds holes on one
        // array — so the latch is the only variable between the two arms.
        js_array_set_length(out, 6.0);
        assert!(!array_has_own_index(out, 4));
        assert!(!array_has_own_index(out, 5));
        let latch_was =
            crate::object::prototype_chain::test_swap_array_static_proto_recorded(false);
        assert!(
            lane(out, 4, 8.0),
            "no recorded array prototype: the hole fill stays on the fast lane"
        );
        assert!(array_has_own_index(out, 4));
        crate::object::prototype_chain::test_swap_array_static_proto_recorded(true);
        assert!(!lane(out, 5, 8.0), "hole slot requires the [[Set]] walk");
        assert!(!array_has_own_index(out, 5));
        crate::object::prototype_chain::test_swap_array_static_proto_recorded(latch_was);
    }
}

/// Whether `f` threw a runtime exception.
fn catch_runtime_throw(f: impl FnOnce()) -> bool {
    let env = crate::exception::js_try_push();
    let jumped = unsafe { crate::ffi::setjmp::setjmp(env as *mut std::os::raw::c_int) };
    if jumped == 0 {
        f();
        crate::exception::js_try_end();
        false
    } else {
        crate::exception::js_try_end();
        crate::exception::js_clear_exception();
        true
    }
}

/// #9394: a rejected element write throws only in STRICT code.
///
/// ES2024 §6.2.5.7 calls `Set(O, P, V, Throw)` with `Throw =
/// IsStrictReference`, so a sloppy `arr[i] = v` against a frozen /
/// non-extensible / non-writable target is a silent no-op — exactly as it is
/// for an ordinary object, and exactly as Node behaves.
///
/// BOTH arms are asserted here on purpose. #9326 shipped the throw-only half
/// with a 64-check differential and a 205-line gap fixture, all of it module
/// (strict) code, and every one of them stayed green while sloppy code — the
/// whole of a CommonJS bundle — started throwing.
#[test]
fn element_store_rejection_throws_only_in_strict_mode() {
    // SAFETY: plain array construction plus the public element setters; every
    // pointer below is a live head this test allocated.
    unsafe {
        let values = [1.0, 2.0, 3.0];

        let frozen = js_array_from_f64(values.as_ptr(), values.len() as u32);
        crate::object::js_object_freeze(crate::value::js_nanbox_pointer(frozen as i64));

        assert!(
            catch_runtime_throw(|| {
                js_array_set_f64_extend_strict(frozen, 0, 9.0);
            }),
            "strict: a frozen element is non-writable"
        );
        assert_eq!(js_array_get_f64(frozen, 0), 1.0);

        assert!(
            !catch_runtime_throw(|| {
                crate::array::js_array_set_f64_extend_sloppy(frozen, 0, 9.0);
            }),
            "sloppy: the same rejection is silent"
        );
        assert_eq!(js_array_get_f64(frozen, 0), 1.0);

        // A new index on a non-extensible array is the other rejection shape.
        assert!(
            catch_runtime_throw(|| {
                js_array_set_f64_extend_strict(frozen, 7, 9.0);
            }),
            "strict: a frozen array cannot gain an element"
        );
        assert!(
            !catch_runtime_throw(|| {
                crate::array::js_array_set_f64_extend_sloppy(frozen, 7, 9.0);
            }),
            "sloppy: the same rejection is silent"
        );
        assert_eq!((*frozen).length, 3);

        // A writable element is stored in both modes — the sloppy entry is a
        // no-op only where the strict one would have thrown.
        let open = js_array_from_f64(values.as_ptr(), values.len() as u32);
        let out = crate::array::js_array_set_f64_extend_sloppy(open, 0, 42.0);
        assert_eq!(js_array_get_f64(out, 0), 42.0);
        let out = crate::array::js_array_set_f64_extend_sloppy(out, 3, 4.0);
        assert_eq!((*out).length, 4);
        assert_eq!(js_array_get_f64(out, 3), 4.0);
    }
}
