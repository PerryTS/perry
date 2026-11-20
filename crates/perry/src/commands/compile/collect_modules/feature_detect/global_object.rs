//! Materialized global-object capabilities relevant to namespace selection.
use perry_hir::walker::{stmt_any_expr, walk_expr_children};
use perry_hir::{Expr, Function, Module, Stmt};

fn receiver(expr: &Expr, sloppy: bool) -> bool {
    matches!(expr, Expr::GlobalThisExpr)
        || (sloppy && matches!(expr, Expr::This))
        || matches!(expr, Expr::PropertyGet { object, property, .. }
            if matches!(property.as_str(), "global" | "globalThis")
                && (matches!(object.as_ref(), Expr::GlobalGet(0)) || receiver(object, sloppy)))
}
fn expression(expr: &Expr, sloppy: bool) -> bool {
    match expr {
        Expr::GlobalThisExpr => return true,
        // Zero is also a namespace sentinel during lowering; only the named
        // global/globalThis value accessor proves a materialized global.
        Expr::GlobalGet(0) => return false,
        Expr::PropertyGet {
            object, property, ..
        } if matches!(object.as_ref(), Expr::GlobalGet(0))
            && matches!(property.as_str(), "global" | "globalThis") =>
        {
            return true
        }
        Expr::This => return sloppy,
        Expr::Closure {
            body,
            params,
            is_strict,
            is_arrow,
            ..
        } => {
            let own_sloppy = !is_strict && (!is_arrow || sloppy);
            return statements(body, own_sloppy)
                || params.iter().any(|p| {
                    p.default
                        .as_ref()
                        .is_some_and(|e| expression(e, own_sloppy))
                        || p.decorators
                            .iter()
                            .any(|d| d.args.iter().any(|e| expression(e, false)))
                });
        }
        Expr::PropertyGet {
            object, property, ..
        } if receiver(object, sloppy) => {
            return matches!(property.as_str(), "global" | "globalThis");
        }
        Expr::PropertySet { object, value, .. } if receiver(object, sloppy) => {
            return expression(value, sloppy);
        }
        Expr::PropertyUpdate { object, .. } if receiver(object, sloppy) => return false,
        Expr::IndexGet { object, .. }
        | Expr::IndexSet { object, .. }
        | Expr::IndexUpdate { object, .. }
            if receiver(object, sloppy) =>
        {
            return true
        }
        _ => {}
    }
    let mut found = false;
    walk_expr_children(expr, &mut |child| {
        found |= expression(child, sloppy);
    });
    found
}
fn statements(stmts: &[Stmt], sloppy: bool) -> bool {
    stmts
        .iter()
        .any(|stmt| stmt_any_expr(stmt, &mut |expr| expression(expr, sloppy)))
}
fn function(function: &Function, sloppy: bool) -> bool {
    statements(&function.body, sloppy)
        || function.params.iter().any(|p| {
            p.default.as_ref().is_some_and(|e| expression(e, sloppy))
                || p.decorators
                    .iter()
                    .any(|d| d.args.iter().any(|e| expression(e, false)))
        })
        || function
            .decorators
            .iter()
            .any(|d| d.args.iter().any(|e| expression(e, false)))
}
pub(super) fn may_escape(module: &Module) -> bool {
    statements(&module.init, false)
        || module.functions.iter().any(|f| function(f, !f.is_strict))
        || module.classes.iter().any(|class| {
            class
                .extends_expr
                .as_deref()
                .is_some_and(|e| expression(e, false))
                || class
                    .constructor
                    .iter()
                    .chain(&class.methods)
                    .chain(&class.static_methods)
                    .chain(class.getters.iter().map(|(_, f)| f))
                    .chain(class.setters.iter().map(|(_, f)| f))
                    .any(|f| function(f, false))
                || class.fields.iter().chain(&class.static_fields).any(|f| {
                    f.init.as_ref().is_some_and(|e| expression(e, false))
                        || f.key_expr.as_ref().is_some_and(|e| expression(e, false))
                        || f.decorators
                            .iter()
                            .any(|d| d.args.iter().any(|e| expression(e, false)))
                })
                || class
                    .computed_members
                    .iter()
                    .any(|m| expression(&m.key_expr, false) || function(&m.function, false))
                || class
                    .decorators
                    .iter()
                    .any(|d| d.args.iter().any(|e| expression(e, false)))
        })
}
