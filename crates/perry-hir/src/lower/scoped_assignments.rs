//! Scope-aware scan for identifiers assigned from anywhere in a module.
//!
//! A top-level `function f` normally lowers to a direct `FuncRef`. An
//! assignment to `f` can only take effect when `f` is a mutable local, so the
//! module lowering promotes every function whose name this scan reports.
//! Babel's `_typeof` helper reassigns itself from inside its own body, which is
//! why the scan walks into nested functions, classes, and blocks. A name
//! declared by an enclosing parameter list, `var`, `let`, `const`, `class`,
//! function declaration, catch parameter, or loop head shadows the module-level
//! binding, so an assignment to it is not reported. Parameter properties and
//! setter parameters are not tracked as shadows; that only over-reports.

use std::collections::HashSet;
use swc_ecma_ast as ast;
use swc_ecma_visit::{noop_visit_type, Visit, VisitWith};

/// Names assigned (`=`, compound, destructuring, `++`/`--`, `for (x of ..)`)
/// where no enclosing declaration shadows them.
pub(super) fn collect_scoped_assigned_identifiers(ast_module: &ast::Module) -> HashSet<String> {
    let mut scan = Scan {
        scopes: Vec::new(),
        out: HashSet::new(),
    };
    ast_module.visit_with(&mut scan);
    scan.out
}

struct Scan {
    scopes: Vec<HashSet<String>>,
    out: HashSet<String>,
}

impl Scan {
    fn assigned(&mut self, name: &str) {
        if !self.scopes.iter().any(|scope| scope.contains(name)) {
            self.out.insert(name.to_string());
        }
    }

    fn assigned_names(&mut self, names: Vec<String>) {
        for name in names {
            self.assigned(&name);
        }
    }

    fn scoped(&mut self, names: HashSet<String>, walk: impl FnOnce(&mut Self)) {
        self.scopes.push(names);
        walk(self);
        self.scopes.pop();
    }
}

fn unwrap_ident(expr: &ast::Expr) -> Option<&str> {
    match expr {
        ast::Expr::Ident(ident) => Some(ident.sym.as_ref()),
        ast::Expr::Paren(paren) => unwrap_ident(&paren.expr),
        ast::Expr::TsAs(e) => unwrap_ident(&e.expr),
        ast::Expr::TsNonNull(e) => unwrap_ident(&e.expr),
        ast::Expr::TsSatisfies(e) => unwrap_ident(&e.expr),
        ast::Expr::TsTypeAssertion(e) => unwrap_ident(&e.expr),
        _ => None,
    }
}

fn simple_target_ident(target: &ast::SimpleAssignTarget) -> Option<&str> {
    match target {
        ast::SimpleAssignTarget::Ident(ident) => Some(ident.id.sym.as_ref()),
        ast::SimpleAssignTarget::Paren(paren) => unwrap_ident(&paren.expr),
        ast::SimpleAssignTarget::TsAs(e) => unwrap_ident(&e.expr),
        ast::SimpleAssignTarget::TsNonNull(e) => unwrap_ident(&e.expr),
        ast::SimpleAssignTarget::TsSatisfies(e) => unwrap_ident(&e.expr),
        ast::SimpleAssignTarget::TsTypeAssertion(e) => unwrap_ident(&e.expr),
        _ => None,
    }
}

/// Identifiers written by a destructuring pattern, whether it declares them
/// or assigns to existing bindings.
fn pat_names(pat: &ast::Pat, out: &mut Vec<String>) {
    match pat {
        ast::Pat::Ident(ident) => out.push(ident.id.sym.to_string()),
        ast::Pat::Array(array) => array_pat_names(array, out),
        ast::Pat::Object(object) => object_pat_names(object, out),
        ast::Pat::Assign(assign) => pat_names(&assign.left, out),
        ast::Pat::Rest(rest) => pat_names(&rest.arg, out),
        ast::Pat::Expr(expr) => {
            if let Some(name) = unwrap_ident(expr) {
                out.push(name.to_string());
            }
        }
        ast::Pat::Invalid(_) => {}
    }
}

