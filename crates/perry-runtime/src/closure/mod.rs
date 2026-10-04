//! Closure runtime support for Perry
//!
//! A closure is a function pointer plus captured environment.
//! Layout:
//!   - ClosureHeader at the start
//!   - Followed by captured values (as f64 or i64 pointers)

mod alloc;
pub(crate) mod body_call;
mod dispatch;
mod dynamic_props;
pub(crate) mod props;
mod registry;
pub(crate) mod shape;
mod unbox;
mod v8_stubs;
mod wide_call;

#[cfg(test)]
mod receiver_param_tests;
#[cfg(test)]
mod tests;

pub use alloc::{
    closure_alloc_storage, closure_capture_slots_mut, closure_payload_size, js_closure_alloc,
    js_closure_alloc_singleton, js_closure_alloc_with_captures_singleton,
    js_closure_get_capture_bits, js_closure_get_capture_f64, js_closure_get_capture_ptr,
    js_closure_get_func, js_closure_set_box_capture_ptr, js_closure_set_capture_bits,
    js_closure_set_capture_f64, js_closure_set_capture_ptr, note_closure_capture_slot,
    rebuild_closure_layout_and_barriers, scan_singleton_closure_roots_mut, ClosureHeader,
    CLOSURE_ALLOC_COUNT, CLOSURE_CAP_SINGLETON_HIT, CLOSURE_CAP_SINGLETON_MISS,
    CLOSURE_SHAPE_OFFSET,
};
pub(crate) use alloc::{
    closure_install_boxed_captures, gc_capture_slot_range, singleton_closure_if_cached,
};
pub use body_call::{call_value, plain_call_receiver, JsThis};
pub use shape::closure_kind_probe;

pub(crate) use registry::DispatchKind;
pub(crate) use wide_call::dispatch_wide_abi;

/// `PERRY_GC_CENSUS`: every closure-keyed side table outside the registries.
pub(crate) fn closure_side_table_census() -> Vec<crate::gc::census::SideTableRow> {
    let mut rows = alloc::singleton_closure_census();
    rows.extend(dynamic_props::dynamic_props_census());
    rows
}
pub use registry::{
    build_rest_array, build_rest_array_rooted, closure_arity, closure_info, closure_is_arrow,
    closure_is_bound_method, closure_length, dispatch_rest_bundled, dispatch_with_arity,
    info_arity, info_has, info_length, info_rest, real_capture_count, resolve_strategy,
    DispatchStrategy, JsFunctionInfo, RestDispatchKind, BOUND_FUNCTION_FUNC_PTR,
    BOUND_FUNCTION_INFO, BOUND_METHOD_FUNC_PTR, BOUND_METHOD_INFO, CAPTURES_THIS_FLAG, FN_ARROW,
    FN_ASYNC, FN_ASYNC_GENERATOR, FN_BUILTIN, FN_GENERATOR, FN_HAS_DECLARED, FN_HAS_LENGTH,
    FN_NON_CONSTRUCTOR, FN_REST_MASK, FN_REST_SYNTHETIC_ARGUMENTS, FN_REST_USER,
    FN_REST_USER_AND_ARGUMENTS, FN_STRICT, NO_THIS_REBIND_FLAG,
};
pub(crate) use registry::{
    closure_body_is_non_constructor, info_receives_primitive_this, info_trusted_direct,
    info_versioned_loop_direct,
};

pub(crate) use dispatch::{
    bound_function_lazy_name, bound_function_length, bound_method_source_func_ptr,
    coerce_call_this, rebind_explicit_this, rebind_explicit_this_allocates,
    reify_function_method_value, reset_throw_not_callable_counter,
};
pub(crate) use dispatch::{
    call_compiled_body_this, call_compiled_closure_this, native_call_value_this,
};
pub use dispatch::{
    clean_closure_ptr, dispatch_bound_function, dispatch_bound_method, get_valid_func_ptr,
    get_valid_info, js_closure_call0, js_closure_call1, js_closure_call10, js_closure_call11,
    js_closure_call12, js_closure_call13, js_closure_call14, js_closure_call15, js_closure_call16,
    js_closure_call2, js_closure_call3, js_closure_call4, js_closure_call5, js_closure_call6,
    js_closure_call7, js_closure_call8, js_closure_call9, js_closure_call_apply_with_spread,
    js_closure_call_array, js_function_bind, js_native_call_value, throw_not_callable, DirectCall1,
    DirectCall2, DirectCall3, DirectCall4,
};
pub use unbox::{js_closure_unbox_callee_checked, js_closure_unbox_callee_checked_rebind};

#[cfg(feature = "wasm-host")]
pub(crate) use dynamic_props::register_wasm_funcref_external;
#[cfg(test)]
pub(crate) use dynamic_props::test_clear_closure_side_tables;
pub(crate) use dynamic_props::{
    clear_closure_side_tables_for_dead_ptr, clone_closure_rebind_this,
    closure_dynamic_props_owner_moved, closure_dynamic_side_tables_nonempty,
    closure_get_dynamic_prop_keyed, closure_reads_this_from_capture,
    closure_set_via_function_prototype_descriptor, function_prototype_fallback_target,
    function_prototype_inherited_get, prune_dead_closure_side_table_owners,
    prune_dead_closure_side_table_owners_young, release_closure_side_table_owners_in_ranges,
};
pub use dynamic_props::{
    closure_delete_own_dynamic_prop, closure_dynamic_props_snapshot, closure_get_dynamic_prop,
    closure_get_own_dynamic_prop, closure_has_own_dynamic_prop, closure_is_key_deleted,
    closure_mark_key_deleted, closure_set_dynamic_prop, closure_set_static_prototype,
    closure_static_prototype, is_closure_ptr, scan_closure_dynamic_props_roots_mut,
};

// v8_stubs re-exports the AOT stubs + non-macOS Rust V8-interop stubs.
#[cfg(not(target_os = "macos"))]
pub use v8_stubs::{
    js_await_js_promise, js_call_function, js_create_callback, js_get_export, js_load_module,
    js_new_from_handle, js_new_instance, js_runtime_init, js_set_property,
};

pub use v8_stubs::{
    js_argon2_hash_options, js_lodash_ends_with, js_lodash_escape, js_lodash_includes,
    js_lodash_lower_first, js_lodash_replace, js_lodash_split, js_lodash_start_case,
    js_lodash_starts_with, js_lodash_unescape, js_lodash_upper_first, js_sharp_negate,
    js_sharp_quality, js_sharp_to_format,
};

#[cfg(test)]
pub(crate) use alloc::{
    test_captured_singleton_closure_cache_entries, test_clear_singleton_closure_caches,
    test_seed_captured_singleton_closure_cache, test_seed_singleton_closure_cache,
    test_singleton_closure_cache_entry,
};
