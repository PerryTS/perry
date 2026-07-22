//! Issue #256: spec-compliant microtask ordering for plain async functions.
//!
//! ## What this pass does
//!
//! Pre-pass that runs before `transform_generators`. For every top-level
//! function with `is_async = true && !is_generator`:
//!
//! 1. **Hoists non-top-level awaits**: any `await x` not in a top-level
//!    statement position (let init, expr stmt, return) is lifted into a
//!    fresh `let __awaitN = await x;` placed before the containing
//!    statement, and the original site is replaced with `LocalGet(__awaitN)`.
//!    Without this, expressions like `console.log("x: " + await y)` lower
//!    to `console.log("x: " + 0)` because the generator transform's
//!    `linearize_body` only recognises yields at top-level positions; a
//!    yield buried inside a concat operator hits codegen's
//!    `Expr::Yield => double_literal(0.0)` arm instead.
//! 2. **Rewrites await→yield**: every `Expr::Await(x)` becomes
//!    `Expr::Yield { value: Some(x), delegate: false }`.
//! 3. **Flips the flags**: `is_async = false`, `is_generator = true`,
//!    `was_plain_async = true`.
//!
//! After this pass, the existing generator state-machine transform lifts
//! the function into a `{ next, return, throw }` iterator. The
//! `was_plain_async` flag tells the generator transform to wrap the
//! iterator in an async-step driver so the function returns a Promise
//! that resolves to the user's return value, with each yield/await
//! suspending into a microtask.
//!
//! ## Why this fixes the spec gap
//!
//! Pre-fix Perry's async functions ran their entire body synchronously on
//! the calling thread, with each `await` lowered to a busy-wait poll loop
//! on the awaited Promise. This diverges from spec semantics: an `await`
//! should always yield to the microtask queue, even on already-resolved
//! Promises, so synchronous code following an unawaited async call runs
//! before the awaited body's continuation.
//!
//! Post-fix the async function becomes a state machine. The first state
//! runs synchronously (matching spec). Each `await x` lowers to a yield
//! that suspends the state machine and chains the continuation through
//! `Promise.resolve(x).then(continuation)`, which puts the rest of the
//! body in a microtask. The microtask runs after all currently-executing
//! synchronous code finishes — exactly the spec ordering.
//!
//! ## Scope and limitations
//!
//! - **Top-level functions and async closures (since #1021 phase 2)**:
//!   `Expr::Closure { is_async: true }` whose body contains awaits is now
//!   also rewritten — the closure body goes through
//!   `transform_plain_async_closure_body` (in `generator.rs`) which reuses
//!   the same state-machine + async-step-driver path. Detected closures'
//!   func_ids land in `Module.async_step_closures`. Without this, the
//!   busy-wait at `expr.rs:10588` deadlocks self-fetch from inside V8
//!   trampoline frames (issue #1021).
//! - **No new HIR variants or runtime helpers**: the rewrite produces
//!   only existing variants (Yield, Closure, Promise.then chains via
//!   GlobalGet(0)). The async-step driver is built inline in the
//!   generator transform. This sidesteps the LLVM constant-folding
//!   mystery the prior prototype hit (issue #256 background section 1).
//! - **State-machine idempotency (issue #1029, fixed in this branch)**:
//!   the underlying state machine was not idempotent across re-invocation
//!   (call 2+ returned undefined) because the generator transform's
//!   internally-built next/return/throw/step closures listed state/done/
//!   sent in `mutable_captures` but the boxing analysis missed them, so
//!   `js_closure_alloc_with_captures_singleton` cached on capture-VALUE
//!   bits — identical every call → stale closure reused. Fixed by
//!   prepending `Stmt::PreallocateBoxes` for the state-machine internals
//!   in `transform_generator_function_with_extra_captures` so captures
//!   lower to box pointers (distinct per call). This applies uniformly
//!   to top-level async fns and to the async closures rewritten by
//!   #1021 phase 2.

use perry_hir::ir::*;
use perry_hir::types::{LocalId, Type};
use std::collections::HashSet;

// #5293: the max-LocalId / max-FuncId scans were copy-pasted here; route through
// the canonical exhaustive-walker implementations in `generator::id_scan`.
use crate::generator::{compute_max_func_id, compute_max_local_id};

/// Run the plain-async → generator CPS conversion on one function / class-method
/// body: hoist non-top-level awaits, rewrite `await`→`yield`, and (if the body
/// had any await) mark it a `was_plain_async` generator. `transform_generators`
/// then builds the state machine — for class methods with the class
/// `this`/`enclosing_class` context — and the shared lowering wraps
/// `was_plain_async` generators in the async-step driver so the fn/method still
/// returns a Promise. No-op for non-async or already-generator functions.
fn cps_mark_async_fn(func: &mut Function, next_local_id: &mut LocalId) {
    if !func.is_async || func.is_generator {
        return;
    }
    let mut had_await = false;
    // #691 Phase 3: peephole `await Promise.resolve(<provably-non-Promise>)` →
    // `await <arg>` (spec-equivalent for a non-Promise arg; skips a Promise
    // alloc + the unwrap-Fulfilled branch, hitting the primitive fast-path).
    strip_redundant_promise_resolve_in_func(func);
    // Hoist non-top-level awaits so every Await ends up in a top-level position
    // `linearize_body` can split states at, then rewrite await→yield.
    hoist_awaits_in_stmts(&mut func.body, next_local_id);
    rewrite_stmts(&mut func.body, &mut had_await);
    // Without awaits the direct-call path is correct + cheaper; leave is_async.
    if had_await {
        func.is_async = false;
        func.is_generator = true;
        func.was_plain_async = true;
    }
}

/// Run the pre-pass on every async function in the module.
pub fn transform_async_to_generator(module: &mut Module) {
    // NOTE: previously bailed the ENTIRE module (incl. every top-level async
    // fn) to block-wait when any class had `__perry_cap_*` fields (v0.5.323
    // issue #212 capture rewrite), because the async-step driver's fresh
    // LocalId allocations could collide with class-method capture rebind ids
    // (`[PERRY WARN] js_box_set: null box pointer` — the async-step `__iter`
    // LocalGet returning a captured box pointer). That collision is now
    // prevented deterministically: #5293 routed `compute_max_local_id`
    // through the exhaustive `generator::id_scan`, which explicitly scans
    // class member bodies (methods/getters/setters/ctor/field inits), so
    // `next_local_id` is always greater than every class-method capture id
    // (see id_scan.rs "Also scan class member bodies … #212 fix allocates
    // method-local rebind ids"). The blanket module bail was over-broad: any
    // module with a single capturing class forced EVERY top-level async fn onto
    // block-wait, so a common shape like
    // `async function f(){ … await once(t, (done) => render(x, { onDone: done })) }`
    // — where the awaited promise is resolved by that very callback — deadlocks,
    // because block-wait starves the callback that would resolve it. Removed;
    // rely on the exhaustive id scan (+ #1029 PreallocateBoxes for cross-state
    // locals).
    let mut next_local_id = compute_max_local_id(module) + 1;
    for func in &mut module.functions {
        cps_mark_async_fn(func, &mut next_local_id);
    }
    // Async CLASS METHODS (e.g. an `async validate()` / `async parseAsync()` on
    // a class). The loop above only covers `module.functions`; class member
    // bodies were never marked, so async class methods stayed raw `is_async` and
    // fell back to block-wait — every async method on a class deadlocked the
    // same way top-level async fns did before this pass covered them.
    // `transform_generators` ALREADY builds generator
    // methods with the class `this`/`enclosing_class` context (generator/mod.rs)
    // and the shared lowering wraps `was_plain_async` generators in the
    // async-step driver, so marking them here is all that's required. Match the
    // member set `transform_generators` handles: instance methods, static
    // methods, and computed-key members. (Constructors / getters / setters can't
    // be `async` in JS, so `cps_mark_async_fn`'s `is_async` guard no-ops them.)
    for class in &mut module.classes {
        for m in &mut class.methods {
            cps_mark_async_fn(m, &mut next_local_id);
        }
        for m in &mut class.static_methods {
            cps_mark_async_fn(m, &mut next_local_id);
        }
        for member in &mut class.computed_members {
            cps_mark_async_fn(&mut member.function, &mut next_local_id);
        }
    }

    // Issue #1021 phase 2: rewrite async closures (`Expr::Closure {
    // is_async: true }` with awaits) into state machines. Without this,
    // `app.listen(port, async () => { await fetch(self) })` callbacks
    // busy-wait at `expr.rs:10588` while holding a V8 trampoline scope,
    // blocking deno's executor from settling the accept-loop continuation
    // and deadlocking self-fetch.
    //
    // Populate the worklist of candidate closures NOW (after the
    // capturing-classes bailout has cleared) so the set stays consistent
    // with what's actually rewritten — i.e. it's never populated without
    // a matching rewrite, and `module.async_step_closures` is a reliable
    // ground-truth for "this closure body returns a Promise via the
    // async-step driver" rather than just "would have been rewritten if
    // the module-level bailout hadn't fired".
    collect_async_step_closures(module);

    if !module.async_step_closures.is_empty() {
        let mut next_func_id: perry_hir::types::FuncId = compute_max_func_id(module) + 1;
        // Walk the HIR, rewriting matched async closures in-place. The
        // walker descends into nested closures so chains like
        // `async () => { items.map(async x => await f(x)) }` are
        // handled.
        let work = module.async_step_closures.clone();
        for func in &mut module.functions {
            rewrite_async_closures_in_stmts(
                &mut func.body,
                &work,
                &mut next_local_id,
                &mut next_func_id,
            );
        }
        rewrite_async_closures_in_stmts(
            &mut module.init,
            &work,
            &mut next_local_id,
            &mut next_func_id,
        );
        for class in &mut module.classes {
            for m in &mut class.methods {
                rewrite_async_closures_in_stmts(
                    &mut m.body,
                    &work,
                    &mut next_local_id,
                    &mut next_func_id,
                );
            }
            for m in &mut class.static_methods {
                rewrite_async_closures_in_stmts(
                    &mut m.body,
                    &work,
                    &mut next_local_id,
                    &mut next_func_id,
                );
            }
            if let Some(ctor) = &mut class.constructor {
                rewrite_async_closures_in_stmts(
                    &mut ctor.body,
                    &work,
                    &mut next_local_id,
                    &mut next_func_id,
                );
            }
            for getter in &mut class.getters {
                rewrite_async_closures_in_stmts(
                    &mut getter.1.body,
                    &work,
                    &mut next_local_id,
                    &mut next_func_id,
                );
            }
            for setter in &mut class.setters {
                rewrite_async_closures_in_stmts(
                    &mut setter.1.body,
                    &work,
                    &mut next_local_id,
                    &mut next_func_id,
                );
            }
            for member in &mut class.computed_members {
                rewrite_async_closures_in_expr(
                    &mut member.key_expr,
                    &work,
                    &mut next_local_id,
                    &mut next_func_id,
                );
                rewrite_async_closures_in_stmts(
                    &mut member.function.body,
                    &work,
                    &mut next_local_id,
                    &mut next_func_id,
                );
            }
            // #5854: instance + static FIELD initializers (and computed-key
            // exprs) can hold async closures (`handler = async () => await x`);
            // mirror the collect scan above so they are actually rewritten into
            // the async-step state machine, not merely listed in the set.
            for field in class
                .fields
                .iter_mut()
                .chain(class.static_fields.iter_mut())
            {
                if let Some(init) = &mut field.init {
                    rewrite_async_closures_in_expr(
                        init,
                        &work,
                        &mut next_local_id,
                        &mut next_func_id,
                    );
                }
                if let Some(key_expr) = &mut field.key_expr {
                    rewrite_async_closures_in_expr(
                        key_expr,
                        &work,
                        &mut next_local_id,
                        &mut next_func_id,
                    );
                }
            }
        }
    }
}

