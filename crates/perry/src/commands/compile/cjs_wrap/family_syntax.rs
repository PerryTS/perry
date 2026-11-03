//! Closed CommonJS class exports, from original syntax rather than the
//! wrapper's synthetic imports/exports. An escape or unknown write refuses
//! the affected namespace; there is no runtime state associated with this.

use std::collections::{BTreeMap, BTreeSet};

use perry_hir::ir::ClassFamilySyntax;
use swc_ecma_ast as ast;
use swc_ecma_visit::{Visit, VisitWith};

fn key(member: &ast::MemberExpr) -> Option<String> {
    match &member.prop {
        ast::MemberProp::Ident(id) => Some(id.sym.to_string()),
        ast::MemberProp::Computed(c) => match c.expr.as_ref() {
            ast::Expr::Lit(ast::Lit::Str(s)) => Some(s.value.to_string_lossy().into_owned()),
            _ => None,
        },
        _ => None,
    }
}

fn require(e: &ast::Expr) -> Option<String> {
    let ast::Expr::Call(c) = e else { return None };
    let ast::Callee::Expr(callee) = &c.callee else {
        return None;
    };
    if !matches!(callee.as_ref(), ast::Expr::Ident(id) if id.sym == *"require") {
        return None;
    }
    let [arg] = c.args.as_slice() else {
        return None;
    };
    match arg.expr.as_ref() {
        ast::Expr::Lit(ast::Lit::Str(s)) if arg.spread.is_none() => {
            Some(s.value.to_string_lossy().into_owned())
        }
        _ => None,
    }
}

#[derive(Default)]
struct Scan {
    // Namespace variables, including mutable ones for the write census.
    namespaces: BTreeMap<String, String>,
    // Only original top-level const bindings can identify a parent.
    const_namespaces: BTreeMap<String, String>,
    bindings: BTreeMap<String, (String, String)>,
    classes: BTreeSet<String>,
    writes: Vec<(Option<String>, String)>,
    declarations: BTreeMap<String, usize>,
    reassigned: BTreeSet<String>,
}

impl Scan {
    /// Some(None) is the local exports object; Some(Some(spec)) an imported
    /// CommonJS namespace. Recognize only literal, syntactically bound uses.
    fn namespace(&self, e: &ast::Expr) -> Option<Option<String>> {
        if let Some(spec) = require(e) {
            return Some(Some(spec));
        }
        match e {
            ast::Expr::Ident(id) if id.sym == *"exports" => Some(None),
            ast::Expr::Ident(id) => self.namespaces.get(id.sym.as_ref()).cloned().map(Some),
            ast::Expr::Member(m)
                if matches!(m.obj.as_ref(), ast::Expr::Ident(id) if id.sym == *"module")
                    && key(m).as_deref() == Some("exports") =>
            {
                Some(None)
            }
            ast::Expr::Paren(p) => self.namespace(&p.expr),
            _ => None,
        }
    }

    fn escape(&mut self, e: &ast::Expr) -> bool {
        if let Some(target) = self.namespace(e) {
            self.writes.push((target, String::new()));
            true
        } else {
            false
        }
    }

    fn write_member(&mut self, m: &ast::MemberExpr) {
        if let Some(target) = self.namespace(&m.obj) {
            self.writes.push((target, key(m).unwrap_or_default()));
        } else if self.namespace(&ast::Expr::Member(m.clone())) == Some(None) {
            self.writes.push((None, String::new()));
        }
    }

    fn parent(&self, e: &ast::Expr) -> Option<(String, String)> {
        match e {
            ast::Expr::Ident(id) => self.bindings.get(id.sym.as_ref()).cloned(),
            ast::Expr::Member(m) => {
                let ast::Expr::Ident(id) = m.obj.as_ref() else {
                    return None;
                };
                Some((self.const_namespaces.get(id.sym.as_ref())?.clone(), key(m)?))
            }
            _ => None,
        }
    }
}

impl Visit for Scan {
    fn visit_binding_ident(&mut self, id: &ast::BindingIdent) {
        *self.declarations.entry(id.id.sym.to_string()).or_default() += 1;
        id.visit_children_with(self);
    }

