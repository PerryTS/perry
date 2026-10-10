use crate::closure::{ClosureHeader, JsThis};

extern "C" fn packed_push(_: *const ClosureHeader, this: JsThis, rest: f64) -> f64 {
    let args = super::global_this_rest_array_values(rest);
    crate::array::array_proto_mutator(this.as_f64(), "push", args.as_ptr(), args.len())
}

#[test]
fn variadic_push_removes_the_rest_allocation_with_a_packed_control() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _suppress = crate::gc::GcSuppressScope::new();
    unsafe {
        let array = crate::array::js_array_alloc(128);
        let this = JsThis::from_f64(crate::value::js_nanbox_pointer(array as i64));
        let native = crate::closure::js_closure_alloc(
            crate::fn_info!(native_args super::array_prototype_push_thunk, 1),
            0,
        );
        let packed =
            crate::closure::js_closure_alloc(crate::fn_info!(packed_push, 1; with_rest(0)), 0);
        let args = [3.0, 5.0];
        let call =
            |closure| crate::closure::js_closure_call_array(closure as i64, this, args.as_ptr(), 2);
        assert_eq!(call(native), 2.0);
        let before = crate::arena::arena_in_use_bytes();
        assert_eq!(call(packed), 4.0);
        assert!(
            crate::arena::arena_in_use_bytes() > before,
            "the packed control allocates"
        );
        let before = crate::arena::arena_in_use_bytes();
        for _ in 0..32 {
            call(native);
        }
        assert_eq!(
            crate::arena::arena_in_use_bytes(),
            before,
            "the native list needs no rest Array"
        );
        assert_eq!((*array).length, 68);
        let wide = [9.0; 32];
        assert_eq!(
            crate::closure::js_closure_call_array(native as i64, this, wide.as_ptr(), 32,),
            100.0
        );
        assert_eq!(crate::array::js_array_get_f64(array, 99), 9.0);
    }
}
