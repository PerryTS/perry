//! Check mode, −0 tolerance (wpfacts PR D, RFC §10.3 as amended).
//!
//! A local the facts make I32 by tolerance (`ElisionFacts::zero_tolerant_locals`)
//! may hold a `-0` that its i32 slot keeps as `0`. The facts claim that no
//! use of it observes the sign of a zero, so a sign-observing operation
//! (`/` and `**` of either operand, `%` of its dividend, `Math.sign`,
//! `Math.min`/`max`, `Math.pow`, `Math.atan2`, `Object.is`) never reads one.
//! Such a read, when it runs, is a false proof: the witness exits 86. With
//! a sound analysis this code is never emitted (the operation is a
//! zero-sign seed of the local); the negative control `zerosign` drops the
//! division's seed and must be caught here.
use perry_hir::{BinaryOp, Expr};

use crate::expr::FnCtx;
use crate::types::{DOUBLE, I32};

/// The operands of `e` whose zero sign the operation observes.
fn observed(e: &Expr) -> Vec<&Expr> {
    match e {
        Expr::Binary {
            op: BinaryOp::Div | BinaryOp::Pow,
            left,
            right,
        } => vec![left.as_ref(), right.as_ref()],
        Expr::Binary {
            op: BinaryOp::Mod,
            left,
            ..
        } => vec![left.as_ref()],
        Expr::MathSign(x) => vec![x.as_ref()],
        Expr::MathPow(a, b) | Expr::MathAtan2(a, b) | Expr::ObjectIs(a, b) => {
            vec![a.as_ref(), b.as_ref()]
        }
        Expr::MathMin(xs) | Expr::MathMax(xs) => xs.iter().collect(),
        _ => Vec::new(),
    }
}

/// Check mode: before `e` runs, a tolerated local among its sign-observed
/// operands is a false proof.
pub(crate) fn witness(ctx: &mut FnCtx<'_>, e: &Expr) {
    if !super::enabled() || ctx.wpfacts.zero_tolerant_locals.is_empty() {
        return;
    }
    for o in observed(e) {
        let Expr::LocalGet(id) = o else {
            continue;
        };
        if !ctx.wpfacts.zero_tolerant_locals.contains(id) {
            continue;
        }
        super::covered(super::Fact::ZeroSign, || serde_json::json!({ "local": id }));
        let site = super::site_arg(
            ctx,
            &format!("zero sign of local {id}: observed by a tolerated read"),
            super::Fact::ZeroSign,
            &[],
        );
        let _ = super::check(
            ctx,
            I32,
            "perry_wpfacts_check_miss",
            &[(DOUBLE, "0.0")],
            &site,
        );
    }
}
