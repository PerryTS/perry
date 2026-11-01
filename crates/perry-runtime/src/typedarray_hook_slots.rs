//! Typed-array operations selected by the program installer.
use crate::array::ArrayHeader;
pub(crate) use crate::byte_cell_support::shared_access::copy_lane as copy_shared_lane;
#[cfg(test)]
pub(crate) use crate::byte_cell_support::test_typed_array_registry_probe_count;
pub(crate) use crate::byte_cell_support::{
    arena_payload_has_gc_type, classify_element_read_receiver, data_ptr_mut, element_kind,
    element_length, invalidate_caches_in_range, jsvalue_to_f64, jsvalue_to_uint8, owning_u32_addr,
    strip_nanbox, throw_range_error, throw_type_error, type_for_kind,
    typed_array_has_shared_backing, ElementReadReceiver,
};
pub use crate::byte_cell_support::{
    class_id_for_kind, clean_ta_ptr, elem_size_for_kind, is_offheap_sidetable_alloc, kind_for_name,
    lookup_typed_array_kind, name_for_kind, register_typed_array, typed_array_bytes,
    typed_array_bytes_mut, typed_array_to_array_buffer,
};
use crate::closure::ClosureHeader;
use crate::feature_hooks::Hook;
pub use crate::typedarray::TypedArrayHeader;
pub use crate::typedarray::{
    CLASS_ID_BIGINT64_ARRAY, CLASS_ID_BIGUINT64_ARRAY, CLASS_ID_FLOAT16_ARRAY,
    CLASS_ID_FLOAT32_ARRAY, CLASS_ID_FLOAT64_ARRAY, CLASS_ID_INT16_ARRAY, CLASS_ID_INT32_ARRAY,
    CLASS_ID_INT8_ARRAY, CLASS_ID_UINT16_ARRAY, CLASS_ID_UINT32_ARRAY, CLASS_ID_UINT8_ARRAY,
    CLASS_ID_UINT8_CLAMPED_ARRAY, KIND_BIGINT64, KIND_BIGUINT64, KIND_FLOAT16, KIND_FLOAT32,
    KIND_FLOAT64, KIND_INT16, KIND_INT32, KIND_INT8, KIND_UINT16, KIND_UINT32, KIND_UINT8,
    KIND_UINT8_CLAMPED,
};
static COERCE_FOR_KIND: Hook<fn(u8, f64) -> f64> = Hook::empty();
pub(crate) fn coerce_for_kind(dst_kind: u8, raw: f64) -> f64 {
    COERCE_FOR_KIND.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(dst_kind, raw),
    )
}
static FORMAT_TYPED_ARRAY: Hook<fn(*const TypedArrayHeader) -> String> = Hook::empty();
pub(crate) fn format_typed_array(ta: *const TypedArrayHeader) -> String {
    FORMAT_TYPED_ARRAY
        .get()
        .map_or_else(|| String::new(), |run| run(ta))
}
#[cfg(test)]
static JS_NATIVE_MEMORY_COPY: Hook<extern "C" fn(u64, u64) -> ()> = Hook::empty();
#[cfg(test)]
pub(crate) extern "C" fn js_native_memory_copy(dst_raw: u64, src_raw: u64) -> () {
    JS_NATIVE_MEMORY_COPY
        .get()
        .map_or_else(|| (), |run| run(dst_raw, src_raw))
}
#[cfg(test)]
static JS_NATIVE_MEMORY_FILL_U32: Hook<extern "C" fn(u64, f64) -> ()> = Hook::empty();
#[cfg(test)]
pub(crate) extern "C" fn js_native_memory_fill_u32(view_raw: u64, value: f64) -> () {
    JS_NATIVE_MEMORY_FILL_U32
        .get()
        .map_or_else(|| (), |run| run(view_raw, value))
}
static JS_TYPED_ARRAY_AT: Hook<extern "C" fn(*const TypedArrayHeader, f64) -> f64> = Hook::empty();
pub(crate) extern "C" fn js_typed_array_at(ta: *const TypedArrayHeader, index: f64) -> f64 {
    JS_TYPED_ARRAY_AT.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(ta, index),
    )
}
static JS_TYPED_ARRAY_COPY_WITHIN: Hook<
    extern "C" fn(*mut TypedArrayHeader, f64, f64, f64) -> *mut TypedArrayHeader,
