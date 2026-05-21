//! Generator function state machine transformation
//!
//! Transforms generator functions (function*) into regular functions
//! that return iterator objects with a next() method implementing
//! a state machine.
//!
//! The next() method contains a `while(true)` loop with `if (__state === N)`
//! blocks. Non-yielding states set __state and `continue`. Yielding states
//! set __state and `return {value, done: false}`.

use perry_hir::ir::*;
use perry_types::{FuncId, LocalId, Type};

mod break_continue;
mod helpers;
mod id_scan;
mod iter_result_rewrite;
mod linearize;
mod lower;
mod rewrite_returns;

// Explicit named re-exports so siblings can reach each other via
// `use super::*;`. Globs don't propagate transitively, so spell every
// cross-module symbol here.
pub(crate) use break_continue::{
    body_contains_yield, collect_hoisted_vars, collect_vars_recursive,
    fix_break_continue_sentinels, fix_break_continue_sentinels_in_stmt,
    fix_break_continue_sentinels_in_stmts, fix_placeholder_state, rewrite_break_continue_in_stmt,
    rewrite_break_continue_in_stmts,
};
pub(crate) use helpers::{
    alloc_local, make_iter_result, rewrite_hoisted_lets_in_stmt, rewrite_hoisted_lets_in_stmts,
    wrap_in_promise_resolve, wrap_returns_in_promise,
};
pub(crate) use id_scan::{
    compute_max_func_id, compute_max_local_id, scan_expr_for_max_func, scan_expr_for_max_local,
    scan_stmt_for_max_func, scan_stmt_for_max_local, scan_stmts_for_max_func,
    scan_stmts_for_max_local,
};
pub(crate) use iter_result_rewrite::{rewrite_expr, rewrite_expr_children, rewrite_stmt};
pub(crate) use linearize::{linearize_body, State, StateExit};
pub(crate) use lower::{
    build_async_step_driver_direct, transform_generator_function,
    transform_generator_function_with_extra_captures,
};
pub(crate) use rewrite_returns::{
    body_contains_return, is_iter_result, prepend_done_before_returns,
    rewrite_catch_returns_to_iter_result, rewrite_catch_returns_to_iter_result_in_stmt,
    rewrite_iter_results_in_stmts, rewrite_iter_results_to_scratch, rewrite_returns_as_done,
    rewrite_returns_to_labeled_break, rewrite_returns_to_labeled_break_in_stmt,
    rewrite_yield_to_await_in_expr, rewrite_yield_to_await_in_expr_children,
    rewrite_yield_to_await_in_stmt, rewrite_yield_to_await_in_stmts,
};

/// Transform all generator functions in a module into state machine form.
pub fn transform_generators(module: &mut Module) {
    // Compute the next available local and func IDs by scanning the module
    let mut next_local_id = compute_max_local_id(module) + 1;
    let mut next_func_id = compute_max_func_id(module) + 1;

    for func in &mut module.functions {
        if func.is_generator {
            transform_generator_function(func, &mut next_local_id, &mut next_func_id);
        }
    }
}

/// Issue #1021: apply the same generator + async-step-driver transform that
/// `transform_generators` runs on top-level functions to a single
/// `Expr::Closure` body. Used by `transform_async_to_generator` for async
/// arrow callbacks (`app.listen(port, async () => { await fetch(self) })`)
/// that would otherwise lower to the busy-wait at `expr.rs:10588` and
/// deadlock self-fetch inside a V8 trampoline frame.
///
/// Preconditions: the body has already had `hoist_awaits_in_stmts` and
/// `rewrite_stmts` applied (i.e. all `Expr::Await` have been turned into
/// `Expr::Yield` and the body is in linearizable form). The caller (in
/// `async_to_generator.rs`) is responsible for that.
///
/// Returns the rewritten body. The closure's `params` are unchanged. The
/// caller should set `is_async = false` on the closure and register the
/// closure's `func_id` in `module.async_step_closures`.
pub fn transform_plain_async_closure_body(
    body: Vec<Stmt>,
    params: &[perry_hir::Param],
    outer_captures: &[LocalId],
    outer_mutable_captures: &[LocalId],
    outer_captures_this: bool,
    outer_enclosing_class: Option<String>,
    next_local_id: &mut LocalId,
    next_func_id: &mut FuncId,
) -> Vec<Stmt> {
    // Construct a temporary Function so we can reuse the existing
    // `transform_generator_function_with_extra_captures` plumbing
    // verbatim. Fields not consulted by the transform are stubbed.
    let synth_func_id = {
        let id = *next_func_id;
        *next_func_id += 1;
        id
    };
    let mut synth = Function {
        id: synth_func_id,
        name: "__async_closure_body".to_string(),
        type_params: Vec::new(),
        params: params.to_vec(),
        return_type: Type::Any,
        body,
        is_async: false,
        is_generator: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: true,
        was_unrolled: false,
    };
    transform_generator_function_with_extra_captures(
        &mut synth,
        next_local_id,
        next_func_id,
        outer_captures,
        outer_mutable_captures,
        outer_captures_this,
        outer_enclosing_class,
    );
    synth.body
}
