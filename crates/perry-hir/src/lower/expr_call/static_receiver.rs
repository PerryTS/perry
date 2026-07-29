//! `static_receiver_class` — receiver-class classification for ambiguous
//! method-call arms (Date/URL/Object).
//!
//! Extracted from `expr_call/mod.rs` in #1104 as a pure mechanical move;
//! the function's only consumer is `lower_call_inner` inside this module.

use crate::types::Type;
use swc_ecma_ast as ast;

use super::super::LoweringContext;

/// Issue #650: classify the static type of a method-call receiver well enough
/// to decide whether the ambiguous Date method arms (`toJSON`, `toString`,
/// `toLocaleString`, `valueOf`, `to{Date,Time,LocaleDate,LocaleTime}String`)
/// should fire. Returns `Some("Date")` for `new Date(...)` and locals typed
/// as `Date`; `Some("URL")` for `new URL(...)` and locals typed as `URL`;
/// `Some("Object")` for source-visible class/object receivers that are
/// definitely not Date; `None` for everything else. Matches receiver shapes by
/// AST first, then by the caller's `local_types` table — both source-level
/// shapes the user typically writes for these objects.
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
        // Issue #5912 (CodeRabbit follow-up): `new globalThis.URL(...)`
        // always reaches the REAL global regardless of any local `URL`
        // shadowing — that's the entire point of the explicit `globalThis.`
        // qualifier. Track which callee shape matched instead of collapsing
        // both into one `class_name` capture; the original version fed both
        // forms into the same shadow check below, incorrectly downgrading
        // `new globalThis.URL()` to "Object" whenever a local `URL` shadowed
        // the bare name.
        let (class_name, is_global_qualified) = match new_expr.callee.as_ref() {
            ast::Expr::Ident(ident) => (Some(ident.sym.as_ref()), false),
            ast::Expr::Member(member)
                if matches!(member.obj.as_ref(), ast::Expr::Ident(obj) if obj.sym.as_ref() == "globalThis")
                    && ctx.lookup_local("globalThis").is_none() =>
            {
                match &member.prop {
                    ast::MemberProp::Ident(prop) => (Some(prop.sym.as_ref()), true),
                    _ => (None, false),
                }
            }
            _ => (None, false),
        };
        if let Some(class_name) = class_name {
            // Issue #5912: a local function/const/class/imported-binding
            // (`shadows_unqualified_global` covers all four — see #5912
            // review follow-up) shadowing one of the well-known names below
            // (e.g. a vendored `function URL(url) { ... }` polyfill) is the
            // user's own value, never perry's native built-in — classify as
            // a generic "Object" (skips the ambiguous Date/URL method arms,
            // same treatment as an object-literal receiver below) instead of
            // misrouting `.toString()`/`.toJSON()` through the native fast
            // paths (`UrlInstanceToJSON` etc.) on a value that was never
            // actually constructed via the native path. Doesn't apply to the
            // `globalThis.X` form above — see the comment on
            // `is_global_qualified`.
            if !is_global_qualified && ctx.shadows_unqualified_global(class_name) {
                return Some("Object");
            }
            let resolved_class = ctx
                .resolve_class_alias(class_name)
                .unwrap_or_else(|| class_name.to_string());
            return match resolved_class.as_str() {
                "Date" => Some("Date"),
                "URL" => Some("URL"),
                "URLPattern" => Some("URLPattern"),
                "Buffer" => Some("Buffer"),
                "BlockList" => Some("BlockList"),
                "SocketAddress" => Some("SocketAddress"),
                "Uint8Array" => Some("Uint8Array"),
                "Uint8ClampedArray" => Some("Uint8ClampedArray"),
                _ if ctx.lookup_class(&resolved_class).is_some() => Some("Object"),
                _ => None,
            };
        }
        if let ast::Expr::Member(member) = new_expr.callee.as_ref() {
            if let ast::MemberProp::Ident(prop) = &member.prop {
                let module_name = match member.obj.as_ref() {
                    ast::Expr::Ident(obj) => ctx.lookup_builtin_module_alias(obj.sym.as_ref()),
                    _ => None,
                };
                if module_name == Some("net")
                    && matches!(prop.sym.as_ref(), "BlockList" | "SocketAddress")
                {
                    return match prop.sym.as_ref() {
                        "BlockList" => Some("BlockList"),
                        _ => Some("SocketAddress"),
                    };
                }
            }
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
            let named = match ty {
                Type::Named(n) => Some(n.as_str()),
                Type::Generic { base, .. } => Some(base.as_str()),
                _ => None,
            };
            if let Some(n) = named {
                // Issue #5912: the local's inferred type name can legitimately
                // be "URL" because it holds an instance of a REAL user class
                // named `URL` (shadowing the global) — not perry's native
                // WHATWG URL. Route those through generic dispatch too, same
                // as the `New`-expression branch above.
                if ctx.lookup_class(n).is_some() {
                    return Some("Object");
                }
                return match n {
                    "Date" => Some("Date"),
                    "URL" => Some("URL"),
                    "URLPattern" => Some("URLPattern"),
                    "Buffer" => Some("Buffer"),
                    "BlockList" => Some("BlockList"),
                    "SocketAddress" => Some("SocketAddress"),
                    "Uint8Array" => Some("Uint8Array"),
                    "Uint8ClampedArray" => Some("Uint8ClampedArray"),
                    _ => Some("Object"),
                };
            }
        }
    }
    None
}
