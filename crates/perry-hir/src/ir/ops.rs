//! Operator enums (BinaryOp, UnaryOp, CompareOp, LogicalOp, UpdateOp) and
//! spread-aware element wrappers (ArrayElement, CallArg). Re-exported from
//! `super`.

use super::*;

/// Binary operators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    UShr,
}

/// Unary operators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
    BitNot,
    Pos,
}

/// Comparison operators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    Eq,      // ===
    Ne,      // !==
    LooseEq, // ==
    LooseNe, // !=
    Lt,      // <
    Le,      // <=
    Gt,      // >
    Ge,      // >=
}

/// Logical operators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalOp {
    And,      // &&
    Or,       // ||
    Coalesce, // ??
}

/// Update operators (++/--)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateOp {
    Increment, // ++
    Decrement, // --
}

/// Element in an array literal with spread support
#[derive(Debug, Clone)]
pub enum ArrayElement {
    /// Regular element: [1, 2, 3]
    Expr(Expr),
    /// Spread element: [...arr]
    Spread(Expr),
}

/// Argument in a function call with spread support
#[derive(Debug, Clone)]
pub enum CallArg {
    /// Regular argument: fn(x, y)
    Expr(Expr),
    /// Spread argument: fn(...arr)
    Spread(Expr),
}
