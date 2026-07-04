//! break/continue sentinel rewriting + body-analysis helpers (yield/return detection, hoisted-var collection).

use super::*;

/// Fix the placeholder `0.0` state number in condition branches.
/// Sentinel state-number for `Stmt::Break` placeholders. Chosen to fall well
/// outside any legitimate state count (state numbers grow from 0; even huge
/// async functions stay in the thousands). After body linearization completes,
/// `fix_break_continue_sentinels` swaps every occurrence with the loop's
/// real `after_loop` state number.
const BREAK_SENTINEL: f64 = 1_000_001.0;
/// Sentinel for `Stmt::Continue`. Swapped with the loop's `update_state`
/// (for-loops) or `cond_state` (while-loops) post-linearization.
const CONTINUE_SENTINEL: f64 = 1_000_002.0;

/// Walk a body and rewrite every top-level `Stmt::Break` / `Stmt::Continue`
/// into `[LocalSet(state_id, <sentinel>), Stmt::Continue]`. The trailing
/// `Stmt::Continue` is the state-machine's dispatch-loop continue, which
/// re-enters the while(true) and re-dispatches on the new state. Stops at
/// nested loop / closure boundaries — their own break/continue belong to
/// those constructs, not to us. A nested `switch` captures `break` but
/// NEVER `continue`: a switch whose cases carry a loop-level `continue` is
/// desugared into plain `if`s first (#5868 — previously the raw
/// `Stmt::Continue` survived verbatim inside the switch in the state body
/// and the dispatch lowering silently ignored it, so the rest of the loop
/// iteration ran anyway).
pub fn rewrite_break_continue_in_stmts(
    stmts: &mut Vec<Stmt>,
    state_id: LocalId,
    next_local_id: &mut u32,
) {
    let mut i = 0;
    while i < stmts.len() {
        let stmt = std::mem::replace(&mut stmts[i], Stmt::Continue);
        match stmt {
            Stmt::Break => {
                stmts[i] = Stmt::Expr(Expr::LocalSet(
                    state_id,
                    Box::new(Expr::Number(BREAK_SENTINEL)),
                ));
                stmts.insert(i + 1, Stmt::Continue);
                i += 2;
            }
            Stmt::Continue => {
                stmts[i] = Stmt::Expr(Expr::LocalSet(
                    state_id,
                    Box::new(Expr::Number(CONTINUE_SENTINEL)),
                ));
                stmts.insert(i + 1, Stmt::Continue);
                i += 2;
            }
            Stmt::Switch {
                discriminant,
                cases,
            } if switch_cases_have_loop_continue(&cases) => {
                // Replace the switch (currently a placeholder after the
                // mem::replace) with its if-chain desugar and reprocess from
                // the same index: the desugared statements are plain `if`s,
                // so this rewriter descends them and converts the loop-level
                // `continue`s to sentinels; `break`s were already folded
                // into the desugar's done-flag.
                let desugared = desugar_switch_to_ifs(&discriminant, &cases, next_local_id);
                stmts.splice(i..=i, desugared);
            }
            mut other => {
                rewrite_break_continue_in_stmt(&mut other, state_id, next_local_id);
                stmts[i] = other;
                i += 1;
            }
        }
    }
}

pub fn rewrite_break_continue_in_stmt(stmt: &mut Stmt, state_id: LocalId, next_local_id: &mut u32) {
    match stmt {
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_break_continue_in_stmts(then_branch, state_id, next_local_id);
            if let Some(eb) = else_branch.as_mut() {
                rewrite_break_continue_in_stmts(eb, state_id, next_local_id);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            rewrite_break_continue_in_stmts(body, state_id, next_local_id);
            if let Some(c) = catch.as_mut() {
                rewrite_break_continue_in_stmts(&mut c.body, state_id, next_local_id);
            }
            if let Some(f) = finally.as_mut() {
                rewrite_break_continue_in_stmts(f, state_id, next_local_id);
            }
        }
        // Inside nested loops / closure expressions, the user's
        // `break`/`continue` belongs to that construct and not to the outer
        // loop the state machine is unrolling. Leave them as-is so the inner
        // linearize_body (if it yields) / regular codegen (if it doesn't)
        // handles them. A `switch` reaching here carries no loop-level
        // `continue` (the stmts-level pass desugared those), and its
        // `break`s bind to the switch itself. `Labeled` is left as-is
        // (pre-existing single-sentinel limitation).
        Stmt::For { .. } | Stmt::While { .. } | Stmt::DoWhile { .. } => {}
        Stmt::Switch { .. } => {}
        Stmt::Labeled { .. } => {}
        _ => {}
    }
}