> = Hook::empty();
pub(crate) extern "C" fn js_typed_array_copy_within(
    ta: *mut TypedArrayHeader,
    target_value: f64,
    start_value: f64,
    end_value: f64,
) -> *mut TypedArrayHeader {
    JS_TYPED_ARRAY_COPY_WITHIN.get().map_or_else(
        || std::ptr::null_mut(),
        |run| run(ta, target_value, start_value, end_value),
    )
}
static JS_TYPED_ARRAY_EVERY: Hook<
    fn(*const TypedArrayHeader, *const ClosureHeader, f64) -> f64,
> = Hook::empty();
pub(crate) fn js_typed_array_every(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
    this_arg: f64,
) -> f64 {
    JS_TYPED_ARRAY_EVERY.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(ta, callback, this_arg),
    )
}
static JS_TYPED_ARRAY_FILL: Hook<
    extern "C" fn(*mut TypedArrayHeader, f64, i32, f64, i32, f64) -> *mut TypedArrayHeader,
> = Hook::empty();
pub(crate) extern "C" fn js_typed_array_fill(
    ta: *mut TypedArrayHeader,
    value: f64,
    has_start: i32,
    start: f64,
    has_end: i32,
    end: f64,
) -> *mut TypedArrayHeader {
    JS_TYPED_ARRAY_FILL.get().map_or_else(
        || std::ptr::null_mut(),
        |run| run(ta, value, has_start, start, has_end, end),
    )
}
static JS_TYPED_ARRAY_FILTER: Hook<
    fn(*const TypedArrayHeader, *const ClosureHeader, f64) -> *mut TypedArrayHeader,
> = Hook::empty();
pub(crate) fn js_typed_array_filter(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
    this_arg: f64,
) -> *mut TypedArrayHeader {
    JS_TYPED_ARRAY_FILTER
        .get()
        .map_or_else(|| std::ptr::null_mut(), |run| run(ta, callback, this_arg))
}
static JS_TYPED_ARRAY_FIND: Hook<
    fn(*const TypedArrayHeader, *const ClosureHeader, f64) -> f64,
> = Hook::empty();
pub(crate) fn js_typed_array_find(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
    this_arg: f64,
) -> f64 {
    JS_TYPED_ARRAY_FIND.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(ta, callback, this_arg),
    )
}
static JS_TYPED_ARRAY_FIND_INDEX: Hook<
    fn(*const TypedArrayHeader, *const ClosureHeader, f64) -> f64,
> = Hook::empty();
pub(crate) fn js_typed_array_find_index(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
    this_arg: f64,
) -> f64 {
    JS_TYPED_ARRAY_FIND_INDEX.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(ta, callback, this_arg),
    )
}
static JS_TYPED_ARRAY_FIND_LAST: Hook<
    fn(*const TypedArrayHeader, *const ClosureHeader, f64) -> f64,
> = Hook::empty();
pub(crate) fn js_typed_array_find_last(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
    this_arg: f64,
) -> f64 {
    JS_TYPED_ARRAY_FIND_LAST.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(ta, callback, this_arg),
    )
}
static JS_TYPED_ARRAY_FIND_LAST_INDEX: Hook<
    fn(*const TypedArrayHeader, *const ClosureHeader, f64) -> f64,
> = Hook::empty();
pub(crate) fn js_typed_array_find_last_index(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
    this_arg: f64,
) -> f64 {
    JS_TYPED_ARRAY_FIND_LAST_INDEX.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(ta, callback, this_arg),
    )
}
static JS_TYPED_ARRAY_FOR_EACH: Hook<
    fn(*const TypedArrayHeader, *const ClosureHeader, f64) -> f64,
> = Hook::empty();
pub(crate) fn js_typed_array_for_each(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
    this_arg: f64,
) -> f64 {
    JS_TYPED_ARRAY_FOR_EACH.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(ta, callback, this_arg),
    )
}
static JS_TYPED_ARRAY_GET: Hook<extern "C" fn(*const TypedArrayHeader, i32) -> f64> = Hook::empty();
pub extern "C" fn js_typed_array_get(ta: *const TypedArrayHeader, index: i32) -> f64 {
    JS_TYPED_ARRAY_GET.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(ta, index),
    )
}
static JS_TYPED_ARRAY_INDEX_GET_DYNAMIC: Hook<extern "C" fn(*const TypedArrayHeader, f64) -> f64> =
    Hook::empty();
