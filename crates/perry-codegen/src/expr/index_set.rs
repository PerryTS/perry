//! IndexSet (arr[i] = v).
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
use crate::native_value::{
    BoundsState, BufferAccessMode, ExpectedNativeRep, LoweredValue, MaterializationReason,
    NativeRep, SemanticKind,
};
#[allow(unused_imports)]
use crate::type_analysis::{
    compute_auto_captures, is_array_expr, is_bigint_expr, is_bool_expr, is_map_expr,
    is_numeric_expr, is_set_expr, is_string_expr, is_url_search_params_expr, receiver_class_name,
};
#[allow(unused_imports)]
use crate::types::{DOUBLE, F32, I1, I16, I32, I64, I8, PTR};

#[allow(unused_imports)]
use super::{
    array_store_needs_layout_note, array_store_needs_write_barrier,
    buffer_access_materialization_reason, buffer_alias_metadata_suffix,
    emit_array_numeric_write_note_on_block, emit_jsvalue_slot_store_on_block,
    emit_layout_note_slot_on_block, emit_root_nanbox_store_on_block, emit_shadow_slot_clear,
    emit_shadow_slot_update_for_expr, emit_string_literal_global,
    emit_typed_feedback_register_site, emit_v8_export_call, emit_v8_member_method_call,
    emit_write_barrier, emit_write_barrier_slot_on_block,
    expr_has_numeric_pointer_free_array_layout, expr_is_known_non_pointer_shadow_value,
    extract_array_of_object_shape, i32_bool_to_nanbox, import_origin_suffix,
    is_global_this_builtin_function_name, is_global_this_builtin_name, is_known_finite,
    lower_array_literal, lower_channel_reduction, lower_expr, lower_expr_as_i32, lower_expr_native,
    lower_index_set_fast, lower_js_args_array, lower_object_literal, lower_stream_super_init,
    lower_typed_array_store, lower_url_string_getter, materialize_js_value, nanbox_bigint_inline,
    nanbox_pointer_inline, nanbox_pointer_inline_pub, nanbox_string_inline, proxy_build_args_array,
    raw_f64_layout_fact, try_flat_const_2d_int, try_lower_flat_const_index_get,
    try_match_channel_reduction, try_static_class_name, unbox_str_handle, unbox_to_i64,
    variant_name, ChannelReduction, FlatConstInfo, FnCtx, I18nLowerCtx, TypedFeedbackContract,
    TypedFeedbackKind,
};

fn canonicalize_raw_f64_numeric_store_value(
    blk: &mut crate::block::LlBlock,
    value_double: &str,
) -> String {
    blk.call(
        DOUBLE,
        "js_array_numeric_value_to_raw_f64",
        &[(DOUBLE, value_double)],
    )
}

fn lower_value_for_optional_barrier(
    ctx: &mut FnCtx<'_>,
    value: &Expr,
    write_barrier_needed: bool,
) -> Result<(String, Option<String>)> {
    if !write_barrier_needed {
        return Ok((lower_expr(ctx, value)?, None));
    }
    let value_bits = lower_expr_native(ctx, value, ExpectedNativeRep::JsValueBits)?.value;
    let value_double = ctx.block().bitcast_i64_to_double(&value_bits);
    Ok((value_double, Some(value_bits)))
}

fn is_width_tracked_typed_array_receiver(ctx: &FnCtx<'_>, object: &Expr) -> bool {
    matches!(
        receiver_class_name(ctx, object).as_deref(),
        Some(
            "Int8Array"
                | "Uint8ClampedArray"
                | "Int16Array"
                | "Uint16Array"
                | "Int32Array"
                | "Uint32Array"
                | "Float16Array"
                | "Float32Array"
                | "Float64Array"
        )
    )
}

fn is_uint8array_receiver(ctx: &FnCtx<'_>, object: &Expr) -> bool {
    matches!(
        receiver_class_name(ctx, object).as_deref(),
        Some("Uint8Array")
    )
}

