//! Issue #1098: extracted shadow-slot helper free functions.
//!
//! Pure mechanical move out of `expr/mod.rs`. These `pub(crate)` free
//! functions are re-exported from the trunk so existing
//! `crate::expr::X` call paths resolve unchanged.
use super::*;

use anyhow::{anyhow, Result};

use perry_hir::types::Type as HirType;
use perry_hir::{BinaryOp, Expr};

use crate::types::{I32, I64, PTR};

/// The current closure pointer to feed to `js_closure_get/set_capture_bits`,
/// re-read from the shadow-rooted slot when one exists (#7055).
///
/// Every capture access MUST go through here rather than reusing the
/// `%this_closure` SSA parameter. The parameter is a register value the
/// collector cannot see: an evacuating young collection at a loop back-edge
/// poll inside the body relocates the closure, rewrites every root it knows
/// about, and then resets from-space — after which the register points at
/// recycled memory whose `capture_count` no longer covers the index, so
/// `js_closure_get_capture_bits` returns 0 and every subsequent boxed-capture
/// read/write silently no-ops. Returns `None` when the body has no closure
/// pointer at all (top-level functions/methods).
pub(crate) fn try_current_closure_ptr_value(ctx: &mut FnCtx<'_>) -> Option<String> {
    if let Some(slot) = ctx.current_closure_slot.clone() {
        let bits = ctx.block().load(I64, &slot);
        return Some(ctx.block().and(I64, &bits, crate::nanbox::POINTER_MASK_I64));
    }
    ctx.current_closure_ptr.clone()
}

/// [`try_current_closure_ptr_value`] with the shared "no current_closure_ptr"
/// diagnostic; `what` names the lowering that needed it.
pub(crate) fn current_closure_ptr_value(ctx: &mut FnCtx<'_>, what: &str) -> Result<String> {
    try_current_closure_ptr_value(ctx).ok_or_else(|| anyhow!("{what} but no current_closure_ptr"))
}

pub(crate) fn expr_is_known_non_pointer_shadow_value(ctx: &FnCtx<'_>, expr: &Expr) -> bool {
    match expr {
        Expr::Undefined | Expr::Null | Expr::Bool(_) | Expr::Number(_) | Expr::Integer(_) => true,
        Expr::LocalGet(id) => {
            // A reserved shadow slot means the local is pointer-possible even
            // if its initializer refined `local_types` to a scalar.
            !ctx.shadow_slot_map.contains_key(id)
                && matches!(
                    ctx.local_types.get(id),
                    Some(
                        HirType::Number
                            | HirType::Int32
                            | HirType::Boolean
                            | HirType::Null
                            | HirType::Void
                            | HirType::Never
                            | HirType::Symbol
                    )
                )
        }
        Expr::Compare { .. } | Expr::Void(_) => true,
        Expr::Unary { .. } => true,
        Expr::Binary { op, .. } => !matches!(op, BinaryOp::Add),
        // #6750 follow-up: a masked-index read covered by an ACTIVE
        // masked-window fact is a guard-proven numeric element load — never
        // a pointer — even when the receiver's static type is erased.
        Expr::IndexGet { object, index } => matches!(
            object.as_ref(),
            Expr::LocalGet(arr_id)
                if super::masked_window_fact_for_index(ctx, *arr_id, index).is_some()
        ),
        // #6996: a typed-array / Buffer element read is a number (or
        // `undefined` out of range) BY CONSTRUCTION -- `lower_buffer_load`'s
        // inline byte load, `js_uint8array_index_get_value` and
        // `js_buffer_index_get_value` can only ever produce one. It is never a
        // heap reference, so #6951's argument-temporary rooting has nothing to
        // protect and its push/re-read/truncate trio is pure TLS traffic in
        // exactly the loops that can least afford it (`buf[i] + packet.tag`
        // rooted the byte across the property get, once per iteration).
        //
        // The proof is about the LOWERING, not the declared type: annotations
        // are unenforced, so `buf: Buffer` holding something else must not be
        // load-bearing -- and it isn't, because both runtime accessors answer
        // `undefined` for a receiver that is not a Uint8Array/Buffer.
        //
        // The three lowerings of this node that CAN yield a heap value are
        // excluded by construction, one condition each:
        //   * a symbol key (`u8[Symbol.iterator]`) goes to
        //     `js_object_get_symbol_property`, which returns the accessor;
        //   * in JS-value context, a key without the integer-array-index proof
        //     goes to `js_typed_array_index_get_dynamic`, which falls through
        //     to string-keyed property lookup (an expando holds anything);
        //   * in i32 context (`lower_uint8array_get_i32`), a key that is not
        //     numeric-proven goes to `js_object_get_index_polymorphic`, which
        //     dispatches a string key to that same property path. That arm is
        //     gated on `is_numeric_expr`, NOT on the index proof, so testing
        //     the proof alone would not cover it -- both conditions are
        //     required. `is_numeric_expr` is used here only to NARROW: a wrong
        //     `true` from it still leaves a byte read on every arm it admits.
        // `BufferIndexGet` has none of these paths -- every arm coerces the key
        // to i32 and reads a byte -- so it needs no condition.
        Expr::Uint8ArrayGet { index, .. } => {
            !matches!(index.as_ref(), Expr::SymbolFor(_))
                && crate::type_analysis::is_numeric_expr(ctx, index)
                && super::index_get::numeric_index_has_integer_array_index_proof(ctx, index)
        }
        Expr::BufferIndexGet { .. } => true,
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => {
            expr_is_known_non_pointer_shadow_value(ctx, then_expr)
                && expr_is_known_non_pointer_shadow_value(ctx, else_expr)
        }
        Expr::Sequence(exprs) => exprs
            .last()
            .is_some_and(|last| expr_is_known_non_pointer_shadow_value(ctx, last)),
        _ => false,
    }
}

