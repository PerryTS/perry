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
mod per_iteration;
mod rewrite_returns;

// Explicit named re-exports so siblings can reach each other via
// `use super::*;`. Globs don't propagate transitively, so spell every
// cross-module symbol here.
pub(crate) use break_continue::{
    body_contains_yield, collect_hoisted_vars, fix_break_continue_sentinels,
    fix_break_continue_sentinels_in_catches, fix_break_continue_sentinels_in_stmts,
    fix_placeholder_state, prefix_loop_continues, rewrite_break_continue_in_stmts,
    stmts_have_continue_inside_try_finally,
};
pub(crate) use helpers::{
    alloc_local, make_iter_result, rewrite_hoisted_lets_in_stmts, wrap_returns_in_promise,
};
pub(crate) use hoist_yields::hoist_yields_in_stmts;
pub(crate) use id_scan::{compute_max_func_id, compute_max_local_id};
pub(crate) use iter_result_rewrite::rewrite_stmt;
pub(crate) use linearize::{
    linearize_body, CatchRoute, DelegationRoute, FinallyRoute, State, StateExit,
};
pub(crate) use lower::{
    transform_generator_function, transform_generator_function_with_extra_captures,
};
pub(crate) use per_iteration::{
    collect_per_iteration_ids, collect_written_suspended_loop_captures,
    rewrite_written_captures_to_cells, snapshot_suspended_loop_captures,
};
pub(crate) use rewrite_returns::{
    body_contains_return, prepend_done_before_returns, rewrite_catch_returns_to_iter_result,
    rewrite_iter_results_in_stmts, rewrite_returns_as_done, rewrite_returns_to_labeled_break,
    rewrite_yield_to_await_in_stmts,
};

