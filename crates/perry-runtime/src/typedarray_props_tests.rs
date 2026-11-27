use super::*;
use std::cell::Cell;

thread_local! {
    static VALUE_OF_CALLS: Cell<u32> = const { Cell::new(0) };
}

extern "C" fn counting_value_of(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    VALUE_OF_CALLS.with(|calls| calls.set(calls.get() + 1));
    3.0
}

#[test]
fn a_non_finite_number_key_store_still_converts_the_value() {
    // TypedArraySetElement runs ToNumber(value) before IsValidIntegerIndex:
    // `ta[NaN] = { valueOf }` calls valueOf once and stores nothing.
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let scope = crate::gc::RuntimeHandleScope::new();
        let ta = crate::typedarray::typed_array_alloc(crate::typedarray::KIND_INT16, 2);
        let value = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
        let value_of = crate::closure::js_closure_alloc(
            crate::fn_info!(counting_value_of, 0; with_declared(0)),
            0,
        );
        let key = crate::string::js_string_from_bytes(b"valueOf".as_ptr(), 7);
        value.with_mut_ptr::<crate::object::ObjectHeader, _>(|obj| {
            crate::object::js_object_set_field_by_name(
                obj,
                key,
                crate::value::js_nanbox_pointer(value_of as i64),
            )
        });
        for key in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1.5] {
            VALUE_OF_CALLS.with(|calls| calls.set(0));
            let boxed = value.with_const_ptr::<crate::object::ObjectHeader, _>(|obj| {
                crate::value::js_nanbox_pointer(obj as i64)
            });
            js_typed_array_index_set_dynamic(ta, key, boxed);
            assert_eq!(VALUE_OF_CALLS.with(Cell::get), 1, "key {key}");
        }
        assert_eq!(crate::typedarray::js_typed_array_get(ta, 0), 0.0);
        assert_eq!(crate::typedarray::js_typed_array_get(ta, 1), 0.0);
    }
}
