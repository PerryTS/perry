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
mod hoist_yields;
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
pub(crate) use hoist_yields::hoist_yields_in_stmts;
pub(crate) use id_scan::{
    compute_max_func_id, compute_max_local_id, scan_expr_for_max_func, scan_expr_for_max_local,
    scan_stmt_for_max_func, scan_stmt_for_max_local, scan_stmts_for_max_func,
    scan_stmts_for_max_local,
};
pub(crate) use iter_result_rewrite::{rewrite_expr, rewrite_expr_children, rewrite_stmt};
pub(crate) use linearize::{linearize_body, CatchRoute, State, StateExit};
pub(crate) use lower::{
    build_async_step_driver_direct, transform_generator_function,
    transform_generator_function_with_extra_captures,
};
pub(crate) use rewrite_returns::{
    body_contains_return, is_iter_result, prepend_done_before_returns,
    rewrite_catch_returns_to_iter_result, rewrite_catch_returns_to_iter_result_in_stmt,
    rewrite_iter_results_in_stmts, rewrite_returns_as_done, rewrite_returns_to_labeled_break,
    rewrite_returns_to_labeled_break_in_stmt, rewrite_yield_to_await_in_expr,
    rewrite_yield_to_await_in_expr_children, rewrite_yield_to_await_in_stmt,
    rewrite_yield_to_await_in_stmts,
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

    // #321: generator function EXPRESSIONS (`function*(){}`, e.g. effect's
    // `Effect.gen(function*(){...})`) lower to `Expr::Closure { is_generator:
    // true }`. Apply the SAME state-machine transform to each, using the
    // captures-aware path so the closure's outer captures are threaded into the
    // generated next/return/throw step closures. After the transform the
    // closure's body returns a `{next,return,throw}` object when called — the
    // same contract as a named `function* g(){}`.
    for func in &mut module.functions {
        let mut body = std::mem::take(&mut func.body);
        transform_generator_closures_in_stmts(&mut body, &mut next_local_id, &mut next_func_id);
        func.body = body;
    }
    // Top-level statements live in `module.init`, not `module.functions` — a
    // `const g = function*(){...}` at module scope (and effect's `Effect.gen(
    // function*(){...})`) is here.
    {
        let mut init = std::mem::take(&mut module.init);
        transform_generator_closures_in_stmts(&mut init, &mut next_local_id, &mut next_func_id);
        module.init = init;
    }
    // Class method / constructor / accessor bodies can also hold generator
    // expressions (effect's classes do).
    for class in &mut module.classes {
        if let Some(ctor) = &mut class.constructor {
            let mut b = std::mem::take(&mut ctor.body);
            transform_generator_closures_in_stmts(&mut b, &mut next_local_id, &mut next_func_id);
            ctor.body = b;
        }
        for m in class
            .methods
            .iter_mut()
            .chain(class.static_methods.iter_mut())
            .chain(class.getters.iter_mut().map(|(_, f)| f))
            .chain(class.setters.iter_mut().map(|(_, f)| f))
        {
            let mut b = std::mem::take(&mut m.body);
            transform_generator_closures_in_stmts(&mut b, &mut next_local_id, &mut next_func_id);
            m.body = b;
        }
    }
}

fn transform_generator_closures_in_stmts(
    stmts: &mut [Stmt],
    next_local_id: &mut LocalId,
    next_func_id: &mut FuncId,
) {
    for s in stmts.iter_mut() {
        transform_generator_closures_in_stmt(s, next_local_id, next_func_id);
    }
}

