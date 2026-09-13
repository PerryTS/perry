//! #10185: the element-shape clone's LOOP-CARRIED index — the `random` access
//! shape.
//!
//! ```text
//! let cursor = 0;
//! for (let i = 0; i < count; i++) {
//!     cursor = (cursor * 17 + 7) % length;   // the recurrence
//!     const index = cursor;                  // optional alias
//!     sum += rows[index].id;
//! }
//! ```
//!
//! ## What makes this different from `const d = i % m`
//!
//! `d` is dead the moment the iteration ends, so #10123 could keep it entirely
//! virtual — its `Let` writes a private i32 alloca and nothing ever reads the
//! real slot. `cursor` is a real `let` that OUTLIVES the loop, so the clone
//! owes a write-back.
//!
//! **Where that write-back goes is a correctness question, not a scheduling
//! one.** The clone's residual check side-exits by resuming the CURRENT
//! iteration in the slow clone, which re-runs the whole body — the recurrence
//! included. A write-back at the update site would therefore apply
//! `cursor = (cursor * 17 + 7) % length` twice for that iteration and every
//! subsequent index would be wrong (silently: still in bounds, still a valid
//! record, just not the one JavaScript names). So the commit is the LAST thing
//! the iteration does, after every side exit it can take:
//!
//! * mid-iteration exit ⇒ the real slot still holds the previous commit, which
//!   is exactly this iteration's entry value, and the slow clone advances it
//!   once;
//! * normal exit ⇒ the last iteration committed, so the real slot holds the
//!   final value the code after the loop reads.
//!
//! ## Why the recurrence is evaluated in i64
//!
//! JavaScript evaluates `(cursor * 17 + 7) % length` in doubles. `frem` on
//! aarch64 is a libm `fmod` call — inside this clone a call is not a slow path,
//! it DELETES the clone (#7690) — so the recurrence is folded to one affine
//! pair `(a, b)` and evaluated as `srem i64`.
//!
//! That is only equal to what JavaScript computes while every intermediate is
//! exactly representable as a double, so the matcher tracks the maximum
//! magnitude any sub-expression can reach over `carried ∈ [0, i32::MAX]` and
//! declines above 2^53. (Fitting i64, which the original design note asked for,
//! is necessary but NOT sufficient: `c * 1e10 + 1` fits i64 comfortably and
//! loses its low bits in the f64 JavaScript actually runs.) `a` and `b` are
//! also required non-negative, so the dividend is non-negative and `srem`
//! agrees with JS `%` — which returns a NEGATIVE remainder for a negative
//! dividend, i.e. an out-of-bounds subscript, not a slow path.

use anyhow::Result;
use perry_hir::{BinaryOp, Expr, Stmt};

use super::loops::{local_bound_is_loop_invariant, local_has_readable_slot};
use crate::expr::FnCtx;
use crate::types::{DOUBLE, I32, I64};

/// The largest integer magnitude an `f64` represents exactly. Above it the i64
/// recurrence and the f64 one JavaScript runs can disagree.
const EXACT_F64_INTEGER_LIMIT: i64 = 9_007_199_254_740_992; // 2^53

/// The preheader materializes the entry value in `0..=i32::MAX`, so this bounds
/// every `carried` the recurrence can ever see.
const MAX_CARRIED: i64 = i32::MAX as i64;

/// A matched `carried = <affine> % m` update.
#[derive(Clone, Copy, Debug)]
pub(super) struct MatchedCarried {
    pub carried_id: u32,
    pub modulus_id: u32,
    pub coeff_a: i64,
    pub coeff_b: i64,
}

