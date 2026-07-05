//! Helpers used across destructuring lowering modules.

use super::*;

/// Collect the statically-known property keys explicitly bound by an object
/// pattern's KeyValue/Assign props — the keys to EXCLUDE from an object
/// `...rest` binding (`Expr::ObjectRest`). Computed keys have no static name and
/// are skipped, matching the KeyValue key lowering which skips them too. Shared
/// by the destructuring declaration and assignment (expr + stmt) lowering paths.
pub(crate) fn collect_static_object_pattern_keys(props: &[ast::ObjectPatProp]) -> Vec<String> {
    props
        .iter()
        .filter_map(|p| match p {
            ast::ObjectPatProp::KeyValue(kv) => match &kv.key {
                ast::PropName::Ident(i) => Some(i.sym.to_string()),
                ast::PropName::Str(s) => Some(s.value.as_str().unwrap_or("").to_string()),
                ast::PropName::Num(n) => Some(n.value.to_string()),
                _ => None,
            },
            ast::ObjectPatProp::Assign(a) => Some(a.key.sym.to_string()),
            ast::ObjectPatProp::Rest(_) => None,
        })
        .collect()
}

/// Returns `Some("fetch")` when `expr` is a bare-`Ident` fetch-like call —
/// `fetch(url)` / `fetchWithAuth(...)` / `fetchPostWithAuth(...)`. The caller
/// registers the binding as a fetch `Response` native instance.
///
/// Closes #644: all three return the same Response handle, so they must
/// register under module="fetch". The codegen dispatch in
/// `lower_fetch_native_method` gates on `module == "fetch"` — pre-fix,
/// registering under "fetchWithAuth"/"fetchPostWithAuth" missed the gate so a
/// post-narrowing `r.status` lowered as a NativeMethodCall with
/// module="fetchWithAuth" and fell through to a generic 0.0-returning arm.
/// (Without narrowing the access went through generic PropertyGet → handle
/// dispatch → js_fetch_response_status, so the bug only surfaced inside an
/// `if r !== null/undefined` block.)
pub(crate) fn get_fetch_module(expr: &ast::Expr) -> Option<&'static str> {
    if let ast::Expr::Call(call_expr) = expr {
        if let ast::Callee::Expr(callee_expr) = &call_expr.callee {
            if let ast::Expr::Ident(ident) = callee_expr.as_ref() {
                return match ident.sym.as_ref() {
                    "fetch" | "fetchWithAuth" | "fetchPostWithAuth" => Some("fetch"),
                    _ => None,
                };
            }
        }
    }
    None
}

/// #5432: true when `expr` is a member-call `.fetch(...)` (optionally awaited) —
/// `app.fetch(req)` / `await app.fetch(req)`, the Fetch-API / WinterCG
/// server-handler convention (Hono, itty-router, Cloudflare Workers) that
/// returns a native fetch `Response`. Used to populate
/// `fetch_call_response_locals` so `res.headers.<m>()` routes through the
/// Headers FFI instead of being folded into `js_array_forEach` on a handle id.
pub(crate) fn is_member_fetch_call(expr: &ast::Expr) -> bool {
    let call = match expr {
        ast::Expr::Await(a) => match a.arg.as_ref() {
            ast::Expr::Call(c) => c,
            _ => return false,
        },
        ast::Expr::Call(c) => c,
        _ => return false,
    };
    if let ast::Callee::Expr(callee) = &call.callee {
        if let ast::Expr::Member(member) = callee.as_ref() {
            if let ast::MemberProp::Ident(prop) = &member.prop {
                return prop.sym.as_ref() == "fetch";
            }
        }
    }
    false
}

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
                || n.args.as_ref().is_some_and(|args| {
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
