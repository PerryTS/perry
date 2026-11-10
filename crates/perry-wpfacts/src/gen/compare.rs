//! Equality and comparison operators (`Expr::Compare`), and where they observe
//! identity (DESIGN-p4l4 §2.2, `Cons::Identity`).
use super::*;
use perry_hir::{CompareOp, Expr};

impl<'a> Gen<'a> {
    /// `left op right`.
    pub(crate) fn compare_expr(
        &mut self,
        fx: &Fx,
        op: &CompareOp,
        left: &Expr,
        right: &Expr,
    ) -> NodeId {
        match op {
            CompareOp::Eq | CompareOp::Ne => {
                // `===` / `!==` (IsStrictlyEqual) runs no protocol; it
                // compares objects by identity. Against a primitive
                // literal it is a presence test, no identity.
                let l = self.expr(fx, left);
                let r = self.expr(fx, right);
                self.raise_on(l);
                self.raise_on(r);
                if !primitive_literal(left) && !primitive_literal(right) {
                    self.m.identity(l, IdentityRoute::StrictEq);
                    self.m.identity(r, IdentityRoute::StrictEq);
                }
                self.equality_demand(left, right, l, r);
                self.prim_node(prim::BOOL)
            }
            // `==` / `!=` (IsLooselyEqual): ToPrimitive on an object
            // against a primitive, identity between two objects.
            CompareOp::LooseEq | CompareOp::LooseNe
                if !primitive_literal(left) && !primitive_literal(right) =>
            {
                self.prim_op_identity(
                    fx,
                    &[left, right],
                    prim::BOOL,
                    "ToPrimitive compare",
                    IdentityRoute::LooseEq,
                )
            }
            _ => self.prim_op(
                fx,
                &[left, right],
                prim::BOOL,
                "ToPrimitive compare",
                Protocol::ToPrimitive,
            ),
        }
    }

    /// `prim_op` with ToPrimitive whose operands are also compared by
    /// identity (`Model::identity`, value records only): the same nodes and
    /// constraints as `prim_op` otherwise.
    pub(crate) fn prim_op_identity(
        &mut self,
        fx: &Fx,
        operands: &[&Expr],
        result: u16,
        why: &str,
        route: IdentityRoute,
    ) -> NodeId {
        let r = self.m.reason(why);
        for o in operands {
            let v = self.expr(fx, o);
            self.m.cons(
                v,
                Cons::Implicit {
                    reason: r,
                    proto: Protocol::ToPrimitive,
                },
            );
            self.m.identity(v, route);
        }
        let out = self.tmp("primop");
        self.m.add_prim(out, result);
        out
    }
}

/// A primitive literal operand: `x === undefined`, `x === 0`, `s === "k"`
/// compare a value with a primitive, which observes no object identity.
pub(crate) fn primitive_literal(e: &Expr) -> bool {
    matches!(
        e,
        Expr::Undefined
            | Expr::Null
            | Expr::Bool(_)
            | Expr::Number(_)
            | Expr::Integer(_)
            | Expr::BigInt(_)
            | Expr::String(_)
            | Expr::WtfString(_)
    )
}