/// Fold `expr` to `a * carried + b`, tracking the largest magnitude ANY
/// sub-expression can reach for `carried ∈ [0, i32::MAX]`.
///
/// Returns `(a, b, max_abs)`. `None` declines — an unknown node, a second
/// variable, a non-integral literal, or any overflow of the checked i64
/// arithmetic used to compute the bounds.
///
/// The magnitude is tracked per NODE rather than taken from the folded result
/// because folding can cancel: `c * 1000 - c * 999` is `1 * c`, but JavaScript
/// still evaluates both thousand-fold products.
fn affine_over_carried(expr: &Expr, carried_id: u32) -> Option<(i64, i64, i64)> {
    let magnitude = |a: i64, b: i64| -> Option<i64> {
        a.checked_abs()?
            .checked_mul(MAX_CARRIED)?
            .checked_add(b.checked_abs()?)
    };
    match expr {
        Expr::LocalGet(id) if *id == carried_id => Some((1, 0, MAX_CARRIED)),
        Expr::Integer(k) => Some((0, *k, k.checked_abs()?)),
        Expr::Number(n) if n.is_finite() && n.fract() == 0.0 => {
            // `as i64` saturates rather than wrapping, so an out-of-range
            // literal would fold to i64::MAX and read as a huge-but-valid
            // coefficient. Range-check before converting.
            if *n < -(EXACT_F64_INTEGER_LIMIT as f64) || *n > EXACT_F64_INTEGER_LIMIT as f64 {
                return None;
            }
            let k = *n as i64;
            Some((0, k, k.checked_abs()?))
        }
        Expr::Binary { op, left, right } => {
            let (la, lb, lmax) = affine_over_carried(left, carried_id)?;
            let (ra, rb, rmax) = affine_over_carried(right, carried_id)?;
            let (a, b) = match op {
                BinaryOp::Add => (la.checked_add(ra)?, lb.checked_add(rb)?),
                BinaryOp::Sub => (la.checked_sub(ra)?, lb.checked_sub(rb)?),
                // A product is affine only when one side is a constant; `c * c`
                // is not, and neither is `c * m` for a runtime `m`.
                BinaryOp::Mul if la == 0 => (ra.checked_mul(lb)?, rb.checked_mul(lb)?),
                BinaryOp::Mul if ra == 0 => (la.checked_mul(rb)?, lb.checked_mul(rb)?),
                _ => return None,
            };
            let here = magnitude(a, b)?;
            Some((a, b, lmax.max(rmax).max(here)))
        }
        _ => None,
    }
}

/// Does `expr` read `id`?
///
/// Conservative by construction: an expression node this does not enumerate
/// answers "yes". The alternative shape — a catch-all that returns `false` —
/// is the #6377 / `collect_local_refs_expr` failure mode CLAUDE.md names, and
/// here it would let an accumulator read be folded into a statement that no
/// longer sees the value it was written against.
pub(super) fn expr_reads_local(expr: &Expr, id: u32) -> bool {
    match expr {
        Expr::LocalGet(read) => *read == id,
        Expr::Number(_) | Expr::Integer(_) => false,
        Expr::Binary { left, right, .. }
        | Expr::MathImul(left, right)
        | Expr::MathPow(left, right) => expr_reads_local(left, id) || expr_reads_local(right, id),
        Expr::NumberCoerce(operand) => expr_reads_local(operand, id),
        Expr::MathMin(values) | Expr::MathMax(values) => {
            values.iter().any(|e| expr_reads_local(e, id))
        }
        Expr::MathAbs(value)
        | Expr::MathSqrt(value)
        | Expr::MathFloor(value)
        | Expr::MathCeil(value)
        | Expr::MathRound(value)
        | Expr::MathTrunc(value)
        | Expr::MathSign(value)
        | Expr::MathF16round(value) => expr_reads_local(value, id),
        Expr::PropertyGet { object, .. } => expr_reads_local(object, id),
        Expr::IndexGet { object, index } => {
            expr_reads_local(object, id) || expr_reads_local(index, id)
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            expr_reads_local(condition, id)
                || expr_reads_local(then_expr, id)
                || expr_reads_local(else_expr, id)
        }
        _ => true,
    }
}

