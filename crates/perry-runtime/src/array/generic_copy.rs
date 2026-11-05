//! The spec-generic bodies of the `Array.prototype` methods that build a new
//! array from an arbitrary array-like receiver: `flatMap`, `toReversed`,
//! `toSorted` and `with`.
//!
//! The dense-array bodies (`js_array_flatMap`, `js_array_to_reversed`, ...)
//! read an `ArrayHeader`'s elements directly. A receiver that is not an Array
//! (a Proxy over one, as a Solid store hands out, or a plain `{ length, 0: ... }`
//! object reached through `Array.prototype.flatMap.call(obj, ...)`) goes
//! through these instead: `LengthOfArrayLike` and every element read are the
//! receiver's own `[[Get]]` / `[[HasProperty]]` (`al_length` / `al_get` /
//! `al_has`), so Proxy traps fire in the order the specification gives.
#![allow(non_snake_case)]

use super::generic::{al_get, al_has, al_length, nanbox_arr, to_object};
use super::*;

/// Append `value` to the rooted result array, keeping the root current.
fn push_rooted(result: &crate::gc::RuntimeHandle<'_>, value: f64) {
    let grown = result.with_mut_ptr(|array| js_array_push_f64(array, value));
    result.set_raw_mut_ptr(grown);
}

/// `Array.prototype.toReversed()` over an array-like receiver
/// (§23.1.3.33): `A[k] = Get(O, len - 1 - k)`, holes read as `undefined`.
#[no_mangle]
pub extern "C" fn js_arraylike_toReversed(recv: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let recv_h = scope.root_nanbox_f64(to_object(recv));
    let len = al_length(recv_h.get_nanbox_f64());
    if len > u32::MAX as i64 {
        crate::array::array_length_range_error();
    }
    let result = scope.root_raw_mut_ptr(js_array_alloc(len as u32));
    for k in 0..len {
        let v = al_get(recv_h.get_nanbox_f64(), len - 1 - k);
        push_rooted(&result, v);
    }
    result.with_mut_ptr::<ArrayHeader, _>(nanbox_arr)
}

/// `Array.prototype.toSorted(comparefn)` over an array-like receiver
/// (§23.1.3.34): validate the comparator, read every element with `Get`
/// (holes become `undefined`), then sort that list exactly as the dense body
/// sorts its copy.
#[no_mangle]
pub extern "C" fn js_arraylike_toSorted(recv: f64, comparator: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let cmp_h = scope.root_nanbox_f64(comparator);
    // The comparator check precedes ToObject(this) for toSorted.
    let has_cmp = crate::array::js_validate_array_comparator(cmp_h.get_nanbox_f64()) != 0;
    let recv_h = scope.root_nanbox_f64(to_object(recv));
    let len = al_length(recv_h.get_nanbox_f64());
    if len > u32::MAX as i64 {
        crate::array::array_length_range_error();
    }
    let items = scope.root_raw_mut_ptr(js_array_alloc(len as u32));
    for k in 0..len {
        let v = al_get(recv_h.get_nanbox_f64(), k);
        push_rooted(&items, v);
    }
    let sorted = if has_cmp {
        let cmp = crate::value::JSValue::from_bits(cmp_h.get_nanbox_u64())
            .as_pointer::<crate::closure::ClosureHeader>();
        items.with_mut_ptr(|array| js_array_to_sorted_with_comparator(array, cmp))
    } else {
        items.with_mut_ptr(|array| js_array_to_sorted_default(array))
    };
    nanbox_arr(sorted)
}

/// `Array.prototype.with(index, value)` over an array-like receiver
/// (§23.1.3.39): the replaced index is not read; every other index is `Get`.
#[no_mangle]
pub extern "C" fn js_arraylike_with(recv: f64, index: f64, value: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_h = scope.root_nanbox_f64(value);
    let recv_h = scope.root_nanbox_f64(to_object(recv));
    let len = al_length(recv_h.get_nanbox_f64());
    // ToIntegerOrInfinity(index), relative to len.
    let rel = crate::builtins::js_number_coerce(index);
    let rel = if rel.is_nan() { 0.0 } else { rel.trunc() };
    let actual = if rel < 0.0 { len as f64 + rel } else { rel };
    if !actual.is_finite() || actual < 0.0 || actual >= len as f64 {
        super::immutable::throw_invalid_index(rel);
    }
    if len > u32::MAX as i64 {
        crate::array::array_length_range_error();
    }
    let actual = actual as i64;
    let result = scope.root_raw_mut_ptr(js_array_alloc(len as u32));
    for k in 0..len {
        let v = if k == actual {
            value_h.get_nanbox_f64()
        } else {
            al_get(recv_h.get_nanbox_f64(), k)
        };
        push_rooted(&result, v);
    }
    result.with_mut_ptr::<ArrayHeader, _>(nanbox_arr)
}

/// A holey Array copy of a Proxy receiver: `LengthOfArrayLike`, then for each
/// index `HasProperty` (absent stays a hole) and `Get`, all through the
/// Proxy's traps. `js_array_from_arraylike_holey_value` uses it for the
/// receiver kinds it cannot read as an object (`Array.prototype.flat` on a
/// Proxy over an Array).
pub(super) fn proxy_holey_snapshot(recv: f64) -> *mut ArrayHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let recv_h = scope.root_nanbox_f64(recv);
    let len = al_length(recv_h.get_nanbox_f64());
    if len > u32::MAX as i64 {
        crate::array::array_length_range_error();
    }
    let mut current = js_array_alloc_with_length(len as u32);
    let result = scope.root_raw_mut_ptr(current);
    for k in 0..len {
        // Both proxy operations may collect, including a missing property.
        // Refresh the result after them before touching its dense slots.
        let (value, array) = result.across_mut::<ArrayHeader, _>(|| {
            al_has(recv_h.get_nanbox_f64(), k).then(|| al_get(recv_h.get_nanbox_f64(), k))
        });
        current = array;
        let Some(v) = value else {
            continue;
        };
        unsafe {
            let elems = crate::array::array_elements_ptr(array as *const ArrayHeader) as *mut f64;
            // GC_STORE_AUDIT(BARRIERED): note_array_slot below re-stores this slot with the barrier.
            std::ptr::write(elems.add(k as usize), v);
            note_array_slot(array, k as usize, v.to_bits());
        }
    }
    current
}