pub(crate) fn emit_shadow_slot_clear(ctx: &mut FnCtx<'_>, slot_idx: u32) {
    if ctx.persistent_shadow_slots.contains(&slot_idx) {
        return;
    }
    // Never-bound slot: it provably still holds its initial 0 (slots are only
    // written through bind/set, and every value-set site binds first), so the
    // clear would be a redundant `js_shadow_slot_set(idx, 0)` TLS hit.
    if !ctx.shadow_slots_bound.contains(&slot_idx) {
        return;
    }
    // #6794 follow-up (b): the slot was already cleared to 0 for a currently
    // shadow-suppressed masked-window-region local, and suppression blocks every
    // subsequent write to it (`emit_shadow_slot_update_for_expr`), so it provably
    // still holds 0 — this clear is a redundant `js_shadow_slot_set(slot, 0)`
    // (the `_tlv_get_addr` TLS hit that dominated bcryptjs `_encipher`). Skip it.
    if ctx.suppressed_cleared_shadow_slots.contains(&slot_idx) {
        return;
    }
    // #7088: emitted inline against this activation's cached `ShadowStackState`
    // pointer when it has one; falls through to the call otherwise.
    if super::shadow_inline::emit_inline_slot_clear(ctx, slot_idx) {
        return;
    }
    ctx.block().call_void(
        "js_shadow_slot_set",
        &[(I32, &slot_idx.to_string()), (I64, "0")],
    );
}

/// Bind an immutable `const item = rootedArray[index]` local once in the
/// function-entry setup and retain its current value until return.
///
/// The alloca is entry-hoisted and initialized to `undefined`, so the early
/// bind is valid even when the declaration itself sits in a loop or branch.
/// Every later iteration writes the same alloca, which the GC scanner follows
/// through `slot_ptrs`. Pointer-capable updates still emit the root shading
/// barrier required when an incremental collection has already scanned roots;
/// only the repeated TLS slot rebinding and lexical-death clear are removed.
pub(crate) fn enable_persistent_shadow_slot_for_array_alias(
    ctx: &mut FnCtx<'_>,
    local_id: u32,
    init: &Expr,
) {
    let Expr::IndexGet { object, .. } = init else {
        return;
    };
    if !matches!(object.as_ref(), Expr::LocalGet(_)) {
        return;
    }
    let Some(slot_idx) = ctx.shadow_slot_map.get(&local_id).copied() else {
        return;
    };
    let Some(local_slot) = ctx.locals.get(&local_id).cloned() else {
        return;
    };
    if !ctx.persistent_shadow_slots.insert(slot_idx) {
        return;
    }
    ctx.shadow_slots_bound.insert(slot_idx);
    let slot_idx_string = slot_idx.to_string();
    ctx.func.entry_setup_call_void(
        "js_shadow_slot_bind",
        &[(I32, &slot_idx_string), (PTR, &local_slot)],
    );
}

