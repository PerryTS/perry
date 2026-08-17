use crate::ir::{Expr, Module, Stmt};
use crate::walker::walk_expr_children;

/// Scan `module.init` for an `await` expression outside any function/
/// closure body and set `module.has_top_level_await` accordingly.
///
/// Idempotent — safe to call multiple times. Closure bodies are NOT
/// descended into because awaits inside them belong to the closure's
/// own async scope, not the module's top level.
pub fn detect_top_level_await(module: &mut Module) {
    module.has_top_level_await = module.init.iter().any(stmt_has_top_level_await);
}

fn stmt_has_top_level_await(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Let { init, .. } => init.as_ref().is_some_and(expr_has_top_level_await),
        Stmt::Expr(e) => expr_has_top_level_await(e),
        Stmt::Return(opt) => opt.as_ref().is_some_and(expr_has_top_level_await),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expr_has_top_level_await(condition)
                || then_branch.iter().any(stmt_has_top_level_await)
                || else_branch
                    .as_ref()
                    .is_some_and(|b| b.iter().any(stmt_has_top_level_await))
        }
        Stmt::While { condition, body } => {
            expr_has_top_level_await(condition) || body.iter().any(stmt_has_top_level_await)
        }
        Stmt::DoWhile { body, condition } => {
            body.iter().any(stmt_has_top_level_await) || expr_has_top_level_await(condition)
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_deref().is_some_and(stmt_has_top_level_await)
                || condition.as_ref().is_some_and(expr_has_top_level_await)
                || update.as_ref().is_some_and(expr_has_top_level_await)
                || body.iter().any(stmt_has_top_level_await)
        }
        Stmt::Labeled { body, .. } => stmt_has_top_level_await(body),
        Stmt::Throw(e) => expr_has_top_level_await(e),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            body.iter().any(stmt_has_top_level_await)
                || catch
                    .as_ref()
                    .is_some_and(|c| c.body.iter().any(stmt_has_top_level_await))
                || finally
                    .as_ref()
                    .is_some_and(|f| f.iter().any(stmt_has_top_level_await))
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            expr_has_top_level_await(discriminant)
                || cases.iter().any(|c| {
                    c.test.as_ref().is_some_and(expr_has_top_level_await)
                        || c.body.iter().any(stmt_has_top_level_await)
                })
        }
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_)
        | Stmt::ReleaseBoxes(_) => false,
    }
}

fn expr_has_top_level_await(expr: &Expr) -> bool {
    if matches!(expr, Expr::Await(_)) {
        return true;
    }
    let mut found = false;
    walk_expr_children(expr, &mut |child| {
        if !found && expr_has_top_level_await(child) {
            found = true;
        }
    });
    found
}
