//! Masked-window array-read lowering for the dense packed-f64 range loop.
//!
//! The dense range guard (`js_typed_feedback_packed_f64_range_loop_guard_dense`
//! / `_dense_i32`, see `stmt/loops.rs`) validates a whole static index window
//! `[min_idx, max_idx_exclusive)` of a plain raw-f64 numeric array at loop
//! entry — hole-free, so in-window reads need no guard call, no hole check,
//! and no side exit. The helpers here consult the per-scope
//! [`MaskedWindowArrayFact`]s that guard establishes and emit the bare
//! in-window element loads (`S[x & 1023]`, `S[256 + ((x >>> 16) & 0xff)]` —
//! the bcryptjs Blowfish S-box shapes).

use anyhow::Result;
use perry_hir::Expr;

use crate::nanbox::POINTER_MASK_I64;
use crate::native_value::{
    BoundsState, BufferAccessMode, LoweredValue, NativeFactUse, NativeRep, SemanticKind,
};
use crate::types::{DOUBLE, I32, I64};

use super::{
    array_kind_fact, lower_expr, lower_expr_as_i32, raw_f64_layout_fact, FnCtx,
    MaskedWindowArrayFact, MaskedWindowElem,
};

/// Look up an active masked-window fact for `(arr, index-expr)`: the index's
/// static value window (`collectors::static_index_window` — the same function
/// the range-loop matcher used, so match-time and lowering-time agree) must
/// sit inside a window the dense range guard validated for this array in the
/// current fast-loop scope.
pub(crate) fn masked_window_fact_for_index(
    ctx: &FnCtx<'_>,
    arr_id: u32,
    index: &Expr,
) -> Option<MaskedWindowArrayFact> {
    let (lo, hi) = crate::collectors::static_index_window(index)?;
    ctx.masked_window_array_facts
        .iter()
        .rev()
        .find(|fact| {
            fact.array_local_id == arr_id && lo >= fact.min_idx && hi < fact.max_idx_exclusive
        })
        .cloned()
}

/// Emit the raw in-window f64 element load of the plain-array tiers:
/// `header + 8 + idx * 8` on the pointer-masked array handle.
fn emit_raw_window_load(ctx: &mut FnCtx<'_>, arr_box: &str, idx_i32: &str) -> String {
    let blk = ctx.block();
    let arr_bits = blk.bitcast_double_to_i64(arr_box);
    let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);
    let idx_i64 = blk.zext(I32, idx_i32, I64);
    let byte_offset = blk.shl(I64, &idx_i64, "3");
    let with_header = blk.add(I64, &byte_offset, "8");
    let element_addr = blk.add(I64, &arr_handle, &with_header);
    let element_ptr = blk.inttoptr(I64, &element_addr);
    blk.load(DOUBLE, &element_ptr)
}

/// Emit the raw in-window typed-array element load of the TA tiers:
/// `data_ptr + idx << shift`, where `data_ptr` is the element-0 address the
/// preheader probe hoisted (stable — the fast copy is call-free).
fn emit_ta_window_load(
    ctx: &mut FnCtx<'_>,
    data_ptr: &str,
    idx_i32: &str,
    shift: &str,
    elem_ty: crate::types::LlvmType,
) -> String {
    let blk = ctx.block();
    let idx_i64 = blk.zext(I32, idx_i32, I64);
    let byte_offset = blk.shl(I64, &idx_i64, shift);
    let element_addr = blk.add(I64, data_ptr, &byte_offset);
    let element_ptr = blk.inttoptr(I64, &element_addr);
    blk.load(elem_ty, &element_ptr)
}

/// Emit the in-window element load for `fact`, materialized as a DOUBLE
/// (number semantics): plain raw-f64 and Float64Array slots load directly;
/// Int32Array loads sign-extend (`sitofp`), Uint32Array loads are UNSIGNED
/// (`uitofp` — elements may exceed `i32::MAX`).
fn emit_window_load_f64(
    ctx: &mut FnCtx<'_>,
    arr_box: &str,
    idx_i32: &str,
    fact: &MaskedWindowArrayFact,
) -> String {
    match &fact.elem {
        MaskedWindowElem::PlainF64 => emit_raw_window_load(ctx, arr_box, idx_i32),
        MaskedWindowElem::TaI32 { data_ptr } => {
            let data_ptr = data_ptr.clone();
            let raw = emit_ta_window_load(ctx, &data_ptr, idx_i32, "2", I32);
            ctx.block().sitofp(I32, &raw, DOUBLE)
        }
        MaskedWindowElem::TaU32 { data_ptr } => {
            let data_ptr = data_ptr.clone();
            let raw = emit_ta_window_load(ctx, &data_ptr, idx_i32, "2", I32);
            ctx.block().uitofp(I32, &raw, DOUBLE)
        }
        MaskedWindowElem::TaF64 { data_ptr } => {
            let data_ptr = data_ptr.clone();
            emit_ta_window_load(ctx, &data_ptr, idx_i32, "3", DOUBLE)
        }
    }
}

