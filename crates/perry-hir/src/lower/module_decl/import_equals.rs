use super::*;

/// `import G = Ns.Member` (an entity name, not `require`) is the declaration
/// tsc emits for it: `var G = Ns.Member`, or `export var` for `export import`.
/// A namespace is an ordinary object (#12775). Rewritten before any pass, so
/// the hoisting pre-scan, export registration and lowering all see an
/// ordinary `var`. `None` (no such alias) lowers the original with no clone.
pub(crate) fn rewrite_import_equals_aliases(ast_module: &ast::Module) -> Option<ast::Module> {
    let is_alias = |item: &ast::ModuleItem| {
        matches!(
            item,
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::TsImportEquals(d))
                if !d.is_type_only && matches!(d.module_ref, ast::TsModuleRef::TsEntityName(_))
        )
    };
    if !ast_module.body.iter().any(is_alias) {
        return None;
    }
    let mut out = ast_module.clone();
    for item in &mut out.body {
        let ast::ModuleItem::ModuleDecl(ast::ModuleDecl::TsImportEquals(d)) = item else {
            continue;
        };
        let ast::TsModuleRef::TsEntityName(entity) = &d.module_ref else {
            continue;
        };
        if d.is_type_only {
            continue;
        }
        let var = ast::Decl::Var(Box::new(ast::VarDecl {
            span: d.span,
            ctxt: Default::default(),
            kind: ast::VarDeclKind::Var,
            declare: false,
            decls: vec![ast::VarDeclarator {
                span: d.span,
                name: ast::Pat::Ident(d.id.clone().into()),
                init: Some(Box::new(ts_entity_expr(entity))),
                definite: false,
            }],
        }));
        *item = if d.is_export {
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(ast::ExportDecl {
                span: d.span,
                decl: var,
            }))
        } else {
            ast::ModuleItem::Stmt(ast::Stmt::Decl(var))
        };
    }
    Some(out)
}

/// `A.B.C` of a TypeScript entity name as the member expression it reads.
pub(super) fn ts_entity_expr(entity: &ast::TsEntityName) -> ast::Expr {
    match entity {
        ast::TsEntityName::Ident(ident) => ast::Expr::Ident(ident.clone()),
        ast::TsEntityName::TsQualifiedName(q) => ast::Expr::Member(ast::MemberExpr {
            span: q.span,
            obj: Box::new(ts_entity_expr(&q.left)),
            prop: ast::MemberProp::Ident(q.right.clone()),
        }),
    }
}
