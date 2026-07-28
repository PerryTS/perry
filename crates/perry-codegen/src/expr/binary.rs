//! Binary arithmetic / bitwise / string-concat dispatch.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::Result;
use perry_hir::{BinaryOp, Expr, LogicalOp};

use crate::lower_string_method::{
    flatten_string_add_chain, lower_string_coerce_concat, lower_string_concat,
    lower_string_concat_chain,
};
use crate::native_value::{
    materialize_small_bigint_pointer_to_js_value, BufferAccessMode, LoweredValue,
    MaterializationReason,
};
use crate::type_analysis::{
    add_operands_have_pod_materialization_hazard,
    expr_may_return_boxed_value_from_raw_f64_fallback, is_bigint_expr, is_bool_expr,
    is_numeric_expr,
};
use crate::types::{DOUBLE, I1, I128, I32, I64};

use super::{is_known_finite, lower_expr, FnCtx};

fn lower_arithmetic_operand(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<(String, bool)> {
    // Repsel Phase 4a.0 (#6904): a numeric-proven `a || b` / `a && b` /
    // `a ?? b` consumed as an arithmetic operand lowers with BOTH sides in
    // number context, so the selection is a real-double diamond (`fcmp one` +
    // phi — SimplifyCFG folds it to a `select`) instead of a boxed
    // `js_is_truthy` dispatch whose merged value then needs a site
    // `js_number_coerce`. This is the `(counts[v] || 0) + 1` histogram shape.
    //
    // Early coercion is semantics-preserving here because the consumer is an
    // arithmetic operand: every value the coerced test can misclassify
    // relative to JS truthiness under HONEST types is `undefined` (a raw-f64
    // read's hole fallback), and ToNumber(undefined) = NaN is falsy exactly
    // like `undefined`; the passed-through value is coerced by the consumer
    // regardless. `??` keeps its nullish test on the UNCOERCED left value —
    // a coerced hole (NaN) is indistinguishable from a stored NaN, but
    // `NaN ?? x` is NaN while `undefined ?? x` is `x`.
    if let Expr::Logical { op, left, right } = expr {
        if is_numeric_expr(ctx, expr) {
            let value = lower_numeric_logical_for_number_context(ctx, *op, left, right)?;
            return Ok((value, true));
        }
    }
    if expr_may_return_boxed_value_from_raw_f64_fallback(ctx, expr) {
        if let Some(value) =
            super::property_get::lower_raw_f64_class_field_get_for_number_context(ctx, expr)?
        {
            return Ok((value, true));
        }
        if let Some(value) =
            super::index_get::lower_numeric_index_get_for_number_context(ctx, expr)?
        {
            return Ok((value, true));
        }
    }
    // #5525: an untyped-receiver typed-array element read (`S[i]` with `S` an
    // `any` param — bcryptjs's Blowfish hot path) used as a non-`+` arithmetic
    // operand. Lower it as a guaranteed Number (coerce sunk into the cold slow
    // branch) so the hot per-element fast path skips the site `js_number_coerce`.
    // Only non-`+` ops reach here (`+` with an untyped operand returned via
    // `js_dynamic_string_or_number_add` above), and those always `ToNumber`
    // their operands, so early coercion is semantics-preserving.
    if let Some(value) =
        super::index_get::lower_unknown_local_index_get_for_number_context(ctx, expr)?
    {
        return Ok((value, true));
    }
    Ok((lower_expr(ctx, expr)?, false))
}

/// The shared residual-coercion rule for arithmetic operands: a lowered
/// operand still needs a `js_number_coerce` when the fallback did not already
/// coerce it AND it is either not statically numeric (booleans, `null`, …)
/// or can surface a boxed value through a raw-f64 read's cold fallback.
fn operand_needs_residual_coerce(ctx: &FnCtx<'_>, expr: &Expr, fallback_coerced: bool) -> bool {
    !fallback_coerced
        && (!is_numeric_expr(ctx, expr)
            || expr_may_return_boxed_value_from_raw_f64_fallback(ctx, expr))
}

/// Lower an operand in number context: route through
/// [`lower_arithmetic_operand`], then apply the shared residual-coercion rule
/// — the result is ALWAYS a real (canonical) numeric double, never a
/// NaN-boxed value.
fn lower_operand_as_number(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    let (raw, fallback_coerced) = lower_arithmetic_operand(ctx, expr)?;
    if operand_needs_residual_coerce(ctx, expr, fallback_coerced) {
        Ok(ctx
            .block()
            .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &raw)]))
    } else {
        Ok(raw)
    }
}

