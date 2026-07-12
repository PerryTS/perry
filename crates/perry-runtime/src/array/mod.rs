//! Array representation for Perry — split into topical sub-modules.
mod alloc;
mod concat_reverse;
mod flat_clone;
mod from_concat;
mod generic;
mod generic_mutators;
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
mod species;
mod splice_slice;
mod subclass;

#[cfg(test)]
mod tests;

pub(crate) use self::alloc::array_length_range_error;
pub use self::alloc::{
    js_array_alloc, js_array_alloc_literal, js_array_alloc_with_length,
    js_array_alloc_with_length_longlived, js_array_constructor_single, js_array_create,
    js_array_from_arraylike_holey_value, js_array_from_f64,
};
pub use self::concat_reverse::{
    js_array_concat, js_array_concat_new, js_array_fill, js_array_fill_generic,
    js_array_fill_range, js_array_reverse, js_array_reverse_value,
};
pub use self::flat_clone::{
    js_array_clone, js_array_entries, js_array_flat, js_array_flat_depth, js_array_keys,
    js_array_values,
};
pub use self::from_concat::{
    array_from_full, array_of_full, js_array_concat_variadic, js_array_from_mapped,
    js_array_from_value,
};
pub use self::generic::array_proto_mutator;
pub use self::generic::{
    dispatch_arraylike_read_method, js_arraylike_at, js_arraylike_concat, js_arraylike_every,
    js_arraylike_filter, js_arraylike_find, js_arraylike_findIndex, js_arraylike_findLast,
    js_arraylike_findLastIndex, js_arraylike_forEach, js_arraylike_includes, js_arraylike_indexOf,
    js_arraylike_join, js_arraylike_lastIndexOf, js_arraylike_map, js_arraylike_reduce,
    js_arraylike_reduceRight, js_arraylike_slice, js_arraylike_some, js_arraylike_sort,
    js_arraylike_splice, try_array_proto_chain_method, try_object_arraylike_mutator,
};
pub(crate) use self::generic::{
    non_array_object_receiver, object_owns_user_method, object_pop as generic_object_pop,
    object_shift as generic_object_shift, object_sort, object_splice, plain_object_value,
};
pub use self::generic_mutators::{
    js_arraylike_pop, js_arraylike_push, js_arraylike_shift, js_arraylike_unshift,
};
pub(crate) use self::header::{
    array_has_arguments_object_flag, mark_array_as_arguments_object,
    rebuild_array_numeric_raw_f64_allow_holes,
};
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
pub(crate) use self::indexing::{
    array_has_own_index, array_iteration_is_exotic, array_proto_iterator_modified,
    array_prototype_addr, array_prototype_has_index_flag, array_spec_get, array_spec_has_index,
    keys_array_len_capped_to_capacity, note_array_proto_iterator_write,
    note_object_prototype_index_write, object_prototype_addr, object_prototype_addr_matches,
    object_prototype_has_index_flag,
};
pub use self::indexing::{
    js_array_get_element, js_array_get_element_f64, js_array_get_f64, js_array_get_f64_unchecked,
    js_array_get_index_or_string, js_array_get_length, js_array_length,
    js_array_numeric_get_f64_unboxed, js_array_numeric_set_f64_unboxed, js_array_set_f64,
    js_array_set_f64_extend, js_array_set_f64_extend_strict, js_array_set_f64_unchecked,
    js_array_set_index_or_string, js_array_set_index_or_string_strict, js_array_set_string_key,
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
pub(crate) use self::iterator::iter_bt_dump;
pub use self::iterator::{
    js_array_spread_append, js_for_of_to_array, js_get_async_iterator, js_iterator_to_array,
};
pub(crate) use self::sort::object_prototype_has_index_prop;
pub(crate) use self::sort::object_prototype_index_get as sort_object_prototype_index_get;
pub use self::subclass::{
    array_subclass_dense_snapshot, array_subclass_has_iterator_override, is_array_subclass_instance,
};
// Issue #1572 — flatten helpers reused by `node_stream::ns_iter_flat_map`
// so an `async function*` mapper return is driven through the iterator
// protocol instead of being appended as a single chunk.
pub(crate) use self::iterator::{
    async_from_sync_wrap_iterator, async_iterator_to_array_for_flat_map,
    call_symbol_async_iterator_for_flat_map, entries_array_for_small_handle_id, has_iterator_next,
    sync_iterator_to_array_if_not_async,
};
pub use self::jsvalue_api::{
    js_array_from_jsvalue, js_array_get, js_array_get_jsvalue, js_array_push,
    js_array_push_jsvalue, js_array_set, js_array_set_jsvalue, js_array_set_jsvalue_extend,
};
pub(crate) use self::push_pop::guard_writable_length;
pub use self::push_pop::{
    js_array_delete, js_array_grow, js_array_numeric_push_f64_unboxed, js_array_pop_f64,
    js_array_push_f64, js_array_push_hole, js_array_push_spread_f64, js_array_set_length,
    js_array_set_length_strict, js_array_shift_f64, js_array_unshift_f64, js_array_unshift_jsvalue,
    js_array_unshift_variadic,
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
pub(crate) use self::flat_clone::flattenable_array_ptr;
pub(crate) use self::header::{
    array_byte_size, array_is_frozen, array_is_sealed_or_no_extend, array_named_property_delete,
    array_named_property_get, array_named_property_get_by_name, array_named_property_has,
    array_named_property_names, array_named_property_set, array_numeric_raw_f64_get,
    array_numeric_raw_f64_push_inbounds, array_numeric_raw_f64_set_inbounds, array_object_flags,
    array_ptr_as_proxy, canonicalize_array_numeric_store_value, clean_arr_ptr, clean_arr_ptr_mut,
    clear_array_numeric_layout, clear_array_numeric_layout_ptr, gc_element_slot_range,
    mark_array_layout_unknown, mark_array_raw_f64_holes_fresh, normalize_array_receiver,
    note_array_slot, note_array_slot_layout_only, rebuild_array_layout, rebuild_array_layout_exact,
    refresh_array_numeric_layout, replay_array_growth_write_barriers, set_array_numeric_layout,
    store_array_slot, transfer_array_numeric_layout, value_bits_to_number, NumericArrayLayout,
    MIN_ARRAY_CAPACITY,
};

#[cfg(test)]
pub(crate) use self::header::{test_seed_template_raw_roots, test_template_raw_roots};