pub(crate) fn emit_shadow_slot_bind_for_local(ctx: &mut FnCtx<'_>, local_id: u32) {
    let Some(slot_idx) = ctx.shadow_slot_map.get(&local_id).copied() else {
        return;
    };
    if ctx.persistent_shadow_slots.contains(&slot_idx) {
        return;
    }
    let Some(local_slot) = ctx.locals.get(&local_id).cloned() else {
        return;
    };
    ctx.shadow_slots_bound.insert(slot_idx);
    // #7088: the hot per-store root write. Emitted inline against this
    // activation's cached `ShadowStackState` pointer when it has one; falls
    // through to the call otherwise.
    if super::shadow_inline::emit_inline_slot_bind(ctx, slot_idx, &local_slot) {
        return;
    }
    ctx.block().call_void(
        "js_shadow_slot_bind",
        &[(I32, &slot_idx.to_string()), (PTR, &local_slot)],
    );
}

/// Emit the incremental-mark root shading barrier for a value that has just
/// been written into an already-bound (persistent) root slot.
///
/// This is the only part of `js_shadow_slot_bind` that is genuinely per-store:
/// re-recording `slot_ptrs[idx]` and re-mirroring the value are loop-invariant
/// for an entry-hoisted alloca, but a pointer stored into a root *after* the
/// collector scanned roots still has to be shaded. Guarding on
/// `PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT` inline keeps the common
/// (no incremental cycle in flight) path down to a load, a compare, and a
/// not-taken branch instead of a TLS-touching call.
pub(crate) fn emit_persistent_shadow_root_barrier(ctx: &mut FnCtx<'_>, value_bits: &str) {
    let active =
        ctx.block()
            .load_atomic_seq_cst(I32, "@PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT", 4);
    let barrier_needed = ctx.block().icmp_ne(I32, &active, "0");
    let barrier_idx = ctx.new_block("shadow.root.barrier");
    let done_idx = ctx.new_block("shadow.root.barrier.done");
    let barrier_label = ctx.block_label(barrier_idx);
    let done_label = ctx.block_label(done_idx);
    ctx.block()
        .cond_br(&barrier_needed, &barrier_label, &done_label);

    ctx.current_block = barrier_idx;
    ctx.block()
        .call_void("js_write_barrier_root_nanbox", &[(I64, value_bits)]);
    ctx.block().br(&done_label);
    ctx.current_block = done_idx;
}

pub(crate) fn emit_shadow_slot_update_for_expr(
    ctx: &mut FnCtx<'_>,
    local_id: u32,
    value_reg: &str,
    rhs: &Expr,
) {
    // #6750 follow-up: inside a masked-window region fast copy, a local
    // flow-refined to Number had its slot cleared at the refinement point
    // and every subsequent region write stores a proven number — no
    // per-statement shadow traffic needed until the refinement is dropped
    // (see `stmt::masked_window_region`).
    if ctx.masked_region_scalar_locals.contains(&local_id) {
        return;
    }
    let Some(slot_idx) = ctx.shadow_slot_map.get(&local_id).copied() else {
        return;
    };
    if ctx.persistent_shadow_slots.contains(&slot_idx) {
        if !expr_is_known_non_pointer_shadow_value(ctx, rhs) {
            let value_bits = ctx.block().bitcast_double_to_i64(value_reg);
            emit_persistent_shadow_root_barrier(ctx, &value_bits);
        }
        return;
    }
    if expr_is_known_non_pointer_shadow_value(ctx, rhs) {
        emit_shadow_slot_clear(ctx, slot_idx);
    } else {
        // Every caller has already stored the new value in the local alloca.
        // `js_shadow_slot_bind` copies that slot into the shadow frame, marks
        // it active, and runs the root barrier, so a following slot-set call
        // only repeated the same TLS lookup, copy, and barrier.
        emit_shadow_slot_bind_for_local(ctx, local_id);
    }
}
