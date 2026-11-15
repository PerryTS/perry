//! Which module-scope names are assigned anywhere in the module.
//!
//! A top-level `function f` stays a bare `FuncRef` (direct calls) only while
//! nothing can store into its binding. Any store does: a plain or compound
//! assignment, `++`/`--`, a destructuring target, or a `for (f of ...)` /
//! `for (f in ...)` head, in module code or in any nested function, arrow,
//! class member or block. Such a binding is a mutable module local seeded
//! with the function (see `lower_module_fn.rs`).
//!
//! An assignment counts for the module binding unless an enclosing scope
//! declares the same name (parameters, `var` anywhere in the function body,
//! `let`/`const`/`class`/`function` directly in a block, a catch parameter,
//! a `for` head declaration, the name of a function or class expression, a
//! namespace's own declarations). Every other case counts, so the answer can
//! only over-approximate.

use std::collections::HashSet;
use swc_ecma_ast as ast;
use swc_ecma_visit::{Visit, VisitWith};

use crate::lower_decl::{collect_var_binding_names_from_pat, collect_var_binding_names_from_stmt};

/// Names whose module-scope binding is a store target somewhere in `module`.
/// A plain `f = f` stores the value the binding already holds and is skipped.
pub(super) fn collect_module_assigned_names(module: &ast::Module) -> HashSet<String> {
    let mut scan = Scan {
        scopes: Vec::new(),
        out: HashSet::new(),
    };
    module.visit_with(&mut scan);
    scan.out
}

struct Scan {
    scopes: Vec<HashSet<String>>,
    out: HashSet<String>,
}

fn pat_names(pat: &ast::Pat, out: &mut HashSet<String>) {
    let mut names = Vec::new();
    collect_var_binding_names_from_pat(pat, &mut names);
    out.extend(names);
}

fn var_names(stmts: &[ast::Stmt], out: &mut HashSet<String>) {
    let mut names = Vec::new();
    for stmt in stmts {
        collect_var_binding_names_from_stmt(stmt, &mut names);
    }
    out.extend(names);
}

fn decl_names(decl: &ast::Decl, lexical_only: bool, out: &mut HashSet<String>) {
    match decl {
        ast::Decl::Var(var) => {
            if !lexical_only || var.kind != ast::VarDeclKind::Var {
                for d in &var.decls {
                    pat_names(&d.name, out);
                }
            }
        }
        ast::Decl::Using(using) => {
            for d in &using.decls {
                pat_names(&d.name, out);
            }
        }
        ast::Decl::Fn(f) => {
            out.insert(f.ident.sym.to_string());
        }
        ast::Decl::Class(c) => {
            out.insert(c.ident.sym.to_string());
        }
        ast::Decl::TsEnum(e) => {
            out.insert(e.id.sym.to_string());
        }
        ast::Decl::TsModule(m) => {
            if let ast::TsModuleName::Ident(id) = &m.id {
                out.insert(id.sym.to_string());
            }
        }
        ast::Decl::TsInterface(_) | ast::Decl::TsTypeAlias(_) => {}
    }
}

/// Block-scoped declarations directly in `stmts`.
fn lexical_names(stmts: &[ast::Stmt]) -> HashSet<String> {
    let mut out = HashSet::new();
    for stmt in stmts {
        if let ast::Stmt::Decl(decl) = stmt {
            decl_names(decl, true, &mut out);
        }
    }
    out
}

/// Parameters plus every `var` of a function-like body.
fn function_names<'a>(
    params: impl IntoIterator<Item = &'a ast::Pat>,
    body: Option<&[ast::Stmt]>,
) -> HashSet<String> {
    let mut out = HashSet::new();
    for pat in params {
        pat_names(pat, &mut out);
    }
    if let Some(body) = body {
        var_names(body, &mut out);
    }
    out
}

impl Scan {
    fn scoped(&mut self, names: HashSet<String>, f: impl FnOnce(&mut Self)) {
        if names.is_empty() {
            return f(self);
        }
        self.scopes.push(names);
        f(self);
        self.scopes.pop();
    }

