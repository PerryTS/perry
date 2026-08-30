//! The foreign-counter packed-clone read (#9161).
//!
//! Split out of `index_get.rs`, which sits at the 2000-line cap. This is the
//! `arr[i]` case where `i` is a live i32 counter of some OTHER loop than the
//! clone's own — see `foreign_packed_loop_read` for why a fact carrying its
//! own per-element exit is declined.

use super::super::*;
use super::packed_f64_loop_fact;

/// An active packed-loop fact for `arr_id` plus a foreign i32 index local:
/// `arr[i]` where `i` is not the clone's counter. Declines a fact that already
/// carries its own per-element exit condition (holes, a validated window), so
/// the bounds-checked load never stacks two side exits on one read.
pub(crate) fn foreign_packed_loop_read(
    ctx: &FnCtx<'_>,
    arr_id: u32,
    index: &Expr,
) -> Option<(PackedF64LoopFact, u32)> {
    let Expr::LocalGet(idx_id) = index else {
        return None;
    };
    if !ctx.i32_counter_slots.contains_key(idx_id) || !ctx.integer_locals.contains(idx_id) {
        return None;
    }
    let fact = ctx
        .packed_f64_loop_facts
        .iter()
        .rev()
        .find(|fact| {
            fact.array_local_id == arr_id
                && fact.index_local_id != *idx_id
                && !fact.allow_holes
                && !fact.window_validated
        })?
        .clone();
    Some((fact, *idx_id))
}

/// #6011: decompose a packed-loop index expression into `(counter_local_id,
/// constant_offset)`. Matches `i`, `i + c`, `c + i`, and `i - c` with a small
/// |c| — exactly the shapes the packed-f64 range loop matcher admits, so any
/// offset seen here on a fact-carrying (array, counter) pair is inside the
/// range guard's validated window.
pub(crate) fn packed_f64_loop_index_parts(index: &Expr) -> Option<(u32, i32)> {
    use perry_hir::BinaryOp;
    match index {
        Expr::LocalGet(id) => Some((*id, 0)),
        Expr::Binary { op, left, right } if matches!(op, BinaryOp::Add | BinaryOp::Sub) => {
            let (id, offset) = match (left.as_ref(), right.as_ref()) {
                (Expr::LocalGet(id), Expr::Integer(c)) => {
                    let offset = if matches!(op, BinaryOp::Sub) {
                        c.checked_neg()?
                    } else {
                        *c
                    };
                    (*id, offset)
                }
                (Expr::Integer(c), Expr::LocalGet(id)) if matches!(op, BinaryOp::Add) => (*id, *c),
                _ => return None,
            };
            let offset = i32::try_from(offset).ok()?;
            if offset.unsigned_abs() > 64 {
                return None;
            }
            Some((id, offset))
        }
        _ => None,
    }
}

/// Look up a packed-f64 loop fact for `(arr, index-expr)`. Zero-offset
/// indices match any fact; non-zero offsets only match hole-tolerant facts
/// (established by the range guard, which validated the whole offset window —
/// the length-bound guard of the classic matcher only proves `i` itself).
pub(crate) fn packed_f64_loop_fact_for_index(
    ctx: &FnCtx<'_>,
    arr_id: u32,
    index: &Expr,
) -> Option<(PackedF64LoopFact, u32, i32)> {
    let (idx_id, offset) = packed_f64_loop_index_parts(index)?;
    let fact = packed_f64_loop_fact(ctx, arr_id, idx_id)?;
    if offset != 0 && !fact.allow_holes && !fact.window_validated {
        return None;
    }
    Some((fact, idx_id, offset))
}
