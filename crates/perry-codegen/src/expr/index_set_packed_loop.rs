//! Packed-loop store lowering for indexed assignment: the f64 and numeric
//! range-loop `arr[i] = expr` paths.
//!
//! Split out of `index_set.rs` to keep it under the 2,000-line file gate.

use anyhow::Result;
use perry_hir::Expr;

use crate::nanbox::POINTER_MASK_I64;
use crate::native_value::{
    BoundsState, BufferAccessMode, LoweredValue, MaterializationReason, NativeRep, SemanticKind,
};
use crate::types::{DOUBLE, I32, I64};

use super::index_set::{canonicalize_raw_f64_numeric_store_value, packed_f64_loop_fact};
use super::{
    array_kind_fact, emit_typed_feedback_register_site, lower_expr, lower_expr_as_i32,
    raw_f64_layout_fact, FnCtx, PackedNumericLoopKind, TypedFeedbackContract, TypedFeedbackKind,
};

fn lower_packed_f64_loop_store_value(
    ctx: &mut FnCtx<'_>,
    arr_id: u32,
    value: &Expr,
) -> Result<(String, Vec<String>)> {
    if let Expr::MathAbs(operand) = value {
        // Only fold to `llvm.fabs.f64` when the inner read is a PROVEN packed-f64
        // load (same array, index is the packed-loop counter). A general
        // `arr[key]` can lower through the boxed/runtime fallback to a NaN-boxed
        // JS value, and `fabs` (a bare sign-bit clear) would skip `Math.abs`'s
        // ToNumber coercion on it.
        if let Expr::IndexGet { object, index } = operand.as_ref() {
            let proven_packed_load = matches!(object.as_ref(), Expr::LocalGet(id) if *id == arr_id)
                && matches!(index.as_ref(), Expr::LocalGet(idx_id)
                    if packed_f64_loop_fact(ctx, arr_id, *idx_id).is_some());
            if proven_packed_load {
                let raw = lower_expr(ctx, operand)?;
                let abs = ctx.block().call(DOUBLE, "llvm.fabs.f64", &[(DOUBLE, &raw)]);
                return Ok((abs, vec!["rhs_unary_math=llvm.fabs.f64".to_string()]));
            }
        }
    }
    Ok((lower_expr(ctx, value)?, Vec::new()))
}

fn lower_packed_numeric_loop_store_value(
    ctx: &mut FnCtx<'_>,
    arr_id: u32,
    value: &Expr,
    array_kind: PackedNumericLoopKind,
) -> Result<(String, String, Vec<String>)> {
    match array_kind {
        PackedNumericLoopKind::F64 => {
            let (value, notes) = lower_packed_f64_loop_store_value(ctx, arr_id, value)?;
            Ok((value.clone(), value, notes))
        }
        PackedNumericLoopKind::I32 => {
            let value_i32 = lower_expr_as_i32(ctx, value)?;
            let value_double = ctx.block().sitofp(I32, &value_i32, DOUBLE);
            Ok((
                value_double,
                value_i32,
                vec!["rhs_i32_store=sitofp_i32_to_raw_f64_slot".to_string()],
            ))
        }
        PackedNumericLoopKind::U32 => {
            // No packed-U32 store fast path exists yet, and the IndexSet caller
            // already routes U32 facts to the generic array-store path (see the
            // `!matches!(.., U32)` guard below). This arm is therefore
            // unreachable in practice; rather than `bail!` (a whole-compile
            // failure) if a future change ever routes a U32 store here, degrade
            // to the F64 full-value store. A uint32 is representable exactly in
            // f64, so storing the full value is always correct — just not the
            // (nonexistent) packed-U32 fast path. See #5464.
            let (value, notes) = lower_packed_f64_loop_store_value(ctx, arr_id, value)?;
            Ok((value.clone(), value, notes))
        }
    }
}