fn array_pat_names(array: &ast::ArrayPat, out: &mut Vec<String>) {
    for elem in array.elems.iter().flatten() {
        pat_names(elem, out);
    }
}

fn object_pat_names(object: &ast::ObjectPat, out: &mut Vec<String>) {
    for prop in &object.props {
        match prop {
            ast::ObjectPatProp::Assign(assign) => out.push(assign.key.sym.to_string()),
            ast::ObjectPatProp::KeyValue(kv) => pat_names(&kv.value, out),
            ast::ObjectPatProp::Rest(rest) => pat_names(&rest.arg, out),
        }
    }
}

fn var_decl_names(var_decl: &ast::VarDecl, out: &mut HashSet<String>) {
    let mut names = Vec::new();
    for decl in &var_decl.decls {
        pat_names(&decl.name, &mut names);
    }
    out.extend(names);
}

/// Names a statement list declares in its own block scope: `let`, `const`,
/// `class`, and function declarations.
fn lexical_names(stmts: &[ast::Stmt]) -> HashSet<String> {
    let mut out = HashSet::new();
    for stmt in stmts {
        match stmt {
            ast::Stmt::Decl(ast::Decl::Var(var_decl)) if var_decl.kind != ast::VarDeclKind::Var => {
                var_decl_names(var_decl, &mut out);
            }
            ast::Stmt::Decl(ast::Decl::Class(class)) => {
                out.insert(class.ident.sym.to_string());
            }
            ast::Stmt::Decl(ast::Decl::Fn(function)) => {
                out.insert(function.ident.sym.to_string());
            }
            _ => {}
        }
    }
    out
}

/// A function's own scope: parameters plus every `var` in its body.
fn function_scope<'a>(
    params: impl Iterator<Item = &'a ast::Pat>,
    body: Option<&ast::FunctionBody>,
) -> HashSet<String> {
    let mut names = Vec::new();
    for param in params {
        pat_names(param, &mut names);
    }
    if let Some(body) = body {
        for stmt in &body.stmts {
            crate::lower_decl::collect_var_binding_names_from_stmt(stmt, &mut names);
        }
    }
    names.into_iter().collect()
}

impl Visit for Scan {
    noop_visit_type!();

    fn visit_assign_expr(&mut self, assign: &ast::AssignExpr) {
        match &assign.left {
            ast::AssignTarget::Simple(target) => {
                if let Some(name) = simple_target_ident(target) {
                    let is_self_read = assign.op == ast::AssignOp::Assign
                        && unwrap_ident(&assign.right) == Some(name);
                    if !is_self_read {
                        self.assigned(name);
                    }
                }
            }
            ast::AssignTarget::Pat(pat) => {
                let mut names = Vec::new();
                match pat {
                    ast::AssignTargetPat::Array(array) => array_pat_names(array, &mut names),
                    ast::AssignTargetPat::Object(object) => object_pat_names(object, &mut names),
                    ast::AssignTargetPat::Invalid(_) => {}
                }
                self.assigned_names(names);
            }
        }
        assign.visit_children_with(self);
    }

    fn visit_update_expr(&mut self, update: &ast::UpdateExpr) {
        if let Some(name) = unwrap_ident(&update.arg) {
            self.assigned(name);
        }
        update.visit_children_with(self);
    }

    fn visit_function(&mut self, function: &ast::Function) {
        let names = function_scope(
            function.params.iter().map(|param| &param.pat),
            function.body.as_ref(),
        );
        self.scoped(names, |scan| function.visit_children_with(scan));
    }

    fn visit_arrow_expr(&mut self, arrow: &ast::ArrowExpr) {
        let body = match arrow.body.as_ref() {
            ast::ArrowFunctionBody::FunctionBody(body) => Some(body),
            ast::ArrowFunctionBody::Expr(_) => None,
        };
        let names = function_scope(arrow.params.iter(), body);
        self.scoped(names, |scan| arrow.visit_children_with(scan));
    }

    fn visit_fn_expr(&mut self, function: &ast::FnExpr) {
        let names = function
            .ident
            .iter()
            .map(|ident| ident.sym.to_string())
            .collect();
        self.scoped(names, |scan| function.visit_children_with(scan));
    }