pub(crate) extern "C" fn js_typed_array_index_get_dynamic(
    ta: *const TypedArrayHeader,
    key: f64,
) -> f64 {
    JS_TYPED_ARRAY_INDEX_GET_DYNAMIC.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(ta, key),
    )
}
static JS_TYPED_ARRAY_INDEX_SET_DYNAMIC: Hook<
    extern "C" fn(*mut TypedArrayHeader, f64, f64) -> f64,
> = Hook::empty();
pub(crate) extern "C" fn js_typed_array_index_set_dynamic(
    ta: *mut TypedArrayHeader,
    key: f64,
    value: f64,
) -> f64 {
    JS_TYPED_ARRAY_INDEX_SET_DYNAMIC
        .get()
        .map_or_else(|| value, |run| run(ta, key, value))
}
static JS_TYPED_ARRAY_JOIN: Hook<
    extern "C" fn(
        *const TypedArrayHeader,
        *const crate::string::StringHeader,
    ) -> *mut crate::string::StringHeader,
> = Hook::empty();
pub(crate) extern "C" fn js_typed_array_join(
    ta: *const TypedArrayHeader,
    separator: *const crate::string::StringHeader,
) -> *mut crate::string::StringHeader {
    JS_TYPED_ARRAY_JOIN
        .get()
        .map_or_else(|| std::ptr::null_mut(), |run| run(ta, separator))
}
static JS_TYPED_ARRAY_JOIN_VALUE: Hook<
    extern "C" fn(*const TypedArrayHeader, f64) -> *mut crate::string::StringHeader,
> = Hook::empty();
pub(crate) extern "C" fn js_typed_array_join_value(
    ta: *const TypedArrayHeader,
    separator_value: f64,
) -> *mut crate::string::StringHeader {
    JS_TYPED_ARRAY_JOIN_VALUE
        .get()
        .map_or_else(|| std::ptr::null_mut(), |run| run(ta, separator_value))
}
static JS_TYPED_ARRAY_LENGTH: Hook<extern "C" fn(*const TypedArrayHeader) -> i32> = Hook::empty();
pub extern "C" fn js_typed_array_length(ta: *const TypedArrayHeader) -> i32 {
    JS_TYPED_ARRAY_LENGTH.get().map_or_else(|| 0, |run| run(ta))
}
static JS_TYPED_ARRAY_MAP: Hook<
    fn(*const TypedArrayHeader, *const ClosureHeader, f64) -> *mut TypedArrayHeader,
> = Hook::empty();
pub(crate) fn js_typed_array_map(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
    this_arg: f64,
) -> *mut TypedArrayHeader {
    JS_TYPED_ARRAY_MAP
        .get()
        .map_or_else(|| std::ptr::null_mut(), |run| run(ta, callback, this_arg))
}
static JS_TYPED_ARRAY_NEW: Hook<extern "C" fn(i32, f64) -> *mut TypedArrayHeader> = Hook::empty();
pub(crate) extern "C" fn js_typed_array_new(kind: i32, val: f64) -> *mut TypedArrayHeader {
    JS_TYPED_ARRAY_NEW
        .get()
        .map_or_else(|| std::ptr::null_mut(), |run| run(kind, val))
}
static JS_TYPED_ARRAY_NEW_EMPTY: Hook<extern "C" fn(i32, i32) -> *mut TypedArrayHeader> =
    Hook::empty();
pub extern "C" fn js_typed_array_new_empty(kind: i32, length: i32) -> *mut TypedArrayHeader {
    JS_TYPED_ARRAY_NEW_EMPTY
        .get()
        .map_or_else(|| std::ptr::null_mut(), |run| run(kind, length))
}
static JS_TYPED_ARRAY_NEW_FROM_ARRAY: Hook<
    extern "C" fn(i32, *const ArrayHeader) -> *mut TypedArrayHeader,
> = Hook::empty();
pub(crate) extern "C" fn js_typed_array_new_from_array(
    kind: i32,
    arr: *const ArrayHeader,
) -> *mut TypedArrayHeader {
    JS_TYPED_ARRAY_NEW_FROM_ARRAY
        .get()
        .map_or_else(|| std::ptr::null_mut(), |run| run(kind, arr))
}
static JS_TYPED_ARRAY_REDUCE: Hook<
    extern "C" fn(*const TypedArrayHeader, *const ClosureHeader, i32, f64) -> f64,
