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
//! The same holds for the rest of a function body after `const c = new C()`
//! ([`try_lower_versioned_tail`]): one test at the declaration, then the
//! remaining statements lowered twice. In the first-evaluation copy `c` is an
//! instance of the shared class, exactly what a single evaluation's
//! `new C()` gives, so the `Ptr<Shape>` facts its template lineage proved
//! (`collectors::ptr_shape::ShapeProof::lineage`) apply to it unchanged:
//! bare field loads and direct method calls.
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

    let exact = first_evaluation_instances(ctx, std::slice::from_ref(stmt), binding);
    for (idx, is_first) in [(first_idx, true), (later_idx, false)] {
        ctx.current_block = idx;
        let mut copy = stmt.clone();
        resolve_guards_in_stmt(&mut copy, binding, is_first);
        // Scalar replacement registers a binding's field slots by id; the
        // other copy binds the same ids and must not find this copy's.
        let scalar_replaced = ctx.scalar_replaced.clone();
        let overlays = is_first.then(|| install_exact(ctx, &exact));
        lower_stmt(ctx, &copy)?;
        if let Some(saved) = overlays {
            restore_exact(ctx, saved);
        }
        ctx.scalar_replaced = scalar_replaced;
        if !ctx.block().is_terminated() {
            ctx.block().br(&join_l);
        }
    }
    ctx.current_block = join_idx;
    Ok(true)
}

/// The most HIR nodes a versioned function-body tail may hold: it is lowered
/// twice, so this bounds the code it adds.
const TAIL_NODE_LIMIT: usize = 400;

/// Lower `stmts[i..]`, the rest of a function body, versioned on the
/// first-evaluation guard of `stmts[i]` when that is `let c = new C()`
/// through a repeatable class declaration's binding. `index_base` is
/// `stmts[i]`'s index in the body (the shadow-slot clear plan is keyed by
/// it). `Ok(false)`: not applicable, and the caller lowers the statement as
/// usual.
pub(super) fn try_lower_versioned_tail(
    ctx: &mut FnCtx<'_>,
    tail: &[Stmt],
    index_base: usize,
    emit_shadow_clears: bool,
) -> Result<bool> {
    let Some((binding, template)) = tail_guard(ctx, tail) else {
        return Ok(false);
    };
    let value = Expr::LocalGet(binding);
    let first = crate::expr::class_first_evaluation::is_first_i1(ctx, &value, &template)?;
    let first_idx = ctx.new_block("classtail.first");
    let later_idx = ctx.new_block("classtail.later");
    let join_idx = ctx.new_block("classtail.join");
    let first_l = ctx.block_label(first_idx);
    let later_l = ctx.block_label(later_idx);
    let join_l = ctx.block_label(join_idx);
    ctx.block().cond_br(&first, &first_l, &later_l);

    // The body's shadow-slot clear plan, keyed from the tail's first
    // statement for the duration of the two copies.
    let clears = if emit_shadow_clears {
        let shifted = ctx
            .shadow_slot_clears_after_stmt
            .iter()
            .filter(|(k, _)| **k >= index_base)
            .map(|(k, v)| (*k - index_base, v.clone()))
            .collect();
        Some(std::mem::replace(
            &mut ctx.shadow_slot_clears_after_stmt,
            shifted,
        ))
    } else {
        None
    };
    let exact = first_evaluation_instances(ctx, tail, binding);
    let mut result = Ok(());
    for (idx, is_first) in [(first_idx, true), (later_idx, false)] {
        ctx.current_block = idx;
        let mut copy = tail.to_vec();
        for s in copy.iter_mut() {
            resolve_guards_in_stmt(s, binding, is_first);
        }
        let scalar_replaced = ctx.scalar_replaced.clone();
        let overlays = is_first.then(|| install_exact(ctx, &exact));
        result = super::lower_stmts_versioned_tail(ctx, &copy, emit_shadow_clears);
        if let Some(saved) = overlays {
            restore_exact(ctx, saved);
        }
        ctx.scalar_replaced = scalar_replaced;
        if result.is_err() {
            break;
        }
        if !ctx.block().is_terminated() {
            ctx.block().br(&join_l);
        }
    }
    if let Some(clears) = clears {
        ctx.shadow_slot_clears_after_stmt = clears;
    }
    result?;
    ctx.current_block = join_idx;
    Ok(true)
}

/// The guard `tail[0]` binds through: `let c = <first evaluation of C> ?
/// new C(..) : new C(..)` on a binding the tail cannot rebind, in a tail
/// small enough to lower twice that defines no code, and that gains from the
/// split: it uses `c`, or tests the binding outside a loop (a loop versions
/// itself).
fn tail_guard(ctx: &FnCtx<'_>, tail: &[Stmt]) -> Option<(u32, String)> {
    let Some(Stmt::Let {
        id: instance,
        init:
            Some(Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            }),
        ..
    }) = tail.first()
    else {
        return None;
    };
    // A class with captures stamps its first evaluation's instance with that
    // evaluation (`ClassEnvStamp`).
    let then_new = match then_expr.as_ref() {
        Expr::ClassEnvStamp { instance, .. } => instance.as_ref(),
        e => e,
    };
    if !matches!(then_new, Expr::New { .. })
        || !matches!(else_expr.as_ref(), Expr::NewDynamic { .. })
    {
        return None;
    }
    let Expr::ClassIsFirstEvaluation { value, template } = condition.as_ref() else {
        return None;
    };
    let Expr::LocalGet(id) = value.as_ref() else {
        return None;
    };
    if ctx.boxed_vars.contains(id)
        || ctx.module_globals.contains_key(id)
        || ctx.reassigned_locals.contains(id)
        || crate::collectors::rebound_locals(tail).contains(id)
    {
        return None;
    }
    let mut nodes = 0usize;
    let mut defines_code = false;
    for s in tail {
        perry_hir::walker::stmt_any_expr(s, &mut |e| {
            visit_expr(e, &mut |e| {
                nodes += 1;
                if matches!(e, Expr::Closure { .. } | Expr::ClassExprFresh { .. }) {
                    defines_code = true;
                }
            });
            false
        });
        nodes += 1;
    }
    if defines_code || nodes > TAIL_NODE_LIMIT {
        return None;
    }
    let rest = &tail[1..];
    if !mentions_local(rest, *instance) && !guards_outside_loops(rest, *id) {
        return None;
    }
    Some((*id, template.clone()))
}

