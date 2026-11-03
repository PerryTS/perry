use super::*;
use crate::value::js_nanbox_pointer;

/// `Iterator.prototype` carries helper methods and spec accessors; none of
/// them is read by a step, so the builtin family step stays native until a
/// protocol method appears on the shared parent.
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
            !iterator_step_is_builtin(obj),
            "an inherited return requires protocol"
        );
        build_iterator_prototypes();
    }
}

/// Only keys past the birth count are scanned, so a delete must not let a
/// later protocol method slide into the intrinsic range unseen.
#[test]
fn a_protocol_method_added_after_deleting_an_intrinsic_key_still_revokes() {
    unsafe {
        let _stable = crate::gc::GcSuppressScope::new();
        build_iterator_prototypes();
        let array = crate::array::js_array_alloc(4);
        crate::array::js_array_push_f64(array, 1.0);
        let iter = crate::array::array_values_iter(js_nanbox_pointer(array as i64));
        let obj = crate::value::js_nanbox_get_pointer(iter) as *const ObjectHeader;
        let shared = ITERATOR_PROTOTYPE_PTR.load(Ordering::Acquire) as *mut ObjectHeader;
        assert!(iterator_step_is_builtin(obj), "pristine premise");

        super::super::js_object_delete_field(
            shared,
            crate::string::intern_ascii_literal(b"constructor"),
        );
        super::super::js_object_set_field_by_name(
            shared,
            crate::string::intern_ascii_literal(b"return"),
            1.0,
        );
        assert!(
            !iterator_step_is_builtin(obj),
            "a return that replaced a deleted key"
        );
        build_iterator_prototypes();
    }
}
