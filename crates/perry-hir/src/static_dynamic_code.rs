//! Specialize private source-building helpers whose complete call set has
//! literal arguments. This exposes constant source to ordinary AOT lowering.
use std::collections::{BTreeMap, BTreeSet};
use swc_ecma_ast as ast;
use swc_ecma_visit::{Visit, VisitMut, VisitMutWith, VisitWith};

#[derive(Default)]
struct Census {
    declarations: BTreeMap<String, usize>,
    uses: BTreeMap<String, usize>,
    calls: BTreeMap<String, Vec<Vec<String>>>,
    helpers: BTreeMap<String, ast::FnExpr>,
    aliases: BTreeSet<String>,
    writes: BTreeSet<String>,
}
impl Visit for Census {
    fn visit_binding_ident(&mut self, i: &ast::BindingIdent) {
        *self.declarations.entry(i.id.sym.to_string()).or_default() += 1;
    }
    fn visit_ident(&mut self, i: &ast::Ident) {
        *self.uses.entry(i.sym.to_string()).or_default() += 1;
    }
    fn visit_assign_expr(&mut self, a: &ast::AssignExpr) {
        struct Written<'a>(&'a mut BTreeSet<String>);
        impl Visit for Written<'_> {
            fn visit_ident(&mut self, i: &ast::Ident) {
                self.0.insert(i.sym.to_string());
            }
        }
        a.left.visit_with(&mut Written(&mut self.writes));
        a.visit_children_with(self);
    }
    fn visit_update_expr(&mut self, u: &ast::UpdateExpr) {
        if let ast::Expr::Ident(i) = u.arg.as_ref() {
            self.writes.insert(i.sym.to_string());
        }
        u.visit_children_with(self);
    }
    fn visit_var_declarator(&mut self, d: &ast::VarDeclarator) {
        if let (ast::Pat::Ident(i), Some(e)) = (&d.name, &d.init) {
            if matches!(e.as_ref(), ast::Expr::Ident(f) if f.sym.as_ref() == "Function") {
                self.aliases.insert(i.id.sym.to_string());
            }
            if let ast::Expr::Fn(f) = e.as_ref() {
                self.helpers.insert(i.id.sym.to_string(), f.clone());
            }
        }
        d.visit_children_with(self);
    }
    fn visit_call_expr(&mut self, c: &ast::CallExpr) {
        if let ast::Callee::Expr(e) = &c.callee {
            if let ast::Expr::Ident(i) = e.as_ref() {
                if let Some(args) = c
                    .args
                    .iter()
                    .map(|a| {
                        if a.spread.is_none() {
                            crate::eval_classifier::const_string_of(&a.expr)
                        } else {
                            None
                        }
                    })
                    .collect::<Option<Vec<_>>>()
                {
                    self.calls.entry(i.sym.to_string()).or_default().push(args);
                }
            }
        }
        c.visit_children_with(self);
    }
}
struct Substitute<'a>(&'a BTreeMap<String, String>);
impl VisitMut for Substitute<'_> {
    fn visit_mut_expr(&mut self, e: &mut ast::Expr) {
        if let ast::Expr::Ident(i) = e {
            if let Some(s) = self.0.get(i.sym.as_ref()) {
                *e = ast::Expr::Lit(ast::Lit::Str(ast::Str {
                    span: i.span,
                    value: s.clone().into(),
                    raw: None,
                }));
                return;
            }
        }
        e.visit_mut_children_with(self);
    }
}
// A function expression can only be called after it is constructed. Track
// unconditional initializers in each statement list, and do not inherit them
// into hoisted declarations, which can run before their textual predecessors.
#[derive(Default)]
struct Dominance {
    available: BTreeSet<String>,
    bad_calls: BTreeSet<String>,
    aliases_at_definition: BTreeMap<String, BTreeSet<String>>,
}
impl Dominance {
    fn stmt(&mut self, stmt: &ast::Stmt) {
        stmt.visit_with(self);
        if let ast::Stmt::Decl(ast::Decl::Var(v)) = stmt {
            for d in &v.decls {
                if let (ast::Pat::Ident(i), Some(_)) = (&d.name, &d.init) {
                    self.available.insert(i.id.sym.to_string());
                }
            }
        }
    }
}
impl Visit for Dominance {
    fn visit_module(&mut self, m: &ast::Module) {
        for item in &m.body {
            if let ast::ModuleItem::Stmt(s) = item {
                self.stmt(s);
            } else {
                item.visit_children_with(self);
            }
        }
    }
    fn visit_block_stmt(&mut self, b: &ast::BlockStmt) {
        let saved = self.available.clone();
        for s in &b.stmts {
            self.stmt(s);
        }
        self.available = saved;
    }
    // A function body is a statement list in its own right (the CommonJS
    // wrapper puts every module statement in one).
    fn visit_function_body(&mut self, b: &ast::FunctionBody) {
        let saved = self.available.clone();
        for s in &b.stmts {
            self.stmt(s);
        }
        self.available = saved;
    }
    fn visit_fn_decl(&mut self, f: &ast::FnDecl) {
        let saved = std::mem::take(&mut self.available);
        f.visit_children_with(self);
        self.available = saved;
    }
    fn visit_var_declarator(&mut self, d: &ast::VarDeclarator) {
        if let (ast::Pat::Ident(i), Some(e)) = (&d.name, &d.init) {
            if matches!(e.as_ref(), ast::Expr::Fn(_)) {
                self.aliases_at_definition
                    .insert(i.id.sym.to_string(), self.available.clone());
            }
        }
        d.visit_children_with(self);
    }
    fn visit_call_expr(&mut self, c: &ast::CallExpr) {
        if let ast::Callee::Expr(e) = &c.callee {
            if let ast::Expr::Ident(i) = e.as_ref() {
                if !self.available.contains(i.sym.as_ref()) {
                    self.bad_calls.insert(i.sym.to_string());
                }
            }
        }
        c.visit_children_with(self);
    }
}

