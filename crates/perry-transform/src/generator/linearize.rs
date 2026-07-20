//! Body linearization: split generator body into state-machine states keyed by yield points.

use super::*;

thread_local! {
    /// Whether the generator currently being linearized is an `async function*`.
    /// Set by `transform_generator_function_with_extra_captures` right before it
    /// calls `linearize_body` (linearization is fully synchronous, so a
    /// thread-local avoids threading a bool through ~14 recursive call sites).
    /// Read by the `yield*` arms to pick the async vs sync delegation protocol.
    static LINEARIZE_IS_ASYNC_GEN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };

    /// `yield *` delegation suspend-state intervals collected while linearizing an
    /// async generator. After linearization the `.return()` closure consults these
    /// so that `gen.return(v)` while suspended inside a `yield *` forwards to the
    /// delegated iterator's `return` method (spec `yield *` step 6.c) instead of
    /// completing the outer generator directly. Same thread-local rationale as
    /// `LINEARIZE_IS_ASYNC_GEN` (avoids threading a sink through every recursive
    /// `linearize_body` call site).
    static LINEARIZE_DELEGATIONS: std::cell::RefCell<Vec<DelegationRoute>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

pub(crate) fn set_linearize_async_generator(v: bool) {
    LINEARIZE_IS_ASYNC_GEN.with(|c| c.set(v));
}

fn linearize_async_generator() -> bool {
    LINEARIZE_IS_ASYNC_GEN.with(|c| c.get())
}

/// Clear any `yield *` delegation routes left over from a previous generator.
pub(crate) fn reset_delegation_routes() {
    LINEARIZE_DELEGATIONS.with(|c| c.borrow_mut().clear());
}

/// Drain the `yield *` delegation routes collected during the last linearization.
pub(crate) fn take_delegation_routes() -> Vec<DelegationRoute> {
    LINEARIZE_DELEGATIONS.with(|c| std::mem::take(&mut *c.borrow_mut()))
}

