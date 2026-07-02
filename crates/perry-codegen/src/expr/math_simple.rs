//! IsNaN..MapNew: Math/Map/Set/WebAssembly/JsonStringify helpers.
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
    compute_auto_captures, is_array_expr, is_bigint_expr, is_bool_expr, is_definitely_string_expr,
    is_map_expr, is_numeric_expr, is_set_expr, is_string_expr, is_url_search_params_expr,
    map_static_type_args, receiver_class_name,
};
#[allow(unused_imports)]
use crate::types::{DOUBLE, F32, I1, I32, I64, I8, PTR};

#[allow(unused_imports)]
use super::{
    buffer_alias_metadata_suffix, can_lower_expr_as_i32, emit_layout_note_slot_on_block,
    emit_shadow_slot_clear, emit_shadow_slot_update_for_expr, emit_string_literal_global,
    emit_v8_export_call, emit_v8_member_method_call, emit_write_barrier,
    emit_write_barrier_slot_on_block, expr_is_known_non_pointer_shadow_value,
    extract_array_of_object_shape, i32_bool_to_nanbox, import_origin_suffix,
    is_global_this_builtin_function_name, is_global_this_builtin_name, is_known_finite,
    lower_array_literal, lower_channel_reduction, lower_expr, lower_expr_as_i32, lower_expr_native,
    lower_index_set_fast, lower_js_args_array, lower_math_operand, lower_object_literal,
    lower_stream_super_init, lower_url_string_getter, nanbox_bigint_inline, nanbox_pointer_inline,
    nanbox_pointer_inline_pub, nanbox_string_inline, proxy_build_args_array,
    record_collection_number_key_fallback, record_collection_number_key_selected,
    record_collection_string_key_fallback, record_collection_string_key_selected,
    record_collection_string_key_value_selected, record_collection_typed_value_fallback,
    record_collection_typed_value_selected, try_flat_const_2d_int, try_lower_flat_const_index_get,
    try_match_channel_reduction, try_static_class_name, unbox_str_handle, unbox_to_i64,
    variant_name, ChannelReduction, FlatConstInfo, FnCtx, I18nLowerCtx,
};

fn is_static_string_number_map(ctx: &FnCtx<'_>, map: &Expr) -> bool {
    matches!(
        map_static_type_args(ctx, map),
        Some([
            HirType::String | HirType::StringLiteral(_),
            HirType::Number | HirType::Int32
        ])
    )
}

fn is_static_string_i32_map(ctx: &FnCtx<'_>, map: &Expr) -> bool {
    matches!(
        map_static_type_args(ctx, map),
        Some([HirType::String | HirType::StringLiteral(_), HirType::Int32])
    )
}

fn is_perry_u32_type(ctx: &FnCtx<'_>, ty: &HirType) -> bool {
    match ty {
        HirType::Named(name) if name == "PerryU32" => true,
        HirType::Named(name) => ctx
            .type_aliases
            .get(name)
            .is_some_and(|alias| is_perry_u32_type(ctx, alias)),
        _ => false,
    }
}

fn is_static_string_u32_map(ctx: &FnCtx<'_>, map: &Expr) -> bool {
    match map_static_type_args(ctx, map) {
        Some([HirType::String | HirType::StringLiteral(_), value_ty]) => {
            is_perry_u32_type(ctx, value_ty)
        }
        _ => false,
    }
}

fn is_perry_f32_type(ctx: &FnCtx<'_>, ty: &HirType) -> bool {
    match ty {
        HirType::Named(name) if name == "PerryF32" => true,
        HirType::Named(name) => ctx
            .type_aliases
            .get(name)
            .is_some_and(|alias| is_perry_f32_type(ctx, alias)),
        _ => false,
    }
}

fn is_static_string_f32_map(ctx: &FnCtx<'_>, map: &Expr) -> bool {
    match map_static_type_args(ctx, map) {
        Some([HirType::String | HirType::StringLiteral(_), value_ty]) => {
            is_perry_f32_type(ctx, value_ty)
        }
        _ => false,
    }
}

fn is_static_string_boolean_map(ctx: &FnCtx<'_>, map: &Expr) -> bool {
    matches!(
        map_static_type_args(ctx, map),
        Some([
            HirType::String | HirType::StringLiteral(_),
            HirType::Boolean
        ])
    )
}

fn is_static_string_string_map(ctx: &FnCtx<'_>, map: &Expr) -> bool {
    matches!(
        map_static_type_args(ctx, map),
        Some([
            HirType::String | HirType::StringLiteral(_),
            HirType::String | HirType::StringLiteral(_)
        ])
    )
}

fn can_lower_i32_for_collection_value(ctx: &FnCtx<'_>, value: &Expr) -> bool {
    can_lower_expr_as_i32(
        value,
        &ctx.i32_counter_slots,
        ctx.flat_const_arrays,
        &ctx.array_row_aliases,
        ctx.integer_locals,
        ctx.clamp3_functions,
        ctx.clamp_u8_functions,
        ctx.integer_returning_functions,
        ctx.i32_identity_functions,
    )
}

