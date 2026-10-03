//! #11759 (c′): a loop versioned on a class declaration's first evaluation.
//!
//! `new C()` and `C.<static field>` through the binding of a declaration that
//! may be evaluated more than once are guarded per use
//! (`Conditional { ClassIsFirstEvaluation { LocalGet(C) }, static, by value }`).
//! The guard's answer is a fact of the binding's value: the first evaluation
//! is the shared class, and the template's first-evaluation word, once set,
//! never changes. So while a loop cannot rebind `C`, every guard on `C` in it
//! gives the same answer as one test before the loop. The loop is lowered
//! twice under that one test: the first-evaluation copy holds only the static
//! forms (so `let c = new C()` is scalar-replaced exactly as a single
//! evaluation's would be), the later-evaluation copy only the by-value forms.
//!
//! A guard left in either copy (one this rewrite does not reach) still tests
//! at run time, so the rewrite only removes tests; it never decides one.

use anyhow::Result;
use perry_hir::{Expr, Stmt};

use super::lower_stmt;
use crate::expr::FnCtx;

/// Lower `stmt` (a `For`/`While`/`DoWhile`) versioned on a first-evaluation
/// guard it holds. `Ok(false)`: the loop holds no guard it can hoist, and the
/// caller lowers it as usual.
pub(super) fn try_lower_versioned_loop(ctx: &mut FnCtx<'_>, stmt: &Stmt) -> Result<bool> {
    let Some((binding, template)) = hoistable_guard(ctx, stmt) else {
        return Ok(false);
    };
    let value = Expr::LocalGet(binding);
    let first = crate::expr::class_first_evaluation::is_first_i1(ctx, &value, &template)?;
    let first_idx = ctx.new_block("classloop.first");
    let later_idx = ctx.new_block("classloop.later");
    let join_idx = ctx.new_block("classloop.join");
    let first_l = ctx.block_label(first_idx);
    let later_l = ctx.block_label(later_idx);
    let join_l = ctx.block_label(join_idx);
    ctx.block().cond_br(&first, &first_l, &later_l);

    for (idx, is_first) in [(first_idx, true), (later_idx, false)] {
        ctx.current_block = idx;
        let mut copy = stmt.clone();
        resolve_guards_in_stmt(&mut copy, binding, is_first);
        // Scalar replacement registers a binding's field slots by id; the
        // other copy binds the same ids and must not find this copy's.
        let scalar_replaced = ctx.scalar_replaced.clone();
        lower_stmt(ctx, &copy)?;
        ctx.scalar_replaced = scalar_replaced;
        if !ctx.block().is_terminated() {
            ctx.block().br(&join_l);
        }
    }
    ctx.current_block = join_idx;
    Ok(true)
}

/// The first `ClassIsFirstEvaluation` guard on a local in `stmt` whose answer
/// cannot change while `stmt` runs: the local is never rebound in this body
/// (nor in `stmt`), is not boxed (a callee could rebind a box) and is not a
/// module global (another body could). A loop that defines code (a closure
/// or class) is left alone: its copies would define it twice.
fn hoistable_guard(ctx: &FnCtx<'_>, stmt: &Stmt) -> Option<(u32, String)> {
    let mut defines_code = false;
    let mut guard: Option<(u32, String)> = None;
    perry_hir::walker::stmt_any_expr(stmt, &mut |e| {
        visit_expr(e, &mut |e| match e {
            Expr::Closure { .. } | Expr::ClassExprFresh { .. } => defines_code = true,
            Expr::Conditional { condition, .. } if guard.is_none() => {
                if let Expr::ClassIsFirstEvaluation { value, template } = condition.as_ref() {
                    if let Expr::LocalGet(id) = value.as_ref() {
                        guard = Some((*id, template.clone()));
                    }
                }
            }
            _ => {}
        });
        false
    });
    if defines_code {
        return None;
    }
    let (id, template) = guard?;
    if ctx.boxed_vars.contains(&id)
        || ctx.module_globals.contains_key(&id)
        || ctx.reassigned_locals.contains(&id)
        || crate::collectors::rebound_locals(std::slice::from_ref(stmt)).contains(&id)
    {
        return None;
    }
    Some((id, template))
}

/// Visit `expr` and every sub-expression (not into nested statement bodies).
fn visit_expr(expr: &Expr, f: &mut impl FnMut(&Expr)) {
    f(expr);
    perry_hir::walker::walk_expr_children(expr, &mut |child| visit_expr(child, f));
}

/// Replace every guard on `binding` in `stmt` with the branch `is_first`
/// selects.
fn resolve_guards_in_stmt(stmt: &mut Stmt, binding: u32, is_first: bool) {
    let exprs = |e: &mut Expr| resolve_guards_in_expr(e, binding, is_first);
    let stmts = |body: &mut Vec<Stmt>| {
        for s in body.iter_mut() {
            resolve_guards_in_stmt(s, binding, is_first);
        }
    };
    match stmt {
        Stmt::Let { init, .. } | Stmt::Return(init) => {
            if let Some(e) = init {
                exprs(e);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => exprs(e),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            exprs(condition);
            stmts(then_branch);
            if let Some(b) = else_branch {
                stmts(b);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            exprs(condition);
            stmts(body);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(s) = init {
                resolve_guards_in_stmt(s, binding, is_first);
            }
            if let Some(e) = condition {
                exprs(e);
            }
            if let Some(e) = update {
                exprs(e);
            }
            stmts(body);
        }
        Stmt::Labeled { body, .. } => resolve_guards_in_stmt(body, binding, is_first),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            stmts(body);
            if let Some(c) = catch {
                stmts(&mut c.body);
            }
            if let Some(f) = finally {
                stmts(f);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            exprs(discriminant);
            for case in cases {
                if let Some(t) = &mut case.test {
                    exprs(t);
                }
                stmts(&mut case.body);
            }
        }
        // Any other statement holds no expression this rewrite needs; a guard
        // it misses still tests at run time.
        _ => {}
    }
}

fn resolve_guards_in_expr(expr: &mut Expr, binding: u32, is_first: bool) {
    let resolved = match expr {
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } if matches!(
            condition.as_ref(),
            Expr::ClassIsFirstEvaluation { value, .. }
                if matches!(value.as_ref(), Expr::LocalGet(id) if *id == binding)
        ) =>
        {
            let branch = if is_first { then_expr } else { else_expr };
            Some(std::mem::replace(branch.as_mut(), Expr::Undefined))
        }
        _ => None,
    };
    if let Some(branch) = resolved {
        *expr = branch;
        resolve_guards_in_expr(expr, binding, is_first);
        return;
    }
    perry_hir::walker::walk_expr_children_mut(expr, &mut |child| {
        resolve_guards_in_expr(child, binding, is_first)
    });
}
