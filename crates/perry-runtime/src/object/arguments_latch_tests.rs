//! An arguments object is born on its final shape.

use super::*;

fn args_object_of(restricted: bool) -> *mut ObjectHeader {
    let arr = crate::array::js_array_alloc(2);
    let arr = crate::array::js_array_push_f64(arr, 1.0);
    let arr = crate::array::js_array_push_f64(arr, 2.0);
    let raw_args = crate::value::js_nanbox_pointer(arr as i64);
    let callee = f64::from_bits(crate::value::TAG_UNDEFINED);
    js_arguments_object_alloc(raw_args, callee, restricted as i32)
}

/// #10509: an arguments object is an exotic read receiver, and its shape says
/// so through the prototype identity alone. Marking it used to move every
/// object onto a PRIVATE shape lineage (a counter-unique semantic generation),
/// so each call minted ShapeIds from a 2^30 counter that parks at the end: a
/// program making enough arguments objects would fail-stop. One arity and
/// callee kind is one shape, whatever the number of objects.
#[test]
fn arguments_objects_of_one_arity_share_one_shape() {
    for restricted in [false, true] {
        let first = args_object_of(restricted);
        let shape = unsafe { crate::object::shapes::object_shape_stamp(first) };
        let before = crate::object::shapes::test_shape_id_counter();
        for _ in 0..64 {
            let next = args_object_of(restricted);
            assert_eq!(
                unsafe { crate::object::shapes::object_shape_stamp(next) },
                shape,
                "every arguments object of one arity and callee kind carries one ShapeId \
                 (restricted = {restricted})"
            );
        }
        assert_eq!(
            crate::object::shapes::test_shape_id_counter(),
            before,
            "allocating arguments objects must mint no ShapeId (restricted = {restricted})"
        );
    }
}

/// #10509: every strict arguments object holds the SAME restricted-`callee`
/// accessor pair (pairs are immutable and `callee` is non-configurable), stored
/// in its born-in-layout slot, and both halves are %ThrowTypeError%.
#[test]
fn restricted_callee_pair_is_shared() {
    let a = args_object_of(true);
    let b = args_object_of(true);
    let callee_slot = 1; // slot 0 `length`, slot 1 `callee`, then @@iterator and the elements
    let pa = crate::object::js_object_get_field(a, callee_slot).bits();
    let pb = crate::object::js_object_get_field(b, callee_slot).bits();
    assert_eq!(pa, pb, "strict arguments objects share one callee pair");
    let acc = unsafe { crate::object::accessor_pair::pair_of_value(pa) }
        .expect("the callee slot holds an accessor pair");
    assert_ne!(acc.get, 0, "the getter is %ThrowTypeError%");
    assert_eq!(
        acc.get, acc.set,
        "getter and setter are the same %ThrowTypeError%"
    );
}