> = Hook::empty();
pub(crate) extern "C" fn js_typed_array_reduce(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
    has_initial: i32,
    initial: f64,
) -> f64 {
    JS_TYPED_ARRAY_REDUCE.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(ta, callback, has_initial, initial),
    )
}
static JS_TYPED_ARRAY_REDUCE_RIGHT: Hook<
    extern "C" fn(*const TypedArrayHeader, *const ClosureHeader, i32, f64) -> f64,
> = Hook::empty();
pub(crate) extern "C" fn js_typed_array_reduce_right(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
    has_initial: i32,
    initial: f64,
) -> f64 {
    JS_TYPED_ARRAY_REDUCE_RIGHT.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(ta, callback, has_initial, initial),
    )
}
static JS_TYPED_ARRAY_REVERSE: Hook<extern "C" fn(*mut TypedArrayHeader) -> *mut TypedArrayHeader> =
    Hook::empty();
pub(crate) extern "C" fn js_typed_array_reverse(
    ta: *mut TypedArrayHeader,
) -> *mut TypedArrayHeader {
    JS_TYPED_ARRAY_REVERSE
        .get()
        .map_or_else(|| std::ptr::null_mut(), |run| run(ta))
}
static JS_TYPED_ARRAY_SET: Hook<extern "C" fn(*mut TypedArrayHeader, i32, f64) -> ()> =
    Hook::empty();
pub(crate) extern "C" fn js_typed_array_set(
    ta: *mut TypedArrayHeader,
    index: i32,
    value: f64,
) -> () {
    JS_TYPED_ARRAY_SET
        .get()
        .map_or_else(|| (), |run| run(ta, index, value))
}
static JS_TYPED_ARRAY_SET_FROM: Hook<extern "C" fn(*mut TypedArrayHeader, f64, f64) -> f64> =
    Hook::empty();
pub(crate) extern "C" fn js_typed_array_set_from(
    ta: *mut TypedArrayHeader,
    source_value: f64,
    offset_value: f64,
) -> f64 {
    JS_TYPED_ARRAY_SET_FROM.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(ta, source_value, offset_value),
    )
}
static JS_TYPED_ARRAY_SLICE: Hook<
    extern "C" fn(*const TypedArrayHeader, i32, i32) -> *mut TypedArrayHeader,
> = Hook::empty();
pub(crate) extern "C" fn js_typed_array_slice(
    ta: *const TypedArrayHeader,
    start: i32,
    end: i32,
) -> *mut TypedArrayHeader {
    JS_TYPED_ARRAY_SLICE
        .get()
        .map_or_else(|| std::ptr::null_mut(), |run| run(ta, start, end))
}
static JS_TYPED_ARRAY_SOME: Hook<
    fn(*const TypedArrayHeader, *const ClosureHeader, f64) -> f64,
> = Hook::empty();
pub(crate) fn js_typed_array_some(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
    this_arg: f64,
) -> f64 {
    JS_TYPED_ARRAY_SOME.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(ta, callback, this_arg),
    )
}
static JS_TYPED_ARRAY_SORT_DEFAULT: Hook<
    extern "C" fn(*mut TypedArrayHeader) -> *mut TypedArrayHeader,
> = Hook::empty();
pub(crate) extern "C" fn js_typed_array_sort_default(
    ta: *mut TypedArrayHeader,
) -> *mut TypedArrayHeader {
    JS_TYPED_ARRAY_SORT_DEFAULT
        .get()
        .map_or_else(|| std::ptr::null_mut(), |run| run(ta))
}
static JS_TYPED_ARRAY_SORT_WITH_COMPARATOR: Hook<
    extern "C" fn(*mut TypedArrayHeader, *const ClosureHeader) -> *mut TypedArrayHeader,
> = Hook::empty();
pub(crate) extern "C" fn js_typed_array_sort_with_comparator(
    ta: *mut TypedArrayHeader,
    comparator: *const ClosureHeader,
) -> *mut TypedArrayHeader {
    JS_TYPED_ARRAY_SORT_WITH_COMPARATOR
        .get()
        .map_or_else(|| std::ptr::null_mut(), |run| run(ta, comparator))
}
static JS_TYPED_ARRAY_SUBARRAY: Hook<
    extern "C" fn(*const TypedArrayHeader, i32, f64, i32, f64) -> *mut TypedArrayHeader,