    fn visit_class_expr(&mut self, class: &ast::ClassExpr) {
        let names = class
            .ident
            .iter()
            .map(|ident| ident.sym.to_string())
            .collect();
        self.scoped(names, |scan| class.visit_children_with(scan));
    }

    fn visit_function_body(&mut self, body: &ast::FunctionBody) {
        self.scoped(lexical_names(&body.stmts), |scan| {
            body.visit_children_with(scan)
        });
    }

    fn visit_block_stmt(&mut self, block: &ast::BlockStmt) {
        self.scoped(lexical_names(&block.stmts), |scan| {
            block.visit_children_with(scan)
        });
    }

    fn visit_switch_stmt(&mut self, switch: &ast::SwitchStmt) {
        let mut names = HashSet::new();
        for case in &switch.cases {
            names.extend(lexical_names(&case.cons));
        }
        self.scoped(names, |scan| switch.visit_children_with(scan));
    }

    fn visit_catch_clause(&mut self, clause: &ast::CatchClause) {
        let mut names = Vec::new();
        if let Some(param) = &clause.param {
            pat_names(param, &mut names);
        }
        self.scoped(names.into_iter().collect(), |scan| {
            clause.visit_children_with(scan)
        });
    }

    fn visit_for_stmt(&mut self, for_stmt: &ast::ForStmt) {
        let mut names = HashSet::new();
        if let Some(ast::VarDeclOrExpr::VarDecl(var_decl)) = &for_stmt.init {
            if var_decl.kind != ast::VarDeclKind::Var {
                var_decl_names(var_decl, &mut names);
            }
        }
        self.scoped(names, |scan| for_stmt.visit_children_with(scan));
    }

    fn visit_for_in_stmt(&mut self, for_in: &ast::ForInStmt) {
        let names = self.for_head(&for_in.left);
        self.scoped(names, |scan| for_in.visit_children_with(scan));
    }

    fn visit_for_of_stmt(&mut self, for_of: &ast::ForOfStmt) {
        let names = self.for_head(&for_of.left);
        self.scoped(names, |scan| for_of.visit_children_with(scan));
    }
}

impl Scan {
    /// Records a `for (x of ..)` assignment target and returns the names a
    /// `for (let x of ..)` head declares.
    fn for_head(&mut self, head: &ast::ForHead) -> HashSet<String> {
        let mut declared = HashSet::new();
        match head {
            ast::ForHead::Pat(pat) => {
                let mut names = Vec::new();
                pat_names(pat, &mut names);
                self.assigned_names(names);
            }
            ast::ForHead::VarDecl(var_decl) if var_decl.kind != ast::VarDeclKind::Var => {
                var_decl_names(var_decl, &mut declared);
            }
            _ => {}
        }
        declared
    }
}

#[cfg(test)]
mod tests {
    use super::collect_scoped_assigned_identifiers;

    fn assigned(source: &str) -> Vec<String> {
        let parsed = perry_parser::parse_typescript(source, "scan.ts").unwrap();
        let mut names: Vec<String> = collect_scoped_assigned_identifiers(&parsed)
            .into_iter()
            .collect();
        names.sort();
        names
    }

    #[test]
    fn reports_assignment_from_the_function_own_body() {
        assert_eq!(
            assigned("function f(o) { return f = function (x) { return x; }, f(o); }"),
            ["f"]
        );
    }

    #[test]
    fn reports_assignment_from_nested_closures_and_methods() {
        assert_eq!(
            assigned("function a() {} function b() {} class C { m() { a = 1; } } const g = () => { b++; };"),
            ["a", "b"]
        );
    }

    #[test]
    fn ignores_names_shadowed_by_parameters_and_declarations() {
        assert!(assigned(
            "function f() {} function g() {} function h() {} function k() {} \
             function p(f) { f = 1; } \
             function q() { var g; g = 1; } \
             function r() { { let h; h = 1; } } \
             function s() { try {} catch (k) { k = 1; } }"
        )
        .is_empty());
    }
}
