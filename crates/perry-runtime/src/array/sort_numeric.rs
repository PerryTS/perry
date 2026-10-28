//! Conditional numeric ordering, proved from live values before any write.
use super::*;
use crate::closure::ClosureHeader;

#[path = "sort_keyed.rs"]
mod keyed;
#[path = "sort_radix.rs"]
mod radix;
pub use keyed::js_array_sort_key_order;

#[no_mangle]
pub extern "C" fn js_array_sort_numeric_order(
    arr: *mut ArrayHeader,
    comparator: *const ClosureHeader,
    order: i32,
) -> *mut ArrayHeader {
    unsafe {
        if order == 1 || order == -1 {
            let live = clean_owned_arr_ptr(arr) as *mut ArrayHeader;
            if !live.is_null() && try_sort_numeric(live, order == -1) {
                return live;
            }
        }
    }
    js_array_sort_with_comparator(arr, comparator)
}

/// No materialization, property access, user code or allocation in admission.
unsafe fn dense_writable(arr: *mut ArrayHeader) -> bool {
    let rejected = crate::gc::OBJ_FLAG_FROZEN
        | crate::gc::OBJ_FLAG_SEALED
        | crate::gc::OBJ_FLAG_NO_EXTEND
        | crate::gc::GC_ARRAY_CUSTOM_PROTO
        | crate::gc::GC_ARRAY_ARGUMENTS_OBJECT;
    array_object_flags_resolved(arr) & rejected == 0
        && (*arr).named.is_null()
        && (*arr).length <= (*arr).capacity
        && (*arr).ek_prefix >= (*arr).length
}

unsafe fn try_sort_numeric(arr: *mut ArrayHeader, descending: bool) -> bool {
    if !dense_writable(arr) {
        return false;
    }
    let n = (*arr).length as usize;
    if n <= 1 {
        return true;
    }
    let ptr = array_elements_ptr(arr) as *mut f64;
    let values = std::slice::from_raw_parts(ptr, n);
    let flip = if descending { u32::MAX } else { 0 };
    let mut counts = [[0usize; 256]; 4];
    // Actual finite exact-i32 values cannot be NaN-boxed pointers, integers
    // or class references. This proof needs no producer brand or tag lookup.
    let packed = values.iter().all(|&value| {
        let integer = value as i32;
        if value != integer as f64 || value.to_bits() == (-0.0f64).to_bits() {
            return false;
        }
        radix::histogram(
            &mut counts,
            u64::from((integer as u32 ^ 0x8000_0000) ^ flip),
        );
        true
    });
    if packed {
        // Only after complete admission: compress keys into the first half
        // of the receiver's 8*n-byte payload. Each store is below every
        // unread f64. Its other half supplies scratch: no key allocation.
        for i in 0..n {
            // The complete scan proved every unchanged slot is exact i32;
            // unchecked conversion consumes that proof without saturation.
            let integer = (*ptr.add(i)).to_int_unchecked::<i32>();
            let key = (integer as u32 ^ 0x8000_0000) ^ flip;
            // GC_STORE_AUDIT(POINTER_FREE): all original/final slots are numeric; this entire temporary representation has no safepoint.
            ptr.cast::<u32>().add(i).write(key);
        }
        {
            let words = std::slice::from_raw_parts_mut(ptr.cast::<u32>(), n * 2);
            let (keys, scratch) = words.split_at_mut(n);
            radix::radix(keys, scratch, &mut counts, u64::from);
        }
        // Expansion runs backwards so writes cannot clobber unread keys.
        for i in (0..n).rev() {
            let key = *ptr.cast::<u32>().add(i);
            // GC_STORE_AUDIT(POINTER_FREE): restore numeric slots before leaving the callback-free region; slot classifications/prefix remain sound.
            ptr.add(i).write(((key ^ flip ^ 0x8000_0000) as i32) as f64);
        }
        return true;
    }
    sort_f64(arr, descending)
}

/// Reversible total ordering for non-NaN primitive numbers, including
/// fractions, subnormals, infinities, wide integers and boxed primitive i32s.
/// Negative zero retains the original stable comparator protocol.
unsafe fn sort_f64(arr: *mut ArrayHeader, descending: bool) -> bool {
    let n = (*arr).length as usize;
    let ptr = array_elements_ptr(arr) as *mut u64;
    let mut counts = [[0usize; 256]; 8];
    for i in 0..n {
        let Some(value) = header::value_bits_to_number(*ptr.add(i)) else {
            return false;
        };
        if value.to_bits() == (-0.0f64).to_bits() {
            return false;
        }
        let Some(key) = radix::float_key(value, descending) else {
            return false;
        };
        radix::histogram(&mut counts, key);
    }
    // Allocate before changing representation. No allocation or safepoint
    // follows until every slot is restored to a primitive numeric value.
    let mut scratch = vec![0u64; n];
    let keys = std::slice::from_raw_parts_mut(ptr, n);
    for slot in keys.iter_mut() {
        let value = header::value_bits_to_number(*slot).unwrap();
        // GC_STORE_AUDIT(POINTER_FREE): validated primitive values, temporary ordered bits, no intervening safepoint.
        *slot = radix::float_key(value, descending).unwrap();
    }
    radix::radix(keys, &mut scratch, &mut counts, |key| key);
    for slot in keys {
        // GC_STORE_AUDIT(POINTER_FREE): the inverse mapping restores the exact admitted numeric value.
        *slot = radix::float_value(*slot, descending).to_bits();
    }
    true
}

#[cfg(test)]
#[path = "sort_numeric_tests.rs"]
mod tests;
