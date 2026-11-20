//! N1 phase 1: integer temporaries stay integers (representation RFC §10.3,
//! P1 "expression temporaries take the fact's representation").
//!
//! A temporary has no binding, so it has no fact of its own: its
//! representation is the lattice join of what it is built from. A
//! conditional whose two arms are I32 is I32, and so is `Math.min`,
//! `Math.max` or `Math.abs` of I32 operands. These are selections: they make
//! no value the operands did not have, so no interval grows and nothing can
//! wrap. Before this module the i32 chain (`i32_chain_magnitude_bits`)
//! refused them, so `(m & EP) !== 0 ? to - c * 16 : to` or
//! `Math.min(7, file + 1)` was computed in doubles and converted at the
//! integer consumer (`cvttsd2si` into an I32 local, `cvtsi2sd` of the other
//! compare operand every loop iteration).
//!
//! Two parts:
//! - the chain admits the selections (`selection_bits`) and lowers them
//!   natively (`lower_selection`): `select`/`phi`, `llvm.smin`, `llvm.smax`,
//!   `llvm.abs`;
//! - a relational or equality test whose two operands are exact int32
//!   chains is one `icmp` (`try_lower_compare`). Exact means the chain value
//!   is the JavaScript value, not only its low 32 bits: an int literal, a
//!   canonical signed-i32 local, a range-proven expression, or a selection
//!   of exact operands.
//!
//! `PERRY_WP_REP_TEMPS=0` restores the double lowering of both.

use anyhow::Result;
use perry_hir::{CompareOp, Expr};

use super::{FnCtx, I32ChainEnv, I32_CHAIN_LEAF_BITS};
use crate::types::I32;

pub(crate) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_WP_REP_TEMPS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// The magnitude bound of a selection over i32-chain operands, or `None`
/// when `e` is no selection the chain admits. `Math.abs` needs
/// `|v| < 2^31` of its operand: `abs(-2^31)` leaves i32.
pub(super) fn selection_bits(
    e: &Expr,
    env: I32ChainEnv<'_>,
    bits: fn(&Expr, I32ChainEnv<'_>) -> Option<u32>,
) -> Option<u32> {
    if !enabled() {
        return None;
    }
    match e {
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } if pure_test(condition) => Some(bits(then_expr, env)?.max(bits(else_expr, env)?)),
        Expr::MathMin(args) | Expr::MathMax(args) if !args.is_empty() => {
            let mut out = 0;
            for a in args {
                out = out.max(bits(a, env)?);
            }
            Some(out)
        }
        Expr::MathAbs(x) => bits(x, env).filter(|&b| b < I32_CHAIN_LEAF_BITS),
        _ => None,
    }
}

/// A conditional's test the chain may carry: reads of locals and literals
/// under operators, no call, write or property read. A chain is an operand
/// like any other leaf, and some consumers lower an operand on more than
/// one path; a test that runs code must not be one of them.
fn pure_test(e: &Expr) -> bool {
    match e {
        Expr::Integer(_) | Expr::Number(_) | Expr::Bool(_) | Expr::LocalGet(_) => true,
        Expr::Compare { left, right, .. }
        | Expr::Binary { left, right, .. }
        | Expr::Logical { left, right, .. } => pure_test(left) && pure_test(right),
        Expr::Unary { operand, .. } => pure_test(operand),
        _ => false,
    }
}

