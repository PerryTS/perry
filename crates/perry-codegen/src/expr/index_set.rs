//! IndexSet (arr[i] = v).
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
                let blk = ctx.block();
                let obj_handle = unbox_to_i64(blk, &global_box);
                let key_handle = unbox_str_handle(blk, &key_box);
                blk.call_void(
                    "js_object_set_field_by_name",
                    &[
                        (I64, &obj_handle),
                        (I64, &key_handle),
                        (DOUBLE, &val_double),
                    ],
                );
                return Ok(val_double);
            }
            // Uint8ClampedArray writes must apply ToUint8Clamp (NaN → 0,
            // v ≤ 0 → 0, v ≥ 255 → 255, otherwise round-half-to-even).
            // Route to the runtime helper which already implements the
            // proper clamping in its KIND_UINT8_CLAMPED branch. Without
            // this intercept the generic numeric-key path below emits a
            // raw f64 stride store that bypasses clamping entirely.
            if matches!(
                receiver_class_name(ctx, object).as_deref(),
                Some("Uint8ClampedArray")
            ) {
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
                return Ok(val_double);
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
                let value_needs_barrier = !is_numeric_expr(ctx, value);
                let val_double = lower_expr(ctx, value)?;
                let blk = ctx.block();
                let arr_handle = unbox_to_i64(blk, &arr_box);
                let key_handle = unbox_str_handle(blk, &key_box);
                blk.call(
                    I64,
                    "js_array_set_string_key",
                    &[
                        (I64, &arr_handle),
                        (I64, &key_handle),
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
                let value_needs_barrier = !is_numeric_expr(ctx, value);
                let val_double = lower_expr(ctx, value)?;
                let blk = ctx.block();
                let arr_handle = unbox_to_i64(blk, &arr_box);
                blk.call(
                    I64,
                    "js_array_set_index_or_string",
                    &[
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
                    if ctx.bounded_index_pairs.contains(&(*idx_id, *arr_id)) {
                        let layout_note_needed = array_store_needs_layout_note(ctx, object, value);
                        let write_barrier_needed = !is_numeric_expr(ctx, value);
                        let arr_box = lower_expr(ctx, object)?;
                        let val_double = lower_expr(ctx, value)?;
                        // Grab i32 slot name before mutably borrowing ctx for block().
                        let i32_slot_opt = ctx.i32_counter_slots.get(idx_id).cloned();
                        let idx_i32 = if let Some(ref i32_slot) = i32_slot_opt {
                            ctx.block().load(I32, i32_slot)
                        } else {
                            let idx_double = lower_expr(ctx, index)?;
                            ctx.block().fptosi(DOUBLE, &idx_double, I32)
                        };
                        let blk = ctx.block();
                        let arr_bits = blk.bitcast_double_to_i64(&arr_box);
                        let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);
                        // ptr = arr_handle + 8 + idx*8
                        let idx_i64 = blk.zext(I32, &idx_i32, I64);
                        let byte_offset = blk.shl(I64, &idx_i64, "3");
                        let with_header = blk.add(I64, &byte_offset, "8");
                        let element_addr = blk.add(I64, &arr_handle, &with_header);
                        let element_ptr = blk.inttoptr(I64, &element_addr);
                        blk.store(DOUBLE, &val_double, &element_ptr);
                        let val_bits = blk.bitcast_double_to_i64(&val_double);
                        if layout_note_needed {
                            emit_layout_note_slot_on_block(blk, &arr_handle, &idx_i32, &val_bits);
                        }
                        if write_barrier_needed {
                            emit_write_barrier_slot_on_block(
                                blk,
                                &arr_handle,
                                &element_addr,
                                &val_bits,
                            );
                        }
                        return Ok(val_double);
                    }
                }

                let layout_note_needed = array_store_needs_layout_note(ctx, object, value);
                let write_barrier_needed = !is_numeric_expr(ctx, value);
                let arr_box = lower_expr(ctx, object)?;
                let idx_double = lower_expr(ctx, index)?;
                let val_double = lower_expr(ctx, value)?;
                let local_id = if let Expr::LocalGet(id) = object.as_ref() {
                    Some(*id)
                } else {
                    None
                };
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
                        )?;
                    } else if let Some(global_name) = ctx.module_globals.get(&id).cloned() {
                        let blk = ctx.block();
                        let arr_bits = blk.bitcast_double_to_i64(&arr_box);
                        let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);
                        let idx_i32 = blk.fptosi(DOUBLE, &idx_double, I32);
                        let new_handle = blk.call(
                            I64,
                            "js_array_set_f64_extend",
                            &[(I64, &arr_handle), (I32, &idx_i32), (DOUBLE, &val_double)],
                        );
                        let new_box = nanbox_pointer_inline(blk, &new_handle);
                        let g_ref = format!("@{}", global_name);
                        ctx.block().store(DOUBLE, &new_box, &g_ref);
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
                            "js_array_set_f64_extend",
                            &[(I64, &arr_handle), (I32, &idx_i32), (DOUBLE, &val_double)],
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
                        "js_array_set_f64_extend",
                        &[(I64, &arr_handle), (I32, &idx_i32), (DOUBLE, &val_double)],
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
                let blk = ctx.block();
                let obj_bits = blk.bitcast_double_to_i64(&obj_box);
                let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
                let key_box = blk.load(DOUBLE, &key_handle_global);
                let key_bits = blk.bitcast_double_to_i64(&key_box);
                let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                blk.call_void(
                    "js_object_set_field_by_name",
                    &[(I64, &obj_handle), (I64, &key_raw), (DOUBLE, &val_double)],
                );
                return Ok(val_double);
            }
            if is_string_expr(ctx, index) {
                let obj_box = lower_expr(ctx, object)?;
                let key_box = lower_expr(ctx, index)?;
                let val_double = lower_expr(ctx, value)?;
                let blk = ctx.block();
                let obj_bits = blk.bitcast_double_to_i64(&obj_box);
                let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
                // SSO-safe key unbox — see IndexGet branch above for rationale.
                let key_handle = unbox_str_handle(blk, &key_box);
                blk.call_void(
                    "js_object_set_field_by_name",
                    &[
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
            let blk = ctx.block();
            let obj_handle = unbox_to_i64(blk, &obj_box);
            // Symbol check: js_is_symbol returns 1 if idx_box is a Symbol.
            let is_sym_i32 = blk.call(I32, "js_is_symbol", &[(DOUBLE, &idx_box)]);
            let is_sym_bit = blk.icmp_ne(I32, &is_sym_i32, "0");
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
            let blk = ctx.block();
            let key_handle = unbox_str_handle(blk, &idx_box);
            ctx.block().call(
                I64,
                "js_array_set_string_key",
                &[
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
                    "js_object_set_index_polymorphic",
                    &[
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
