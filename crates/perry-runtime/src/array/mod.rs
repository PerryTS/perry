//! Array representation for Perry — split into topical sub-modules.
mod alloc;
mod concat_reverse;
mod flat_clone;
mod header;
mod immutable;
mod indexing;
mod is_array;
mod iter_methods;
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
    js_array_alloc_with_length_longlived, js_array_create, js_array_from_f64,
};
pub use self::concat_reverse::{
    js_array_concat, js_array_concat_new, js_array_fill, js_array_fill_range, js_array_reverse,
};
pub use self::flat_clone::{
    js_array_clone, js_array_entries, js_array_flat, js_array_flat_depth, js_array_keys,
    js_array_values,
};
pub use self::header::{
    js_tagged_template_register_raw, js_template_raw, scan_template_raw_roots,
    scan_template_raw_roots_mut, ArrayHeader,
};
pub use self::immutable::{
    js_array_copy_within, js_array_to_reversed, js_array_to_sorted_default,
    js_array_to_sorted_with_comparator, js_array_to_spliced, js_array_with,
};
pub use self::indexing::{
    js_array_get_element, js_array_get_element_f64, js_array_get_f64, js_array_get_f64_unchecked,
    js_array_get_length, js_array_length, js_array_set_f64, js_array_set_f64_extend,
    js_array_set_f64_unchecked, js_array_set_index_or_string, js_array_set_string_key,
};
pub use self::is_array::js_array_is_array;
pub use self::iter_methods::{
    js_array_at, js_array_every, js_array_filter, js_array_find, js_array_findIndex,
    js_array_find_last, js_array_find_last_index, js_array_flatMap, js_array_forEach,
    js_array_join, js_array_map, js_array_map_discard, js_array_reduce, js_array_some,
};
pub use self::iterator::js_iterator_to_array;
pub use self::jsvalue_api::{
    js_array_from_jsvalue, js_array_get, js_array_get_jsvalue, js_array_push,
    js_array_push_jsvalue, js_array_set, js_array_set_jsvalue, js_array_set_jsvalue_extend,
};
pub use self::push_pop::{
    js_array_delete, js_array_grow, js_array_pop_f64, js_array_push_f64, js_array_push_spread_f64,
    js_array_set_length, js_array_shift_f64, js_array_unshift_f64, js_array_unshift_jsvalue,
};
pub use self::reduce_right::js_array_reduce_right;
pub use self::search::{
    js_array_includes_f64, js_array_includes_jsvalue, js_array_indexOf_f64,
    js_array_indexOf_jsvalue,
};
pub use self::sort::{js_array_sort_default, js_array_sort_with_comparator};
pub use self::splice_slice::{js_array_slice, js_array_splice};

pub(crate) use self::alloc::{js_array_from_arraylike, js_array_from_string_codepoints};
pub(crate) use self::header::{
    array_byte_size, clean_arr_ptr, clean_arr_ptr_mut, gc_element_slot_range,
    mark_array_layout_unknown, note_array_slot, rebuild_array_layout, rebuild_array_layout_exact,
    replay_array_growth_write_barriers, MIN_ARRAY_CAPACITY,
};

#[cfg(test)]
pub(crate) use self::header::{test_seed_template_raw_roots, test_template_raw_roots};
