//! TypedArray materialization and immutable/sort transforms:
//! `to_array`, `toReversed`, `sort`/`toSorted` (default + comparator),
//! `with`, `findLast`/`findLastIndex`. Split out of `typedarray/mod.rs`.

use super::*;

use std::alloc::{alloc, Layout};
use std::cell::RefCell;
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::array::ArrayHeader;
use crate::closure::ClosureHeader;
use crate::typedarray_half::{f16_bits_to_f64, f64_to_f16_bits};

/// Materialize a typed array as a regular Array of f64s. Each element is
/// loaded via the per-kind accessor (`load_at`) so `Uint8Array([10,20,30,40])`
/// becomes `Array[10.0, 20.0, 30.0, 40.0]` rather than four raw NaN-box-bit
/// reinterpretations of the byte buffer. Issue #578.
///
/// Used by `js_array_clone` (Array.from / for-of materialize), `js_array_concat`
/// (`[...typedArray]` spread + `concat`), and any other path that bridges
/// from typed-array storage into a normal Array.
pub fn typed_array_to_array(ta: *const TypedArrayHeader) -> *mut crate::array::ArrayHeader {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return crate::array::js_array_alloc(0);
    }
    unsafe {
        let len = (*ta).length as usize;
        let result = crate::array::js_array_alloc(len as u32);
        if len == 0 {
            return result;
        }
        let dst =
            (result as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *mut f64;
        for i in 0..len {
            *dst.add(i) = load_at(ta, i);
        }
        (*result).length = len as u32;
        result
    }
}

/// `ta.toReversed()` — new typed array of same kind with reversed elements.
#[no_mangle]
pub extern "C" fn js_typed_array_to_reversed(ta: *const TypedArrayHeader) -> *mut TypedArrayHeader {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return typed_array_alloc(KIND_FLOAT64, 0);
    }
    unsafe {
        let kind = (*ta).kind;
        let len = (*ta).length as usize;
        let out = typed_array_alloc(kind, len as u32);
        for i in 0..len {
            let v = load_at(ta, len - 1 - i);
            store_at(out, i, v);
        }
        out
    }
}

/// Spec default sort order for typed-array Numbers (`%TypedArray%.prototype.
/// sort` without a comparator): ascending, every NaN at the end, and `-0`
/// before `+0`. `partial_cmp` got neither right (NaN compared `Equal` so NaNs
/// stayed in place; `-0 == +0` left zeros in input order).
fn typed_array_default_number_cmp(a: &f64, b: &f64) -> std::cmp::Ordering {
    match (a.is_nan(), b.is_nan()) {
        (true, true) => std::cmp::Ordering::Equal,
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        _ => a.total_cmp(b),
    }
}

/// Default-sort `ta`'s elements in place. BigInt kinds sort the raw 64-bit
/// lanes (signed/unsigned) — `load_at` boxes each element as a fresh BigInt
/// pointer, and sorting those bit patterns scrambled the array.
unsafe fn typed_array_sort_default_in_place(ta: *mut TypedArrayHeader) {
    let len = (*ta).length as usize;
    if len <= 1 {
        return;
    }
    match (*ta).kind {
        KIND_BIGINT64 => {
            let base = data_ptr_mut(ta) as *mut i64;
            std::slice::from_raw_parts_mut(base, len).sort_unstable();
        }
        KIND_BIGUINT64 => {
            let base = data_ptr_mut(ta) as *mut u64;
            std::slice::from_raw_parts_mut(base, len).sort_unstable();
        }
        _ => {
            let mut buf: Vec<f64> = (0..len).map(|i| load_at(ta, i)).collect();
            buf.sort_by(typed_array_default_number_cmp);
            for (i, v) in buf.into_iter().enumerate() {
                store_at(ta, i, v);
            }
        }
    }
}

/// `ta.sort()` — default ascending numeric sort, **in place**. Per the
/// JS spec, the same typed-array reference is returned. Issue #654.
#[no_mangle]
pub extern "C" fn js_typed_array_sort_default(ta: *mut TypedArrayHeader) -> *mut TypedArrayHeader {
    let ta_clean = clean_ta_ptr(ta as *const TypedArrayHeader) as *mut TypedArrayHeader;
    if ta_clean.is_null() {
        return ta_clean;
    }
    unsafe {
        typed_array_sort_default_in_place(ta_clean);
        ta_clean
    }
}

