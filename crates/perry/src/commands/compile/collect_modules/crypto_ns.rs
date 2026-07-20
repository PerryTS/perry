//! Global-`crypto` namespace usage detection.
//!
//! Extracted from `collect_modules.rs` (file-size split). Walks a
//! module's HIR to decide whether the stdlib crypto runtime must be
//! linked because the source reads the global `crypto` object (e.g.
//! `crypto.randomUUID()`), including reads inside closure bodies that
//! the shared expression walker intentionally skips.

use perry_hir::{Expr, Stmt};

fn expr_uses_global_crypto_namespace(expr: &Expr) -> bool {
    if matches!(
        expr,
        Expr::PropertyGet { object, property, .. }
            if property == "crypto" && matches!(object.as_ref(), Expr::GlobalGet(0))
    ) {
        return true;
    }

    // The shared expression walker intentionally does not enter closure
    // bodies; global crypto reads inside closures still need stdlib crypto
    // linked for runtime-dispatched calls such as `c.randomUUID()`.
    if let Expr::Closure { body, .. } = expr {
        if stmts_use_global_crypto_namespace(body) {
            return true;
        }
    }

    let mut found = false;
    perry_hir::walker::walk_expr_children(expr, &mut |child| {
        if !found && expr_uses_global_crypto_namespace(child) {
            found = true;
        }
    });
    found
}

fn stmts_use_global_crypto_namespace(stmts: &[Stmt]) -> bool {
    stmts.iter().any(stmt_uses_global_crypto_namespace)
}

fn stmt_uses_global_crypto_namespace(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Let { init, .. } => init
            .as_ref()
            .map(expr_uses_global_crypto_namespace)
            .unwrap_or(false),
        Stmt::Expr(expr) | Stmt::Throw(expr) => expr_uses_global_crypto_namespace(expr),
        Stmt::Return(expr) => expr
            .as_ref()
            .map(expr_uses_global_crypto_namespace)
            .unwrap_or(false),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expr_uses_global_crypto_namespace(condition)
                || stmts_use_global_crypto_namespace(then_branch)
                || else_branch
                    .as_ref()
                    .map(|branch| stmts_use_global_crypto_namespace(branch))
                    .unwrap_or(false)
        }
        Stmt::While { condition, body } => {
            expr_uses_global_crypto_namespace(condition) || stmts_use_global_crypto_namespace(body)
        }
        Stmt::DoWhile { body, condition } => {
            stmts_use_global_crypto_namespace(body) || expr_uses_global_crypto_namespace(condition)
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_ref()
                .map(|stmt| stmt_uses_global_crypto_namespace(stmt))
                .unwrap_or(false)
                || condition
                    .as_ref()
                    .map(expr_uses_global_crypto_namespace)
                    .unwrap_or(false)
                || update
                    .as_ref()
                    .map(expr_uses_global_crypto_namespace)
                    .unwrap_or(false)
                || stmts_use_global_crypto_namespace(body)
        }
        Stmt::Labeled { body, .. } => stmt_uses_global_crypto_namespace(body),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            stmts_use_global_crypto_namespace(body)
                || catch
                    .as_ref()
                    .map(|catch| stmts_use_global_crypto_namespace(&catch.body))
                    .unwrap_or(false)
                || finally
                    .as_ref()
                    .map(|body| stmts_use_global_crypto_namespace(body))
                    .unwrap_or(false)
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            expr_uses_global_crypto_namespace(discriminant)
                || cases.iter().any(|case| {
                    case.test
                        .as_ref()
                        .map(expr_uses_global_crypto_namespace)
                        .unwrap_or(false)
                        || stmts_use_global_crypto_namespace(&case.body)
                })
        }
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_) => false,
    }
}

fn function_uses_global_crypto_namespace(function: &perry_hir::Function) -> bool {
    function
        .params
        .iter()
        .filter_map(|param| param.default.as_ref())
        .any(expr_uses_global_crypto_namespace)
        || stmts_use_global_crypto_namespace(&function.body)
}

pub(super) fn module_uses_global_crypto_namespace(module: &perry_hir::Module) -> bool {
    stmts_use_global_crypto_namespace(&module.init)
        || module
            .functions
            .iter()
            .any(function_uses_global_crypto_namespace)
}
