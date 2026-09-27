//! #11394: method calls whose name the program writes onto a builtin
//! prototype (`Array.prototype.push = f; arr.push(x)`).
//!
//! A proven array/Map receiver folds `arr.push(x)` / `m.get(k)` to an
//! intrinsic (`Expr::ArrayPush`, `Expr::MapGet`) that never reads the
//! prototype slot. When the whole-program pre-scan (`crate::patched_builtins`)
//! saw that method name written onto `Array`/`Map`/`Set`/`Function.prototype`
//! anywhere, the call is instead lowered as a plain property-get-then-call.
//! Codegen routes that shape, for a patched name, to the runtime entry that
//! looks the method up before dispatching (`js_native_call_method_patched_proto`).
//! This runs BEFORE every receiver-specific arm, so none of them can fold it.
//!
//! Receivers with their own dedicated lowering keep it: `this`/`super`, a
//! global namespace (`Math`, `JSON`, `console` — #10848's own concern), a
//! class (a static method), a module namespace import, and a native module or
//! native instance (their methods are not on a builtin prototype at all).

use anyhow::Result;
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::patched_builtins::{prototype_method_patched, static_member_name};

use super::super::{lower_expr, LoweringContext};
use super::patched_builtin_call::dynamic_member_call;
use super::unwrap_call_callee_ts_wrappers;

/// An identifier receiver that is an ordinary value: a local binding that is
/// not a native instance, a function declaration (`g.bind(t)`), or a value
/// imported from another compiled module (not its namespace object).
fn is_plain_value_ident(ctx: &LoweringContext, name: &str) -> bool {
    if ctx.lookup_local(name).is_some() {
        return ctx.lookup_native_instance(name).is_none()
            && ctx.lookup_native_module(name).is_none();
    }
    if ctx.lookup_class(name).is_some() || ctx.lookup_native_module(name).is_some() {
        return false;
    }
    if ctx.lookup_imported_func(name).is_some() {
        return !ctx.namespace_import_locals.contains(name)
            && ctx.lookup_builtin_named_import(name).is_none();
    }
    ctx.lookup_func(name).is_some()
}

/// May a call on this receiver land on a builtin prototype method? See the
/// module docs for the receivers that keep their own lowering.
fn receiver_is_eligible(ctx: &LoweringContext, obj: &ast::Expr) -> bool {
    match unwrap_call_callee_ts_wrappers(obj) {
        ast::Expr::Ident(id) => is_plain_value_ident(ctx, id.sym.as_ref()),
        ast::Expr::This(_) | ast::Expr::SuperProp(_) => false,
        // `a.b.push(x)`: eligible when the chain is rooted at an ordinary
        // value, so `this.items.push(x)` is, and `process.argv.push(x)` or
        // `fs.promises.get(…)` keep their dedicated lowering.
        ast::Expr::Member(m) => match unwrap_call_callee_ts_wrappers(&m.obj) {
            ast::Expr::This(_) => true,
            ast::Expr::Ident(id) => is_plain_value_ident(ctx, id.sym.as_ref()),
            inner @ ast::Expr::Member(_) => receiver_is_eligible(ctx, inner),
            _ => true,
        },
        _ => true,
    }
}

pub(super) fn try_patched_proto_call(
    ctx: &mut LoweringContext,
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
    if !prototype_method_patched(&method) || !receiver_is_eligible(ctx, &member.obj) {
        return Ok(Err(args));
    }
    let object = lower_expr(ctx, &member.obj)?;
    Ok(Ok(dynamic_member_call(call, object, method, args)))
}
