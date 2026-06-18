//! Unary operators.
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
    compute_auto_captures, expr_may_return_boxed_value_from_raw_f64_fallback, is_array_expr,
    is_bigint_expr, is_bool_expr, is_map_expr, is_numeric_expr, is_set_expr, is_string_expr,
    is_url_search_params_expr, receiver_class_name,
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
