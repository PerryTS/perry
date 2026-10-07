//! Diagnose computed CommonJS dependency loads before the wrapper hides them.

use std::collections::HashMap;
use swc_ecma_ast as ast;
use swc_ecma_visit::{Visit, VisitWith};

/// Return original-source byte offsets of computed calls to the wrapper's
/// require, including local aliases such as moment's aliasedRequire = require.
/// Literal calls are already handled by static dependency discovery.
pub(super) fn computed_require_offsets(module: &ast::Module) -> Vec<usize> {
    let mut visitor = ComputedRequires::default();
    module.visit_with(&mut visitor);
    visitor.offsets
}

#[derive(Default)]
struct ComputedRequires {
    bindings: Vec<HashMap<String, bool>>,
    offsets: Vec<usize>,
    function_scopes: Vec<usize>,
}

impl ComputedRequires {
    fn is_require(&self, name: &str) -> bool {
        self.bindings
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
            .unwrap_or(name == "require")
    }

    fn bind(&mut self, name: &str, is_require: bool) {
        if self.bindings.is_empty() {
            self.bindings.push(HashMap::new());
        }
        self.bindings
            .last_mut()
            .unwrap()
            .insert(name.to_string(), is_require);
    }

    fn initializer_is_require(&self, expr: &ast::Expr) -> bool {
        match expr {
            ast::Expr::Ident(name) => self.is_require(name.sym.as_ref()),
            ast::Expr::Paren(inner) => self.initializer_is_require(&inner.expr),
            _ => false,
        }
    }

    fn predeclare(&mut self, declaration: &ast::Decl) {
        match declaration {
            ast::Decl::Var(vars) if vars.kind != ast::VarDeclKind::Var => {
                for var in &vars.decls {
                    self.shadow_pattern(&var.name);
                }
            }
            ast::Decl::Fn(function) => self.bind(function.ident.sym.as_ref(), false),
            ast::Decl::Class(class) => self.bind(class.ident.sym.as_ref(), false),
            _ => {}
        }
    }

    fn parameters<'a>(&mut self, patterns: impl Iterator<Item = &'a ast::Pat>) {
        self.function_scopes.push(self.bindings.len());
        self.bindings.push(HashMap::new());
        for pattern in patterns {
            self.shadow_pattern(pattern);
        }
    }

    fn leave_function(&mut self) {
        self.bindings.pop();
        self.function_scopes.pop();
    }

    fn hoist<T: VisitWith<HoistedVars>>(&mut self, body: &T) {
        let mut vars = HoistedVars::default();
        body.visit_with(&mut vars);
        for name in vars.names {
            self.bind(&name, false);
        }
    }

    fn visit_named_function(&mut self, function: &ast::Function, name: Option<&str>) {
        function.decorators.visit_with(self);
        self.parameters(function.params.iter().map(|param| &param.pat));
        if let Some(name) = name {
            self.bind(name, false);
        }
        function.params.visit_with(self);
        self.hoist(&function.body);
        function.body.visit_with(self);
        self.leave_function();
    }

    fn shadow_pattern(&mut self, pattern: &ast::Pat) {
        match pattern {
            ast::Pat::Ident(name) => self.bind(name.id.sym.as_ref(), false),
            ast::Pat::Assign(assign) => self.shadow_pattern(&assign.left),
            ast::Pat::Rest(rest) => self.shadow_pattern(&rest.arg),
            ast::Pat::Array(array) => {
                for element in array.elems.iter().flatten() {
                    self.shadow_pattern(element);
                }
            }
            ast::Pat::Object(object) => {
                for prop in &object.props {
                    match prop {
                        ast::ObjectPatProp::KeyValue(pair) => self.shadow_pattern(&pair.value),
                        ast::ObjectPatProp::Assign(assign) => {
                            self.bind(assign.key.id.sym.as_ref(), false)
                        }
                        ast::ObjectPatProp::Rest(rest) => self.shadow_pattern(&rest.arg),
                    }
                }
            }
            _ => {}
        }
    }
}

impl Visit for ComputedRequires {
    fn visit_module(&mut self, module: &ast::Module) {
        self.parameters(std::iter::empty());
        self.hoist(module);
        for item in &module.body {
            if let ast::ModuleItem::Stmt(ast::Stmt::Decl(declaration)) = item {
                self.predeclare(declaration);
            }
        }
        module.visit_children_with(self);
        self.leave_function();
    }

