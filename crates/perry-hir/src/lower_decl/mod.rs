//! Declaration lowering.
//!
//! Split from a single 5,557-line file into topical sub-modules in
//! v0.5.1019 to satisfy the file-size CI gate. mod.rs is a re-export
//! hub — public-API shape (`crate::lower_decl::*`) is preserved exactly.
//!
//! Contains functions for lowering function declarations, class declarations,
//! enum declarations, interface declarations, type alias declarations,
//! constructors, class methods, getters, setters, and class properties.

mod block;
mod body_stmt;
mod class_captures;
mod class_computed;
mod class_decl;
mod class_members;
mod class_validation;
mod enum_decl;
mod fn_decl;
mod helpers;
mod interface_decl;
mod private_members;
mod type_alias;
mod typeof_narrow;

// Explicit named re-exports — glob `pub use foo::*` doesn't transitively
// expose names through an outer `pub(crate) use crate::lower_decl::*;`
// (the consumer at `crate::lower::*` would otherwise see nothing). Keep
// this list in sync with each sibling's `pub fn` declarations.
pub(crate) use block::{
    collect_annexb_block_fn_decl_names, collect_lexical_decl_names,
    collect_refs_in_closure_bodies_stmt, collect_top_level_let_ids_stmt,
    collect_var_binding_names_from_pat, collect_var_binding_names_from_stmt,
    compute_prealloc_for_hoisted_closures, lower_block_stmt, lower_block_stmt_scoped,
    lower_fn_body_block_stmt, lower_stmts_using_aware, pre_register_forward_captured_lets,
};
pub(crate) use body_stmt::gen_capture_scan::forward_referenced_nested_generators;
pub(crate) use body_stmt::{find_native_return_in_stmts, lower_body_stmt};
pub(crate) use class_captures::{append_new_args_stmt, synthesize_class_captures};
pub(crate) use class_computed::class_computed_member_registration_expr;
pub(crate) use class_decl::{lower_class_decl, lower_class_from_ast};
pub(crate) use class_members::{
    lower_class_method, lower_class_method_with_name, lower_class_prop, lower_constructor,
    lower_getter_method, lower_getter_method_with_name, lower_setter_method,
    lower_setter_method_with_name,
};
pub(crate) use class_validation::{
    validate_class_element_early_errors, validate_legacy_decorator_surface,
};
pub(crate) use enum_decl::{compute_enum_members, lower_enum_decl};
pub(crate) use fn_decl::lower_fn_decl;
pub(crate) use helpers::{
    append_synthetic_arguments_param, body_has_use_strict, body_uses_arguments,
    build_default_param_stmts, collect_let_decls_in_stmt, init_is_webassembly_instantiate,
    is_inspect_custom_key, is_symbol_iterator_key, mapped_argument_parameter_ids,
    params_are_simple_arguments_list, params_use_arguments, symbol_well_known_key,
};
pub(crate) use interface_decl::lower_interface_decl;
pub(crate) use private_members::{
    build_private_scope, lower_private_getter, lower_private_method, lower_private_prop,
    lower_private_setter,
};
pub(crate) use type_alias::lower_type_alias_decl;
