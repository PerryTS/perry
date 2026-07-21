//! Class method vtable registry — enables runtime dispatch for
//! interface-typed and dynamically-typed method calls. Each class
//! registers its methods, getters, and setters at startup;
//! `js_native_call_method` / `js_dynamic_object_get_property` look up
//! the vtable by the object's `class_id` when static dispatch isn't
//! possible. Also home for the per-callsite inline cache
//! (`vtable_ic_*` / `call_vtable_method`) and the parent-chain
//! registration helpers used by codegen.
//!
//! Split out of `object/mod.rs` (issue #1103). Pure relocation — no
//! logic changes.
//!
//! Further split into topical sub-modules (chore: split-large-files). The
//! implementation lives in the sibling modules declared below; this trunk
//! keeps the shared `class_handles` re-export and re-exports every item that
//! other modules reach via `crate::object::class_registry::<name>` (or via the
//! `pub use class_registry::*` glob in `object/mod.rs`). Pure relocation.

pub use super::class_handles::{
    event_emitter_async_resource_handle_probe, event_emitter_get_domain,
    event_emitter_handle_probe, event_emitter_on, event_emitter_set_domain,
    fetch_handle_kind_probe, handle_method_dispatch, handle_own_property_names_dispatch,
    handle_property_dispatch, handle_property_set_dispatch, handle_prototype_dispatch,
    js_register_event_emitter_async_resource_handle_probe, js_register_event_emitter_get_domain,
    js_register_event_emitter_handle_probe, js_register_event_emitter_on,
    js_register_event_emitter_set_domain, js_register_fetch_handle_kind_probe,
    js_register_handle_method_dispatch, js_register_handle_own_property_names_dispatch,
    js_register_handle_property_dispatch, js_register_handle_property_set_dispatch,
    js_register_handle_prototype_dispatch, js_register_net_socket_handle_probe,
    js_register_stream_expando_set, js_register_stream_handle_kind_probe,
    js_register_stream_handle_probe, net_socket_handle_probe, stream_expando_set,
    stream_handle_kind_probe, stream_handle_probe, EventEmitterAsyncResourceHandleProbeFn,
    EventEmitterGetDomainFn, EventEmitterHandleProbeFn, EventEmitterOnFn, EventEmitterSetDomainFn,
    FetchHandleKindProbeFn, HandleMethodDispatchFn, HandleOwnPropertyNamesDispatchFn,
    HandlePropertyDispatchFn, HandlePropertySetDispatchFn, HandlePrototypeDispatchFn,
    NetSocketHandleProbeFn, StreamHandleKindProbeFn, StreamHandleProbeFn,
};
use super::*;

mod class_meta;
mod construct;
mod dispatch;
mod gc_roots;
mod parent_static;
mod prototype_methods;
mod prototype_objects;
mod registration;
mod state;

// ── state.rs ────────────────────────────────────────────────────────────────
pub(crate) use state::{
    class_decl_prototype_object, class_decl_prototype_value,
    class_decl_prototype_value_for_instance_class, class_delete_own_dynamic_prop,
    class_dynamic_prop_root_store, class_has_own_dynamic_prop, class_id_for_decl_prototype_object,
    class_is_key_deleted, class_mark_key_deleted, class_object_value_for_cid,
    class_object_value_root_store, class_own_enumerable_field_names, class_own_static_field_value,
    class_parent_closure, class_parent_closure_root_store, class_prototype_method_is_enumerable,
    class_prototype_method_set_enumerable, class_prototype_method_value_cache_root_store,
    class_prototype_object_root_store, global_object_prototype_bits,
    is_bound_native_method_closure_value, is_non_constructable_builtin_function_value,
    parent_closure_in_chain, throw_non_constructable_builtin_function,
};
pub use state::{
    ClassVTable, VTableMethodEntry, CLASS_DECL_PROTOTYPE_OBJECTS, CLASS_DYNAMIC_PARENT_VALUE,
    CLASS_METHOD_BIND_LENGTHS, CLASS_OBJECT_VALUES, CLASS_PARENT_CLOSURES,
    CLASS_PROTOTYPE_METHOD_NONENUM, CLASS_PROTOTYPE_OBJECTS, CLASS_STATIC_ACCESSORS,
    CLASS_STATIC_METHODS, CLASS_STATIC_METHOD_BIND_LENGTHS, CLASS_SYMBOL_ACCESSORS,
    CLASS_SYMBOL_METHODS, CLASS_VTABLE_REGISTRY, FUNCTION_CLASS_IDS, REGISTERED_CLASS_IDS,
};

// ── prototype_objects.rs ────────────────────────────────────────────────────
pub(crate) use prototype_objects::{
    class_prototype_object, ensure_function_prototype_object, function_class_id,
    function_value_for_class_id, resolve_proto_chain_field,
    resolve_proto_chain_field_with_receiver, resolve_proto_chain_symbol,
};
pub use prototype_objects::{js_set_function_prototype, NEXT_SYNTHETIC_CLASS_ID};

