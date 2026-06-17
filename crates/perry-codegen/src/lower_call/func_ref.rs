//! User function call via `Expr::FuncRef(fid)` — direct LLVM call to a
//! known per-function symbol, with clamp-pattern intrinsification and
//! rest-parameter bundling.

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::{lower_expr, nanbox_pointer_inline, FnCtx};
use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I32, I64, PTR};

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
    //
    // clamp3-shaped functions return one of their ARGUMENTS verbatim, so
    // the i32 intrinsification is only sound when every argument is
    // provably i32-lowerable (`can_lower_expr_as_i32` — whose contract
    // `lower_expr_as_i32` requires anyway). Unconditional intrinsification
    // fptosi'd fractional doubles (`clamp3(2.5, 0, 5)` returned 2) and
    // NaN-boxed pointers (i32::MIN — the #4785 `(number).method is not a
    // function` bug class) at every call site. Non-i32 arguments fall
    // through to the ordinary direct call, whose compiled body has the
    // correct verbatim-return semantics. clampU8 stays unconditional: its
    // detector verifies the body ends in `return v | 0`, and fptosi +
    // smax(0)/smin(255) agrees with that coercion for every f64 input
    // (out-of-range values hit the clamp bounds first; NaN and boxed
    // pointers coerce to 0 either way).
    if ctx.clamp3_functions.contains(fid) && args.len() == 3 {
        let args_are_i32 = args.iter().all(|a| {
            crate::expr::can_lower_expr_as_i32(
                a,
                &ctx.i32_counter_slots,
                ctx.flat_const_arrays,
                &ctx.array_row_aliases,
                ctx.integer_locals,
                ctx.clamp3_functions,
                ctx.clamp_u8_functions,
                ctx.integer_returning_functions,
                ctx.i32_identity_functions,
            )
        });
        if args_are_i32 {
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
    let (declared_count, has_rest, _, synthetic_is_rest) =
        sig.unwrap_or((args.len(), false, false, false));
    let mut lowered: Vec<String> = Vec::with_capacity(declared_count);
    if ctx.func_synthetic_arguments.contains(fid) && has_rest && !synthetic_is_rest {
        let lowered_args: Vec<String> = args
            .iter()
            .map(|arg| lower_expr(ctx, arg))
            .collect::<Result<_>>()?;
        let fixed_count = declared_count.saturating_sub(2);
        let undef_lit = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        for idx in 0..fixed_count {
            if let Some(arg) = lowered_args.get(idx) {
                lowered.push(arg.clone());
            } else {
                lowered.push(undef_lit.clone());
            }
        }

        let rest_count = args.len().saturating_sub(fixed_count);
        let cap = (rest_count as u32).to_string();
        let mut current = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
        for v in lowered_args.iter().skip(fixed_count) {
            let blk = ctx.block();
            current = blk.call(
                I64,
                "js_array_push_f64",
                &[(I64, &current), (DOUBLE, v.as_str())],
            );
        }
        let rest_box = nanbox_pointer_inline(ctx.block(), &current);
        lowered.push(rest_box);

        let cap = (args.len() as u32).to_string();
        let mut current = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
        for v in &lowered_args {
            let blk = ctx.block();
            current = blk.call(
                I64,
                "js_array_push_f64",
                &[(I64, &current), (DOUBLE, v.as_str())],
            );
        }
        let arguments_box = nanbox_pointer_inline(ctx.block(), &current);
        lowered.push(arguments_box);
    } else if has_rest && ctx.func_synthetic_arguments.contains(fid) {
        let lowered_args: Vec<String> = args
            .iter()
            .map(|arg| lower_expr(ctx, arg))
            .collect::<Result<_>>()?;
        let fixed_count = declared_count.saturating_sub(1);
        let undef_lit = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        for idx in 0..fixed_count {
            if let Some(arg) = lowered_args.get(idx) {
                lowered.push(arg.clone());
            } else {
                lowered.push(undef_lit.clone());
            }
        }

        let cap = (args.len() as u32).to_string();
        let mut current = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
        for v in &lowered_args {
            let blk = ctx.block();
            current = blk.call(
                I64,
                "js_array_push_f64",
                &[(I64, &current), (DOUBLE, v.as_str())],
            );
        }
        current = ctx
            .block()
            .call(I64, "js_array_mark_arguments_object", &[(I64, &current)]);
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

    // OrdinaryCallBindThis for a receiverless call: `f()` binds `this` to
    // undefined (sloppy bodies then substitute globalThis at the read).
    // Without the reset, a bare call inside a method body leaks the
    // enclosing dispatch's IMPLICIT_THIS into the callee — a nested
    // `function inner(){ return this; }` called as `inner()` inside
    // `o.m()` must NOT see `o` (#3576). Gated on the callee actually
    // reading dynamic `this` so ordinary helper calls pay nothing. Args
    // are lowered BEFORE the reset: `this` inside an argument expression
    // still sees the enclosing binding.
    let resets_this = ctx.funcs_reading_dynamic_this.contains(fid);
    let prev_this = if resets_this {
        let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        Some(
            ctx.block()
                .call(DOUBLE, "js_implicit_this_set", &[(DOUBLE, &undef)]),
        )
    } else {
        None
    };
    let result = ctx.block().call(DOUBLE, &fname, &arg_slices);
    if let Some(prev) = &prev_this {
        let _ = ctx
            .block()
            .call(DOUBLE, "js_implicit_this_set", &[(DOUBLE, prev)]);
    }
    if ctx.local_generator_funcs.contains(fid) {
        let wrap_ptr = format!("@__perry_wrap_{}", fname);
        let closure_handle =
            ctx.block()
                .call(I64, "js_closure_alloc_singleton", &[(PTR, &wrap_ptr)]);
        return Ok(Some(ctx.block().call(
            DOUBLE,
            "js_generator_attach_closure_prototype",
            &[(DOUBLE, &result), (I64, &closure_handle)],
        )));
    }

    Ok(Some(result))
}
