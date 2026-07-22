// This module is part of the perry-codegen-arkts crate. It was
// mechanically split out of the former monolithic lib.rs (issue
// #1100). Pure code move — no logic changes.
#![allow(clippy::too_many_arguments)]
use crate::*;

pub(crate) fn walk_for_set_hidden_targets_in_stmts(
    stmts: &[Stmt],
    out: &mut std::collections::BTreeSet<LocalId>,
) {
    for stmt in stmts {
        walk_for_set_hidden_targets_in_stmt(stmt, out);
    }
}

/// Variant for walking module.init: descends through nested control flow
/// + sub-exprs WITHOUT recording any widgetSetHidden targets it sees at
/// the outer scope, but when it encounters an `Expr::Closure`, switches
/// to the unrestricted target-recording walker for the closure body. This
/// is what makes "module-init top-level widgetSetHidden stays static" work
/// while "widgetSetHidden inside an onClick closure earns a binding"
/// also work — same module, different scope.
pub(crate) fn walk_init_for_closure_targets(
    stmts: &[Stmt],
    out: &mut std::collections::BTreeSet<LocalId>,
) {
    for stmt in stmts {
        walk_init_for_closure_targets_in_stmt(stmt, out);
    }
}

pub(crate) fn walk_init_for_closure_targets_in_stmt(
    stmt: &Stmt,
    out: &mut std::collections::BTreeSet<LocalId>,
) {
    match stmt {
        Stmt::Expr(e) | Stmt::Let { init: Some(e), .. } | Stmt::Return(Some(e)) => {
            walk_init_for_closure_targets_in_expr(e, out);
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            walk_init_for_closure_targets_in_expr(condition, out);
            walk_init_for_closure_targets(then_branch, out);
            if let Some(eb) = else_branch {
                walk_init_for_closure_targets(eb, out);
            }
        }
        Stmt::While {
            condition, body, ..
        }
        | Stmt::DoWhile {
            body, condition, ..
        } => {
            walk_init_for_closure_targets_in_expr(condition, out);
            walk_init_for_closure_targets(body, out);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
            ..
        } => {
            if let Some(i) = init {
                walk_init_for_closure_targets_in_stmt(i.as_ref(), out);
            }
            if let Some(c) = condition {
                walk_init_for_closure_targets_in_expr(c, out);
            }
            if let Some(u) = update {
                walk_init_for_closure_targets_in_expr(u, out);
            }
            walk_init_for_closure_targets(body, out);
        }
        _ => {}
    }
}

pub(crate) fn walk_init_for_closure_targets_in_expr(
    e: &Expr,
    out: &mut std::collections::BTreeSet<LocalId>,
) {
    // The whole point: a Closure body switches to the unrestricted walker.
    if let Expr::Closure { body, .. } = e {
        walk_for_set_hidden_targets_in_stmts(body, out);
        return;
    }
    // Otherwise descend without recording.
    match e {
        Expr::Call { callee, args, .. } => {
            walk_init_for_closure_targets_in_expr(callee, out);
            for a in args {
                walk_init_for_closure_targets_in_expr(a, out);
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                walk_init_for_closure_targets_in_expr(o, out);
            }
            for a in args {
                walk_init_for_closure_targets_in_expr(a, out);
            }
        }
        Expr::PropertyGet { object, .. } => {
            walk_init_for_closure_targets_in_expr(object, out);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            walk_init_for_closure_targets_in_expr(condition, out);
            walk_init_for_closure_targets_in_expr(then_expr, out);
            walk_init_for_closure_targets_in_expr(else_expr, out);
        }
        Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } => {
            walk_init_for_closure_targets_in_expr(left, out);
            walk_init_for_closure_targets_in_expr(right, out);
        }
        Expr::Unary { operand, .. } => {
            walk_init_for_closure_targets_in_expr(operand, out);
        }
        Expr::Array(items) => {
            for i in items {
                walk_init_for_closure_targets_in_expr(i, out);
            }
        }
        Expr::Object(props) => {
            for (_, v) in props {
                walk_init_for_closure_targets_in_expr(v, out);
            }
        }
        Expr::New { args, .. } => {
            for a in args {
                walk_init_for_closure_targets_in_expr(a, out);
            }
        }
        _ => {}
    }
}