fn rewrite_async_closures_in_stmts(
    stmts: &mut Vec<Stmt>,
    work: &std::collections::HashSet<perry_hir::types::FuncId>,
    next_local_id: &mut LocalId,
    next_func_id: &mut perry_hir::types::FuncId,
) {
    for s in stmts {
        rewrite_async_closures_in_stmt(s, work, next_local_id, next_func_id);
    }
}

fn rewrite_async_closures_in_stmt(
    stmt: &mut Stmt,
    work: &std::collections::HashSet<perry_hir::types::FuncId>,
    next_local_id: &mut LocalId,
    next_func_id: &mut perry_hir::types::FuncId,
) {
    match stmt {
        Stmt::Let { init: Some(e), .. } => {
            rewrite_async_closures_in_expr(e, work, next_local_id, next_func_id)
        }
        Stmt::Expr(e) | Stmt::Throw(e) => {
            rewrite_async_closures_in_expr(e, work, next_local_id, next_func_id)
        }
        Stmt::Return(Some(e)) => {
            rewrite_async_closures_in_expr(e, work, next_local_id, next_func_id)
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            rewrite_async_closures_in_expr(condition, work, next_local_id, next_func_id);
            rewrite_async_closures_in_stmts(then_branch, work, next_local_id, next_func_id);
            if let Some(eb) = else_branch {
                rewrite_async_closures_in_stmts(eb, work, next_local_id, next_func_id);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            rewrite_async_closures_in_expr(condition, work, next_local_id, next_func_id);
            rewrite_async_closures_in_stmts(body, work, next_local_id, next_func_id);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                rewrite_async_closures_in_stmt(i, work, next_local_id, next_func_id);
            }
            if let Some(c) = condition {
                rewrite_async_closures_in_expr(c, work, next_local_id, next_func_id);
            }
            if let Some(u) = update {
                rewrite_async_closures_in_expr(u, work, next_local_id, next_func_id);
            }
            rewrite_async_closures_in_stmts(body, work, next_local_id, next_func_id);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            rewrite_async_closures_in_stmts(body, work, next_local_id, next_func_id);
            if let Some(c) = catch {
                rewrite_async_closures_in_stmts(&mut c.body, work, next_local_id, next_func_id);
            }
            if let Some(f) = finally {
                rewrite_async_closures_in_stmts(f, work, next_local_id, next_func_id);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            rewrite_async_closures_in_expr(discriminant, work, next_local_id, next_func_id);
            for case in cases.iter_mut() {
                if let Some(t) = &mut case.test {
                    rewrite_async_closures_in_expr(t, work, next_local_id, next_func_id);
                }
                rewrite_async_closures_in_stmts(&mut case.body, work, next_local_id, next_func_id);
            }
        }
        Stmt::Labeled { body, .. } => {
            rewrite_async_closures_in_stmt(body, work, next_local_id, next_func_id)
        }
        _ => {}
    }
}

fn rewrite_async_closures_in_expr(
    expr: &mut Expr,
    work: &std::collections::HashSet<perry_hir::types::FuncId>,
    next_local_id: &mut LocalId,
    next_func_id: &mut perry_hir::types::FuncId,
) {
    // Match-and-rewrite at the current level.
    let should_rewrite = if let Expr::Closure {
        func_id,
        is_async,
        is_generator,
        ..
    } = expr
    {
        *is_async && !*is_generator && work.contains(func_id)
    } else {
        false
    };
    if should_rewrite {
        if let Expr::Closure {
            params,
            body,
            captures,
            mutable_captures,
            captures_this,
            captures_new_target,
            enclosing_class,
            is_strict,
            is_async,
            ..
        } = expr
        {
            // Step A: descend into the body first to handle nested async
            // closures bottom-up. After that, the body has no nested async
            // closures-with-awaits remaining (they've been turned into
            // state-machine bodies returning Promises).
            rewrite_async_closures_in_stmts(body, work, next_local_id, next_func_id);

            // Step B: hoist + await→yield over THIS closure's body.
            hoist_awaits_in_stmts(body, next_local_id);
            let mut had_await = false;
            rewrite_stmts(body, &mut had_await);
            if had_await {
                let owned_body = std::mem::take(body);
                let owned_params = params.clone();
                let owned_captures = captures.clone();
                let owned_mutable_captures = mutable_captures.clone();
                // Issue #1021 follow-up: propagate `captures_this` +
                // `enclosing_class` through to the synthesized state-machine
                // helpers so `Expr::This` references in the original body
                // (which end up inlined inside the step closure) still
                // resolve to the outer scope's receiver. Without this, an
                // async arrow inside a class method that uses both `this`
                // AND `await` silently halts (the step closure has
                // captures_this=false and Expr::This doesn't lower).
                let owned_captures_this = *captures_this;
                let owned_captures_new_target = *captures_new_target;
                let owned_enclosing_class = enclosing_class.clone();
                let new_body = crate::generator::transform_plain_async_closure_body(
                    owned_body,
                    &owned_params,
                    &owned_captures,
                    &owned_mutable_captures,
                    owned_captures_this,
                    owned_captures_new_target,
                    owned_enclosing_class,
                    *is_strict,
                    next_local_id,
                    next_func_id,
                );
                *body = new_body;
                *is_async = false;
            }
            return;
        }
    }
    // Otherwise descend into children. NOTE: `walk_expr_children_mut` visits a
    // Closure's PARAM DEFAULTS only, NOT its body — so for a NON-matching
    // closure (non-async, a generator, or async-without-a-direct-await) we must
    // descend into the body explicitly, mirroring the scan pass
    // (`scan_expr_for_async_closures`, which unconditionally descends into every
    // closure body). Without this, an async closure that WAS collected into
    // `work` but is lexically nested inside a non-matching closure is never
    // rewritten to a state machine — it silently falls back to block-wait,
    // which deadlocks when its awaited promise is resolved by a callback the
    // block-wait loop is itself starving.
    if let Expr::Closure { body, .. } = expr {
        rewrite_async_closures_in_stmts(body, work, next_local_id, next_func_id);
    }
    perry_hir::walker::walk_expr_children_mut(expr, &mut |child| {
        rewrite_async_closures_in_expr(child, work, next_local_id, next_func_id);
    });
}

fn alloc_local(next_id: &mut LocalId) -> LocalId {
    let id = *next_id;
    *next_id += 1;
    id
}

// ─── Hoist non-top-level awaits ──────────────────────────────────────────
//
// A "top-level" position is one of:
//   - The full init expression of a `Stmt::Let { init: Some(_) }`
//   - The full operand of a `Stmt::Expr(_)`
//   - The full operand of a `Stmt::Return(Some(_))`
//
// In any other position (an arg of a Call, an operand of a BinOp, an
// element of an Object/Array literal, a condition of an If/While, etc.),
// the `Await` gets hoisted into a fresh `let __await{id} = await <expr>`
// placed immediately before the containing statement, and the original
// site is replaced with `LocalGet(__await{id})`.
//
// We process statements one at a time and use mem::take + Vec splicing to
// insert the hoisted lets. Inner blocks (then/else/while-body/etc.) are
// processed recursively so awaits inside a nested `if (cond) { x = y +
// await z; }` are hoisted into the inner block, not the outer scope.

pub(crate) fn hoist_awaits_in_stmts(stmts: &mut Vec<Stmt>, next_id: &mut LocalId) {
    let mut out: Vec<Stmt> = Vec::with_capacity(stmts.len());
    for stmt in std::mem::take(stmts) {
        let mut hoisted: Vec<Stmt> = Vec::new();
        let new_stmt = hoist_awaits_in_stmt(stmt, next_id, &mut hoisted);
        for h in hoisted {
            out.push(h);
        }
        out.push(new_stmt);
    }
    *stmts = out;
}

fn hoist_awaits_in_stmt(mut stmt: Stmt, next_id: &mut LocalId, hoisted: &mut Vec<Stmt>) -> Stmt {
    match &mut stmt {
        // Top-level positions: don't hoist the *outer* await but do
        // hoist any nested awaits inside the operand.
        Stmt::Let { init: Some(e), .. } => {
            hoist_awaits_avoiding_top_level(e, next_id, hoisted);
        }
        Stmt::Expr(e) => {
            hoist_awaits_avoiding_top_level(e, next_id, hoisted);
        }
        Stmt::Return(Some(e)) => {
            hoist_awaits_avoiding_top_level(e, next_id, hoisted);
        }
        Stmt::Throw(e) => {
            // `throw await x` — we treat this like a return: the outer
            // await stays in place, inner awaits hoisted.
            hoist_awaits_avoiding_top_level(e, next_id, hoisted);
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            // The condition is NOT a top-level await position (it's
            // nested in If) — fully hoist all awaits in it.
            hoist_awaits_in_expr_full(condition, next_id, hoisted);
            hoist_awaits_in_stmts(then_branch, next_id);
            if let Some(eb) = else_branch {
                hoist_awaits_in_stmts(eb, next_id);
            }
        }
        Stmt::While { condition, body } => {
            if expr_contains_await(condition) {
                // JS spec: a `while` condition containing `await` runs the
                // await on EVERY iteration. The old single hoist-before-the-
                // loop evaluated it once (#5933): a truthy first value looped
                // forever on stale state, a falsy one never entered — async
                // drain loops (`while ((v = await q.next()) !== undefined)`)
                // never re-awaited. Restructure instead of hoisting out:
                //
                //   while (true) {
                //     let __loop_cond_await_N = <condition>;
                //     if (!__loop_cond_await_N) break;
                //     <body>
                //   }
                //
                // `continue` re-enters at the condition evaluation (loop
                // top), matching spec; `break` is unchanged. Re-entering the
                // hoist then normalizes the awaits inside the new `let` (and
                // the body) to statement positions INSIDE the loop.
                let cond = std::mem::replace(condition, Expr::Bool(true));
                let taken_body = std::mem::take(body);
                let restructured = restructure_awaited_loop_cond(cond, taken_body, next_id);
                return hoist_awaits_in_stmt(restructured, next_id, hoisted);
            }
            hoist_awaits_in_expr_full(condition, next_id, hoisted);
            hoist_awaits_in_stmts(body, next_id);
        }
        Stmt::DoWhile { body, condition } => {
            if expr_contains_await(condition) {
                // do-while with an awaited condition: apply the linearizer's
                // first-iteration-flag desugar here so the condition lands in
                // a `while` head, then the While arm's restructure applies on
                // re-entry. The flag keeps do-while semantics: the body's
                // first run precedes the first condition evaluation
                // (short-circuited by the flag), and `continue` falls through
                // to the condition evaluation (the flag is already false).
                //
                //   let __dw_first_N = true;          (before the loop)
                //   while (__dw_first_N || <cond>) {  → restructured again
                //     __dw_first_N = false;
                //     <body>
                //   }
                let first = alloc_local(next_id);
                hoisted.push(Stmt::Let {
                    id: first,
                    name: format!("__dw_first_{}", first),
                    ty: Type::Any,
                    mutable: true,
                    init: Some(Expr::Bool(true)),
                });
                let cond = std::mem::replace(condition, Expr::Bool(true));
                let mut new_body = Vec::with_capacity(body.len() + 1);
                new_body.push(Stmt::Expr(Expr::LocalSet(
                    first,
                    Box::new(Expr::Bool(false)),
                )));
                new_body.append(body);
                let while_stmt = Stmt::While {
                    condition: Expr::Logical {
                        op: LogicalOp::Or,
                        left: Box::new(Expr::LocalGet(first)),
                        right: Box::new(cond),
                    },
                    body: new_body,
                };
                return hoist_awaits_in_stmt(while_stmt, next_id, hoisted);
            }
            hoist_awaits_in_stmts(body, next_id);
            hoist_awaits_in_expr_full(condition, next_id, hoisted);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            let cond_awaits = condition.as_ref().is_some_and(|c| expr_contains_await(c));
            let upd_awaits = update.as_ref().is_some_and(|u| expr_contains_await(u));
            // A continue inside try{..}finally{..} is an abrupt completion:
            // the finally must run BEFORE the update (and can override it).
            // prefix_loop_continues would insert the update before the
            // finally, so keep the previous lowering for that rare shape
            // instead of reordering it (CodeRabbit review on #5934).
            let upd_movable =
                upd_awaits && !crate::generator::stmts_have_continue_inside_try_finally(body);
            if cond_awaits || upd_movable {
                // Awaited condition/update must run on every iteration
                // (#5933). Keep the `For` (so `continue` still routes
                // through update-then-condition per spec), but move the
                // awaited slot(s) into the body:
                //   - awaited condition → body-top
                //     `let __t = C; if (!__t) break;` with the For's
                //     condition slot emptied (an empty condition is
                //     always-true);
                //   - awaited update → body-end statement, with every
                //     loop-level `continue` prefixed by the update so
                //     continue → update → condition order holds.
                // The init keeps its run-once semantics via the recursive
                // call's normal For handling.
                let cond_taken = if cond_awaits { condition.take() } else { None };
                let upd_taken = if upd_movable { update.take() } else { None };
                let mut new_body = Vec::with_capacity(body.len() + 3);
                if let Some(c) = cond_taken {
                    let t = alloc_local(next_id);
                    new_body.push(Stmt::Let {
                        id: t,
                        name: format!("__loop_cond_await_{}", t),
                        ty: Type::Any,
                        mutable: true,
                        init: Some(c),
                    });
                    new_body.push(Stmt::If {
                        condition: Expr::Unary {
                            op: UnaryOp::Not,
                            operand: Box::new(Expr::LocalGet(t)),
                        },
                        then_branch: vec![Stmt::Break],
                        else_branch: None,
                    });
                }
                let mut taken_body = std::mem::take(body);
                if let Some(u) = upd_taken {
                    let upd_stmt = Stmt::Expr(u);
                    crate::generator::prefix_loop_continues(
                        &mut taken_body,
                        std::slice::from_ref(&upd_stmt),
                    );
                    new_body.append(&mut taken_body);
                    new_body.push(upd_stmt);
                } else {
                    new_body.append(&mut taken_body);
                }
                let new_for = Stmt::For {
                    init: init.take(),
                    condition: condition.take(),
                    update: update.take(),
                    body: new_body,
                };
                return hoist_awaits_in_stmt(new_for, next_id, hoisted);
            }
            if let Some(i) = init {
                let mut inner_hoisted = Vec::new();
                let i_replaced = hoist_awaits_in_stmt((**i).clone(), next_id, &mut inner_hoisted);
                for h in inner_hoisted {
                    hoisted.push(h);
                }
                **i = i_replaced;
            }
            if let Some(c) = condition {
                hoist_awaits_in_expr_full(c, next_id, hoisted);
            }
            if let Some(u) = update {
                hoist_awaits_in_expr_full(u, next_id, hoisted);
            }
            hoist_awaits_in_stmts(body, next_id);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            hoist_awaits_in_stmts(body, next_id);
            if let Some(c) = catch {
                hoist_awaits_in_stmts(&mut c.body, next_id);
            }
            if let Some(f) = finally {
                hoist_awaits_in_stmts(f, next_id);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            hoist_awaits_in_expr_full(discriminant, next_id, hoisted);
            for case in cases.iter_mut() {
                if let Some(t) = &mut case.test {
                    hoist_awaits_in_expr_full(t, next_id, hoisted);
                }
                hoist_awaits_in_stmts(&mut case.body, next_id);
            }
        }
        Stmt::Labeled { body, .. } => {
            let mut inner = Vec::new();
            let body_taken = std::mem::replace(body.as_mut(), Stmt::Break);
            let new_body = hoist_awaits_in_stmt(body_taken, next_id, &mut inner);
            for h in inner {
                hoisted.push(h);
            }
            **body = new_body;
        }
        _ => {}
    }
    stmt
}

/// Hoist all awaits in an expression INCLUDING any at the top level of
/// the expression itself. Used for non-statement-positioned operands
/// (If condition, While condition, Switch discriminant, etc.).
fn hoist_awaits_in_expr_full(expr: &mut Expr, next_id: &mut LocalId, hoisted: &mut Vec<Stmt>) {
    if matches!(expr, Expr::Closure { .. }) {
        // Don't descend into closure bodies; nested closures are out of
        // scope for the v1 plain-async pre-pass.
        return;
    }
    // A `cond ? await a() : b()` would otherwise have BOTH branches'
    // awaits hoisted unconditionally above the containing statement,
    // executing both calls regardless of `cond` — breaks JS semantics
    // (issue #342). Lift the conditional to a statement-level if/else
    // with a temp before the general hoisting walks into it.
    if matches!(expr, Expr::Conditional { .. }) && conditional_branches_contain_await(expr) {
        lift_conditional_with_await_branches(expr, next_id, hoisted);
        return;
    }
    // A `a || await b()` / `a && await b()` / `a ?? await b()` would
    // otherwise have the RHS await hoisted unconditionally above the
    // containing statement, evaluating `b()` even when `a` short-circuits
    // — breaks JS semantics (issue #5434). Lift it to a guarded if/else
    // with a temp before the general hoisting walks into it.
    if matches!(expr, Expr::Logical { .. }) && logical_rhs_contains_await(expr) {
        lift_logical_with_await_rhs(expr, next_id, hoisted);
        return;
    }
    // A sequence like `this.l = new L, this.p = await this.l.start()`
    // would otherwise have the await hoisted above the containing
    // statement, evaluating the awaited operand BEFORE the sequence's
    // earlier operands ran — the awaited receiver reads its
    // pre-assignment value (issue #5925; minifiers emit this shape
    // constantly). Lift the non-final operands to statements first, in
    // evaluation order, then continue with the final operand.
    if matches!(expr, Expr::Sequence(_)) && expr_contains_await(expr) {
        lift_sequence_with_await(expr, next_id, hoisted);
        hoist_awaits_in_expr_full(expr, next_id, hoisted);
        return;
    }
    // `{ a: f(), v: await p, ...src }` — the object-with-spread lowering
    // wraps the build in a synthetic IIFE, trapping the await inside a
    // NON-async closure the pre-pass would otherwise skip. Inline it so
    // the awaits reach the enclosing async context (see the fn comment).
    if inline_obj_iife_with_await(expr, next_id, hoisted) {
        return;
    }
    // Recurse into children first (innermost-first hoisting).
    perry_hir::walker::walk_expr_children_mut(expr, &mut |child| {
        hoist_awaits_in_expr_full(child, next_id, hoisted);
    });
    if matches!(expr, Expr::Await(_)) {
        let id = alloc_local(next_id);
        let mut original = std::mem::replace(expr, Expr::LocalGet(id));
        // Issue #617: hoist a fetchWithAuth/fetchPostWithAuth operand
        // out of the await BEFORE we push the hoisted let. See the
        // longer comment in `hoist_awaits_avoiding_top_level`.
        hoist_fetch_with_auth_inside_await(&mut original, next_id, hoisted);
        hoisted.push(Stmt::Let {
            id,
            name: format!("__await_{}", id),
            ty: Type::Any,
            mutable: false,
            init: Some(original),
        });
    }
}

/// Issue #617: `await fetchWithAuth(url, auth)` (and the POST variant)
/// returned `undefined` for the inline form while the explicit two-step
/// `let p = fetchWithAuth(...); await p;` produced the resolved Response.
/// The two forms only diverge in HIR shape: the inline form lowers to
/// `Yield { value: FetchGetWithAuth }` (after the await→yield rewrite),
/// so the generator transform plants the `js_fetch_get_with_auth` call
/// inline as the `value` field of the yielded `{value, done}`
/// iter-result object — i.e. the promise is allocated *while* the
/// iter-result object literal is being constructed. The two-step form
/// lowers to `LocalSet(p, FetchGetWithAuth); Yield { value: LocalGet(p) }`
/// — the promise lands in a dominating stack slot first and the yielded
/// object reads it back via a plain load.
///
/// Mechanize the workaround at the HIR level: when the immediate await
/// operand is one of the recognized Promise-producing call shapes, hoist
/// it into a fresh let. The await's operand becomes a LocalGet of that
/// let, which the generator transform plants in the iter-result —
/// matching the working two-step path. Preserves call ordering (the
/// temp's init runs in the same sequence point the inline call would
/// have) and is a no-op for any other await operand.
///
/// Issue #617 (closed in v0.5.749) covered the two `fetchWithAuth`
/// built-ins. Issue #644 expands the coverage: cross-compile +
/// `--enable-js-runtime` exhibits the same inline-await regression on
/// generic `Expr::Call` and `Expr::Method` operands too. The user's
/// verified workaround on that pipeline was a mass-replace of
/// `await X(...)` → `let p = X(...); await p;` for every call site —
/// which is exactly what this hoist now performs. Hoisting any call
/// expression is semantically equivalent (the call still runs in the
/// same sequence point and the await operates on the same Promise);
/// the only side effect is an extra `LocalGet` in the IR.
fn hoist_fetch_with_auth_inside_await(
    await_expr: &mut Expr,
    next_id: &mut LocalId,
    hoisted: &mut Vec<Stmt>,
) {
    let Expr::Await(inner) = await_expr else {
        return;
    };
    let should_hoist = matches!(
        inner.as_ref(),
        Expr::FetchGetWithAuth { .. }
            | Expr::FetchPostWithAuth { .. }
            | Expr::Call { .. }
            | Expr::CallSpread { .. }
            | Expr::NativeMethodCall { .. }
            | Expr::StaticMethodCall { .. }
            | Expr::SuperMethodCall { .. }
    );
    if !should_hoist {
        return;
    }
    // If the operand is already a simple LocalGet (i.e. user wrote the
    // two-step form themselves, or this hoist already fired for this
    // operand earlier in the pass), don't re-hoist — that would
    // introduce a redundant alias and tickle the local-id allocator.
    if matches!(inner.as_ref(), Expr::LocalGet(_)) {
        return;
    }
    let id = alloc_local(next_id);
    let original = std::mem::replace(inner.as_mut(), Expr::LocalGet(id));
    hoisted.push(Stmt::Let {
        id,
        name: format!("__await_fetch_{}", id),
        ty: Type::Any,
        mutable: false,
        init: Some(original),
    });
}

/// Hoist nested awaits but leave a top-level await alone. Used for
/// statement-positioned operands (Let init, Stmt::Expr operand, etc.)
/// where the outer await is something the generator transform handles.
fn hoist_awaits_avoiding_top_level(
    expr: &mut Expr,
    next_id: &mut LocalId,
    hoisted: &mut Vec<Stmt>,
) {
    if let Expr::Await(_) = expr {
        // Outer is an await — keep it but recursively hoist nested awaits
        // inside the operand fully (they are nested, not top-level).
        if let Expr::Await(inner) = expr {
            hoist_awaits_in_expr_full(inner.as_mut(), next_id, hoisted);
        }
        // Issue #617: hoist fetchWithAuth/fetchPostWithAuth operand into
        // a let so the generator transform sees a `Yield(LocalGet(...))`
        // instead of `Yield(FetchGetWithAuth)`. See the comment on
        // `hoist_fetch_with_auth_inside_await` for the full story.
        hoist_fetch_with_auth_inside_await(expr, next_id, hoisted);
        return;
    }
    if matches!(expr, Expr::Closure { .. }) {
        return;
    }
    // Top-level conditional like `let r = cond ? await a() : b();` — see
    // the matching note in `hoist_awaits_in_expr_full`. Lift here too so
    // the await ends up inside an if-branch instead of unconditionally
    // above the let.
    if matches!(expr, Expr::Conditional { .. }) && conditional_branches_contain_await(expr) {
        lift_conditional_with_await_branches(expr, next_id, hoisted);
        return;
    }
    // Top-level logical with an await in the RHS, e.g.
    // `return a || await b();` — see the matching note in
    // `hoist_awaits_in_expr_full`. Lift here too so the RHS await ends up
    // inside a guarded if-branch instead of unconditionally above the
    // statement (issue #5434).
    if matches!(expr, Expr::Logical { .. }) && logical_rhs_contains_await(expr) {
        lift_logical_with_await_rhs(expr, next_id, hoisted);
        return;
    }
    // Top-level sequence with an await, e.g. the minified
    // `this.l = new L, this.p = await this.l.start();` — see the matching
    // note in `hoist_awaits_in_expr_full` (issue #5925). Lift the earlier
    // operands to statements, then re-process the final operand as the new
    // top-level expression (it may itself be a directly-handled await).
    if matches!(expr, Expr::Sequence(_)) && expr_contains_await(expr) {
        lift_sequence_with_await(expr, next_id, hoisted);
        hoist_awaits_avoiding_top_level(expr, next_id, hoisted);
        return;
    }
    // Top-level awaited obj-IIFE, e.g. `return { v: await p, ...src };` —
    // see the matching arm in `hoist_awaits_in_expr_full`.
    if inline_obj_iife_with_await(expr, next_id, hoisted) {
        return;
    }
    // Outer is NOT an await. Children may contain awaits which ARE
    // nested — fully hoist them.
    perry_hir::walker::walk_expr_children_mut(expr, &mut |child| {
        hoist_awaits_in_expr_full(child, next_id, hoisted);
    });
}

/// `{ a: f(), v: await p, ...src }` lowers (lower/expr_object.rs) to a
/// synthetic single-param IIFE (`__perry_obj_iife`) that builds the object —
/// which traps a property-value `await` inside a NON-async closure. The
/// pre-pass (correctly) never descends into closures, so that await stayed
/// raw and codegen's fallback BLOCKED the frame on the re-entrant microtask
/// pump — draining unrelated tasks mid-expression (the Next.js Flight
/// row-reorder: next-intl's provider wrapper has exactly this shape). The
/// IIFE runs exactly once at its own sequence point and HIR closure bodies
/// reference enclosing locals by their original ids, so inline it: bind the
/// seed object to the param's id, replay the body statements in evaluation
/// order (hoisting their awaits into the enclosing async context), and
/// replace the call with a read of the object local. Non-await obj-IIFEs
/// keep the closure form untouched.
fn inline_obj_iife_with_await(
    expr: &mut Expr,
    next_id: &mut LocalId,
    hoisted: &mut Vec<Stmt>,
) -> bool {
    {
        let Expr::Call { callee, args, .. } = &*expr else {
            return false;
        };
        let Expr::Closure {
            params,
            body,
            is_async,
            is_generator,
            ..
        } = callee.as_ref()
        else {
            return false;
        };
        if *is_async
            || *is_generator
            || params.len() != 1
            || args.len() != 1
            || params[0].name != "__perry_obj_iife"
            || !body_contains_await(body)
        {
            return false;
        }
    }
    let Expr::Call { callee, args, .. } = expr else {
        unreachable!()
    };
    let Expr::Closure { params, body, .. } = callee.as_mut() else {
        unreachable!()
    };
    let obj_id = params[0].id;
    let seed = args.remove(0);
    hoisted.push(Stmt::Let {
        id: obj_id,
        name: "__perry_obj_iife".to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(seed),
    });
    let stmts = std::mem::take(body);
    for mut stmt in stmts {
        match &mut stmt {
            // The synthesized body ends with `return __perry_obj_iife` —
            // any return terminates the replay.
            Stmt::Return(_) => break,
            Stmt::Expr(e) => {
                hoist_awaits_in_expr_full(e, next_id, hoisted);
                hoisted.push(stmt);
            }
            // Lowered obj-IIFE bodies are flat Expr/Return statements today;
            // forward anything else untouched.
            _ => hoisted.push(stmt),
        }
    }
    *expr = Expr::LocalGet(obj_id);
    true
}

/// Lift a sequence (comma) expression's non-final operands into statements
/// pushed onto `hoisted`, leaving `expr` as the final operand (issue #5925).
///
/// Each lifted operand is itself fully hoisted first, so awaits inside
/// earlier operands suspend in evaluation order, before anything in the
/// final operand. With the operands lifted, the `let __await_N = await …`
/// the caller hoists for the final operand lands AFTER them — preserving JS
/// comma semantics (`a = new X, b = await a.m()` must run the assignment
/// before evaluating `a.m()`), where previously the await jumped the whole
/// sequence.
fn lift_sequence_with_await(expr: &mut Expr, next_id: &mut LocalId, hoisted: &mut Vec<Stmt>) {
    let Expr::Sequence(ops) = expr else {
        return;
    };
    let mut ops = std::mem::take(ops);
    let Some(last) = ops.pop() else {
        *expr = Expr::Undefined;
        return;
    };
    for mut op in ops {
        hoist_awaits_in_expr_full(&mut op, next_id, hoisted);
        hoisted.push(Stmt::Expr(op));
    }
    *expr = last;
}

/// Returns true if either branch of `expr` (assumed `Expr::Conditional`)
/// contains an `Expr::Await`, anywhere except inside nested closures.
fn conditional_branches_contain_await(expr: &Expr) -> bool {
    if let Expr::Conditional {
        then_expr,
        else_expr,
        ..
    } = expr
    {
        return expr_contains_await(then_expr) || expr_contains_await(else_expr);
    }
    false
}

fn expr_contains_await(expr: &Expr) -> bool {
    if matches!(expr, Expr::Await(_)) {
        return true;
    }
    if let Expr::Closure { params, body, .. } = expr {
        // The synthetic object-with-spread IIFE executes inline at its own
        // sequence point — an await inside it IS an await of the enclosing
        // function (`inline_obj_iife_with_await` surfaces it during the
        // hoist). Every other closure owns its awaits.
        if params.len() == 1 && params[0].name == "__perry_obj_iife" {
            return body_contains_await(body);
        }
        return false;
    }
    let mut found = false;
    perry_hir::walker::walk_expr_children(expr, &mut |child| {
        if !found && expr_contains_await(child) {
            found = true;
        }
    });
    found
}

/// Build the per-iteration form for a loop condition containing `await`
/// (#5933):
///
///   while (true) {
///     let __loop_cond_await_N = <cond>;
///     if (!__loop_cond_await_N) break;
///     <body>
///   }
///
/// The caller re-enters the hoist on the result so the awaits inside the
/// new `let` (a top-level-await-friendly position) and the body are
/// normalized to statement form INSIDE the loop.
fn restructure_awaited_loop_cond(cond: Expr, body: Vec<Stmt>, next_id: &mut LocalId) -> Stmt {
    let t = alloc_local(next_id);
    let mut new_body = Vec::with_capacity(body.len() + 2);
    new_body.push(Stmt::Let {
        id: t,
        name: format!("__loop_cond_await_{}", t),
        ty: Type::Any,
        mutable: true,
        init: Some(cond),
    });
    new_body.push(Stmt::If {
        condition: Expr::Unary {
            op: UnaryOp::Not,
            operand: Box::new(Expr::LocalGet(t)),
        },
        then_branch: vec![Stmt::Break],
        else_branch: None,
    });
    new_body.extend(body);
    Stmt::While {
        condition: Expr::Bool(true),
        body: new_body,
    }
}

/// Replace `cond ? then_e : else_e` (where then_e or else_e contains an
/// await) with `LocalGet(__cond_await_N)`, and emit before the containing
/// statement:
///
///   let __cond_await_N: any;
///   if (cond) { __cond_await_N = then_e; } else { __cond_await_N = else_e; }
///
/// Awaits inside each branch's `LocalSet` are then hoisted by the recursive
/// `hoist_awaits_in_stmts` call so they end up at the top of their own
/// if-branch — the position the await→yield rewrite expects.
fn lift_conditional_with_await_branches(
    expr: &mut Expr,
    next_id: &mut LocalId,
    hoisted: &mut Vec<Stmt>,
) {
    let temp_id = alloc_local(next_id);
    let owned = std::mem::replace(expr, Expr::LocalGet(temp_id));
    if let Expr::Conditional {
        condition,
        then_expr,
        else_expr,
    } = owned
    {
        hoisted.push(Stmt::Let {
            id: temp_id,
            name: format!("__cond_await_{}", temp_id),
            ty: Type::Any,
            mutable: true,
            init: None,
        });

        let mut then_branch = vec![Stmt::Expr(Expr::LocalSet(temp_id, then_expr))];
        hoist_awaits_in_stmts(&mut then_branch, next_id);

        let mut else_branch = vec![Stmt::Expr(Expr::LocalSet(temp_id, else_expr))];
        hoist_awaits_in_stmts(&mut else_branch, next_id);

        hoisted.push(Stmt::If {
            condition: *condition,
            then_branch,
            else_branch: Some(else_branch),
        });
    }
}

/// Returns true if the RIGHT operand of `expr` (assumed `Expr::Logical`)
/// contains an `Expr::Await`, anywhere except inside nested closures. The
/// LEFT operand is always evaluated, so an await there is fine to hoist
/// normally — only a guarded RHS await needs the short-circuit lift.
fn logical_rhs_contains_await(expr: &Expr) -> bool {
    if let Expr::Logical { right, .. } = expr {
        return expr_contains_await(right);
    }
    false
}

/// Replace `left || right` / `left && right` / `left ?? right` (where
/// `right` contains an await) with `LocalGet(__logic_await_N)`, and emit
/// before the containing statement:
///
///   let __logic_await_N: any;
///   __logic_await_N = left;
///   if (<short-circuit guard>) { __logic_await_N = right; }
///
/// The guard runs `right` only when `left` does NOT already decide the
/// result, preserving short-circuit semantics:
///   ||  →  if (!__logic_await_N)          (left falsy)
///   &&  →  if (__logic_await_N)           (left truthy)
///   ??  →  if (__logic_await_N == null)   (left null/undefined)
///
/// Awaits inside `left`'s assignment and inside the guarded branch are then
/// hoisted by the recursive `hoist_awaits_in_stmts` calls so each lands at a
/// top-level position the await→yield rewrite expects. Without this lift the
/// general hoisting walk pulls `right`'s await unconditionally above the
/// statement, evaluating it even when `left` short-circuits (issue #5434).
fn lift_logical_with_await_rhs(expr: &mut Expr, next_id: &mut LocalId, hoisted: &mut Vec<Stmt>) {
    let temp_id = alloc_local(next_id);
    let owned = std::mem::replace(expr, Expr::LocalGet(temp_id));
    if let Expr::Logical { op, left, right } = owned {
        hoisted.push(Stmt::Let {
            id: temp_id,
            name: format!("__logic_await_{}", temp_id),
            ty: Type::Any,
            mutable: true,
            init: None,
        });

        // __logic_await_N = left;  — any awaits in `left` are unconditional,
        // so hoist them normally before the guard.
        let mut left_stmts = vec![Stmt::Expr(Expr::LocalSet(temp_id, left))];
        hoist_awaits_in_stmts(&mut left_stmts, next_id);
        hoisted.extend(left_stmts);

        // Short-circuit guard on the stored left value.
        let guard = match op {
            LogicalOp::Or => Expr::Unary {
                op: UnaryOp::Not,
                operand: Box::new(Expr::LocalGet(temp_id)),
            },
            LogicalOp::And => Expr::LocalGet(temp_id),
            LogicalOp::Coalesce => Expr::Compare {
                op: CompareOp::LooseEq,
                left: Box::new(Expr::LocalGet(temp_id)),
                right: Box::new(Expr::Null),
            },
        };

        // if (guard) { __logic_await_N = right; }  — `right`'s awaits get
        // hoisted into the branch so they only fire when the guard passes.
        let mut then_branch = vec![Stmt::Expr(Expr::LocalSet(temp_id, right))];
        hoist_awaits_in_stmts(&mut then_branch, next_id);

        hoisted.push(Stmt::If {
            condition: guard,
            then_branch,
            else_branch: None,
        });
    }
}

// ─── Rewrite await → yield ───────────────────────────────────────────────
//
// Runs after hoisting, so every Await is now in a top-level position the
// generator transform can split states at.

fn rewrite_stmts(stmts: &mut [Stmt], had_await: &mut bool) {
    for stmt in stmts.iter_mut() {
        rewrite_stmt(stmt, had_await);
    }
}

fn rewrite_stmt(stmt: &mut Stmt, had_await: &mut bool) {
    match stmt {
        Stmt::Let { init: Some(e), .. } => rewrite_expr(e, had_await),
        Stmt::Expr(e) => rewrite_expr(e, had_await),
        Stmt::Return(Some(e)) => rewrite_expr(e, had_await),
        Stmt::Throw(e) => rewrite_expr(e, had_await),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            rewrite_expr(condition, had_await);
            rewrite_stmts(then_branch, had_await);
            if let Some(eb) = else_branch {
                rewrite_stmts(eb, had_await);
            }
        }
        Stmt::While { condition, body } => {
            rewrite_expr(condition, had_await);
            rewrite_stmts(body, had_await);
        }
        Stmt::DoWhile { body, condition } => {
            rewrite_stmts(body, had_await);
            rewrite_expr(condition, had_await);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                rewrite_stmt(i, had_await);
            }
            if let Some(c) = condition {
                rewrite_expr(c, had_await);
            }
            if let Some(u) = update {
                rewrite_expr(u, had_await);
            }
            rewrite_stmts(body, had_await);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            rewrite_stmts(body, had_await);
            if let Some(c) = catch {
                rewrite_stmts(&mut c.body, had_await);
            }
            if let Some(f) = finally {
                rewrite_stmts(f, had_await);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            rewrite_expr(discriminant, had_await);
            for case in cases.iter_mut() {
                rewrite_stmts(&mut case.body, had_await);
            }
        }
        Stmt::Labeled { body, .. } => rewrite_stmt(body, had_await),
        _ => {}
    }
}

// ─── #691 Phase 3: strip `await Promise.resolve(<non-Promise>)` ─────────
//
// Detects the `await Promise.resolve(arg)` source pattern (HIR shape:
// `Await(Call(PropertyGet(GlobalGet(0), "resolve"), [arg]))`) and rewrites
// to `Await(arg)` whenever `arg` is statically provable to be non-Promise.
// Tracks non-Promise locals via param types + propagation through let
// inits (including the await results: `let x = await Promise.resolve(y)`
// where y is non-Promise → x is non-Promise after strip).
//
// Spec-equivalence: for non-Promise `arg`, `await arg` and
// `await Promise.resolve(arg)` both take exactly one microtask hop and
// resolve to `arg` itself. The probe suite's only divergence case is
// probe 05 (`await Promise.resolve(<Promise>)` is 2 hops vs `await <Promise>`
// is 1 hop) — `is_non_promise_expr` excludes anything that could carry a
// Promise (Calls, Any/Unknown locals, etc.), so probe 05 stays correct.

fn strip_redundant_promise_resolve_in_func(func: &mut Function) {
    let mut non_promise: HashSet<LocalId> = HashSet::new();
    for param in &func.params {
        if is_non_promise_type(&param.ty) {
            non_promise.insert(param.id);
        }
    }
    strip_in_stmts(&mut func.body, &mut non_promise);
}

fn strip_in_stmts(stmts: &mut [Stmt], non_promise: &mut HashSet<LocalId>) {
    for stmt in stmts {
        strip_in_stmt(stmt, non_promise);
    }
}

fn strip_in_stmt(stmt: &mut Stmt, non_promise: &mut HashSet<LocalId>) {
    match stmt {
        Stmt::Let { id, ty, init, .. } => {
            if let Some(init_expr) = init {
                strip_in_expr(init_expr, non_promise);
                if is_non_promise_type(ty) || is_non_promise_expr(init_expr, non_promise) {
                    non_promise.insert(*id);
                }
            } else if is_non_promise_type(ty) {
                non_promise.insert(*id);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => strip_in_expr(e, non_promise),
        Stmt::Return(Some(e)) => strip_in_expr(e, non_promise),
        Stmt::Return(None)
        | Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_) => {}
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            strip_in_expr(condition, non_promise);
            strip_in_stmts(then_branch, non_promise);
            if let Some(eb) = else_branch {
                strip_in_stmts(eb, non_promise);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            strip_in_expr(condition, non_promise);
            strip_in_stmts(body, non_promise);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init_stmt) = init {
                strip_in_stmt(init_stmt, non_promise);
            }
            if let Some(c) = condition {
                strip_in_expr(c, non_promise);
            }
            if let Some(u) = update {
                strip_in_expr(u, non_promise);
            }
            strip_in_stmts(body, non_promise);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            strip_in_stmts(body, non_promise);
            if let Some(c) = catch {
                strip_in_stmts(&mut c.body, non_promise);
            }
            if let Some(f) = finally {
                strip_in_stmts(f, non_promise);
            }
        }
        Stmt::Labeled { body, .. } => strip_in_stmt(body, non_promise),
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            strip_in_expr(discriminant, non_promise);
            for case in cases {
                if let Some(c) = &mut case.test {
                    strip_in_expr(c, non_promise);
                }
                strip_in_stmts(&mut case.body, non_promise);
            }
        }
        _ => {}
    }
}

