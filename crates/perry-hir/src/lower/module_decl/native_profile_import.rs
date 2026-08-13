use swc_ecma_ast as ast;

use crate::lower::LoweringContext;

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
        let imported = named
            .imported
            .as_ref()
            .map(|name| match name {
                ast::ModuleExportName::Ident(id) => id.sym.to_string(),
                ast::ModuleExportName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
            })
            .unwrap_or_else(|| local.clone());
        ctx.register_native_profile_type_alias(local, &imported);
    }
}
