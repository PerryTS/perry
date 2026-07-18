//! URL/URLSearchParams/Date instance methods, WeakRef/FinalizationRegistry.
//!
//! Extracted from `expr_call/mod.rs` as a mechanical move.

use anyhow::Result;
use perry_types::Type;
use swc_ecma_ast as ast;

use super::static_receiver::static_receiver_class;
use super::url_search_params::build_url_search_params_method_call;
use crate::ir::*;

use super::super::{lower_expr, LoweringContext};

fn new_callee_name(ctx: &LoweringContext, new_expr: &ast::NewExpr) -> Option<String> {
    match new_expr.callee.as_ref() {
        ast::Expr::Ident(class_ident) => {
            let raw = class_ident.sym.as_ref();
            Some(
                ctx.resolve_class_alias(raw)
                    .unwrap_or_else(|| raw.to_string()),
            )
        }
        ast::Expr::Member(member)
            if matches!(member.obj.as_ref(), ast::Expr::Ident(obj) if obj.sym.as_ref() == "globalThis")
                && ctx.lookup_local("globalThis").is_none() =>
        {
            match &member.prop {
                ast::MemberProp::Ident(prop_ident) => Some(prop_ident.sym.to_string()),
                _ => None,
            }
        }
        _ => None,
    }
}

