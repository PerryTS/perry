//! Generator-function state-machine lowering and async step-driver construction.
//!
//! The big `transform_generator_function*` entry points live here; cohesive
//! helper groups are split into sibling modules under `lower/`.

use super::*;

mod abrupt;
mod async_step;
mod call_this;
mod resume;
mod yield_await;

// Re-export the moved items that the trunk (and sibling modules, via
// `use super::*`) reference. Globs do not propagate transitively in this
// repo, so spell every cross-module symbol explicitly.
pub(crate) use abrupt::{
    build_abrupt_routing, build_async_catch_route_body, build_async_throw_body,
    build_completion_resume_stmts, build_dispatch_catch_handler, build_finally_run_stmts,
    build_yield_star_return_routes, build_yield_star_throw_routes, catch_route_condition,
    finally_abrupt_condition, finally_route_condition, rewrite_dispatch_continue_to_suspend,
    wrap_dispatch_loop,
};
pub(crate) use async_step::{
    build_async_catch_route_body_direct, build_async_step_driver_direct,
    build_async_throw_body_direct,
};
pub(crate) use call_this::{
    generator_body_uses_call_this, generator_expr_uses_call_this, generator_stmt_uses_call_this,
};
pub(crate) use resume::{
    generator_executing_guard, generator_executing_type_error, generator_resume_rethrow,
    prepend_executing_clear_before_returns, promise_reject, wrap_async_gen_step_body,
    wrap_generator_resume_body,
};
pub(crate) use yield_await::await_async_generator_yield_operands;

/// #6709: read the currently-running step closure (`Expr::CurrentStepClosure`).
fn current_step() -> Expr {
    Expr::CurrentStepClosure
}

/// #6709: `AsyncStepChain(value, __step_self)` — suspend the async-generator
/// activation on the microtask queue (an inner `await`) and resume the step
/// when `value` settles.
fn async_step_chain(value: Expr) -> Expr {
    Expr::AsyncStepChain {
        value: Box::new(value),
        step_closure: Box::new(current_step()),
    }
}

/// #6709: `AsyncStepDone({value, done}, __step_self)` — settle THIS activation's
/// result Promise with the iterator-result object (a consumer `yield` or the
/// generator's completion) and stop, leaving the state machine suspended for
/// the next `.next()`.
fn async_step_resolve(iter_result: Expr) -> Expr {
    Expr::AsyncStepDone {
        value: Box::new(iter_result),
        step_closure: Box::new(current_step()),
    }
}

/// #6709: Build the `while (true) { <state dispatch> }` body over `states`.
///
/// `async_step = false` reproduces the historical sync-generator / busy-wait
/// shape: yield/done states `return {value, done}`, and (async-generator only)
/// `await` states busy-wait inline (`__sent = await value; continue`). This
/// feeds the `.return()` closure and sync generators unchanged.
///
/// `async_step = true` is the async-generator suspend shape: `await` states
/// `return AsyncStepChain(value, __step_self)` (suspend on the microtask
/// queue); yield/done states still emit `return {value, done}` here — the
/// caller runs [`wrap_iter_result_returns_in_async_step_done`] over the FINAL
/// step body afterward to convert every iter-result return into an
/// `AsyncStepDone` that settles the activation's result Promise. Splitting it
/// this way lets the wrap also cover returns that `wrap_dispatch_loop` and the
/// throw-routing inject later.
#[allow(clippy::too_many_arguments)]
fn build_dispatch_while_body(
    states: &[State],
    async_step: bool,
    state_id: LocalId,
    done_id: LocalId,
    sent_id: LocalId,
) -> Vec<Stmt> {
    let mut while_body: Vec<Stmt> = Vec::new();
    for state in states {
        let num = state.num;
        let mut case_body = state.body.clone();
        match &state.exit {
            StateExit::Yield { value, next_state } => {
                if body_contains_return(&case_body) {
                    prepend_done_before_returns(&mut case_body, done_id);
                    rewrite_returns_as_done(&mut case_body);
                }
                case_body.push(Stmt::Expr(Expr::LocalSet(
                    state_id,
                    Box::new(Expr::Number(*next_state as f64)),
                )));
                case_body.push(Stmt::Return(Some(make_iter_result(value.clone(), false))));
            }
            StateExit::Await { value, next_state } => {
                // A user `return X` can sit in a yield/await-free control-flow
                // block (e.g. `if (c) return X;`) that the linearizer's catch-all
                // accumulated into this Await state's body ahead of the `await`.
                // Rewrite it exactly like the Yield/Goto/Done arms so the step
                // closure settles via an iter-result completion (which the later
                // `wrap_iter_result_returns_in_async_step_done` pass converts to
                // `AsyncStepDone`) instead of escaping as a raw return with the
                // wrong completion shape / a stale `__done` flag.
                if body_contains_return(&case_body) {
                    prepend_done_before_returns(&mut case_body, done_id);
                    rewrite_returns_as_done(&mut case_body);
                }
                case_body.push(Stmt::Expr(Expr::LocalSet(
                    state_id,
                    Box::new(Expr::Number(*next_state as f64)),
                )));
                if async_step {
                    // Suspend on the microtask queue; resume delivers the
                    // settled value through `__sent`.
                    case_body.push(Stmt::Return(Some(async_step_chain(value.clone()))));
                } else {
                    // Busy-wait fallback (the `.return()` continuation path):
                    // evaluate the await inline and continue to the next state.
                    case_body.push(Stmt::Expr(Expr::LocalSet(
                        sent_id,
                        Box::new(Expr::Await(Box::new(value.clone()))),
                    )));
                    case_body.push(Stmt::Continue);
                }
            }
            StateExit::Goto(next_state) => {
                if body_contains_return(&case_body) {
                    prepend_done_before_returns(&mut case_body, done_id);
                    rewrite_returns_as_done(&mut case_body);
                }
                case_body.push(Stmt::Expr(Expr::LocalSet(
                    state_id,
                    Box::new(Expr::Number(*next_state as f64)),
                )));
                case_body.push(Stmt::Continue);
            }
            StateExit::Done => {
                let has_return = body_contains_return(&case_body);
                if has_return {
                    prepend_done_before_returns(&mut case_body, done_id);
                    rewrite_returns_as_done(&mut case_body);
                    let last_is_return = matches!(case_body.last(), Some(Stmt::Return(_)));
                    if !last_is_return {
                        case_body.push(Stmt::Expr(Expr::LocalSet(
                            done_id,
                            Box::new(Expr::Bool(true)),
                        )));
                        case_body.push(Stmt::Return(Some(make_iter_result(Expr::Undefined, true))));
                    }
                } else {
                    case_body.push(Stmt::Expr(Expr::LocalSet(
                        done_id,
                        Box::new(Expr::Bool(true)),
                    )));
                    case_body.push(Stmt::Return(Some(make_iter_result(Expr::Undefined, true))));
                }
            }
        }

        while_body.push(Stmt::If {
            condition: Expr::Compare {
                op: CompareOp::Eq,
                left: Box::new(Expr::LocalGet(state_id)),
                right: Box::new(Expr::Number(num as f64)),
            },
            then_branch: case_body,
            else_branch: None,
        });
    }

    // Default: done
    while_body.push(Stmt::Expr(Expr::LocalSet(
        done_id,
        Box::new(Expr::Bool(true)),
    )));
    while_body.push(Stmt::Return(Some(make_iter_result(Expr::Undefined, true))));
    while_body
}

