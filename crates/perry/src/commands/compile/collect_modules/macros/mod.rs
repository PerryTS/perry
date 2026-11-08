//! Expand attributed macro calls before their modules enter the runtime graph.

use anyhow::{anyhow, bail, Context, Result};
use std::path::Path;
use swc_common::{Globals, Mark, Span, SyntaxContext, GLOBALS};
use swc_ecma_ast::{self as ast, Pass};
use swc_ecma_transforms_base::resolver;
use swc_ecma_visit::{Visit, VisitMut, VisitMutWith, VisitWith};

use super::super::{cached_resolve_import, CompilationContext};

mod arguments;
mod runner;
#[cfg(test)]
mod tests;
mod value;

fn is_macro(import: &ast::ImportDecl) -> bool {
    !import.type_only && import.with.as_ref().is_some_and(|attributes| {
        attributes.props.iter().any(|property| {
            let ast::PropOrSpread::Prop(property) = property else { return false };
            let ast::Prop::KeyValue(property) = property.as_ref() else { return false };
            let key = match &property.key {
                ast::PropName::Ident(key) => key.sym == "type",
                ast::PropName::Str(key) => key.value == "type",
                _ => false,
            };
            key && matches!(property.value.as_ref(), ast::Expr::Lit(ast::Lit::Str(value)) if value.value == "macro")
        })
    })
}

struct Binding<'a> {
    import: &'a ast::ImportDecl,
    export: String,
}

fn binding<'a>(module: &'a ast::Module, expr: &ast::Expr) -> Option<Binding<'a>> {
    let (id, member) = match expr {
        ast::Expr::Ident(id) => (id.to_id(), None),
        ast::Expr::Member(member) => {
            let ast::Expr::Ident(id) = member.obj.as_ref() else {
                return None;
            };
            let name = match &member.prop {
                ast::MemberProp::Ident(name) => name.sym.to_string(),
                ast::MemberProp::Computed(key) => match key.expr.as_ref() {
                    ast::Expr::Lit(ast::Lit::Str(name)) => name.value.as_str()?.to_string(),
                    _ => return None,
                },
                _ => return None,
            };
            (id.to_id(), Some(name))
        }
        ast::Expr::Paren(paren) => return binding(module, &paren.expr),
        _ => return None,
    };
    module.body.iter().find_map(|item| {
        let ast::ModuleItem::ModuleDecl(ast::ModuleDecl::Import(import)) = item else {
            return None;
        };
        if !is_macro(import) {
            return None;
        }
        import.specifiers.iter().find_map(|specifier| {
            let export = match specifier {
                ast::ImportSpecifier::Named(named)
                    if !named.is_type_only && member.is_none() && named.local.to_id() == id =>
                {
                    named.imported.as_ref().map_or_else(
                        || named.local.sym.to_string(),
                        |name| match name {
                            ast::ModuleExportName::Ident(name) => name.sym.to_string(),
                            ast::ModuleExportName::Str(name) => {
                                name.value.as_str().unwrap_or_default().to_string()
                            }
                        },
                    )
                }
                ast::ImportSpecifier::Default(default)
                    if member.is_none() && default.local.to_id() == id =>
                {
                    "default".to_string()
                }
                ast::ImportSpecifier::Namespace(namespace) if namespace.local.to_id() == id => {
                    member.clone().unwrap_or_else(|| "*".to_string())
                }
                _ => return None,
            };
            Some(Binding { import, export })
        })
    })
}

struct Invocation {
    span: Span,
    identity: SyntaxContext,
    specifier: String,
    export: String,
    arguments: Vec<String>,
}

struct Plan<'a> {
    module: &'a ast::Module,
    calls: Vec<Invocation>,
    error: Option<anyhow::Error>,
}

impl Visit for Plan<'_> {
    fn visit_call_expr(&mut self, call: &ast::CallExpr) {
        call.visit_children_with(self);
        let ast::Callee::Expr(callee) = &call.callee else {
            return;
        };
        let Some(binding) = binding(self.module, callee) else {
            return;
        };
        if self.error.is_some() {
            return;
        }
        let specifier = binding
            .import
            .src
            .value
            .as_str()
            .unwrap_or_default()
            .to_string();
        let result = call
            .args
            .iter()
            .map(|arg| {
                if arg.spread.is_some() {
                    bail!("spread macro arguments are not statically known");
                }
                arguments::source(&arg.expr, self.module, &self.calls, &mut Vec::new())
            })
            .collect::<Result<Vec<_>>>();
        match result {
            Ok(arguments) => {
                self.calls.push(Invocation {
                    span: call.span,
                    identity: call.ctxt,
                    specifier,
                    export: binding.export,
                    arguments,
                });
            }
            Err(error) => {
                self.error = Some(error.context(format!(
                    "macro import {specifier:?} export {:?} at byte {}",
                    binding.export, call.span.lo.0
                )))
            }
        }
    }
}

