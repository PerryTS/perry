//! Arrays seen through a Proxy: flattening, element reads and writes, and
//! the values iterator.

use super::*;

#[test]
fn flattenable_array_ptr_accepts_only_arrays_and_array_proxies() {
    let array = js_array_alloc(0);
    let array_value = boxed_pointer(array as *mut u8);
    assert_eq!(flattenable_array_ptr(array_value), array);

    let object = crate::object::js_object_alloc(0, 0);
    assert!(flattenable_array_ptr(boxed_pointer(object as *mut u8)).is_null());

    let closure = crate::closure::js_closure_alloc(ptr::null(), 0);
    assert!(flattenable_array_ptr(boxed_pointer(closure as *mut u8)).is_null());

    let handler = crate::object::js_object_alloc(0, 0);
    let proxy = crate::proxy::js_proxy_new(array_value, boxed_pointer(handler as *mut u8));
    assert_eq!(flattenable_array_ptr(proxy), array);

    let nested_proxy = crate::proxy::js_proxy_new(proxy, boxed_pointer(handler as *mut u8));
    assert_eq!(flattenable_array_ptr(nested_proxy), array);
}

/// #11875: a Proxy over an array, held in a `T[]`-annotated binding, reaches
/// the element-read helpers as a proxy id (masked by the typed callers, boxed
/// by the fallback). Each must answer through the proxy's `[[Get]]`; the masked
/// id used to be read as a heap header (SIGSEGV) and the boxed one answered
/// `undefined`.
#[test]
fn proxied_array_element_reads_answer_through_the_proxy() {
    let array = js_array_alloc(0);
    js_array_push_f64(array, 7.0);
    js_array_push_f64(array, 8.0);
    let array_value = boxed_pointer(array as *mut u8);
    let handler = crate::object::js_object_alloc(0, 0);
    let proxy = crate::proxy::js_proxy_new(array_value, boxed_pointer(handler as *mut u8));
    let boxed_id = proxy.to_bits();
    let masked_id = boxed_id & crate::value::POINTER_MASK;
    assert!(
        crate::value::addr_class::is_proxy_id_band(masked_id as usize),
        "premise: the proxy is a handle-band id"
    );
    for (index, want) in [(0u32, 7.0), (1u32, 8.0)] {
        assert_eq!(
            js_array_get_f64(masked_id as *const ArrayHeader, index),
            want,
            "masked proxy id, index {index}"
        );
        assert_eq!(
            js_array_get_f64(boxed_id as *const ArrayHeader, index),
            want,
            "boxed proxy id, index {index}"
        );
        assert_eq!(
            crate::typed_feedback::js_typed_feedback_array_index_get_fallback_boxed(
                0,
                proxy,
                index as f64
            ),
            want,
            "fallback read, index {index}"
        );
    }
}

/// #11891: every array element-store funnel may receive either the masked or
/// boxed id of a Proxy bound to a `T[]` parameter. They must invoke [[Set]];
/// an empty handler forwards each write to the array target.
#[test]
fn proxied_array_element_writes_answer_through_the_proxy() {
    let array = js_array_alloc(0);
    js_array_push_f64(array, 7.0);
    js_array_push_f64(array, 8.0);
    let array_value = boxed_pointer(array as *mut u8);
    let handler = crate::object::js_object_alloc(0, 0);
    let proxy = crate::proxy::js_proxy_new(array_value, boxed_pointer(handler as *mut u8));
    let boxed_id = proxy.to_bits();
    let masked_id = boxed_id & crate::value::POINTER_MASK;

    js_array_set_f64_extend(masked_id as *mut ArrayHeader, 0, 10.0);
    assert_eq!(js_array_get_f64(array, 0), 10.0);

    js_array_set_f64_extend_strict(boxed_id as *mut ArrayHeader, 1, 20.0);
    assert_eq!(js_array_get_f64(array, 1), 20.0);

    js_array_set_index_or_string_with_strictness(masked_id as *mut ArrayHeader, 0.0, 30.0, true);
    assert_eq!(js_array_get_f64(array, 0), 30.0);

    crate::typed_feedback::js_typed_feedback_array_index_set_fallback_boxed(0, proxy, 1.0, 40.0, 1);
    assert_eq!(js_array_get_f64(array, 1), 40.0);
}

#[test]
fn array_proxy_values_iterator_uses_live_trapped_reads() {
    let array = js_array_alloc(4);
    js_array_push_f64(array, 7.0);
    js_array_push_f64(array, 8.0);
    let array_value = boxed_pointer(array as *mut u8);
    let handler = crate::object::js_object_alloc(0, 0);
    let proxy = crate::proxy::js_proxy_new(array_value, boxed_pointer(handler as *mut u8));

    let scope = crate::gc::RuntimeHandleScope::new();
    let iter_h = scope.root_nanbox_f64(array_values_iter(proxy));
    let next = || unsafe {
        let iter = crate::value::js_nanbox_get_pointer(iter_h.get_nanbox_f64())
            as *mut crate::object::ObjectHeader;
        let result = dispatch_array_iterator_method(iter, "next");
        let result =
            crate::value::js_nanbox_get_pointer(result) as *const crate::object::ObjectHeader;
        (
            f64::from_bits(crate::object::js_object_get_field(result, 0).bits()),
            crate::object::js_object_get_field(result, 1).bits() == crate::value::TAG_TRUE,
        )
    };

    assert_eq!(next(), (7.0, false));
    js_array_set_f64(array, 1, 9.0);
    assert_eq!(next(), (9.0, false), "indexed Get must stay live");
    js_array_push_f64(array, 10.0);
    assert_eq!(next(), (10.0, false), "length Get must stay live");
    assert!(next().1);
}
