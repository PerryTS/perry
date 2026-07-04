//! AST to HIR lowering
//!
//! Converts SWC's TypeScript AST into our HIR representation.
//!
//! This file is the orientation entry point for the `lower` module. The
//! actual lowering logic lives in topical sibling files:
//!
//! - `lowering_context.rs` — the `LoweringContext` struct (state shared
//!   across every AST → HIR pass).
//! - `lower_module_fn.rs` — the seven `lower_module*` public entry
//!   points that drive a whole-module conversion.
//! - `lower_expr.rs` — `lower_expr`, `lower_expr_assignment`, and the
//!   `Text` reactive-template desugar (the smaller inline arms; larger
//!   variants delegate to `expr_*` siblings).
//! - `array_fold.rs` — `try_fold_array_method_call` plus the
//!   `is_known_*_static_method` recognisers driving the `typeof` fold.
//! - `typed_parse.rs` — `JSON.parse<T>` source-order / structural type
//!   helpers consumed by `expr_call/*`.
//!
//! Older extractions (`context.rs`, `stmt.rs`, `module_decl.rs`,
//! `misc.rs`, `pre_scan.rs`, `closure_analysis.rs`, `decorators.rs`,
//! `template.rs`, `widget_decl.rs`, plus the `expr_*` arm splits) sit
//! alongside as direct submodules and re-export through this hub.

// Tier 2.3 (v0.5.337-338): incremental extraction of `lower_expr` arms
// into `lower/expr_*.rs` sub-modules. Same pattern as Tier 2.1
// (compile.rs split) and 2.2 (ui_styling extracted from lower_call.rs).
//
// - `expr_misc.rs` (v0.5.337): 8 small variants (Cond, Await,
//   SuperProp, Update, Tpl, Seq, MetaProp, Yield).
// - `expr_function.rs` (v0.5.338): Arrow + Fn expression closures,
//   sharing the closure-capture analysis helper.
// - `expr_object.rs` (v0.5.338): Object literal lowering (479 LOC,
//   the largest single arm extracted so far).
// - `expr_member.rs` / `expr_assign.rs` / `expr_new.rs` (v0.5.339):
//   property access, assignment, and `new C()` constructor calls.
mod context;
pub(crate) mod expr_assign;
mod expr_call;
pub(crate) mod expr_function;
pub(crate) use expr_function::capture_function_source;
mod expr_member;
pub(crate) use expr_member::{wrap_private_guard, PRIV_OP_WRITE};
mod expr_misc;
mod expr_new;
mod expr_new_builtins;
mod expr_object;
mod stmt;
mod unimpl_hints;
pub(crate) use stmt::*;
mod for_head;
pub(crate) use for_head::{for_head_binding_stmts, predefine_for_head};
mod stmt_loops;
pub(crate) use stmt_loops::{
    insert_iterator_close_on_abrupt, lazy_iter_for_stmt, lazy_or_index_elem, lower_stmt_for_in,
    lower_stmt_for_of, wrap_lazy_for_of_body_close_on_throw,
};
mod module_decl;
pub(crate) use module_decl::*;
mod misc;
pub(crate) use misc::*;
mod pre_scan;
pub(crate) use pre_scan::*;
mod closure_analysis;
mod const_fold_fn;
mod eval_super_scan;
mod fn_ctor_env;
mod global_eval_hoist;
pub(crate) mod type_widening;
pub(crate) use closure_analysis::*;
mod decorators;
pub(crate) use decorators::*;
mod template;
pub(crate) use template::*;
mod widget_decl;
pub(crate) use widget_decl::*;

// Newly-extracted topical siblings (split out from the previously
// ~2,700-LOC `mod.rs` body). Each is re-exported by *explicit name*
// rather than via `use foo::*;` glob — globs don't always propagate
// transitively when downstream files reach into `crate::lower::Name`,
// so spelling them out keeps the public-and-internal API stable.
mod lowering_context;
pub(crate) use lowering_context::{
    LoweringContext, PrivKind, PrivMember, PrivateScope, WithEnvFrame,
};

mod locals;
pub(crate) use locals::Locals;

mod typed_parse;
pub(crate) use typed_parse::{extract_typed_parse_source_order, resolve_typed_parse_ty};

mod array_fold;
pub(crate) use array_fold::{
    is_known_array_prototype_method, is_known_namespace_static_function,
    is_known_object_prototype_method, is_known_string_prototype_method, try_fold_array_method_call,
};

mod lower_module_fn;
pub use lower_module_fn::{
    lower_module, lower_module_full, lower_module_with_class_id,
    lower_module_with_class_id_and_types, lower_module_with_class_id_types_and_seed,
    lower_module_with_class_id_types_seed_and_entry,
};

mod lower_expr;
pub(crate) use lower_expr::{
    lower_expr, lower_expr_assignment, throw_reference_error_expr, try_desugar_reactive_text,
    with_implicit_unset_let, with_set_fallback_for_ident,
};

// Re-export extracted module functions
pub(crate) use crate::analysis::*;
pub(crate) use crate::destructuring::*;
pub(crate) use crate::jsx::*;
pub(crate) use crate::lower_decl::*;
pub(crate) use crate::lower_patterns::*;
pub(crate) use crate::lower_types::*;

#[cfg(test)]
mod tests;
