//! MATHC1: the namespace pairs of the facts lowering, each replaced by the ONE
//! arm its call site's verdict picks.
//!
//! With the whole-program facts on, a direct builtin namespace call is lowered
//! as `NamespacePatched(ns) ? globalThis.<ns>.<m>(args) : <intrinsic>`
//! (`lower/expr_call/patched_builtin_call.rs`), so the solver sees its member
//! read. Once the facts are solved, the pair becomes the intrinsic when the
//! generic call is proven to read the original builtin (ECMA-262 13.3.6.1:
//! the call evaluates the member `N.m` and calls that value), else the generic
//! call alone. No pair is left for a shadow-bit test, and the intrinsic is the
//! same HIR the call lowers to without the pair.

use std::collections::BTreeSet;

use crate::ir::{Expr, Module, Stmt};

/// The generic call's member offset when `generic` is a pair's generic arm
/// `globalThis.<ns>.<m>(args…)`.
fn pair_offset(ns: &str, generic: &Expr) -> Option<u32> {
    let (Expr::Call { callee, .. } | Expr::CallSpread { callee, .. }) = generic else {
        return None;
    };
    let Expr::PropertyGet { object, byte_offset, .. } = callee.as_ref() else {
        return None;
    };
    match object.as_ref() {
        Expr::PropertyGet { object: g, property, .. }
            if matches!(g.as_ref(), Expr::GlobalThisExpr) && property == ns =>
        {
            Some(*byte_offset)
        }
        _ => None,
    }
}

fn pick(e: &mut Expr, proven: &BTreeSet<u32>) {
    if let Expr::Conditional { condition, then_expr, else_expr } = e {
        if let Expr::NamespacePatched(ns) = condition.as_ref() {
            if let Some(offset) = pair_offset(ns, then_expr) {
                let arm = if offset != 0 && proven.contains(&offset) {
                    std::mem::replace(else_expr.as_mut(), Expr::Undefined)
                } else {
                    std::mem::replace(then_expr.as_mut(), Expr::Undefined)
                };
                *e = arm;
                return pick(e, proven);
            }
        }
    }
    if let Expr::Closure { body, .. } = e {
        stmts(body, proven);
    }
    crate::walker::walk_expr_children_mut(e, &mut |c| pick(c, proven));
}

fn stmts(body: &mut [Stmt], proven: &BTreeSet<u32>) {
    for stmt in body {
        match stmt {
            Stmt::Let { init: Some(e), .. } | Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Return(Some(e)) => {
                pick(e, proven)
            }
            Stmt::If { condition, then_branch, else_branch } => {
                pick(condition, proven);
                stmts(then_branch, proven);
                if let Some(b) = else_branch {
                    stmts(b, proven);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
                pick(condition, proven);
                stmts(body, proven);
            }
            Stmt::For { init, condition, update, body } => {
                if let Some(init) = init {
                    stmts(std::slice::from_mut(init), proven);
                }
                if let Some(e) = condition {
                    pick(e, proven);
                }
                if let Some(e) = update {
                    pick(e, proven);
                }
                stmts(body, proven);
            }
            Stmt::Labeled { body, .. } => stmts(std::slice::from_mut(body), proven),
            Stmt::Try { body, catch, finally, .. } => {
                stmts(body, proven);
                if let Some(c) = catch {
                    stmts(&mut c.body, proven);
                }
                if let Some(b) = finally {
                    stmts(b, proven);
                }
            }
            Stmt::Switch { discriminant, cases } => {
                pick(discriminant, proven);
                for case in cases {
                    if let Some(e) = &mut case.test {
                        pick(e, proven);
                    }
                    stmts(&mut case.body, proven);
                }
            }
            _ => {}
        }
    }
}

fn function(f: &mut crate::ir::Function, proven: &BTreeSet<u32>) {
    for p in &mut f.params {
        if let Some(d) = &mut p.default {
            pick(d, proven);
        }
    }
    stmts(&mut f.body, proven);
}

/// Replace every namespace pair of `module` by its one arm: the intrinsic
/// where `proven` holds the generic call's member offset, else the generic
/// call. A pair codegen meets elsewhere is picked there by the same rule.
pub fn pick_namespace_pair_arms(module: &mut Module, proven: &BTreeSet<u32>) {
    stmts(&mut module.init, proven);
    for global in &mut module.globals {
        if let Some(e) = &mut global.init {
            pick(e, proven);
        }
    }
    for f in &mut module.functions {
        function(f, proven);
    }
    for class in &mut module.classes {
        if let Some(e) = &mut class.extends_expr {
            pick(e, proven);
        }
        for field in class.fields.iter_mut().chain(&mut class.static_fields) {
            if let Some(e) = &mut field.key_expr {
                pick(e, proven);
            }
            if let Some(e) = &mut field.init {
                pick(e, proven);
            }
        }
        for f in class
            .constructor
            .iter_mut()
            .chain(&mut class.methods)
            .chain(&mut class.static_methods)
            .chain(class.getters.iter_mut().map(|(_, f)| f))
            .chain(class.setters.iter_mut().map(|(_, f)| f))
        {
            function(f, proven);
        }
    }
}