pub(crate) fn walk_for_set_hidden_targets_in_stmt(
    stmt: &Stmt,
    out: &mut std::collections::BTreeSet<LocalId>,
) {
    match stmt {
        Stmt::Expr(e) | Stmt::Let { init: Some(e), .. } | Stmt::Return(Some(e)) => {
            walk_for_set_hidden_targets_in_expr(e, out);
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            walk_for_set_hidden_targets_in_expr(condition, out);
            walk_for_set_hidden_targets_in_stmts(then_branch, out);
            if let Some(eb) = else_branch {
                walk_for_set_hidden_targets_in_stmts(eb, out);
            }
        }
        Stmt::While {
            condition, body, ..
        }
        | Stmt::DoWhile {
            body, condition, ..
        } => {
            walk_for_set_hidden_targets_in_expr(condition, out);
            walk_for_set_hidden_targets_in_stmts(body, out);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
            ..
        } => {
            if let Some(i) = init {
                walk_for_set_hidden_targets_in_stmt(i.as_ref(), out);
            }
            if let Some(c) = condition {
                walk_for_set_hidden_targets_in_expr(c, out);
            }
            if let Some(u) = update {
                walk_for_set_hidden_targets_in_expr(u, out);
            }
            walk_for_set_hidden_targets_in_stmts(body, out);
        }
        _ => {}
    }
}

pub(crate) fn walk_for_set_hidden_targets_in_expr(
    e: &Expr,
    out: &mut std::collections::BTreeSet<LocalId>,
) {
    // Detect `widgetSetHidden(LocalGet(target), _)` shape first.
    if let Some((target, _)) =
        extract_widget_set_hidden_literal(e).or_else(|| extract_widget_set_hidden_target(e))
    {
        out.insert(target);
    }
    // Recurse into all sub-expressions so closures, nested calls, etc.
    // contribute their targets too.
    match e {
        Expr::Closure { body, .. } => {
            walk_for_set_hidden_targets_in_stmts(body, out);
        }
        Expr::Call { callee, args, .. } => {
            walk_for_set_hidden_targets_in_expr(callee, out);
            for a in args {
                walk_for_set_hidden_targets_in_expr(a, out);
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                walk_for_set_hidden_targets_in_expr(o, out);
            }
            for a in args {
                walk_for_set_hidden_targets_in_expr(a, out);
            }
        }
        Expr::PropertyGet { object, .. } => {
            walk_for_set_hidden_targets_in_expr(object, out);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            walk_for_set_hidden_targets_in_expr(condition, out);
            walk_for_set_hidden_targets_in_expr(then_expr, out);
            walk_for_set_hidden_targets_in_expr(else_expr, out);
        }
        Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } => {
            walk_for_set_hidden_targets_in_expr(left, out);
            walk_for_set_hidden_targets_in_expr(right, out);
        }
        Expr::Unary { operand, .. } => {
            walk_for_set_hidden_targets_in_expr(operand, out);
        }
        Expr::Array(items) => {
            for i in items {
                walk_for_set_hidden_targets_in_expr(i, out);
            }
        }
        Expr::Object(props) => {
            for (_, v) in props {
                walk_for_set_hidden_targets_in_expr(v, out);
            }
        }
        Expr::New { args, .. } => {
            for a in args {
                walk_for_set_hidden_targets_in_expr(a, out);
            }
        }
        _ => {}
    }
}

/// Recognize `widgetSetHidden(LocalGet(target), V)` where V is a literal,
/// returning `(target_id, hide)`. Both the perry/ui native-method shape
/// AND the bare-call shape from non-typed-import paths are accepted.
pub(crate) fn extract_widget_set_hidden_literal(e: &Expr) -> Option<(LocalId, bool)> {
    let (target, val) = extract_widget_set_hidden_call(e)?;
    let hide = match val {
        Expr::Bool(true) => true,
        Expr::Bool(false) => false,
        Expr::Number(n) => *n != 0.0,
        Expr::Integer(n) => *n != 0,
        _ => return None,
    };
    Some((target, hide))
}

/// Recognize `widgetSetHidden(LocalGet(target), _)` — target only, no
/// requirement on the value being a literal. Used in target collection.
pub(crate) fn extract_widget_set_hidden_target(e: &Expr) -> Option<(LocalId, bool)> {
    let (target, _) = extract_widget_set_hidden_call(e)?;
    Some((target, false))
}

pub(crate) fn walk_for_funcref_calls_in_closures_in_stmts(
    stmts: &[Stmt],
    out: &mut std::collections::HashSet<perry_hir::types::FuncId>,
) {
    for stmt in stmts {
        walk_for_funcref_calls_in_closures_in_stmt(stmt, out);
    }
}