/// Match the body's first statement as `carried = <affine over carried> % m`.
#[allow(clippy::too_many_arguments)]
pub(super) fn match_carried_update(
    ctx: &FnCtx<'_>,
    stmt: &Stmt,
    counter_id: u32,
    condition: &Expr,
    update: Option<&Expr>,
    body: &[Stmt],
) -> Option<MatchedCarried> {
    let Stmt::Expr(Expr::LocalSet(carried_id, value)) = stmt else {
        return None;
    };
    let Expr::Binary {
        op: BinaryOp::Mod,
        left,
        right,
    } = value.as_ref()
    else {
        return None;
    };
    let Expr::LocalGet(modulus_id) = right.as_ref() else {
        return None;
    };
    if *carried_id == counter_id || *modulus_id == counter_id || *modulus_id == *carried_id {
        return None;
    }

    let (coeff_a, coeff_b, max_abs) = affine_over_carried(left, *carried_id)?;
    // A negative dividend makes JS `%` return a negative remainder — an
    // out-of-bounds subscript the preheader's `m <= length` says nothing about.
    // With `carried >= 0` (the preheader materializes it in `0..=i32::MAX`) and
    // both coefficients non-negative, the dividend cannot be negative.
    if coeff_a < 0 || coeff_b < 0 {
        return None;
    }
    if max_abs > EXACT_F64_INTEGER_LIMIT {
        return None;
    }

    // The binding must be a plain, directly-addressable local the clone can
    // write back to with one store. A module global would need the rooted-store
    // path, a boxed or captured binding lives in a cell a plain store would
    // leave stale for an observer outside the clone.
    if !ctx.locals.contains_key(carried_id)
        || ctx.module_globals.contains_key(carried_id)
        || ctx.boxed_vars.contains(carried_id)
        || ctx.closure_captures.contains_key(carried_id)
        || !local_has_readable_slot(ctx, *carried_id)
        || ctx.pod_records.contains_key(carried_id)
        || ctx.scalar_replaced.contains_key(carried_id)
    {
        return None;
    }
    // `carried` is written exactly once per iteration — by the statement above.
    // Every other statement the body matcher admits writes the accumulator, and
    // it rejects a body whose accumulator IS the carried local, so the only way
    // a second write could appear is a `for` update expression naming it.
    if update.is_some_and(|u| expr_writes_local(u, *carried_id)) {
        return None;
    }
    // The modulus is materialized ONCE in the preheader and every iteration's
    // `srem` uses that value, so a body (or update, or condition) that could
    // rewrite it would derive indices against a stale bound.
    if ctx.boxed_vars.contains(modulus_id)
        || ctx.closure_captures.contains_key(modulus_id)
        || !(local_has_readable_slot(ctx, *modulus_id)
            || ctx.module_globals.contains_key(modulus_id))
        || !local_bound_is_loop_invariant(condition, update, body, *modulus_id)
    {
        return None;
    }

    Some(MatchedCarried {
        carried_id: *carried_id,
        modulus_id: *modulus_id,
        coeff_a,
        coeff_b,
    })
}

/// Does `expr` assign to `id`? Conservative in the same direction as
/// [`expr_reads_local`] — an unenumerated node answers "yes".
fn expr_writes_local(expr: &Expr, id: u32) -> bool {
    match expr {
        Expr::LocalSet(target, _) => *target == id,
        Expr::Update { id: target, .. } => *target == id,
        Expr::LocalGet(_) | Expr::Number(_) | Expr::Integer(_) => false,
        Expr::Binary { left, right, .. } => {
            expr_writes_local(left, id) || expr_writes_local(right, id)
        }
        _ => true,
    }
}

