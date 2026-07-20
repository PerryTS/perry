// This module is part of the perry-codegen-arkts crate. It was
// mechanically split out of the former monolithic lib.rs (issue
// #1100). Pure code move — no logic changes.
#![allow(clippy::too_many_arguments)]
use crate::*;

/// Walk a Vec<Stmt> and rewrite any `state.set(v)` calls (where state's
/// LocalId is in the registry) to `setText(synth_id, v)` calls. Recurses
/// into closure bodies, blocks, control flow.
pub(crate) fn rewrite_state_calls_in_stmts(
    stmts: &mut Vec<Stmt>,
    reg: &HashMap<LocalId, StateBinding>,
) {
    for stmt in stmts.iter_mut() {
        rewrite_state_in_stmt(stmt, reg);
    }
}

pub(crate) fn rewrite_state_in_stmt(stmt: &mut Stmt, reg: &HashMap<LocalId, StateBinding>) {
    match stmt {
        Stmt::Expr(e) => rewrite_state_in_expr(e, reg),
        Stmt::Let { init: Some(e), .. } => rewrite_state_in_expr(e, reg),
        Stmt::Return(Some(e)) => rewrite_state_in_expr(e, reg),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_state_in_expr(condition, reg);
            rewrite_state_calls_in_stmts(then_branch, reg);
            if let Some(else_branch) = else_branch {
                rewrite_state_calls_in_stmts(else_branch, reg);
            }
        }
        Stmt::While {
            condition, body, ..
        }
        | Stmt::DoWhile {
            body, condition, ..
        } => {
            rewrite_state_in_expr(condition, reg);
            rewrite_state_calls_in_stmts(body, reg);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
            ..
        } => {
            if let Some(init) = init {
                rewrite_state_in_stmt(init.as_mut(), reg);
            }
            if let Some(c) = condition {
                rewrite_state_in_expr(c, reg);
            }
            if let Some(u) = update {
                rewrite_state_in_expr(u, reg);
            }
            rewrite_state_calls_in_stmts(body, reg);
        }
        _ => {}
    }
}

pub(crate) fn rewrite_state_in_expr(e: &mut Expr, reg: &HashMap<LocalId, StateBinding>) {
    // Detect `state.set(v)` first (most specific shape).
    if let Expr::Call { callee, args, .. } = e {
        if args.len() == 1 {
            if let Expr::PropertyGet {
                object, property, ..
            } = callee.as_ref()
            {
                if property == "set" {
                    if let Expr::LocalGet(state_id) = object.as_ref() {
                        if let Some(binding) = reg.get(state_id) {
                            let value_expr = args[0].clone();
                            *e = Expr::NativeMethodCall {
                                module: "perry/ui".to_string(),
                                class_name: None,
                                object: None,
                                method: "setText".to_string(),
                                args: vec![Expr::String(binding.synth_id.clone()), value_expr],
                            };
                            return;
                        }
                    }
                }
            }
        }
    }
    // Recurse into ALL expression children so nested state.set(v) calls
    // inside method args / object literals / closure bodies / etc. are
    // also rewritten. Each variant unrolls its sub-Exprs explicitly so
    // we don't miss any HIR shape.
    match e {
        Expr::Call { callee, args, .. } => {
            rewrite_state_in_expr(callee, reg);
            for a in args.iter_mut() {
                rewrite_state_in_expr(a, reg);
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                rewrite_state_in_expr(o, reg);
            }
            for a in args.iter_mut() {
                rewrite_state_in_expr(a, reg);
            }
        }
        Expr::Object(props) => {
            for (_, v) in props.iter_mut() {
                rewrite_state_in_expr(v, reg);
            }
        }
        Expr::Array(items) => {
            for v in items.iter_mut() {
                rewrite_state_in_expr(v, reg);
            }
        }
        Expr::Closure { body, .. } => {
            rewrite_state_calls_in_stmts(body, reg);
        }
        Expr::PropertyGet { object, .. } => {
            rewrite_state_in_expr(object, reg);
        }
        Expr::PropertySet { object, value, .. } => {
            rewrite_state_in_expr(object, reg);
            rewrite_state_in_expr(value, reg);
        }
        Expr::IndexGet { object, index } => {
            rewrite_state_in_expr(object, reg);
            rewrite_state_in_expr(index, reg);
        }
        Expr::Binary { left, right, .. } => {
            rewrite_state_in_expr(left, reg);
            rewrite_state_in_expr(right, reg);
        }
        Expr::ArrayMap { array, callback } => {
            rewrite_state_in_expr(array, reg);
            rewrite_state_in_expr(callback, reg);
        }
        Expr::New { args, .. } => {
            for a in args.iter_mut() {
                rewrite_state_in_expr(a, reg);
            }
        }
        // Leaf/other variants don't carry rewriteable sub-Exprs (or are
        // rare enough that v6 deferring them is fine — file as v6.5
        // follow-up if anyone hits a real-world miss).
        _ => {}
    }
}
