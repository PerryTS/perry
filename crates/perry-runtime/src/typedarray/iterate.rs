//! `%TypedArray%.prototype` iteration methods (map/filter/every/some/forEach/
//! find/findIndex/reduce/reduceRight). Split out of `typedarray/mod.rs`.

use super::*;

use crate::closure::ClosureHeader;

// %TypedArray%.prototype iteration methods. The generic `js_array_*` helpers
// detect a TypedArray receiver via `lookup_typed_array_kind` and delegate
// here (mirroring the existing sort / at / findLast delegation), so these
// read elements through the element-typed `load_at` instead of reinterpreting
// the raw int/float storage as NaN-boxed f64 (which produced garbage values).
// The callback receives `(element, index)` — same 2-arg convention the rest of
// this file and the generic array helpers use.

/// `ta.map(cb)` — returns a NEW TypedArray of the SAME kind (per spec, not a
/// plain Array). Each result is coerced back to the element type via the same
/// `jsvalue_to_f64` path `ta[i] = v` uses.
#[no_mangle]
pub extern "C" fn js_typed_array_map(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
) -> *mut TypedArrayHeader {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return typed_array_alloc(KIND_FLOAT64, 0);
    }
    unsafe {
        let kind = (*ta).kind;
        let len = (*ta).length as usize;
        let recv = ta_receiver_value(ta);
        // 23.2.3.20 step 5: A is TypedArraySpeciesCreate(O, « len ») — BEFORE
        // the callback loop (so a throwing constructor/@@species getter aborts
        // before any callback runs).
        let choice = species::species_constructor(ta as usize, kind);
        let result = species::species_create_length(&choice, kind, len);
        let Some(result_addr) = crate::typedarray_props::typed_array_addr_from_value(result) else {
            return species::result_as_ptr(result);
        };
        for i in 0..len {
            let v = load_at(ta, i);
            let r = crate::closure::js_closure_call3(callback, v, i as f64, recv);
            crate::typedarray_props::species_result_store(result_addr, i, r);
        }
        species::result_as_ptr(result)
    }
}

/// `ta.filter(cb)` — returns a NEW TypedArray of the SAME kind holding the
/// elements for which `cb` returned truthy.
#[no_mangle]
pub extern "C" fn js_typed_array_filter(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
) -> *mut TypedArrayHeader {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return typed_array_alloc(KIND_FLOAT64, 0);
    }
    unsafe {
        let kind = (*ta).kind;
        let len = (*ta).length as usize;
        let recv = ta_receiver_value(ta);
        // 23.2.3.10: the callback runs for every element FIRST (collecting the
        // kept values), THEN A = TypedArraySpeciesCreate(O, « captured »). The
        // @@species getter is therefore observed after all callbacks.
        let mut kept: Vec<f64> = Vec::new();
        for i in 0..len {
            let v = load_at(ta, i);
            let r = crate::closure::js_closure_call3(callback, v, i as f64, recv);
            if crate::value::js_is_truthy(r) != 0 {
                kept.push(v);
            }
        }
        let choice = species::species_constructor(ta as usize, kind);
        let result = species::species_create_length(&choice, kind, kept.len());
        let Some(result_addr) = crate::typedarray_props::typed_array_addr_from_value(result) else {
            return species::result_as_ptr(result);
        };
        for (i, v) in kept.into_iter().enumerate() {
            crate::typedarray_props::species_result_store(result_addr, i, v);
        }
        species::result_as_ptr(result)
    }
}

/// `ta.every(cb)` — NaN-boxed boolean.
#[no_mangle]
pub extern "C" fn js_typed_array_every(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
) -> f64 {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return f64::from_bits(crate::value::TAG_TRUE);
    }
    unsafe {
        let len = (*ta).length as usize;
        let recv = ta_receiver_value(ta);
        for i in 0..len {
            let v = load_at(ta, i);
            let r = crate::closure::js_closure_call3(callback, v, i as f64, recv);
            if crate::value::js_is_truthy(r) == 0 {
                return f64::from_bits(crate::value::TAG_FALSE);
            }
        }
        f64::from_bits(crate::value::TAG_TRUE)
    }
}

