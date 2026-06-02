//! Generator-function state-machine lowering and async step-driver construction.

use super::*;

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
    enclosing_class: Option<String>,
) {
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
    linearize_body(
        &func.body,
        &mut states,
        &mut current,
        &mut state_num,
        state_id,
        next_local_id,
        sent_id,
        &mut catches,
    );
    let extra_local_ids: Vec<LocalId> = (local_id_before..*next_local_id).collect();

    // Push final state (code after last yield / end of function)
    states.push(State {
        num: state_num,
        body: current,
        exit: StateExit::Done,
    });

    // Collect hoisted var IDs first so we know which Lets to rewrite
    let hoisted_ids: std::collections::HashSet<LocalId> = collect_hoisted_vars(&func.body)
        .iter()
        .map(|(id, _, _)| *id)
        .collect();

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
    for state in &states {
        let mut case_body = state.body.clone();
        match &state.exit {
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
                    Box::new(Expr::Number(*next_state as f64)),
                )));
                case_body.push(Stmt::Return(Some(make_iter_result(value.clone(), false))));
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
                    Box::new(Expr::Number(*next_state as f64)),
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
                right: Box::new(Expr::Number(state.num as f64)),
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

    // Build next() method body
    let mut next_body = vec![
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
            body: while_body,
        },
    ];
    if is_async_generator {
        wrap_returns_in_promise(&mut next_body);
    }

    // Build the new function body
    let mut new_body: Vec<Stmt> = Vec::new();

    // Hoist variable declarations from the original body — collected
    // here (before the prealloc emit) so the prealloc set is complete.
    let hoisted = collect_hoisted_vars(&func.body);

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
    let mut prealloc_ids: Vec<LocalId> = vec![state_id, done_id, sent_id];
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
    let mut captures = vec![state_id, done_id, sent_id];
    let mut mutable_captures = vec![state_id, done_id, sent_id];
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

    // Build .return(value) closure — immediately marks done and returns {value, done: true}
    let return_param_id = alloc_local(next_local_id);
    let return_func_id_val = {
        let id = *next_func_id;
        *next_func_id += 1;
        id
    };
    let mut return_body: Vec<Stmt> = vec![
        Stmt::Expr(Expr::LocalSet(done_id, Box::new(Expr::Bool(true)))),
        Stmt::Return(Some(make_iter_result(
            Expr::LocalGet(return_param_id),
            true,
        ))),
    ];
    if is_async_generator {
        wrap_returns_in_promise(&mut return_body);
    }
    let return_closure = Expr::Closure {
        func_id: return_func_id_val,
        params: vec![perry_hir::Param {
            id: return_param_id,
            name: "__ret_val".to_string(),
            ty: Type::Any,
            is_rest: false,
            default: None,
            decorators: Vec::new(),
        }],
        return_type: Type::Any,
        body: return_body,
        captures: captures.clone(),
        mutable_captures: mutable_captures.clone(),
        captures_this,
        enclosing_class: enclosing_class.clone(),
        is_strict: func.is_strict,
        is_async: false,
        is_generator: false,
    };

    // Build .throw(error) closure. Each catch route owns the state interval
    // for the try body it protects, so multiple independent try/catch regions
    // in the same async function resume at the correct post-catch state.
    let throw_param_id = alloc_local(next_local_id);
    let throw_func_id_val = {
        let id = *next_func_id;
        *next_func_id += 1;
        id
    };
    let mut throw_body = build_async_throw_body(&catches, state_id, throw_param_id, &hoisted_ids);
    if is_async_generator {
        wrap_returns_in_promise(&mut throw_body);
    }
    let throw_closure = Expr::Closure {
        func_id: throw_func_id_val,
        params: vec![perry_hir::Param {
            id: throw_param_id,
            name: "__throw_val".to_string(),
            ty: Type::Any,
            is_rest: false,
            default: None,
            decorators: Vec::new(),
        }],
        return_type: Type::Any,
        body: throw_body.clone(),
        captures: captures.clone(),
        mutable_captures: mutable_captures.clone(),
        captures_this,
        enclosing_class: enclosing_class.clone(),
        is_strict: func.is_strict,
        is_async: false,
        is_generator: false,
    };

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
            Some((catches.clone(), state_id, hoisted_ids.clone()))
        };
        let mut next_body_for_step = next_body;
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
        // Plain generator: build the iterator object and return it directly.
        let next_closure = Expr::Closure {
            func_id: next_func_id_val,
            params: vec![perry_hir::Param {
                id: next_param_id,
                name: "__val".to_string(),
                ty: Type::Any,
                is_rest: false,
                default: None,
                decorators: Vec::new(),
            }],
            return_type: Type::Any,
            body: next_body,
            captures: captures.clone(),
            mutable_captures: mutable_captures.clone(),
            captures_this,
            enclosing_class: enclosing_class.clone(),
            is_strict: func.is_strict,
            is_async: false,
            is_generator: false,
        };
        let iter_obj = Expr::Object(vec![
            ("next".to_string(), next_closure),
            ("return".to_string(), return_closure),
            ("throw".to_string(), throw_closure),
        ]);
        new_body.push(Stmt::Return(Some(iter_obj)));
    }

    func.body = new_body;
    func.is_generator = false;
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
    state_id: LocalId,
    throw_param_id: LocalId,
    hoisted_ids: &std::collections::HashSet<LocalId>,
) -> Vec<Stmt> {
    let mut fallback = vec![Stmt::Throw(Expr::LocalGet(throw_param_id))];

    // Build nested `if` dispatch in source order. We iterate from the back so
    // the first collected route is tested first; nested try/catch routes are
    // collected before their containing route and should win on overlap.
    for route in catches.iter().rev() {
        let then_branch =
            build_async_catch_route_body(route, state_id, throw_param_id, hoisted_ids);
        fallback = vec![Stmt::If {
            condition: catch_route_condition(route, state_id),
            then_branch,
            else_branch: Some(fallback),
        }];
    }

    fallback
}

