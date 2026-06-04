//! Generator-function state-machine lowering and async step-driver construction.

use super::*;
use perry_hir::walker::walk_expr_children;

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

    let state_id = alloc_local(next_local_id);
    let done_id = alloc_local(next_local_id);
    let sent_id = alloc_local(next_local_id); // value passed by caller via next(val)
    let executing_id = alloc_local(next_local_id);

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
    let extra_local_ids: Vec<LocalId> = (local_id_before..*next_local_id).collect();

    // Push final state (code after last yield / end of function)
    states.push(State {
        num: state_num,
        body: current,
        exit: StateExit::Done,
    });

    // Collect hoisted var IDs first so we know which Lets to rewrite
    let hoisted_for_rewrite = collect_hoisted_vars(&func.body);
    let hoisted_ids: std::collections::HashSet<LocalId> =
        hoisted_for_rewrite.iter().map(|(id, _, _)| *id).collect();

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

    // #4438: for sync generators, wrap the dispatch loop body in a real
    // try/catch so a `throw` *executing inside a try block during a normal
    // .next()* is caught and routed to the matching catch's linearized states
    // (or, when no catch matches, runs any pending `finally` and completes the
    // generator before propagating). Pre-#4438 the catch handler existed only
    // in the .throw() closure, so such a throw escaped uncaught.
    let has_state_based_catch = catches.iter().any(|r| r.catch_entry_state.is_some());
    let has_inlineable_finally = finallys.iter().any(|r| !r.has_yields);
    let wrap_dispatch = !is_async_generator && (has_state_based_catch || has_inlineable_finally);
    let dispatch_body = if wrap_dispatch {
        let disp_err_id = alloc_local(next_local_id);
        let handler = build_dispatch_catch_handler(
            &catches,
            &finallys,
            state_id,
            done_id,
            disp_err_id,
            &hoisted_ids,
        );
        vec![Stmt::Try {
            body: while_body,
            catch: Some(CatchClause {
                param: Some((disp_err_id, "__gen_disp_err".to_string())),
                body: handler,
            }),
            finally: None,
        }]
    } else {
        while_body
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
    let mut prealloc_ids: Vec<LocalId> = vec![state_id, done_id, sent_id, executing_id];
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

    // Build captures: state, done, sent, params, hoisted vars, extra locals
    let mut captures = vec![state_id, done_id, sent_id, executing_id];
    let mut mutable_captures = vec![state_id, done_id, sent_id, executing_id];
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
        // Mark done (abrupt completion), then run pending finallys. A finally
        // that itself `return`s supersedes `v` (rewritten to an iter-result
        // return inside build_finally_run_stmts); a finally that throws
        // propagates out of this closure.
        return_resume_body.push(Stmt::Expr(Expr::LocalSet(
            executing_id,
            Box::new(Expr::Bool(true)),
        )));
        return_resume_body.push(Stmt::Expr(Expr::LocalSet(
            done_id,
            Box::new(Expr::Bool(true)),
        )));
        return_resume_body.extend(build_finally_run_stmts(&finallys, state_id, &hoisted_ids));
        return_resume_body.push(Stmt::Return(Some(make_iter_result(
            Expr::LocalGet(return_param_id),
            true,
        ))));
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

fn generator_body_uses_call_this(body: &[Stmt]) -> bool {
    body.iter().any(generator_stmt_uses_call_this)
}

fn wrap_generator_resume_body(
    mut body: Vec<Stmt>,
    executing_id: LocalId,
    done_id: LocalId,
    catch_id: LocalId,
    is_async_generator: bool,
) -> Vec<Stmt> {
    prepend_executing_clear_before_returns(&mut body, executing_id);
    if is_async_generator {
        wrap_returns_in_promise(&mut body);
    }

    vec![
        generator_executing_guard(executing_id, is_async_generator),
        Stmt::Try {
            body,
            catch: Some(CatchClause {
                param: Some((catch_id, "__gen_exec_e".to_string())),
                body: vec![
                    Stmt::Expr(Expr::LocalSet(done_id, Box::new(Expr::Bool(true)))),
                    Stmt::Expr(Expr::LocalSet(executing_id, Box::new(Expr::Bool(false)))),
                    generator_resume_rethrow(Expr::LocalGet(catch_id), is_async_generator),
                ],
            }),
            finally: None,
        },
    ]
}

fn generator_executing_guard(executing_id: LocalId, is_async_generator: bool) -> Stmt {
    Stmt::If {
        condition: Expr::LocalGet(executing_id),
        then_branch: vec![generator_resume_rethrow(
            generator_executing_type_error(),
            is_async_generator,
        )],
        else_branch: None,
    }
}

fn generator_resume_rethrow(value: Expr, is_async_generator: bool) -> Stmt {
    if is_async_generator {
        Stmt::Return(Some(promise_reject(value)))
    } else {
        Stmt::Throw(value)
    }
}

fn generator_executing_type_error() -> Expr {
    Expr::TypeErrorNew(Box::new(Expr::String(
        "Generator is already executing".to_string(),
    )))
}

fn promise_reject(value: Expr) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(Expr::GlobalGet(0)),
            property: "reject".to_string(),
        }),
        args: vec![value],
        type_args: vec![],
    }
}

