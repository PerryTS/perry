//! Structural probe for object literals behind TypeScript-only wrappers.

use swc_ecma_ast as ast;

/// Whether `expr` is an object literal once TypeScript-only wrappers
/// (`as`, `!`, `satisfies`, `<T>`, `as const`) and parentheses are peeled off.
pub(super) fn is_direct_object_literal(expr: &ast::Expr) -> bool {
    let mut current = expr;
    loop {
        match current {
            ast::Expr::TsAs(wrapper) => current = &wrapper.expr,
            ast::Expr::TsNonNull(wrapper) => current = &wrapper.expr,
            ast::Expr::TsSatisfies(wrapper) => current = &wrapper.expr,
            ast::Expr::TsTypeAssertion(wrapper) => current = &wrapper.expr,
            ast::Expr::TsConstAssertion(wrapper) => current = &wrapper.expr,
            ast::Expr::Paren(wrapper) => current = &wrapper.expr,
            ast::Expr::Object(_) => return true,
            _ => return false,
        }
    }
}
