//! Comparison operators.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::{anyhow, bail, Result};
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
    compute_auto_captures, is_array_expr, is_bigint_expr, is_bool_expr, is_map_expr,
    is_numeric_expr, is_set_expr, is_string_expr, is_url_search_params_expr, receiver_class_name,
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

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::Compare { op, left, right } => {
            // BigInt comparison fast path: NaN-tagged BIGINT_TAG values
            // are unordered under fcmp (NaN), so `a > b` on two bigints
            // always returns false. Route through js_bigint_cmp which
            // returns -1/0/1 for the three bigint ordering outcomes.
            if is_bigint_expr(ctx, left) || is_bigint_expr(ctx, right) {
                let l = lower_expr(ctx, left)?;
                let r = lower_expr(ctx, right)?;
                let blk = ctx.block();
                let l_handle = unbox_to_i64(blk, &l);
                let r_handle = unbox_to_i64(blk, &r);
                let cmp = blk.call(I32, "js_bigint_cmp", &[(I64, &l_handle), (I64, &r_handle)]);
                let bit = match op {
                    CompareOp::Lt => blk.icmp_slt(I32, &cmp, "0"),
                    CompareOp::Le => blk.icmp_sle(I32, &cmp, "0"),
                    CompareOp::Gt => blk.icmp_sgt(I32, &cmp, "0"),
                    CompareOp::Ge => blk.icmp_sge(I32, &cmp, "0"),
                    CompareOp::Eq | CompareOp::LooseEq => blk.icmp_eq(I32, &cmp, "0"),
                    CompareOp::Ne | CompareOp::LooseNe => blk.icmp_ne(I32, &cmp, "0"),
                };
                let tagged = blk.select(
                    crate::types::I1,
                    &bit,
                    I64,
                    crate::nanbox::TAG_TRUE_I64,
                    crate::nanbox::TAG_FALSE_I64,
                );
                return Ok(blk.bitcast_i64_to_double(&tagged));
            }
            // Boolean equality fast path: NaN-tagged TAG_TRUE/FALSE
            // bits don't compare correctly with fcmp. For
            // ===/!== where EITHER side is statically boolean, compare
            // the raw i64 bits via icmp. icmp on bits also works for
            // any other NaN-tagged value (string ptr, object ptr) when
            // the bool literal is on one side — TAG_TRUE bits never
            // match a string/pointer, so the result is correctly false.
            // STRICT only: for LooseEq/LooseNe, booleans need coercion
            // (false == "" → true) which the later js_loose_eq handles.
            let either_bool = is_bool_expr(ctx, left) || is_bool_expr(ctx, right);
            if either_bool && matches!(op, CompareOp::Eq | CompareOp::Ne) {
                let l = lower_expr(ctx, left)?;
                let r = lower_expr(ctx, right)?;
                let blk = ctx.block();
                let l_bits = blk.bitcast_double_to_i64(&l);
                let r_bits = blk.bitcast_double_to_i64(&r);
                let bit = if matches!(op, CompareOp::Ne | CompareOp::LooseNe) {
                    blk.icmp_ne(I64, &l_bits, &r_bits)
                } else {
                    blk.icmp_eq(I64, &l_bits, &r_bits)
                };
                let tagged = blk.select(
                    crate::types::I1,
                    &bit,
                    I64,
                    crate::nanbox::TAG_TRUE_I64,
                    crate::nanbox::TAG_FALSE_I64,
                );
                return Ok(blk.bitcast_i64_to_double(&tagged));
            }
            // Null/Undefined literal fast path: `x === null` / `x === undefined` /
            // `x !== null` etc. Both TAG_NULL and TAG_UNDEFINED are NaN-tagged
            // doubles, so fcmp is unordered (always false) and the string/js_eq
            // fallbacks misclassify these tags as "invalid string → both equal".
            // Compare raw i64 bits directly.
            //
            // For LooseEq/LooseNe (== / !=), null and undefined are loosely
            // equal to each other but not to anything else. Handle that by
            // routing `x == null` to `(bits == TAG_NULL) | (bits == TAG_UNDEF)`.
            let left_is_null = matches!(left.as_ref(), Expr::Null);
            let left_is_undef = matches!(left.as_ref(), Expr::Undefined);
            let right_is_null = matches!(right.as_ref(), Expr::Null);
            let right_is_undef = matches!(right.as_ref(), Expr::Undefined);
            let either_nullish_lit =
                left_is_null || left_is_undef || right_is_null || right_is_undef;
            if either_nullish_lit
                && matches!(
                    op,
                    CompareOp::Eq | CompareOp::Ne | CompareOp::LooseEq | CompareOp::LooseNe
                )
            {
                let l = lower_expr(ctx, left)?;
                let r = lower_expr(ctx, right)?;
                let blk = ctx.block();
                let l_bits = blk.bitcast_double_to_i64(&l);
                let r_bits = blk.bitcast_double_to_i64(&r);
                let is_loose = matches!(op, CompareOp::LooseEq | CompareOp::LooseNe);
                let bit = if is_loose {
                    // Loose equality: x == null → (x === null) || (x === undefined)
                    let eq_l_r = blk.icmp_eq(I64, &l_bits, &r_bits);
                    let cmp_l_null = blk.icmp_eq(I64, &l_bits, crate::nanbox::TAG_NULL_I64);
                    let cmp_l_undef = blk.icmp_eq(I64, &l_bits, crate::nanbox::TAG_UNDEFINED_I64);
                    let cmp_r_null = blk.icmp_eq(I64, &r_bits, crate::nanbox::TAG_NULL_I64);
                    let cmp_r_undef = blk.icmp_eq(I64, &r_bits, crate::nanbox::TAG_UNDEFINED_I64);
                    let l_nullish = blk.or(crate::types::I1, &cmp_l_null, &cmp_l_undef);
                    let r_nullish = blk.or(crate::types::I1, &cmp_r_null, &cmp_r_undef);
                    let both_nullish = blk.and(crate::types::I1, &l_nullish, &r_nullish);
                    blk.or(crate::types::I1, &eq_l_r, &both_nullish)
                } else {
                    // Strict equality: bit-exact compare
                    blk.icmp_eq(I64, &l_bits, &r_bits)
                };
                let bit_final = if matches!(op, CompareOp::Ne | CompareOp::LooseNe) {
                    blk.xor(crate::types::I1, &bit, "true")
                } else {
                    bit
                };
                let tagged = blk.select(
                    crate::types::I1,
                    &bit_final,
                    I64,
                    crate::nanbox::TAG_TRUE_I64,
                    crate::nanbox::TAG_FALSE_I64,
                );
                return Ok(blk.bitcast_i64_to_double(&tagged));
            }
            // "One side is statically string, other is unknown"
            // fallback: `c === Color.Red` where Color is a const
            // object. Neither js_eq (bit-compare, wrong for string
            // content) nor fcmp (NaN-tagged, always false) works.
            //
            // Dispatch through js_string_equals after extracting
            // both string pointers via js_get_string_pointer_unified.
            // That helper returns null for non-string NaN-tagged
            // values, which js_string_equals treats as "not equal"
            // — the correct answer when the unknown side isn't a
            // string at runtime.
            let both_strings_check = is_string_expr(ctx, left) && is_string_expr(ctx, right);
            let one_side_string = !both_strings_check
                && ((is_string_expr(ctx, left)
                    && !is_numeric_expr(ctx, right)
                    && !is_bool_expr(ctx, right))
                    || (is_string_expr(ctx, right)
                        && !is_numeric_expr(ctx, left)
                        && !is_bool_expr(ctx, left)));
            if one_side_string
                && matches!(
                    op,
                    CompareOp::Eq | CompareOp::LooseEq | CompareOp::Ne | CompareOp::LooseNe
                )
            {
                let l = lower_expr(ctx, left)?;
                let r = lower_expr(ctx, right)?;
                let blk = ctx.block();
                let l_handle = blk.call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &l)]);
                let r_handle = blk.call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &r)]);
                let i32_eq = blk.call(
                    I32,
                    "js_string_equals",
                    &[(I64, &l_handle), (I64, &r_handle)],
                );
                let bit = blk.icmp_ne(I32, &i32_eq, "0");
                let bit_final = if matches!(op, CompareOp::Ne | CompareOp::LooseNe) {
                    blk.xor(crate::types::I1, &bit, "true")
                } else {
                    bit
                };
                let tagged = blk.select(
                    crate::types::I1,
                    &bit_final,
                    I64,
                    crate::nanbox::TAG_TRUE_I64,
                    crate::nanbox::TAG_FALSE_I64,
                );
                return Ok(blk.bitcast_i64_to_double(&tagged));
            }
            // Generic equality fallback: when neither operand is
            // statically numeric, dispatch through js_eq which
            // handles strings, booleans, objects, null, undefined
            // via NaN-tag inspection. Used by `eq` helpers in tests
            // that take `any` and pass NaN-tagged values.
            let either_non_numeric = !is_numeric_expr(ctx, left) && !is_numeric_expr(ctx, right);
            let only_eq = matches!(
                op,
                CompareOp::Eq | CompareOp::LooseEq | CompareOp::Ne | CompareOp::LooseNe
            );
            // We still let the more specific paths below win for
            // statically-typed string/bool operands; this fallback
            // only handles the truly-Any case.
            let unknown_l = !is_numeric_expr(ctx, left)
                && !is_string_expr(ctx, left)
                && !is_bool_expr(ctx, left);
            let unknown_r = !is_numeric_expr(ctx, right)
                && !is_string_expr(ctx, right)
                && !is_bool_expr(ctx, right);
            if either_non_numeric && only_eq && unknown_l && unknown_r {
                let l = lower_expr(ctx, left)?;
                let r = lower_expr(ctx, right)?;
                let blk = ctx.block();
                // Use js_loose_eq for == / != (handles null==undefined,
                // cross-type coercion). Use js_eq for === / !==.
                let eq_fn = if matches!(op, CompareOp::LooseEq | CompareOp::LooseNe) {
                    "js_loose_eq"
                } else {
                    "js_eq"
                };
                let l_bits = blk.bitcast_double_to_i64(&l);
                let r_bits = blk.bitcast_double_to_i64(&r);
                let result_bits = blk.call(I64, eq_fn, &[(I64, &l_bits), (I64, &r_bits)]);
                let result = blk.bitcast_i64_to_double(&result_bits);
                if matches!(op, CompareOp::Ne | CompareOp::LooseNe) {
                    let cmp = blk.icmp_eq(I64, &result_bits, crate::nanbox::TAG_TRUE_I64);
                    let inv = blk.xor(crate::types::I1, &cmp, "true");
                    let tagged = blk.select(
                        crate::types::I1,
                        &inv,
                        I64,
                        crate::nanbox::TAG_TRUE_I64,
                        crate::nanbox::TAG_FALSE_I64,
                    );
                    return Ok(blk.bitcast_i64_to_double(&tagged));
                }
                return Ok(result);
            }

            // String equality fast path: fcmp doesn't work on
            // NaN-tagged string pointers (NaN comparisons are
            // unordered → always false). When both operands are
            // statically strings, dispatch through js_string_equals.
            let both_strings = is_string_expr(ctx, left) && is_string_expr(ctx, right);
            if both_strings
                && matches!(
                    op,
                    CompareOp::Eq | CompareOp::LooseEq | CompareOp::Ne | CompareOp::LooseNe
                )
            {
                let l = lower_expr(ctx, left)?;
                let r = lower_expr(ctx, right)?;
                let blk = ctx.block();
                // Issue #214: SSO-safe unbox — the inline mask returns
                // garbage for SHORT_STRING_TAG values (e.g. SSO results
                // from `JSON.parse('["hello"]')[0]`), causing
                // `js_string_equals` to deref the inline payload bytes.
                let l_handle = unbox_str_handle(blk, &l);
                let r_handle = unbox_str_handle(blk, &r);
                let i32_eq = blk.call(
                    I32,
                    "js_string_equals",
                    &[(I64, &l_handle), (I64, &r_handle)],
                );
                let bit = blk.icmp_ne(I32, &i32_eq, "0");
                let bit_final = if matches!(op, CompareOp::Ne | CompareOp::LooseNe) {
                    blk.xor(crate::types::I1, &bit, "true")
                } else {
                    bit
                };
                let tagged_i64 = blk.select(
                    crate::types::I1,
                    &bit_final,
                    crate::types::I64,
                    crate::nanbox::TAG_TRUE_I64,
                    crate::nanbox::TAG_FALSE_I64,
                );
                return Ok(blk.bitcast_i64_to_double(&tagged_i64));
            }
            // String relational fast path: `s1 < s2`, `s1 > s2`, etc.
            // fcmp on NaN-tagged pointers is unordered (always false),
            // so dispatch through js_string_compare which returns
            // -1/0/1 like memcmp. Then test the result against 0 with
            // the right icmp predicate.
            if both_strings
                && matches!(
                    op,
                    CompareOp::Lt | CompareOp::Le | CompareOp::Gt | CompareOp::Ge
                )
            {
                let l = lower_expr(ctx, left)?;
                let r = lower_expr(ctx, right)?;
                let blk = ctx.block();
                // Issue #214: SSO-safe unbox.
                let l_handle = unbox_str_handle(blk, &l);
                let r_handle = unbox_str_handle(blk, &r);
                let cmp_i32 = blk.call(
                    I32,
                    "js_string_compare",
                    &[(I64, &l_handle), (I64, &r_handle)],
                );
                let bit = match op {
                    CompareOp::Lt => blk.icmp_slt(I32, &cmp_i32, "0"),
                    CompareOp::Le => blk.icmp_sle(I32, &cmp_i32, "0"),
                    CompareOp::Gt => blk.icmp_sgt(I32, &cmp_i32, "0"),
                    CompareOp::Ge => blk.icmp_sge(I32, &cmp_i32, "0"),
                    _ => unreachable!(),
                };
                let tagged_i64 = blk.select(
                    crate::types::I1,
                    &bit,
                    crate::types::I64,
                    crate::nanbox::TAG_TRUE_I64,
                    crate::nanbox::TAG_FALSE_I64,
                );
                return Ok(blk.bitcast_i64_to_double(&tagged_i64));
            }

            // Loose equality (==, !=): dispatch through js_loose_eq
            // which handles cross-type coercion (null==undefined,
            // "1"==1, false==0, etc.). Strict === already handled
            // above by the typed fast paths.
            if matches!(op, CompareOp::LooseEq | CompareOp::LooseNe) {
                let l = lower_expr(ctx, left)?;
                let r = lower_expr(ctx, right)?;
                let blk = ctx.block();
                let l_bits = blk.bitcast_double_to_i64(&l);
                let r_bits = blk.bitcast_double_to_i64(&r);
                let result_bits = blk.call(I64, "js_loose_eq", &[(I64, &l_bits), (I64, &r_bits)]);
                if matches!(op, CompareOp::LooseNe) {
                    let cmp = blk.icmp_eq(I64, &result_bits, crate::nanbox::TAG_TRUE_I64);
                    let inv = blk.xor(crate::types::I1, &cmp, "true");
                    let tagged = blk.select(
                        crate::types::I1,
                        &inv,
                        I64,
                        crate::nanbox::TAG_TRUE_I64,
                        crate::nanbox::TAG_FALSE_I64,
                    );
                    return Ok(blk.bitcast_i64_to_double(&tagged));
                }
                return Ok(blk.bitcast_i64_to_double(&result_bits));
            }

            let l = lower_expr(ctx, left)?;
            let r = lower_expr(ctx, right)?;
            let pred = match op {
                CompareOp::Eq => "oeq",
                // !== uses `une` (unordered or not equal), NOT `one`.
                // `one` is "ordered and not equal" which returns false
                // when either operand is NaN. JS !== on NaN must return
                // true: NaN !== NaN → !(NaN === NaN) → !false → true.
                CompareOp::Ne => "une",
                CompareOp::Lt => "olt",
                CompareOp::Le => "ole",
                CompareOp::Gt => "ogt",
                CompareOp::Ge => "oge",
                // LooseEq/Ne handled above
                CompareOp::LooseEq | CompareOp::LooseNe => unreachable!(),
            };
            let blk = ctx.block();
            let bit = blk.fcmp(pred, &l, &r);
            let tag_true_i64 = crate::nanbox::TAG_TRUE_I64;
            let tag_false_i64 = crate::nanbox::TAG_FALSE_I64;
            let tagged_i64 = blk.select(
                crate::types::I1,
                &bit,
                crate::types::I64,
                tag_true_i64,
                tag_false_i64,
            );
            Ok(blk.bitcast_i64_to_double(&tagged_i64))
        }

        // -------- Objects (Phase B.4) --------
        // `{ k1: v1, k2: v2, … }` literal: allocate, set each field by
        // name (key string sourced from the StringPool), NaN-box the
        // pointer via js_nanbox_pointer.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
