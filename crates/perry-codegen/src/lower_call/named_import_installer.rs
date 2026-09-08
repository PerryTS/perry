//! Registry installation required before materializing named Node imports.

use perry_hir::Expr;

use crate::expr::FnCtx;

/// Named imports from runtime-backed Node modules are represented in HIR as
/// `js_native_module_named_esm_export_value(module, export)`. Unlike a
/// namespace/property read, that shape has no `NativeModuleRef` for the
/// ordinary installer paths to see. Arm the precise module before the runtime
/// mints and caches its bound callable: stream constructor statics
/// (`Readable.toWeb`) and `pipeline[util.promisify.custom]` are attached at
/// mint time, while indirect crypto calls (`promisify(scrypt)`) need the
/// dispatch bucket later.
pub(super) fn install_for_named_import(ctx: &mut FnCtx<'_>, name: &str, args: &[Expr]) {
    if name != "js_native_module_named_esm_export_value" {
        return;
    }
    let Some(Expr::String(module_name)) = args.first() else {
        return;
    };
    let bare = module_name.strip_prefix("node:").unwrap_or(module_name);
    if matches!(bare, "stream" | "crypto") {
        if let Some(install) = crate::nm_install::nm_install_symbol(bare) {
            ctx.block().call_void(install, &[]);
        }
    }
}
