use std::collections::HashSet;
use std::path::Path;
use swc_common::{Globals, Mark, SyntaxContext, GLOBALS};
use swc_ecma_ast::{self as ast, Pass};
use swc_ecma_transforms_base::resolver;
use swc_ecma_visit::{Visit, VisitWith};

pub(in super::super) fn filenames(source: &str, path: &Path) -> Vec<String> {
    if !source.contains("worker_threads") {
        return Vec::new();
    }
    let Ok(module) =
        perry_parser::parse_typescript(&format!("{source}\nexport {{}};"), &path.to_string_lossy())
    else {
        return Vec::new();
    };
    let mut program = ast::Program::Module(module);
    let unresolved = GLOBALS.set(&Globals::new(), || {
        let mark = Mark::new();
        resolver(mark, Mark::new(), true).process(&mut program);
        SyntaxContext::empty().apply_mark(mark)
    });
    let mut origins = Origins {
        unresolved,
        workers: HashSet::new(),
        namespaces: HashSet::new(),
        modified: HashSet::new(),
    };
    program.visit_with(&mut origins);
    if origins
        .modified
        .iter()
        .any(|(name, ctxt)| name == "require" && *ctxt == unresolved)
    {
        return Vec::new();
    }
    let mut calls = Constructors {
        origins: &origins,
        paths: Vec::new(),
    };
    program.visit_with(&mut calls);
    calls.paths
}

struct Origins {
    unresolved: SyntaxContext,
    workers: HashSet<ast::Id>,
    namespaces: HashSet<ast::Id>,
    modified: HashSet<ast::Id>,
}

impl Origins {
    fn worker_require(&self, expr: &ast::Expr) -> bool {
        let ast::Expr::Call(call) = expr else {
            return false;
        };
        let ast::Callee::Expr(callee) = &call.callee else {
            return false;
        };
        let ast::Expr::Ident(loader) = callee.as_ref() else {
            return false;
        };
        loader.sym == "require"
            && loader.ctxt == self.unresolved
            && call.args.len() == 1
            && call.args[0].spread.is_none()
            && matches!(call.args[0].expr.as_ref(), ast::Expr::Lit(ast::Lit::Str(spec))
                if matches!(spec.value.as_str(), Some("worker_threads" | "node:worker_threads")))
    }
}

struct Assigned<'a>(&'a mut HashSet<ast::Id>);
impl Visit for Assigned<'_> {
    fn visit_ident(&mut self, ident: &ast::Ident) {
        self.0.insert(ident.to_id());
    }
}

