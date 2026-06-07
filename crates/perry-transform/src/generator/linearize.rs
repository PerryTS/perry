//! Body linearization: split generator body into state-machine states keyed by yield points.

use super::*;

thread_local! {
    /// Whether the generator currently being linearized is an `async function*`.
    /// Set by `transform_generator_function_with_extra_captures` right before it
    /// calls `linearize_body` (linearization is fully synchronous, so a
    /// thread-local avoids threading a bool through ~14 recursive call sites).
    /// Read by the `yield*` arms to pick the async vs sync delegation protocol.
    static LINEARIZE_IS_ASYNC_GEN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub(crate) fn set_linearize_async_generator(v: bool) {
    LINEARIZE_IS_ASYNC_GEN.with(|c| c.set(v));
}

fn linearize_async_generator() -> bool {
    LINEARIZE_IS_ASYNC_GEN.with(|c| c.get())
}

/// Resolve a `yield*` operand into its iterator. An async generator delegates
/// through the async-iterator protocol (`GetAsyncIterator`, which honours
/// `[Symbol.asyncIterator]` and wraps a sync iterable via
/// CreateAsyncFromSyncIterator); a sync generator uses `GetIterator`.
fn delegate_get_iterator(inner: Expr) -> Expr {
    if linearize_async_generator() {
        Expr::GetAsyncIterator(Box::new(inner))
    } else {
        Expr::GetIterator(Box::new(inner))
    }
}

/// Wrap a delegated-iterator `next()`/`return()`/`throw()` call. In an async
/// generator the delegated iterator's methods return a promise of the
/// iter-result, which must be awaited before `.value`/`.done` are read (`await`
/// is a synchronous promise-drain in codegen). Sync generators read the
/// iter-result directly.
fn delegate_await(call: Expr) -> Expr {
    if linearize_async_generator() {
        Expr::Await(Box::new(call))
    } else {
        call
    }
}

pub struct State {
    pub num: u32,
    pub body: Vec<Stmt>,
    pub exit: StateExit,
}

pub enum StateExit {
    /// Yield a value and advance to next_state
    Yield { value: Expr, next_state: u32 },
    /// Goto another state (non-yielding transition)
    Goto(u32),
    /// Function is done
    Done,
}

#[derive(Clone)]
pub struct CatchRoute {
    pub param_id: Option<LocalId>,
    pub param_name: Option<String>,
    pub body: Vec<Stmt>,
    pub protected_start_state: u32,
    pub post_catch_state: u32,
    /// #4438: upper bound of the protected suspended-state interval for routing
    /// a thrown error into this catch. Covers the try-body states (and the
    /// post-last-yield happy landing state) but EXCLUDES the catch's own states,
    /// so a `throw` executing inside the catch propagates to an enclosing
    /// handler instead of re-entering this one. For sync generators the catch
    /// body is linearized into the state machine (`catch_entry_state`); async
    /// generators still inline `body` into the `.throw()` closure and ignore
    /// these two fields.
    pub protected_end_state: u32,
    /// #4438: first state of the linearized catch body. `Some` for sync
    /// generators (runtime throws + `.throw()` route here); `None` when the
    /// catch was not linearized into states.
    pub catch_entry_state: Option<u32>,
}

/// A `finally` block that protects a state interval. On abrupt completion
/// (`.return()`/`.throw()`) while the generator is suspended inside the
/// protected interval, the finally body must run before the generator
/// completes — innermost finally first (#4374).
///
/// `protected_start_state`/`post_finally_state` use the same suspended-state
/// convention as [`CatchRoute`]: a finally applies when
/// `state > protected_start_state && state <= post_finally_state`.
#[derive(Clone)]
pub struct FinallyRoute {
    pub body: Vec<Stmt>,
    pub protected_start_state: u32,
    pub post_finally_state: u32,
    /// `true` if the finally body itself contains yields/awaits (await-using
    /// path). Such finallys are linearized into states and can't be inlined
    /// into the `.return()`/`.throw()` closures synchronously.
    pub has_yields: bool,
    /// #4438 B2-finally: for a YIELDING finally, the first state of its
    /// linearized body. Abrupt completion (`.throw()`/`.return()`/runtime
    /// throw) while suspended in the protected try interval routes here so the
    /// finally's own `yield`s suspend; on completion the pending throw/return
    /// is re-raised. `None` for non-yielding finallys (inlined as before).
    pub finally_entry_state: Option<u32>,
    /// #4438 B2-finally: upper bound of the protected suspended-state interval
    /// for routing into a yielding finally. Covers the try body but EXCLUDES
    /// the finally's own states, so an abrupt completion while suspended INSIDE
    /// the finally supersedes the pending one instead of re-entering it.
    pub protected_end_state: u32,
    /// #4438 B2-finally: the state whose body, after the finally completes,
    /// re-raises a pending throw/return completion (the completion check is
    /// appended in `transform_generator_function`).
    pub completion_check_state: Option<u32>,
}

/// Linearize the generator body into a sequence of states.
/// Splits at yield points and handles for-loops with yields.
pub fn linearize_body(
    stmts: &[Stmt],
    states: &mut Vec<State>,
    current: &mut Vec<Stmt>,
    state_num: &mut u32,
    state_id: LocalId,
    #[allow(unused_variables)] next_local_id: &mut u32,
    sent_id: LocalId,
    catches: &mut Vec<CatchRoute>,
    finallys: &mut Vec<FinallyRoute>,
) {
    for stmt in stmts {
        match stmt {
            // yield* delegation: iterate the inner iterator and yield each value
            Stmt::Expr(Expr::Yield {
                value: Some(inner),
                delegate: true,
            }) => {
                // Desugar yield* into:
                //   let __del_iter = inner_expr;  (inner is a generator call)
                //   let __del_result = __del_iter.next();
                //   while (!__del_result.done) {
                //     yield __del_result.value;
                //     __del_result = __del_iter.next();
                //   }
                // We don't actually need real vars — we can inline this as states.
                // But the simplest approach: expand into statements and re-linearize.
                let del_iter_id = alloc_local(next_local_id);
                let del_result_id = alloc_local(next_local_id);

                // Initial pull: `__del_iter.next()` with no argument (the value
                // passed to the *first* `next()` of a generator is discarded per
                // spec).
                let next_call = Expr::Call {
                    callee: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(del_iter_id)),
                        property: "next".to_string(),
                    }),
                    args: vec![],
                    type_args: vec![],
                };
                // #1832: in-loop pull must forward the value the *outer* generator
                // was resumed with (`outer.next(v)` → stored in `sent_id`) into the
                // delegated iterator's `next(v)`, so `yield*`-delegated two-way
                // communication matches spec. Argless here silently dropped it.
                let next_call_resumed = Expr::Call {
                    callee: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(del_iter_id)),
                        property: "next".to_string(),
                    }),
                    args: vec![Expr::LocalGet(sent_id)],
                    type_args: vec![],
                };

                // Add hoisted var declarations to current (they'll be emitted in the state body)
                // #1831: resolve the iterator. For a generator *call* the result
                // already is its iterator; for an arbitrary iterable (effect,
                // custom `[Symbol.iterator]`) `js_get_iterator` invokes the
                // well-known-symbol method to obtain one.
                current.push(Stmt::Expr(Expr::LocalSet(
                    del_iter_id,
                    Box::new(delegate_get_iterator(*inner.clone())),
                )));
                current.push(Stmt::Expr(Expr::LocalSet(
                    del_result_id,
                    Box::new(delegate_await(next_call)),
                )));

                // Build the while loop with yield
                let while_body = vec![
                    Stmt::Expr(Expr::Yield {
                        value: Some(Box::new(Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(del_result_id)),
                            property: "value".to_string(),
                        })),
                        delegate: false,
                    }),
                    Stmt::Expr(Expr::LocalSet(
                        del_result_id,
                        Box::new(delegate_await(next_call_resumed)),
                    )),
                ];

                let while_stmt = Stmt::While {
                    condition: Expr::Unary {
                        op: UnaryOp::Not,
                        operand: Box::new(Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(del_result_id)),
                            property: "done".to_string(),
                        }),
                    },
                    body: while_body,
                };

                // Now linearize the expanded while (it contains a yield, so the while handler picks it up)
                linearize_body(
                    &[while_stmt],
                    states,
                    current,
                    state_num,
                    state_id,
                    next_local_id,
                    sent_id,
                    catches,
                    finallys,
                );
            }

            // yield expr at statement level (non-delegate)
            Stmt::Expr(Expr::Yield {
                value,
                delegate: false,
            })
            | Stmt::Expr(Expr::Yield { value, .. }) => {
                let yield_val = value
                    .as_ref()
                    .map(|v| *v.clone())
                    .unwrap_or(Expr::Undefined);
                let this_state = *state_num;
                *state_num += 1;
                states.push(State {
                    num: this_state,
                    body: std::mem::take(current),
                    exit: StateExit::Yield {
                        value: yield_val,
                        next_state: *state_num,
                    },
                });
            }

            // #34: `return yield* inner` — delegation in return position.
            // The earlier catch-all arm (`Return(Some(Yield { .. }))`) ignored
            // the `delegate` flag and yielded `inner` itself as a single
            // (non-delegated) value, so the outer generator handed the raw
            // delegated object straight to its consumer. For `yield* effect`
            // that consumer is effect's `yieldWrapGet`, which expects a
            // `YieldWrap` (produced by the effect's `[Symbol.iterator]`) and
            // threw "BUG: yieldWrapGet" on the bare effect. Desugar identically
            // to the `Let`-initializer delegation arm (drive the inner
            // iterator, re-yielding each value through the iterator protocol),
            // then `return { value: <final>, done: true }` so the iterator's
            // completion value becomes the generator's return value.
            Stmt::Return(Some(Expr::Yield {
                value: Some(inner),
                delegate: true,
            })) => {
                let del_iter_id = alloc_local(next_local_id);
                let del_result_id = alloc_local(next_local_id);

                // Initial pull: argless (the first `next()` value is discarded
                // per spec).
                let next_call = Expr::Call {
                    callee: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(del_iter_id)),
                        property: "next".to_string(),
                    }),
                    args: vec![],
                    type_args: vec![],
                };
                // #1832: in-loop pull forwards the outer resume value (`sent_id`).
                let next_call_resumed = Expr::Call {
                    callee: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(del_iter_id)),
                        property: "next".to_string(),
                    }),
                    args: vec![Expr::LocalGet(sent_id)],
                    type_args: vec![],
                };

                // #1831: resolve the iterator (effect / custom `[Symbol.iterator]`
                // operands need `js_get_iterator` to invoke the well-known-symbol
                // method; a generator call already is its iterator). Async
                // generators delegate through the async-iterator protocol.
                current.push(Stmt::Expr(Expr::LocalSet(
                    del_iter_id,
                    Box::new(delegate_get_iterator(*inner.clone())),
                )));
                current.push(Stmt::Expr(Expr::LocalSet(
                    del_result_id,
                    Box::new(delegate_await(next_call)),
                )));

                let while_body = vec![
                    Stmt::Expr(Expr::Yield {
                        value: Some(Box::new(Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(del_result_id)),
                            property: "value".to_string(),
                        })),
                        delegate: false,
                    }),
                    Stmt::Expr(Expr::LocalSet(
                        del_result_id,
                        Box::new(delegate_await(next_call_resumed)),
                    )),
                ];

                let while_stmt = Stmt::While {
                    condition: Expr::Unary {
                        op: UnaryOp::Not,
                        operand: Box::new(Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(del_result_id)),
                            property: "done".to_string(),
                        }),
                    },
                    body: while_body,
                };

                linearize_body(
                    &[while_stmt],
                    states,
                    current,
                    state_num,
                    state_id,
                    next_local_id,
                    sent_id,
                    catches,
                    finallys,
                );

                // After the loop, the iterator's final `value` (from
                // {value, done:true}) is the value of `yield* inner`, which is
                // exactly what `return yield* inner` returns. Wrap it as the
                // generator's terminal {value, done:true} and flush a Done state.
                current.push(Stmt::Return(Some(make_iter_result(
                    Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(del_result_id)),
                        property: "value".to_string(),
                    },
                    true,
                ))));
                let cont_state = *state_num;
                *state_num += 1;
                states.push(State {
                    num: cont_state,
                    body: std::mem::take(current),
                    exit: StateExit::Done,
                });
            }

            // return (yield expr)  — i.e. `return await x` after async→generator rewrite
            // The yield must be emitted as a real yield state so the async-step driver can
            // await the expression; the continuation state then returns {value: __sent, done: true}
            // where __sent is the resolved value delivered back by the step driver.
            Stmt::Return(Some(yield_expr @ Expr::Yield { .. })) => {
                let yield_val = if let Expr::Yield { value, .. } = yield_expr {
                    value
                        .as_ref()
                        .map(|v| *v.clone())
                        .unwrap_or(Expr::Undefined)
                } else {
                    unreachable!()
                };
                // Flush pre-return code as a yield state
                let this_state = *state_num;
                *state_num += 1;
                states.push(State {
                    num: this_state,
                    body: std::mem::take(current),
                    exit: StateExit::Yield {
                        value: yield_val,
                        next_state: *state_num,
                    },
                });
                // Continuation state: return { value: __sent, done: true }
                current.push(Stmt::Return(Some(make_iter_result(
                    Expr::LocalGet(sent_id),
                    true,
                ))));
                let cont_state = *state_num;
                *state_num += 1;
                states.push(State {
                    num: cont_state,
                    body: std::mem::take(current),
                    exit: StateExit::Done,
                });
            }

            // return expr (terminal - ends the generator)
            Stmt::Return(val) => {
                // Add the return with {value: expr, done: true} wrapping
                let return_val = val.clone().unwrap_or(Expr::Undefined);
                current.push(Stmt::Return(Some(make_iter_result(return_val, true))));
                // Flush current as a terminal state
                let this_state = *state_num;
                *state_num += 1;
                states.push(State {
                    num: this_state,
                    body: std::mem::take(current),
                    exit: StateExit::Done,
                });
            }

            // For-loop containing yield(s)
            Stmt::For {
                init,
                condition,
                update,
                body,
            } if body_contains_yield(body) => {
                // State N: pre-loop code + init, goto condition check
                let init_state = *state_num;
                *state_num += 1;
                let mut init_body = std::mem::take(current);
                // Add init statement (typically `let i = start`)
                // But we need to convert it to an assignment since the var is hoisted
                if let Some(init_stmt) = init {
                    match init_stmt.as_ref() {
                        Stmt::Let {
                            id,
                            init: Some(init_expr),
                            ..
                        } => {
                            init_body
                                .push(Stmt::Expr(Expr::LocalSet(*id, Box::new(init_expr.clone()))));
                        }
                        other => init_body.push(other.clone()),
                    }
                }
                let cond_state = *state_num;
                states.push(State {
                    num: init_state,
                    body: init_body,
                    exit: StateExit::Goto(cond_state),
                });

                // State N+1: condition check
                *state_num += 1;
                let body_state = *state_num;
                // Condition check: if true, fall through to body; if false, done
                let cond_body = if let Some(cond) = condition {
                    // Build the done return as part of the else branch
                    vec![Stmt::If {
                        condition: Expr::Unary {
                            op: UnaryOp::Not,
                            operand: Box::new(cond.clone()),
                        },
                        then_branch: vec![
                            // Loop ended - jump past the loop
                            Stmt::Expr(Expr::LocalSet(
                                state_id,
                                Box::new(Expr::Number(0.0)), // placeholder, fixed below
                            )),
                            // Continue the while(true) so the Goto exit doesn't overwrite state
                            Stmt::Continue,
                        ],
                        else_branch: None,
                    }]
                } else {
                    vec![]
                };
                // We'll fix the after-loop state number after processing body
                states.push(State {
                    num: cond_state,
                    body: cond_body,
                    exit: StateExit::Goto(body_state),
                });

                // Pre-rewrite the body so any top-level `break` / `continue`
                // inside (but NOT inside nested loops / switch / closure)
                // becomes a placeholder state assignment + dispatch-continue.
                // After body processing we know what the loop's break and
                // continue targets are; the fix-up pass below replaces the
                // sentinel numbers. Without this, `for (let i ...) { await
                // x; if (cond) break; }` lowers the inner `break` as a raw
                // `Stmt::Break` — the state-machine-emitted while(true) loop
                // exits early, then the post-dispatch code reads the scratch
                // iter-result set by the await (done=false), returns
                // `AsyncStepChain(stale_promise, step)`, and the chain loops
                // forever on a stale promise. Same shape covers `continue`
                // skipping the body tail without going through the update.
                let body_states_before = states.len();
                let body_current_before = current.len();
                let mut body_rewritten = body.clone();
                rewrite_break_continue_in_stmts(&mut body_rewritten, state_id);

                // Process loop body (may contain yields)
                linearize_body(
                    &body_rewritten,
                    states,
                    current,
                    state_num,
                    state_id,
                    next_local_id,
                    sent_id,
                    catches,
                    finallys,
                );

                // Body-tail state: contains the user body's residual stmts
                // (everything after the last yield in the body). On fall-
                // through it transitions to `update_state` so the for-loop
                // semantics (run body, run update, re-check cond) hold.
                let tail_state = *state_num;
                *state_num += 1;
                let tail_body = std::mem::take(current);

                // `continue` target: a dedicated state that ONLY runs the
                // update expression and then jumps back to cond. Distinct
                // from `tail_state` so a user-`continue` from inside the
                // body skips the post-continue body residual but still runs
                // the for-loop's update expression. Without this split, a
                // user `continue` written inside the post-yield body region
                // would land back in the tail state, re-execute the body
                // residual, and (depending on guard placement) loop forever
                // on the same iteration.
                let update_state = *state_num;
                *state_num += 1;
                let mut update_body: Vec<Stmt> = Vec::new();
                if let Some(upd) = update {
                    update_body.push(Stmt::Expr(upd.clone()));
                }

                // Push tail_state pointing at update_state.
                states.push(State {
                    num: tail_state,
                    body: tail_body,
                    exit: StateExit::Goto(update_state),
                });
                // Push update_state pointing at cond_state.
                states.push(State {
                    num: update_state,
                    body: update_body,
                    exit: StateExit::Goto(cond_state),
                });

                // Fix up the condition state's false branch to jump to after-loop state
                let after_loop_state = *state_num;
                // Find the condition state and fix the placeholder
                for state in states.iter_mut() {
                    if state.num == cond_state {
                        fix_placeholder_state(&mut state.body, state_id, after_loop_state);
                    }
                }
                // Fix break / continue placeholders that landed in the
                // newly-created states (from body and tail_state) or in
                // the trailing `current` buffer (none for For — tail_state
                // already drained it, but covered for symmetry).
                fix_break_continue_sentinels(
                    &mut states[body_states_before..],
                    state_id,
                    after_loop_state,
                    update_state,
                );
                fix_break_continue_sentinels_in_stmts(
                    &mut current[body_current_before..],
                    state_id,
                    after_loop_state,
                    update_state,
                );
            }

            // While-loop containing yield(s) - similar to for-loop
            Stmt::While {
                condition,
                body: while_body,
            } if body_contains_yield(while_body) => {
                // Pre-loop code gets its own state (if non-empty)
                let pre_body = std::mem::take(current);
                if !pre_body.is_empty() {
                    let pre_state = *state_num;
                    *state_num += 1;
                    let cond_target = *state_num; // will be the cond_state below
                    states.push(State {
                        num: pre_state,
                        body: pre_body,
                        exit: StateExit::Goto(cond_target),
                    });
                }

                let cond_state = *state_num;
                *state_num += 1;

                let body_state = *state_num;
                // Condition check
                states.push(State {
                    num: cond_state,
                    body: vec![Stmt::If {
                        condition: Expr::Unary {
                            op: UnaryOp::Not,
                            operand: Box::new(condition.clone()),
                        },
                        then_branch: vec![
                            Stmt::Expr(Expr::LocalSet(
                                state_id,
                                Box::new(Expr::Number(0.0)), // placeholder
                            )),
                            Stmt::Continue,
                        ],
                        else_branch: None,
                    }],
                    exit: StateExit::Goto(body_state),
                });

                // Pre-rewrite while body's break/continue sentinels.
                // For a while-loop, `continue` jumps back to the condition
                // state (no separate update); `break` jumps to after_loop.
                let while_states_before = states.len();
                let while_current_before = current.len();
                let mut while_body_rewritten = while_body.clone();
                rewrite_break_continue_in_stmts(&mut while_body_rewritten, state_id);

                // Process body
                linearize_body(
                    &while_body_rewritten,
                    states,
                    current,
                    state_num,
                    state_id,
                    next_local_id,
                    sent_id,
                    catches,
                    finallys,
                );

                // After body, goto condition
                let loop_back_state = *state_num;
                *state_num += 1;
                states.push(State {
                    num: loop_back_state,
                    body: std::mem::take(current),
                    exit: StateExit::Goto(cond_state),
                });

                // Fix placeholder
                let after_loop = *state_num;
                for state in states.iter_mut() {
                    if state.num == cond_state {
                        fix_placeholder_state(&mut state.body, state_id, after_loop);
                    }
                }
                // Fix break / continue sentinels inside the while-body states
                // (`continue` here jumps to the cond_state; `break` jumps to
                // after_loop).
                fix_break_continue_sentinels(
                    &mut states[while_states_before..],
                    state_id,
                    after_loop,
                    cond_state,
                );
                fix_break_continue_sentinels_in_stmts(
                    &mut current[while_current_before..],
                    state_id,
                    after_loop,
                    cond_state,
                );
            }

            // Try-catch/finally containing yield(s) — linearize the try body and
            // the catch body into states (#4438) so a `throw` during dispatch is
            // routed to the catch and a `yield` inside the catch suspends.
            //
            // #4438: the guard must also fire when the yield lives ONLY in the
            // catch body (e.g. `try { throw } catch (e) { yield }`). Pre-fix that
            // fell into the catch-all, which emitted the whole `Stmt::Try`
            // literally and the catch's `yield` hit the codegen
            // `Expr::Yield => double_literal(0.0)` arm and was swallowed.
            Stmt::Try {
                body,
                catch,
                finally,
            } if body_contains_yield(body)
                || finally.as_ref().is_some_and(|f| body_contains_yield(f))
                || catch.as_ref().is_some_and(|c| body_contains_yield(&c.body)) =>
            {
                // #4438: flush any pending pre-try code (e.g. a `throw` or
                // assignment sitting between a preceding `yield` and this `try`)
                // as its own state, so the try's protected interval starts
                // cleanly at the try body. Without this the pre-try code lands in
                // the try's first state and a throw there is wrongly routed to
                // THIS try's handler instead of an enclosing one.
                if !current.is_empty() {
                    let pre_state = *state_num;
                    *state_num += 1;
                    states.push(State {
                        num: pre_state,
                        body: std::mem::take(current),
                        exit: StateExit::Goto(*state_num),
                    });
                }
                let protected_start_state = *state_num;

                // Issue #256: widen the guard to also fire when yields live ONLY
                // in the finally block. `await using` desugars to
                // `try { body } finally { await dispose() }` — the body may have
                // no awaits while the finally has one, and pre-fix this fell into
                // the catch-all which compiled the whole try/finally as a single
                // unit inside one state — the yield-in-finally then hit the
                // codegen `Expr::Yield => double_literal(0.0)` arm and the await
                // was silently fire-and-forgotten.
                if body_contains_yield(body) {
                    // Linearize the try body directly (yields become normal states)
                    linearize_body(
                        body,
                        states,
                        current,
                        state_num,
                        state_id,
                        next_local_id,
                        sent_id,
                        catches,
                        finallys,
                    );
                } else {
                    // Body has no yields: push as-is to current state.
                    for s in body {
                        current.push(s.clone());
                    }
                }

                // Issue #621 / #4438: if the try has a catch handler, split the
                // post-await happy-path continuation (currently in `current`)
                // from the catch and post-try-catch continuations, and linearize
                // the catch body into its own states.
                //
                // Pre-#4438 the catch body was stashed and only inlined into the
                // `.throw()` closure, so the catch handler did not exist in the
                // normal `.next()` dispatch at all. Two consequences:
                //   (a) a runtime `throw` executing inside the try during a plain
                //       `.next()` was never caught — it propagated out of next();
                //   (b) a `yield` inside the catch was swallowed (it was rewritten
                //       to an `await` in the throw closure).
                // Now the catch body is real states. The happy path skips them;
                // runtime throws (via the dispatch-loop try/catch in lower.rs) and
                // `.throw()` route into `catch_entry_state` for sync generators.
                let post_catch_state;
                if let Some(catch_clause) = catch {
                    // Flush the happy-path tail (post-last-yield-in-try code) as
                    // its own landing state. The last yield inside the try resumes
                    // here on a normal `.next()`; it must skip the catch states.
                    let happy_state = *state_num;
                    *state_num += 1;
                    let happy_idx = states.len();
                    states.push(State {
                        num: happy_state,
                        body: std::mem::take(current),
                        exit: StateExit::Goto(0), // patched to post_catch below
                    });
                    // Throws while suspended in the try body (states
                    // protected_start..=happy_state) route to the catch. Catch
                    // states (> happy_state) are EXCLUDED so a `throw` inside the
                    // catch escapes to an enclosing handler, not back into here.
                    let protected_end_state = happy_state;

                    // Linearize the catch body into states.
                    let catch_entry_state = *state_num;
                    let mut catch_current = Vec::new();
                    if body_contains_yield(&catch_clause.body) {
                        linearize_body(
                            &catch_clause.body,
                            states,
                            &mut catch_current,
                            state_num,
                            state_id,
                            next_local_id,
                            sent_id,
                            catches,
                            finallys,
                        );
                    } else {
                        for s in &catch_clause.body {
                            catch_current.push(s.clone());
                        }
                    }
                    // The catch tail falls through to the code after try/catch.
                    let catch_tail_state = *state_num;
                    *state_num += 1;
                    let catch_tail_idx = states.len();
                    states.push(State {
                        num: catch_tail_state,
                        body: std::mem::take(&mut catch_current),
                        exit: StateExit::Goto(0), // patched below
                    });

                    post_catch_state = *state_num;
                    states[happy_idx].exit = StateExit::Goto(post_catch_state);
                    states[catch_tail_idx].exit = StateExit::Goto(post_catch_state);

                    let (param_id, param_name) = catch_clause
                        .param
                        .as_ref()
                        .map(|(id, name)| (Some(*id), Some(name.clone())))
                        .unwrap_or((None, None));
                    catches.push(CatchRoute {
                        param_id,
                        param_name,
                        body: catch_clause.body.clone(),
                        protected_start_state,
                        post_catch_state,
                        protected_end_state,
                        catch_entry_state: Some(catch_entry_state),
                    });
                } else {
                    post_catch_state = *state_num;
                }

                // Finally block.
                if let Some(fin) = finally {
                    let fin_has_yields = body_contains_yield(fin);
                    if fin_has_yields {
                        // #4438 B2-finally: a YIELDING finally is linearized into
                        // states with a clean entry so abrupt completion can route
                        // INTO it (its `yield`s suspend) and re-raise the pending
                        // throw/return once it finishes.
                        //
                        // Flush the happy-path tail currently in `current` (the
                        // post-last-yield try code, when there's no catch) as its
                        // own state so the finally starts fresh — the abrupt path
                        // must not re-run the try tail.
                        let tail_state = *state_num;
                        *state_num += 1;
                        let tail_idx = states.len();
                        states.push(State {
                            num: tail_state,
                            body: std::mem::take(current),
                            exit: StateExit::Goto(0), // patched to finally_entry below
                        });
                        let finally_entry_state = *state_num;
                        states[tail_idx].exit = StateExit::Goto(finally_entry_state);
                        // Throws/returns while suspended in the try body (states
                        // protected_start..=tail_state) route into the finally.
                        // The finally's own states (> tail_state) are excluded.
                        let finally_protected_end = tail_state;

                        // Linearize the finally body into states.
                        linearize_body(
                            fin,
                            states,
                            current,
                            state_num,
                            state_id,
                            next_local_id,
                            sent_id,
                            catches,
                            finallys,
                        );

                        // Flush the finally tail as the completion-check state.
                        // `transform_generator_function` appends the re-raise of a
                        // pending throw/return to this state's body; on the normal
                        // path (no pending) it just falls through to post-finally.
                        let completion_state = *state_num;
                        *state_num += 1;
                        let comp_idx = states.len();
                        states.push(State {
                            num: completion_state,
                            body: std::mem::take(current),
                            exit: StateExit::Goto(0), // patched to post_finally below
                        });
                        let post_finally_state = *state_num;
                        states[comp_idx].exit = StateExit::Goto(post_finally_state);

                        finallys.push(FinallyRoute {
                            body: fin.clone(),
                            protected_start_state,
                            post_finally_state,
                            has_yields: true,
                            finally_entry_state: Some(finally_entry_state),
                            protected_end_state: finally_protected_end,
                            completion_check_state: Some(completion_state),
                        });
                    } else {
                        // #4374: a non-yielding finally is inlined into the
                        // .return()/.throw()/dispatch closures on abrupt
                        // completion (and pushed as-is for the happy path).
                        finallys.push(FinallyRoute {
                            body: fin.clone(),
                            protected_start_state,
                            post_finally_state: post_catch_state,
                            has_yields: false,
                            finally_entry_state: None,
                            protected_end_state: post_catch_state,
                            completion_check_state: None,
                        });
                        for s in fin {
                            current.push(s.clone());
                        }
                    }
                }
            }

            // If-statement containing yield(s) — linearize both branches
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } if body_contains_yield(then_branch)
                || else_branch.as_ref().is_some_and(|e| body_contains_yield(e)) =>
            {
                // Flush pre-if code as its own state
                let pre_state = *state_num;
                *state_num += 1;
                let pre_body = std::mem::take(current);

                let then_state = *state_num;
                // We'll figure out else_state and after_state as we go
                // For now, emit the condition check with a branch
                let else_state_placeholder = 0u32; // fixed below

                states.push(State {
                    num: pre_state,
                    body: {
                        let mut b = pre_body;
                        b.push(Stmt::If {
                            condition: condition.clone(),
                            then_branch: vec![
                                Stmt::Expr(Expr::LocalSet(
                                    state_id,
                                    Box::new(Expr::Number(then_state as f64)),
                                )),
                                Stmt::Continue,
                            ],
                            else_branch: Some(vec![
                                Stmt::Expr(Expr::LocalSet(
                                    state_id,
                                    Box::new(Expr::Number(else_state_placeholder as f64)),
                                )),
                                Stmt::Continue,
                            ]),
                        });
                        b
                    },
                    exit: StateExit::Done, // won't be reached (branches above jump)
                });

                // Linearize then-branch
                linearize_body(
                    then_branch,
                    states,
                    current,
                    state_num,
                    state_id,
                    next_local_id,
                    sent_id,
                    catches,
                    finallys,
                );
                // After then-branch, flush into a goto-after state
                let then_end_state = *state_num;
                *state_num += 1;
                states.push(State {
                    num: then_end_state,
                    body: std::mem::take(current),
                    exit: StateExit::Goto(0), // placeholder for after_state
                });

                // Linearize else-branch
                let else_state = *state_num;
                if let Some(else_stmts) = else_branch {
                    linearize_body(
                        else_stmts,
                        states,
                        current,
                        state_num,
                        state_id,
                        next_local_id,
                        sent_id,
                        catches,
                        finallys,
                    );
                }
                let else_end_state = *state_num;
                *state_num += 1;
                states.push(State {
                    num: else_end_state,
                    body: std::mem::take(current),
                    exit: StateExit::Goto(0), // placeholder for after_state
                });

                let after_state = *state_num;

                // Fix else_state_placeholder in pre_state
                for state in states.iter_mut() {
                    if state.num == pre_state {
                        fix_placeholder_state(&mut state.body, state_id, else_state);
                    }
                }
                // Fix then_end → after and else_end → after
                for state in states.iter_mut() {
                    if state.num == then_end_state || state.num == else_end_state {
                        if let StateExit::Goto(ref mut target) = state.exit {
                            if *target == 0 {
                                *target = after_state;
                            }
                        }
                    }
                }
            }

            // Let with yield* delegation initializer: `const x = yield* inner()`.
            // Per spec: x receives the value returned by inner when it completes
            // (the `value` field of `{value, done: true}`). Without this arm
            // the catch-all below treated `yield*` like `yield`, sending the
            // iterator object itself as the yielded value and assigning __sent
            // (typically undefined) to x — both wrong.
            Stmt::Let {
                id,
                init:
                    Some(Expr::Yield {
                        value: Some(inner),
                        delegate: true,
                    }),
                mutable,
                ty,
                name,
            } => {
                let del_iter_id = alloc_local(next_local_id);
                let del_result_id = alloc_local(next_local_id);

                // Initial pull: argless (first `next()` value is discarded per spec).
                let next_call = Expr::Call {
                    callee: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(del_iter_id)),
                        property: "next".to_string(),
                    }),
                    args: vec![],
                    type_args: vec![],
                };
                // #1832: in-loop pull forwards the outer resume value (`sent_id`).
                let next_call_resumed = Expr::Call {
                    callee: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(del_iter_id)),
                        property: "next".to_string(),
                    }),
                    args: vec![Expr::LocalGet(sent_id)],
                    type_args: vec![],
                };

                // #1831: resolve the iterator. For a generator *call* the result
                // already is its iterator; for an arbitrary iterable (effect,
                // custom `[Symbol.iterator]`) `js_get_iterator` invokes the
                // well-known-symbol method to obtain one. Async generators
                // delegate through the async-iterator protocol.
                current.push(Stmt::Expr(Expr::LocalSet(
                    del_iter_id,
                    Box::new(delegate_get_iterator(*inner.clone())),
                )));
                current.push(Stmt::Expr(Expr::LocalSet(
                    del_result_id,
                    Box::new(delegate_await(next_call)),
                )));

                let while_body = vec![
                    Stmt::Expr(Expr::Yield {
                        value: Some(Box::new(Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(del_result_id)),
                            property: "value".to_string(),
                        })),
                        delegate: false,
                    }),
                    Stmt::Expr(Expr::LocalSet(
                        del_result_id,
                        Box::new(delegate_await(next_call_resumed)),
                    )),
                ];

                let while_stmt = Stmt::While {
                    condition: Expr::Unary {
                        op: UnaryOp::Not,
                        operand: Box::new(Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(del_result_id)),
                            property: "done".to_string(),
                        }),
                    },
                    body: while_body,
                };

                linearize_body(
                    &[while_stmt],
                    states,
                    current,
                    state_num,
                    state_id,
                    next_local_id,
                    sent_id,
                    catches,
                    finallys,
                );

                // After the loop, the iterator's final `value` (from
                // {value, done:true}) becomes the value of `yield* expr`.
                current.push(Stmt::Let {
                    id: *id,
                    init: Some(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(del_result_id)),
                        property: "value".to_string(),
                    }),
                    mutable: *mutable,
                    ty: ty.clone(),
                    name: name.clone(),
                });
            }

            // Let with yield initializer: `const x = yield expr` (two-way yield)
            // After resuming, `x` receives the value passed by the caller via next(val),
            // which is stored in __sent by the next() closure preamble.
            Stmt::Let {
                id,
                init: Some(Expr::Yield { value, .. }),
                mutable,
                ty,
                name,
            } => {
                let yield_val = value
                    .as_ref()
                    .map(|v| *v.clone())
                    .unwrap_or(Expr::Undefined);
                let this_state = *state_num;
                *state_num += 1;
                states.push(State {
                    num: this_state,
                    body: std::mem::take(current),
                    exit: StateExit::Yield {
                        value: yield_val,
                        next_state: *state_num,
                    },
                });
                // Assign __sent (the value from next(val)) to the target local
                current.push(Stmt::Let {
                    id: *id,
                    init: Some(Expr::LocalGet(sent_id)),
                    mutable: *mutable,
                    ty: ty.clone(),
                    name: name.clone(),
                });
            }

            // `do { B } while (C)` containing yield(s): desugar to a flagged
            // `while` so the existing While arm splits the embedded yield into
            // resume states (#1824 — previously this fell through to the
            // catch-all and the yield was never lifted, so the loop-body
            // locals were lost across the await).
            //
            //   __dw_first = true                 (extra local, auto-boxed)
            //   while (__dw_first || C) {
            //     __dw_first = false;
            //     B
            //   }
            //
            // The flag makes the first iteration run unconditionally while
            // keeping `continue` correct (it jumps to the condition; the flag
            // is already false so C is re-checked) and short-circuiting C on
            // the first iteration (matching do-while, where C is not evaluated
            // before the first body run).
            Stmt::DoWhile { body, condition } if body_contains_yield(body) => {
                let first_id = alloc_local(next_local_id);
                current.push(Stmt::Expr(Expr::LocalSet(
                    first_id,
                    Box::new(Expr::Bool(true)),
                )));
                let mut while_body = Vec::with_capacity(body.len() + 1);
                while_body.push(Stmt::Expr(Expr::LocalSet(
                    first_id,
                    Box::new(Expr::Bool(false)),
                )));
                while_body.extend(body.iter().cloned());
                let while_stmt = Stmt::While {
                    condition: Expr::Logical {
                        op: LogicalOp::Or,
                        left: Box::new(Expr::LocalGet(first_id)),
                        right: Box::new(condition.clone()),
                    },
                    body: while_body,
                };
                linearize_body(
                    std::slice::from_ref(&while_stmt),
                    states,
                    current,
                    state_num,
                    state_id,
                    next_local_id,
                    sent_id,
                    catches,
                    finallys,
                );
            }

            // `label: <loop>` containing yield(s): linearize the wrapped loop
            // so the embedded yield is split into resume states (#1824 —
            // previously the whole labeled statement fell through to the
            // catch-all and was emitted unsplit). `break label` / `continue
            // label` that target this loop from its own body level are first
            // rewritten to plain break/continue, which the loop's own
            // linearization then maps to its state targets. (Labeled
            // break/continue from a *nested* loop is left unconverted — the
            // single-sentinel scheme can't yet distinguish targets; this was
            // already unsupported before this arm existed, so no regression.)
            Stmt::Labeled { label, body } if body_contains_yield(std::slice::from_ref(&**body)) => {
                let mut inner = (**body).clone();
                match &mut inner {
                    Stmt::For { body, .. }
                    | Stmt::While { body, .. }
                    | Stmt::DoWhile { body, .. } => {
                        rewrite_labeled_bc_in_stmts(body, label);
                    }
                    _ => {}
                }
                linearize_body(
                    std::slice::from_ref(&inner),
                    states,
                    current,
                    state_num,
                    state_id,
                    next_local_id,
                    sent_id,
                    catches,
                    finallys,
                );
            }

            // Regular statement (no yield) - accumulate
            other => {
                current.push(other.clone());
            }
        }
    }
}

