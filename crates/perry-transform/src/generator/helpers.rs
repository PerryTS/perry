//! Small utility helpers: hoisted let rewriting, local-id allocation, iter-result and Promise wrapping.

use super::*;

/// Recursively rewrite `Stmt::Let { id, init: Some(...) }` to
/// `Stmt::Expr(LocalSet(id, init))` for any id in `hoisted_ids`. Walks
/// into nested control-flow (For init/body, While body, If branches,
/// Try body/catch/finally, Switch case bodies, Labeled body) so a Let
/// nested inside a for-of's desugared loop body still gets routed
/// through the captured box. Issue #256.
pub fn rewrite_hoisted_lets_in_stmts(
    stmts: &mut [Stmt],
    hoisted_ids: &std::collections::HashSet<LocalId>,
) {
    for stmt in stmts.iter_mut() {
        rewrite_hoisted_lets_in_stmt(stmt, hoisted_ids);
    }
}

pub fn rewrite_hoisted_lets_in_stmt(
    stmt: &mut Stmt,
    hoisted_ids: &std::collections::HashSet<LocalId>,
) {
    if let Stmt::Let {
        id,
        init: Some(init_expr),
        ..
    } = stmt
    {
        if hoisted_ids.contains(id) {
            *stmt = Stmt::Expr(Expr::LocalSet(*id, Box::new(init_expr.clone())));
            return;
        }
    }
    match stmt {
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_hoisted_lets_in_stmts(then_branch, hoisted_ids);
            if let Some(eb) = else_branch {
                rewrite_hoisted_lets_in_stmts(eb, hoisted_ids);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            rewrite_hoisted_lets_in_stmts(body, hoisted_ids);
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                rewrite_hoisted_lets_in_stmt(i, hoisted_ids);
            }
            rewrite_hoisted_lets_in_stmts(body, hoisted_ids);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            rewrite_hoisted_lets_in_stmts(body, hoisted_ids);
            if let Some(c) = catch {
                rewrite_hoisted_lets_in_stmts(&mut c.body, hoisted_ids);
            }
            if let Some(f) = finally {
                rewrite_hoisted_lets_in_stmts(f, hoisted_ids);
            }
        }
        Stmt::Switch { cases, .. } => {
            for case in cases.iter_mut() {
                rewrite_hoisted_lets_in_stmts(&mut case.body, hoisted_ids);
            }
        }
        Stmt::Labeled { body, .. } => {
            rewrite_hoisted_lets_in_stmt(body, hoisted_ids);
        }
        _ => {}
    }
}

pub fn alloc_local(next_id: &mut u32) -> LocalId {
    let id = *next_id;
    *next_id += 1;
    id
}

/// Create an iterator result object: { value: expr, done: bool }
pub fn make_iter_result(value: Expr, done: bool) -> Expr {
    Expr::Object(vec![
        ("value".to_string(), value),
        ("done".to_string(), Expr::Bool(done)),
    ])
}

/// Wrap any expression in `Promise.resolve(expr)`. Used by async
/// generators so `gen.next()` returns a Promise the caller can
/// `await`, matching JS async-iterator semantics.
///
/// We build the same HIR shape that `Promise.resolve(x)` sourced
/// from user code would produce (`Call { callee: PropertyGet {
/// GlobalGet(0), "resolve" }, args: [x] }`), which the codegen
/// already recognizes and lowers via `js_promise_resolved`.
pub fn wrap_in_promise_resolve(value: Expr) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            byte_offset: 0,
            object: Box::new(Expr::GlobalGet(0)),
            property: "resolve".to_string(),
        }),
        args: vec![value],
        type_args: vec![],
        byte_offset: 0,
    }
}

/// Walk a statement list and wrap every `Stmt::Return(Some(v))`
/// in `Promise.resolve(v)`. Recurses through If/While/For/Try/Switch
/// bodies so nested returns inside the state-machine's if-chain are
/// all covered. Used on `.next()` / `.return()` / `.throw()` closure
/// bodies of async generators.
pub fn wrap_returns_in_promise(stmts: &mut Vec<Stmt>) {
    for stmt in stmts.iter_mut() {
        match stmt {
            Stmt::Return(Some(expr)) => {
                let inner = std::mem::replace(expr, Expr::Undefined);
                *expr = wrap_in_promise_resolve(inner);
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                wrap_returns_in_promise(then_branch);
                if let Some(eb) = else_branch {
                    wrap_returns_in_promise(eb);
                }
            }
            Stmt::While { body, .. } => wrap_returns_in_promise(body),
            Stmt::DoWhile { body, .. } => wrap_returns_in_promise(body),
            Stmt::For { body, .. } => wrap_returns_in_promise(body),
            Stmt::Labeled { body, .. } => {
                // Box<Stmt> — recurse over a single-element slice.
                let mut v = vec![std::mem::replace(body.as_mut(), Stmt::Break)];
                wrap_returns_in_promise(&mut v);
                **body = v.into_iter().next().unwrap();
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                wrap_returns_in_promise(body);
                if let Some(c) = catch {
                    wrap_returns_in_promise(&mut c.body);
                }
                if let Some(f) = finally {
                    wrap_returns_in_promise(f);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases.iter_mut() {
                    wrap_returns_in_promise(&mut case.body);
                }
            }
            _ => {}
        }
    }
}
