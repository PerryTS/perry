//! Ternary conditional.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::Result;
use perry_hir::Expr;

use crate::lower_conditional::lower_conditional;

use super::FnCtx;

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => lower_conditional(ctx, condition, then_expr, else_expr),

        // `arr.push(x)` (Phase B.7) — special HIR variant that already
        // tells us the array LocalId and the value. We load the array
        // from its slot, unbox, push, NaN-box the (possibly-reallocated)
        // pointer, and store it back into the slot so subsequent uses
        // see the up-to-date pointer.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