fn prepend_executing_clear_before_returns(stmts: &mut Vec<Stmt>, executing_id: LocalId) {
    let mut new_body: Vec<Stmt> = Vec::with_capacity(stmts.len());
    for mut stmt in stmts.drain(..) {
        match &mut stmt {
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                prepend_executing_clear_before_returns(then_branch, executing_id);
                if let Some(else_branch) = else_branch {
                    prepend_executing_clear_before_returns(else_branch, executing_id);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
                prepend_executing_clear_before_returns(body, executing_id);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                prepend_executing_clear_before_returns(body, executing_id);
                if let Some(catch) = catch {
                    prepend_executing_clear_before_returns(&mut catch.body, executing_id);
                }
                if let Some(finally) = finally {
                    prepend_executing_clear_before_returns(finally, executing_id);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases.iter_mut() {
                    prepend_executing_clear_before_returns(&mut case.body, executing_id);
                }
            }
            Stmt::Labeled { body, .. } => {
                let mut wrapped = vec![std::mem::replace(body.as_mut(), Stmt::Break)];
                prepend_executing_clear_before_returns(&mut wrapped, executing_id);
                **body = wrapped.into_iter().next().unwrap();
            }
            _ => {}
        }
        if matches!(stmt, Stmt::Return(_)) {
            new_body.push(Stmt::Expr(Expr::LocalSet(
                executing_id,
                Box::new(Expr::Bool(false)),
            )));
        }
        new_body.push(stmt);
    }
    *stmts = new_body;
}

fn generator_stmt_uses_call_this(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Let {
            init: Some(expr), ..
        } => generator_expr_uses_call_this(expr),
        Stmt::Let { init: None, .. } => false,
        Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
            generator_expr_uses_call_this(expr)
        }
        Stmt::Return(None)
        | Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_) => false,
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            generator_expr_uses_call_this(condition)
                || then_branch.iter().any(generator_stmt_uses_call_this)
                || else_branch
                    .as_ref()
                    .is_some_and(|body| body.iter().any(generator_stmt_uses_call_this))
        }
        Stmt::While { condition, body } => {
            generator_expr_uses_call_this(condition)
                || body.iter().any(generator_stmt_uses_call_this)
        }
        Stmt::DoWhile { body, condition } => {
            body.iter().any(generator_stmt_uses_call_this)
                || generator_expr_uses_call_this(condition)
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_ref()
                .is_some_and(|stmt| generator_stmt_uses_call_this(stmt))
                || condition
                    .as_ref()
                    .is_some_and(generator_expr_uses_call_this)
                || update.as_ref().is_some_and(generator_expr_uses_call_this)
                || body.iter().any(generator_stmt_uses_call_this)
        }
        Stmt::Labeled { body, .. } => generator_stmt_uses_call_this(body),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            body.iter().any(generator_stmt_uses_call_this)
                || catch
                    .as_ref()
                    .is_some_and(|catch| catch.body.iter().any(generator_stmt_uses_call_this))
                || finally
                    .as_ref()
                    .is_some_and(|body| body.iter().any(generator_stmt_uses_call_this))
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            generator_expr_uses_call_this(discriminant)
                || cases.iter().any(|case| {
                    case.test
                        .as_ref()
                        .is_some_and(generator_expr_uses_call_this)
                        || case.body.iter().any(generator_stmt_uses_call_this)
                })
        }
    }
}