/// One `yield *` delegation region in an async generator. The outer generator is
/// "suspended inside this `yield *`" whenever its state id is in
/// `(suspend_state_lo, suspend_state_hi]` — the same exclusive-lower/inclusive-
/// upper convention as [`FinallyRoute`]. `iter_id` is the captured delegated
/// iterator (the `this` for its `return`/`throw` methods).
#[derive(Clone)]
pub struct DelegationRoute {
    pub suspend_state_lo: u32,
    pub suspend_state_hi: u32,
    pub iter_id: LocalId,
    /// The drive loop's iter-result local (`__del_result`). `gen.throw(e)`
    /// forwards into the delegated iterator's `throw`, stores the awaited
    /// result here, then re-drives the state machine so the loop's condition
    /// state observes `__del_result.done` (spec `yield *` step 6.b.ii.5-7).
    pub result_id: LocalId,
    /// The drive loop's condition-check state — where `gen.throw(e)` re-enters
    /// the dispatch loop after writing `result_id`, so a `done` inner result
    /// resumes the outer body after the `yield *` and a not-`done` result
    /// re-yields. This is the `while` loop's `cond_state`, which the linearizer
    /// emits as `suspend_state_lo + 1` (the non-empty pre-loop state always
    /// takes `suspend_state_lo`).
    pub resume_state: u32,
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

/// Spec `yield *` in an **async** generator awaits each delegated
/// `next()`/`return()`/`throw()` result and then requires it to be an Object —
/// `If Type(innerResult) is not Object, throw a TypeError exception`
/// (AsyncGeneratorYield delegation; also enforced by the
/// `%AsyncFromSyncIteratorPrototype%` wrapper for a wrapped sync iterator).
/// Returns the guard statement (empty for sync generators, where
/// `IteratorComplete`/`IteratorValue` read `.done`/`.value` off a primitive via
/// `GetV` and never throw). `result_id` holds the just-awaited iter-result.
fn delegate_result_object_check(result_id: LocalId) -> Vec<Stmt> {
    if !linearize_async_generator() {
        return Vec::new();
    }
    // not Object  <=>  result === null
    //                  || (typeof result !== "object" && typeof result !== "function")
    let not_object = Expr::Logical {
        op: LogicalOp::Or,
        left: Box::new(Expr::Compare {
            op: CompareOp::Eq,
            left: Box::new(Expr::LocalGet(result_id)),
            right: Box::new(Expr::Null),
        }),
        right: Box::new(Expr::Logical {
            op: LogicalOp::And,
            left: Box::new(Expr::Compare {
                op: CompareOp::Ne,
                left: Box::new(Expr::TypeOf(Box::new(Expr::LocalGet(result_id)))),
                right: Box::new(Expr::String("object".to_string())),
            }),
            right: Box::new(Expr::Compare {
                op: CompareOp::Ne,
                left: Box::new(Expr::TypeOf(Box::new(Expr::LocalGet(result_id)))),
                right: Box::new(Expr::String("function".to_string())),
            }),
        }),
    };
    vec![Stmt::If {
        condition: not_object,
        then_branch: vec![Stmt::Throw(Expr::TypeErrorNew(Box::new(Expr::String(
            "Iterator result is not an object".to_string(),
        ))))],
        else_branch: None,
    }]
}

/// Invoke the captured delegated `[[NextMethod]]` (`del_next_id`) with `this` =
/// the delegated iterator (`del_iter_id`). Spec `yield *` reads `next` exactly
/// once at iterator-record creation (GetIterator) and re-uses the captured
/// method for every pull, so the desugar must NOT re-read `del_iter.next` on
/// each loop iteration (that re-ran the iterator's `get next` accessor and an
/// extra property read, diverging from Node's operation order — test262
/// yield-star-{async,sync}-next, yield-star-next-*).
///
/// Generated shape:
///   typeof __del_next === "function"
///     ? __del_next.call(__del_iter, arg)   // observable case: captured method
///     : __del_iter.next(arg)               // fallback: method dispatch
///
/// The captured-method path calls through `.call` (reads Function.prototype,
/// not the iterator's getters, and binds `this` for builtin/inherited `next`
/// thunks — e.g. the array-iterator prototype's `next`). The fallback covers
/// builtin iterators that expose no *readable* `next` property (string and
/// typed-array iterators dispatch `.next()` through the class-id method tower);
/// for those, re-reading is harmless because there is no observable getter.
fn delegate_next_call(del_next_id: LocalId, del_iter_id: LocalId, arg: Expr) -> Expr {
    // Spec passes `received.[[Value]]` to every inner `next()` — including the
    // first pull, where `received` is `NormalCompletion(undefined)`. So the
    // delegated `next` is ALWAYS called with exactly one argument (the first
    // pull with an explicit `undefined`, not argless — test262
    // yield-star-{sync,async}-next assert `next args.length === 1`).
    let call_args = vec![Expr::LocalGet(del_iter_id), arg.clone()];
    let method_args = vec![arg];
    Expr::Conditional {
        condition: Box::new(Expr::Compare {
            op: CompareOp::Eq,
            left: Box::new(Expr::TypeOf(Box::new(Expr::LocalGet(del_next_id)))),
            right: Box::new(Expr::String("function".to_string())),
        }),
        then_expr: Box::new(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(Expr::LocalGet(del_next_id)),
                property: "call".to_string(),
            }),
            args: call_args,
            type_args: vec![],
            byte_offset: 0,
        }),
        else_expr: Box::new(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(Expr::LocalGet(del_iter_id)),
                property: "next".to_string(),
            }),
            args: method_args,
            type_args: vec![],
            byte_offset: 0,
        }),
    }
}

