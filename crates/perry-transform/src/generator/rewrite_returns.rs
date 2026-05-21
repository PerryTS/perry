//! Return / yield-to-await / iter-result post-linearization rewriting passes.

use super::*;

/// Rewrite Return(Some(expr)) to Return(Some({value: expr, done: true})).
/// Recurses through If/While/DoWhile/For/Try/Switch/Labeled bodies so a
/// user `return X` nested inside any control flow inside a state's body
/// is wrapped — without this, `return X` inside an `if` then-branch
/// inside the post-await tail of a state slipped through and the bare
/// value got returned to the runtime's `__step_r.done` access (#594).
pub fn rewrite_returns_as_done(stmts: &mut Vec<Stmt>) {
    for stmt in stmts.iter_mut() {
        match stmt {
            Stmt::Return(Some(expr)) => {
                // Don't double-wrap if already an iter result
                if !is_iter_result(expr) {
                    let val = expr.clone();
                    *expr = make_iter_result(val, true);
                }
            }
            Stmt::Return(None) => {
                *stmt = Stmt::Return(Some(make_iter_result(Expr::Undefined, true)));
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                rewrite_returns_as_done(then_branch);
                if let Some(eb) = else_branch {
                    rewrite_returns_as_done(eb);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
                rewrite_returns_as_done(body);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                rewrite_returns_as_done(body);
                if let Some(c) = catch {
                    rewrite_returns_as_done(&mut c.body);
                }
                if let Some(f) = finally {
                    rewrite_returns_as_done(f);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases.iter_mut() {
                    rewrite_returns_as_done(&mut case.body);
                }
            }
            Stmt::Labeled { body, .. } => {
                let mut v = vec![std::mem::replace(body.as_mut(), Stmt::Break)];
                rewrite_returns_as_done(&mut v);
                **body = v.into_iter().next().unwrap();
            }
            _ => {}
        }
    }
}

/// Walk `stmts` and prepend `LocalSet(done_id, true)` immediately before
/// every `Stmt::Return` — at any depth. Mirrors `rewrite_returns_as_done`
/// but inserts the done-flag set instead of rewriting the return value.
/// Without this, a user `return X` inside an `if`/`try`/etc. nested
/// block of a state body left `__gen_done` false, so the next call to
/// `gen.next()` re-entered the state machine and ran the same state
/// again — surfacing as the iterator producing the user's return value
/// then hanging or producing `undefined` on subsequent ticks. (#594.)
pub fn prepend_done_before_returns(stmts: &mut Vec<Stmt>, done_id: u32) {
    let mut new_body: Vec<Stmt> = Vec::with_capacity(stmts.len());
    for s in stmts.drain(..) {
        let mut s = s;
        // First recurse so done-flag set lands at the deepest enclosing
        // statement of each nested return.
        match &mut s {
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                prepend_done_before_returns(then_branch, done_id);
                if let Some(eb) = else_branch {
                    prepend_done_before_returns(eb, done_id);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
                prepend_done_before_returns(body, done_id);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                prepend_done_before_returns(body, done_id);
                if let Some(c) = catch {
                    prepend_done_before_returns(&mut c.body, done_id);
                }
                if let Some(f) = finally {
                    prepend_done_before_returns(f, done_id);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases.iter_mut() {
                    prepend_done_before_returns(&mut case.body, done_id);
                }
            }
            Stmt::Labeled { body, .. } => {
                let mut v = vec![std::mem::replace(body.as_mut(), Stmt::Break)];
                prepend_done_before_returns(&mut v, done_id);
                **body = v.into_iter().next().unwrap();
            }
            _ => {}
        }
        if matches!(s, Stmt::Return(_)) {
            new_body.push(Stmt::Expr(Expr::LocalSet(
                done_id,
                Box::new(Expr::Bool(true)),
            )));
        }
        new_body.push(s);
    }
    *stmts = new_body;
}

/// True iff any nested statement is a `Stmt::Return(_)` at any depth
/// (recurses through If/While/DoWhile/For/Try/Switch/Labeled). Used by
/// the StateExit::Done arm to decide whether the state body already
/// contains a user-level return that needs rewriting (#594).
pub fn body_contains_return(stmts: &[Stmt]) -> bool {
    for s in stmts {
        match s {
            Stmt::Return(_) => return true,
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                if body_contains_return(then_branch) {
                    return true;
                }
                if let Some(eb) = else_branch {
                    if body_contains_return(eb) {
                        return true;
                    }
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
                if body_contains_return(body) {
                    return true;
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                if body_contains_return(body) {
                    return true;
                }
                if let Some(c) = catch {
                    if body_contains_return(&c.body) {
                        return true;
                    }
                }
                if let Some(f) = finally {
                    if body_contains_return(f) {
                        return true;
                    }
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    if body_contains_return(&case.body) {
                        return true;
                    }
                }
            }
            Stmt::Labeled { body, .. } => {
                if body_contains_return(std::slice::from_ref(body.as_ref())) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// Check if an expression is already an iterator result object
pub fn is_iter_result(expr: &Expr) -> bool {
    if let Expr::Object(props) = expr {
        props.len() == 2
            && props.iter().any(|(k, _)| k == "value")
            && props.iter().any(|(k, _)| k == "done")
    } else {
        false
    }
}

/// Walk an expression / statement tree and replace every
/// `Expr::Object({value: V, done: D})` with `Expr::IterResultSet(V, D)`.
/// Recurses through Closure bodies, blocks, control-flow, etc. — needs
/// to descend into the nested generator state-machine closures the
/// generator transform just synthesized.
///
/// Bool literals (`Expr::Bool(true)` / `Expr::Bool(false)`) and number-1
/// patterns are recognized for the `done` half. Other shapes (computed
/// done) fall through to the original object alloc — they're not on
/// the hot path.
pub fn rewrite_iter_results_to_scratch(expr: &mut Expr) {
    rewrite_expr(expr);
}

/// Rewrite every `Expr::Yield { value, .. }` to `Expr::Await(value)` recursively
/// through statement and expression trees. Does NOT descend into nested closures
/// (their await/yield context is independent).
///
/// Used by the async-step `__async_throw` builder: the global await→yield
/// rewrite in `transform_async_to_generator` runs on the whole function body
/// indiscriminately, but catch bodies aren't lifted into state-machine states —
/// they're inlined into a regular sync closure where yield codegens to
/// `double_literal(0.0)`. Flipping the yields back to awaits restores the
/// busy-wait await semantics for awaits inside catch handlers.
pub fn rewrite_yield_to_await_in_stmts(stmts: &mut Vec<Stmt>) {
    for s in stmts.iter_mut() {
        rewrite_yield_to_await_in_stmt(s);
    }
}

pub fn rewrite_yield_to_await_in_stmt(stmt: &mut Stmt) {
    match stmt {
        Stmt::Expr(e) | Stmt::Throw(e) => rewrite_yield_to_await_in_expr(e),
        Stmt::Let { init: Some(e), .. } => rewrite_yield_to_await_in_expr(e),
        Stmt::Return(Some(e)) => rewrite_yield_to_await_in_expr(e),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            rewrite_yield_to_await_in_expr(condition);
            rewrite_yield_to_await_in_stmts(then_branch);
            if let Some(eb) = else_branch.as_mut() {
                rewrite_yield_to_await_in_stmts(eb);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            rewrite_yield_to_await_in_expr(condition);
            rewrite_yield_to_await_in_stmts(body);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init.as_mut() {
                rewrite_yield_to_await_in_stmt(i.as_mut());
            }
            if let Some(c) = condition.as_mut() {
                rewrite_yield_to_await_in_expr(c);
            }
            if let Some(u) = update.as_mut() {
                rewrite_yield_to_await_in_expr(u);
            }
            rewrite_yield_to_await_in_stmts(body);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            rewrite_yield_to_await_in_stmts(body);
            if let Some(c) = catch.as_mut() {
                rewrite_yield_to_await_in_stmts(&mut c.body);
            }
            if let Some(f) = finally.as_mut() {
                rewrite_yield_to_await_in_stmts(f);
            }
        }
        Stmt::Switch { cases, .. } => {
            for case in cases.iter_mut() {
                rewrite_yield_to_await_in_stmts(&mut case.body);
            }
        }
        Stmt::Labeled { body, .. } => {
            rewrite_yield_to_await_in_stmt(body.as_mut());
        }
        _ => {}
    }
}

pub fn rewrite_yield_to_await_in_expr(expr: &mut Expr) {
    if let Expr::Yield { value, .. } = expr {
        let inner = value.take().map(|b| *b).unwrap_or(Expr::Undefined);
        let mut new_inner = inner;
        rewrite_yield_to_await_in_expr(&mut new_inner);
        *expr = Expr::Await(Box::new(new_inner));
        return;
    }
    // Recurse into child expressions but stop at Closure boundaries —
    // a nested closure has its own await/yield context.
    rewrite_yield_to_await_in_expr_children(expr);
}

pub fn rewrite_yield_to_await_in_expr_children(expr: &mut Expr) {
    use perry_hir::walker::walk_expr_children_mut;
    // Skip Closure bodies — their inner statements have their own
    // independent semantics (already-rewritten or not as their own
    // closure-level pass dictates).
    if matches!(expr, Expr::Closure { .. }) {
        return;
    }
    walk_expr_children_mut(expr, &mut |child: &mut Expr| {
        rewrite_yield_to_await_in_expr(child);
    });
}

/// Rewrite every `Stmt::Return(Some(X))` to `Stmt::Return(Some(make_iter_result(X, true)))`
/// and `Stmt::Return(None)` to `Stmt::Return(Some(make_iter_result(Undefined, true)))`.
/// Recurses through If/While/DoWhile/For/Try/Switch/Labeled control flow but
/// does NOT descend into nested closures — a `return` inside an inner closure
/// belongs to that closure, not to the catch body.
///
/// Used by the async-step `__async_throw` builder: a user-source `return X`
/// inside a `catch (e) { ... }` block must propagate to the function's
/// returned Promise (resolved with X), not silently exit `__async_throw`
/// while leaving the scratch iter-result slots stale.
pub fn rewrite_catch_returns_to_iter_result(stmts: &mut Vec<Stmt>) {
    for stmt in stmts.iter_mut() {
        rewrite_catch_returns_to_iter_result_in_stmt(stmt);
    }
}

pub fn rewrite_catch_returns_to_iter_result_in_stmt(stmt: &mut Stmt) {
    match stmt {
        Stmt::Return(opt) => {
            let value = opt.take().unwrap_or(Expr::Undefined);
            *opt = Some(make_iter_result(value, true));
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_catch_returns_to_iter_result(then_branch);
            if let Some(eb) = else_branch.as_mut() {
                rewrite_catch_returns_to_iter_result(eb);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            rewrite_catch_returns_to_iter_result(body);
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init.as_mut() {
                rewrite_catch_returns_to_iter_result_in_stmt(i.as_mut());
            }
            rewrite_catch_returns_to_iter_result(body);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            rewrite_catch_returns_to_iter_result(body);
            if let Some(c) = catch.as_mut() {
                rewrite_catch_returns_to_iter_result(&mut c.body);
            }
            if let Some(f) = finally.as_mut() {
                rewrite_catch_returns_to_iter_result(f);
            }
        }
        Stmt::Switch { cases, .. } => {
            for case in cases.iter_mut() {
                rewrite_catch_returns_to_iter_result(&mut case.body);
            }
        }
        Stmt::Labeled { body, .. } => {
            rewrite_catch_returns_to_iter_result_in_stmt(body.as_mut());
        }
        _ => {}
    }
}

/// Apply iter-result-to-scratch rewrites directly to a statement list
/// (rather than to a Closure expression). Used by the fused async-step
/// driver, where next_body is inlined into step's body instead of being
/// wrapped in a separate closure.
pub fn rewrite_iter_results_in_stmts(stmts: &mut Vec<Stmt>) {
    for s in stmts.iter_mut() {
        rewrite_stmt(s);
    }
}

/// Rewrite every `Stmt::Return` inside `stmts` (recursing through
/// nested control flow) to `Expr(value) + LabeledBreak(label)` — or
/// just `LabeledBreak(label)` for `Return(None)`. Used by the fused
/// async-step driver: the inlined next_body's iter-result returns
/// (which used to exit the next closure) now break out to the
/// `__step_done` label so step's post-dispatch code (IterResultGetDone
/// check + AsyncStepChain) runs instead of step itself returning early.
///
/// Does NOT descend into nested closures — those Returns belong to
/// the closures themselves, not to the outer step body.
pub fn rewrite_returns_to_labeled_break(stmts: &mut Vec<Stmt>, label: &str) {
    let mut i = 0;
    while i < stmts.len() {
        let stmt = std::mem::replace(&mut stmts[i], Stmt::Continue);
        match stmt {
            Stmt::Return(Some(e)) => {
                stmts[i] = Stmt::Expr(e);
                stmts.insert(i + 1, Stmt::LabeledBreak(label.to_string()));
                i += 2;
            }
            Stmt::Return(None) => {
                stmts[i] = Stmt::LabeledBreak(label.to_string());
                i += 1;
            }
            mut other => {
                rewrite_returns_to_labeled_break_in_stmt(&mut other, label);
                stmts[i] = other;
                i += 1;
            }
        }
    }
}

pub fn rewrite_returns_to_labeled_break_in_stmt(stmt: &mut Stmt, label: &str) {
    match stmt {
        Stmt::Return(_) => {
            unreachable!("rewrite_returns_to_labeled_break_in_stmt should not see a bare Return");
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_returns_to_labeled_break(then_branch, label);
            if let Some(eb) = else_branch.as_mut() {
                rewrite_returns_to_labeled_break(eb, label);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            rewrite_returns_to_labeled_break(body, label);
        }
        Stmt::For { init, body, .. } => {
            if let Some(init_stmt) = init.as_mut() {
                rewrite_returns_to_labeled_break_in_stmt(init_stmt.as_mut(), label);
            }
            rewrite_returns_to_labeled_break(body, label);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            rewrite_returns_to_labeled_break(body, label);
            if let Some(c) = catch.as_mut() {
                rewrite_returns_to_labeled_break(&mut c.body, label);
            }
            if let Some(f) = finally.as_mut() {
                rewrite_returns_to_labeled_break(f, label);
            }
        }
        Stmt::Switch { cases, .. } => {
            for c in cases.iter_mut() {
                rewrite_returns_to_labeled_break(&mut c.body, label);
            }
        }
        Stmt::Labeled { body, .. } => {
            rewrite_returns_to_labeled_break_in_stmt(body.as_mut(), label);
        }
        _ => {}
    }
}
