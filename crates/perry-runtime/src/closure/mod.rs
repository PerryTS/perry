//! Closure runtime support for Perry
//!
//! A closure is a function pointer plus captured environment.
//! Layout:
//!   - ClosureHeader at the start
//!   - Followed by captured values (as f64 or i64 pointers)

mod alloc;
mod dispatch;
mod dynamic_props;
mod registry;
mod unbox;
mod v8_stubs;

#[cfg(test)]
mod tests;

pub(crate) use alloc::gc_capture_slot_range;
pub use alloc::{
    closure_alloc_storage, closure_capture_slots_mut, closure_payload_size, js_closure_alloc,
    js_closure_alloc_singleton, js_closure_alloc_with_captures_singleton,
    js_closure_get_capture_f64, js_closure_get_capture_ptr, js_closure_get_func,
    js_closure_set_capture_f64, js_closure_set_capture_ptr, note_closure_capture_slot,
    rebuild_closure_layout_and_barriers, scan_singleton_closure_roots_mut, ClosureHeader,
    CLOSURE_ALLOC_COUNT, CLOSURE_CAP_SINGLETON_HIT, CLOSURE_CAP_SINGLETON_MISS,
};

pub use registry::{
    build_rest_array, closure_arity, closure_is_arrow, closure_is_bound_method, closure_length,
    dispatch_rest_bundled, dispatch_with_arity, is_registered_arrow_function,
    is_registered_async_function, is_registered_async_generator_function,
    is_registered_generator_function, is_registered_strict_function, js_register_closure_arity,
    js_register_closure_arrow_function, js_register_closure_async_function,
    js_register_closure_async_generator_function, js_register_closure_generator_function,
    js_register_closure_length, js_register_closure_rest, js_register_closure_rest_and_arguments,
    js_register_closure_strict_function, js_register_closure_synthetic_arguments,
    lookup_closure_arity, lookup_closure_length, lookup_closure_rest, lookup_closure_rest_full,
    real_capture_count, resolve_strategy, DispatchStrategy, BOUND_FUNCTION_FUNC_PTR,
    BOUND_METHOD_FUNC_PTR, CAPTURES_THIS_FLAG, CLOSURE_MAGIC, NO_THIS_REBIND_FLAG,
};

pub use dispatch::{
    clean_closure_ptr, dispatch_bound_function, dispatch_bound_method, get_valid_func_ptr,
    js_closure_call0, js_closure_call1, js_closure_call10, js_closure_call11, js_closure_call12,
    js_closure_call13, js_closure_call14, js_closure_call15, js_closure_call16, js_closure_call2,
    js_closure_call3, js_closure_call4, js_closure_call5, js_closure_call6, js_closure_call7,
    js_closure_call8, js_closure_call9, js_closure_call_apply_with_spread, js_closure_call_array,
    js_function_bind, js_native_call_value, throw_not_callable,
};
pub(crate) use dispatch::{
    coerce_call_this, rebind_explicit_this, reify_function_method_value,
    reset_throw_not_callable_counter, resolve_call2_direct,
};
pub use unbox::js_closure_unbox_callee_checked;

#[cfg(test)]
pub(crate) use dynamic_props::test_clear_closure_side_tables;
pub(crate) use dynamic_props::{
    clone_closure_rebind_this, closure_dynamic_props_owner_moved,
    visit_closure_dynamic_prop_value_slots_mut, visit_closure_static_prototype_slot_mut,
};
pub use dynamic_props::{
    closure_delete_own_dynamic_prop, closure_dynamic_props_snapshot, closure_get_dynamic_prop,
    closure_get_own_dynamic_prop, closure_has_own_dynamic_prop, closure_is_key_deleted,
    closure_mark_key_deleted, closure_set_dynamic_prop, closure_set_static_prototype,
    closure_static_prototype, is_closure_ptr, js_closure_unbind_this,
    scan_closure_dynamic_props_roots_mut,
};

// v8_stubs re-exports the AOT stubs + non-macOS Rust V8-interop stubs.
#[cfg(not(target_os = "macos"))]
pub use v8_stubs::{
    js_await_js_promise, js_call_function, js_create_callback, js_get_export, js_load_module,
    js_new_from_handle, js_new_instance, js_runtime_init, js_set_property,
};

pub use v8_stubs::{
    js_argon2_hash_options, js_axios_create, js_axios_request, js_lodash_ends_with,
    js_lodash_escape, js_lodash_includes, js_lodash_lower_first, js_lodash_replace,
    js_lodash_split, js_lodash_start_case, js_lodash_starts_with, js_lodash_unescape,
    js_lodash_upper_first, js_ratelimit_create, js_sharp_negate, js_sharp_quality,
    js_sharp_to_format,
};

#[cfg(test)]
pub(crate) use alloc::{
    test_captured_singleton_closure_cache_entries, test_clear_singleton_closure_caches,
    test_seed_captured_singleton_closure_cache, test_seed_singleton_closure_cache,
    test_singleton_closure_cache_entry,
};