    fn record(&mut self, name: &str) {
        if !self.scopes.iter().any(|scope| scope.contains(name)) {
            self.out.insert(name.to_string());
        }
    }

    fn record_pat(&mut self, pat: &ast::Pat) {
        match pat {
            ast::Pat::Ident(ident) => self.record(&ident.id.sym),
            ast::Pat::Array(array) => {
                for elem in array.elems.iter().flatten() {
                    self.record_pat(elem);
                }
            }
            ast::Pat::Object(object) => self.record_object_pat(object),
            ast::Pat::Assign(assign) => self.record_pat(&assign.left),
            ast::Pat::Rest(rest) => self.record_pat(&rest.arg),
            ast::Pat::Expr(expr) => self.record_expr(expr),
            ast::Pat::Invalid(_) => {}
        }
    }

    fn record_object_pat(&mut self, object: &ast::ObjectPat) {
        for prop in &object.props {
            match prop {
                ast::ObjectPatProp::KeyValue(kv) => self.record_pat(&kv.value),
                ast::ObjectPatProp::Assign(assign) => self.record(&assign.key.sym),
                ast::ObjectPatProp::Rest(rest) => self.record_pat(&rest.arg),
            }
        }
    }

    /// An expression in store position: an identifier, possibly wrapped in
    /// parentheses or TypeScript assertions. Member targets store elsewhere.
    fn record_expr(&mut self, expr: &ast::Expr) {
        match expr {
            ast::Expr::Ident(ident) => self.record(&ident.sym),
            ast::Expr::Paren(e) => self.record_expr(&e.expr),
            ast::Expr::TsAs(e) => self.record_expr(&e.expr),
            ast::Expr::TsNonNull(e) => self.record_expr(&e.expr),
            ast::Expr::TsSatisfies(e) => self.record_expr(&e.expr),
            ast::Expr::TsTypeAssertion(e) => self.record_expr(&e.expr),
            ast::Expr::TsInstantiation(e) => self.record_expr(&e.expr),
            _ => {}
        }
    }

    fn record_simple(&mut self, target: &ast::SimpleAssignTarget) {
        match target {
            ast::SimpleAssignTarget::Ident(ident) => self.record(&ident.id.sym),
            ast::SimpleAssignTarget::Paren(e) => self.record_expr(&e.expr),
            ast::SimpleAssignTarget::TsAs(e) => self.record_expr(&e.expr),
            ast::SimpleAssignTarget::TsNonNull(e) => self.record_expr(&e.expr),
            ast::SimpleAssignTarget::TsSatisfies(e) => self.record_expr(&e.expr),
            ast::SimpleAssignTarget::TsTypeAssertion(e) => self.record_expr(&e.expr),
            ast::SimpleAssignTarget::TsInstantiation(e) => self.record_expr(&e.expr),
            ast::SimpleAssignTarget::Member(_)
            | ast::SimpleAssignTarget::SuperProp(_)
            | ast::SimpleAssignTarget::OptChain(_)
            | ast::SimpleAssignTarget::Invalid(_) => {}
        }
    }

    fn record_for_head(&mut self, head: &ast::ForHead) {
        if let ast::ForHead::Pat(pat) = head {
            self.record_pat(pat);
        }
    }

    fn for_head_names(head: &ast::ForHead) -> HashSet<String> {
        let mut out = HashSet::new();
        match head {
            ast::ForHead::VarDecl(var) if var.kind != ast::VarDeclKind::Var => {
                for d in &var.decls {
                    pat_names(&d.name, &mut out);
                }
            }
            ast::ForHead::UsingDecl(using) => {
                for d in &using.decls {
                    pat_names(&d.name, &mut out);
                }
            }
            _ => {}
        }
        out
    }
}

impl Visit for Scan {
    fn visit_ts_type(&mut self, _: &ast::TsType) {}

