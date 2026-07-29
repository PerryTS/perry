//! Post-args dispatch hooks: proxy apply/revoke, `Object.<static>`
//! aliased calls, and the `Object.prototype.<method>.call(...)` /
//! `Object.hasOwnProperty.call(...)` shape rewrites.
//!
//! These run AFTER `args` has been lowered but BEFORE the big
//! `match &call.callee { ... }` dispatch. Extracted from
//! `expr_call/mod.rs` as a mechanical move.

use crate::types::Type;
use swc_ecma_ast as ast;

use crate::ir::*;

use super::super::{lower_expr, LoweringContext};
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
                if ctx.proxy_locals.contains(&name) {
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

/// `Object.hasOwnProperty.call(obj, key)` → `js_object_has_own(obj, key)`.
///
/// Current NestJS `@Module()` uses this inherited Object.prototype helper
/// through the `Object` constructor value instead of spelling
/// `Object.prototype.hasOwnProperty.call(...)`.
pub(super) fn try_object_has_own_call(
    call: &ast::CallExpr,
    args: Vec<Expr>,
    has_spread: bool,
) -> Result<Expr, Vec<Expr>> {
    if !has_spread && args.len() == 2 {
        if let ast::Callee::Expr(callee_expr) = &call.callee {
            if let ast::Expr::Member(outer) = callee_expr.as_ref() {
                if let (ast::MemberProp::Ident(outer_prop), ast::Expr::Member(mid)) =
                    (&outer.prop, outer.obj.as_ref())
                {
                    if outer_prop.sym.as_ref() == "call" {
                        if let (ast::MemberProp::Ident(mid_prop), ast::Expr::Ident(mid_obj)) =
                            (&mid.prop, mid.obj.as_ref())
                        {
                            if mid_obj.sym.as_ref() == "Object" {
                                let runtime_fn = match mid_prop.sym.as_ref() {
                                    "hasOwnProperty" => Some("js_object_has_own"),
                                    // #2891: Object.propertyIsEnumerable.call(obj, key)
                                    "propertyIsEnumerable" => {
                                        Some("js_object_property_is_enumerable")
                                    }
                                    _ => None,
                                };
                                if let Some(runtime_fn) = runtime_fn {
                                    return Ok(Expr::Call {
                                        callee: Box::new(Expr::ExternFuncRef {
                                            name: runtime_fn.to_string(),
                                            param_types: Vec::new(),
                                            return_type: Type::Any,
                                        }),
                                        args,
                                        type_args: Vec::new(),
                                        byte_offset: 0,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Err(args)
}

/// `obj.hasOwnProperty(key)` → `js_object_has_own(obj, key)`.
pub(super) fn try_direct_has_own_call(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    args: Vec<Expr>,
    has_spread: bool,
) -> Result<Expr, Vec<Expr>> {
    if has_spread || args.len() != 1 {
        return Err(args);
    }
    if let ast::Callee::Expr(callee_expr) = &call.callee {
        if let ast::Expr::Member(member) = callee_expr.as_ref() {
            if let ast::MemberProp::Ident(prop) = &member.prop {
                if prop.sym.as_ref() == "hasOwnProperty" {
                    let receiver =
                        lower_expr(ctx, member.obj.as_ref()).map_err(|_| args.clone())?;
                    return Ok(Expr::Call {
                        callee: Box::new(Expr::ExternFuncRef {
                            name: "js_object_has_own".to_string(),
                            param_types: Vec::new(),
                            return_type: Type::Any,
                        }),
                        args: vec![receiver, args[0].clone()],
                        type_args: Vec::new(),
                        byte_offset: 0,
                    });
                }
            }
        }
    }
    Err(args)
}

/// `obj.propertyIsEnumerable(key)` → `js_object_property_is_enumerable(obj, key)`.
///
/// Mirrors [`try_direct_has_own_call`]. Needed because on built-in prototype
/// objects (`RegExp.prototype`, `Date.prototype`, …) `propertyIsEnumerable`
/// is installed as a no-op closure that shadows the real
/// `native_call_method` dispatch, so `RegExp.prototype.propertyIsEnumerable("global")`
/// resolved to the closure and returned garbage instead of `false`
/// (test262 built-ins/RegExp/prototype/{global,ignoreCase,multiline}/S15.10.7.*_A8).
/// Routing directly to the canonical FFI entry point makes it descriptor-aware
/// for every receiver, exactly as `hasOwnProperty` already is.
pub(super) fn try_direct_property_is_enumerable_call(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    args: Vec<Expr>,
    has_spread: bool,
) -> Result<Expr, Vec<Expr>> {
    if has_spread || args.len() != 1 {
        return Err(args);
    }
    if let ast::Callee::Expr(callee_expr) = &call.callee {
        if let ast::Expr::Member(member) = callee_expr.as_ref() {
            if let ast::MemberProp::Ident(prop) = &member.prop {
                if prop.sym.as_ref() == "propertyIsEnumerable" {
                    let receiver =
                        lower_expr(ctx, member.obj.as_ref()).map_err(|_| args.clone())?;
                    return Ok(Expr::Call {
                        callee: Box::new(Expr::ExternFuncRef {
                            name: "js_object_property_is_enumerable".to_string(),
                            param_types: Vec::new(),
                            return_type: Type::Any,
                        }),
                        args: vec![receiver, args[0].clone()],
                        type_args: Vec::new(),
                        byte_offset: 0,
                    });
                }
            }
        }
    }
    Err(args)
}

/// `Object.prototype.toString.call(x)` → `js_object_to_string(x)` and
/// `Object.prototype.hasOwnProperty.call(obj, key)` → `js_object_has_own(obj, key)`.
///
/// AST shape is a four-level member expression:
///   call.call(x)
///   ^^^^^^^^^^ outer member: (Object.prototype.toString).call
/// The runtime helper consults the class's `Symbol.toStringTag`
/// getter (registered at module init via `__perry_wk_tostringtag_*`)
/// and returns `[object <tag>]` or the default `[object Object]`.
/// `Object.prototype.<method>.call(...)` idioms — perry doesn't expose
/// `Object.prototype` so we rewrite to runtime helpers. Refs #420.
pub(super) fn try_object_prototype_call(
    call: &ast::CallExpr,
    args: Vec<Expr>,
    has_spread: bool,
) -> Result<Expr, Vec<Expr>> {
    if !has_spread && (1..=3).contains(&args.len()) {
        if let ast::Callee::Expr(callee_expr) = &call.callee {
            if let ast::Expr::Member(outer) = callee_expr.as_ref() {
                if let (ast::MemberProp::Ident(outer_prop), ast::Expr::Member(mid)) =
                    (&outer.prop, outer.obj.as_ref())
                {
                    if outer_prop.sym.as_ref() == "call" {
                        if let (ast::MemberProp::Ident(mid_prop), ast::Expr::Member(inner)) =
                            (&mid.prop, mid.obj.as_ref())
                        {
                            let runtime_fn = match (mid_prop.sym.as_ref(), args.len()) {
                                ("toString", 1) => Some("js_object_to_string"),
                                ("hasOwnProperty", 2) => Some("js_object_has_own"),
                                // #2891: Object.prototype.propertyIsEnumerable.call(obj, key)
                                ("propertyIsEnumerable", 2) => {
                                    Some("js_object_property_is_enumerable")
                                }
                                // Annex B accessor helpers: .call(obj, key[, fn]).
                                ("__defineGetter__", 3) => Some("js_object_define_getter"),
                                ("__defineSetter__", 3) => Some("js_object_define_setter"),
                                ("__lookupGetter__", 2) => Some("js_object_lookup_getter"),
                                ("__lookupSetter__", 2) => Some("js_object_lookup_setter"),
                                _ => None,
                            };
                            if let Some(runtime_fn) = runtime_fn {
                                if let (
                                    ast::MemberProp::Ident(inner_prop),
                                    ast::Expr::Ident(inner_obj),
                                ) = (&inner.prop, inner.obj.as_ref())
                                {
                                    if inner_obj.sym.as_ref() == "Object"
                                        && inner_prop.sym.as_ref() == "prototype"
                                    {
                                        return Ok(Expr::Call {
                                            callee: Box::new(Expr::ExternFuncRef {
                                                name: runtime_fn.to_string(),
                                                param_types: Vec::new(),
                                                return_type: Type::Any,
                                            }),
                                            args,
                                            type_args: Vec::new(),
                                            byte_offset: 0,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Err(args)
}
