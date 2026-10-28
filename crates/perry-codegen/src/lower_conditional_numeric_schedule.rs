//! Schedule a cheap scalar guard before an independent FP comparison.
//!
//! This only changes Boolean test control flow. Both comparisons must be
//! total, effect-free computations over actual Number locals/literals. It
//! never changes the order of arithmetic operators within either operand.
use crate::expr::FnCtx;
use perry_hir::{BinaryOp, Expr};

fn number(expr: &Expr, local: &impl Fn(u32) -> bool) -> bool {
    match expr {
        Expr::Integer(_) | Expr::Number(_) => true,
        Expr::LocalGet(id) => local(*id),
        Expr::Binary {
            op: BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div,
            left,
            right,
        } => number(left, local) && number(right, local),
        _ => false,
    }
}
fn simple(expr: &Expr) -> bool {
    matches!(expr, Expr::Integer(_) | Expr::Number(_) | Expr::LocalGet(_))
}
fn comparison(expr: &Expr, local: &impl Fn(u32) -> bool) -> Option<bool> {
    let Expr::Compare { left, right, .. } = expr else {
        return None;
    };
    (number(left, local) && number(right, local)).then(|| simple(left) && simple(right))
}
pub(super) fn cheap_guard_first(ctx: &FnCtx<'_>, left: &Expr, right: &Expr) -> bool {
    let local = |id| {
        !ctx.pod_records.contains_key(&id)
            && crate::type_analysis::expr_produces_canonical_raw_f64(ctx, &Expr::LocalGet(id))
    };
    comparison(left, &local) == Some(false) && comparison(right, &local) == Some(true)
}
#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::CompareOp;
    fn cmp(left: Expr) -> Expr {
        Expr::Compare {
            op: CompareOp::Lt,
            left: Box::new(left),
            right: Box::new(Expr::Integer(4)),
        }
    }
    #[test]
    fn numeric_schedule_requires_actual_scalar_leaves() {
        let arithmetic = cmp(Expr::Binary {
            op: BinaryOp::Mul,
            left: Box::new(Expr::LocalGet(1)),
            right: Box::new(Expr::LocalGet(1)),
        });
        assert_eq!(comparison(&arithmetic, &|_| true), Some(false));
        assert_eq!(comparison(&arithmetic, &|_| false), None);
        assert_eq!(comparison(&cmp(Expr::LocalGet(1)), &|_| true), Some(true));
        assert_eq!(comparison(&cmp(Expr::String("1".into())), &|_| true), None);
        assert!(!number(
            &Expr::NumberCoerce(Box::new(Expr::LocalGet(1))),
            &|_| true
        ));
        assert!(!number(&Expr::GlobalGet(1), &|_| true));
    }
}