fn generator_expr_uses_call_this(expr: &Expr) -> bool {
    match expr {
        Expr::This
        | Expr::SuperCall(_)
        | Expr::SuperMethodCall { .. }
        | Expr::SuperPropertyGet { .. } => true,
        Expr::Closure { captures_this, .. } => *captures_this,
        _ => {
            let mut found = false;
            walk_expr_children(expr, &mut |child| {
                if !found && generator_expr_uses_call_this(child) {
                    found = true;
                }
            });
            found
        }
    }
}

/// Build the async-step driver (issue #256). Returns the statements that
/// take the place of the plain `return iter_obj` that a normal generator
/// would emit. Equivalent TypeScript:
///
/// ```ts
/// const __iter = <iter_obj>;
/// let __step;
/// __step = (value, isError) => {
///     let r;
///     try {
///         r = isError ? __iter.throw(value) : __iter.next(value);
///     } catch (e) {
///         return Promise.reject(e);
///     }
///     if (r.done) return Promise.resolve(r.value);
///     return Promise.resolve(r.value).then(
///         v => __step(v, false),
///         e => __step(e, true),
///     );
/// };
/// return __step(undefined, false);
/// ```
///
/// The two-step `let __step; __step = ...;` pattern is required because
fn build_async_throw_body(
    catches: &[CatchRoute],
    finallys: &[FinallyRoute],
    state_id: LocalId,
    done_id: LocalId,
    throw_param_id: LocalId,
    inner_catch_id: LocalId,
    hoisted_ids: &std::collections::HashSet<LocalId>,
    // #4374: for sync generators, the cloned state-dispatch loop. When present,
    // a matched catch route sets the resume state and *falls through* to this
    // loop, so the inlined finally runs and the generator continues to the next
    // yield / completion within the `.throw()` call. When `None` (async
    // generators) the catch route returns {undefined, false} as before.
    continuation: Option<Vec<Stmt>>,
) -> Vec<Stmt> {
    let fall_through = continuation.is_some();
    // #4374: when no catch handles the throw, run any pending `finally`
    // (innermost first) before propagating the error — `try { yield }
    // finally { ... }` must run its finally on `.throw(e)`. A `finally` that
    // `return`s supersedes the thrown value (rewritten to an iter-result
    // return inside build_finally_run_stmts). For a try WITH a catch, the
    // catch route below returns first, so this only fires for
    // try/finally-without-catch and for truly unhandled throws.
    let mut fallback = Vec::new();
    // An unhandled throw completes the generator (subsequent .next() must
    // return {done: true}). Sync generators only — async generators keep the
    // existing deferred behavior to stay byte-identical.
    if fall_through {
        fallback.push(Stmt::Expr(Expr::LocalSet(
            done_id,
            Box::new(Expr::Bool(true)),
        )));
    }
    fallback.extend(build_finally_run_stmts(finallys, state_id, hoisted_ids));
    fallback.push(Stmt::Throw(Expr::LocalGet(throw_param_id)));

    // Build nested `if` dispatch in source order. We iterate from the back so
    // the first collected route is tested first; nested try/catch routes are
    // collected before their containing route and should win on overlap.
    for route in catches.iter().rev() {
        // #4438: sync generators route the thrown error into the catch's
        // linearized states (set the catch param + jump to `catch_entry_state`,
        // then fall through to the appended continuation loop, which dispatches
        // the catch body — so a `yield` inside the catch actually suspends).
        // Async generators (and any route without a linearized catch) keep the
        // legacy inline-the-catch-body behavior.
        let state_based = fall_through && route.catch_entry_state.is_some();
        let then_branch = if state_based {
            let mut b = Vec::new();
            if let Some(cp_id) = route.param_id {
                b.push(Stmt::Expr(Expr::LocalSet(
                    cp_id,
                    Box::new(Expr::LocalGet(throw_param_id)),
                )));
            }
            b.push(Stmt::Expr(Expr::LocalSet(
                state_id,
                Box::new(Expr::Number(route.catch_entry_state.unwrap() as f64)),
            )));
            b
        } else {
            build_async_catch_route_body(
                route,
                finallys,
                state_id,
                done_id,
                throw_param_id,
                inner_catch_id,
                hoisted_ids,
                fall_through,
            )
        };
        fallback = vec![Stmt::If {
            condition: catch_route_condition(route, state_id, state_based, false),
            then_branch,
            else_branch: Some(fallback),
        }];
    }

    // #4374: append the continuation loop. Only a fallen-through catch route
    // reaches it (the no-catch branch throws and the catch routes either
    // `return` on a user `return`/finally-return or fall through after setting
    // the resume state).
    if let Some(cont) = continuation {
        fallback.push(Stmt::While {
            condition: Expr::Bool(true),
            body: cont,
        });
    }

    fallback
}

