//! Closure expressions.
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
        Expr::Closure {
            func_id,
            params,
            body,
            captures,
            mutable_captures,
            captures_this,
            is_async,
            ..
        } => {
            // captures_this used to be a hard error here. Phase H.3
            // initializes the closure's `this_stack` with a sentinel
            // when enclosing_class is set, so the body lowering won't
            // crash on `this` references — they just produce garbage
            // until full this-capture support lands. The wrong-but-
            // doesn't-crash trade unblocks dozens of test files.
            //
            // Async-closure handling (post #1021 phase 2): async closures
            // whose body contains an `await` are pre-rewritten upstream
            // by `transform_async_to_generator` (via
            // `transform_plain_async_closure_body` in
            // `perry-transform/src/generator.rs`). By the time codegen sees
            // them, the rewrite has flipped `is_async` to false and the
            // body is a state machine returning a Promise. What still
            // arrives here with `is_async: true` is async closures
            // *without* awaits — for those the body just runs once and
            // returns its value, and the caller's `await` (if any) wraps
            // it in `Promise.resolve(value)` semantics via the surrounding
            // codegen. No state-machine wrapping needed here.
            let _ = is_async;
            // mutable_captures uses the same get/set runtime path —
            // they work as long as the outer scope doesn't also access
            // the captured variable after the closure is created.
            let _ = mutable_captures;

            // Auto-detect captures from the body. The HIR's captures
            // list is sometimes empty for closures passed as arguments
            // (the closure conversion pass doesn't visit every site).
            // We must detect the same set as `compile_closure` so the
            // creation site and the body lower with consistent slot
            // indices.
            let auto_captures = compute_auto_captures(ctx, params, body, captures);

            // Lower each captured value from the OUTER scope (this is
            // an outer-scope access, NOT a closure capture access — at
            // closure creation we're still outside the closure body).
            //
            // Boxed captures are special: the CAPTURE VALUE is the
            // box pointer itself (not the value inside the box). We
            // store the box pointer (as a bit-castable double) in
            // the closure's capture slot, so reads/writes inside the
            // closure body can deref it via js_box_get/set. Without
            // this, each closure would get a snapshot of the box's
            // current value.
            let mut captured_values: Vec<String> = Vec::with_capacity(auto_captures.len());
            for cap_id in &auto_captures {
                if ctx.boxed_vars.contains(cap_id) {
                    // If the enclosing function has this id boxed,
                    // we want to forward the BOX POINTER through
                    // the capture slot, not the value inside the
                    // box. Read the slot (which holds the box
                    // pointer bit-cast to double) directly without
                    // going through the normal LocalGet path (which
                    // would deref via js_box_get).
                    if let Some(&_capture_idx) = ctx.closure_captures.get(cap_id) {
                        // We're inside a closure and this id is a
                        // transitively-captured box. Read the
                        // capture slot RAW (it holds the box ptr
                        // as a double) and propagate directly.
                        let closure_ptr = ctx.current_closure_ptr.clone().ok_or_else(|| {
                            anyhow!("nested boxed capture but no current_closure_ptr")
                        })?;
                        let idx_str = _capture_idx.to_string();
                        let v = ctx.block().call(
                            DOUBLE,
                            "js_closure_get_capture_f64",
                            &[(I64, &closure_ptr), (I32, &idx_str)],
                        );
                        captured_values.push(v);
                    } else if let Some(slot) = ctx.locals.get(cap_id).cloned() {
                        // Enclosing function owns the box: slot
                        // holds the box pointer as a double.
                        let v = ctx.block().load(DOUBLE, &slot);
                        captured_values.push(v);
                    } else if let Some(global_name) = ctx.module_globals.get(cap_id).cloned() {
                        // Global boxed var (rare).
                        let g_ref = format!("@{}", global_name);
                        let v = ctx.block().load(DOUBLE, &g_ref);
                        captured_values.push(v);
                    } else {
                        captured_values.push(double_literal(0.0));
                    }
                } else {
                    let v = lower_expr(ctx, &Expr::LocalGet(*cap_id))?;
                    captured_values.push(v);
                }
            }

            // Compute the closure function name BEFORE taking the
            // mutable block borrow.
            let func_name = format!("perry_closure_{}__{}", ctx.strings.module_prefix(), func_id);

            // Closures with `captures_this` reserve one extra capture
            // slot (at index `auto_captures.len()`) for the receiver.
            // `lower_object_literal` patches that slot with the
            // containing object pointer AFTER the closure is built.
            // Arrow-in-class closures leave it at 0.0, the existing
            // non-crashing fallback.
            let total_caps = if *captures_this {
                auto_captures.len() + 1
            } else {
                auto_captures.len()
            };

            let func_ref = format!("@{}", func_name);
            // Issue #450: when `captures_this`, OR in the runtime's
            // `CAPTURES_THIS_FLAG` (0x8000_0000) so the runtime can detect
            // closures whose last capture slot is the reserved `this` slot.
            // `js_closure_alloc` masks the flag off when computing allocation
            // size (real_capture_count) but preserves it in the stored
            // `capture_count` field. Used by `clone_closure_rebind_this` at
            // `Object.defineProperty(obj, k, { get(){}, set(){} })` time so
            // accessor invocation sees `this === obj` per spec, and by
            // `js_closure_unbind_this` for detached method references.
            let cap_count_val = if *captures_this {
                (total_caps as u32) | 0x8000_0000u32
            } else {
                total_caps as u32
            };
            let cap_count = cap_count_val.to_string();
            // Closures with NO captures (and no `this` to patch) are
            // observationally identical across every call site that
            // produces them, so route through `js_closure_alloc_singleton`
            // to share a single ClosureHeader cached by func_ptr.
            //
            // Closures WITH captures route through
            // `js_closure_alloc_with_captures_singleton` whenever none of
            // those captures are mutated by the body (the common case
            // for ECS callbacks like `(eid, arch, compId) => { ...
            // changeset ... }` capturing `this._changeset`). The cache
            // keys on (func_ptr, capture_bits…) so distinct capture
            // values still produce distinct closures; identical
            // (func, captures) at a hot call site re-uses the cached
            // ClosureHeader and skips gc_malloc + gc_check_trigger.
            //
            // We skip the captured-singleton path for closures whose
            // body mutates an unboxed capture: those want fresh
            // per-call identity because the captured slot itself holds
            // mutable state for that invocation.
            //
            // Closures that capture `this` are still routable through
            // the captured-singleton path — we include the `this` value
            // in the cache buffer (at slot `auto_captures.len()`,
            // matching the runtime layout), so distinct receivers
            // produce distinct cache keys. Hot ECS class methods like
            // `World.executeEntityCommands` benefit from this: their
            // inner arrow `(eid, arch, compId) => ... changeset ...`
            // is created per-call but always with the same `this` (the
            // World) and same captures (`this._changeset`).
            // Boxed captures still allow the cache path: the closure
            // stores the BOX POINTER (a stable per-allocation address),
            // and the box's contents are read dynamically inside the
            // body via `js_box_get`. Two closure-literal sites that
            // capture the same boxed local store identical box-pointer
            // bits, so the cache (keyed on bit-equality of capture
            // slots) still hits. The cache backing is a small LRU per
            // func_ptr, which tolerates the parallel-instance pattern
            // (50 concurrent unitOfWork calls each capturing a
            // different `__async_step` box) by holding multiple
            // captures rather than overwriting one slot per call.
            //
            // We previously bailed out when any captured local was
            // boxed (`mutable_captures` non-empty). That made the
            // async-to-generator transform's per-`await` `cb_v` /
            // `cb_e` closures (which capture the boxed `__async_step`
            // self-reference) miss the cache 100% of the time —
            // 2 fresh closure allocs per await ≈ 300 ns of `gc_malloc`
            // work even though the box pointers are stable across
            // call sites. The relaxed gate plus the multi-slot LRU
            // backing reclaims that overhead.
            let no_capture_singleton = total_caps == 0;
            let mut write_ids = std::collections::HashSet::new();
            crate::boxed_vars::collect_write_ids_in_stmts(body, &mut write_ids);
            let writes_unboxed_capture = auto_captures
                .iter()
                .any(|cap_id| !ctx.boxed_vars.contains(cap_id) && write_ids.contains(cap_id));
            let captured_singleton = !no_capture_singleton && !writes_unboxed_capture;

            // For captures_this, the cache buffer needs an extra slot
            // for the `this` value so the cache key distinguishes
            // closures with different receivers. We load `this` here
            // (mirroring the post-create patch site below) when we're
            // taking the captured-singleton path.
            let this_value_for_cache = if captured_singleton && *captures_this {
                let this_slot = ctx.this_stack.last().cloned();
                Some(if let Some(slot) = this_slot {
                    ctx.block().load(DOUBLE, &slot)
                } else {
                    double_literal(0.0)
                })
            } else {
                None
            };

            let closure_handle = if no_capture_singleton {
                let blk = ctx.block();
                blk.call(I64, "js_closure_alloc_singleton", &[(PTR, &func_ref)])
            } else if captured_singleton {
                // Stack-allocate a `[u64; total_caps]` capture buffer
                // (auto captures, plus `this` at the reserved slot if
                // captures_this). The runtime helper copies these
                // verbatim into the cached closure's capture slots.
                let n_total = total_caps;
                let buf = ctx.func.alloca_entry_array(I64, n_total);
                {
                    let blk = ctx.block();
                    for (i, v) in captured_values.iter().enumerate() {
                        let slot = blk.gep(I64, &buf, &[(I64, &format!("{}", i))]);
                        let v_bits = blk.bitcast_double_to_i64(v);
                        blk.store(I64, &v_bits, &slot);
                    }
                    if let Some(this_v) = &this_value_for_cache {
                        let this_idx = auto_captures.len();
                        let slot = blk.gep(I64, &buf, &[(I64, &format!("{}", this_idx))]);
                        let v_bits = blk.bitcast_double_to_i64(this_v);
                        blk.store(I64, &v_bits, &slot);
                    }
                }
                let blk = ctx.block();
                blk.call(
                    I64,
                    "js_closure_alloc_with_captures_singleton",
                    &[(PTR, &func_ref), (I32, &cap_count), (PTR, &buf)],
                )
            } else {
                let blk = ctx.block();
                blk.call(
                    I64,
                    "js_closure_alloc",
                    &[(PTR, &func_ref), (I32, &cap_count)],
                )
            };

            // The captured-singleton helper writes captures internally
            // (so the cached layout matches a fresh allocation). The
            // other paths still need explicit per-slot writes.
            if !captured_singleton {
                let blk = ctx.block();
                for (idx, val) in captured_values.iter().enumerate() {
                    let idx_str = idx.to_string();
                    blk.call_void(
                        "js_closure_set_capture_f64",
                        &[(I64, &closure_handle), (I32, &idx_str), (DOUBLE, val)],
                    );
                }
            }
            // Issue #291: when the closure is built inside a method
            // body (or constructor), the enclosing frame's `this` is the
            // topmost entry on `this_stack`; load and write that into
            // the reserved capture slot. Without this, the closure's
            // `Expr::This` reads back 0.0 and any `this.field` access in
            // the body crashes. Module-level / function-level call sites
            // have an empty `this_stack` — keep the 0.0 sentinel there
            // (matches the previous behavior for top-level arrow expressions
            // that legitimately have no `this` binding).
            if *captures_this {
                let this_idx = auto_captures.len().to_string();
                let this_slot = ctx.this_stack.last().cloned();
                let this_value = if let Some(slot) = this_slot {
                    ctx.block().load(DOUBLE, &slot)
                } else {
                    double_literal(0.0)
                };
                let blk = ctx.block();
                blk.call_void(
                    "js_closure_set_capture_f64",
                    &[
                        (I64, &closure_handle),
                        (I32, &this_idx),
                        (DOUBLE, &this_value),
                    ],
                );
            }
            Ok(nanbox_pointer_inline(ctx.block(), &closure_handle))
        }

        // -------- Classes (Phase C.1) --------
        // `new ClassName(args...)` — allocate an anonymous object,
        // inline-execute the constructor body with `this` bound to the
        // new object, return the NaN-boxed object. No method tables yet,
        // no inheritance — just data classes with constructor field
        // assignments.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