/// Clone only modules with a private helper whose body builds source.
pub fn specialize(module: &ast::Module) -> Option<ast::Module> {
    let mut c = Census::default();
    module.visit_with(&mut c);
    if c.declarations.contains_key("Function") || c.writes.contains("Function") {
        return None;
    }
    c.aliases
        .retain(|n| c.declarations.get(n) == Some(&1) && !c.writes.contains(n));
    let mut dominance = Dominance::default();
    module.visit_with(&mut dominance);
    let helper_names: BTreeSet<String> = c.helpers.keys().cloned().collect();
    c.helpers.retain(|name, f| {
        let Some(calls) = c.calls.get(name) else { return false; };
        // Binding ids are not counted as expression uses. A reassignment,
        // escape or call with unknown args disqualifies the helper.
        !dominance.bad_calls.contains(name)
            && {
                let mut body = Census::default(); f.visit_with(&mut body);
                c.aliases.iter().filter(|a| body.uses.contains_key(*a)).all(|a|
                    dominance.aliases_at_definition.get(name).is_some_and(|available| available.contains(a)))
            }
            && c.declarations.get(name) == Some(&1) && !c.writes.contains(name) && c.uses.get(name).copied().unwrap_or(0) == calls.len()
            && f.function.params.iter().all(|p| matches!(&p.pat, ast::Pat::Ident(i) if c.declarations.get(i.id.sym.as_ref()) == Some(&1) && !c.writes.contains(i.id.sym.as_ref())))
            && {
                let mut body = Census::default(); f.visit_with(&mut body);
                !body.uses.keys().any(|n| helper_names.contains(n))
                    && !body.uses.contains_key("eval") && !body.uses.contains_key("arguments")
            }
            && f.ident.is_none() && !f.function.is_generator && !f.function.is_async
            && calls.iter().all(|a| a.len() == f.function.params.len())
            && {
                struct Source<'a> { found: bool, aliases: &'a BTreeSet<String> }
                impl Visit for Source<'_> {
                    fn visit_call_expr(&mut self, call: &ast::CallExpr) {
                        if let ast::Callee::Expr(e) = &call.callee {
                            if matches!(e.as_ref(), ast::Expr::Ident(i) if i.sym.as_ref() == "Function" || self.aliases.contains(i.sym.as_ref())) {
                                self.found = true;
                            }
                        }
                        call.visit_children_with(self);
                    }
                    fn visit_new_expr(&mut self, n: &ast::NewExpr) {
                        if matches!(n.callee.as_ref(), ast::Expr::Ident(i) if i.sym.as_ref() == "Function" || self.aliases.contains(i.sym.as_ref())) { self.found = true; }
                        n.visit_children_with(self);
                    }
                }
                let mut source = Source { found: false, aliases: &c.aliases };
                f.visit_with(&mut source);
                source.found
            }
    });
    if c.helpers.is_empty() {
        return None;
    }
    struct Rewrite {
        helpers: BTreeMap<String, ast::FnExpr>,
        aliases: BTreeSet<String>,
    }
    impl VisitMut for Rewrite {
        fn visit_mut_var_declarator(&mut self, d: &mut ast::VarDeclarator) {
            if matches!(&d.name, ast::Pat::Ident(i) if self.helpers.contains_key(i.id.sym.as_ref()))
            {
                // Every use has been specialized; the original body must not
                // leave a non-constant source site in the program.
                d.init = Some(Box::new(ast::Expr::Lit(ast::Lit::Null(ast::Null {
                    span: d.span,
                }))));
                return;
            }
            d.visit_mut_children_with(self);
        }
        fn visit_mut_call_expr(&mut self, call: &mut ast::CallExpr) {
            if let ast::Callee::Expr(e) = &mut call.callee {
                if let ast::Expr::Ident(i) = e.as_mut() {
                    if let Some(f) = self.helpers.get(i.sym.as_ref()) {
                        let values: BTreeMap<_, _> = f
                            .function
                            .params
                            .iter()
                            .zip(&call.args)
                            .filter_map(|(p, a)| {
                                let ast::Pat::Ident(i) = &p.pat else {
                                    return None;
                                };
                                Some((
                                    i.id.sym.to_string(),
                                    crate::eval_classifier::const_string_of(&a.expr)?,
                                ))
                            })
                            .collect();
                        let mut f = f.clone();
                        // Retain params so ordinary function lowering preserves
                        // call/return/try semantics. Only pure literal uses fold.
                        f.function.body.visit_mut_with(&mut Substitute(&values));
                        struct AliasCalls<'a>(&'a BTreeSet<String>);
                        impl VisitMut for AliasCalls<'_> {
                            fn visit_mut_call_expr(&mut self, c: &mut ast::CallExpr) {
                                if let ast::Callee::Expr(e) = &mut c.callee {
                                    if let ast::Expr::Ident(i) = e.as_mut() {
                                        if self.0.contains(i.sym.as_ref()) {
                                            i.sym = "Function".into();
                                        }
                                    }
                                }
                                c.visit_mut_children_with(self);
                            }
                        }
                        f.function
                            .body
                            .visit_mut_with(&mut AliasCalls(&self.aliases));
                        *e = Box::new(ast::Expr::Fn(f));
                    }
                }
            }
            call.visit_mut_children_with(self);
        }
    }
    let mut out = module.clone();
    out.visit_mut_with(&mut Rewrite {
        helpers: c.helpers,
        aliases: c.aliases,
    });
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_source_helper_is_specialized_but_escaping_helper_is_not() {
        let source = r#"const F = Function; const helper = function (piece) {
            return F('return (' + piece + ').constructor;')();
        }; helper('async function () {}'); helper('function* () {}');"#;
        let module = perry_parser::parse_typescript(source, "static.cjs").unwrap();
        let specialized = specialize(&module).expect("known private calls");
        let mut reach = crate::dynamic_code::DynamicCodeReach::default();
        crate::dynamic_code::scan(&specialized, "static.cjs", &mut reach);
        assert!(
            reach.is_empty(),
            "constant helper must leave no interpreted source site: {reach:?}"
        );
        for extra in [
            "module.exports = helper;",
            "helper(process.argv[2]);",
            "helper = other;",
            "",
        ] {
            if extra.is_empty() {
                continue;
            }
            let module =
                perry_parser::parse_typescript(&format!("{source}{extra}"), "static.cjs").unwrap();
            assert!(specialize(&module).is_none(), "{extra}");
        }
    }
}