fn catch_route_condition(
    route: &CatchRoute,
    state_id: LocalId,
    state_based: bool,
    inclusive_lower: bool,
) -> Expr {
    // Awaited rejection re-enters after the yield state has advanced to its
    // resume/post state, so lifted catch ownership is open on the start state
    // and closed on the post-catch state.
    //
    // #4438: for sync state-based routing the upper bound is
    // `protected_end_state` (the post-last-yield-in-try happy landing state),
    // which EXCLUDES the catch's own states — a throw inside the catch must
    // escape to an enclosing handler, not re-enter this one. The legacy inline
    // (async) path keeps `post_catch_state` as before.
    //
    // `inclusive_lower` selects `>=` vs `>` on the start state. The runtime
    // dispatch wrapper (a `throw` *executing* inside a try) uses `>=`: a throw
    // in the try's first state runs at exactly `protected_start_state`. The
    // `.throw()`-injection path uses `>`: it only fires while *suspended* at a
    // yield, whose resume state is already `> protected_start_state`, and a
    // yield sitting just before the try (state == protected_start) is outside
    // the try and must not be caught.
    let upper = if state_based {
        route.protected_end_state
    } else {
        route.post_catch_state
    };
    let lower_op = if inclusive_lower {
        CompareOp::Ge
    } else {
        CompareOp::Gt
    };
    Expr::Logical {
        op: LogicalOp::And,
        left: Box::new(Expr::Compare {
            op: lower_op,
            left: Box::new(Expr::LocalGet(state_id)),
            right: Box::new(Expr::Number(route.protected_start_state as f64)),
        }),
        right: Box::new(Expr::Compare {
            op: CompareOp::Le,
            left: Box::new(Expr::LocalGet(state_id)),
            right: Box::new(Expr::Number(upper as f64)),
        }),
    }
}

/// #4438: build the catch handler for the sync-generator dispatch loop. When a
/// `throw` executes inside a try during a normal `.next()` dispatch, route the
/// error to the matching catch's linearized states (set the catch param, jump
/// to `catch_entry_state`, and `continue` the dispatch loop so the catch body
/// runs — including any `yield` inside it). When no catch owns the current
/// state, run any pending `finally`, complete the generator, and rethrow.
fn build_dispatch_catch_handler(
    catches: &[CatchRoute],
    finallys: &[FinallyRoute],
    state_id: LocalId,
    done_id: LocalId,
    err_id: LocalId,
    hoisted_ids: &std::collections::HashSet<LocalId>,
) -> Vec<Stmt> {
    let mut fallback = vec![Stmt::Expr(Expr::LocalSet(
        done_id,
        Box::new(Expr::Bool(true)),
    ))];
    fallback.extend(build_finally_run_stmts(finallys, state_id, hoisted_ids));
    fallback.push(Stmt::Throw(Expr::LocalGet(err_id)));
    for route in catches.iter().rev() {
        let Some(entry) = route.catch_entry_state else {
            continue;
        };
        let mut then_branch = Vec::new();
        if let Some(cp_id) = route.param_id {
            then_branch.push(Stmt::Expr(Expr::LocalSet(
                cp_id,
                Box::new(Expr::LocalGet(err_id)),
            )));
        }
        then_branch.push(Stmt::Expr(Expr::LocalSet(
            state_id,
            Box::new(Expr::Number(entry as f64)),
        )));
        then_branch.push(Stmt::Continue);
        fallback = vec![Stmt::If {
            condition: catch_route_condition(route, state_id, true, true),
            then_branch,
            else_branch: Some(fallback),
        }];
    }
    fallback
}