fn numeric_index_needs_runtime_key(index: &Expr) -> bool {
    // Only a LITERAL numeric key that is not a clean array index in
    // `0..=i32::MAX` needs the runtime key helper: out-of-range/negative
    // integers (`a[2**32-1]`, `a[-1]`), non-integer floats (`a[1.5]`), and
    // non-finite values (`a[NaN]`/`a[Infinity]`). These become string-keyed
    // properties and must reach `js_array_*_index_or_string`.
    //
    // Computed/dynamic numeric indices are deliberately NOT rerouted here:
    // they keep flowing through the typed-feedback numeric-array guard path,
    // which already carries its own out-of-range/non-integer fallback. Sending
    // them to the runtime key helper would defeat the native numeric-array hot
    // path and drop the index guard (regressing the native-region proof and
    // the typed-feedback hot-path tests). (#4557/#4543)
    match index {
        Expr::Integer(i) => *i < 0 || *i > i32::MAX as i64,
        Expr::Number(n) => {
            !(n.is_finite() && n.fract() == 0.0 && *n >= 0.0 && *n <= i32::MAX as f64)
        }
        _ => false,
    }
}

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            // Issue #611: `globalThis[<key>] = value` writes to the
            // persistent global-this singleton (see the matching IndexGet
            // arm above for context).
            if matches!(object.as_ref(), Expr::GlobalGet(_))
                && (matches!(index.as_ref(), Expr::String(_)) || is_string_expr(ctx, index))
            {
                let global_box = ctx.block().call(DOUBLE, "js_get_global_this", &[]);
                let key_box = lower_expr(ctx, index)?;
                let val_double = lower_expr(ctx, value)?;
                let (obj_handle, key_handle) = {
                    let blk = ctx.block();
                    let obj_handle = unbox_to_i64(blk, &global_box);
                    let key_handle = unbox_str_handle(blk, &key_box);
                    (obj_handle, key_handle)
                };
                let site_id = emit_typed_feedback_register_site(
                    ctx,
                    TypedFeedbackKind::PropertySet,
                    "globalThis[index]",
                    TypedFeedbackContract::object_set_by_name(),
                );
                ctx.block().call_void(
                    "js_typed_feedback_object_set_field_by_name",
                    &[
                        (I64, &site_id),
                        (I64, &obj_handle),
                        (I64, &key_handle),
                        (DOUBLE, &val_double),
                    ],
                );
                return Ok(val_double);
            }
            if is_width_tracked_typed_array_receiver(ctx, object) {
                // A non-numeric index (a Symbol, or a string property name) is
                // never an integer-indexed element. The width-tracked native
                // store coerces the index with `fptosi`, which truncates a
                // NaN-boxed Symbol to 0 and clobbers element 0 instead of
                // storing the symbol property (test262 TypedArray symbol-key
                // internals, #5735). Route such keys through the runtime
                // dispatcher, which triages symbol / string / numeric keys —
                // mirroring the symmetric IndexGet guard (index_get.rs). A
                // literal / loop-counter index stays `is_numeric_expr`, so every
                // proven element fast path below is preserved.
                if !is_numeric_expr(ctx, index) {
                    let arr_box = lower_expr(ctx, object)?;
                    let idx_double = lower_expr(ctx, index)?;
                    let val_double = lower_expr(ctx, value)?;
                    let blk = ctx.block();
                    let arr_bits = blk.bitcast_double_to_i64(&arr_box);
                    let arr_i64 = blk.and(I64, &arr_bits, POINTER_MASK_I64);
                    return Ok(blk.call(
                        DOUBLE,
                        "js_typed_array_index_set_dynamic",
                        &[
                            (I64, &arr_i64),
                            (DOUBLE, &idx_double),
                            (DOUBLE, &val_double),
                        ],
                    ));
                }
                if let Some(store) = lower_typed_array_store(ctx, object, index, value)? {
                    if ctx.discard_expr_value {
                        return Ok(double_literal(0.0));
                    }
                    return Ok(materialize_js_value(
                        ctx,
                        store.result,
                        MaterializationReason::FunctionAbi,
                    ));
                }

                // Stores fall back for untracked views, unknown bounds, unsafe
                // conversions, and Uint8ClampedArray's ToUint8Clamp semantics.
                let arr_box = lower_expr(ctx, object)?;
                let idx_double = lower_expr(ctx, index)?;
                let val_double = lower_expr(ctx, value)?;
                let blk = ctx.block();
                let arr_bits = blk.bitcast_double_to_i64(&arr_box);
                let arr_i64 = blk.and(I64, &arr_bits, POINTER_MASK_I64);
                let idx_i32 = blk.fptosi(DOUBLE, &idx_double, I32);
                blk.call_void(
                    "js_typed_array_set",
                    &[(I64, &arr_i64), (I32, &idx_i32), (DOUBLE, &val_double)],
                );
                let slow = LoweredValue::js_value(val_double.clone());
                ctx.record_lowered_value_with_access_mode(
                    "TypedArraySet",
                    None,
                    "TypedArraySet.slow_path",
                    &slow,
                    Some(BoundsState::Unknown),
                    None,
                    Some(BufferAccessMode::DynamicFallback),
                    Some(buffer_access_materialization_reason(ctx, object)),
                    false,
                    false,
                    vec!["typed_array_fallback=untracked_or_unproven".to_string()],
                );
                return Ok(val_double);
            }
            if is_uint8array_receiver(ctx, object) && !is_numeric_expr(ctx, index) {
                let arr_box = lower_expr(ctx, object)?;
                let idx_double = lower_expr(ctx, index)?;
                let val_double = lower_expr(ctx, value)?;
                let blk = ctx.block();
                let arr_bits = blk.bitcast_double_to_i64(&arr_box);
                let arr_i64 = blk.and(I64, &arr_bits, POINTER_MASK_I64);
                return Ok(blk.call(
                    DOUBLE,
                    "js_typed_array_index_set_dynamic",
                    &[
                        (I64, &arr_i64),
                        (DOUBLE, &idx_double),
                        (DOUBLE, &val_double),
                    ],
                ));
            }
            // #5525: when the receiver's static type is genuinely unknown
            // (`Type::Any`/`Type::Unknown`) and the index is numeric, route the
            // write through `js_dyn_index_set` — the exact symmetric counterpart
            // of the IndexGet `recv_unknown` arm (index_get.rs), which routes
            // reads through `js_dyn_index_get`. Both helpers carry the #5525
            // process-global typed-array kind cache + inline `typed_array_fast_
            // index_{get,set}` fast path, so a hot monomorphic `S[i]`/`P[i] = v`
            // on an `Int32Array` reaching a function through an untyped
            // `Array.<number>` parameter (bcryptjs's Blowfish P/S boxes) lands on
            // a cached load/store instead of the polymorphic feedback helper's
            // thread-local registry dispatch (`typed_array_owner_*` →
            // `_tlv_get_addr`). Pre-fix this fell all the way through to
            // `js_typed_feedback_object_set_index_polymorphic`, whose
            // `typed_array_set_numeric_index` path dominated the bcrypt profile.
            // The gate is narrow (only Any/Unknown receiver + numeric index) so
            // every statically-typed array / typed-array / object fast path below
            // is preserved.
            let recv_ty = crate::type_analysis::static_type_of(ctx, object);
            let recv_unknown = matches!(
                recv_ty,
                None | Some(perry_types::Type::Any) | Some(perry_types::Type::Unknown)
            );
            // The index may be numeric, a runtime string, or (rarely) a runtime
            // symbol — `js_dyn_index_set` triages all three. We only keep the
            // statically-known string-literal / symbol keys on their dedicated
            // (interned-handle / symbol-side-table) routes below; everything else
            // on an unknown receiver goes through the cached fast path. bcryptjs's
            // `lr[off]`/`lr[off + 1]` writes have an `off` param typed `any`, so
            // `off + 1` is NOT provably numeric — gating on `is_numeric_expr`
            // (the original #5525 attempt) missed exactly those ~4M hot writes
            // and they kept falling through to `js_put_value_set`.
            let index_is_static_string_or_symbol = matches!(
                index.as_ref(),
                Expr::String(_) | Expr::WtfString(_) | Expr::SymbolFor(_)
            ) || is_string_expr(ctx, index);
            if recv_unknown && !index_is_static_string_or_symbol {
                let obj_box = lower_expr(ctx, object)?;
                let idx_d = lower_expr(ctx, index)?;
                let val_double = lower_expr(ctx, value)?;
                // #5525 follow-up: guarded inline typed-array element STORE at the
                // access site, mirroring the inline read in index_get.rs. Removes
                // the per-element out-of-line `js_dyn_index_set` call +
                // `lookup_typed_array_kind` for bcrypt's `P[i]=`/`S[i]=` writes,
                // falling back to `js_dyn_index_set` on any guard miss.
                return Ok(lower_inline_dyn_typed_array_set(
                    ctx,
                    &obj_box,
                    &idx_d,
                    &val_double,
                ));
            }
            // Issue #637 / hono r2 followup: `arr[stringKey] = val` where
            // the index is statically string-typed (e.g. `for (const i in
            // sparseArr)` produces string i; then `out[i] = val`). Pre-fix
            // the array fast path below ran `fptosi(double, i32)` on the
            // NaN-boxed string, producing garbage indices that collapsed
            // every iteration's write onto slot 0. Route to the runtime
            // helper which parses the string as an integer and dispatches
            // to `js_array_set_f64_extend`, falling back to object-property
            // set on non-numeric keys per spec.
            if is_array_expr(ctx, object) && is_string_expr(ctx, index) {
                let arr_box = lower_expr(ctx, object)?;
                let key_box = lower_expr(ctx, index)?;
                let value_needs_barrier = array_store_needs_write_barrier(ctx, value);
                let (val_double, val_bits) =
                    lower_value_for_optional_barrier(ctx, value, value_needs_barrier)?;
                let (arr_handle, key_handle) = {
                    let blk = ctx.block();
                    let arr_handle = unbox_to_i64(blk, &arr_box);
                    let key_handle = unbox_str_handle(blk, &key_box);
                    (arr_handle, key_handle)
                };
                let site_id = emit_typed_feedback_register_site(
                    ctx,
                    TypedFeedbackKind::ArrayElement,
                    "array[string_index]",
                    TypedFeedbackContract::array_set_string_key(),
                );
                ctx.block().call(
                    I64,
                    "js_typed_feedback_array_set_string_key",
                    &[
                        (I64, &site_id),
                        (I64, &arr_handle),
                        (I64, &key_handle),
                        (DOUBLE, &val_double),
                    ],
                );
                if value_needs_barrier {
                    let arr_bits = ctx.block().bitcast_double_to_i64(&arr_box);
                    let val_bits =
                        val_bits.unwrap_or_else(|| ctx.block().bitcast_double_to_i64(&val_double));
                    emit_write_barrier(ctx, &arr_bits, &val_bits);
                }
                return Ok(val_double);
            }
            // Issue #637 followup: `arr[k] = X` where receiver is array
            // but index is dynamically-typed (Any) — most commonly a
            // forEach callback's `(item, k)` parameter where `k` could
            // be a string (for-in over object keys, replace callback
            // capture-group params, etc.). The array fast-path's
            // `fptosi(double, i32)` collapses NaN-boxed strings to slot 0.
            // Route to a runtime helper that detects the tag at runtime:
            // string → parse + array-extend; numeric → fptosi + extend.
            // Only fires when index isn't statically numeric (otherwise
            // the existing fast path is correct and avoids the runtime
            // dispatch overhead).
            if is_array_expr(ctx, object) && !is_numeric_expr(ctx, index) {
                let arr_box = lower_expr(ctx, object)?;
                let idx_double = lower_expr(ctx, index)?;
                let value_needs_barrier = array_store_needs_write_barrier(ctx, value);
                let val_double = lower_expr(ctx, value)?;
                let arr_handle = {
                    let blk = ctx.block();
                    unbox_to_i64(blk, &arr_box)
                };
                let site_id = emit_typed_feedback_register_site(
                    ctx,
                    TypedFeedbackKind::ArrayElement,
                    "array[dynamic_index]",
                    TypedFeedbackContract::array_set_index(),
                );
                ctx.block().call(
                    I64,
                    "js_typed_feedback_array_set_index_or_string",
                    &[
                        (I64, &site_id),
                        (I64, &arr_handle),
                        (DOUBLE, &idx_double),
                        (DOUBLE, &val_double),
                    ],
                );
                if value_needs_barrier {
                    let val_bits = ctx.block().bitcast_double_to_i64(&val_double);
                    let arr_bits = ctx.block().bitcast_double_to_i64(&arr_box);
                    emit_write_barrier(ctx, &arr_bits, &val_bits);
                }
                return Ok(val_double);
            }
            if is_array_expr(ctx, object) && numeric_index_needs_runtime_key(index) {
                let arr_box = lower_expr(ctx, object)?;
                let idx_double = lower_expr(ctx, index)?;
                let value_needs_barrier = array_store_needs_write_barrier(ctx, value);
                let val_double = lower_expr(ctx, value)?;
                let arr_handle = {
                    let blk = ctx.block();
                    unbox_to_i64(blk, &arr_box)
                };
                let site_id = emit_typed_feedback_register_site(
                    ctx,
                    TypedFeedbackKind::ArrayElement,
                    "array[boundary_index]",
                    TypedFeedbackContract::array_set_index(),
                );
                ctx.block().call(
                    I64,
                    "js_typed_feedback_array_set_index_or_string",
                    &[
                        (I64, &site_id),
                        (I64, &arr_handle),
                        (DOUBLE, &idx_double),
                        (DOUBLE, &val_double),
                    ],
                );
                if value_needs_barrier {
                    let val_bits = ctx.block().bitcast_double_to_i64(&val_double);
                    let arr_bits = ctx.block().bitcast_double_to_i64(&arr_box);
                    emit_write_barrier(ctx, &arr_bits, &val_bits);
                }
                return Ok(val_double);
            }
            // Same dispatch tree as IndexGet: known array → fast inline,
            // string key on dynamic receiver → object field set, otherwise
            // bail with a clear error.
            if is_array_expr(ctx, object) {
                // Bounded-index fast-fast path: when the surrounding
                // for-loop has registered `(counter_id, arr_id)` as a
                // bounded pair (via `lower_for`'s
                // `classify_for_length_hoist` analysis) and this
                // IndexSet matches it, we can skip the bound check +
                // capacity check + realloc fallback entirely. The
                // for-loop already proved `i < arr.length` and the
                // body provably can't change `arr.length`, so the
                // IndexSet at `arr[i]` is statically inbounds.
                if let (Expr::LocalGet(arr_id), Expr::LocalGet(idx_id)) =
                    (object.as_ref(), index.as_ref())
                {
                    if ctx.bounded_index_pairs.iter().any(|fact| {
                        fact.index_local_id == *idx_id && fact.array_local_id == *arr_id
                    }) {
                        let layout_note_needed = array_store_needs_layout_note(ctx, object, value);
                        let write_barrier_needed = array_store_needs_write_barrier(ctx, value);
                        let value_is_numeric = is_numeric_expr(ctx, value);
                        let require_numeric_layout = value_is_numeric
                            && expr_has_numeric_pointer_free_array_layout(ctx, object);
                        let arr_box = lower_expr(ctx, object)?;
                        let idx_double = lower_expr(ctx, index)?;
                        // Grab i32 slot name before mutably borrowing ctx for block().
                        let i32_slot_opt = ctx.i32_counter_slots.get(idx_id).cloned();
                        let idx_i32 = if let Some(ref i32_slot) = i32_slot_opt {
                            ctx.block().load(I32, i32_slot)
                        } else {
                            ctx.block().fptosi(DOUBLE, &idx_double, I32)
                        };
                        let val_double = lower_expr(ctx, value)?;
                        if require_numeric_layout {
                            let feedback_site_id = emit_typed_feedback_register_site(
                                ctx,
                                TypedFeedbackKind::ArrayElement,
                                "array[index]=",
                                TypedFeedbackContract::numeric_array_set_index(),
                            );
                            let fast_idx = ctx.new_block("idxset.bounded_numeric_fast");
                            let fallback_idx = ctx.new_block("idxset.bounded_numeric_fallback");
                            let merge_idx = ctx.new_block("idxset.bounded_numeric_merge");
                            let fast_label = ctx.block_label(fast_idx);
                            let fallback_label = ctx.block_label(fallback_idx);
                            let merge_label = ctx.block_label(merge_idx);

                            let guard_ok = {
                                let blk = ctx.block();
                                let guard_i32 = blk.call(
                                    I32,
                                    "js_typed_feedback_numeric_array_index_set_guard",
                                    &[
                                        (I64, &feedback_site_id),
                                        (DOUBLE, &arr_box),
                                        (I32, &idx_i32),
                                        (DOUBLE, &val_double),
                                        (I32, "1"),
                                    ],
                                );
                                blk.icmp_ne(I32, &guard_i32, "0")
                            };
                            ctx.block().cond_br(&guard_ok, &fast_label, &fallback_label);

                            ctx.current_block = fallback_idx;
                            {
                                let fallback_box = ctx.block().call(
                                    DOUBLE,
                                    "js_typed_feedback_array_index_set_fallback_boxed",
                                    &[
                                        (I64, &feedback_site_id),
                                        (DOUBLE, &arr_box),
                                        (DOUBLE, &idx_double),
                                        (DOUBLE, &val_double),
                                    ],
                                );
                                if let Some(slot) = ctx.locals.get(arr_id).cloned() {
                                    ctx.block().store(DOUBLE, &fallback_box, &slot);
                                }
                                ctx.block().br(&merge_label);
                                let fallback = LoweredValue {
                                    semantic: SemanticKind::JsValue,
                                    rep: NativeRep::JsValue,
                                    llvm_ty: DOUBLE,
                                    value: fallback_box,
                                };
                                ctx.record_lowered_value_with_access_mode_and_facts(
                                    "NumericArrayIndexSet",
                                    Some(*arr_id),
                                    "js_typed_feedback_array_index_set_fallback_boxed",
                                    &fallback,
                                    Some(BoundsState::Unknown),
                                    None,
                                    Some(BufferAccessMode::DynamicFallback),
                                    Some(MaterializationReason::RuntimeApi),
                                    None,
                                    None,
                                    Vec::new(),
                                    vec![
                                        raw_f64_layout_fact(
                                            Some(*arr_id),
                                            "rejected",
                                            "numeric_array_index_set_guard",
                                            Some(MaterializationReason::RuntimeApi),
                                        ),
                                        raw_f64_layout_fact(
                                            Some(*arr_id),
                                            "invalidated",
                                            "runtime_api",
                                            Some(MaterializationReason::RuntimeApi),
                                        ),
                                    ],
                                    false,
                                    false,
                                    Vec::new(),
                                );
                            }

                            ctx.current_block = fast_idx;
                            {
                                let blk = ctx.block();
                                let arr_bits = blk.bitcast_double_to_i64(&arr_box);
                                let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);
                                // The numeric-array set guard above was called with
                                // `in_bounds=true`, so it has already proved a live,
                                // non-forwarded plain Array in raw-f64 layout, a numeric
                                // RHS, and an in-bounds index. Store the f64 slot inline
                                // instead of calling the helper that re-validates the same
                                // facts before doing this store.
                                let idx_i64 = blk.zext(I32, &idx_i32, I64);
                                let byte_offset = blk.shl(I64, &idx_i64, "3");
                                let with_header = blk.add(I64, &byte_offset, "8");
                                let element_addr = blk.add(I64, &arr_handle, &with_header);
                                let element_ptr = blk.inttoptr(I64, &element_addr);
                                let numeric_value =
                                    canonicalize_raw_f64_numeric_store_value(blk, &val_double);
                                // GC_STORE_AUDIT(POINTER_FREE): guarded raw-f64
                                // numeric store — the canonicalized value is a
                                // plain f64, never a GC pointer, so no barrier.
                                blk.store(DOUBLE, &numeric_value, &element_ptr);
                                blk.br(&merge_label);
                            }
                            let stored = LoweredValue {
                                semantic: SemanticKind::JsNumber,
                                rep: NativeRep::F64,
                                llvm_ty: DOUBLE,
                                value: val_double.clone(),
                            };
                            ctx.record_lowered_value_with_access_mode_and_facts(
                                "NumericArrayIndexSet",
                                Some(*arr_id),
                                "js_array_numeric_set_f64_unboxed",
                                &stored,
                                Some(BoundsState::Guarded {
                                    guard_id: "numeric_array_index_set_guard".to_string(),
                                }),
                                None,
                                Some(BufferAccessMode::CheckedNative),
                                None,
                                None,
                                None,
                                vec![raw_f64_layout_fact(
                                    Some(*arr_id),
                                    "consumed",
                                    "numeric_array_index_set_guard",
                                    None,
                                )],
                                Vec::new(),
                                false,
                                false,
                                Vec::new(),
                            );

                            ctx.current_block = merge_idx;
                            return Ok(val_double);
                        }
                        let blk = ctx.block();
                        let arr_bits = blk.bitcast_double_to_i64(&arr_box);
                        let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);
                        // ptr = arr_handle + 8 + idx*8
                        let idx_i64 = blk.zext(I32, &idx_i32, I64);
                        let byte_offset = blk.shl(I64, &idx_i64, "3");
                        let with_header = blk.add(I64, &byte_offset, "8");
                        let element_addr = blk.add(I64, &arr_handle, &with_header);
                        let element_ptr = blk.inttoptr(I64, &element_addr);
                        let value_bits = emit_jsvalue_slot_store_on_block(
                            blk,
                            &element_ptr,
                            &val_double,
                            &arr_handle,
                            &idx_i32,
                            layout_note_needed,
                            &arr_handle,
                            &element_addr,
                            write_barrier_needed,
                        );
                        if !value_is_numeric {
                            let value_bits = value_bits
                                .unwrap_or_else(|| blk.bitcast_double_to_i64(&val_double));
                            emit_array_numeric_write_note_on_block(blk, &arr_handle, &value_bits);
                        }
                        return Ok(val_double);
                    }
                }

                let layout_note_needed = array_store_needs_layout_note(ctx, object, value);
                let write_barrier_needed = array_store_needs_write_barrier(ctx, value);
                let value_is_numeric = is_numeric_expr(ctx, value);
                let require_numeric_layout =
                    value_is_numeric && expr_has_numeric_pointer_free_array_layout(ctx, object);
                let arr_box = lower_expr(ctx, object)?;
                let idx_double = lower_expr(ctx, index)?;
                let val_double = lower_expr(ctx, value)?;
                let local_id = if let Expr::LocalGet(id) = object.as_ref() {
                    Some(*id)
                } else {
                    None
                };
                let feedback_site_id = emit_typed_feedback_register_site(
                    ctx,
                    TypedFeedbackKind::ArrayElement,
                    "array[index]=",
                    if require_numeric_layout {
                        TypedFeedbackContract::numeric_array_set_index()
                    } else {
                        TypedFeedbackContract::array_set_index()
                    },
                );
                // Use the fast inlined IndexSet path only when the
                // receiver is a local that's actually in ctx.locals
                // (stack slot). Module-level arrays accessed from inside
                // a function are in ctx.module_globals instead — for
                // those we use js_array_set_f64_extend (the realloc-
                // capable variant) and write the new pointer back to
                // the global slot. Issue #221: the previous code
                // funneled module globals through js_array_set_f64
                // which returns silently when `index >= length` — so
                // every `arr[i] = v` against a `const A: T[] = []`
                // declared empty was a silent no-op, both the value
                // and the implicit length update vanishing.
                if let Some(id) = local_id {
                    if ctx.locals.contains_key(&id) {
                        lower_index_set_fast(
                            ctx,
                            &arr_box,
                            &idx_double,
                            &val_double,
                            id,
                            layout_note_needed,
                            write_barrier_needed,
                            value_is_numeric,
                            require_numeric_layout,
                            &feedback_site_id,
                        )?;
                    } else if let Some(global_name) = ctx.module_globals.get(&id).cloned() {
                        let blk = ctx.block();
                        let arr_bits = blk.bitcast_double_to_i64(&arr_box);
                        let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);
                        let idx_i32 = blk.fptosi(DOUBLE, &idx_double, I32);
                        let new_handle = blk.call(
                            I64,
                            "js_typed_feedback_array_set_f64_extend",
                            &[
                                (I64, &feedback_site_id),
                                (I64, &arr_handle),
                                (I32, &idx_i32),
                                (DOUBLE, &val_double),
                            ],
                        );
                        let new_box = nanbox_pointer_inline(blk, &new_handle);
                        let g_ref = format!("@{}", global_name);
                        // GC_STORE_AUDIT(ROOT): module global array slot is a registered mutable GC root.
                        emit_root_nanbox_store_on_block(ctx.block(), &new_box, &g_ref);
                        // Gen-GC Phase C2: write barrier on array element store.
                        if write_barrier_needed {
                            let val_bits = ctx.block().bitcast_double_to_i64(&val_double);
                            emit_write_barrier(ctx, &arr_bits, &val_bits);
                        }
                    } else {
                        // Closure-captured array, or local without a
                        // stack slot (rare). Issue #637 followup / hono r2:
                        // pre-fix this called `js_array_set_f64` (non-
                        // extending), which silently returned when `index
                        // >= length` (matching `js_array_set_f64`'s in-
                        // bounds gate at array.rs:571). For an empty
                        // captured array (common pattern: closure body
                        // does `arr[++i] = X` to populate from outer
                        // scope), this dropped every write. Switch to
                        // `js_array_set_f64_extend` — the forwarding-
                        // pointer mechanism (issue #233) handles realloc
                        // visibility for the caller, so we don't need a
                        // writeback target here. Discard the returned
                        // pointer; downstream reads via clean_arr_ptr
                        // follow the forwarding chain to the new head.
                        let blk = ctx.block();
                        let arr_bits = blk.bitcast_double_to_i64(&arr_box);
                        let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);
                        let idx_i32 = blk.fptosi(DOUBLE, &idx_double, I32);
                        blk.call(
                            I64,
                            "js_typed_feedback_array_set_f64_extend",
                            &[
                                (I64, &feedback_site_id),
                                (I64, &arr_handle),
                                (I32, &idx_i32),
                                (DOUBLE, &val_double),
                            ],
                        );
                        // Gen-GC Phase C2: write barrier on array element store.
                        if write_barrier_needed {
                            let val_bits = ctx.block().bitcast_double_to_i64(&val_double);
                            emit_write_barrier(ctx, &arr_bits, &val_bits);
                        }
                    }
                } else {
                    let blk = ctx.block();
                    let arr_bits = blk.bitcast_double_to_i64(&arr_box);
                    let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);
                    let idx_i32 = blk.fptosi(DOUBLE, &idx_double, I32);
                    // Issue #637 followup / hono r2: use the extend variant
                    // so `arr[i] = X` for i >= length grows the array per
                    // JS spec, instead of silently no-op'ing (which the
                    // non-extend `js_array_set_f64` did via `if index >=
                    // length { return; }`). The hono Trie's
                    // `indexReplacementMap[++captureIndex] = N` pattern
                    // (sparse-extend from a closure capturing the array)
                    // was the load-bearing site — pre-fix the array stayed
                    // length 0 inside the closure, so `for (const i in
                    // indexReplacementMap)` outside the closure iterated
                    // zero times and `handlerMap` ended up empty.
                    blk.call(
                        I64,
                        "js_typed_feedback_array_set_f64_extend",
                        &[
                            (I64, &feedback_site_id),
                            (I64, &arr_handle),
                            (I32, &idx_i32),
                            (DOUBLE, &val_double),
                        ],
                    );
                    // Gen-GC Phase C2: write barrier on array element store.
                    if write_barrier_needed {
                        let val_bits = ctx.block().bitcast_double_to_i64(&val_double);
                        emit_write_barrier(ctx, &arr_bits, &val_bits);
                    }
                }
                return Ok(val_double);
            }
            if let Expr::String(literal) = index.as_ref() {
                let obj_box = lower_expr(ctx, object)?;
                let val_double = lower_expr(ctx, value)?;
                let key_idx = ctx.strings.intern(literal);
                let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
                let obj_bits = ctx.block().bitcast_double_to_i64(&obj_box);
                super::property_set::emit_nullish_write_guard(
                    ctx,
                    &obj_bits,
                    literal,
                    "iset.literal",
                );
                let static_classref =
                    super::index_get::index_object_is_class_or_proto_ref(ctx, object.as_ref());
                let (obj_handle, key_raw) = {
                    let blk = ctx.block();
                    let obj_handle = super::index_get::classref_preserving_handle(
                        blk,
                        &obj_bits,
                        static_classref,
                    );
                    let key_box = blk.load(DOUBLE, &key_handle_global);
                    let key_bits = blk.bitcast_double_to_i64(&key_box);
                    let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                    (obj_handle, key_raw)
                };
                let site_id = emit_typed_feedback_register_site(
                    ctx,
                    TypedFeedbackKind::PropertySet,
                    literal,
                    TypedFeedbackContract::object_set_by_name(),
                );
                ctx.block().call_void(
                    "js_typed_feedback_object_set_field_by_name",
                    &[
                        (I64, &site_id),
                        (I64, &obj_handle),
                        (I64, &key_raw),
                        (DOUBLE, &val_double),
                    ],
                );
                return Ok(val_double);
            }
            if is_string_expr(ctx, index) {
                let obj_box = lower_expr(ctx, object)?;
                let key_box = lower_expr(ctx, index)?;
                let val_double = lower_expr(ctx, value)?;
                let obj_bits = ctx.block().bitcast_double_to_i64(&obj_box);
                super::property_set::emit_nullish_write_guard(
                    ctx,
                    &obj_bits,
                    "index",
                    "iset.string",
                );
                let static_classref =
                    super::index_get::index_object_is_class_or_proto_ref(ctx, object.as_ref());
                let (obj_handle, key_handle) = {
                    let blk = ctx.block();
                    let obj_handle = super::index_get::classref_preserving_handle(
                        blk,
                        &obj_bits,
                        static_classref,
                    );
                    // SSO-safe key unbox — see IndexGet branch above for rationale.
                    let key_handle = unbox_str_handle(blk, &key_box);
                    (obj_handle, key_handle)
                };
                let site_id = emit_typed_feedback_register_site(
                    ctx,
                    TypedFeedbackKind::PropertySet,
                    "object[string_index]",
                    TypedFeedbackContract::object_set_by_name(),
                );
                ctx.block().call_void(
                    "js_typed_feedback_object_set_field_by_name",
                    &[
                        (I64, &site_id),
                        (I64, &obj_handle),
                        (I64, &key_handle),
                        (DOUBLE, &val_double),
                    ],
                );
                return Ok(val_double);
            }
            // Fallback with runtime STRING_TAG check, matching IndexGet.
            // Layout: first runtime-check whether the index is a Symbol
            // (POINTER_TAG with SYMBOL_MAGIC). If so, dispatch to the
            // symbol-property side table. Otherwise fall through to the
            // string/numeric dispatch.
            let obj_box = lower_expr(ctx, object)?;
            let idx_box = lower_expr(ctx, index)?;
            let val_double = lower_expr(ctx, value)?;
            let obj_bits = ctx.block().bitcast_double_to_i64(&obj_box);
            super::property_set::emit_nullish_write_guard(ctx, &obj_bits, "index", "iset");
            let static_classref =
                super::index_get::index_object_is_class_or_proto_ref(ctx, object.as_ref());
            let obj_handle = {
                let blk = ctx.block();
                super::index_get::classref_preserving_handle(blk, &obj_bits, static_classref)
            };
            let feedback_site_id = emit_typed_feedback_register_site(
                ctx,
                TypedFeedbackKind::ArrayElement,
                "index_set",
                TypedFeedbackContract::polymorphic_index_set(),
            );
            // Symbol check: js_is_symbol returns 1 if idx_box is a Symbol.
            let is_sym_i32 = ctx.block().call(I32, "js_is_symbol", &[(DOUBLE, &idx_box)]);
            let is_sym_bit = ctx.block().icmp_ne(I32, &is_sym_i32, "0");
            let sym_set = ctx.new_block("iset.sym");
            let nonsym_set = ctx.new_block("iset.nonsym");
            let str_set = ctx.new_block("iset.str");
            let num_set = ctx.new_block("iset.num");
            let set_merge = ctx.new_block("iset.merge");
            let sym_lbl = ctx.block_label(sym_set);
            let nonsym_lbl = ctx.block_label(nonsym_set);
            let str_lbl = ctx.block_label(str_set);
            let num_lbl = ctx.block_label(num_set);
            let merge_lbl = ctx.block_label(set_merge);
            ctx.block().cond_br(&is_sym_bit, &sym_lbl, &nonsym_lbl);
            // Symbol key → side-table set.
            ctx.current_block = sym_set;
            ctx.block().call(
                DOUBLE,
                "js_object_set_symbol_property",
                &[
                    (DOUBLE, &obj_box),
                    (DOUBLE, &idx_box),
                    (DOUBLE, &val_double),
                ],
            );
            ctx.block().br(&merge_lbl);
            // Not a symbol — recompute idx_bits in this block (LLVM SSA, no
            // dominance issue: each branch starts fresh).
            ctx.current_block = nonsym_set;
            let blk = ctx.block();
            let idx_bits = blk.bitcast_double_to_i64(&idx_box);
            let top16 = blk.lshr(I64, &idx_bits, "48");
            // STRING_TAG (0x7FFF) heap pointer + SHORT_STRING_TAG (0x7FF9) SSO.
            // See IndexGet path comment / issue #434 for the SSO rationale.
            let is_str_tag_heap = blk.icmp_eq(I64, &top16, "32767");
            let lower48 = blk.and(I64, &idx_bits, POINTER_MASK_I64);
            let is_valid_ptr = blk.icmp_ugt(I64, &lower48, "4095");
            let is_str_heap = blk.and(crate::types::I1, &is_str_tag_heap, &is_valid_ptr);
            let is_str_tag_sso = blk.icmp_eq(I64, &top16, "32761");
            let is_str = blk.or(crate::types::I1, &is_str_heap, &is_str_tag_sso);
            ctx.block().cond_br(&is_str, &str_lbl, &num_lbl);
            // String key → polymorphic helper that detects array receivers
            // and parses numeric-string keys as array indices, falling
            // through to `js_object_set_field_by_name` for Object/Closure
            // receivers. Issue #637: pre-fix this called the object setter
            // unconditionally, which silently no-op'd `arr[stringKey] = X`
            // on captured arrays whose static type was lost across the
            // closure boundary (forEach callbacks, replace callbacks, etc.).
            ctx.current_block = str_set;
            let key_handle = {
                let blk = ctx.block();
                unbox_str_handle(blk, &idx_box)
            };
            ctx.block().call(
                I64,
                "js_typed_feedback_array_set_string_key",
                &[
                    (I64, &feedback_site_id),
                    (I64, &obj_handle),
                    (I64, &key_handle),
                    (DOUBLE, &val_double),
                ],
            );
            ctx.block().br(&merge_lbl);
            // Numeric key → polymorphic dispatch.
            //
            // Closes #471: the previous fallback emitted an inline
            // `obj_handle + 8 + idx*8` store on the assumption that the
            // receiver had an ArrayHeader (8-byte header) layout. That's
            // a load-bearing assumption for `arr[i] = v` against an
            // unknown-typed receiver where `is_array_expr` couldn't
            // narrow it statically — but ObjectHeader is 24 bytes plus
            // `max(field_count, 8)` inline slots, so writing at offset
            // `8 + idx*8` for any `idx ≥ 7` overflows the object's
            // allocation and corrupts the adjacent heap object. The
            // @perryts/mongodb #471 repro hit this with `idMap[i] = …`
            // (a `Record<number, unknown>`) and trampled the keys_array
            // of an unrelated object that the BSON encoder later read
            // as an empty doc, producing structurally-truncated wire data.
            //
            // Route through the runtime which checks the receiver's GC
            // type and dispatches: arrays/buffers/typed-arrays through
            // js_array_set_f64_extend (handles forwarding + per-kind
            // stores), plain objects through stringify-the-index +
            // js_object_set_field_by_name. The forwarding-chain handling
            // that the previous code's inline-vs-fwd branch did is now
            // inside js_array_set_f64_extend's clean_arr_ptr_mut.
            ctx.current_block = num_set;
            {
                let blk = ctx.block();
                blk.call_void(
                    "js_typed_feedback_object_set_index_polymorphic",
                    &[
                        (I64, &feedback_site_id),
                        (I64, &obj_handle),
                        (DOUBLE, &idx_box),
                        (DOUBLE, &val_double),
                    ],
                );
            }
            ctx.block().br(&merge_lbl);
            ctx.current_block = set_merge;
            Ok(val_double)
        }

        // `obj.field = v` — generic object field write.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}