/// Storage-layout audit facts + note for `fact`'s tier.
fn window_layout_facts(fact: &MaskedWindowArrayFact, arr_id: u32) -> (Vec<NativeFactUse>, String) {
    match &fact.elem {
        MaskedWindowElem::PlainF64 => (
            vec![raw_f64_layout_fact(
                Some(arr_id),
                "consumed",
                &fact.guard_id,
                None,
            )],
            "storage_layout=raw_f64_numeric_slots".to_string(),
        ),
        MaskedWindowElem::TaI32 { .. } => (
            vec![array_kind_fact(
                Some(arr_id),
                "consumed",
                &fact.guard_id,
                None,
            )],
            "storage_layout=typed_array_i32_slots".to_string(),
        ),
        MaskedWindowElem::TaU32 { .. } => (
            vec![array_kind_fact(
                Some(arr_id),
                "consumed",
                &fact.guard_id,
                None,
            )],
            "storage_layout=typed_array_u32_slots".to_string(),
        ),
        MaskedWindowElem::TaF64 { .. } => (
            vec![array_kind_fact(
                Some(arr_id),
                "consumed",
                &fact.guard_id,
                None,
            )],
            "storage_layout=typed_array_f64_slots".to_string(),
        ),
    }
}

/// Emit the in-window element load for a masked-window fact: the entry guard
/// already proved a numeric array with every slot in
/// `[min_idx, max_idx_exclusive)` an in-bounds number (no holes), so the load
/// is a bare width-correct read — no guard call, no hole check, no side exit.
pub(crate) fn lower_masked_window_index_get(
    ctx: &mut FnCtx<'_>,
    arr_id: u32,
    arr_box: &str,
    idx_i32: &str,
    fact: &MaskedWindowArrayFact,
) -> String {
    let value = emit_window_load_f64(ctx, arr_box, idx_i32, fact);
    let lowered = LoweredValue {
        semantic: SemanticKind::JsNumber,
        rep: NativeRep::F64,
        llvm_ty: DOUBLE,
        value: value.clone(),
    };
    let (layout_facts, layout_note) = window_layout_facts(fact, arr_id);
    ctx.record_lowered_value_with_access_mode_and_facts(
        "NumericArrayIndexGet",
        Some(arr_id),
        "packed_f64_masked_window_load",
        &lowered,
        Some(BoundsState::Guarded {
            guard_id: fact.guard_id.clone(),
        }),
        None,
        Some(BufferAccessMode::CheckedNative),
        None,
        None,
        None,
        layout_facts,
        Vec::new(),
        false,
        false,
        vec![
            "index_range=static_window_guarded".to_string(),
            "length_range=guarded_i32".to_string(),
            layout_note,
        ],
    );
    value
}

/// True when `object[index]` matches an active i32-tier masked-window fact —
/// the dense-i32 range guard proved every window slot is an i32-representable
/// integer, so the load can produce a native `i32` with a bare exact `fptosi`.
pub(crate) fn masked_window_i32_load_is_provable(
    ctx: &FnCtx<'_>,
    object: &Expr,
    index: &Expr,
) -> bool {
    let Expr::LocalGet(arr_id) = object else {
        return false;
    };
    masked_window_fact_for_index(ctx, *arr_id, index).is_some_and(|fact| fact.values_i32)
}

/// i32-tier masked-window load. Plain tier: raw in-window f64 element load +
/// bare `fptosi` (exact — the dense-i32 guard proved the value is an i32
/// integer). Int32Array tier: a direct `load i32` from the hoisted data
/// pointer — no float round-trip at all. Returns `None` when no i32-tier
/// fact covers the access (`values_i32` is never set for the Uint32Array /
/// Float64Array tiers, whose elements are not i32-representable).
pub(crate) fn lower_masked_window_index_get_i32(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    index: &Expr,
) -> Result<Option<String>> {
    let Expr::LocalGet(arr_id) = object else {
        return Ok(None);
    };
    let Some(fact) =
        masked_window_fact_for_index(ctx, *arr_id, index).filter(|fact| fact.values_i32)
    else {
        return Ok(None);
    };
    let arr_box = lower_expr(ctx, object)?;
    let idx_i32 = lower_expr_as_i32(ctx, index)?;
    let (value, materialization_note) = match &fact.elem {
        MaskedWindowElem::PlainF64 => {
            let raw_f64 = emit_raw_window_load(ctx, &arr_box, &idx_i32);
            (
                ctx.block().fptosi(DOUBLE, &raw_f64, I32),
                "integer_materialization=fptosi_guarded_dense_i32",
            )
        }
        MaskedWindowElem::TaI32 { data_ptr } => {
            let data_ptr = data_ptr.clone();
            (
                emit_ta_window_load(ctx, &data_ptr, &idx_i32, "2", I32),
                "integer_materialization=direct_i32_load_ta",
            )
        }
        MaskedWindowElem::TaU32 { .. } | MaskedWindowElem::TaF64 { .. } => {
            unreachable!("values_i32 fact with non-i32 element kind")
        }
    };
    let lowered = LoweredValue {
        semantic: SemanticKind::JsNumber,
        rep: NativeRep::I32,
        llvm_ty: I32,
        value: value.clone(),
    };
    let (layout_facts, layout_note) = window_layout_facts(&fact, *arr_id);
    ctx.record_lowered_value_with_access_mode_and_facts(
        "NumericArrayIndexGet",
        Some(*arr_id),
        "packed_f64_masked_window_load_i32",
        &lowered,
        Some(BoundsState::Guarded {
            guard_id: fact.guard_id.clone(),
        }),
        None,
        Some(BufferAccessMode::CheckedNative),
        None,
        None,
        None,
        layout_facts,
        Vec::new(),
        false,
        false,
        vec![
            "index_range=static_window_guarded".to_string(),
            "length_range=guarded_i32".to_string(),
            layout_note,
            materialization_note.to_string(),
        ],
    );
    Ok(Some(value))
}