fn can_use_string_i32_map_value(ctx: &FnCtx<'_>, value: &Expr) -> bool {
    can_lower_i32_for_collection_value(ctx, value)
}

fn can_use_string_u32_map_value(ctx: &FnCtx<'_>, value: &Expr) -> bool {
    match value {
        Expr::Integer(n) => *n >= 0 && u32::try_from(*n).is_ok(),
        Expr::Binary {
            op: BinaryOp::UShr,
            left,
            right,
        } => {
            can_lower_i32_for_collection_value(ctx, left)
                && can_lower_i32_for_collection_value(ctx, right)
        }
        Expr::Uint8ArrayGet { .. }
        | Expr::BufferIndexGet { .. }
        | Expr::Uint8ArrayLength(_)
        | Expr::BufferLength(_) => true,
        Expr::LocalGet(id) => ctx.unsigned_i32_locals.contains(id),
        _ => false,
    }
}

fn literal_f64(expr: &Expr) -> Option<f64> {
    match expr {
        Expr::Integer(n) => Some(*n as f64),
        Expr::Number(n) => Some(*n),
        _ => None,
    }
}

fn f32_roundtrips_exact(value: f64) -> bool {
    let narrowed = value as f32;
    (narrowed as f64).to_bits() == value.to_bits()
}

fn can_use_string_f32_map_value(value: &Expr) -> bool {
    literal_f64(value).is_some_and(f32_roundtrips_exact)
}

fn can_use_string_boolean_map_value(ctx: &FnCtx<'_>, value: &Expr) -> bool {
    match value {
        Expr::Bool(_) => true,
        Expr::LocalGet(id) => {
            ctx.i1_local_slots.contains_key(id)
                && !ctx.closure_captures.contains_key(id)
                && !ctx.boxed_vars.contains(id)
                && !ctx.module_globals.contains_key(id)
        }
        _ => false,
    }
}

fn is_static_string_key_map(ctx: &FnCtx<'_>, map: &Expr) -> bool {
    matches!(
        map_static_type_args(ctx, map),
        Some([HirType::String | HirType::StringLiteral(_), _])
    )
}

fn is_static_number_key_map(ctx: &FnCtx<'_>, map: &Expr) -> bool {
    matches!(
        map_static_type_args(ctx, map),
        Some([HirType::Number | HirType::Int32, _])
    )
}

fn is_static_number_string_map(ctx: &FnCtx<'_>, map: &Expr) -> bool {
    matches!(
        map_static_type_args(ctx, map),
        Some([
            HirType::Number | HirType::Int32,
            HirType::String | HirType::StringLiteral(_)
        ])
    )
}

fn guarded_map_number_key_set(
    ctx: &mut FnCtx<'_>,
    map_handle: &str,
    key_box: &str,
    value_box: &str,
) -> String {
    let guard_raw = ctx
        .block()
        .call(I32, "js_typed_f64_arg_guard", &[(DOUBLE, key_box)]);
    let guard = ctx.block().icmp_ne(I32, &guard_raw, "0");
    let fast_idx = ctx.new_block("map_number_key.set.fast");
    let fallback_idx = ctx.new_block("map_number_key.set.fallback");
    let merge_idx = ctx.new_block("map_number_key.set.merge");
    let fast_label = ctx.block_label(fast_idx);
    let fallback_label = ctx.block_label(fallback_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&guard, &fast_label, &fallback_label);

    ctx.current_block = fast_idx;
    let key_raw = ctx
        .block()
        .call(DOUBLE, "js_typed_f64_arg_to_raw", &[(DOUBLE, key_box)]);
    let fast_value = ctx.block().call(
        I64,
        "js_map_set_number_key",
        &[(I64, map_handle), (DOUBLE, &key_raw), (DOUBLE, value_box)],
    );
    record_collection_number_key_selected(
        ctx,
        "MapSet",
        "collection_number_key.map_set",
        &key_raw,
        "map",
        "number_key_helper",
        "js_map_set_number_key",
        "key",
    );
    let after_fast = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = fallback_idx;
    let fallback_value = ctx.block().call(
        I64,
        "js_map_set",
        &[(I64, map_handle), (DOUBLE, key_box), (DOUBLE, value_box)],
    );
    record_collection_number_key_fallback(
        ctx,
        "MapSet",
        "collection_number_key.map_set_generic",
        key_box,
        "map",
        "number_key_helper",
        "js_map_set",
        "runtime_key_guard_failed",
        "key",
    );
    let after_fallback = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = merge_idx;
    ctx.block().phi(
        I64,
        &[
            (fast_value.as_str(), after_fast.as_str()),
            (fallback_value.as_str(), after_fallback.as_str()),
        ],
    )
}

