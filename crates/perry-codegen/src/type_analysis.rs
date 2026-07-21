//! Type analysis helpers for expression codegen.
//!
//! Pure predicates and type refinement that don't emit IR themselves.
//! Used by `expr.rs`, `lower_call.rs`, `lower_string_method.rs`,
//! `lower_conditional.rs`, and `stmt.rs`.

#[cfg(test)]
pub(crate) use crate::type_analysis_facts::{
    hir_inferred_refinable_type_from_facts, hir_inferred_refinable_type_from_locals,
    hir_inferred_static_type_from_locals, CodegenTypeFacts,
};
#[cfg(test)]
use perry_hir::Expr;
#[cfg(test)]
use perry_types::Type as HirType;

// Class-field layout / declared-type resolution lives in a sibling module
// (file-size gate). Re-exported here so existing `type_analysis::*` call
// sites keep resolving, and brought into scope for local callers.
pub(crate) use crate::type_analysis_class_fields::{
    class_field_declared_type, class_field_global_index,
};

// The body of this module was split into topical sub-modules to keep each
// file under the size gate. The split is a pure code move — every item is
// re-exported below so existing `crate::type_analysis::*` call sites keep
// resolving unchanged.
mod numeric;
mod pod;
mod predicates;
mod refine;
mod strings;

pub(crate) use numeric::{is_bigint_expr, is_bool_expr, is_integer_valued_expr, is_numeric_expr};
pub(crate) use pod::{
    add_operands_have_pod_materialization_hazard,
    expr_may_return_boxed_value_from_raw_f64_fallback, expression_has_numeric_length,
    is_fixed_width_buffer_numeric_read, is_numeric_typed_array_class, pod_record_field_is_numeric,
    scalar_replaced_array_element_is_raw_f64, scalar_replaced_field_is_raw_f64,
    scalar_replaced_field_raw_f64_store_state,
};
pub(crate) use predicates::{
    is_array_expr, is_native_module_dynamic_index, is_promise_expr, receiver_class_name,
    receiver_is_error_type, static_type_of,
};
// Re-exported so the `#[cfg(test)] mod tests` (which reaches trunk items via
// `super::*`) can keep calling `tuple_index_literal` directly.
#[cfg(test)]
pub(crate) use predicates::tuple_index_literal;
pub(crate) use refine::{
    compute_auto_captures, is_crypto_digest_chain, is_global_constructor_expr,
    is_process_namespace_version_property, refine_type_from_init,
};
pub(crate) use strings::{
    class_name_extends_url_search_params, is_definitely_string_expr, is_map_expr, is_set_expr,
    is_string_expr, is_url_search_params_expr, is_url_search_params_subclass_expr,
    map_static_type_args, set_static_type_args,
};

#[cfg(test)]
#[path = "type_analysis_tests.rs"]
mod tests;
