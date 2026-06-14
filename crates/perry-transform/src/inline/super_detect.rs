//! Recursion guarding for expression inlining and lexical-`super` detection.
//!
//! These helpers keep `call_inliner` from recursing without bound when inlining
//! deeply-nested expressions, and detect whether a method body references a
//! lexical `super` set (which would be unsafe to inline onto a different
//! receiver).

use perry_hir::walker::walk_expr_children;
use perry_hir::{Expr, Function, Stmt};
use std::cell::Cell;

const MAX_INLINE_EXPR_RECURSION_DEPTH: usize = 128;

thread_local! {
    static INLINE_EXPR_RECURSION_DEPTH: Cell<usize> = const { Cell::new(0) };
}

pub(crate) struct InlineExprRecursionGuard;

impl Drop for InlineExprRecursionGuard {
    fn drop(&mut self) {
        INLINE_EXPR_RECURSION_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

pub(crate) fn enter_inline_expr_recursion() -> Option<InlineExprRecursionGuard> {
    let entered = INLINE_EXPR_RECURSION_DEPTH.with(|depth| {
        let current = depth.get();
        if current >= MAX_INLINE_EXPR_RECURSION_DEPTH {
            false
        } else {
            depth.set(current + 1);
            true
        }
    });
    entered.then_some(InlineExprRecursionGuard)
}

fn expr_contains_lexical_super_set(expr: &Expr) -> bool {
    if matches!(expr, Expr::SuperPropertySet { .. }) {
        return true;
    }
    let mut found = false;
    walk_expr_children(expr, &mut |child| {
        if !found && expr_contains_lexical_super_set(child) {
            found = true;
        }
    });
    found
}

fn stmt_contains_lexical_super_set(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Let { init, .. } => init.as_ref().is_some_and(expr_contains_lexical_super_set),
        Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
            expr_contains_lexical_super_set(expr)
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expr_contains_lexical_super_set(condition)
                || then_branch.iter().any(stmt_contains_lexical_super_set)
                || else_branch
                    .as_ref()
                    .is_some_and(|branch| branch.iter().any(stmt_contains_lexical_super_set))
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            expr_contains_lexical_super_set(condition)
                || body.iter().any(stmt_contains_lexical_super_set)
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_ref()
                .is_some_and(|stmt| stmt_contains_lexical_super_set(stmt.as_ref()))
                || condition
                    .as_ref()
                    .is_some_and(expr_contains_lexical_super_set)
                || update.as_ref().is_some_and(expr_contains_lexical_super_set)
                || body.iter().any(stmt_contains_lexical_super_set)
        }
        Stmt::Labeled { body, .. } => stmt_contains_lexical_super_set(body),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            body.iter().any(stmt_contains_lexical_super_set)
                || catch
                    .as_ref()
                    .is_some_and(|catch| catch.body.iter().any(stmt_contains_lexical_super_set))
                || finally
                    .as_ref()
                    .is_some_and(|body| body.iter().any(stmt_contains_lexical_super_set))
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            expr_contains_lexical_super_set(discriminant)
                || cases.iter().any(|case| {
                    case.test
                        .as_ref()
                        .is_some_and(expr_contains_lexical_super_set)
                        || case.body.iter().any(stmt_contains_lexical_super_set)
                })
        }
        Stmt::Return(None)
        | Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_) => false,
    }
}

pub(crate) fn method_contains_lexical_super_set(method: &Function) -> bool {
    method.body.iter().any(stmt_contains_lexical_super_set)
}
