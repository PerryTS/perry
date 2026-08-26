//! Representation-selection Phase 2: **guard-free checked-native element
//! access** for storage-proven typed-array views.
//!
//! `expr/buffer_access.rs`'s bare-load tier requires a static bounds proof
//! (`bounds_for_buffer_access_width`). When the index is dynamic (`lr[off]`
//! with an arbitrary i32 `off` — the bcryptjs `_encipher` entry/exit reads and
//! writes), that proof cannot exist, and the access previously fell back to a
//! `js_typed_array_get`/`js_typed_array_index_set_dynamic` runtime call or to
//! the per-site KIND-GUARDED inline path (`ta_param_f64_read.rs` — measured to
//! LOSE on unrolled bodies).
//!
//! This tier serves receivers whose `BufferViewSlot` carries
//! `storage_inline_proven` (fresh non-view construction: literal-length
//! `new TA(n)` let-bindings, or a specialized-entry `TaPtr` param whose
//! call-site pre-pass proved the construction). For those, element kind,
//! data pointer, and length are fixed for the array's lifetime, so the access
//! needs NO kind guard, NO view guard, and NO runtime call — just:
//!
//! ```text
//!   %in = icmp ult i32 %idx, %len      ; len loaded once from the header
//!   br %in, load/store, oob
//!   load:  bare element load → f64     (store: bare element store)
//!   oob:   TAG_UNDEFINED               (store: no-op)
//! ```
//!
//! Bit-exact with the runtime helpers: an in-bounds read yields the numeric
//! element; OOB / negative / wrapped indices fail the UNSIGNED compare and
//! yield the `TAG_UNDEFINED` double (reads) or silently skip (writes) —
//! exactly ECMAScript's IntegerIndexedExotic [[Get]]/[[Set]].
//!
//! Index soundness: only shapes proven to be EXACT signed-i32 integers are
//! accepted (`index_is_exact_i32_shape`) — a fractional or >2^31 index must
//! keep the runtime path, because an `fptosi`/wrapped i32 could alias a
//! different (in-bounds) element.

use anyhow::Result;
use perry_hir::{BinaryOp, Expr};

use super::{
    attach_buffer_view_facts, can_lower_expr_as_i32, is_numeric_expr, lower_expr,
    lower_expr_as_i32, lower_expr_native, FnCtx,
};
use crate::nanbox::{double_literal, TAG_UNDEFINED};
use crate::native_value::{
    BoundsState, BufferAccessMode, BufferElem, BufferIndexUnit, ExpectedNativeRep, LoweredValue,
};
use crate::types::{DOUBLE, F32, I16, I32, I64, I8, PTR};

/// `x & <mask>` with a compile-time non-negative i32 mask literal on either
/// side: the result is a non-negative integer array index for ANY `x`.
/// (Hosted here from `index_get.rs` for the file-size gate.)
pub(crate) fn bitand_has_nonnegative_i32_mask(left: &Expr, right: &Expr) -> bool {
    fn mask(expr: &Expr) -> Option<i64> {
        match expr {
            Expr::Integer(i) => Some(*i),
            Expr::Number(n) if n.is_finite() && n.fract() == 0.0 => Some(*n as i64),
            _ => None,
        }
    }
    mask(left)
        .or_else(|| mask(right))
        .is_some_and(|mask| (0..=i32::MAX as i64).contains(&mask))
}

/// Small additive offsets keep the wrap analysis trivial: `local + c` with
/// `0 <= c <= 2^20` can only wrap PAST `i32::MAX`, landing negative, which the
/// unsigned bounds compare rejects — indistinguishable from the true
/// (out-of-bounds) index.
const MAX_ADDEND: i64 = 1 << 20;

