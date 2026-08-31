//! Unary operators.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::Result;
use perry_hir::{Expr, UnaryOp};

use crate::lower_conditional::lower_expr_with_truthy;
use crate::type_analysis::{
    expr_may_return_boxed_value_from_raw_f64_fallback, is_numeric_expr, is_provably_not_bigint,
};
use crate::types::{DOUBLE, I32, I64};

use super::{is_known_i32_range, lower_expr, FnCtx};

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::Unary { op, operand } => {
            let statically_numeric = is_numeric_expr(ctx, operand);
            let numeric = statically_numeric
                && !expr_may_return_boxed_value_from_raw_f64_fallback(ctx, operand);
            let native_bitnot =
                matches!(op, UnaryOp::BitNot) && numeric && is_provably_not_bigint(ctx, operand);
            let bitnot_known_i32 = native_bitnot && is_known_i32_range(ctx, operand);
            // Unary minus performs ToNumeric, not ToNumber: a possible BigInt
            // operand must use the dynamic helper so `-1n` remains a BigInt.
            // A statically numeric expression is already proven to yield a
            // Number (even if a boxed cold fallback still needs coercion), and
            // a separately proven non-BigInt value can use `js_number_coerce`.
            // Everything else may be a BigInt at runtime — notably an indexed
            // read from a plain array — and routing it through ToNumber would
            // silently round large BigInts before `fneg` (#9142).
            let _dynamic_neg = matches!(op, UnaryOp::Neg)
                && !statically_numeric
                && !is_provably_not_bigint(ctx, operand);
            let (v, precomputed_truthy) = if matches!(op, UnaryOp::Not) {
                let (boxed, truthy) = lower_expr_with_truthy(ctx, operand)?;
                (boxed, Some(truthy))
            } else {
                (lower_expr(ctx, operand)?, None)
            };
            let blk = ctx.block();
            match op {
                UnaryOp::Neg => {
                    if numeric {
                        Ok(blk.fneg(&v))
                    } else {
                        // Mirrors `Pos` below: anything not statically numeric
                        // goes through the dynamic helper, which performs a real
                        // ToNumeric. The old `js_number_coerce` + `fneg` fallback
                        // answered NaN when the operand's `valueOf` THREW, so
                        // `-{ valueOf() { throw … } }` silently produced NaN
                        // where every other operator (`+x`, `~x`, `x * 2`)
                        // propagated. `js_dynamic_neg` also keeps a BigInt a
                        // BigInt, which is why `dynamic_neg` folded in here.
                        Ok(blk.call(DOUBLE, "js_dynamic_neg", &[(DOUBLE, &v)]))
                    }
                }
                UnaryOp::Pos => {
                    if numeric {
                        Ok(v)
                    } else {
                        Ok(blk.call(DOUBLE, "js_dynamic_pos", &[(DOUBLE, &v)]))
                    }
                }
                UnaryOp::Not => {
                    // !x: truthiness inverted, then NaN-box as a JS
                    // boolean (TAG_TRUE / TAG_FALSE) so console.log
                    // prints "true" / "false" instead of 1 / 0.
                    let bit =
                        precomputed_truthy.expect("UnaryOp::Not precomputes operand truthiness");
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
                    // A proven Number result can perform ToInt32 and `~`
                    // directly. This notably covers coercive arithmetic such
                    // as `~~(erased / 32)`: the division either throws for a
                    // mixed BigInt or returns a Number, so both bitwise-NOTs
                    // are native. An erased direct operand and a potentially
                    // BigInt-producing chain (`~(a & b)`) retain the dynamic
                    // helper, which is what preserves BigInt semantics.
                    if native_bitnot {
                        let i = if bitnot_known_i32 {
                            blk.toint32_fast(&v)
                        } else {
                            blk.toint32_wrap(&v)
                        };
                        let flipped = blk.xor(I32, &i, "-1");
                        Ok(blk.sitofp(I32, &flipped, DOUBLE))
                    } else {
                        Ok(blk.call(DOUBLE, "js_dynamic_bitnot", &[(DOUBLE, &v)]))
                    }
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
