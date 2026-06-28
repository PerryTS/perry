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
    build_yield_star_return_routes, catch_route_condition, finally_abrupt_condition,
    finally_route_condition, rewrite_dispatch_continue_to_suspend, wrap_dispatch_loop,
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
    prepend_executing_clear_before_returns, promise_reject, wrap_generator_resume_body,
};
pub(crate) use yield_await::await_async_generator_yield_operands;

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
    }

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
    // state falls through to post-finally. Sync only in practice — async never
    // sets `pending_type`, so the checks are dead on the async path.
    if !is_async_generator {
        let resume = build_completion_resume_stmts(pending_type_id, pending_value_id, done_id);
        for route in &finallys {
            if let Some(cc) = route.completion_check_state {
                if let Some(state) = states.iter_mut().find(|s| s.num == cc) {
                    state.body.extend(resume.iter().cloned());
                }
            }
        }
    }

    // Collect hoisted var IDs first so we know which Lets to rewrite
    let hoisted_for_rewrite = collect_hoisted_vars(&func.body);
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

    // Build the if-chain inside while(true)
    let mut while_body: Vec<Stmt> = Vec::new();
    for state in states {
        let State { num, body, exit } = state;
        let mut case_body = body;
        match exit {
            StateExit::Yield { value, next_state } => {
                // #1047: a user `return X` inside this state body — at
                // any depth — must terminate the whole async function,
                // not just exit the state. Without rewriting, the bare
                // `return existing.kid` returns a non-iter-result from
                // next(), the AsyncStepChain caller treats the missing
                // `.done` as `false`, and re-enters the same state with
                // the SAME state_id (the synthesized `state_id = N + 1`
                // append below is unreachable when the user's return
                // fires first). Result: infinite loop. Same fix as the
                // `StateExit::Done` arm — set `__gen_done = true` and
                // wrap the returned value in an iter-result with
                // `done = true` so the async-step driver short-circuits.
                if body_contains_return(&case_body) {
                    prepend_done_before_returns(&mut case_body, done_id);
                    rewrite_returns_as_done(&mut case_body);
                }
                case_body.push(Stmt::Expr(Expr::LocalSet(
                    state_id,
                    Box::new(Expr::Number(next_state as f64)),
                )));
                case_body.push(Stmt::Return(Some(make_iter_result(value, false))));
            }
            StateExit::Goto(next_state) => {
                // #1196: a user `return X` inside this state body — at any
                // depth — must terminate the whole async function, not just
                // fall through to `next_state`. Mirrors the Yield/Done arms
                // above. Without the rewrite, `rewrite_returns_to_labeled_break`
                // later strips the return to `[Expr(X), LabeledBreak]`
                // (value discarded, IterResult never set). The post-step
                // code then sees the IterResult left over from the previous
                // yield (done=false) and re-chains the step closure onto
                // it via AsyncStepChain — re-entering this same state,
                // taking the same early-return, and looping forever.
                // Symptom: ~123 MB arena growth per outer call, GC every
                // ~250 ms, 90%+ CPU. Triggered when the state body fans
                // into a Goto (e.g. an `if (...) return X;` immediately
                // before a `for` loop with `await` inside).
                if body_contains_return(&case_body) {
                    prepend_done_before_returns(&mut case_body, done_id);
                    rewrite_returns_as_done(&mut case_body);
                }
                case_body.push(Stmt::Expr(Expr::LocalSet(
                    state_id,
                    Box::new(Expr::Number(next_state as f64)),
                )));
                case_body.push(Stmt::Continue);
            }
            StateExit::Done => {
                // Check if the body already has a return (from the user's `return expr`)
                // — at ANY depth, since user code can `return` inside `if` /
                // `try` / `switch` etc. inside a state body. Without the
                // recursion (#594), a user `return X` inside an
                // `if (cond) { return X }` block fell through both rewrites
                // — the bare `Return(X)` reached the iterator caller and
                // `__step_r.done` access threw "Cannot read properties of
                // undefined".
                let has_return = body_contains_return(&case_body);
                if has_return {
                    // Rewrite existing returns to iter results, and prepend done=true
                    // Insert done=true BEFORE the return so it's reachable.
                    // Both passes recurse through nested control flow so a
                    // `return X` at any depth inside this state body is
                    // covered.
                    prepend_done_before_returns(&mut case_body, done_id);
                    rewrite_returns_as_done(&mut case_body);
                    // The body still needs a trailing iter-result if NOT every
                    // path returns (e.g. `if (cond) return X` falls through
                    // when `cond` is false). Append a default
                    // `__gen_done = true; return { value: undefined, done: true }`
                    // unless the LAST stmt is unconditionally a Return.
                    let last_is_return = matches!(case_body.last(), Some(Stmt::Return(_)));
                    if !last_is_return {
                        case_body.push(Stmt::Expr(Expr::LocalSet(
                            done_id,
                            Box::new(Expr::Bool(true)),
                        )));
                        case_body.push(Stmt::Return(Some(make_iter_result(Expr::Undefined, true))));
                    }
                } else {
                    // No explicit return: add done + default return
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

    // The next() closure parameter — receives the value from next(val) calls
    let next_param_id = alloc_local(next_local_id);

    // #4374: clone the state-dispatch loop so the .throw() closure can
    // *continue* the state machine after running a catch handler — running
    // the inlined finally and proceeding to the next yield / completion,
    // instead of returning {value: undefined, done: false} and deferring to
    // the next .next(). Only the sync-generator .throw() path uses this.
    let while_body_for_throw = while_body.clone();
    // #4438 B2-finally: the `.return()` closure needs the same continuation loop
    // when it routes into a yielding finally (so the finally's `yield`s suspend).
    let while_body_for_return = while_body.clone();

    // #4438: for sync generators, wrap each state-dispatch loop body in a real
    // try/catch so a `throw` *executing inside a try block during dispatch* is
    // caught and routed to the matching catch/finally (or runs pending finally +
    // completes the generator when unhandled). This applies to the `.next()`
    // loop AND the `.throw()`/`.return()` continuation loops — e.g. a `catch`
    // that rethrows must still run a non-yielding `finally` on the way out.
    let has_state_based_catch = catches.iter().any(|r| r.catch_entry_state.is_some());
    let has_inlineable_finally = finallys.iter().any(|r| !r.has_yields);
    let wrap_dispatch = !is_async_generator
        && (has_state_based_catch || has_inlineable_finally || has_yielding_finally);
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
        // #4374: sync generators continue the state machine after a catch
        // (running the inlined finally + reaching the next yield/completion);
        // async generators keep the existing deferred-resume behavior to stay
        // byte-identical on the async path.
        let throw_continuation = if is_async_generator {
            None
        } else {
            Some(while_body_for_throw)
        };
        // #4374: fresh binding for the inner catch that re-runs a try's finally
        // when its catch handler itself throws (catch-rethrow-with-finally).
        let inner_catch_id = alloc_local(next_local_id);
        let mut throw_resume_body = vec![Stmt::Expr(Expr::LocalSet(
            executing_id,
            Box::new(Expr::Bool(true)),
        ))];
        throw_resume_body.extend(build_async_throw_body(
            &catches,
            &finallys,
            state_id,
            done_id,
            throw_param_id,
            inner_catch_id,
            pending_type_id,
            pending_value_id,
            &hoisted_ids,
            throw_continuation,
        ));
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
        new_body.push(Stmt::Return(Some(linked)));
    }

    func.body = new_body;
    func.is_generator = false;
}