    fn visit_block_stmt(&mut self, block: &ast::BlockStmt) {
        self.bindings.push(HashMap::new());
        for statement in &block.stmts {
            if let ast::Stmt::Decl(declaration) = statement {
                self.predeclare(declaration);
            }
        }
        block.visit_children_with(self);
        self.bindings.pop();
    }

    fn visit_function_body(&mut self, body: &ast::FunctionBody) {
        // Function bodies used to visit as BlockStmt. Retain their lexical
        // predeclarations so a later let/const binding shadows require throughout.
        self.bindings.push(HashMap::new());
        for statement in &body.stmts {
            if let ast::Stmt::Decl(declaration) = statement {
                self.predeclare(declaration);
            }
        }
        body.visit_children_with(self);
        self.bindings.pop();
    }

    fn visit_call_expr(&mut self, call: &ast::CallExpr) {
        if let ast::Callee::Expr(callee) = &call.callee {
            if matches!(callee.as_ref(), ast::Expr::Ident(name) if self.is_require(name.sym.as_ref()))
            {
                if let Some(arg) = call.args.first() {
                    if arg.spread.is_some()
                        || !matches!(arg.expr.as_ref(), ast::Expr::Lit(ast::Lit::Str(_)))
                    {
                        self.offsets.push(call.span.lo.0.saturating_sub(1) as usize);
                    }
                }
            }
        }
        call.visit_children_with(self);
    }

    fn visit_var_decl(&mut self, declaration: &ast::VarDecl) {
        for var in &declaration.decls {
            var.init.visit_with(self);
            if let ast::Pat::Ident(name) = &var.name {
                // A bare var declaration does not clear an earlier assignment.
                if declaration.kind == ast::VarDeclKind::Var && var.init.is_none() {
                    continue;
                }
                let alias = var
                    .init
                    .as_deref()
                    .is_some_and(|init| self.initializer_is_require(init));
                let scope = if declaration.kind == ast::VarDeclKind::Var {
                    *self.function_scopes.last().unwrap()
                } else {
                    self.bindings.len() - 1
                };
                self.bindings[scope].insert(name.id.sym.to_string(), alias);
            } else {
                self.shadow_pattern(&var.name);
            }
        }
    }

    fn visit_assign_expr(&mut self, assignment: &ast::AssignExpr) {
        assignment.visit_children_with(self);
        if let ast::AssignTarget::Simple(ast::SimpleAssignTarget::Ident(name)) = &assignment.left {
            let alias = assignment.op == ast::AssignOp::Assign
                && self.initializer_is_require(&assignment.right);
            let function_scope = *self.function_scopes.last().unwrap();
            let scope = self
                .bindings
                .iter()
                .rposition(|scope| scope.contains_key(name.id.sym.as_ref()))
                .unwrap_or(function_scope)
                .max(function_scope);
            self.bindings[scope].insert(name.id.sym.to_string(), alias);
        }
    }

    fn visit_function(&mut self, function: &ast::Function) {
        self.visit_named_function(function, None);
    }

    fn visit_fn_expr(&mut self, expression: &ast::FnExpr) {
        self.visit_named_function(
            &expression.function,
            expression.ident.as_ref().map(|name| name.sym.as_ref()),
        );
    }

    fn visit_arrow_expr(&mut self, arrow: &ast::ArrowExpr) {
        self.parameters(arrow.params.iter());
        arrow.params.visit_with(self);
        self.hoist(&arrow.body);
        arrow.body.visit_with(self);
        self.leave_function();
    }

    fn visit_getter_prop(&mut self, getter: &ast::GetterProp) {
        getter.key.visit_with(self);
        self.parameters(std::iter::empty());
        self.hoist(&getter.function.body);
        getter.function.body.visit_with(self);
        self.leave_function();
    }

    fn visit_setter_prop(&mut self, setter: &ast::SetterProp) {
        setter.key.visit_with(self);
        self.parameters(setter.function.params.iter().map(|param| &param.pat));
        self.hoist(&setter.function.body);
        setter.function.body.visit_with(self);
        self.leave_function();
    }