/// Walk a slice of generator states and replace BREAK_SENTINEL /
/// CONTINUE_SENTINEL with their real target state numbers. Called after a
/// For/While body has been fully linearized into the state list.
pub fn fix_break_continue_sentinels(
    states: &mut [State],
    state_id: LocalId,
    break_target: u32,
    continue_target: u32,
) {
    for state in states.iter_mut() {
        fix_break_continue_sentinels_in_stmts(
            &mut state.body,
            state_id,
            break_target,
            continue_target,
        );
    }
}

pub fn fix_break_continue_sentinels_in_stmts(
    stmts: &mut [Stmt],
    state_id: LocalId,
    break_target: u32,
    continue_target: u32,
) {
    for stmt in stmts.iter_mut() {
        fix_break_continue_sentinels_in_stmt(stmt, state_id, break_target, continue_target);
    }
}

/// Fix BREAK/CONTINUE sentinels inside the bodies of `CatchRoute`s captured
/// while linearizing a loop body. The async-generator `.throw()` closure
/// inlines `route.body` verbatim (no dispatch loop), so a user `continue`/
/// `break` inside such a catch was rewritten to
/// `[LocalSet(state, SENTINEL), Stmt::Continue]` but its sentinel never got
/// fixed (`fix_break_continue_sentinels` only walks the linearized `states`,
/// not the extracted catch routes). Apply the same loop targets to those
/// catch-route bodies so the resume state is correct (the dangling dispatch
/// `Stmt::Continue` is then neutralized by the async catch-route inliner).
pub fn fix_break_continue_sentinels_in_catches(
    catches: &mut [CatchRoute],
    state_id: LocalId,
    break_target: u32,
    continue_target: u32,
) {
    for route in catches.iter_mut() {
        fix_break_continue_sentinels_in_stmts(
            &mut route.body,
            state_id,
            break_target,
            continue_target,
        );
    }
}

