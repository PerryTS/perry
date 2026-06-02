//! Conditional and logical expression lowering.
//!
//! Contains `lower_conditional` (ternary), `lower_logical` (&&/||/??),
//! and `lower_truthy` (truthiness test).

use anyhow::Result;
use perry_hir::{Expr, LogicalOp};

use crate::expr::{lower_expr, FnCtx};
use crate::type_analysis::{is_bool_expr, is_numeric_expr};
use crate::types::{DOUBLE, I32, I64};

/// Convert a lowered condition value to an `i1` for `cond_br`.
///
/// Fast path 1 (numeric): if the expression is statically a numeric
/// double, emit `fcmp one cond, 0.0` (5-cycle ALU op).
///
/// Fast path 2 (NaN-boxed bool): if the expression is a `Compare` /
/// `Logical` / `Bool` / known-bool local, the lowered value is the
/// NaN-tagged `TAG_TRUE` / `TAG_FALSE` bit pattern. Inline the
/// truthiness check as `bitcast → icmp ne TAG_FALSE` (2 ALU ops, no
/// function call). This is the **dominant cost** in tight loops where
/// the loop condition is `i < N` — without this fast path, every
/// iteration calls `js_is_truthy` which prevents LLVM from
/// constant-propagating / hoisting the comparison.
///
/// Slow path: for everything else (strings, objects, unions),
/// dispatch through `js_is_truthy(double) -> i32` which inspects the
/// NaN tag to handle null/undefined/false correctly. The slow path is
/// a function call but produces correct results across the entire JS
/// truthiness table.
pub(crate) fn lower_truthy(ctx: &mut FnCtx<'_>, cond_val: &str, cond_expr: &Expr) -> String {
    if is_numeric_expr(ctx, cond_expr) {
        return ctx.block().fcmp("one", cond_val, "0.0");
    }
    if is_bool_expr(ctx, cond_expr) {
        // The lowered cond_val is *normally* NaN-boxed TAG_TRUE or TAG_FALSE,
        // but for optional `boolean` parameters that the caller didn't pass,
        // codegen pads the missing arg with TAG_UNDEFINED at the call site
        // (see `lower_call.rs::pad with TAG_UNDEFINED`). Pre-fix the
        // `bits != TAG_FALSE` shortcut treated TAG_UNDEFINED as truthy —
        // `if (optionalFlag)` for `optionalFlag: boolean | undefined`
        // unconditionally took the truthy branch on `undefined`, even
        // though JS truthiness says `if (undefined)` is falsy. ECS
        // sync-hotpath / perf-comprehensive crashed on this because
        // `world.query([Position])` (single-arg) dispatched into the
        // `includeComponents=true` overload variant — the function returned
        // an `Array<{entity, components}>` of length 0 and downstream
        // assertions on the entity count fired.
        //
        // Use `bits == TAG_TRUE` instead, which is also two ALU ops and
        // correctly reports `false` for both TAG_FALSE and TAG_UNDEFINED.
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(cond_val);
        return blk.icmp_eq(I64, &bits, crate::nanbox::TAG_TRUE_I64);
    }
    let i32_truthy = ctx.block().call(I32, "js_is_truthy", &[(DOUBLE, cond_val)]);
    ctx.block().icmp_ne(I32, &i32_truthy, "0")
}

/// Lower `cond ? then_expr : else_expr` to a 4-block CFG with a phi at
/// the merge: condition → conditional cond_br → then → merge ← else.
/// Both then and else are always lowered (no short-circuit), but only one
/// runs at runtime depending on the condition.
pub(crate) fn lower_conditional(
    ctx: &mut FnCtx<'_>,
    condition: &Expr,
    then_expr: &Expr,
    else_expr: &Expr,
) -> Result<String> {
    let cond = lower_expr(ctx, condition)?;
    let cond_bool = lower_truthy(ctx, &cond, condition);

    let then_idx = ctx.new_block("ternary.then");
    let else_idx = ctx.new_block("ternary.else");
    let merge_idx = ctx.new_block("ternary.merge");

    let then_label = ctx.block_label(then_idx);
    let else_label = ctx.block_label(else_idx);
    let merge_label = ctx.block_label(merge_idx);

    ctx.block().cond_br(&cond_bool, &then_label, &else_label);

    ctx.current_block = then_idx;
    let then_val = lower_expr(ctx, then_expr)?;
    let then_after_label = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = else_idx;
    let else_val = lower_expr(ctx, else_expr)?;
    let else_after_label = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = merge_idx;
    Ok(ctx.block().phi(
        DOUBLE,
        &[
            (&then_val, &then_after_label),
            (&else_val, &else_after_label),
        ],
    ))
}

