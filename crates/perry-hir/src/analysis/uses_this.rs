//! `this`-usage analysis extracted from analysis.rs to keep that module
//! under the 2000-line file-size lint cap. See analysis.rs for the rest
//! of the HIR analysis helpers.

use crate::ir::*;
use crate::walker::walk_expr_children;

pub(crate) fn uses_this_expr(expr: &Expr) -> bool {
    match expr {
        Expr::This => true,
        // Nested arrow / closure: if it itself captures `this`, our
        // surrounding scope MUST also capture `this` so the nested closure
        // can inherit it (arrow functions inherit `this` from the enclosing
        // lexical scope). A closure with captures_this=false has no `this`
        // dependency and can be skipped.
        //
        // (function expressions also lower to Expr::Closure but always have
        // their own `this` binding; the HIR sets captures_this=false for
        // them in expr_function.rs:288, so this condition still does the
        // right thing for both forms.)
        Expr::Closure { captures_this, .. } => *captures_this,
        // Every other variant: descend into sub-expressions via the generic
        // walker so we never silently miss a specialized variant
        // (ArrayMap/ArrayFilter/ArrayForEach/etc., Math/Object/Map/Set fast
        // paths, …). Pre-fix this used a hand-rolled match that fell through
        // to `_ => false` for any variant it didn't enumerate; an arrow body
        // like `() => this.items.map(cb)` lowers to `Expr::ArrayMap` which
        // wasn't in the list, so `closure_uses_this` returned false,
        // `captures_this` stayed false, the closure's `class_stack` came up
        // empty, and `Expr::This` resolved to the 0.0 sentinel — making
        // `js_array_map` see a null receiver and return `[]`.
        _ => {
            let mut found = false;
            walk_expr_children(expr, &mut |child| {
                if !found && uses_this_expr(child) {
                    found = true;
                }
            });
            found
        }
    }
}

/// Check if a statement or its children use `this`
pub(crate) fn uses_this_stmt(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Let {
            init: Some(expr), ..
        } => uses_this_expr(expr),
        Stmt::Expr(expr) => uses_this_expr(expr),
        Stmt::Return(Some(expr)) => uses_this_expr(expr),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            uses_this_expr(condition)
                || then_branch.iter().any(uses_this_stmt)
                || else_branch
                    .as_ref()
                    .map(|b| b.iter().any(uses_this_stmt))
                    .unwrap_or(false)
        }
        Stmt::While { condition, body } => {
            uses_this_expr(condition) || body.iter().any(uses_this_stmt)
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_ref().map(|s| uses_this_stmt(s)).unwrap_or(false)
                || condition.as_ref().map(uses_this_expr).unwrap_or(false)
                || update.as_ref().map(uses_this_expr).unwrap_or(false)
                || body.iter().any(uses_this_stmt)
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            body.iter().any(uses_this_stmt)
                || catch
                    .as_ref()
                    .map(|c| c.body.iter().any(uses_this_stmt))
                    .unwrap_or(false)
                || finally
                    .as_ref()
                    .map(|f| f.iter().any(uses_this_stmt))
                    .unwrap_or(false)
        }
        Stmt::Throw(expr) => uses_this_expr(expr),
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            uses_this_expr(discriminant)
                || cases.iter().any(|c| {
                    c.test.as_ref().map(uses_this_expr).unwrap_or(false)
                        || c.body.iter().any(uses_this_stmt)
                })
        }
        _ => false,
    }
}

/// Check if a closure body uses `this`
pub(crate) fn closure_uses_this(body: &[Stmt]) -> bool {
    body.iter().any(uses_this_stmt)
}