/// `true` when `index`'s runtime value is provably an exact signed-i32 (or
/// safely-wrapping, see `MAX_ADDEND`) integer, so `lower_expr_as_i32` +
/// unsigned bounds compare reproduces JS semantics exactly.
pub(crate) fn index_is_exact_i32_shape(ctx: &FnCtx<'_>, index: &Expr) -> bool {
    if let Some((lo, hi)) = crate::collectors::static_index_window(index) {
        if lo >= 0 && hi <= i64::from(i32::MAX) {
            return true;
        }
    }
    match index {
        Expr::Integer(n) => i32::try_from(*n).is_ok(),
        Expr::Number(n) => {
            n.is_finite()
                && n.fract() == 0.0
                && *n >= f64::from(i32::MIN)
                && *n <= f64::from(i32::MAX)
        }
        // A canonical-i32 local IS a true signed i32 by construction (Phase 1
        // slot rep) — including `U32`, whose bit pattern compares correctly
        // under the unsigned bounds check when read BARE.
        Expr::LocalGet(id) => ctx.local_slot_reps.contains_key(id),
        Expr::Binary { op, left, right } if matches!(op, BinaryOp::Add) => {
            match (left.as_ref(), right.as_ref()) {
                (Expr::LocalGet(id), Expr::Integer(c)) | (Expr::Integer(c), Expr::LocalGet(id)) => {
                    // SIGNED reps only: a `U32` local's value near 2^32 is a
                    // NEGATIVE i32 bit pattern, and `+ c` can wrap it UP
                    // through zero into a valid index (`bits(-1) + 2 == 1`)
                    // while the true JS index (4294967297) must read
                    // `undefined`. The signed wrap argument (`i32::MAX + c` →
                    // negative → rejected by `icmp ult`) holds only for `I32`.
                    matches!(ctx.local_slot_reps.get(id), Some(crate::expr::SlotRep::I32))
                        && (0..=MAX_ADDEND).contains(c)
                }
                _ => false,
            }
        }
        _ => false,
    }
}

/// `true` when `id` is a proven-inline-storage integer-element view eligible
/// as a checked-store receiver (`expr/proven_view_access` store tier and the
/// masked-window region's proven store suffix).
pub(crate) fn local_is_proven_int_store_view(ctx: &FnCtx<'_>, id: u32) -> bool {
    // Mirror `proven_view_for`'s closure gate: a captured receiver could be
    // reassigned between proof and access (also independently caught by the
    // alias/scope downgrade, kept here for consistency).
    if ctx.closure_captures.contains_key(&id) {
        return false;
    }
    ctx.buffer_view_slots.get(&id).is_some_and(|view| {
        view.pointer_state.is_stable()
            && view.storage_inline_proven
            && view.native_owned.is_none()
            && view.index_unit == BufferIndexUnit::Element
            && view.alias.allows_noalias()
            && view.scope_idx.is_some()
            && matches!(
                view.elem,
                BufferElem::I8
                    | BufferElem::U8
                    | BufferElem::I16
                    | BufferElem::U16
                    | BufferElem::I32
                    | BufferElem::U32
            )
    })
}

/// The proven view for `object`, when every gate for the checked tier holds.
fn proven_view_for(
    ctx: &FnCtx<'_>,
    object: &Expr,
    index: &Expr,
) -> Option<(u32, crate::native_value::BufferViewSlot)> {
    if ctx.disable_buffer_fast_path {
        return None;
    }
    let Expr::LocalGet(id) = object else {
        return None;
    };
    let view = ctx.buffer_view_slots.get(id)?.clone();
    if !view.pointer_state.is_stable()
        || !view.storage_inline_proven
        || view.native_owned.is_some()
        || view.index_unit != BufferIndexUnit::Element
    {
        return None;
    }
    // Downgrade marker: hazard paths clear `scope_idx` / demote `alias` when
    // the tracked data pointer can no longer be trusted (reassignment
    // refresh, aliasing, escapes). Mirror `lower_typed_array_load`'s gates so
    // this tier can never read through a stale slot.
    if !view.alias.allows_noalias() || view.scope_idx.is_none() {
        return None;
    }
    // A closure-captured receiver can be reassigned between proof and access.
    if ctx.closure_captures.contains_key(id) {
        return None;
    }
    let stable_u32_index = crate::stmt::stable_packed_loop::has_u32_index_fact(ctx, index);
    if !stable_u32_index {
        if !index_is_exact_i32_shape(ctx, index) {
            return None;
        }
        if !can_lower_expr_as_i32(
            index,
            &ctx.i32_counter_slots,
            ctx.flat_const_arrays,
            &ctx.array_row_aliases,
            ctx.integer_locals,
            &ctx.const_number_locals,
            ctx.clamp3_functions,
            ctx.clamp_u8_functions,
            ctx.integer_returning_functions,
            ctx.i32_identity_functions,
        ) {
            return None;
        }
    }
    Some((*id, view))
}

