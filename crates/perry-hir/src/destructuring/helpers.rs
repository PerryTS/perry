//! Helpers used across destructuring lowering modules.

use super::*;

/// Recognize the ink-shape `useState(initial)` pattern when the
/// callee is a perry/tui-imported `useState`. Returns a rewritten
/// HIR expression that calls `useStateTuple` instead — returning a
/// real `[value, setter]` array — when the pattern matches. Otherwise
/// returns None and the caller falls back to standard lowering.
///
/// Only handles the direct-call shape `useState(x)`. Member-call
/// shapes `tui.useState(x)` are also recognized when the namespace
/// resolves to perry/tui.
pub(crate) fn rewrite_use_state_tuple(ctx: &mut LoweringContext, init: &ast::Expr) -> Option<Expr> {
    let call = match init {
        ast::Expr::Call(c) => c,
        _ => return None,
    };
    let (is_use_state, method) = match &call.callee {
        ast::Callee::Expr(e) => match e.as_ref() {
            ast::Expr::Ident(id) => {
                let name = id.sym.as_ref();
                let m = ctx.lookup_native_module(name);
                match m {
                    Some(("perry/tui", Some("useState"))) | Some(("perry/tui", None))
                        if name == "useState" =>
                    {
                        (true, "useStateTuple")
                    }
                    _ => (false, ""),
                }
            }
            ast::Expr::Member(m) => {
                if let (ast::Expr::Ident(obj), ast::MemberProp::Ident(prop)) =
                    (m.obj.as_ref(), &m.prop)
                {
                    if prop.sym.as_ref() == "useState" {
                        match ctx.lookup_native_module(obj.sym.as_ref()) {
                            Some(("perry/tui", _)) => (true, "useStateTuple"),
                            _ => (false, ""),
                        }
                    } else {
                        (false, "")
                    }
                } else {
                    (false, "")
                }
            }
            _ => (false, ""),
        },
        _ => return None,
    };
    if !is_use_state {
        return None;
    }
    let mut arg_exprs: Vec<Expr> = Vec::new();
    for a in &call.args {
        if a.spread.is_some() {
            // Don't rewrite if user code spreads — let the standard path error/handle.
            return None;
        }
        arg_exprs.push(lower_expr(ctx, &a.expr).ok()?);
    }
    Some(Expr::NativeMethodCall {
        module: "perry/tui".to_string(),
        class_name: None,
        object: None,
        method: method.to_string(),
        args: arg_exprs,
    })
}

/// True iff `e` contains an `ast::Expr::Arrow` or `ast::Expr::Fn` at
/// any depth. Used by the let-decl pre-registration path (#593) to
/// extend the issue-#461 self-recursion fix to indirect shapes —
/// `const f = wrap(() => f())` (closure inside a Call), `const sub =
/// subject.subscribe({ next: () => sub.unsubscribe() })` (closure
/// inside an Object), `const h = handlers.map(b => () => h())[0]`
/// (closure inside an Array+Member chain). The cost of a recursive
/// scan over the init AST is negligible — every let-decl runs through
/// it once at HIR lowering time.
pub(crate) fn ast_expr_contains_function_expr(e: &ast::Expr) -> bool {
    use ast::Expr;
    match e {
        Expr::Arrow(_) | Expr::Fn(_) => true,
        Expr::Call(c) => {
            (match &c.callee {
                ast::Callee::Expr(e) => ast_expr_contains_function_expr(e),
                _ => false,
            }) || c
                .args
                .iter()
                .any(|a| ast_expr_contains_function_expr(&a.expr))
        }
        Expr::New(n) => {
            ast_expr_contains_function_expr(&n.callee)
                || n.args.as_ref().map_or(false, |args| {
                    args.iter()
                        .any(|a| ast_expr_contains_function_expr(&a.expr))
                })
        }
        Expr::Member(m) => ast_expr_contains_function_expr(&m.obj),
        Expr::Object(o) => o.props.iter().any(|p| match p {
            ast::PropOrSpread::Spread(s) => ast_expr_contains_function_expr(&s.expr),
            ast::PropOrSpread::Prop(p) => match &**p {
                ast::Prop::KeyValue(kv) => ast_expr_contains_function_expr(&kv.value),
                ast::Prop::Method(_) | ast::Prop::Getter(_) | ast::Prop::Setter(_) => true,
                _ => false,
            },
        }),
        Expr::Array(a) => a
            .elems
            .iter()
            .filter_map(|e| e.as_ref())
            .any(|e| ast_expr_contains_function_expr(&e.expr)),
        Expr::Bin(b) => {
            ast_expr_contains_function_expr(&b.left) || ast_expr_contains_function_expr(&b.right)
        }
        Expr::Unary(u) => ast_expr_contains_function_expr(&u.arg),
        Expr::Cond(c) => {
            ast_expr_contains_function_expr(&c.test)
                || ast_expr_contains_function_expr(&c.cons)
                || ast_expr_contains_function_expr(&c.alt)
        }
        Expr::Paren(p) => ast_expr_contains_function_expr(&p.expr),
        Expr::TsAs(t) => ast_expr_contains_function_expr(&t.expr),
        Expr::TsNonNull(t) => ast_expr_contains_function_expr(&t.expr),
        Expr::TsTypeAssertion(t) => ast_expr_contains_function_expr(&t.expr),
        Expr::TsSatisfies(t) => ast_expr_contains_function_expr(&t.expr),
        Expr::Assign(a) => ast_expr_contains_function_expr(&a.right),
        Expr::Seq(s) => s.exprs.iter().any(|e| ast_expr_contains_function_expr(e)),
        Expr::Tpl(t) => t.exprs.iter().any(|e| ast_expr_contains_function_expr(e)),
        Expr::TaggedTpl(t) => {
            ast_expr_contains_function_expr(&t.tag)
                || t.tpl
                    .exprs
                    .iter()
                    .any(|e| ast_expr_contains_function_expr(e))
        }
        Expr::OptChain(o) => match &*o.base {
            ast::OptChainBase::Member(m) => ast_expr_contains_function_expr(&m.obj),
            ast::OptChainBase::Call(c) => {
                ast_expr_contains_function_expr(&c.callee)
                    || c.args
                        .iter()
                        .any(|a| ast_expr_contains_function_expr(&a.expr))
            }
        },
        _ => false,
    }
}
