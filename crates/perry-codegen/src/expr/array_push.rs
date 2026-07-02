//! ArrayPush / ArrayPushSpread.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::{anyhow, Result};
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
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

#[allow(unused_imports)]
use super::{
    array_store_needs_layout_note, array_store_needs_write_barrier, buffer_alias_metadata_suffix,
    can_lower_expr_as_i32, emit_array_numeric_write_note_on_block,
    emit_jsvalue_slot_store_on_block, emit_jsvalue_slot_store_with_value_bits_on_block,
    emit_layout_note_slot_on_block, emit_root_nanbox_store_on_block, emit_shadow_slot_clear,
    emit_shadow_slot_update_for_expr, emit_string_literal_global,
    emit_typed_feedback_register_site, emit_v8_export_call, emit_v8_member_method_call,
    emit_write_barrier, emit_write_barrier_slot_on_block,
    expr_has_numeric_pointer_free_array_layout, expr_is_known_non_pointer_shadow_value,
    extract_array_of_object_shape, i32_bool_to_nanbox, import_origin_suffix,
    is_global_this_builtin_function_name, is_global_this_builtin_name, is_known_finite,
    lower_array_literal, lower_channel_reduction, lower_expr, lower_expr_as_i32, lower_expr_native,
    lower_index_set_fast, lower_js_args_array, lower_object_literal, lower_stream_super_init,
    lower_url_string_getter, nanbox_bigint_inline, nanbox_pointer_inline,
    nanbox_pointer_inline_pub, nanbox_string_inline, proxy_build_args_array, raw_f64_layout_fact,
    try_flat_const_2d_int, try_lower_flat_const_index_get, try_match_channel_reduction,
    try_static_class_name, unbox_str_handle, unbox_to_i64, variant_name, ChannelReduction,
    FlatConstInfo, FnCtx, I18nLowerCtx, TypedFeedbackContract, TypedFeedbackKind,
};

fn emit_array_handle_length(ctx: &mut FnCtx<'_>, array_handle: &str) -> String {
    let blk = ctx.block();
    let len_i32 = blk.call(I32, "js_array_length", &[(I64, array_handle)]);
    blk.sitofp(I32, &len_i32, DOUBLE)
}

fn emit_array_box_length(ctx: &mut FnCtx<'_>, array_box: &str) -> String {
    let blk = ctx.block();
    let array_handle = unbox_to_i64(blk, array_box);
    emit_array_handle_length(ctx, &array_handle)
}

