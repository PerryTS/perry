//! The class templates this module evaluates once per evaluation
//! (`Expr::ClassExprFresh`): each evaluation creates its own class object, and
//! the template's static method function objects are at home in it
//! (`js_static_method_entry_enter_home`).

use perry_hir::{Expr, Stmt};
use std::collections::HashSet;

/// Every `ClassExprFresh` template named anywhere in `hir`: module init,
/// function bodies, class members and nested closures.
pub(crate) fn fresh_class_templates(hir: &perry_hir::Module) -> HashSet<String> {
    fn visit_expr(e: &Expr, out: &mut HashSet<String>) {
        if let Expr::ClassExprFresh { template, .. } = e {
            out.insert(template.clone());
        }
        if let Expr::Closure { body, .. } = e {
            visit_body(body, out);
        }
        perry_hir::walker::walk_expr_children(e, &mut |c| visit_expr(c, out));
    }
    fn visit_body(body: &[Stmt], out: &mut HashSet<String>) {
        for s in body {
            perry_hir::walker::stmt_any_expr(s, &mut |e| {
                visit_expr(e, out);
                false
            });
        }
    }
    let mut out = HashSet::new();
    visit_body(&hir.init, &mut out);
    for f in &hir.functions {
        visit_body(&f.body, &mut out);
    }
    for class in &hir.classes {
        if let Some(ctor) = &class.constructor {
            visit_body(&ctor.body, &mut out);
        }
        for f in class
            .methods
            .iter()
            .chain(class.static_methods.iter())
            .chain(class.getters.iter().map(|(_, f)| f))
            .chain(class.setters.iter().map(|(_, f)| f))
        {
            visit_body(&f.body, &mut out);
        }
        for field in class.fields.iter().chain(class.static_fields.iter()) {
            if let Some(init) = &field.init {
                visit_expr(init, &mut out);
            }
        }
    }
    out
}