fn build_async_catch_route_body(
    route: &CatchRoute,
    finallys: &[FinallyRoute],
    state_id: LocalId,
    done_id: LocalId,
    throw_param_id: LocalId,
    inner_catch_id: LocalId,
    hoisted_ids: &std::collections::HashSet<LocalId>,
    // #4374: when true (sync generators), run the catch body, set the resume
    // state, and fall through to the caller's continuation loop instead of
    // returning {undefined, false}. A user `return`/finally-return inside the
    // catch still exits (it's rewritten to an iter-result return below).
    fall_through: bool,
) -> Vec<Stmt> {
    let mut body = Vec::new();
    if let Some(cp_id) = route.param_id {
        body.push(Stmt::Expr(Expr::LocalSet(
            cp_id,
            Box::new(Expr::LocalGet(throw_param_id)),
        )));
    }

    let mut rewritten = route.body.clone();
    rewrite_hoisted_lets_in_stmts(&mut rewritten, hoisted_ids);
    rewrite_yield_to_await_in_stmts(&mut rewritten);
    rewrite_catch_returns_to_iter_result(&mut rewritten);

    // #4374: if this try also has a (sync) finally, a `throw` inside the catch
    // handler must still run that finally before propagating. The normal
    // (catch completes) path runs the finally via the inlined post-catch state
    // in the continuation loop, so we only need to cover the throwing path:
    // wrap the catch body in `try { <catch> } catch (e) { <finally>; throw e }`.
    // On normal completion the inner catch never fires (no double finally run).
    let matching_finally = if fall_through {
        finallys.iter().find(|f| {
            !f.has_yields
                && f.protected_start_state == route.protected_start_state
                && f.post_finally_state == route.post_catch_state
        })
    } else {
        None
    };
    if let Some(fin) = matching_finally {
        let mut fin_body = fin.body.clone();
        rewrite_hoisted_lets_in_stmts(&mut fin_body, hoisted_ids);
        rewrite_catch_returns_to_iter_result(&mut fin_body);
        let mut handler = vec![Stmt::Expr(Expr::LocalSet(
            done_id,
            Box::new(Expr::Bool(true)),
        ))];
        handler.extend(fin_body);
        handler.push(Stmt::Throw(Expr::LocalGet(inner_catch_id)));
        body.push(Stmt::Try {
            body: rewritten,
            catch: Some(CatchClause {
                param: Some((inner_catch_id, "__gen_fin_e".to_string())),
                body: handler,
            }),
            finally: None,
        });
    } else {
        body.extend(rewritten);
    }

    body.push(Stmt::Expr(Expr::LocalSet(
        state_id,
        Box::new(Expr::Number(route.post_catch_state as f64)),
    )));
    if !fall_through {
        body.push(Stmt::Return(Some(make_iter_result(Expr::Undefined, false))));
    }
    body
}

/// #4374: build the statements that run pending `finally` blocks on abrupt
/// completion (`.return()`/`.throw()`), innermost first. Each finally runs
/// only when the generator is suspended inside its protected state interval
/// (`state > protected_start && state <= post_finally`). A `return X` inside
/// a finally is rewritten to `return {value: X, done: true}` so it supersedes
/// the abrupt completion value; a `throw` inside a finally is left intact and
/// propagates out of the closure. Finallys that themselves yield/await
/// (`has_yields`) can't be inlined synchronously and are skipped.
fn build_finally_run_stmts(
    finallys: &[FinallyRoute],
    state_id: LocalId,
    hoisted_ids: &std::collections::HashSet<LocalId>,
) -> Vec<Stmt> {
    let mut out = Vec::new();
    for route in finallys.iter().filter(|r| !r.has_yields) {
        let mut body = route.body.clone();
        rewrite_hoisted_lets_in_stmts(&mut body, hoisted_ids);
        rewrite_catch_returns_to_iter_result(&mut body);
        out.push(Stmt::If {
            condition: finally_route_condition(route, state_id),
            then_branch: body,
            else_branch: None,
        });
    }
    out
}

fn finally_route_condition(route: &FinallyRoute, state_id: LocalId) -> Expr {
    Expr::Logical {
        op: LogicalOp::And,
        left: Box::new(Expr::Compare {
            op: CompareOp::Gt,
            left: Box::new(Expr::LocalGet(state_id)),
            right: Box::new(Expr::Number(route.protected_start_state as f64)),
        }),
        right: Box::new(Expr::Compare {
            op: CompareOp::Le,
            left: Box::new(Expr::LocalGet(state_id)),
            right: Box::new(Expr::Number(route.post_finally_state as f64)),
        }),
    }
}