    fn visit_assign_expr(&mut self, assign: &ast::AssignExpr) {
        match &assign.left {
            ast::AssignTarget::Simple(ast::SimpleAssignTarget::Ident(ident))
                if assign.op == ast::AssignOp::Assign
                    && matches!(
                        assign.right.as_ref(),
                        ast::Expr::Ident(rhs) if rhs.sym == ident.id.sym
                    ) => {}
            ast::AssignTarget::Simple(simple) => self.record_simple(simple),
            ast::AssignTarget::Pat(ast::AssignTargetPat::Array(array)) => {
                for elem in array.elems.iter().flatten() {
                    self.record_pat(elem);
                }
            }
            ast::AssignTarget::Pat(ast::AssignTargetPat::Object(object)) => {
                self.record_object_pat(object)
            }
            ast::AssignTarget::Pat(ast::AssignTargetPat::Invalid(_)) => {}
        }
        assign.visit_children_with(self);
    }

    fn visit_update_expr(&mut self, update: &ast::UpdateExpr) {
        self.record_expr(&update.arg);
        update.visit_children_with(self);
    }

    fn visit_function(&mut self, function: &ast::Function) {
        let names = function_names(
            function.params.iter().map(|p| &p.pat),
            function.body.as_ref().map(|b| b.stmts.as_slice()),
        );
        self.scoped(names, |s| function.visit_children_with(s));
    }

    fn visit_arrow_expr(&mut self, arrow: &ast::ArrowExpr) {
        let body = match arrow.body.as_ref() {
            ast::ArrowFunctionBody::FunctionBody(body) => Some(body.stmts.as_slice()),
            _ => None,
        };
        let names = function_names(arrow.params.iter(), body);
        self.scoped(names, |s| arrow.visit_children_with(s));
    }

    fn visit_constructor(&mut self, ctor: &ast::Constructor) {
        let mut names = function_names(
            std::iter::empty(),
            ctor.body.as_ref().map(|b| b.stmts.as_slice()),
        );
        for param in &ctor.params {
            match param {
                ast::ParamOrTsParamProp::Param(p) => pat_names(&p.pat, &mut names),
                ast::ParamOrTsParamProp::TsParamProp(prop) => match &prop.param {
                    ast::TsParamPropParam::Ident(ident) => {
                        names.insert(ident.id.sym.to_string());
                    }
                    ast::TsParamPropParam::Assign(assign) => pat_names(&assign.left, &mut names),
                },
            }
        }
        self.scoped(names, |s| ctor.visit_children_with(s));
    }

    fn visit_static_block(&mut self, block: &ast::StaticBlock) {
        let names = function_names(std::iter::empty(), Some(block.body.stmts.as_slice()));
        self.scoped(names, |s| block.visit_children_with(s));
    }

    fn visit_fn_expr(&mut self, function: &ast::FnExpr) {
        let names = function.ident.iter().map(|i| i.sym.to_string()).collect();
        self.scoped(names, |s| function.visit_children_with(s));
    }

    fn visit_class_expr(&mut self, class: &ast::ClassExpr) {
        let names = class.ident.iter().map(|i| i.sym.to_string()).collect();
        self.scoped(names, |s| class.visit_children_with(s));
    }

    fn visit_function_body(&mut self, body: &ast::FunctionBody) {
        let names = lexical_names(&body.stmts);
        self.scoped(names, |s| body.visit_children_with(s));
    }

    fn visit_block_stmt(&mut self, block: &ast::BlockStmt) {
        let names = lexical_names(&block.stmts);
        self.scoped(names, |s| block.visit_children_with(s));
    }

    fn visit_switch_stmt(&mut self, switch: &ast::SwitchStmt) {
        switch.discriminant.visit_with(self);
        let mut names = HashSet::new();
        for case in &switch.cases {
            names.extend(lexical_names(&case.cons));
        }
        self.scoped(names, |s| switch.cases.visit_with(s));
    }

    fn visit_catch_clause(&mut self, clause: &ast::CatchClause) {
        let mut names = HashSet::new();
        if let Some(param) = &clause.param {
            pat_names(param, &mut names);
        }
        self.scoped(names, |s| clause.visit_children_with(s));
    }

