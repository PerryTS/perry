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
        Expr::BoxedPrimitiveNew { kind, arg } => {
            let v = lower_expr(ctx, arg)?;
            let runtime = match kind {
                perry_hir::BoxedPrimitiveKind::Number => "js_boxed_number_new",
                perry_hir::BoxedPrimitiveKind::String => "js_boxed_string_new",
                perry_hir::BoxedPrimitiveKind::Boolean => "js_boxed_boolean_new",
            };
            Ok(ctx.block().call(DOUBLE, runtime, &[(DOUBLE, &v)]))
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
                let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                let mut vals: Vec<String> = Vec::with_capacity(7);
                for a in args.iter().take(7) {
                    vals.push(lower_expr(ctx, a)?);
                }
                while vals.len() < 7 {
                    vals.push(undef.clone());
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
            let cb_handle = unbox_to_i64(blk, &cb_box);
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
            let cb_handle = unbox_to_i64(blk, &cb_box);
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
            let cb_handle = unbox_to_i64(blk, &cb_box);
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
            let cb_handle = unbox_to_i64(blk, &cb_box);
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
            Ok(double_literal(0.0))
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
            let p_handle = unbox_to_i64(blk, &p_box);
            let result = blk.call(I64, "js_path_to_namespaced_path", &[(I64, &p_handle)]);
            Ok(nanbox_string_inline(blk, &result))
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
                "js_object_has_property",
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
            return Ok(ctx
                .block()
                .call(DOUBLE, "js_number_is_nan", &[(DOUBLE, &v)]));
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
            // it to `js_buffer_alloc` so we don't read garbage from a number-as-array.
            // Other shapes flow through `js_uint8array_from_array` which reads
            // from the array storage region.
            match arg.as_deref() {
                None => {
                    let blk = ctx.block();
                    let h = blk.call(I64, "js_buffer_alloc", &[(I32, "0"), (I32, "0")]);
                    Ok(nanbox_pointer_inline(blk, &h))
                }
                Some(Expr::Integer(n)) => {
                    let size_str = (*n as i32).to_string();
                    let blk = ctx.block();
                    let h = blk.call(I64, "js_buffer_alloc", &[(I32, &size_str), (I32, "0")]);
                    Ok(nanbox_pointer_inline(blk, &h))
                }
                Some(Expr::Number(n))
                    if n.fract() == 0.0 && *n >= 0.0 && *n < (i32::MAX as f64) =>
                {
                    let size_str = (*n as i32).to_string();
                    let blk = ctx.block();
                    let h = blk.call(I64, "js_buffer_alloc", &[(I32, &size_str), (I32, "0")]);
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
            Ok(blk.sitofp(I32, &len_i32, DOUBLE))
        }
        Expr::Uint8ArrayGet { array, index } => {
            // Inline `buf[idx]` for statically-typed Buffer / Uint8Array (issue #47).
            // The bounds check uses `@llvm.assume` instead of a branch: we tell
            // LLVM the access IS in-bounds (which it always is for the dominant
            // pattern: clamped indices in image processing / codec loops). This
            // eliminates the control-flow diamond that blocked the LoopVectorizer.
            // For truly OOB accesses, the assume is UB — but Perry's Buffer.alloc
            // always pads to arena-block alignment, so reading 1 byte past the
            // declared length never faults; the result is just garbage (same as
            // the branch-based path's "return 0" semantics are rarely observed
            // in practice).
            //
            // Fast path: when `array` is a `LocalGet` whose LocalId has a
            // pre-computed `ptr`-typed data-base slot (populated by the
            // `Stmt::Let` lowering for `BufferAlloc` inits), use
            // `getelementptr inbounds i8, ptr %base, i32 %idx` instead of the
            // `inttoptr(handle + offset)` chain — LLVM's LoopVectorizer needs
            // proper pointer provenance to identify array bounds, and per-
            // buffer alias scope metadata so it can prove src reads don't
            // alias dst writes.
            let buffer_slot_info = if let Expr::LocalGet(id) = array.as_ref() {
                ctx.buffer_data_slots.get(id).cloned()
            } else {
                None
            };
            // Check upfront whether index is i32-lowerable (no clones —
            // borrows released before lower_expr_as_i32 borrows mutably).
            let idx_is_i32 = can_lower_expr_as_i32(
                index,
                &ctx.i32_counter_slots,
                ctx.flat_const_arrays,
                &ctx.array_row_aliases,
                ctx.integer_locals,
                ctx.clamp3_functions,
                ctx.clamp_u8_functions,
            );
            let idx_i32 = if idx_is_i32 {
                lower_expr_as_i32(ctx, index)?
            } else {
                let i = lower_expr(ctx, index)?;
                ctx.block().fptosi(DOUBLE, &i, I32)
            };
            if let Some((ptr_slot, scope_idx)) = buffer_slot_info {
                let blk = ctx.block();
                let data_ptr = blk.load(PTR, &ptr_slot);
                // Length lives 8 bytes before the data start (BufferHeader).
                // Loaded with !invariant.load so LICM hoists it out of loops.
                let header_ptr = blk.gep(I8, &data_ptr, &[(I32, "-8")]);
                let len_i32 = blk.load_invariant(I32, &header_ptr);
                let in_bounds = blk.icmp_ult(I32, &idx_i32, &len_i32);
                blk.emit_raw(format!("call void @llvm.assume(i1 {})", in_bounds));
                let byte_ptr = blk.gep_inbounds(I8, &data_ptr, &[(I32, &idx_i32)]);
                let byte_val = blk.fresh_reg();
                let meta = buffer_alias_metadata_suffix(scope_idx);
                blk.emit_raw(format!("{} = load i8, ptr {}{}", byte_val, byte_ptr, meta));
                let result_i32 = blk.zext(I8, &byte_val, I32);
                return Ok(ctx.block().sitofp(I32, &result_i32, DOUBLE));
            }
            // Issue #1205 slow path: route the indexed read through
            // `js_buffer_get` so a view receiver (registered in the
            // runtime view registry by `js_buffer_slice`) reads from
            // the ultimate backing buffer instead of its own
            // possibly-stale snapshot.  Fast path above stays direct
            // since `buffer_data_slots` is only populated for
            // `Buffer.alloc` locals, which are never views.
            let a = lower_expr(ctx, array)?;
            let blk = ctx.block();
            let handle = unbox_to_i64(blk, &a);
            let byte_i32 = blk.call(I32, "js_buffer_get", &[(I64, &handle), (I32, &idx_i32)]);
            Ok(ctx.block().sitofp(I32, &byte_i32, DOUBLE))
        }
        Expr::Uint8ArraySet {
            array,
            index,
            value,
        } => {
            // Inline `buf[idx] = v` — branchless via @llvm.assume.
            // Uses i32 fast path for both index and value when possible,
            // eliminating double↔int conversions in tight byte-write loops.
            let buffer_slot_info = if let Expr::LocalGet(id) = array.as_ref() {
                ctx.buffer_data_slots.get(id).cloned()
            } else {
                None
            };
            let idx_is_i32 = can_lower_expr_as_i32(
                index,
                &ctx.i32_counter_slots,
                ctx.flat_const_arrays,
                &ctx.array_row_aliases,
                ctx.integer_locals,
                ctx.clamp3_functions,
                ctx.clamp_u8_functions,
            );
            let val_is_i32 = can_lower_expr_as_i32(
                value,
                &ctx.i32_counter_slots,
                ctx.flat_const_arrays,
                &ctx.array_row_aliases,
                ctx.integer_locals,
                ctx.clamp3_functions,
                ctx.clamp_u8_functions,
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
            if let Some((ptr_slot, scope_idx)) = buffer_slot_info {
                let blk = ctx.block();
                let data_ptr = blk.load(PTR, &ptr_slot);
                let header_ptr = blk.gep(I8, &data_ptr, &[(I32, "-8")]);
                let len_i32 = blk.load_invariant(I32, &header_ptr);
                let in_bounds = blk.icmp_ult(I32, &idx_i32, &len_i32);
                blk.emit_raw(format!("call void @llvm.assume(i1 {})", in_bounds));
                let byte_ptr = blk.gep_inbounds(I8, &data_ptr, &[(I32, &idx_i32)]);
                let byte_val = blk.trunc(I32, &val_i32, I8);
                let meta = buffer_alias_metadata_suffix(scope_idx);
                blk.emit_raw(format!("store i8 {}, ptr {}{}", byte_val, byte_ptr, meta));
                return Ok(ctx.block().sitofp(I32, &val_i32, DOUBLE));
            }
            // Issue #1205 slow path: route the indexed store through
            // `js_buffer_set` so a view receiver propagates the write
            // to its backing buffer (and any sister views).  Fast
            // path above stays direct — `buffer_data_slots` only
            // tracks `Buffer.alloc` locals, which are never views.
            let a = lower_expr(ctx, array)?;
            let blk = ctx.block();
            let handle = unbox_to_i64(blk, &a);
            blk.call_void(
                "js_buffer_set",
                &[(I64, &handle), (I32, &idx_i32), (I32, &val_i32)],
            );
            // Return the stored value as a double (for expression contexts).
            Ok(ctx.block().sitofp(I32, &val_i32, DOUBLE))
        }

        // `new Int32Array([1,2,3])` etc. — generic typed array constructor.
        // Routes through `js_typed_array_new_empty(kind, length)` for
        // compile-time-constant numeric lengths, or `js_typed_array_new(kind, val)`
        // for runtime-dispatched arguments (which inspects the NaN-box tag to
        // distinguish a numeric length from a source-array pointer).
        // Result is a raw pointer bitcast to f64 (no NaN-box tag) — the runtime
        // formatter and `js_array_*` dispatch helpers detect it via TYPED_ARRAY_REGISTRY.
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
                    Ok(ctx.block().bitcast_i64_to_double(&p))
                }
                Some(arg_expr) => match arg_expr.as_ref() {
                    // Literal integer length: `new Int32Array(3)`.
                    Expr::Integer(n) => {
                        let len_str = (*n as i32).max(0).to_string();
                        let p = ctx.block().call(
                            I64,
                            "js_typed_array_new_empty",
                            &[(I32, &kind_str), (I32, &len_str)],
                        );
                        Ok(ctx.block().bitcast_i64_to_double(&p))
                    }
                    // Literal float that is a non-negative integer: `new Int32Array(3.0)`.
                    Expr::Number(f) if f.fract() == 0.0 && *f >= 0.0 && *f < (i32::MAX as f64) => {
                        let len_str = (*f as i32).to_string();
                        let p = ctx.block().call(
                            I64,
                            "js_typed_array_new_empty",
                            &[(I32, &kind_str), (I32, &len_str)],
                        );
                        Ok(ctx.block().bitcast_i64_to_double(&p))
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
                        Ok(blk.bitcast_i64_to_double(&p))
                    }
                },
            }
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
            // Write back to the local's storage.
            if let Some(&capture_idx) = ctx.closure_captures.get(array_id) {
                let closure_ptr = ctx
                    .current_closure_ptr
                    .clone()
                    .ok_or_else(|| anyhow!("ArrayUnshift captured but no current_closure_ptr"))?;
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
            }
            let blk = ctx.block();
            let len_i32 = blk.call(I32, "js_array_length", &[(I64, &new_handle)]);
            let len_f64 = blk.sitofp(I32, &len_i32, DOUBLE);
            Ok(len_f64)
        }

        // -------- arr.entries() / .keys() / .values() (eager) --------
        Expr::ArrayEntries(arr) => {
            let arr_box = lower_expr(ctx, arr)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let result = blk.call(I64, "js_array_entries", &[(I64, &arr_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }
        Expr::ArrayKeys(arr) => {
            let arr_box = lower_expr(ctx, arr)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let result = blk.call(I64, "js_array_keys", &[(I64, &arr_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }
        Expr::ArrayValues(arr) => {
            let arr_box = lower_expr(ctx, arr)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let result = blk.call(I64, "js_array_values", &[(I64, &arr_handle)]);
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