    fn visit_var_declarator(&mut self, d: &ast::VarDeclarator) {
        d.name.visit_with(self);
        if let Some(init) = &d.init {
            // A declared require binding is the one allowed whole-namespace
            // use. Aliases of exports or namespaces escape conservatively.
            if require(init).is_some()
                && (matches!(&d.name, ast::Pat::Ident(id)
                    if self.namespaces.contains_key(id.id.sym.as_ref()))
                    || matches!(&d.name, ast::Pat::Object(p) if p.props.iter().all(|prop|
                    match prop {
                        ast::ObjectPatProp::Assign(a) => a.value.is_none(),
                        ast::ObjectPatProp::KeyValue(k) => matches!(k.value.as_ref(), ast::Pat::Ident(_)),
                        _ => false,
                    })))
            {
                return;
            }
            init.visit_with(self);
        }
    }

    fn visit_expr(&mut self, e: &ast::Expr) {
        if self.escape(e) {
            return;
        }
        // An arbitrary use of `module` can replace its exports property.
        if matches!(e, ast::Expr::Ident(id) if id.sym == *"module") {
            self.writes.push((None, String::new()));
            return;
        }
        // Computed/aliased require and eval can expose or rewrite any
        // namespace. Literal requires were consumed by namespace() or the
        // declaration visitor; a remaining bare function is an open use.
        if matches!(e, ast::Expr::Ident(id) if id.sym == *"require" || id.sym == *"eval") {
            self.writes.push((Some(String::new()), String::new()));
            return;
        }
        e.visit_children_with(self);
    }

    fn visit_named_export(&mut self, e: &ast::NamedExport) {
        for spec in &e.specifiers {
            match spec {
                ast::ExportSpecifier::Named(n) => {
                    if let Some(source) = &e.src {
                        let default = match &n.orig {
                            ast::ModuleExportName::Ident(id) => id.sym == *"default",
                            ast::ModuleExportName::Str(name) => {
                                name.value.to_string_lossy() == "default"
                            }
                        };
                        if default {
                            self.writes.push((
                                Some(source.value.to_string_lossy().into_owned()),
                                String::new(),
                            ));
                        }
                    } else if let ast::ModuleExportName::Ident(id) = &n.orig {
                        if let Some(source) = self.namespaces.get(id.sym.as_ref()) {
                            self.writes.push((Some(source.clone()), String::new()));
                        }
                    }
                }
                ast::ExportSpecifier::Namespace(_) => {
                    if let Some(source) = &e.src {
                        self.writes.push((
                            Some(source.value.to_string_lossy().into_owned()),
                            String::new(),
                        ));
                    }
                }
                _ => {}
            }
        }
        e.visit_children_with(self);
    }

    fn visit_member_expr(&mut self, m: &ast::MemberExpr) {
        // A key read/write does not itself expose the namespace object.
        if self.namespace(&m.obj).is_some() {
            if let ast::MemberProp::Computed(k) = &m.prop {
                k.expr.visit_with(self);
            }
            return;
        }
        m.visit_children_with(self);
    }

    fn visit_assign_expr(&mut self, a: &ast::AssignExpr) {
        match &a.left {
            ast::AssignTarget::Simple(ast::SimpleAssignTarget::Member(m)) => {
                self.write_member(m);
            }
            ast::AssignTarget::Simple(ast::SimpleAssignTarget::Ident(id)) => {
                self.reassigned.insert(id.id.sym.to_string());
                if id.id.sym == *"exports" || id.id.sym == *"module" {
                    self.writes.push((None, String::new()));
                }
                if let Some(spec) = self.namespaces.get(id.id.sym.as_ref()) {
                    self.writes.push((Some(spec.clone()), String::new()));
                }
            }
            _ => {}
        }
        a.visit_children_with(self);
    }

    fn visit_pat(&mut self, p: &ast::Pat) {
        // Destructuring assignment can write through a member target too.
        if let ast::Pat::Expr(e) = p {
            if let ast::Expr::Member(m) = e.as_ref() {
                self.write_member(m);
            }
        }
        p.visit_children_with(self);
    }

    fn visit_update_expr(&mut self, u: &ast::UpdateExpr) {
        if let ast::Expr::Member(m) = u.arg.as_ref() {
            self.write_member(m);
        }
        if let ast::Expr::Ident(id) = u.arg.as_ref() {
            self.reassigned.insert(id.sym.to_string());
        }
        u.visit_children_with(self);
    }

