//! The class templates this module evaluates once per evaluation
//! (`Expr::ClassExprFresh`): each evaluation creates its own class object, and
//! the template's static method function objects are at home in it
//! (`js_static_method_entry_enter_home`).

use perry_hir::{Expr, Stmt};
use std::collections::HashSet;

/// The template cell of per-evaluation template `cid`: the template's own
/// record, an internal `[N x i64]` the module's string-pool initializer
/// defines and the evaluation site passes to `js_class_evaluation_object`
/// (which names it in the class object's first own key) and
/// `js_class_object_set_ctor_caps`. Codegen writes only its length in words
/// (word 0); the runtime owns the rest of its layout
/// (`class_object_template::TemplateCell`).
pub(crate) fn template_cell_global(cid: u32) -> String {
    format!("perry_ctpl.{cid}")
}

/// How many words template cell of `class` gets: the runtime's fixed words
/// plus two per slot of the class object's and the prototype's final shapes
/// (the template key, `length`, `name`, one per static method and the pinned
/// parent; `constructor` and one per method). The runtime checks every record
/// against the length, so a short cell only declines to memoize.
pub(crate) fn template_cell_words(class: Option<&perry_hir::Class>) -> usize {
    32 + 2 * (5 + class.map_or(16, |c| c.static_methods.len() + c.methods.len()))
}

/// Every `ClassExprFresh` template named anywhere in `hir`: module init,
/// function bodies, class members and nested closures.
pub(crate) fn fresh_class_templates(hir: &perry_hir::Module) -> HashSet<String> {
    templates_where(hir, |_| true)
}

/// The templates whose first evaluation is the shared class (#11759 (c′),
/// `ClassExprFresh { shared_first_evaluation: Some(_) }`) or that a
/// `ClassIsFirstEvaluation` names, in a stable order.
pub(crate) fn shared_first_templates(hir: &perry_hir::Module) -> Vec<String> {
    let mut out: Vec<String> = collect(hir, &mut |e, out| match e {
        Expr::ClassExprFresh {
            template,
            shared_first_evaluation: Some(_),
            ..
        }
        | Expr::ClassIsFirstEvaluation { template, .. } => {
            out.insert(template.clone());
        }
        _ => {}
    })
    .into_iter()
    .collect();
    out.sort();
    out
}

fn templates_where(hir: &perry_hir::Module, wanted: fn(&Expr) -> bool) -> HashSet<String> {
    let visit_expr = &mut |e: &Expr, out: &mut HashSet<String>| {
        if let Expr::ClassExprFresh { template, .. } = e {
            if wanted(e) {
                out.insert(template.clone());
            }
        }
    };
    collect(hir, visit_expr)
}

fn collect(
    hir: &perry_hir::Module,
    on_expr: &mut dyn FnMut(&Expr, &mut HashSet<String>),
) -> HashSet<String> {
    fn visit_expr(
        e: &Expr,
        out: &mut HashSet<String>,
        on_expr: &mut dyn FnMut(&Expr, &mut HashSet<String>),
    ) {
        on_expr(e, out);
        if let Expr::Closure { body, .. } = e {
            visit_body(body, out, on_expr);
        }
        perry_hir::walker::walk_expr_children(e, &mut |c| visit_expr(c, out, on_expr));
    }
    fn visit_body(
        body: &[Stmt],
        out: &mut HashSet<String>,
        on_expr: &mut dyn FnMut(&Expr, &mut HashSet<String>),
    ) {
        for s in body {
            perry_hir::walker::stmt_any_expr(s, &mut |e| {
                visit_expr(e, out, on_expr);
                false
            });
        }
    }
    let mut out = HashSet::new();
    visit_body(&hir.init, &mut out, on_expr);
    for f in &hir.functions {
        visit_body(&f.body, &mut out, on_expr);
    }
    for class in &hir.classes {
        if let Some(ctor) = &class.constructor {
            visit_body(&ctor.body, &mut out, on_expr);
        }
        for f in class
            .methods
            .iter()
            .chain(class.static_methods.iter())
            .chain(class.getters.iter().map(|(_, f)| f))
            .chain(class.setters.iter().map(|(_, f)| f))
        {
            visit_body(&f.body, &mut out, on_expr);
        }
        for field in class.fields.iter().chain(class.static_fields.iter()) {
            if let Some(init) = &field.init {
                visit_expr(init, &mut out, on_expr);
            }
        }
    }
    out
}