/// Repsel Phase 4a.0: number-context lowering of a numeric-proven logical
/// selection (see the caller comment in [`lower_arithmetic_operand`]).
///
/// `&&` / `||`: the left side is lowered in number context (a real double),
/// so its truthiness test is a bare `fcmp one l, 0.0` — falsy is exactly
/// {`+0`, `-0`, NaN}, and the values that JS-truthiness could disagree on
/// (boxed `undefined` from a hole fallback) have already been coerced to NaN
/// (falsy — identical verdict to `undefined`). Both phi inputs are real
/// doubles, so the merged value feeds `fadd`/`fmul`/… with no further
/// dispatch.
///
/// `??`: the nullish test runs on the UNCOERCED left value (`bits ==
/// TAG_NULL | TAG_UNDEFINED`); the pass-through edge then coerces (only when
/// the operand carries the boxed-fallback hazard), keeping `NaN ?? x` = NaN
/// vs `undefined ?? x` = `x` byte-exact.
fn lower_numeric_logical_for_number_context(
    ctx: &mut FnCtx<'_>,
    op: LogicalOp,
    left: &Expr,
    right: &Expr,
) -> Result<String> {
    if matches!(op, LogicalOp::Coalesce) {
        let l_boxed = lower_expr(ctx, left)?;
        let is_nullish = {
            let blk = ctx.block();
            let l_bits = blk.bitcast_double_to_i64(&l_boxed);
            let is_null = blk.icmp_eq(I64, &l_bits, crate::nanbox::TAG_NULL_I64);
            let is_undef = blk.icmp_eq(I64, &l_bits, crate::nanbox::TAG_UNDEFINED_I64);
            blk.or(I1, &is_null, &is_undef)
        };
        let right_idx = ctx.new_block("numlog.coalesce.right");
        let keep_idx = ctx.new_block("numlog.coalesce.keep");
        let merge_idx = ctx.new_block("numlog.coalesce.merge");
        let right_label = ctx.block_label(right_idx);
        let keep_label = ctx.block_label(keep_idx);
        let merge_label = ctx.block_label(merge_idx);
        ctx.block().cond_br(&is_nullish, &right_label, &keep_label);

        ctx.current_block = right_idx;
        let r = lower_operand_as_number(ctx, right)?;
        let r_end = ctx.block().label.clone();
        ctx.block().br(&merge_label);

        ctx.current_block = keep_idx;
        // Non-nullish left: coerce only when the operand can surface a boxed
        // value (e.g. an INT32-boxed number from a read fallback). A plain
        // proven double passes through untouched.
        let l_num = if expr_may_return_boxed_value_from_raw_f64_fallback(ctx, left) {
            ctx.block()
                .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &l_boxed)])
        } else {
            l_boxed
        };
        let keep_end = ctx.block().label.clone();
        ctx.block().br(&merge_label);

        ctx.current_block = merge_idx;
        return Ok(ctx
            .block()
            .phi(DOUBLE, &[(&r, &r_end), (&l_num, &keep_end)]));
    }

    let l = lower_operand_as_number(ctx, left)?;
    let l_bool = ctx.block().fcmp("one", &l, "0.0");
    let l_end = ctx.block().label.clone();

    let then_idx = ctx.new_block("numlog.then");
    let merge_idx = ctx.new_block("numlog.merge");
    let then_label = ctx.block_label(then_idx);
    let merge_label = ctx.block_label(merge_idx);
    match op {
        // a && b: truthy left evaluates the right side; falsy left is the
        // result.
        LogicalOp::And => ctx.block().cond_br(&l_bool, &then_label, &merge_label),
        // a || b: truthy left is the result; falsy left evaluates the right.
        LogicalOp::Or => ctx.block().cond_br(&l_bool, &merge_label, &then_label),
        LogicalOp::Coalesce => unreachable!("handled above"),
    }

    ctx.current_block = then_idx;
    let r = lower_operand_as_number(ctx, right)?;
    let r_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    Ok(ctx.block().phi(DOUBLE, &[(&l, &l_end), (&r, &r_end)]))
}