fn strip_in_expr(expr: &mut Expr, non_promise: &HashSet<LocalId>) {
    // Don't descend into nested closures — they have their own scope and
    // their own param/local set. Nested async closures are rewritten
    // independently by the async-closure pass (phase 2).
    if matches!(expr, Expr::Closure { .. }) {
        return;
    }
    perry_hir::walker::walk_expr_children_mut(expr, &mut |c| strip_in_expr(c, non_promise));
    if let Expr::Await(inner) = expr {
        if let Some(stripped) = try_strip_promise_resolve(inner, non_promise) {
            **inner = stripped;
        }
    }
}

fn try_strip_promise_resolve(expr: &Expr, non_promise: &HashSet<LocalId>) -> Option<Expr> {
    let Expr::Call { callee, args, .. } = expr else {
        return None;
    };
    if args.len() != 1 {
        return None;
    }
    let Expr::PropertyGet {
        object, property, ..
    } = callee.as_ref()
    else {
        return None;
    };
    if property != "resolve" {
        return None;
    }
    // The `Promise` global is `GlobalGet(0)` (see build_async_step_driver_direct
    // for the convention).
    if !matches!(object.as_ref(), Expr::GlobalGet(0)) {
        return None;
    }
    if is_non_promise_expr(&args[0], non_promise) {
        Some(args[0].clone())
    } else {
        None
    }
}