fn guarded_map_number_key_get(ctx: &mut FnCtx<'_>, map_handle: &str, key_box: &str) -> String {
    let guard_raw = ctx
        .block()
        .call(I32, "js_typed_f64_arg_guard", &[(DOUBLE, key_box)]);
    let guard = ctx.block().icmp_ne(I32, &guard_raw, "0");
    let fast_idx = ctx.new_block("map_number_key.get.fast");
    let fallback_idx = ctx.new_block("map_number_key.get.fallback");
    let merge_idx = ctx.new_block("map_number_key.get.merge");
    let fast_label = ctx.block_label(fast_idx);
    let fallback_label = ctx.block_label(fallback_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&guard, &fast_label, &fallback_label);

    ctx.current_block = fast_idx;
    let key_raw = ctx
        .block()
        .call(DOUBLE, "js_typed_f64_arg_to_raw", &[(DOUBLE, key_box)]);
    let fast_value = ctx.block().call(
        DOUBLE,
        "js_map_get_number_key",
        &[(I64, map_handle), (DOUBLE, &key_raw)],
    );
    record_collection_number_key_selected(
        ctx,
        "MapGet",
        "collection_number_key.map_get",
        &key_raw,
        "map",
        "number_key_helper",
        "js_map_get_number_key",
        "key",
    );
    let after_fast = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = fallback_idx;
    let fallback_value = ctx.block().call(
        DOUBLE,
        "js_map_get",
        &[(I64, map_handle), (DOUBLE, key_box)],
    );
    record_collection_number_key_fallback(
        ctx,
        "MapGet",
        "collection_number_key.map_get_generic",
        key_box,
        "map",
        "number_key_helper",
        "js_map_get",
        "runtime_key_guard_failed",
        "key",
    );
    let after_fallback = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = merge_idx;
    ctx.block().phi(
        DOUBLE,
        &[
            (fast_value.as_str(), after_fast.as_str()),
            (fallback_value.as_str(), after_fallback.as_str()),
        ],
    )
}

