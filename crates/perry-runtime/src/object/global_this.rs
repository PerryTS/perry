//! `globalThis` singleton plus built-in constructor/namespace population.

use super::*;

#[path = "global_this_webassembly.rs"]
mod global_this_webassembly;

// Topical sub-modules split out of the original monolithic `global_this.rs`
// (pure code move; see the per-module re-exports below for the resolving paths).

mod array_error;
mod bigint_promise;
mod builtin_thunks;
mod ctor_thunks;
mod fetch_globals;
mod generator;
mod install_static;
mod math_temporal;
mod populate;
mod proto_methods;
mod typed_array;

pub(crate) use array_error::{
    array_proto_at_thunk, array_proto_join_thunk, array_prototype_concat_thunk,
    array_prototype_pop_thunk, array_prototype_push_thunk, array_prototype_reverse_thunk,
    array_prototype_shift_thunk, array_prototype_slice_thunk, array_prototype_sort_thunk,
    array_prototype_splice_thunk, array_prototype_unshift_thunk, date_prototype_to_string_thunk,
    error_prototype_to_string_thunk, function_prototype_apply_thunk, function_prototype_bind_thunk,
    function_prototype_call_thunk, function_prototype_to_string_thunk,
    global_this_clear_immediate_thunk, global_this_clear_interval_thunk,
    global_this_clear_timeout_thunk, global_this_queue_microtask_thunk,
    global_this_rest_array_values, global_this_set_immediate_thunk, global_this_set_interval_thunk,
    global_this_set_timeout_thunk, is_native_error_subclass_constructor,
    object_prototype_define_getter_thunk, object_prototype_define_setter_thunk,
    object_prototype_has_own_property_thunk, object_prototype_is_prototype_of_thunk,
    object_prototype_lookup_getter_thunk, object_prototype_lookup_setter_thunk,
    object_prototype_property_is_enumerable_thunk, object_prototype_to_locale_string_thunk,
    object_prototype_to_string_thunk, object_prototype_value_of_thunk,
};
pub(crate) use bigint_promise::{
    array_from_thunk, array_is_array_thunk, array_of_thunk, bigint_as_int_n_thunk,
    bigint_as_n_dispatch, bigint_as_uint_n_thunk, json_is_raw_json_thunk, json_parse_thunk,
    json_raw_json_thunk, json_stringify_thunk, number_is_finite_thunk, number_is_integer_thunk,
    number_is_nan_thunk, number_is_safe_integer_thunk, number_parse_float_thunk,
    number_parse_int_thunk, object_assign_thunk, object_create_thunk,
    object_define_properties_thunk, object_define_property_thunk, object_entries_thunk,
    object_freeze_thunk, object_from_entries_thunk, object_get_own_property_descriptor_thunk,
    object_get_own_property_descriptors_thunk, object_get_own_property_names_thunk,
    object_get_own_property_symbols_thunk, object_get_prototype_of_thunk, object_group_by_thunk,
    object_hasown_thunk, object_is_extensible_thunk, object_is_frozen_thunk,
    object_is_sealed_thunk, object_is_thunk, object_keys_thunk, object_prevent_extensions_thunk,
    object_seal_thunk, object_set_prototype_of_thunk, object_values_thunk,
    promise_static_function_spec, reflect_apply_thunk, string_from_char_code_static,
    string_from_code_point_static, string_raw_static, symbol_for_thunk, symbol_key_for_thunk,
    typed_array_from_thunk, typed_array_of_thunk,
};
pub use bigint_promise::{js_bigint_as_int_n_call, js_bigint_as_uint_n_call};
pub use builtin_thunks::js_function_ctor_from_strings;
pub(crate) use builtin_thunks::{
    global_this_array_thunk, global_this_atob_thunk, global_this_boolean_thunk,
    global_this_btoa_thunk, global_this_decode_uri_component_thunk, global_this_decode_uri_thunk,
    global_this_encode_uri_component_thunk, global_this_encode_uri_thunk,
    global_this_error_capture_stack_trace_thunk, global_this_error_is_error_thunk,
    global_this_error_prepare_stack_trace_thunk, global_this_escape_thunk, global_this_gc_thunk,
    global_this_is_finite_thunk, global_this_is_nan_thunk, global_this_number_thunk,
    global_this_object_thunk, global_this_parse_float_thunk, global_this_parse_int_thunk,
    global_this_string_thunk, global_this_structured_clone_thunk, global_this_unescape_thunk,
    math_atan2_thunk, math_clz32_thunk, math_f16round_thunk, math_hypot_thunk, math_imul_thunk,
    math_max_thunk, math_min_thunk, math_pow_thunk, math_random_thunk, math_round_thunk,
    math_sign_thunk,
};
pub use ctor_thunks::js_webcrypto_illegal_constructor;
pub(crate) use ctor_thunks::{
    builtin_prototype_value, cryptokey_algorithm_getter_thunk, cryptokey_extractable_getter_thunk,
    cryptokey_type_getter_thunk, cryptokey_usages_getter_thunk, error_constructor_call_thunk,
    eval_error_constructor_call_thunk, global_this_crypto_getter_thunk,
    global_this_url_pattern_call_thunk, is_function_prototype_object_value,
    map_constructor_call_thunk, normalize_eval_this_body, promise_constructor_call_thunk,
    range_error_constructor_call_thunk, reference_error_constructor_call_thunk,
    set_constructor_call_thunk, subtle_crypto_method_value, syntax_error_constructor_call_thunk,
    type_error_constructor_call_thunk, typed_array_constructor_call_thunk,
    uri_error_constructor_call_thunk, weak_map_constructor_call_thunk,
    weak_ref_constructor_call_thunk, weak_set_constructor_call_thunk,
    webcrypto_get_random_values_thunk, webcrypto_illegal_constructor_thunk, webcrypto_method_value,
    webcrypto_random_uuid_thunk, webcrypto_subtle_getter_thunk,
};
#[cfg(feature = "temporal")]
pub(crate) use fetch_globals::temporal_subclass_super;
pub(crate) use fetch_globals::{
    attach_fetch_handle_for_construction, global_this_blob_thunk, global_this_builtin_noop_thunk,
    global_this_date_thunk, global_this_eval_thunk, global_this_file_thunk,
    global_this_headers_thunk, global_this_request_thunk, global_this_response_error_thunk,
    global_this_response_json_thunk, global_this_response_redirect_thunk,
    global_this_response_thunk,
};
pub use fetch_globals::{
    js_fetch_or_value_super, js_get_global_this, js_global_or_console_property_by_name,
    js_module_top_this, js_request_subclass_init, js_response_subclass_init,
};
pub(crate) use generator::{
    ensure_generator_intrinsics, generator_function_constructor_of, generator_function_proto_of,
    generator_function_prototype_of, set_intrinsic_data_prop, set_intrinsic_to_string_tag,
};
pub use generator::{js_generator_attach_closure_prototype, js_generator_attach_prototype};
pub use install_static::js_promise_static_function_value;
pub(crate) use install_static::{
    install_atomics_namespace_members, install_builtin_constructor_statics,
    install_constructor_static, install_constructor_static_with_call_arity,
    install_json_namespace_members, install_noop_proto_methods,
    install_number_static_data_properties, install_proto_method, install_proto_method_alias,
    install_proto_method_rest, install_proto_method_rest_with_length,
    install_reflect_namespace_members, subtle_crypto_decapsulate_bits_thunk,
    subtle_crypto_decapsulate_key_thunk, subtle_crypto_encapsulate_bits_thunk,
    subtle_crypto_encapsulate_key_thunk, url_pattern_exec_thunk, url_pattern_test_thunk,
};
#[cfg(feature = "temporal")]
pub(crate) use math_temporal::install_temporal_namespace;
#[cfg(feature = "temporal")]
pub(crate) use math_temporal::temporal_kind_prototype;
pub(crate) use math_temporal::{install_math_namespace, temporal_ctor_kind};
pub(crate) use populate::{
    default_prepare_stack_trace_func_ptr, populate_global_this_builtins, ERROR_CONSTRUCTOR_PTR,
};
pub(crate) use proto_methods::{
    install_error_prototype_data_properties, populate_builtin_prototype_methods,
};
pub(crate) use typed_array::{
    array_buffer_byte_length_getter_thunk, array_buffer_is_view_thunk,
    ensure_typed_array_intrinsic, install_function_has_instance_symbol,
    shared_array_buffer_byte_length_getter_thunk, shared_array_buffer_slice_thunk,
    typed_array_constructor_this_kind, typed_array_intrinsic_proto_ptr,
};
