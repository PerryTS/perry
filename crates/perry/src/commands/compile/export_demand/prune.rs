//! Remove what the plan found dead from a module's AST before lowering.
//! Every import and re-export edge stays (as a bare `import 'x'` when no
//! binding of it is used), so evaluation order is unchanged.

use swc_ecma_ast as ast;

use super::plan::Prune;
use super::summary::export_name;

pub(crate) fn prune(module: &ast::Module, plan: &Prune) -> ast::Module {
    let mut out = module.clone();
    out.body = module
        .body
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            if plan.items.contains(&index) {
                return None;
            }
            match item {
                ast::ModuleItem::ModuleDecl(ast::ModuleDecl::Import(import))
                    if !import.type_only =>
                {
                    let mut import = import.clone();
                    import.specifiers.retain(|s| match s {
                        ast::ImportSpecifier::Named(n) if n.is_type_only => true,
                        ast::ImportSpecifier::Named(n) => !plan.imports.contains(&*n.local.sym),
                        ast::ImportSpecifier::Default(d) => !plan.imports.contains(&*d.local.sym),
                        ast::ImportSpecifier::Namespace(n) => !plan.imports.contains(&*n.local.sym),
                    });
                    Some(ast::ModuleItem::ModuleDecl(ast::ModuleDecl::Import(import)))
                }
                ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportNamed(named))
                    if !named.type_only =>
                {
                    let mut named = named.clone();
                    named.specifiers.retain(|s| match s {
                        ast::ExportSpecifier::Named(n) if n.is_type_only => true,
                        ast::ExportSpecifier::Named(n) => {
                            let exported = n.exported.as_ref().unwrap_or(&n.orig);
                            !plan.exports.contains(&export_name(exported))
                        }
                        ast::ExportSpecifier::Namespace(n) => {
                            !plan.exports.contains(&export_name(&n.name))
                        }
                        ast::ExportSpecifier::Default(d) => {
                            !plan.exports.contains(&*d.exported.sym)
                        }
                    });
                    if !named.specifiers.is_empty() {
                        return Some(ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportNamed(
                            named,
                        )));
                    }
                    // A re-export still evaluates its source: keep the edge.
                    let src = named.src?;
                    Some(ast::ModuleItem::ModuleDecl(ast::ModuleDecl::Import(
                        ast::ImportDecl {
                            span: named.span,
                            specifiers: Vec::new(),
                            src,
                            type_only: false,
                            with: named.with,
                            phase: ast::ImportPhase::Evaluation,
                        },
                    )))
                }
                _ => Some(item.clone()),
            }
        })
        .collect();
    out
}