fn is_non_promise_expr(expr: &Expr, non_promise: &HashSet<LocalId>) -> bool {
    match expr {
        Expr::Number(_)
        | Expr::Integer(_)
        | Expr::String(_)
        | Expr::Bool(_)
        | Expr::Undefined
        | Expr::Null => true,
        Expr::LocalGet(id) => non_promise.contains(id),
        Expr::Binary { left, right, .. } => {
            is_non_promise_expr(left, non_promise) && is_non_promise_expr(right, non_promise)
        }
        Expr::Unary { operand, .. } => is_non_promise_expr(operand, non_promise),
        Expr::Compare { left, right, .. } => {
            is_non_promise_expr(left, non_promise) && is_non_promise_expr(right, non_promise)
        }
        Expr::Logical { left, right, .. } => {
            // Logical && / || / ?? return one of the operands. If both are
            // non-Promise, the result is non-Promise.
            is_non_promise_expr(left, non_promise) && is_non_promise_expr(right, non_promise)
        }
        // `await X` for non-Promise X resolves to X itself (1 microtask hop),
        // so the result is non-Promise. The peephole above handles the
        // Promise.resolve(non-Promise) sub-case before we even ask here.
        Expr::Await(inner) => is_non_promise_expr(inner, non_promise),
        _ => false,
    }
}

