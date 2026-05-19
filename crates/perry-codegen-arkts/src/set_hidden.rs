// This module is part of the perry-codegen-arkts crate. It was
// mechanically split out of the former monolithic lib.rs (issue
// #1100). Pure code move — no logic changes.
#![allow(clippy::too_many_arguments)]
use crate::*;

/// Phase 2 v3.5 — rewrite every `widgetSetHidden(LocalGet(target), value)`
/// call in `stmts` to a NAPI bridge call when target has a binding. The
/// bridge call is shaped as `Expr::NativeMethodCall { module: "perry/arkts",
/// method: "setVisibility", args: [String(synth_id), value] }`. The codegen
/// dispatcher (`crates/perry-codegen/src/lower_call/native.rs`) recognizes
/// this shape and lowers to the runtime FFI `perry_arkts_set_visibility`,
/// which pushes the (id, hidden) tuple to a NAPI drain queue.
pub(crate) fn rewrite_set_hidden_calls_in_stmts(
    stmts: &mut Vec<Stmt>,
    bindings: &HashMap<LocalId, VisibilityBinding>,
) {
    for stmt in stmts.iter_mut() {
        rewrite_set_hidden_in_stmt(stmt, bindings);
    }
}

pub(crate) fn rewrite_set_hidden_in_stmt(
    stmt: &mut Stmt,
    bindings: &HashMap<LocalId, VisibilityBinding>,
) {
    match stmt {
        Stmt::Expr(e) => rewrite_set_hidden_in_expr(e, bindings),
        Stmt::Let { init: Some(e), .. } => rewrite_set_hidden_in_expr(e, bindings),
        Stmt::Return(Some(e)) => rewrite_set_hidden_in_expr(e, bindings),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            rewrite_set_hidden_in_expr(condition, bindings);
            rewrite_set_hidden_calls_in_stmts(then_branch, bindings);
            if let Some(eb) = else_branch {
                rewrite_set_hidden_calls_in_stmts(eb, bindings);
            }
        }
        Stmt::While {
            condition, body, ..
        }
        | Stmt::DoWhile {
            body, condition, ..
        } => {
            rewrite_set_hidden_in_expr(condition, bindings);
            rewrite_set_hidden_calls_in_stmts(body, bindings);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
            ..
        } => {
            if let Some(i) = init {
                rewrite_set_hidden_in_stmt(i.as_mut(), bindings);
            }
            if let Some(c) = condition {
                rewrite_set_hidden_in_expr(c, bindings);
            }
            if let Some(u) = update {
                rewrite_set_hidden_in_expr(u, bindings);
            }
            rewrite_set_hidden_calls_in_stmts(body, bindings);
        }
        _ => {}
    }
}

pub(crate) fn rewrite_set_hidden_in_expr(
    e: &mut Expr,
    bindings: &HashMap<LocalId, VisibilityBinding>,
) {
    // Detect the rewrite target FIRST (most specific shape), before
    // recursing into children — otherwise children of the call to
    // rewrite would be visited as if they were in a regular call.
    if let Expr::NativeMethodCall {
        module,
        method,
        args,
        object: None,
        ..
    } = e
    {
        if module == "perry/ui" && method == "widgetSetHidden" && args.len() == 2 {
            if let Expr::LocalGet(target_id) = &args[0] {
                if let Some(binding) = bindings.get(target_id) {
                    // Coerce literal numbers/integers to bool so the runtime
                    // side gets a proper boolean. Non-literal values pass
                    // through and get coerced runtime-side via the same
                    // js_jsvalue_to_string-style helper as setText.
                    let hidden_arg = match &args[1] {
                        Expr::Bool(b) => Expr::Bool(*b),
                        Expr::Number(n) => Expr::Bool(*n != 0.0),
                        Expr::Integer(n) => Expr::Bool(*n != 0),
                        other => other.clone(),
                    };
                    *e = Expr::NativeMethodCall {
                        module: "perry/arkts".to_string(),
                        class_name: None,
                        object: None,
                        method: "setVisibility".to_string(),
                        args: vec![Expr::String(binding.synth_id.clone()), hidden_arg],
                    };
                    return;
                }
            }
        }
    }
    // Recurse into all sub-expressions so closures, nested calls, etc.
    // get their setHidden calls rewritten too.
    match e {
        Expr::Closure { body, .. } => {
            rewrite_set_hidden_calls_in_stmts(body, bindings);
        }
        Expr::Call { callee, args, .. } => {
            rewrite_set_hidden_in_expr(callee, bindings);
            for a in args.iter_mut() {
                rewrite_set_hidden_in_expr(a, bindings);
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                rewrite_set_hidden_in_expr(o, bindings);
            }
            for a in args.iter_mut() {
                rewrite_set_hidden_in_expr(a, bindings);
            }
        }
        Expr::PropertyGet { object, .. } => {
            rewrite_set_hidden_in_expr(object, bindings);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            rewrite_set_hidden_in_expr(condition, bindings);
            rewrite_set_hidden_in_expr(then_expr, bindings);
            rewrite_set_hidden_in_expr(else_expr, bindings);
        }
        Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } => {
            rewrite_set_hidden_in_expr(left, bindings);
            rewrite_set_hidden_in_expr(right, bindings);
        }
        Expr::Unary { operand, .. } => {
            rewrite_set_hidden_in_expr(operand, bindings);
        }
        Expr::Array(items) => {
            for i in items.iter_mut() {
                rewrite_set_hidden_in_expr(i, bindings);
            }
        }
        Expr::Object(props) => {
            for (_, v) in props.iter_mut() {
                rewrite_set_hidden_in_expr(v, bindings);
            }
        }
        Expr::New { args, .. } => {
            for a in args.iter_mut() {
                rewrite_set_hidden_in_expr(a, bindings);
            }
        }
        _ => {}
    }
}

