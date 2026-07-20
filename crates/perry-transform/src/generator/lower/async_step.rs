//! The async-step driver for `was_plain_async` generators (the Promise-
//! returning step closure that drives the state machine, plus the direct
//! variants of the throw/catch-route builders it uses). Split out of
//! `lower.rs`.

use super::*;

pub(crate) fn build_async_throw_body_direct(
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

pub(crate) fn build_async_catch_route_body_direct(
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
            byte_offset: 0,
            object: Box::new(promise_global()),
            property: "resolve".to_string(),
        }),
        args: vec![arg],
        type_args: vec![],
        byte_offset: 0,
    };
    let promise_reject = |arg: Expr| Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            byte_offset: 0,
            object: Box::new(promise_global()),
            property: "reject".to_string(),
        }),
        args: vec![arg],
        type_args: vec![],
        byte_offset: 0,
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
                byte_offset: 0,
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
                        byte_offset: 0,
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
