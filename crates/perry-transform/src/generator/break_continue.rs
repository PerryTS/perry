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
/// nested loop / switch / closure boundaries — their own break/continue
/// belong to those constructs, not to us.
pub fn rewrite_break_continue_in_stmts(stmts: &mut Vec<Stmt>, state_id: LocalId) {
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
            mut other => {
                rewrite_break_continue_in_stmt(&mut other, state_id);
                stmts[i] = other;
                i += 1;
            }
        }
    }
}

pub fn rewrite_break_continue_in_stmt(stmt: &mut Stmt, state_id: LocalId) {
    match stmt {
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_break_continue_in_stmts(then_branch, state_id);
            if let Some(eb) = else_branch.as_mut() {
                rewrite_break_continue_in_stmts(eb, state_id);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            rewrite_break_continue_in_stmts(body, state_id);
            if let Some(c) = catch.as_mut() {
                rewrite_break_continue_in_stmts(&mut c.body, state_id);
            }
            if let Some(f) = finally.as_mut() {
                rewrite_break_continue_in_stmts(f, state_id);
            }
        }
        // Inside nested loops / switch / labeled / closure expressions, the
        // user's `break`/`continue` belongs to that construct and not to the
        // outer loop the state machine is unrolling. Leave them as-is so the
        // inner linearize_body (if it yields) / regular codegen (if it
        // doesn't) handles them.
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
            Stmt::While { body, .. } => {
                if body_contains_yield(body) {
                    return true;
                }
            }
            // A yield buried in a do-while or labeled loop must still be seen
            // by the enclosing construct's linearization (#1824), otherwise it
            // is never split into resume states.
            Stmt::DoWhile { body, .. } => {
                if body_contains_yield(body) {
                    return true;
                }
            }
            Stmt::Labeled { body, .. } => {
                if body_contains_yield(std::slice::from_ref(&**body)) {
                    return true;
                }
            }
            Stmt::For { body, .. } => {
                if body_contains_yield(body) {
                    return true;
                }
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