impl Visit for Origins {
    fn visit_var_decl(&mut self, decl: &ast::VarDecl) {
        if decl.kind == ast::VarDeclKind::Const {
            for binding in &decl.decls {
                if !binding
                    .init
                    .as_deref()
                    .is_some_and(|init| self.worker_require(init))
                {
                    continue;
                }
                match &binding.name {
                    ast::Pat::Ident(name) => {
                        self.namespaces.insert(name.id.to_id());
                    }
                    ast::Pat::Object(pattern) => {
                        for prop in &pattern.props {
                            match prop {
                                ast::ObjectPatProp::Assign(prop)
                                    if prop.key.sym == "Worker" && prop.value.is_none() =>
                                {
                                    self.workers.insert(prop.key.id.to_id());
                                }
                                ast::ObjectPatProp::KeyValue(prop) if matches!(&prop.key, ast::PropName::Ident(key) if key.sym == "Worker") => {
                                    if let ast::Pat::Ident(name) = prop.value.as_ref() {
                                        self.workers.insert(name.id.to_id());
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
        decl.visit_children_with(self);
    }
    fn visit_assign_expr(&mut self, expr: &ast::AssignExpr) {
        expr.left.visit_with(&mut Assigned(&mut self.modified));
        expr.visit_children_with(self);
    }
    fn visit_update_expr(&mut self, expr: &ast::UpdateExpr) {
        expr.arg.visit_with(&mut Assigned(&mut self.modified));
        expr.visit_children_with(self);
    }
}

struct Constructors<'a> {
    origins: &'a Origins,
    paths: Vec<String>,
}

impl Visit for Constructors<'_> {
    fn visit_new_expr(&mut self, expr: &ast::NewExpr) {
        let origin = match expr.callee.as_ref() {
            ast::Expr::Ident(name) => {
                self.origins.workers.contains(&name.to_id())
                    && !self.origins.modified.contains(&name.to_id())
            }
            ast::Expr::Member(member) if matches!(&member.prop, ast::MemberProp::Ident(prop) if prop.sym == "Worker") =>
            {
                matches!(member.obj.as_ref(), ast::Expr::Ident(name)
                    if self.origins.namespaces.contains(&name.to_id()) && !self.origins.modified.contains(&name.to_id()))
            }
            _ => false,
        };
        if origin {
            if let Some(args) = &expr.args {
                if (1..=2).contains(&args.len())
                    && args.iter().all(|arg| arg.spread.is_none())
                    && args.get(1).is_none_or(|arg| file_options(&arg.expr))
                {
                    if let ast::Expr::Lit(ast::Lit::Str(path)) = args[0].expr.as_ref() {
                        if let Some(path) = path.value.as_str() {
                            if (path.starts_with("./")
                                || path.starts_with("../")
                                || path.starts_with('/'))
                                && !self.paths.iter().any(|old| old == path)
                            {
                                self.paths.push(path.to_string());
                            }
                        }
                    }
                }
            }
        }
        expr.visit_children_with(self);
    }
}

fn file_options(expr: &ast::Expr) -> bool {
    let ast::Expr::Object(object) = expr else {
        return false;
    };
    object.props.iter().all(|prop| {
        let ast::PropOrSpread::Prop(prop) = prop else {
            return false;
        };
        let ast::Prop::KeyValue(prop) = prop.as_ref() else {
            return false;
        };
        let key = match &prop.key {
            ast::PropName::Ident(key) => Some(key.sym.as_ref()),
            ast::PropName::Str(key) => key.value.as_str(),
            _ => None,
        };
        match key {
            Some("eval") => {
                matches!(prop.value.as_ref(), ast::Expr::Lit(ast::Lit::Bool(value)) if !value.value)
            }
            Some(_) => true,
            None => false,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_lexically_sourced_file_workers_are_candidates() {
        let cases = [
            ("const {Worker} = require('worker_threads'); new Worker('./a.cjs');", true),
            ("const {Worker: W} = require('node:worker_threads'); new W('./a.cjs', {eval:false});", true),
            ("const t = require('worker_threads'); new t.Worker('./a.cjs');", true),
            ("const {Worker} = require('other'); new Worker('./a.cjs');", false),
            ("function f(require) {const {Worker}=require('worker_threads'); new Worker('./a.cjs');}", false),
            ("const {Worker}=require('worker_threads'); function f(Worker) {new Worker('./a.cjs');}", false),
            ("const {Worker}=require('worker_threads'); new Worker('./a.cjs', {eval:true});", false),
            ("const {Worker}=require('worker_threads'); new Worker('./a.cjs', options);", false),
            ("const {Worker}=require('worker_threads'); new Worker('./a.cjs', {...options});", false),
            ("const {Worker}=require('worker_threads'); new Worker('./a.cjs', {get eval(){return false;}});", false),
            ("require=custom; const {Worker}=require('worker_threads'); new Worker('./a.cjs');", false),
            ("let {Worker}=require('worker_threads'); new Worker('./a.cjs');", false),
            ("const {Worker=Custom}=require('worker_threads'); new Worker('./a.cjs');", false),
            ("const {Worker}=require('worker_threads'); new Worker(...args);", false),
        ];
        for (source, expected) in cases {
            assert_eq!(
                filenames(source, Path::new("test.cjs")),
                if expected { vec!["./a.cjs"] } else { vec![] },
                "{source}"
            );
        }
    }
}
