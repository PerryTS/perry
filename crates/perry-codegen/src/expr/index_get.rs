//! IndexGet (arr[i] / obj[k]).
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
        Expr::IndexGet { object, index } => {
            // Issue #611: `globalThis[<key>]` reads from the persistent
            // global-this singleton. Pre-fix, `Expr::GlobalGet` lowered
            // to the `0.0` sentinel and the generic IndexGet path called
            // `js_object_get_field_by_name_f64(0, key)` which returned
            // undefined — `(globalThis as any)[id] = m; (globalThis as
            // any)[id]` round-trip lost the value. Route through the
            // real singleton (`js_get_global_this`) when receiver is
            // GlobalGet AND the key is string-typed.
            if matches!(object.as_ref(), Expr::GlobalGet(_))
                && (matches!(index.as_ref(), Expr::String(_)) || is_string_expr(ctx, index))
            {
                let key_box = lower_expr(ctx, index)?;
                let blk = ctx.block();
                let key_handle = unbox_str_handle(blk, &key_box);
                return Ok(blk.call(
                    DOUBLE,
                    "js_global_or_console_property_by_name",
                    &[(I64, &key_handle)],
                ));
            }
            // Uint8ClampedArray reads must use byte-stride load (the
            // runtime stores 1 byte per element, not 8). Route to the
            // runtime helper which handles the per-kind load. Without
            // this intercept the generic numeric-key path below emits
            // an f64 stride read that returns garbage on byte-packed
            // Uint8ClampedArray storage.
            if matches!(
                receiver_class_name(ctx, object).as_deref(),
                Some("Uint8ClampedArray")
            ) {
                let arr_box = lower_expr(ctx, object)?;
                let idx_double = lower_expr(ctx, index)?;
                let blk = ctx.block();
                let arr_bits = blk.bitcast_double_to_i64(&arr_box);
                let arr_i64 = blk.and(I64, &arr_bits, POINTER_MASK_I64);
                let idx_i32 = blk.fptosi(DOUBLE, &idx_double, I32);
                return Ok(blk.call(
                    DOUBLE,
                    "js_typed_array_get",
                    &[(I64, &arr_i64), (I32, &idx_i32)],
                ));
            }
            // Scalar-replaced array literal: `arr[k]` where arr was bound to
            // `[...]` and never escaped, and k is a compile-time index in
            // range. Loads directly from the kth stack alloca — no heap,
            // no runtime call, no bounds check. See `collect_non_escaping_arrays`.
            if let Expr::LocalGet(id) = object.as_ref() {
                if let Some(slots) = ctx.scalar_replaced_arrays.get(id).cloned() {
                    let k = match index.as_ref() {
                        Expr::Integer(k) if *k >= 0 => Some(*k as usize),
                        Expr::Number(f) if f.is_finite() && *f >= 0.0 && f.fract() == 0.0 => {
                            Some(*f as usize)
                        }
                        _ => None,
                    };
                    if let Some(k) = k {
                        if k < slots.len() {
                            return Ok(ctx.block().load(DOUBLE, &slots[k]));
                        }
                    }
                }
            }

            // Issue #50: flat-const 2D int array fast path. Replaces
            // `X[i][j]` (inline) and `krow[j]` (aliased row pattern)
            // with a direct GEP + load from a private `[N x i32]`
            // global emitted at module compile. Skips the arena header
            // + length check + double reload per access. Returns the
            // element as a NaN-boxed double (`sitofp i32 → double`) so
            // callers that expect fp receive the same JSValue shape
            // they already do; callers that expect i32 (via the #49
            // `lower_expr_as_i32` path) collapse the `fptosi(sitofp)`
            // round-trip during instcombine.
            if let Some(v) = try_lower_flat_const_index_get(ctx, object, index)? {
                return Ok(v);
            }

            // String indexing fast path: `s[i]` returns the char at
            // position i as a single-char string. Handled before the
            // array path so `str[0]` doesn't fall through to a raw
            // double load.
            if is_string_expr(ctx, object) {
                let s_box = lower_expr(ctx, object)?;
                let idx_d = lower_expr(ctx, index)?;
                let blk = ctx.block();
                let s_handle = unbox_to_i64(blk, &s_box);
                let idx_i32 = blk.fptosi(DOUBLE, &idx_d, I32);
                let result = blk.call(
                    I64,
                    "js_string_char_at",
                    &[(I64, &s_handle), (I32, &idx_i32)],
                );
                return Ok(nanbox_string_inline(blk, &result));
            }
            // Issue #514: when the receiver's static type is genuinely
            // unknown (`Type::Any` / `Type::Unknown`) and the index is
            // numeric, route through the runtime tag-aware dispatcher.
            // The pre-fix array fast path interpreted `*StringHeader`
            // pointers as `*ArrayHeader`, returning the byte_len as a
            // subnormal f64 — the load-bearing bug behind hono's
            // mergePath template-literal logic that mixes `s?.[0]` /
            // `s?.at(-1)` / `s?.slice(1)` on `(s: any)` parameters.
            // The gate is narrow (only Type::Any/Unknown) so existing
            // TypedArray, Object-with-numeric-keys, and class-instance
            // fast paths keep their inline-offset reads.
            let recv_ty = crate::type_analysis::static_type_of(ctx, object);
            let recv_unknown = matches!(
                recv_ty,
                None | Some(perry_types::Type::Any) | Some(perry_types::Type::Unknown)
            );
            if recv_unknown && is_numeric_expr(ctx, index) {
                let obj_box = lower_expr(ctx, object)?;
                let idx_d = lower_expr(ctx, index)?;
                let blk = ctx.block();
                return Ok(blk.call(
                    DOUBLE,
                    "js_dyn_index_get",
                    &[(DOUBLE, &obj_box), (DOUBLE, &idx_d)],
                ));
            }
            // Three cases:
            //   1. Receiver is a known array → inline f64 element load
            //   2. Index is a string (literal or string-typed local) →
            //      generic object field access via js_object_get_field_by_name_f64
            //   3. Anything else → fall back to dynamic object field
            //      access by stringifying the index at runtime
            if is_array_expr(ctx, object) {
                // Bounded-index fast path (mirrors the IndexSet
                // optimization in the same file): if the surrounding
                // for-loop registered `(counter_id, arr_id)` as
                // bounded via `lower_for`'s `classify_for_length_hoist`,
                // we can skip the bound check + OOB phi entirely.
                // The loop already proved `i < arr.length` and the
                // body provably can't change `arr.length`.
                if let (Expr::LocalGet(arr_id), Expr::LocalGet(idx_id)) =
                    (object.as_ref(), index.as_ref())
                {
                    if ctx.bounded_index_pairs.contains(&(*idx_id, *arr_id)) {
                        let arr_box = lower_expr(ctx, object)?;
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

                        // Issue #179 Phase 3: lazy-array guard on the
                        // bounded-index fast path. Same story as the
                        // generic path above — a LazyArrayHeader has
                        // unrelated bytes at `arr + 8 + idx*8`, so we
                        // need to route through the slow path when
                        // the receiver is lazy. Branchy here but the
                        // branch is almost always trivially
                        // well-predicted (same array for the loop's
                        // duration; LLVM can often hoist the check).
                        //
                        // Issue #233: also detect FORWARDED arrays
                        // (post-grow stale pointers from async-fn
                        // parameter handoff). Same routing — slow
                        // path's `clean_arr_ptr` follows the chain.
                        let gc_type_addr = blk.sub(I64, &arr_handle, "8");
                        let gc_type_ptr = blk.inttoptr(I64, &gc_type_addr);
                        let gc_type = blk.load(I8, &gc_type_ptr);
                        let is_lazy = blk.icmp_eq(I8, &gc_type, "9");
                        let gc_flags_addr = blk.sub(I64, &arr_handle, "7");
                        let gc_flags_ptr = blk.inttoptr(I64, &gc_flags_addr);
                        let gc_flags = blk.load(I8, &gc_flags_ptr);
                        let fwd_bits = blk.and(I8, &gc_flags, "128"); // GC_FLAG_FORWARDED
                        let is_fwd = blk.icmp_ne(I8, &fwd_bits, "0");
                        let needs_slow = blk.or(I1, &is_lazy, &is_fwd);

                        let lazy_idx = ctx.new_block("bidx.lazy");
                        let fast_idx = ctx.new_block("bidx.fast");
                        let merge_idx = ctx.new_block("bidx.merge");
                        let lazy_label = ctx.block_label(lazy_idx);
                        let fast_label = ctx.block_label(fast_idx);
                        let merge_label = ctx.block_label(merge_idx);
                        ctx.block().cond_br(&needs_slow, &lazy_label, &fast_label);

                        ctx.current_block = lazy_idx;
                        let lazy_blk = ctx.block();
                        let lazy_val = lazy_blk.call(
                            DOUBLE,
                            "js_array_get_f64",
                            &[(I64, &arr_handle), (I32, &idx_i32)],
                        );
                        let lazy_end_label = lazy_blk.label.clone();
                        lazy_blk.br(&merge_label);

                        ctx.current_block = fast_idx;
                        let fast_blk = ctx.block();
                        let idx_i64 = fast_blk.zext(I32, &idx_i32, I64);
                        let byte_offset = fast_blk.shl(I64, &idx_i64, "3");
                        let with_header = fast_blk.add(I64, &byte_offset, "8");
                        let element_addr = fast_blk.add(I64, &arr_handle, &with_header);
                        let element_ptr = fast_blk.inttoptr(I64, &element_addr);
                        let fast_raw = fast_blk.load(DOUBLE, &element_ptr);
                        // Issue #323: `new Array(n)` slots are pre-filled with
                        // TAG_HOLE so `Object.keys` / `in` can distinguish a
                        // never-written slot from `arr[i] = undefined`. Reads
                        // must translate HOLE → UNDEFINED so user code never
                        // sees the sentinel. Branchless select.
                        let fast_raw_bits = fast_blk.bitcast_double_to_i64(&fast_raw);
                        let is_hole =
                            fast_blk.icmp_eq(I64, &fast_raw_bits, crate::nanbox::TAG_HOLE_I64);
                        let undef_d =
                            fast_blk.bitcast_i64_to_double(crate::nanbox::TAG_UNDEFINED_I64);
                        let fast_val = fast_blk.select(
                            crate::types::I1,
                            &is_hole,
                            DOUBLE,
                            &undef_d,
                            &fast_raw,
                        );
                        let fast_end_label = fast_blk.label.clone();
                        fast_blk.br(&merge_label);

                        ctx.current_block = merge_idx;
                        return Ok(ctx.block().phi(
                            DOUBLE,
                            &[(&fast_val, &fast_end_label), (&lazy_val, &lazy_end_label)],
                        ));
                    }
                }

                let arr_box = lower_expr(ctx, object)?;
                let idx_double = lower_expr(ctx, index)?;
                let blk = ctx.block();
                let arr_bits = blk.bitcast_double_to_i64(&arr_box);
                let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);
                let idx_i32 = blk.fptosi(DOUBLE, &idx_double, I32);

                // Issue #179 Phase 3: lazy-array guard on the inline
                // IndexGet path. A `LazyArrayHeader` lives at the
                // same pointer but has (magic, root_idx, tape_len,
                // blob_str, materialized) after offset 0's
                // `cached_length`, NOT element f64s. Reading `arr +
                // 8 + idx*8` on a lazy value returns garbage. Check
                // `GcHeader::obj_type` at `arr - 8` before the
                // element load; if it's `GC_TYPE_LAZY_ARRAY` (9),
                // route through `js_array_get_f64` which funnels
                // every access through `clean_arr_ptr` —
                // `clean_arr_ptr` force-materializes lazy values
                // idempotently and returns the `ArrayHeader`-backed
                // tree. Adds 2 instructions (sub + load i8) + one
                // comparison + one branch on the fast path; the cost
                // is in the same order as the existing null-guard
                // and tag checks.
                let gc_type_addr = blk.sub(I64, &arr_handle, "8");
                let gc_type_ptr = blk.inttoptr(I64, &gc_type_addr);
                let gc_type = blk.load(I8, &gc_type_ptr);
                let is_lazy = blk.icmp_eq(I8, &gc_type, "9"); // GC_TYPE_LAZY_ARRAY
                                                              // Issue #233: also detect FORWARDED arrays (post-grow
                                                              // stale pointers from async-fn parameter handoff). The
                                                              // slow path's `clean_arr_ptr` follows forwarding
                                                              // chains. Same lazy_idx branch (slow path) handles
                                                              // both shapes correctly.
                let gc_flags_addr = blk.sub(I64, &arr_handle, "7");
                let gc_flags_ptr = blk.inttoptr(I64, &gc_flags_addr);
                let gc_flags = blk.load(I8, &gc_flags_ptr);
                let fwd_bits = blk.and(I8, &gc_flags, "128"); // GC_FLAG_FORWARDED
                let is_fwd = blk.icmp_ne(I8, &fwd_bits, "0");
                let needs_slow = blk.or(I1, &is_lazy, &is_fwd);

                let lazy_idx = ctx.new_block("arr.lazy");
                let fast_idx = ctx.new_block("arr.fast");
                let merge_idx = ctx.new_block("arr.merge");
                let lazy_label = ctx.block_label(lazy_idx);
                let fast_label = ctx.block_label(fast_idx);
                let merge_label = ctx.block_label(merge_idx);
                ctx.block().cond_br(&needs_slow, &lazy_label, &fast_label);

                // Lazy branch: js_array_get_f64 → clean_arr_ptr →
                // force_materialize_lazy → element load on the
                // materialized tree. Subsequent calls hit the
                // cached `materialized` pointer — O(1) after the
                // first access.
                ctx.current_block = lazy_idx;
                let lazy_blk = ctx.block();
                let lazy_val = lazy_blk.call(
                    DOUBLE,
                    "js_array_get_f64",
                    &[(I64, &arr_handle), (I32, &idx_i32)],
                );
                let lazy_end_label = lazy_blk.label.clone();
                lazy_blk.br(&merge_label);

                // Fast branch: unchanged inline load with bounds check.
                ctx.current_block = fast_idx;
                let fast_blk = ctx.block();
                let len_i32 = fast_blk.safe_load_i32_from_ptr(&arr_handle);
                let in_bounds = fast_blk.icmp_ult(I32, &idx_i32, &len_i32);
                let ok_idx = ctx.new_block("arr.ok");
                let oob_idx = ctx.new_block("arr.oob");
                let ok_label = ctx.block_label(ok_idx);
                let oob_label = ctx.block_label(oob_idx);
                ctx.block().cond_br(&in_bounds, &ok_label, &oob_label);
                ctx.current_block = ok_idx;
                let blk = ctx.block();
                let idx_i64 = blk.zext(I32, &idx_i32, I64);
                let byte_offset = blk.shl(I64, &idx_i64, "3");
                let with_header = blk.add(I64, &byte_offset, "8");
                let element_addr = blk.add(I64, &arr_handle, &with_header);
                let element_ptr = blk.inttoptr(I64, &element_addr);
                let raw = blk.load(DOUBLE, &element_ptr);
                // Issue #323: HOLE → UNDEFINED translation (see bounded-index
                // path above). Branchless select keeps the hot path tight.
                let raw_bits = blk.bitcast_double_to_i64(&raw);
                let is_hole = blk.icmp_eq(I64, &raw_bits, crate::nanbox::TAG_HOLE_I64);
                let undef_d = blk.bitcast_i64_to_double(crate::nanbox::TAG_UNDEFINED_I64);
                let val = blk.select(crate::types::I1, &is_hole, DOUBLE, &undef_d, &raw);
                let ok_end_label = ctx.block().label.clone();
                ctx.block().br(&merge_label);
                ctx.current_block = oob_idx;
                let undef_bits = crate::nanbox::i64_literal(crate::nanbox::TAG_UNDEFINED);
                let undef_val = ctx.block().bitcast_i64_to_double(&undef_bits);
                let oob_end_label = ctx.block().label.clone();
                ctx.block().br(&merge_label);
                ctx.current_block = merge_idx;
                return Ok(ctx.block().phi(
                    DOUBLE,
                    &[
                        (&val, &ok_end_label),
                        (&undef_val, &oob_end_label),
                        (&lazy_val, &lazy_end_label),
                    ],
                ));
            }
            // Generic dynamic object access: stringify the index (no-op
            // for already-string keys, format for numeric keys) and
            // call js_object_get_field_by_name_f64.
            if let Expr::String(literal) = index.as_ref() {
                // Static string key: use the interned StringPool entry
                // so we get the same handle as obj["foo"].
                let obj_box = lower_expr(ctx, object)?;
                let key_idx = ctx.strings.intern(literal);
                let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
                let blk = ctx.block();
                let obj_bits = blk.bitcast_double_to_i64(&obj_box);
                let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
                let key_box = blk.load(DOUBLE, &key_handle_global);
                let key_bits = blk.bitcast_double_to_i64(&key_box);
                let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                return Ok(blk.call(
                    DOUBLE,
                    "js_object_get_field_by_name_f64",
                    &[(I64, &obj_handle), (I64, &key_raw)],
                ));
            }
            if is_string_expr(ctx, index) {
                // Dynamic string key: unbox both pointers and call.
                // `key_handle` routes through `unbox_str_handle` because the
                // key may be an SSO value (e.g. from JSON.parse, .slice, or
                // any short-string-producing op); the runtime fn dereferences
                // it as `*StringHeader`. Issue #214 SSO bug class.
                let obj_box = lower_expr(ctx, object)?;
                let key_box = lower_expr(ctx, index)?;
                let blk = ctx.block();
                let obj_handle = unbox_to_i64(blk, &obj_box);
                let key_handle = unbox_str_handle(blk, &key_box);
                return Ok(blk.call(
                    DOUBLE,
                    "js_object_get_field_by_name_f64",
                    &[(I64, &obj_handle), (I64, &key_handle)],
                ));
            }
            // Last-resort fallback with runtime tag checks on the index.
            // First runtime-check whether the index is a Symbol; if so,
            // dispatch to the symbol-property side table — mirrors the
            // IndexSet branch. Otherwise fall through to string/numeric.
            let obj_box = lower_expr(ctx, object)?;
            let idx_box = lower_expr(ctx, index)?;
            let blk = ctx.block();
            let obj_handle = unbox_to_i64(blk, &obj_box);
            let is_sym_i32 = blk.call(I32, "js_is_symbol", &[(DOUBLE, &idx_box)]);
            let is_sym_bit = blk.icmp_ne(I32, &is_sym_i32, "0");
            let sym_idx = ctx.new_block("iget.sym");
            let nonsym_idx = ctx.new_block("iget.nonsym");
            let str_idx = ctx.new_block("iget.str");
            let num_idx = ctx.new_block("iget.num");
            let merge_idx = ctx.new_block("iget.merge");
            let sym_lbl = ctx.block_label(sym_idx);
            let nonsym_lbl = ctx.block_label(nonsym_idx);
            let str_lbl = ctx.block_label(str_idx);
            let num_lbl = ctx.block_label(num_idx);
            let merge_lbl = ctx.block_label(merge_idx);
            ctx.block().cond_br(&is_sym_bit, &sym_lbl, &nonsym_lbl);
            // Symbol key → side-table get.
            ctx.current_block = sym_idx;
            let v_sym = ctx.block().call(
                DOUBLE,
                "js_object_get_symbol_property",
                &[(DOUBLE, &obj_box), (DOUBLE, &idx_box)],
            );
            let sym_end_lbl = ctx.block().label.clone();
            ctx.block().br(&merge_lbl);
            // Not a symbol → recompute idx_bits in this block.
            ctx.current_block = nonsym_idx;
            let blk = ctx.block();
            let idx_bits = blk.bitcast_double_to_i64(&idx_box);
            let top16 = blk.lshr(I64, &idx_bits, "48");
            // STRING_TAG (0x7FFF = 32767): heap StringHeader pointer.
            let is_str_tag_heap = blk.icmp_eq(I64, &top16, "32767");
            let lower48 = blk.and(I64, &idx_bits, POINTER_MASK_I64);
            let is_valid_ptr = blk.icmp_ugt(I64, &lower48, "4095");
            let is_str_heap = blk.and(crate::types::I1, &is_str_tag_heap, &is_valid_ptr);
            // SHORT_STRING_TAG (0x7FF9 = 32761): inline SSO from JSON.parse,
            // .slice, etc. Lower 48 encode length+bytes, NOT a pointer, so we
            // can't AND-mask to a StringHeader; route through unbox_str_handle
            // which materializes SSO to a heap StringHeader (issue #434).
            let is_str_tag_sso = blk.icmp_eq(I64, &top16, "32761");
            let is_str = blk.or(crate::types::I1, &is_str_heap, &is_str_tag_sso);
            ctx.block().cond_br(&is_str, &str_lbl, &num_lbl);
            // String key → object field access.
            ctx.current_block = str_idx;
            let blk = ctx.block();
            let key_handle = unbox_str_handle(blk, &idx_box);
            let v_str = blk.call(
                DOUBLE,
                "js_object_get_field_by_name_f64",
                &[(I64, &obj_handle), (I64, &key_handle)],
            );
            let str_end_lbl = ctx.block().label.clone();
            ctx.block().br(&merge_lbl);
            // Numeric key → polymorphic dispatch.
            //
            // Closes #471 (read side, paired with the IndexSet polymorphic
            // fix above): the previous fallback emitted an inline
            // `obj_handle + 8 + idx*8` load on the assumption that the
            // receiver had an ArrayHeader (8-byte header) layout. Once the
            // IndexSet path stopped writing through that layout for plain
            // objects, the read side had to follow — `constMap[i] = v;
            // constMap[i]` would otherwise set via the object setter
            // (key stringified into the keys_array) and read from
            // `obj+8+i*8` (stale ObjectHeader fields), returning garbage
            // f64 values.
            //
            // Route through the runtime which checks the receiver's GC
            // type and dispatches: arrays/lazy/buffers/typed-arrays
            // through js_array_get_f64 (handles forwarding-chain follow
            // + lazy-materialize + per-kind reads), plain objects
            // through stringify-the-index + js_object_get_field_by_name_f64.
            ctx.current_block = num_idx;
            let v_num = ctx.block().call(
                DOUBLE,
                "js_object_get_index_polymorphic",
                &[(I64, &obj_handle), (DOUBLE, &idx_box)],
            );
            let num_end_lbl = ctx.block().label.clone();
            ctx.block().br(&merge_lbl);
            // Merge.
            ctx.current_block = merge_idx;
            Ok(ctx.block().phi(
                DOUBLE,
                &[
                    (&v_sym, &sym_end_lbl),
                    (&v_str, &str_end_lbl),
                    (&v_num, &num_end_lbl),
                ],
            ))
        }

        // Phase H err: `agg.errors.length` — receiver is
        // PropertyGet(.., "errors") which resolves to a NaN-boxed
        // ArrayHeader pointer (via the dedicated "errors" arm below).
        // Inline-read length at offset 0 just like any other array.
        // Placed ahead of the generic length fast path so we don't
        // need static type analysis to recognize the shape.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