> = Hook::empty();
pub(crate) extern "C" fn js_typed_array_subarray(
    ta: *const TypedArrayHeader,
    has_begin: i32,
    begin: f64,
    has_end: i32,
    end: f64,
) -> *mut TypedArrayHeader {
    JS_TYPED_ARRAY_SUBARRAY.get().map_or_else(
        || std::ptr::null_mut(),
        |run| run(ta, has_begin, begin, has_end, end),
    )
}
static JS_TYPED_ARRAY_TO_REVERSED: Hook<
    extern "C" fn(*const TypedArrayHeader) -> *mut TypedArrayHeader,
> = Hook::empty();
pub(crate) extern "C" fn js_typed_array_to_reversed(
    ta: *const TypedArrayHeader,
) -> *mut TypedArrayHeader {
    JS_TYPED_ARRAY_TO_REVERSED
        .get()
        .map_or_else(|| std::ptr::null_mut(), |run| run(ta))
}
static JS_TYPED_ARRAY_TO_SORTED_DEFAULT: Hook<
    extern "C" fn(*const TypedArrayHeader) -> *mut TypedArrayHeader,
> = Hook::empty();
pub(crate) extern "C" fn js_typed_array_to_sorted_default(
    ta: *const TypedArrayHeader,
) -> *mut TypedArrayHeader {
    JS_TYPED_ARRAY_TO_SORTED_DEFAULT
        .get()
        .map_or_else(|| std::ptr::null_mut(), |run| run(ta))
}
static JS_TYPED_ARRAY_TO_SORTED_WITH_COMPARATOR: Hook<
    extern "C" fn(*const TypedArrayHeader, *const ClosureHeader) -> *mut TypedArrayHeader,
> = Hook::empty();
pub(crate) extern "C" fn js_typed_array_to_sorted_with_comparator(
    ta: *const TypedArrayHeader,
    comparator: *const ClosureHeader,
) -> *mut TypedArrayHeader {
    JS_TYPED_ARRAY_TO_SORTED_WITH_COMPARATOR
        .get()
        .map_or_else(|| std::ptr::null_mut(), |run| run(ta, comparator))
}
static JS_TYPED_ARRAY_VIEW: Hook<extern "C" fn(i32, f64, f64, f64) -> *mut TypedArrayHeader> =
    Hook::empty();
pub(crate) extern "C" fn js_typed_array_view(
    kind: i32,
    source: f64,
    offset_value: f64,
    length_value: f64,
) -> *mut TypedArrayHeader {
    JS_TYPED_ARRAY_VIEW.get().map_or_else(
        || std::ptr::null_mut(),
        |run| run(kind, source, offset_value, length_value),
    )
}
static JS_TYPED_ARRAY_WITH: Hook<
    extern "C" fn(*const TypedArrayHeader, f64, f64) -> *mut TypedArrayHeader,
> = Hook::empty();
pub(crate) extern "C" fn js_typed_array_with(
    ta: *const TypedArrayHeader,
    index: f64,
    value: f64,
) -> *mut TypedArrayHeader {
    JS_TYPED_ARRAY_WITH
        .get()
        .map_or_else(|| std::ptr::null_mut(), |run| run(ta, index, value))
}
#[cfg(test)]
static JS_UINT8ARRAY_GET: Hook<extern "C" fn(*const TypedArrayHeader, i32) -> i32> = Hook::empty();
#[cfg(test)]
pub(crate) extern "C" fn js_uint8array_get(target: *const TypedArrayHeader, index: i32) -> i32 {
    JS_UINT8ARRAY_GET
        .get()
        .map_or_else(|| 0, |run| run(target, index))
}
#[cfg(test)]
static JS_UINT8ARRAY_SET: Hook<extern "C" fn(*mut TypedArrayHeader, i32, i32) -> ()> =
    Hook::empty();
#[cfg(test)]
pub(crate) extern "C" fn js_uint8array_set(
    target: *mut TypedArrayHeader,
    index: i32,
    value: i32,
) -> () {
    JS_UINT8ARRAY_SET
        .get()
        .map_or_else(|| (), |run| run(target, index, value))
}

