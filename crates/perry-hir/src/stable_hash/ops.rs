//! `SH` impls for HIR operators + small expression-element enums.
//! Split out of `stable_hash.rs` (no behavior change).

use super::primitives::{tag, SH};
use super::StableHasher;
use crate::ir::*;

// --- Operators -------------------------------------------------------------

impl SH for BinaryOp {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        let n: u8 = match self {
            BinaryOp::Add => 0,
            BinaryOp::Sub => 1,
            BinaryOp::Mul => 2,
            BinaryOp::Div => 3,
            BinaryOp::Mod => 4,
            BinaryOp::Pow => 5,
            BinaryOp::BitAnd => 6,
            BinaryOp::BitOr => 7,
            BinaryOp::BitXor => 8,
            BinaryOp::Shl => 9,
            BinaryOp::Shr => 10,
            BinaryOp::UShr => 11,
        };
        h.write(&[n]);
    }
}

impl SH for UnaryOp {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        let n: u8 = match self {
            UnaryOp::Neg => 0,
            UnaryOp::Not => 1,
            UnaryOp::BitNot => 2,
            UnaryOp::Pos => 3,
        };
        h.write(&[n]);
    }
}

impl SH for CompareOp {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        let n: u8 = match self {
            CompareOp::Eq => 0,
            CompareOp::Ne => 1,
            CompareOp::LooseEq => 2,
            CompareOp::LooseNe => 3,
            CompareOp::Lt => 4,
            CompareOp::Le => 5,
            CompareOp::Gt => 6,
            CompareOp::Ge => 7,
        };
        h.write(&[n]);
    }
}

impl SH for LogicalOp {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        let n: u8 = match self {
            LogicalOp::And => 0,
            LogicalOp::Or => 1,
            LogicalOp::Coalesce => 2,
        };
        h.write(&[n]);
    }
}

impl SH for UpdateOp {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        let n: u8 = match self {
            UpdateOp::Increment => 0,
            UpdateOp::Decrement => 1,
        };
        h.write(&[n]);
    }
}

impl SH for ArrayElement {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        match self {
            ArrayElement::Expr(e) => {
                tag(h, 0);
                e.hash(h);
            }
            ArrayElement::Spread(e) => {
                tag(h, 1);
                e.hash(h);
            }
        }
    }
}

impl SH for CallArg {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        match self {
            CallArg::Expr(e) => {
                tag(h, 0);
                e.hash(h);
            }
            CallArg::Spread(e) => {
                tag(h, 1);
                e.hash(h);
            }
        }
    }
}