    fn visit_constructor(&mut self, constructor: &ast::Constructor) {
        constructor.key.visit_with(self);
        self.parameters(constructor.params.iter().filter_map(|param| match param {
            ast::ParamOrTsParamProp::Param(param) => Some(&param.pat),
            _ => None,
        }));
        constructor.params.visit_with(self);
        self.hoist(&constructor.body);
        constructor.body.visit_with(self);
        self.leave_function();
    }
}

/// Hoist var bindings without entering nested callable environments. This is
/// compile-time syntax analysis, not a runtime cache or control-flow proof.
#[derive(Default)]
struct HoistedVars {
    names: Vec<String>,
}
impl Visit for HoistedVars {
    fn visit_var_decl(&mut self, declaration: &ast::VarDecl) {
        if declaration.kind == ast::VarDeclKind::Var {
            for var in &declaration.decls {
                var.name.visit_with(self);
            }
        }
    }
    fn visit_binding_ident(&mut self, name: &ast::BindingIdent) {
        self.names.push(name.id.sym.to_string());
    }
    fn visit_assign_pat(&mut self, pattern: &ast::AssignPat) {
        pattern.left.visit_with(self);
    }
    fn visit_function(&mut self, _: &ast::Function) {}
    fn visit_arrow_expr(&mut self, _: &ast::ArrowExpr) {}
    fn visit_constructor(&mut self, _: &ast::Constructor) {}
    fn visit_getter_prop(&mut self, _: &ast::GetterProp) {}
    fn visit_setter_prop(&mut self, _: &ast::SetterProp) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offsets(source: &str) -> Vec<usize> {
        computed_require_offsets(&perry_parser::parse_typescript(source, "computed.cjs").unwrap())
    }

    #[test]
    fn diagnoses_concatenation_template_and_local_aliases() {
        let source = "exports.concat = name => require('./locale/' + name);\nexports.template = name => require(`./locale/${name}.js`);\nexports.alias = name => { const r = require; return r('./locale/' + name); };\nexports.assignment = name => { let r; r = require; return r('./locale/' + name); };";
        let sites = offsets(source);
        assert_eq!(sites.len(), 4);
        for offset in sites {
            assert!(source[offset..].starts_with("require(") || source[offset..].starts_with("r("));
        }
    }

    #[test]
    fn ignores_literals_comments_strings_members_and_shadowed_parameters() {
        let source = "require('./dep.js'); // require(name)\nconst text = 'require(name)';\nother.require(name);\nfunction load(require) { return require(name); }\nconst arrow = (require) => require(name);";
        assert!(offsets(source).is_empty());
    }

    #[test]
    fn lexical_blocks_and_hoisted_declarations_do_not_hide_outer_requires() {
        assert_eq!(
            offsets("{ const require = other; require(name); } require(name);").len(),
            1
        );
        assert_eq!(offsets("{ const r = require; r(name); } r(name);").len(), 1);
        assert!(offsets("require(name); function require(name) { return name; }").is_empty());
        assert!(offsets("function load() { require(name); var require = other; }").is_empty());
    }

    #[test]
    fn function_hoisting_named_expressions_and_block_assignments() {
        assert!(
            offsets("function load() { require(name); if (flag) { var require = other; } }")
                .is_empty()
        );
        assert!(offsets("const f = function require() { require(name); };").is_empty());
        assert!(offsets("let r = require; { r = other; } r(name);").is_empty());
        assert_eq!(offsets("let r; { r = require; } r(name);").len(), 1);
        assert_eq!(
            offsets("function load() { { var r = require; } r(name); }").len(),
            1
        );
        assert_eq!(
            offsets("let r = require; function load() { r = other; r(name); } r(name);").len(),
            1
        );
    }

    #[test]
    fn reassignments_and_function_scopes_do_not_leak_aliases() {
        assert_eq!(
            offsets("function a() { const r = require; r(name); } function b() { r(name); }").len(),
            1
        );
        assert!(offsets("const r = require; r = other; r(name);").is_empty());
    }

    #[test]
    fn function_body_lexical_bindings_and_accessor_parameters_shadow_require() {
        for source in [
            "function load() { require(name); let require = other; }",
            "const load = () => { require(name); const require = other; };",
            "const obj = { get value() { require(name); let require = other; } };",
            "const obj = { set value(require) { require(name); } };",
        ] {
            assert!(offsets(source).is_empty(), "{source}");
        }
        assert_eq!(
            offsets("const obj = { get value() { return require(name); }, set value(v) { require(v); } };").len(),
            2
        );
    }
}