fn is_non_promise_type(ty: &Type) -> bool {
    match ty {
        Type::Number
        | Type::Int32
        | Type::Boolean
        | Type::String
        | Type::Void
        | Type::Null
        | Type::BigInt
        | Type::Symbol => true,
        Type::Promise(_) => false,
        // Any/Unknown could carry a Promise at runtime.
        // Object/Function could be a thenable. Named/Generic could resolve
        // to Promise. Stay conservative and don't strip.
        _ => false,
    }
}

fn rewrite_expr(expr: &mut Expr, had_await: &mut bool) {
    if matches!(expr, Expr::Await(_)) {
        *had_await = true;
        if let Expr::Await(inner) = std::mem::replace(expr, Expr::Undefined) {
            let mut inner = *inner;
            rewrite_expr(&mut inner, had_await);
            *expr = Expr::Yield {
                value: Some(Box::new(inner)),
                delegate: false,
            };
        }
        return;
    }
    if matches!(expr, Expr::Closure { .. }) {
        return;
    }
    perry_hir::walker::walk_expr_children_mut(expr, &mut |e| rewrite_expr(e, had_await));
}

// ─── #1021: async-closure detection ────────────────────────────────────────
//
// Walks the entire HIR (top-level fn bodies, class members, module init, and
// recursively through nested closures) collecting func_ids of every
// `Expr::Closure { is_async: true }` whose body contains at least one
// `Expr::Await`. These are the closures that today lower to a busy-wait at
// `expr.rs:10588` and deadlock self-fetch from inside a V8 trampoline
// (issue #1021).
//
// Phase 1 (this commit): populate `module.async_step_closures` so the rest
// of the compiler can see the set. No HIR rewriting yet — `compile_closure`
// reads the set but does not act on it.
//
// Phase 2 (follow-up): rewrite each detected closure's body via
// `hoist_awaits_in_stmts` + await→yield, then run the generator
// state-machine transform on the body so the closure returns a Promise
// immediately and resumes via `js_async_step_chain` / `Task::AsyncStep`.
//
// Phase 3 (follow-up): `compile_closure` emits the wrapped form when the
// closure's func_id is in `module.async_step_closures`.
fn collect_async_step_closures(module: &mut Module) {
    let mut found: std::collections::HashSet<perry_hir::types::FuncId> =
        std::collections::HashSet::new();
    for func in &module.functions {
        scan_stmts_for_async_closures(&func.body, &mut found);
    }
    for stmt in &module.init {
        scan_stmt_for_async_closures(stmt, &mut found);
    }
    for class in &module.classes {
        for m in &class.methods {
            scan_stmts_for_async_closures(&m.body, &mut found);
        }
        for m in &class.static_methods {
            scan_stmts_for_async_closures(&m.body, &mut found);
        }
        if let Some(ctor) = &class.constructor {
            scan_stmts_for_async_closures(&ctor.body, &mut found);
        }
        for getter in &class.getters {
            scan_stmts_for_async_closures(&getter.1.body, &mut found);
        }
        for setter in &class.setters {
            scan_stmts_for_async_closures(&setter.1.body, &mut found);
        }
        // #5854: async closures reachable ONLY through computed-key members
        // (`async [k]() {…}` / `[k] = async () => …`), instance FIELD
        // initializers (`f = async () => …`), and static-field initializers.
        // The rewrite pass walks these same containers but only rewrites a
        // closure whose FuncId is in this set — so without scanning them here
        // the rewrite is a no-op and the closure stays a raw block-waiting
        // async fn (the exact dead-code the computed-members rewrite was before
        // this scan). Safe against the #212/#5143 id-collision class:
        // `compute_max_local_id`/`compute_max_func_id` (id_scan.rs) already
        // scan these exact containers, so the state machine's fresh
        // state/done/sent ids always clear a field/computed-member closure's.
        for member in &class.computed_members {
            scan_expr_for_async_closures(&member.key_expr, &mut found);
            scan_stmts_for_async_closures(&member.function.body, &mut found);
        }
        for field in class.fields.iter().chain(class.static_fields.iter()) {
            if let Some(init) = &field.init {
                scan_expr_for_async_closures(init, &mut found);
            }
            if let Some(key_expr) = &field.key_expr {
                scan_expr_for_async_closures(key_expr, &mut found);
            }
        }
    }
    module.async_step_closures = found;
}

