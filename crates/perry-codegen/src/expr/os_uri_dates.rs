//! OsVersion..DateSetTime (OS/URI/date getters and setters).
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
        Expr::OsVersion => {
            let blk = ctx.block();
            let h = blk.call(I64, "js_os_version", &[]);
            Ok(nanbox_string_inline(blk, &h))
        }
        Expr::ProcessMemoryUsage => {
            // Runtime returns an already NaN-boxed pointer (f64).
            Ok(ctx.block().call(DOUBLE, "js_process_memory_usage", &[]))
        }
        Expr::ProcessThreadCpuUsage => {
            // Runtime returns an already NaN-boxed pointer (f64) for
            // { user, system }.
            Ok(ctx.block().call(DOUBLE, "js_process_thread_cpu_usage", &[]))
        }
        Expr::EncodeURI(o) => {
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let h = blk.call(I64, "js_encode_uri", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &h))
        }
        Expr::DecodeURI(o) => {
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let h = blk.call(I64, "js_decode_uri", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &h))
        }
        Expr::EncodeURIComponent(o) => {
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let h = blk.call(I64, "js_encode_uri_component", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &h))
        }
        Expr::DecodeURIComponent(o) => {
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let h = blk.call(I64, "js_decode_uri_component", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &h))
        }
        Expr::DateToDateString(o) => {
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_date_to_date_string", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &handle))
        }
        Expr::DateToTimeString(o) => {
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_date_to_time_string", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &handle))
        }
        Expr::DateToLocaleDateString(o) => {
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_date_to_locale_date_string", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &handle))
        }
        Expr::DateToLocaleTimeString(o) => {
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_date_to_locale_time_string", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &handle))
        }
        Expr::DateToJSON(o) => {
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_date_to_json", &[(DOUBLE, &v)]);
            let is_null = blk.icmp_eq(I64, &handle, "0");
            let as_string = nanbox_string_inline(blk, &handle);
            let str_bits = blk.bitcast_double_to_i64(&as_string);
            let selected = blk.select(I1, &is_null, I64, crate::nanbox::TAG_NULL_I64, &str_bits);
            Ok(blk.bitcast_i64_to_double(&selected))
        }
        Expr::ArrayWith {
            array,
            index,
            value,
        } => {
            let arr_box = lower_expr(ctx, array)?;
            let idx_d = lower_expr(ctx, index)?;
            let val_d = lower_expr(ctx, value)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let result = blk.call(
                I64,
                "js_array_with",
                &[(I64, &arr_handle), (DOUBLE, &idx_d), (DOUBLE, &val_d)],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }
        Expr::ArrayCopyWithin {
            array_id,
            target,
            start,
            end,
        } => {
            let arr_box = lower_expr(ctx, &Expr::LocalGet(*array_id))?;
            let target_d = lower_expr(ctx, target)?;
            let start_d = lower_expr(ctx, start)?;
            let (has_end_str, end_d) = if let Some(e) = end {
                let v = lower_expr(ctx, e)?;
                ("1".to_string(), v)
            } else {
                ("0".to_string(), "0.0".to_string())
            };
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let result = blk.call(
                I64,
                "js_array_copy_within",
                &[
                    (I64, &arr_handle),
                    (DOUBLE, &target_d),
                    (DOUBLE, &start_d),
                    (I32, &has_end_str),
                    (DOUBLE, &end_d),
                ],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }
        Expr::ArrayToReversed { array } => {
            let arr_box = lower_expr(ctx, array)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let result = blk.call(I64, "js_array_to_reversed", &[(I64, &arr_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }
        Expr::ArrayToSorted { array, comparator } => {
            let arr_box = lower_expr(ctx, array)?;
            let result = if let Some(c) = comparator {
                let cmp_box = lower_expr(ctx, c)?;
                let blk = ctx.block();
                let arr_handle = unbox_to_i64(blk, &arr_box);
                let cmp_handle = unbox_to_i64(blk, &cmp_box);
                blk.call(
                    I64,
                    "js_array_to_sorted_with_comparator",
                    &[(I64, &arr_handle), (I64, &cmp_handle)],
                )
            } else {
                let blk = ctx.block();
                let arr_handle = unbox_to_i64(blk, &arr_box);
                blk.call(I64, "js_array_to_sorted_default", &[(I64, &arr_handle)])
            };
            Ok(nanbox_pointer_inline(ctx.block(), &result))
        }
        Expr::ArrayToSpliced {
            array,
            start,
            delete_count,
            items,
        } => {
            let arr_box = lower_expr(ctx, array)?;
            let start_d = lower_expr(ctx, start)?;
            let count_d = lower_expr(ctx, delete_count)?;

            // Lower items to a Vec of f64 expressions
            let mut item_vals: Vec<String> = Vec::new();
            for it in items {
                item_vals.push(lower_expr(ctx, it)?);
            }

            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);

            let (items_ptr, items_count_str) = if item_vals.is_empty() {
                ("null".to_string(), "0".to_string())
            } else {
                let n = item_vals.len();
                let items_count_str = format!("{}", n);
                let buf_reg = blk.next_reg();
                blk.emit_raw(format!("{} = alloca [{} x double]", buf_reg, n));
                for (i, val) in item_vals.iter().enumerate() {
                    let slot = blk.gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
                    blk.store(DOUBLE, val, &slot);
                }
                (buf_reg, items_count_str)
            };

            let result = blk.call(
                I64,
                "js_array_to_spliced",
                &[
                    (I64, &arr_handle),
                    (DOUBLE, &start_d),
                    (DOUBLE, &count_d),
                    (PTR, &items_ptr),
                    (I32, &items_count_str),
                ],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }
        Expr::ArrayAt { array, index } => {
            // arr.at(i) — negative index counts from the end. The
            // runtime handles the negative-index adjustment +
            // bounds clamp.
            let arr_box = lower_expr(ctx, array)?;
            let idx_d = lower_expr(ctx, index)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            Ok(blk.call(
                DOUBLE,
                "js_array_at",
                &[(I64, &arr_handle), (DOUBLE, &idx_d)],
            ))
        }
        Expr::DateSetUtcMinutes { date, value } => {
            let d = lower_expr(ctx, date)?;
            let v = lower_expr(ctx, value)?;
            Ok(ctx.block().call(
                DOUBLE,
                "js_date_set_utc_minutes",
                &[(DOUBLE, &d), (DOUBLE, &v)],
            ))
        }
        Expr::DateSetUtcSeconds { date, value } => {
            let d = lower_expr(ctx, date)?;
            let v = lower_expr(ctx, value)?;
            Ok(ctx.block().call(
                DOUBLE,
                "js_date_set_utc_seconds",
                &[(DOUBLE, &d), (DOUBLE, &v)],
            ))
        }
        Expr::DateSetUtcMilliseconds { date, value } => {
            let d = lower_expr(ctx, date)?;
            let v = lower_expr(ctx, value)?;
            Ok(ctx.block().call(
                DOUBLE,
                "js_date_set_utc_milliseconds",
                &[(DOUBLE, &d), (DOUBLE, &v)],
            ))
        }
        Expr::Yield { value, .. } => {
            // Generators not implemented; lower the yielded value for
            // side effects and return undefined.
            if let Some(v) = value {
                let _ = lower_expr(ctx, v)?;
            }
            Ok(double_literal(0.0))
        }
        // Each Error subclass gets its own runtime constructor so the
        // ErrorHeader's `error_kind` field is set to the right
        // ERROR_KIND_* — required for `e instanceof TypeError` etc. to
        // walk the ErrorHeader discriminant in `js_instanceof`.
        Expr::TypeErrorNew(msg) => {
            let m = lower_expr(ctx, msg)?;
            let blk = ctx.block();
            let msg_handle = unbox_to_i64(blk, &m);
            let err_handle = blk.call(I64, "js_typeerror_new", &[(I64, &msg_handle)]);
            Ok(nanbox_pointer_inline(blk, &err_handle))
        }
        Expr::RangeErrorNew(msg) => {
            let m = lower_expr(ctx, msg)?;
            let blk = ctx.block();
            let msg_handle = unbox_to_i64(blk, &m);
            let err_handle = blk.call(I64, "js_rangeerror_new", &[(I64, &msg_handle)]);
            Ok(nanbox_pointer_inline(blk, &err_handle))
        }
        Expr::SyntaxErrorNew(msg) => {
            let m = lower_expr(ctx, msg)?;
            let blk = ctx.block();
            let msg_handle = unbox_to_i64(blk, &m);
            let err_handle = blk.call(I64, "js_syntaxerror_new", &[(I64, &msg_handle)]);
            Ok(nanbox_pointer_inline(blk, &err_handle))
        }
        Expr::ReferenceErrorNew(msg) => {
            let m = lower_expr(ctx, msg)?;
            let blk = ctx.block();
            let msg_handle = unbox_to_i64(blk, &m);
            let err_handle = blk.call(I64, "js_referenceerror_new", &[(I64, &msg_handle)]);
            Ok(nanbox_pointer_inline(blk, &err_handle))
        }
        Expr::NumberIsSafeInteger(operand) => {
            let v = lower_expr(ctx, operand)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_number_is_safe_integer", &[(DOUBLE, &v)]))
        }
        Expr::ObjectFreeze(o) => {
            let v = lower_expr(ctx, o)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_object_freeze", &[(DOUBLE, &v)]))
        }
        Expr::ObjectSeal(o) => {
            let v = lower_expr(ctx, o)?;
            Ok(ctx.block().call(DOUBLE, "js_object_seal", &[(DOUBLE, &v)]))
        }
        Expr::ObjectPreventExtensions(o) => {
            let v = lower_expr(ctx, o)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_object_prevent_extensions", &[(DOUBLE, &v)]))
        }
        Expr::DateSetUtcMonth { date, value } => {
            let d = lower_expr(ctx, date)?;
            let v = lower_expr(ctx, value)?;
            Ok(ctx.block().call(
                DOUBLE,
                "js_date_set_utc_month",
                &[(DOUBLE, &d), (DOUBLE, &v)],
            ))
        }
        // Local-time Date setters (#1187). The runtime functions all share
        // the same `(timestamp, value) -> new_timestamp` shape, so the
        // lowering is the same modulo the C symbol name.
        Expr::DateSetFullYear { date, value } => {
            let d = lower_expr(ctx, date)?;
            let v = lower_expr(ctx, value)?;
            Ok(ctx.block().call(
                DOUBLE,
                "js_date_set_full_year",
                &[(DOUBLE, &d), (DOUBLE, &v)],
            ))
        }
        Expr::DateSetMonth { date, value } => {
            let d = lower_expr(ctx, date)?;
            let v = lower_expr(ctx, value)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_date_set_month", &[(DOUBLE, &d), (DOUBLE, &v)]))
        }
        Expr::DateSetDate { date, value } => {
            let d = lower_expr(ctx, date)?;
            let v = lower_expr(ctx, value)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_date_set_date", &[(DOUBLE, &d), (DOUBLE, &v)]))
        }
        Expr::DateSetHours { date, value } => {
            let d = lower_expr(ctx, date)?;
            let v = lower_expr(ctx, value)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_date_set_hours", &[(DOUBLE, &d), (DOUBLE, &v)]))
        }
        Expr::DateSetMinutes { date, value } => {
            let d = lower_expr(ctx, date)?;
            let v = lower_expr(ctx, value)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_date_set_minutes", &[(DOUBLE, &d), (DOUBLE, &v)]))
        }
        Expr::DateSetSeconds { date, value } => {
            let d = lower_expr(ctx, date)?;
            let v = lower_expr(ctx, value)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_date_set_seconds", &[(DOUBLE, &d), (DOUBLE, &v)]))
        }
        Expr::DateSetMilliseconds { date, value } => {
            let d = lower_expr(ctx, date)?;
            let v = lower_expr(ctx, value)?;
            Ok(ctx.block().call(
                DOUBLE,
                "js_date_set_milliseconds",
                &[(DOUBLE, &d), (DOUBLE, &v)],
            ))
        }
        Expr::DateSetTime { date, value } => {
            let d = lower_expr(ctx, date)?;
            let v = lower_expr(ctx, value)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_date_set_time", &[(DOUBLE, &d), (DOUBLE, &v)]))
        }
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