/// #6709: Rewrite every `Stmt::Return(Some(<iter-result object>))` in an
/// async-generator step body into `return AsyncStepDone(<iter-result>,
/// __step_self)`, so a consumer `yield` / completion settles the activation's
/// result Promise with `{value, done}` instead of returning the raw object.
/// `AsyncStepChain` (await) returns are NOT iter-result objects, so they are
/// left untouched. Does not descend into nested closures (their returns are
/// their own). Recurses through control flow.
fn wrap_iter_result_returns_in_async_step_done(stmts: &mut Vec<Stmt>) {
    for stmt in stmts.iter_mut() {
        match stmt {
            Stmt::Return(Some(expr)) => {
                if is_iter_result(expr) {
                    let inner = std::mem::replace(expr, Expr::Undefined);
                    *expr = async_step_resolve(inner);
                }
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                wrap_iter_result_returns_in_async_step_done(then_branch);
                if let Some(eb) = else_branch {
                    wrap_iter_result_returns_in_async_step_done(eb);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
                wrap_iter_result_returns_in_async_step_done(body);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                wrap_iter_result_returns_in_async_step_done(body);
                if let Some(c) = catch {
                    wrap_iter_result_returns_in_async_step_done(&mut c.body);
                }
                if let Some(f) = finally {
                    wrap_iter_result_returns_in_async_step_done(f);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases.iter_mut() {
                    wrap_iter_result_returns_in_async_step_done(&mut case.body);
                }
            }
            Stmt::Labeled { body, .. } => {
                let mut v = vec![std::mem::replace(body.as_mut(), Stmt::Break)];
                wrap_iter_result_returns_in_async_step_done(&mut v);
                **body = v.into_iter().next().unwrap();
            }
            _ => {}
        }
    }
}

/// Transform a single generator function into a state machine.
pub fn transform_generator_function(
    func: &mut Function,
    next_local_id: &mut u32,
    next_func_id: &mut u32,
) {
    transform_generator_function_with_extra_captures(
        func,
        next_local_id,
        next_func_id,
        &[],
        &[],
        false,
        false,
        None,
    );
}

/// Issue #1021: variant that augments the internally-generated
/// next/return/throw/step closures with extra captures from an enclosing
/// scope. Used when this transform is applied to a synthetic Function
/// built from an `Expr::Closure` body — the body's `LocalGet`s to
/// outer-scope variables (e.g. `server` in `app.listen(port, async () =>
/// { ... server.close() })`) need those LocalIds in the step closure's
/// captures so Perry's transitive closure-capture mechanism (see
/// `expr.rs:4984-4997`) resolves them via the enclosing closure pointer.
///
/// For top-level fns (`extra_captures` empty) the behavior is identical
/// to the pre-refactor implementation.
pub fn transform_generator_function_with_extra_captures(
    func: &mut Function,
    next_local_id: &mut u32,
    next_func_id: &mut u32,
    extra_captures: &[LocalId],
    extra_mutable_captures: &[LocalId],
    captures_this: bool,
    captures_new_target: bool,
    enclosing_class: Option<String>,
) {
    // Generator bodies run later inside synthesized step closures, so direct
    // `this` reads need the receiver from the original generator call.
    let captures_this = captures_this || generator_body_uses_call_this(&func.body);

    // Remember whether this was an async generator (`async function*`).
    // Async generators are still lowered via the same state-machine
    // transform, but:
    //
    //   (1) The outer wrapper must NOT be marked `is_async` anymore —
    //       otherwise `Stmt::Return` in the LLVM backend wraps the
    //       `{ next, return, throw }` iterator object in
    //       `js_promise_resolved`, so `gen.next()` at the call site
    //       dereferences a Promise pointer as if it were an object
    //       and segfaults.
    //
    //   (2) The `.next()` / `.return()` / `.throw()` closure bodies
    //       wrap their iter-result object in a resolved Promise, so
    //       callers can still write `await gen.next()` and get
    //       `{ value, done }` back (matching async-generator semantics
    //       where `.next()` always returns a Promise).
    //
    // A non-async generator keeps the direct iter-result return path.
    let is_async_generator = func.is_async;
    func.is_async = false;

    // Spec: generator/async-generator parameter binding
    // (FunctionDeclarationInstantiation) runs *synchronously* when the function
    // is called — before the generator object is created — so an
    // iterator/RequireObjectCoercible/TDZ error during destructuring or default
    // evaluation throws at call time, not on the first `.next()`. Lowering
    // prepends the param prologue (default guards + destructuring binding) to
    // the body; here we lift those leading statements out so they run in the
    // outer wrapper (run-at-call) rather than state 0 of the state machine.
    // `gen_prologue_len` returns 0 for generators with no destructuring/default
    // params, leaving this fully inert for the common case.
    let prologue_len = super::gen_prologue_len(func.id);
    let param_prologue: Vec<Stmt> = if prologue_len > 0 && prologue_len <= func.body.len() {
        func.body.drain(..prologue_len).collect()
    } else {
        Vec::new()
    };
    // Locals the prologue binds (destructured targets + scaffolding temps). They
    // are written in the outer wrapper but read by the state machine, so they
    // must be boxed captures like any other cross-state local.
    let prologue_hoist = collect_hoisted_vars(&param_prologue);

    // #321: hoist `yield` / `yield*` that live inside a larger expression
    // (`return (yield 1) + (yield 2)`, call args, array/object literals, etc.)
    // into ordered `let __ygen_N = yield E;` temps so the linearizer below only
    // ever encounters a yield at a position it already splits into states.
    // Without this, a buried yield falls into the linearizer's catch-all and
    // codegen lowers it via the "generators not implemented" arm (returns 0.0)
    // — the resumed value is dropped and the generator never suspends at it.
    // The temps land as `Stmt::Let` in the body, so `collect_hoisted_vars`
    // below picks them up and boxes/preallocates them like any other hoisted
    // local. Allocated before `local_id_before` so they are not double-counted
    // in `extra_local_ids`.
    hoist_yields_in_stmts(&mut func.body, next_local_id);

    // Async generators await each `yield` operand before delivering it (spec
    // `AsyncGeneratorYield(? Await(value))`). Run after `hoist_yields` so every
    // remaining yield is at a statement-level position this pass recognises.
    if is_async_generator {
        await_async_generator_yield_operands(&mut func.body, next_local_id);
        // #6709: hoist every non-top-level `await` (nested in a call arg, a
        // binary operand, an if/while condition, …) into a fresh
        // `let __awaitN = await <expr>;` at statement level — including the
        // yield-operand awaits just inserted above — so `linearize_body`'s
        // `StateExit::Await` arms see each `await` in a position they split
        // into a suspend state. Mirrors the plain-async pre-pass
        // (`transform_async_to_generator`), which does the same before it
        // rewrites `await`→`yield`.
        crate::async_to_generator::hoist_awaits_in_stmts(&mut func.body, next_local_id);
    }

    // #6354: a per-iteration binding a closure WRITES that also outlives a
    // suspend is fixed by neither #6345 path (a value snapshot would drop the
    // write, so it stays in `mutable_captures` and keeps its shared box). Back
    // each such binding with a one-element heap cell BEFORE the #6345 passes
    // run: the cell reference is then a read-only per-iteration capture the
    // snapshot below handles, while writes go to the shared element. See
    // `per_iteration.rs` part 3.
    let cell_ids = collect_written_suspended_loop_captures(&func.body);
    rewrite_written_captures_to_cells(&mut func.body, &cell_ids);

    // #6345: decide which loop bindings must NOT be hoisted into the
    // activation-wide box frame, and snapshot the ones that outlive a suspend
    // into per-state locals. Both run BEFORE `linearize_body` so the inserted
    // `Let`s land in the same state as the closure that reads them, and before
    // `local_id_before` below so the new ids are not swept into
    // `extra_local_ids` (which is force-preallocated).
    let per_iteration_ids = collect_per_iteration_ids(&func.body);
    let mut no_hoist_ids =
        snapshot_suspended_loop_captures(&mut func.body, next_local_id, &per_iteration_ids);
    no_hoist_ids.extend(per_iteration_ids);

    let state_id = alloc_local(next_local_id);
    let done_id = alloc_local(next_local_id);
    let sent_id = alloc_local(next_local_id); // value passed by caller via next(val)
    let executing_id = alloc_local(next_local_id);
    // #4438 B2-finally: pending abrupt-completion record for routing through a
    // YIELDING finally. `pending_type`: 0 = none, 1 = throw, 2 = return.
    // `pending_value`: the thrown error / returned value. Set when abrupt
    // completion routes into a finally; re-raised at the finally's completion
    // check. (Sync generators only — async never sets these, so the appended
    // completion checks are inert on the async path.)
    let pending_type_id = alloc_local(next_local_id);
    let pending_value_id = alloc_local(next_local_id);

    // #6709: the shared async-generator step closure. `.next(v)`/`.throw(e)`
    // are thin outer closures that call `js_async_generator_resume(__agstep,
    // v, is_error)`; `__agstep` drives the state machine and suspends inner
    // `await`s on the microtask queue. It is boxed + captured like the other
    // state-machine internals so `.next`/`.throw` resolve it per-call.
    let agstep_id = if is_async_generator {
        Some(alloc_local(next_local_id))
    } else {
        None
    };

    // Collect all states from the generator body
    let mut states: Vec<State> = Vec::new();
    let mut current: Vec<Stmt> = Vec::new();
    let mut state_num: u32 = 0;

    // Track IDs allocated during linearization (e.g. yield* delegation vars)
    let local_id_before = *next_local_id;
    // Catch routes collected during linearization. Each route records the
    // state interval protected by one `try` plus the state after its `catch`.
    // The .throw() closure uses that interval to route a rejected await to
    // the matching catch handler instead of always using the first catch.
    let mut catches: Vec<CatchRoute> = Vec::new();
    // #4374: finally blocks collected during linearization, so the
    // .return()/.throw() closures can run pending finallys on abrupt
    // completion. Innermost finallys are pushed first (the recursion into a
    // try body collects nested finallys before the enclosing one).
    let mut finallys: Vec<FinallyRoute> = Vec::new();
    // Tell the linearizer whether `yield*` should delegate through the
    // async-iterator protocol (await each delegated `next()`); see the `yield*`
    // arms in `linearize.rs`.
    super::linearize::set_linearize_async_generator(is_async_generator);
    super::linearize::reset_delegation_routes();
    linearize_body(
        &func.body,
        &mut states,
        &mut current,
        &mut state_num,
        state_id,
        next_local_id,
        sent_id,
        &mut catches,
        &mut finallys,
    );
    // `yield *` delegation regions (async generators only), used below so that
    // `gen.return(v)` while suspended inside a `yield *` forwards into the
    // delegated iterator's `return` method (spec `yield *` step 6.c).
    let delegations = super::linearize::take_delegation_routes();
    let extra_local_ids: Vec<LocalId> = (local_id_before..*next_local_id).collect();

    // Push final state (code after last yield / end of function)
    states.push(State {
        num: state_num,
        body: current,
        exit: StateExit::Done,
    });

    // #4438 B2-finally: whether any yielding finally needs the pending-completion
    // record + routing machinery (kept off for generators without one).
    let has_yielding_finally = finallys.iter().any(|f| f.finally_entry_state.is_some());

    // #4438 B2-finally: append the completion-resume check to each yielding
    // finally's completion-check state. After the finally body runs (on either
    // the happy path or an abrupt completion routed into it), re-raise a pending
    // throw/return; on the normal path (pending_type == 0) it's inert and the
    // state falls through to post-finally.
    //
    // Async generators need this for the same reason sync ones do: now that the
    // dispatch loop routes body-internal throws (below), a throw routed into a
    // yielding finally must be re-raised after the finally body — otherwise it is
    // silently swallowed.
    {
        let resume = build_completion_resume_stmts(pending_type_id, pending_value_id, done_id);
        for route in &finallys {
            if let Some(cc) = route.completion_check_state {
                if let Some(state) = states.iter_mut().find(|s| s.num == cc) {
                    state.body.extend(resume.iter().cloned());
                }
            }
        }
    }

    // Collect hoisted var IDs first so we know which Lets to rewrite.
    //
    // #6345: NOT every body `Let` may be hoisted. A `let`/`const` declared in a
    // loop gets a FRESH binding per iteration, and a closure made in iteration
    // k must capture iteration k's binding. Hoisting moves the declaration into
    // the activation-wide `PreallocateBoxes` frame (one box per call), which
    // collapses every iteration onto a single cell — so all closures read the
    // last value (`for (let i…) { const j = i; fns.push(() => j) }` printed the
    // final `j` N times). `no_hoist_ids` (computed above) holds the bindings
    // that keep their in-loop declaration — where codegen re-executes and
    // re-boxes them every iteration, exactly as the non-async path does — plus
    // the per-state snapshot locals. `var` and anything else live across an
    // `await` is excluded there and keeps today's hoisting.
    let hoisted_for_rewrite: Vec<(LocalId, String, Type)> = collect_hoisted_vars(&func.body)
        .into_iter()
        .filter(|(id, _, _)| !no_hoist_ids.contains(id))
        .collect();
    let mut hoisted_ids: std::collections::HashSet<LocalId> =
        hoisted_for_rewrite.iter().map(|(id, _, _)| *id).collect();
    // The lifted param prologue defines locals (destructured targets + temps)
    // that the state machine reads; treat them as hoisted so their `Let`s route
    // through the prealloc box (`js_box_set`) instead of shadowing the capture.
    for (id, _, _) in &prologue_hoist {
        hoisted_ids.insert(*id);
    }

    // Rewrite `Let { id, init: Some(expr) }` → `Expr(LocalSet(id, expr))` for hoisted
    // variables inside state bodies. Without this, the Let creates a fresh local that
    // shadows the captured box, and subsequent mutations in other states don't see the
    // update.
    //
    // Issue #256: must recurse into nested control-flow (For/While/If/Try/Switch
    // bodies). A for-of loop inside a state body desugars to a `for (let i = 0;
    // i < arr.length; ++i) { let v = arr[i]; ... }` shape; without the recursion
    // the inner `let v` and `let i` stay as Lets and create shadow slots that
    // hide the outer captured box. Manifested as `for (const v of arr) sum += v`
    // returning sum=0 inside transformed async functions (test_issue_233).
    for state in &mut states {
        rewrite_hoisted_lets_in_stmts(&mut state.body, &hoisted_ids);
    }

    // Build the `while (true) { <state dispatch> }` body. #6709: for async
    // generators, `.next()`/`.throw()` drive the state machine through an
    // async-step driver, so `await` states must suspend on the microtask queue
    // (`async_step = true`). Sync generators and the `was_plain_async` path use
    // the historical shape (`async_step = false`); they have no `await` states.
    let while_body =
        build_dispatch_while_body(&states, is_async_generator, state_id, done_id, sent_id);

    // The next() closure parameter — receives the value from next(val) calls
    let next_param_id = alloc_local(next_local_id);

    // #4374: clone the state-dispatch loop so the .throw() closure can
    // *continue* the state machine after running a catch handler.
    let while_body_for_throw = while_body.clone();
    // #4438 B2-finally: the `.return()` closure needs the same continuation loop
    // when it routes into a yielding finally (so the finally's `yield`s suspend).
    // #6709: the `.return()` closure is NOT an async-step driver (it cannot
    // chain an inner `await` through `CurrentStepClosure`), so its dispatch
    // keeps the busy-wait `await` shape — matching pre-#6709 `.return()`.
    let while_body_for_return = if is_async_generator {
        build_dispatch_while_body(&states, false, state_id, done_id, sent_id)
    } else {
        while_body.clone()
    };

    // #4438: wrap each state-dispatch loop body in a real try/catch so a `throw`
    // *executing inside a try block during dispatch* is caught and routed to the
    // matching catch/finally (or runs pending finally + completes the generator
    // when unhandled). This applies to the `.next()` loop AND the
    // `.throw()`/`.return()` continuation loops — e.g. a `catch` that rethrows
    // must still run a non-yielding `finally` on the way out.
    //
    // Async generators need this exactly as much as sync ones. When a `try`
    // contains a `yield`, the linearizer destroys the `Stmt::Try` and re-emits the
    // catch body as its own states, reachable only by the dispatch handler setting
    // `__gen_state = catch_entry_state`. Gating the wrapper off for async
    // generators therefore left those states unreachable: every throw inside such a
    // `try` unwound past the state loop to the resume-body handler, which force-
    // completes the generator and rejects — i.e. `try {} catch {}` in an
    // `async function*` never caught anything.
    let has_state_based_catch = catches.iter().any(|r| r.catch_entry_state.is_some());
    let has_inlineable_finally = finallys.iter().any(|r| !r.has_yields);
    let wrap_dispatch = has_state_based_catch || has_inlineable_finally || has_yielding_finally;
    let dispatch_body = if wrap_dispatch {
        let disp_err_id = alloc_local(next_local_id);
        wrap_dispatch_loop(
            while_body,
            &catches,
            &finallys,
            state_id,
            done_id,
            pending_type_id,
            pending_value_id,
            disp_err_id,
            &hoisted_ids,
        )
    } else {
        while_body
    };
    let while_body_for_throw = if wrap_dispatch {
        let disp_err_id = alloc_local(next_local_id);
        wrap_dispatch_loop(
            while_body_for_throw,
            &catches,
            &finallys,
            state_id,
            done_id,
            pending_type_id,
            pending_value_id,
            disp_err_id,
            &hoisted_ids,
        )
    } else {
        while_body_for_throw
    };
    let while_body_for_return = if wrap_dispatch {
        let disp_err_id = alloc_local(next_local_id);
        wrap_dispatch_loop(
            while_body_for_return,
            &catches,
            &finallys,
            state_id,
            done_id,
            pending_type_id,
            pending_value_id,
            disp_err_id,
            &hoisted_ids,
        )
    } else {
        while_body_for_return
    };

    // Build next() method body
    let mut next_resume_body = vec![
        // __sent = <param from next(val)>
        Stmt::Expr(Expr::LocalSet(
            sent_id,
            Box::new(Expr::LocalGet(next_param_id)),
        )),
        // if (__done) return { value: undefined, done: true };
        Stmt::If {
            condition: Expr::LocalGet(done_id),
            then_branch: vec![Stmt::Return(Some(make_iter_result(Expr::Undefined, true)))],
            else_branch: None,
        },
        // while (true) { if-chain }
        Stmt::While {
            condition: Expr::Bool(true),
            body: dispatch_body,
        },
    ];

    // Build the new function body
    let mut new_body: Vec<Stmt> = Vec::new();

    // Hoist variable declarations from the original body — collected
    // here (before the prealloc emit) so the prealloc set is complete.
    let mut hoisted = hoisted_for_rewrite;
    // Box + capture the lifted prologue's locals so the state machine can read
    // the destructured param values it bound in the outer wrapper.
    for v in &prologue_hoist {
        if !hoisted.iter().any(|(id, _, _)| *id == v.0) {
            hoisted.push(v.clone());
        }
    }
    for route in &catches {
        if let (Some(param_id), Some(param_name)) = (route.param_id, route.param_name.as_ref()) {
            if !hoisted.iter().any(|(id, _, _)| *id == param_id) {
                // Lifted catch routes run in the async throw arm, outside
                // codegen's normal Stmt::Try catch binding path, so their
                // params need cross-state boxes.
                hoisted.push((param_id, param_name.clone(), Type::Any));
            }
        }
    }

    // Issue #1029: the state-machine internals (`state`, `done`, `sent`)
    // plus hoisted user-vars and the transform-allocated `extra_local_ids`
    // are all captured-by-reference into the synthesized next/return/throw/
    // step closures (they're in `mutable_captures` of those closures).
    // Without an explicit box, the captures lower to NaN-boxed VALUES
    // (TAG_FALSE / TAG_UNDEFINED / 0), and the closure cache at
    // `js_closure_alloc_with_captures_singleton` (closure.rs:712) keys on
    // capture-bit-equality — every call to f() produces the same bits, so
    // the cache returns the SAME closure, whose slots still hold the
    // terminal-state values (done=true) from call 1. Subsequent calls
    // hit the `if (__gen_done) return iter_result(undefined, true)` short-
    // circuit and never run the body. Symptom: call 1 of any state-
    // machined fn returns the right value; calls 2+ return undefined.
    //
    // Emit a `Stmt::PreallocateBoxes` BEFORE the Lets. This:
    //   1. Marks every listed id in `ctx.boxed_vars` via
    //      `collect_prealloc_box_ids_in_stmts` (boxed_vars.rs:48-99) so
    //      LocalGet/LocalSet inside the step body route through
    //      js_box_get/js_box_set.
    //   2. Allocates a fresh box per call (stmt.rs:1082-1102 emits
    //      js_box_alloc into the entry block — runs every call).
    //   3. Makes the closure cache key the BOX POINTER (distinct address
    //      per call) — cache miss → fresh closure per call → correct
    //      idempotency.
    //
    // The subsequent Stmt::Let { id, init } no longer allocates a new
    // box; it routes through the prealloc_boxes branch in stmt.rs:594-614
    // and just js_box_set's the init value into the existing per-call
    // box. Net effect per call: one js_box_alloc + one js_box_set per id,
    // versus the pre-fix path which did one js_box_alloc inside the Let
    // (same cost, but the cache then hit on stale captures).
    // #4438 B2-finally: only allocate/box the pending-completion record when a
    // yielding finally exists (otherwise it's unused — keep other generators'
    // box set unchanged).
    let mut prealloc_ids: Vec<LocalId> = vec![state_id, done_id, sent_id, executing_id];
    if has_yielding_finally {
        prealloc_ids.push(pending_type_id);
        prealloc_ids.push(pending_value_id);
    }
    // #6709: box the shared async-generator step closure so `.next`/`.throw`
    // read a distinct instance per generator activation (closure-cache key by
    // box pointer, matching the #1029 idempotency fix for the other internals).
    if let Some(id) = agstep_id {
        prealloc_ids.push(id);
    }
    for (var_id, _, _) in &hoisted {
        prealloc_ids.push(*var_id);
    }
    for extra_id in &extra_local_ids {
        prealloc_ids.push(*extra_id);
    }
    prealloc_ids.sort();
    prealloc_ids.dedup();
    new_body.push(Stmt::PreallocateBoxes(prealloc_ids));

    // let __state = 0
    new_body.push(Stmt::Let {
        id: state_id,
        name: "__gen_state".to_string(),
        ty: Type::Number,
        mutable: true,
        init: Some(Expr::Number(0.0)),
    });

    // let __done = false
    new_body.push(Stmt::Let {
        id: done_id,
        name: "__gen_done".to_string(),
        ty: Type::Boolean,
        mutable: true,
        init: Some(Expr::Bool(false)),
    });

    new_body.push(Stmt::Let {
        id: executing_id,
        name: "__gen_executing".to_string(),
        ty: Type::Boolean,
        mutable: true,
        init: Some(Expr::Bool(false)),
    });

    // #4438 B2-finally: let __pending_type = 0; let __pending_value = undefined
    if has_yielding_finally {
        new_body.push(Stmt::Let {
            id: pending_type_id,
            name: "__gen_pending_type".to_string(),
            ty: Type::Number,
            mutable: true,
            init: Some(Expr::Number(0.0)),
        });
        new_body.push(Stmt::Let {
            id: pending_value_id,
            name: "__gen_pending_value".to_string(),
            ty: Type::Any,
            mutable: true,
            init: Some(Expr::Undefined),
        });
    }

    // Re-emit hoisted Let stubs (prealloc already covered the boxes;
    // these Lets now route through the prealloc-boxes path and just
    // set the box value via js_box_set).
    for (var_id, var_name, var_ty) in &hoisted {
        new_body.push(Stmt::Let {
            id: *var_id,
            name: var_name.clone(),
            ty: var_ty.clone(),
            mutable: true,
            init: None,
        });
    }
    // Also hoist any extra locals allocated during linearization (e.g. yield* delegation)
    for extra_id in &extra_local_ids {
        new_body.push(Stmt::Let {
            id: *extra_id,
            name: format!("__gen_tmp_{}", extra_id),
            ty: Type::Any,
            mutable: true,
            init: None,
        });
    }

    // __sent variable for two-way yield: stores value from next(val) calls
    new_body.push(Stmt::Let {
        id: sent_id,
        name: "__gen_sent".to_string(),
        ty: Type::Any,
        mutable: true,
        init: Some(Expr::Undefined),
    });

    // Run the lifted parameter prologue in the outer wrapper, after the box
    // stubs are in place (so its destructured-target `Let`s route to
    // `js_box_set` on the prealloc'd boxes the state machine captures) and
    // before the generator object is built/returned. Any iterator /
    // RequireObjectCoercible / TDZ error here propagates synchronously out of
    // the call, matching spec FunctionDeclarationInstantiation order.
    if !param_prologue.is_empty() {
        let mut prologue = param_prologue;
        rewrite_hoisted_lets_in_stmts(&mut prologue, &hoisted_ids);
        new_body.extend(prologue);
    }

    // Build captures: state, done, sent, params, hoisted vars, extra locals
    let mut captures = vec![state_id, done_id, sent_id, executing_id];
    let mut mutable_captures = vec![state_id, done_id, sent_id, executing_id];
    // #4438 B2-finally: the pending-completion record is read/written across the
    // next/throw/return closures, so capture it by reference like the other
    // state-machine internals (only when a yielding finally uses it).
    if has_yielding_finally {
        captures.push(pending_type_id);
        captures.push(pending_value_id);
        mutable_captures.push(pending_type_id);
        mutable_captures.push(pending_value_id);
    }
    // #6709: the outer `.next`/`.throw` closures reference `__agstep` (and the
    // step captures the same boxes as the resume closures); capture it by
    // reference like the other boxed state-machine internals.
    if let Some(id) = agstep_id {
        captures.push(id);
        mutable_captures.push(id);
    }
    for param in &func.params {
        captures.push(param.id);
    }
    for (var_id, _, _) in &hoisted {
        captures.push(*var_id);
        mutable_captures.push(*var_id);
    }
    for extra_id in &extra_local_ids {
        captures.push(*extra_id);
        mutable_captures.push(*extra_id);
    }
    // Issue #1021: when transforming a closure body, the body may reference
    // LocalIds captured from outer scope. Add them so the internally-built
    // next/return/throw/step closures can resolve them transitively through
    // the enclosing closure pointer.
    for cap_id in extra_captures {
        captures.push(*cap_id);
    }
    for mcap_id in extra_mutable_captures {
        mutable_captures.push(*mcap_id);
    }
    captures.sort();
    captures.dedup();
    mutable_captures.sort();
    mutable_captures.dedup();

    let next_func_id_val = {
        let id = *next_func_id;
        *next_func_id += 1;
        id
    };
    // For the `was_plain_async` path we inline `next_body` directly
    // into the step closure (see below) rather than wrap it in a
    // separate `next_closure`. Defer building `next_closure` so we can
    // hand the raw `next_body` to `build_async_step_driver_direct`.

    let throw_param_id = alloc_local(next_local_id);
    if func.was_plain_async {
        // Issue #256: this function was originally a plain async function;
        // the async_to_generator pre-pass rewrote await→yield. Wrap the
        // iterator in an async-step driver so the function returns a
        // Promise that respects spec microtask ordering. See
        // `build_async_step_driver_direct` for the structure.
        //
        // Perf: for plain-async generators we skip the `__iter` object
        // allocation entirely AND the `return` closure (never invoked
        // for plain-async — the spec `return()` method only runs when
        // user code calls it directly on a generator object, which
        // can't happen here since the function returns a Promise, not
        // an iterator). We further FUSE the `__next` body directly
        // into the step closure body — eliminating the per-call
        // `__next` allocation, the closure dispatch, and the captures-
        // box re-lookup that the separate closure-call path required.
        // Inline the throw path too when user try/catch with awaits was
        // lifted by linearize_body: it must update the same step-local
        // control flow that resumes after a catch route. When no such catch
        // routes exist, the throw path collapses to a pure rethrow.
        // When no user try/catch with awaits was lifted by linearize_body
        // (`catches` empty), the throw closure body collapses to a single
        // `throw __throw_val` — pure rethrow, no captures referenced.
        // Skip the closure construction entirely and let the step driver
        // emit `Stmt::Throw(value)` inline in its is-error arm, saving one
        // closure allocation per async-fn invocation (50k/run on the
        // promise_all_chains kernel).
        let throw_routes_for_step = if catches.is_empty() {
            None
        } else {
            Some((catches, state_id, hoisted_ids.clone()))
        };
        let mut next_body_for_step = next_resume_body;
        rewrite_iter_results_in_stmts(&mut next_body_for_step);
        let wrapper_stmts = build_async_step_driver_direct(
            next_body_for_step,
            next_param_id,
            captures.clone(),
            mutable_captures.clone(),
            None,
            throw_routes_for_step,
            throw_param_id,
            next_local_id,
            next_func_id,
            captures_this,
            captures_new_target,
            enclosing_class.clone(),
            func.is_strict,
        );
        for s in wrapper_stmts {
            new_body.push(s);
        }
        // Keep was_plain_async = true so codegen can populate
        // local_async_funcs and is_promise_expr() correctly recognises
        // calls to this function as Promise-returning (issue #269 fix).
        // The flag is safe to keep set — the generator transform only
        // checks it here, and codegen only reads it.
    } else {
        // #6709: for async generators, the shared `__agstep` closure to bind
        // into a boxed local (emitted into `new_body` just before the iterator
        // object is returned). `None` for sync generators.
        let mut agstep_init: Option<(LocalId, Expr)> = None;
        // Build .return(value) closure — immediately marks done and returns {value, done: true}
        let return_param_id = alloc_local(next_local_id);
        let return_func_id_val = {
            let id = *next_func_id;
            *next_func_id += 1;
            id
        };
        // #4374: `.return(v)` on a generator suspended inside a `try` must run
        // the pending `finally` blocks (innermost first) before completing.
        let mut return_resume_body: Vec<Stmt> = Vec::new();
        // Already-done generators just complete with {value: v, done: true} —
        // no finally re-run (the finally already ran on normal completion).
        return_resume_body.push(Stmt::If {
            condition: Expr::LocalGet(done_id),
            then_branch: vec![Stmt::Return(Some(make_iter_result(
                Expr::LocalGet(return_param_id),
                true,
            )))],
            else_branch: None,
        });
        // #4445: mark the generator "executing" while the resume runs (the
        // executing guard rejects a re-entrant resume).
        return_resume_body.push(Stmt::Expr(Expr::LocalSet(
            executing_id,
            Box::new(Expr::Bool(true)),
        )));
        // Spec `yield *` step 6.c: when suspended inside a `yield *`, `return(v)`
        // forwards to the delegated iterator's `return` method (async generators
        // only; `delegations` is empty otherwise). Each route returns on a match,
        // so control falls through to the generic completion below only when not
        // suspended in a delegation.
        return_resume_body.extend(build_yield_star_return_routes(
            &delegations,
            state_id,
            return_param_id,
            done_id,
            next_local_id,
        ));
        // Unhandled path: mark done, run pending non-yielding finallys, return
        // {v, true}. A finally that itself `return`s supersedes `v` (rewritten to
        // an iter-result return inside build_finally_run_stmts); a finally that
        // throws propagates out of this closure.
        let mut return_fallback = vec![Stmt::Expr(Expr::LocalSet(
            done_id,
            Box::new(Expr::Bool(true)),
        ))];
        return_fallback.extend(build_finally_run_stmts(&finallys, state_id, &hoisted_ids));
        return_fallback.push(Stmt::Return(Some(make_iter_result(
            Expr::LocalGet(return_param_id),
            true,
        ))));
        if !is_async_generator && has_yielding_finally {
            // #4438 B2-finally: route `.return(v)` into the innermost enclosing
            // yielding finally (record the pending return + jump in), then fall
            // through to the continuation loop so the finally's `yield`s suspend;
            // its completion check re-raises the return. Catches don't catch a
            // return completion, so only finally routes apply.
            return_resume_body.extend(build_abrupt_routing(
                &catches,
                &finallys,
                state_id,
                pending_type_id,
                pending_value_id,
                &Expr::LocalGet(return_param_id),
                false,
                2.0,
                false,
                false,
                return_fallback,
            ));
            return_resume_body.push(Stmt::While {
                condition: Expr::Bool(true),
                body: while_body_for_return,
            });
        } else {
            return_resume_body.extend(return_fallback);
        }
        // #4445: wrap with the executing guard + a catch that clears `executing`
        // and marks `done` on any escaping throw (also wraps returns in a Promise
        // for async generators).
        let return_catch_id = alloc_local(next_local_id);
        let return_body = wrap_generator_resume_body(
            return_resume_body,
            executing_id,
            done_id,
            return_catch_id,
            is_async_generator,
        );
        let return_closure = Expr::Closure {
            func_id: return_func_id_val,
            params: vec![perry_hir::Param {
                id: return_param_id,
                name: "__ret_val".to_string(),
                ty: Type::Any,
                is_rest: false,
                default: None,
                decorators: Vec::new(),
                arguments_object: None,
            }],
            return_type: Type::Any,
            body: return_body,
            captures: captures.clone(),
            mutable_captures: mutable_captures.clone(),
            captures_this,
            captures_new_target: false,
            enclosing_class: enclosing_class.clone(),
            is_arrow: false,
            is_strict: func.is_strict,
            is_async: false,
            is_generator: false,
        };

        // Build .throw(error) closure. Each catch route owns the state interval
        // for the try body it protects, so multiple independent try/catch regions
        // in the same async function resume at the correct post-catch state.
        let throw_func_id_val = {
            let id = *next_func_id;
            *next_func_id += 1;
            id
        };
        // #5745: spec `yield *` step 6.b — when an async generator is suspended
        // inside a `yield *` and `gen.throw(e)` is called, forward the error into
        // the delegated iterator's `throw` (re-yielding or, on a `done` result,
        // resuming the outer body past the `yield *`) rather than routing it into
        // the outer generator's own catch handlers. Each route re-drives the
        // state machine via the `while_body_for_throw` continuation loop, so it
        // is built BEFORE the loop is moved into `throw_continuation` below.
        // Empty for sync generators (`delegations` is only recorded for async).
        // #6709: for async generators `.throw(e)` is the error arm of the
        // shared step closure, so the thrown value arrives through the step's
        // value param (`next_param_id`) rather than a dedicated `.throw`
        // closure param. Sync generators keep the dedicated `throw_param_id`.
        let throw_val_id = if is_async_generator {
            next_param_id
        } else {
            throw_param_id
        };
        let yield_star_throw_routes = build_yield_star_throw_routes(
            &delegations,
            &catches,
            &finallys,
            state_id,
            throw_val_id,
            pending_type_id,
            pending_value_id,
            &while_body_for_throw,
            &hoisted_ids,
            next_local_id,
        );
        // #4374: continue the state machine after a catch — run the inlined
        // finally and reach the next yield/completion within the `.throw()` call.
        //
        // Async generators took the legacy deferred-resume path here, which inlines
        // the catch body into the `.throw()` closure. That only works when the catch
        // body is still inline; once the `try` contains a `yield` the linearizer has
        // moved the catch into its own states, so the inlined copy had nothing to run
        // and `gen.throw(e)` resolved to `{value: undefined, done: false}` instead of
        // the value the catch yields. Routing to the catch's states (as sync
        // generators do) is what Node's semantics require.
        let throw_continuation = Some(while_body_for_throw);
        // #4374: fresh binding for the inner catch that re-runs a try's finally
        // when its catch handler itself throws (catch-rethrow-with-finally).
        let inner_catch_id = alloc_local(next_local_id);
        let mut throw_resume_body = vec![Stmt::Expr(Expr::LocalSet(
            executing_id,
            Box::new(Expr::Bool(true)),
        ))];
        // The `yield *` throw routes return/throw from inside their own
        // continuation loop on a match, so they precede the catch-routing body
        // and only fall through to it when not suspended in a delegation.
        throw_resume_body.extend(yield_star_throw_routes);
        throw_resume_body.extend(build_async_throw_body(
            &catches,
            &finallys,
            state_id,
            done_id,
            throw_val_id,
            inner_catch_id,
            pending_type_id,
            pending_value_id,
            &hoisted_ids,
            throw_continuation,
        ));
        // #6709: for async generators, `.next`/`.throw` are thin outer closures
        // driving a shared async-step `__agstep` closure so inner `await`s
        // suspend on the microtask queue; sync generators keep direct closures.
        let (next_closure, throw_closure) = if is_async_generator {
            // Non-error arm = the next dispatch (deliver `value` to `__sent`);
            // insert the executing flag before the dispatch loop.
            next_resume_body.insert(
                2,
                Stmt::Expr(Expr::LocalSet(executing_id, Box::new(Expr::Bool(true)))),
            );
            let is_error_param_id = alloc_local(next_local_id);
            let agstep_func_id = {
                let id = *next_func_id;
                *next_func_id += 1;
                id
            };
            let agstep_catch_id = alloc_local(next_local_id);

            // `if (__is_error) { <throw routing> } else { <next dispatch> }`.
            // An `await` that REJECTS re-enters the step with __is_error = true,
            // routing the rejection to the enclosing catch exactly like an
            // explicit `.throw()`; a settled `await` re-enters with
            // __is_error = false, delivering the value through `__sent`.
            let step_inner = vec![Stmt::If {
                condition: Expr::LocalGet(is_error_param_id),
                then_branch: throw_resume_body,
                else_branch: Some(next_resume_body),
            }];
            let mut step_body =
                wrap_async_gen_step_body(step_inner, executing_id, done_id, agstep_catch_id);
            // Turn every `{value, done}` return into an `AsyncStepDone` that
            // settles the activation's result Promise; `AsyncStepChain` (await)
            // returns are left as-is.
            wrap_iter_result_returns_in_async_step_done(&mut step_body);

            let agstep_local_id = agstep_id.expect("agstep_id set for async generators");
            let agstep_closure = Expr::Closure {
                func_id: agstep_func_id,
                params: vec![
                    perry_hir::Param {
                        id: next_param_id,
                        name: "__val".to_string(),
                        ty: Type::Any,
                        is_rest: false,
                        default: None,
                        decorators: Vec::new(),
                        arguments_object: None,
                    },
                    perry_hir::Param {
                        id: is_error_param_id,
                        name: "__is_error".to_string(),
                        ty: Type::Boolean,
                        is_rest: false,
                        default: None,
                        decorators: Vec::new(),
                        arguments_object: None,
                    },
                ],
                return_type: Type::Any,
                body: step_body,
                captures: captures.clone(),
                mutable_captures: mutable_captures.clone(),
                captures_this,
                captures_new_target: false,
                enclosing_class: enclosing_class.clone(),
                is_arrow: false,
                is_strict: func.is_strict,
                is_async: false,
                is_generator: false,
            };
            // `let __agstep = <agstep_closure>` is emitted into the outer body
            // just before the iterator object is returned (see `agstep_init`).
            agstep_init = Some((agstep_local_id, agstep_closure));

            // Outer `.next(v)` closure: `return js_async_generator_resume(
            //   __agstep, v, false)`.
            let outer_next_param_id = alloc_local(next_local_id);
            let outer_next_closure = Expr::Closure {
                func_id: next_func_id_val,
                params: vec![perry_hir::Param {
                    id: outer_next_param_id,
                    name: "__val".to_string(),
                    ty: Type::Any,
                    is_rest: false,
                    default: None,
                    decorators: Vec::new(),
                    arguments_object: None,
                }],
                return_type: Type::Any,
                body: vec![Stmt::Return(Some(Expr::AsyncGenResume {
                    step_closure: Box::new(Expr::LocalGet(agstep_local_id)),
                    value: Box::new(Expr::LocalGet(outer_next_param_id)),
                    is_error: false,
                }))],
                captures: captures.clone(),
                mutable_captures: mutable_captures.clone(),
                captures_this,
                captures_new_target: false,
                enclosing_class: enclosing_class.clone(),
                is_arrow: false,
                is_strict: func.is_strict,
                is_async: false,
                is_generator: false,
            };
            // Outer `.throw(e)` closure: `return js_async_generator_resume(
            //   __agstep, e, true)`.
            let outer_throw_param_id = alloc_local(next_local_id);
            let outer_throw_closure = Expr::Closure {
                func_id: throw_func_id_val,
                params: vec![perry_hir::Param {
                    id: outer_throw_param_id,
                    name: "__throw_val".to_string(),
                    ty: Type::Any,
                    is_rest: false,
                    default: None,
                    decorators: Vec::new(),
                    arguments_object: None,
                }],
                return_type: Type::Any,
                body: vec![Stmt::Return(Some(Expr::AsyncGenResume {
                    step_closure: Box::new(Expr::LocalGet(agstep_local_id)),
                    value: Box::new(Expr::LocalGet(outer_throw_param_id)),
                    is_error: true,
                }))],
                captures: captures.clone(),
                mutable_captures: mutable_captures.clone(),
                captures_this,
                captures_new_target: false,
                enclosing_class: enclosing_class.clone(),
                is_arrow: false,
                is_strict: func.is_strict,
                is_async: false,
                is_generator: false,
            };
            (outer_next_closure, outer_throw_closure)
        } else {
            let throw_catch_id = alloc_local(next_local_id);
            let throw_body = wrap_generator_resume_body(
                throw_resume_body,
                executing_id,
                done_id,
                throw_catch_id,
                is_async_generator,
            );
            let throw_closure = Expr::Closure {
                func_id: throw_func_id_val,
                params: vec![perry_hir::Param {
                    id: throw_param_id,
                    name: "__throw_val".to_string(),
                    ty: Type::Any,
                    is_rest: false,
                    default: None,
                    decorators: Vec::new(),
                    arguments_object: None,
                }],
                return_type: Type::Any,
                body: throw_body,
                captures: captures.clone(),
                mutable_captures: mutable_captures.clone(),
                captures_this,
                captures_new_target: false,
                enclosing_class: enclosing_class.clone(),
                is_arrow: false,
                is_strict: func.is_strict,
                is_async: false,
                is_generator: false,
            };

            // Plain generator: build the iterator object and return it directly.
            let next_catch_id = alloc_local(next_local_id);
            next_resume_body.insert(
                2,
                Stmt::Expr(Expr::LocalSet(executing_id, Box::new(Expr::Bool(true)))),
            );
            let next_body = wrap_generator_resume_body(
                next_resume_body,
                executing_id,
                done_id,
                next_catch_id,
                is_async_generator,
            );
            let next_closure = Expr::Closure {
                func_id: next_func_id_val,
                params: vec![perry_hir::Param {
                    id: next_param_id,
                    name: "__val".to_string(),
                    ty: Type::Any,
                    is_rest: false,
                    default: None,
                    decorators: Vec::new(),
                    arguments_object: None,
                }],
                return_type: Type::Any,
                body: next_body,
                captures: captures.clone(),
                mutable_captures: mutable_captures.clone(),
                captures_this,
                captures_new_target: false,
                enclosing_class: enclosing_class.clone(),
                is_arrow: false,
                is_strict: func.is_strict,
                is_async: false,
                is_generator: false,
            };
            (next_closure, throw_closure)
        };
        let iter_obj = Expr::Object(vec![
            ("next".to_string(), next_closure),
            ("return".to_string(), return_closure),
            ("throw".to_string(), throw_closure),
        ]);
        // #4141: wire the instance's `[[Prototype]]` chain
        // (`gen() → g.prototype → %Generator.prototype%`) so reflective
        // access via the instance (`Object.getPrototypeOf(Object.getPrototypeOf(
        // gen()))`) reaches the brand-checked prototype methods. The object
        // literal is hidden inside the wrapper in return position; escape
        // analysis leaves the unanalyzed allocation on the heap (correct — a
        // generator object always escapes via the return).
        let linked = Expr::LinkGeneratorPrototype {
            obj: Box::new(iter_obj),
            is_async: is_async_generator,
        };
        // #6709: bind the shared async-generator step closure into its boxed
        // local before returning the iterator object, so the `.next`/`.throw`
        // closures (which captured the box) read a fresh `__agstep` per call.
        if let Some((agstep_local_id, agstep_closure)) = agstep_init {
            new_body.push(Stmt::Let {
                id: agstep_local_id,
                name: "__agstep".to_string(),
                ty: Type::Any,
                mutable: true,
                init: Some(agstep_closure),
            });
        }
        new_body.push(Stmt::Return(Some(linked)));
    }

    func.body = new_body;
    func.is_generator = false;
}