pub(super) fn try_url_date_weakref_instance(
    ctx: &mut LoweringContext,
    _call: &ast::CallExpr,
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
            if let Some(class_name) = new_callee_name(ctx, new_expr) {
                if class_name == "URLSearchParams" {
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
            if static_receiver_class(ctx, member.obj.as_ref()) == Some("URLPattern")
                && matches!(method_name, "exec" | "test")
            {
                let pattern_expr = lower_expr(ctx, &member.obj)?;
                return Ok(Ok(Expr::NativeMethodCall {
                    module: "url".to_string(),
                    class_name: Some("URLPattern".to_string()),
                    object: Some(Box::new(pattern_expr)),
                    method: method_name.to_string(),
                    args,
                }));
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
        let receiver_may_be_date = !matches!(
            recv_class,
            Some("URL")
                | Some("Object")
                | Some("Buffer")
                | Some("BlockList")
                | Some("SocketAddress")
                | Some("Uint8Array")
                | Some("Uint8ClampedArray")
                | Some("Array")
        );
        // Most ambiguous Date methods retain the historical "unknown may be
        // Date" behavior. Direct `.toJSON()` is different: userland classes
        // commonly expose it as a plain method, and bracket/computed forms
        // already dispatch generically, so only statically-known Date
        // receivers should use the Date intrinsic.

        // Check for Date instance method calls (date.getTime(), etc.)
        if let ast::MemberProp::Ident(method_ident) = &member.prop {
            let method_name = method_ident.sym.as_ref();
            // toJSON has competing receiver shapes: Buffer-like values need
            // their own `.toJSON()` (exact Node toJSON output), statically
            // known Dates need DateToJSON, and ordinary userland objects must
            // remain a generic method call. Cache the lowered receiver so we
            // don't re-lower in the Date `toJSON` arm below.
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
                        | Expr::BufferConcatWithLength { .. }
                ) {
                    return Ok(Ok(Expr::Call {
                        callee: Box::new(Expr::PropertyGet {
                            object: Box::new(recv_expr),
                            property: "toJSON".to_string(),
                        }),
                        args,
                        type_args: vec![],
                        byte_offset: 0,
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
                    | "toUTCString"
                    | "toGMTString"
                    | "toLocaleDateString"
                    | "toLocaleTimeString"
                    | "toISOString"
                    | "valueOf"
            );
            let allow_date_method = if method_name == "toJSON" {
                recv_class == Some("Date")
            } else {
                receiver_may_be_date
            };
            if method_name == "setTime"
                && is_node_test_mock_timers_receiver(ctx, member.obj.as_ref())
            {
                // `node:test` exposes `mock.timers.setTime(ms)`. The broad
                // Date setter fallback below also matches `.setTime(...)` on
                // unknown receivers, so keep this known non-Date receiver on
                // the generic method-call path.
            } else if ambiguous && !allow_date_method {
                // Receiver is statically a non-Date class (e.g. URL), or this
                // is `.toJSON()` on an unknown/userland receiver.
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
                    "toUTCString" | "toGMTString" => {
                        let date_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::DateToUTCString(Box::new(date_expr))));
                    }
                    "toLocaleDateString" => {
                        // Only the zero-arg call folds to the fast default-format
                        // path. With a locale/options argument the call must fall
                        // through so the args reach the real Intl formatter (the
                        // `date_to_locale_date_string_opts` prototype thunk),
                        // otherwise e.g. `d.toLocaleDateString("en-US",
                        // {month:"short",day:"numeric"})` silently ignores the
                        // options and prints the numeric default. Mirrors
                        // `toLocaleString` (#5800).
                        if args.is_empty() {
                            let date_expr = lower_expr(ctx, &member.obj)?;
                            return Ok(Ok(Expr::DateToLocaleDateString(Box::new(date_expr))));
                        }
                        // fall through to generic method call with args intact
                    }
                    "toLocaleTimeString" => {
                        if args.is_empty() {
                            let date_expr = lower_expr(ctx, &member.obj)?;
                            return Ok(Ok(Expr::DateToLocaleTimeString(Box::new(date_expr))));
                        }
                        // fall through to generic method call with args intact
                    }
                    "toLocaleString" => {
                        // Only fold zero-arg calls to the fast DateToLocaleString
                        // path.  Calls with locale/options must fall through so
                        // arguments are preserved — both Temporal types and Date
                        // itself now handle locale/options on the generic path
                        // (Date via the `date_to_locale_string_opts` thunk, #5800).
                        if args.is_empty() {
                            let date_expr = lower_expr(ctx, &member.obj)?;
                            return Ok(Ok(Expr::DateToLocaleString(Box::new(date_expr))));
                        }
                        // fall through to generic method call with args intact
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
                        // #2851: Node Date setters accept optional trailing
                        // components (e.g. `setUTCHours(h, min?, sec?, ms?)`)
                        // and apply every supplied field in one call; an
                        // omitted *leading* argument (`setHours()`) coerces to
                        // NaN and makes the Date Invalid. We forward the entire
                        // argument list to the runtime, which handles defaults,
                        // NaN propagation, and the zero-argument case. So the
                        // dispatch no longer requires `!args.is_empty()`.
                        let setter_args = args;
                        let date_expr = lower_expr(ctx, &member.obj)?;
                        let date = Box::new(date_expr);
                        let setter_call = match method_name {
                            "setUTCFullYear" => Expr::DateSetUtcFullYear {
                                date,
                                args: setter_args,
                            },
                            "setUTCMonth" => Expr::DateSetUtcMonth {
                                date,
                                args: setter_args,
                            },
                            "setUTCDate" => Expr::DateSetUtcDate {
                                date,
                                args: setter_args,
                            },
                            "setUTCHours" => Expr::DateSetUtcHours {
                                date,
                                args: setter_args,
                            },
                            "setUTCMinutes" => Expr::DateSetUtcMinutes {
                                date,
                                args: setter_args,
                            },
                            "setUTCSeconds" => Expr::DateSetUtcSeconds {
                                date,
                                args: setter_args,
                            },
                            "setUTCMilliseconds" => Expr::DateSetUtcMilliseconds {
                                date,
                                args: setter_args,
                            },
                            "setFullYear" => Expr::DateSetFullYear {
                                date,
                                args: setter_args,
                            },
                            "setMonth" => Expr::DateSetMonth {
                                date,
                                args: setter_args,
                            },
                            "setDate" => Expr::DateSetDate {
                                date,
                                args: setter_args,
                            },
                            "setHours" => Expr::DateSetHours {
                                date,
                                args: setter_args,
                            },
                            "setMinutes" => Expr::DateSetMinutes {
                                date,
                                args: setter_args,
                            },
                            "setSeconds" => Expr::DateSetSeconds {
                                date,
                                args: setter_args,
                            },
                            "setMilliseconds" => Expr::DateSetMilliseconds {
                                date,
                                args: setter_args,
                            },
                            "setTime" => Expr::DateSetTime {
                                date,
                                args: setter_args,
                            },
                            _ => unreachable!(),
                        };
                        // #2089: Date is a reference type — a setter mutates the
                        // shared `DateCell` in place, so its effect is already
                        // visible through every alias / param / closure that
                        // holds this Date. The setter call evaluates to the
                        // numeric ms (the JS setter return value).
                        return Ok(Ok(setter_call));
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
                            let mut iter = args.into_iter();
                            let target = iter.next().unwrap_or(Expr::Undefined);
                            let held = iter.next().unwrap_or(Expr::Undefined);
                            let token = iter.next().map(Box::new);
                            return Ok(Ok(Expr::FinalizationRegistryRegister {
                                registry: Box::new(Expr::LocalGet(registry_id)),
                                target: Box::new(target),
                                held: Box::new(held),
                                token,
                            }));
                        }
                        "unregister" => {
                            let token = args.into_iter().next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::FinalizationRegistryUnregister {
                                registry: Box::new(Expr::LocalGet(registry_id)),
                                token: Box::new(token),
                            }));
                        }
                        _ => {}
                    }
                }
                // WeakMap/WeakSet — route to dedicated runtime functions
                // (NOT the regular Map/Set HIR variants) so reference-equality
                // works for object keys. Primitive keys/values are rejected by
                // the runtime helpers themselves (#2772): `js_weakmap_set` /
                // `js_weakset_add` validate the key/value at runtime and throw
                // Node's exact `Invalid value used as weak map key` /
                // `Invalid value used in weak set`. This covers both literal
                // and dynamic primitives uniformly, so no AST-literal fast path
                // is needed here.
                let make_extern_call = |name: &str, args: Vec<Expr>| -> Expr {
                    Expr::Call {
                        callee: Box::new(Expr::ExternFuncRef {
                            name: name.to_string(),
                            param_types: Vec::new(),
                            return_type: Type::Any,
                        }),
                        args,
                        type_args: Vec::new(),
                        byte_offset: 0,
                    }
                };
                if ctx.weakmap_locals.contains(&recv_name) {
                    let map_id = ctx.lookup_local(&recv_name).unwrap_or(0);
                    let recv = Expr::LocalGet(map_id);
                    match method_name {
                        "set" if args.len() >= 2 => {
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

fn is_node_test_mock_timers_receiver(ctx: &LoweringContext, expr: &ast::Expr) -> bool {
    let ast::Expr::Member(inner) = expr else {
        return false;
    };
    if !matches!(&inner.prop, ast::MemberProp::Ident(prop) if prop.sym.as_ref() == "timers") {
        return false;
    }
    let ast::Expr::Ident(root) = inner.obj.as_ref() else {
        return false;
    };
    ctx.lookup_imported_func(root.sym.as_ref()) == Some("mock")
}
