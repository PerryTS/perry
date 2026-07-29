//! Representation-selection Phase 4a.3: guard-free `Ptr<NumArray>` element
//! access lowering (RFC `docs/representation-selection-rfc.md` §4
//! `Array<number>` row, §5.7).
//!
//! The eligibility proof lives in `collectors/ptr_numarray.rs`; this module
//! holds the per-site consumers: the in-bounds site proof and the guard-free
//! load/store emitters. See the collector's module doc for the full soundness
//! argument (provenance + containment + density gating + the stale-binding
//! exemption).

use anyhow::Result;
use perry_hir::Expr;

use crate::nanbox::POINTER_MASK_I64;
use crate::native_value::{
    BoundsProof, BoundsState, BufferAccessMode, LoweredValue, NativeRep, SemanticKind,
};
use crate::types::{DOUBLE, I1, I32, I64};

use super::{lower_expr, lower_expr_as_i32, raw_f64_layout_fact, FnCtx};

/// Repsel Phase 4a.3: per-site in-bounds proof for a `Ptr<NumArray>` local —
/// the index's integer range is `[0, max]` with `max <` the static allocation
/// length (length can only grow for a proven local, so the bound is
/// permanent).
pub(crate) fn num_array_index_statically_in_bounds(
    ctx: &FnCtx<'_>,
    fact: &crate::collectors::NumArrayLocal,
    index: &Expr,
) -> bool {
    super::range_facts::int_range_expr(ctx, index)
        .is_some_and(|range| range.min >= 0 && range.max < fact.proven_initial_length)
}

/// Repsel Phase 4a.3: the guard-free `Ptr<NumArray>` element load — slot
/// reload → mask → `+8` → `idx<<3` → `load double`, with NO guard tier, no
/// bounds check (the caller proved in-bounds), no runtime call anywhere.
/// `HolesOk` locals canonicalize any NaN payload (a `TAG_HOLE` slot) to the
/// quiet NaN — bit-exact with `ToNumber(undefined)` in the number contexts
/// this is emitted for; `Dense` locals skip even that select.
fn lower_num_array_guard_free_get(
    ctx: &mut FnCtx<'_>,
    arr_id: u32,
    arr_box: &str,
    idx_i32: &str,
    fact: &crate::collectors::NumArrayLocal,
    proof: BoundsProof,
) -> String {
    let value = {
        let blk = ctx.block();
        let arr_bits = blk.bitcast_double_to_i64(arr_box);
        let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);
        let idx_i64 = blk.zext(I32, idx_i32, I64);
        let byte_offset = blk.shl(I64, &idx_i64, "3");
        let with_header = blk.add(I64, &byte_offset, "8");
        let element_addr = blk.add(I64, &arr_handle, &with_header);
        let element_ptr = blk.inttoptr(I64, &element_addr);
        let raw = blk.load(DOUBLE, &element_ptr);
        if fact.density == crate::collectors::NumArrayDensity::HolesOk {
            let is_ord = blk.fcmp("ord", &raw, &raw);
            blk.select(I1, &is_ord, DOUBLE, &raw, "0x7FF8000000000000")
        } else {
            raw
        }
    };
    let lowered = LoweredValue {
        semantic: SemanticKind::JsNumber,
        rep: NativeRep::F64,
        llvm_ty: DOUBLE,
        value: value.clone(),
    };
    ctx.record_lowered_value_with_access_mode_and_facts(
        "NumArrayIndexGet",
        Some(arr_id),
        "ptr_numarray.guard_free_load",
        &lowered,
        Some(BoundsState::Proven { proof }),
        None,
        Some(BufferAccessMode::CheckedNative),
        None,
        None,
        None,
        vec![raw_f64_layout_fact(
            Some(arr_id),
            "consumed",
            "ptr_numarray_local_proof",
            None,
        )],
        Vec::new(),
        false,
        false,
        vec![
            "index_range=proven_in_bounds".to_string(),
            "storage_layout=raw_f64_or_hole_slots".to_string(),
            "guard=none_static_proof".to_string(),
        ],
    );
    value
}