/// Lower an admitted selection as i32 (`selection_bits` admitted it).
pub(super) fn lower_selection(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<Option<String>> {
    match e {
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            let test = crate::lower_conditional::lower_test(ctx, condition)?;
            let then_b = ctx.new_block("i32sel.then");
            let else_b = ctx.new_block("i32sel.else");
            let merge = ctx.new_block("i32sel.merge");
            let then_l = ctx.block_label(then_b);
            let else_l = ctx.block_label(else_b);
            let merge_l = ctx.block_label(merge);
            ctx.block().cond_br(&test, &then_l, &else_l);
            ctx.current_block = then_b;
            let t = super::lower_expr_as_i32(ctx, then_expr)?;
            let t_end = ctx.block().label.clone();
            ctx.block().br(&merge_l);
            ctx.current_block = else_b;
            let f = super::lower_expr_as_i32(ctx, else_expr)?;
            let f_end = ctx.block().label.clone();
            ctx.block().br(&merge_l);
            ctx.current_block = merge;
            Ok(Some(ctx.block().phi(
                I32,
                &[(t.as_str(), t_end.as_str()), (f.as_str(), f_end.as_str())],
            )))
        }
        Expr::MathMin(args) | Expr::MathMax(args) if !args.is_empty() => {
            let f = if matches!(e, Expr::MathMin(_)) {
                "llvm.smin.i32"
            } else {
                "llvm.smax.i32"
            };
            // Every argument is evaluated, in order (ToNumber of each).
            let mut vals = Vec::with_capacity(args.len());
            for a in args {
                vals.push(super::lower_expr_as_i32(ctx, a)?);
            }
            let mut acc = vals[0].clone();
            for v in &vals[1..] {
                let blk = ctx.block();
                let r = blk.fresh_reg();
                blk.emit_raw(format!("{r} = call i32 @{f}(i32 {acc}, i32 {v})"));
                acc = r;
            }
            Ok(Some(acc))
        }
        Expr::MathAbs(x) => {
            let v = super::lower_expr_as_i32(ctx, x)?;
            let blk = ctx.block();
            let r = blk.fresh_reg();
            blk.emit_raw(format!("{r} = call i32 @llvm.abs.i32(i32 {v}, i1 false)"));
            Ok(Some(r))
        }
        _ => Ok(None),
    }
}

/// The i32 chain of `e` computes its JavaScript value exactly (module docs).
pub(crate) fn exact(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::Integer(n) => i32::try_from(*n).is_ok(),
        Expr::LocalGet(id)
            if matches!(ctx.local_slot_reps.get(id), Some(crate::expr::SlotRep::I32)) =>
        {
            true
        }
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } if enabled() => exact(ctx, then_expr) && exact(ctx, else_expr),
        Expr::MathMin(args) | Expr::MathMax(args) if enabled() && !args.is_empty() => {
            args.iter().all(|a| exact(ctx, a))
        }
        _ => super::is_known_i32_range(ctx, e),
    }
}

/// `left <op> right` as one `icmp` when both operands are exact int32
/// chains (module docs); `None` leaves the test to the double lowering.
pub(crate) fn try_lower_compare(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<Option<String>> {
    if !enabled() {
        return Ok(None);
    }
    let Expr::Compare { op, left, right } = e else {
        return Ok(None);
    };
    if !matches!(
        op,
        CompareOp::Lt
            | CompareOp::Le
            | CompareOp::Gt
            | CompareOp::Ge
            | CompareOp::Eq
            | CompareOp::Ne
    ) {
        return Ok(None);
    }
    // Two operands LLVM already compares as integers (`sitofp` of i32 slots
    // and literals) gain nothing; take only a test with a temporary.
    let plain = |e: &Expr| matches!(e, Expr::Integer(_) | Expr::LocalGet(_));
    if plain(left) && plain(right) {
        return Ok(None);
    }
    if !(super::can_lower_expr_as_i32_in_current_region(ctx, left)
        && super::can_lower_expr_as_i32_in_current_region(ctx, right)
        && exact(ctx, left)
        && exact(ctx, right))
    {
        return Ok(None);
    }
    let l = super::lower_expr_as_i32(ctx, left)?;
    let r = super::lower_expr_as_i32(ctx, right)?;
    let blk = ctx.block();
    Ok(Some(match op {
        CompareOp::Lt => blk.icmp_slt(I32, &l, &r),
        CompareOp::Le => blk.icmp_sle(I32, &l, &r),
        CompareOp::Gt => blk.icmp_sgt(I32, &l, &r),
        CompareOp::Ge => blk.icmp_sge(I32, &l, &r),
        CompareOp::Eq => blk.icmp_eq(I32, &l, &r),
        _ => blk.icmp_ne(I32, &l, &r),
    }))
}

#[cfg(test)]
#[path = "temps_tests.rs"]
mod tests;
