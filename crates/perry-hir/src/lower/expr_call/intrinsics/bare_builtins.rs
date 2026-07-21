use super::*;

use anyhow::Result;
use swc_ecma_ast as ast;

use super::super::super::{lower_expr, LoweringContext};

/// Followup to #957 / PR #959 — `Function('return this')()`.
///
/// Every CJS/UMD-shaped library (lodash, underscore, Effect, …)
/// computes its "give me whatever the host calls `globalThis` here"
/// root with the double-call idiom:
///   var root = freeGlobal || freeSelf || Function('return this')();
/// Pre-fix the bare `Function` ident lowers to `Expr::GlobalGet(0)`
/// (the no-resolution sentinel), then the inner `Function('return this')`
/// lowers to `Call { callee: GlobalGet(0), args: [String("return this")] }`
/// which codegen treats as "call a non-callable" — the outer `()` then
/// tries to call the returned value and the closure validator throws
/// `TypeError: value is not a function` at module init, leaving the
/// import resolved to undefined.
///
/// PR #959 closed the sibling `.call(this)` IIFE bug and called this
/// one out in its commit message ("the next runtime gap"); fix here.
/// Match the full two-call shape at the AST level (the inner `Function`
/// ident still carries its name, so we can verify it really is the
/// builtin) and fold to `Expr::GlobalThisExpr`, which lowers to the
/// runtime's `js_get_global_this()` singleton — the same object
/// `globalThis[X] = V` already writes to (see #611).
///
/// Conservative: requires the LITERAL "return this" (with optional
/// semicolon / whitespace) AND the outer Call must have no args. Any
/// other `Function(...)` shape (e.g. dynamic body, real `new Function`)
/// falls through to the existing GlobalGet(0) path; arbitrary
/// `new Function(body)` is still not supported (an architectural
/// change — issue #960 / future work).
pub(crate) fn try_function_return_this(
    ctx: &LoweringContext,
    call: &ast::CallExpr,
    has_spread: bool,
) -> Option<Expr> {
    if !has_spread && call.args.is_empty() {
        if let ast::Callee::Expr(outer_callee) = &call.callee {
            let mut inner = outer_callee.as_ref();
            while let ast::Expr::Paren(p) = inner {
                inner = p.expr.as_ref();
            }
            if let ast::Expr::Call(inner_call) = inner {
                let inner_args_ok =
                    inner_call.args.len() == 1 && inner_call.args[0].spread.is_none();
                if inner_args_ok {
                    if let ast::Callee::Expr(inner_callee) = &inner_call.callee {
                        let mut inner_target = inner_callee.as_ref();
                        while let ast::Expr::Paren(p) = inner_target {
                            inner_target = p.expr.as_ref();
                        }
                        if let ast::Expr::Ident(ident) = inner_target {
                            if ident.sym.as_ref() == "Function"
                                && ctx.lookup_local("Function").is_none()
                                && ctx.lookup_func("Function").is_none()
                            {
                                if let ast::Expr::Lit(ast::Lit::Str(s)) =
                                    inner_call.args[0].expr.as_ref()
                                {
                                    let body = s.value.as_str().unwrap_or("").trim();
                                    let body = body.trim_end_matches(';').trim();
                                    if body == "return this" {
                                        return Some(Expr::GlobalThisExpr);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// Followup to #957 / PR #959 — `RegExp(<args>)` as a bare function call.
///
/// lodash 4 builds half a dozen of these at module init:
///   var reEscapedHtml = /&(?:amp|lt|gt|quot|#39);/g,
///       reHasEscapedHtml = RegExp(reEscapedHtml.source);
/// The bare `RegExp` ident lowers to `Expr::GlobalGet(0)` (no resolved
/// value), so the function-call form dispatches through
/// `js_closure_call1` with a null closure handle and throws
/// `TypeError: value is not a function`. Fold here to
/// `Expr::RegExpDynamic` which lowers to the same `js_regexp_new`
/// runtime entrypoint the static `/foo/g` arm uses.
///
/// Conservative: only `RegExp(pattern)` and `RegExp(pattern, flags)`
/// with no spread. Any local/import named `RegExp` shadows the
/// builtin and falls through to its normal dispatch.
pub(crate) fn try_bare_regexp_call(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    has_spread: bool,
) -> Result<Option<Expr>> {
    if !has_spread && call.args.len() <= 2 {
        if let ast::Callee::Expr(callee_expr) = &call.callee {
            let mut callee_inner = callee_expr.as_ref();
            while let ast::Expr::Paren(p) = callee_inner {
                callee_inner = p.expr.as_ref();
            }
            if let ast::Expr::Ident(ident) = callee_inner {
                if ident.sym.as_ref() == "RegExp"
                    && ctx.lookup_local("RegExp").is_none()
                    && ctx.lookup_func("RegExp").is_none()
                {
                    // Zero-arg `RegExp()` is `RegExp(undefined)` → an empty
                    // source `/(?:)/`. Without an explicit `Expr::Undefined`
                    // pattern the call fell through to the bare-ident dispatch
                    // and produced `null` (test262 S15.10.3.1_A1_T4, #5586).
                    let pattern = if call.args.is_empty() {
                        Expr::Undefined
                    } else {
                        lower_expr(ctx, &call.args[0].expr)?
                    };
                    let flags = if call.args.len() == 2 {
                        Some(Box::new(lower_expr(ctx, &call.args[1].expr)?))
                    } else {
                        None
                    };
                    return Ok(Some(Expr::RegExpDynamic {
                        pattern: Box::new(pattern),
                        flags,
                        // Function-call form `RegExp(x)`: eligible for the
                        // ECMA-262 22.2.4.1 identity shortcut (#5586).
                        is_call: true,
                    }));
                }
            }
        }
    }
    Ok(None)
}

/// #2874: `Iterator.from(x)` — wrap an iterable in a lazy iterator-helper
/// object. Only fires when `Iterator` is the global (not a local/func/import).
/// The produced helper's `.map`/`.filter`/`.take`/etc. dispatch at runtime via
/// `js_native_call_method`, so no further HIR variants are needed.
pub(crate) fn try_iterator_from(
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
    let mut callee = callee_expr.as_ref();
    while let ast::Expr::Paren(p) = callee {
        callee = p.expr.as_ref();
    }
    let ast::Expr::Member(member) = callee else {
        return Ok(None);
    };
    let ast::MemberProp::Ident(prop) = &member.prop else {
        return Ok(None);
    };
    if prop.sym.as_ref() != "from" {
        return Ok(None);
    }
    let mut obj = member.obj.as_ref();
    while let ast::Expr::Paren(p) = obj {
        obj = p.expr.as_ref();
    }
    let ast::Expr::Ident(obj_ident) = obj else {
        return Ok(None);
    };
    if obj_ident.sym.as_ref() != "Iterator"
        || ctx.lookup_local("Iterator").is_some()
        || ctx.lookup_func("Iterator").is_some()
    {
        return Ok(None);
    }
    let arg = if call.args.is_empty() {
        Expr::Undefined
    } else {
        lower_expr(ctx, &call.args[0].expr)?
    };
    Ok(Some(Expr::IteratorFrom(Box::new(arg))))
}
