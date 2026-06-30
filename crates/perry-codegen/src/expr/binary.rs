//! Binary arithmetic / bitwise / string-concat dispatch.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::Result;
#[allow(unused_imports)]
use perry_hir::{BinaryOp, CompareOp, Expr, UnaryOp, UpdateOp};
#[allow(unused_imports)]
use perry_types::Type as HirType;

#[allow(unused_imports)]
use crate::lower_call::{lower_call, lower_native_method_call, lower_new};
#[allow(unused_imports)]
use crate::lower_conditional::{lower_conditional, lower_logical, lower_truthy};
#[allow(unused_imports)]
use crate::lower_string_method::{
    flatten_string_add_chain, lower_string_coerce_concat, lower_string_concat,
    lower_string_concat_chain, lower_string_self_append,
};
#[allow(unused_imports)]
use crate::nanbox::{double_literal, POINTER_MASK_I64};
#[allow(unused_imports)]
use crate::type_analysis::{
    add_operands_have_pod_materialization_hazard, compute_auto_captures,
    expr_may_return_boxed_value_from_raw_f64_fallback, is_array_expr, is_bigint_expr, is_bool_expr,
    is_map_expr, is_numeric_expr, is_set_expr, is_string_expr, is_url_search_params_expr,
    receiver_class_name,
};
#[allow(unused_imports)]
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

#[allow(unused_imports)]
use super::{
    buffer_alias_metadata_suffix, can_lower_expr_as_i32, emit_layout_note_slot_on_block,
    emit_shadow_slot_clear, emit_shadow_slot_update_for_expr, emit_string_literal_global,
    emit_v8_export_call, emit_v8_member_method_call, emit_write_barrier,
    emit_write_barrier_slot_on_block, expr_is_known_non_pointer_shadow_value,
    extract_array_of_object_shape, i32_bool_to_nanbox, import_origin_suffix,
    is_global_this_builtin_function_name, is_global_this_builtin_name, is_known_finite,
    lower_array_literal, lower_channel_reduction, lower_expr, lower_expr_as_i32,
    lower_index_set_fast, lower_js_args_array, lower_object_literal, lower_stream_super_init,
    lower_url_string_getter, nanbox_bigint_inline, nanbox_pointer_inline,
    nanbox_pointer_inline_pub, nanbox_string_inline, proxy_build_args_array, try_flat_const_2d_int,
    try_lower_flat_const_index_get, try_match_channel_reduction, try_static_class_name,
    unbox_str_handle, unbox_to_i64, variant_name, ChannelReduction, FlatConstInfo, FnCtx,
    I18nLowerCtx,
};

fn lower_arithmetic_operand(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<(String, bool)> {
    if expr_may_return_boxed_value_from_raw_f64_fallback(ctx, expr) {
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
                let helper = match op {
                    BinaryOp::Add => Some("js_dynamic_add"),
                    BinaryOp::Sub => Some("js_dynamic_sub"),
                    BinaryOp::Mul => Some("js_dynamic_mul"),
                    BinaryOp::Div => Some("js_dynamic_div"),
                    BinaryOp::Mod => Some("js_dynamic_mod"),
                    // Bitwise ops on bigints dispatch to the same
                    // unbox→bigint-op→rebox helpers used for arithmetic.
                    // Without this, `5n ^ 1n` fell through to the i32
                    // ToInt32 path that interprets the NaN-boxed bigint
                    // bits as a double — `fptosi` on a NaN-payload f64
                    // yielded a small signed integer (e.g. -6 for XOR of
                    // two 64-bit bigints) and masking with
                    // 0xFFFFFFFFFFFFFFFFn collapsed to 0 (closes #39).
                    BinaryOp::BitAnd => Some("js_dynamic_bitand"),
                    BinaryOp::BitOr => Some("js_dynamic_bitor"),
                    BinaryOp::BitXor => Some("js_dynamic_bitxor"),
                    BinaryOp::Shl => Some("js_dynamic_shl"),
                    BinaryOp::Shr => Some("js_dynamic_shr"),
                    // `bigint ** bigint` is a BigInt operation (RangeError on
                    // negative exponent); `>>>` on any BigInt is a TypeError.
                    // Both are routed through the dynamic helpers so the
                    // numeric fallback only fires when neither side is a
                    // BigInt at runtime (#2908).
                    BinaryOp::Pow => Some("js_dynamic_pow"),
                    BinaryOp::UShr => Some("js_dynamic_ushr"),
                    _ => None,
                };
                if let Some(fname) = helper {
                    let l = lower_expr(ctx, left)?;
                    let r = lower_expr(ctx, right)?;
                    return Ok(ctx
                        .block()
                        .call(DOUBLE, fname, &[(DOUBLE, &l), (DOUBLE, &r)]));
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
            let l_numeric = is_numeric_expr(ctx, left);
            let r_numeric = is_numeric_expr(ctx, right);
            let l_needs_coerce = !l_fallback_coerced
                && (!l_numeric || expr_may_return_boxed_value_from_raw_f64_fallback(ctx, left));
            let r_needs_coerce = !r_fallback_coerced
                && (!r_numeric || expr_may_return_boxed_value_from_raw_f64_fallback(ctx, right));
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
                        blk.toint32(&l)
                    };
                    let ri = if r_safe {
                        blk.toint32_fast(&r)
                    } else {
                        blk.toint32(&r)
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
                        blk.toint32(&l)
                    };
                    let ri = if r_safe {
                        blk.toint32_fast(&r)
                    } else {
                        blk.toint32(&r)
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