fn lower_checked_index(ctx: &mut FnCtx<'_>, index: &Expr) -> Result<String> {
    if let Some(index) = crate::stmt::stable_packed_loop::try_lower_u32_index(ctx, index) {
        return Ok(index);
    }
    lower_expr_as_i32(ctx, index)
}

pub(crate) fn is_proven_u32_view_read(ctx: &FnCtx<'_>, value: &Expr) -> bool {
    let Expr::IndexGet { object, index } = value else {
        return false;
    };
    let Expr::LocalGet(id) = object.as_ref() else {
        return false;
    };
    ctx.buffer_view_slots.get(id).is_some_and(|view| {
        view.pointer_state.is_stable()
            && view.storage_inline_proven
            && view.native_owned.is_none()
            && view.index_unit == BufferIndexUnit::Element
            && view.alias.allows_noalias()
            && view.scope_idx.is_some()
            && matches!(view.elem, BufferElem::U32)
            && crate::stmt::stable_packed_loop::has_u32_index_fact(ctx, index)
    })
}

fn proven_u32_view_value(ctx: &FnCtx<'_>, value: &Expr) -> bool {
    if is_proven_u32_view_read(ctx, value) {
        return true;
    }
    let Expr::LocalGet(id) = value else {
        return false;
    };
    ctx.stable_packed_loop_facts
        .iter()
        .rev()
        .any(|fact| fact.u32_view_derived_locals.contains_key(id))
}

/// Data pointer + entry-derived length for the proven view. The length load
/// is `invariant` — a non-view typed array's length is immutable.
fn load_data_and_len(
    ctx: &mut FnCtx<'_>,
    view: &crate::native_value::BufferViewSlot,
) -> (String, String) {
    let blk = ctx.block();
    let data_ptr = blk.load(PTR, &view.data_slot);
    let len = if let Some(length_slot) = view.length_slot.as_ref() {
        blk.load(I32, length_slot)
    } else {
        let len_ptr = blk.gep(
            I8,
            &data_ptr,
            &[(I32, &view.length_offset_from_data.to_string())],
        );
        blk.load_invariant(I32, &len_ptr)
    };
    (data_ptr, len)
}

fn elem_llvm_ty(elem: &BufferElem) -> crate::types::LlvmType {
    match elem {
        BufferElem::I8 | BufferElem::U8 | BufferElem::U8Clamped => I8,
        BufferElem::I16 | BufferElem::U16 => I16,
        BufferElem::I32 | BufferElem::U32 => I32,
        BufferElem::F32 => F32,
        BufferElem::F64 => DOUBLE,
    }
}