/// #6011: inline store for the hole-tolerant *range-guarded* packed-f64 loop.
///
/// The range guard already proved at loop entry that every index this loop
/// can touch is in bounds, that the receiver is a plain, mutable (not
/// frozen/sealed), descriptor-free array, and that its slots are raw-f64
/// numbers or `TAG_HOLE` — and the matcher proved the body cannot invalidate
/// any of that mid-loop (no calls/closures/awaits, stores only through this
/// path). The only per-iteration check left is on the RHS *value*: a NaN-boxed
/// non-double (string/object/undefined/INT32-boxed int/…) side-exits to the
/// slow loop, which re-executes the current iteration through the generic
/// store (the side exit fires before the store, so nothing double-applies).
/// The store itself is a raw f64 write; overwriting `TAG_HOLE` with a number
/// is exactly JS element definition on an in-bounds index, and a number never
/// carries a heap edge, so no barrier / layout note is needed (the guard
/// (re)asserted the pointer-free GC layout).
pub(super) fn lower_packed_f64_range_loop_index_set(
    ctx: &mut FnCtx<'_>,
    arr_id: u32,
    idx_i32: &str,
    value: &Expr,
    guard_id: &str,
    side_exit_label: &str,
) -> Result<String> {
    // A RHS that provably materializes a genuine double by construction —
    // a literal, the canonical-i32 counter, an in-window guarded load, or
    // float arithmetic over those (the masked store's admission predicate)
    // — needs no runtime value check at all: the nanbox tag test below
    // (fmov + tag math + compare + branch, five per-element instructions)
    // exists only for values that could be boxed at runtime.
    let statically_genuine =
        crate::expr::masked_window::masked_store_rhs_is_genuine_f64(ctx, value);
    let (val_double, rhs_notes) = lower_packed_f64_loop_store_value(ctx, arr_id, value)?;
    if statically_genuine {
        let arr_expr = Expr::LocalGet(arr_id);
        let arr_box = lower_expr(ctx, &arr_expr)?;
        let arr_handle = super::packed_receiver_handle_i64(ctx, Some(arr_id), &arr_box);
        let blk = ctx.block();
        let idx_i64 = blk.zext(I32, idx_i32, I64);
        let byte_offset = blk.shl(I64, &idx_i64, "3");
        let with_header = blk.add(I64, &byte_offset, "8");
        let element_addr = blk.add(I64, &arr_handle, &with_header);
        let element_ptr = blk.inttoptr(I64, &element_addr);
        // GC_STORE_AUDIT(POINTER_FREE): statically-genuine packed store —
        // the RHS predicate proves an unboxed double, never a heap pointer.
        blk.store(DOUBLE, &val_double, &element_ptr);
        let stored = LoweredValue {
            semantic: SemanticKind::JsNumber,
            rep: NativeRep::F64,
            llvm_ty: DOUBLE,
            value: val_double.clone(),
        };
        ctx.record_lowered_value_with_access_mode_and_facts(
            "PackedF64RangeLoopStore",
            Some(arr_id),
            "packed_f64_range_loop_store",
            &stored,
            Some(BoundsState::Guarded {
                guard_id: guard_id.to_string(),
            }),
            None,
            Some(BufferAccessMode::CheckedNative),
            None,
            None,
            None,
            vec![
                array_kind_fact(Some(arr_id), "consumed", "packed_f64", None),
                raw_f64_layout_fact(Some(arr_id), "consumed", guard_id, None),
            ],
            Vec::new(),
            false,
            false,
            {
                let mut notes = vec![
                    "rhs_numeric_guard=static_genuine_f64_proof".to_string(),
                    "index_range=range_guarded_i32_window".to_string(),
                    "storage_layout=raw_f64_or_hole_slots".to_string(),
                ];
                notes.extend(rhs_notes);
                notes
            },
        );
        let _ = side_exit_label;
        return Ok(val_double);
    }

    let fast_idx = ctx.new_block("packed_f64_range_store.fast");
    let exit_idx = ctx.new_block("packed_f64_range_store.side_exit");
    let fast_label = ctx.block_label(fast_idx);
    let exit_label = ctx.block_label(exit_idx);

    // Numeric-bits check: (bits >> 48) - 0x7FF9 <u 7 detects every NaN-box tag
    // (0x7FF9..=0x7FFF: BigInt/short-string/singletons/pointer/INT32/string).
    // Genuine doubles — including canonical NaN (0x7FF8) and negative NaNs
    // (0xFFF8+) — pass and are stored raw. INT32-boxed integers side-exit
    // rather than being converted inline: the slow loop stores them correctly
    // and the shapes this matcher admits (raw loads + float arithmetic)
    // produce plain doubles.
    {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(&val_double);
        let upper = blk.lshr(I64, &bits, "48");
        let rel = blk.sub(I64, &upper, "32761"); // 0x7FF9
        let is_boxed = blk.icmp_ult(I64, &rel, "7");
        blk.cond_br(&is_boxed, &exit_label, &fast_label);
    }

    ctx.current_block = exit_idx;
    {
        ctx.block().br(side_exit_label);
        let fallback = LoweredValue {
            semantic: SemanticKind::JsValue,
            rep: NativeRep::JsValue,
            llvm_ty: DOUBLE,
            value: val_double.clone(),
        };
        ctx.record_lowered_value_with_access_mode_and_facts(
            "PackedF64RangeLoopStore",
            Some(arr_id),
            "packed_f64_range_loop_store_side_exit",
            &fallback,
            Some(BoundsState::Unknown),
            None,
            Some(BufferAccessMode::DynamicFallback),
            Some(MaterializationReason::RuntimeApi),
            None,
            None,
            Vec::new(),
            vec![raw_f64_layout_fact(
                Some(arr_id),
                "rejected",
                "packed_f64_range_loop_store_value_check",
                Some(MaterializationReason::RuntimeApi),
            )],
            false,
            false,
            vec![
                "rhs_numeric_guard=inline_nanbox_tag_check".to_string(),
                "store_guard_failure=side_exit_slow_restart".to_string(),
            ],
        );
    }

    ctx.current_block = fast_idx;
    {
        let arr_expr = Expr::LocalGet(arr_id);
        let arr_box = lower_expr(ctx, &arr_expr)?;
        let arr_handle = super::packed_receiver_handle_i64(ctx, Some(arr_id), &arr_box);
        let blk = ctx.block();
        let idx_i64 = blk.zext(I32, idx_i32, I64);
        let byte_offset = blk.shl(I64, &idx_i64, "3");
        let with_header = blk.add(I64, &byte_offset, "8");
        let element_addr = blk.add(I64, &arr_handle, &with_header);
        let element_ptr = blk.inttoptr(I64, &element_addr);
        // GC_STORE_AUDIT(POINTER_FREE): range-guarded packed numeric element
        // store — the inline tag check above proved `val_double` is a genuine
        // (unboxed) double, never a heap pointer, so the slot carries no edge.
        blk.store(DOUBLE, &val_double, &element_ptr);
    }
    let stored = LoweredValue {
        semantic: SemanticKind::JsNumber,
        rep: NativeRep::F64,
        llvm_ty: DOUBLE,
        value: val_double.clone(),
    };
    ctx.record_lowered_value_with_access_mode_and_facts(
        "PackedF64RangeLoopStore",
        Some(arr_id),
        "packed_f64_range_loop_store",
        &stored,
        Some(BoundsState::Guarded {
            guard_id: guard_id.to_string(),
        }),
        None,
        Some(BufferAccessMode::CheckedNative),
        None,
        None,
        None,
        vec![
            array_kind_fact(Some(arr_id), "consumed", "packed_f64", None),
            raw_f64_layout_fact(Some(arr_id), "consumed", guard_id, None),
        ],
        Vec::new(),
        false,
        false,
        {
            let mut notes = vec![
                "rhs_numeric_guard=inline_nanbox_tag_check".to_string(),
                "store_guard_failure=side_exit_slow_restart".to_string(),
                "index_range=range_guarded_i32_window".to_string(),
                "storage_layout=raw_f64_or_hole_slots".to_string(),
            ];
            notes.extend(rhs_notes);
            notes
        },
    );
    Ok(val_double)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn lower_packed_numeric_loop_index_set(
    ctx: &mut FnCtx<'_>,
    arr_id: u32,
    idx_i32: &str,
    value: &Expr,
    guard_id: &str,
    side_exit_label: &str,
    array_kind: PackedNumericLoopKind,
    allow_holes: bool,
) -> Result<String> {
    if matches!(array_kind, PackedNumericLoopKind::F64) {
        // Both F64 fact kinds route to the inline-check store. The
        // hole-tolerant range fact always did; the versioned-loop fact
        // (`allow_holes=false`, bound = `arr.length`) used to keep a
        // per-iteration `js_typed_feedback_numeric_array_index_set_guard`
        // CALL in its fast body — plus the store's own
        // `js_array_numeric_value_to_raw_f64` call — costing 9.1 vs 3.3
        // ns/store against the same loop with a constant bound, i.e. the
        // "fast" version ran slower than the plain per-store diamond. Every
        // check that call performed is already proven here: the loop-entry
        // guard proved the dense RawF64 layout and the body walk proved
        // nothing in the loop can invalidate it; the fact only ever matches
        // offset-0 indices (`packed_f64_loop_fact_for_index` rejects offsets
        // on non-holes facts), so the loop condition `i < arr.length`
        // (re-read each iteration) proves the store in bounds; and the RHS
        // value check is the range store's inline nanbox tag test — a boxed
        // value side-exits to the slow loop exactly as the guard's failure
        // arm did, before the store, so nothing double-applies.
        return lower_packed_f64_range_loop_index_set(
            ctx,
            arr_id,
            idx_i32,
            value,
            guard_id,
            side_exit_label,
        );
    }
    let _ = allow_holes;
    let (val_double, native_value, rhs_notes) =
        lower_packed_numeric_loop_store_value(ctx, arr_id, value, array_kind)?;
    let arr_expr = Expr::LocalGet(arr_id);
    let arr_box = lower_expr(ctx, &arr_expr)?;
    let feedback_site_id = emit_typed_feedback_register_site(
        ctx,
        TypedFeedbackKind::ArrayElement,
        match array_kind {
            PackedNumericLoopKind::F64 => "array[packed_f64_loop]=",
            PackedNumericLoopKind::I32 => "array[packed_i32_loop]=",
            PackedNumericLoopKind::U32 => "array[packed_u32_loop]=",
        },
        TypedFeedbackContract::bounded_numeric_array_set_index(),
    );
    let loop_label = array_kind.loop_label();
    let fast_idx = ctx.new_block(&format!("{loop_label}_loop_store.fast"));
    let fallback_idx = ctx.new_block(&format!("{loop_label}_loop_store.fallback"));
    let merge_idx = ctx.new_block(&format!("{loop_label}_loop_store.merge"));
    let fast_label = ctx.block_label(fast_idx);
    let fallback_label = ctx.block_label(fallback_idx);
    let merge_label = ctx.block_label(merge_idx);

    {
        let blk = ctx.block();
        let guard_i32 = blk.call(
            I32,
            "js_typed_feedback_numeric_array_index_set_guard",
            &[
                (I64, &feedback_site_id),
                (DOUBLE, &arr_box),
                (I32, idx_i32),
                (DOUBLE, &val_double),
                (I32, "1"),
            ],
        );
        let guard_ok = blk.icmp_ne(I32, &guard_i32, "0");
        blk.cond_br(&guard_ok, &fast_label, &fallback_label);
    }

    ctx.current_block = fallback_idx;
    {
        ctx.block().br(side_exit_label);
        let fallback = LoweredValue {
            semantic: SemanticKind::JsValue,
            rep: NativeRep::JsValue,
            llvm_ty: DOUBLE,
            value: arr_box.clone(),
        };
        ctx.record_lowered_value_with_access_mode_and_facts(
            array_kind.store_expr_kind(),
            Some(arr_id),
            array_kind.store_side_exit_consumer(),
            &fallback,
            Some(BoundsState::Unknown),
            None,
            Some(BufferAccessMode::DynamicFallback),
            Some(MaterializationReason::RuntimeApi),
            None,
            None,
            Vec::new(),
            vec![
                array_kind_fact(
                    Some(arr_id),
                    "rejected",
                    array_kind.array_kind_label(),
                    Some(MaterializationReason::RuntimeApi),
                ),
                raw_f64_layout_fact(
                    Some(arr_id),
                    "rejected",
                    array_kind.store_guard_detail(),
                    Some(MaterializationReason::RuntimeApi),
                ),
                raw_f64_layout_fact(
                    Some(arr_id),
                    "invalidated",
                    "runtime_api",
                    Some(MaterializationReason::RuntimeApi),
                ),
            ],
            false,
            false,
            vec![
                "rhs_numeric_guard=side_exit_slow_restart".to_string(),
                "store_guard_failure=side_exit_slow_restart".to_string(),
            ],
        );
    }

    ctx.current_block = fast_idx;
    {
        let slot_value = {
            match array_kind {
                PackedNumericLoopKind::F64 => {
                    let blk = ctx.block();
                    canonicalize_raw_f64_numeric_store_value(blk, &val_double)
                }
                PackedNumericLoopKind::I32 => val_double.clone(),
                PackedNumericLoopKind::U32 => val_double.clone(),
            }
        };
        let fast_arr_box = lower_expr(ctx, &arr_expr)?;
        let blk = ctx.block();
        let arr_bits = blk.bitcast_double_to_i64(&fast_arr_box);
        let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);
        let idx_i64 = blk.zext(I32, idx_i32, I64);
        let byte_offset = blk.shl(I64, &idx_i64, "3");
        let with_header = blk.add(I64, &byte_offset, "8");
        let element_addr = blk.add(I64, &arr_handle, &with_header);
        let element_ptr = blk.inttoptr(I64, &element_addr);
        // GC_STORE_AUDIT(POINTER_FREE): packed numeric-array element store —
        // `slot_value` is a raw numeric f64 (canonicalized via
        // `js_array_numeric_value_to_raw_f64` for F64, or `sitofp` of an i32 for
        // I32) written into a numeric-layout array element. A number is never a
        // GC pointer, so the slot carries no heap edge and needs no barrier.
        blk.store(DOUBLE, &slot_value, &element_ptr);
        blk.br(&merge_label);
    }
    let stored = LoweredValue {
        semantic: SemanticKind::JsNumber,
        rep: match array_kind {
            PackedNumericLoopKind::F64 => NativeRep::F64,
            PackedNumericLoopKind::I32 => NativeRep::I32,
            PackedNumericLoopKind::U32 => NativeRep::U32,
        },
        llvm_ty: match array_kind {
            PackedNumericLoopKind::F64 => DOUBLE,
            PackedNumericLoopKind::I32 => I32,
            PackedNumericLoopKind::U32 => I32,
        },
        value: native_value,
    };
    ctx.record_lowered_value_with_access_mode_and_facts(
        array_kind.store_expr_kind(),
        Some(arr_id),
        array_kind.store_consumer(),
        &stored,
        Some(BoundsState::Guarded {
            guard_id: guard_id.to_string(),
        }),
        None,
        Some(BufferAccessMode::CheckedNative),
        None,
        None,
        None,
        vec![
            array_kind_fact(
                Some(arr_id),
                "consumed",
                array_kind.array_kind_label(),
                None,
            ),
            raw_f64_layout_fact(Some(arr_id), "consumed", guard_id, None),
        ],
        Vec::new(),
        false,
        false,
        {
            let mut notes = vec![
                "rhs_numeric_guard=js_typed_feedback_numeric_array_index_set_guard".to_string(),
                "array_reloaded_after_rhs=1".to_string(),
                "array_reloaded_after_store_guard=1".to_string(),
                "store_guard_failure=side_exit_slow_restart".to_string(),
                "index_range=nonnegative_i32".to_string(),
                "length_range=guarded_i32".to_string(),
                format!("storage_layout={}", array_kind.array_kind_label()),
            ];
            if matches!(array_kind, PackedNumericLoopKind::F64) {
                notes.push("raw_f64_canonicalized=js_array_numeric_value_to_raw_f64".to_string());
                notes.push("array_reloaded_after_canonicalization=1".to_string());
            }
            notes.extend(rhs_notes);
            notes
        },
    );
    ctx.current_block = merge_idx;
    Ok(val_double)
}
