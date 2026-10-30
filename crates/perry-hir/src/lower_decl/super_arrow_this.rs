//! An arrow passed directly to `super(...)` reads the derived constructor's
//! `this` binding, which `super()` initializes when it returns: to the
//! object the parent constructor returned, when it returned one. Closures
//! capture `this` when they are created, before that. The callable-class
//! pattern (`Callable`'s constructor returns
//! `Object.setPrototypeOf(fn.bind(this), new.target.prototype)`; ArkType's
//! `GenericRoot` and `BaseNode` pass the arrow `fn`) then ran the arrow on the
//! discarded allocation (`this.paramDefs` undefined).
//!
//! Each such `super(...)` statement becomes
//!
//! ```text
//! const cell = {};
//! super(.., () => .. cell.t .., ..);
//! cell.t = this;
//! ```
//!
//! An arrow that runs before `super()` returns reads `undefined` where the
//! binding would throw a ReferenceError.

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
            Stmt::Expr(Expr::SuperCall(args)) if args.iter().any(is_this_arrow) => {
                let cell = ctx.fresh_local();
                let read = Expr::PropertyGet {
                    object: Box::new(Expr::LocalGet(cell)),
                    property: "t".to_string(),
                    byte_offset: 0,
                    byte_end: 0,
                };
                for arg in args.iter_mut().filter(|a| is_this_arrow(a)) {
                    crate::analysis::substitute_lexical_this_in_expr(arg, &read);
                }
                stmts.insert(i, cell_decl(cell));
                stmts.insert(
                    i + 2,
                    Stmt::Expr(Expr::PropertySet {
                        object: Box::new(Expr::LocalGet(cell)),
                        property: "t".to_string(),
                        value: Box::new(Expr::This),
                    }),
                );
                i += 3;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
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
