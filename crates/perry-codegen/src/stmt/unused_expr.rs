//! Discarded-expression lowering — the `let`-arm helpers that decide when a
//! value whose result is never read can be elided (or lowered to a cheaper
//! side-effect-only form) without dropping observable effects.

use super::*;

use crate::types::I64;

pub(super) fn lower_unused_expr(ctx: &mut FnCtx<'_>, expr: &perry_hir::Expr) -> Result<bool> {
    if unused_expr_is_pure_nonthrowing(ctx, expr) {
        return Ok(true);
    }
    match expr {
        perry_hir::Expr::New {
            class_name, args, ..
        } if class_name.starts_with("__AnonShape_") => {
            // Anonymous-shape `new` is how object literals lower. When the
            // constructed value is immediately discarded by scalar replacement
            // we still must preserve evaluation order of every property value,
            // but we can skip the synthetic object allocation/field stores.
            for arg in args {
                if !lower_unused_expr(ctx, arg)? {
                    let _ = lower_expr(ctx, arg)?;
                }
            }
            Ok(true)
        }
        perry_hir::Expr::ArrayMap { array, callback } => {
            if array_map_callback_is_discard_pure(callback) {
                // The map result is unused and the callback only builds an
                // anonymous object from its parameter. Evaluate the receiver to
                // preserve source-order effects, but skip closure allocation,
                // callback dispatch, and all discarded object construction.
                let _ = lower_expr(ctx, array)?;
                return Ok(true);
            }
            let arr_box = lower_expr(ctx, array)?;
            let cb_box = lower_expr(ctx, callback)?;
            let blk = ctx.block();
            let arr_handle = crate::expr::unbox_to_i64(blk, &arr_box);
            // #4091: throw TypeError for a non-callable callback before iterating
            // (the discarded-result path still validates per spec).
            let cb_handle = blk.call(
                I64,
                "js_validate_array_map_callback",
                &[(I64, &arr_handle), (DOUBLE, &cb_box)],
            );
            blk.call_void(
                "js_array_map_discard",
                &[(I64, &arr_handle), (I64, &cb_handle)],
            );
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn unused_expr_is_pure_nonthrowing(ctx: &FnCtx<'_>, expr: &perry_hir::Expr) -> bool {
    match expr {
        perry_hir::Expr::Undefined
        | perry_hir::Expr::Null
        | perry_hir::Expr::Bool(_)
        | perry_hir::Expr::Number(_)
        | perry_hir::Expr::Integer(_)
        | perry_hir::Expr::String(_)
        | perry_hir::Expr::WtfString(_) => true,
        perry_hir::Expr::Unary { operand, .. } => {
            crate::type_analysis::is_numeric_expr(ctx, operand)
                && unused_expr_is_pure_nonthrowing(ctx, operand)
        }
        perry_hir::Expr::Binary { op, left, right } => {
            unused_binary_is_pure_nonthrowing(ctx, op, left, right)
                && unused_expr_is_pure_nonthrowing(ctx, left)
                && unused_expr_is_pure_nonthrowing(ctx, right)
        }
        perry_hir::Expr::Compare { left, right, .. } => {
            unused_primitive_expr_is_nonthrowing(ctx, left)
                && unused_primitive_expr_is_nonthrowing(ctx, right)
                && unused_expr_is_pure_nonthrowing(ctx, left)
                && unused_expr_is_pure_nonthrowing(ctx, right)
        }
        perry_hir::Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            unused_expr_is_pure_nonthrowing(ctx, condition)
                && unused_expr_is_pure_nonthrowing(ctx, then_expr)
                && unused_expr_is_pure_nonthrowing(ctx, else_expr)
        }
        _ => false,
    }
}

fn unused_binary_is_pure_nonthrowing(
    ctx: &FnCtx<'_>,
    op: &perry_hir::BinaryOp,
    left: &perry_hir::Expr,
    right: &perry_hir::Expr,
) -> bool {
    match op {
        perry_hir::BinaryOp::Add => {
            let l_num = crate::type_analysis::is_numeric_expr(ctx, left);
            let r_num = crate::type_analysis::is_numeric_expr(ctx, right);
            if l_num && r_num {
                return true;
            }
            let l_str = crate::type_analysis::is_definitely_string_expr(ctx, left);
            let r_str = crate::type_analysis::is_definitely_string_expr(ctx, right);
            (l_str || r_str)
                && unused_primitive_expr_is_nonthrowing(ctx, left)
                && unused_primitive_expr_is_nonthrowing(ctx, right)
        }
        perry_hir::BinaryOp::Sub
        | perry_hir::BinaryOp::Mul
        | perry_hir::BinaryOp::Div
        | perry_hir::BinaryOp::Mod
        | perry_hir::BinaryOp::BitAnd
        | perry_hir::BinaryOp::BitOr
        | perry_hir::BinaryOp::BitXor
        | perry_hir::BinaryOp::Shl
        | perry_hir::BinaryOp::Shr
        | perry_hir::BinaryOp::UShr => {
            crate::type_analysis::is_numeric_expr(ctx, left)
                && crate::type_analysis::is_numeric_expr(ctx, right)
        }
        _ => false,
    }
}

fn unused_primitive_expr_is_nonthrowing(ctx: &FnCtx<'_>, expr: &perry_hir::Expr) -> bool {
    crate::type_analysis::is_numeric_expr(ctx, expr)
        || crate::type_analysis::is_definitely_string_expr(ctx, expr)
        || crate::type_analysis::is_bool_expr(ctx, expr)
        || matches!(
            expr,
            perry_hir::Expr::Undefined
                | perry_hir::Expr::Null
                | perry_hir::Expr::String(_)
                | perry_hir::Expr::WtfString(_)
                | perry_hir::Expr::Number(_)
                | perry_hir::Expr::Integer(_)
                | perry_hir::Expr::Bool(_)
        )
}

fn array_map_callback_is_discard_pure(callback: &perry_hir::Expr) -> bool {
    let perry_hir::Expr::Closure {
        params,
        body,
        captures,
        mutable_captures,
        captures_this,
        is_async,
        ..
    } = callback
    else {
        return false;
    };
    if *is_async
        || *captures_this
        || !captures.is_empty()
        || !mutable_captures.is_empty()
        || params.is_empty()
    {
        return false;
    }
    let param_id = params[0].id;
    matches!(body.as_slice(), [perry_hir::Stmt::Return(Some(expr))] if discard_pure_expr(expr, param_id))
}

fn discard_pure_expr(expr: &perry_hir::Expr, param_id: perry_types::LocalId) -> bool {
    match expr {
        perry_hir::Expr::Undefined
        | perry_hir::Expr::Null
        | perry_hir::Expr::Bool(_)
        | perry_hir::Expr::Number(_)
        | perry_hir::Expr::Integer(_)
        | perry_hir::Expr::String(_)
        | perry_hir::Expr::WtfString(_) => true,
        perry_hir::Expr::LocalGet(id) => *id == param_id,
        // PropertyGet is deliberately *not* in the pure set: TypeScript
        // `get` accessors can run user code, so eliding the map body
        // would drop visible side effects. The intended target of this
        // optimization is the anonymous-shape `Expr::New` arm below.
        perry_hir::Expr::New {
            class_name, args, ..
        } if class_name.starts_with("__AnonShape_") => {
            args.iter().all(|arg| discard_pure_expr(arg, param_id))
        }
        _ => false,
    }
}
