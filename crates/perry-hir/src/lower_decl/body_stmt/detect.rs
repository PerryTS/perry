//! for-await/for-of TARGET DETECTION helpers: predicates that decide
//! whether a `for await (…)` / `for (… of …)` head expression is a web
//! ReadableStream, a Node Readable, a readline interface, or an fs.Dir
//! handle (each gets a specialized lowering in `lower_body_stmt`), plus
//! the shared `iterator_return_call` / `insert_iterator_return_before_abrupts`
//! IteratorClose machinery. Split out of `body_stmt.rs` for the 2000-line
//! file-size gate; the twins of these helpers for the `lower/stmt_loops.rs`
//! duplicate lowering path live there (see #4786).

use super::*;

pub(super) fn unwrap_stream_expr(mut expr: &ast::Expr) -> &ast::Expr {
    loop {
        expr = match expr {
            ast::Expr::TsAs(ts_as) => &ts_as.expr,
            ast::Expr::TsNonNull(non_null) => &non_null.expr,
            ast::Expr::TsConstAssertion(assertion) => &assertion.expr,
            ast::Expr::TsTypeAssertion(assertion) => &assertion.expr,
            ast::Expr::Paren(paren) => &paren.expr,
            _ => break,
        };
    }
    expr
}

pub(super) fn web_readable_stream_values_receiver(expr: &ast::Expr) -> Option<&ast::Expr> {
    let ast::Expr::Call(call) = unwrap_stream_expr(expr) else {
        return None;
    };
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return None;
    };
    let ast::Expr::Member(member) = callee_expr.as_ref() else {
        return None;
    };
    if !matches!(&member.prop, ast::MemberProp::Ident(prop) if prop.sym.as_ref() == "values") {
        return None;
    }
    Some(member.obj.as_ref())
}

pub(super) fn is_web_readable_stream_expr(ctx: &LoweringContext, expr: &ast::Expr) -> bool {
    match unwrap_stream_expr(expr) {
        ast::Expr::Ident(ident) => {
            let name = ident.sym.as_ref();
            matches!(
                ctx.lookup_native_instance(name),
                Some((_, "ReadableStream"))
            ) || matches!(
                ctx.lookup_local_type(name),
                Some(Type::Named(n)) if n == "ReadableStream"
            )
        }
        ast::Expr::New(new_expr) => matches!(
            new_expr.callee.as_ref(),
            ast::Expr::Ident(callee) if callee.sym.as_ref() == "ReadableStream"
        ),
        _ => false,
    }
}

pub(super) fn strip_for_of_expr_wrappers(mut expr: &ast::Expr) -> &ast::Expr {
    loop {
        expr = match expr {
            ast::Expr::TsAs(x) => &x.expr,
            ast::Expr::TsNonNull(x) => &x.expr,
            ast::Expr::TsConstAssertion(x) => &x.expr,
            ast::Expr::Paren(x) => &x.expr,
            _ => return expr,
        };
    }
}

pub(super) fn is_node_readable_class_ref(expr: &ast::Expr) -> bool {
    match strip_for_of_expr_wrappers(expr) {
        ast::Expr::Ident(ident) => ident.sym.as_ref() == "Readable",
        ast::Expr::Member(member) => {
            matches!(&member.prop, ast::MemberProp::Ident(prop) if prop.sym.as_ref() == "Readable")
        }
        _ => false,
    }
}

pub(super) fn is_node_readable_static_factory(expr: &ast::Expr) -> bool {
    let ast::Expr::Call(call) = strip_for_of_expr_wrappers(expr) else {
        return false;
    };
    let ast::Callee::Expr(callee) = &call.callee else {
        return false;
    };
    let ast::Expr::Member(member) = strip_for_of_expr_wrappers(callee.as_ref()) else {
        return false;
    };
    let ast::MemberProp::Ident(prop) = &member.prop else {
        return false;
    };
    matches!(prop.sym.as_ref(), "from" | "of") && is_node_readable_class_ref(&member.obj)
}

pub(super) fn is_node_readable_expr(ctx: &LoweringContext, expr: &ast::Expr) -> bool {
    is_node_readable_static_factory(expr)
        || is_node_readable_helper_chain(ctx, expr)
        || matches!(
            crate::lower_types::infer_type_from_expr(strip_for_of_expr_wrappers(expr), ctx),
            Type::Named(name) if name == "Readable"
        )
}

