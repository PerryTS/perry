use super::*;

extern "C" fn test_closure_func(closure: *const ClosureHeader) -> f64 {
    unsafe {
        let captured = js_closure_get_capture_f64(closure, 0);
        captured * 2.0
    }
}

#[test]
fn test_closure_basic() {
    let closure = js_closure_alloc(test_closure_func as *const u8, 1);
    js_closure_set_capture_f64(closure, 0, 21.0);
    let result = js_closure_call0(closure);
    assert_eq!(result, 42.0);
}