    fn visit_for_stmt(&mut self, stmt: &ast::ForStmt) {
        let mut names = HashSet::new();
        if let Some(ast::VarDeclOrExpr::VarDecl(var)) = &stmt.init {
            if var.kind != ast::VarDeclKind::Var {
                for d in &var.decls {
                    pat_names(&d.name, &mut names);
                }
            }
        }
        self.scoped(names, |s| stmt.visit_children_with(s));
    }

    fn visit_for_in_stmt(&mut self, stmt: &ast::ForInStmt) {
        self.record_for_head(&stmt.left);
        let names = Self::for_head_names(&stmt.left);
        self.scoped(names, |s| stmt.visit_children_with(s));
    }

    fn visit_for_of_stmt(&mut self, stmt: &ast::ForOfStmt) {
        self.record_for_head(&stmt.left);
        let names = Self::for_head_names(&stmt.left);
        self.scoped(names, |s| stmt.visit_children_with(s));
    }

    fn visit_ts_module_block(&mut self, block: &ast::TsModuleBlock) {
        let mut names = HashSet::new();
        for item in &block.body {
            match item {
                ast::ModuleItem::Stmt(ast::Stmt::Decl(decl)) => decl_names(decl, false, &mut names),
                ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export)) => {
                    decl_names(&export.decl, false, &mut names)
                }
                ast::ModuleItem::Stmt(stmt) => {
                    let mut vars = Vec::new();
                    collect_var_binding_names_from_stmt(stmt, &mut vars);
                    names.extend(vars);
                }
                _ => {}
            }
        }
        self.scoped(names, |s| block.visit_children_with(s));
    }
}

#[cfg(test)]
mod tests {
    use super::collect_module_assigned_names;

    fn assigned(src: &str) -> Vec<String> {
        let module = perry_parser::parse_typescript(src, "t.ts").expect("parse");
        let mut names: Vec<String> = collect_module_assigned_names(&module).into_iter().collect();
        names.sort();
        names
    }

    #[test]
    fn stores_from_any_nesting_count() {
        assert_eq!(assigned("function f(){} function r(){ f = g }"), ["f"]);
        assert_eq!(assigned("function f(){} const r = () => { f = g }"), ["f"]);
        assert_eq!(assigned("function f(){} class C { m(){ f = g } }"), ["f"]);
        assert_eq!(assigned("function f(){} function r(){ f += 'x' }"), ["f"]);
        assert_eq!(assigned("function f(){} function r(){ f++ }"), ["f"]);
        assert_eq!(assigned("function f(){} function r(){ [f] = [g] }"), ["f"]);
        assert_eq!(
            assigned("function f(){} function r(){ ({ f } = o) }"),
            ["f"]
        );
        assert_eq!(
            assigned("function f(){} function r(){ ({ a: [f = 1] } = o) }"),
            ["f"]
        );
        assert_eq!(
            assigned("function f(){} function r(){ for (f of xs); }"),
            ["f"]
        );
        assert_eq!(assigned("f = g; function f(){}"), ["f"]);
        assert_eq!(
            assigned("function f(){} function r(){ (f as any) = g }"),
            ["f"]
        );
        assert_eq!(
            assigned("function f(){} function r(){ { let x; } f = g }"),
            ["f"]
        );
    }

    #[test]
    fn shadowed_stores_do_not_count() {
        assert!(assigned("function f(){} function r(f){ f = g }").is_empty());
        assert!(assigned("function f(){} function r(){ var f; f = g }").is_empty());
        assert!(assigned("function f(){} function r(){ if (x) { var f } f = g }").is_empty());
        assert!(assigned("function f(){} function r(){ let f; f = g }").is_empty());
        assert!(assigned("function f(){} function r(){ f = g; function f(){} }").is_empty());
        assert!(assigned("function f(){} const r = (f) => { f = g }").is_empty());
        assert!(assigned("function f(){} try {} catch (f) { f = g }").is_empty());
        assert!(assigned("function f(){} for (let f of xs) { f = g }").is_empty());
        assert!(assigned("function f(){} function r(){ f = f }").is_empty());
        assert!(assigned("function f(){ return f(); }").is_empty());
    }
}
