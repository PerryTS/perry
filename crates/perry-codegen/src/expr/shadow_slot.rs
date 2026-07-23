//! Issue #1098: extracted shadow-slot helper free functions.
//!
//! Pure mechanical move out of `expr/mod.rs`. These `pub(crate)` free
//! functions are re-exported from the trunk so existing
//! `crate::expr::X` call paths resolve unchanged.
use super::*;

use perry_hir::{BinaryOp, Expr};
use perry_types::Type as HirType;

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
    ctx.block().call_void(
        "js_shadow_slot_set",
        &[(I32, &slot_idx.to_string()), (I64, "0")],
    );
}

pub(crate) fn emit_shadow_slot_bind_for_local(ctx: &mut FnCtx<'_>, local_id: u32) {
    let Some(slot_idx) = ctx.shadow_slot_map.get(&local_id).copied() else {
        return;
    };
    let Some(local_slot) = ctx.locals.get(&local_id).cloned() else {
        return;
    };
    ctx.block().call_void(
        "js_shadow_slot_bind",
        &[(I32, &slot_idx.to_string()), (PTR, &local_slot)],
    );
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
    if expr_is_known_non_pointer_shadow_value(ctx, rhs) {
        emit_shadow_slot_clear(ctx, slot_idx);
    } else {
        emit_shadow_slot_bind_for_local(ctx, local_id);
        let v_i64 = ctx.block().bitcast_double_to_i64(value_reg);
        ctx.block().call_void(
            "js_shadow_slot_set",
            &[(I32, &slot_idx.to_string()), (I64, &v_i64)],
        );
    }
}
