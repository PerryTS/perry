//! Abrupt-completion routing for sync generators: catch/finally interval
//! conditions, the merged abrupt-routing if-chain, the dispatch-loop
//! try/catch wrapper, and the async `.throw()` catch-route builders. Split
//! out of `lower.rs`.

use super::*;

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
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_async_throw_body(
    catches: &[CatchRoute],
    finallys: &[FinallyRoute],
    state_id: LocalId,
    done_id: LocalId,
    throw_param_id: LocalId,
    inner_catch_id: LocalId,
    pending_type_id: LocalId,
    pending_value_id: LocalId,
    hoisted_ids: &std::collections::HashSet<LocalId>,
    // #4374: for sync generators, the cloned state-dispatch loop. When present,
    // a matched catch route sets the resume state and *falls through* to this
    // loop, so the inlined finally runs and the generator continues to the next
    // yield / completion within the `.throw()` call. When `None` (async
    // generators) the catch route returns {undefined, false} as before.
    continuation: Option<Vec<Stmt>>,
) -> Vec<Stmt> {
    let fall_through = continuation.is_some();
    // #4374: when no catch handles the throw, run any pending non-yielding
    // `finally` before propagating the error. A `finally` that `return`s
    // supersedes the thrown value (rewritten to an iter-result return inside
    // build_finally_run_stmts). For a try WITH a catch, a route below matches
    // first, so this only fires for unhandled throws.
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

    let mut body = if fall_through {
        // #4438: sync generators route the thrown error to the innermost
        // enclosing catch (jump to its linearized states) or yielding finally
        // (record the pending throw + jump in), then fall through to the
        // appended continuation loop which dispatches it — so a `yield` inside
        // the catch/finally suspends.
        build_abrupt_routing(
            catches,
            finallys,
            state_id,
            pending_type_id,
            pending_value_id,
            &Expr::LocalGet(throw_param_id),
            true,
            1.0,
            false,
            false,
            fallback,
        )
    } else {
        // Async generators: legacy inline-the-catch-body behavior.
        for route in catches.iter().rev() {
            let then_branch = build_async_catch_route_body(
                route,
                finallys,
                state_id,
                done_id,
                throw_param_id,
                inner_catch_id,
                hoisted_ids,
                fall_through,
            );
            fallback = vec![Stmt::If {
                condition: catch_route_condition(route, state_id, false, false),
                then_branch,
                else_branch: Some(fallback),
            }];
        }
        fallback
    };

    // #4374: append the continuation loop. Only a fallen-through catch/finally
    // route reaches it (the unhandled branch throws; matched routes set the
    // resume state and fall through).
    if let Some(cont) = continuation {
        body.push(Stmt::While {
            condition: Expr::Bool(true),
            body: cont,
        });
    }

    body
}

pub(crate) fn catch_route_condition(
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

/// #4438 B2-finally: interval condition for routing an abrupt completion into a
/// yielding finally — `state` in (or `>=` for runtime throws) the protected try
/// interval, up to `protected_end_state` (which excludes the finally's own
/// states so a completion while suspended INSIDE the finally supersedes it).
pub(crate) fn finally_abrupt_condition(
    route: &FinallyRoute,
    state_id: LocalId,
    inclusive_lower: bool,
) -> Expr {
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
            right: Box::new(Expr::Number(route.protected_end_state as f64)),
        }),
    }
}

