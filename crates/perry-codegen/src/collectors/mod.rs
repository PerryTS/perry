//! Basic AST walkers for collecting closures, extern func refs, let ids,
//! and ref ids from HIR statements and expressions.
//!
//! Split from a single 6,428-line file into topical sub-modules in
//! v0.5.1019 to satisfy the file-size CI gate. mod.rs is a re-export
//! hub — public-API shape (`crate::collectors::*`) is preserved.

mod clamp_detect;
mod class_accessors;
mod closures;
mod escape_arrays;
mod escape_check;
mod escape_news;
mod escape_objects;
mod hir_facts;
mod i32_locals;
mod i64_emit;
mod index_uses;
mod integer_locals;
mod local_refs;
mod mutation;
mod pointer_locals;
mod refs;
mod scalar_method_dispatch;
mod scalar_methods;
mod shadow_slots;
mod this_as_value;
mod uppercase_strings;

// Public re-exports for the visible API (`pub fn emit_i64_function` etc.).
pub use clamp_detect::{
    detect_clamp3, detect_clamp_u8, is_integer_specializable, returns_i32_identity_arg,
    returns_integer,
};
pub use i64_emit::emit_i64_function;

// Internal-to-crate re-exports — explicit names because globs don't
// transitively expose through `pub(crate) use crate::collectors::*`.
pub(crate) use class_accessors::{is_class_getter, is_class_setter};
pub(crate) use closures::{collect_closures_in_expr, collect_closures_in_stmts};
pub(crate) use escape_arrays::{
    check_array_escapes_in_expr, check_array_escapes_in_stmts, collect_non_escaping_arrays,
    const_index, find_array_candidates, MAX_SCALAR_OBJECT_FIELDS,
};
pub(crate) use escape_check::{check_escapes_in_expr, check_escapes_in_stmts, find_new_candidates};
pub(crate) use escape_news::{
    collect_non_escaping_new_used_fields, collect_non_escaping_news, MAX_SCALAR_ARRAY_LEN,
};
pub(crate) use escape_objects::{
    check_object_literal_escapes_in_expr, check_object_literal_escapes_in_stmts,
    collect_non_escaping_object_literals, find_object_literal_candidates,
};
pub(crate) use hir_facts::{
    collect_hir_facts, collect_native_region_fact_graph, collect_type_facts, NativeRegionFactGraph,
};
pub(crate) use i32_locals::{
    collect_integer_let_ids, collect_localset_ids_in_expr_filtered, collect_localset_ids_in_stmts,
    collect_localset_ids_in_stmts_filtered, collect_strictly_i32_bounded_locals,
    collect_unsigned_i32_locals, is_bitwise_expr, is_flat_const_indexget,
    is_strictly_i32_bounded_expr, is_ushr_zero, walk_writes_for_strict,
    walk_writes_in_expr_for_strict,
};
pub(crate) use i64_emit::{i64_body, i64_cond, i64_val};
pub(crate) use index_uses::{
    absorb_writes_in_expr, absorb_writes_into_index_used, collect_index_used_locals,
    collect_localsets_in_expr_for_propagate, propagate_index_used_transitive,
    walk_index_uses_in_expr, walk_index_uses_in_stmts,
};
pub(crate) use integer_locals::{
    collect_extra_integer_let_ids, collect_flat_row_aliases, collect_integer_locals,
    is_int32_producing_expr,
};
pub(crate) use local_refs::{expr_contains_local_get, mark_all_candidate_refs_in_expr};
pub(crate) use mutation::has_any_mutation;
pub(crate) use pointer_locals::collect_pointer_typed_locals;
pub(crate) use refs::{
    collect_let_ids, collect_ref_ids_in_expr, collect_ref_ids_in_stmts, is_clamp_call,
};
pub(crate) use scalar_method_dispatch::{
    collect_module_dispatch_facts, mark_unstable_scalar_method_receivers, ModuleDispatchFacts,
};
pub(crate) use scalar_methods::simple_scalar_method_summary;
pub(crate) use shadow_slots::{
    collect_declared_shadow_slots_in_stmts, collect_shadow_slot_clear_points,
};
pub(crate) use this_as_value::{
    class_chain_extends_builtin_error, class_chain_has_unmodeled_base, class_uses_this_as_value,
};
