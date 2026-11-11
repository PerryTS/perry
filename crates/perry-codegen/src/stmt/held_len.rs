//! Which loop holds `a.length` by the facts (`expr::held_len`; wpfacts
//! `held_lengths`).
//!
//! `for (…; k < a.length; k++)` whose body the loop's own walker refuses (a
//! call, a store it cannot place) is still the hoisted loop when:
//! - `a` is a plain local the facts give `Arr` and a stable length
//!   (`BindingFacts::stable_length`), not at rest;
//! - nothing in the loop assigns `a`;
//! - `k` starts non-negative, is stepped by the update only and by one
//!   (`loops::loop_counter_bounds_are_safe`), so `k` is in `[0, length)`
//!   wherever the body runs.

use perry_hir::{CompareOp, Expr, Stmt};

use crate::codegen::FactsRep;
use crate::expr::held_len::{self, HeldLen};
use crate::expr::FnCtx;

/// The loop `cond; update; body` holds its bound's length by the facts.
pub(super) fn classify(
    ctx: &FnCtx<'_>,
    cond: &Expr,
    update: Option<&Expr>,
    body: &[Stmt],
) -> Option<HeldLen> {
    let Expr::Compare {
        op: CompareOp::Lt,
        left,
        right,
    } = cond
    else {
        return None;
    };
    let Expr::LocalGet(counter) = left.as_ref() else {
        return None;
    };
    let Expr::PropertyGet {
        object, property, ..
    } = right.as_ref()
    else {
        return None;
    };
    let Expr::LocalGet(arr) = object.as_ref() else {
        return None;
    };
    if property != "length" {
        return None;
    }
    let front = held_len::stable(ctx, *arr)?;
    let refuse = |why: &str| {
        held_len::diag(ctx, *arr, &format!("refused:{why}"));
        None
    };
    if !super::loops::array_length_receiver_is_loop_local(ctx, *arr) {
        return refuse("receiver");
    }
    match crate::codegen::facts_rep::local(ctx, *arr) {
        Some(FactsRep::Arr(rep)) if !rep.at_rest => {}
        _ => return refuse("rep"),
    }
    if super::loops::stmts_mutate_local(body, *arr)
        || update.is_some_and(|e| super::loops::expr_mutates_local(e, *arr))
    {
        return refuse("array-assigned");
    }
    if !super::loops::loop_counter_bounds_are_safe(ctx, *counter, update, body) {
        return refuse("counter");
    }
    held_len::diag(ctx, *arr, if front { "held:front" } else { "held" });
    Some(HeldLen {
        arr: *arr,
        counter: *counter,
        front,
    })
}

/// `len` (a length, as a double) or `i32::MAX`, whichever is less.
pub(super) fn int32_bound(ctx: &mut FnCtx<'_>, len: &str) -> String {
    let max = crate::nanbox::double_literal(f64::from(i32::MAX));
    let blk = ctx.block();
    let past = blk.fcmp("ogt", len, &max);
    blk.select(crate::types::I1, &past, crate::types::DOUBLE, &max, len)
}
