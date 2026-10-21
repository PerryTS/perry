//! Adapt the folded builtin entries' i64 address carriers to actual Rust
//! pointer types. Calling the definition directly keeps wasm32 signatures
//! correct; redeclaring those pointers as i64 only happened to work on LP64.
#![allow(non_snake_case)]
use crate::array::ArrayHeader;
use crate::closure::ClosureHeader;
use crate::string::StringHeader;

#[inline]
pub(super) unsafe fn js_array_forEach(arr: i64, callback: i64) {
    crate::array::js_array_forEach(arr as *const ArrayHeader, callback as *const ClosureHeader)
}

#[inline]
pub(super) unsafe fn js_array_map(arr: i64, callback: i64) -> i64 {
    crate::array::js_array_map(arr as *const ArrayHeader, callback as *const ClosureHeader) as i64
}

#[inline]
pub(super) unsafe fn js_array_map_discard(arr: i64, callback: i64) {
    crate::array::js_array_map_discard(arr as *const ArrayHeader, callback as *const ClosureHeader)
}

#[inline]
pub(super) unsafe fn js_array_filter(arr: i64, callback: i64) -> i64 {
    crate::array::js_array_filter(arr as *const ArrayHeader, callback as *const ClosureHeader)
        as i64
}

#[inline]
pub(super) unsafe fn js_array_find(arr: i64, callback: i64) -> f64 {
    crate::array::js_array_find(arr as *const ArrayHeader, callback as *const ClosureHeader)
}

#[inline]
pub(super) unsafe fn js_array_findIndex(arr: i64, callback: i64) -> i32 {
    crate::array::js_array_findIndex(arr as *const ArrayHeader, callback as *const ClosureHeader)
}

#[inline]
pub(super) unsafe fn js_array_find_last(arr: i64, callback: i64) -> f64 {
    crate::array::js_array_find_last(arr as *const ArrayHeader, callback as *const ClosureHeader)
}

#[inline]
pub(super) unsafe fn js_array_find_last_index(arr: i64, callback: i64) -> i32 {
    crate::array::js_array_find_last_index(
        arr as *const ArrayHeader,
        callback as *const ClosureHeader,
    )
}

#[inline]
pub(super) unsafe fn js_array_some(arr: i64, callback: i64) -> f64 {
    crate::array::js_array_some(arr as *const ArrayHeader, callback as *const ClosureHeader)
}

#[inline]
pub(super) unsafe fn js_array_every(arr: i64, callback: i64) -> f64 {
    crate::array::js_array_every(arr as *const ArrayHeader, callback as *const ClosureHeader)
}

#[inline]
pub(super) unsafe fn js_array_flatMap(arr: i64, callback: i64) -> i64 {
    crate::array::js_array_flatMap(arr as *const ArrayHeader, callback as *const ClosureHeader)
        as i64
}

#[inline]
pub(super) unsafe fn js_array_sort_with_comparator(arr: i64, comparator: i64) -> i64 {
    crate::array::js_array_sort_with_comparator(
        arr as *mut ArrayHeader,
        comparator as *const ClosureHeader,
    ) as i64
}

#[inline]
pub(super) unsafe fn js_array_reduce(
    arr: i64,
    callback: i64,
    has_initial: i32,
    initial: f64,
) -> f64 {
    crate::array::js_array_reduce(
        arr as *const ArrayHeader,
        callback as *const ClosureHeader,
        has_initial,
        initial,
    )
}

#[inline]
pub(super) unsafe fn js_array_reduce_right(
    arr: i64,
    callback: i64,
    has_initial: i32,
    initial: f64,
) -> f64 {
    crate::array::js_array_reduce_right(
        arr as *const ArrayHeader,
        callback as *const ClosureHeader,
        has_initial,
        initial,
    )
}

#[inline]
pub(super) unsafe fn js_array_indexOf_jsvalue(
    arr: i64,
    value: f64,
    from_index: f64,
    has_from: i32,
) -> i64 {
    crate::array::js_array_indexOf_jsvalue(arr as *const ArrayHeader, value, from_index, has_from)
}

#[inline]
pub(super) unsafe fn js_array_last_index_of_jsvalue(
    arr: i64,
    value: f64,
    from_index: f64,
    has_from: i32,
) -> i64 {
    crate::array::js_array_last_index_of_jsvalue(
        arr as *const ArrayHeader,
        value,
        from_index,
        has_from,
    )
}

#[inline]
pub(super) unsafe fn js_array_includes_jsvalue(
    arr: i64,
    value: f64,
    from_index: f64,
    has_from: i32,
) -> i32 {
    crate::array::js_array_includes_jsvalue(arr as *const ArrayHeader, value, from_index, has_from)
}

#[inline]
pub(super) unsafe fn js_array_at(arr: i64, index: f64) -> f64 {
    crate::array::js_array_at(arr as *const ArrayHeader, index)
}

#[inline]
pub(super) unsafe fn js_array_join_value(arr: i64, separator_value: f64) -> *mut StringHeader {
    crate::array::js_array_join_value(arr as *const ArrayHeader, separator_value)
}

#[inline]
pub(super) unsafe fn js_array_flat(arr: i64) -> i64 {
    crate::array::js_array_flat(arr as *const ArrayHeader) as i64
}

#[inline]
pub(super) unsafe fn js_array_to_reversed(arr: i64) -> i64 {
    crate::array::js_array_to_reversed(arr as *const ArrayHeader) as i64
}

#[inline]
pub(super) unsafe fn js_array_to_sorted_default(arr: i64) -> i64 {
    crate::array::js_array_to_sorted_default(arr as *const ArrayHeader) as i64
}

#[inline]
pub(super) unsafe fn js_array_to_sorted_with_comparator(arr: i64, comparator: i64) -> i64 {
    crate::array::js_array_to_sorted_with_comparator(
        arr as *const ArrayHeader,
        comparator as *const ClosureHeader,
    ) as i64
}

#[inline]
pub(super) unsafe fn js_array_with(arr: i64, index: f64, value: f64) -> i64 {
    crate::array::js_array_with(arr as *const ArrayHeader, index, value) as i64
}

#[inline]
pub(super) unsafe fn js_array_entries_iter_obj(arr: i64) -> i64 {
    crate::array::js_array_entries_iter_obj(arr as *const ArrayHeader)
}

#[inline]
pub(super) unsafe fn js_array_keys_iter_obj(arr: i64) -> i64 {
    crate::array::js_array_keys_iter_obj(arr as *const ArrayHeader)
}

#[inline]
pub(super) unsafe fn js_array_values_iter_obj(arr: i64) -> i64 {
    crate::array::js_array_values_iter_obj(arr as *const ArrayHeader)
}

#[inline]
pub(super) unsafe fn js_array_slice_values(arr: i64, start_value: f64, end_value: f64) -> i64 {
    crate::array::js_array_slice_values(arr as *const ArrayHeader, start_value, end_value) as i64
}

#[inline]
pub(super) unsafe fn js_array_shift_f64(arr: i64) -> f64 {
    crate::array::js_array_shift_f64(arr as *mut ArrayHeader)
}
