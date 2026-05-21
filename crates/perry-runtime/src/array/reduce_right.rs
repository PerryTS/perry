//! Array.prototype.reduceRight.
use super::*;
use crate::closure::{js_closure_call3, ClosureHeader};

/// `arr.reduceRight(callback, initial?)` — reduce from right to left
#[no_mangle]
pub extern "C" fn js_array_reduce_right(
    arr: *const ArrayHeader,
    callback: *const ClosureHeader,
    has_initial: i32,
    initial: f64,
) -> f64 {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return if has_initial != 0 { initial } else { f64::NAN };
    }
    unsafe {
        let length = (*arr).length as usize;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        if length == 0 {
            return if has_initial != 0 { initial } else { f64::NAN };
        }

        let (mut accumulator, start_idx) = if has_initial != 0 {
            (initial, length)
        } else {
            (*elements_ptr.add(length - 1), length - 1)
        };

        if start_idx > 0 {
            for i in (0..start_idx).rev() {
                let element = *elements_ptr.add(i);
                // Refs #488: pass index as 3rd arg to match spec
                // `(accumulator, currentValue, currentIndex, array)`.
                accumulator = js_closure_call3(callback, accumulator, element, i as f64);
            }
        }

        accumulator
    }
}
