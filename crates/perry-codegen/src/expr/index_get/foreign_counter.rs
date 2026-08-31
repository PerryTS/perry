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

/// Look up a packed-f64 loop fact for `(arr, index-expr)`, reporting whether
/// a non-zero offset needs an inline bounds check rather than declining it.
///
/// #9259: declining here was not merely losing one element load. The offset
/// read fell back to a helper CALL, and the versioned matcher's post-hoc
/// `fast_clone_not_call_free` scan then discarded the ENTIRE clone — so a
/// loop like `s += a[k] + a[k-1]` lost the fast path for `a[k]` too, 9x. The
/// bounds check restores it: `lower_packed_f64_loop_index_get` tests the index
/// against the live length and takes the fact's side exit, which is a compare
/// and a never-taken branch, not a call. That is the same treatment a foreign
/// counter already gets, and for the same reason — an index the loop bound
/// does not cover needs a run-time test, not a refusal.
///
/// The comparison is UNSIGNED, so a negative index (`a[k-1]` at `k == 0`)
/// exceeds any length and takes the side exit. Reads only: a store side exit
/// has replay semantics this does not reason about.
pub(crate) fn packed_f64_loop_offset_read(
    ctx: &FnCtx<'_>,
    arr_id: u32,
    index: &Expr,
) -> Option<(PackedF64LoopFact, u32, i32, bool)> {
    let (idx_id, offset) = packed_f64_loop_index_parts(index)?;
    let fact = packed_f64_loop_fact(ctx, arr_id, idx_id)?;
    // A window-validated or hole-tolerant fact already covers the offset; only
    // the length-bound guard of the classic matcher leaves it unproven.
    let needs_bounds_check = offset != 0 && !fact.allow_holes && !fact.window_validated;
    Some((fact, idx_id, offset, needs_bounds_check))
}

/// #9253: an active receiver-only (`affine_indices`) fact for `arr_id`, when
/// `index` is an affine integer expression rather than a bare local.
///
/// The bare-local cases belong to `foreign_packed_loop_read` and the clone's
/// own counter path; this is `a[i * size + k]`.

/// Conservative magnitude bound of an affine index tree, with every leaf at
/// its i32 extreme. `None` means a node outside the affine grammar.
///
/// #9294 shipped the i64 materialization with the claim that proven-i32
/// leaves cannot overflow it. That is true for one multiply — |i32 * i32|
/// <= 2^62 — and FALSE beyond it: three chained near-2^31 factors reach
/// 2^93, wrap i64, and a wrapped value that happens to land in [0, len)
/// passes the unsigned bounds check and reads a DIFFERENT element than the
/// generic path (JS computes the index in doubles, goes out of bounds, and
/// yields `undefined`). Flagged by review on the follow-up PR. The bound is
/// computed in i128 at match time, so admission costs nothing at run time:
/// a tree whose worst case fits i63 cannot wrap, and anything else declines
/// to the generic path.

/// How many times the counter appears in an affine tree. A tree LINEAR in
/// the counter (exactly one occurrence, and only under Add/Sub/Mul with
/// invariant co-factors) takes its extreme values at the loop's endpoints,
/// which is what licenses the endpoint window proof in the entry guard. A
/// tree where the counter appears twice (`k * k`) is not monotone and keeps
/// its per-read bounds check.
pub(crate) fn affine_counter_occurrences(index: &Expr, counter_id: u32) -> u32 {
    match index {
        Expr::LocalGet(id) if *id == counter_id => 1,
        Expr::Binary { left, right, .. } => {
            affine_counter_occurrences(left, counter_id)
                + affine_counter_occurrences(right, counter_id)
        }
        _ => 0,
    }
}