static PROTOTYPE_CONSTRUCTOR_PATCH: Hook<unsafe fn(u8, usize) -> Option<f64>> = Hook::empty();
pub(crate) unsafe fn prototype_constructor_patch(kind: u8, owner: usize) -> Option<f64> {
    PROTOTYPE_CONSTRUCTOR_PATCH
        .get()
        .map_or_else(|| None, |run| unsafe { run(kind, owner) })
}
static STORE_COERCED: Hook<unsafe fn(*mut TypedArrayHeader, usize, u8, f64) -> ()> = Hook::empty();
pub(crate) unsafe fn store_coerced(
    ta: *mut TypedArrayHeader,
    index: usize,
    kind: u8,
    raw: f64,
) -> () {
    STORE_COERCED
        .get()
        .map_or_else(|| (), |run| unsafe { run(ta, index, kind, raw) })
}
static TO_BIGINT_FOR_STORE: Hook<fn(f64) -> f64> = Hook::empty();
pub(crate) fn to_bigint_for_store(value: f64) -> f64 {
    TO_BIGINT_FOR_STORE.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(value),
    )
}
static TO_NUMBER: Hook<fn(f64) -> f64> = Hook::empty();
pub fn to_number(raw: f64) -> f64 {
    TO_NUMBER.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(raw),
    )
}
pub use crate::byte_cell_support::typed_array_alloc;
static TYPED_ARRAY_FAST_INDEX_GET: Hook<fn(usize, u8, f64) -> Option<f64>> = Hook::empty();
pub(crate) fn typed_array_fast_index_get(ptr: usize, kind: u8, index: f64) -> Option<f64> {
    TYPED_ARRAY_FAST_INDEX_GET
        .get()
        .map_or_else(|| None, |run| run(ptr, kind, index))
}
static TYPED_ARRAY_FAST_INDEX_SET: Hook<fn(usize, u8, f64, f64) -> bool> = Hook::empty();
pub(crate) fn typed_array_fast_index_set(ptr: usize, kind: u8, index: f64, value: f64) -> bool {
    TYPED_ARRAY_FAST_INDEX_SET
        .get()
        .map_or_else(|| false, |run| run(ptr, kind, index, value))
}
static TYPED_ARRAY_FROM_SOURCE_RAW_VALUES: Hook<unsafe fn(f64) -> Vec<f64>> = Hook::empty();
pub(crate) unsafe fn typed_array_from_source_raw_values(val: f64) -> Vec<f64> {
    TYPED_ARRAY_FROM_SOURCE_RAW_VALUES
        .get()
        .map_or_else(|| Vec::new(), |run| unsafe { run(val) })
}

static TYPED_ARRAY_TO_ARRAY: Hook<fn(*const TypedArrayHeader) -> *mut crate::array::ArrayHeader> =
    Hook::empty();