// ── class_meta.rs ───────────────────────────────────────────────────────────
#[cfg(test)]
pub(crate) use class_meta::test_text_encoding_stream_new_with_constructor;
pub use class_meta::{
    class_name_for_id, is_anon_shape_class_id, js_compression_stream_new,
    js_decompression_stream_new, js_register_anon_shape_class_id, js_register_class_id,
    js_register_class_name, js_text_decoder_stream_new, js_text_encoder_stream_new,
    js_text_encoding_stream_new, ANON_SHAPE_CLASS_IDS, CLASS_NAMES,
};
pub(crate) use class_meta::{
    identify_global_builtin_constructor, report_dispatch_miss, text_decoder_bool_option,
    text_encoding_stream_new_with_constructor, validate_web_compression_stream_format,
    CLASS_ID_COMPRESSION_STREAM, CLASS_ID_DECOMPRESSION_STREAM, CLASS_ID_TEXT_DECODER_STREAM,
    CLASS_ID_TEXT_ENCODER_STREAM,
};
#[cfg(test)]
pub(crate) use prototype_methods::CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED;
#[cfg(test)]
pub(crate) use state::CLASS_DELETED_KEYS;

// ── prototype_methods.rs ────────────────────────────────────────────────────
pub(crate) use prototype_methods::{
    class_prototype_fast_guards_invalidated, class_prototype_method_root_store,
    invalidate_class_prototype_fast_guards, mirror_prototype_method_on_object,
    synthetic_class_id_for_function,
};
pub use prototype_methods::{
    js_class_register_static_field, js_get_function_prototype_method,
    js_register_function_prototype_method, js_register_prototype_method, CLASS_PROTOTYPE_METHODS,
};

// ── construct.rs ────────────────────────────────────────────────────────────
pub(crate) use construct::{
    extends_target_must_throw, function_would_have_own_prototype, is_callable_function_value,
    js_value_is_constructor, lookup_prototype_method, nm_ctor_fs, nm_ctor_readline, nm_ctor_repl,
    nm_ctor_stream, nm_ctor_tls, nm_ctor_tty, nm_ctor_vm, nm_ctor_wasi,
    ordinary_function_prototype_value_for_read, promise_parent_in_chain,
};
pub use construct::{
    js_ctor_return_override, js_function_prototype_value_for_read, js_new_function_construct,
    js_new_function_construct_apply, js_new_function_construct_with_new_target,
    js_new_target_value,
};

// ── gc_roots.rs ─────────────────────────────────────────────────────────────
pub(crate) use gc_roots::{
    new_class_side_table_root_scan_state, scan_class_side_table_roots_mut_step,
};
pub use gc_roots::{scan_class_side_table_roots, scan_class_side_table_roots_mut};
#[cfg(test)]
pub(crate) use gc_roots::{
    test_class_dynamic_prop_root_bits, test_class_parent_closure_root_addr,
    test_class_prototype_method_root_bits, test_class_prototype_method_value_root_bits,
    test_class_prototype_object_root_addr, test_clear_class_side_table_roots,
    test_function_class_id_key_for_class, test_seed_class_dynamic_prop_root,
    test_seed_class_parent_closure_root, test_seed_class_prototype_method_root,
    test_seed_class_prototype_method_value_root, test_seed_class_prototype_object_root,
    test_seed_function_class_id_key,
};

// ── registration.rs ─────────────────────────────────────────────────────────
pub(crate) use registration::{
    class_accessor_function_value, class_own_accessor_ptrs, class_own_static_accessor_ptrs,
};
pub use registration::{
    is_class_id_registered, js_register_class_getter, js_register_class_method,
    js_register_class_method_bind_length, js_register_class_setter,
    js_register_class_static_getter, js_register_class_static_method_bind_length,
    js_register_class_static_setter,
};

// ── dispatch.rs ─────────────────────────────────────────────────────────────
#[cfg(test)]
pub(crate) use dispatch::test_bump_vtable_generation;
pub(crate) use dispatch::{
    call_vtable_method, fetch_parent_kind_in_chain, vtable_generation, vtable_ic_insert,
    vtable_ic_lookup, VTABLE_GEN,
};

// ── parent_static.rs ────────────────────────────────────────────────────────
pub(crate) use parent_static::{
    call_registered_static_method, call_static_method, class_chain_has_instance_accessor,
    class_has_instance_getter, class_has_own_static_method, class_has_symbol_member_in_chain,
    class_instance_setter_apply, class_method_bind_length, class_object_own_field_bytes,
    class_object_pinned_parent, class_own_symbol_member_keys, class_static_accessor_getter_value,
    class_static_accessor_setter_apply, class_symbol_getter_value, class_symbol_setter_apply,
    get_parent_class_id, lookup_class_symbol_method_in_chain, lookup_static_method_in_chain,
    register_class,
};
pub use parent_static::{
    is_class_object_ptr, is_class_object_value, is_registered_class_prototype_object,
    js_class_static_method_call, js_get_dynamic_parent_value, js_object_mark_class,
    js_register_class_computed_accessor, js_register_class_computed_method,
    js_register_class_parent, js_register_class_parent_dynamic, js_register_class_static_method,
    lookup_class_method_in_chain, method_owner_class_id,
};