/// Lower the fast clone's carried-index statements, or report that this
/// expression is not one.
///
/// Two shapes, both synthesized or admitted by the matcher, and neither lowered
/// generically:
///
/// * the UPDATE `carried = <affine> % m` — one `mul`/`add`/`srem` chain in i64
///   writing the clone's private i32 slot, never the real binding;
/// * the COMMIT `carried = carried`, the trailing statement the matcher appends
///   to the synthesized fast body — one `sitofp` + `store` that publishes the
///   iteration's value to the real slot.
///
/// The commit is spelled as a self-assignment because it has to be a statement
/// the generic lowering would also accept (the slow clone never sees it — the
/// facts are popped first — but nothing may depend on that). A user-written
/// `c = c` inside a body cannot reach here: the body matcher admits only the
/// carried update, one virtual binding and accumulator writes, and `c` is not
/// the accumulator.
pub(super) fn lower_virtual_carried_stmt(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<bool> {
    let Expr::LocalSet(id, value) = expr else {
        return Ok(false);
    };
    let Some((carried, scope_id)) = ctx.element_shape_loop_facts.iter().rev().find_map(|fact| {
        let crate::expr::ElementShapeIndex::Carried(carried) = &fact.index else {
            return None;
        };
        (carried.local_id == *id).then(|| (carried.clone(), fact.scope_id))
    }) else {
        return Ok(false);
    };

    if matches!(value.as_ref(), Expr::LocalGet(read) if *read == *id) {
        // COMMIT. The value published here is the one every read in this
        // iteration already used, so the slot and the binding agree from the
        // end of the iteration onward.
        let slot = carried.slot.clone();
        let commit_slot = carried.commit_slot.clone();
        let blk = ctx.block();
        let current = blk.load(I32, &slot);
        let boxed = blk.sitofp(I32, &current, DOUBLE);
        blk.store(DOUBLE, &boxed, &commit_slot);
        return Ok(true);
    }

    // UPDATE. `0 <= carried <= i32::MAX` and `1 <= m <= i32::MAX` hold by the
    // preheader's two materializations, and the matcher proved
    // `a * i32::MAX + b < 2^53`, so the i64 chain cannot overflow and agrees
    // with the f64 arithmetic JavaScript performs.
    let slot = carried.slot.clone();
    let modulus_i32 = carried.modulus_i32.clone();
    let coeff_a = carried.coeff_a.to_string();
    let coeff_b = carried.coeff_b.to_string();
    {
        let blk = ctx.block();
        let current = blk.load(I32, &slot);
        let current64 = blk.sext(I32, &current, I64);
        let scaled = blk.mul(I64, &current64, &coeff_a);
        let shifted = blk.add(I64, &scaled, &coeff_b);
        let modulus64 = blk.sext(I32, &modulus_i32, I64);
        let wrapped = blk.srem(I64, &shifted, &modulus64);
        let next = blk.trunc(I64, &wrapped, I32);
        blk.store(I32, &next, &slot);
    }
    // The index for this iteration is now in its slot, so a body with no alias
    // binding hangs the shared element prologue off the update itself.
    super::element_shape_loop::emit_element_prefetch_for(ctx, scope_id, *id);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(id: u32) -> Expr {
        Expr::LocalGet(id)
    }
    fn mul(l: Expr, r: Expr) -> Expr {
        Expr::Binary {
            op: BinaryOp::Mul,
            left: Box::new(l),
            right: Box::new(r),
        }
    }
    fn add(l: Expr, r: Expr) -> Expr {
        Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(l),
            right: Box::new(r),
        }
    }
    fn sub(l: Expr, r: Expr) -> Expr {
        Expr::Binary {
            op: BinaryOp::Sub,
            left: Box::new(l),
            right: Box::new(r),
        }
    }

    #[test]
    fn the_benchmark_recurrence_folds() {
        // `(cursor * 17 + 7)`
        let e = add(mul(local(9), Expr::Integer(17)), Expr::Integer(7));
        assert_eq!(
            affine_over_carried(&e, 9),
            Some((17, 7, 17 * MAX_CARRIED + 7))
        );
    }

    #[test]
    fn a_constant_only_expression_folds_to_b() {
        assert_eq!(affine_over_carried(&Expr::Integer(5), 9), Some((0, 5, 5)));
    }

    #[test]
    fn a_second_variable_declines() {
        assert_eq!(affine_over_carried(&add(local(9), local(10)), 9), None);
        assert_eq!(affine_over_carried(&mul(local(9), local(9)), 9), None);
    }

    /// The magnitude bound is per NODE, not of the folded pair: cancellation
    /// must not hide an intermediate JavaScript really computes in f64.
    #[test]
    fn cancelling_products_still_report_their_intermediates() {
        // `c * 1000000000 - c * 999999999` folds to `1 * c`, but both products
        // reach ~2e18 for a large `c` — far past 2^53, where the f64 JS runs
        // and this i64 chain stop agreeing.
        let e = sub(
            mul(local(9), Expr::Integer(1_000_000_000)),
            mul(local(9), Expr::Integer(999_999_999)),
        );
        let (a, b, max_abs) = affine_over_carried(&e, 9).expect("affine");
        assert_eq!((a, b), (1, 0));
        assert!(
            max_abs > EXACT_F64_INTEGER_LIMIT,
            "the intermediate magnitude must be reported, not the folded one"
        );
    }

    #[test]
    fn a_non_integral_literal_declines() {
        assert_eq!(affine_over_carried(&Expr::Number(1.5), 9), None);
        assert_eq!(affine_over_carried(&Expr::Number(f64::NAN), 9), None);
        assert_eq!(affine_over_carried(&Expr::Number(f64::INFINITY), 9), None);
        // Saturating `as i64` would turn this into a plausible coefficient.
        assert_eq!(affine_over_carried(&Expr::Number(1e300), 9), None);
    }

    #[test]
    fn reads_are_detected_conservatively() {
        assert!(expr_reads_local(&add(local(9), Expr::Integer(1)), 9));
        assert!(!expr_reads_local(&add(local(8), Expr::Integer(1)), 9));
        // An unenumerated node answers "reads it".
        assert!(expr_reads_local(&Expr::This, 9));
    }
}