fn scan_stmts_for_async_closures(
    stmts: &[Stmt],
    found: &mut std::collections::HashSet<perry_hir::types::FuncId>,
) {
    for s in stmts {
        scan_stmt_for_async_closures(s, found);
    }
}

fn scan_stmt_for_async_closures(
    stmt: &Stmt,
    found: &mut std::collections::HashSet<perry_hir::types::FuncId>,
) {
    match stmt {
        Stmt::Let { init: Some(e), .. } => scan_expr_for_async_closures(e, found),
        Stmt::Expr(e) | Stmt::Throw(e) => scan_expr_for_async_closures(e, found),
        Stmt::Return(Some(e)) => scan_expr_for_async_closures(e, found),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            scan_expr_for_async_closures(condition, found);
            scan_stmts_for_async_closures(then_branch, found);
            if let Some(eb) = else_branch {
                scan_stmts_for_async_closures(eb, found);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            scan_expr_for_async_closures(condition, found);
            scan_stmts_for_async_closures(body, found);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                scan_stmt_for_async_closures(i, found);
            }
            if let Some(c) = condition {
                scan_expr_for_async_closures(c, found);
            }
            if let Some(u) = update {
                scan_expr_for_async_closures(u, found);
            }
            scan_stmts_for_async_closures(body, found);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            scan_stmts_for_async_closures(body, found);
            if let Some(c) = catch {
                scan_stmts_for_async_closures(&c.body, found);
            }
            if let Some(f) = finally {
                scan_stmts_for_async_closures(f, found);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            scan_expr_for_async_closures(discriminant, found);
            for case in cases {
                if let Some(t) = &case.test {
                    scan_expr_for_async_closures(t, found);
                }
                scan_stmts_for_async_closures(&case.body, found);
            }
        }
        Stmt::Labeled { body, .. } => scan_stmt_for_async_closures(body, found),
        _ => {}
    }
}