pub fn fix_break_continue_sentinels_in_stmt(
    stmt: &mut Stmt,
    state_id: LocalId,
    break_target: u32,
    continue_target: u32,
) {
    match stmt {
        Stmt::Expr(Expr::LocalSet(id, val)) if *id == state_id => {
            if let Expr::Number(n) = val.as_ref() {
                if *n == BREAK_SENTINEL {
                    **val = Expr::Number(break_target as f64);
                } else if *n == CONTINUE_SENTINEL {
                    **val = Expr::Number(continue_target as f64);
                }
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            fix_break_continue_sentinels_in_stmts(
                then_branch,
                state_id,
                break_target,
                continue_target,
            );
            if let Some(eb) = else_branch.as_mut() {
                fix_break_continue_sentinels_in_stmts(eb, state_id, break_target, continue_target);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            fix_break_continue_sentinels_in_stmts(body, state_id, break_target, continue_target);
        }
        Stmt::For { body, .. } => {
            fix_break_continue_sentinels_in_stmts(body, state_id, break_target, continue_target);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            fix_break_continue_sentinels_in_stmts(body, state_id, break_target, continue_target);
            if let Some(c) = catch.as_mut() {
                fix_break_continue_sentinels_in_stmts(
                    &mut c.body,
                    state_id,
                    break_target,
                    continue_target,
                );
            }
            if let Some(f) = finally.as_mut() {
                fix_break_continue_sentinels_in_stmts(f, state_id, break_target, continue_target);
            }
        }
        Stmt::Switch { cases, .. } => {
            for case in cases.iter_mut() {
                fix_break_continue_sentinels_in_stmts(
                    &mut case.body,
                    state_id,
                    break_target,
                    continue_target,
                );
            }
        }
        Stmt::Labeled { body, .. } => {
            fix_break_continue_sentinels_in_stmt(
                body.as_mut(),
                state_id,
                break_target,
                continue_target,
            );
        }
        _ => {}
    }
}

pub fn fix_placeholder_state(stmts: &mut [Stmt], state_id: LocalId, target_state: u32) {
    fn fix_branch(branch: &mut [Stmt], state_id: LocalId, target_state: u32) {
        for inner in branch.iter_mut() {
            if let Stmt::Expr(Expr::LocalSet(id, val)) = inner {
                if *id == state_id {
                    if let Expr::Number(n) = val.as_ref() {
                        if *n == 0.0 {
                            **val = Expr::Number(target_state as f64);
                        }
                    }
                }
            }
        }
    }
    for stmt in stmts.iter_mut() {
        if let Stmt::If {
            then_branch,
            else_branch,
            ..
        } = stmt
        {
            fix_branch(then_branch, state_id, target_state);
            if let Some(eb) = else_branch {
                fix_branch(eb, state_id, target_state);
            }
        }
    }
}

/// Check if any statement in the body contains a yield expression.
pub fn body_contains_yield(stmts: &[Stmt]) -> bool {
    for stmt in stmts {
        match stmt {
            Stmt::Expr(Expr::Yield { .. }) => return true,
            Stmt::Let {
                init: Some(Expr::Yield { .. }),
                ..
            } => return true,
            Stmt::Return(Some(Expr::Yield { .. })) => return true,
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                if body_contains_yield(then_branch) {
                    return true;
                }
                if let Some(eb) = else_branch {
                    if body_contains_yield(eb) {
                        return true;
                    }
                }
            }
            Stmt::While { body, .. } if body_contains_yield(body) => {
                return true;
            }
            // A yield buried in a do-while or labeled loop must still be seen
            // by the enclosing construct's linearization (#1824), otherwise it
            // is never split into resume states.
            Stmt::DoWhile { body, .. } if body_contains_yield(body) => {
                return true;
            }
            Stmt::Labeled { body, .. } if body_contains_yield(std::slice::from_ref(&**body)) => {
                return true;
            }
            Stmt::For { body, .. } if body_contains_yield(body) => {
                return true;
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                if body_contains_yield(body) {
                    return true;
                }
                if let Some(c) = catch {
                    if body_contains_yield(&c.body) {
                        return true;
                    }
                }
                if let Some(f) = finally {
                    if body_contains_yield(f) {
                        return true;
                    }
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    if body_contains_yield(&case.body) {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

/// Collect variable declarations that need to be hoisted to the outer scope.
pub fn collect_hoisted_vars(stmts: &[Stmt]) -> Vec<(LocalId, String, Type)> {
    let mut vars = Vec::new();
    collect_vars_recursive(stmts, &mut vars);
    vars
}

pub fn collect_vars_recursive(stmts: &[Stmt], vars: &mut Vec<(LocalId, String, Type)>) {
    for stmt in stmts {
        match stmt {
            Stmt::Let { id, name, ty, .. } => {
                vars.push((*id, name.clone(), ty.clone()));
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_vars_recursive(then_branch, vars);
                if let Some(eb) = else_branch {
                    collect_vars_recursive(eb, vars);
                }
            }
            Stmt::While { body, .. } => collect_vars_recursive(body, vars),
            // `do { ... } while (cond)` — a `let` declared in the body that is
            // live across an `await` must be hoisted just like a `while` body,
            // otherwise its box is never preallocated and the value is lost
            // across the state-machine split (#1824).
            Stmt::DoWhile { body, .. } => collect_vars_recursive(body, vars),
            // A labeled statement (`outer: for (...) { ... }`) wraps its loop
            // in `Stmt::Labeled`; descend into the wrapped statement so the
            // loop-body `let`s are still hoisted (#1824). Without this, every
            // local inside a labeled loop is dropped across an `await`.
            Stmt::Labeled { body, .. } => {
                collect_vars_recursive(std::slice::from_ref(&**body), vars)
            }
            Stmt::For { init, body, .. } => {
                if let Some(init) = init {
                    collect_vars_recursive(&[(**init).clone()], vars);
                }
                collect_vars_recursive(body, vars);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_vars_recursive(body, vars);
                if let Some(c) = catch {
                    // Catch params are hoisted only for catch routes that
                    // linearize_body lifts into the async throw path. Ordinary
                    // post-await Stmt::Try bodies must keep codegen's direct
                    // catch binding slot.
                    collect_vars_recursive(&c.body, vars);
                }
                if let Some(f) = finally {
                    collect_vars_recursive(f, vars);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    collect_vars_recursive(&case.body, vars);
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// #5868: switch desugaring for state-machine bodies
// ---------------------------------------------------------------------------

/// Does any case body carry a `continue` that binds to the ENCLOSING LOOP
/// (i.e. at switch-case level, or nested only through `if`/`try`/inner
/// `switch` — all constructs that do not capture `continue`)? Loops and
/// labeled statements capture their own `continue`s, so descent stops there.
fn switch_cases_have_loop_continue(cases: &[SwitchCase]) -> bool {
    cases
        .iter()
        .any(|c| stmts_have_loop_level_continue(&c.body))
}

fn stmts_have_loop_level_continue(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| match s {
        Stmt::Continue => true,
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            stmts_have_loop_level_continue(then_branch)
                || else_branch
                    .as_ref()
                    .is_some_and(|e| stmts_have_loop_level_continue(e))
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            stmts_have_loop_level_continue(body)
                || catch
                    .as_ref()
                    .is_some_and(|c| stmts_have_loop_level_continue(&c.body))
                || finally
                    .as_ref()
                    .is_some_and(|f| stmts_have_loop_level_continue(f))
        }
        Stmt::Switch { cases, .. } => switch_cases_have_loop_continue(cases),
        _ => false,
    })
}

/// Desugar a `switch` into an equivalent match-index + guarded-`if` chain
/// (#5868). Used in two places:
///
///   1. `linearize_body`'s yielding-switch arm — a `yield`/`await` inside a
///      case body previously fell through to the catch-all, was emitted
///      unsplit inside one state, and codegen lowered the residual
///      `Expr::Yield` to `0.0`.
///   2. `rewrite_break_continue_in_stmts` — a loop-level `continue` inside
///      a (yield-free) switch in a linearized loop body previously survived
///      as a raw `Stmt::Continue` the dispatch loop ignored.
///
/// Shape (JS switch semantics preserved):
///
/// ```text
///   __sw_d   = <discriminant>;            // evaluated exactly once
///   __sw_idx = UNMATCHED;
///   // case tests, evaluated only while still unmatched (first match wins;
///   // spec order == source order of the non-default clauses):
///   if (__sw_idx === UNMATCHED) { __sw_t = <test_i>; if (__sw_d === __sw_t) __sw_idx = i; }
///   ...
///   if (__sw_idx === UNMATCHED) __sw_idx = <default position, or past-end>;
///   __sw_done = false;
///   // bodies in POSITIONAL order — `__sw_idx <= i` gives fallthrough;
///   // `break` becomes `__sw_done = true` plus remainder-guarding:
///   if (!__sw_done && __sw_idx <= i) { <guarded body_i> }
///   ...
/// ```
///
/// `continue` / `return` / `throw` in case bodies pass through untouched —
/// after the desugar they sit in plain `if`s, where the loop machinery (or
/// function-level lowering) handles them normally. Fresh locals follow the
/// DoWhile-flag pattern (plain `LocalSet` on an `alloc_local` id; generator
/// local persistence carries them across suspend states).
pub fn desugar_switch_to_ifs(
    discriminant: &Expr,
    cases: &[SwitchCase],
    next_local_id: &mut u32,
) -> Vec<Stmt> {
    let n = cases.len();
    let unmatched = (n + 1) as f64;
    let default_pos = cases.iter().position(|c| c.test.is_none());
    let start_when_unmatched = default_pos.unwrap_or(n) as f64;

    let d_id = alloc_local(next_local_id);
    let idx_id = alloc_local(next_local_id);
    let done_id = alloc_local(next_local_id);

    let idx_is_unmatched = || Expr::Compare {
        op: CompareOp::Eq,
        left: Box::new(Expr::LocalGet(idx_id)),
        right: Box::new(Expr::Number(unmatched)),
    };

    let mut out = Vec::with_capacity(2 * n + 4);
    out.push(Stmt::Expr(Expr::LocalSet(
        d_id,
        Box::new(discriminant.clone()),
    )));
    out.push(Stmt::Expr(Expr::LocalSet(
        idx_id,
        Box::new(Expr::Number(unmatched)),
    )));

    // Tests in source order over the non-default clauses — identical to the
    // spec's pre-default-then-post-default order, since the default clause
    // contributes no test. Each test evaluates only while unmatched, so
    // side-effecting tests after the first match are (correctly) skipped.
    for (i, case) in cases.iter().enumerate() {
        let Some(test) = &case.test else { continue };
        let t_id = alloc_local(next_local_id);
        out.push(Stmt::If {
            condition: idx_is_unmatched(),
            then_branch: vec![
                Stmt::Expr(Expr::LocalSet(t_id, Box::new(test.clone()))),
                Stmt::If {
                    condition: Expr::Compare {
                        op: CompareOp::Eq,
                        left: Box::new(Expr::LocalGet(d_id)),
                        right: Box::new(Expr::LocalGet(t_id)),
                    },
                    then_branch: vec![Stmt::Expr(Expr::LocalSet(
                        idx_id,
                        Box::new(Expr::Number(i as f64)),
                    ))],
                    else_branch: None,
                },
            ],
            else_branch: None,
        });
    }
    out.push(Stmt::If {
        condition: idx_is_unmatched(),
        then_branch: vec![Stmt::Expr(Expr::LocalSet(
            idx_id,
            Box::new(Expr::Number(start_when_unmatched)),
        ))],
        else_branch: None,
    });
    out.push(Stmt::Expr(Expr::LocalSet(
        done_id,
        Box::new(Expr::Bool(false)),
    )));

    for (i, case) in cases.iter().enumerate() {
        let mut guarded = Vec::new();
        guard_switch_breaks(&case.body, done_id, &mut guarded);
        out.push(Stmt::If {
            condition: Expr::Logical {
                op: LogicalOp::And,
                left: Box::new(Expr::Unary {
                    op: UnaryOp::Not,
                    operand: Box::new(Expr::LocalGet(done_id)),
                }),
                right: Box::new(Expr::Compare {
                    op: CompareOp::Le,
                    left: Box::new(Expr::LocalGet(idx_id)),
                    right: Box::new(Expr::Number(i as f64)),
                }),
            },
            then_branch: guarded,
            else_branch: None,
        });
    }
    out
}

/// Copy a case body into `out`, rewriting every `break` that binds to the
/// switch being desugared into `__sw_done = true`, and guarding every
/// statement that follows a potentially-breaking statement behind
/// `if (!__sw_done)`. Descends `if`/`try` (which don't capture `break`);
/// stops at nested loops, switches, and labeled statements (whose `break`
/// binds to themselves). Statements directly after a bare `break` are
/// unreachable and dropped.
fn guard_switch_breaks(stmts: &[Stmt], done_id: LocalId, out: &mut Vec<Stmt>) {
    let mut i = 0;
    while i < stmts.len() {
        let s = &stmts[i];
        if matches!(s, Stmt::Break) {
            out.push(Stmt::Expr(Expr::LocalSet(
                done_id,
                Box::new(Expr::Bool(true)),
            )));
            return;
        }
        let may_break = stmt_may_break_switch(s);
        out.push(rewrite_switch_breaks_in_stmt(s, done_id));
        i += 1;
        if may_break && i < stmts.len() {
            let mut rest = Vec::new();
            guard_switch_breaks(&stmts[i..], done_id, &mut rest);
            out.push(Stmt::If {
                condition: Expr::Unary {
                    op: UnaryOp::Not,
                    operand: Box::new(Expr::LocalGet(done_id)),
                },
                then_branch: rest,
                else_branch: None,
            });
            return;
        }
    }
}

/// Can executing this statement hit a `break` that binds to the switch
/// being desugared? Mirrors `guard_switch_breaks`'s descent scoping.
fn stmt_may_break_switch(s: &Stmt) -> bool {
    match s {
        Stmt::Break => true,
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            then_branch.iter().any(stmt_may_break_switch)
                || else_branch
                    .as_ref()
                    .is_some_and(|e| e.iter().any(stmt_may_break_switch))
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            body.iter().any(stmt_may_break_switch)
                || catch
                    .as_ref()
                    .is_some_and(|c| c.body.iter().any(stmt_may_break_switch))
                || finally
                    .as_ref()
                    .is_some_and(|f| f.iter().any(stmt_may_break_switch))
        }
        _ => false,
    }
}

/// Rebuild one statement with switch-binding `break`s rewritten (via
/// `guard_switch_breaks`) inside its `if`/`try` sub-bodies.
fn rewrite_switch_breaks_in_stmt(s: &Stmt, done_id: LocalId) -> Stmt {
    let guarded = |body: &[Stmt]| {
        let mut v = Vec::new();
        guard_switch_breaks(body, done_id, &mut v);
        v
    };
    match s {
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => Stmt::If {
            condition: condition.clone(),
            then_branch: guarded(then_branch),
            else_branch: else_branch.as_ref().map(|e| guarded(e)),
        },
        Stmt::Try {
            body,
            catch,
            finally,
        } => Stmt::Try {
            body: guarded(body),
            catch: catch.as_ref().map(|c| CatchClause {
                param: c.param.clone(),
                body: guarded(&c.body),
            }),
            finally: finally.as_ref().map(|f| guarded(f)),
        },
        other => other.clone(),
    }
}

/// Prefix every loop-level `continue` in `stmts` with copies of `prefix`
/// (#5933: a `for` loop's awaited update moves to the body end, and each
/// `continue` must still run it before re-entering the loop, preserving the
/// spec's continue → update → condition order). Descends `if`/`try` and
/// `switch` CASES (none of which capture `continue`); stops at nested loops
/// and labeled statements, whose `continue` binds to them, and never enters
/// closures (statement walk only).
pub fn prefix_loop_continues(stmts: &mut Vec<Stmt>, prefix: &[Stmt]) {
    let mut i = 0;
    while i < stmts.len() {
        match &mut stmts[i] {
            Stmt::Continue => {
                for (k, p) in prefix.iter().enumerate() {
                    stmts.insert(i + k, p.clone());
                }
                i += prefix.len() + 1;
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                prefix_loop_continues(then_branch, prefix);
                if let Some(eb) = else_branch {
                    prefix_loop_continues(eb, prefix);
                }
                i += 1;
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                prefix_loop_continues(body, prefix);
                if let Some(c) = catch {
                    prefix_loop_continues(&mut c.body, prefix);
                }
                if let Some(f) = finally {
                    prefix_loop_continues(f, prefix);
                }
                i += 1;
            }
            Stmt::Switch { cases, .. } => {
                for case in cases.iter_mut() {
                    prefix_loop_continues(&mut case.body, prefix);
                }
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }
}

/// Does `stmts` contain a loop-level `continue` nested inside a
/// `try`/`catch` that has a `finally` block? Such a `continue` is an abrupt
/// completion: the `finally` must run BEFORE the loop's update — and if the
/// `finally` itself completes abruptly (`return`/`throw`), the update must
/// not run at all. `prefix_loop_continues` would insert the update BEFORE
/// the `finally`, so callers moving an awaited/yielded `for`-update into the
/// body bail back to the previous lowering when this shape is present
/// (#5933 review). A `continue` inside the `finally` block itself is fine —
/// the `finally` has already run at that point — as is any `continue` under
/// a `try` without `finally`.
pub fn stmts_have_continue_inside_try_finally(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| match s {
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            stmts_have_continue_inside_try_finally(then_branch)
                || else_branch
                    .as_ref()
                    .is_some_and(|e| stmts_have_continue_inside_try_finally(e))
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            if finally.is_some()
                && (stmts_have_loop_level_continue(body)
                    || catch
                        .as_ref()
                        .is_some_and(|c| stmts_have_loop_level_continue(&c.body)))
            {
                return true;
            }
            stmts_have_continue_inside_try_finally(body)
                || catch
                    .as_ref()
                    .is_some_and(|c| stmts_have_continue_inside_try_finally(&c.body))
                || finally
                    .as_ref()
                    .is_some_and(|f| stmts_have_continue_inside_try_finally(f))
        }
        Stmt::Switch { cases, .. } => cases
            .iter()
            .any(|c| stmts_have_continue_inside_try_finally(&c.body)),
        _ => false,
    })
}