/// Emit the common `yield *` delegation prelude + driving loop into `current`
/// and linearize it. Shared by all three desugar positions (statement-level
/// `yield* e`, `return yield* e`, `let x = yield* e`). Returns the local id
/// holding the delegated iterator's completion value — spec
/// `IteratorValue(innerResult)` read once from the final `{value, done: true}`
/// result. The read happens unconditionally even at statement level (where the
/// value is discarded), because a `value` getter that throws must still fire and
/// reject the generator (test262 yield-star-next-call-value-get-abrupt).
#[allow(clippy::too_many_arguments)]
fn emit_yield_star_loop(
    inner: &Expr,
    states: &mut Vec<State>,
    current: &mut Vec<Stmt>,
    state_num: &mut u32,
    state_id: LocalId,
    next_local_id: &mut u32,
    sent_id: LocalId,
    catches: &mut Vec<CatchRoute>,
    finallys: &mut Vec<FinallyRoute>,
) -> LocalId {
    let del_iter_id = alloc_local(next_local_id);
    let del_next_id = alloc_local(next_local_id);
    let del_result_id = alloc_local(next_local_id);

    // #1831: resolve the iterator (a generator *call* already is its iterator;
    // an arbitrary iterable resolves via `[Symbol.iterator]` /
    // `[Symbol.asyncIterator]`).
    current.push(Stmt::Expr(Expr::LocalSet(
        del_iter_id,
        Box::new(delegate_get_iterator(inner.clone())),
    )));
    // Capture `[[NextMethod]]` once (see `delegate_next_call`).
    current.push(Stmt::Expr(Expr::LocalSet(
        del_next_id,
        Box::new(Expr::PropertyGet {
            byte_offset: 0,
            object: Box::new(Expr::LocalGet(del_iter_id)),
            property: "next".to_string(),
        }),
    )));
    // Initial pull: `received` starts as `NormalCompletion(undefined)`, so the
    // first inner `next()` gets an explicit `undefined` argument.
    current.push(Stmt::Expr(Expr::LocalSet(
        del_result_id,
        Box::new(delegate_await(delegate_next_call(
            del_next_id,
            del_iter_id,
            Expr::Undefined,
        ))),
    )));
    // Async `yield *`: the awaited iter-result must be an Object or `next()`
    // throws a TypeError (test262 yield-star-next-non-object-ignores-then).
    current.extend(delegate_result_object_check(del_result_id));

    // #1832: in-loop pull forwards the outer resume value (`outer.next(v)` →
    // `sent_id`) into the delegated iterator's `next(v)`.
    let mut while_body = vec![
        // Spec step `received be AsyncGeneratorYield(? IteratorValue(innerResult))`.
        // Unlike a plain `yield x` (which is `AsyncGeneratorYield(? Await(x))` and
        // is handled by the #4777 await pass), the DELEGATED value is NOT awaited:
        // current `AsyncGeneratorYield` does not await its operand, so a delegated
        // promise value flows through un-unwrapped (test262
        // yield-star-promise-not-unwrapped). Only the `next()` RESULT is awaited
        // (via `delegate_await` on the pull below).
        Stmt::Expr(Expr::Yield {
            value: Some(Box::new(Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(Expr::LocalGet(del_result_id)),
                property: "value".to_string(),
            })),
            delegate: false,
        }),
        Stmt::Expr(Expr::LocalSet(
            del_result_id,
            Box::new(delegate_await(delegate_next_call(
                del_next_id,
                del_iter_id,
                Expr::LocalGet(sent_id),
            ))),
        )),
    ];
    // Same Object-check on the in-loop pull (async generators only).
    while_body.extend(delegate_result_object_check(del_result_id));
    let while_stmt = Stmt::While {
        condition: Expr::Unary {
            op: UnaryOp::Not,
            operand: Box::new(Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(Expr::LocalGet(del_result_id)),
                property: "done".to_string(),
            }),
        },
        body: while_body,
    };

    // Record the suspend-state interval of the drive loop's single re-yield so
    // an async `gen.return(v)` issued while suspended here forwards into the
    // delegated iterator's `return` (spec `yield *` step 6.c) rather than
    // completing the outer generator outright. The loop's only suspendable state
    // is the inner `yield`, whose resume state lands in `(lo, hi]`.
    let deleg_lo = *state_num;
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
    if linearize_async_generator() {
        let deleg_hi = *state_num;
        LINEARIZE_DELEGATIONS.with(|c| {
            c.borrow_mut().push(DelegationRoute {
                suspend_state_lo: deleg_lo,
                suspend_state_hi: deleg_hi,
                iter_id: del_iter_id,
                result_id: del_result_id,
                // `current` always holds the iterator-setup prelude pushed above,
                // so the `while` linearizer emits a non-empty pre-loop state at
                // `deleg_lo` and the condition state at `deleg_lo + 1`.
                resume_state: deleg_lo + 1,
            })
        });
    }

    // Spec step 6.a.vi: `If done is true, then Return ? IteratorValue(innerResult)`.
    // Read the final result's `.value` exactly once into a dedicated local. This
    // runs on the loop-exit (done) path, so a throwing `value` getter rejects the
    // generator here regardless of position — including bare statement-level
    // `yield* e`, where the value is otherwise discarded.
    let del_value_id = alloc_local(next_local_id);
    current.push(Stmt::Expr(Expr::LocalSet(
        del_value_id,
        Box::new(Expr::PropertyGet {
            byte_offset: 0,
            object: Box::new(Expr::LocalGet(del_result_id)),
            property: "value".to_string(),
        }),
    )));

    del_value_id
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
                // Desugar `yield* inner` into a drive loop:
                //   let __del_iter = GetIterator(inner);
                //   let __del_next = __del_iter.next;          // captured ONCE
                //   let __del_result = __del_next.call(__del_iter);
                //   while (!__del_result.done) {
                //     yield __del_result.value;
                //     __del_result = __del_next.call(__del_iter, __sent);
                //   }
                // (see `emit_yield_star_loop` for the shared implementation).
                emit_yield_star_loop(
                    inner,
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
                let del_value_id = emit_yield_star_loop(
                    inner,
                    states,
                    current,
                    state_num,
                    state_id,
                    next_local_id,
                    sent_id,
                    catches,
                    finallys,
                );

                // The iterator's final `value` (read once by `emit_yield_star_loop`
                // as spec `IteratorValue`) is the value of `yield* inner`, which is
                // exactly what `return yield* inner` returns. Wrap it as the
                // generator's terminal {value, done:true} and flush a Done state.
                current.push(Stmt::Return(Some(make_iter_result(
                    Expr::LocalGet(del_value_id),
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

            // While condition containing a yield (direct-generator
            // `while (yield …)` / `while ((x = yield …) !== s)`; async fns'
            // awaited conditions are restructured at the hoist layer before
            // they become yields, so this arm covers the generator case).
            // The old path cloned the condition — with its embedded yield —
            // into the cond_state, where codegen's fallback lowers the
            // residual `Expr::Yield` to `0.0` (#5933). Restructure to the
            // per-iteration form and recurse; `hoist_yields_in_stmts`
            // normalizes the condition's yields to the statement positions
            // the Let-with-yield arms handle.
            //
            //   while (true) {
            //     let __loop_cond_yield_N = <cond>;
            //     if (!__loop_cond_yield_N) break;
            //     <body>
            //   }
            Stmt::While { condition, body }
                if super::hoist_yields::expr_contains_yield(condition) =>
            {
                let t = alloc_local(next_local_id);
                let mut prefix = vec![Stmt::Let {
                    id: t,
                    name: format!("__loop_cond_yield_{}", t),
                    ty: Type::Any,
                    mutable: true,
                    init: Some(condition.clone()),
                }];
                hoist_yields_in_stmts(&mut prefix, next_local_id);
                prefix.push(Stmt::If {
                    condition: Expr::Unary {
                        op: UnaryOp::Not,
                        operand: Box::new(Expr::LocalGet(t)),
                    },
                    then_branch: vec![Stmt::Break],
                    else_branch: None,
                });
                prefix.extend(body.iter().cloned());
                let restructured = Stmt::While {
                    condition: Expr::Bool(true),
                    body: prefix,
                };
                linearize_body(
                    std::slice::from_ref(&restructured),
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

            // do-while with a yield in the condition: first-iteration-flag
            // desugar (same as the body-yield DoWhile arm below), which puts
            // the yield into a While condition — the arm above then applies
            // on recursion, and `continue` correctly falls through to the
            // condition evaluation.
            Stmt::DoWhile { body, condition }
                if super::hoist_yields::expr_contains_yield(condition) =>
            {
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

            // For-loop with a yield in the CONDITION or UPDATE slot: move
            // the yielding slot(s) into the body (condition → body-top
            // check; update → body-end with loop-level `continue`s prefixed
            // by it so the continue → update → condition order holds), then
            // recurse — the For/While arms below handle the rest.
            Stmt::For {
                init,
                condition,
                update,
                body,
            } if condition
                .as_ref()
                .is_some_and(super::hoist_yields::expr_contains_yield)
                || (update
                    .as_ref()
                    .is_some_and(super::hoist_yields::expr_contains_yield)
                    && !stmts_have_continue_inside_try_finally(body)) =>
            {
                let cond_yields = condition
                    .as_ref()
                    .is_some_and(super::hoist_yields::expr_contains_yield);
                let upd_yields = update
                    .as_ref()
                    .is_some_and(super::hoist_yields::expr_contains_yield)
                    // Abrupt-completion ordering: see the async twin — a
                    // continue inside try/finally must not have the update
                    // prefixed ahead of the finally (#5934 review).
                    && !stmts_have_continue_inside_try_finally(body);
                let mut new_body = Vec::with_capacity(body.len() + 3);
                if cond_yields {
                    let t = alloc_local(next_local_id);
                    let mut prefix = vec![Stmt::Let {
                        id: t,
                        name: format!("__loop_cond_yield_{}", t),
                        ty: Type::Any,
                        mutable: true,
                        init: condition.clone(),
                    }];
                    hoist_yields_in_stmts(&mut prefix, next_local_id);
                    new_body.append(&mut prefix);
                    new_body.push(Stmt::If {
                        condition: Expr::Unary {
                            op: UnaryOp::Not,
                            operand: Box::new(Expr::LocalGet(t)),
                        },
                        then_branch: vec![Stmt::Break],
                        else_branch: None,
                    });
                }
                let mut taken_body = body.clone();
                if upd_yields {
                    let mut upd_stmts = vec![Stmt::Expr(update.clone().unwrap())];
                    hoist_yields_in_stmts(&mut upd_stmts, next_local_id);
                    prefix_loop_continues(&mut taken_body, &upd_stmts);
                    new_body.append(&mut taken_body);
                    new_body.extend(upd_stmts);
                } else {
                    new_body.append(&mut taken_body);
                }
                let new_for = Stmt::For {
                    init: init.clone(),
                    condition: if cond_yields { None } else { condition.clone() },
                    update: if upd_yields { None } else { update.clone() },
                    body: new_body,
                };
                linearize_body(
                    std::slice::from_ref(&new_for),
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
                let body_catches_before = catches.len();
                let mut body_rewritten = body.clone();
                rewrite_break_continue_in_stmts(&mut body_rewritten, state_id, next_local_id);

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
                // Async-generator `.throw()` closures inline catch-route bodies
                // verbatim; fix any break/continue sentinels they captured from
                // this loop body (a user `continue`/`break` inside a `catch`).
                fix_break_continue_sentinels_in_catches(
                    &mut catches[body_catches_before..],
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
                let while_catches_before = catches.len();
                let mut while_body_rewritten = while_body.clone();
                rewrite_break_continue_in_stmts(&mut while_body_rewritten, state_id, next_local_id);

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
                // Async-generator `.throw()` closures inline catch-route bodies
                // verbatim; fix any break/continue sentinels they captured from
                // this loop body (a user `continue`/`break` inside a `catch`).
                fix_break_continue_sentinels_in_catches(
                    &mut catches[while_catches_before..],
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
                let del_value_id = emit_yield_star_loop(
                    inner,
                    states,
                    current,
                    state_num,
                    state_id,
                    next_local_id,
                    sent_id,
                    catches,
                    finallys,
                );

                // The iterator's final `value` (read once by `emit_yield_star_loop`
                // as spec `IteratorValue`) becomes the value of `yield* expr`.
                current.push(Stmt::Let {
                    id: *id,
                    init: Some(Expr::LocalGet(del_value_id)),
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
                    // A labeled yielding SWITCH: `break label` at case-body
                    // level is the switch's own break — rewrite it to plain
                    // `break` so the yielding-switch desugar below folds it
                    // into the done-flag (#5868).
                    Stmt::Switch { cases, .. } => {
                        for case in cases.iter_mut() {
                            rewrite_labeled_bc_in_stmts(&mut case.body, label);
                        }
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

            // `switch` containing yield(s) — in a case body, a case test, or
            // the discriminant: desugar into a match-index + guarded-`if`
            // chain (see `desugar_switch_to_ifs`) and recurse, so the
            // existing `If` linearization splits the yield into resume
            // states. Previously this fell through to the catch-all: the
            // switch was emitted unsplit inside one state and codegen
            // lowered the embedded residual `Expr::Yield` to `0.0` —
            // `async f(x){ switch(x){ case 1: return await g() } }` resolved
            // to `0` without ever suspending (#5868).
            Stmt::Switch {
                discriminant,
                cases,
            } if super::hoist_yields::expr_contains_yield(discriminant)
                || cases.iter().any(|c| {
                    body_contains_yield(&c.body)
                        || c.test
                            .as_ref()
                            .is_some_and(super::hoist_yields::expr_contains_yield)
                }) =>
            {
                let desugared = super::break_continue::desugar_switch_to_ifs(
                    discriminant,
                    cases,
                    next_local_id,
                );
                linearize_body(
                    &desugared,
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
            // #5975: a `continue <label>` that targets THIS enclosing labeled
            // loop from inside a nested `switch` case. A switch never captures
            // `continue`, so it continues the loop — rewrite it to a plain
            // `continue` here so the loop's linearization (and the #5868
            // yielding-switch desugar) map it to the loop's re-entry sentinel.
            // Without this the `LabeledContinue` survives verbatim into the
            // desugared switch's state machine, where nothing lowers it, and a
            // `loop: while (…) { switch (…) { case …: yield …; continue loop } }`
            // (e.g. the `yaml` package's block-scalar / indicator lexer, a
            // generator) spins forever. `break <label>` is deliberately NOT
            // rewritten in a nested switch: a switch DOES capture `break`, so a
            // plain `break` would exit only the switch, not the loop — that is
            // the pre-existing single-sentinel limitation documented above.
            Stmt::Switch { cases, .. } => {
                for case in cases.iter_mut() {
                    rewrite_labeled_continue_in_stmts(&mut case.body, label);
                }
            }
            _ => {}
        }
    }
}

/// Rewrite `continue <label>` → plain `continue` for `label`, descending
/// through `if` / `try` / `switch` (none of which capture `continue`) but
/// stopping at nested loops (which bind their own `continue`). Unlike
/// [`rewrite_labeled_bc_in_stmts`] this does NOT touch `break <label>`: it is
/// used only when recursing into a nested `switch`, where a plain `break`
/// would be captured by the switch rather than escaping to the loop (#5975).
fn rewrite_labeled_continue_in_stmts(stmts: &mut [Stmt], label: &str) {
    for s in stmts.iter_mut() {
        match s {
            Stmt::LabeledContinue(l) if l == label => *s = Stmt::Continue,
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                rewrite_labeled_continue_in_stmts(then_branch, label);
                if let Some(eb) = else_branch.as_mut() {
                    rewrite_labeled_continue_in_stmts(eb, label);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                rewrite_labeled_continue_in_stmts(body, label);
                if let Some(c) = catch.as_mut() {
                    rewrite_labeled_continue_in_stmts(&mut c.body, label);
                }
                if let Some(f) = finally.as_mut() {
                    rewrite_labeled_continue_in_stmts(f, label);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases.iter_mut() {
                    rewrite_labeled_continue_in_stmts(&mut case.body, label);
                }
            }
            _ => {}
        }
    }
}
