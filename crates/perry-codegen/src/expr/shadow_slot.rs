//! Issue #1098: extracted shadow-slot helper free functions.
//!
//! Pure mechanical move out of `expr/mod.rs`. These `pub(crate)` free
//! functions are re-exported from the trunk so existing
//! `crate::expr::X` call paths resolve unchanged.
use super::*;

use perry_hir::types::Type as HirType;
use perry_hir::{BinaryOp, Expr};

use crate::types::{I32, I64, PTR};

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
    ctx.block().call_void(
        "js_shadow_slot_bind",
        &[(I32, &slot_idx.to_string()), (PTR, &local_slot)],
    );
}

fn emit_persistent_shadow_root_barrier(ctx: &mut FnCtx<'_>, value_bits: &str) {
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