/// Repsel Phase 4a.3: try the guard-free `Ptr<NumArray>` read for a
/// number-context element load. Returns `None` (fall through to the guarded
/// tiers) unless the receiver is a proven local AND the site carries an
/// in-bounds proof (static range vs the allocation length, or a
/// bounded-loop-pair fact).
pub(crate) fn try_lower_num_array_guard_free_get(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    index: &Expr,
) -> Result<Option<String>> {
    let Expr::LocalGet(arr_id) = object else {
        return Ok(None);
    };
    let Some(fact) = ctx.native_facts.num_array_local(*arr_id).cloned() else {
        return Ok(None);
    };
    if num_array_index_statically_in_bounds(ctx, &fact, index) {
        let arr_box = lower_expr(ctx, &Expr::LocalGet(*arr_id))?;
        let idx_i32 = lower_expr_as_i32(ctx, index)?;
        return Ok(Some(lower_num_array_guard_free_get(
            ctx,
            *arr_id,
            &arr_box,
            &idx_i32,
            &fact,
            BoundsProof::MinLength,
        )));
    }
    if let Expr::LocalGet(idx_id) = index {
        if ctx
            .bounded_index_pairs
            .iter()
            .any(|f| f.index_local_id == *idx_id && f.array_local_id == *arr_id)
        {
            if let Some(i32_slot) = ctx.i32_counter_slots.get(idx_id).cloned() {
                let arr_box = lower_expr(ctx, &Expr::LocalGet(*arr_id))?;
                let idx_i32 = ctx.block().load(I32, &i32_slot);
                return Ok(Some(lower_num_array_guard_free_get(
                    ctx,
                    *arr_id,
                    &arr_box,
                    &idx_i32,
                    &fact,
                    BoundsProof::LoopGuard,
                )));
            }
        }
    }
    Ok(None)
}

/// Repsel Phase 4a.3: try the guard-free `Ptr<NumArray>` store. Returns
/// `None` (fall through to the guarded tiers) unless the receiver is a proven
/// local, the RHS is canonical-raw-f64 by construction, AND the site carries
/// an in-bounds proof (static range vs the allocation length, or a
/// bounded-loop-pair fact).
pub(crate) fn try_lower_num_array_guard_free_set(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    index: &Expr,
    value: &Expr,
) -> Result<Option<String>> {
    let Expr::LocalGet(arr_id) = object else {
        return Ok(None);
    };
    let Some(fact) = ctx.native_facts.num_array_local(*arr_id).cloned() else {
        return Ok(None);
    };
    // The store must keep the raw-f64-or-hole slot invariant with no runtime
    // check, so only a canonical-by-construction RHS qualifies (an INT32-boxed
    // or NaN-payload value stored verbatim would corrupt it).
    if !crate::type_analysis::expr_produces_canonical_raw_f64(ctx, value) {
        return Ok(None);
    }
    let statically = num_array_index_statically_in_bounds(ctx, &fact, index);
    let bounded_slot = if statically {
        None
    } else {
        match index {
            Expr::LocalGet(idx_id)
                if ctx
                    .bounded_index_pairs
                    .iter()
                    .any(|f| f.index_local_id == *idx_id && f.array_local_id == *arr_id) =>
            {
                ctx.i32_counter_slots.get(idx_id).cloned()
            }
            _ => None,
        }
    };
    if !statically && bounded_slot.is_none() {
        return Ok(None);
    }
    // JS evaluation order: target ref → key → value.
    let arr_box = lower_expr(ctx, &Expr::LocalGet(*arr_id))?;
    let (idx_i32, proof) = if statically {
        (lower_expr_as_i32(ctx, index)?, BoundsProof::MinLength)
    } else {
        (
            ctx.block().load(I32, &bounded_slot.unwrap()),
            BoundsProof::LoopGuard,
        )
    };
    let val_double = lower_expr(ctx, value)?;
    {
        let blk = ctx.block();
        let arr_bits = blk.bitcast_double_to_i64(&arr_box);
        let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);
        let idx_i64 = blk.zext(I32, &idx_i32, I64);
        let byte_offset = blk.shl(I64, &idx_i64, "3");
        let with_header = blk.add(I64, &byte_offset, "8");
        let element_addr = blk.add(I64, &arr_handle, &with_header);
        let element_ptr = blk.inttoptr(I64, &element_addr);
        // GC_STORE_AUDIT(POINTER_FREE): canonical raw-f64 store under the
        // `Ptr<NumArray>` local proof — never a GC pointer, no barrier, no
        // layout note, and no length bump (in-bounds ⇒ length unchanged).
        blk.store(DOUBLE, &val_double, &element_ptr);
    }
    let stored = LoweredValue {
        semantic: SemanticKind::JsNumber,
        rep: NativeRep::F64,
        llvm_ty: DOUBLE,
        value: val_double.clone(),
    };
    ctx.record_lowered_value_with_access_mode_and_facts(
        "NumArrayIndexSet",
        Some(*arr_id),
        "ptr_numarray.guard_free_store",
        &stored,
        Some(BoundsState::Proven { proof }),
        None,
        Some(BufferAccessMode::CheckedNative),
        None,
        None,
        None,
        vec![raw_f64_layout_fact(
            Some(*arr_id),
            "consumed",
            "ptr_numarray_local_proof",
            None,
        )],
        Vec::new(),
        false,
        false,
        vec![
            "index_range=proven_in_bounds".to_string(),
            "storage_layout=raw_f64_or_hole_slots".to_string(),
            "guard=none_static_proof".to_string(),
        ],
    );
    Ok(Some(val_double))
}
