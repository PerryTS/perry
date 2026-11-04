use super::*;
use crate::value::js_nanbox_pointer;

/// `Iterator.prototype` carries helper methods and spec accessors; none of
/// them is read by a step. A step reads only `next`, which the family
/// prototype shadows, and `return` belongs to IteratorClose, so the builtin
/// family step stays native whatever the shared parent gains.
/// `a_next_on_the_shared_parent_is_read_once_the_family_next_is_deleted`
/// shows the same read revoking when the shared parent's key is the one read.
#[test]
fn shared_parent_keeps_the_native_step_until_a_protocol_method_appears() {
    unsafe {
        let _stable = crate::gc::GcSuppressScope::new();
        build_iterator_prototypes();
        let array = crate::array::js_array_alloc(4);
        crate::array::js_array_push_f64(array, 1.0);
        let iter = crate::array::array_values_iter(js_nanbox_pointer(array as i64));
        let obj = crate::value::js_nanbox_get_pointer(iter) as *const ObjectHeader;
        let shared = ITERATOR_PROTOTYPE_PTR.load(Ordering::Acquire) as *mut ObjectHeader;
        assert!(
            iterator_step_is_builtin(obj),
            "the @@toStringTag accessor is not a protocol method"
        );

        super::super::js_object_set_field_by_name(
            shared,
            crate::string::intern_ascii_literal(b"map"),
            1.0,
        );
        assert!(
            iterator_step_is_builtin(obj),
            "a helper method is not a protocol method"
        );

        super::super::js_object_set_field_by_name(
            shared,
            crate::string::intern_ascii_literal(b"return"),
            1.0,
        );
        assert!(
            iterator_step_is_builtin(obj),
            "an inherited return is IteratorClose's, not the step's"
        );
        build_iterator_prototypes();
    }
}

/// The step reads `next` by an ordinary lookup: once the family prototype
/// loses its own `next`, the shared parent's `next` is the method the step
/// calls, and it is not the family intrinsic.
#[test]
fn a_next_on_the_shared_parent_is_read_once_the_family_next_is_deleted() {
    unsafe {
        let _stable = crate::gc::GcSuppressScope::new();
        build_iterator_prototypes();
        let array = crate::array::js_array_alloc(4);
        crate::array::js_array_push_f64(array, 1.0);
        let iter = crate::array::array_values_iter(js_nanbox_pointer(array as i64));
        let obj = crate::value::js_nanbox_get_pointer(iter) as *const ObjectHeader;
        let shared = ITERATOR_PROTOTYPE_PTR.load(Ordering::Acquire) as *mut ObjectHeader;
        assert!(iterator_step_is_builtin(obj), "pristine premise");

        let family = ARRAY_ITERATOR_PROTOTYPE_PTR.load(Ordering::Acquire) as *mut ObjectHeader;
        let next_key = crate::string::intern_ascii_literal(b"next");
        let other = super::super::js_object_alloc(0, 0);
        super::super::js_object_set_field_by_name(
            shared,
            next_key,
            js_nanbox_pointer(other as i64),
        );
        assert!(
            iterator_step_is_builtin(obj),
            "the family prototype's own next shadows the shared parent's"
        );
        super::super::js_object_delete_field(family, next_key);
        assert!(
            !iterator_step_is_builtin(obj),
            "a next inherited from the shared parent after the delete"
        );
        build_iterator_prototypes();
    }
}
