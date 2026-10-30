//! An arrow created before `super()` returns reads the derived constructor's
//! `this` binding, which `super()` initializes when it returns: to the object
//! the parent constructor returned, when it returned one. Closures capture
//! `this` when they are created, before that. The callable-class pattern
//! (`Callable`'s constructor returns
//! `Object.setPrototypeOf(fn.bind(this), new.target.prototype)`; ArkType's
//! `GenericRoot` and `BaseNode` pass the arrow `fn`) then ran the arrow on the
//! discarded allocation (`this.paramDefs` undefined). The same holds for an
//! arrow created in a statement before `super(...)` (the constructor's own
//! default parameters included: they are its first statements here) and for
//! arrows nested in such an arrow.
//!
//! When a statement list holds a `super(...)` statement and an arrow before
//! it (or in its arguments) reads `this`, it becomes
//!
//! ```text
//! const cell = {};
//! <statements before, their arrows reading cell.t>
//! super(.., () => .. cell.t .., ..);
//! cell.t = this;
//! ```
//!
//! Arrows created after `super()` returns read `this` as before. An arrow
//! that runs before `super()` returns reads `undefined` where the binding
//! would throw a ReferenceError.

use crate::ir::*;
use crate::lower::LoweringContext;
use crate::types::{LocalId, Type};

pub(super) fn bind_super_argument_arrows(ctx: &mut LoweringContext, stmts: &mut Vec<Stmt>) {
    let mut i = 0;
    while i < stmts.len() {
        match &mut stmts[i] {
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                bind_super_argument_arrows(ctx, then_branch);
                if let Some(else_branch) = else_branch {
                    bind_super_argument_arrows(ctx, else_branch);
                }
            }
            Stmt::Expr(Expr::SuperCall(_)) => {
                if bind_before_super(ctx, stmts, i) {
                    // The cell declaration and its assignment were inserted.
                    i += 3;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
}

/// `stmts[at]` is a `super(...)` statement. Rewrites the arrows created
/// before it returns (in `stmts[..at]` and in its arguments) to read the
/// binding cell, when any of them reads `this`; true when it did.
fn bind_before_super(ctx: &mut LoweringContext, stmts: &mut Vec<Stmt>, at: usize) -> bool {
    let cell = ctx.fresh_local();
    let read = Expr::PropertyGet {
        object: Box::new(Expr::LocalGet(cell)),
        property: "t".to_string(),
        byte_offset: 0,
        byte_end: 0,
    };
    // Rewrite a copy: only an arrow that reads `this` makes it read the cell,
    // so a constructor without one stays exactly as it was.
    let mut before: Vec<Stmt> = stmts[..at].to_vec();
    crate::analysis::substitute_lexical_this_in_stmts(&mut before, &read);
    let Stmt::Expr(Expr::SuperCall(args)) = &stmts[at] else {
        return false;
    };
    let mut args = args.clone();
    for arg in args.iter_mut().filter(|a| is_this_arrow(a)) {
        crate::analysis::substitute_lexical_this_in_expr(arg, &read);
    }
    let mut refs = Vec::new();
    let mut visited = std::collections::HashSet::new();
    for s in &before {
        crate::analysis::collect_local_refs_stmt(s, &mut refs, &mut visited);
    }
    for a in &args {
        crate::analysis::collect_local_refs_expr(a, &mut refs, &mut visited);
    }
    if !refs.contains(&cell) {
        return false;
    }
    stmts.splice(..at, before);
    stmts[at] = Stmt::Expr(Expr::SuperCall(args));
    stmts.insert(at + 1, Stmt::Expr(Expr::PropertySet {
        object: Box::new(Expr::LocalGet(cell)),
        property: "t".to_string(),
        value: Box::new(Expr::This),
    }));
    stmts.insert(0, cell_decl(cell));
    true
}

fn is_this_arrow(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Closure {
            captures_this: true,
            is_arrow: true,
            ..
        }
    )
}

fn cell_decl(cell: LocalId) -> Stmt {
    Stmt::Let {
        id: cell,
        name: format!("__perry_super_this_{cell}"),
        ty: Type::Any,
        mutable: false,
        init: Some(Expr::Object(Vec::new())),
    }
}
