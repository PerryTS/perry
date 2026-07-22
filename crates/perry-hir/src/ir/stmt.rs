//! Statement-level HIR: Stmt enum, SwitchCase, CatchClause. Re-exported from
//! `super`.

use super::*;
use crate::types::{LocalId, Type};

/// Statement in function body
#[derive(Debug, Clone)]
pub enum Stmt {
    /// Local variable declaration: let/const x = expr
    Let {
        id: LocalId,
        name: String,
        ty: Type,
        mutable: bool,
        init: Option<Expr>,
    },
    /// Expression statement
    Expr(Expr),
    /// Return statement
    Return(Option<Expr>),
    /// If statement
    If {
        condition: Expr,
        then_branch: Vec<Stmt>,
        else_branch: Option<Vec<Stmt>>,
    },
    /// While loop
    While { condition: Expr, body: Vec<Stmt> },
    /// Do-while loop (body runs at least once, condition checked at the end)
    DoWhile { body: Vec<Stmt>, condition: Expr },
    /// For loop (lowered from various JS for loops)
    For {
        init: Option<Box<Stmt>>,
        condition: Option<Expr>,
        update: Option<Expr>,
        body: Vec<Stmt>,
    },
    /// Labeled statement: `label: for/while/do/block`
    Labeled { label: String, body: Box<Stmt> },
    /// Break statement
    Break,
    /// Continue statement
    Continue,
    /// Labeled break: `break label;`
    LabeledBreak(String),
    /// Labeled continue: `continue label;`
    LabeledContinue(String),
    /// Throw statement
    Throw(Expr),
    /// Try-catch-finally
    Try {
        body: Vec<Stmt>,
        catch: Option<CatchClause>,
        finally: Option<Vec<Stmt>>,
    },
    /// Switch statement
    Switch {
        discriminant: Expr,
        cases: Vec<SwitchCase>,
    },
    /// Pre-allocate slot+box for a set of LocalIds at function-body
    /// entry. Emitted by `lower_fn_body_block_stmt` to support hoisted
    /// inner `function`-declarations that capture sibling FnDecls or
    /// forward `let`/`const` bindings whose own `Stmt::Let` would
    /// otherwise lazily allocate the box at source position. Issue #569.
    PreallocateBoxes(Vec<LocalId>),
    /// Like `PreallocateBoxes`, but seeds each box with the TAG_TDZ sentinel
    /// (Temporal Dead Zone) instead of `undefined`. Emitted for lexical
    /// `let`/`const`/`class` bindings that are referenced (directly or via a
    /// closure) BEFORE their declaration in the same function/module body. A
    /// read of such a box before its `Stmt::Let` runs throws a spec
    /// ReferenceError; the `Stmt::Let` (or `let x;` with no init) overwrites
    /// the sentinel with the real value / `undefined`, ending the dead zone.
    PreallocateTdzBoxes(Vec<LocalId>),
}

/// A case in a switch statement
#[derive(Debug, Clone)]
pub struct SwitchCase {
    /// Test expression (None for default case)
    pub test: Option<Expr>,
    /// Statements in this case (including fallthrough)
    pub body: Vec<Stmt>,
}

/// Catch clause in try statement
#[derive(Debug, Clone)]
pub struct CatchClause {
    pub param: Option<(LocalId, String)>,
    pub body: Vec<Stmt>,
}
