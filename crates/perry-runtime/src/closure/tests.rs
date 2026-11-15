use super::*;

extern "C" fn test_closure_func(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    let captured = js_closure_get_capture_f64(closure, 0);
    captured * 2.0
}

static TEST_CLOSURE_FUNC: JsFunctionInfo =
    JsFunctionInfo::of(test_closure_func as crate::codegen_abi::JsBody0<ClosureHeader>);

#[test]
fn test_closure_basic() {
    let closure = js_closure_alloc(&TEST_CLOSURE_FUNC, 1);
    js_closure_set_capture_f64(closure, 0, 21.0);
    let result = js_closure_call0(closure, crate::closure::plain_call_receiver());
    assert_eq!(result, 42.0);
}

#[test]
fn test_closure_capture_bits_roundtrip_tagged_values() {
    let captures = [
        crate::value::TAG_UNDEFINED,
        crate::value::TAG_NULL,
        crate::value::TAG_FALSE,
        crate::value::TAG_TRUE,
        f64::from(-17).to_bits(),
        crate::value::JSValue::try_short_string(b"cap")
            .unwrap()
            .bits(),
        (-0.0f64).to_bits(),
    ];
    let closure = js_closure_alloc(&TEST_CLOSURE_FUNC, captures.len() as u32);

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

/// A heap reference in a capture slot is a POINTER_TAG value: the collector
/// reads the slot as value bits, and the reader gets the address back.
#[test]
fn capture_ref_is_stored_tagged_and_read_back_as_the_address() {
    let target = js_closure_alloc(&TEST_CLOSURE_FUNC, 0);
    let closure = js_closure_alloc(&TEST_CLOSURE_FUNC, 5);
    closure_set_capture_ref(closure, 0, target as i64);
    let bits = js_closure_get_capture_bits(closure, 0);
    assert_eq!(bits & crate::value::TAG_MASK, crate::value::POINTER_TAG);
    assert_eq!(bits & crate::value::POINTER_MASK, target as u64);
    assert_eq!(closure_capture_ref(closure, 0), target as i64);
    assert!(crate::gc::layout_pointer_bearing_bits(bits));
    // The value the slot holds is the function object itself.
    assert_eq!(
        crate::value::js_nanbox_get_pointer(js_closure_get_capture_f64(closure, 0)),
        target as i64
    );

    // Null stays the zero word and reads back as null.
    closure_set_capture_ref(closure, 1, 0);
    assert_eq!(js_closure_get_capture_bits(closure, 1), 0);
    assert_eq!(closure_capture_ref(closure, 1), 0);

    // An address that already carries its tag is stored once-tagged.
    closure_set_capture_ref(
        closure,
        2,
        (crate::value::POINTER_TAG | target as u64) as i64,
    );
    assert_eq!(js_closure_get_capture_bits(closure, 2), bits);

    // The reader takes the payload of a string- or bigint-tagged word too,
    // and answers null for every value that is not a reference.
    js_closure_set_capture_bits(closure, 3, crate::value::STRING_TAG | target as u64);
    assert_eq!(closure_capture_ref(closure, 3), target as i64);
    for not_a_reference in [
        crate::value::TAG_UNDEFINED,
        crate::value::TAG_NULL,
        crate::value::TAG_TRUE,
        1.5f64.to_bits(),
        (-0.0f64).to_bits(),
    ] {
        js_closure_set_capture_bits(closure, 4, not_a_reference);
        assert_eq!(closure_capture_ref(closure, 4), 0, "{not_a_reference:#x}");
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
        &TEST_CLOSURE_FUNC,
        captures.len() as u32,
        captures.as_ptr(),
    );

    for (index, &bits) in captures.iter().enumerate() {
        assert_eq!(js_closure_get_capture_bits(closure, index as u32), bits);
    }
}
