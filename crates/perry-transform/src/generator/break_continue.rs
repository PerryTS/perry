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

/// Base for per-label `break <label>` sentinels. A labeled break/continue that
/// targets an OUTER loop from inside a NESTED loop/switch can't use the plain
/// `BREAK_SENTINEL`/`CONTINUE_SENTINEL`, because the nested loop's own
/// linearization (`fix_break_continue_sentinels`) would claim those and route
/// them to the WRONG (inner) loop's targets. Instead each label is assigned a
/// distinct index `i`; the labeled break uses `LABEL_BREAK_SENTINEL_BASE + i`
/// and the labeled continue `LABEL_CONTINUE_SENTINEL_BASE + i`. Only the
/// linearization of the loop that actually carries label `i` fixes these up
/// (`fix_label_sentinels_*`), so inner loops leave them untouched. Bases sit
/// far above any real state count (thousands) and above the plain sentinels.
const LABEL_BREAK_SENTINEL_BASE: f64 = 2_000_000.0;
const LABEL_CONTINUE_SENTINEL_BASE: f64 = 3_000_000.0;

/// Sentinel number for `break <label>` / `continue <label>` given the label's
/// assigned index.
pub fn label_break_sentinel(index: u32) -> f64 {
    LABEL_BREAK_SENTINEL_BASE + index as f64
}
pub fn label_continue_sentinel(index: u32) -> f64 {
    LABEL_CONTINUE_SENTINEL_BASE + index as f64
}

/// True if `n` is any of the synthesized dispatch sentinels (plain break/
/// continue or any per-label break/continue). Used to recognize a
/// `[LocalSet(state, <sentinel>), Stmt::Continue]` pair as a synthesized
/// dispatch re-entry rather than a user `continue`, so a later
/// `rewrite_break_continue_in_stmts` pass (run by an enclosing loop) leaves the
/// trailing dispatch `Stmt::Continue` alone instead of re-wrapping it.
fn is_dispatch_sentinel(n: f64) -> bool {
    n == BREAK_SENTINEL
        || n == CONTINUE_SENTINEL
        || n >= LABEL_BREAK_SENTINEL_BASE
}

