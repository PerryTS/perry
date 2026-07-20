//! DateNew..ClassRef precursor.
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
use crate::native_value::{
    layout_runtime_id, BoundsState, BufferAccessMode, LoweredValue, MaterializationReason,
    NativeRep, PodLayoutManifest, SemanticKind,
};
#[allow(unused_imports)]
use crate::type_analysis::{
    compute_auto_captures, is_array_expr, is_bigint_expr, is_bool_expr, is_map_expr,
    is_numeric_expr, is_set_expr, is_string_expr, is_url_search_params_expr, receiver_class_name,
};
#[allow(unused_imports)]
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

#[allow(unused_imports)]
use super::{
    buffer_access_materialization_reason, can_lower_expr_as_i32, emit_layout_note_slot_on_block,
    emit_root_nanbox_store_on_block, emit_shadow_slot_clear, emit_shadow_slot_update_for_expr,
    emit_string_literal_global, emit_v8_export_call, emit_v8_member_method_call,
    emit_write_barrier, emit_write_barrier_slot_on_block, expr_is_known_non_pointer_shadow_value,
    extract_array_of_object_shape, i32_bool_to_nanbox, import_origin_suffix, int_range_expr,
    is_global_this_builtin_function_name, is_global_this_builtin_name, is_known_finite,
    lower_array_literal, lower_buffer_load, lower_buffer_store, lower_channel_reduction,
    lower_expr, lower_expr_as_i32, lower_index_set_fast, lower_js_args_array, lower_object_literal,
    lower_stream_super_init, lower_url_string_getter, materialize_js_value, nanbox_bigint_inline,
    nanbox_pointer_inline, nanbox_pointer_inline_pub, nanbox_string_inline, proxy_build_args_array,
    try_flat_const_2d_int, try_lower_flat_const_index_get, try_match_channel_reduction,
    try_static_class_name, unbox_str_handle, unbox_to_i64, variant_name, BufferAccessSpec,
    ChannelReduction, FlatConstInfo, FnCtx, I18nLowerCtx,
};

fn lower_index_i32(ctx: &mut FnCtx<'_>, index: &Expr) -> Result<String> {
    if can_lower_expr_as_i32(
        index,
        &ctx.i32_counter_slots,
        ctx.flat_const_arrays,
        &ctx.array_row_aliases,
        ctx.integer_locals,
        ctx.clamp3_functions,
        ctx.clamp_u8_functions,
        ctx.integer_returning_functions,
        ctx.i32_identity_functions,
    ) {
        lower_expr_as_i32(ctx, index)
    } else {
        let i = lower_expr(ctx, index)?;
        Ok(ctx.block().fptosi(DOUBLE, &i, I32))
    }
}

fn numeric_index_has_integer_array_index_proof(ctx: &FnCtx<'_>, index: &Expr) -> bool {
    fn range_is_nonnegative_i32(ctx: &FnCtx<'_>, index: &Expr) -> bool {
        int_range_expr(ctx, index)
            .is_some_and(|range| range.min >= 0 && range.max <= i32::MAX as i64)
    }

    match index {
        Expr::Integer(i) => (0..=i32::MAX as i64).contains(i),
        Expr::Number(n) => n.is_finite() && n.fract() == 0.0 && *n >= 0.0 && *n <= i32::MAX as f64,
        Expr::Binary { op, left, right } if matches!(op, BinaryOp::BitAnd) => {
            fn mask(expr: &Expr) -> Option<i64> {
                match expr {
                    Expr::Integer(i) => Some(*i),
                    Expr::Number(n) if n.is_finite() && n.fract() == 0.0 => Some(*n as i64),
                    _ => None,
                }
            }
            mask(left)
                .or_else(|| mask(right))
                .is_some_and(|mask| (0..=i32::MAX as i64).contains(&mask))
        }
        Expr::LocalGet(id) => {
            ctx.integer_locals.contains(id)
                && ctx.i32_counter_slots.contains_key(id)
                && (ctx.nonnegative_integer_locals.contains(id)
                    || ctx
                        .int_range_facts
                        .iter()
                        .any(|fact| fact.local_id == *id && fact.range.min >= 0))
                || range_is_nonnegative_i32(ctx, index)
        }
        _ => range_is_nonnegative_i32(ctx, index),
    }
}

pub(crate) fn lower_uint8array_get_i32(
    ctx: &mut FnCtx<'_>,
    array: &Expr,
    index: &Expr,
) -> Result<LoweredValue> {
    if let Some(value) = lower_buffer_load(ctx, array, index, BufferAccessSpec::uint8array_get())? {
        return Ok(value);
    }

    // An UNPROVEN key may be a string at runtime: a Buffer is an ordinary
    // object in Node, so `buf[k]` with a non-numeric `k` reads a property (an
    // own expando, else the prototype method), not a byte. Coercing it to i32
    // read byte 0 and yielded `undefined`, which broke the ubiquitous
    // feature-probe `typeof obj[k] === "function"` — mysql2's `MockBuffer`
    // relies on it to neutralize a zero-length Buffer's write methods while
    // sizing each outgoing packet, so the MySQL handshake died with RangeError
    // [ERR_OUT_OF_RANGE]. Route to the polymorphic helper (it dispatches
    // numeric keys to the byte read and string keys to the property path); the
    // proven-numeric fast paths above are untouched.
    if !is_numeric_expr(ctx, index) {
        let a = lower_expr(ctx, array)?;
        let key = lower_expr(ctx, index)?;
        let blk = ctx.block();
        let handle = unbox_to_i64(blk, &a);
        let result = blk.call(
            DOUBLE,
            "js_object_get_index_polymorphic",
            &[(I64, &handle), (DOUBLE, &key)],
        );
        return Ok(LoweredValue::js_value(result));
    }

    let idx_i32 = lower_index_i32(ctx, index)?;
    let a = lower_expr(ctx, array)?;
    let blk = ctx.block();
    let handle = unbox_to_i64(blk, &a);
    let byte_i32 = blk.call(I32, "js_uint8array_get", &[(I64, &handle), (I32, &idx_i32)]);
    let slow = LoweredValue {
        semantic: SemanticKind::JsNumber,
        rep: NativeRep::I32,
        llvm_ty: I32,
        value: byte_i32,
    };
    ctx.record_lowered_value_with_access_mode(
        "Uint8ArrayGet",
        None,
        "Uint8ArrayGet.slow_path_i32",
        &slow,
        Some(BoundsState::Unknown),
        None,
        Some(BufferAccessMode::DynamicFallback),
        Some(buffer_access_materialization_reason(ctx, array)),
        false,
        false,
        Vec::new(),
    );
    Ok(slow)
}

