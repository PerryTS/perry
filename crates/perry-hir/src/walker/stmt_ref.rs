//! Exhaustive statement descent for expression predicates.

use crate::ir::Expr;
use crate::ir::Stmt;

/// Test expressions in a statement and its nested statements. The predicate
/// controls expression descent, including whether to enter nested closures.
/// New statement variants must explicitly declare their children here.
pub fn stmt_any_expr(stmt: &Stmt, predicate: &mut impl FnMut(&Expr) -> bool) -> bool {
    match stmt {
        Stmt::Let { init, .. } | Stmt::Return(init) => init.as_ref().is_some_and(&mut *predicate),
        Stmt::Expr(expr) | Stmt::Throw(expr) => predicate(expr),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            predicate(condition)
                || then_branch.iter().any(|s| stmt_any_expr(s, predicate))
                || else_branch
                    .as_ref()
                    .is_some_and(|body| body.iter().any(|s| stmt_any_expr(s, predicate)))
        }
        Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
            predicate(condition) || body.iter().any(|s| stmt_any_expr(s, predicate))
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_ref().is_some_and(|s| stmt_any_expr(s, predicate))
                || condition.as_ref().is_some_and(&mut *predicate)
                || update.as_ref().is_some_and(&mut *predicate)
                || body.iter().any(|s| stmt_any_expr(s, predicate))
        }
        Stmt::Labeled { body, .. } => stmt_any_expr(body, predicate),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            body.iter().any(|s| stmt_any_expr(s, predicate))
                || catch
                    .as_ref()
                    .is_some_and(|c| c.body.iter().any(|s| stmt_any_expr(s, predicate)))
                || finally
                    .as_ref()
                    .is_some_and(|body| body.iter().any(|s| stmt_any_expr(s, predicate)))
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            predicate(discriminant)
                || cases.iter().any(|c| {
                    c.test.as_ref().is_some_and(&mut *predicate)
                        || c.body.iter().any(|s| stmt_any_expr(s, predicate))
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
