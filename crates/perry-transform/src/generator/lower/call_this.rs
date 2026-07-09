//! Generator `this`-capture analysis helpers, split out of `lower.rs`.
//! These determine whether a generator body reads `this`/`super`, so the
//! transform knows to capture the receiver into the synthesized step closures.

use super::*;
use perry_hir::walker::walk_expr_children;

pub(crate) fn generator_body_uses_call_this(body: &[Stmt]) -> bool {
    body.iter().any(generator_stmt_uses_call_this)
}

pub(crate) fn generator_stmt_uses_call_this(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Let {
            init: Some(expr), ..
        } => generator_expr_uses_call_this(expr),
        Stmt::Let { init: None, .. } => false,
        Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
            generator_expr_uses_call_this(expr)
        }
        Stmt::Return(None)
        | Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_) => false,
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            generator_expr_uses_call_this(condition)
                || then_branch.iter().any(generator_stmt_uses_call_this)
                || else_branch
                    .as_ref()
                    .is_some_and(|body| body.iter().any(generator_stmt_uses_call_this))
        }
        Stmt::While { condition, body } => {
            generator_expr_uses_call_this(condition)
                || body.iter().any(generator_stmt_uses_call_this)
        }
        Stmt::DoWhile { body, condition } => {
            body.iter().any(generator_stmt_uses_call_this)
                || generator_expr_uses_call_this(condition)
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_ref()
                .is_some_and(|stmt| generator_stmt_uses_call_this(stmt))
                || condition
                    .as_ref()
                    .is_some_and(generator_expr_uses_call_this)
                || update.as_ref().is_some_and(generator_expr_uses_call_this)
                || body.iter().any(generator_stmt_uses_call_this)
        }
        Stmt::Labeled { body, .. } => generator_stmt_uses_call_this(body),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            body.iter().any(generator_stmt_uses_call_this)
                || catch
                    .as_ref()
                    .is_some_and(|catch| catch.body.iter().any(generator_stmt_uses_call_this))
                || finally
                    .as_ref()
                    .is_some_and(|body| body.iter().any(generator_stmt_uses_call_this))
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            generator_expr_uses_call_this(discriminant)
                || cases.iter().any(|case| {
                    case.test
                        .as_ref()
                        .is_some_and(generator_expr_uses_call_this)
                        || case.body.iter().any(generator_stmt_uses_call_this)
                })
        }
    }
}

pub(crate) fn generator_expr_uses_call_this(expr: &Expr) -> bool {
    match expr {
        Expr::This
        | Expr::SuperCall(_)
        | Expr::SuperMethodCall { .. }
        | Expr::SuperPropertyGet { .. } => true,
        Expr::Closure { captures_this, .. } => *captures_this,
        _ => {
            let mut found = false;
            walk_expr_children(expr, &mut |child| {
                if !found && generator_expr_uses_call_this(child) {
                    found = true;
                }
            });
            found
        }
    }
}
