//! `globalThis` singleton plus built-in constructor/namespace population.

use super::*;

#[path = "global_this_webassembly.rs"]
mod global_this_webassembly;
pub(crate) use global_this_webassembly::{install_engine as install_wasm_engine, WasmEngine};
pub(crate) use global_this_webassembly::{
    is_registered_wasm_module, webassembly_error_ctor_instanceof, webassembly_value_ctor_instanceof,
};
// Only the WebAssembly engine constructs real modules (via
// `webassembly::make_module_object`).
pub(crate) use global_this_webassembly::{
    register_extern_wrapper as register_wasm_extern_wrapper,
    register_module_wrapper as register_wasm_module_wrapper, registered_extern_handle,
    registered_module_handle,
};

// Topical sub-modules split out of the original monolithic `global_this.rs`
// (pure code move; see the per-module re-exports below for the resolving paths).

mod array_error;
mod bigint_promise;
pub(super) mod builtin_thunks;
pub(super) mod ctor_thunks;
pub(super) mod fetch_globals;

/// `dyn-eval` install: every hub slot the script evaluator fills in this
/// module (see `crate::feature_hooks`).
#[cfg(feature = "dyn-eval")]
pub(crate) fn install_dyn_eval() {
    fetch_globals::install_global_eval();
}
mod async_generator_state;
mod generator;
mod generator_state;
mod install_static;
mod math_temporal;

// Complete namespace construction, reached through feature hooks.
mod builtin_namespace;
mod json_reflect_namespaces;
mod math_namespace;

mod object_intrinsic;
mod populate;
mod proto_methods;
pub(crate) mod proto_room;
mod typed_array;
pub(crate) mod web_hooks;
pub(crate) use typed_array::typed_array_uint8_intrinsic_prototype_value;

/// Is `func` the body of `%Object.prototype%.valueOf`? The dispatcher's
/// `valueOf` arm answers that body natively instead of calling it.
#[inline]
pub(crate) fn is_object_prototype_value_of_code(func: *const u8) -> bool {
    func == array_error::object_prototype_value_of_thunk as *const u8
}

/// Which `Function.prototype` intrinsic (`bind`/`call`/`apply`) has code
/// pointer `func`, if any: the identity the shape-proven method path compares
/// the prototype slot's VALUE against.
#[inline]
pub(crate) fn function_prototype_intrinsic_of(func: *const u8) -> Option<&'static str> {
    if func == array_error::function_prototype_bind_thunk as *const u8 {
        Some("bind")
    } else if func == array_error::function_prototype_call_thunk as *const u8 {
        Some("call")
    } else if func == array_error::function_prototype_apply_thunk as *const u8 {
        Some("apply")
    } else {
        None
    }
}

#[cfg(test)]
pub(crate) fn function_prototype_call_thunk_for_test() -> *const crate::closure::JsFunctionInfo {
    crate::fn_info!(native_args array_error::function_prototype_call_thunk, 1; with_flags(crate::closure::FN_BUILTIN))
}

#[cfg(test)]
pub(crate) fn function_prototype_bind_thunk_for_test() -> *const crate::closure::JsFunctionInfo {
    crate::fn_info!(native_args array_error::function_prototype_bind_thunk, 1; with_flags(crate::closure::FN_BUILTIN))
}

