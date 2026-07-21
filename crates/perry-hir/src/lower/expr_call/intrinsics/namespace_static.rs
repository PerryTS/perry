use super::*;

use anyhow::Result;
use swc_ecma_ast as ast;

use super::super::super::{is_known_namespace_static_function, LoweringContext};

/// #2143 — namespace-static `.bind`/`.call`/`.apply` immediate-call rewrites.
///
/// Built-in function values like `Promise.resolve`, `Math.min`, `JSON.parse`
/// do not inherit `Function.prototype` in Perry's representation (each direct
/// call site is special-cased in codegen — there's no reified function value
/// to hang `.call`/`.apply`/`.bind` off). The bare value-read lowers to a
/// numeric fallback, so `Promise.resolve.bind(Promise)(x)` throws
/// "value is not a function" at the outer call.
///
/// Rewrite at the AST level for the shapes whose intent is unambiguous and
/// where the `thisArg` is irrelevant (most namespace statics don't read `this`):
///
///   `<NS>.<static>.call(thisArg, a, b, …)`     → `<NS>.<static>(a, b, …)`
///   `<NS>.<static>.apply(thisArg)`             → `<NS>.<static>()`
///   `<NS>.<static>.apply(thisArg, [a, b, …])`  → `<NS>.<static>(a, b, …)`
///   `<NS>.<static>.bind(thisArg, …pre)(…rest)` → `<NS>.<static>(…pre, …rest)`
///
/// Promise statics are handled only when the borrowed-call receiver is the real
/// global `Promise` constructor. ECMA-262 reads their `this` value as the
/// constructor receiver, so `Promise.resolve.call({}, x)` must not become
/// `Promise.resolve(x)`.
///
/// The deferred-bind shape (`const f = Promise.resolve.bind(Promise);
/// f(x);`) cannot be rewritten purely at the AST level — that needs a
/// real reified function value and is tracked as follow-up.
pub(crate) fn try_namespace_static_method_apply_call_bind(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    has_spread: bool,
) -> Result<Option<Expr>> {
    if has_spread {
        return Ok(None);
    }
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return Ok(None);
    };

    // Form A/B: `<NS>.<static>.call(…)` or `.apply(…)`.
    if let ast::Expr::Member(outer) = callee_expr.as_ref() {
        if let ast::MemberProp::Ident(outer_prop) = &outer.prop {
            let mode = match outer_prop.sym.as_ref() {
                "call" => Some(false),
                "apply" => Some(true),
                _ => None,
            };
            if let Some(is_apply) = mode {
                if let Some(inner) = match_promise_static_member(ctx, outer.obj.as_ref()) {
                    if call.args.first().is_some_and(|arg| {
                        expr_is_global_promise_constructor(ctx, arg.expr.as_ref())
                    }) {
                        return rewrite_dropping_this(ctx, call, &inner, is_apply);
                    }
                }
                if let Some(inner) = match_namespace_static_member(ctx, outer.obj.as_ref()) {
                    return rewrite_dropping_this(ctx, call, &inner, is_apply);
                }
            }
        }
    }

    // Form C: `(<NS>.<static>.bind(thisArg, …pre))(…rest)` — the outer call's
    // callee is itself a CallExpr to `.bind`.
    if let ast::Expr::Call(bind_call) = callee_expr.as_ref() {
        if let ast::Callee::Expr(bind_callee) = &bind_call.callee {
            if let ast::Expr::Member(bind_member) = bind_callee.as_ref() {
                if let ast::MemberProp::Ident(bind_prop) = &bind_member.prop {
                    if bind_prop.sym.as_ref() == "bind" {
                        // The bind call itself can't have spreads we don't
                        // understand; require at least `thisArg`.
                        let bind_spread = bind_call.args.iter().any(|a| a.spread.is_some());
                        if !bind_spread && !bind_call.args.is_empty() {
                            if let Some(inner_member) =
                                match_promise_static_member(ctx, bind_member.obj.as_ref())
                            {
                                if expr_is_global_promise_constructor(
                                    ctx,
                                    bind_call.args[0].expr.as_ref(),
                                ) {
                                    // Build: <inner_member>(…preBound, …rest)
                                    let pre_bound: Vec<ast::ExprOrSpread> =
                                        bind_call.args.iter().skip(1).cloned().collect();
                                    let mut synth = call.clone();
                                    synth.callee = ast::Callee::Expr(Box::new(ast::Expr::Member(
                                        inner_member,
                                    )));
                                    let mut combined = pre_bound;
                                    combined.extend(call.args.iter().cloned());
                                    synth.args = combined;
                                    return Ok(Some(super::super::lower_call(ctx, &synth)?));
                                }
                            }
                            if let Some(inner_member) =
                                match_namespace_static_member(ctx, bind_member.obj.as_ref())
                            {
                                // Build: <inner_member>(…preBound, …rest)
                                let pre_bound: Vec<ast::ExprOrSpread> =
                                    bind_call.args.iter().skip(1).cloned().collect();
                                let mut synth = call.clone();
                                synth.callee =
                                    ast::Callee::Expr(Box::new(ast::Expr::Member(inner_member)));
                                let mut combined = pre_bound;
                                combined.extend(call.args.iter().cloned());
                                synth.args = combined;
                                return Ok(Some(super::super::lower_call(ctx, &synth)?));
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(None)
}

fn match_promise_static_member(ctx: &LoweringContext, expr: &ast::Expr) -> Option<ast::MemberExpr> {
    let ast::Expr::Member(m) = expr else {
        return None;
    };
    let ast::MemberProp::Ident(prop) = &m.prop else {
        return None;
    };
    let ast::Expr::Ident(base) = m.obj.as_ref() else {
        return None;
    };
    let ns = base.sym.as_ref();
    let name = prop.sym.as_ref();
    if ns != "Promise" {
        return None;
    }
    if ctx.lookup_local(ns).is_some()
        || ctx.lookup_func(ns).is_some()
        || ctx.lookup_imported_func(ns).is_some()
    {
        return None;
    }
    if !is_known_namespace_static_function(ns, name) {
        return None;
    }
    Some(m.clone())
}

fn expr_is_global_promise_constructor(ctx: &LoweringContext, expr: &ast::Expr) -> bool {
    let mut expr = expr;
    loop {
        expr = match expr {
            ast::Expr::TsAs(x) => x.expr.as_ref(),
            ast::Expr::TsNonNull(x) => x.expr.as_ref(),
            ast::Expr::TsSatisfies(x) => x.expr.as_ref(),
            ast::Expr::TsTypeAssertion(x) => x.expr.as_ref(),
            ast::Expr::TsConstAssertion(x) => x.expr.as_ref(),
            ast::Expr::Paren(x) => x.expr.as_ref(),
            _ => break,
        };
    }
    matches!(expr, ast::Expr::Ident(ident) if ident.sym.as_ref() == "Promise")
        && ctx.lookup_local("Promise").is_none()
        && ctx.lookup_func("Promise").is_none()
        && ctx.lookup_imported_func("Promise").is_none()
}

/// If `expr` is `<NS>.<static>` where `<NS>` is a known namespace-static
/// holder (Math/JSON/Number/String/Object/Array) not shadowed by a
/// local, and `<static>` is a known method on it, return a clone of that
/// MemberExpr so it can be reused as the rewritten callee.
fn match_namespace_static_member(
    ctx: &LoweringContext,
    expr: &ast::Expr,
) -> Option<ast::MemberExpr> {
    let ast::Expr::Member(m) = expr else {
        return None;
    };
    let ast::MemberProp::Ident(prop) = &m.prop else {
        return None;
    };
    let ast::Expr::Ident(base) = m.obj.as_ref() else {
        return None;
    };
    let ns = base.sym.as_ref();
    let name = prop.sym.as_ref();
    if ns == "Promise" {
        return None;
    }
    if ctx.lookup_local(ns).is_some() || ctx.lookup_func(ns).is_some() {
        return None;
    }
    if !is_known_namespace_static_function(ns, name) {
        return None;
    }
    // #4521: the Promise combinators read the `this` constructor
    // (`NewPromiseCapability(this)` / `GetPromiseResolve(this)`), so
    // `Promise.all.call(C, …)` / `.apply` / `.bind` must NOT drop the
    // thisArg — let them fall through to the generic reified-static dispatch
    // (which preserves `this` via the implicit-this mechanism).
    // `resolve` / `reject` are likewise `this`-sensitive: `Promise.{resolve,
    // reject}.call(C, x)` go through `NewPromiseCapability(C)` (a non-ctor /
    // non-object `this` throws; a custom constructor's executor runs), so they
    // must keep their receiver too.
    if ns == "Promise"
        && matches!(
            name,
            "all" | "race" | "allSettled" | "any" | "resolve" | "reject"
        )
    {
        return None;
    }
    // `Array.from` / `Array.of` are `this`-sensitive: per ECMA-262 §23.1.2.1 /
    // §23.1.2.3 each constructs the result via its `this` value when that is a
    // constructor (`Array.from.call(C, items)` / `Array.of.call(C, …)` build an
    // instance of `C`). The `this`-dropping fold below would discard the
    // receiver, so route these through the dynamic dispatch path (the runtime
    // thunks read the implicit `this` and run the full algorithm).
    if ns == "Array" && matches!(name, "from" | "of") {
        return None;
    }
    Some(m.clone())
}

/// Rewrite `<NS>.<static>.{call,apply}(thisArg, …)` to a direct call,
/// dropping the `thisArg` (namespace statics don't use it). For `.apply`,
/// the args array must be a clean literal.
fn rewrite_dropping_this(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    inner: &ast::MemberExpr,
    is_apply: bool,
) -> Result<Option<Expr>> {
    let mut synth = call.clone();
    synth.callee = ast::Callee::Expr(Box::new(ast::Expr::Member(inner.clone())));
    if is_apply {
        // `.apply(thisArg)` / `.apply(thisArg, [a, b, …])`.
        synth.args = match call.args.get(1) {
            None => Vec::new(),
            Some(arr_arg) => match arr_arg.expr.as_ref() {
                ast::Expr::Array(arr) => {
                    let clean = arr
                        .elems
                        .iter()
                        .all(|e| matches!(e, Some(eos) if eos.spread.is_none()));
                    if !clean {
                        return rewrite_dynamic_apply_spread(ctx, call, inner);
                    }
                    arr.elems.iter().filter_map(|e| e.clone()).collect()
                }
                _ => return rewrite_dynamic_apply_spread(ctx, call, inner),
            },
        };
    } else {
        // `.call(thisArg, …args)` — drop thisArg, keep the rest.
        synth.args = call.args.iter().skip(1).cloned().collect();
    }
    Ok(Some(super::super::lower_call(ctx, &synth)?))
}

fn rewrite_dynamic_apply_spread(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    inner: &ast::MemberExpr,
) -> Result<Option<Expr>> {
    if !namespace_static_supports_dynamic_apply_spread(inner) {
        return Ok(None);
    }
    let Some(arg_array) = call.args.get(1) else {
        return Ok(Some(super::super::lower_call(
            ctx,
            &ast::CallExpr {
                callee: ast::Callee::Expr(Box::new(ast::Expr::Member(inner.clone()))),
                args: Vec::new(),
                ..call.clone()
            },
        )?));
    };
    let mut synth = call.clone();
    synth.callee = ast::Callee::Expr(Box::new(ast::Expr::Member(inner.clone())));
    synth.args = vec![ast::ExprOrSpread {
        spread: Some(call.span),
        expr: arg_array.expr.clone(),
    }];
    Ok(Some(super::super::lower_call(ctx, &synth)?))
}

fn namespace_static_supports_dynamic_apply_spread(inner: &ast::MemberExpr) -> bool {
    let ast::Expr::Ident(base) = inner.obj.as_ref() else {
        return false;
    };
    let ast::MemberProp::Ident(prop) = &inner.prop else {
        return false;
    };
    matches!(
        (base.sym.as_ref(), prop.sym.as_ref()),
        ("Math", "min" | "max") | ("String", "fromCharCode")
    )
}