/// Walk a body and rewrite every top-level `Stmt::Break` / `Stmt::Continue`
/// into `[LocalSet(state_id, <sentinel>), Stmt::Continue]`. The trailing
/// `Stmt::Continue` is the state-machine's dispatch-loop continue, which
/// re-enters the while(true) and re-dispatches on the new state. Stops at
/// nested loop / switch / closure boundaries — their own break/continue
/// belong to those constructs, not to us.
pub fn rewrite_break_continue_in_stmts(stmts: &mut Vec<Stmt>, state_id: LocalId) {
    let mut i = 0;
    while i < stmts.len() {
        // A previously-synthesized dispatch re-entry —
        // `[LocalSet(state, <sentinel>), Stmt::Continue]` emitted for an OUTER
        // loop's (possibly labeled) break/continue — must be left intact. The
        // trailing `Stmt::Continue` is the state-machine dispatch continue, NOT
        // a user `continue`; re-wrapping it here would corrupt the outer loop's
        // control flow. Skip both statements of the pair.
        if let Stmt::Expr(Expr::LocalSet(id, val)) = &stmts[i] {
            if *id == state_id {
                if let Expr::Number(n) = val.as_ref() {
                    if is_dispatch_sentinel(*n)
                        && matches!(stmts.get(i + 1), Some(Stmt::Continue))
                    {
                        i += 2;
                        continue;
                    }
                }
            }
        }
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

/// Rewrite every `break <label>` / `continue <label>` that targets `label`
/// (with `label_index`) into a per-label dispatch sentinel
/// `[LocalSet(state, label_break/continue_sentinel(index)), Stmt::Continue]`.
///
/// Unlike `rewrite_break_continue_in_stmts` (which handles the loop's OWN
/// plain break/continue and stops at nested loops/switch), this DESCENDS into
/// nested loops and switches — a `break outer` inside an inner loop still
/// targets the outer labeled loop. The per-label sentinel survives the inner
/// loop's own `fix_break_continue_sentinels` (different number) and is fixed up
/// only by the labeled loop's `fix_label_sentinels_*` once its target states
/// are known. Stops at:
///   - a nested `Stmt::Labeled` re-using the SAME name (shadows this label);
///   - closure expressions (a different function's break/continue).
/// A label-shadowing inner loop with the same name is not rewritten by THIS
/// call; the inner labeled arm handles its own.
pub fn rewrite_labeled_break_continue_in_vec(
    stmts: &mut Vec<Stmt>,
    label: &str,
    label_index: u32,
    state_id: LocalId,
) {
    let mut i = 0;
    while i < stmts.len() {
        match &stmts[i] {
            Stmt::LabeledBreak(l) if l == label => {
                stmts[i] = Stmt::Expr(Expr::LocalSet(
                    state_id,
                    Box::new(Expr::Number(label_break_sentinel(label_index))),
                ));
                stmts.insert(i + 1, Stmt::Continue);
                i += 2;
            }
            Stmt::LabeledContinue(l) if l == label => {
                stmts[i] = Stmt::Expr(Expr::LocalSet(
                    state_id,
                    Box::new(Expr::Number(label_continue_sentinel(label_index))),
                ));
                stmts.insert(i + 1, Stmt::Continue);
                i += 2;
            }
            _ => {
                rewrite_labeled_break_continue_in_stmt(
                    &mut stmts[i],
                    label,
                    label_index,
                    state_id,
                );
                i += 1;
            }
        }
    }
}

fn rewrite_labeled_break_continue_in_stmt(
    stmt: &mut Stmt,
    label: &str,
    label_index: u32,
    state_id: LocalId,
) {
    match stmt {
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_labeled_break_continue_in_vec(then_branch, label, label_index, state_id);
            if let Some(eb) = else_branch.as_mut() {
                rewrite_labeled_break_continue_in_vec(eb, label, label_index, state_id);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            rewrite_labeled_break_continue_in_vec(body, label, label_index, state_id);
            if let Some(c) = catch.as_mut() {
                rewrite_labeled_break_continue_in_vec(&mut c.body, label, label_index, state_id);
            }
            if let Some(f) = finally.as_mut() {
                rewrite_labeled_break_continue_in_vec(f, label, label_index, state_id);
            }
        }
        // Descend into nested loops / switch — a `break <label>` inside one of
        // them still targets the OUTER labeled loop.
        Stmt::For { body, .. } | Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            rewrite_labeled_break_continue_in_vec(body, label, label_index, state_id);
        }
        Stmt::Switch { cases, .. } => {
            for case in cases.iter_mut() {
                rewrite_labeled_break_continue_in_vec(&mut case.body, label, label_index, state_id);
            }
        }
        // A nested `Stmt::Labeled` with a DIFFERENT name still encloses a
        // `break <label>` that targets us — descend. A SAME-name label shadows
        // us; leave it for the inner labeled arm.
        Stmt::Labeled { label: inner, body } => {
            if inner != label {
                rewrite_labeled_break_continue_in_stmt(body.as_mut(), label, label_index, state_id);
            }
        }
        // Closures are a different function — their break/continue belong to
        // their own loops.
        _ => {}
    }
}

/// Fix a single label's break/continue sentinels in a slice of states. Called
/// by the labeled loop's For/While linearization once its `after_loop` /
/// `update`/`cond` target states are known. Mirrors
/// `fix_break_continue_sentinels` but keyed on the label's sentinel numbers.
pub fn fix_label_sentinels(
    states: &mut [State],
    state_id: LocalId,
    label_index: u32,
    break_target: u32,
    continue_target: u32,
) {
    for state in states.iter_mut() {
        fix_label_sentinels_in_stmts(
            &mut state.body,
            state_id,
            label_index,
            break_target,
            continue_target,
        );
    }
}

pub fn fix_label_sentinels_in_catches(
    catches: &mut [CatchRoute],
    state_id: LocalId,
    label_index: u32,
    break_target: u32,
    continue_target: u32,
) {
    for route in catches.iter_mut() {
        fix_label_sentinels_in_stmts(
            &mut route.body,
            state_id,
            label_index,
            break_target,
            continue_target,
        );
    }
}

pub fn fix_label_sentinels_in_stmts(
    stmts: &mut [Stmt],
    state_id: LocalId,
    label_index: u32,
    break_target: u32,
    continue_target: u32,
) {
    for stmt in stmts.iter_mut() {
        fix_label_sentinels_in_stmt(stmt, state_id, label_index, break_target, continue_target);
    }
}

pub fn fix_label_sentinels_in_stmt(
    stmt: &mut Stmt,
    state_id: LocalId,
    label_index: u32,
    break_target: u32,
    continue_target: u32,
) {
    let brk = label_break_sentinel(label_index);
    let cont = label_continue_sentinel(label_index);
    match stmt {
        Stmt::Expr(Expr::LocalSet(id, val)) if *id == state_id => {
            if let Expr::Number(n) = val.as_ref() {
                if *n == brk {
                    **val = Expr::Number(break_target as f64);
                } else if *n == cont {
                    **val = Expr::Number(continue_target as f64);
                }
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            fix_label_sentinels_in_stmts(
                then_branch,
                state_id,
                label_index,
                break_target,
                continue_target,
            );
            if let Some(eb) = else_branch.as_mut() {
                fix_label_sentinels_in_stmts(
                    eb,
                    state_id,
                    label_index,
                    break_target,
                    continue_target,
                );
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
            fix_label_sentinels_in_stmts(
                body,
                state_id,
                label_index,
                break_target,
                continue_target,
            );
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            fix_label_sentinels_in_stmts(
                body,
                state_id,
                label_index,
                break_target,
                continue_target,
            );
            if let Some(c) = catch.as_mut() {
                fix_label_sentinels_in_stmts(
                    &mut c.body,
                    state_id,
                    label_index,
                    break_target,
                    continue_target,
                );
            }
            if let Some(f) = finally.as_mut() {
                fix_label_sentinels_in_stmts(
                    f,
                    state_id,
                    label_index,
                    break_target,
                    continue_target,
                );
            }
        }
        Stmt::Switch { cases, .. } => {
            for case in cases.iter_mut() {
                fix_label_sentinels_in_stmts(
                    &mut case.body,
                    state_id,
                    label_index,
                    break_target,
                    continue_target,
                );
            }
        }
        Stmt::Labeled { body, .. } => {
            fix_label_sentinels_in_stmt(
                body.as_mut(),
                state_id,
                label_index,
                break_target,
                continue_target,
            );
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Extract the `Number` payload of a `Stmt::Expr(LocalSet(state_id, Number))`.
    fn localset_num(stmt: &Stmt, state_id: LocalId) -> Option<f64> {
        if let Stmt::Expr(Expr::LocalSet(id, val)) = stmt {
            if *id == state_id {
                if let Expr::Number(n) = val.as_ref() {
                    return Some(*n);
                }
            }
        }
        None
    }

    /// A `break <label>` / `continue <label>` nested inside an inner loop (and
    /// an `if`) must be rewritten into a per-label dispatch sentinel pair
    /// `[LocalSet(state, label_sentinel), Continue]`, then resolved by
    /// `fix_label_sentinels_in_stmts` to the labeled loop's real target states.
    #[test]
    fn labeled_break_continue_from_nested_loop_gets_label_sentinel_then_fixed() {
        let state_id: LocalId = 7;
        let label_index: u32 = 3;
        // q: { ... inner while { if(..) break q; if(..) continue q; } }
        let mut body = vec![Stmt::While {
            condition: Expr::Bool(true),
            body: vec![
                Stmt::If {
                    condition: Expr::Bool(true),
                    then_branch: vec![Stmt::LabeledBreak("q".to_string())],
                    else_branch: None,
                },
                Stmt::If {
                    condition: Expr::Bool(true),
                    then_branch: vec![Stmt::LabeledContinue("q".to_string())],
                    else_branch: None,
                },
            ],
        }];

        rewrite_labeled_break_continue_in_vec(&mut body, "q", label_index, state_id);

        // Descend to the inner while body.
        let Stmt::While { body: inner, .. } = &body[0] else {
            panic!("expected while");
        };
        // First if → break q → label break sentinel + dispatch continue.
        let Stmt::If { then_branch, .. } = &inner[0] else {
            panic!("expected if");
        };
        assert_eq!(then_branch.len(), 2, "break expands to sentinel + continue");
        assert_eq!(
            localset_num(&then_branch[0], state_id),
            Some(label_break_sentinel(label_index)),
        );
        assert!(matches!(then_branch[1], Stmt::Continue));
        // Second if → continue q → label continue sentinel + dispatch continue.
        let Stmt::If { then_branch, .. } = &inner[1] else {
            panic!("expected if");
        };
        assert_eq!(
            localset_num(&then_branch[0], state_id),
            Some(label_continue_sentinel(label_index)),
        );

        // Now resolve the sentinels to real target states.
        let after_loop = 42u32;
        let cond_state = 17u32;
        fix_label_sentinels_in_stmts(&mut body, state_id, label_index, after_loop, cond_state);

        let Stmt::While { body: inner, .. } = &body[0] else {
            panic!("expected while");
        };
        let Stmt::If { then_branch, .. } = &inner[0] else {
            panic!("expected if");
        };
        assert_eq!(
            localset_num(&then_branch[0], state_id),
            Some(after_loop as f64),
            "break <label> resolves to after-loop state",
        );
        let Stmt::If { then_branch, .. } = &inner[1] else {
            panic!("expected if");
        };
        assert_eq!(
            localset_num(&then_branch[0], state_id),
            Some(cond_state as f64),
            "continue <label> resolves to the loop's continue target",
        );
    }

    /// A previously-synthesized dispatch re-entry `[LocalSet(state, sentinel),
    /// Continue]` (e.g. for an OUTER labeled loop) must be left intact when an
    /// inner loop runs `rewrite_break_continue_in_stmts` over the same body —
    /// the trailing dispatch `Continue` must NOT be re-wrapped as a user
    /// `continue`.
    #[test]
    fn rewrite_break_continue_skips_synthesized_dispatch_reentry() {
        let state_id: LocalId = 5;
        let mut stmts = vec![
            Stmt::Expr(Expr::LocalSet(
                state_id,
                Box::new(Expr::Number(label_break_sentinel(2))),
            )),
            Stmt::Continue,
        ];
        rewrite_break_continue_in_stmts(&mut stmts, state_id);
        // Must be unchanged: still exactly the 2-statement pair, NOT expanded.
        assert_eq!(stmts.len(), 2, "dispatch re-entry must not be re-wrapped");
        assert_eq!(
            localset_num(&stmts[0], state_id),
            Some(label_break_sentinel(2)),
        );
        assert!(matches!(stmts[1], Stmt::Continue));
    }

    /// The plain-sentinel dispatch re-entry is likewise preserved.
    #[test]
    fn rewrite_break_continue_skips_plain_sentinel_reentry() {
        let state_id: LocalId = 9;
        let mut stmts = vec![
            Stmt::Expr(Expr::LocalSet(state_id, Box::new(Expr::Number(BREAK_SENTINEL)))),
            Stmt::Continue,
        ];
        rewrite_break_continue_in_stmts(&mut stmts, state_id);
        assert_eq!(stmts.len(), 2);
        assert_eq!(localset_num(&stmts[0], state_id), Some(BREAK_SENTINEL));
        assert!(matches!(stmts[1], Stmt::Continue));
    }
}
