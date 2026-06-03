//! Array.prototype.reduceRight.
use super::*;
use crate::array::throw_reduce_of_empty;
use crate::closure::{js_closure_call4, ClosureHeader};

/// `arr.reduceRight(callback, initial?)` — reduce from right to left
#[no_mangle]
pub extern "C" fn js_array_reduce_right(
    arr: *const ArrayHeader,
    callback: *const ClosureHeader,
    has_initial: i32,
    initial: f64,
) -> f64 {
    let arr = normalize_array_receiver(arr);
    if arr.is_null() {
        if has_initial != 0 {
            return initial;
        }
        throw_reduce_of_empty();
    }
    // Typed-array receiver: read elements per element-kind. Issue #2799.
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        return crate::typedarray::js_typed_array_reduce_right(
            arr as *const crate::typedarray::TypedArrayHeader,
            callback,
            has_initial,
            initial,
        );
    }
    unsafe {
        let length = (*arr).length as usize;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        if length == 0 {
            if has_initial != 0 {
                return initial;
            }
            // Per spec (ES2015 §22.1.3.19): empty array with no initial value
            // throws `TypeError: Reduce of empty array with no initial value`.
            throw_reduce_of_empty();
        }

        let (mut accumulator, start_idx) = if has_initial != 0 {
            (initial, length)
        } else {
            (*elements_ptr.add(length - 1), length - 1)
        };

        let arr_value = f64::from_bits(crate::value::JSValue::pointer(arr as *const u8).bits());
        if start_idx > 0 {
            for i in (0..start_idx).rev() {
                let element = *elements_ptr.add(i);
                // Spec callback `(accumulator, currentValue, currentIndex, array)`.
                accumulator = js_closure_call4(callback, accumulator, element, i as f64, arr_value);
            }
        }

        accumulator
    }
}