/// Inline checked f64 element read (`object[index]` in a boxed/numeric
/// context). Returns the DOUBLE SSA value: numeric element in-bounds,
/// `TAG_UNDEFINED` OOB — bit-exact with `js_typed_array_get`.
pub(crate) fn try_lower_proven_view_checked_f64_load(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    index: &Expr,
) -> Result<Option<String>> {
    let Some((id, view)) = proven_view_for(ctx, object, index) else {
        return Ok(None);
    };
    // #7640 section E audit: `proven_view_for` only reads compile-time facts;
    // no receiver value or backing-store pointer has been materialized yet.
    // Lower the index expression first, then load `data_slot` below, so even a
    // collecting proven index leaves no movable or raw address live.
    let idx_i32 = lower_checked_index(ctx, index)?;
    let (data_ptr, len) = load_data_and_len(ctx, &view);

    let load_idx = ctx.new_block("pview.get.load");
    let oob_idx = ctx.new_block("pview.get.oob");
    let merge_idx = ctx.new_block("pview.get.merge");
    let load_label = ctx.block_label(load_idx);
    let oob_label = ctx.block_label(oob_idx);
    let merge_label = ctx.block_label(merge_idx);

    {
        let blk = ctx.block();
        // `ult` rejects negative / wrapped indices too (huge unsigned).
        let in_bounds = blk.icmp_ult(I32, &idx_i32, &len);
        blk.cond_br(&in_bounds, &load_label, &oob_label);
    }

    ctx.current_block = load_idx;
    let (load_val, load_end) = {
        let blk = ctx.block();
        let idx_i64 = blk.zext(I32, &idx_i32, I64);
        let byte_off = if view.element_width_bytes > 1 {
            blk.shl(
                I64,
                &idx_i64,
                &view.element_width_bytes.trailing_zeros().to_string(),
            )
        } else {
            idx_i64
        };
        let elem_ptr = blk.gep(I8, &data_ptr, &[(I64, &byte_off)]);
        let raw = blk.load(elem_llvm_ty(&view.elem), &elem_ptr);
        let val = match view.elem {
            BufferElem::I8 => blk.sitofp(I8, &raw, DOUBLE),
            BufferElem::U8 | BufferElem::U8Clamped => blk.uitofp(I8, &raw, DOUBLE),
            BufferElem::I16 => blk.sitofp(I16, &raw, DOUBLE),
            BufferElem::U16 => blk.uitofp(I16, &raw, DOUBLE),
            BufferElem::I32 => blk.sitofp(I32, &raw, DOUBLE),
            BufferElem::U32 => blk.uitofp(I32, &raw, DOUBLE),
            BufferElem::F32 => blk.fpext(F32, &raw, DOUBLE),
            BufferElem::F64 => raw,
        };
        let end = blk.label.clone();
        blk.br(&merge_label);
        (val, end)
    };

    ctx.current_block = oob_idx;
    let (oob_val, oob_end) = {
        let blk = ctx.block();
        let end = blk.label.clone();
        blk.br(&merge_label);
        (double_literal(f64::from_bits(TAG_UNDEFINED)), end)
    };

    ctx.current_block = merge_idx;
    let result = ctx.block().phi(
        DOUBLE,
        &[
            (load_val.as_str(), load_end.as_str()),
            (oob_val.as_str(), oob_end.as_str()),
        ],
    );
    let lowered = LoweredValue::js_value(result.clone());
    ctx.record_lowered_value_with_access_mode(
        "TypedArrayGet",
        Some(id),
        "TypedArrayGet.proven_view_checked",
        &lowered,
        Some(BoundsState::Guarded {
            guard_id: "proven_view_checked_bounds".to_string(),
        }),
        Some(view.alias.clone()),
        Some(BufferAccessMode::CheckedNative),
        None,
        false,
        false,
        vec!["proven_view=checked_inline; guards=none".to_string()],
    );
    attach_buffer_view_facts(ctx, &view);
    Ok(Some(result))
}

