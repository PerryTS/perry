//! URL/URLSearchParams/Date instance methods, WeakRef/FinalizationRegistry.
//!
//! Extracted from `expr_call/mod.rs` as a mechanical move.

use anyhow::{anyhow, Result};
use perry_types::{LocalId, Type};
use swc_ecma_ast as ast;

use super::static_receiver::static_receiver_class;
use super::url_search_params::build_url_search_params_method_call;
use crate::ir::*;
use crate::lower_types::extract_ts_type_with_ctx;

use super::super::{
    extract_typed_parse_source_order, is_generator_call_expr, is_widget_modifier_name, lower_expr,
    resolve_typed_parse_ty, LoweringContext,
};

pub(super) fn try_url_date_weakref_instance(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    expr: &ast::Expr,
    mut args: Vec<Expr>,
) -> Result<Result<Expr, Vec<Expr>>> {
    if let ast::Expr::Member(member) = expr {
        // Chained `new URLSearchParams(init).<method>(args)` — without
        // an intermediate let binding the typed-local arm below
        // doesn't fire and `.toString()` falls through to
        // Object.prototype.toString printing `"[object Object]"`.
        // Refs #575.
        if let ast::Expr::New(new_expr) = member.obj.as_ref() {
            if let ast::Expr::Ident(class_ident) = new_expr.callee.as_ref() {
                if class_ident.sym.as_ref() == "URLSearchParams" {
                    if let ast::MemberProp::Ident(method_ident) = &member.prop {
                        let method_name = method_ident.sym.as_ref();
                        let recv = lower_expr(ctx, &member.obj)?;
                        match build_url_search_params_method_call(recv, method_name, args) {
                            Ok(expr) => return Ok(Ok(expr)),
                            Err(returned_args) => args = returned_args,
                        }
                    }
                }
            }
        }

        // Chained `url.searchParams.<method>(args)` where `url` is a
        // URL-typed local or `new URL(...)`. Without this the call
        // falls through to generic property dispatch which can't find
        // `.get`/`.append`/etc. on the searchParams object (it's
        // backed by an opaque ObjectHeader). Rewrite the inner
        // `url.searchParams` access to `UrlGetSearchParams(url)` so
        // the URLSearchParams method dispatch fires.
        if let ast::Expr::Member(inner) = member.obj.as_ref() {
            if matches!(&inner.prop, ast::MemberProp::Ident(p) if p.sym.as_ref() == "searchParams")
                && static_receiver_class(ctx, inner.obj.as_ref()) == Some("URL")
            {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
                    let url_expr = lower_expr(ctx, &inner.obj)?;
                    let recv = Expr::UrlGetSearchParams(Box::new(url_expr));
                    match build_url_search_params_method_call(recv, method_name, args) {
                        Ok(expr) => return Ok(Ok(expr)),
                        Err(returned_args) => args = returned_args,
                    }
                }
            }
        }

        // Issue #650: URL instance toString / toJSON. Check this
        // before the Date arms so `urlInstance.toJSON()` doesn't
        // get rewritten as `DateToJSON(url)` and return a Date
        // string. Receiver-class detection only matches:
        //   `new URL(...).toString()` (immediate ctor)
        //   `let u = new URL(...); u.toString()` (typed local)
        if let ast::MemberProp::Ident(method_ident) = &member.prop {
            let method_name = method_ident.sym.as_ref();
            if static_receiver_class(ctx, member.obj.as_ref()) == Some("URL") {
                match method_name {
                    "toString" => {
                        let url_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::UrlInstanceToString(Box::new(url_expr))));
                    }
                    "toJSON" => {
                        let url_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::UrlInstanceToJSON(Box::new(url_expr))));
                    }
                    _ => {}
                }
            }
        }

        // Issue #650: gate the AMBIGUOUS Date instance method arms
        // on a receiver-type check. Methods like `toJSON` /
        // `toString` / `toLocaleString` / `valueOf` exist on every
        // JS object — pre-fix the arms below fired unconditionally,
        // so calling any of them on a URL / class instance / array
        // got silently rewritten as a Date method, returning a Date
        // string for the URL.toJSON() case the issue tracks.
        let recv_class = if let ast::MemberProp::Ident(_) = &member.prop {
            static_receiver_class(ctx, member.obj.as_ref())
        } else {
            None
        };
        // #809: `Some("Object")` (object literal / `Object.create`)
        // joins URL as a "definitely not a Date" receiver.
        let allow_ambiguous_date = !matches!(
            recv_class,
            Some("URL")
                | Some("Object")
                | Some("Buffer")
                | Some("Uint8Array")
                | Some("Uint8ClampedArray")
        );
        // Methods we treat as Date-only when the receiver is unambiguously
        // Date or unknown (current behavior). `toString` / `toJSON` etc.
        // skip these arms when `recv_class` proves the receiver is NOT a Date.

        // Check for Date instance method calls (date.getTime(), etc.)
        if let ast::MemberProp::Ident(method_ident) = &member.prop {
            let method_name = method_ident.sym.as_ref();
            // toJSON has two competing receiver shapes: Buffer-like values
            // need their own `.toJSON()` (exact Node toJSON output), every
            // other shape falls into the Date arms below. We must lower the
            // receiver to discriminate. Cache the lowered expr so we don't
            // re-lower in the Date `toJSON` arm at line ~263.
            let mut cached_recv: Option<Expr> = None;
            if method_name == "toJSON" {
                let recv_expr = lower_expr(ctx, &member.obj)?;
                if matches!(
                    recv_expr,
                    Expr::BufferFrom { .. }
                        | Expr::BufferFromArrayBuffer { .. }
                        | Expr::BufferAlloc { .. }
                        | Expr::BufferAllocUnsafe(_)
                        | Expr::BufferConcat(_)
                ) {
                    return Ok(Ok(Expr::Call {
                        callee: Box::new(Expr::PropertyGet {
                            object: Box::new(recv_expr),
                            property: "toJSON".to_string(),
                        }),
                        args,
                        type_args: vec![],
                    }));
                }
                cached_recv = Some(recv_expr);
            }
            let ambiguous = matches!(
                method_name,
                "toJSON"
                    | "toString"
                    | "toLocaleString"
                    | "toDateString"
                    | "toTimeString"
                    | "toLocaleDateString"
                    | "toLocaleTimeString"
                    | "toISOString"
                    | "valueOf"
            );
            if ambiguous && !allow_ambiguous_date {
                // Receiver is statically a non-Date class (e.g. URL).
                // Skip the Date arms below — fall through to generic.
            } else {
                match method_name {
                    "getTime" => {
                        let date_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::DateGetTime(Box::new(date_expr))));
                    }
                    "toISOString" => {
                        let date_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::DateToISOString(Box::new(date_expr))));
                    }
                    "getFullYear" => {
                        let date_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::DateGetFullYear(Box::new(date_expr))));
                    }
                    "getMonth" => {
                        let date_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::DateGetMonth(Box::new(date_expr))));
                    }
                    "getDate" => {
                        let date_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::DateGetDate(Box::new(date_expr))));
                    }
                    "getDay" => {
                        let date_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::DateGetDay(Box::new(date_expr))));
                    }
                    "getHours" => {
                        let date_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::DateGetHours(Box::new(date_expr))));
                    }
                    "getMinutes" => {
                        let date_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::DateGetMinutes(Box::new(date_expr))));
                    }
                    "getSeconds" => {
                        let date_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::DateGetSeconds(Box::new(date_expr))));
                    }
                    "getMilliseconds" => {
                        let date_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::DateGetMilliseconds(Box::new(date_expr))));
                    }
                    // UTC getters
                    "getUTCDay" => {
                        let date_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::DateGetUtcDay(Box::new(date_expr))));
                    }
                    "getUTCFullYear" => {
                        let date_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::DateGetUtcFullYear(Box::new(date_expr))));
                    }
                    "getUTCMonth" => {
                        let date_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::DateGetUtcMonth(Box::new(date_expr))));
                    }
                    "getUTCDate" => {
                        let date_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::DateGetUtcDate(Box::new(date_expr))));
                    }
                    "getUTCHours" => {
                        let date_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::DateGetUtcHours(Box::new(date_expr))));
                    }
                    "getUTCMinutes" => {
                        let date_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::DateGetUtcMinutes(Box::new(date_expr))));
                    }
                    "getUTCSeconds" => {
                        let date_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::DateGetUtcSeconds(Box::new(date_expr))));
                    }
                    "getUTCMilliseconds" => {
                        let date_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::DateGetUtcMilliseconds(Box::new(date_expr))));
                    }
                    // Other getters/methods
                    "valueOf" => {
                        let date_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::DateValueOf(Box::new(date_expr))));
                    }
                    "toDateString" => {
                        let date_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::DateToDateString(Box::new(date_expr))));
                    }
                    "toTimeString" => {
                        let date_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::DateToTimeString(Box::new(date_expr))));
                    }
                    "toLocaleDateString" => {
                        let date_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::DateToLocaleDateString(Box::new(date_expr))));
                    }
                    "toLocaleTimeString" => {
                        let date_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::DateToLocaleTimeString(Box::new(date_expr))));
                    }
                    "toLocaleString" => {
                        let date_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::DateToLocaleString(Box::new(date_expr))));
                    }
                    "getTimezoneOffset" => {
                        let date_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::DateGetTimezoneOffset(Box::new(date_expr))));
                    }
                    "toJSON" => {
                        let date_expr = match cached_recv.take() {
                            Some(e) => e,
                            None => lower_expr(ctx, &member.obj)?,
                        };
                        return Ok(Ok(Expr::DateToJSON(Box::new(date_expr))));
                    }
                    // UTC setters — mutate the local variable in place.
                    // Local-time setters (#1187) live in the same arm —
                    // pre-fix `setHours` / `setDate` / etc. fell through
                    // and surfaced as `(number).setHours is not a
                    // function` at runtime because Date is stored as a
                    // raw f64 with no method table.
                    "setUTCFullYear" | "setUTCMonth" | "setUTCDate" | "setUTCHours"
                    | "setUTCMinutes" | "setUTCSeconds" | "setUTCMilliseconds" | "setFullYear"
                    | "setMonth" | "setDate" | "setHours" | "setMinutes" | "setSeconds"
                    | "setMilliseconds" | "setTime" => {
                        if !args.is_empty() {
                            let value_expr = args.into_iter().next().unwrap();
                            let date_expr = lower_expr(ctx, &member.obj)?;
                            let setter_call = match method_name {
                                "setUTCFullYear" => Expr::DateSetUtcFullYear {
                                    date: Box::new(date_expr.clone()),
                                    value: Box::new(value_expr),
                                },
                                "setUTCMonth" => Expr::DateSetUtcMonth {
                                    date: Box::new(date_expr.clone()),
                                    value: Box::new(value_expr),
                                },
                                "setUTCDate" => Expr::DateSetUtcDate {
                                    date: Box::new(date_expr.clone()),
                                    value: Box::new(value_expr),
                                },
                                "setUTCHours" => Expr::DateSetUtcHours {
                                    date: Box::new(date_expr.clone()),
                                    value: Box::new(value_expr),
                                },
                                "setUTCMinutes" => Expr::DateSetUtcMinutes {
                                    date: Box::new(date_expr.clone()),
                                    value: Box::new(value_expr),
                                },
                                "setUTCSeconds" => Expr::DateSetUtcSeconds {
                                    date: Box::new(date_expr.clone()),
                                    value: Box::new(value_expr),
                                },
                                "setUTCMilliseconds" => Expr::DateSetUtcMilliseconds {
                                    date: Box::new(date_expr.clone()),
                                    value: Box::new(value_expr),
                                },
                                "setFullYear" => Expr::DateSetFullYear {
                                    date: Box::new(date_expr.clone()),
                                    value: Box::new(value_expr),
                                },
                                "setMonth" => Expr::DateSetMonth {
                                    date: Box::new(date_expr.clone()),
                                    value: Box::new(value_expr),
                                },
                                "setDate" => Expr::DateSetDate {
                                    date: Box::new(date_expr.clone()),
                                    value: Box::new(value_expr),
                                },
                                "setHours" => Expr::DateSetHours {
                                    date: Box::new(date_expr.clone()),
                                    value: Box::new(value_expr),
                                },
                                "setMinutes" => Expr::DateSetMinutes {
                                    date: Box::new(date_expr.clone()),
                                    value: Box::new(value_expr),
                                },
                                "setSeconds" => Expr::DateSetSeconds {
                                    date: Box::new(date_expr.clone()),
                                    value: Box::new(value_expr),
                                },
                                "setMilliseconds" => Expr::DateSetMilliseconds {
                                    date: Box::new(date_expr.clone()),
                                    value: Box::new(value_expr),
                                },
                                "setTime" => Expr::DateSetTime {
                                    date: Box::new(date_expr.clone()),
                                    value: Box::new(value_expr),
                                },
                                _ => unreachable!(),
                            };
                            // If receiver is a local variable, mutate it in place by wrapping
                            // the setter result in a LocalSet so the new timestamp is stored back.
                            if let Expr::LocalGet(local_id) = &date_expr {
                                return Ok(Ok(Expr::LocalSet(*local_id, Box::new(setter_call))));
                            }
                            return Ok(Ok(setter_call));
                        }
                    }
                    _ => {} // Fall through to other handling
                }
            } // close `else` of `if ambiguous && !allow_ambiguous_date`
        }

        // Check for WeakRef.deref() / FinalizationRegistry.register() / .unregister()
        // dispatch BEFORE the generic array method dispatch — these receivers were
        // tracked in the pre-scan pass.
        if let ast::MemberProp::Ident(method_ident) = &member.prop {
            let method_name = method_ident.sym.as_ref();
            if let ast::Expr::Ident(recv_ident) = member.obj.as_ref() {
                let recv_name = recv_ident.sym.to_string();
                if ctx.weakref_locals.contains(&recv_name) && method_name == "deref" {
                    return Ok(Ok(Expr::WeakRefDeref(Box::new(Expr::LocalGet(
                        ctx.lookup_local(&recv_name).unwrap_or(0),
                    )))));
                }
                if ctx.finreg_locals.contains(&recv_name) {
                    let registry_id = ctx.lookup_local(&recv_name).unwrap_or(0);
                    match method_name {
                        "register" => {
                            if args.len() >= 2 {
                                let mut iter = args.into_iter();
                                let target = iter.next().unwrap();
                                let held = iter.next().unwrap();
                                let token = iter.next().map(Box::new);
                                return Ok(Ok(Expr::FinalizationRegistryRegister {
                                    registry: Box::new(Expr::LocalGet(registry_id)),
                                    target: Box::new(target),
                                    held: Box::new(held),
                                    token,
                                }));
                            }
                        }
                        "unregister" => {
                            if !args.is_empty() {
                                let token = args.into_iter().next().unwrap();
                                return Ok(Ok(Expr::FinalizationRegistryUnregister {
                                    registry: Box::new(Expr::LocalGet(registry_id)),
                                    token: Box::new(token),
                                }));
                            }
                        }
                        _ => {}
                    }
                }
                // WeakMap/WeakSet — route to dedicated runtime functions
                // (NOT the regular Map/Set HIR variants) so reference-equality
                // works for object keys. Primitive keys/values throw via
                // js_weak_throw_primitive when the AST shows a bare literal.
                let make_extern_call = |name: &str, args: Vec<Expr>| -> Expr {
                    Expr::Call {
                        callee: Box::new(Expr::ExternFuncRef {
                            name: name.to_string(),
                            param_types: Vec::new(),
                            return_type: Type::Any,
                        }),
                        args,
                        type_args: Vec::new(),
                    }
                };
                let throw_primitive_expr = || -> Expr {
                    Expr::Call {
                        callee: Box::new(Expr::ExternFuncRef {
                            name: "js_weak_throw_primitive".to_string(),
                            param_types: Vec::new(),
                            return_type: Type::Any,
                        }),
                        args: Vec::new(),
                        type_args: Vec::new(),
                    }
                };
                if ctx.weakmap_locals.contains(&recv_name) {
                    let map_id = ctx.lookup_local(&recv_name).unwrap_or(0);
                    let recv = Expr::LocalGet(map_id);
                    match method_name {
                        "set" if args.len() >= 2 => {
                            let key_is_primitive_lit = matches!(
                                call.args.first().map(|a| a.expr.as_ref()),
                                Some(ast::Expr::Lit(_))
                            );
                            if key_is_primitive_lit {
                                return Ok(Ok(throw_primitive_expr()));
                            }
                            let mut iter = args.into_iter();
                            let key = iter.next().unwrap();
                            let value = iter.next().unwrap();
                            return Ok(Ok(make_extern_call(
                                "js_weakmap_set",
                                vec![recv, key, value],
                            )));
                        }
                        "get" if !args.is_empty() => {
                            return Ok(Ok(make_extern_call(
                                "js_weakmap_get",
                                vec![recv, args.into_iter().next().unwrap()],
                            )));
                        }
                        "has" if !args.is_empty() => {
                            return Ok(Ok(make_extern_call(
                                "js_weakmap_has",
                                vec![recv, args.into_iter().next().unwrap()],
                            )));
                        }
                        "delete" if !args.is_empty() => {
                            return Ok(Ok(make_extern_call(
                                "js_weakmap_delete",
                                vec![recv, args.into_iter().next().unwrap()],
                            )));
                        }
                        _ => {}
                    }
                }
                if ctx.weakset_locals.contains(&recv_name) {
                    let set_id = ctx.lookup_local(&recv_name).unwrap_or(0);
                    let recv = Expr::LocalGet(set_id);
                    match method_name {
                        "add" if !args.is_empty() => {
                            let value_is_primitive_lit = matches!(
                                call.args.first().map(|a| a.expr.as_ref()),
                                Some(ast::Expr::Lit(_))
                            );
                            if value_is_primitive_lit {
                                return Ok(Ok(throw_primitive_expr()));
                            }
                            return Ok(Ok(make_extern_call(
                                "js_weakset_add",
                                vec![recv, args.into_iter().next().unwrap()],
                            )));
                        }
                        "has" if !args.is_empty() => {
                            return Ok(Ok(make_extern_call(
                                "js_weakset_has",
                                vec![recv, args.into_iter().next().unwrap()],
                            )));
                        }
                        "delete" if !args.is_empty() => {
                            return Ok(Ok(make_extern_call(
                                "js_weakset_delete",
                                vec![recv, args.into_iter().next().unwrap()],
                            )));
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    Ok(Err(args))
}
