//! Array index-set fast-path lowering (extracted from `expr.rs`, issue
//! #1098). Pure move — no logic changes.

use anyhow::{anyhow, Result};

use super::{
    emit_array_numeric_write_note_on_block, emit_jsvalue_slot_store_on_block,
    emit_jsvalue_slot_store_scalar_aware_on_block, emit_write_barrier_slot_on_block,
    nanbox_pointer_inline, raw_f64_layout_fact, FnCtx,
};
use crate::block::LlBlock;
use crate::nanbox::POINTER_MASK_I64;
use crate::native_value::{
    BoundsState, BufferAccessMode, LoweredValue, MaterializationReason, NativeRep, SemanticKind,
};
use crate::types::{DOUBLE, I1, I32, I64};

fn canonicalize_raw_f64_numeric_store_value(blk: &mut LlBlock, value_double: &str) -> String {
    blk.call(
        DOUBLE,
        "js_array_numeric_value_to_raw_f64",
        &[(DOUBLE, value_double)],
    )
}

/// Inline fast-path lowering for `local_arr[i] = v`.
///
/// Compiles to:
///
/// ```text
///   <current>:
///     %arr_handle = unbox(arr_box)
///     %idx_i32 = fptosi %idx
///     %guard_ok = call @js_typed_feedback_plain_array_index_set_guard(...)
///     br i1 %guard_ok, label %guarded, label %fallback
///
///   <guarded>:
///     %length = load i32, ptr @ arr_handle+0
///     %in_bounds = icmp ult %idx_i32, %length
///     br i1 %in_bounds, label %fast_inbounds, label %check_capacity
///
///   fast_inbounds:
///     ; element_ptr = arr_handle + 8 + idx*8
///     store double %v, ptr %element_ptr
///     br merge
///
///   check_capacity:
///     %capacity = load i32, ptr @ arr_handle+4
///     %within_cap = icmp ult %idx_i32, %capacity
///     %dense_append = icmp eq %idx_i32, %length
///     %can_extend_inline = and %within_cap, %dense_append
///     br i1 %can_extend_inline, label %extend_inline, label %runtime_extend
///
///   extend_inline:
///     store double %v, ptr %element_ptr
///     %new_len = add i32 %idx, 1
///     store i32 %new_len, ptr @ arr_handle+0
///     br merge
///
///   runtime_extend:
///     %new_handle = call i64 @js_array_set_f64_extend(...)
///     %new_box = nanbox_pointer_inline(new_handle)
///     store double %new_box, ptr %local_slot
///     br merge
///
///   fallback:
///     %new_box = call double @js_typed_feedback_array_index_set_fallback_boxed(...)
///     store double %new_box, ptr %local_slot
///     br merge
///
///   merge:
///     <continues here>
/// ```
///
/// The inline store paths are entered only after the runtime guard proves the
/// receiver is a live, non-forwarded plain array with a sane header. The realloc
/// path also handles sparse extensions so holes are filled and numeric raw
/// layout is downgraded before JavaScript can observe the gap.
pub(crate) fn lower_index_set_fast(
    ctx: &mut FnCtx<'_>,
    arr_box: &str,
    idx_double: &str,
    val_double: &str,
    local_id: u32,
    layout_note_needed: bool,
    write_barrier_needed: bool,
    value_is_numeric: bool,
    require_numeric_layout: bool,
    feedback_site_id: &str,
) -> Result<()> {
    // Capture the local slot for the realloc path.
    let slot = ctx
        .locals
        .get(&local_id)
        .ok_or_else(|| anyhow!("IndexSet: local {} not in scope", local_id))?
        .clone();

    // Unbox the array pointer.
    let blk = ctx.block();
    let arr_bits = blk.bitcast_double_to_i64(arr_box);
    let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);
    let idx_i32 = blk.fptosi(DOUBLE, idx_double, I32);

    let guarded_idx = ctx.new_block("idxset.guarded");
    let guard_fallback_idx = ctx.new_block("idxset.guard_fallback");
    let inbounds_idx = ctx.new_block("idxset.inbounds");
    let check_cap_idx = ctx.new_block("idxset.check_cap");
    let extend_inline_idx = ctx.new_block("idxset.extend_inline");
    let realloc_idx = ctx.new_block("idxset.realloc");
    let merge_idx = ctx.new_block("idxset.merge");

    let guarded_label = ctx.block_label(guarded_idx);
    let guard_fallback_label = ctx.block_label(guard_fallback_idx);
    let inbounds_label = ctx.block_label(inbounds_idx);
    let check_cap_label = ctx.block_label(check_cap_idx);
    let extend_inline_label = ctx.block_label(extend_inline_idx);
    let realloc_label = ctx.block_label(realloc_idx);
    let merge_label = ctx.block_label(merge_idx);

    // Runtime guard before any ArrayHeader read or raw element store. This
    // rejects dynamic/cross-boundary receivers, lazy arrays, stale forwarded
    // heads, and corrupt layouts; the fallback then uses boxed JSValue
    // semantics and writes the returned receiver back to the local slot.
    let guard_ok = {
        let blk = ctx.block();
        let guard_fn = if require_numeric_layout {
            "js_typed_feedback_numeric_array_index_set_guard"
        } else {
            "js_typed_feedback_plain_array_index_set_guard"
        };
        let guard_i32 = blk.call(
            I32,
            guard_fn,
            &[
                (I64, feedback_site_id),
                (DOUBLE, arr_box),
                (I32, &idx_i32),
                (DOUBLE, val_double),
                (I32, "0"),
            ],
        );
        blk.icmp_ne(I32, &guard_i32, "0")
    };
    ctx.block()
        .cond_br(&guard_ok, &guarded_label, &guard_fallback_label);

    ctx.current_block = guard_fallback_idx;
    {
        let fallback_box = ctx.block().call(
            DOUBLE,
            "js_typed_feedback_array_index_set_fallback_boxed",
            &[
                (I64, feedback_site_id),
                (DOUBLE, arr_box),
                (DOUBLE, idx_double),
                (DOUBLE, val_double),
            ],
        );
        ctx.block().store(DOUBLE, &fallback_box, &slot);
        ctx.block().br(&merge_label);
        if require_numeric_layout {
            let fallback = LoweredValue {
                semantic: SemanticKind::JsValue,
                rep: NativeRep::JsValue,
                llvm_ty: DOUBLE,
                value: fallback_box,
            };
            ctx.record_lowered_value_with_access_mode_and_facts(
                "NumericArrayIndexSet",
                Some(local_id),
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
                        Some(local_id),
                        "rejected",
                        "numeric_array_index_set_guard",
                        Some(MaterializationReason::RuntimeApi),
                    ),
                    raw_f64_layout_fact(
                        Some(local_id),
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
    }

    ctx.current_block = guarded_idx;
    // Load length from offset 0 (null-guarded).
    let length = ctx.block().safe_load_i32_from_ptr(&arr_handle);
    let in_bounds = ctx.block().icmp_ult(I32, &idx_i32, &length);
    ctx.block()
        .cond_br(&in_bounds, &inbounds_label, &check_cap_label);

    // Helper: compute element_ptr = arr_ptr + 8 + idx*8.
    fn element_slot(blk: &mut LlBlock, arr_handle: &str, idx_i32: &str) -> (String, String) {
        let idx_i64 = blk.zext(I32, idx_i32, I64);
        let byte_offset = blk.shl(I64, &idx_i64, "3"); // *8
        let with_header = blk.add(I64, &byte_offset, "8"); // +8 for header
        let element_addr = blk.add(I64, arr_handle, &with_header);
        let element_ptr = blk.inttoptr(I64, &element_addr);
        (element_addr, element_ptr)
    }

    // FASTEST: in-bounds path. Store directly, jump to merge.
    ctx.current_block = inbounds_idx;
    {
        let blk = ctx.block();
        let (element_addr, element_ptr) = element_slot(blk, &arr_handle, &idx_i32);
        if require_numeric_layout {
            let numeric_value = canonicalize_raw_f64_numeric_store_value(blk, val_double);
            // GC_STORE_AUDIT(POINTER_FREE): require_numeric_layout proves the
            // array is raw-f64 and the value is canonicalized to a plain f64 —
            // no GC pointer is written into the slot, so no write barrier.
            blk.store(DOUBLE, &numeric_value, &element_ptr);
        } else {
            // In-place overwrite of a non-raw-layout (e.g. downgraded `any[]`)
            // array element: the slot holds a valid value, so the scalar-aware
            // note skips the GC layout hashmap on scalar-over-scalar stores
            // (#5094 — ~9× on bench_numeric_array_downgrade).
            let value_bits = emit_jsvalue_slot_store_scalar_aware_on_block(
                blk,
                &element_ptr,
                val_double,
                &arr_handle,
                &idx_i32,
                layout_note_needed,
                &arr_handle,
                &element_addr,
                write_barrier_needed,
            )
            .unwrap_or_else(|| blk.bitcast_double_to_i64(val_double));
            if !value_is_numeric {
                emit_array_numeric_write_note_on_block(blk, &arr_handle, &value_bits);
            }
        }
        blk.br(&merge_label);
    }
    if require_numeric_layout {
        let stored = LoweredValue {
            semantic: SemanticKind::JsNumber,
            rep: NativeRep::F64,
            llvm_ty: DOUBLE,
            value: val_double.to_string(),
        };
        ctx.record_lowered_value_with_access_mode_and_facts(
            "NumericArrayIndexSet",
            Some(local_id),
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
                Some(local_id),
                "consumed",
                "numeric_array_index_set_guard",
                None,
            )],
            Vec::new(),
            false,
            false,
            Vec::new(),
        );
    }

    // MEDIUM: idx == length and idx < capacity. Store + bump length.
    // Sparse writes must go through `js_array_set_f64_extend` so gaps become
    // TAG_HOLE and raw numeric layout is downgraded before user-visible reads.
    ctx.current_block = check_cap_idx;
    let capacity = {
        let blk = ctx.block();
        // Load capacity from offset 4 — we need a typed pointer that
        // points 4 bytes into the array header. Use inttoptr after add.
        let cap_addr = blk.add(I64, &arr_handle, "4");
        let cap_ptr = blk.inttoptr(I64, &cap_addr);
        blk.load(I32, &cap_ptr)
    };
    let can_extend_inline = {
        let blk = ctx.block();
        let within_cap = blk.icmp_ult(I32, &idx_i32, &capacity);
        let dense_append = blk.icmp_eq(I32, &idx_i32, &length);
        blk.and(I1, &within_cap, &dense_append)
    };
    ctx.block()
        .cond_br(&can_extend_inline, &extend_inline_label, &realloc_label);

    ctx.current_block = extend_inline_idx;
    {
        let blk = ctx.block();
        let (element_addr, element_ptr) = element_slot(blk, &arr_handle, &idx_i32);
        if require_numeric_layout {
            let numeric_value = canonicalize_raw_f64_numeric_store_value(blk, val_double);
            // GC_STORE_AUDIT(POINTER_FREE): require_numeric_layout proves the
            // array is raw-f64 and the value is canonicalized to a plain f64 —
            // no GC pointer is written into the slot, so no write barrier.
            blk.store(DOUBLE, &numeric_value, &element_ptr);
        } else {
            let value_bits = emit_jsvalue_slot_store_on_block(
                blk,
                &element_ptr,
                val_double,
                &arr_handle,
                &idx_i32,
                layout_note_needed,
                &arr_handle,
                &element_addr,
                write_barrier_needed,
            )
            .unwrap_or_else(|| blk.bitcast_double_to_i64(val_double));
            if !value_is_numeric {
                emit_array_numeric_write_note_on_block(blk, &arr_handle, &value_bits);
            }
        }
        // Bump length: store idx+1 to arr_ptr+0.
        let new_len = blk.add(I32, &idx_i32, "1");
        let len_ptr = blk.inttoptr(I64, &arr_handle); // length is at offset 0
        blk.store(I32, &new_len, &len_ptr);
        blk.br(&merge_label);
    }

    // SLOW: realloc needed. Call the runtime, write new ptr to local.
    ctx.current_block = realloc_idx;
    {
        let blk = ctx.block();
        blk.call_void(
            "js_typed_feedback_record_fallback_call",
            &[(I64, feedback_site_id)],
        );
        // Strict `arr[i] = v`: a frozen array's element is non-writable and a
        // non-extensible array rejects a new index, so route to the throwing
        // variant. (The inline fast/medium paths above are only reached for
        // arrays with a proven dense-numeric layout, which excludes frozen /
        // sealed / non-extensible arrays — those always fall to this call.)
        let new_handle = blk.call(
            I64,
            "js_array_set_f64_extend_strict",
            &[(I64, &arr_handle), (I32, &idx_i32), (DOUBLE, val_double)],
        );
        let new_box = nanbox_pointer_inline(blk, &new_handle);
        blk.store(DOUBLE, &new_box, &slot);
        let val_bits = blk.bitcast_double_to_i64(val_double);
        emit_write_barrier_slot_on_block(blk, &arr_handle, "0", &val_bits);
        blk.br(&merge_label);
    }

    ctx.current_block = merge_idx;
    Ok(())
}