/// Inline checked Uint32Array read that preserves the raw lane for a native
/// consumer. The out-of-bounds arm is zero because the public read produces
/// `undefined`, whose ToUint32 value is zero.
pub(crate) fn try_lower_proven_view_checked_u32_load(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    index: &Expr,
) -> Result<Option<LoweredValue>> {
    let Some((id, view)) = proven_view_for(ctx, object, index) else {
        return Ok(None);
    };
    if !matches!(view.elem, BufferElem::U32) {
        return Ok(None);
    }
    let common_bound = crate::stmt::stable_packed_loop::has_u32_component_bound(ctx, index);
    let idx_i32 = lower_checked_index(ctx, index)?;
    let (data_ptr, len) = load_data_and_len(ctx, &view);
    if common_bound {
        let idx_i64 = ctx.block().zext(I32, &idx_i32, I64);
        let byte_off = ctx.block().shl(I64, &idx_i64, "2");
        let elem_ptr = ctx.block().gep(I8, &data_ptr, &[(I64, &byte_off)]);
        let lowered = LoweredValue::u32(ctx.block().load(I32, &elem_ptr));
        ctx.record_lowered_value_with_access_mode(
            "TypedArrayGet",
            Some(id),
            "TypedArrayGet.proven_view_common_bound_u32",
            &lowered,
            Some(BoundsState::Guarded {
                guard_id: "stable_packed_u32_component_bound".to_string(),
            }),
            Some(view.alias.clone()),
            Some(BufferAccessMode::UncheckedNative),
            None,
            false,
            false,
            vec!["proven_view=unchecked_common_bound_u32".to_string()],
        );
        attach_buffer_view_facts(ctx, &view);
        return Ok(Some(lowered));
    }
    let load_idx = ctx.new_block("pview.get_u32.load");
    let oob_idx = ctx.new_block("pview.get_u32.oob");
    let merge_idx = ctx.new_block("pview.get_u32.merge");
    let load_label = ctx.block_label(load_idx);
    let oob_label = ctx.block_label(oob_idx);
    let merge_label = ctx.block_label(merge_idx);
    let in_bounds = ctx.block().icmp_ult(I32, &idx_i32, &len);
    ctx.block().cond_br(&in_bounds, &load_label, &oob_label);

    ctx.current_block = load_idx;
    let idx_i64 = ctx.block().zext(I32, &idx_i32, I64);
    let byte_off = ctx.block().shl(I64, &idx_i64, "2");
    let elem_ptr = ctx.block().gep(I8, &data_ptr, &[(I64, &byte_off)]);
    let raw = ctx.block().load(I32, &elem_ptr);
    let load_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = oob_idx;
    let oob_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    let value = ctx.block().phi(I32, &[(&raw, &load_end), ("0", &oob_end)]);
    let lowered = LoweredValue::u32(value);
    ctx.record_lowered_value_with_access_mode(
        "TypedArrayGet",
        Some(id),
        "TypedArrayGet.proven_view_checked_u32",
        &lowered,
        Some(BoundsState::Guarded {
            guard_id: "proven_view_checked_bounds".to_string(),
        }),
        Some(view.alias.clone()),
        Some(BufferAccessMode::CheckedNative),
        None,
        false,
        false,
        vec!["proven_view=checked_inline_u32; guards=none".to_string()],
    );
    attach_buffer_view_facts(ctx, &view);
    Ok(Some(lowered))
}

fn emit_proven_view_store(
    ctx: &mut FnCtx<'_>,
    view: &crate::native_value::BufferViewSlot,
    data_ptr: &str,
    idx_i32: &str,
    value_native: &LoweredValue,
) {
    let blk = ctx.block();
    let idx_i64 = blk.zext(I32, idx_i32, I64);
    let byte_off = if view.element_width_bytes > 1 {
        blk.shl(
            I64,
            &idx_i64,
            &view.element_width_bytes.trailing_zeros().to_string(),
        )
    } else {
        idx_i64
    };
    let elem_ptr = blk.gep(I8, data_ptr, &[(I64, &byte_off)]);
    // GC_STORE_AUDIT(POINTER_FREE): typed-array backing store.
    match view.elem {
        BufferElem::I8 | BufferElem::U8 => {
            let byte = blk.trunc(I32, &value_native.value, I8);
            blk.store(I8, &byte, &elem_ptr);
        }
        BufferElem::I16 | BufferElem::U16 => {
            let half = blk.trunc(I32, &value_native.value, I16);
            // GC_STORE_AUDIT(POINTER_FREE): typed-array backing store.
            blk.store(I16, &half, &elem_ptr);
        }
        BufferElem::I32 | BufferElem::U32 => {
            // GC_STORE_AUDIT(POINTER_FREE): typed-array backing store.
            blk.store(I32, &value_native.value, &elem_ptr);
        }
        BufferElem::F32 => {
            let narrow = blk.fptrunc(DOUBLE, &value_native.value, F32);
            // GC_STORE_AUDIT(POINTER_FREE): typed-array backing store.
            blk.store(F32, &narrow, &elem_ptr);
        }
        BufferElem::F64 => {
            // GC_STORE_AUDIT(POINTER_FREE): typed-array backing store.
            blk.store(DOUBLE, &value_native.value, &elem_ptr);
        }
        BufferElem::U8Clamped => unreachable!("gated before store emission"),
    }
}

