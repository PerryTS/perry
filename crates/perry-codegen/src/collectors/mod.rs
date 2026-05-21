//! Basic AST walkers for collecting closures, extern func refs, let ids,
//! and ref ids from HIR statements and expressions.
//!
//! Split from a single 6,428-line file into topical sub-modules in
//! v0.5.1019 to satisfy the file-size CI gate. mod.rs is a re-export
//! hub — public-API shape (`crate::collectors::*`) is preserved.

mod clamp_detect;
mod closures;
mod escape_arrays;
mod escape_news;
mod escape_objects;
mod i32_locals;
mod i64_emit;
mod index_uses;
mod integer_locals;
mod mutation;
mod pointer_locals;
mod refs;
mod shadow_slots;

// Public re-exports for the visible API (`pub fn emit_i64_function` etc.).
pub use clamp_detect::{detect_clamp3, detect_clamp_u8, is_integer_specializable, returns_integer};
pub use i64_emit::emit_i64_function;

// Internal-to-crate re-exports — explicit names because globs don't
// transitively expose through `pub(crate) use crate::collectors::*`.
pub(crate) use clamp_detect::{i64s_expr, i64s_stmts, returns_int_expr, returns_int_stmts};
pub(crate) use closures::{collect_closures_in_expr, collect_closures_in_stmts};
pub(crate) use escape_arrays::{
    check_array_escapes_in_expr, check_array_escapes_in_stmts, collect_non_escaping_arrays,
    const_index, find_array_candidates, MAX_SCALAR_OBJECT_FIELDS,
};
pub(crate) use escape_news::{
    check_escapes_in_expr, check_escapes_in_stmts, class_chain_extends_builtin_error,
    class_uses_this_as_value, collect_non_escaping_news, expr_contains_local_get,
    expr_uses_this_as_value, find_new_candidates, is_class_getter, is_class_setter,
    mark_all_candidate_refs_in_expr, stmts_use_this_as_value, MAX_SCALAR_ARRAY_LEN,
};
pub(crate) use escape_objects::{
    check_object_literal_escapes_in_expr, check_object_literal_escapes_in_stmts,
    collect_non_escaping_object_literals, find_object_literal_candidates,
};
pub(crate) use i32_locals::{
    collect_integer_let_ids, collect_localset_ids_in_expr_filtered, collect_localset_ids_in_stmts,
    collect_localset_ids_in_stmts_filtered, collect_non_int_localset_ids_in_stmts,
    collect_strictly_i32_bounded_locals, is_bitwise_expr, is_flat_const_indexget,
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
pub(crate) use mutation::{expr_has_mutation, has_any_mutation, is_local_get_chain};
pub(crate) use pointer_locals::collect_pointer_typed_locals;
pub(crate) use refs::{
    collect_let_ids, collect_ref_ids_in_expr, collect_ref_ids_in_stmts, is_clamp_call,
};
pub(crate) use shadow_slots::{
    collect_declared_shadow_locals_in_stmt, collect_declared_shadow_slots_in_stmts,
    collect_shadow_slot_clear_points,
};