/// Invoke a typed-array sort comparator on two raw BigInt64/BigUint64 lanes.
/// Each lane is boxed as a fresh GC BigInt ONLY for the duration of this one
/// call, and each box is rooted in a scope: the second box's allocation (and
/// the comparator body itself) can trigger a GC that would otherwise sweep —
/// or move — the first while its pointer sits in a bare Rust local.
unsafe fn bigint_lane_compare(
    comparator: *const ClosureHeader,
    a_bits: u64,
    b_bits: u64,
    signed: bool,
) -> std::cmp::Ordering {
    let scope = crate::gc::RuntimeHandleScope::new();
    let box_lane = |bits: u64| -> f64 {
        if signed {
            crate::value::js_nanbox_bigint(crate::bigint::js_bigint_from_i64(bits as i64) as i64)
        } else {
            crate::value::js_nanbox_bigint(crate::bigint::js_bigint_from_u64(bits) as i64)
        }
    };
    let a_handle = scope.root_nanbox_f64(box_lane(a_bits));
    let b_handle = scope.root_nanbox_f64(box_lane(b_bits));
    let r = crate::closure::js_closure_call2(
        comparator,
        a_handle.get_nanbox_f64(),
        b_handle.get_nanbox_f64(),
    );
    if r < 0.0 {
        std::cmp::Ordering::Less
    } else if r > 0.0 {
        std::cmp::Ordering::Greater
    } else {
        std::cmp::Ordering::Equal
    }
}

/// Comparator-sorted copy of a BigInt64/BigUint64 array's raw 64-bit lanes.
/// The lanes live in an OWNED Rust buffer for the whole sort — no boxed
/// BigInt pointers (the old code parked a `Vec<f64>` full of unrooted boxes
/// across every comparator call) and no pointer into the typed-array storage
/// are held across user code; each compare boxes its two operands lazily,
/// mirroring the raw-lane special case the default no-comparator sort already
/// had. The comparator closure itself is re-derived from a rooted handle per
/// call (a comparator-triggered GC can relocate its own closure header).
unsafe fn sorted_bigint_lanes(
    ta: *const TypedArrayHeader,
    len: usize,
    signed: bool,
    comparator: *const ClosureHeader,
) -> Vec<u64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let cmp_handle = scope.root_raw_const_ptr(comparator);
    let mut lanes: Vec<u64> = std::slice::from_raw_parts(data_ptr(ta) as *const u64, len).to_vec();
    lanes.sort_by(|&a, &b| {
        bigint_lane_compare(
            cmp_handle.get_raw_const_ptr::<ClosureHeader>(),
            a,
            b,
            signed,
        )
    });
    lanes
}

