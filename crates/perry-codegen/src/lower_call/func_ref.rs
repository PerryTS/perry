//! User function call via `Expr::FuncRef(fid)` — direct LLVM call to a
//! known per-function symbol, with clamp-pattern intrinsification and
//! rest-parameter bundling.

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::{lower_expr, nanbox_pointer_inline, FnCtx};
use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I32, I64};

pub fn try_lower_func_ref_call(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
    // User function call via FuncRef.
    let Expr::FuncRef(fid) = callee else {
        return Ok(None);
    };
    // (Issue #436 plan #1) Clamp-pattern fast path: when the callee
    // is a function recognized as `clampIdx(v, lo, hi)` or
    // `clampU8(v)` and we're being lowered in an f64-required
    // context, emit `@llvm.smin.i32` / `@llvm.smax.i32` directly +
    // `sitofp` to double, mirroring the i32 path in
    // `lower_expr_as_i32`. The HIR inliner is configured to leave
    // these calls intact (`is_clamp3`/`is_clamp_u8` short-circuit
    // `is_inlinable`) so this path fires at every call site and the
    // `dowhile/break` shape that blocked LLVM's auto-vectorizer
    // never appears in the IR.
    if ctx.clamp3_functions.contains(fid) && args.len() == 3 {
        let v = crate::expr::lower_expr_as_i32(ctx, &args[0])?;
        let lo = crate::expr::lower_expr_as_i32(ctx, &args[1])?;
        let hi = crate::expr::lower_expr_as_i32(ctx, &args[2])?;
        let blk = ctx.block();
        let r1 = blk.fresh_reg();
        blk.emit_raw(format!(
            "{} = call i32 @llvm.smax.i32(i32 {}, i32 {})",
            r1, v, lo
        ));
        let r2 = blk.fresh_reg();
        blk.emit_raw(format!(
            "{} = call i32 @llvm.smin.i32(i32 {}, i32 {})",
            r2, r1, hi
        ));
        return Ok(Some(blk.sitofp(I32, &r2, DOUBLE)));
    }
    if ctx.clamp_u8_functions.contains(fid) && args.len() == 1 {
        let v = crate::expr::lower_expr_as_i32(ctx, &args[0])?;
        let blk = ctx.block();
        let r1 = blk.fresh_reg();
        blk.emit_raw(format!(
            "{} = call i32 @llvm.smax.i32(i32 {}, i32 0)",
            r1, v
        ));
        let r2 = blk.fresh_reg();
        blk.emit_raw(format!(
            "{} = call i32 @llvm.smin.i32(i32 {}, i32 255)",
            r2, r1
        ));
        return Ok(Some(blk.sitofp(I32, &r2, DOUBLE)));
    }

    let Some(fname) = ctx.func_names.get(fid).cloned() else {
        for a in args {
            let _ = lower_expr(ctx, a)?;
        }
        return Ok(Some(double_literal(0.0)));
    };

    // Rest parameter handling: if the called function has a
    // rest parameter, bundle all trailing args (those at and
    // beyond the rest position) into an array literal and
    // pass that as a single argument.
    let sig = ctx.func_signatures.get(fid).copied();
    let (declared_count, has_rest, _) = sig.unwrap_or((args.len(), false, false));
    let mut lowered: Vec<String> = Vec::with_capacity(declared_count);
    if has_rest && ctx.func_synthetic_arguments.contains(fid) {
        let fixed_count = declared_count.saturating_sub(1);
        let undef_lit = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        for idx in 0..fixed_count {
            if let Some(arg) = args.get(idx) {
                lowered.push(lower_expr(ctx, arg)?);
            } else {
                lowered.push(undef_lit.clone());
            }
        }

        let cap = (args.len() as u32).to_string();
        let mut current = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
        for a in args {
            let v = lower_expr(ctx, a)?;
            let blk = ctx.block();
            current = blk.call(I64, "js_array_push_f64", &[(I64, &current), (DOUBLE, &v)]);
        }
        let arguments_box = nanbox_pointer_inline(ctx.block(), &current);
        lowered.push(arguments_box);
    } else if has_rest {
        // Rest is always the LAST declared param. Pass the
        // first (declared_count - 1) args as-is, then bundle
        // the rest into an array.
        let fixed_count = declared_count.saturating_sub(1);
        for a in args.iter().take(fixed_count) {
            lowered.push(lower_expr(ctx, a)?);
        }
        // Materialize the rest array.
        let rest_count = args.len().saturating_sub(fixed_count);
        let cap = (rest_count as u32).to_string();
        let mut current = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
        for a in args.iter().skip(fixed_count) {
            let v = lower_expr(ctx, a)?;
            let blk = ctx.block();
            current = blk.call(I64, "js_array_push_f64", &[(I64, &current), (DOUBLE, &v)]);
        }
        let rest_box = nanbox_pointer_inline(ctx.block(), &current);
        lowered.push(rest_box);
    } else {
        for a in args {
            lowered.push(lower_expr(ctx, a)?);
        }
    }
    let arg_slices: Vec<(crate::types::LlvmType, &str)> =
        lowered.iter().map(|s| (DOUBLE, s.as_str())).collect();

    Ok(Some(ctx.block().call(DOUBLE, &fname, &arg_slices)))
}
