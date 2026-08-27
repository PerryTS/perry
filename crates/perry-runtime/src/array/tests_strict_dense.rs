//! Unit tests — strict dense overwrite layouts (split from `tests.rs` for the
//! 2,000-line file gate; `use super::*` reaches the shared test helpers).

use super::*;

#[test]
fn strict_dense_overwrite_preserves_numeric_and_pointer_layouts() {
    let arr = js_array_alloc(4);
    js_array_push_f64(arr, 10.0);
    js_array_push_f64(arr, 20.0);
    assert_eq!(js_array_is_numeric_f64_layout(arr), 1);

    let tagged_i32 = f64::from_bits(crate::value::INT32_TAG | 33);
    assert_eq!(
        indexing::try_strict_dense_index_set(arr, 1, tagged_i32),
        Some(arr)
    );
    assert_eq!(js_array_get_f64(arr, 1), 33.0);
    assert_eq!(unsafe { raw_slot_bits(arr, 1) }, 33.0f64.to_bits());

    let class_id = 0x0074_8693;
    crate::object::js_register_class_parent(class_id, 0);
    unsafe { crate::object::js_register_class_id(class_id) };
    let class_ref = f64::from_bits(crate::value::INT32_TAG | u64::from(class_id));
    assert_eq!(
        indexing::try_strict_dense_index_set(arr, 1, class_ref),
        Some(arr),
        "a ClassRef may use the general barriered path but not the numeric path"
    );
    assert_eq!(js_array_get_f64(arr, 1).to_bits(), class_ref.to_bits());
    assert_eq!(js_array_is_numeric_f64_layout(arr), 0);

    let pointer_arr = js_array_alloc(2);
    let first = boxed_pointer(crate::object::js_object_alloc(0, 0).cast());
    let second = boxed_pointer(crate::object::js_object_alloc(0, 0).cast());
    js_array_push_f64(pointer_arr, first);
    assert_eq!(
        indexing::try_strict_dense_index_set(pointer_arr, 0, second),
        Some(pointer_arr)
    );
    assert_eq!(js_array_get_f64(pointer_arr, 0).to_bits(), second.to_bits());
    assert_eq!(
        crate::gc::test_layout_pointer_slot_count(pointer_arr as usize, 1),
        Some(1),
        "the resolved general path must retain the ordinary GC slot note"
    );

    let holes = js_array_alloc_with_length(2);
    assert_eq!(
        indexing::try_strict_dense_index_set(holes, 0, 1.0),
        None,
        "a hole may be intercepted by a prototype setter and is not an existing own slot"
    );

    let header = unsafe {
        (arr as *mut u8)
            .sub(crate::gc::GC_HEADER_SIZE)
            .cast::<crate::gc::GcHeader>()
    };
    unsafe { (*header)._reserved |= crate::gc::OBJ_FLAG_FROZEN };
    assert_eq!(
        indexing::try_strict_dense_index_set(arr, 1, 44.0),
        None,
        "the fast path must leave strict frozen-array throwing to the fallback"
    );
}