fn catch_route_condition(route: &CatchRoute, state_id: LocalId) -> Expr {
    // Awaited rejection re-enters after the yield state has advanced to its
    // resume/post state, so lifted catch ownership is open on the start state
    // and closed on the post-catch state.
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
            right: Box::new(Expr::Number(route.post_catch_state as f64)),
        }),
    }
}

fn build_async_catch_route_body(
    route: &CatchRoute,
    state_id: LocalId,
    throw_param_id: LocalId,
    hoisted_ids: &std::collections::HashSet<LocalId>,
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
    body.extend(rewritten);

    body.push(Stmt::Expr(Expr::LocalSet(
        state_id,
        Box::new(Expr::Number(route.post_catch_state as f64)),
    )));
    body.push(Stmt::Return(Some(make_iter_result(Expr::Undefined, false))));
    body
}

fn build_async_throw_body_direct(
    catches: &[CatchRoute],
    state_id: LocalId,
    throw_param_id: LocalId,
    hoisted_ids: &std::collections::HashSet<LocalId>,
    step_done_label: &str,
) -> Vec<Stmt> {
    let mut fallback = vec![Stmt::Throw(Expr::LocalGet(throw_param_id))];

    for route in catches.iter().rev() {
        let then_branch = build_async_catch_route_body_direct(
            route,
            state_id,
            throw_param_id,
            hoisted_ids,
            step_done_label,
        );
        fallback = vec![Stmt::If {
            condition: catch_route_condition(route, state_id),
            then_branch,
            else_branch: Some(fallback),
        }];
    }

    fallback
}

fn build_async_catch_route_body_direct(
    route: &CatchRoute,
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

    let mut rewritten = route.body.clone();
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
    let mut else_branch: Vec<Stmt> = vec![next_value_let.clone()];
    else_branch.extend(next_body.clone());

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
                &catches,
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
    let dispatch_inner = Stmt::If {
        condition: Expr::LocalGet(is_error_param_id),
        then_branch: throw_arm,
        else_branch: Some(else_branch),
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
            then_branch: match dispatch_inner {
                Stmt::If { then_branch, .. } => then_branch,
                _ => unreachable!("dispatch_inner is always an if"),
            },
            else_branch: normal_sent.map(|stmt| vec![stmt]),
        };
        let mut body = vec![next_value_let, direct_dispatch];
        body.extend(normal_tail);
        body
    } else {
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
            },
            perry_hir::Param {
                id: is_error_param_id,
                name: "__step_is_error".to_string(),
                ty: bool_ty.clone(),
                is_rest: false,
                default: None,
                decorators: Vec::new(),
            },
        ],
        return_type: any_ty.clone(),
        body: step_body,
        captures: step_captures,
        mutable_captures: step_mut_captures,
        captures_this,
        enclosing_class: enclosing_class.clone(),
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