fn build_async_throw_body_direct(
    catches: Vec<CatchRoute>,
    state_id: LocalId,
    throw_param_id: LocalId,
    hoisted_ids: &std::collections::HashSet<LocalId>,
    step_done_label: &str,
) -> Vec<Stmt> {
    let mut fallback = vec![Stmt::Throw(Expr::LocalGet(throw_param_id))];

    for route in catches.into_iter().rev() {
        let condition = catch_route_condition(&route, state_id, false, false);
        let then_branch = build_async_catch_route_body_direct(
            route,
            state_id,
            throw_param_id,
            hoisted_ids,
            step_done_label,
        );
        fallback = vec![Stmt::If {
            condition,
            then_branch,
            else_branch: Some(fallback),
        }];
    }

    fallback
}

fn build_async_catch_route_body_direct(
    route: CatchRoute,
    state_id: LocalId,
    throw_param_id: LocalId,
    hoisted_ids: &std::collections::HashSet<LocalId>,
    step_done_label: &str,
) -> Vec<Stmt> {
    let mut body = Vec::new();
    if let Some(cp_id) = route.param_id {
        body.push(Stmt::Expr(Expr::LocalSet(
            cp_id,
            Box::new(Expr::LocalGet(throw_param_id)),
        )));
    }

    let mut rewritten = route.body;
    rewrite_hoisted_lets_in_stmts(&mut rewritten, hoisted_ids);
    rewrite_yield_to_await_in_stmts(&mut rewritten);
    rewrite_catch_returns_to_iter_result(&mut rewritten);
    rewrite_returns_to_labeled_break(&mut rewritten, step_done_label);
    rewrite_iter_results_in_stmts(&mut rewritten);
    body.extend(rewritten);

    body.push(Stmt::Expr(Expr::LocalSet(
        state_id,
        Box::new(Expr::Number(route.post_catch_state as f64)),
    )));
    body
}