pub(crate) fn lower_native_pod_view_with_layout(
    ctx: &mut FnCtx<'_>,
    owner: &Expr,
    byte_offset: &Expr,
    count: &Expr,
    layout: &PodLayoutManifest,
) -> Result<String> {
    let owner_value = lower_expr(ctx, owner)?;
    let byte_offset = lower_expr(ctx, byte_offset)?;
    let count = lower_expr(ctx, count)?;
    let blk = ctx.block();
    let owner_handle = unbox_to_i64(blk, &owner_value);
    let byte_offset_i64 = blk.fptosi(DOUBLE, &byte_offset, I64);
    let count_i64 = blk.fptosi(DOUBLE, &count, I64);
    let stride_i64 = layout.size.to_string();
    let alignment_i64 = layout.alignment.to_string();
    let layout_id = (layout_runtime_id(&layout.layout_id) as i64).to_string();
    let view = blk.call(
        I64,
        "js_native_pod_view",
        &[
            (I64, &owner_handle),
            (I64, &byte_offset_i64),
            (I64, &count_i64),
            (I64, &stride_i64),
            (I64, &alignment_i64),
            (I64, &layout_id),
        ],
    );
    Ok(nanbox_pointer_inline(blk, &view))
}

pub(crate) fn lower_native_pod_view(
    ctx: &mut FnCtx<'_>,
    owner: &Expr,
    byte_offset: &Expr,
    count: &Expr,
    expected_ty: Option<&HirType>,
    view_type: Option<&HirType>,
) -> Result<String> {
    if let Some(expected_ty) = expected_ty {
        match crate::native_value::layout_for_pod_view_type(ctx, expected_ty) {
            Ok(layout) => {
                return lower_native_pod_view_with_layout(ctx, owner, byte_offset, count, &layout);
            }
            Err(_)
                if view_type.is_some()
                    && matches!(expected_ty, HirType::Any | HirType::Unknown) => {}
            Err(reason) => {
                bail!(
                    "__perry_native_pod_view requires PerryPodView<T> where T resolves to PerryPod<...>: {}",
                    reason
                );
            }
        }
    }

    let Some(view_type) = view_type else {
        bail!("__perry_native_pod_view requires an explicit PerryPodView<T> type annotation");
    };
    let layout = crate::native_value::layout_for_pod_view_type(ctx, view_type).map_err(|reason| {
        anyhow!(
            "__perry_native_pod_view requires PerryPodView<T> where T resolves to PerryPod<...>: {}",
            reason
        )
    })?;
    lower_native_pod_view_with_layout(ctx, owner, byte_offset, count, &layout)
}