/// #4438 B2-finally: the re-raise appended to a yielding finally's
/// completion-check state. After the finally runs, a pending throw is re-thrown
/// (and re-routed by the dispatch wrapper to an enclosing handler, or propagated
/// when unhandled) and a pending return completes the generator with its value.
/// On the normal path (`pending_type == 0`) both checks are skipped.
pub(crate) fn build_completion_resume_stmts(
    pending_type_id: LocalId,
    pending_value_id: LocalId,
    done_id: LocalId,
) -> Vec<Stmt> {
    vec![
        Stmt::If {
            condition: Expr::Compare {
                op: CompareOp::Eq,
                left: Box::new(Expr::LocalGet(pending_type_id)),
                right: Box::new(Expr::Number(1.0)),
            },
            then_branch: vec![
                Stmt::Expr(Expr::LocalSet(pending_type_id, Box::new(Expr::Number(0.0)))),
                Stmt::Throw(Expr::LocalGet(pending_value_id)),
            ],
            else_branch: None,
        },
        Stmt::If {
            condition: Expr::Compare {
                op: CompareOp::Eq,
                left: Box::new(Expr::LocalGet(pending_type_id)),
                right: Box::new(Expr::Number(2.0)),
            },
            then_branch: vec![
                Stmt::Expr(Expr::LocalSet(pending_type_id, Box::new(Expr::Number(0.0)))),
                Stmt::Expr(Expr::LocalSet(done_id, Box::new(Expr::Bool(true)))),
                Stmt::Return(Some(make_iter_result(
                    Expr::LocalGet(pending_value_id),
                    true,
                ))),
            ],
            else_branch: None,
        },
    ]
}

/// #4438: build the merged abrupt-completion routing if-chain for sync
/// generators. A thrown error / returned value routes to the innermost
/// enclosing handler: a `catch` (jump to its linearized states) or a yielding
/// `finally` (record the pending completion + jump into the finally). Routes are
/// ordered innermost-first (protected-start descending; a `catch` beats a
/// `finally` at the same try). `value_src` is the error/return value;
/// `pending_kind` is 1 (throw) or 2 (return) for finally routes. `with_continue`
/// appends `continue` (dispatch wrapper) vs falling through (the throw/return
/// closures, which append their own continuation loop). When nothing matches the
/// current state, `fallback` runs.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_abrupt_routing(
    catches: &[CatchRoute],
    finallys: &[FinallyRoute],
    state_id: LocalId,
    pending_type_id: LocalId,
    pending_value_id: LocalId,
    value_src: &Expr,
    include_catch: bool,
    pending_kind: f64,
    with_continue: bool,
    inclusive_lower: bool,
    fallback: Vec<Stmt>,
) -> Vec<Stmt> {
    // (protected_start, kind, index): kind 0 = catch, 1 = finally.
    let mut routes: Vec<(u32, u8, usize)> = Vec::new();
    if include_catch {
        for (i, r) in catches.iter().enumerate() {
            if r.catch_entry_state.is_some() {
                routes.push((r.protected_start_state, 0, i));
            }
        }
    }
    for (i, r) in finallys.iter().enumerate() {
        if r.finally_entry_state.is_some() {
            routes.push((r.protected_start_state, 1, i));
        }
    }
    // Innermost first: start descending, catch before finally on a tie.
    routes.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

    let mut chain = fallback;
    for (_, kind, idx) in routes.iter().rev() {
        let (condition, mut then_branch) = if *kind == 0 {
            let route = &catches[*idx];
            let mut a = Vec::new();
            if let Some(cp_id) = route.param_id {
                a.push(Stmt::Expr(Expr::LocalSet(
                    cp_id,
                    Box::new(value_src.clone()),
                )));
            }
            a.push(Stmt::Expr(Expr::LocalSet(
                state_id,
                Box::new(Expr::Number(route.catch_entry_state.unwrap() as f64)),
            )));
            (
                catch_route_condition(route, state_id, true, inclusive_lower),
                a,
            )
        } else {
            let route = &finallys[*idx];
            let a = vec![
                Stmt::Expr(Expr::LocalSet(
                    pending_type_id,
                    Box::new(Expr::Number(pending_kind)),
                )),
                Stmt::Expr(Expr::LocalSet(
                    pending_value_id,
                    Box::new(value_src.clone()),
                )),
                Stmt::Expr(Expr::LocalSet(
                    state_id,
                    Box::new(Expr::Number(route.finally_entry_state.unwrap() as f64)),
                )),
            ];
            (
                finally_abrupt_condition(route, state_id, inclusive_lower),
                a,
            )
        };
        if with_continue {
            then_branch.push(Stmt::Continue);
        }
        chain = vec![Stmt::If {
            condition,
            then_branch,
            else_branch: Some(chain),
        }];
    }
    chain
}

