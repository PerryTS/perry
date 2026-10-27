//! Pure numeric sort operation. The certificate is supplied only by the HIR
//! effect proof; the receiver and key domain are proved here from live data.
use super::*;
use crate::closure::ClosureHeader;

/// Sort an admitted dense ordinary numeric array by a proven pure ordering.
/// This first domain is exact i32 values (excluding -0). Other numbers and
/// every exotic receiver keep the existing comparator protocol unchanged.
#[no_mangle]
pub extern "C" fn js_array_sort_numeric_order(
    arr: *mut ArrayHeader,
    comparator: *const ClosureHeader,
    order: i32,
) -> *mut ArrayHeader {
    unsafe {
        if order == 1 || order == -1 {
            // Admission must not materialize lazy JSON arrays (that can run
            // JS and collect). This resolver only admits tracked real arrays
            // and follows their growth/GC forwarding without allocating.
            let live = clean_owned_arr_ptr(arr) as *mut ArrayHeader;
            if !live.is_null() && try_sort_numeric(live, order == -1) {
                return live;
            }
        }
    }
    js_array_sort_with_comparator(arr, comparator)
}

/// No JS call, GC allocation or safepoint occurs anywhere in the admitted
/// operation. Rust-owned key workspaces contain no managed pointers; neither
/// a root nor a remembered-edge/layout rebuild is needed for the permutation.
unsafe fn try_sort_numeric(arr: *mut ArrayHeader, descending: bool) -> bool {
    let flags = array_object_flags_resolved(arr);
    let rejected = crate::gc::OBJ_FLAG_FROZEN
        | crate::gc::OBJ_FLAG_SEALED
        | crate::gc::OBJ_FLAG_NO_EXTEND
        | crate::gc::GC_ARRAY_CUSTOM_PROTO
        | crate::gc::GC_ARRAY_ARGUMENTS_OBJECT;
    let n = (*arr).length as usize;
    if flags & rejected != 0
        || !(*arr).named.is_null()
        || (*arr).length > (*arr).capacity
        || (*arr).ek_prefix < (*arr).length
    {
        return false;
    }
    let elements = std::slice::from_raw_parts_mut(array_elements_ptr(arr) as *mut f64, n);
    if n <= 1 {
        return true;
    }
    let flip = if descending { u32::MAX } else { 0 };
    let mut keys = Vec::with_capacity(n);
    let mut counts = [[0usize; 256]; 4];
    for &slot in elements.iter() {
        // The dense-prefix/ordinary-shape proof admits plain own slots;
        // their values still need a proof. A producer's raw-f64 brand is
        // sufficient but unnecessary: this scan also admits numeric slots
        // born without that brand, including boxed primitive integers.
        let Some(value) = crate::array::header::value_bits_to_number(slot.to_bits()) else {
            return false;
        };
        let integer = value as i32;
        // Includes NaN, Infinity, fractions, out-of-range integers and -0.
        // Refuse before ANY receiver write, preserving fallback effects and
        // the comparator's non-total NaN ordering exactly as before.
        if value != integer as f64 || value.to_bits() == (-0.0f64).to_bits() {
            return false;
        }
        let key = (integer as u32 ^ 0x8000_0000) ^ flip;
        keys.push(key);
        for (digit, count) in counts.iter_mut().enumerate() {
            count[((key >> (digit * 8)) & 255) as usize] += 1;
        }
    }
    radix_keys(&mut keys, &mut counts);
    for (slot, key) in elements.iter_mut().zip(keys) {
        // GC_STORE_AUDIT(POINTER_FREE): every old and new live slot is a
        // proved primitive number. Numeric/non-pointer (or conservative)
        // slot classifications, representation flags and prefix stay sound.
        *slot = ((key ^ flip ^ 0x8000_0000) as i32) as f64;
    }
    true
}

/// Stable LSD radix distribution; bytes shared by every key need no pass.
/// The fixed byte digits are a representation choice, with no size knobs,
/// thresholds, caches or callback-specific generated native algorithms.
fn radix_keys(keys: &mut Vec<u32>, counts: &mut [[usize; 256]; 4]) {
    let n = keys.len();
    let mut scratch = vec![0u32; n];
    for (digit, count) in counts.iter_mut().enumerate() {
        if count.iter().any(|&c| c == n) {
            continue;
        }
        let mut at = 0;
        for c in count.iter_mut() {
            let size = *c;
            *c = at;
            at += size;
        }
        for &key in keys.iter() {
            let bucket = ((key >> (digit * 8)) & 255) as usize;
            scratch[count[bucket]] = key;
            count[bucket] += 1;
        }
        std::mem::swap(keys, &mut scratch);
    }
}

#[cfg(test)]
#[path = "sort_numeric_tests.rs"]
mod tests;