/// `ta.some(cb)` — NaN-boxed boolean.
#[no_mangle]
pub extern "C" fn js_typed_array_some(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
) -> f64 {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return f64::from_bits(crate::value::TAG_FALSE);
    }
    unsafe {
        let len = (*ta).length as usize;
        let recv = ta_receiver_value(ta);
        for i in 0..len {
            let v = load_at(ta, i);
            let r = crate::closure::js_closure_call3(callback, v, i as f64, recv);
            if crate::value::js_is_truthy(r) != 0 {
                return f64::from_bits(crate::value::TAG_TRUE);
            }
        }
        f64::from_bits(crate::value::TAG_FALSE)
    }
}

/// `ta.forEach(cb)` — returns undefined.
#[no_mangle]
pub extern "C" fn js_typed_array_for_each(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
) -> f64 {
    let ta = clean_ta_ptr(ta);
    if !ta.is_null() {
        unsafe {
            let len = (*ta).length as usize;
            let recv = ta_receiver_value(ta);
            for i in 0..len {
                let v = load_at(ta, i);
                let _ = crate::closure::js_closure_call3(callback, v, i as f64, recv);
            }
        }
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// `ta.find(cb)` — first element for which `cb` is truthy, else undefined.
#[no_mangle]
pub extern "C" fn js_typed_array_find(
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
        for i in 0..len {
            let v = load_at(ta, i);
            let r = crate::closure::js_closure_call3(callback, v, i as f64, recv);
            if crate::value::js_is_truthy(r) != 0 {
                return v;
            }
        }
        f64::from_bits(crate::value::TAG_UNDEFINED)
    }
}

/// `ta.findIndex(cb)` — first matching index as plain f64, else -1.
#[no_mangle]
pub extern "C" fn js_typed_array_find_index(
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
        for i in 0..len {
            let v = load_at(ta, i);
            let r = crate::closure::js_closure_call3(callback, v, i as f64, recv);
            if crate::value::js_is_truthy(r) != 0 {
                return i as f64;
            }
        }
        -1.0
    }
}

/// `ta.reduce(cb, initial?)` — accumulate left→right. Reads elements through
/// `load_at` (element-typed) and calls the reducer as
/// `(accumulator, currentValue, currentIndex, array)`. Throws
/// `TypeError: Reduce of empty array with no initial value` when the typed
/// array is empty and no initial value was provided. Issue #2799.
#[no_mangle]
pub extern "C" fn js_typed_array_reduce(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
    has_initial: i32,
    initial: f64,
) -> f64 {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        if has_initial != 0 {
            return initial;
        }
        crate::array::throw_reduce_of_empty();
    }
    unsafe {
        let len = (*ta).length as usize;
        if len == 0 {
            if has_initial != 0 {
                return initial;
            }
            crate::array::throw_reduce_of_empty();
        }
        let recv = ta_receiver_value(ta);
        let (mut accumulator, start_idx) = if has_initial != 0 {
            (initial, 0)
        } else {
            (load_at(ta, 0), 1)
        };
        for i in start_idx..len {
            let v = load_at(ta, i);
            accumulator =
                crate::closure::js_closure_call4(callback, accumulator, v, i as f64, recv);
        }
        accumulator
    }
}

/// `ta.reduceRight(cb, initial?)` — accumulate right→left. Same reducer
/// contract as `js_typed_array_reduce`. Issue #2799.
#[no_mangle]
pub extern "C" fn js_typed_array_reduce_right(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
    has_initial: i32,
    initial: f64,
) -> f64 {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        if has_initial != 0 {
            return initial;
        }
        crate::array::throw_reduce_of_empty();
    }
    unsafe {
        let len = (*ta).length as usize;
        if len == 0 {
            if has_initial != 0 {
                return initial;
            }
            crate::array::throw_reduce_of_empty();
        }
        let recv = ta_receiver_value(ta);
        let (mut accumulator, start_idx) = if has_initial != 0 {
            (initial, len)
        } else {
            (load_at(ta, len - 1), len - 1)
        };
        if start_idx > 0 {
            for i in (0..start_idx).rev() {
                let v = load_at(ta, i);
                accumulator =
                    crate::closure::js_closure_call4(callback, accumulator, v, i as f64, recv);
            }
        }
        accumulator
    }
}