fn mentions_local(stmts: &[Stmt], id: u32) -> bool {
    stmts.iter().any(|s| {
        perry_hir::walker::stmt_any_expr(s, &mut |e| {
            let mut found = false;
            visit_expr(e, &mut |e| {
                found |= matches!(e, Expr::LocalGet(l) if *l == id)
            });
            found
        })
    })
}

/// Does `stmts` test `binding`'s first evaluation outside a loop?
fn guards_outside_loops(stmts: &[Stmt], binding: u32) -> bool {
    let guards = |e: &Expr| {
        let mut found = false;
        visit_expr(e, &mut |e| {
            found |= matches!(e, Expr::ClassIsFirstEvaluation { value, .. }
                if matches!(value.as_ref(), Expr::LocalGet(l) if *l == binding));
        });
        found
    };
    stmts.iter().any(|s| match s {
        Stmt::For { .. } | Stmt::While { .. } | Stmt::DoWhile { .. } => false,
        Stmt::Let { init: Some(e), .. }
        | Stmt::Return(Some(e))
        | Stmt::Expr(e)
        | Stmt::Throw(e) => guards(e),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            guards(condition)
                || guards_outside_loops(then_branch, binding)
                || else_branch
                    .as_ref()
                    .is_some_and(|b| guards_outside_loops(b, binding))
        }
        Stmt::Labeled { body, .. } => guards_outside_loops(std::slice::from_ref(body), binding),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            guards_outside_loops(body, binding)
                || catch
                    .as_ref()
                    .is_some_and(|c| guards_outside_loops(&c.body, binding))
                || finally
                    .as_ref()
                    .is_some_and(|f| guards_outside_loops(f, binding))
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            guards(discriminant)
                || cases.iter().any(|c| {
                    c.test.as_ref().is_some_and(guards) || guards_outside_loops(&c.body, binding)
                })
        }
        // Any other statement: a guard it holds still tests at run time.
        _ => false,
    })
}

/// The locals `stmts` binds to a guarded `new` through `binding` that the
/// template-lineage proof covers: in the first-evaluation copy each holds an
/// instance of the shared class, which is the proof's missing exactness.
fn first_evaluation_instances(
    ctx: &FnCtx<'_>,
    stmts: &[Stmt],
    binding: u32,
) -> Vec<(u32, crate::collectors::PtrShapeLocal)> {
    let mut out = Vec::new();
    for s in stmts {
        collect_guarded_lets(s, binding, &mut out);
    }
    out.into_iter()
        .filter_map(|id| {
            ctx.native_facts
                .shape_lineage_local(id)
                .map(|fact| (id, fact.clone()))
        })
        .collect()
}

fn collect_guarded_lets(stmt: &Stmt, binding: u32, out: &mut Vec<u32>) {
    let guarded = |e: &Expr| {
        matches!(e, Expr::Conditional { condition, then_expr, .. }
            if matches!(then_expr.as_ref(), Expr::New { .. })
                && matches!(condition.as_ref(), Expr::ClassIsFirstEvaluation { value, .. }
                    if matches!(value.as_ref(), Expr::LocalGet(id) if *id == binding)))
    };
    let nested = |body: &[Stmt], out: &mut Vec<u32>| {
        for s in body {
            collect_guarded_lets(s, binding, out);
        }
    };
    match stmt {
        Stmt::Let {
            id, init: Some(e), ..
        } if guarded(e) => out.push(*id),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            nested(then_branch, out);
            if let Some(b) = else_branch {
                nested(b, out);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => nested(body, out),
        Stmt::For { init, body, .. } => {
            if let Some(s) = init {
                collect_guarded_lets(s, binding, out);
            }
            nested(body, out);
        }
        Stmt::Labeled { body, .. } => collect_guarded_lets(body, binding, out),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            nested(body, out);
            if let Some(c) = catch {
                nested(&c.body, out);
            }
            if let Some(f) = finally {
                nested(f, out);
            }
        }
        Stmt::Switch { cases, .. } => {
            for case in cases {
                nested(&case.body, out);
            }
        }
        _ => {}
    }
}

/// Make each first-evaluation instance's lineage fact an exact `Ptr<Shape>`
/// fact for the first-evaluation copy; returns what it displaced.
fn install_exact(
    ctx: &mut FnCtx<'_>,
    exact: &[(u32, crate::collectors::PtrShapeLocal)],
) -> Vec<(u32, Option<crate::collectors::PtrShapeLocal>)> {
    exact
        .iter()
        .map(|(id, fact)| (*id, ctx.proven_shape_params.insert(*id, fact.clone())))
        .collect()
}

fn restore_exact(ctx: &mut FnCtx<'_>, saved: Vec<(u32, Option<crate::collectors::PtrShapeLocal>)>) {
    for (id, previous) in saved {
        match previous {
            Some(fact) => {
                ctx.proven_shape_params.insert(id, fact);
            }
            None => {
                ctx.proven_shape_params.remove(&id);
            }
        }
    }
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
