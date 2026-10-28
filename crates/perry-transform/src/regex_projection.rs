//! Project nonescaping exec results from their actual reads. The first slice
//! accepts null tests and one constant numbered capture (possibly read many
//! times). Everything else keeps the complete result, including escapes,
//! aliases, writes, dynamic indices, metadata and closure captures.
//!
//! This proves only result use. The runtime proves RegExp brand and the exact
//! fetched builtin method. We preserve Get(exec) before evaluating the input;
//! Arbitrary receivers and overrides retain the ordinary call with that
//! already-fetched method and original receiver; builtin input coercion remains
//! observable, including calls to user code and Symbol rejection.
use crate::local_occurrences::count_module_occurrences;
use perry_hir::types::{LocalId, Type};
use perry_hir::walker::{stmt_any_expr, walk_expr_children};
use perry_hir::{CompareOp, Expr, Function, Module, Stmt};
use std::collections::HashMap;

pub fn run(module: &mut Module) {
    // Runtime eval can observe lexical names without a LocalGet in this HIR.
    // Constant eval that was fully lowered already has ordinary counted uses.
    if perry_hir::has_deferred_dynamic_code_sites() {
        return;
    }
    let counts = count_module_occurrences(module);
    // The init's direct declarations can be script globals or exports.
    for stmt in &mut module.init {
        descend(stmt, &counts);
    }
    for f in &mut module.functions {
        function(f, &counts);
    }
    for class in &mut module.classes {
        for f in class
            .constructor
            .iter_mut()
            .chain(class.methods.iter_mut())
            .chain(class.static_methods.iter_mut())
            .chain(class.getters.iter_mut().map(|(_, f)| f))
            .chain(class.setters.iter_mut().map(|(_, f)| f))
            .chain(class.computed_members.iter_mut().map(|m| &mut m.function))
        {
            function(f, &counts);
        }
    }
}

fn function(f: &mut Function, counts: &HashMap<LocalId, usize>) {
    block(&mut f.body, counts);
}

fn index(expr: &Expr) -> Option<usize> {
    match expr {
        Expr::Integer(n) => usize::try_from(*n).ok(),
        Expr::Number(n)
            if n.is_finite() && *n >= 0.0 && *n < u32::MAX as f64 && n.fract() == 0.0 =>
        {
            Some(*n as usize)
        }
        _ => None,
    }
}

fn reads(
    expr: &Expr,
    id: LocalId,
    capture: &mut Option<usize>,
    allowed: &mut usize,
    different: &mut bool,
) {
    match expr {
        Expr::IndexGet { object, index: key } if matches!(object.as_ref(), Expr::LocalGet(l) if *l == id) => {
            if let Some(n) = index(key) {
                if capture.is_some_and(|previous| previous != n) {
                    *different = true;
                }
                *capture = Some(n);
                *allowed += 1;
                return;
            }
        }
        Expr::Compare {
            op: CompareOp::Eq | CompareOp::Ne | CompareOp::LooseEq | CompareOp::LooseNe,
            left,
            right,
        } if (matches!(left.as_ref(), Expr::LocalGet(l) if *l == id)
            && matches!(right.as_ref(), Expr::Null))
            || (matches!(right.as_ref(), Expr::LocalGet(l) if *l == id)
                && matches!(left.as_ref(), Expr::Null)) =>
        {
            *allowed += 1;
            return;
        }
        // Captured references must fail the module-wide census comparison.
        Expr::Closure { .. } | Expr::Delete(_) => return,
        _ => {}
    }
    walk_expr_children(expr, &mut |child| {
        reads(child, id, capture, allowed, different)
    });
}

fn block(stmts: &mut [Stmt], counts: &HashMap<LocalId, usize>) {
    for i in 0..stmts.len() {
        let Stmt::Let {
            id,
            mutable: false,
            init: Some(Expr::Call { callee, args, .. }),
            ..
        } = &stmts[i]
        else {
            continue;
        };
        let Expr::PropertyGet {
            object, property, ..
        } = callee.as_ref()
        else {
            continue;
        };
        if property != "exec" || !matches!(object.as_ref(), Expr::LocalGet(_)) || args.len() != 1 {
            continue;
        }
        let (mut capture, mut allowed, mut different) = (None, 0, false);
        for stmt in &stmts[i + 1..] {
            stmt_any_expr(stmt, &mut |expr| {
                reads(expr, *id, &mut capture, &mut allowed, &mut different);
                false
            });
        }
        let Some(capture) = capture else {
            continue;
        };
        if different || counts.get(id).copied() != Some(allowed + 1) {
            continue;
        }
        let projected = Expr::Call {
            callee: Box::new(Expr::ExternFuncRef {
                name: "js_regexp_exec_projected".into(),
                param_types: vec![Type::Any; 4],
                return_type: Type::Any,
            }),
            args: vec![
                object.as_ref().clone(),
                callee.as_ref().clone(),
                args[0].clone(),
                Expr::Number(capture as f64),
            ],
            type_args: vec![],
            byte_offset: 0,
        };
        if let Stmt::Let { init, .. } = &mut stmts[i] {
            *init = Some(projected);
        }
    }
    for stmt in stmts {
        descend(stmt, counts);
    }
}

fn descend(stmt: &mut Stmt, counts: &HashMap<LocalId, usize>) {
    match stmt {
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            block(then_branch, counts);
            if let Some(body) = else_branch {
                block(body, counts);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
            block(body, counts)
        }
        Stmt::Labeled { body, .. } => descend(body, counts),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            block(body, counts);
            if let Some(catch) = catch {
                block(&mut catch.body, counts);
            }
            if let Some(body) = finally {
                block(body, counts);
            }
        }
        Stmt::Switch { cases, .. } => {
            for case in cases {
                block(&mut case.body, counts);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
#[path = "regex_projection_tests.rs"]
mod tests;