fn scan_expr_for_async_closures(
    expr: &Expr,
    found: &mut std::collections::HashSet<perry_hir::types::FuncId>,
) {
    if let Expr::Closure {
        func_id,
        body,
        is_async,
        is_generator,
        ..
    } = expr
    {
        if *is_async && !*is_generator && body_contains_await(body) {
            found.insert(*func_id);
        }
        // Descend into the closure body too — nested async closures inside
        // an outer closure body are independent candidates.
        scan_stmts_for_async_closures(body, found);
        return;
    }
    perry_hir::walker::walk_expr_children(expr, &mut |e| scan_expr_for_async_closures(e, found));
}

fn body_contains_await(stmts: &[Stmt]) -> bool {
    let mut found = false;
    for s in stmts {
        if stmt_contains_await(s, &mut found) {
            return true;
        }
        if found {
            return true;
        }
    }
    false
}

fn stmt_contains_await(stmt: &Stmt, found: &mut bool) -> bool {
    if *found {
        return true;
    }
    match stmt {
        Stmt::Let { init: Some(e), .. } => expr_contains_await_shallow(e, found),
        Stmt::Expr(e) | Stmt::Throw(e) => expr_contains_await_shallow(e, found),
        Stmt::Return(Some(e)) => expr_contains_await_shallow(e, found),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expr_contains_await_shallow(condition, found)
                || body_contains_await(then_branch)
                || else_branch
                    .as_ref()
                    .is_some_and(|eb| body_contains_await(eb))
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            expr_contains_await_shallow(condition, found) || body_contains_await(body)
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_ref().is_some_and(|i| stmt_contains_await(i, found))
                || condition
                    .as_ref()
                    .is_some_and(|c| expr_contains_await_shallow(c, found))
                || update
                    .as_ref()
                    .is_some_and(|u| expr_contains_await_shallow(u, found))
                || body_contains_await(body)
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            body_contains_await(body)
                || catch.as_ref().is_some_and(|c| body_contains_await(&c.body))
                || finally.as_ref().is_some_and(|f| body_contains_await(f))
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            expr_contains_await_shallow(discriminant, found)
                || cases.iter().any(|c| body_contains_await(&c.body))
        }
        Stmt::Labeled { body, .. } => stmt_contains_await(body, found),
        _ => false,
    }
}

