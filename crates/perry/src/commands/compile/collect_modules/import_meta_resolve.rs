//! Resolve literal module/asset URLs without importing or executing the target.

use super::super::{cached_resolve_import, CompilationContext};
use anyhow::Result;
use std::path::{Path, PathBuf};
use swc_ecma_ast as ast;
use swc_ecma_visit::{Visit, VisitMut, VisitMutWith, VisitWith};

fn is_resolve(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::Paren(paren) => is_resolve(&paren.expr),
        ast::Expr::Member(member) => {
            matches!(member.obj.as_ref(), ast::Expr::MetaProp(meta) if meta.kind == ast::MetaPropKind::ImportMeta)
                && match &member.prop {
                    ast::MemberProp::Ident(name) => name.sym == "resolve",
                    ast::MemberProp::Computed(key) => {
                        matches!(key.expr.as_ref(), ast::Expr::Lit(ast::Lit::Str(name)) if name.value.as_str() == Some("resolve"))
                    }
                    _ => false,
                }
        }
        _ => false,
    }
}

pub(super) fn resolve_static(
    module: &ast::Module,
    importer: &Path,
    ctx: &mut CompilationContext,
) -> Result<Option<ast::Module>> {
    struct Find(bool);
    impl Visit for Find {
        fn visit_call_expr(&mut self, call: &ast::CallExpr) {
            self.0 |= matches!(&call.callee, ast::Callee::Expr(expr) if is_resolve(expr));
            call.visit_children_with(self);
        }
    }
    let mut find = Find(false);
    module.visit_with(&mut find);
    if !find.0 {
        return Ok(None);
    }
    let mut result = module.clone();
    result.visit_mut_with(&mut Resolve { importer, ctx });
    Ok(Some(result))
}

struct Resolve<'a> {
    importer: &'a Path,
    ctx: &'a mut CompilationContext,
}
impl VisitMut for Resolve<'_> {
    fn visit_mut_expr(&mut self, expr: &mut ast::Expr) {
        expr.visit_mut_children_with(self);
        let ast::Expr::Call(call) = expr else { return };
        if !matches!(&call.callee, ast::Callee::Expr(expr) if is_resolve(expr))
            || call.args.is_empty()
            || call.args.len() > 2
            || call.args.iter().any(|arg| arg.spread.is_some())
        {
            return;
        }
        let ast::Expr::Lit(ast::Lit::Str(spec)) = call.args[0].expr.as_ref() else {
            return;
        };
        let Some(spec) = spec.value.as_str() else {
            return;
        };
        let importer = if let Some(parent) = call.args.get(1) {
            let ast::Expr::Lit(ast::Lit::Str(parent)) = parent.expr.as_ref() else {
                return;
            };
            let Some(parent) = parent.value.as_str() else {
                return;
            };
            if parent.starts_with("file:") {
                let Some(path) = url::Url::parse(parent)
                    .ok()
                    .and_then(|url| url.to_file_path().ok())
                else {
                    return;
                };
                if parent.ends_with('/') || path.is_dir() {
                    path.join("__perry_resolve__.ts")
                } else {
                    path
                }
            } else {
                let path = PathBuf::from(parent);
                if !path.is_absolute() {
                    return;
                }
                if path.is_file() {
                    path
                } else {
                    path.join("__perry_resolve__.ts")
                }
            }
        } else {
            self.importer.to_path_buf()
        };
        let resolved = if spec.starts_with("file:") {
            url::Url::parse(spec)
                .ok()
                .and_then(|url| url.to_file_path().ok())
                .filter(|path| path.is_file())
        } else {
            cached_resolve_import(spec, &importer, self.ctx)
                .map(|(path, _)| path)
                .filter(|path| path.is_file())
        };
        let Some(path) = resolved.and_then(|path| std::fs::canonicalize(path).ok()) else {
            return;
        };
        let Ok(url) = url::Url::from_file_path(&path) else {
            return;
        };
        self.ctx.resolve_inputs.insert(path);
        *expr = ast::Expr::Lit(ast::Lit::Str(ast::Str {
            span: call.span,
            value: url.to_string().into(),
            raw: None,
        }));
    }
}
