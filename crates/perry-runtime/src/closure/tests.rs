use super::*;

extern "C" fn test_closure_func(closure: *const ClosureHeader) -> f64 {
    let captured = js_closure_get_capture_f64(closure, 0);
    captured * 2.0
}

#[test]
fn test_closure_basic() {
    let closure = js_closure_alloc(test_closure_func as *const u8, 1);
    js_closure_set_capture_f64(closure, 0, 21.0);
    let result = js_closure_call0(closure);
    assert_eq!(result, 42.0);
}

#[test]
fn test_closure_capture_bits_roundtrip_tagged_values() {
    let captures = [
        crate::value::TAG_UNDEFINED,
        crate::value::TAG_NULL,
        crate::value::TAG_FALSE,
        crate::value::TAG_TRUE,
        crate::value::JSValue::int32(-17).bits(),
        crate::value::JSValue::try_short_string(b"cap")
            .unwrap()
            .bits(),
        (-0.0f64).to_bits(),
    ];
    let closure = js_closure_alloc(test_closure_func as *const u8, captures.len() as u32);

    for (index, &bits) in captures.iter().enumerate() {
        js_closure_set_capture_bits(closure, index as u32, bits);
    }

    for (index, &bits) in captures.iter().enumerate() {
        assert_eq!(js_closure_get_capture_bits(closure, index as u32), bits);
        assert_eq!(
            js_closure_get_capture_f64(closure, index as u32).to_bits(),
            bits
        );
        assert_eq!(
            js_closure_get_capture_ptr(closure, index as u32) as u64,
            bits
        );
    }
}

#[test]
fn test_closure_alloc_with_captures_singleton_preserves_capture_bits() {
    test_clear_singleton_closure_caches();
    let captures = [
        crate::value::TAG_UNDEFINED,
        crate::value::TAG_FALSE,
        crate::value::JSValue::try_short_string(b"env")
            .unwrap()
            .bits(),
    ];

    let closure = js_closure_alloc_with_captures_singleton(
        test_closure_func as *const u8,
        captures.len() as u32,
        captures.as_ptr(),
    );

    for (index, &bits) in captures.iter().enumerate() {
        assert_eq!(js_closure_get_capture_bits(closure, index as u32), bits);
    }
}
