//! Body linearization: split generator body into state-machine states keyed by yield points.

use super::*;

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
    catches: &mut Vec<(Option<LocalId>, Vec<Stmt>, u32)>,
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

                let next_call = Expr::Call {
                    callee: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(del_iter_id)),
                        property: "next".to_string(),
                    }),
                    args: vec![],
                    type_args: vec![],
                };

                // Add hoisted var declarations to current (they'll be emitted in the state body)
                current.push(Stmt::Expr(Expr::LocalSet(
                    del_iter_id,
                    Box::new(*inner.clone()),
                )));
                current.push(Stmt::Expr(Expr::LocalSet(
                    del_result_id,
                    Box::new(next_call.clone()),
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
                    Stmt::Expr(Expr::LocalSet(del_result_id, Box::new(next_call))),
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

            // Try-catch containing yield(s) — linearize the try body directly and
            // stash the catch body so the .throw() closure can inline it.
            // Limitations: no per-state exception handler tracking, so only the
            // first catch encountered will run on .throw(). Catches themselves
            // must not yield — they run to completion inside the throw closure.
            Stmt::Try {
                body,
                catch,
                finally,
            } if body_contains_yield(body)
                || finally.as_ref().is_some_and(|f| body_contains_yield(f)) =>
            {
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
                    );
                } else {
                    // Body has no yields: push as-is to current state.
                    for s in body {
                        current.push(s.clone());
                    }
                }

                // Issue #621: if the try has a catch handler, split the
                // post-await happy-path continuation (currently in `current`)
                // from the post-try-catch continuation. Stmts that follow
                // the LAST yield in the try body should only run on the
                // happy path; the throw path runs the catch body and then
                // resumes at the stmts AFTER the try/catch. Without this
                // split, both paths land in the same state and the catch
                // path incorrectly runs the post-await stmts.
                if catch.is_some() {
                    if !current.is_empty() {
                        let happy_state = *state_num;
                        *state_num += 1;
                        let goto_target = *state_num;
                        states.push(State {
                            num: happy_state,
                            body: std::mem::take(current),
                            exit: StateExit::Goto(goto_target),
                        });
                    }
                }
                let post_catch_state = *state_num;

                // Stash the catch so transform_generator_function can inline it
                // into the .throw() closure later.
                if let Some(catch_clause) = catch {
                    let param_id = catch_clause.param.as_ref().map(|(id, _)| *id);
                    catches.push((param_id, catch_clause.body.clone(), post_catch_state));
                }

                // Finally block: linearize if it has yields (await-using path),
                // otherwise push as-is.
                if let Some(fin) = finally {
                    if body_contains_yield(fin) {
                        linearize_body(
                            fin,
                            states,
                            current,
                            state_num,
                            state_id,
                            next_local_id,
                            sent_id,
                            catches,
                        );
                    } else {
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

                let next_call = Expr::Call {
                    callee: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(del_iter_id)),
                        property: "next".to_string(),
                    }),
                    args: vec![],
                    type_args: vec![],
                };

                current.push(Stmt::Expr(Expr::LocalSet(
                    del_iter_id,
                    Box::new(*inner.clone()),
                )));
                current.push(Stmt::Expr(Expr::LocalSet(
                    del_result_id,
                    Box::new(next_call.clone()),
                )));

                let while_body = vec![
                    Stmt::Expr(Expr::Yield {
                        value: Some(Box::new(Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(del_result_id)),
                            property: "value".to_string(),
                        })),
                        delegate: false,
                    }),
                    Stmt::Expr(Expr::LocalSet(del_result_id, Box::new(next_call))),
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

            // Regular statement (no yield) - accumulate
            other => {
                current.push(other.clone());
            }
        }
    }
}