#[cfg(test)]
mod wrapped_tests {
    /// The CommonJS wrapper puts the module's statements in a function body.
    #[test]
    fn helper_inside_a_function_body_is_specialized() {
        let source = r#"var m = (function () { function body() {
            var $F = Function;
            var helper = function (piece) {
                try { return $F('"use strict"; return (' + piece + ').constructor;')(); } catch (e) {}
            };
            var go = function go(name) { return name === 'a' ? helper('async function () {}') : helper('function* () {}'); };
            return go;
        } return body(); })();"#;
        let module = perry_parser::parse_typescript(source, "static.cjs").unwrap();
        let specialized = super::specialize(&module).expect("wrapped helper specializes");
        let mut reach = crate::dynamic_code::DynamicCodeReach::default();
        crate::dynamic_code::scan(&specialized, "static.cjs", &mut reach);
        assert!(reach.is_empty(), "{reach:?}");
    }
}

#[cfg(test)]
mod initialization_tests {
    #[test]
    fn initialization_order_is_part_of_the_proof() {
        for source in [
            "var helper = function (p) { return F('return ' + p)(); }; helper('42'); var F = Function;",
            "go(); var F = Function; var helper = function (p) { return F('return ' + p)(); }; function go() { helper('42'); }",
            "const F=Function; const helper=function(p){ arguments[0]='0'; return F('return '+p)(); }; helper('42');",
        ] {
            let module = perry_parser::parse_typescript(source, "static.cjs").unwrap();
            assert!(super::specialize(&module).is_none(), "{source}");
        }
    }
}