/// Lower `a && b` / `a || b` with short-circuit evaluation.
///
/// Pattern (for `&&` — `||` swaps the cond_br targets):
/// ```llvm
///   ; <current>: evaluate left, branch on truthiness
///   %l = <lower left>
///   %lb = fcmp one double %l, 0.0
///   br i1 %lb, label %then, label %merge
/// then:
///   %r = <lower right>
///   br label %merge
/// merge:
///   %result = phi double [ %l, %left_block ], [ %r, %right_block ]
/// ```
///
/// The phi predecessors are captured AFTER lowering each side, because
/// `lower_expr` may itself create new blocks (nested if/logical/etc.) and
/// the actual incoming block is the last block of that subexpression's
/// codegen, not the original entry block we started in.
///
/// `??` (Coalesce) needs runtime null/undefined NaN-tag checks via
/// `js_is_truthy` or a dedicated `js_is_nullish` helper — deferred.
pub(crate) fn lower_logical(
    ctx: &mut FnCtx<'_>,
    op: LogicalOp,
    left: &Expr,
    right: &Expr,
) -> Result<String> {
    // ?? — nullish coalesce. Inline test: bitcast left to i64, compare
    // against TAG_NULL_I64 and TAG_UNDEFINED_I64. If either matches, the
    // value is "nullish" and we return the right side; otherwise return
    // the left.
    if matches!(op, LogicalOp::Coalesce) {
        let l = lower_expr(ctx, left)?;
        let l_block_label = ctx.block().label.clone();
        let blk = ctx.block();
        let l_bits = blk.bitcast_double_to_i64(&l);
        let is_null = blk.icmp_eq(I64, &l_bits, crate::nanbox::TAG_NULL_I64);
        let is_undef = blk.icmp_eq(I64, &l_bits, crate::nanbox::TAG_UNDEFINED_I64);
        let is_nullish = blk.or(crate::types::I1, &is_null, &is_undef);

        let then_idx = ctx.new_block("coalesce.right");
        let merge_idx = ctx.new_block("coalesce.merge");
        let then_label = ctx.block_label(then_idx);
        let merge_label = ctx.block_label(merge_idx);

        ctx.block().cond_br(&is_nullish, &then_label, &merge_label);

        ctx.current_block = then_idx;
        let r = lower_expr(ctx, right)?;
        let r_block_label = ctx.block().label.clone();
        if !ctx.block().is_terminated() {
            ctx.block().br(&merge_label);
        }

        ctx.current_block = merge_idx;
        return Ok(ctx
            .block()
            .phi(DOUBLE, &[(&l, &l_block_label), (&r, &r_block_label)]));
    }

    // Lower left in the current block.
    let l = lower_expr(ctx, left)?;
    // Capture the post-left block — left's lowering may have created new
    // blocks via nested control flow.
    let l_block_label = ctx.block().label.clone();
    // Truthiness test: fast fcmp for numeric, js_is_truthy for NaN-boxed.
    let l_bool = lower_truthy(ctx, &l, left);

    let then_idx = ctx.new_block("logical.then");
    let merge_idx = ctx.new_block("logical.merge");
    let then_label = ctx.block_label(then_idx);
    let merge_label = ctx.block_label(merge_idx);

    match op {
        LogicalOp::And => {
            // a && b: if a true, evaluate b; otherwise short-circuit to merge
            ctx.block().cond_br(&l_bool, &then_label, &merge_label);
        }
        LogicalOp::Or => {
            // a || b: if a true, short-circuit to merge; otherwise evaluate b
            ctx.block().cond_br(&l_bool, &merge_label, &then_label);
        }
        LogicalOp::Coalesce => unreachable!("guarded above"),
    }

    // The "then" block evaluates the right side.
    ctx.current_block = then_idx;
    let r = lower_expr(ctx, right)?;
    let r_block_label = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    // Merge block: phi between l (short-circuit path) and r (normal path).
    ctx.current_block = merge_idx;
    Ok(ctx
        .block()
        .phi(DOUBLE, &[(&l, &l_block_label), (&r, &r_block_label)]))
}
