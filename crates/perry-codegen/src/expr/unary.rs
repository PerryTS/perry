//! Unary operators.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::Result;
use perry_hir::{Expr, UnaryOp};

use crate::lower_conditional::lower_expr_with_truthy;
use crate::native_value::LoweredValue;
use crate::type_analysis::{
    expr_may_return_boxed_value_from_raw_f64_fallback, is_bigint_expr, is_numeric_expr,
    is_provably_not_bigint,
};
use crate::types::{DOUBLE, I32, I64};

use super::{is_known_i32_range, lower_expr, FnCtx};

/// `~operand` as a native int32 value, or `None` when the operand has no
/// native int32 form (#10512).
///
/// Over a Number, `~x` IS `x ^ -1` (Number::bitwiseNOT is ToInt32 with every
/// bit flipped), so the operand takes exactly the path a binary bitwise
/// operand takes and the result stays an `i32` for whatever consumes it. The
/// ordinary lowering in [`lower`] instead left through `sitofp`, and every
/// enclosing `&`/`^`/`| 0` paid the ToInt32 tower again on the way back in:
/// a sha256 `Chi` round ran 10x slower than the same loop spelled `B ^ -1`.
///
/// `lower_bitwise_operand_i32` only produces a value for an operand it can
/// prove is a Number, and a BigInt-typed operand declines up front, so a
/// BigInt `~x` (a BigInt result) still reaches the dynamic helper in [`lower`].
/// `~v` for an operand the compiler cannot prove is a Number (#10511): one
/// inline test, the numeric arm inline, the dynamic helper cold.
///
/// This is the unary twin of the binary bitwise guard
/// (`binary::lower_guarded_numeric_arith`) and uses the same test,
/// `|v| < 2^63`. Every NaN-boxed tag is a NaN bit pattern, so a string,
/// object, boolean, `undefined`, an int32 box or a BigInt fails the ordered
/// compare and reaches `js_dynamic_bitnot`, which keeps ToNumeric's exact
/// semantics: a BigInt stays a BigInt (`~1n === -2n`), `valueOf` runs, and
/// a throwing coercion throws. The Numbers the test turns away — NaN,
/// ±Infinity, |v| >= 2^63 — are the ones whose ToInt32 needs the helper's
/// special cases; for every Number that passes, truncating to i64 and keeping
/// the low 32 bits IS ToInt32, so the numeric arm is one `fptosi`.
///
/// `v` is the already-lowered operand: nothing runs between its evaluation
/// and either arm, so no rooting window opens that the old unconditional
/// helper call did not already have.
fn lower_guarded_bitnot(ctx: &mut FnCtx<'_>, v: &str) -> String {
    let is_num = super::binary::emit_is_int64_exact_number(ctx, v);
    let fast_idx = ctx.new_block("guarded_bitnot.numeric");
    let slow_idx = ctx.new_block("guarded_bitnot.dynamic");
    let merge_idx = ctx.new_block("guarded_bitnot.merge");
    let fast_label = ctx.block_label(fast_idx);
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&is_num, &fast_label, &slow_label);

    ctx.current_block = fast_idx;
    let fast_val = {
        let blk = ctx.block();
        let i = blk.toint32_fast(v);
        let flipped = blk.xor(I32, &i, "-1");
        blk.sitofp(I32, &flipped, DOUBLE)
    };
    let fast_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = slow_idx;
    super::emit_versioned_loop_callback_deopt(ctx);
    let slow_val = ctx
        .block()
        .call(DOUBLE, "js_dynamic_bitnot", &[(DOUBLE, v)]);
    let slow_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    ctx.block()
        .phi(DOUBLE, &[(&fast_val, &fast_end), (&slow_val, &slow_end)])
}

pub(crate) fn lower_bitnot_value(
    ctx: &mut FnCtx<'_>,
    operand: &Expr,
) -> Result<Option<LoweredValue>> {
    if is_bigint_expr(ctx, operand) {
        return Ok(None);
    }
    let Some(value) = super::lower_bitwise_operand_i32(ctx, operand)? else {
        return Ok(None);
    };
    let lowered = LoweredValue::i32(ctx.block().xor(I32, &value, "-1"));
    ctx.record_lowered_value(
        "Unary",
        None,
        "ordinary_expr_value.bitnot_i32",
        &lowered,
        None,
        None,
        None,
        false,
        false,
        Vec::new(),
    );
    Ok(Some(lowered))
}

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
                            ctx.toint32_wrap(&v)
                        };
                        let blk = ctx.block();
                        let flipped = blk.xor(I32, &i, "-1");
                        Ok(blk.sitofp(I32, &flipped, DOUBLE))
                    } else if super::binary::guarded_bitwise_enabled() {
                        Ok(lower_guarded_bitnot(ctx, &v))
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
