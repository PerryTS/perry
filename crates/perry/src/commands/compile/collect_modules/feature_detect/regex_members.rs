//! Receiver-aware detection of string methods that implicitly create regexes.
use perry_hir::walker::walk_expr_children;
use perry_hir::{Expr, Module, Stmt};

fn user_method(module: &Module, receiver: &Expr, property: &str) -> bool {
    if let Expr::LocalGet(id) = receiver {
        // Recover only an immutable, directly constructed value from the HIR
        // declaration itself. No type assertions, alias table or memoization.
        let mut known = false;
        perry_hir::tdz_check::for_each_module_stmt_list(module, &mut |stmts| {
            for stmt in stmts {
                if let Stmt::Let {
                    id: binding,
                    mutable: false,
                    init: Some(init @ Expr::New { .. }),
                    ..
                } = stmt
                {
                    if binding == id {
                        known |= user_method(module, init, property);
                    }
                }
            }
        });
        return known
            && !super::module_any_expr(module, &mut |expr| local_escapes(expr, *id, property));
    }
    let Expr::New { class_name, .. } = receiver else {
        return false;
    };
    module.classes.iter().any(|class| {
        // A constructor or parent can return an arbitrary object; decorators
        // can replace the constructor. Do not infer a receiver from a type
        // annotation, which can also hide strings or regexes at runtime.
        class.name == *class_name
            && !class.is_exported
            && class.fields.is_empty()
            && class.static_fields.is_empty()
            && class.getters.is_empty()
            && class.setters.is_empty()
            && class.computed_members.is_empty()
            && class.constructor.as_ref().is_none_or(|constructor| {
                constructor.params.is_empty()
                    && constructor
                        .body
                        .iter()
                        .all(|stmt| matches!(stmt, Stmt::Return(None | Some(Expr::This))))
            })
            && class
                .methods
                .iter()
                .chain(&class.static_methods)
                .all(|method| {
                    method.decorators.is_empty()
                        && !perry_hir::analysis::body_reads_dynamic_this(&method.body)
                        && method.params.iter().all(|param| param.default.is_none())
                })
            && class.extends.is_none()
            && class.extends_name.is_none()
            && class.extends_expr.is_none()
            && class.native_extends.is_none()
            && class.decorators.is_empty()
            && class.methods.iter().any(|method| method.name == property)
    })
}

fn mutable_class_surface(expr: &Expr) -> bool {
    if matches!(expr, Expr::ClassRef(_)) || perry_hir::analysis::expr_is_shape_barrier(expr) {
        return true;
    }
    if let Expr::Closure { body, .. } = expr {
        if super::stmts_any_expr(body, &mut |e| mutable_class_surface(e)) {
            return true;
        }
    }
    let mut found = false;
    walk_expr_children(expr, &mut |child| found |= mutable_class_surface(child));
    found
}

fn local_escapes(expr: &Expr, id: u32, method: &str) -> bool {
    match expr {
        // Only reads/calls of this proven method may use the instance. Passing
        // it elsewhere or assigning one of its fields invalidates the proof.
        Expr::PropertyGet {
            object, property, ..
        } if matches!(object.as_ref(), Expr::LocalGet(local) if *local == id)
            && property == method =>
        {
            return false
        }
        Expr::LocalGet(local) | Expr::LocalSet(local, _) if *local == id => return true,
        Expr::Closure { body, .. }
            if super::stmts_any_expr(body, &mut |e| local_escapes(e, id, method)) =>
        {
            return true
        }
        _ => {}
    }
    let mut found = false;
    walk_expr_children(expr, &mut |child| found |= local_escapes(child, id, method));
    found
}

pub(super) fn needed(module: &Module) -> bool {
    fn expression(module: &Module, expr: &Expr, isolated_classes: bool) -> bool {
        match expr {
            Expr::PropertyGet {
                object, property, ..
            } if matches!(property.as_str(), "search" | "match" | "matchAll")
                && !(isolated_classes && user_method(module, object, property)) =>
            {
                return true
            }
            Expr::Closure { body, .. }
                if super::stmts_any_expr(body, &mut |e| {
                    expression(module, e, isolated_classes)
                }) =>
            {
                return true
            }
            _ => {}
        }
        let mut found = false;
        walk_expr_children(expr, &mut |child| {
            found |= expression(module, child, isolated_classes)
        });
        found
    }
    // A constructor value can expose its prototype to arbitrary code. Compute
    // this module-wide restriction once; the receiver proof itself uses only
    // the declarations, without a per-value side table.
    let isolated_classes = !module.classes.is_empty()
        && !super::global_object::may_escape(module)
        && !super::module_any_expr(module, &mut mutable_class_surface);
    super::module_any_expr(module, &mut |expr| {
        expression(module, expr, isolated_classes)
    })
}