/// #4438: wrap a state-dispatch loop body in a real `try/catch` whose handler
/// routes a throw executing during dispatch to the matching catch/finally
/// (`continue`) or runs pending non-yielding finallys + completes + rethrows
/// when unhandled. Used for the `.next()` loop and the `.throw()`/`.return()`
/// continuation loops alike.
#[allow(clippy::too_many_arguments)]
pub(crate) fn wrap_dispatch_loop(
    loop_body: Vec<Stmt>,
    catches: &[CatchRoute],
    finallys: &[FinallyRoute],
    state_id: LocalId,
    done_id: LocalId,
    pending_type_id: LocalId,
    pending_value_id: LocalId,
    err_id: LocalId,
    hoisted_ids: &std::collections::HashSet<LocalId>,
) -> Vec<Stmt> {
    let handler = build_dispatch_catch_handler(
        catches,
        finallys,
        state_id,
        done_id,
        pending_type_id,
        pending_value_id,
        err_id,
        hoisted_ids,
    );
    vec![Stmt::Try {
        body: loop_body,
        catch: Some(CatchClause {
            param: Some((err_id, "__gen_disp_err".to_string())),
            body: handler,
        }),
        finally: None,
    }]
}

/// #4438: the catch handler for the sync-generator dispatch loop. Routes a throw
/// executing inside a try (during a normal `.next()`) to the matching catch's
/// or yielding finally's states (and `continue`s the loop), or runs pending
/// non-yielding finallys + completes + rethrows when unhandled.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_dispatch_catch_handler(
    catches: &[CatchRoute],
    finallys: &[FinallyRoute],
    state_id: LocalId,
    done_id: LocalId,
    pending_type_id: LocalId,
    pending_value_id: LocalId,
    err_id: LocalId,
    hoisted_ids: &std::collections::HashSet<LocalId>,
) -> Vec<Stmt> {
    let mut fallback = vec![Stmt::Expr(Expr::LocalSet(
        done_id,
        Box::new(Expr::Bool(true)),
    ))];
    fallback.extend(build_finally_run_stmts(finallys, state_id, hoisted_ids));
    fallback.push(Stmt::Throw(Expr::LocalGet(err_id)));
    build_abrupt_routing(
        catches,
        finallys,
        state_id,
        pending_type_id,
        pending_value_id,
        &Expr::LocalGet(err_id),
        true,
        1.0,
        true,
        true,
        fallback,
    )
}

