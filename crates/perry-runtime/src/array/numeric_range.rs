//! Dense numeric-window `arr[i] = arr[i] + delta` kernel
//! (`js_array_numeric_range_add` / `js_array_numeric_range_add_len`), split
//! from `indexing.rs` for the file-size gate (#8872). The transactional
//! validate-then-mutate contract is unchanged.

use super::header::value_bits_to_number;
use super::*;
use std::ptr;

/// Try to perform `arr[i] = arr[i] + delta` over a dense numeric window.
///
/// Receiver-level validation happens up front; element mutation is a single
/// fused pass. Returns:
///   * `>= 0`  -- the whole window was numeric and is updated; the value is
///     the counter the source loop would have on exit.
///   * `-1`    -- receiver-level decline (wrong type, frozen, descriptors,
///     out-of-range window); NO slot has been changed.
///   * `<= -2` -- slots `[start, k)` were updated and slot `k = -ret - 2` is
///     not a plain number; the caller resumes the ordinary loop at `k`. This
///     replaced the old all-or-nothing two-pass contract: each element gets
///     exactly one `+ delta` either way, so a partial update plus resume is
///     observably identical and halves the memory traffic.
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
        // One fused pass instead of validate-then-mutate. The all-or-nothing
        // contract the two-pass version provided was stronger than the source
        // semantics require: each element gets exactly one `+ delta` either
        // way, so mutating up to the first non-number and letting the
        // ordinary loop RESUME there (return `-(index) - 2`) is observably
        // identical -- and halves the memory traffic, which on a 1MB window
        // was the whole cost. The double lane is checked first: after the
        // first call every slot holds a boxed double, so the int lane only
        // runs on freshly built integer arrays.
        for index in start..end {
            let slot = elements.add(index as usize);
            let bits = ptr::read(slot);
            let tag = bits >> 48;
            // Genuine boxed double: anything outside the NaN-box tag band.
            // This is `value_bits_to_number`'s double arm, fused inline.
            let number = if !(0x7FF9..=0x7FFF).contains(&tag) {
                super::header::canonical_raw_f64(f64::from_bits(bits))
            } else if let Some(number) = value_bits_to_number(bits) {
                // Int-tagged (minus registered class ids) via the shared
                // decoder, so the class-ref exclusion stays in one place.
                number
            } else {
                // Not a plain number: the ordinary loop takes over from here.
                // Slots [start, index) are already updated, which the caller's
                // resume contract accounts for.
                return -i64::from(index) - 2;
            };
            // GC_STORE_AUDIT(POINTER_FREE): the operand was proven numeric,
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

/// Strided constant fill: `for (let j = start; j < end; j += step) arr[j] = C`
/// where `C` is a compile-time non-pointer constant (boolean, null,
/// undefined, or a number), passed as its NaN-box bits.
///
/// The sieve idiom. Per-element, the source loop pays the inline dense-store
/// guard chain (~10 header/flag/bounds checks) for a receiver that cannot
/// change mid-loop; this validates the receiver once and stores in a tight
/// native loop.
///
/// Same result contract as `array_numeric_range_add_impl`:
///   * `>= 0`  -- whole strided window stored; value is the counter on exit
///     (the first `j >= end`).
///   * `-1`    -- receiver-level decline; nothing stored. Declines: non-array
///     receivers, frozen/sealed/descriptor arrays, raw-f64-layout arrays
///     (writing tag bits into unboxed-double storage would corrupt it),
///     an active incremental-mark phase (overwriting pointer slots then
///     needs the deletion barrier the generic store path emits), and
///     out-of-range windows.
///
/// There is no partial return: every slot in the window is overwritten with
/// the same constant, so there is no per-slot failure mode once the receiver
/// qualifies.
#[no_mangle]
pub unsafe extern "C" fn js_array_fill_range_strided_tagged(
    receiver: f64,
    start: f64,
    end: f64,
    step: f64,
    value_bits: u64,
) -> i64 {
    let receiver_value = crate::value::JSValue::from_bits(receiver.to_bits());
    if !receiver_value.is_pointer() {
        return -1;
    }
    let raw = receiver_value.as_pointer::<ArrayHeader>() as usize;
    let Some(header) = crate::value::addr_class::try_read_gc_header(raw) else {
        return -1;
    };
    if header.obj_type != crate::gc::GC_TYPE_ARRAY {
        return -1;
    }
    let arr = clean_arr_ptr_mut(raw as *mut ArrayHeader);
    if arr.is_null() {
        return -1;
    }

    let to_u32 = |v: f64| -> Option<u32> {
        let n = value_bits_to_number(v.to_bits())?;
        if !n.is_finite() || n.fract() != 0.0 || !(0.0..=i32::MAX as f64).contains(&n) {
            return None;
        }
        Some(n as u32)
    };
    let (Some(start), Some(end), Some(step)) = (to_u32(start), to_u32(end), to_u32(step)) else {
        return -1;
    };
    if step == 0 {
        return -1;
    }

    let flags = array_object_flags(arr);
    if flags
        & (crate::gc::OBJ_FLAG_FROZEN
            | crate::gc::OBJ_FLAG_SEALED
            | crate::gc::OBJ_FLAG_NO_EXTEND
            | crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS)
        != 0
    {
        return -1;
    }
    // Tag bits stored into raw-f64 storage would be read back as doubles.
    // The generic store path downgrades the layout properly; decline here.
    if crate::array::js_array_is_numeric_f64_layout(arr as *const ArrayHeader) != 0 {
        return -1;
    }
    // Overwriting a slot that holds a heap pointer during an active
    // incremental mark requires the deletion barrier the generic per-element
    // store emits. Rare phase; take the ordinary loop there.
    if crate::gc::PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT
        .load(std::sync::atomic::Ordering::Relaxed)
        != 0
    {
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
        let mut index = start;
        // GC_STORE_AUDIT(POINTER_FREE): the stored constant is a non-pointer
        // NaN-box (boolean/null/undefined/number bits), the receiver's layout
        // is boxed (raw-f64 declined above), and the incremental-mark phase
        // was excluded, so no barrier or addref applies.
        while index < end {
            ptr::write(elements.add(index as usize), value_bits);
            index += step;
        }
        i64::from(index)
    }
}

#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_ARRAY_FILL_RANGE_STRIDED_TAGGED: unsafe extern "C" fn(f64, f64, f64, f64, u64) -> i64 =
    js_array_fill_range_strided_tagged;
