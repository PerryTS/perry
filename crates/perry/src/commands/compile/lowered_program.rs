//! Whole-program facts applied to the lowered HIR before any transform,
//! codegen or facts pass.

use super::CompilationContext;

pub(super) fn settle(ctx: &mut CompilationContext) -> anyhow::Result<()> {
    // CommonJS closed world (`cjs_link`): one bit for the program, applied to
    // the wrapper's escape forms.
    ctx.cjs_closed = super::cjs_link::program_is_closed(&ctx.native_modules);
    for hir_module in ctx.native_modules.values_mut() {
        super::cjs_link::resolve_open_sites(hir_module, ctx.cjs_closed)?;
    }
    // reflect-metadata patches the realm's Reflect, which any module can read
    // while it loads, so every module starts with the install.
    if ctx
        .native_modules
        .values()
        .any(perry_hir::installs_reflect_metadata)
    {
        ctx.native_modules
            .values_mut()
            .for_each(perry_hir::prepend_reflect_metadata_install);
    }
    Ok(())
}
