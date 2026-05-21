//! `Stmt::If` lowering.

use super::*;

use crate::lower_conditional::lower_truthy;

/// If-else lowering using explicit then/else/merge blocks.
///
/// Truthiness uses `lower_truthy` which dispatches to either an inline
/// `fcmp one cond, 0.0` (statically-numeric conditions) or a runtime
/// `js_is_truthy` call (NaN-boxed booleans, strings, objects, unions).
/// Try to evaluate a condition at compile time using known constants.
/// Returns `Some(true)` or `Some(false)` if the condition can be folded,
/// `None` if it depends on runtime values.
fn try_const_fold_condition(ctx: &FnCtx<'_>, condition: &perry_hir::Expr) -> Option<bool> {
    use perry_hir::{CompareOp, Expr, LogicalOp};
    match condition {
        Expr::Compare { op, left, right } => {
            // Try to extract a known constant from one side and a literal
            // from the other.
            let (const_val, literal_val) = match (left.as_ref(), right.as_ref()) {
                (Expr::LocalGet(id), Expr::Integer(n)) => {
                    (ctx.compile_time_constants.get(id)?, *n as f64)
                }
                (Expr::Integer(n), Expr::LocalGet(id)) => {
                    (ctx.compile_time_constants.get(id)?, *n as f64)
                }
                (Expr::LocalGet(id), Expr::Number(n)) => (ctx.compile_time_constants.get(id)?, *n),
                (Expr::Number(n), Expr::LocalGet(id)) => (ctx.compile_time_constants.get(id)?, *n),
                _ => return None,
            };
            let c = *const_val;
            Some(match op {
                CompareOp::Eq | CompareOp::LooseEq => c == literal_val,
                CompareOp::Ne | CompareOp::LooseNe => c != literal_val,
                CompareOp::Lt => c < literal_val,
                CompareOp::Le => c <= literal_val,
                CompareOp::Gt => c > literal_val,
                CompareOp::Ge => c >= literal_val,
            })
        }
        Expr::Logical { op, left, right } => {
            let l = try_const_fold_condition(ctx, left)?;
            match op {
                LogicalOp::And => {
                    if !l {
                        Some(false)
                    } else {
                        try_const_fold_condition(ctx, right)
                    }
                }
                LogicalOp::Or => {
                    if l {
                        Some(true)
                    } else {
                        try_const_fold_condition(ctx, right)
                    }
                }
                LogicalOp::Coalesce => None,
            }
        }
        _ => None,
    }
}

pub(crate) fn lower_if(
    ctx: &mut FnCtx<'_>,
    condition: &perry_hir::Expr,
    then_branch: &[Stmt],
    else_branch: Option<&[Stmt]>,
) -> Result<()> {
    // Compile-time constant folding: when the condition involves only
    // known constants (e.g., `__platform__ === 1`), skip the dead branch
    // entirely. This prevents emitting `declare`/`call` instructions for
    // extern FFI functions that only exist on other platforms.
    if let Some(is_true) = try_const_fold_condition(ctx, condition) {
        if is_true {
            lower_stmts(ctx, then_branch)?;
        } else if let Some(else_stmts) = else_branch {
            lower_stmts(ctx, else_stmts)?;
        }
        return Ok(());
    }

    let cond_val = lower_expr(ctx, condition)?;
    let i1 = lower_truthy(ctx, &cond_val, condition);

    let then_idx = ctx.new_block("if.then");
    let else_idx = ctx.new_block("if.else");
    let merge_idx = ctx.new_block("if.merge");

    let then_label = ctx.block_label(then_idx);
    let else_label = ctx.block_label(else_idx);
    let merge_label = ctx.block_label(merge_idx);

    // Emit the branch in the incoming current block.
    ctx.block().cond_br(&i1, &then_label, &else_label);

    // Compile then branch.
    ctx.current_block = then_idx;
    lower_stmts(ctx, then_branch)?;
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    // Compile else branch. If there's no explicit else, the else block is
    // still created so both sides of the condBr have a valid target — it
    // just branches immediately to merge.
    ctx.current_block = else_idx;
    if let Some(else_stmts) = else_branch {
        lower_stmts(ctx, else_stmts)?;
    }
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    // Continue emitting subsequent statements into the merge block.
    ctx.current_block = merge_idx;
    Ok(())
}