/// Shallow: matches `Expr::Await` at any depth in this expression, but
/// STOPS at nested closures — an `await` inside a different closure
/// belongs to that closure's body, not the current one.
fn expr_contains_await_shallow(expr: &Expr, found: &mut bool) -> bool {
    if *found {
        return true;
    }
    if matches!(expr, Expr::Await(_)) {
        *found = true;
        return true;
    }
    if let Expr::Closure { params, body, .. } = expr {
        // See `expr_contains_await`: the synthetic object-with-spread IIFE's
        // awaits belong to the enclosing function — without this, an async
        // closure whose ONLY awaits sit in an obj-IIFE never enters the
        // collect work set, stays un-rewritten, and its raw awaits BLOCK the
        // frame on the busy-wait pump (next-intl's provider wrapper in the
        // Next.js Flight row-reorder).
        if params.len() == 1 && params[0].name == "__perry_obj_iife" && body_contains_await(body) {
            *found = true;
            return true;
        }
        return false;
    }
    perry_hir::walker::walk_expr_children(expr, &mut |e| {
        if !*found {
            expr_contains_await_shallow(e, found);
        }
    });
    *found
}

#[cfg(test)]
mod computed_and_field_async_tests {
    use super::*;

    // The minimal shape the collect scan matches: `async () => { await 1 }`
    // — `Expr::Closure { is_async, !is_generator }` whose body has an Await.
    fn async_closure_with_await(func_id: perry_hir::types::FuncId) -> Expr {
        Expr::Closure {
            func_id,
            params: Vec::new(),
            return_type: Type::Any,
            body: vec![Stmt::Expr(Expr::Await(Box::new(Expr::Integer(1))))],
            captures: Vec::new(),
            mutable_captures: Vec::new(),
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: true,
            is_async: true,
            is_generator: false,
            is_strict: false,
        }
    }

    fn empty_fn(id: perry_hir::types::FuncId, body: Vec<Stmt>) -> Function {
        Function {
            id,
            name: String::new(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Type::Any,
            body,
            is_async: false,
            is_generator: false,
            is_strict: false,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        }
    }

    fn empty_class(name: &str) -> Class {
        Class {
            id: 1,
            name: name.to_string(),
            type_params: Vec::new(),
            extends: None,
            extends_name: None,
            native_extends: None,
            extends_expr: None,
            heritage_lexically_shadowed: false,
            fields: Vec::new(),
            constructor: None,
            methods: Vec::new(),
            getters: Vec::new(),
            setters: Vec::new(),
            static_accessor_names: Vec::new(),
            static_accessor_fn_ids: Vec::new(),
            computed_members: Vec::new(),
            static_fields: Vec::new(),
            static_methods: Vec::new(),
            decorators: Vec::new(),
            is_exported: false,
            aliases: Vec::new(),
            is_nested: false,
        }
    }

    fn field_with_init(name: &str, init: Expr) -> ClassField {
        ClassField {
            name: name.to_string(),
            key_expr: None,
            ty: Type::Any,
            init: Some(init),
            is_private: false,
            is_readonly: false,
            decorators: Vec::new(),
        }
    }

    // `h = async () => await 1` as an INSTANCE field: before #5854's collect
    // extension the scan skipped `class.fields`, so this closure's FuncId never
    // entered `async_step_closures`, the rewrite pass (which filters on that
    // set) skipped it, and it stayed a raw block-waiting async fn. It must now
    // be BOTH collected and CPS-rewritten to a generator.
    #[test]
    fn async_closure_in_instance_field_is_collected_and_rewritten() {
        let mut module = Module::new("test");
        let mut class = empty_class("C");
        class
            .fields
            .push(field_with_init("h", async_closure_with_await(50)));
        module.classes.push(class);

        transform_async_to_generator(&mut module);

        assert!(
            module.async_step_closures.contains(&50),
            "instance-field async closure FuncId must be collected"
        );
        // An async CLOSURE with awaits is rewritten in place: its body becomes a
        // state machine (via transform_plain_async_closure_body) and `is_async`
        // is cleared. (Unlike a top-level async fn, it is NOT re-flagged as a
        // generator — the transformed body IS the driver.) A cleared `is_async`
        // is the definitive signal the rewrite fired rather than falling back to
        // raw block-wait.
        match &module.classes[0].fields[0].init {
            Some(Expr::Closure { is_async, .. }) => assert!(
                !*is_async,
                "field async closure must be CPS-rewritten (is_async cleared)"
            ),
            other => panic!("field init should still be a Closure, got {other:?}"),
        }
    }

    // Companion: a STATIC field initializer is a separate container the collect
    // scan also skipped pre-#5854.
    #[test]
    fn async_closure_in_static_field_is_collected() {
        let mut module = Module::new("test");
        let mut class = empty_class("C");
        class
            .static_fields
            .push(field_with_init("h", async_closure_with_await(60)));
        module.classes.push(class);

        transform_async_to_generator(&mut module);

        assert!(
            module.async_step_closures.contains(&60),
            "static-field async closure FuncId must be collected"
        );
    }

    // A computed-key member body (`[0]() { async () => await 1 }`). The rewrite
    // loop already walked `computed_members` (commit f80652ad0) but the collect
    // scan did not, so the id set it filters on never listed the closure and the
    // walk was dead. With both sides covering computed_members it works.
    #[test]
    fn async_closure_in_computed_member_body_is_collected() {
        let mut module = Module::new("test");
        let mut class = empty_class("C");
        class.computed_members.push(ClassComputedMember {
            key_expr: Expr::Integer(0),
            function: empty_fn(2, vec![Stmt::Expr(async_closure_with_await(70))]),
            is_static: false,
            kind: ClassComputedMemberKind::Method,
        });
        module.classes.push(class);

        transform_async_to_generator(&mut module);

        assert!(
            module.async_step_closures.contains(&70),
            "computed-member-body async closure FuncId must be collected"
        );
    }
}