pub(crate) fn walk_for_funcref_calls_in_closures_in_stmt(
    stmt: &Stmt,
    out: &mut std::collections::HashSet<perry_hir::types::FuncId>,
) {
    match stmt {
        Stmt::Expr(e) | Stmt::Let { init: Some(e), .. } | Stmt::Return(Some(e)) => {
            walk_for_funcref_calls_in_closures_in_expr(e, out);
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            walk_for_funcref_calls_in_closures_in_stmts(then_branch, out);
            if let Some(eb) = else_branch {
                walk_for_funcref_calls_in_closures_in_stmts(eb, out);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            walk_for_funcref_calls_in_closures_in_stmts(body, out);
        }
        Stmt::For { body, .. } => {
            walk_for_funcref_calls_in_closures_in_stmts(body, out);
        }
        _ => {}
    }
}

pub(crate) fn walk_for_funcref_calls_in_closures_in_expr(
    e: &Expr,
    out: &mut std::collections::HashSet<perry_hir::types::FuncId>,
) {
    if let Expr::Closure { body, .. } = e {
        walk_for_funcref_calls_in_body(body, out);
        return;
    }
    match e {
        Expr::Call { callee, args, .. } => {
            walk_for_funcref_calls_in_closures_in_expr(callee, out);
            for a in args {
                walk_for_funcref_calls_in_closures_in_expr(a, out);
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                walk_for_funcref_calls_in_closures_in_expr(o, out);
            }
            for a in args {
                walk_for_funcref_calls_in_closures_in_expr(a, out);
            }
        }
        _ => {}
    }
}

/// Walks the body recording every `Expr::Call { callee: Expr::FuncRef(id) }`
/// — the inverse of the outer "skip into closures" walker. Used to record
/// which functions are called transitively inside a closure body.
pub(crate) fn walk_for_funcref_calls_in_body(
    stmts: &[Stmt],
    out: &mut std::collections::HashSet<perry_hir::types::FuncId>,
) {
    for stmt in stmts {
        walk_for_funcref_calls_in_body_stmt(stmt, out);
    }
}

pub(crate) fn walk_for_funcref_calls_in_body_stmt(
    stmt: &Stmt,
    out: &mut std::collections::HashSet<perry_hir::types::FuncId>,
) {
    match stmt {
        Stmt::Expr(e) | Stmt::Let { init: Some(e), .. } | Stmt::Return(Some(e)) => {
            walk_for_funcref_calls_in_body_expr(e, out);
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            walk_for_funcref_calls_in_body(then_branch, out);
            if let Some(eb) = else_branch {
                walk_for_funcref_calls_in_body(eb, out);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            walk_for_funcref_calls_in_body(body, out);
        }
        Stmt::For { body, .. } => {
            walk_for_funcref_calls_in_body(body, out);
        }
        _ => {}
    }
}

pub(crate) fn walk_for_funcref_calls_in_body_expr(
    e: &Expr,
    out: &mut std::collections::HashSet<perry_hir::types::FuncId>,
) {
    if let Expr::Call { callee, args, .. } = e {
        if let Expr::FuncRef(id) = callee.as_ref() {
            out.insert(*id);
        }
        walk_for_funcref_calls_in_body_expr(callee, out);
        for a in args {
            walk_for_funcref_calls_in_body_expr(a, out);
        }
        return;
    }
    match e {
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                walk_for_funcref_calls_in_body_expr(o, out);
            }
            for a in args {
                walk_for_funcref_calls_in_body_expr(a, out);
            }
        }
        Expr::Closure { body, .. } => {
            walk_for_funcref_calls_in_body(body, out);
        }
        _ => {}
    }
}

/// Variant that walks ONLY top-level Stmts (skips into closures and
/// nested functions). Used to find functions called from module.init's
/// top-level (Phase A inlining sees these and inlines them).
pub(crate) fn walk_for_funcref_calls_top_level_in_stmts(
    stmts: &[Stmt],
    out: &mut std::collections::HashSet<perry_hir::types::FuncId>,
) {
    for stmt in stmts {
        if let Stmt::Expr(e) = stmt {
            if let Expr::Call { callee, .. } = e {
                if let Expr::FuncRef(id) = callee.as_ref() {
                    out.insert(*id);
                }
            }
        }
    }
}

pub(crate) fn extract_widget_set_hidden_call(e: &Expr) -> Option<(LocalId, &Expr)> {
    match e {
        Expr::NativeMethodCall {
            module,
            method,
            args,
            object: None,
            ..
        } if module == "perry/ui" && method == "widgetSetHidden" && args.len() == 2 => {
            if let Expr::LocalGet(id) = &args[0] {
                return Some((*id, &args[1]));
            }
            None
        }
        _ => None,
    }
}