/// Within a labeled loop's body, rewrite `break label` / `continue label`
/// that target THIS label into plain `break` / `continue`, so the loop's own
/// For/While linearization (which only knows about plain break/continue) maps
/// them to the loop's state targets. Descends only through `if` / `try`
/// (which don't capture break/continue), mirroring the scoping of
/// `rewrite_break_continue_in_stmt`. Stops at nested loops and `switch` —
/// a `break label` from inside one of those still targets this loop, but the
/// current single-sentinel scheme can't express that, so those are left
/// as-is (pre-existing limitation).
fn rewrite_labeled_bc_in_stmts(stmts: &mut [Stmt], label: &str) {
    for s in stmts.iter_mut() {
        match s {
            Stmt::LabeledBreak(l) if l == label => *s = Stmt::Break,
            Stmt::LabeledContinue(l) if l == label => *s = Stmt::Continue,
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                rewrite_labeled_bc_in_stmts(then_branch, label);
                if let Some(eb) = else_branch.as_mut() {
                    rewrite_labeled_bc_in_stmts(eb, label);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                rewrite_labeled_bc_in_stmts(body, label);
                if let Some(c) = catch.as_mut() {
                    rewrite_labeled_bc_in_stmts(&mut c.body, label);
                }
                if let Some(f) = finally.as_mut() {
                    rewrite_labeled_bc_in_stmts(f, label);
                }
            }
            _ => {}
        }
    }
}
