//! Expose suspensions in a source-ordered, immediately invoked builder.
//!
//! Object literal lowering uses a non-async, non-generator closure whose
//! straight-line body returns its seed parameter. Raw await/yield expressions
//! in that body still belong to the enclosing source function. Recognize that
//! HIR structure, rather than a generated parameter name, and replay it through
//! the enclosing suspension pass. Actual async/generator closures keep their
//! own suspension boundary.

use perry_hir::types::{LocalId, Type};
use perry_hir::{Expr, Stmt};

fn builder_body(expr: &Expr) -> Option<&[Stmt]> {
    let Expr::Closure {
        params,
        body,
        is_async: false,
        is_generator: false,
        ..
    } = expr
    else {
        return None;
    };
    let [param] = params.as_slice() else {
        return None;
    };
    if param.is_rest || param.default.is_some() {
        return None;
    }
    let (last, steps) = body.split_last()?;
    if !matches!(last, Stmt::Return(Some(Expr::LocalGet(id))) if *id == param.id)
        || !steps
            .iter()
            .all(|stmt| matches!(stmt, Stmt::Let { .. } | Stmt::Expr(_)))
    {
        return None;
    }
    Some(steps)
}

/// Only closures with a transparent builder structure may expose suspensions
/// to the enclosing expression walk.
pub(crate) fn contains(expr: &Expr, predicate: fn(&Expr) -> bool) -> bool {
    builder_body(expr).is_some_and(|steps| {
        steps.iter().any(|stmt| match stmt {
            Stmt::Let {
                init: Some(expr), ..
            }
            | Stmt::Expr(expr) => predicate(expr),
            _ => false,
        })
    })
}

pub(crate) fn inline(
    expr: &mut Expr,
    next_id: &mut LocalId,
    hoisted: &mut Vec<Stmt>,
    predicate: fn(&Expr) -> bool,
    hoist: fn(&mut Vec<Stmt>, &mut LocalId),
) -> bool {
    let Expr::Call { callee, args, .. } = expr else {
        return false;
    };
    if args.len() != 1 || !contains(callee, predicate) {
        return false;
    }
    let Expr::Closure { params, body, .. } = callee.as_mut() else {
        unreachable!();
    };
    let param = &params[0];
    let id = param.id;
    let mut steps = vec![Stmt::Let {
        id,
        name: param.name.clone(),
        ty: Type::Any,
        mutable: false,
        init: Some(args.remove(0)),
    }];
    let mut body = std::mem::take(body);
    body.pop(); // The validated return of the seed parameter.
    steps.extend(body);
    hoist(&mut steps, next_id);
    hoisted.extend(steps);
    *expr = Expr::LocalGet(id);
    true
}
