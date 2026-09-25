//! #10848: direct calls to a built-in member the program replaces.
//!
//! `console.log(…)`, `Math.max(…)`, `JSON.stringify(…)` normally devirtualize
//! to their intrinsic implementation. When the whole-program pre-scan
//! (`crate::patched_builtins`) saw that member being written anywhere, the
//! intrinsic is no longer known to be what the property holds, so the call is
//! lowered as an ordinary dynamic method call: read the property off the live
//! receiver object and invoke it with that receiver as `this`. This runs
//! BEFORE every intrinsic dispatch arm, so none of them can bind the call.

use anyhow::Result;
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::patched_builtins::{is_builtin_namespace, namespace_member_patched, static_member_name};

use super::super::LoweringContext;
use super::unwrap_call_callee_ts_wrappers;

/// An identifier that still names the global built-in (no local, function,
/// class or import binding shadows it).
fn is_unshadowed_global(ctx: &LoweringContext, name: &str) -> bool {
    ctx.lookup_local(name).is_none()
        && ctx.lookup_func(name).is_none()
        && ctx.lookup_class(name).is_none()
        && ctx.lookup_imported_func(name).is_none()
}

/// `N` / `globalThis.N` for an unshadowed builtin namespace `N`.
fn namespace_receiver_name(ctx: &LoweringContext, obj: &ast::Expr) -> Option<String> {
    match unwrap_call_callee_ts_wrappers(obj) {
        ast::Expr::Ident(id) => {
            let name = id.sym.as_ref();
            (is_builtin_namespace(name) && is_unshadowed_global(ctx, name))
                .then(|| name.to_string())
        }
        ast::Expr::Member(m) => {
            let ast::Expr::Ident(root) = unwrap_call_callee_ts_wrappers(&m.obj) else {
                return None;
            };
            let root = root.sym.as_ref();
            if !matches!(root, "globalThis" | "global") || !is_unshadowed_global(ctx, root) {
                return None;
            }
            static_member_name(&m.prop).filter(|n| is_builtin_namespace(n))
        }
        _ => None,
    }
}

pub(super) fn try_patched_builtin_call(
    ctx: &LoweringContext,
    call: &ast::CallExpr,
    args: Vec<Expr>,
) -> Result<Result<Expr, Vec<Expr>>> {
    let ast::Callee::Expr(callee) = &call.callee else {
        return Ok(Err(args));
    };
    let ast::Expr::Member(member) = unwrap_call_callee_ts_wrappers(callee) else {
        return Ok(Err(args));
    };
    let Some(method) = static_member_name(&member.prop) else {
        return Ok(Err(args));
    };

    let Some(ns) = namespace_receiver_name(ctx, &member.obj) else {
        return Ok(Err(args));
    };
    if !namespace_member_patched(&ns, &method) {
        return Ok(Err(args));
    }
    // The live namespace object off the global object — the same object the
    // patching write stored into (and the replacement object after
    // `globalThis.console = {…}`).
    let object = Expr::PropertyGet {
        byte_offset: 0,
        object: Box::new(Expr::GlobalGet(0)),
        property: ns,
    };

    let callee = Box::new(Expr::PropertyGet {
        byte_offset: call.span.lo.0,
        object: Box::new(object),
        property: method,
    });
    if call.args.iter().any(|a| a.spread.is_some()) {
        let args = call
            .args
            .iter()
            .zip(args)
            .map(|(ast_arg, lowered)| {
                if ast_arg.spread.is_some() {
                    CallArg::Spread(lowered)
                } else {
                    CallArg::Expr(lowered)
                }
            })
            .collect();
        return Ok(Ok(Expr::CallSpread {
            callee,
            args,
            type_args: Vec::new(),
        }));
    }
    Ok(Ok(Expr::Call {
        callee,
        args,
        type_args: Vec::new(),
        byte_offset: call.span.lo.0,
    }))
}