fn lower_array_push_value(
    ctx: &mut FnCtx<'_>,
    value: &Expr,
    layout_note_needed: bool,
    write_barrier_needed: bool,
) -> Result<(String, Option<String>)> {
    if !layout_note_needed && !write_barrier_needed {
        return Ok((lower_expr(ctx, value)?, None));
    }

    let lowered = lower_expr_native(ctx, value, ExpectedNativeRep::JsValueBits)?;
    let value_bits = lowered.value.clone();
    let value_double = ctx.block().bitcast_i64_to_double(&value_bits);
    ctx.record_lowered_value_with_access_mode(
        "ArrayPush",
        None,
        "array_push.slot_value_bits",
        &lowered,
        None,
        None,
        None,
        None,
        false,
        false,
        vec![
            format!("layout_note_needed={}", layout_note_needed as u8),
            format!("write_barrier_needed={}", write_barrier_needed as u8),
            "boxed_at=array_push_slot_or_runtime_helper_edge".to_string(),
        ],
    );
    Ok((value_double, Some(value_bits)))
}

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::ArrayPush { array_id, value } => {
            // Resolve the array storage in priority order: closure
            // capture (slot in the closure header), local alloca slot,
            // module-level global. The realloc-pointer write-back must
            // go to whichever storage we read from.
            let array_expr = Expr::LocalGet(*array_id);
            let layout_note_needed = array_store_needs_layout_note(ctx, &array_expr, value);
            let write_barrier_needed = array_store_needs_write_barrier(ctx, value);
            let value_is_numeric = is_numeric_expr(ctx, value);
            let require_numeric_layout =
                value_is_numeric && expr_has_numeric_pointer_free_array_layout(ctx, &array_expr);
            let (v, v_bits) =
                lower_array_push_value(ctx, value, layout_note_needed, write_barrier_needed)?;
            let arr_box = lower_expr(ctx, &array_expr)?;

            if require_numeric_layout
                && !ctx.boxed_vars.contains(array_id)
                && !ctx.closure_captures.contains_key(array_id)
                && ctx.locals.contains_key(array_id)
            {
                let slot = ctx.locals.get(array_id).cloned().unwrap();
                let feedback_site_id = emit_typed_feedback_register_site(
                    ctx,
                    TypedFeedbackKind::ArrayElement,
                    "array.push",
                    TypedFeedbackContract::numeric_array_push(),
                );
                let fast_idx = ctx.new_block("apush.numeric_fast");
                let fallback_idx = ctx.new_block("apush.numeric_fallback");
                let merge_idx = ctx.new_block("apush.numeric_merge");
                let fast_label = ctx.block_label(fast_idx);
                let fallback_label = ctx.block_label(fallback_idx);
                let merge_label = ctx.block_label(merge_idx);

                let guard_ok = {
                    let blk = ctx.block();
                    let guard_i32 = blk.call(
                        I32,
                        "js_typed_feedback_numeric_array_push_guard",
                        &[(I64, &feedback_site_id), (DOUBLE, &arr_box), (DOUBLE, &v)],
                    );
                    blk.icmp_ne(I32, &guard_i32, "0")
                };
                ctx.block().cond_br(&guard_ok, &fast_label, &fallback_label);

                ctx.current_block = fast_idx;
                {
                    let blk = ctx.block();
                    let arr_handle = unbox_to_i64(blk, &arr_box);
                    let new_handle = blk.call(
                        I64,
                        "js_array_numeric_push_f64_unboxed",
                        &[(I64, &arr_handle), (DOUBLE, &v)],
                    );
                    let new_box = nanbox_pointer_inline(blk, &new_handle);
                    blk.store(DOUBLE, &new_box, &slot);
                    blk.br(&merge_label);
                }
                let pushed = LoweredValue {
                    semantic: SemanticKind::JsNumber,
                    rep: NativeRep::F64,
                    llvm_ty: DOUBLE,
                    value: v.clone(),
                };
                ctx.record_lowered_value_with_access_mode_and_facts(
                    "NumericArrayPush",
                    Some(*array_id),
                    "js_array_numeric_push_f64_unboxed",
                    &pushed,
                    Some(BoundsState::Guarded {
                        guard_id: "numeric_array_push_guard".to_string(),
                    }),
                    None,
                    Some(BufferAccessMode::CheckedNative),
                    None,
                    None,
                    None,
                    vec![raw_f64_layout_fact(
                        Some(*array_id),
                        "consumed",
                        "numeric_array_push_guard",
                        None,
                    )],
                    Vec::new(),
                    false,
                    false,
                    Vec::new(),
                );

                ctx.current_block = fallback_idx;
                {
                    let blk = ctx.block();
                    blk.call_void(
                        "js_typed_feedback_record_fallback_call",
                        &[(I64, &feedback_site_id)],
                    );
                    let arr_handle = unbox_to_i64(blk, &arr_box);
                    let new_handle = blk.call(
                        I64,
                        "js_array_push_f64",
                        &[(I64, &arr_handle), (DOUBLE, &v)],
                    );
                    let new_box = nanbox_pointer_inline(blk, &new_handle);
                    blk.store(DOUBLE, &new_box, &slot);
                    blk.br(&merge_label);
                }
                let fallback = LoweredValue {
                    semantic: SemanticKind::JsValue,
                    rep: NativeRep::JsValue,
                    llvm_ty: DOUBLE,
                    value: v.clone(),
                };
                ctx.record_lowered_value_with_access_mode_and_facts(
                    "NumericArrayPush",
                    Some(*array_id),
                    "js_array_push_f64",
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
                            Some(*array_id),
                            "rejected",
                            "numeric_array_push_guard",
                            Some(MaterializationReason::RuntimeApi),
                        ),
                        raw_f64_layout_fact(
                            Some(*array_id),
                            "invalidated",
                            "runtime_api",
                            Some(MaterializationReason::RuntimeApi),
                        ),
                    ],
                    false,
                    false,
                    Vec::new(),
                );

                ctx.current_block = merge_idx;
                let current_box = ctx.block().load(DOUBLE, &slot);
                return Ok(emit_array_box_length(ctx, &current_box));
            }

            // Fast path: local-bound, non-captured, non-boxed array.
            // This is the canonical hot shape — `out.push(...)` over a
            // local array variable. The runtime's `js_array_push_f64`
            // does `clean_arr_ptr_mut` (heap-range check + forwarding
            // chain walk + length/capacity sanity check + lazy detect)
            // before every store; for an array that's known to be a
            // plain heap pointer, that's wasted work on the *millions*
            // of pushes a JSON-pipeline-style workload performs.
            //
            // Inline shape (mirrors `lower_index_set_fast`):
            //
            //   if (gc_flags & FORWARDED): call js_array_push_f64 (slow)
            //   else:
            //     length   = load i32, arr+0
            //     capacity = load i32, arr+4
            //     if (length < capacity):
            //       store double value, arr+8+length*8
            //       store i32 (length+1), arr+0
            //       done
            //     else:
            //       call js_array_push_f64 (grow path)
            //
            // The fast inline branch needs no slot write-back — the
            // array pointer doesn't change unless we grow. The slow
            // branches both update the slot via the existing
            // boxed/captured/local fall-through below.
            if !ctx.boxed_vars.contains(array_id)
                && !ctx.closure_captures.contains_key(array_id)
                && ctx.locals.contains_key(array_id)
            {
                let slot = ctx.locals.get(array_id).cloned().unwrap();
                let blk = ctx.block();
                let arr_handle = unbox_to_i64(blk, &arr_box);

                // Issue #233: forwarded arrays must follow the
                // forwarding chain. Route through the runtime which
                // calls clean_arr_ptr_mut and writes into the live
                // head — the inline path's offset-0 length read would
                // otherwise pick up the lower 32 bits of the
                // forwarding pointer (garbage).
                let gc_flags_addr = blk.sub(I64, &arr_handle, "7");
                let gc_flags_ptr = blk.inttoptr(I64, &gc_flags_addr);
                let gc_flags = blk.load(I8, &gc_flags_ptr);
                let fwd_bits = blk.and(I8, &gc_flags, "128");
                let is_fwd = blk.icmp_ne(I8, &fwd_bits, "0");

                let fwd_idx = ctx.new_block("apush.fwd");
                let nofwd_idx = ctx.new_block("apush.nofwd");
                let inbounds_idx = ctx.new_block("apush.inbounds");
                let realloc_idx = ctx.new_block("apush.realloc");
                let merge_idx = ctx.new_block("apush.merge");

                let fwd_label = ctx.block_label(fwd_idx);
                let nofwd_label = ctx.block_label(nofwd_idx);
                let inbounds_label = ctx.block_label(inbounds_idx);
                let realloc_label = ctx.block_label(realloc_idx);
                let merge_label = ctx.block_label(merge_idx);

                ctx.block().cond_br(&is_fwd, &fwd_label, &nofwd_label);

                // FORWARDED branch: route through runtime.
                ctx.current_block = fwd_idx;
                {
                    let blk = ctx.block();
                    let new_handle = blk.call(
                        I64,
                        "js_array_push_f64",
                        &[(I64, &arr_handle), (DOUBLE, &v)],
                    );
                    let new_box = nanbox_pointer_inline(blk, &new_handle);
                    blk.store(DOUBLE, &new_box, &slot);
                    blk.br(&merge_label);
                }

                // No forwarding — read length & capacity, branch on
                // capacity. inline_store on length < capacity, slow
                // call on full.
                ctx.current_block = nofwd_idx;
                {
                    let blk = ctx.block();
                    let length = blk.safe_load_i32_from_ptr(&arr_handle);
                    let cap_addr = blk.add(I64, &arr_handle, "4");
                    let cap_ptr = blk.inttoptr(I64, &cap_addr);
                    let capacity = blk.load(I32, &cap_ptr);
                    let has_room = blk.icmp_ult(I32, &length, &capacity);
                    blk.cond_br(&has_room, &inbounds_label, &realloc_label);
                }

                // Inline store: arr+8+length*8 = value, length++.
                ctx.current_block = inbounds_idx;
                {
                    let blk = ctx.block();
                    let length = blk.safe_load_i32_from_ptr(&arr_handle);
                    let length_i64 = blk.zext(I32, &length, I64);
                    let byte_offset = blk.shl(I64, &length_i64, "3");
                    let with_header = blk.add(I64, &byte_offset, "8");
                    let element_addr = blk.add(I64, &arr_handle, &with_header);
                    let element_ptr = blk.inttoptr(I64, &element_addr);
                    let value_bits = if let Some(value_bits) = v_bits.as_deref() {
                        emit_jsvalue_slot_store_with_value_bits_on_block(
                            blk,
                            &element_ptr,
                            &v,
                            value_bits,
                            &arr_handle,
                            &length,
                            layout_note_needed,
                            &arr_handle,
                            &element_addr,
                            write_barrier_needed,
                        )
                    } else {
                        emit_jsvalue_slot_store_on_block(
                            blk,
                            &element_ptr,
                            &v,
                            &arr_handle,
                            &length,
                            layout_note_needed,
                            &arr_handle,
                            &element_addr,
                            write_barrier_needed,
                        )
                    };
                    if !value_is_numeric {
                        let value_bits =
                            value_bits.unwrap_or_else(|| blk.bitcast_double_to_i64(&v));
                        emit_array_numeric_write_note_on_block(blk, &arr_handle, &value_bits);
                    }
                    let new_length = blk.add(I32, &length, "1");
                    let arr_ptr = blk.inttoptr(I64, &arr_handle);
                    // GC_STORE_AUDIT(POINTER_FREE): array length header update has no child pointer.
                    blk.store(I32, &new_length, &arr_ptr);
                    blk.br(&merge_label);
                }

                // Realloc: capacity exhausted. Runtime allocates a
                // bigger backing block and installs the forwarding
                // pointer; writeback the new head to the local slot.
                ctx.current_block = realloc_idx;
                {
                    let blk = ctx.block();
                    let new_handle = blk.call(
                        I64,
                        "js_array_push_f64",
                        &[(I64, &arr_handle), (DOUBLE, &v)],
                    );
                    let new_box = nanbox_pointer_inline(blk, &new_handle);
                    blk.store(DOUBLE, &new_box, &slot);
                    blk.br(&merge_label);
                }

                ctx.current_block = merge_idx;
                let current_box = ctx.block().load(DOUBLE, &slot);
                return Ok(emit_array_box_length(ctx, &current_box));
            }

            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let new_handle = blk.call(
                I64,
                "js_array_push_f64",
                &[(I64, &arr_handle), (DOUBLE, &v)],
            );
            let new_box = nanbox_pointer_inline(blk, &new_handle);
            // Write back to whichever storage backs the local.
            // Boxed var takes priority: write through the box so
            // every closure sharing the box sees the new pointer.
            if ctx.boxed_vars.contains(array_id) {
                // Captured-through-closure boxed var.
                if let Some(&capture_idx) = ctx.closure_captures.get(array_id) {
                    let closure_ptr = ctx.current_closure_ptr.clone().ok_or_else(|| {
                        anyhow!("ArrayPush boxed captured but no current_closure_ptr")
                    })?;
                    let idx_str = capture_idx.to_string();
                    let blk = ctx.block();
                    let box_ptr = blk.call(
                        I64,
                        "js_closure_get_capture_bits",
                        &[(I64, &closure_ptr), (I32, &idx_str)],
                    );
                    let new_bits = blk.bitcast_double_to_i64(&new_box);
                    blk.call_void("js_box_set_bits", &[(I64, &box_ptr), (I64, &new_bits)]);
                    // Gen-GC Phase C2: the realloc'd array head is a (possibly
                    // young) heap pointer stored into an existing box — barrier
                    // the box parent so a minor GC can't miss it.
                    emit_write_barrier(ctx, &box_ptr, &new_bits);
                    // The capture slot holds the BOX pointer; the box content is
                    // the shared storage every closure sees. Return here — do NOT
                    // fall through to the `closure_set_capture_bits` store below,
                    // which would clobber the box pointer in the capture slot with
                    // the array pointer, so the next push would treat the array as
                    // the box and silently lose the realloc write-back.
                    return Ok(emit_array_handle_length(ctx, &new_handle));
                } else if let Some(slot) = ctx.locals.get(array_id).cloned() {
                    let blk = ctx.block();
                    let box_ptr = blk.load(I64, &slot);
                    let new_bits = blk.bitcast_double_to_i64(&new_box);
                    blk.call_void("js_box_set_bits", &[(I64, &box_ptr), (I64, &new_bits)]);
                    // Gen-GC Phase C2: barrier the box parent (see capture path).
                    emit_write_barrier(ctx, &box_ptr, &new_bits);
                    // The slot holds the BOX pointer — the box is the shared
                    // storage. Return so the slot keeps pointing at the box (see
                    // the captured branch above).
                    return Ok(emit_array_handle_length(ctx, &new_handle));
                }
                // #5459: `array_id` is in `boxed_vars` but has no box location in
                // THIS context — it's a module-level global accessed directly from
                // a nested function (the load path read `@global`, not a box-get).
                // Returning here would skip the realloc write-back entirely, so the
                // relocated array header is never stored to the registered GC-root
                // global slot: the old head is freed on the next GC and the global
                // dangles (use-after-free / corrupted length). Fall through to the
                // module-global store-back below instead of returning.
            }
            if let Some(&capture_idx) = ctx.closure_captures.get(array_id) {
                let closure_ptr = ctx
                    .current_closure_ptr
                    .clone()
                    .ok_or_else(|| anyhow!("ArrayPush captured but no current_closure_ptr"))?;
                let idx_str = capture_idx.to_string();
                let new_bits = ctx.block().bitcast_double_to_i64(&new_box);
                ctx.block().call_void(
                    "js_closure_set_capture_bits",
                    &[(I64, &closure_ptr), (I32, &idx_str), (I64, &new_bits)],
                );
                // Gen-GC Phase C2: the realloc'd array head stored into the
                // closure capture is a (possibly young) heap pointer — barrier
                // the closure parent.
                emit_write_barrier(ctx, &closure_ptr, &new_bits);
            } else if let Some(slot) = ctx.locals.get(array_id).cloned() {
                ctx.block().store(DOUBLE, &new_box, &slot);
            } else if let Some(global_name) = ctx.module_globals.get(array_id).cloned() {
                let g_ref = format!("@{}", global_name);
                // GC_STORE_AUDIT(ROOT): module global array slot is a registered mutable GC root.
                emit_root_nanbox_store_on_block(ctx.block(), &new_box, &g_ref);
            } else {
                return Err(anyhow!("ArrayPush({}): local not in scope", array_id));
            }
            Ok(emit_array_handle_length(ctx, &new_handle))
        }

        // `arr.push(...src)` — HIR variant carrying the destination
        // array's LocalId and the source expression (any iterable, in
        // practice an array or Set). Mirrors `Expr::ArrayPush` above:
        // load the destination from its slot, unbox both pointers, call
        // the runtime's `js_array_concat` (which walks the source and
        // calls `js_array_push_f64` per element + already handles
        // Set sources via SET_REGISTRY), NaN-box the realloc-aware
        // return pointer, and write back to whichever storage backs
        // `array_id`. Issue #248.
        Expr::ArrayPushSpread { array_id, source } => {
            let src_box = lower_expr(ctx, source)?;
            let arr_box = lower_expr(ctx, &Expr::LocalGet(*array_id))?;
            let blk = ctx.block();
            let dst_handle = unbox_to_i64(blk, &arr_box);
            let src_handle = unbox_to_i64(blk, &src_box);
            let new_handle = blk.call(
                I64,
                "js_array_concat",
                &[(I64, &dst_handle), (I64, &src_handle)],
            );
            let new_box = nanbox_pointer_inline(blk, &new_handle);
            if ctx.boxed_vars.contains(array_id) {
                if let Some(&capture_idx) = ctx.closure_captures.get(array_id) {
                    let closure_ptr = ctx.current_closure_ptr.clone().ok_or_else(|| {
                        anyhow!("ArrayPushSpread boxed captured but no current_closure_ptr")
                    })?;
                    let idx_str = capture_idx.to_string();
                    let blk = ctx.block();
                    let box_ptr = blk.call(
                        I64,
                        "js_closure_get_capture_bits",
                        &[(I64, &closure_ptr), (I32, &idx_str)],
                    );
                    let new_bits = blk.bitcast_double_to_i64(&new_box);
                    blk.call_void("js_box_set_bits", &[(I64, &box_ptr), (I64, &new_bits)]);
                    // Gen-GC Phase C2: the realloc'd array head is a (possibly
                    // young) heap pointer stored into an existing box — barrier
                    // the box parent so a minor GC can't miss it.
                    emit_write_barrier(ctx, &box_ptr, &new_bits);
                    // Box content is the shared storage; the capture slot must keep
                    // pointing at the box. Return so we don't fall through to the
                    // capture-slot store, which would clobber the box pointer (see
                    // the matching note in `Expr::ArrayPush`).
                    return Ok(emit_array_handle_length(ctx, &new_handle));
                } else if let Some(slot) = ctx.locals.get(array_id).cloned() {
                    let blk = ctx.block();
                    let box_ptr = blk.load(I64, &slot);
                    let new_bits = blk.bitcast_double_to_i64(&new_box);
                    blk.call_void("js_box_set_bits", &[(I64, &box_ptr), (I64, &new_bits)]);
                    // Gen-GC Phase C2: barrier the box parent (see capture path).
                    emit_write_barrier(ctx, &box_ptr, &new_bits);
                    return Ok(emit_array_handle_length(ctx, &new_handle));
                }
                // #5459: in `boxed_vars` but no box location here — a module-level
                // global accessed directly from a nested function. Fall through to
                // the module-global store-back so the relocated head reaches the
                // GC-root slot (see the matching note in `Expr::ArrayPush`).
            }
            if let Some(&capture_idx) = ctx.closure_captures.get(array_id) {
                let closure_ptr = ctx.current_closure_ptr.clone().ok_or_else(|| {
                    anyhow!("ArrayPushSpread captured but no current_closure_ptr")
                })?;
                let idx_str = capture_idx.to_string();
                let new_bits = ctx.block().bitcast_double_to_i64(&new_box);
                ctx.block().call_void(
                    "js_closure_set_capture_bits",
                    &[(I64, &closure_ptr), (I32, &idx_str), (I64, &new_bits)],
                );
                // Gen-GC Phase C2: the realloc'd array head stored into the
                // closure capture is a (possibly young) heap pointer — barrier
                // the closure parent.
                emit_write_barrier(ctx, &closure_ptr, &new_bits);
            } else if let Some(slot) = ctx.locals.get(array_id).cloned() {
                ctx.block().store(DOUBLE, &new_box, &slot);
            } else if let Some(global_name) = ctx.module_globals.get(array_id).cloned() {
                let g_ref = format!("@{}", global_name);
                // GC_STORE_AUDIT(ROOT): module global array slot is a registered mutable GC root.
                emit_root_nanbox_store_on_block(ctx.block(), &new_box, &g_ref);
            } else {
                return Err(anyhow!("ArrayPushSpread({}): local not in scope", array_id));
            }
            Ok(emit_array_handle_length(ctx, &new_handle))
        }

        // -------- Closures (Phase D.1) --------
        // `function() { ... }` / `(x) => { ... }` — allocate a closure
        // object pointing at a pre-emitted function body, populate
        // capture slots, return the NaN-boxed pointer.
        //
        // The closure body is emitted as a top-level LLVM function
        // (`perry_closure_<modprefix>__<func_id>`) earlier in
        // `compile_module` via the `compile_closure` pass.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
