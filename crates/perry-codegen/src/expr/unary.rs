//! Unary operators.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::Result;
use perry_hir::{Expr, UnaryOp};

use crate::lower_conditional::lower_truthy;
use crate::type_analysis::{
    expr_may_return_boxed_value_from_raw_f64_fallback, is_bigint_expr, is_numeric_expr,
};
use crate::types::{DOUBLE, I64};

use super::{lower_expr, FnCtx};

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::Unary { op, operand } => {
            let numeric = is_numeric_expr(ctx, operand)
                && !expr_may_return_boxed_value_from_raw_f64_fallback(ctx, operand);
            // `-<bigint>` must stay a BigInt (`typeof -1n === "bigint"`).
            // `fneg` on a NaN-boxed BigInt flips the NaN payload's sign bit
            // and produces a garbage number, so route negation through the
            // runtime dynamic helper when the operand is statically bigint.
            let is_big = matches!(op, UnaryOp::Neg) && is_bigint_expr(ctx, operand);
            let v = lower_expr(ctx, operand)?;
            let blk = ctx.block();
            match op {
                UnaryOp::Neg => {
                    if is_big {
                        Ok(blk.call(DOUBLE, "js_dynamic_neg", &[(DOUBLE, &v)]))
                    } else if numeric {
                        Ok(blk.fneg(&v))
                    } else {
                        let coerced = blk.call(DOUBLE, "js_number_coerce", &[(DOUBLE, &v)]);
                        Ok(blk.fneg(&coerced))
                    }
                }
                UnaryOp::Pos => {
                    if numeric {
                        Ok(v)
                    } else {
                        Ok(blk.call(DOUBLE, "js_number_coerce", &[(DOUBLE, &v)]))
                    }
                }
                UnaryOp::Not => {
                    // !x: truthiness inverted, then NaN-box as a JS
                    // boolean (TAG_TRUE / TAG_FALSE) so console.log
                    // prints "true" / "false" instead of 1 / 0.
                    let bit = lower_truthy(ctx, &v, operand);
                    let blk = ctx.block();
                    let inv = blk.xor(crate::types::I1, &bit, "true");
                    let tagged_i64 = blk.select(
                        crate::types::I1,
                        &inv,
                        I64,
                        crate::nanbox::TAG_TRUE_I64,
                        crate::nanbox::TAG_FALSE_I64,
                    );
                    Ok(blk.bitcast_i64_to_double(&tagged_i64))
                }
                UnaryOp::BitNot => {
                    // `~x` preserves BigInt when the runtime value is a BigInt
                    // and otherwise falls back to JS ToInt32 semantics.
                    Ok(blk.call(DOUBLE, "js_dynamic_bitnot", &[(DOUBLE, &v)]))
                }
            }
        }

        // -------- Comparison --------
        // LLVM `fcmp` returns `i1`. We zext to double so the value fits the
        // standard number ABI used by the rest of the codegen — JS "true"
        // round-trips through numeric contexts as 1.0 and "false" as 0.0,
        // which is what Perry's runtime expects from typed boolean returns.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