/// #5525 follow-up: guarded **inline** typed-array element STORE for an
/// `obj[i] = v` whose receiver static type is erased (`any`/unknown) but is, at
/// runtime, commonly an owning numeric typed array (bcryptjs's `P[i]=`/`S[i]=`
/// Int32Array boxes). Mirrors [`index_get::lower_inline_dyn_typed_array_get`]:
/// the same pointer / `PERRY_TA_VIEW_GUARD` / `PERRY_TA_KIND_CACHE` / index
/// guards, then a direct per-kind store into `header + 16 + idx*elem_size`,
/// falling back to `js_dyn_index_set` on any guard miss. The store result is the
/// assigned value (`val_double`), matching `js_dyn_index_set`'s return.
///
/// Only the kinds with a simple ToInt32/ToUint32 truncating store (Int8/Uint8/
/// Int16/Uint16/Int32/Uint32) or a direct float store (Float32/Float64) are
/// inlined — i.e. `kind <= KIND_FLOAT64` (7). Uint8ClampedArray (round-half-to-
/// even clamp), the BigInt kinds (ToBigInt / throw) and Float16 (f16 encode) are
/// excluded by the guard and defer to the runtime, which already owns them. The
/// integer truncation here (`toint32(value)` then narrow) is bit-identical to
/// the runtime `store_at`'s `to_uint32_bits(value) as <width>`; the float store
/// is identical to `store_at`'s direct slot write — so behavior matches the
/// existing runtime fast path exactly.
fn lower_inline_dyn_typed_array_set(
    ctx: &mut FnCtx<'_>,
    obj_box: &str,
    idx_d: &str,
    val_double: &str,
) -> String {
    let tag_mask = crate::nanbox::i64_literal(crate::nanbox::TAG_MASK);
    let pointer_tag = crate::nanbox::POINTER_TAG_I64;
    let pointer_mask = crate::nanbox::POINTER_MASK_I64;

    let fast_idx = ctx.new_block("tav.set.fast");
    let store_idx = ctx.new_block("tav.set.store");
    let slow_idx = ctx.new_block("tav.set.slow");
    let merge_idx = ctx.new_block("tav.set.merge");
    let fast_label = ctx.block_label(fast_idx);
    let store_label = ctx.block_label(store_idx);
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);

    // ---- entry: combined cache/kind/range guard -> fast | slow ----
    let entry_guard = {
        let blk = ctx.block();
        let obj_bits = blk.bitcast_double_to_i64(obj_box);
        let raw = blk.and(I64, &obj_bits, pointer_mask);
        let tagged = blk.and(I64, &obj_bits, &tag_mask);
        let is_ptr = blk.icmp_eq(I64, &tagged, pointer_tag);
        let vg = blk.load(I64, "@PERRY_TA_VIEW_GUARD");
        let vg_zero = blk.icmp_eq(I64, &vg, "0");
        let slot = blk.lshr(I64, &raw, "3");
        let slot = blk.and(I64, &slot, "63");
        let entry_ptr = blk.gep(
            "[64 x i64]",
            "@PERRY_TA_KIND_CACHE",
            &[(I64, "0"), (I64, &slot)],
        );
        let entry_val = blk.load(I64, &entry_ptr);
        let entry_addr = blk.lshr(I64, &entry_val, "8");
        let addr_match = blk.icmp_eq(I64, &entry_addr, &raw);
        let kind = blk.and(I64, &entry_val, "255");
        // Stores inline only kinds with a trivial truncating/float store:
        // kind <= KIND_FLOAT64 (7). Uint8Clamped (8), BigInt (9/10), Float16
        // (11), and the 0xFF sentinel all defer to the runtime.
        let kind_ok = blk.icmp_ule(I64, &kind, "7");
        let idx_ge0 = blk.fcmp("oge", idx_d, "0.0");
        let idx_lt = blk.fcmp("olt", idx_d, "4294967296.0");
        let g = blk.and(I1, &is_ptr, &vg_zero);
        let g = blk.and(I1, &g, &addr_match);
        let g = blk.and(I1, &g, &kind_ok);
        let g = blk.and(I1, &g, &idx_ge0);
        blk.and(I1, &g, &idx_lt)
    };
    ctx.block().cond_br(&entry_guard, &fast_label, &slow_label);

    // ---- fast: validate integer index + bounds -> store | slow ----
    ctx.current_block = fast_idx;
    let (raw, idx_i64, kind) = {
        let blk = ctx.block();
        let obj_bits = blk.bitcast_double_to_i64(obj_box);
        let raw = blk.and(I64, &obj_bits, pointer_mask);
        let slot = blk.lshr(I64, &raw, "3");
        let slot = blk.and(I64, &slot, "63");
        let entry_ptr = blk.gep(
            "[64 x i64]",
            "@PERRY_TA_KIND_CACHE",
            &[(I64, "0"), (I64, &slot)],
        );
        let entry_val = blk.load(I64, &entry_ptr);
        let kind = blk.and(I64, &entry_val, "255");
        let idx_i64 = blk.fptosi(DOUBLE, idx_d, I64);
        (raw, idx_i64, kind)
    };
    let fast_ok = {
        let blk = ctx.block();
        let idx_back = blk.sitofp(I64, &idx_i64, DOUBLE);
        let is_int = blk.fcmp("oeq", &idx_back, idx_d);
        let hdr_ptr = blk.inttoptr(I64, &raw);
        let len = blk.load(I32, &hdr_ptr);
        let len_i64 = blk.zext(I32, &len, I64);
        let in_bounds = blk.icmp_ult(I64, &idx_i64, &len_i64);
        blk.and(I1, &is_int, &in_bounds)
    };
    ctx.block().cond_br(&fast_ok, &store_label, &slow_label);

    // ---- store: per-kind direct element store (data = header + 16) ----
    ctx.current_block = store_idx;
    let data_base = {
        let blk = ctx.block();
        blk.add(I64, &raw, "16")
    };
    // ToInt32 of the value once (shared by all integer kinds). For float kinds
    // we use the raw double directly. `toint32` matches the runtime
    // `to_uint32_bits` (NaN/±Inf/±0 → 0, else trunc-toward-zero mod 2^32).
    let val_i32 = ctx.block().toint32(val_double);

    let b_i8 = ctx.new_block("tav.s.i8");
    let b_u8 = ctx.new_block("tav.s.u8");
    let b_i16 = ctx.new_block("tav.s.i16");
    let b_u16 = ctx.new_block("tav.s.u16");
    let b_i32 = ctx.new_block("tav.s.i32");
    let b_u32 = ctx.new_block("tav.s.u32");
    let b_f32 = ctx.new_block("tav.s.f32");
    let b_f64 = ctx.new_block("tav.s.f64");
    let l_i8 = ctx.block_label(b_i8);
    let l_u8 = ctx.block_label(b_u8);
    let l_i16 = ctx.block_label(b_i16);
    let l_u16 = ctx.block_label(b_u16);
    let l_i32 = ctx.block_label(b_i32);
    let l_u32 = ctx.block_label(b_u32);
    let l_f32 = ctx.block_label(b_f32);
    let l_f64 = ctx.block_label(b_f64);

    // Dispatch chain on `kind` (in the store block, after data_base/val_i32).
    let chk = |ctx: &mut FnCtx<'_>, k: &str, hit: &str, next_idx: usize| {
        let next_label = ctx.block_label(next_idx);
        let cond = ctx.block().icmp_eq(I64, &kind, k);
        ctx.block().cond_br(&cond, hit, &next_label);
    };
    let c1 = ctx.new_block("tav.sd1");
    let c2 = ctx.new_block("tav.sd2");
    let c3 = ctx.new_block("tav.sd3");
    let c4 = ctx.new_block("tav.sd4");
    let c5 = ctx.new_block("tav.sd5");
    let c6 = ctx.new_block("tav.sd6");
    chk(ctx, "0", &l_i8, c1);
    ctx.current_block = c1;
    chk(ctx, "1", &l_u8, c2);
    ctx.current_block = c2;
    chk(ctx, "2", &l_i16, c3);
    ctx.current_block = c3;
    chk(ctx, "3", &l_u16, c4);
    ctx.current_block = c4;
    chk(ctx, "4", &l_i32, c5);
    ctx.current_block = c5;
    chk(ctx, "5", &l_u32, c6);
    ctx.current_block = c6;
    // remaining: kind 6 → f32, else (7) → f64.
    let is_f32 = ctx.block().icmp_eq(I64, &kind, "6");
    ctx.block().cond_br(&is_f32, &l_f32, &l_f64);

    // Per-kind stores. Each: off = idx << shift; addr = data_base + off;
    // store narrowed value; br merge.
    emit_inline_ta_int_store(
        ctx,
        b_i8,
        &idx_i64,
        &data_base,
        &merge_label,
        "0",
        &val_i32,
        I8,
    );
    emit_inline_ta_int_store(
        ctx,
        b_u8,
        &idx_i64,
        &data_base,
        &merge_label,
        "0",
        &val_i32,
        I8,
    );
    emit_inline_ta_int_store(
        ctx,
        b_i16,
        &idx_i64,
        &data_base,
        &merge_label,
        "1",
        &val_i32,
        I16,
    );
    emit_inline_ta_int_store(
        ctx,
        b_u16,
        &idx_i64,
        &data_base,
        &merge_label,
        "1",
        &val_i32,
        I16,
    );
    emit_inline_ta_int_store(
        ctx,
        b_i32,
        &idx_i64,
        &data_base,
        &merge_label,
        "2",
        &val_i32,
        I32,
    );
    emit_inline_ta_int_store(
        ctx,
        b_u32,
        &idx_i64,
        &data_base,
        &merge_label,
        "2",
        &val_i32,
        I32,
    );
    // F32: fptrunc the double to float, store.
    {
        ctx.current_block = b_f32;
        let blk = ctx.block();
        let off = blk.shl(I64, &idx_i64, "2");
        let addr = blk.add(I64, &data_base, &off);
        let ptr = blk.inttoptr(I64, &addr);
        let f = blk.fptrunc(DOUBLE, val_double, F32);
        blk.store(F32, &f, &ptr);
        blk.br(&merge_label);
    }
    // F64: store the double raw.
    {
        ctx.current_block = b_f64;
        let blk = ctx.block();
        let off = blk.shl(I64, &idx_i64, "3");
        let addr = blk.add(I64, &data_base, &off);
        let ptr = blk.inttoptr(I64, &addr);
        blk.store(DOUBLE, val_double, &ptr);
        blk.br(&merge_label);
    }

    // ---- slow: the unchanged runtime setter ----
    ctx.current_block = slow_idx;
    ctx.block().call(
        DOUBLE,
        "js_dyn_index_set",
        &[(DOUBLE, obj_box), (DOUBLE, idx_d), (DOUBLE, val_double)],
    );
    ctx.block().br(&merge_label);

    // ---- merge: assignment yields the stored value on every path ----
    ctx.current_block = merge_idx;
    // All paths produce `val_double` as the expression result (matching
    // `js_dyn_index_set`'s `return value`), so no phi is needed.
    val_double.to_string()
}

/// Emit one per-kind integer typed-array element store block for
/// [`lower_inline_dyn_typed_array_set`]: switches to `blk_idx`, computes the
/// element address (`data_base + (idx << shift)`), narrows the shared
/// ToInt32-coerced `val_i32` to `elem_ty`, stores it, and branches to
/// `merge_label`.
#[allow(clippy::too_many_arguments)]
fn emit_inline_ta_int_store(
    ctx: &mut FnCtx<'_>,
    blk_idx: usize,
    idx_i64: &str,
    data_base: &str,
    merge_label: &str,
    shift: &str,
    val_i32: &str,
    elem_ty: crate::types::LlvmType,
) {
    ctx.current_block = blk_idx;
    let blk = ctx.block();
    let off = blk.shl(I64, idx_i64, shift);
    let addr = blk.add(I64, data_base, &off);
    let ptr = blk.inttoptr(I64, &addr);
    let narrowed = if elem_ty == I32 {
        val_i32.to_string()
    } else {
        blk.trunc(I32, val_i32, elem_ty)
    };
    blk.store(elem_ty, &narrowed, &ptr);
    blk.br(merge_label);
}