/// Replace the synthesized dispatch re-entry `Stmt::Continue` (emitted by
/// `rewrite_break_continue_in_stmts` for a user `break`/`continue`) with a
/// suspend-return. Used when inlining a catch-route body into the async
/// `.throw()` closure, which has no dispatch `while(true)` loop. Mirrors the
/// recursion in `rewrite_break_continue_in_stmt`: descends into `if`/`try`
/// (where the dispatch continue can sit) but stops at nested loops / switch /
/// labeled / closures, whose own `continue`/`break` belong to them.
pub(crate) fn rewrite_dispatch_continue_to_suspend(stmts: &mut Vec<Stmt>) {
    for stmt in stmts.iter_mut() {
        match stmt {
            Stmt::Continue | Stmt::Break => {
                *stmt = Stmt::Return(Some(make_iter_result(Expr::Undefined, false)));
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                rewrite_dispatch_continue_to_suspend(then_branch);
                if let Some(eb) = else_branch.as_mut() {
                    rewrite_dispatch_continue_to_suspend(eb);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                rewrite_dispatch_continue_to_suspend(body);
                if let Some(c) = catch.as_mut() {
                    rewrite_dispatch_continue_to_suspend(&mut c.body);
                }
                if let Some(f) = finally.as_mut() {
                    rewrite_dispatch_continue_to_suspend(f);
                }
            }
            // Nested loops / switch / labeled / closures own their own
            // break/continue — leave them untouched.
            _ => {}
        }
    }
}

pub(crate) fn build_async_catch_route_body(
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

    // Legacy async path: a `.throw()` resumed into this catch closes the
    // generator if the catch handler `return`s (the rewrite below turns
    // `return X` into `return {value: X, done: true}`, which exits the closure
    // *before* the post-catch state/`done` bookkeeping runs). Mark `done = true`
    // up front so a subsequent `.next()` sees a completed generator; if the
    // catch instead completes normally and falls through, the reset below
    // restores `done = false` so the post-catch suspension stays live.
    if !fall_through {
        body.push(Stmt::Expr(Expr::LocalSet(
            done_id,
            Box::new(Expr::Bool(true)),
        )));
    }

    let mut rewritten = route.body.clone();
    rewrite_hoisted_lets_in_stmts(&mut rewritten, hoisted_ids);
    rewrite_yield_to_await_in_stmts(&mut rewritten);
    rewrite_catch_returns_to_iter_result(&mut rewritten);
    // A user `break`/`continue` inside this catch was rewritten by
    // `rewrite_break_continue_in_stmts` into `[LocalSet(state, TARGET),
    // Stmt::Continue]` — the trailing `Stmt::Continue` re-enters the dispatch
    // `while(true)` loop. The async `.throw()` closure has NO dispatch loop
    // (it runs the handler, then suspends), so that dangling dispatch-continue
    // would be a `continue` with no enclosing loop. The preceding `LocalSet`
    // already moved the state to the loop's resume target (cond/update/after-
    // loop, fixed up by `fix_break_continue_sentinels_in_catches`), so the
    // correct async behavior is to suspend right there: convert the dispatch
    // re-entry into a `return { value: undefined, done: false }`.
    if !fall_through {
        rewrite_dispatch_continue_to_suspend(&mut rewritten);
    }

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

    if !fall_through {
        // Catch completed normally (no `return`): the generator is not done —
        // undo the up-front `done = true` and suspend at the post-catch state.
        body.push(Stmt::Expr(Expr::LocalSet(
            done_id,
            Box::new(Expr::Bool(false)),
        )));
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
pub(crate) fn build_finally_run_stmts(
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

pub(crate) fn finally_route_condition(route: &FinallyRoute, state_id: LocalId) -> Expr {
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

/// `result === null || (typeof result !== "object" && typeof result !== "function")`
/// — `Type(result) is not Object`. Used to enforce the spec requirement that a
/// delegated `return()`/`next()` result be an Object.
fn not_object_condition(result: Expr) -> Expr {
    Expr::Logical {
        op: LogicalOp::Or,
        left: Box::new(Expr::Compare {
            op: CompareOp::Eq,
            left: Box::new(result.clone()),
            right: Box::new(Expr::Null),
        }),
        right: Box::new(Expr::Logical {
            op: LogicalOp::And,
            left: Box::new(Expr::Compare {
                op: CompareOp::Ne,
                left: Box::new(Expr::TypeOf(Box::new(result.clone()))),
                right: Box::new(Expr::String("object".to_string())),
            }),
            right: Box::new(Expr::Compare {
                op: CompareOp::Ne,
                left: Box::new(Expr::TypeOf(Box::new(result))),
                right: Box::new(Expr::String("function".to_string())),
            }),
        }),
    }
}

/// Build the `gen.return(v)` forwarding routes for an async generator's `.return`
/// closure. When the generator is suspended inside a `yield *` delegation
/// (`state` within a recorded [`DelegationRoute`] interval), `return(v)` must
/// forward to the delegated iterator's `return` method rather than completing
/// the outer generator directly — spec `yield *` step 6.c:
///
/// ```text
///   return = GetMethod(iterator, "return")
///   if return is undefined -> return Completion(received)   // complete with v
///   innerResult = ? Await(? Call(return, iterator, «v»))
///   if Type(innerResult) is not Object -> throw a TypeError
///   if IteratorComplete(innerResult) -> return Completion{return, IteratorValue} // complete
///   else -> AsyncGeneratorYield(IteratorValue(innerResult))   // re-yield, stay suspended
/// ```
///
/// Each route emits an `if (state in interval) { ... }`; at most one matches and
/// it always returns, so control only falls through to the generic completion
/// path when not suspended in a `yield *`. The thrown `TypeError` and the
/// returned iter-results are caught / promise-wrapped by
/// `wrap_generator_resume_body`. Empty for sync generators (no routes recorded).
pub(crate) fn build_yield_star_return_routes(
    delegations: &[DelegationRoute],
    state_id: LocalId,
    return_param_id: LocalId,
    done_id: LocalId,
    next_local_id: &mut u32,
) -> Vec<Stmt> {
    let mut out = Vec::with_capacity(delegations.len());
    for route in delegations {
        let m_id = alloc_local(next_local_id); // captured `return` method
        let r_id = alloc_local(next_local_id); // awaited inner result

        let in_interval = Expr::Logical {
            op: LogicalOp::And,
            left: Box::new(Expr::Compare {
                op: CompareOp::Gt,
                left: Box::new(Expr::LocalGet(state_id)),
                right: Box::new(Expr::Number(route.suspend_state_lo as f64)),
            }),
            right: Box::new(Expr::Compare {
                op: CompareOp::Le,
                left: Box::new(Expr::LocalGet(state_id)),
                right: Box::new(Expr::Number(route.suspend_state_hi as f64)),
            }),
        };

        // let __m = iterator.return;
        let read_method = Stmt::Let {
            id: m_id,
            name: "__yield_star_ret_m".to_string(),
            ty: Type::Any,
            mutable: false,
            init: Some(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(route.iter_id)),
                property: "return".to_string(),
            }),
        };
        // if (__m === undefined || __m === null) { done = true; return {v, true}; }
        let no_method = Stmt::If {
            condition: Expr::Logical {
                op: LogicalOp::Or,
                left: Box::new(Expr::Compare {
                    op: CompareOp::Eq,
                    left: Box::new(Expr::LocalGet(m_id)),
                    right: Box::new(Expr::Undefined),
                }),
                right: Box::new(Expr::Compare {
                    op: CompareOp::Eq,
                    left: Box::new(Expr::LocalGet(m_id)),
                    right: Box::new(Expr::Null),
                }),
            },
            then_branch: vec![
                Stmt::Expr(Expr::LocalSet(done_id, Box::new(Expr::Bool(true)))),
                Stmt::Return(Some(make_iter_result(
                    Expr::LocalGet(return_param_id),
                    true,
                ))),
            ],
            else_branch: None,
        };
        // let __r = await __m.call(iterator, v);
        let call_ret = Stmt::Let {
            id: r_id,
            name: "__yield_star_ret_r".to_string(),
            ty: Type::Any,
            mutable: false,
            init: Some(Expr::Await(Box::new(Expr::Call {
                callee: Box::new(Expr::PropertyGet {
                    object: Box::new(Expr::LocalGet(m_id)),
                    property: "call".to_string(),
                }),
                args: vec![
                    Expr::LocalGet(route.iter_id),
                    Expr::LocalGet(return_param_id),
                ],
                type_args: vec![],
                byte_offset: 0,
            }))),
        };
        // if (Type(__r) is not Object) throw new TypeError(...);
        let obj_check = Stmt::If {
            condition: not_object_condition(Expr::LocalGet(r_id)),
            then_branch: vec![Stmt::Throw(Expr::TypeErrorNew(Box::new(Expr::String(
                "Iterator result is not an object".to_string(),
            ))))],
            else_branch: None,
        };
        // if (__r.done) { done = true; return {__r.value, true}; }
        // else            return {__r.value, false};   // re-yield, generator not done
        let dispatch_done = Stmt::If {
            condition: Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(r_id)),
                property: "done".to_string(),
            },
            then_branch: vec![
                Stmt::Expr(Expr::LocalSet(done_id, Box::new(Expr::Bool(true)))),
                Stmt::Return(Some(make_iter_result(
                    Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(r_id)),
                        property: "value".to_string(),
                    },
                    true,
                ))),
            ],
            else_branch: Some(vec![Stmt::Return(Some(make_iter_result(
                Expr::PropertyGet {
                    object: Box::new(Expr::LocalGet(r_id)),
                    property: "value".to_string(),
                },
                false,
            )))]),
        };

        out.push(Stmt::If {
            condition: in_interval,
            then_branch: vec![read_method, no_method, call_ret, obj_check, dispatch_done],
            else_branch: None,
        });
    }
    out
}

