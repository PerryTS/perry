use super::*;
use crate::closure::{ClosureHeader, JsFunctionInfo, JsThis};

extern "C" fn encode(
    _closure: *const ClosureHeader,
    this: JsThis,
    value: f64,
    index: f64,
    receiver: f64,
) -> f64 {
    assert!(crate::value::JSValue::from_bits(receiver.to_bits()).is_pointer());
    f64::from_bits(this.bits()) + value * 10.0 + index
}

static ENCODE: JsFunctionInfo =
    JsFunctionInfo::of(encode as crate::codegen_abi::JsBody3<ClosureHeader>)
        .with_flags(crate::closure::FN_STRICT);

extern "C" fn sum(
    _closure: *const ClosureHeader,
    _this: JsThis,
    accumulator: f64,
    value: f64,
    index: f64,
    _receiver: f64,
) -> f64 {
    accumulator + value + index
}

static SUM: JsFunctionInfo = JsFunctionInfo::of(sum as crate::codegen_abi::JsBody4<ClosureHeader>);

#[test]
fn generic_map_passes_this_index_and_original_receiver() {
    let values = [1.0, 2.0, 3.0];
    let arr = js_array_from_f64(values.as_ptr(), values.len() as u32);
    let callback = crate::closure::js_closure_alloc(&ENCODE, 0);
    assert!(crate::closure::DirectCall3::resolve(callback).is_direct());
    let result = js_arraylike_map(
        crate::value::js_nanbox_pointer(arr as i64),
        crate::value::js_nanbox_pointer(callback as i64),
        100.0,
    );
    let result = crate::value::js_nanbox_get_pointer(result) as *const ArrayHeader;
    assert_eq!(js_array_length(result), 3);
    assert_eq!(js_array_get_f64(result, 0), 110.0);
    assert_eq!(js_array_get_f64(result, 1), 121.0);
    assert_eq!(js_array_get_f64(result, 2), 132.0);
}

#[test]
fn generic_reduce_and_reduce_right_use_four_argument_callback() {
    let values = [1.0, 2.0, 3.0];
    let arr = js_array_from_f64(values.as_ptr(), values.len() as u32);
    let callback = crate::closure::js_closure_alloc(&SUM, 0);
    assert!(crate::closure::DirectCall4::resolve(callback).is_direct());
    let receiver = crate::value::js_nanbox_pointer(arr as i64);
    let callback = crate::value::js_nanbox_pointer(callback as i64);
    assert_eq!(js_arraylike_reduce(receiver, callback, 1, 10.0), 19.0);
    assert_eq!(js_arraylike_reduceRight(receiver, callback, 1, 10.0), 19.0);
}

#[test]
fn generic_map_preserves_bound_callback_dispatch() {
    let values = [1.0, 2.0];
    let arr = js_array_from_f64(values.as_ptr(), values.len() as u32);
    let callback = crate::closure::js_closure_alloc(&ENCODE, 0);
    let args = [50.0];
    let bound = unsafe {
        crate::closure::js_function_bind(
            crate::value::js_nanbox_pointer(callback as i64),
            args.as_ptr(),
            args.len(),
        )
    };
    let bound_ptr = crate::value::js_nanbox_get_pointer(bound) as *const ClosureHeader;
    assert!(!crate::closure::DirectCall3::resolve(bound_ptr).is_direct());
    let result = js_arraylike_map(crate::value::js_nanbox_pointer(arr as i64), bound, 999.0);
    let result = crate::value::js_nanbox_get_pointer(result) as *const ArrayHeader;
    assert_eq!(js_array_get_f64(result, 0), 60.0);
    assert_eq!(js_array_get_f64(result, 1), 71.0);
}