pub(crate) fn typed_array_to_array(ta: *const TypedArrayHeader) -> *mut crate::array::ArrayHeader {
    TYPED_ARRAY_TO_ARRAY
        .get()
        .map_or_else(|| std::ptr::null_mut(), |run| run(ta))
}
pub(crate) mod species {
    pub(crate) use super::{prototype_constructor_patch, store_coerced, to_number};
}
pub(crate) mod bigint {
    pub(crate) use super::{coerce_for_kind, to_bigint_for_store};
    pub(crate) use crate::byte_cell_support::{is_bigint_kind, throw_bigint_number_mix};
}
pub(crate) fn install() {
    crate::perry_install_once!({
        COERCE_FOR_KIND.set(crate::typedarray::bigint::coerce_for_kind);
        FORMAT_TYPED_ARRAY.set(crate::typedarray::format_typed_array);
        #[cfg(test)]
        JS_NATIVE_MEMORY_COPY.set(crate::typedarray::js_native_memory_copy);
        #[cfg(test)]
        JS_NATIVE_MEMORY_FILL_U32.set(crate::typedarray::js_native_memory_fill_u32);
        JS_TYPED_ARRAY_AT.set(crate::typedarray::js_typed_array_at);
        JS_TYPED_ARRAY_COPY_WITHIN.set(crate::typedarray::js_typed_array_copy_within);
        JS_TYPED_ARRAY_EVERY.set(crate::typedarray::typed_array_every_this);
        JS_TYPED_ARRAY_FILL.set(crate::typedarray::js_typed_array_fill);
        JS_TYPED_ARRAY_FILTER.set(crate::typedarray::typed_array_filter_this);
        JS_TYPED_ARRAY_FIND.set(crate::typedarray::typed_array_find_this);
        JS_TYPED_ARRAY_FIND_INDEX.set(crate::typedarray::typed_array_find_index_this);
        JS_TYPED_ARRAY_FIND_LAST.set(crate::typedarray::typed_array_find_last_this);
        JS_TYPED_ARRAY_FIND_LAST_INDEX.set(crate::typedarray::typed_array_find_last_index_this);
        JS_TYPED_ARRAY_FOR_EACH.set(crate::typedarray::typed_array_for_each_this);
        JS_TYPED_ARRAY_GET.set(crate::typedarray::js_typed_array_get);
        JS_TYPED_ARRAY_INDEX_GET_DYNAMIC.set(crate::typedarray::js_typed_array_index_get_dynamic);
        JS_TYPED_ARRAY_INDEX_SET_DYNAMIC
            .set(crate::typedarray_props::js_typed_array_index_set_dynamic);
        JS_TYPED_ARRAY_JOIN.set(crate::typedarray::js_typed_array_join);
        JS_TYPED_ARRAY_JOIN_VALUE.set(crate::typedarray::js_typed_array_join_value);
        JS_TYPED_ARRAY_LENGTH.set(crate::typedarray::js_typed_array_length);
        JS_TYPED_ARRAY_MAP.set(crate::typedarray::typed_array_map_this);
        JS_TYPED_ARRAY_NEW.set(crate::typedarray::js_typed_array_new);
        JS_TYPED_ARRAY_NEW_EMPTY.set(crate::typedarray::js_typed_array_new_empty);
        JS_TYPED_ARRAY_NEW_FROM_ARRAY.set(crate::typedarray::js_typed_array_new_from_array);
        JS_TYPED_ARRAY_REDUCE.set(crate::typedarray::js_typed_array_reduce);
        JS_TYPED_ARRAY_REDUCE_RIGHT.set(crate::typedarray::js_typed_array_reduce_right);
        JS_TYPED_ARRAY_REVERSE.set(crate::typedarray::js_typed_array_reverse);
        JS_TYPED_ARRAY_SET.set(crate::typedarray::js_typed_array_set);
        JS_TYPED_ARRAY_SET_FROM.set(crate::typedarray::js_typed_array_set_from);
        JS_TYPED_ARRAY_SLICE.set(crate::typedarray::js_typed_array_slice);
        JS_TYPED_ARRAY_SOME.set(crate::typedarray::typed_array_some_this);
        JS_TYPED_ARRAY_SORT_DEFAULT.set(crate::typedarray::js_typed_array_sort_default);
        JS_TYPED_ARRAY_SORT_WITH_COMPARATOR
            .set(crate::typedarray::js_typed_array_sort_with_comparator);
        JS_TYPED_ARRAY_SUBARRAY.set(crate::typedarray::js_typed_array_subarray);
        JS_TYPED_ARRAY_TO_REVERSED.set(crate::typedarray::js_typed_array_to_reversed);
        JS_TYPED_ARRAY_TO_SORTED_DEFAULT.set(crate::typedarray::js_typed_array_to_sorted_default);
        JS_TYPED_ARRAY_TO_SORTED_WITH_COMPARATOR
            .set(crate::typedarray::js_typed_array_to_sorted_with_comparator);
        JS_TYPED_ARRAY_VIEW.set(crate::typedarray_view::js_typed_array_view);
        JS_TYPED_ARRAY_WITH.set(crate::typedarray::js_typed_array_with);
        #[cfg(test)]
        JS_UINT8ARRAY_GET.set(crate::typedarray::js_uint8array_get);
        #[cfg(test)]
        JS_UINT8ARRAY_SET.set(crate::typedarray::js_uint8array_set);
        PROTOTYPE_CONSTRUCTOR_PATCH.set(crate::typedarray::species::prototype_constructor_patch);
        STORE_COERCED.set(crate::typedarray::species::store_coerced);
        TO_BIGINT_FOR_STORE.set(crate::typedarray::bigint::to_bigint_for_store);
        TO_NUMBER.set(crate::typedarray::species::to_number);
        TYPED_ARRAY_FAST_INDEX_GET.set(crate::typedarray::typed_array_fast_index_get);
        TYPED_ARRAY_FAST_INDEX_SET.set(crate::typedarray::typed_array_fast_index_set);
        TYPED_ARRAY_FROM_SOURCE_RAW_VALUES
            .set(crate::typedarray::typed_array_from_source_raw_values);
        TYPED_ARRAY_TO_ARRAY.set(crate::typedarray::typed_array_to_array);
    });
}