/// Variant that ONLY recurses into closures inside module.init, leaving
/// the top-level Stmts alone. Used when we want closure-body widgetSetHidden
/// calls rewritten (so taps push to drain) but module-init top-level calls
/// preserved (their initial value is captured statically by
/// `collect_visibility_bindings` Pass 2).
pub(crate) fn rewrite_set_hidden_in_closures_in_stmts(
    stmts: &mut Vec<Stmt>,
    bindings: &HashMap<LocalId, VisibilityBinding>,
) {
    for stmt in stmts.iter_mut() {
        rewrite_set_hidden_in_closures_in_stmt(stmt, bindings);
    }
}

pub(crate) fn rewrite_set_hidden_in_closures_in_stmt(
    stmt: &mut Stmt,
    bindings: &HashMap<LocalId, VisibilityBinding>,
) {
    match stmt {
        Stmt::Expr(e) | Stmt::Return(Some(e)) => {
            rewrite_set_hidden_in_closures_in_expr(e, bindings);
        }
        Stmt::Let { init: Some(e), .. } => {
            rewrite_set_hidden_in_closures_in_expr(e, bindings);
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            rewrite_set_hidden_in_closures_in_expr(condition, bindings);
            rewrite_set_hidden_in_closures_in_stmts(then_branch, bindings);
            if let Some(eb) = else_branch {
                rewrite_set_hidden_in_closures_in_stmts(eb, bindings);
            }
        }
        Stmt::While {
            condition, body, ..
        }
        | Stmt::DoWhile {
            body, condition, ..
        } => {
            rewrite_set_hidden_in_closures_in_expr(condition, bindings);
            rewrite_set_hidden_in_closures_in_stmts(body, bindings);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
            ..
        } => {
            if let Some(i) = init {
                rewrite_set_hidden_in_closures_in_stmt(i.as_mut(), bindings);
            }
            if let Some(c) = condition {
                rewrite_set_hidden_in_closures_in_expr(c, bindings);
            }
            if let Some(u) = update {
                rewrite_set_hidden_in_closures_in_expr(u, bindings);
            }
            rewrite_set_hidden_in_closures_in_stmts(body, bindings);
        }
        _ => {}
    }
}

pub(crate) fn rewrite_set_hidden_in_closures_in_expr(
    e: &mut Expr,
    bindings: &HashMap<LocalId, VisibilityBinding>,
) {
    // When we hit a closure, its body IS the call site — recurse there
    // with the full rewriter (which treats every level as a target).
    if let Expr::Closure { body, .. } = e {
        rewrite_set_hidden_calls_in_stmts(body, bindings);
        return;
    }
    // Otherwise descend into sub-exprs without rewriting top-level
    // widgetSetHidden calls.
    match e {
        Expr::Call { callee, args, .. } => {
            rewrite_set_hidden_in_closures_in_expr(callee, bindings);
            for a in args.iter_mut() {
                rewrite_set_hidden_in_closures_in_expr(a, bindings);
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                rewrite_set_hidden_in_closures_in_expr(o, bindings);
            }
            for a in args.iter_mut() {
                rewrite_set_hidden_in_closures_in_expr(a, bindings);
            }
        }
        Expr::PropertyGet { object, .. } => {
            rewrite_set_hidden_in_closures_in_expr(object, bindings);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            rewrite_set_hidden_in_closures_in_expr(condition, bindings);
            rewrite_set_hidden_in_closures_in_expr(then_expr, bindings);
            rewrite_set_hidden_in_closures_in_expr(else_expr, bindings);
        }
        Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } => {
            rewrite_set_hidden_in_closures_in_expr(left, bindings);
            rewrite_set_hidden_in_closures_in_expr(right, bindings);
        }
        Expr::Unary { operand, .. } => {
            rewrite_set_hidden_in_closures_in_expr(operand, bindings);
        }
        Expr::Array(items) => {
            for i in items.iter_mut() {
                rewrite_set_hidden_in_closures_in_expr(i, bindings);
            }
        }
        Expr::Object(props) => {
            for (_, v) in props.iter_mut() {
                rewrite_set_hidden_in_closures_in_expr(v, bindings);
            }
        }
        Expr::New { args, .. } => {
            for a in args.iter_mut() {
                rewrite_set_hidden_in_closures_in_expr(a, bindings);
            }
        }
        _ => {}
    }
}