/// Build the async step driver without allocating the `__iter` object.
/// allocation entirely. Used for `was_plain_async = true` generators
/// where the iter object is never observable from user code (the
/// async-step driver wraps the generator into a Promise-returning
/// shape; the user never holds an iterator handle). Captures the
/// next/throw closures directly as locals so the step body's
/// `__iter.next(value)` becomes a single LocalGet+Call instead of a
/// PropertyGet+Call. Also drops the `return` closure (never invoked
/// for plain-async — spec `gen.return()` can't be called when the
/// function returns a Promise instead of an iterator).
pub fn build_async_step_driver_direct(
    next_body: Vec<Stmt>,
    next_param_id: LocalId,
    next_captures: Vec<LocalId>,
    next_mutable_captures: Vec<LocalId>,
    throw_closure_expr: Option<Expr>,
    throw_routes_direct: Option<(Vec<CatchRoute>, LocalId, std::collections::HashSet<LocalId>)>,
    throw_param_id: LocalId,
    next_local_id: &mut u32,
    next_func_id: &mut u32,
    captures_this: bool,
    captures_new_target: bool,
    enclosing_class: Option<String>,
    is_strict: bool,
) -> Vec<Stmt> {
    // When `throw_closure_expr` is None, the function had no awaiting
    // try/catch so the throw path is a plain rethrow — we inline it
    // directly into the step body and skip the per-invocation
    // `__async_throw` allocation entirely.
    let throw_id = throw_closure_expr
        .as_ref()
        .map(|_| alloc_local(next_local_id));
    // #691 Phase 2: step closure no longer captures itself. Body
    // uses `Expr::CurrentStepClosure` (reads INLINE_TRAP.current_step
    // TLS) wherever it previously did `LocalGet(step_id)`. The
    // wrapper still needs a local to hand the freshly-constructed
    // closure to `Expr::AsyncFirstCall`, but it's a regular immutable
    // let (no `js_box_alloc`).
    let step_id = alloc_local(next_local_id);

    // Step closure params + locals
    let value_param_id = alloc_local(next_local_id);
    let is_error_param_id = alloc_local(next_local_id);
    let catch_e_id = alloc_local(next_local_id);
    let step_self_id = alloc_local(next_local_id);

    let step_func_id = {
        let id = *next_func_id;
        *next_func_id += 1;
        id
    };

    let any_ty = Type::Any;
    let bool_ty = Type::Boolean;

    let promise_global = || Expr::GlobalGet(0);
    // #854: paired resolve-builder kept alongside the used promise_reject for
    // symmetry of the async-step driver; not emitted on the current path.
    let _promise_resolve = |arg: Expr| Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(promise_global()),
            property: "resolve".to_string(),
        }),
        args: vec![arg],
        type_args: vec![],
    };
    let promise_reject = |arg: Expr| Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(promise_global()),
            property: "reject".to_string(),
        }),
        args: vec![arg],
        type_args: vec![],
    };

    // Rewrite every Return inside next_body to LabeledBreak(__step_done)
    // so they fall through to step's post-dispatch code instead of
    // exiting step entirely. The IterResultSet expression sets the
    // (value, done) TLS slots; LabeledBreak escapes the inlined body.
    let step_done_label = "__step_done".to_string();
    let mut next_body = next_body;
    rewrite_returns_to_labeled_break(&mut next_body, &step_done_label);

    // The inlined next_body references `next_param_id` (the original
    // `__val` parameter of the next closure). After fusion that ID
    // becomes a local of step; we initialize it from value_param_id
    // before running the body.
    let next_value_let = Stmt::Let {
        id: next_param_id,
        name: "__val".to_string(),
        ty: any_ty.clone(),
        mutable: false,
        init: Some(Expr::LocalGet(value_param_id)),
    };
    // step body
    //   try {
    //     "__step_done": do {
    //        if (isError) {
    //            // when no user catch: throw value; (caught by outer try)
    //            // when user catch: __throw(value);
    //        } else { let __val = value; <next_body inlined> }
    //     } while (false);
    //   } catch (e) {
    //     if (isError) return Promise.reject(e);
    //     return __step(e, true);
    //   }
    //   if (js_iter_result_get_done()) return Promise.resolve(js_iter_result_get_value());
    //   return AsyncStepChain(js_iter_result_get_value(), __step);
    let mut direct_routes_enabled = false;
    let throw_arm: Vec<Stmt> =
        if let Some((catches, route_state_id, route_hoisted_ids)) = throw_routes_direct {
            direct_routes_enabled = true;
            let mut body = vec![Stmt::Let {
                id: throw_param_id,
                name: "__throw_val".to_string(),
                ty: any_ty.clone(),
                mutable: false,
                init: Some(Expr::LocalGet(value_param_id)),
            }];
            let direct_body = build_async_throw_body_direct(
                catches,
                route_state_id,
                throw_param_id,
                &route_hoisted_ids,
                &step_done_label,
            );
            body.extend(direct_body);
            body
        } else if let Some(tid) = throw_id {
            vec![Stmt::Expr(Expr::Call {
                callee: Box::new(Expr::LocalGet(tid)),
                args: vec![Expr::LocalGet(value_param_id)],
                type_args: vec![],
            })]
        } else {
            // No __async_throw closure was constructed (callee passed None).
            // The throw body would have been a plain rethrow, so inline it:
            // the outer try/catch re-enters __step(e, true) which then hits
            // this same path with isError=true a second time, and the catch
            // arm returns Promise.reject (the `if (isError)` short-circuit).
            vec![Stmt::Throw(Expr::LocalGet(value_param_id))]
        };
    let labeled_body = if direct_routes_enabled {
        let mut normal_tail = next_body;
        let normal_sent = if normal_tail.is_empty() {
            None
        } else {
            Some(normal_tail.remove(0))
        };
        let direct_dispatch = Stmt::If {
            condition: Expr::LocalGet(is_error_param_id),
            then_branch: throw_arm,
            else_branch: normal_sent.map(|stmt| vec![stmt]),
        };
        let mut body = vec![next_value_let, direct_dispatch];
        body.extend(normal_tail);
        body
    } else {
        let mut else_branch: Vec<Stmt> = vec![next_value_let];
        else_branch.extend(next_body);
        let dispatch_inner = Stmt::If {
            condition: Expr::LocalGet(is_error_param_id),
            then_branch: throw_arm,
            else_branch: Some(else_branch),
        };
        vec![dispatch_inner]
    };

    // Wrap dispatch in `do { dispatch; } while(false)` so the
    // wrapping `Stmt::Labeled` registers its label on a loop —
    // codegen's `label_targets` map is populated only for for/while/
    // do-while bodies, so plain `Stmt::Labeled { body: If }` would
    // leave LabeledBreak with no jump target. DoWhile with a constant-
    // false condition runs the body exactly once.
    let labeled_loop = Stmt::Labeled {
        label: step_done_label.clone(),
        body: Box::new(Stmt::DoWhile {
            body: labeled_body,
            condition: Expr::Bool(false),
        }),
    };

    let step_body: Vec<Stmt> = vec![
        Stmt::Let {
            id: step_self_id,
            name: "__step_self".to_string(),
            ty: any_ty.clone(),
            mutable: false,
            init: Some(Expr::CurrentStepClosure),
        },
        Stmt::Try {
            body: vec![labeled_loop],
            catch: Some(CatchClause {
                param: Some((catch_e_id, "__step_catch_e".to_string())),
                body: vec![
                    Stmt::If {
                        condition: Expr::LocalGet(is_error_param_id),
                        then_branch: vec![Stmt::Return(Some(promise_reject(Expr::LocalGet(
                            catch_e_id,
                        ))))],
                        else_branch: None,
                    },
                    // Use the step closure captured at entry so nested
                    // calls cannot disturb the TLS self-reference before
                    // the error re-entry path runs.
                    Stmt::Return(Some(Expr::Call {
                        callee: Box::new(Expr::LocalGet(step_self_id)),
                        args: vec![Expr::LocalGet(catch_e_id), Expr::Bool(true)],
                        type_args: vec![],
                    })),
                ],
            }),
            finally: None,
        },
        Stmt::If {
            condition: Expr::IterResultGetDone,
            // Optimized: AsyncStepDone reuses INLINE_TRAP_NEXT instead
            // of allocating a fresh `Promise.resolve(value)` Promise.
            // Saves one js_promise_resolved alloc per async function
            // call (50k/run on promise_all_chains).
            then_branch: vec![Stmt::Return(Some(Expr::AsyncStepDone {
                value: Box::new(Expr::IterResultGetValue),
                step_closure: Box::new(Expr::LocalGet(step_self_id)),
            }))],
            else_branch: None,
        },
        Stmt::Return(Some(Expr::AsyncStepChain {
            value: Box::new(Expr::IterResultGetValue),
            step_closure: Box::new(Expr::LocalGet(step_self_id)),
        })),
    ];

    // step closure captures = next_captures + [throw_id?]
    // #691 Phase 2: step_id is NOT captured — the body reads its own
    // pointer via `Expr::CurrentStepClosure` (INLINE_TRAP.current_step
    // TLS). This saves one capture slot per step closure and removes
    // the per-invocation `js_box_alloc` for step_id.
    let mut step_captures: Vec<LocalId> = next_captures;
    if let Some(tid) = throw_id {
        step_captures.push(tid);
    }
    step_captures.sort();
    step_captures.dedup();
    let step_mut_captures: Vec<LocalId> = next_mutable_captures;

    let step_closure = Expr::Closure {
        func_id: step_func_id,
        params: vec![
            perry_hir::Param {
                id: value_param_id,
                name: "__step_value".to_string(),
                ty: any_ty.clone(),
                is_rest: false,
                default: None,
                decorators: Vec::new(),
                arguments_object: None,
            },
            perry_hir::Param {
                id: is_error_param_id,
                name: "__step_is_error".to_string(),
                ty: bool_ty.clone(),
                is_rest: false,
                default: None,
                decorators: Vec::new(),
                arguments_object: None,
            },
        ],
        return_type: any_ty.clone(),
        body: step_body,
        captures: step_captures,
        mutable_captures: step_mut_captures,
        captures_this,
        captures_new_target,
        enclosing_class: enclosing_class.clone(),
        is_arrow: false,
        is_strict,
        is_async: false,
        is_generator: false,
    };

    // Outer wrapper:
    //   let __throw = <throw_closure>;   // omitted when throw_id is None
    //   let __step = <step_closure>;     // #691 Phase 2: immutable,
    //                                    //   no js_box_alloc
    //   return AsyncFirstCall(__step);   // sets TLS, calls
    //                                    //   step(undefined, false)
    let mut wrapper: Vec<Stmt> = Vec::with_capacity(3);
    if let (Some(tid), Some(tc_expr)) = (throw_id, throw_closure_expr) {
        wrapper.push(Stmt::Let {
            id: tid,
            name: "__async_throw".to_string(),
            ty: any_ty.clone(),
            mutable: false,
            init: Some(tc_expr),
        });
    }
    wrapper.extend([
        Stmt::Let {
            id: step_id,
            name: "__async_step".to_string(),
            ty: any_ty.clone(),
            mutable: false,
            init: Some(step_closure),
        },
        Stmt::Return(Some(Expr::AsyncFirstCall {
            step_closure: Box::new(Expr::LocalGet(step_id)),
        })),
    ]);
    wrapper
}
