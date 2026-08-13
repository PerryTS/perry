use swc_ecma_ast as ast;

use crate::lower::LoweringContext;

/// Register `perry/native` imports before any source-order type or expression
/// lowering. ES module imports are hoisted, so code before the declaration may
/// use both its type names and value aliases.
pub(in crate::lower) fn pre_register_native_profile_imports(
    ctx: &mut LoweringContext,
    module: &ast::Module,
) {
    for item in &module.body {
        let ast::ModuleItem::ModuleDecl(ast::ModuleDecl::Import(import_decl)) = item else {
            continue;
        };
        let source = import_decl.src.value.as_str().unwrap_or("");
        if source != "perry/native" {
            continue;
        }

        register_native_profile_type_imports(ctx, source, import_decl);
        if import_decl.type_only {
            continue;
        }
        for spec in &import_decl.specifiers {
            let ast::ImportSpecifier::Named(named) = spec else {
                continue;
            };
            if named.is_type_only {
                continue;
            }
            let local = named.local.sym.to_string();
            let imported = imported_name(named, &local);
            ctx.register_native_module(local, "perry/native".to_string(), Some(imported));
        }
    }
}

/// Preserve type-only imports from Perry's compiler-owned native profile.
/// Native modules have no source HIR declarations, so these aliases would
/// otherwise be erased before TypeScript annotation extraction sees them.
pub(super) fn register_native_profile_type_imports(
    ctx: &mut LoweringContext,
    source: &str,
    import_decl: &ast::ImportDecl,
) {
    if source != "perry/native" {
        return;
    }
    for spec in &import_decl.specifiers {
        let ast::ImportSpecifier::Named(named) = spec else {
            continue;
        };
        let local = named.local.sym.to_string();
        let imported = imported_name(named, &local);
        ctx.register_native_profile_type_alias(local, &imported);
    }
}

fn imported_name(named: &ast::ImportNamedSpecifier, local: &str) -> String {
    named
        .imported
        .as_ref()
        .map(|name| match name {
            ast::ModuleExportName::Ident(id) => id.sym.to_string(),
            ast::ModuleExportName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
        })
        .unwrap_or_else(|| local.to_string())
}
