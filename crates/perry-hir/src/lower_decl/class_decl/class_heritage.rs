use super::*;

/// Class heritage is evaluated inside the class's lexical name binding. That
/// binding is uninitialized until the heritage expression finishes, so
/// `class C extends C {}` (including a parenthesized `C`) must fail with a
/// ReferenceError instead of resolving an outer binding or creating a
/// recursive static parent edge.
pub(super) fn is_class_self_heritage(expr: &ast::Expr, inner_name: &str) -> bool {
    match expr {
        ast::Expr::Ident(ident) => ident.sym == inner_name,
        ast::Expr::Paren(paren) => is_class_self_heritage(&paren.expr, inner_name),
        _ => false,
    }
}

/// Class definitions are strict-mode code, including function expressions
/// created while evaluating the heritage. Keep the strict context scoped to
/// the heritage expression so a superclass such as
/// `class D extends function(){ arguments.callee } {}` gets a strict
/// arguments object without leaking strictness into surrounding source.
pub(super) fn lower_class_heritage_expr(
    ctx: &mut LoweringContext,
    expr: &ast::Expr,
) -> Result<Expr> {
    ctx.enter_strict_mode(true);
    let lowered = lower_expr(ctx, expr);
    ctx.exit_strict_mode();
    lowered
}

/// Whether this class delegates directly to a dynamic-function heritage.
/// `true` means its implicit constructor forwards the `new` site's arguments;
/// `false` is the exact no-argument `constructor() { super(); }` form.
pub(super) fn dynamic_function_forwarding_mode(class: &ast::Class) -> Option<bool> {
    let constructor = class.body.iter().find_map(|member| match member {
        ast::ClassMember::Constructor(constructor) => Some(constructor),
        _ => None,
    });
    let Some(constructor) = constructor else {
        return Some(true);
    };
    if !constructor.params.is_empty() {
        return None;
    }
    let [ast::Stmt::Expr(statement)] = constructor.body.as_ref()?.stmts.as_slice() else {
        return None;
    };
    let ast::Expr::Call(call) = statement.expr.as_ref() else {
        return None;
    };
    if matches!(call.callee, ast::Callee::Super(_)) && call.args.is_empty() {
        Some(false)
    } else {
        None
    }
}