fn small_bigint_literal_value(expr: &Expr) -> Option<i64> {
    let Expr::BigInt(raw) = expr else {
        return None;
    };
    let normalized = raw.replace('_', "");
    let s = normalized.strip_suffix('n').unwrap_or(&normalized);
    let (negative, digits) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    if digits.is_empty() {
        return None;
    }
    let (radix, digits) = if let Some(rest) = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        (16, rest)
    } else if let Some(rest) = digits
        .strip_prefix("0o")
        .or_else(|| digits.strip_prefix("0O"))
    {
        (8, rest)
    } else if let Some(rest) = digits
        .strip_prefix("0b")
        .or_else(|| digits.strip_prefix("0B"))
    {
        (2, rest)
    } else {
        (10, digits)
    };
    if digits.is_empty() {
        return None;
    }
    let magnitude = i128::from_str_radix(digits, radix).ok()?;
    let value = if negative { -magnitude } else { magnitude };
    i64::try_from(value).ok()
}

fn small_bigint_native_op(op: BinaryOp) -> Option<(&'static str, &'static str)> {
    match op {
        BinaryOp::Add => Some(("add", "js_dynamic_add")),
        BinaryOp::Sub => Some(("sub", "js_dynamic_sub")),
        BinaryOp::Mul => Some(("mul", "js_dynamic_mul")),
        _ => None,
    }
}

/// Six bitwise/shift ops whose result is always `ToInt32`/`ToUint32`-wrapped
/// (a plain JS Number). These are the ops the non-BigInt inline fast path
/// covers; the arithmetic ops in the same dynamic-helper bail (`Mul`/`Div`/
/// `Mod`/`Sub`/`Pow`) are deliberately excluded.
fn is_bitwise_op(op: BinaryOp) -> bool {
    matches!(
        op,
        BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor
            | BinaryOp::Shl
            | BinaryOp::Shr
            | BinaryOp::UShr
    )
}