/// `ta.sort(cmp)` — in-place sort with comparator. Issue #654.
#[no_mangle]
pub extern "C" fn js_typed_array_sort_with_comparator(
    ta: *mut TypedArrayHeader,
    comparator: *const ClosureHeader,
) -> *mut TypedArrayHeader {
    // #2796: null comparator (validated `undefined`) -> default sort.
    if comparator.is_null() {
        return js_typed_array_sort_default(ta);
    }
    let ta_clean = clean_ta_ptr(ta as *const TypedArrayHeader) as *mut TypedArrayHeader;
    if ta_clean.is_null() {
        return ta_clean;
    }
    unsafe {
        let len = (*ta_clean).length as usize;
        if len <= 1 {
            return ta_clean;
        }
        let kind = (*ta_clean).kind;
        if kind == KIND_BIGINT64 || kind == KIND_BIGUINT64 {
            // Sort the raw lanes with lazy per-compare boxing; the receiver is
            // rooted so the write-back targets its CURRENT address even when a
            // comparator-triggered GC relocated it.
            let scope = crate::gc::RuntimeHandleScope::new();
            let ta_handle = scope.root_raw_mut_ptr(ta_clean);
            let lanes = sorted_bigint_lanes(ta_clean, len, kind == KIND_BIGINT64, comparator);
            let ta_cur = ta_handle.get_raw_mut_ptr::<TypedArrayHeader>();
            let base = data_ptr_mut(ta_cur) as *mut u64;
            for (i, bits) in lanes.into_iter().enumerate() {
                *base.add(i) = bits;
            }
            return ta_cur;
        }
        // Non-BigInt kinds: `load_at` yields plain numeric f64s (no heap
        // pointers), so the owned buffer is GC-inert by construction.
        let mut buf: Vec<f64> = (0..len).map(|i| load_at(ta_clean, i)).collect();
        buf.sort_by(|a, b| {
            let r = crate::closure::js_closure_call2(comparator, *a, *b);
            if r < 0.0 {
                std::cmp::Ordering::Less
            } else if r > 0.0 {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
        for (i, v) in buf.into_iter().enumerate() {
            store_at(ta_clean, i, v);
        }
        ta_clean
    }
}

/// `ta.toSorted()` — default ascending numeric sort.
#[no_mangle]
pub extern "C" fn js_typed_array_to_sorted_default(
    ta: *const TypedArrayHeader,
) -> *mut TypedArrayHeader {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return typed_array_alloc(KIND_FLOAT64, 0);
    }
    unsafe {
        let kind = (*ta).kind;
        let len = (*ta).length as usize;
        let out = typed_array_alloc(kind, len as u32);
        // Copy the raw lanes, then reuse the in-place default sort (BigInt
        // kinds sort raw 64-bit lanes; Number kinds use the spec NaN/-0 order).
        let elem = (*ta).elem_size as usize;
        ptr::copy_nonoverlapping(data_ptr(ta), data_ptr_mut(out), len * elem);
        typed_array_sort_default_in_place(out);
        out
    }
}

/// `ta.toSorted(cmp)`.
#[no_mangle]
pub extern "C" fn js_typed_array_to_sorted_with_comparator(
    ta: *const TypedArrayHeader,
    comparator: *const ClosureHeader,
) -> *mut TypedArrayHeader {
    // #2796: null comparator (validated `undefined`) -> default sort.
    if comparator.is_null() {
        return js_typed_array_to_sorted_default(ta);
    }
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return typed_array_alloc(KIND_FLOAT64, 0);
    }
    unsafe {
        let kind = (*ta).kind;
        let len = (*ta).length as usize;
        if kind == KIND_BIGINT64 || kind == KIND_BIGUINT64 {
            // Copy the raw lanes out FIRST (owned buffer), sort with lazy
            // per-compare boxing (no unrooted BigInt boxes parked across
            // comparator calls), then allocate the result only after all
            // user code has run.
            let lanes = sorted_bigint_lanes(ta, len, kind == KIND_BIGINT64, comparator);
            let out = typed_array_alloc(kind, len as u32);
            let base = data_ptr_mut(out) as *mut u64;
            for (i, bits) in lanes.into_iter().enumerate() {
                *base.add(i) = bits;
            }
            return out;
        }
        // Non-BigInt kinds: plain numeric f64s — the owned buffer is GC-inert.
        let mut buf: Vec<f64> = (0..len).map(|i| load_at(ta, i)).collect();
        buf.sort_by(|a, b| {
            let r = crate::closure::js_closure_call2(comparator, *a, *b);
            if r < 0.0 {
                std::cmp::Ordering::Less
            } else if r > 0.0 {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
        let out = typed_array_alloc(kind, len as u32);
        for (i, v) in buf.into_iter().enumerate() {
            store_at(out, i, v);
        }
        out
    }
}

/// `ta.with(index, value)` — return new array with single element replaced.
#[no_mangle]
pub extern "C" fn js_typed_array_with(
    ta: *const TypedArrayHeader,
    index: f64,
    value: f64,
) -> *mut TypedArrayHeader {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return typed_array_alloc(KIND_FLOAT64, 0);
    }
    unsafe {
        let kind = (*ta).kind;
        let len = (*ta).length as usize;
        // ECMA ToIntegerOrInfinity: NaN -> 0, reject non-finite / out-of-range
        // with RangeError("Invalid typed array index") (Node parity, #2792).
        let rel = if index.is_nan() { 0.0 } else { index };
        if !rel.is_finite() {
            throw_range_error(b"Invalid typed array index");
        }
        let resolved = if rel < 0.0 { rel + len as f64 } else { rel };
        if resolved < 0.0 || resolved >= len as f64 {
            throw_range_error(b"Invalid typed array index");
        }
        let idx = resolved as i64;
        let replacement = bigint::coerce_for_kind(kind, value);
        let out = typed_array_alloc(kind, len as u32);
        for i in 0..len {
            if i as i64 == idx {
                store_at(out, i, replacement);
            } else {
                store_at(out, i, load_at(ta, i));
            }
        }
        out
    }
}

/// `ta.findLast(cb)`. Returns the matched element as a plain f64
/// (NOT NaN-boxed), or NaN-boxed undefined if none match.
#[no_mangle]
pub extern "C" fn js_typed_array_find_last(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
) -> f64 {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    unsafe {
        let len = (*ta).length as usize;
        let recv = ta_receiver_value(ta);
        for i in (0..len).rev() {
            let v = load_at(ta, i);
            let r = crate::closure::js_closure_call3(callback, v, i as f64, recv);
            if crate::value::js_is_truthy(r) != 0 {
                return v;
            }
        }
        f64::from_bits(crate::value::TAG_UNDEFINED)
    }
}

/// `ta.findLastIndex(cb)`. Returns plain f64 index, or -1.
#[no_mangle]
pub extern "C" fn js_typed_array_find_last_index(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
) -> f64 {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return -1.0;
    }
    unsafe {
        let len = (*ta).length as usize;
        let recv = ta_receiver_value(ta);
        for i in (0..len).rev() {
            let v = load_at(ta, i);
            let r = crate::closure::js_closure_call3(callback, v, i as f64, recv);
            if crate::value::js_is_truthy(r) != 0 {
                return i as f64;
            }
        }
        -1.0
    }
}