fn check_escapes(module: &ast::Module) -> Result<()> {
    struct Escapes<'a> {
        module: &'a ast::Module,
        error: Option<anyhow::Error>,
    }
    impl Visit for Escapes<'_> {
        fn visit_import_decl(&mut self, _: &ast::ImportDecl) {}
        fn visit_ts_type(&mut self, _: &ast::TsType) {}
        fn visit_prop(&mut self, prop: &ast::Prop) {
            if let ast::Prop::Shorthand(name) = prop {
                self.visit_expr(&ast::Expr::Ident(name.clone()));
            } else {
                prop.visit_children_with(self);
            }
        }
        fn visit_named_export(&mut self, export: &ast::NamedExport) {
            if export.type_only || export.src.is_some() {
                return;
            }
            for specifier in &export.specifiers {
                if let ast::ExportSpecifier::Named(named) = specifier {
                    if !named.is_type_only {
                        if let ast::ModuleExportName::Ident(name) = &named.orig {
                            self.visit_expr(&ast::Expr::Ident(name.clone()));
                        }
                    }
                }
            }
        }
        fn visit_call_expr(&mut self, call: &ast::CallExpr) {
            if matches!(&call.callee, ast::Callee::Expr(callee) if binding(self.module, callee).is_some())
            {
                call.args.visit_with(self);
            } else {
                call.visit_children_with(self);
            }
        }
        fn visit_expr(&mut self, expr: &ast::Expr) {
            if let Some(binding) = binding(self.module, expr) {
                self.error = Some(anyhow!(
                    "macro import {:?} export {:?} cannot escape a direct compile-time call",
                    binding.import.src.value,
                    binding.export
                ));
            }
            expr.visit_children_with(self);
        }
    }
    let mut scan = Escapes {
        module,
        error: None,
    };
    module.visit_with(&mut scan);
    if let Some(error) = scan.error {
        return Err(error);
    }
    Ok(())
}

pub(super) fn expand(
    module: &ast::Module,
    importer: &Path,
    ctx: &mut CompilationContext,
) -> Result<Option<ast::Module>> {
    if !module.body.iter().any(|item| matches!(item, ast::ModuleItem::ModuleDecl(ast::ModuleDecl::Import(import)) if is_macro(import))) {
        return Ok(None);
    }
    expand_with(module, |module, calls| {
        if ctx.macro_import_stack.iter().any(|path| path == importer) {
            bail!(
                "cyclic macro expansion in {} (import stack: {:?})",
                importer.display(),
                ctx.macro_import_stack
            );
        }
        let mut paths = Vec::new();
        for call in calls {
            let (path, _) =
                cached_resolve_import(&call.specifier, importer, ctx).with_context(|| {
                    format!(
                        "cannot resolve macro import {:?} in {}",
                        call.specifier,
                        importer.display()
                    )
                })?;
            paths.push(path);
        }
        let _ = module;
        runner::execute(
            importer,
            calls,
            &paths,
            &ctx.macro_import_stack,
            &mut ctx.has_build_time_execution,
        )
    })
    .with_context(|| format!("expanding macro imports in {}", importer.display()))
    .map(Some)
}

fn expand_with(
    module: &ast::Module,
    evaluate: impl FnOnce(&ast::Module, &[Invocation]) -> Result<Vec<ast::Expr>>,
) -> Result<ast::Module> {
    GLOBALS.set(&Globals::new(), || {
        let mut program = ast::Program::Module(module.clone());
        resolver(Mark::new(), Mark::new(), true).process(&mut program);
        let ast::Program::Module(mut module) = program else { unreachable!() };
        // Rewrites can give distinct calls the same span. Hygiene identity
        // travels with a cloned call and disappears when the call is inlined.
        struct Identify<'a>(&'a ast::Module);
        impl VisitMut for Identify<'_> {
            fn visit_mut_call_expr(&mut self, call: &mut ast::CallExpr) {
                call.visit_mut_children_with(self);
                if matches!(&call.callee, ast::Callee::Expr(callee) if binding(self.0, callee).is_some()) {
                    call.ctxt = SyntaxContext::empty().apply_mark(Mark::new());
                }
            }
        }
        let scope = module.clone();
        module.visit_mut_with(&mut Identify(&scope));
        check_escapes(&module)?;
        let mut plan = Plan { module: &module, calls: Vec::new(), error: None };
        module.visit_with(&mut plan);
        if let Some(error) = plan.error { return Err(error); }
        let values = if plan.calls.is_empty() { Vec::new() } else { evaluate(&module, &plan.calls)? };
        if values.len() != plan.calls.len() { bail!("macro runner returned the wrong number of results"); }
        struct Inline<'a> { calls: &'a [Invocation], values: &'a [ast::Expr] }
        impl VisitMut for Inline<'_> {
            fn visit_mut_expr(&mut self, expr: &mut ast::Expr) {
                expr.visit_mut_children_with(self);
                let ast::Expr::Call(call) = expr else { return };
                if let Some(i) = self.calls.iter().position(|item| item.identity == call.ctxt) {
                    let span = call.span;
                    *expr = self.values[i].clone();
                    struct Location(Span);
                    impl VisitMut for Location {
                        fn visit_mut_span(&mut self, span: &mut Span) { *span = self.0; }
                    }
                    expr.visit_mut_with(&mut Location(span));
                }
            }
        }
        let calls = plan.calls;
        module.visit_mut_with(&mut Inline { calls: &calls, values: &values });
        module.body.retain(|item| !matches!(item, ast::ModuleItem::ModuleDecl(ast::ModuleDecl::Import(import)) if is_macro(import)));
        // Resolver contexts belong to the local GLOBALS. The next lowering
        // starts with ordinary parser syntax and reconstructs lexical scopes.
        struct ResetHygiene;
        impl VisitMut for ResetHygiene {
            fn visit_mut_syntax_context(&mut self, context: &mut SyntaxContext) {
                *context = SyntaxContext::empty();
            }
        }
        module.visit_mut_with(&mut ResetHygiene);
        Ok(module)
    })
}
