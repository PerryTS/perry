//! Which parameters keep the value they were called with for the whole call.
//!
//! A parameter is UNREBOUND when nothing in its function can give the binding
//! another value after entry. Every read of it then yields the argument value
//! (or its default), so a read stands in for an earlier read of the same
//! binding. `hoist_compound_member_assign` relies on that to name the
//! parameter itself as the receiver of `a[k] op= v` instead of copying it into
//! a temp (the same reasoning as for a `const` binding, and a parameter in the
//! body has no TDZ).
//!
//! The scan is syntactic and by name, over the parameter list and the whole
//! body, nested functions included (a closure can assign a captured
//! parameter). A name counts as rebound when it appears as
//!
//! * any binding identifier: an assignment or destructuring-assignment target,
//!   a `var`/`let`/`const`/catch/parameter binding (a `var` re-declaration
//!   reuses the parameter's binding, see #11802), a `for (a of …)` head;
//! * the operand of `++`/`--`;
//! * a function declaration's name (it re-initializes the binding at entry,
//!   and a block-nested one assigns it under Annex B).
//!
//! Shadowing is not resolved, which can only make the answer more
//! conservative. Two things make every parameter of the function rebound:
//!
//! * `arguments` named anywhere. A sloppy function with a simple parameter
//!   list has a MAPPED arguments object, and `arguments[0] = v` assigns the
//!   first parameter. Any mention counts, strict or not, since the object can
//!   escape before it is written.
//! * `eval` named anywhere: a direct eval can assign any binding in scope,
//!   in strict code too.
//!
//! Type annotations are not scanned (a function type's parameter names bind
//! nothing).

use std::collections::HashSet;

use swc_ecma_ast as ast;
use swc_ecma_visit::{Visit, VisitWith};

use crate::ir::Param;
use crate::lower::LoweringContext;

#[derive(Default)]
pub(crate) struct ParamRebinds {
    names: HashSet<String>,
    /// `arguments` or `eval` was named: no parameter is unrebound.
    opaque: bool,
}

impl ParamRebinds {
    /// Scan a parameter list. A parameter's own top-level binding identifier
    /// is its declaration, not a rebinding; everything else (defaults,
    /// destructuring patterns and the closures inside them) is scanned.
    pub(crate) fn scan_params<'a>(&mut self, pats: impl IntoIterator<Item = &'a ast::Pat>) {
        for pat in pats {
            match pat {
                ast::Pat::Ident(b) => self.note_ident(&b.id),
                ast::Pat::Assign(a) if matches!(&*a.left, ast::Pat::Ident(_)) => {
                    if let ast::Pat::Ident(b) = &*a.left {
                        self.note_ident(&b.id);
                    }
                    a.right.visit_with(self);
                }
                ast::Pat::Rest(r) if matches!(&*r.arg, ast::Pat::Ident(_)) => {
                    if let ast::Pat::Ident(b) = &*r.arg {
                        self.note_ident(&b.id);
                    }
                }
                other => other.visit_with(self),
            }
        }
    }

    /// Record every parameter of `params` that the scanned code cannot rebind.
    pub(crate) fn commit(self, ctx: &mut LoweringContext, params: &[Param]) {
        if self.opaque {
            return;
        }
        for p in params {
            if p.arguments_object.is_none() && !self.names.contains(&p.name) {
                ctx.unrebound_params.insert(p.id);
            }
        }
    }

    fn note_ident(&mut self, ident: &ast::Ident) {
        if matches!(&*ident.sym, "arguments" | "eval") {
            self.opaque = true;
        }
    }

    fn note_target(&mut self, expr: &ast::Expr) {
        if let Some(ident) = target_ident(expr) {
            self.note_ident(ident);
            self.names.insert(ident.sym.to_string());
        }
    }
}

/// The identifier an assignment or update target names, through parentheses
/// and TypeScript expression wrappers (`(a as any) = v`, `a!++`).
fn target_ident(mut expr: &ast::Expr) -> Option<&ast::Ident> {
    loop {
        expr = match expr {
            ast::Expr::Ident(i) => return Some(i),
            ast::Expr::Paren(p) => &p.expr,
            ast::Expr::TsAs(e) => &e.expr,
            ast::Expr::TsSatisfies(e) => &e.expr,
            ast::Expr::TsNonNull(e) => &e.expr,
            ast::Expr::TsTypeAssertion(e) => &e.expr,
            ast::Expr::TsConstAssertion(e) => &e.expr,
            ast::Expr::TsInstantiation(e) => &e.expr,
            _ => return None,
        };
    }
}

impl Visit for ParamRebinds {
    fn visit_ident(&mut self, ident: &ast::Ident) {
        self.note_ident(ident);
    }

    fn visit_binding_ident(&mut self, b: &ast::BindingIdent) {
        self.note_ident(&b.id);
        self.names.insert(b.id.sym.to_string());
    }

    fn visit_update_expr(&mut self, u: &ast::UpdateExpr) {
        self.note_target(&u.arg);
        u.visit_children_with(self);
    }

    fn visit_simple_assign_target(&mut self, t: &ast::SimpleAssignTarget) {
        match t {
            ast::SimpleAssignTarget::Paren(p) => self.note_target(&p.expr),
            ast::SimpleAssignTarget::TsAs(e) => self.note_target(&e.expr),
            ast::SimpleAssignTarget::TsSatisfies(e) => self.note_target(&e.expr),
            ast::SimpleAssignTarget::TsNonNull(e) => self.note_target(&e.expr),
            ast::SimpleAssignTarget::TsTypeAssertion(e) => self.note_target(&e.expr),
            ast::SimpleAssignTarget::TsInstantiation(e) => self.note_target(&e.expr),
            _ => {}
        }
        t.visit_children_with(self);
    }

    fn visit_pat(&mut self, pat: &ast::Pat) {
        if let ast::Pat::Expr(e) = pat {
            self.note_target(e);
        }
        pat.visit_children_with(self);
    }

    fn visit_fn_decl(&mut self, decl: &ast::FnDecl) {
        self.note_ident(&decl.ident);
        self.names.insert(decl.ident.sym.to_string());
        decl.visit_children_with(self);
    }

    fn visit_ts_type(&mut self, _: &ast::TsType) {}
}

/// Record which of `params` (the function's lowered parameter list) its
/// parameter patterns `pats` and its `body` cannot rebind. Call after the
/// parameters are lowered and before the body is.
pub(crate) fn note<'a, N: VisitWith<ParamRebinds>>(
    ctx: &mut LoweringContext,
    params: &[Param],
    pats: impl IntoIterator<Item = &'a ast::Pat>,
    body: Option<&N>,
) {
    let mut scan = ParamRebinds::default();
    scan.scan_params(pats);
    if let Some(body) = body {
        body.visit_with(&mut scan);
    }
    scan.commit(ctx, params);
}

#[cfg(test)]
#[path = "unrebound_params_tests.rs"]
mod tests;