/// Inline checked element write (`object[index] = value`). The value is
/// evaluated and coerced BEFORE the bounds check (spec order); an OOB write is
/// a silent no-op. Returns the lowered (coerced-native) RHS so the assignment
/// expression value matches the proven fast-store tier.
pub(crate) fn try_lower_proven_view_checked_store(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    index: &Expr,
    value: &Expr,
) -> Result<Option<LoweredValue>> {
    let Some((id, view)) = proven_view_for(ctx, object, index) else {
        return Ok(None);
    };
    // Same value-shape gates as `lower_typed_array_store`: integer kinds need
    // a proven ToInt32-exact value, float kinds a proven number. U8Clamped
    // stores keep the runtime clamping helper.
    let int_kind = matches!(
        view.elem,
        BufferElem::I8
            | BufferElem::U8
            | BufferElem::I16
            | BufferElem::U16
            | BufferElem::I32
            | BufferElem::U32
    );
    if matches!(view.elem, BufferElem::U8Clamped) {
        return Ok(None);
    }
    // The ctx-free store gate misses proven typed-array-read operands
    // (`r ^ P[17]`); the region-aware predicate is the crate's established
    // "lowerable as exact i32 with ToInt32 semantics" contract and covers
    // them (its IndexGet arm requires the same bounds proofs the unchecked
    // load emitter does).
    if int_kind
        && !super::can_lower_integer_typed_array_store_value(ctx, value)
        && !super::can_lower_expr_as_i32_in_current_region(ctx, value)
        && !proven_u32_view_value(ctx, value)
    {
        return Ok(None);
    }
    if matches!(view.elem, BufferElem::F32 | BufferElem::F64) {
        let numeric_candidate = crate::codegen::typed_arg_is_guard_candidate(
            ctx,
            crate::codegen::TypedParamRep::F64,
            value,
        );
        if !numeric_candidate {
            return Ok(None);
        }
    }

    let idx_i32 = lower_checked_index(ctx, index)?;
    let value_native = if int_kind {
        let expected = if matches!(view.elem, BufferElem::U32) {
            ExpectedNativeRep::U32
        } else {
            ExpectedNativeRep::I32
        };
        lower_expr_native(ctx, value, expected)?
    } else if !is_numeric_expr(ctx, value) {
        // Source metadata may select this checked store, but only the live
        // value may supply raw bytes. Avoid `lower_expr_native(F64)`'s legacy
        // annotation shortcut and perform explicit ToNumber coercion.
        let boxed = lower_expr(ctx, value)?;
        let coerced = ctx
            .block()
            .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &boxed)]);
        LoweredValue::f64(coerced)
    } else {
        lower_expr_native(ctx, value, ExpectedNativeRep::F64)?
    };
    let (data_ptr, len) = load_data_and_len(ctx, &view);
    if crate::stmt::stable_packed_loop::has_u32_component_bound(ctx, index) {
        emit_proven_view_store(ctx, &view, &data_ptr, &idx_i32, &value_native);
        ctx.record_lowered_value_with_access_mode(
            "TypedArraySet",
            Some(id),
            "TypedArraySet.proven_view_common_bound",
            &value_native,
            Some(BoundsState::Guarded {
                guard_id: "stable_packed_u32_component_bound".to_string(),
            }),
            Some(view.alias.clone()),
            Some(BufferAccessMode::UncheckedNative),
            None,
            false,
            false,
            vec!["proven_view=unchecked_common_bound".to_string()],
        );
        attach_buffer_view_facts(ctx, &view);
        return Ok(Some(value_native));
    }

    let store_idx = ctx.new_block("pview.set.store");
    let done_idx = ctx.new_block("pview.set.done");
    let store_label = ctx.block_label(store_idx);
    let done_label = ctx.block_label(done_idx);

    {
        let blk = ctx.block();
        let in_bounds = blk.icmp_ult(I32, &idx_i32, &len);
        blk.cond_br(&in_bounds, &store_label, &done_label);
    }

    ctx.current_block = store_idx;
    {
        emit_proven_view_store(ctx, &view, &data_ptr, &idx_i32, &value_native);
        ctx.block().br(&done_label);
    }
    ctx.current_block = done_idx;

    ctx.record_lowered_value_with_access_mode(
        "TypedArraySet",
        Some(id),
        "TypedArraySet.proven_view_checked",
        &value_native,
        Some(BoundsState::Guarded {
            guard_id: "proven_view_checked_bounds".to_string(),
        }),
        Some(view.alias.clone()),
        Some(BufferAccessMode::CheckedNative),
        None,
        false,
        false,
        vec!["proven_view=checked_inline; guards=none".to_string()],
    );
    attach_buffer_view_facts(ctx, &view);
    Ok(Some(value_native))
}
