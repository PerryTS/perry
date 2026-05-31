//! `static_receiver_class` — receiver-class classification for ambiguous
//! method-call arms (Date/URL/Object).
//!
//! Extracted from `expr_call/mod.rs` in #1104 as a pure mechanical move;
//! the function's only consumer is `lower_call_inner` inside this module.

use perry_types::Type;
use swc_ecma_ast as ast;

use super::super::LoweringContext;

/// Issue #650: classify the static type of a method-call receiver well enough
/// to decide whether the ambiguous Date method arms (`toJSON`, `toString`,
/// `toLocaleString`, `valueOf`, `to{Date,Time,LocaleDate,LocaleTime}String`)
/// should fire. Returns `Some("Date")` for `new Date(...)` and locals typed
/// as `Date`; `Some("URL")` for `new URL(...)` and locals typed as `URL`;
/// `None` for everything else (in which case the call falls through to
/// generic dispatch). Matches receiver shapes by AST first, then by the
/// caller's `local_types` table — both source-level shapes the user typically
/// writes for these objects.
pub(super) fn static_receiver_class(
    ctx: &LoweringContext,
    obj: &ast::Expr,
) -> Option<&'static str> {
    // #2808: an array receiver is definitely NOT a Date. Detect array literals,
    // `as` / const-assertion casts wrapping any of the above, and locals typed
    // as `T[]` / tuple, so `(arr as any).toLocaleString()` (and toString/etc.)
    // skip the ambiguous Date arms and fall through to generic dynamic
    // dispatch, where the runtime Array.prototype.toLocaleString lives.
    {
        // Peel `as`/`as const`/parens to inspect the underlying receiver shape.
        let mut cur = obj;
        loop {
            match cur {
                ast::Expr::TsAs(ts_as) => cur = ts_as.expr.as_ref(),
                ast::Expr::TsConstAssertion(c) => cur = c.expr.as_ref(),
                ast::Expr::Paren(p) => cur = p.expr.as_ref(),
                _ => break,
            }
        }
        if matches!(cur, ast::Expr::Array(_)) {
            return Some("Array");
        }
        if let ast::Expr::Ident(ident) = cur {
            if let Some(ty) = ctx.lookup_local_type(ident.sym.as_ref()) {
                if matches!(ty, Type::Array(_) | Type::Tuple(_)) {
                    return Some("Array");
                }
            }
        }
    }
    if let ast::Expr::New(new_expr) = obj {
        if let ast::Expr::Ident(ident) = new_expr.callee.as_ref() {
            return match ident.sym.as_ref() {
                "Date" => Some("Date"),
                "URL" => Some("URL"),
                "Buffer" => Some("Buffer"),
                "Uint8Array" => Some("Uint8Array"),
                "Uint8ClampedArray" => Some("Uint8ClampedArray"),
                _ => None,
            };
        }
    }
    // #809: an object literal receiver, or `Object.create(...)`, is
    // provably a plain object — never a Date. Returning `Some("Object")`
    // makes the ambiguous-Date-method gate skip the Date arms for
    // `({...}).toJSON()` / `Object.create(p).toJSON()` the same way it
    // does for URL, so the call falls through to generic dynamic dispatch
    // and finds the object's own method.
    if matches!(obj, ast::Expr::Object(_)) {
        return Some("Object");
    }
    if let ast::Expr::Call(call) = obj {
        if let ast::Callee::Expr(callee) = &call.callee {
            if let ast::Expr::Member(m) = callee.as_ref() {
                if matches!(m.obj.as_ref(), ast::Expr::Ident(o) if o.sym.as_ref() == "Object")
                    && matches!(&m.prop, ast::MemberProp::Ident(p) if p.sym.as_ref() == "create")
                {
                    return Some("Object");
                }
                // #1387: `performance.mark(...).toJSON()` /
                // `performance.measure(...).toJSON()` — the entry is a plain
                // shaped object, not a Date. Classify as "Object" so the
                // ambiguous-Date arms are skipped and the call reaches the
                // synthesized PerformanceEntry#toJSON.
                if matches!(m.obj.as_ref(), ast::Expr::Ident(o) if o.sym.as_ref() == "performance")
                    && matches!(&m.prop, ast::MemberProp::Ident(p) if p.sym.as_ref() == "mark" || p.sym.as_ref() == "measure")
                {
                    return Some("Object");
                }
            }
        }
    }
    if let ast::Expr::Ident(ident) = obj {
        let name = ident.sym.as_ref();
        if ctx.plain_object_locals.contains(name) {
            return Some("Object");
        }
        if let Some(ty) = ctx.lookup_local_type(name) {
            if let Type::Named(n) = ty {
                return match n.as_str() {
                    "Date" => Some("Date"),
                    "URL" => Some("URL"),
                    "Buffer" => Some("Buffer"),
                    "Uint8Array" => Some("Uint8Array"),
                    "Uint8ClampedArray" => Some("Uint8ClampedArray"),
                    _ => None,
                };
            }
        }
    }
    None
}
