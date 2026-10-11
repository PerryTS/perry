//! Borrowed equality for boxed strings, including inline short strings.

use super::{js_string_equals, StringHeader};

/// The boxed counterpart of the legacy unified-pointer equality sequence.
/// String operands already own their bytes; comparing them never needs an
/// interned heap copy. Other values retain the unified helper's coercion and
/// invalid-pointer behavior. Only that fallback can allocate.
#[no_mangle]
pub extern "C" fn js_string_equals_value(a: f64, b: f64) -> i32 {
    let av = crate::value::JSValue::from_bits(a.to_bits());
    let bv = crate::value::JSValue::from_bits(b.to_bits());
    if av.is_any_string() && bv.is_any_string() {
        return crate::value::js_jsvalue_equals(a, b);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let left = scope.root_nanbox_f64(a);
    let right = scope.root_nanbox_f64(b);
    let left_ptr = scope.root_raw_mut_ptr(crate::value::js_get_string_pointer_unified(
        left.get_nanbox_f64(),
    ) as *mut StringHeader);
    let right_ptr =
        crate::value::js_get_string_pointer_unified(right.get_nanbox_f64()) as *const StringHeader;
    left_ptr.with_const_ptr(|left_ptr| js_string_equals(left_ptr, right_ptr))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_inline_and_heap_equality_does_not_publish_weak_interns() {
        let _guard = crate::gc::CopyingNurseryTestGuard::new(0);
        for text in ["", "/echo", "café", "😀"] {
            let heap = super::super::js_string_from_bytes(text.as_ptr(), text.len() as u32);
            let heap = crate::value::js_nanbox_string(heap as i64);
            let inline = f64::from_bits(
                crate::value::JSValue::try_short_string(text.as_bytes())
                    .unwrap()
                    .bits(),
            );
            super::super::discard_weak_interns();
            for _ in 0..100 {
                assert_eq!(js_string_equals_value(heap, inline), 1);
                assert_eq!(js_string_equals_value(inline, heap), 1);
            }
            assert!(super::super::intern_lookup_bytes(text.as_bytes()).is_none());
        }
    }

    #[test]
    fn boxed_equality_preserves_the_legacy_nonstring_fallback() {
        crate::gc::register_runtime_handle_root_scanner_for_tests();
        let scope = crate::gc::RuntimeHandleScope::new();
        let heap = scope.root_nanbox_f64(crate::value::js_nanbox_string(
            super::super::js_string_from_bytes(b"12345".as_ptr(), 5) as i64,
        ));
        assert_eq!(js_string_equals_value(heap.get_nanbox_f64(), 12345.0), 1);
        assert_eq!(js_string_equals_value(12345.0, heap.get_nanbox_f64()), 1);
        assert_eq!(js_string_equals_value(heap.get_nanbox_f64(), 12346.0), 0);
        let invalid = f64::from_bits(crate::value::TAG_UNDEFINED);
        assert_eq!(js_string_equals_value(invalid, invalid), 1);
        assert_eq!(js_string_equals_value(heap.get_nanbox_f64(), invalid), 0);
    }
}
