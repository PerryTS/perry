//! #11826: the module-level Temporal Dead Zone check
//! (`perry_hir::tdz_check`).
//!
//! The checked binding is a module global; `module_globals_emit` seeds the
//! global of every binding a check names with `TAG_TDZ`, and the declarator's
//! store replaces it. The check is a load of the global's bits, one compare
//! and a cold call that raises the ReferenceError: no call, and so no
//! safepoint, on the path where the binding is initialized.
//!
//! The binding operand is never lowered as a read: constant folding would
//! turn it into the value the declarator installs later, which is exactly the
//! dead-zone read the check exists to catch.

use anyhow::{anyhow, Result};
use perry_hir::tdz_check::{TDZ_CHECK, TDZ_CHECK_THEN};
use perry_hir::Expr;

use super::FnCtx;
use crate::types::{DOUBLE, I64};

/// Lower `callee(args)` when it is a TDZ check; `None` for any other call.
pub(crate) fn try_lower(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
    let Expr::ExternFuncRef { name, .. } = callee else {
        return Ok(None);
    };
    let then = match name.as_str() {
        TDZ_CHECK => false,
        TDZ_CHECK_THEN => true,
        _ => return Ok(None),
    };
    let (Some(Expr::LocalGet(id)), Some(Expr::String(binding))) = (args.first(), args.get(1))
    else {
        return Err(anyhow!("malformed module TDZ check: {args:?}"));
    };
    let id = *id;
    // `check_then` evaluates its value before the check, as PutValue orders
    // a write after its right-hand side.
    let value = if then {
        let value = args
            .get(2)
            .ok_or_else(|| anyhow!("module TDZ check_then without a value"))?;
        Some(super::lower_expr(ctx, value)?)
    } else {
        None
    };
    let result = value.unwrap_or_else(|| {
        crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
    });

    // Only a module global is seeded with the sentinel. A binding this
    // function holds in a frame slot was never in a dead zone it can observe.
    if ctx.locals.contains_key(&id) {
        return Ok(Some(result));
    }
    let Some(global_name) = ctx.module_globals.get(&id).cloned() else {
        return Ok(Some(result));
    };
    let current = crate::codegen::global_transfer::load_module_global(ctx, id, &global_name);
    let bits = ctx.block().bitcast_double_to_i64(&current);
    let is_tdz = ctx.block().icmp_eq(I64, &bits, crate::nanbox::TAG_TDZ_I64);
    let throw_idx = ctx.new_block("module_tdz.throw");
    let merge_idx = ctx.new_block("module_tdz.ok");
    let throw_label = ctx.block_label(throw_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&is_tdz, &throw_label, &merge_label);

    ctx.current_block = throw_idx;
    // The throw allocates its error: a versioned-loop clone must poison its
    // caller first, as on any other cold call.
    crate::expr::emit_versioned_loop_callback_deopt(ctx);
    let interned = ctx.strings.intern(binding);
    let handle = format!("@{}", ctx.strings.entry(interned).handle_global);
    let name_value = ctx.block().load(DOUBLE, &handle);
    ctx.block().call(
        DOUBLE,
        perry_hir::tdz_check::TDZ_THROW,
        &[(DOUBLE, &name_value)],
    );
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    Ok(Some(result))
}
