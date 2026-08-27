//! Dense numeric-window `arr[i] = arr[i] + delta` kernel
//! (`js_array_numeric_range_add` / `js_array_numeric_range_add_len`), split
//! from `indexing.rs` for the file-size gate (#8872). The transactional
//! validate-then-mutate contract is unchanged.

use super::header::value_bits_to_number;
use super::*;
use std::ptr;

/// Try to perform `arr[i] = arr[i] + delta` over a dense numeric window.
///
/// This is intentionally transactional: the first pass validates the actual
/// runtime receiver and every source slot, and only then does the second pass
/// mutate. Returning `-1` means "run the ordinary JS loop"; no slot has been
/// changed in that case. A non-negative return is the counter value the source
/// loop would have on exit.
fn array_numeric_range_add_impl(receiver: f64, start: f64, end: Option<f64>, delta: f64) -> i64 {
    let receiver_value = crate::value::JSValue::from_bits(receiver.to_bits());
    if !receiver_value.is_pointer() {
        return -1;
    }
    let raw = receiver_value.as_pointer::<ArrayHeader>() as usize;
    let Some(header) = (unsafe { crate::value::addr_class::try_read_gc_header(raw) }) else {
        return -1;
    };
    if header.obj_type != crate::gc::GC_TYPE_ARRAY {
        return -1;
    }
    let arr = clean_arr_ptr_mut(raw as *mut ArrayHeader);
    if arr.is_null() {
        return -1;
    }

    let Some(start_number) = value_bits_to_number(start.to_bits()) else {
        return -1;
    };
    if !start_number.is_finite()
        || start_number.fract() != 0.0
        || !(0.0..=i32::MAX as f64).contains(&start_number)
    {
        return -1;
    }
    let start = start_number as u32;

    let end = match end {
        Some(end) => {
            let Some(end_number) = value_bits_to_number(end.to_bits()) else {
                return -1;
            };
            if !end_number.is_finite()
                || end_number.fract() != 0.0
                || !(0.0..=i32::MAX as f64).contains(&end_number)
            {
                return -1;
            }
            end_number as u32
        }
        None => unsafe { (*arr).length },
    };
    let flags = array_object_flags(arr);
    if flags & (crate::gc::OBJ_FLAG_FROZEN | crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS) != 0 {
        return -1;
    }

    unsafe {
        if end > (*arr).length || end > (*arr).capacity {
            return -1;
        }
        if start >= end {
            return i64::from(start);
        }
        let elements = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut u64;
        for index in start..end {
            if value_bits_to_number(ptr::read(elements.add(index as usize))).is_none() {
                return -1;
            }
        }
        for index in start..end {
            let slot = elements.add(index as usize);
            let number = value_bits_to_number(ptr::read(slot))
                .expect("numeric range was validated before mutation");
            // GC_STORE_AUDIT(POINTER_FREE): both operands were proven numeric,
            // so the replacement is an unboxed IEEE-754 value.
            ptr::write(slot, (number + delta).to_bits());
        }
    }
    i64::from(end)
}

#[no_mangle]
pub extern "C" fn js_array_numeric_range_add(
    receiver: f64,
    start: f64,
    end: f64,
    delta: f64,
) -> i64 {
    array_numeric_range_add_impl(receiver, start, Some(end), delta)
}

#[no_mangle]
pub extern "C" fn js_array_numeric_range_add_len(receiver: f64, start: f64, delta: f64) -> i64 {
    array_numeric_range_add_impl(receiver, start, None, delta)
}

#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_ARRAY_NUMERIC_RANGE_ADD: extern "C" fn(f64, f64, f64, f64) -> i64 =
    js_array_numeric_range_add;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_ARRAY_NUMERIC_RANGE_ADD_LEN: extern "C" fn(f64, f64, f64) -> i64 =
    js_array_numeric_range_add_len;
