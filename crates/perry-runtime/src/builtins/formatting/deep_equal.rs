//! `util.isDeepStrictEqual` and its helpers, split from `formatting.rs`
//! for the 2000-line file cap.

use super::*;

pub(crate) fn formatted_deep_equal(left: f64, right: f64, skip_prototype: bool) -> bool {
    let _guard = DeepEqualSkipPrototypeFormatGuard::new(skip_prototype);
    format_jsvalue_for_json(left, 0) == format_jsvalue_for_json(right, 0)
}

pub(crate) fn js_util_deep_strict_equal_bool(
    left: f64,
    right: f64,
    depth: usize,
    skip_prototype: bool,
) -> bool {
    if depth > 64 {
        return formatted_deep_equal(left, right, skip_prototype);
    }
    let left_value = crate::value::JSValue::from_bits(left.to_bits());
    let right_value = crate::value::JSValue::from_bits(right.to_bits());
    let left_boxed = boxed_primitives::boxed_primitive_payload(left);
    let right_boxed = boxed_primitives::boxed_primitive_payload(right);
    if left_boxed.is_some() || right_boxed.is_some() {
        return match (left_boxed, right_boxed) {
            (Some((left_class, left_payload)), Some((right_class, right_payload)))
                if left_class == right_class =>
            {
                js_util_deep_strict_equal_bool(
                    left_payload,
                    right_payload,
                    depth + 1,
                    skip_prototype,
                )
            }
            _ => false,
        };
    }
    if let Some(equal) =
        collection_equality::deep_strict_collection_equal(left, right, depth, skip_prototype)
    {
        return equal;
    }
    if let Some(equal) = typed_array_equality::deep_strict_typed_array_equal(left, right) {
        return equal;
    }
    if identity_equality::is_identity_only_deep_equal_value(left)
        || identity_equality::is_identity_only_deep_equal_value(right)
    {
        return left.to_bits() == right.to_bits();
    }
    let has_tagged_heap_operand = left_value.is_pointer() || right_value.is_pointer();
    let has_raw_heap_operand =
        looks_like_raw_heap_pointer(left) || looks_like_raw_heap_pointer(right);
    if has_raw_heap_operand {
        false
    } else if has_tagged_heap_operand {
        // #2934: Node's default deepStrictEqual is prototype-sensitive — two
        // objects with the same own properties but different `[[Prototype]]`
        // are not equal. Gate before comparing the formatted body.
        if !skip_prototype && prototype_equality::prototypes_differ(left, right) {
            return false;
        }
        formatted_deep_equal(left, right, skip_prototype)
    } else {
        // A class REFERENCE is compared by identity, never by its rendering.
        // Since #9415 a class ref renders as `[class Name]`, so two DISTINCT
        // classes that happen to share a name format identically — the
        // formatted comparison would call them deep-equal, where node (and the
        // pre-#9415 class-id rendering, by accident) says they are not. The
        // probe runs only after `js_jsvalue_equals` has already answered "not
        // equal", so an ordinary integer that collides with a live class id is
        // unaffected: equal integers were settled one line above, and unequal
        // ones are unequal either way.
        crate::value::js_jsvalue_equals(left, right) != 0
            || (crate::object::class_ref_id(left).is_none()
                && crate::object::class_ref_id(right).is_none()
                && formatted_deep_equal(left, right, skip_prototype))
    }
}

#[no_mangle]
pub extern "C" fn js_util_is_deep_strict_equal(left: f64, right: f64) -> f64 {
    let equal = js_util_deep_strict_equal_bool(left, right, 0, false);
    f64::from_bits(crate::value::JSValue::bool(equal).bits())
}

pub fn js_util_is_deep_strict_equal_skip_prototype(left: f64, right: f64) -> f64 {
    let equal = js_util_deep_strict_equal_bool(left, right, 0, true);
    f64::from_bits(crate::value::JSValue::bool(equal).bits())
}
