//! Global-object bindings emitted at the top of an entry module's `main`:
//! GlobalDeclarationInstantiation's `CreateGlobalFunctionBinding` for
//! Script-level function declarations, and `CreateGlobalVarBinding` for
//! Script `var`s and Annex B block-nested function declarations. Split out of
//! `entry.rs` for the 2000-line file-size gate.

use perry_hir::Module as HirModule;

use crate::expr::FnCtx;
use crate::types::{DOUBLE, I64, PTR};

pub(super) fn emit_script_global_function_decls(ctx: &mut FnCtx<'_>, hir: &HirModule) {
    for (name, fid) in &hir.script_global_functions {
        if ctx.block().is_terminated() {
            break;
        }
        let func_name = match ctx.func_names.get(fid) {
            Some(n) => n.clone(),
            None => continue,
        };
        let wrap_ptr = format!("@__perry_wrap_{}", func_name);
        let key_idx = ctx.strings.intern(name);
        let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
        let blk = ctx.block();
        let global_box = blk.call(DOUBLE, "js_get_global_this", &[]);
        let obj_raw = crate::expr::unbox_to_i64(blk, &global_box);
        let closure_handle = blk.call(I64, "js_closure_alloc_singleton", &[(PTR, &wrap_ptr)]);
        let closure_box = crate::expr::nanbox_pointer_inline(blk, &closure_handle);
        let key_box = blk.load(DOUBLE, &key_handle_global);
        let key_raw = crate::expr::unbox_to_i64(blk, &key_box);
        // #5833: GlobalDeclarationInstantiation's `CreateGlobalFunctionBinding`
        // runs with `D = false` for a Script (only sloppy-eval's Annex B.3.3.3
        // path uses `D = true`), so the reflected property must be
        // non-configurable — a plain `js_object_set_field_by_name` created it
        // configurable, failing `verifyProperty(this, name, {configurable:
        // false})` (test262 `language/global-code/decl-func.js`).
        blk.call_void(
            "js_object_set_field_by_name_nonconfigurable",
            &[(I64, &obj_raw), (I64, &key_raw), (DOUBLE, &closure_box)],
        );
    }
}

/// Emit the early global-object bindings for Script-level `var`s and Annex B
/// block-nested top-level function declarations —
/// `globalThis[name] = undefined`, as a non-configurable own property.
///
/// GlobalDeclarationInstantiation's `CreateGlobalVarBinding` (B.3.3.2 step
/// 5.b.i) runs for these names before any top-level statement executes, so
/// the property must already be observable — with value `undefined` — ahead
/// of the statement that later assigns the real value. The HIR reflection
/// pass keeps subsequent writes synchronized; this prelude establishes the
/// descriptor it must preserve (test262 `language/eval-code/*/
/// var-env-var-init-global-exstng` and Annex B global-init cases).
pub(super) fn emit_annexb_global_undefined_decls(ctx: &mut FnCtx<'_>, hir: &HirModule) {
    if hir.annexb_global_undefined_names.is_empty() {
        return;
    }
    let undef = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    for name in &hir.annexb_global_undefined_names {
        if ctx.block().is_terminated() {
            break;
        }
        let key_idx = ctx.strings.intern(name);
        let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
        let blk = ctx.block();
        let global_box = blk.call(DOUBLE, "js_get_global_this", &[]);
        let obj_raw = crate::expr::unbox_to_i64(blk, &global_box);
        let key_box = blk.load(DOUBLE, &key_handle_global);
        let key_raw = crate::expr::unbox_to_i64(blk, &key_box);
        blk.call_void(
            "js_object_set_field_by_name_nonconfigurable",
            &[(I64, &obj_raw), (I64, &key_raw), (DOUBLE, &undef)],
        );
    }
}
