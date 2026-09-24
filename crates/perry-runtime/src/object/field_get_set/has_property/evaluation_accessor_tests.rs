use super::*;
use crate::object::class_registry;
use std::cell::Cell;

thread_local! {
    static CALLS: Cell<u32> = const { Cell::new(0) };
}
extern "C" fn getter(_this: f64) -> f64 {
    CALLS.with(|calls| calls.set(calls.get() + 1));
    42.0
}
extern "C" fn setter(_this: f64, _value: f64) -> f64 {
    CALLS.with(|calls| calls.set(calls.get() + 1));
    0.0
}

#[test]
fn evaluated_class_accessors_are_present_on_the_actual_prototype_chain() {
    let _lock = crate::gc::global_side_table_test_lock();
    const EVALUATION_ACCESSOR_TEST_CLASS_ID: u32 = 0x1111_2001;
    unsafe {
        CALLS.set(0);
        class_registry::js_register_class_id(EVALUATION_ACCESSOR_TEST_CLASS_ID);
        class_registry::js_register_class_getter(
            EVALUATION_ACCESSOR_TEST_CLASS_ID as i64,
            b"getOnly".as_ptr(),
            7,
            getter as *const () as i64,
        );
        class_registry::js_register_class_setter(
            EVALUATION_ACCESSOR_TEST_CLASS_ID as i64,
            b"setOnly".as_ptr(),
            7,
            setter as *const () as i64,
        );
        let scope = crate::gc::RuntimeHandleScope::new();
        let class = scope.root_raw_mut_ptr(crate::object::js_object_alloc(
            EVALUATION_ACCESSOR_TEST_CLASS_ID,
            0,
        ));
        class.with_mut_ptr::<ObjectHeader, _>(|p| class_registry::js_object_mark_class(p as i64));
        let proto_value = class
            .with_mut_ptr::<ObjectHeader, _>(|p| super::super::class_object_prototype_value(p));
        let proto = scope.root_nanbox_f64(f64::from_bits(proto_value.bits()));
        let instance = scope.root_raw_mut_ptr(crate::object::js_object_alloc(
            EVALUATION_ACCESSOR_TEST_CLASS_ID,
            0,
        ));
        instance.with_mut_ptr::<ObjectHeader, _>(|p| {
            crate::object::prototype_chain::object_link_class_evaluation_prototype(
                p as usize,
                proto.get_nanbox_f64().to_bits(),
            );
        });
        let get =
            scope.root_string_ptr(crate::string::js_string_from_bytes(b"getOnly".as_ptr(), 7));
        let set =
            scope.root_string_ptr(crate::string::js_string_from_bytes(b"setOnly".as_ptr(), 7));
        let has = |value: f64, key: &crate::gc::RuntimeHandle| {
            let key_value = key.with_const_ptr::<crate::StringHeader, _>(|p| {
                crate::value::js_nanbox_string(p as i64)
            });
            crate::value::js_is_truthy(js_in_operator(value, key_value)) != 0
        };
        let instance_value = || {
            instance.with_mut_ptr::<ObjectHeader, _>(|p| crate::value::js_nanbox_pointer(p as i64))
        };
        assert!(
            has(instance_value(), &get),
            "capturing class getter must be present"
        );
        assert!(
            has(instance_value(), &set),
            "setter-only accessor must be present"
        );
        assert!(has(proto.get_nanbox_f64(), &get));
        assert_eq!(CALLS.get(), 0, "presence must not invoke accessors");
        class_registry::class_mark_key_deleted(EVALUATION_ACCESSOR_TEST_CLASS_ID, "getOnly");
        assert!(
            !has(instance_value(), &get),
            "deleted accessor must stay absent"
        );
        class_registry::class_unmark_key_deleted(EVALUATION_ACCESSOR_TEST_CLASS_ID, "getOnly");
        crate::object::js_object_set_prototype_of(
            instance_value(),
            f64::from_bits(0x7FFC_0000_0000_0002),
        );
        assert!(
            !has(instance_value(), &get),
            "replacement prototype is authoritative"
        );
    }
}