pub(crate) fn affine_index_magnitude_bound(index: &Expr) -> Option<i128> {
    match index {
        Expr::Integer(v) => Some((*v as i128).unsigned_abs().min(1 << 31) as i128),
        Expr::Number(n) if n.fract() == 0.0 && n.abs() <= f64::from(i32::MAX) => {
            Some(n.abs() as i128)
        }
        Expr::LocalGet(_) => Some(1_i128 << 31),
        Expr::Binary { op, left, right } => {
            let l = affine_index_magnitude_bound(left)?;
            let r = affine_index_magnitude_bound(right)?;
            match op {
                perry_hir::BinaryOp::Add | perry_hir::BinaryOp::Sub => l.checked_add(r),
                perry_hir::BinaryOp::Mul => l.checked_mul(r),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The i64 materialization is wrap-free for exactly the trees this accepts.
pub(crate) fn affine_index_fits_i64(index: &Expr) -> bool {
    affine_index_magnitude_bound(index).is_some_and(|b| b < (1_i128 << 63))
}

pub(crate) fn affine_packed_loop_read(
    ctx: &FnCtx<'_>,
    arr_id: u32,
    index: &Expr,
) -> Option<PackedF64LoopFact> {
    if matches!(index, Expr::LocalGet(_)) {
        return None;
    }
    let fact = ctx
        .packed_f64_loop_facts
        .iter()
        .rev()
        .find(|fact| fact.array_local_id == arr_id && fact.affine_indices)?
        .clone();
    (affine_index_fits_i64(index)
        && affine_index_leaves_materializable(ctx, index, fact.index_local_id))
    .then_some(fact)
}

/// Every leaf must be readable as an i32 here, matching exactly what the
/// matcher's `affine_leaf_ok` admitted. If these two disagree the matcher
/// admits a shape this lowering declines, the read falls back to a helper
/// call, and the clone's call-free scan discards the whole clone — the #9259
/// cascade.
fn affine_index_leaves_materializable(ctx: &FnCtx<'_>, index: &Expr, counter_id: u32) -> bool {
    match index {
        Expr::Integer(v) => i32::try_from(*v).is_ok(),
        Expr::Number(n) => {
            n.fract() == 0.0 && *n >= f64::from(i32::MIN) && *n <= f64::from(i32::MAX)
        }
        Expr::LocalGet(id) => *id == counter_id || ctx.i32_counter_slots.contains_key(id),
        Expr::Binary {
            op: perry_hir::BinaryOp::Add | perry_hir::BinaryOp::Sub | perry_hir::BinaryOp::Mul,
            left,
            right,
        } => {
            affine_index_leaves_materializable(ctx, left, counter_id)
                && affine_index_leaves_materializable(ctx, right, counter_id)
        }
        _ => false,
    }
}

/// Materialise the index in **i64** from each leaf's shared i32 shadow.
///
/// i64 rather than i32 because the source expression evaluates in doubles and
/// `i * size` can exceed i32 for a large matrix even when the final sum is a
/// valid index; computing in i32 would wrap and could produce an in-bounds
/// index for an out-of-bounds access. Every leaf is a proven i32, so three
/// levels of add/sub/mul cannot overflow i64.
pub(crate) fn emit_affine_index_i64(
    ctx: &mut FnCtx<'_>,
    index: &Expr,
    counter_id: u32,
) -> Option<String> {
    emit_affine_index_i64_with(ctx, index, counter_id, None)
}

/// Like [`emit_affine_index_i64`], with the counter's value optionally
/// substituted by a caller-provided i64 SSA value instead of its slot load.
/// The entry guard uses this to evaluate an affine tree at the loop's
/// endpoints (counter = start, counter = bound - 1) while every other leaf
/// still reads its live, loop-invariant slot.
pub(crate) fn emit_affine_index_i64_with(
    ctx: &mut FnCtx<'_>,
    index: &Expr,
    counter_id: u32,
    counter_override: Option<&str>,
) -> Option<String> {
    match index {
        Expr::Integer(v) => Some(v.to_string()),
        Expr::Number(n) => Some(format!("{}", *n as i64)),
        Expr::LocalGet(id) => {
            if *id == counter_id {
                if let Some(value) = counter_override {
                    return Some(value.to_string());
                }
            }
            let slot = ctx.i32_counter_slots.get(id)?.clone();
            let narrow = ctx.block().load(I32, &slot);
            Some(ctx.block().sext(I32, &narrow, I64))
        }
        Expr::Binary { op, left, right } => {
            let l = emit_affine_index_i64_with(ctx, left, counter_id, counter_override)?;
            let r = emit_affine_index_i64_with(ctx, right, counter_id, counter_override)?;
            Some(match op {
                perry_hir::BinaryOp::Add => ctx.block().add(I64, &l, &r),
                perry_hir::BinaryOp::Sub => ctx.block().sub(I64, &l, &r),
                perry_hir::BinaryOp::Mul => ctx.block().mul(I64, &l, &r),
                _ => return None,
            })
        }
        _ => None,
    }
}