// #3664: per-thread accumulator for async-generator func_ids discovered while
// transforming a module. The generator transform clears `is_async`/
// `is_generator` (both async and sync generators lower to an identical
// `{next,return,throw}` wrapper), so we record the async ones here BEFORE the
// flags are cleared, then drain into `module.async_generator_funcs` at the end
// of `transform_generators`. A thread_local keeps this isolated when modules
// are transformed in parallel; `transform_generators` clears it on entry so a
// reused worker thread never leaks IDs across modules.
thread_local! {
    static ASYNC_GENERATOR_FUNC_IDS: std::cell::RefCell<std::collections::HashSet<FuncId>> =
        std::cell::RefCell::new(std::collections::HashSet::new());
    // Per-generator count of leading param-prologue statements (default guards +
    // destructuring binding) to lift from the state machine back into the outer
    // wrapper, so generator param binding runs synchronously at call time (spec
    // FunctionDeclarationInstantiation). Keyed by func_id. Populated from
    // `module.gen_param_prologue_len` at the start of `transform_generators`;
    // the closure handler copies an entry to the synthetic body func_id it
    // creates so `transform_generator_function_with_extra_captures` can read it
    // by the function it actually transforms.
    static GEN_PROLOGUE_LENS: std::cell::RefCell<std::collections::HashMap<FuncId, usize>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Read (and for closures, alias) the recorded param-prologue length for a
/// generator function id. Returns 0 when none was recorded (the common case —
/// no destructuring / default params — keeping the lift fully inert).
pub(crate) fn gen_prologue_len(func_id: FuncId) -> usize {
    GEN_PROLOGUE_LENS.with(|m| m.borrow().get(&func_id).copied().unwrap_or(0))
}

/// Copy a recorded prologue length from `from` to `to`. Used by the closure
/// handler: the original `Expr::Closure` func_id carries the lowering-recorded
/// length, but the transform runs on a synthetic `Function` with a fresh id.
fn alias_prologue_len(from: FuncId, to: FuncId) {
    GEN_PROLOGUE_LENS.with(|m| {
        let mut m = m.borrow_mut();
        if let Some(len) = m.get(&from).copied() {
            m.insert(to, len);
        }
    });
}

/// Record a func_id as belonging to an `async function*` (declaration or
/// expression). Called from the generator transform while the original
/// `is_async` flag is still observable. (#3664)
fn record_async_generator_func(id: FuncId) {
    ASYNC_GENERATOR_FUNC_IDS.with(|s| {
        s.borrow_mut().insert(id);
    });
}

/// Transform all generator functions in a module into state machine form.
pub fn transform_generators(module: &mut Module) {
    // #3664: reset the per-thread async-generator accumulator for this module.
    ASYNC_GENERATOR_FUNC_IDS.with(|s| s.borrow_mut().clear());
    // Load the param-prologue lengths recorded by lowering so the transform can
    // lift generator param binding into the outer wrapper (run at call time).
    GEN_PROLOGUE_LENS.with(|m| {
        let mut m = m.borrow_mut();
        m.clear();
        for (id, len) in &module.gen_param_prologue_len {
            m.insert(*id, *len);
        }
    });

    // Compute the next available local and func IDs by scanning the module
    let mut next_local_id = compute_max_local_id(module) + 1;
    let mut next_func_id = compute_max_func_id(module) + 1;

    for func in &mut module.functions {
        if func.is_generator {
            // #3664: `async function* name(){}` — record before the transform
            // clears `is_async`.
            if func.is_async {
                record_async_generator_func(func.id);
            }
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
        for m in &mut class.methods {
            if m.is_generator {
                transform_generator_function_with_extra_captures(
                    m,
                    &mut next_local_id,
                    &mut next_func_id,
                    &[],
                    &[],
                    true,
                    false,
                    Some(class.name.clone()),
                );
            }
            let mut b = std::mem::take(&mut m.body);
            transform_generator_closures_in_stmts(&mut b, &mut next_local_id, &mut next_func_id);
            m.body = b;
        }
        for m in &mut class.static_methods {
            if m.is_generator {
                transform_generator_function(m, &mut next_local_id, &mut next_func_id);
            }
            let mut b = std::mem::take(&mut m.body);
            transform_generator_closures_in_stmts(&mut b, &mut next_local_id, &mut next_func_id);
            m.body = b;
        }
        // Computed-key members (#3557) are instance methods installed on the
        // prototype, so generator computed methods get the same class-context
        // capture treatment as ordinary methods.
        for member in &mut class.computed_members {
            let m = &mut member.function;
            if m.is_generator {
                transform_generator_function_with_extra_captures(
                    m,
                    &mut next_local_id,
                    &mut next_func_id,
                    &[],
                    &[],
                    true,
                    false,
                    Some(class.name.clone()),
                );
            }
            let mut b = std::mem::take(&mut m.body);
            transform_generator_closures_in_stmts(&mut b, &mut next_local_id, &mut next_func_id);
            m.body = b;
        }
        for m in class
            .getters
            .iter_mut()
            .map(|(_, f)| f)
            .chain(class.setters.iter_mut().map(|(_, f)| f))
        {
            let mut b = std::mem::take(&mut m.body);
            transform_generator_closures_in_stmts(&mut b, &mut next_local_id, &mut next_func_id);
            m.body = b;
        }
    }

    // #3664: drain the async-generator func_ids collected above (named + closure
    // expressions) into the module so codegen can register them.
    ASYNC_GENERATOR_FUNC_IDS.with(|s| {
        let collected = std::mem::take(&mut *s.borrow_mut());
        module.async_generator_funcs.extend(collected);
    });
}

fn transform_generator_closures_in_stmts(
    stmts: &mut [Stmt],
    next_local_id: &mut LocalId,
    next_func_id: &mut FuncId,
) {
    enum Frame {
        Stmt(*mut Stmt),
        ExprEnter(*mut Expr),
        ExprExit(*mut Expr),
    }

    fn push_stmt_slice(stack: &mut Vec<Frame>, stmts: &mut [Stmt]) {
        for stmt in stmts.iter_mut().rev() {
            stack.push(Frame::Stmt(stmt as *mut Stmt));
        }
    }

    fn push_expr(stack: &mut Vec<Frame>, expr: &mut Expr) {
        stack.push(Frame::ExprEnter(expr as *mut Expr));
    }

    let mut stack = Vec::new();
    push_stmt_slice(&mut stack, stmts);

    while let Some(frame) = stack.pop() {
        match frame {
            Frame::Stmt(stmt) => {
                // The traversal owns the only active mutable borrow for each HIR
                // node. Raw pointers let us keep an explicit work stack instead
                // of using Rust call stack for deeply nested generated schemas.
                let stmt = unsafe { &mut *stmt };
                match stmt {
                    Stmt::Let { init: Some(e), .. }
                    | Stmt::Expr(e)
                    | Stmt::Throw(e)
                    | Stmt::Return(Some(e)) => push_expr(&mut stack, e),
                    Stmt::If {
                        condition,
                        then_branch,
                        else_branch,
                    } => {
                        if let Some(eb) = else_branch {
                            push_stmt_slice(&mut stack, eb);
                        }
                        push_stmt_slice(&mut stack, then_branch);
                        push_expr(&mut stack, condition);
                    }
                    Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                        push_stmt_slice(&mut stack, body);
                        push_expr(&mut stack, condition);
                    }
                    Stmt::For {
                        init,
                        condition,
                        update,
                        body,
                    } => {
                        push_stmt_slice(&mut stack, body);
                        if let Some(u) = update {
                            push_expr(&mut stack, u);
                        }
                        if let Some(c) = condition {
                            push_expr(&mut stack, c);
                        }
                        if let Some(i) = init {
                            stack.push(Frame::Stmt(i.as_mut() as *mut Stmt));
                        }
                    }
                    Stmt::Try {
                        body,
                        catch,
                        finally,
                    } => {
                        if let Some(f) = finally {
                            push_stmt_slice(&mut stack, f);
                        }
                        if let Some(c) = catch {
                            push_stmt_slice(&mut stack, &mut c.body);
                        }
                        push_stmt_slice(&mut stack, body);
                    }
                    Stmt::Switch {
                        discriminant,
                        cases,
                    } => {
                        for case in cases.iter_mut().rev() {
                            push_stmt_slice(&mut stack, &mut case.body);
                            if let Some(t) = &mut case.test {
                                push_expr(&mut stack, t);
                            }
                        }
                        push_expr(&mut stack, discriminant);
                    }
                    Stmt::Labeled { body, .. } => {
                        stack.push(Frame::Stmt(body.as_mut() as *mut Stmt));
                    }
                    _ => {}
                }
            }
            Frame::ExprEnter(expr) => {
                let expr = unsafe { &mut *expr };
                stack.push(Frame::ExprExit(expr as *mut Expr));

                let mut children = Vec::new();
                perry_hir::walker::walk_expr_children_mut(expr, &mut |child| {
                    children.push(child as *mut Expr);
                });
                for child in children.into_iter().rev() {
                    stack.push(Frame::ExprEnter(child));
                }

                // Closure bodies are intentionally not visited by
                // `walk_expr_children_mut`, but generator closures inside those
                // bodies must still be transformed before the closure itself.
                if let Expr::Closure { body, .. } = expr {
                    push_stmt_slice(&mut stack, body);
                }
            }
            Frame::ExprExit(expr) => {
                let expr = unsafe { &mut *expr };
                if let Expr::Closure {
                    func_id,
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
                        // #3664: `async function*(){}` expression — record its func_id
                        // (the symbol codegen registers) before `is_async` is cleared
                        // below.
                        if *is_async {
                            record_async_generator_func(*func_id);
                        }
                        let synth_id = {
                            let id = *next_func_id;
                            *next_func_id += 1;
                            id
                        };
                        // The transform reads the prologue length by the func it
                        // transforms (the synthetic body fn); lowering recorded
                        // it under the original closure func_id. Alias it across.
                        alias_prologue_len(*func_id, synth_id);
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
                            false,
                            enclosing_class.clone(),
                        );
                        *body = synth.body;
                        // The closure now returns a {next,return,throw} object
                        // when called; it is no longer a generator (and not
                        // async — the transform handles async-generator
                        // Promise-wrapping internally).
                        *is_generator = false;
                        *is_async = false;
                    }
                }
            }
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
    outer_captures_new_target: bool,
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
        outer_captures_new_target,
        outer_enclosing_class,
    );
    synth.body
}