/// `PERRY_INLINE_NONBIGINT_BITWISE` fast-path gate. Enabled by default;
/// `=0`/`off`/`false` reverts to the BigInt-aware `js_dynamic_bit*` runtime
/// call for every non-statically-numeric bitwise operand (pre-fix behavior).
/// Kept as an env flag for A/B bisection, consistent with the sibling codegen
/// fast paths (the object cache keys this var so a warm cache can't serve an
/// object built under the other setting).
fn inline_nonbigint_bitwise_enabled() -> bool {
    !matches!(
        std::env::var("PERRY_INLINE_NONBIGINT_BITWISE").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

fn bigint_dynamic_helper(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "js_dynamic_add",
        BinaryOp::Sub => "js_dynamic_sub",
        BinaryOp::Mul => "js_dynamic_mul",
        BinaryOp::Div => "js_dynamic_div",
        BinaryOp::Mod => "js_dynamic_mod",
        BinaryOp::BitAnd => "js_dynamic_bitand",
        BinaryOp::BitOr => "js_dynamic_bitor",
        BinaryOp::BitXor => "js_dynamic_bitxor",
        BinaryOp::Shl => "js_dynamic_shl",
        BinaryOp::Shr => "js_dynamic_shr",
        BinaryOp::Pow => "js_dynamic_pow",
        BinaryOp::UShr => "js_dynamic_ushr",
    }
}

fn record_small_bigint_rejection(
    ctx: &mut FnCtx<'_>,
    reason: &'static str,
    fallback_helper: &'static str,
) {
    let lowered = LoweredValue::js_value("0.0");
    ctx.record_lowered_value_with_access_mode(
        "BigIntSmallBinaryRejected",
        None,
        "small_bigint.literal_binary_rejected",
        &lowered,
        None,
        None,
        Some(BufferAccessMode::DynamicFallback),
        Some(MaterializationReason::RuntimeApi),
        false,
        false,
        vec![
            format!("small_bigint_rejected={reason}"),
            format!("fallback={fallback_helper}"),
            "boxed_at=generic_bigint_dynamic_helper".to_string(),
        ],
    );
}

fn try_lower_small_bigint_literal_binary(
    ctx: &mut FnCtx<'_>,
    op: BinaryOp,
    left: &Expr,
    right: &Expr,
) -> Option<String> {
    let (native_op, fallback_helper) = small_bigint_native_op(op)?;
    let Some(left_i64) = small_bigint_literal_value(left) else {
        record_small_bigint_rejection(ctx, "requires_left_i64_literal", fallback_helper);
        return None;
    };
    let Some(right_i64) = small_bigint_literal_value(right) else {
        record_small_bigint_rejection(ctx, "requires_right_i64_literal", fallback_helper);
        return None;
    };

    let left_const = left_i64.to_string();
    let right_const = right_i64.to_string();
    let result_i128 = {
        let blk = ctx.block();
        let left_wide = blk.sext(I64, &left_const, I128);
        let right_wide = blk.sext(I64, &right_const, I128);
        match op {
            BinaryOp::Add => blk.add(I128, &left_wide, &right_wide),
            BinaryOp::Sub => blk.sub(I128, &left_wide, &right_wide),
            BinaryOp::Mul => blk.mul(I128, &left_wide, &right_wide),
            _ => return None,
        }
    };
    let lowered = LoweredValue::small_bigint(result_i128.clone());
    ctx.record_lowered_value(
        "BigIntSmallBinary",
        None,
        "small_bigint.literal_binary_i128",
        &lowered,
        None,
        None,
        None,
        false,
        false,
        vec![
            "proof=both_operands_bigint_literals_fit_i64".to_string(),
            format!("native_op=i128_{native_op}"),
            "public_semantics=materialize_bigint_object_before_js_boundary".to_string(),
        ],
    );
    let ptr = {
        let blk = ctx.block();
        let lo = blk.trunc(I128, &result_i128, I64);
        let hi_wide = blk.ashr(I128, &result_i128, "64");
        let hi = blk.trunc(I128, &hi_wide, I64);
        blk.call(I64, "js_bigint_from_i128_parts", &[(I64, &lo), (I64, &hi)])
    };
    Some(materialize_small_bigint_pointer_to_js_value(
        ctx,
        &ptr,
        MaterializationReason::RuntimeApi,
    ))
}

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::Binary { op, left, right } => {
            if matches!(op, BinaryOp::Add) {
                // Use the stricter `is_definitely_string_expr` check for
                // the string-concat fast path. A union type `string|number`
                // that happens to contain a number at runtime would get
                // misrouted through lower_string_coerce_concat, which
                // treats the operand as a string pointer (bitcast + mask)
                // and reads garbage. The numeric Add path below handles
                // narrowed-number unions correctly via js_number_coerce.
                let l_is_str = crate::type_analysis::is_definitely_string_expr(ctx, left);
                let r_is_str = crate::type_analysis::is_definitely_string_expr(ctx, right);

                // N-way string concat fold (v0.5.771): when this is a
                // chain of `a + b + c + ...` where every Add node has at
                // least one statically-string operand, flatten the entire
                // left-spine and emit a single `js_string_concat_chain`
                // call. Saves N-1 intermediate StringHeader allocations
                // per row in mixed-type CSV / log-line / template
                // patterns. Only fires for chains of 3+ parts; smaller
                // shapes go through the existing pairwise paths.
                if l_is_str && r_is_str {
                    if let Some(parts) = flatten_string_add_chain(ctx, left, right) {
                        if parts.len() >= 3 {
                            return lower_string_concat_chain(ctx, &parts);
                        }
                    }
                }

                if l_is_str && r_is_str {
                    return lower_string_concat(ctx, left, right);
                }
                if l_is_str || r_is_str {
                    let other_known_primitive = if l_is_str {
                        crate::type_analysis::is_numeric_expr(ctx, right)
                            || is_bigint_expr(ctx, right)
                            || is_bool_expr(ctx, right)
                    } else {
                        crate::type_analysis::is_numeric_expr(ctx, left)
                            || is_bigint_expr(ctx, left)
                            || is_bool_expr(ctx, left)
                    };
                    if other_known_primitive {
                        return lower_string_coerce_concat(ctx, left, right, l_is_str, r_is_str);
                    }
                    let l = lower_expr(ctx, left)?;
                    let r = lower_expr(ctx, right)?;
                    return Ok(ctx.block().call(
                        DOUBLE,
                        "js_dynamic_string_or_number_add",
                        &[(DOUBLE, &l), (DOUBLE, &r)],
                    ));
                }
                if is_bigint_expr(ctx, left) && is_bigint_expr(ctx, right) {
                    if let Some(value) = try_lower_small_bigint_literal_binary(
                        ctx,
                        *op,
                        left.as_ref(),
                        right.as_ref(),
                    ) {
                        return Ok(value);
                    }
                    let l = lower_expr(ctx, left)?;
                    let r = lower_expr(ctx, right)?;
                    return Ok(ctx.block().call(
                        DOUBLE,
                        "js_dynamic_add",
                        &[(DOUBLE, &l), (DOUBLE, &r)],
                    ));
                }
                // Refs #486: neither operand is statically known. Per JS
                // spec for `+`, if EITHER side is a string at runtime, the
                // result is string concatenation; otherwise numeric add
                // (or BigInt add when bigint is involved). Pre-fix, the
                // numeric-fallback path below called js_number_coerce on
                // both sides — turning `"c" + ""` into `NaN + 0 = NaN` for
                // any string operand whose type wasn't statically inferred.
                // Hono's `Node.buildRegExpStr` does `k + c.buildRegExpStr()`
                // inside a for-of loop over `Object.keys(...)` results;
                // both operands lower as plain f64s with type Any, the
                // string-concat fast path didn't fire, and every recursive
                // step poisoned the result. Dispatch through the runtime
                // helper that checks NaN-box tags: STRING_TAG / SHORT_STRING_TAG
                // → string concat, BIGINT → bigint add, otherwise numeric.
                if !(crate::type_analysis::is_numeric_expr(ctx, left)
                    && crate::type_analysis::is_numeric_expr(ctx, right))
                    || add_operands_have_pod_materialization_hazard(ctx, left, right)
                {
                    let l = lower_expr(ctx, left)?;
                    let r = lower_expr(ctx, right)?;
                    return Ok(ctx.block().call(
                        DOUBLE,
                        "js_dynamic_string_or_number_add",
                        &[(DOUBLE, &l), (DOUBLE, &r)],
                    ));
                }
            }
            // BigInt arithmetic fast path. NaN-tagged bigints compare
            // unordered under `fadd`/`fsub`/`fmul`/`fdiv`/`frem` (the
            // tag bits make the f64 a NaN), so the default numeric path
            // returns `NaN` for `5n + 3n` and friends. When either side
            // is statically bigint-typed we dispatch to the runtime's
            // dynamic helpers — they unbox, call `js_bigint_<op>`, and
            // re-box with BIGINT_TAG. These helpers also tolerate
            // mixed bigint/int32 operands (they upcast to bigint), so
            // `n * 10n` where `n` is a bigint loop accumulator works
            // even when the numeric literal side isn't a bigint. Add is
            // in here too — `bigint + bigint` is arithmetic, not string
            // concat (the `is_definitely_string_expr` check above
            // already ruled out the string case). Closes GH #33.
            if is_bigint_expr(ctx, left) || is_bigint_expr(ctx, right) {
                let fname = bigint_dynamic_helper(*op);
                if let Some(value) =
                    try_lower_small_bigint_literal_binary(ctx, *op, left.as_ref(), right.as_ref())
                {
                    return Ok(value);
                }
                let l = lower_expr(ctx, left)?;
                let r = lower_expr(ctx, right)?;
                return Ok(ctx
                    .block()
                    .call(DOUBLE, fname, &[(DOUBLE, &l), (DOUBLE, &r)]));
            }
            // A non-primitive operand may `ToNumeric` to a BigInt at runtime
            // (`Object(1n)`, or an object with a BigInt-returning
            // `Symbol.toPrimitive`/`valueOf`). The numeric fast path below
            // `js_number_coerce`s both sides — collapsing a boxed BigInt to a
            // Number and silently producing a Number result instead of the
            // spec-mandated TypeError (mixed) or BigInt (both-bigint). Route
            // such operands through the dynamic helper, which runs full
            // `ToNumeric` (test262 `bigint-and-number` / `bigint-non-primitive`
            // for the object cases). Only the arithmetic/bitwise ops with a
            // dynamic helper are affected; the common all-numeric shapes (both
            // operands statically numeric/bool) keep the fast path untouched.
            if matches!(
                op,
                BinaryOp::BitAnd
                    | BinaryOp::BitOr
                    | BinaryOp::BitXor
                    | BinaryOp::Shl
                    | BinaryOp::Shr
                    | BinaryOp::UShr
                    | BinaryOp::Mul
                    | BinaryOp::Div
                    | BinaryOp::Mod
                    | BinaryOp::Sub
                    | BinaryOp::Pow
            ) {
                let l_prim =
                    crate::type_analysis::is_numeric_expr(ctx, left) || is_bool_expr(ctx, left);
                let r_prim =
                    crate::type_analysis::is_numeric_expr(ctx, right) || is_bool_expr(ctx, right);
                if !(l_prim && r_prim) {
                    // Non-BigInt inline fast path (the bcryptjs `_encipher`
                    // Feistel lever): for the six BITWISE ops, when BOTH
                    // operands are provably-not-BigInt we skip the dynamic
                    // helper and fall through to the inline `ToInt32 <op>
                    // ToInt32 + sitofp` lowering below. That path already
                    // picks the NaN-safe guarded `toint32_wrap` for any
                    // operand not proven finite (e.g. an OOB typed-array read
                    // → `undefined`/NaN), and `js_number_coerce`s non-numeric
                    // operands, so semantics are preserved. We keep the
                    // dynamic-helper bail whenever an operand *could* be a
                    // BigInt (so `bigint <op> number` still throws and
                    // `bigint <op> bigint` still computes a BigInt), and for
                    // the arithmetic ops (`Mul`/`Div`/`Mod`/`Sub`/`Pow`),
                    // which are out of scope for this fast path.
                    let inline_bitwise = is_bitwise_op(*op)
                        && inline_nonbigint_bitwise_enabled()
                        && crate::type_analysis::is_provably_not_bigint(ctx, left)
                        && crate::type_analysis::is_provably_not_bigint(ctx, right);
                    if !inline_bitwise {
                        let fname = bigint_dynamic_helper(*op);
                        let l = lower_expr(ctx, left)?;
                        let r = lower_expr(ctx, right)?;
                        return Ok(ctx
                            .block()
                            .call(DOUBLE, fname, &[(DOUBLE, &l), (DOUBLE, &r)]));
                    }
                }
            }
            // Fast path: `<integer-valued> % <integer literal>` (the
            // factorial / `i % 1000` loop shape). `frem double` lowers
            // to a libm `fmod()` call on ARM — no hardware instruction
            // — at ~15ns per iteration. Emitting `fptosi → srem →
            // sitofp` lets LLVM's SCEV hoist the float↔int conversions
            // out of the loop and replace the div with a reciprocal-
            // multiplication trick. On the factorial benchmark this
            // takes the inner loop from 1550ms → ~150ms.
            //
            // Safety: both operands must be provably integer-valued.
            // A fractional LHS would lose its fraction bits through
            // fptosi, producing the wrong result. `is_integer_valued_expr`
            // only returns true when we can prove the value is a whole
            // number (integer literals, integer loop counters, or nested
            // integer arithmetic). A zero RHS falls through to `frem`
            // because srem(x,0) is UB in LLVM (on ARM the CPU silently
            // gives 0, but JS requires NaN for any x % 0). For everything
            // else we fall through to the `frem` path.
            let right_is_known_zero = matches!(**right, Expr::Integer(0))
                || matches!(**right, Expr::Number(v) if v == 0.0);
            if matches!(op, BinaryOp::Mod)
                && crate::type_analysis::is_integer_valued_expr(ctx, left)
                && crate::type_analysis::is_integer_valued_expr(ctx, right)
                && !right_is_known_zero
            {
                let l_raw = lower_expr(ctx, left)?;
                let r_raw = lower_expr(ctx, right)?;
                let blk = ctx.block();
                let li = blk.fptosi(DOUBLE, &l_raw, I64);
                let ri = blk.fptosi(DOUBLE, &r_raw, I64);
                let m = blk.srem(I64, &li, &ri);
                // IEEE 754: when the integer remainder is 0 and the
                // dividend was negative, the result must be -0.0.
                // srem gives 0i64 → sitofp always produces +0.0,
                // so correct: if m==0 && l<0 → fneg(0.0) = -0.0.
                let result_f = blk.sitofp(I64, &m, DOUBLE);
                let m_is_zero = blk.icmp_eq(I64, &m, "0");
                let l_neg = blk.fcmp("olt", &l_raw, "0.0");
                let need_neg = blk.and(I1, &m_is_zero, &l_neg);
                let neg_result = blk.fneg(&result_f);
                return Ok(blk.select(I1, &need_neg, DOUBLE, &neg_result, &result_f));
            }

            let (l_raw, l_fallback_coerced) = lower_arithmetic_operand(ctx, left)?;
            let (r_raw, r_fallback_coerced) = lower_arithmetic_operand(ctx, right)?;
            // Coerce non-numeric operands to numbers for arithmetic.
            // JS: `true + true = 2`, `null + 1 = 1`, etc. Without
            // this, fadd on NaN-tagged booleans propagates the NaN
            // payload instead of computing 1.0 + 1.0 = 2.0.
            let l_needs_coerce = operand_needs_residual_coerce(ctx, left, l_fallback_coerced);
            let r_needs_coerce = operand_needs_residual_coerce(ctx, right, r_fallback_coerced);
            let l = if l_needs_coerce {
                ctx.block()
                    .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &l_raw)])
            } else {
                l_raw
            };
            let r = if r_needs_coerce {
                ctx.block()
                    .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &r_raw)])
            } else {
                r_raw
            };
            let v = match op {
                BinaryOp::Add => {
                    let blk = ctx.block();
                    blk.fadd(&l, &r)
                }
                BinaryOp::Sub => {
                    let blk = ctx.block();
                    blk.fsub(&l, &r)
                }
                BinaryOp::Mul => {
                    let blk = ctx.block();
                    blk.fmul(&l, &r)
                }
                BinaryOp::Div => {
                    let blk = ctx.block();
                    blk.fdiv(&l, &r)
                }
                BinaryOp::Mod => {
                    let blk = ctx.block();
                    blk.frem(&l, &r)
                }
                BinaryOp::Pow => {
                    ctx.block()
                        .call(DOUBLE, "js_math_pow", &[(DOUBLE, &l), (DOUBLE, &r)])
                }
                // Bitwise ops: use toint32_fast (skip NaN/Inf guard) when
                // operands are known-finite from integer analysis.
                //
                // `x | 0` and `x >>> 0` where x is known-finite: the op
                // is just a ToInt32/ToUint32 coercion. When x comes from
                // the integer path (already finite), skip the toint32
                // entirely — just fptosi + sitofp (identity for in-range
                // values, LLVM eliminates via instcombine).
                BinaryOp::BitOr
                    if matches!(right.as_ref(), Expr::Integer(0)) && is_known_finite(ctx, left) =>
                {
                    let blk = ctx.block();
                    let li = blk.toint32_fast(&l);
                    blk.sitofp(I32, &li, DOUBLE)
                }
                BinaryOp::BitAnd
                | BinaryOp::BitOr
                | BinaryOp::BitXor
                | BinaryOp::Shl
                | BinaryOp::Shr => {
                    let l_safe = is_known_finite(ctx, left);
                    let r_safe = is_known_finite(ctx, right);
                    let blk = ctx.block();
                    let li = if l_safe {
                        blk.toint32_fast(&l)
                    } else {
                        blk.toint32_wrap(&l)
                    };
                    let ri = if r_safe {
                        blk.toint32_fast(&r)
                    } else {
                        blk.toint32_wrap(&r)
                    };
                    let v = match op {
                        BinaryOp::BitAnd => blk.and(I32, &li, &ri),
                        BinaryOp::BitOr => blk.or(I32, &li, &ri),
                        BinaryOp::BitXor => blk.xor(I32, &li, &ri),
                        BinaryOp::Shl => blk.shl(I32, &li, &ri),
                        BinaryOp::Shr => blk.ashr(I32, &li, &ri),
                        _ => unreachable!(),
                    };
                    blk.sitofp(I32, &v, DOUBLE)
                }
                BinaryOp::UShr
                    if matches!(right.as_ref(), Expr::Integer(0)) && is_known_finite(ctx, left) =>
                {
                    let blk = ctx.block();
                    let li = blk.toint32_fast(&l);
                    blk.uitofp(I32, &li, DOUBLE)
                }
                BinaryOp::UShr => {
                    let l_safe = is_known_finite(ctx, left);
                    let r_safe = is_known_finite(ctx, right);
                    let blk = ctx.block();
                    let li = if l_safe {
                        blk.toint32_fast(&l)
                    } else {
                        blk.toint32_wrap(&l)
                    };
                    let ri = if r_safe {
                        blk.toint32_fast(&r)
                    } else {
                        blk.toint32_wrap(&r)
                    };
                    let v = blk.lshr(I32, &li, &ri);
                    blk.uitofp(I32, &v, DOUBLE)
                }
            };
            Ok(v)
        }

        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
