//! #10045: own callable properties must beat Function.prototype fast paths.
use crate::{closure, gc::RuntimeHandleScope, value};

extern "C" fn original(_closure: *const closure::ClosureHeader, _arg: f64) -> f64 {
    -1.0
}

extern "C" fn own_method(_closure: *const closure::ClosureHeader, _arg: f64) -> f64 {
    crate::object::js_implicit_this_get()
}

fn check_own_method_dispatch(proxy: bool) {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = RuntimeHandleScope::new();
    closure::js_register_closure_arity(original as *const u8, 1);
    closure::js_register_closure_arity(own_method as *const u8, 1);
    for method in ["bind", "call", "apply", "toString"] {
        let receiver = scope.root_nanbox_f64(value::js_nanbox_pointer(closure::js_closure_alloc(
            original as *const u8,
            0,
        ) as i64));
        let implementation = scope.root_nanbox_f64(value::js_nanbox_pointer(
            closure::js_closure_alloc(own_method as *const u8, 0) as i64,
        ));
        if proxy {
            let handler = scope.root_nanbox_f64(value::js_nanbox_pointer(
                crate::object::js_object_alloc(0, 0) as i64,
            ));
            implementation.set_nanbox_f64(crate::proxy::js_proxy_new(
                implementation.get_nanbox_f64(),
                handler.get_nanbox_f64(),
            ));
        }
        closure::closure_set_dynamic_prop(
            value::js_nanbox_get_pointer(receiver.get_nanbox_f64()) as usize,
            method,
            implementation.get_nanbox_f64(),
        );
        let args = [42.0];
        let result = unsafe {
            super::js_native_call_method(
                receiver.get_nanbox_f64(),
                method.as_ptr().cast(),
                method.len(),
                args.as_ptr(),
                args.len(),
            )
        };
        assert_eq!(
            result.to_bits(),
            receiver.get_nanbox_u64(),
            "own {method} receiver"
        );
    }
}

#[test]
fn closure_own_function_methods_override_intrinsic_dispatch() {
    check_own_method_dispatch(false);
}

#[test]
fn closure_own_function_methods_preserve_proxy_receiver() {
    check_own_method_dispatch(true);
}
