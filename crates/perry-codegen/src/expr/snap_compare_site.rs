//! SNAPK2b census: the `compare_literal` site kind (DESIGN27 §3), emitted in
//! instrumented compiles only (`PERRY_SNAP_CENSUS` set while compiling). An
//! ordinary compile emits nothing.
//!
//! The generic equality compare (`===`, `!==`, `==`, `!=` in
//! `expr::compare::lower`) calls [`emit`] with the two operand values it is
//! about to compare, so the hook reads exactly the values the compare reads,
//! in the function and block that compares them. `js_snap_compare_literal(op,
//! left, right, literal)` runs before the compare itself; `literal` has bit 0
//! (1) set when the left (right) operand is a string literal. The runtime keeps
//! a row only when one operand is a host value or env string and the other a
//! string: the guard then holds the predicate, not the host value.
//!
//! A compare a fast path lowers (`typeof x === "…"`, a numeric literal operand,
//! a typed or chained compare) never reaches [`emit`] and emits no row, so the
//! read stays a whole-value member. Nothing is kept between calls and no table
//! is emitted.

use perry_hir::{CompareOp, Expr};

use crate::types::{DOUBLE, I32};

use super::FnCtx;

/// Emit the census hook for the equality compare `left op right` whose
/// operands the caller has lowered to the JS values `l` and `r`.
pub(crate) fn emit(
    ctx: &mut FnCtx<'_>,
    op: CompareOp,
    left: &Expr,
    right: &Expr,
    l: &str,
    r: &str,
) {
    let op = match op {
        CompareOp::Eq => 0,
        CompareOp::Ne => 1,
        CompareOp::LooseEq => 2,
        CompareOp::LooseNe => 3,
        _ => return,
    };
    if !super::snap_env_site::census_compile() {
        return;
    }
    let literal = i32::from(matches!(left, Expr::String(_)))
        | (i32::from(matches!(right, Expr::String(_))) << 1);
    ctx.block().call_void(
        "js_snap_compare_literal",
        &[
            (I32, &op.to_string()),
            (DOUBLE, l),
            (DOUBLE, r),
            (I32, &literal.to_string()),
        ],
    );
}

#[cfg(test)]
#[path = "snap_compare_site_tests.rs"]
mod tests;
