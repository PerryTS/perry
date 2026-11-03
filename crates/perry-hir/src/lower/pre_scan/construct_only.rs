//! Functions whose every reference is the callee of a `new` expression.
//!
//! A sloppy function's `this` is the global object only when the function can
//! be called with an undefined receiver. A function declaration whose binding
//! is read nowhere except as `new F(...)` is never called any other way, so
//! its `this` is always a fresh object. Feature detection reads the fact
//! (`Function::construct_only`, `Expr::Closure::construct_only`) to decide
//! whether a sloppy `this` handed to code lets the global object escape.
//!
//! The proof runs on the module with its bindings resolved (swc `resolver`),
//! so a shadowing parameter or local is another binding. It is deliberately
//! over-approximate (a false "no" costs only the old behaviour):
//!
//! - only a declaration directly in a module or function body counts (a
//!   block-level declaration has an Annex B outer `var` copy);
//! - any other occurrence of the binding disqualifies it: a call, `F.call`,
//!   `F.prototype`, an export (`export function F`, `export { F }`,
//!   `module.exports = F`), an assignment, an argument, `extends F`;
//! - a module with a direct `eval`, an `arguments.callee` or a read of a
//!   `.constructor` property (an instance's `constructor` is the function, and
//!   a detached call of it has no receiver) claims nothing. Only the literal
//!   spellings count: a key computed at run time that happens to be
//!   `"constructor"` is not followed.
//!
//! The result is the set of function spans `(lo, hi)`, the key lowering uses
//! for run-once classes as well.

use std::collections::{HashMap, HashSet};
use swc_common::{Globals, Mark, GLOBALS};
use swc_ecma_ast as ast;
use swc_ecma_visit::{Visit, VisitMutWith, VisitWith};

pub(crate) fn construct_only_fn_spans(ast_module: &ast::Module) -> HashSet<(u32, u32)> {
    // Cheap by-name gate: most modules declare no function that is called
    // with `new`, and the resolver walk is the expensive part.
    let mut gate = Gate::default();
    ast_module.visit_with(&mut gate);
    if !gate.has_new_ident || gate.poisoned {
        return HashSet::new();
    }
    GLOBALS.set(&Globals::new(), || {
        let mut module = ast_module.clone();
        module.visit_mut_with(&mut swc_ecma_transforms_base::resolver(
            Mark::new(),
            Mark::new(),
            false,
        ));
        let mut scan = Scan::default();
        scan.scope_items(&module.body);
        scan.candidates
            .into_iter()
            .filter(|(id, _)| !scan.other_use.contains(id))
            .map(|(_, span)| span)
            .collect()
    })
}

#[derive(Default)]
struct Gate {
    has_new_ident: bool,
    poisoned: bool,
}

impl Visit for Gate {
    fn visit_new_expr(&mut self, n: &ast::NewExpr) {
        if matches!(&*n.callee, ast::Expr::Ident(_)) {
            self.has_new_ident = true;
        }
        n.visit_children_with(self);
    }
    fn visit_call_expr(&mut self, n: &ast::CallExpr) {
        if let ast::Callee::Expr(e) = &n.callee {
            if matches!(&**e, ast::Expr::Ident(i) if &*i.sym == "eval") {
                self.poisoned = true;
            }
        }
        n.visit_children_with(self);
    }
    fn visit_member_expr(&mut self, n: &ast::MemberExpr) {
        match &n.prop {
            ast::MemberProp::Ident(p) if &*p.sym == "constructor" || &*p.sym == "callee" => {
                self.poisoned = true
            }
            ast::MemberProp::Computed(c) => {
                if matches!(&*c.expr, ast::Expr::Lit(ast::Lit::Str(s))
                    if &*s.value == "constructor" || &*s.value == "callee")
                {
                    self.poisoned = true;
                }
            }
            _ => {}
        }
        n.visit_children_with(self);
    }
    // `{ constructor }` destructuring reads the property too.
    fn visit_key_value_pat_prop(&mut self, n: &ast::KeyValuePatProp) {
        if matches!(&n.key, ast::PropName::Ident(i) if &*i.sym == "constructor") {
            self.poisoned = true;
        }
        n.visit_children_with(self);
    }
    fn visit_assign_pat_prop(&mut self, n: &ast::AssignPatProp) {
        if &*n.key.sym == "constructor" {
            self.poisoned = true;
        }
        n.visit_children_with(self);
    }
}

#[derive(Default)]
struct Scan {
    /// Declarations directly in a module or function body.
    candidates: HashMap<ast::Id, (u32, u32)>,
    /// Bindings with any occurrence other than the declaration or a `new` callee.
    other_use: HashSet<ast::Id>,
}

impl Scan {
    fn scope_items(&mut self, items: &[ast::ModuleItem]) {
        for item in items {
            match item {
                ast::ModuleItem::Stmt(s) => self.scope_stmt(s),
                other => other.visit_with(self),
            }
        }
    }
    fn scope_stmt(&mut self, stmt: &ast::Stmt) {
        if let ast::Stmt::Decl(ast::Decl::Fn(f)) = stmt {
            let span = f.function.span;
            self.candidates
                .insert(f.ident.to_id(), (span.lo.0, span.hi.0));
            f.function.visit_with(self);
        } else {
            stmt.visit_with(self);
        }
    }
}

impl Visit for Scan {
    fn visit_ident(&mut self, i: &ast::Ident) {
        self.other_use.insert(i.to_id());
    }
    fn visit_new_expr(&mut self, n: &ast::NewExpr) {
        // The callee identifier itself is the one allowed occurrence.
        if !matches!(&*n.callee, ast::Expr::Ident(_)) {
            n.callee.visit_with(self);
        }
        n.args.visit_with(self);
        n.type_args.visit_with(self);
    }
    fn visit_function(&mut self, f: &ast::Function) {
        f.params.visit_with(self);
        f.decorators.visit_with(self);
        if let Some(body) = &f.body {
            for stmt in &body.stmts {
                self.scope_stmt(stmt);
            }
        }
    }
}