pub(crate) fn lower_buffer_index_get_i32(
    ctx: &mut FnCtx<'_>,
    buffer: &Expr,
    index: &Expr,
) -> Result<LoweredValue> {
    if let Some(value) =
        lower_buffer_load(ctx, buffer, index, BufferAccessSpec::buffer_index_get())?
    {
        return Ok(value);
    }

    let idx_i32 = lower_index_i32(ctx, index)?;
    let a = lower_expr(ctx, buffer)?;
    let blk = ctx.block();
    let handle = unbox_to_i64(blk, &a);
    let byte_i32 = blk.call(I32, "js_buffer_get", &[(I64, &handle), (I32, &idx_i32)]);
    let slow = LoweredValue {
        semantic: SemanticKind::JsNumber,
        rep: NativeRep::I32,
        llvm_ty: I32,
        value: byte_i32,
    };
    ctx.record_lowered_value_with_access_mode(
        "BufferIndexGet",
        None,
        "BufferIndexGet.slow_path_i32",
        &slow,
        Some(BoundsState::Unknown),
        None,
        Some(BufferAccessMode::DynamicFallback),
        Some(buffer_access_materialization_reason(ctx, buffer)),
        false,
        false,
        Vec::new(),
    );
    Ok(slow)
}

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::BoxedPrimitiveNew {
            kind,
            arg,
            arg_present,
        } => {
            let v = lower_expr(ctx, arg)?;
            match kind {
                perry_hir::BoxedPrimitiveKind::Number => {
                    Ok(ctx
                        .block()
                        .call(DOUBLE, "js_boxed_number_new", &[(DOUBLE, &v)]))
                }
                perry_hir::BoxedPrimitiveKind::Boolean => {
                    Ok(ctx
                        .block()
                        .call(DOUBLE, "js_boxed_boolean_new", &[(DOUBLE, &v)]))
                }
                // `has_arg` is an explicit compile-time flag, not an
                // `undefined`-value sentinel: a present argument that
                // evaluates to `undefined` at runtime (`new String(x)` where
                // `x` holds `undefined`) must still box to `"undefined"`, not
                // the no-arg `""` default — see the `arg_present` doc comment
                // on `Expr::BoxedPrimitiveNew`.
                perry_hir::BoxedPrimitiveKind::String => {
                    let has_arg = if *arg_present { "1" } else { "0" };
                    Ok(ctx.block().call(
                        DOUBLE,
                        "js_boxed_string_new",
                        &[(DOUBLE, &v), (I32, has_arg)],
                    ))
                }
            }
        }
        Expr::DateNew(args) => match args.len() {
            0 => Ok(ctx.block().call(DOUBLE, "js_date_new", &[])),
            1 => {
                let ts = lower_expr(ctx, &args[0])?;
                Ok(ctx
                    .block()
                    .call(DOUBLE, "js_date_new_from_value", &[(DOUBLE, &ts)]))
            }
            _ => {
                // Multi-arg constructor: `new Date(year, month, day?, hour?,
                // minute?, second?, ms?)` in local time. dayjs's parseDate
                // takes this branch with regex-captured string args — see
                // js_date_new_local_components for the coercion path.
                let mut vals: Vec<String> = Vec::with_capacity(7);
                for a in args.iter().take(7) {
                    vals.push(lower_expr(ctx, a)?);
                }
                // Pad *absent* trailing components with their ECMA-262 default
                // literal (slot 2 `day` → 1, time slots 3-6 → 0) rather than
                // `undefined`, so the runtime can run a plain ToNumber on every
                // forwarded slot: a *present* `undefined` then coerces to NaN
                // (Invalid Date), while a truly-omitted arg uses its default.
                // Slots: 0 year, 1 month, 2 day, 3 hour, 4 min, 5 sec, 6 ms.
                while vals.len() < 7 {
                    let default = if vals.len() == 2 { 1.0 } else { 0.0 };
                    vals.push(double_literal(default));
                }
                let blk = ctx.block();
                let call_args: Vec<(crate::types::LlvmType, &str)> =
                    vals.iter().map(|v| (DOUBLE, v.as_str())).collect();
                Ok(blk.call(DOUBLE, "js_date_new_local_components", &call_args))
            }
        },

        // -------- arr.find(cb) / findIndex(cb) / findLast(cb) / findLastIndex(cb) --------
        Expr::ArrayFind { array, callback } => {
            let arr_box = lower_expr(ctx, array)?;
            let cb_box = lower_expr(ctx, callback)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            // #4091: throw TypeError for a non-callable callback before iterating.
            let cb_handle = blk.call(I64, "js_validate_array_callback", &[(DOUBLE, &cb_box)]);
            Ok(blk.call(
                DOUBLE,
                "js_array_find",
                &[(I64, &arr_handle), (I64, &cb_handle)],
            ))
        }
        Expr::ArrayFindIndex { array, callback } => {
            let arr_box = lower_expr(ctx, array)?;
            let cb_box = lower_expr(ctx, callback)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            // #4091: throw TypeError for a non-callable callback before iterating.
            let cb_handle = blk.call(I64, "js_validate_array_callback", &[(DOUBLE, &cb_box)]);
            let i32_v = blk.call(
                I32,
                "js_array_findIndex",
                &[(I64, &arr_handle), (I64, &cb_handle)],
            );
            Ok(blk.sitofp(I32, &i32_v, DOUBLE))
        }
        Expr::ArrayFindLast { array, callback } => {
            let arr_box = lower_expr(ctx, array)?;
            let cb_box = lower_expr(ctx, callback)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            // #4091: throw TypeError for a non-callable callback before iterating.
            let cb_handle = blk.call(I64, "js_validate_array_callback", &[(DOUBLE, &cb_box)]);
            Ok(blk.call(
                DOUBLE,
                "js_array_find_last",
                &[(I64, &arr_handle), (I64, &cb_handle)],
            ))
        }
        Expr::ArrayFindLastIndex { array, callback } => {
            let arr_box = lower_expr(ctx, array)?;
            let cb_box = lower_expr(ctx, callback)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            // #4091: throw TypeError for a non-callable callback before iterating.
            let cb_handle = blk.call(I64, "js_validate_array_callback", &[(DOUBLE, &cb_box)]);
            let i32_v = blk.call(
                I32,
                "js_array_find_last_index",
                &[(I64, &arr_handle), (I64, &cb_handle)],
            );
            Ok(blk.sitofp(I32, &i32_v, DOUBLE))
        }

        // -------- Object.is, Number.isInteger, etc. --------
        Expr::ObjectIs(a, b) => {
            let av = lower_expr(ctx, a)?;
            let bv = lower_expr(ctx, b)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_object_is", &[(DOUBLE, &av), (DOUBLE, &bv)]))
        }
        Expr::NumberIsInteger(operand) => {
            // Runtime already returns NaN-tagged TAG_TRUE/TAG_FALSE.
            let v = lower_expr(ctx, operand)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_number_is_integer", &[(DOUBLE, &v)]))
        }

        // -------- Map.clear --------
        Expr::MapClear(map) => {
            let m_box = lower_expr(ctx, map)?;
            let blk = ctx.block();
            let m_handle = unbox_to_i64(blk, &m_box);
            blk.call_void("js_map_clear", &[(I64, &m_handle)]);
            // Map.prototype.clear() returns undefined, not 0.
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }

        // -------- Map.entries / Map.keys / Map.values --------
        // All three take a map pointer and return an array pointer.
        // Used by for...of desugaring on Maps.
        Expr::MapEntries(map) | Expr::MapKeys(map) | Expr::MapValues(map) => {
            let m_box = lower_expr(ctx, map)?;
            let blk = ctx.block();
            let m_handle = unbox_to_i64(blk, &m_box);
            let func_name = match expr {
                Expr::MapEntries(_) => "js_map_entries",
                Expr::MapKeys(_) => "js_map_keys",
                Expr::MapValues(_) => "js_map_values",
                _ => unreachable!(),
            };
            let result = blk.call(I64, func_name, &[(I64, &m_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }

        // -------- MapEntryKeyAt / MapEntryValueAt --------
        // Direct flat-array entry access — used by the
        // `for (const [k, v] of mapExpr)` fast path so the loop reads
        // entries straight out of the Map's internal buffer instead of
        // calling `js_map_entries` (which materializes N+1 small Arrays).
        Expr::MapEntryKeyAt { map, idx } | Expr::MapEntryValueAt { map, idx } => {
            let m_box = lower_expr(ctx, map)?;
            let i_dbl = lower_expr(ctx, idx)?;
            let blk = ctx.block();
            let m_handle = unbox_to_i64(blk, &m_box);
            let i_i32 = blk.fptosi(DOUBLE, &i_dbl, I32);
            let runtime_fn = match expr {
                Expr::MapEntryKeyAt { .. } => "js_map_entry_key_at",
                Expr::MapEntryValueAt { .. } => "js_map_entry_value_at",
                _ => unreachable!(),
            };
            Ok(blk.call(DOUBLE, runtime_fn, &[(I64, &m_handle), (I32, &i_i32)]))
        }

        // -------- Set direct-element fast path --------
        // Counterpart to MapEntryValueAt: read the i-th element of a Set
        // without materializing the buffer into an Array. Used by the
        // `for (const x of setExpr)` HIR fast path.
        Expr::SetValueAt { set, idx } => {
            let s_box = lower_expr(ctx, set)?;
            let i_dbl = lower_expr(ctx, idx)?;
            let blk = ctx.block();
            let s_handle = unbox_to_i64(blk, &s_box);
            let i_i32 = blk.fptosi(DOUBLE, &i_dbl, I32);
            Ok(blk.call(
                DOUBLE,
                "js_set_value_at",
                &[(I64, &s_handle), (I32, &i_i32)],
            ))
        }

        // -------- Set.values (set → array conversion for iteration) --------
        Expr::SetValues(set) => {
            let s_box = lower_expr(ctx, set)?;
            let blk = ctx.block();
            let s_handle = unbox_to_i64(blk, &s_box);
            let result = blk.call(I64, "js_set_to_array", &[(I64, &s_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }

        // -------- Object.isFrozen / isSealed / isExtensible --------
        // Runtime returns f64 already NaN-boxed as TAG_TRUE/TAG_FALSE.
        Expr::ObjectIsFrozen(o) => {
            let v = lower_expr(ctx, o)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_object_is_frozen", &[(DOUBLE, &v)]))
        }
        Expr::ObjectIsSealed(o) => {
            let v = lower_expr(ctx, o)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_object_is_sealed", &[(DOUBLE, &v)]))
        }
        Expr::ObjectIsExtensible(o) => {
            let v = lower_expr(ctx, o)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_object_is_extensible", &[(DOUBLE, &v)]))
        }

        // -------- FuncRef as expression value (function reference) --------
        // When a user function is passed as a value (e.g. `apply(add,
        // 3, 4)`), wrap it in a heap closure so the receiver can call
        // it via `js_closure_callN`. The wrapper function
        // `__perry_wrap_<name>` is emitted by `compile_module` for
        // every user function and has the closure-call ABI: it takes
        // `(closure_ptr, arg0, arg1, ...)` and forwards to the
        // underlying function.
        Expr::FuncRef(id) => {
            let func_name = ctx
                .func_names
                .get(id)
                .cloned()
                .unwrap_or_else(|| "perry_unknown_func".to_string());
            let wrap_name = format!("__perry_wrap_{}", func_name);
            let blk = ctx.block();
            let wrap_ptr = format!("@{}", wrap_name);
            // FuncRef wrappers always have 0 captures, so we can route
            // through the singleton-cached allocator: same func_ptr always
            // yields the same ClosureHeader. Eliminates the per-evaluation
            // gc_malloc + gc_check_trigger that was the dominant cost in
            // tight loops which pass a function as a callback.
            let closure_handle = blk.call(I64, "js_closure_alloc_singleton", &[(PTR, &wrap_ptr)]);
            Ok(nanbox_pointer_inline(blk, &closure_handle))
        }

        // -------- path.extname(p) -> string --------
        Expr::PathExtname(p) => {
            let p_box = lower_expr(ctx, p)?;
            let blk = ctx.block();
            let p_handle = unbox_to_i64(blk, &p_box);
            let result = blk.call(I64, "js_path_extname", &[(I64, &p_handle)]);
            Ok(nanbox_string_inline(blk, &result))
        }
        // -------- path.sep / path.delimiter constants --------
        Expr::PathSep => {
            let blk = ctx.block();
            let h = blk.call(I64, "js_path_sep_get", &[]);
            Ok(nanbox_string_inline(blk, &h))
        }
        Expr::PathDelimiter => {
            let blk = ctx.block();
            let h = blk.call(I64, "js_path_delimiter_get", &[]);
            Ok(nanbox_string_inline(blk, &h))
        }
        Expr::PathFormat(o) => {
            let obj_box = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let result = blk.call(I64, "js_path_format", &[(DOUBLE, &obj_box)]);
            Ok(nanbox_string_inline(blk, &result))
        }
        Expr::PathToNamespacedPath(p) => {
            let p_box = lower_expr(ctx, p)?;
            let blk = ctx.block();
            Ok(blk.call(
                DOUBLE,
                "js_path_to_namespaced_path_value",
                &[(DOUBLE, &p_box)],
            ))
        }
        Expr::PathMatchesGlob(p, pat) => {
            let p_box = lower_expr(ctx, p)?;
            let pat_box = lower_expr(ctx, pat)?;
            let blk = ctx.block();
            let p_handle = unbox_to_i64(blk, &p_box);
            let pat_handle = unbox_to_i64(blk, &pat_box);
            let i32_v = blk.call(
                I32,
                "js_path_matches_glob",
                &[(I64, &p_handle), (I64, &pat_handle)],
            );
            Ok(i32_bool_to_nanbox(blk, &i32_v))
        }
        Expr::PathResolveJoin(a, b) => {
            let a_box = lower_expr(ctx, a)?;
            let b_box = lower_expr(ctx, b)?;
            let blk = ctx.block();
            let a_handle = unbox_to_i64(blk, &a_box);
            let b_handle = unbox_to_i64(blk, &b_box);
            let result = blk.call(
                I64,
                "js_path_resolve_join",
                &[(I64, &a_handle), (I64, &b_handle)],
            );
            Ok(nanbox_string_inline(blk, &result))
        }
        Expr::ProcessVersion => {
            let blk = ctx.block();
            let handle = blk.call(I64, "js_process_version", &[]);
            Ok(nanbox_string_inline(blk, &handle))
        }
        Expr::ObjectHasOwn(obj, key) => {
            let obj_box = lower_expr(ctx, obj)?;
            let key_box = lower_expr(ctx, key)?;
            Ok(ctx.block().call(
                DOUBLE,
                "js_object_has_own",
                &[(DOUBLE, &obj_box), (DOUBLE, &key_box)],
            ))
        }
        Expr::NumberIsNaN(operand) => {
            // Number.isNaN is strict: only returns true for actual
            // NaN values, NOT for NaN-tagged strings/pointers/bools.
            // The inline fcmp("uno",x,x) would return true for any
            // NaN-tagged value. Use the runtime which checks
            // is_number() first.
            let v = lower_expr(ctx, operand)?;
            // #853: the runtime fcmp inline pattern that used to follow
            // was kept as a reference; it was unreachable code after the
            // early return above. Removed — the comment block immediately
            // above this arm documents why the runtime call is required.
            Ok(ctx
                .block()
                .call(DOUBLE, "js_number_is_nan", &[(DOUBLE, &v)]))
        }
        Expr::FsMkdirSync(p) => {
            // Phase H fs: call js_fs_mkdir_sync. Node's fs.mkdirSync
            // is void so we discard the i32 status.
            let path_box = lower_expr(ctx, p)?;
            let _ = ctx
                .block()
                .call(I32, "js_fs_mkdir_sync", &[(DOUBLE, &path_box)]);
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }
        Expr::IteratorToArray(o) => {
            // Walk the iterator protocol: call .next() in a loop, collect .value entries
            // into a fresh array. Runtime returns the raw ArrayHeader pointer, we re-NaN-box
            // so callers that expect an array-valued NaN-box work correctly.
            let iter_box = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let arr_ptr = blk.call(I64, "js_iterator_to_array", &[(DOUBLE, &iter_box)]);
            Ok(nanbox_pointer_inline(blk, &arr_ptr))
        }
        Expr::GetIterator(o) => {
            // #1831: `yield*` iterator resolution. `js_get_iterator` returns the
            // operand's `Symbol.iterator` result when iterable (effect, custom
            // iterables) or the operand unchanged when it is already an iterator
            // (a perry generator object). Returns a NaN-boxed JSValue directly.
            let v = lower_expr(ctx, o)?;
            Ok(ctx.block().call(DOUBLE, "js_get_iterator", &[(DOUBLE, &v)]))
        }
        Expr::GetAsyncIterator(o) => {
            let v = lower_expr(ctx, o)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_get_async_iterator", &[(DOUBLE, &v)]))
        }
        Expr::ForOfToArray(o) => {
            // #321: materialize an untyped `for...of` receiver into a plain
            // Array. Runtime inspects the value's GC kind (Map → [k,v]
            // pairs, Set → values, Array → itself, string → chars, else
            // drive `[Symbol.iterator]`) and returns a NaN-boxed array
            // JSValue the index loop can read via `.length` / `arr[i]`.
            let v = lower_expr(ctx, o)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_for_of_to_array", &[(DOUBLE, &v)]))
        }
        Expr::ForAwaitToArray(o) => {
            let v = lower_expr(ctx, o)?;
            let undefined = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            Ok(ctx.block().call(
                DOUBLE,
                "js_array_from_async",
                &[(DOUBLE, &v), (DOUBLE, &undefined), (DOUBLE, &undefined)],
            ))
        }
        Expr::WeakRefDeref(o) => {
            // `ref.deref()` — returns the wrapped target (or undefined if
            // collected; GC never clears the stub slot, so always returns
            // the target). Runtime reads the `target` field from the WeakRef
            // wrapper object and returns its stored NaN-boxed value, so
            // downstream paths (`.length`, method dispatch) see the real
            // tagged pointer again.
            let v = lower_expr(ctx, o)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_weakref_deref", &[(DOUBLE, &v)]))
        }
        // `new Uint8Array([1, 2, 3])` — materialize an Array<number>
        // and convert to a BufferHeader via js_buffer_from_array so
        // `TextDecoder.decode(new Uint8Array([...]))` works and
        // `encoder.encode(...)` result can be used interchangeably.
        Expr::Uint8ArrayNew(arg) => {
            // `new Uint8Array(arg)` has three forms:
            //   - `new Uint8Array()` → empty buffer (length 0)
            //   - `new Uint8Array(N)` where N is a number → zero-filled buffer of length N
            //   - `new Uint8Array([1, 2, 3])` → buffer initialized from array
            // The codegen detects the literal-number case at compile time and routes
            // it to `js_uint8array_alloc` so we don't read garbage from a
            // number-as-array while still preserving Uint8Array identity.
            // Other shapes flow through `js_uint8array_new`, which dispatches
            // between numeric lengths and source arrays at runtime.
            match arg.as_deref() {
                None => {
                    let blk = ctx.block();
                    let h = blk.call(I64, "js_uint8array_alloc", &[(I32, "0")]);
                    Ok(nanbox_pointer_inline(blk, &h))
                }
                // Only take the i32 fast path when the literal fits in an i32;
                // a larger length (`new Uint8Array(1e15)`) would truncate via
                // `*n as i32`, so fall through to the runtime f64 path which
                // throws `RangeError: Array buffer allocation failed` (#5067).
                Some(Expr::Integer(n)) if *n >= i32::MIN as i64 && *n <= i32::MAX as i64 => {
                    let size_str = (*n as i32).to_string();
                    let blk = ctx.block();
                    let h = blk.call(I64, "js_uint8array_alloc", &[(I32, &size_str)]);
                    Ok(nanbox_pointer_inline(blk, &h))
                }
                Some(Expr::Number(n))
                    if n.fract() == 0.0 && *n >= 0.0 && *n < (i32::MAX as f64) =>
                {
                    let size_str = (*n as i32).to_string();
                    let blk = ctx.block();
                    let h = blk.call(I64, "js_uint8array_alloc", &[(I32, &size_str)]);
                    Ok(nanbox_pointer_inline(blk, &h))
                }
                Some(e) => {
                    // Non-literal case: `new Uint8Array(x)` where x is a
                    // variable/expression. At codegen time we can't tell if
                    // x is a number (length) or an array (source data), so
                    // dispatch at runtime via `js_uint8array_new` which
                    // inspects the NaN-box tag. Prior to this fix the catch-
                    // all always called `js_uint8array_from_array`, which
                    // treated numeric lengths as ArrayHeader pointers and
                    // silently returned a zero-length buffer (closes #38).
                    let val_box = lower_expr(ctx, e)?;
                    let blk = ctx.block();
                    let buf_handle = blk.call(I64, "js_uint8array_new", &[(DOUBLE, &val_box)]);
                    Ok(nanbox_pointer_inline(blk, &buf_handle))
                }
            }
        }
        Expr::Uint8ArrayLength(arr) => {
            let v = lower_expr(ctx, arr)?;
            let blk = ctx.block();
            let handle = unbox_to_i64(blk, &v);
            let len_i32 = blk.call(I32, "js_buffer_length", &[(I64, &handle)]);
            let lowered = LoweredValue::buffer_len(len_i32);
            ctx.record_lowered_value(
                "Uint8ArrayLength",
                None,
                "Uint8ArrayLength.native_buffer_len",
                &lowered,
                None,
                None,
                None,
                false,
                false,
                Vec::new(),
            );
            Ok(materialize_js_value(
                ctx,
                lowered,
                MaterializationReason::FunctionAbi,
            ))
        }
        Expr::Uint8ArrayGet { array, index } => {
            // A symbol-keyed read (`u8[Symbol.toStringTag]`,
            // `u8[Symbol.iterator]`) must resolve through the symbol-property
            // path, which exposes the `%TypedArray%.prototype` symbol accessors
            // (`@@toStringTag` — used by `safe-stable-stringify` — and
            // `@@iterator`). The dynamic-index helper below stringifies the key
            // and would miss them.
            if matches!(index.as_ref(), Expr::SymbolFor(_)) {
                let a = lower_expr(ctx, array)?;
                let key = lower_expr(ctx, index)?;
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_object_get_symbol_property",
                    &[(DOUBLE, &a), (DOUBLE, &key)],
                ));
            }
            if let Some(value) =
                lower_buffer_load(ctx, array, index, BufferAccessSpec::uint8array_get())?
            {
                let reason = buffer_access_materialization_reason(ctx, array);
                return Ok(materialize_js_value(ctx, value, reason));
            }
            if !numeric_index_has_integer_array_index_proof(ctx, index) {
                let a = lower_expr(ctx, array)?;
                let key = lower_expr(ctx, index)?;
                let blk = ctx.block();
                let handle = unbox_to_i64(blk, &a);
                return Ok(blk.call(
                    DOUBLE,
                    "js_typed_array_index_get_dynamic",
                    &[(I64, &handle), (DOUBLE, &key)],
                ));
            }
            // #6088: a proven non-negative integer key whose value is NOT
            // proven in bounds (the inline load above bailed). The native i32
            // accessor returns the `0` byte-sentinel for an out-of-range read;
            // a JS-value `u8[i]` must instead read `undefined` (ECMAScript
            // IntegerIndexedExotic `[[Get]]`). In-range reads still return the
            // byte as a number.
            let a = lower_expr(ctx, array)?;
            let idx_i32 = lower_index_i32(ctx, index)?;
            let blk = ctx.block();
            let handle = unbox_to_i64(blk, &a);
            Ok(blk.call(
                DOUBLE,
                "js_uint8array_index_get_value",
                &[(I64, &handle), (I32, &idx_i32)],
            ))
        }
        Expr::BufferIndexGet { buffer, index } => {
            // Proven-bounds inline load keeps the native fast path.
            if let Some(value) =
                lower_buffer_load(ctx, buffer, index, BufferAccessSpec::buffer_index_get())?
            {
                let reason = buffer_access_materialization_reason(ctx, buffer);
                return Ok(materialize_js_value(ctx, value, reason));
            }
            // #6088: out-of-range → `undefined`, not the `0` byte-sentinel the
            // native `js_buffer_get` accessor is forced to return.
            let a = lower_expr(ctx, buffer)?;
            let idx_i32 = lower_index_i32(ctx, index)?;
            let blk = ctx.block();
            let handle = unbox_to_i64(blk, &a);
            Ok(blk.call(
                DOUBLE,
                "js_buffer_index_get_value",
                &[(I64, &handle), (I32, &idx_i32)],
            ))
        }
        Expr::Uint8ArraySet {
            array,
            index,
            value,
        } => {
            if let Some(store) =
                lower_buffer_store(ctx, array, index, value, BufferAccessSpec::uint8array_set())?
            {
                if ctx.discard_expr_value {
                    return Ok(double_literal(0.0));
                }
                return Ok(materialize_js_value(
                    ctx,
                    store.result,
                    MaterializationReason::FunctionAbi,
                ));
            }
            if !numeric_index_has_integer_array_index_proof(ctx, index) {
                // A non-numeric key stores an OWN property (Node's Buffer is an
                // ordinary object, and an own key shadows the same-named
                // prototype method — mysql2's `MockBuffer` overwrites the write
                // methods of a zero-length Buffer to size a packet). The
                // typed-array helper coerces the key to a number and dropped the
                // store; the polymorphic setter dispatches numeric keys to the
                // byte write and string keys to the own-prop table.
                let key_maybe_string = !is_numeric_expr(ctx, index);
                let a = lower_expr(ctx, array)?;
                let key = lower_expr(ctx, index)?;
                let val = lower_expr(ctx, value)?;
                let blk = ctx.block();
                let handle = unbox_to_i64(blk, &a);
                let result = if key_maybe_string {
                    blk.call_void(
                        "js_object_set_index_polymorphic",
                        &[(I64, &handle), (DOUBLE, &key), (DOUBLE, &val)],
                    );
                    val.clone()
                } else {
                    blk.call(
                        DOUBLE,
                        "js_typed_array_index_set_dynamic",
                        &[(I64, &handle), (DOUBLE, &key), (DOUBLE, &val)],
                    )
                };
                if ctx.discard_expr_value {
                    return Ok(double_literal(0.0));
                }
                return Ok(result);
            }

            let idx_is_i32 = can_lower_expr_as_i32(
                index,
                &ctx.i32_counter_slots,
                ctx.flat_const_arrays,
                &ctx.array_row_aliases,
                ctx.integer_locals,
                ctx.clamp3_functions,
                ctx.clamp_u8_functions,
                ctx.integer_returning_functions,
                ctx.i32_identity_functions,
            );
            let val_is_i32 = can_lower_expr_as_i32(
                value,
                &ctx.i32_counter_slots,
                ctx.flat_const_arrays,
                &ctx.array_row_aliases,
                ctx.integer_locals,
                ctx.clamp3_functions,
                ctx.clamp_u8_functions,
                ctx.integer_returning_functions,
                ctx.i32_identity_functions,
            );
            let idx_i32 = if idx_is_i32 {
                lower_expr_as_i32(ctx, index)?
            } else {
                let i = lower_expr(ctx, index)?;
                ctx.block().fptosi(DOUBLE, &i, I32)
            };
            let val_i32 = if val_is_i32 {
                lower_expr_as_i32(ctx, value)?
            } else {
                let v = lower_expr(ctx, value)?;
                ctx.block().fptosi(DOUBLE, &v, I32)
            };
            // Slow path accepts either BufferHeader-backed Uint8Arrays or
            // NativeArena typed views.
            let a = lower_expr(ctx, array)?;
            let blk = ctx.block();
            let handle = unbox_to_i64(blk, &a);
            blk.call_void(
                "js_uint8array_set",
                &[(I64, &handle), (I32, &idx_i32), (I32, &val_i32)],
            );
            let reason = buffer_access_materialization_reason(ctx, array);
            let slow = LoweredValue {
                semantic: SemanticKind::JsNumber,
                rep: NativeRep::I32,
                llvm_ty: I32,
                value: val_i32.clone(),
            };
            ctx.record_lowered_value_with_access_mode(
                "Uint8ArraySet",
                None,
                "Uint8ArraySet.slow_path",
                &slow,
                Some(BoundsState::Unknown),
                None,
                Some(BufferAccessMode::DynamicFallback),
                Some(reason.clone()),
                false,
                false,
                Vec::new(),
            );
            if ctx.discard_expr_value {
                return Ok(double_literal(0.0));
            }
            Ok(materialize_js_value(ctx, slow, reason))
        }
        Expr::BufferIndexSet {
            buffer,
            index,
            value,
        } => {
            if let Some(store) = lower_buffer_store(
                ctx,
                buffer,
                index,
                value,
                BufferAccessSpec::buffer_index_set(),
            )? {
                if ctx.discard_expr_value {
                    return Ok(double_literal(0.0));
                }
                return Ok(materialize_js_value(
                    ctx,
                    store.result,
                    MaterializationReason::FunctionAbi,
                ));
            }

            let idx_is_i32 = can_lower_expr_as_i32(
                index,
                &ctx.i32_counter_slots,
                ctx.flat_const_arrays,
                &ctx.array_row_aliases,
                ctx.integer_locals,
                ctx.clamp3_functions,
                ctx.clamp_u8_functions,
                ctx.integer_returning_functions,
                ctx.i32_identity_functions,
            );
            let val_is_i32 = can_lower_expr_as_i32(
                value,
                &ctx.i32_counter_slots,
                ctx.flat_const_arrays,
                &ctx.array_row_aliases,
                ctx.integer_locals,
                ctx.clamp3_functions,
                ctx.clamp_u8_functions,
                ctx.integer_returning_functions,
                ctx.i32_identity_functions,
            );
            let idx_i32 = if idx_is_i32 {
                lower_expr_as_i32(ctx, index)?
            } else {
                let i = lower_expr(ctx, index)?;
                ctx.block().fptosi(DOUBLE, &i, I32)
            };
            let val_i32 = if val_is_i32 {
                lower_expr_as_i32(ctx, value)?
            } else {
                let v = lower_expr(ctx, value)?;
                ctx.block().fptosi(DOUBLE, &v, I32)
            };
            let a = lower_expr(ctx, buffer)?;
            let blk = ctx.block();
            let handle = unbox_to_i64(blk, &a);
            blk.call_void(
                "js_buffer_set",
                &[(I64, &handle), (I32, &idx_i32), (I32, &val_i32)],
            );
            let reason = buffer_access_materialization_reason(ctx, buffer);
            let slow = LoweredValue {
                semantic: SemanticKind::JsNumber,
                rep: NativeRep::I32,
                llvm_ty: I32,
                value: val_i32.clone(),
            };
            ctx.record_lowered_value_with_access_mode(
                "BufferIndexSet",
                None,
                "BufferIndexSet.slow_path",
                &slow,
                Some(BoundsState::Unknown),
                None,
                Some(BufferAccessMode::DynamicFallback),
                Some(reason.clone()),
                false,
                false,
                Vec::new(),
            );
            if ctx.discard_expr_value {
                return Ok(double_literal(0.0));
            }
            Ok(materialize_js_value(ctx, slow, reason))
        }

        // `new Int32Array([1,2,3])` etc. — generic typed array constructor.
        // Routes through `js_typed_array_new_empty(kind, length)` for
        // compile-time-constant numeric lengths, or `js_typed_array_new(kind, val)`
        // for runtime-dispatched arguments (which inspects the NaN-box tag to
        // distinguish a numeric length from a source-array pointer).
        // Result is a normal POINTER_TAG JS value. Element/property fast paths
        // mask off the tag before consulting TYPED_ARRAY_REGISTRY, and runtime
        // consumers such as Atomics require the value to satisfy is_pointer().
        Expr::TypedArrayNew { kind, arg } => {
            let kind_str = (*kind as i32).to_string();
            match arg {
                None => {
                    let zero = "0".to_string();
                    let p = ctx.block().call(
                        I64,
                        "js_typed_array_new_empty",
                        &[(I32, &kind_str), (I32, &zero)],
                    );
                    Ok(nanbox_pointer_inline(ctx.block(), &p))
                }
                Some(arg_expr) => match arg_expr.as_ref() {
                    // Literal integer length: `new Int32Array(3)`. A negative
                    // literal (`new Int32Array(-1)`) is passed through verbatim
                    // so the runtime throws the spec RangeError (#3662).
                    // Only take the i32 fast path when the literal actually fits
                    // in an i32 — otherwise `*n as i32` would silently truncate
                    // a huge length (`new Uint8Array(1e15)` → a bogus -1.5e9),
                    // so fall through to the f64 runtime path which throws the
                    // proper `RangeError: Array buffer allocation failed` (#5067).
                    Expr::Integer(n) if *n >= i32::MIN as i64 && *n <= i32::MAX as i64 => {
                        let len_str = (*n as i32).to_string();
                        let p = ctx.block().call(
                            I64,
                            "js_typed_array_new_empty",
                            &[(I32, &kind_str), (I32, &len_str)],
                        );
                        Ok(nanbox_pointer_inline(ctx.block(), &p))
                    }
                    // Literal float that is a non-negative integer: `new Int32Array(3.0)`.
                    Expr::Number(f) if f.fract() == 0.0 && *f >= 0.0 && *f < (i32::MAX as f64) => {
                        let len_str = (*f as i32).to_string();
                        let p = ctx.block().call(
                            I64,
                            "js_typed_array_new_empty",
                            &[(I32, &kind_str), (I32, &len_str)],
                        );
                        Ok(nanbox_pointer_inline(ctx.block(), &p))
                    }
                    // Non-literal: dispatch at runtime based on the NaN-box tag.
                    // `js_typed_array_new` detects POINTER_TAG → copy from array,
                    // INT32_TAG / plain double → use as length.
                    _ => {
                        let val_box = lower_expr(ctx, arg_expr)?;
                        let blk = ctx.block();
                        let p = blk.call(
                            I64,
                            "js_typed_array_new",
                            &[(I32, &kind_str), (DOUBLE, &val_box)],
                        );
                        Ok(nanbox_pointer_inline(blk, &p))
                    }
                },
            }
        }

        Expr::NativeArenaAlloc(byte_length) => {
            let byte_length = lower_expr(ctx, byte_length)?;
            let byte_length_i64 = ctx.block().fptosi(DOUBLE, &byte_length, I64);
            let owner = ctx
                .block()
                .call(I64, "js_native_arena_alloc", &[(I64, &byte_length_i64)]);
            Ok(nanbox_pointer_inline(ctx.block(), &owner))
        }

        Expr::NativeArenaView {
            owner,
            kind,
            byte_offset,
            length,
        } => {
            let owner_value = lower_expr(ctx, owner)?;
            let byte_offset = lower_expr(ctx, byte_offset)?;
            let length = lower_expr(ctx, length)?;
            let blk = ctx.block();
            let owner_handle = unbox_to_i64(blk, &owner_value);
            let kind_i32 = (*kind as i32).to_string();
            let byte_offset_i64 = blk.fptosi(DOUBLE, &byte_offset, I64);
            let length_i64 = blk.fptosi(DOUBLE, &length, I64);
            let view = blk.call(
                I64,
                "js_native_arena_view",
                &[
                    (I64, &owner_handle),
                    (I32, &kind_i32),
                    (I64, &byte_offset_i64),
                    (I64, &length_i64),
                ],
            );
            Ok(nanbox_pointer_inline(blk, &view))
        }

        Expr::NativePodView {
            owner,
            byte_offset,
            count,
            view_type,
        } => lower_native_pod_view(ctx, owner, byte_offset, count, None, view_type.as_ref()),

        Expr::NativeArenaDispose(owner) => {
            let owner_value = lower_expr(ctx, owner)?;
            let blk = ctx.block();
            let owner_handle = unbox_to_i64(blk, &owner_value);
            blk.call_void("js_native_arena_dispose", &[(I64, &owner_handle)]);
            super::invalidate_native_owned_views_for_dispose(ctx, owner);
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }

        // -------- arr.unshift(value) --------
        // Issue #656: returns the new length per ECMA-262, not the (possibly
        // reallocated) array pointer. The runtime helper returns the new
        // header pointer for writeback purposes; we still need that to
        // update the local/capture/global slot, but the call's *value* is
        // the array length read from the new header.
        Expr::ArrayUnshift { array_id, value } => {
            let v = lower_expr(ctx, value)?;
            let arr_box = lower_expr(ctx, &Expr::LocalGet(*array_id))?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let new_handle = blk.call(
                I64,
                "js_array_unshift_f64",
                &[(I64, &arr_handle), (DOUBLE, &v)],
            );
            let new_box = nanbox_pointer_inline(blk, &new_handle);
            // Write back the (possibly reallocated) head to the receiver's
            // storage. #6229: this previously handled closure-capture / local /
            // global but NOT the boxed-var case, so a growing single-arg
            // `unshift` on a boxed async local overwrote the slot's box pointer
            // with the array pointer, and the next read decoded the array as a
            // box → `undefined`. Route through the shared boxed-aware writeback.
            crate::lower_array_method::emit_grow_mutator_writeback(ctx, *array_id, &new_box)?;
            let blk = ctx.block();
            let len_i32 = blk.call(I32, "js_array_length", &[(I64, &new_handle)]);
            let len_f64 = blk.sitofp(I32, &len_i32, DOUBLE);
            Ok(len_f64)
        }

        // -------- arr.entries() / .keys() / .values() --------
        // #2384: build a real `.next()`-bearing iterator OBJECT (not an eager
        // materialized array) so `const e = arr.entries(); e.next().value`
        // matches Node. Spread / for-of / Array.from already detect the
        // iterator class id (`js_array_clone`, `js_for_of_to_array`) and drive
        // `.next()`, so those paths keep working.
        Expr::ArrayEntries(arr) => {
            let arr_box = lower_expr(ctx, arr)?;
            let blk = ctx.block();
            // Full bits, not the 48-bit mask: the runtime router classifies
            // non-pointer receivers (Web Streams handle ids are plain
            // doubles whose masked bits look like heap addresses).
            let arr_handle = blk.bitcast_double_to_i64(&arr_box);
            let result = blk.call(I64, "js_array_entries_iter_obj", &[(I64, &arr_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }
        Expr::ArrayKeys(arr) => {
            let arr_box = lower_expr(ctx, arr)?;
            let blk = ctx.block();
            let arr_handle = blk.bitcast_double_to_i64(&arr_box);
            let result = blk.call(I64, "js_array_keys_iter_obj", &[(I64, &arr_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }
        Expr::ArrayValues(arr) => {
            let arr_box = lower_expr(ctx, arr)?;
            let blk = ctx.block();
            let arr_handle = blk.bitcast_double_to_i64(&arr_box);
            let result = blk.call(I64, "js_array_values_iter_obj", &[(I64, &arr_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }

        // -------- ClassRef --------
        // Lower to the class's runtime id NaN-boxed with INT32_TAG. The
        // runtime distinguishes class refs from other values via the tag,
        // letting `Object.prototype.hasOwnProperty.call(SomeClass, sym)`
        // route through the class-static-symbol side table for drizzle's
        // `is(value, type)`. Falls back to `0.0` when the class isn't in
        // class_ids (legacy callers checking truthiness). Refs #420.
        Expr::ClassRef(name) => {
            if let Some(&cid) = ctx.class_ids.get(name) {
                let bits = crate::nanbox::INT32_TAG | (cid as u64 & 0xFFFF_FFFF);
                Ok(double_literal(f64::from_bits(bits)))
            } else {
                Ok(double_literal(0.0))
            }
        }

        // -------- CallSpread: function call with spread arguments --------
        // The common shape is `fn(...args)` — single spread, no regular
        // args, callee is a known FuncRef whose declared param count we
        // can read. Lower the spread source as an array, then extract
        // expected_count elements via `js_array_get_f64` and call the
        // function with the unpacked args.
        //
        // For unsupported shapes (multiple spread args, mixed regular
        // + spread, non-FuncRef callees, unknown signature) we fall
        // through to the previous stub behavior so the program at
        // least compiles. Those cases need their own follow-up.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
