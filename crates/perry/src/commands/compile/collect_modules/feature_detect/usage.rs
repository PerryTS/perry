//! Feature-detection traversal that distinguishes discarded expression statements.

use perry_hir::{Expr, Stmt};

/// Test expressions in a statement and its nested statements. The predicate
/// controls expression descent, including whether to enter nested closures.
/// New statement variants must explicitly declare their children here.
fn stmt_any_expr<'a>(stmt: &'a Stmt, predicate: &mut impl FnMut(&'a Expr, bool) -> bool) -> bool {
    match stmt {
        Stmt::Let { init, .. } | Stmt::Return(init) => {
            init.as_ref().is_some_and(|e| predicate(e, false))
        }
        Stmt::Expr(expr) => predicate(expr, true),
        Stmt::Throw(expr) => predicate(expr, false),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            predicate(condition, false)
                || then_branch.iter().any(|s| stmt_any_expr(s, predicate))
                || else_branch
                    .as_ref()
                    .is_some_and(|body| body.iter().any(|s| stmt_any_expr(s, predicate)))
        }
        Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
            predicate(condition, false) || body.iter().any(|s| stmt_any_expr(s, predicate))
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_ref().is_some_and(|s| stmt_any_expr(s, predicate))
                || condition.as_ref().is_some_and(|e| predicate(e, false))
                || update.as_ref().is_some_and(|e| predicate(e, true))
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
            predicate(discriminant, false)
                || cases.iter().any(|c| {
                    c.test.as_ref().is_some_and(|e| predicate(e, false))
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

/// The boolean is true only where the expression's result is discarded.
/// Expression descent belongs to the predicate; nested call arguments never
/// inherit the surrounding statement's discarded-result permission.
pub(super) fn statements(stmts: &[Stmt], found: &mut dyn FnMut(&Expr, bool) -> bool) -> bool {
    stmts
        .iter()
        .any(|stmt| stmt_any_expr(stmt, &mut |e, discarded| found(e, discarded)))
}

pub(super) fn module(
    module: &perry_hir::Module,
    found: &mut dyn FnMut(&perry_hir::Expr, bool) -> bool,
) -> bool {
    use perry_hir::Function;

    fn decorators(
        ds: &[perry_hir::Decorator],
        found: &mut dyn FnMut(&perry_hir::Expr, bool) -> bool,
    ) -> bool {
        ds.iter().any(|d| d.args.iter().any(|e| found(e, false)))
    }

    fn function(f: &Function, found: &mut dyn FnMut(&perry_hir::Expr, bool) -> bool) -> bool {
        f.params.iter().any(|param| {
            param.default.as_ref().is_some_and(|e| found(e, false))
                || decorators(&param.decorators, found)
        }) || statements(&f.body, found)
            || decorators(&f.decorators, found)
    }

    statements(&module.init, found)
        || module.functions.iter().any(|f| function(f, found))
        || module.classes.iter().any(|class| {
            class
                .extends_expr
                .as_deref()
                .is_some_and(|e| found(e, false))
                || class
                    .constructor
                    .iter()
                    .chain(&class.methods)
                    .chain(&class.static_methods)
                    .chain(class.getters.iter().map(|(_, f)| f))
                    .chain(class.setters.iter().map(|(_, f)| f))
                    .any(|f| function(f, found))
                || class
                    .fields
                    .iter()
                    .chain(&class.static_fields)
                    .any(|field| {
                        field.init.as_ref().is_some_and(|e| found(e, false))
                            || field.key_expr.as_ref().is_some_and(|e| found(e, false))
                            || decorators(&field.decorators, found)
                    })
                || class.computed_members.iter().any(|member| {
                    found(&member.key_expr, false) || function(&member.function, found)
                })
                || decorators(&class.decorators, found)
        })
}