    fn visit_unary_expr(&mut self, u: &ast::UnaryExpr) {
        if u.op == ast::UnaryOp::Delete {
            if let ast::Expr::Member(m) = u.arg.as_ref() {
                self.write_member(m);
            }
        }
        u.visit_children_with(self);
    }
}

pub(crate) fn analyze(source: &str) -> ClassFamilySyntax {
    let Ok(module) = perry_parser::parse_typescript(source, "family.cjs") else {
        return ClassFamilySyntax {
            writes: vec![(Some(String::new()), String::new())],
            module_capability_uses: 1,
            ..Default::default()
        };
    };
    let mut scan = Scan::default();
    for item in &module.body {
        if let ast::ModuleItem::ModuleDecl(ast::ModuleDecl::Import(import)) = item {
            for specifier in &import.specifiers {
                let name = match specifier {
                    ast::ImportSpecifier::Default(d) => Some(d.local.sym.to_string()),
                    ast::ImportSpecifier::Namespace(n) => Some(n.local.sym.to_string()),
                    _ => None,
                };
                if let Some(name) = name {
                    scan.namespaces
                        .insert(name, import.src.value.to_string_lossy().into_owned());
                }
            }
        }
        let ast::ModuleItem::Stmt(ast::Stmt::Decl(decl)) = item else {
            continue;
        };
        if let ast::Decl::Class(c) = decl {
            scan.classes.insert(c.ident.sym.to_string());
        }
        let ast::Decl::Var(v) = decl else { continue };
        for d in &v.decls {
            let Some(init) = &d.init else { continue };
            let Some(spec) = require(init) else { continue };
            match &d.name {
                ast::Pat::Ident(id) => {
                    scan.namespaces.insert(id.id.sym.to_string(), spec.clone());
                    if v.kind == ast::VarDeclKind::Const {
                        scan.const_namespaces.insert(id.id.sym.to_string(), spec);
                    }
                }
                ast::Pat::Object(p) if v.kind == ast::VarDeclKind::Const => {
                    for prop in &p.props {
                        match prop {
                            ast::ObjectPatProp::Assign(a) if a.value.is_none() => {
                                let name = a.key.id.sym.to_string();
                                scan.bindings.insert(name.clone(), (spec.clone(), name));
                            }
                            ast::ObjectPatProp::KeyValue(k) => {
                                if let (ast::PropName::Ident(key), ast::Pat::Ident(local)) =
                                    (&k.key, k.value.as_ref())
                                {
                                    scan.bindings.insert(
                                        local.id.sym.to_string(),
                                        (spec.clone(), key.sym.to_string()),
                                    );
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }
    module.visit_with(&mut scan);
    for (name, count) in &scan.declarations {
        if *count > 1 {
            if let Some(spec) = scan.namespaces.get(name) {
                scan.writes.push((Some(spec.clone()), String::new()));
            }
            if name == "exports" || name == "module" || name == "require" {
                scan.writes.push((None, String::new()));
            }
        }
    }
    if scan.declarations.contains_key("exports")
        || scan.declarations.contains_key("module")
        || scan.declarations.contains_key("require")
        || scan.reassigned.contains("require")
    {
        scan.writes.push((None, String::new()));
        for spec in scan
            .namespaces
            .values()
            .chain(scan.bindings.values().map(|(s, _)| s))
        {
            scan.writes.push((Some(spec.clone()), String::new()));
        }
    }
    let mut result = ClassFamilySyntax {
        run_once_classes: perry_hir::lower::run_once_class_names(&module),
        module_capability_uses: super::run_once_syntax::module_capability_uses(&module),
        descriptor_capability_uses: super::run_once_syntax::descriptor_capability_uses(&module),
        ..Default::default()
    };
    for item in &module.body {
        if let ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Class(c))) = item {
            if let Some(parent) = c.class.super_class.as_deref().and_then(|e| scan.parent(e)) {
                let binding = match c.class.super_class.as_deref() {
                    Some(ast::Expr::Ident(id)) => Some(id.sym.as_ref()),
                    Some(ast::Expr::Member(m)) => match m.obj.as_ref() {
                        ast::Expr::Ident(id) => Some(id.sym.as_ref()),
                        _ => None,
                    },
                    _ => None,
                };
                if binding.is_some_and(|n| {
                    !scan.reassigned.contains(n) && scan.declarations.get(n).copied() == Some(1)
                }) {
                    result.parents.insert(c.ident.sym.to_string(), parent);
                }
            }
        }
        let ast::ModuleItem::Stmt(ast::Stmt::Expr(s)) = item else {
            continue;
        };
        let ast::Expr::Assign(a) = s.expr.as_ref() else {
            continue;
        };
        let ast::AssignTarget::Simple(ast::SimpleAssignTarget::Member(m)) = &a.left else {
            continue;
        };
        if a.op != ast::AssignOp::Assign || scan.namespace(&m.obj) != Some(None) {
            continue;
        }
        let (Some(k), ast::Expr::Ident(class)) = (key(m), a.right.as_ref()) else {
            continue;
        };
        let name = class.sym.to_string();
        if scan.classes.contains(&name) && !scan.reassigned.contains(&name) {
            result.exports.insert(k, name);
        }
    }
    result.writes = scan.writes;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_parse_failure_cannot_close_any_program_namespace() {
        let syntax = analyze("const =");
        assert!(syntax
            .writes
            .iter()
            .any(|(source, key)| source.as_deref() == Some("") && key.is_empty()));
    }

    #[test]
    fn original_const_require_forms_name_the_export() {
        let syntax = analyze("const {Base: Parent} = require('./base'); const m = require('./base'); class A extends Parent {} class B extends m.Base {}");
        assert_eq!(syntax.parents["A"], ("./base".into(), "Base".into()));
        assert_eq!(syntax.parents["B"], syntax.parents["A"]);
        assert!(syntax.writes.is_empty(), "reads do not escape a namespace");
        let syntax = analyze("const {Base} = require('./base'); class A extends Base {}");
        assert_eq!(syntax.parents["A"].1, "Base");
        assert!(syntax.writes.is_empty());
    }

    #[test]
    fn mutable_computed_and_called_heritage_is_not_a_binding_proof() {
        for source in [
            "let {Base} = require('./base'); class A extends Base {}",
            "var m = require('./base'); class A extends m.Base {}",
            "const m = require(path); class A extends m.Base {}",
            "const m = require('./base'); class A extends m.factory() {}",
            "const {Base} = require('./base'); function f(Base) {} class A extends Base {}",
        ] {
            assert!(analyze(source).parents.is_empty(), "{source}");
        }
    }

    #[test]
    fn exports_require_one_top_level_class_write_and_no_other_writes() {
        let syntax = analyze("class Base {} exports.Base = Base;");
        assert_eq!(syntax.exports["Base"], "Base");
        assert_eq!(syntax.writes, vec![(None, "Base".into())]);
        let syntax = analyze(
            "class Base {} module.exports.Base = Base; function mutate() { exports.Base = Other; }",
        );
        assert_eq!(syntax.writes.iter().filter(|(_, k)| k == "Base").count(), 2);
        assert!(analyze("class Base {} if (ok) exports.Base = Base;")
            .exports
            .is_empty());
        assert!(analyze("class Base {} exports.Base = Base; Base = Other;")
            .exports
            .is_empty());
    }

    #[test]
    fn namespace_mutation_escape_and_replacement_are_censused() {
        for source in [
            "module.exports = {};",
            "exports[key] = Other;",
            "delete exports.Base;",
            "Object.assign(exports, {});",
            "const alias = exports; alias.Base = Other;",
            "function change(module) { module.exports = {}; }",
            "eval('exports.Base = Other');",
            "const run = eval; run('exports.Base = Other');",
        ] {
            let syntax = analyze(source);
            assert!(!syntax.writes.is_empty(), "{source}");
        }
        for source in [
            "const m = require('./base'); m.Base = Other;",
            "const m = require('./base'); delete m.Base;",
            "const m = require('./base'); Object.defineProperty(m, 'Base', {});",
            "import m from './base'; m.Base = Other;",
            "export {default as ns} from './base';",
            "export {\"default\" as ns} from './base';",
            "const m = require('./base'); const alias = m; alias.Base = Other;",
        ] {
            assert!(
                analyze(source)
                    .writes
                    .iter()
                    .any(|(s, _)| s.as_deref() == Some("./base")),
                "{source}"
            );
        }
    }
}
