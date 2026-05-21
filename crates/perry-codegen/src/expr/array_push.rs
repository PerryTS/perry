//! ArrayPush / ArrayPushSpread.
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
    array_store_needs_layout_note, buffer_alias_metadata_suffix, can_lower_expr_as_i32,
    emit_layout_note_slot_on_block, emit_shadow_slot_clear, emit_shadow_slot_update_for_expr,
    emit_string_literal_global, emit_v8_export_call, emit_v8_member_method_call,
    emit_write_barrier, emit_write_barrier_slot_on_block, expr_is_known_non_pointer_shadow_value,
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
        Expr::ArrayPush { array_id, value } => {
            // Resolve the array storage in priority order: closure
            // capture (slot in the closure header), local alloca slot,
            // module-level global. The realloc-pointer write-back must
            // go to whichever storage we read from.
            let array_expr = Expr::LocalGet(*array_id);
            let layout_note_needed = array_store_needs_layout_note(ctx, &array_expr, value);
            let write_barrier_needed = !is_numeric_expr(ctx, value);
            let v = lower_expr(ctx, value)?;
            let arr_box = lower_expr(ctx, &array_expr)?;

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
                    blk.store(DOUBLE, &v, &element_ptr);
                    let new_length = blk.add(I32, &length, "1");
                    let arr_ptr = blk.inttoptr(I64, &arr_handle);
                    blk.store(I32, &new_length, &arr_ptr);
                    let value_bits = blk.bitcast_double_to_i64(&v);
                    if layout_note_needed {
                        emit_layout_note_slot_on_block(blk, &arr_handle, &length, &value_bits);
                    }
                    if write_barrier_needed {
                        emit_write_barrier_slot_on_block(
                            blk,
                            &arr_handle,
                            &element_addr,
                            &value_bits,
                        );
                    }
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
                // Existing arr_box value (pre-push) is still valid for
                // ArrayPush's return value semantics: returns the
                // pushed value cast back to double — but `out.push(...)`
                // statements never read this, and the HIR uses Stmt::Expr
                // which discards. Match the slow path's return shape
                // anyway by re-loading the slot (covers the realloc
                // case where the box changed).
                return Ok(arr_box);
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
                    let cap_dbl = blk.call(
                        DOUBLE,
                        "js_closure_get_capture_f64",
                        &[(I64, &closure_ptr), (I32, &idx_str)],
                    );
                    let box_ptr = blk.bitcast_double_to_i64(&cap_dbl);
                    blk.call_void("js_box_set", &[(I64, &box_ptr), (DOUBLE, &new_box)]);
                } else if let Some(slot) = ctx.locals.get(array_id).cloned() {
                    let blk = ctx.block();
                    let box_dbl = blk.load(DOUBLE, &slot);
                    let box_ptr = blk.bitcast_double_to_i64(&box_dbl);
                    blk.call_void("js_box_set", &[(I64, &box_ptr), (DOUBLE, &new_box)]);
                }
                return Ok(new_box);
            }
            if let Some(&capture_idx) = ctx.closure_captures.get(array_id) {
                let closure_ptr = ctx
                    .current_closure_ptr
                    .clone()
                    .ok_or_else(|| anyhow!("ArrayPush captured but no current_closure_ptr"))?;
                let idx_str = capture_idx.to_string();
                ctx.block().call_void(
                    "js_closure_set_capture_f64",
                    &[(I64, &closure_ptr), (I32, &idx_str), (DOUBLE, &new_box)],
                );
            } else if let Some(slot) = ctx.locals.get(array_id).cloned() {
                ctx.block().store(DOUBLE, &new_box, &slot);
            } else if let Some(global_name) = ctx.module_globals.get(array_id).cloned() {
                let g_ref = format!("@{}", global_name);
                ctx.block().store(DOUBLE, &new_box, &g_ref);
            } else {
                return Err(anyhow!("ArrayPush({}): local not in scope", array_id));
            }
            Ok(new_box)
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
                    let cap_dbl = blk.call(
                        DOUBLE,
                        "js_closure_get_capture_f64",
                        &[(I64, &closure_ptr), (I32, &idx_str)],
                    );
                    let box_ptr = blk.bitcast_double_to_i64(&cap_dbl);
                    blk.call_void("js_box_set", &[(I64, &box_ptr), (DOUBLE, &new_box)]);
                } else if let Some(slot) = ctx.locals.get(array_id).cloned() {
                    let blk = ctx.block();
                    let box_dbl = blk.load(DOUBLE, &slot);
                    let box_ptr = blk.bitcast_double_to_i64(&box_dbl);
                    blk.call_void("js_box_set", &[(I64, &box_ptr), (DOUBLE, &new_box)]);
                }
                return Ok(new_box);
            }
            if let Some(&capture_idx) = ctx.closure_captures.get(array_id) {
                let closure_ptr = ctx.current_closure_ptr.clone().ok_or_else(|| {
                    anyhow!("ArrayPushSpread captured but no current_closure_ptr")
                })?;
                let idx_str = capture_idx.to_string();
                ctx.block().call_void(
                    "js_closure_set_capture_f64",
                    &[(I64, &closure_ptr), (I32, &idx_str), (DOUBLE, &new_box)],
                );
            } else if let Some(slot) = ctx.locals.get(array_id).cloned() {
                ctx.block().store(DOUBLE, &new_box, &slot);
            } else if let Some(global_name) = ctx.module_globals.get(array_id).cloned() {
                let g_ref = format!("@{}", global_name);
                ctx.block().store(DOUBLE, &new_box, &g_ref);
            } else {
                return Err(anyhow!("ArrayPushSpread({}): local not in scope", array_id));
            }
            Ok(new_box)
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
