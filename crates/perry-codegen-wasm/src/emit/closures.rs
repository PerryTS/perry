//! Closure collection: `collect_closures_from_stmts` / `collect_closures_from_expr`.

use perry_hir::ir::*;
use perry_hir::types::{FuncId, LocalId};
use perry_hir::walker::walk_expr_children;

/// A collected closure: its id, params, body, captures, mutable captures, and
/// whether it is an arrow (which shares its enclosing method's `this` and `super`).
pub(super) type CollectedClosure = (
    FuncId,
    Vec<Param>,
    Vec<Stmt>,
    Vec<LocalId>,
    Vec<LocalId>,
    bool,
);

/// Recursively collect all Expr::Closure nodes from statements
pub(super) fn collect_closures_from_stmts(stmts: &[Stmt], out: &mut Vec<CollectedClosure>) {
    for stmt in stmts {
        match stmt {
            Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    collect_closures_from_expr(e, out);
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) => collect_closures_from_expr(e, out),
            Stmt::Return(e) => {
                if let Some(e) = e {
                    collect_closures_from_expr(e, out);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                collect_closures_from_expr(condition, out);
                collect_closures_from_stmts(then_branch, out);
                if let Some(eb) = else_branch {
                    collect_closures_from_stmts(eb, out);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
                collect_closures_from_expr(condition, out);
                collect_closures_from_stmts(body, out);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    collect_closures_from_stmts(std::slice::from_ref(i.as_ref()), out);
                }
                if let Some(c) = condition {
                    collect_closures_from_expr(c, out);
                }
                if let Some(u) = update {
                    collect_closures_from_expr(u, out);
                }
                collect_closures_from_stmts(body, out);
            }
            Stmt::Labeled { body, .. } => {
                collect_closures_from_stmts(std::slice::from_ref(body.as_ref()), out);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_closures_from_stmts(body, out);
                if let Some(c) = catch {
                    collect_closures_from_stmts(&c.body, out);
                }
                if let Some(f) = finally {
                    collect_closures_from_stmts(f, out);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                collect_closures_from_expr(discriminant, out);
                for case in cases {
                    if let Some(t) = &case.test {
                        collect_closures_from_expr(t, out);
                    }
                    collect_closures_from_stmts(&case.body, out);
                }
            }
            _ => {}
        }
    }
}

/// Collect every `Expr::Closure` under `expr`. Descends through `walk_expr_children`,
/// so a closure under any variant is registered; an unregistered one emits `undefined`.
pub(super) fn collect_closures_from_expr(expr: &Expr, out: &mut Vec<CollectedClosure>) {
    if let Expr::Closure {
        func_id,
        params,
        body,
        captures,
        mutable_captures,
        is_arrow,
        ..
    } = expr
    {
        out.push((
            *func_id,
            params.clone(),
            body.clone(),
            captures.clone(),
            mutable_captures.clone(),
            *is_arrow,
        ));
    }
    // A closure's children are its parameter defaults; its body is statements.
    walk_expr_children(expr, &mut |child| collect_closures_from_expr(child, out));
    if let Expr::Closure { body, .. } = expr {
        collect_closures_from_stmts(body, out);
    }
}

/// The captures of the sync generator resume record `body` returns, if it is a
/// lowered generator. The record writes its control slots and the runtime driver
/// reads them between calls, so every one of them lives in a cell.
pub(super) fn generator_record_captures(body: &[Stmt]) -> &[LocalId] {
    match body.last() {
        Some(Stmt::Return(Some(Expr::LinkGeneratorPrototype {
            obj,
            is_async: false,
            fixed_state: true,
            ..
        }))) => match obj.as_ref() {
            Expr::Closure { captures, .. } => captures,
            _ => &[],
        },
        _ => &[],
    }
}