fn guarded_map_number_key_has(ctx: &mut FnCtx<'_>, map_handle: &str, key_box: &str) -> String {
    let guard_raw = ctx
        .block()
        .call(I32, "js_typed_f64_arg_guard", &[(DOUBLE, key_box)]);
    let guard = ctx.block().icmp_ne(I32, &guard_raw, "0");
    let fast_idx = ctx.new_block("map_number_key.has.fast");
    let fallback_idx = ctx.new_block("map_number_key.has.fallback");
    let merge_idx = ctx.new_block("map_number_key.has.merge");
    let fast_label = ctx.block_label(fast_idx);
    let fallback_label = ctx.block_label(fallback_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&guard, &fast_label, &fallback_label);

    ctx.current_block = fast_idx;
    let key_raw = ctx
        .block()
        .call(DOUBLE, "js_typed_f64_arg_to_raw", &[(DOUBLE, key_box)]);
    let fast_value = ctx.block().call(
        I32,
        "js_map_has_number_key",
        &[(I64, map_handle), (DOUBLE, &key_raw)],
    );
    record_collection_number_key_selected(
        ctx,
        "MapHas",
        "collection_number_key.map_has",
        &key_raw,
        "map",
        "number_key_helper",
        "js_map_has_number_key",
        "key",
    );
    let after_fast = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = fallback_idx;
    let fallback_value =
        ctx.block()
            .call(I32, "js_map_has", &[(I64, map_handle), (DOUBLE, key_box)]);
    record_collection_number_key_fallback(
        ctx,
        "MapHas",
        "collection_number_key.map_has_generic",
        key_box,
        "map",
        "number_key_helper",
        "js_map_has",
        "runtime_key_guard_failed",
        "key",
    );
    let after_fallback = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = merge_idx;
    ctx.block().phi(
        I32,
        &[
            (fast_value.as_str(), after_fast.as_str()),
            (fallback_value.as_str(), after_fallback.as_str()),
        ],
    )
}

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::IsNaN(operand) => {
            let v = lower_expr(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "js_is_nan", &[(DOUBLE, &v)]))
        }

        // -------- Math.pow (special variant — separate from Binary::Pow) --------
        Expr::MathPow(base, exp) => {
            let b = lower_math_operand(ctx, base)?;
            let e = lower_math_operand(ctx, exp)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_math_pow", &[(DOUBLE, &b), (DOUBLE, &e)]))
        }

        // -------- Math.imul — 32-bit wrapping integer multiply --------
        // Route through the runtime helper so non-finite inputs use JS
        // ToInt32 semantics (`NaN`/±Infinity -> 0) instead of LLVM fptosi.
        Expr::MathImul(a, b) => {
            let av = lower_expr(ctx, a)?;
            let bv = lower_expr(ctx, b)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_math_imul", &[(DOUBLE, &av), (DOUBLE, &bv)]))
        }

        // -------- new Error() / new Error(message) --------
        Expr::ErrorNew(opt_msg) => {
            if let Some(msg_expr) = opt_msg {
                let msg = lower_expr(ctx, msg_expr)?;
                let blk = ctx.block();
                let err_handle = blk.call(I64, "js_error_new_from_value", &[(DOUBLE, &msg)]);
                Ok(nanbox_pointer_inline(blk, &err_handle))
            } else {
                let err_handle = ctx.block().call(I64, "js_error_new", &[]);
                Ok(nanbox_pointer_inline(ctx.block(), &err_handle))
            }
        }

        // -------- arr.pop() / arr.shift() (special HIR variants) --------
        // Like ArrayPush, the HIR pre-resolves these so we get the
        // local id directly. Pop returns the removed element (NaN if
        // empty); shift removes from the front. We currently support
        // pop only.
        Expr::ArrayPop(array_id) => {
            // pop is a read-only access for the storage; we don't need
            // to write back. Resolve via LocalGet so closure captures
            // and module globals work transparently.
            let arr_box = lower_expr(ctx, &Expr::LocalGet(*array_id))?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            Ok(blk.call(DOUBLE, "js_array_pop_f64", &[(I64, &arr_handle)]))
        }

        // -------- arr.map(callback) (special variant) --------
        // The runtime js_array_map takes a closure header pointer and
        // calls it for each element. The callback expression usually
        // lowers to a NaN-boxed closure value, which we unbox to i64.
        Expr::ArrayMap { array, callback } => {
            let arr_box = lower_expr(ctx, array)?;
            let cb_box = lower_expr(ctx, callback)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            // #4091: throw TypeError for a non-callable callback before iterating.
            // `map` uses a receiver-aware validator (TypedArray.map renders its
            // non-callable message differently than Array.prototype.map).
            let cb_handle = blk.call(
                I64,
                "js_validate_array_map_callback",
                &[(I64, &arr_handle), (DOUBLE, &cb_box)],
            );
            let result = blk.call(
                I64,
                "js_array_map",
                &[(I64, &arr_handle), (I64, &cb_handle)],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }

        // -------- map.set(key, value) / .get / .has --------
        Expr::MapSet { map, key, value } => {
            let has_string_key_map =
                is_static_string_key_map(ctx, map) && is_definitely_string_expr(ctx, key);
            let use_number_key_map = !has_string_key_map
                && is_static_number_key_map(ctx, map)
                && is_numeric_expr(ctx, key);
            let static_number_string_map =
                use_number_key_map && is_static_number_string_map(ctx, map);
            let use_number_string_map =
                static_number_string_map && is_definitely_string_expr(ctx, value);
            let use_string_i32_map = is_static_string_i32_map(ctx, map)
                && is_definitely_string_expr(ctx, key)
                && can_use_string_i32_map_value(ctx, value);
            let use_string_u32_map = is_static_string_u32_map(ctx, map)
                && is_definitely_string_expr(ctx, key)
                && can_use_string_u32_map_value(ctx, value);
            let use_string_f32_map = is_static_string_f32_map(ctx, map)
                && is_definitely_string_expr(ctx, key)
                && can_use_string_f32_map_value(value);
            let use_string_number_map =
                is_static_string_number_map(ctx, map) && is_definitely_string_expr(ctx, key);
            let static_string_boolean_map =
                is_static_string_boolean_map(ctx, map) && is_definitely_string_expr(ctx, key);
            let use_string_boolean_map =
                static_string_boolean_map && can_use_string_boolean_map_value(ctx, value);
            let use_string_string_map = is_static_string_string_map(ctx, map)
                && is_definitely_string_expr(ctx, key)
                && is_definitely_string_expr(ctx, value);
            let m_box = lower_expr(ctx, map)?;
            let k_box = lower_expr(ctx, key)?;
            let m_handle = {
                let blk = ctx.block();
                unbox_to_i64(blk, &m_box)
            };
            let new_handle = if use_string_i32_map {
                let value_i32 =
                    lower_expr_native(ctx, value, crate::native_value::ExpectedNativeRep::I32)?;
                let (k_handle, new_handle) = {
                    let blk = ctx.block();
                    let k_handle = unbox_str_handle(blk, &k_box);
                    let new_handle = blk.call(
                        I64,
                        "js_map_set_string_i32",
                        &[(I64, &m_handle), (I64, &k_handle), (I32, &value_i32.value)],
                    );
                    (k_handle, new_handle)
                };
                record_collection_string_key_value_selected(
                    ctx,
                    "MapSet",
                    "collection_string_key.map_set_string_i32",
                    &value_i32,
                    "map",
                    "int32_value_helper",
                    "js_map_set_string_i32",
                );
                record_collection_string_key_selected(
                    ctx,
                    "MapSet",
                    "collection_string_key.map_set_string_i32_key",
                    &k_handle,
                    "map",
                    "js_map_set_string_i32",
                );
                new_handle
            } else if use_string_u32_map {
                let value_u32 =
                    lower_expr_native(ctx, value, crate::native_value::ExpectedNativeRep::U32)?;
                let (k_handle, new_handle) = {
                    let blk = ctx.block();
                    let k_handle = unbox_str_handle(blk, &k_box);
                    let new_handle = blk.call(
                        I64,
                        "js_map_set_string_u32",
                        &[(I64, &m_handle), (I64, &k_handle), (I32, &value_u32.value)],
                    );
                    (k_handle, new_handle)
                };
                record_collection_string_key_value_selected(
                    ctx,
                    "MapSet",
                    "collection_string_key.map_set_string_u32",
                    &value_u32,
                    "map",
                    "uint32_value_helper",
                    "js_map_set_string_u32",
                );
                record_collection_string_key_selected(
                    ctx,
                    "MapSet",
                    "collection_string_key.map_set_string_u32_key",
                    &k_handle,
                    "map",
                    "js_map_set_string_u32",
                );
                new_handle
            } else if use_string_f32_map {
                let value_f32 =
                    lower_expr_native(ctx, value, crate::native_value::ExpectedNativeRep::F32)?;
                let (k_handle, new_handle) = {
                    let blk = ctx.block();
                    let k_handle = unbox_str_handle(blk, &k_box);
                    let new_handle = blk.call(
                        I64,
                        "js_map_set_string_f32",
                        &[(I64, &m_handle), (I64, &k_handle), (F32, &value_f32.value)],
                    );
                    (k_handle, new_handle)
                };
                record_collection_string_key_value_selected(
                    ctx,
                    "MapSet",
                    "collection_string_key.map_set_string_f32",
                    &value_f32,
                    "map",
                    "float32_value_helper",
                    "js_map_set_string_f32",
                );
                record_collection_string_key_selected(
                    ctx,
                    "MapSet",
                    "collection_string_key.map_set_string_f32_key",
                    &k_handle,
                    "map",
                    "js_map_set_string_f32",
                );
                new_handle
            } else if use_string_number_map {
                let v_box = lower_expr(ctx, value)?;
                let (k_handle, new_handle) = {
                    let blk = ctx.block();
                    let k_handle = unbox_str_handle(blk, &k_box);
                    let new_handle = blk.call(
                        I64,
                        "js_map_set_string_number",
                        &[(I64, &m_handle), (I64, &k_handle), (DOUBLE, &v_box)],
                    );
                    (k_handle, new_handle)
                };
                record_collection_string_key_selected(
                    ctx,
                    "MapSet",
                    "collection_string_key.map_set_string_number",
                    &k_handle,
                    "map",
                    "js_map_set_string_number",
                );
                new_handle
            } else if use_string_boolean_map {
                let value_i1 =
                    lower_expr_native(ctx, value, crate::native_value::ExpectedNativeRep::I1)?;
                let (k_handle, new_handle) = {
                    let blk = ctx.block();
                    let k_handle = unbox_str_handle(blk, &k_box);
                    let value_i32 = blk.zext(I1, &value_i1.value, I32);
                    let new_handle = blk.call(
                        I64,
                        "js_map_set_string_bool",
                        &[(I64, &m_handle), (I64, &k_handle), (I32, &value_i32)],
                    );
                    (k_handle, new_handle)
                };
                record_collection_string_key_value_selected(
                    ctx,
                    "MapSet",
                    "collection_string_key.map_set_string_bool",
                    &value_i1,
                    "map",
                    "boolean_value_helper",
                    "js_map_set_string_bool",
                );
                record_collection_string_key_selected(
                    ctx,
                    "MapSet",
                    "collection_string_key.map_set_string_bool_key",
                    &k_handle,
                    "map",
                    "js_map_set_string_bool",
                );
                new_handle
            } else if use_string_string_map {
                let v_box = lower_expr(ctx, value)?;
                let (k_handle, v_handle, new_handle) = {
                    let blk = ctx.block();
                    let k_handle = unbox_str_handle(blk, &k_box);
                    let v_handle = unbox_str_handle(blk, &v_box);
                    let new_handle = blk.call(
                        I64,
                        "js_map_set_string_string",
                        &[(I64, &m_handle), (I64, &k_handle), (I64, &v_handle)],
                    );
                    (k_handle, v_handle, new_handle)
                };
                let lowered_value = crate::native_value::LoweredValue::string_ref(&v_handle);
                record_collection_string_key_value_selected(
                    ctx,
                    "MapSet",
                    "collection_string_key.map_set_string_string",
                    &lowered_value,
                    "map",
                    "string_value_helper",
                    "js_map_set_string_string",
                );
                record_collection_string_key_selected(
                    ctx,
                    "MapSet",
                    "collection_string_key.map_set_string_string_key",
                    &k_handle,
                    "map",
                    "js_map_set_string_string",
                );
                new_handle
            } else if has_string_key_map {
                let v_box = lower_expr(ctx, value)?;
                let (k_handle, new_handle) = {
                    let blk = ctx.block();
                    let k_handle = unbox_str_handle(blk, &k_box);
                    let new_handle = blk.call(
                        I64,
                        "js_map_set_string_key",
                        &[(I64, &m_handle), (I64, &k_handle), (DOUBLE, &v_box)],
                    );
                    (k_handle, new_handle)
                };
                record_collection_string_key_selected(
                    ctx,
                    "MapSet",
                    "collection_string_key.map_set_string_key",
                    &k_handle,
                    "map",
                    "js_map_set_string_key",
                );
                if static_string_boolean_map {
                    record_collection_typed_value_fallback(
                        ctx,
                        "MapSet",
                        "collection_typed_value.map_set_string_bool_generic",
                        &v_box,
                        "map",
                        "boolean_value_helper",
                        "js_map_set_string_key",
                        "value_expr_not_native_i1",
                    );
                }
                new_handle
            } else if use_number_string_map {
                let v_box = lower_expr(ctx, value)?;
                let (v_handle, v_slot_box) = {
                    let blk = ctx.block();
                    let v_handle = unbox_str_handle(blk, &v_box);
                    let v_slot_box = nanbox_string_inline(blk, &v_handle);
                    (v_handle, v_slot_box)
                };
                let lowered_value = crate::native_value::LoweredValue::string_ref(&v_handle);
                record_collection_typed_value_selected(
                    ctx,
                    "MapSet",
                    "collection_typed_value.map_set_number_string",
                    &lowered_value,
                    "map",
                    "string_value_helper",
                    "js_map_set_number_key",
                    "map_slot",
                );
                guarded_map_number_key_set(ctx, &m_handle, &k_box, &v_slot_box)
            } else if use_number_key_map {
                let v_box = lower_expr(ctx, value)?;
                if static_number_string_map {
                    record_collection_typed_value_fallback(
                        ctx,
                        "MapSet",
                        "collection_typed_value.map_set_number_string_generic",
                        &v_box,
                        "map",
                        "string_value_helper",
                        "js_map_set_number_key",
                        "value_expr_not_definitely_string",
                    );
                }
                guarded_map_number_key_set(ctx, &m_handle, &k_box, &v_box)
            } else {
                let v_box = lower_expr(ctx, value)?;
                let new_handle = {
                    let blk = ctx.block();
                    blk.call(
                        I64,
                        "js_map_set",
                        &[(I64, &m_handle), (DOUBLE, &k_box), (DOUBLE, &v_box)],
                    )
                };
                record_collection_string_key_fallback(
                    ctx,
                    "MapSet",
                    "collection_string_key.map_set_generic",
                    &k_box,
                    "map",
                    "js_map_set",
                    "receiver_or_key_not_static_string",
                );
                new_handle
            };
            // map.set returns the (possibly-realloc'd) map. Re-NaN-box
            // and return. The caller may need to write this back to a
            // local; that's the caller's problem if Map is held in a
            // mutable variable that grows.
            let blk = ctx.block();
            Ok(nanbox_pointer_inline(blk, &new_handle))
        }
        Expr::MapGet { map, key } => {
            let use_string_key_map =
                is_static_string_key_map(ctx, map) && is_definitely_string_expr(ctx, key);
            let use_number_key_map = !use_string_key_map
                && is_static_number_key_map(ctx, map)
                && is_numeric_expr(ctx, key);
            let m_box = lower_expr(ctx, map)?;
            let k_box = lower_expr(ctx, key)?;
            let m_handle = {
                let blk = ctx.block();
                unbox_to_i64(blk, &m_box)
            };
            if use_string_key_map {
                let (k_handle, value) = {
                    let blk = ctx.block();
                    let k_handle = unbox_str_handle(blk, &k_box);
                    let value = blk.call(
                        DOUBLE,
                        "js_map_get_string_key",
                        &[(I64, &m_handle), (I64, &k_handle)],
                    );
                    (k_handle, value)
                };
                record_collection_string_key_selected(
                    ctx,
                    "MapGet",
                    "collection_string_key.map_get",
                    &k_handle,
                    "map",
                    "js_map_get_string_key",
                );
                Ok(value)
            } else if use_number_key_map {
                Ok(guarded_map_number_key_get(ctx, &m_handle, &k_box))
            } else {
                let value = {
                    let blk = ctx.block();
                    blk.call(DOUBLE, "js_map_get", &[(I64, &m_handle), (DOUBLE, &k_box)])
                };
                record_collection_string_key_fallback(
                    ctx,
                    "MapGet",
                    "collection_string_key.map_get_generic",
                    &k_box,
                    "map",
                    "js_map_get",
                    "receiver_or_key_not_static_string",
                );
                Ok(value)
            }
        }
        Expr::MapHas { map, key } => {
            let use_string_key_map =
                is_static_string_key_map(ctx, map) && is_definitely_string_expr(ctx, key);
            let use_number_key_map = !use_string_key_map
                && is_static_number_key_map(ctx, map)
                && is_numeric_expr(ctx, key);
            let m_box = lower_expr(ctx, map)?;
            let k_box = lower_expr(ctx, key)?;
            let m_handle = {
                let blk = ctx.block();
                unbox_to_i64(blk, &m_box)
            };
            let i32_v = if use_string_key_map {
                let (k_handle, i32_v) = {
                    let blk = ctx.block();
                    let k_handle = unbox_str_handle(blk, &k_box);
                    let i32_v = blk.call(
                        I32,
                        "js_map_has_string_key",
                        &[(I64, &m_handle), (I64, &k_handle)],
                    );
                    (k_handle, i32_v)
                };
                record_collection_string_key_selected(
                    ctx,
                    "MapHas",
                    "collection_string_key.map_has",
                    &k_handle,
                    "map",
                    "js_map_has_string_key",
                );
                i32_v
            } else if use_number_key_map {
                guarded_map_number_key_has(ctx, &m_handle, &k_box)
            } else {
                let i32_v = {
                    let blk = ctx.block();
                    blk.call(I32, "js_map_has", &[(I64, &m_handle), (DOUBLE, &k_box)])
                };
                record_collection_string_key_fallback(
                    ctx,
                    "MapHas",
                    "collection_string_key.map_has_generic",
                    &k_box,
                    "map",
                    "js_map_has",
                    "receiver_or_key_not_static_string",
                );
                i32_v
            };
            // NaN-tagged boolean for "true"/"false" printing.
            let blk = ctx.block();
            let bit = blk.icmp_ne(I32, &i32_v, "0");
            let tagged = blk.select(
                crate::types::I1,
                &bit,
                I64,
                crate::nanbox::TAG_TRUE_I64,
                crate::nanbox::TAG_FALSE_I64,
            );
            Ok(blk.bitcast_i64_to_double(&tagged))
        }

        // -------- Math.* unary helpers (Phase B.15) --------
        // Math.* unary functions: use LLVM intrinsics directly so the
        // generated code becomes a single hardware instruction (or
        // libm call resolved at link time, which is always present).
        // Avoids depending on `js_math_*` runtime symbols which the
        // auto-optimizer's dead-stripping was removing from the
        // built `libperry_runtime.a`.
        //
        // Uses LLVM intrinsics (llvm.sqrt.f64, llvm.floor.f64, etc.).
        Expr::MathSqrt(operand) => {
            let v = lower_math_operand(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "llvm.sqrt.f64", &[(DOUBLE, &v)]))
        }
        Expr::MathFloor(operand) => {
            let v = lower_math_operand(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "llvm.floor.f64", &[(DOUBLE, &v)]))
        }
        Expr::MathCeil(operand) => {
            let v = lower_math_operand(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "llvm.ceil.f64", &[(DOUBLE, &v)]))
        }
        Expr::MathRound(operand) => {
            // JS Math.round: round-half-toward-positive-infinity. We
            // emulate via floor(x + 0.5) then fcopysign to preserve -0.
            let v = lower_math_operand(ctx, operand)?;
            let blk = ctx.block();
            let half = blk.fadd(&v, "0.5");
            let floored = blk.call(DOUBLE, "llvm.floor.f64", &[(DOUBLE, &half)]);
            Ok(blk.call(
                DOUBLE,
                "llvm.copysign.f64",
                &[(DOUBLE, &floored), (DOUBLE, &v)],
            ))
        }
        Expr::MathTrunc(operand) => {
            let v = lower_expr(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "js_math_trunc", &[(DOUBLE, &v)]))
        }
        Expr::MathSign(operand) => {
            let v = lower_expr(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "js_math_sign", &[(DOUBLE, &v)]))
        }
        Expr::MathAbs(operand) => {
            let v = lower_math_operand(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "llvm.fabs.f64", &[(DOUBLE, &v)]))
        }
        Expr::MathLog(operand) => {
            let v = lower_math_operand(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "llvm.log.f64", &[(DOUBLE, &v)]))
        }
        Expr::MathLog2(operand) => {
            let v = lower_math_operand(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "llvm.log2.f64", &[(DOUBLE, &v)]))
        }
        Expr::MathLog10(operand) => {
            let v = lower_math_operand(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "llvm.log10.f64", &[(DOUBLE, &v)]))
        }
        Expr::MathLog1p(operand) => {
            let v = lower_math_operand(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "js_math_log1p", &[(DOUBLE, &v)]))
        }
        // Math.random — return 0.5 sentinel. Real impl needs a PRNG
        // we'd link in; sentinel keeps the compile-pass count up.
        Expr::MathRandom => Ok(ctx.block().call(DOUBLE, "js_math_random", &[])),

        // ── WebAssembly host (issue #76) ──────────────────────────────
        // The runtime shims (perry-runtime/src/webassembly.rs) handle
        // bytes extraction, instance handles, and error reporting. The
        // wasmi engine itself is in the optional `perry-wasm-host`
        // crate, only linked when the user passes
        // `--enable-wasm-runtime`. Programs that never call these
        // builtins never reference the runtime shims, so the linker
        // dead-strips them and `perry_wasm_host_*` is never demanded.
        Expr::WebAssemblyValidate(bytes) => {
            let v = lower_expr(ctx, bytes)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_webassembly_validate", &[(DOUBLE, &v)]))
        }
        Expr::WebAssemblyCompile(bytes) => {
            let v = lower_expr(ctx, bytes)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_webassembly_compile", &[(DOUBLE, &v)]))
        }
        Expr::WebAssemblyModuleNew(bytes) => {
            let v = lower_expr(ctx, bytes)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_webassembly_module_new", &[(DOUBLE, &v)]))
        }
        Expr::WebAssemblyModuleExports(module) => {
            let v = lower_expr(ctx, module)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_webassembly_module_exports", &[(DOUBLE, &v)]))
        }
        Expr::WebAssemblyModuleImports(module) => {
            let v = lower_expr(ctx, module)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_webassembly_module_imports", &[(DOUBLE, &v)]))
        }
        Expr::WebAssemblyModuleCustomSections { module, name } => {
            let module_v = lower_expr(ctx, module)?;
            let name_v = lower_expr(ctx, name)?;
            Ok(ctx.block().call(
                DOUBLE,
                "js_webassembly_module_custom_sections",
                &[(DOUBLE, &module_v), (DOUBLE, &name_v)],
            ))
        }
        Expr::WebAssemblyInstantiate(bytes) => {
            let v = lower_expr(ctx, bytes)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_webassembly_instantiate", &[(DOUBLE, &v)]))
        }
        Expr::WebAssemblyCallExport {
            instance,
            name,
            args,
        } => {
            let inst = lower_expr(ctx, instance)?;
            let name_v = lower_expr(ctx, name)?;
            let lowered_args: Vec<String> = args
                .iter()
                .map(|a| lower_expr(ctx, a))
                .collect::<Result<Vec<_>>>()?;
            let blk = ctx.block();
            match lowered_args.len() {
                0 => Ok(blk.call(
                    DOUBLE,
                    "js_webassembly_call_export_0",
                    &[(DOUBLE, &inst), (DOUBLE, &name_v)],
                )),
                1 => Ok(blk.call(
                    DOUBLE,
                    "js_webassembly_call_export_1",
                    &[
                        (DOUBLE, &inst),
                        (DOUBLE, &name_v),
                        (DOUBLE, &lowered_args[0]),
                    ],
                )),
                2 => Ok(blk.call(
                    DOUBLE,
                    "js_webassembly_call_export_2",
                    &[
                        (DOUBLE, &inst),
                        (DOUBLE, &name_v),
                        (DOUBLE, &lowered_args[0]),
                        (DOUBLE, &lowered_args[1]),
                    ],
                )),
                3 => Ok(blk.call(
                    DOUBLE,
                    "js_webassembly_call_export_3",
                    &[
                        (DOUBLE, &inst),
                        (DOUBLE, &name_v),
                        (DOUBLE, &lowered_args[0]),
                        (DOUBLE, &lowered_args[1]),
                        (DOUBLE, &lowered_args[2]),
                    ],
                )),
                _ => Ok(blk.call(
                    DOUBLE,
                    "js_webassembly_call_export_4",
                    &[
                        (DOUBLE, &inst),
                        (DOUBLE, &name_v),
                        (DOUBLE, &lowered_args[0]),
                        (DOUBLE, &lowered_args[1]),
                        (DOUBLE, &lowered_args[2]),
                        (DOUBLE, &lowered_args[3]),
                    ],
                )),
            }
        }

        // `JSON.stringify(value, replacer, indent)` — full form via
        // runtime `js_json_stringify_full` which handles array/function
        // replacers, indent spaces, circular detection (throws
        // TypeError), and `toJSON`.
        Expr::JsonStringifyFull(value, replacer, indent) => {
            let v = lower_expr(ctx, value)?;
            let r = lower_expr(ctx, replacer)?;
            let i = lower_expr(ctx, indent)?;
            let blk = ctx.block();
            let result_i64 = blk.call(
                I64,
                "js_json_stringify_full",
                &[(DOUBLE, &v), (DOUBLE, &r), (DOUBLE, &i)],
            );
            Ok(blk.bitcast_i64_to_double(&result_i64))
        }

        // `new Map()` — alloc with default capacity 8 (the runtime grows
        // as needed). Result is NaN-boxed with POINTER_TAG.
        Expr::MapNew => {
            let cap = "8".to_string();
            let handle = ctx.block().call(I64, "js_map_alloc", &[(I32, &cap)]);
            Ok(nanbox_pointer_inline(ctx.block(), &handle))
        }

        // -------- Logical operators (Phase B.6) --------
        // `a && b` and `a || b` short-circuit. We compile `a` first, branch
        // on its truthiness (treating 0.0 as false / non-zero as true),
        // and either evaluate `b` or jump straight to the merge with `a`'s
        // value. The merge block uses a phi to pick the right result.
        // `??` (Coalesce) requires NaN-tag inspection (null/undefined
        // checks), so it lands in a later slice.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