fn transform_generator_closures_in_stmt(
    stmt: &mut Stmt,
    next_local_id: &mut LocalId,
    next_func_id: &mut FuncId,
) {
    match stmt {
        Stmt::Let { init: Some(e), .. } => {
            transform_generator_closures_in_expr(e, next_local_id, next_func_id)
        }
        Stmt::Expr(e) | Stmt::Throw(e) => {
            transform_generator_closures_in_expr(e, next_local_id, next_func_id)
        }
        Stmt::Return(Some(e)) => {
            transform_generator_closures_in_expr(e, next_local_id, next_func_id)
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            transform_generator_closures_in_expr(condition, next_local_id, next_func_id);
            transform_generator_closures_in_stmts(then_branch, next_local_id, next_func_id);
            if let Some(eb) = else_branch {
                transform_generator_closures_in_stmts(eb, next_local_id, next_func_id);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            transform_generator_closures_in_expr(condition, next_local_id, next_func_id);
            transform_generator_closures_in_stmts(body, next_local_id, next_func_id);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                transform_generator_closures_in_stmt(i, next_local_id, next_func_id);
            }
            if let Some(c) = condition {
                transform_generator_closures_in_expr(c, next_local_id, next_func_id);
            }
            if let Some(u) = update {
                transform_generator_closures_in_expr(u, next_local_id, next_func_id);
            }
            transform_generator_closures_in_stmts(body, next_local_id, next_func_id);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            transform_generator_closures_in_stmts(body, next_local_id, next_func_id);
            if let Some(c) = catch {
                transform_generator_closures_in_stmts(&mut c.body, next_local_id, next_func_id);
            }
            if let Some(f) = finally {
                transform_generator_closures_in_stmts(f, next_local_id, next_func_id);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            transform_generator_closures_in_expr(discriminant, next_local_id, next_func_id);
            for case in cases.iter_mut() {
                if let Some(t) = &mut case.test {
                    transform_generator_closures_in_expr(t, next_local_id, next_func_id);
                }
                transform_generator_closures_in_stmts(&mut case.body, next_local_id, next_func_id);
            }
        }
        Stmt::Labeled { body, .. } => {
            transform_generator_closures_in_stmt(body, next_local_id, next_func_id)
        }
        _ => {}
    }
}

fn transform_generator_closures_in_expr(
    expr: &mut Expr,
    next_local_id: &mut LocalId,
    next_func_id: &mut FuncId,
) {
    // Bottom-up: descend into this closure's own body (the expr-children walker
    // deliberately does NOT visit Closure bodies), then into all sub-exprs, so
    // nested generator closures are transformed before their enclosing one.
    if let Expr::Closure { body, .. } = expr {
        transform_generator_closures_in_stmts(body, next_local_id, next_func_id);
    }
    perry_hir::walker::walk_expr_children_mut(expr, &mut |child| {
        transform_generator_closures_in_expr(child, next_local_id, next_func_id);
    });

    // Now transform THIS closure if it's a generator expression.
    if let Expr::Closure {
        is_generator,
        params,
        body,
        captures,
        mutable_captures,
        captures_this,
        enclosing_class,
        is_strict,
        is_async,
        ..
    } = expr
    {
        if *is_generator {
            let synth_id = {
                let id = *next_func_id;
                *next_func_id += 1;
                id
            };
            let mut synth = Function {
                id: synth_id,
                name: "__gen_closure_body".to_string(),
                type_params: Vec::new(),
                params: params.clone(),
                return_type: Type::Any,
                body: std::mem::take(body),
                is_strict: *is_strict,
                is_async: *is_async,
                is_generator: true,
                is_exported: false,
                captures: Vec::new(),
                decorators: Vec::new(),
                was_plain_async: false,
                was_unrolled: false,
            };
            transform_generator_function_with_extra_captures(
                &mut synth,
                next_local_id,
                next_func_id,
                captures,
                mutable_captures,
                *captures_this,
                enclosing_class.clone(),
            );
            *body = synth.body;
            // The closure now returns a {next,return,throw} object when called;
            // it is no longer a generator (and not async — the transform handles
            // the async-generator Promise-wrapping internally for `async function*`).
            *is_generator = false;
            *is_async = false;
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
    is_strict: bool,
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
        is_strict,
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