pub(crate) use array_error::{
    function_apply_proxy, function_prototype_apply_thunk, function_prototype_call_thunk,
    generic_array_like_to_vec, global_this_rest_array_values, is_native_error_subclass_constructor,
    object_prototype_has_own_property_thunk, object_prototype_is_prototype_of_thunk,
    object_prototype_property_is_enumerable_thunk, object_prototype_to_string_thunk,
};
pub(crate) use async_generator_state::is_async_generator_instance_value;
pub use async_generator_state::js_async_generator_create;
pub(crate) use bigint_promise::{
    bigint_as_n_dispatch, number_parse_float_thunk, number_parse_int_thunk,
    promise_static_function_spec,
};
pub use bigint_promise::{js_bigint_as_int_n_call, js_bigint_as_uint_n_call};
pub use builtin_thunks::js_function_ctor_from_strings;
pub(crate) use builtin_thunks::{
    global_this_array_thunk, global_this_error_prepare_stack_trace_thunk,
    global_this_parse_float_thunk, global_this_parse_int_thunk, js_math_round_value,
    ARRAY_CONSTRUCTOR_INFO,
};
pub use ctor_thunks::js_webcrypto_illegal_constructor;
pub(crate) use ctor_thunks::{
    builtin_prototype_value, is_array_prototype_method_value, is_function_prototype_object_value,
    normalize_eval_this_body, regexp_constructor_call_thunk, subtle_crypto_method_value,
    typed_array_constructor_call_thunk, webcrypto_method_value,
};
#[cfg(test)]
pub(crate) use fetch_globals::collect_before_global_this_alloc_for_test;
#[cfg(feature = "temporal")]
pub(crate) use fetch_globals::temporal_subclass_super;
pub(crate) use fetch_globals::{
    global_this_blob_thunk, global_this_builtin_noop_thunk, global_this_file_thunk,
    global_this_headers_thunk, global_this_is_materialized, global_this_request_thunk,
    global_this_response_thunk, is_thread_realm_global,
};
pub use fetch_globals::{
    js_fetch_or_value_super, js_get_global_this, js_global_or_console_property_by_name,
    js_module_top_this, js_request_subclass_init, js_response_subclass_init,
};
#[cfg(test)]
pub(crate) use generator::append_async_function_root_snapshot;
pub(crate) use generator::function_kind_prototype_unwritten;
pub(crate) use generator::{
    ensure_generator_intrinsics, generator_function_constructor_of, generator_function_proto_of,
    generator_function_prototype_of, generator_original_prototype, set_intrinsic_data_prop,
    set_intrinsic_to_string_tag, wire_function_intrinsic_parents,
};
pub(crate) use generator::{intrinsic_sync_step_kind, resume_intrinsic_sync_step};
pub use generator::{js_generator_attach_closure_prototype, js_generator_attach_prototype};
pub(crate) use generator_state::{
    call_generator_symbol_method, fixed_generator_state, install_class_symbol_method,
    install_evaluation_generator_symbols, STEP_DONE, STEP_INNER_RESULT,
};
pub(crate) use generator_state::{
    generator_observe_record_prototype, generator_original_prototype_key,
    generator_record_prototype_word,
};
pub use generator_state::{js_generator_create, js_generator_result, js_generator_set_slot};
pub use install_static::js_promise_static_function_value;
pub(crate) use install_static::{
    builtin_species_getter_thunk, install_atomics_namespace_members,
    install_builtin_constructor_statics, install_builtin_species_accessor,
    install_constructor_static, install_noop_proto_methods, install_proto_method,
    install_proto_method_alias, install_proto_method_rest, install_proto_method_rest_with_length,
    subtle_crypto_decapsulate_bits_thunk, subtle_crypto_decapsulate_key_thunk,
    subtle_crypto_encapsulate_bits_thunk, subtle_crypto_encapsulate_key_thunk, ConstructorStatics,
};
#[cfg(feature = "temporal")]
pub(crate) use math_temporal::install_temporal_namespace;
#[cfg(feature = "temporal")]
pub(crate) use math_temporal::temporal_ctor_kind_impl;
#[cfg(feature = "temporal")]
pub(crate) use math_temporal::temporal_kind_prototype;

pub(crate) use math_temporal::temporal_ctor_kind;
pub(crate) use object_intrinsic::{
    ensure_object_intrinsics, ensure_object_prototype_shape, object_intrinsics_for_realm,
    object_prototype_intrinsic_bits,
};
pub(crate) use populate::{
    default_prepare_stack_trace_func_ptr, populate_global_this_builtins,
    scan_error_constructor_root_mut, ERROR_CONSTRUCTOR_PTR,
};
pub(crate) use proto_methods::{
    install_error_prototype_data_properties, populate_builtin_prototype_methods,
};
pub(crate) use typed_array::{
    ensure_typed_array_intrinsic, install_function_has_instance_symbol,
    typed_array_constructor_this_kind, typed_array_intrinsic_proto_ptr,
    validate_array_buffer_species_constructor,
};

pub(crate) use array_error::array_prototype_values_thunk;

pub(crate) use install_static::install_proto_method_with_key;

#[cfg(test)]
pub(crate) use global_this_webassembly::owned_wrappers as test_wasm_wrapper_state;

// Existing unit fixtures invoke these bodies directly; production installers
// now name their owning module in the exported info declaration.
#[cfg(test)]
pub(crate) use array_error::{
    object_prototype_to_locale_string_thunk, object_prototype_value_of_thunk,
};
#[cfg(test)]
pub(crate) use typed_array::array_buffer_is_view_thunk;