/// Build the `gen.throw(e)` forwarding routes for an async generator's `.throw`
/// closure. When the generator is suspended inside a `yield *` delegation
/// (`state` within a recorded [`DelegationRoute`] interval), `throw(e)` must
/// forward to the delegated iterator's `throw` method rather than routing the
/// error into the outer generator's own catch handlers — spec `yield *` step 6.b:
///
/// ```text
///   throw = GetMethod(iterator, "throw")
///   if throw is undefined ->                         // close inner, then TypeError
///       AsyncIteratorClose(iterator, normal); throw a TypeError
///   innerResult = ? Await(? Call(throw, iterator, «e»))
///   if Type(innerResult) is not Object -> throw a TypeError
///   if IteratorComplete(innerResult) -> resume the outer body after the `yield *`
///                                       with IteratorValue(innerResult)
///   else -> AsyncGeneratorYield(IteratorValue(innerResult))   // re-yield, stay suspended
/// ```
///
/// Unlike `return`, the *done* case does NOT complete the outer generator — it
/// resumes execution after the `yield *` (which may yield again or run to
/// completion). Both the done (resume) and not-done (re-yield) cases are handled
/// uniformly: the awaited inner result is stored into the delegation's
/// `result_id`, the state is set to the drive loop's condition state
/// (`resume_state`), and a clone of the state-dispatch loop (`while_body`) is
/// re-driven. The condition state reads `result.done` and either exits the loop
/// (continuing the outer body) or re-yields `result.value`. This continuation
/// loop is the async-generator abrupt-resume machinery the `.return()` path does
/// not need (a `return` always completes or re-yields, never resumes the body).
///
/// An abrupt completion *of the delegation protocol itself* — `iterator.throw`
/// rejecting, a non-object inner result, or the `throw`-undefined TypeError —
/// occurs at the `yield *` site, so an outer `try/catch` around the delegation
/// must be able to handle it (spec `?`/`ReturnIfAbrupt` semantics). The protocol
/// work is therefore wrapped in a generated `try/catch` whose handler routes the
/// caught error into the matching outer catch's linearized states (via
/// [`build_abrupt_routing`], then re-driving the dispatch loop, so a `yield`
/// inside that catch suspends) or, when no catch matches, runs pending
/// non-yielding finallys and re-throws to reject the generator.
///
/// Each route returns or throws from inside its `while(true)` continuation loop,
/// so control falls through to the catch-routing fallback only when not
/// suspended in a delegation. Empty for sync generators (no routes recorded).
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_yield_star_throw_routes(
    delegations: &[DelegationRoute],
    catches: &[CatchRoute],
    finallys: &[FinallyRoute],
    state_id: LocalId,
    throw_param_id: LocalId,
    pending_type_id: LocalId,
    pending_value_id: LocalId,
    while_body: &[Stmt],
    hoisted_ids: &std::collections::HashSet<LocalId>,
    next_local_id: &mut u32,
) -> Vec<Stmt> {
    let mut out = Vec::with_capacity(delegations.len());
    for route in delegations {
        let m_id = alloc_local(next_local_id); // captured `throw` method
        let ret_m_id = alloc_local(next_local_id); // `return` method (close path)
        let ic_id = alloc_local(next_local_id); // awaited inner-close result
        let de_id = alloc_local(next_local_id); // caught delegation-protocol error

        let in_interval = Expr::Logical {
            op: LogicalOp::And,
            left: Box::new(Expr::Compare {
                op: CompareOp::Gt,
                left: Box::new(Expr::LocalGet(state_id)),
                right: Box::new(Expr::Number(route.suspend_state_lo as f64)),
            }),
            right: Box::new(Expr::Compare {
                op: CompareOp::Le,
                left: Box::new(Expr::LocalGet(state_id)),
                right: Box::new(Expr::Number(route.suspend_state_hi as f64)),
            }),
        };

        // let __m = iterator.throw;
        let read_method = Stmt::Let {
            id: m_id,
            name: "__yield_star_throw_m".to_string(),
            ty: Type::Any,
            mutable: false,
            init: Some(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(route.iter_id)),
                property: "throw".to_string(),
            }),
        };

        // Spec step 6.b.iii: `throw` undefined ⇒ AsyncIteratorClose the inner
        // iterator (give it a chance to run `return`) and then throw a TypeError.
        //   let __ret = iterator.return;
        //   if (__ret !== undefined && __ret !== null) {
        //       let __ic = await __ret.call(iterator);
        //       if (Type(__ic) is not Object) throw new TypeError(...);
        //   }
        //   throw new TypeError("The iterator does not provide a 'throw' method");
        let read_return = Stmt::Let {
            id: ret_m_id,
            name: "__yield_star_close_m".to_string(),
            ty: Type::Any,
            mutable: false,
            init: Some(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(route.iter_id)),
                property: "return".to_string(),
            }),
        };
        let close_inner = Stmt::If {
            condition: Expr::Logical {
                op: LogicalOp::And,
                left: Box::new(Expr::Compare {
                    op: CompareOp::Ne,
                    left: Box::new(Expr::LocalGet(ret_m_id)),
                    right: Box::new(Expr::Undefined),
                }),
                right: Box::new(Expr::Compare {
                    op: CompareOp::Ne,
                    left: Box::new(Expr::LocalGet(ret_m_id)),
                    right: Box::new(Expr::Null),
                }),
            },
            then_branch: vec![
                Stmt::Let {
                    id: ic_id,
                    name: "__yield_star_close_r".to_string(),
                    ty: Type::Any,
                    mutable: false,
                    init: Some(Expr::Await(Box::new(Expr::Call {
                        callee: Box::new(Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(ret_m_id)),
                            property: "call".to_string(),
                        }),
                        args: vec![Expr::LocalGet(route.iter_id)],
                        type_args: vec![],
                        byte_offset: 0,
                    }))),
                },
                Stmt::If {
                    condition: not_object_condition(Expr::LocalGet(ic_id)),
                    then_branch: vec![Stmt::Throw(Expr::TypeErrorNew(Box::new(Expr::String(
                        "Iterator result is not an object".to_string(),
                    ))))],
                    else_branch: None,
                },
            ],
            else_branch: None,
        };
        let no_method = Stmt::If {
            condition: Expr::Logical {
                op: LogicalOp::Or,
                left: Box::new(Expr::Compare {
                    op: CompareOp::Eq,
                    left: Box::new(Expr::LocalGet(m_id)),
                    right: Box::new(Expr::Undefined),
                }),
                right: Box::new(Expr::Compare {
                    op: CompareOp::Eq,
                    left: Box::new(Expr::LocalGet(m_id)),
                    right: Box::new(Expr::Null),
                }),
            },
            then_branch: vec![
                read_return,
                close_inner,
                Stmt::Throw(Expr::TypeErrorNew(Box::new(Expr::String(
                    "The iterator does not provide a 'throw' method".to_string(),
                )))),
            ],
            else_branch: None,
        };

        // __del_result = await __m.call(iterator, e);
        let call_throw = Stmt::Expr(Expr::LocalSet(
            route.result_id,
            Box::new(Expr::Await(Box::new(Expr::Call {
                callee: Box::new(Expr::PropertyGet {
                    object: Box::new(Expr::LocalGet(m_id)),
                    property: "call".to_string(),
                }),
                args: vec![
                    Expr::LocalGet(route.iter_id),
                    Expr::LocalGet(throw_param_id),
                ],
                type_args: vec![],
                byte_offset: 0,
            }))),
        ));

        // if (Type(__del_result) is not Object) throw new TypeError(...);
        let obj_check = Stmt::If {
            condition: not_object_condition(Expr::LocalGet(route.result_id)),
            then_branch: vec![Stmt::Throw(Expr::TypeErrorNew(Box::new(Expr::String(
                "Iterator result is not an object".to_string(),
            ))))],
            else_branch: None,
        };

        // On success, re-drive the dispatch loop from the drive loop's condition
        // state: it reads `__del_result.done` and either exits the loop (resuming
        // the outer body past the `yield *`) or re-yields `__del_result.value`.
        let set_state = Stmt::Expr(Expr::LocalSet(
            state_id,
            Box::new(Expr::Number(route.resume_state as f64)),
        ));

        // Wrap the protocol work so an abrupt completion at the `yield *` site
        // (inner `throw` rejection / non-object result / `throw`-undefined
        // TypeError) routes into the enclosing `try`'s catch states instead of
        // escaping straight to the generator-level rejection. `state` is still
        // the delegation suspend state here (set_state runs last, only on the
        // success path), so it falls inside the outer try's protected interval.
        // When no catch matches, run pending non-yielding finallys and re-throw
        // — identical to `build_async_throw_body`'s own async fallback. Routing
        // an abrupt completion *into* a YIELDING finally needs the
        // pending-completion re-raise machinery that is gated `!is_async_generator`
        // (an async yielding finally never re-raises the pending throw, so routing
        // there would swallow the error). That async-wide limitation is out of
        // scope here; matching the existing async fallback keeps the error
        // propagating rather than being silently dropped.
        let mut fallback = build_finally_run_stmts(finallys, state_id, hoisted_ids);
        fallback.push(Stmt::Throw(Expr::LocalGet(de_id)));
        let route_to_outer_catch = build_abrupt_routing(
            catches,
            &[], // async can't re-raise from a yielding finally; finallys handled in fallback
            state_id,
            pending_type_id,
            pending_value_id,
            &Expr::LocalGet(de_id),
            true,
            1.0,
            false,
            false,
            fallback,
        );
        let protocol = Stmt::Try {
            body: vec![read_method, no_method, call_throw, obj_check, set_state],
            catch: Some(CatchClause {
                param: Some((de_id, "__yield_star_throw_e".to_string())),
                body: route_to_outer_catch,
            }),
            finally: None,
        };
        // Both the success path (state = resume_state) and a routed catch (state
        // = catch_entry_state) fall through to this loop, which dispatches from
        // the freshly-set state.
        let drive = Stmt::While {
            condition: Expr::Bool(true),
            body: while_body.to_vec(),
        };

        out.push(Stmt::If {
            condition: in_interval,
            then_branch: vec![protocol, drive],
            else_branch: None,
        });
    }
    out
}
