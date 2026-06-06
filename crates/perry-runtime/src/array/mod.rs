//! Array representation for Perry — split into topical sub-modules.
mod alloc;
mod concat_reverse;
mod flat_clone;
mod from_concat;
mod generic;
mod header;
mod immutable;
mod indexing;
mod is_array;
mod iter_methods;
mod iter_object;
mod iterator;
mod jsvalue_api;
mod push_pop;
mod reduce_right;
mod search;
mod sort;
mod splice_slice;

#[cfg(test)]
mod tests;

pub use self::alloc::{
    js_array_alloc, js_array_alloc_literal, js_array_alloc_with_length,
    js_array_alloc_with_length_longlived, js_array_constructor_single, js_array_create,
    js_array_from_arraylike_holey_value, js_array_from_f64,
};
pub use self::concat_reverse::{
    js_array_concat, js_array_concat_new, js_array_fill, js_array_fill_range, js_array_reverse,
};
pub use self::flat_clone::{
    js_array_clone, js_array_entries, js_array_flat, js_array_flat_depth, js_array_keys,
    js_array_values,
};
pub use self::from_concat::{
    array_from_full, array_of_full, js_array_concat_variadic, js_array_from_mapped,
    js_array_from_value,
};
pub use self::generic::{
    js_arraylike_at, js_arraylike_every, js_arraylike_filter, js_arraylike_find,
    js_arraylike_findIndex, js_arraylike_findLast, js_arraylike_findLastIndex,
    js_arraylike_forEach, js_arraylike_includes, js_arraylike_indexOf, js_arraylike_join,
    js_arraylike_lastIndexOf, js_arraylike_map, js_arraylike_reduce, js_arraylike_reduceRight,
    js_arraylike_slice, js_arraylike_some,
};
pub(crate) use self::header::{array_has_arguments_object_flag, mark_array_as_arguments_object};
pub use self::header::{
    js_array_clear_numeric_layout, js_array_is_numeric_f64_layout, js_array_mark_arguments_object,
    js_array_mark_numeric_f64_layout, js_array_note_numeric_write, js_tagged_template_get_or_init,
    js_tagged_template_register_raw, js_template_raw, scan_template_raw_roots,
    scan_template_raw_roots_mut, ArrayHeader,
};
pub use self::immutable::{
    js_array_copy_within, js_array_copy_within_value, js_array_to_reversed,
    js_array_to_sorted_default, js_array_to_sorted_with_comparator, js_array_to_spliced,
    js_array_with,
};
pub use self::indexing::{
    js_array_get_element, js_array_get_element_f64, js_array_get_f64, js_array_get_f64_unchecked,
    js_array_get_index_or_string, js_array_get_length, js_array_length,
    js_array_numeric_get_f64_unboxed, js_array_numeric_set_f64_unboxed, js_array_set_f64,
    js_array_set_f64_extend, js_array_set_f64_unchecked, js_array_set_index_or_string,
    js_array_set_string_key,
};
pub use self::is_array::js_array_is_array;
pub(crate) use self::iter_methods::throw_reduce_of_empty;
pub use self::iter_methods::{
    js_array_at, js_array_every, js_array_filter, js_array_find, js_array_findIndex,
    js_array_find_last, js_array_find_last_index, js_array_flatMap, js_array_forEach,
    js_array_join, js_array_join_value, js_array_map, js_array_map_discard, js_array_reduce,
    js_array_some, js_array_to_locale_string, js_validate_array_callback,
    js_validate_array_map_callback,
};
pub use self::iter_object::{
    array_entries_iter, array_keys_iter, array_values_iter, dispatch_array_iterator_method,
    js_array_entries_iter_obj, js_array_keys_iter_obj, js_array_values_iter_obj,
    ARRAY_ITERATOR_CLASS_ID,
};
pub(crate) use self::iterator::is_builtin_iterator_class_id;
pub use self::iterator::{js_array_spread_append, js_for_of_to_array, js_iterator_to_array};
// Issue #1572 — flatten helpers reused by `node_stream::ns_iter_flat_map`
// so an `async function*` mapper return is driven through the iterator
// protocol instead of being appended as a single chunk.
pub(crate) use self::iterator::{
    async_iterator_to_array_for_flat_map, call_symbol_async_iterator_for_flat_map,
    entries_array_for_small_handle_id, has_iterator_next, sync_iterator_to_array_if_not_async,
};
pub use self::jsvalue_api::{
    js_array_from_jsvalue, js_array_get, js_array_get_jsvalue, js_array_push,
    js_array_push_jsvalue, js_array_set, js_array_set_jsvalue, js_array_set_jsvalue_extend,
};
pub use self::push_pop::{
    js_array_delete, js_array_grow, js_array_numeric_push_f64_unboxed, js_array_pop_f64,
    js_array_push_f64, js_array_push_hole, js_array_push_spread_f64, js_array_set_length,
    js_array_shift_f64, js_array_unshift_f64, js_array_unshift_jsvalue, js_array_unshift_variadic,
};
pub use self::reduce_right::js_array_reduce_right;
pub use self::search::{
    js_array_includes_f64, js_array_includes_jsvalue, js_array_indexOf_f64,
    js_array_indexOf_jsvalue, js_array_last_index_of_jsvalue,
};
pub use self::sort::{
    js_array_sort_default, js_array_sort_with_comparator, js_validate_array_comparator,
};
pub use self::splice_slice::{
    js_array_slice, js_array_slice_values, js_array_splice, js_array_splice_delete_count,
};

pub(crate) use self::alloc::array_length_from_property_value_or_throw;
pub(crate) use self::alloc::{js_array_from_arraylike, js_array_from_string_codepoints};
pub(crate) use self::header::{
    array_byte_size, array_is_frozen, array_is_sealed_or_no_extend, array_named_property_delete,
    array_named_property_get, array_named_property_get_by_name, array_named_property_has,
    array_named_property_names, array_named_property_set, array_numeric_raw_f64_get,
    array_numeric_raw_f64_push_inbounds, array_numeric_raw_f64_set_inbounds, array_object_flags,
    canonicalize_array_numeric_store_value, clean_arr_ptr, clean_arr_ptr_mut,
    clear_array_numeric_layout, clear_array_numeric_layout_ptr, gc_element_slot_range,
    mark_array_layout_unknown, normalize_array_receiver, note_array_slot,
    note_array_slot_layout_only, rebuild_array_layout, rebuild_array_layout_exact,
    refresh_array_numeric_layout, replay_array_growth_write_barriers, set_array_numeric_layout,
    store_array_slot, transfer_array_numeric_layout, value_bits_to_number, NumericArrayLayout,
    MIN_ARRAY_CAPACITY,
};

#[cfg(test)]
pub(crate) use self::header::{test_seed_template_raw_roots, test_template_raw_roots};
