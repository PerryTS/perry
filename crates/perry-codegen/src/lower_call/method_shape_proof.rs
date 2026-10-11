//! Consume an entry receiver proof only while evaluation cannot invalidate it.
//! This is compile-time admission, not an emitted guard or another dispatch path.

use crate::expr::FnCtx;
use perry_hir::{CompareOp, Expr, Function, Stmt, UnaryOp};

pub(super) fn can_consume(
    ctx: &FnCtx<'_>,
    class_name: &str,
    property: &str,
    args: &[Expr],
) -> bool {
    if !ctx
        .pshape_methods
        .contains_key(&(class_name.to_string(), property.to_string()))
    {
        return false;
    }
    let Some(method) = ctx
        .classes
        .get(class_name)
        .and_then(|class| class.methods.iter().find(|method| method.name == property))
    else {
        return false;
    };
    if method.body.is_empty()
        || args.len() != method.params.len()
        || method.params.iter().any(|p| p.default.is_some())
        || !args
            .iter()
            .all(|arg| crate::rooting::expr_is_inert_primitive(ctx, arg))
    {
        return false;
    }
    body(&method.body, method, &mut false)
}

fn body(stmts: &[Stmt], method: &Function, reentered: &mut bool) -> bool {
    stmts.iter().all(|stmt| match stmt {
        Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Return(Some(e)) => {
            value(e, method, reentered).is_some()
        }
        Stmt::Return(None) => true,
        Stmt::Let { init, .. } => init
            .as_ref()
            .is_none_or(|e| value(e, method, reentered).is_some()),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            value(condition, method, reentered).is_some()
                && body(then_branch, method, reentered)
                && else_branch
                    .as_ref()
                    .is_none_or(|b| body(b, method, reentered))
        }
        // Loops need a proof at each backedge; keep their ordinary body.
        _ => false,
    })
}

/// Some(true) is a primitive value, Some(false) an unknown value. None means
/// a receiver read/store could follow reentrant user code. Branch traversal
/// deliberately joins effects conservatively; locals retain no new fact map.
fn value(e: &Expr, method: &Function, reentered: &mut bool) -> Option<bool> {
    match e {
        Expr::Undefined
        | Expr::Null
        | Expr::Bool(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::String(_) => Some(true),
        Expr::LocalGet(id) => Some(method.params.iter().any(|p| p.id == *id)),
        Expr::LocalSet(id, _) if method.params.iter().any(|p| p.id == *id) => None,
        Expr::LocalSet(_, e) => value(e, method, reentered),
        Expr::This => (!*reentered).then_some(false),
        Expr::PropertyGet { object, .. } if matches!(object.as_ref(), Expr::This) => {
            (!*reentered).then_some(false)
        }
        Expr::PropertySet {
            object, value: e, ..
        } if matches!(object.as_ref(), Expr::This) => {
            let primitive = value(e, method, reentered)?;
            (!*reentered).then_some(primitive)
        }
        Expr::Binary { left, right, .. } => {
            let a = value(left, method, reentered)?;
            let b = value(right, method, reentered)?;
            *reentered |= !(a && b);
            Some(true)
        }
        Expr::Unary { op, operand } => {
            let primitive = value(operand, method, reentered)?;
            *reentered |= *op != UnaryOp::Not && !primitive;
            Some(true)
        }
        Expr::Compare { op, left, right } => {
            let a = value(left, method, reentered)?;
            let b = value(right, method, reentered)?;
            *reentered |= !matches!(op, CompareOp::Eq | CompareOp::Ne) && !(a && b);
            Some(true)
        }
        Expr::Logical { left, right, .. } => {
            let a = value(left, method, reentered)?;
            let b = value(right, method, reentered)?;
            Some(a && b)
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            value(condition, method, reentered)?;
            let a = value(then_expr, method, reentered)?;
            let b = value(else_expr, method, reentered)?;
            Some(a && b)
        }
        Expr::TypeOf(e) | Expr::Void(e) => {
            value(e, method, reentered)?;
            Some(true)
        }
        // Calls, other property reads, indexing, updates, constructors and
        // unmodeled operations may run user code; don't share the entry proof.
        _ => None,
    }
}
