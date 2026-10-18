//! Post-args dispatch hooks: proxy apply/revoke, `Object.<static>`
//! aliased calls, and the `Object.prototype.<method>.call(...)` /
//! `Object.hasOwnProperty.call(...)` shape rewrites.
//!
//! These run AFTER `args` has been lowered but BEFORE the big
//! `match &call.callee { ... }` dispatch. Extracted from
//! `expr_call/mod.rs` as a mechanical move.

use swc_ecma_ast as ast;

use crate::ir::*;

use super::super::LoweringContext;
use super::object_static::build_object_static_method_call;

/// Proxy apply / revoke fast path. If the bare callee ident is a
/// proxy-typed local, route directly to `ProxyApply`/`ProxyRevoke`.
pub(super) fn try_proxy_call(
    ctx: &LoweringContext,
    call: &ast::CallExpr,
    args: Vec<Expr>,
    has_spread: bool,
) -> Result<Expr, Vec<Expr>> {
    if !has_spread {
        if let ast::Callee::Expr(callee_expr) = &call.callee {
            if let ast::Expr::Ident(ident) = callee_expr.as_ref() {
                let name = ident.sym.to_string();
                if ctx.is_proxy_local(&name) {
                    if let Some(id) = ctx.lookup_local(&name) {
                        return Ok(Expr::ProxyApply {
                            proxy: Box::new(Expr::LocalGet(id)),
                            args,
                        });
                    }
                }
                if let Some(proxy_name) = ctx.proxy_revoke_locals.get(&name).cloned() {
                    if let Some(id) = ctx.lookup_local(&proxy_name) {
                        return Ok(Expr::ProxyRevoke(Box::new(Expr::LocalGet(id))));
                    }
                }
            }
        }
    }
    Err(args)
}

/// Issue #886: indirect call through an `Object.<staticMethod>` alias.
///
/// esbuild's CJS-bundle prelude emits a constant-aliased dispatch table
/// at the top of every bundled package:
///   var __defProp = Object.defineProperty;
///   var __export = (target, all) => {
///     for (var name in all) __defProp(target, name, { get: all[name], enumerable: true });
///   };
/// Pre-fix the `__defProp(...)` call fell through to the generic
/// `LocalGet(__defProp)(args)` codegen path which evaluates the LocalGet
/// to whatever the init lowering produced — a PropertyGet on the
/// undefined-at-runtime `Object` constructor reference — and then tries
/// to invoke an undefined value, throwing `TypeError: value is not a
/// function`. The recogniser below for the literal `Object.defineProperty
/// (...)` shape (added by #891) didn't fire because the callee isn't a
/// member expression, just a local ident.
///
/// The alias is populated in `destructuring.rs` when the AST shape is
/// `const X = Object.<method>` and `<method>` is in the whitelist of
/// methods that already have a dedicated HIR variant. Route here to
/// synthesize the same Expr the literal recogniser would produce.
pub(super) fn try_object_static_alias_call(
    ctx: &LoweringContext,
    call: &ast::CallExpr,
    args: Vec<Expr>,
    has_spread: bool,
) -> Result<Expr, Vec<Expr>> {
    if !has_spread {
        if let ast::Callee::Expr(callee_expr) = &call.callee {
            if let ast::Expr::Ident(ident) = callee_expr.as_ref() {
                let name = ident.sym.to_string();
                if let Some(id) = ctx.lookup_local(&name) {
                    if let Some(method) = ctx.object_static_method_aliases.get(&id).cloned() {
                        if let Some(method) = method.strip_prefix("Response.") {
                            return Ok(Expr::NativeMethodCall {
                                module: "fetch".to_string(),
                                class_name: None,
                                object: None,
                                method: method.to_string(),
                                args,
                            });
                        }
                        if method == "Array.isArray" {
                            let value = args.first().cloned().unwrap_or(Expr::Undefined);
                            return Ok(Expr::ArrayIsArray(Box::new(value)));
                        }
                        return Ok(build_object_static_method_call(&method, args));
                    }
                }
            }
        }
    }
    Err(args)
}

/// Indirect call through a captured `Array.<staticMethod>` alias.
///
/// Test262's property helper captures `Array.isArray` into `__isArray` and
/// calls that local later. Direct `Array.isArray(x)` lowers through the
/// dedicated intrinsic already; this mirrors the `Object.<static>` alias
/// repair above for the captured form.
pub(super) fn try_array_static_alias_call(
    ctx: &LoweringContext,
    call: &ast::CallExpr,
    args: Vec<Expr>,
    has_spread: bool,
) -> Result<Expr, Vec<Expr>> {
    if !has_spread {
        if let ast::Callee::Expr(callee_expr) = &call.callee {
            if let ast::Expr::Ident(ident) = callee_expr.as_ref() {
                let name = ident.sym.to_string();
                if let Some(id) = ctx.lookup_local(&name) {
                    if matches!(
                        ctx.array_static_method_aliases.get(&id).map(String::as_str),
                        Some("isArray")
                    ) {
                        let value = args.first().cloned().unwrap_or(Expr::Undefined);
                        return Ok(Expr::ArrayIsArray(Box::new(value)));
                    }
                }
            }
        }
    }
    Err(args)
}

// Object borrowed helpers used to discard the source function. Their late
// prototype check cannot cover an own .call override or preserve a callee
// selected before argument side effects; retain the original reflective call.

#[cfg(test)]
mod tests {
    use crate::Module;
    use perry_diagnostics::SourceCache;

    fn lower(src: &str) -> Module {
        let src = src.to_string();
        std::thread::Builder::new()
            .stack_size(32 * 1024 * 1024)
            .spawn(move || {
                let mut cache = SourceCache::new();
                let parsed = perry_parser::parse_typescript_with_cache(
                    &src,
                    "object_proto_hasown.ts",
                    &mut cache,
                )
                .expect("parse should succeed");
                crate::lower_module(&parsed.module, "test", "object_proto_hasown.ts")
                    .expect("lower should succeed")
            })
            .expect("spawn lower thread")
            .join()
            .expect("lower thread panicked")
    }

    #[test]
    fn object_prototype_hasownproperty_call_keeps_dynamic_method_lookup() {
        let hir = format!(
            "{:?}",
            lower(
                r#"
                Object.prototype.hasOwnProperty.call({ x: 1 }, "x");
                "#,
            )
        );

        assert!(
            !hir.contains("js_object_has_own"),
            "hasOwnProperty.call must not bypass a patched prototype method: {hir}"
        );
        assert!(
            hir.contains("PropertyGet"),
            "the call should retain dynamic property lookup: {hir}"
        );
    }
}