pub(super) fn is_node_readable_helper_chain(ctx: &LoweringContext, expr: &ast::Expr) -> bool {
    let ast::Expr::Call(call) = strip_for_of_expr_wrappers(expr) else {
        return false;
    };
    let ast::Callee::Expr(callee) = &call.callee else {
        return false;
    };
    let ast::Expr::Member(member) = strip_for_of_expr_wrappers(callee.as_ref()) else {
        return false;
    };
    let ast::MemberProp::Ident(prop) = &member.prop else {
        return false;
    };
    match prop.sym.as_ref() {
        "from" | "of" => is_node_readable_class_ref(&member.obj),
        "map" | "filter" | "flatMap" | "take" | "drop" | "compose" => {
            is_node_readable_expr(ctx, &member.obj)
        }
        _ => false,
    }
}

/// `for await (const line of rl)` where `rl = readline.createInterface(...)`.
/// Async-function-body counterpart of the same check in `lower/stmt_loops.rs`.
pub(super) fn is_readline_interface_for_await_target(
    ctx: &LoweringContext,
    expr: &ast::Expr,
) -> bool {
    matches!(
        strip_for_of_expr_wrappers(expr),
        ast::Expr::Ident(ident)
            if matches!(
                ctx.lookup_native_instance(ident.sym.as_ref()),
                Some(("readline", "Interface"))
            )
    )
}

pub(super) fn is_fs_dir_type(ty: Type) -> bool {
    matches!(ty, Type::Named(name) if name == "Dir" || name == "fs.Dir")
}

pub(super) fn is_fs_dir_for_await_target(ctx: &LoweringContext, expr: &ast::Expr) -> bool {
    let expr = strip_for_of_expr_wrappers(expr);
    if is_fs_dir_type(crate::lower_types::infer_type_from_expr(expr, ctx)) {
        return true;
    }

    let ast::Expr::Call(call) = expr else {
        return false;
    };
    let ast::Callee::Expr(callee) = &call.callee else {
        return false;
    };
    let ast::Expr::Member(member) = strip_for_of_expr_wrappers(callee.as_ref()) else {
        return false;
    };
    if !matches!(&member.prop, ast::MemberProp::Ident(prop) if prop.sym.as_ref() == "entries") {
        return false;
    }
    is_fs_dir_type(crate::lower_types::infer_type_from_expr(
        strip_for_of_expr_wrappers(&member.obj),
        ctx,
    ))
}

pub(super) fn iterator_return_call(iter_id: LocalId, needs_await: bool) -> Expr {
    let call = Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            byte_offset: 0,
            object: Box::new(Expr::LocalGet(iter_id)),
            property: "return".to_string(),
        }),
        args: vec![],
        type_args: vec![],
        byte_offset: 0,
    };
    if needs_await {
        Expr::Await(Box::new(call))
    } else {
        call
    }
}

pub(super) fn insert_iterator_return_before_abrupts(
    stmts: &mut Vec<Stmt>,
    iter_id: LocalId,
    needs_await: bool,
) {
    let mut rewritten = Vec::with_capacity(stmts.len());
    for stmt in stmts.drain(..) {
        match stmt {
            Stmt::Break => {
                rewritten.push(Stmt::Expr(iterator_return_call(iter_id, needs_await)));
                rewritten.push(Stmt::Break);
            }
            Stmt::LabeledBreak(label) => {
                rewritten.push(Stmt::Expr(iterator_return_call(iter_id, needs_await)));
                rewritten.push(Stmt::LabeledBreak(label));
            }
            Stmt::Return(value) => {
                rewritten.push(Stmt::Expr(iterator_return_call(iter_id, needs_await)));
                rewritten.push(Stmt::Return(value));
            }
            Stmt::Throw(expr) => {
                rewritten.push(Stmt::Expr(iterator_return_call(iter_id, needs_await)));
                rewritten.push(Stmt::Throw(expr));
            }
            Stmt::If {
                condition,
                mut then_branch,
                mut else_branch,
            } => {
                insert_iterator_return_before_abrupts(&mut then_branch, iter_id, needs_await);
                if let Some(else_stmts) = else_branch.as_mut() {
                    insert_iterator_return_before_abrupts(else_stmts, iter_id, needs_await);
                }
                rewritten.push(Stmt::If {
                    condition,
                    then_branch,
                    else_branch,
                });
            }
            other => rewritten.push(other),
        }
    }
    *stmts = rewritten;
}
