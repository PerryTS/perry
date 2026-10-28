//! #10848: direct calls to a built-in member the program replaces.
//!
//! `console.log(…)`, `Math.max(…)`, `JSON.stringify(…)` normally devirtualize
//! to their intrinsic implementation. When the whole-program pre-scan
//! (`crate::patched_builtins`) saw that member being written anywhere, the
//! intrinsic is no longer known to be what the property holds, so the call is
//! lowered as an ordinary dynamic method call: read the property off the live
//! receiver object and invoke it with that receiver as `this`. This runs
//! BEFORE every intrinsic dispatch arm, so none of them can bind the call.
//!
//! #12643: when only dynamic code (a non-constant `eval`/`new Function`
//! reaching the global scope) may replace it, the call is lowered as usual
//! and guarded: `NamespacePatched(ns) ? <the dynamic call> : <the intrinsic>`.
//! The test reads the shadow bit of the namespace's member holder, which the
//! runtime sets on any write to the holder and on any change of the
//! `globalThis[ns]` binding, before the callee would have been read; each arm
//! then evaluates the arguments once, in order.

use anyhow::Result;
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::patched_builtins::{
    is_builtin_namespace, namespace_call_guarded, namespace_member_patched, static_member_name,
};

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

/// What [`try_patched_builtin_call`] decided.
pub(super) enum NamespaceCall {
    /// Not a builtin namespace call, or nothing can replace its member.
    Plain(Vec<Expr>),
    /// The member is replaced by program code: the dynamic call.
    Dynamic(Expr),
    /// Dynamic code may replace the member or the binding: lower the call as
    /// usual, then [`NamespaceGuard::wrap`] it.
    Guarded(Vec<Expr>, NamespaceGuard),
}

/// The generic arm of a guarded namespace call.
pub(super) struct NamespaceGuard {
    ns: String,
    generic: Expr,
}

impl NamespaceGuard {
    /// `NamespacePatched(ns) ? generic : fast`.
    pub(super) fn wrap(self, fast: Expr) -> Expr {
        Expr::Conditional {
            condition: Box::new(Expr::NamespacePatched(self.ns)),
            then_expr: Box::new(self.generic),
            else_expr: Box::new(fast),
        }
    }
}

/// The live namespace object off the global object — the same object a
/// patching write stores into (and the replacement after
/// `globalThis.console = {…}`). Read through the real global object, not the
/// `GlobalGet(0)` sentinel: codegen recognizes `GlobalGet(0).Promise.resolve(…)`
/// as the intrinsic again and would bind the call straight back to it.
fn live_namespace(ns: &str) -> Expr {
    Expr::PropertyGet {
        byte_end: 0,
        byte_offset: 0,
        object: Box::new(Expr::GlobalThisExpr),
        property: ns.to_string(),
    }
}

/// Can the lowered arguments appear in both arms of the guard? Each arm runs
/// alone, so nothing is evaluated twice; but a definition (a closure or class
/// body, with its function id) or a suspension point must exist once.
fn duplicable(args: &[Expr]) -> bool {
    fn ok(e: &Expr) -> bool {
        if matches!(
            e,
            Expr::Closure { .. }
                | Expr::ClassExprFresh { .. }
                | Expr::ClassIsFirstEvaluation { .. }
                | Expr::RegisterClassParentDynamic { .. }
                | Expr::RegisterClassCaptures { .. }
                | Expr::RefreshClassExprCaptures { .. }
                | Expr::RegisterClassStaticSymbol { .. }
                | Expr::RegisterClassComputedMethod { .. }
                | Expr::RegisterClassComputedAccessor { .. }
                | Expr::ClassEnvStamp { .. }
                | Expr::ClassStaticSymbolSet { .. }
                | Expr::ScopedTemp { .. }
                | Expr::Await(..)
                | Expr::Yield { .. }
        ) {
            return false;
        }
        let mut all = true;
        crate::walker::walk_expr_children(e, &mut |c| all &= ok(c));
        all
    }
    args.iter().all(ok)
}

pub(super) fn try_patched_builtin_call(
    ctx: &LoweringContext,
    call: &ast::CallExpr,
    args: Vec<Expr>,
) -> Result<NamespaceCall> {
    let ast::Callee::Expr(callee) = &call.callee else {
        return Ok(NamespaceCall::Plain(args));
    };
    let ast::Expr::Member(member) = unwrap_call_callee_ts_wrappers(callee) else {
        return Ok(NamespaceCall::Plain(args));
    };
    let Some(method) = static_member_name(&member.prop) else {
        return Ok(NamespaceCall::Plain(args));
    };
    let Some(ns) = namespace_receiver_name(ctx, &member.obj) else {
        return Ok(NamespaceCall::Plain(args));
    };
    if namespace_member_patched(&ns, &method) {
        let object = live_namespace(&ns);
        return Ok(NamespaceCall::Dynamic(dynamic_member_call(
            call, object, method, args,
        )));
    }
    if !namespace_call_guarded() {
        return Ok(NamespaceCall::Plain(args));
    }
    if !duplicable(&args) {
        // A definition in the arguments: the one dynamic call.
        let object = live_namespace(&ns);
        return Ok(NamespaceCall::Dynamic(dynamic_member_call(
            call, object, method, args,
        )));
    }
    let generic = dynamic_member_call(call, live_namespace(&ns), method, args.clone());
    Ok(NamespaceCall::Guarded(args, NamespaceGuard { ns, generic }))
}

/// `object.method(args…)` as an ordinary property-get-then-call: the callee is
/// read off the live receiver and invoked with it as `this`, carrying `call`'s
/// spread arguments.
pub(super) fn dynamic_member_call(
    call: &ast::CallExpr,
    object: Expr,
    method: String,
    args: Vec<Expr>,
) -> Expr {
    let callee = Box::new(Expr::PropertyGet {
        byte_end: 0,
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
        return Expr::CallSpread {
            byte_offset: 0,
            byte_end: 0,
            callee,
            args,
            type_args: Vec::new(),
        };
    }
    Expr::Call {
        byte_end: 0,
        callee,
        args,
        type_args: Vec::new(),
        byte_offset: call.span.lo.0,
    }
}
