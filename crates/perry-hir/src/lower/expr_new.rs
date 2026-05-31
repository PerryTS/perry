//! `new C(args)` expression lowering: `ast::Expr::New`.
//!
//! Tier 2.3 round 3 (v0.5.339) — extracts the 393-LOC `New` arm from
//! `lower_expr`. Handles three constructor families: (a) user-defined
//! classes (lowered to `Expr::New { class_name, args }`), (b)
//! built-in JS classes routed to specialised HIR variants
//! (`new Date()` → `Expr::DateNew`, `new Map()` → `Expr::MapNew`,
//! `new RegExp()` → `Expr::RegExp`, `new Int32Array(...)` →
//! `Expr::TypedArrayNew`, etc.), (c) the dynamic
//! `new (someFn)(args)` form via `Expr::NewDynamic`.

use anyhow::{anyhow, Result};
use perry_types::{LocalId, Type};
use swc_ecma_ast as ast;

use crate::ir::Expr;
use crate::lower_decl::lower_class_from_ast;
use crate::lower_types::extract_ts_type_with_ctx;

use super::{lower_expr, LoweringContext};

/// Lower `new TextDecoder(label?, { fatal?, ignoreBOM? })` into
/// `Expr::TextDecoderNew { label, fatal, ignore_bom }`. Shared by
/// `expr_new.rs` (bound to a local) and `textencoder.rs` (inline
/// `new TextDecoder(...).decode(...)`).
pub(crate) fn lower_text_decoder_new(
    ctx: &mut LoweringContext,
    args: Option<&[ast::ExprOrSpread]>,
) -> Result<Expr> {
    let label = match args.and_then(|a| a.first()) {
        Some(arg) => lower_expr(ctx, &arg.expr)?,
        None => Expr::Undefined,
    };
    let mut fatal = Expr::Bool(false);
    let mut ignore_bom = Expr::Bool(false);
    if let Some(opts) = args.and_then(|a| a.get(1)) {
        if let ast::Expr::Object(obj) = opts.expr.as_ref() {
            for prop in &obj.props {
                if let ast::PropOrSpread::Prop(p) = prop {
                    if let ast::Prop::KeyValue(kv) = p.as_ref() {
                        let key = match &kv.key {
                            ast::PropName::Ident(i) => i.sym.to_string(),
                            ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                            _ => continue,
                        };
                        match key.as_str() {
                            "fatal" => fatal = lower_expr(ctx, &kv.value)?,
                            "ignoreBOM" => ignore_bom = lower_expr(ctx, &kv.value)?,
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    Ok(Expr::TextDecoderNew {
        label: Box::new(label),
        fatal: Box::new(fatal),
        ignore_bom: Box::new(ignore_bom),
    })
}

fn peel_new_callee(mut expr: &ast::Expr) -> &ast::Expr {
    loop {
        match expr {
            ast::Expr::Paren(paren) => expr = paren.expr.as_ref(),
            ast::Expr::TsAs(ts_as) => expr = ts_as.expr.as_ref(),
            ast::Expr::TsTypeAssertion(ts_ta) => expr = ts_ta.expr.as_ref(),
            ast::Expr::TsNonNull(ts_non_null) => expr = ts_non_null.expr.as_ref(),
            ast::Expr::TsConstAssertion(ts_const) => expr = ts_const.expr.as_ref(),
            _ => return expr,
        }
    }
}

fn nonconstructable_builtin_throw_expr(name: &str, mut args: Vec<Expr>) -> Expr {
    let helper = match name {
        "Symbol" => "js_throw_symbol_constructor_type_error",
        "BigInt" => "js_throw_bigint_constructor_type_error",
        "Math" => "js_throw_math_constructor_type_error",
        _ => unreachable!(),
    };
    let throw_expr = Expr::Call {
        callee: Box::new(Expr::ExternFuncRef {
            name: helper.to_string(),
            param_types: Vec::new(),
            return_type: Type::Any,
        }),
        args: Vec::new(),
        type_args: Vec::new(),
    };

    if args.is_empty() {
        throw_expr
    } else {
        args.push(throw_expr);
        Expr::Sequence(args)
    }
}

pub(super) fn lower_new(ctx: &mut LoweringContext, new_expr: &ast::NewExpr) -> Result<Expr> {
    let callee_expr = peel_new_callee(new_expr.callee.as_ref());

    // Issue #422: `new net.Socket()` over a `net` module alias. The
    // generic Member-callee path below would lower this to
    // `Expr::NewDynamic`, whose codegen fallback returns an empty
    // ObjectHeader placeholder — every subsequent `sock.connect/.on/.write`
    // would silently no-op. Reroute to a receiver-less `NativeMethodCall`
    // whose method name is the class name; the dispatch table in
    // `lower_call.rs::NATIVE_MODULE_TABLE` has a `("net", "Socket")` row
    // pointing at `js_net_socket_alloc`, and the let-stmt machinery in
    // `lower.rs` registers the result as a `("net", "Socket")` native
    // instance so subsequent method calls dispatch correctly.
    if let ast::Expr::Member(member) = callee_expr {
        if let (ast::Expr::Ident(obj_ident), ast::MemberProp::Ident(prop_ident)) =
            (member.obj.as_ref(), &member.prop)
        {
            let obj_name = obj_ident.sym.as_ref();
            let is_net_module =
                obj_name == "net" || ctx.lookup_builtin_module_alias(obj_name) == Some("net");
            if is_net_module && matches!(prop_ident.sym.as_ref(), "Socket" | "Server") {
                let args = new_expr
                    .args
                    .as_ref()
                    .map(|args| {
                        args.iter()
                            .map(|a| lower_expr(ctx, &a.expr))
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                return Ok(Expr::NativeMethodCall {
                    module: "net".to_string(),
                    class_name: None,
                    object: None,
                    method: prop_ident.sym.to_string(),
                    args,
                });
            }
            // #2129: `new http.Agent(options?)` / `new https.Agent(options?)`.
            // Same pattern as `new net.Socket()` above — reroute to a
            // receiver-less `NativeMethodCall` so the dispatch table's
            // `("http"|"https", "Agent")` row runs `js_*_agent_new`.
            // The let-stmt machinery in `lower.rs` then registers the
            // result as an `("http", "Agent")` native instance so
            // `agent.getName/.destroy/.maxSockets` etc. dispatch through
            // the class-filtered Agent rows. `https` Agent instances are
            // also tagged under `("http", "Agent")` so they share the
            // method surface — only the constructor's default protocol
            // differs.
            let is_http_module =
                obj_name == "http" || ctx.lookup_builtin_module_alias(obj_name) == Some("http");
            let is_https_module =
                obj_name == "https" || ctx.lookup_builtin_module_alias(obj_name) == Some("https");
            if (is_http_module || is_https_module) && prop_ident.sym.as_ref() == "Agent" {
                let args = new_expr
                    .args
                    .as_ref()
                    .map(|args| {
                        args.iter()
                            .map(|a| lower_expr(ctx, &a.expr))
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                return Ok(Expr::NativeMethodCall {
                    module: if is_https_module {
                        "https".to_string()
                    } else {
                        "http".to_string()
                    },
                    class_name: None,
                    object: None,
                    method: "Agent".to_string(),
                    args,
                });
            }
            let dns_module =
                if obj_name == "dns" || ctx.lookup_builtin_module_alias(obj_name) == Some("dns") {
                    Some("dns".to_string())
                } else if ctx.lookup_builtin_module_alias(obj_name) == Some("dns/promises") {
                    Some("dns/promises".to_string())
                } else {
                    ctx.lookup_native_module(obj_name)
                        .and_then(|(module_name, method)| {
                            if method.is_none() && matches!(module_name, "dns" | "dns/promises") {
                                Some(module_name.to_string())
                            } else {
                                None
                            }
                        })
                };
            if let Some(module_name) = dns_module {
                if prop_ident.sym.as_ref() == "Resolver" {
                    let args = new_expr
                        .args
                        .as_ref()
                        .map(|args| {
                            args.iter()
                                .map(|a| lower_expr(ctx, &a.expr))
                                .collect::<Result<Vec<_>>>()
                        })
                        .transpose()?
                        .unwrap_or_default();
                    return Ok(Expr::NativeMethodCall {
                        module: module_name,
                        class_name: None,
                        object: None,
                        method: "Resolver".to_string(),
                        args,
                    });
                }
            }
            let is_module_module = obj_name == "module"
                || ctx.lookup_builtin_module_alias(obj_name) == Some("module")
                || ctx
                    .lookup_native_module(obj_name)
                    .map(|(module_name, _)| module_name == "module")
                    .unwrap_or(false);
            if is_module_module && prop_ident.sym.as_ref() == "SourceMap" {
                let args = new_expr
                    .args
                    .as_ref()
                    .map(|args| {
                        args.iter()
                            .map(|a| lower_expr(ctx, &a.expr))
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                return Ok(Expr::NativeMethodCall {
                    module: "module".to_string(),
                    class_name: None,
                    object: None,
                    method: "SourceMap".to_string(),
                    args,
                });
            }
            let module_alias = obj_ident.sym.as_ref();
            let is_worker_threads_module = module_alias == "worker_threads"
                || ctx.lookup_builtin_module_alias(module_alias) == Some("worker_threads")
                || match ctx.lookup_native_module(module_alias) {
                    Some((module_name, _)) => module_name == "worker_threads",
                    None => false,
                };
            if is_worker_threads_module
                && matches!(
                    prop_ident.sym.as_ref(),
                    "MessageChannel" | "BroadcastChannel"
                )
            {
                let args = new_expr
                    .args
                    .as_ref()
                    .map(|args| {
                        args.iter()
                            .map(|a| lower_expr(ctx, &a.expr))
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                return Ok(Expr::NativeMethodCall {
                    module: "worker_threads".to_string(),
                    class_name: None,
                    object: None,
                    method: prop_ident.sym.to_string(),
                    args,
                });
            }
            if let Some((module_name, _)) = ctx.lookup_native_module(module_alias) {
                let class_name = prop_ident.sym.as_ref();
                if matches!(
                    (module_name, class_name),
                    ("async_hooks", "AsyncLocalStorage" | "AsyncResource")
                ) {
                    let args = new_expr
                        .args
                        .as_ref()
                        .map(|args| {
                            args.iter()
                                .map(|a| lower_expr(ctx, &a.expr))
                                .collect::<Result<Vec<_>>>()
                        })
                        .transpose()?
                        .unwrap_or_default();
                    return Ok(Expr::New {
                        class_name: class_name.to_string(),
                        args,
                        type_args: Vec::new(),
                    });
                }
            }
        }
    }

    // Issue #237: pre-register the controller param of every
    // `start` / `pull` / `cancel` / `transform` / `flush` callback
    // passed to `new ReadableStream({...})` /
    // `new TransformStream({...})` as a native instance so
    // `controller.enqueue(...)` etc. dispatch through the streams
    // arms in lower_call.rs. Without this hook the callback's
    // `controller` param has no type-tagged binding and method
    // calls on it silently no-op. Each field maps to (param_index,
    // module, class_name) — TransformStream's `transform(chunk,
    // controller)` controller is param 1, the rest are param 0.
    if let ast::Expr::Ident(ident) = new_expr.callee.as_ref() {
        let cls = ident.sym.as_ref();
        let field_specs: &[(&'static str, usize, &'static str, &'static str)] = match cls {
            "ReadableStream" => &[
                ("start", 0, "readable_stream", "ReadableStream"),
                ("pull", 0, "readable_stream", "ReadableStream"),
            ],
            "TransformStream" => &[
                ("transform", 1, "readable_stream", "ReadableStream"),
                ("flush", 0, "readable_stream", "ReadableStream"),
            ],
            _ => &[],
        };
        if !field_specs.is_empty() {
            if let Some(args) = new_expr.args.as_ref() {
                if let Some(first) = args.first() {
                    if let ast::Expr::Object(obj_lit) = first.expr.as_ref() {
                        for prop in &obj_lit.props {
                            if let ast::PropOrSpread::Prop(boxed_prop) = prop {
                                let mut handled = false;
                                match boxed_prop.as_ref() {
                                    ast::Prop::KeyValue(kv) => {
                                        let n = match &kv.key {
                                            ast::PropName::Ident(i) => Some(i.sym.as_ref()),
                                            ast::PropName::Str(s) => s.value.as_str(),
                                            _ => None,
                                        };
                                        if let Some(name) = n {
                                            if let Some((_, idx, mod_name, class_name)) =
                                                field_specs.iter().find(|(f, _, _, _)| *f == name)
                                            {
                                                let pat: Option<&ast::Pat> = match kv.value.as_ref()
                                                {
                                                    ast::Expr::Arrow(arrow) => {
                                                        arrow.params.get(*idx)
                                                    }
                                                    ast::Expr::Fn(fn_expr) => fn_expr
                                                        .function
                                                        .params
                                                        .get(*idx)
                                                        .map(|p| &p.pat),
                                                    _ => None,
                                                };
                                                if let Some(ast::Pat::Ident(pid)) = pat {
                                                    ctx.register_native_instance(
                                                        pid.id.sym.to_string(),
                                                        mod_name.to_string(),
                                                        class_name.to_string(),
                                                    );
                                                    handled = true;
                                                }
                                            }
                                        }
                                    }
                                    ast::Prop::Method(m) => {
                                        let n = match &m.key {
                                            ast::PropName::Ident(i) => Some(i.sym.as_ref()),
                                            ast::PropName::Str(s) => s.value.as_str(),
                                            _ => None,
                                        };
                                        if let Some(name) = n {
                                            if let Some((_, idx, mod_name, class_name)) =
                                                field_specs.iter().find(|(f, _, _, _)| *f == name)
                                            {
                                                if let Some(param) = m.function.params.get(*idx) {
                                                    if let ast::Pat::Ident(pid) = &param.pat {
                                                        ctx.register_native_instance(
                                                            pid.id.sym.to_string(),
                                                            mod_name.to_string(),
                                                            class_name.to_string(),
                                                        );
                                                        handled = true;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                                let _ = handled;
                            }
                        }
                    }
                }
            }
        }
    }

    // Try to extract class name from callee
    match callee_expr {
        ast::Expr::Ident(ident) => {
            let class_name = ident.sym.to_string();

            // #1677 `new Function(...)` handling, when `Function` is not
            // shadowed. Phase 1 (#1679) first: when every argument is a
            // compile-time-constant string, fold the call into a real
            // native function. Otherwise Phase 0 (#1678): refuse the
            // runtime-unknown bucket with a precise diagnostic; log the
            // const/known-codegen buckets and fall through to the existing
            // placeholder lowering.
            if class_name == "Function"
                && ctx.lookup_local("Function").is_none()
                && ctx.lookup_func("Function").is_none()
                && ctx.lookup_class("Function").is_none()
            {
                let args_slice = new_expr.args.as_deref().unwrap_or(&[]);
                if let Some(folded) = super::const_fold_fn::try_const_fold_function_construct(
                    ctx,
                    args_slice,
                    crate::eval_classifier::EvalSurface::NewFunction,
                    new_expr.span,
                )? {
                    return Ok(folded);
                }
                // Not fully const-foldable — body is the last argument
                // (`new Function(p1, p2, body)`); earlier args are param names.
                let body_arg = args_slice.last().map(|a| a.expr.as_ref());
                crate::eval_classifier::check_site(
                    crate::eval_classifier::EvalSurface::NewFunction,
                    body_arg,
                    &ctx.source_file_path,
                    new_expr.span,
                )?;
            }

            // #1691: an inline `new Request(...)` / `new Response(...)` / etc.
            // whose result is consumed immediately (never bound to a local)
            // skips the var-decl detection in destructuring/var_decl.rs, so
            // `uses_fetch` would stay false and the auto-optimize build would
            // strip the fetch / http-client feature — the link then fails on
            // `_js_request_new` / `_js_request_text` / … Set the flag here so
            // the inline and variable-assigned forms agree. (Lowering itself
            // is unchanged — these fall through to `Expr::New { class_name }`
            // below, which codegen dispatches to the runtime ctor.)
            if matches!(
                class_name.as_str(),
                "Request"
                    | "Response"
                    | "Headers"
                    | "Blob"
                    | "File"
                    | "ReadableStream"
                    | "WritableStream"
                    | "TransformStream"
            ) {
                ctx.uses_fetch = true;
            }

            // Handle built-in types
            if class_name == "Map" {
                // new Map() or new Map(entries)
                let args = new_expr
                    .args
                    .as_ref()
                    .map(|args| {
                        args.iter()
                            .map(|a| lower_expr(ctx, &a.expr))
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                if args.is_empty() {
                    return Ok(Expr::MapNew);
                } else {
                    return Ok(Expr::MapNewFromArray(Box::new(
                        args.into_iter().next().unwrap(),
                    )));
                }
            }
            if class_name == "Set" {
                // new Set() or new Set(iterable)
                let args = new_expr
                    .args
                    .as_ref()
                    .map(|args| {
                        args.iter()
                            .map(|a| lower_expr(ctx, &a.expr))
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                if args.is_empty() {
                    return Ok(Expr::SetNew);
                } else {
                    return Ok(Expr::SetNewFromArray(Box::new(
                        args.into_iter().next().unwrap(),
                    )));
                }
            }
            if class_name == "Date" {
                // new Date() / new Date(ts) / new Date(year, month, day, h?, m?, s?, ms?).
                // The multi-arg form is what dayjs's parseDate uses
                // (`new Date(d[1], m, d[3] || 1, ...)`) — without it the
                // codegen used to silently discard all but the first
                // argument, so a string year "2024" got parsed as
                // 2024 ms-since-epoch (issue: dayjs format prints
                // "292278994-08" because $d.getTime() ends up garbage).
                let args = new_expr
                    .args
                    .as_ref()
                    .map(|args| {
                        args.iter()
                            .map(|a| lower_expr(ctx, &a.expr))
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                return Ok(Expr::DateNew(args));
            }
            if class_name == "RegExp" {
                // new RegExp(pattern[, flags]) — for string-literal args,
                // route to the same `Expr::RegExp { pattern, flags }`
                // variant the literal `/foo/g` syntax produces. The
                // codegen interns both strings and calls
                // `js_regexp_new(pattern_handle, flags_handle)`.
                //
                // Without this branch, the New expression falls through
                // to generic class instantiation, which silently fails
                // (no user class named RegExp), leaving an unusable
                // ObjectHeader that makes regex.exec() return null and
                // any subsequent indexing on that null crash.
                let args_ast = new_expr.args.as_ref();
                let pattern_lit =
                    args_ast
                        .and_then(|args| args.first())
                        .and_then(|a| match a.expr.as_ref() {
                            ast::Expr::Lit(ast::Lit::Str(s)) => {
                                Some(s.value.as_str().unwrap_or("").to_string())
                            }
                            _ => None,
                        });
                let flags_lit = args_ast
                    .and_then(|args| args.get(1))
                    .and_then(|a| match a.expr.as_ref() {
                        ast::Expr::Lit(ast::Lit::Str(s)) => {
                            Some(s.value.as_str().unwrap_or("").to_string())
                        }
                        _ => None,
                    })
                    .unwrap_or_default();
                if let Some(pattern) = pattern_lit {
                    return Ok(Expr::RegExp {
                        pattern,
                        flags: flags_lit,
                    });
                }
                // Dynamic-arg `new RegExp(...)`: pattern (or flags) is
                // a runtime value. Fold to the same `RegExpDynamic`
                // variant the bare-call recognizer in expr_call.rs
                // produces — both lower to `js_regexp_new` with
                // dynamically-resolved string handles. Followup to
                // #957 / PR #959.
                if let Some(args) = args_ast {
                    if !args.is_empty() && args.iter().all(|a| a.spread.is_none()) {
                        let pattern = lower_expr(ctx, &args[0].expr)?;
                        let flags = if args.len() >= 2 {
                            Some(Box::new(lower_expr(ctx, &args[1].expr)?))
                        } else {
                            None
                        };
                        return Ok(Expr::RegExpDynamic {
                            pattern: Box::new(pattern),
                            flags,
                        });
                    }
                }
            }
            if matches!(class_name.as_str(), "Symbol" | "BigInt" | "Math") {
                let args = new_expr
                    .args
                    .as_ref()
                    .map(|args| {
                        args.iter()
                            .map(|a| lower_expr(ctx, &a.expr))
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                return Ok(nonconstructable_builtin_throw_expr(&class_name, args));
            }
            if class_name == "Proxy" {
                let args = new_expr
                    .args
                    .as_ref()
                    .map(|args| {
                        args.iter()
                            .map(|a| lower_expr(ctx, &a.expr))
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                let mut it = args.into_iter();
                let target = it.next().unwrap_or(Expr::Undefined);
                let handler = it.next().unwrap_or(Expr::Object(vec![]));
                return Ok(Expr::ProxyNew {
                    target: Box::new(target),
                    handler: Box::new(handler),
                });
            }
            if matches!(class_name.as_str(), "Number" | "String" | "Boolean") {
                let mut args = new_expr
                    .args
                    .as_ref()
                    .map(|args| {
                        args.iter()
                            .map(|a| lower_expr(ctx, &a.expr))
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                let kind = match class_name.as_str() {
                    "Number" => crate::BoxedPrimitiveKind::Number,
                    "String" => crate::BoxedPrimitiveKind::String,
                    "Boolean" => crate::BoxedPrimitiveKind::Boolean,
                    _ => unreachable!(),
                };
                let arg = args.drain(..).next().unwrap_or(Expr::Undefined);
                return Ok(Expr::BoxedPrimitiveNew {
                    kind,
                    arg: Box::new(arg),
                });
            }
            if ctx.proxy_locals.contains(&class_name) {
                let args = new_expr
                    .args
                    .as_ref()
                    .map(|args| {
                        args.iter()
                            .map(|a| lower_expr(ctx, &a.expr))
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                // If the proxy's construction wrapped a known class,
                // call the construct trap (for side effects) then
                // instantiate the real class. This matches the
                // test's expected behaviour.
                if let Some(target_class) = ctx.proxy_target_classes.get(&class_name).cloned() {
                    if ctx.lookup_class(&target_class).is_some() {
                        if let Some(id) = ctx.lookup_local(&class_name) {
                            let trap_call = Expr::ProxyConstruct {
                                proxy: Box::new(Expr::LocalGet(id)),
                                args: args.clone(),
                            };
                            return Ok(Expr::Sequence(vec![
                                trap_call,
                                Expr::New {
                                    class_name: target_class,
                                    args,
                                    type_args: vec![],
                                },
                            ]));
                        }
                    }
                }
                if let Some(id) = ctx.lookup_local(&class_name) {
                    return Ok(Expr::ProxyConstruct {
                        proxy: Box::new(Expr::LocalGet(id)),
                        args,
                    });
                }
            }
            // Handle AggregateError separately:
            // `new AggregateError(errors, message?, options?)`.
            //
            // #2838: `errors` is forwarded as a raw runtime value (not coerced
            // to an array literal) so the runtime consumes any iterable and
            // throws TypeError on a missing/non-iterable argument — so a
            // missing first arg defaults to `undefined`, NOT an empty array.
            // #2836: the third `options` argument carries `{ cause }`.
            if class_name == "AggregateError" {
                let args = new_expr
                    .args
                    .as_ref()
                    .map(|args| {
                        args.iter()
                            .map(|a| lower_expr(ctx, &a.expr))
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                let mut iter = args.into_iter();
                let errors = iter.next().unwrap_or(Expr::Undefined);
                let message = iter.next().unwrap_or(Expr::String("".to_string()));
                let options = iter.next().map(Box::new);
                return Ok(Expr::AggregateErrorNew {
                    errors: Box::new(errors),
                    message: Box::new(message),
                    options,
                });
            }

            // Handle Error and its subclasses
            if class_name == "Error"
                || class_name == "TypeError"
                || class_name == "RangeError"
                || class_name == "ReferenceError"
                || class_name == "SyntaxError"
                || class_name == "BugIndicatingError"
            {
                // new Error() / new Error(message) / new Error(message, { cause })
                //
                // 2-arg form detection runs at AST level (not HIR) because Phase 3
                // synthesises anon classes for closed-shape object literals — the
                // options `{ cause: e }` would become `Expr::New { __AnonShape_N }`
                // after lower_expr, and the `Expr::Object(fields)` match below
                // would miss it. Pull `cause` directly from the AST first, then
                // fall through to the standard argument lowering for other shapes.
                let ast_args = new_expr.args.as_deref().unwrap_or(&[]);
                // #2836: a 2-argument constructor — `new <ErrorKind>(message,
                // options)` — applies the ES2022 `{ cause }` option across the
                // base `Error` AND every native subclass. `BugIndicatingError`
                // (an effect-internal Error subclass) keeps its plain shape.
                if ast_args.len() == 2 && class_name != "BugIndicatingError" {
                    let msg = lower_expr(ctx, &ast_args[0].expr)?;
                    // Peel `Expr::Paren(({ cause: e }))` — SWC preserves paren
                    // nodes, so without unwrapping the literal fast path below
                    // would miss `new Error(msg, ({ cause }))`.
                    let mut opts_expr: &ast::Expr = &ast_args[1].expr;
                    while let ast::Expr::Paren(p) = opts_expr {
                        opts_expr = &p.expr;
                    }
                    // Fast path for base `Error` with a literal `{ cause: <e> }`
                    // / `{ cause }` — emits the existing `ErrorNewWithCause`
                    // variant (no runtime options read). Subclasses and dynamic
                    // option objects fall through to the runtime helper below.
                    if class_name == "Error" {
                        if let ast::Expr::Object(opts_obj) = opts_expr {
                            for prop in &opts_obj.props {
                                if let ast::PropOrSpread::Prop(p) = prop {
                                    match p.as_ref() {
                                        ast::Prop::KeyValue(kv) => {
                                            let key = match &kv.key {
                                                ast::PropName::Ident(i) => i.sym.to_string(),
                                                ast::PropName::Str(s) => {
                                                    s.value.as_str().unwrap_or("").to_string()
                                                }
                                                _ => continue,
                                            };
                                            if key == "cause" {
                                                let cause = lower_expr(ctx, &kv.value)?;
                                                return Ok(Expr::ErrorNewWithCause {
                                                    message: Box::new(msg),
                                                    cause: Box::new(cause),
                                                });
                                            }
                                        }
                                        ast::Prop::Shorthand(ident) => {
                                            let name = ident.sym.to_string();
                                            if name != "cause" {
                                                continue;
                                            }
                                            let cause = if let Some(func_id) =
                                                ctx.lookup_func(&name)
                                            {
                                                Expr::FuncRef(func_id)
                                            } else if let Some(local_id) = ctx.lookup_local(&name) {
                                                Expr::LocalGet(local_id)
                                            } else if ctx.lookup_class(&name).is_some() {
                                                Expr::ClassRef(name.clone())
                                            } else {
                                                continue;
                                            };
                                            return Ok(Expr::ErrorNewWithCause {
                                                message: Box::new(msg),
                                                cause: Box::new(cause),
                                            });
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                    // General case: lower the options as a runtime value and let
                    // the runtime read `.cause`. Works for `new TypeError(m,
                    // { cause })`, `new RangeError(m, opts)`, base Error with a
                    // variable-held options object, etc. ERROR_KIND_* values are
                    // hardcoded here (perry-hir has no perry-runtime dep): Error=0,
                    // TypeError=1, RangeError=2, ReferenceError=3, SyntaxError=4.
                    let kind: u32 = match class_name.as_str() {
                        "TypeError" => 1,
                        "RangeError" => 2,
                        "ReferenceError" => 3,
                        "SyntaxError" => 4,
                        _ => 0,
                    };
                    let options = lower_expr(ctx, &ast_args[1].expr)?;
                    return Ok(Expr::ErrorNewWithOptions {
                        kind,
                        message: Box::new(msg),
                        options: Box::new(options),
                    });
                }

                let args = new_expr
                    .args
                    .as_ref()
                    .map(|args| {
                        args.iter()
                            .map(|a| lower_expr(ctx, &a.expr))
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();

                if args.is_empty() {
                    return match class_name.as_str() {
                        "TypeError" => {
                            Ok(Expr::TypeErrorNew(Box::new(Expr::String("".to_string()))))
                        }
                        "RangeError" => {
                            Ok(Expr::RangeErrorNew(Box::new(Expr::String("".to_string()))))
                        }
                        "ReferenceError" => Ok(Expr::ReferenceErrorNew(Box::new(Expr::String(
                            "".to_string(),
                        )))),
                        "SyntaxError" => {
                            Ok(Expr::SyntaxErrorNew(Box::new(Expr::String("".to_string()))))
                        }
                        _ => Ok(Expr::ErrorNew(None)),
                    };
                } else {
                    let msg = args.into_iter().next().unwrap();
                    return match class_name.as_str() {
                        "TypeError" => Ok(Expr::TypeErrorNew(Box::new(msg))),
                        "RangeError" => Ok(Expr::RangeErrorNew(Box::new(msg))),
                        "ReferenceError" => Ok(Expr::ReferenceErrorNew(Box::new(msg))),
                        "SyntaxError" => Ok(Expr::SyntaxErrorNew(Box::new(msg))),
                        _ => Ok(Expr::ErrorNew(Some(Box::new(msg)))),
                    };
                }
            }

            // Handle URL class
            if class_name == "URL" {
                // new URL(url) or new URL(url, base)
                let args = new_expr
                    .args
                    .as_ref()
                    .map(|args| {
                        args.iter()
                            .map(|a| lower_expr(ctx, &a.expr))
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                let mut args_iter = args.into_iter();
                let url_arg = args_iter
                    .next()
                    .ok_or_else(|| anyhow!("URL constructor requires at least 1 argument"))?;
                let base_arg = args_iter.next();
                return Ok(Expr::UrlNew {
                    url: Box::new(url_arg),
                    base: base_arg.map(Box::new),
                });
            }

            // Handle URLSearchParams class
            if class_name == "URLSearchParams" {
                // new URLSearchParams() or new URLSearchParams(init)
                let args = new_expr
                    .args
                    .as_ref()
                    .map(|args| {
                        args.iter()
                            .map(|a| lower_expr(ctx, &a.expr))
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                let init_arg = args.into_iter().next();
                return Ok(Expr::UrlSearchParamsNew(init_arg.map(Box::new)));
            }

            // Handle WeakRef class — wraps a value (object) in a weak reference object.
            // Pragmatic implementation: stores a strong reference and `deref()` always returns it.
            if class_name == "WeakRef" {
                let args = new_expr
                    .args
                    .as_ref()
                    .map(|args| {
                        args.iter()
                            .map(|a| lower_expr(ctx, &a.expr))
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                let target = args.into_iter().next().unwrap_or(Expr::Undefined);
                return Ok(Expr::WeakRefNew(Box::new(target)));
            }

            // Handle FinalizationRegistry class — registers cleanup callbacks invoked when
            // tracked targets are GC'd. Pragmatic implementation: stores registrations but
            // never fires the callback (Perry's GC doesn't track weak references yet).
            if class_name == "FinalizationRegistry" {
                let args = new_expr
                    .args
                    .as_ref()
                    .map(|args| {
                        args.iter()
                            .map(|a| lower_expr(ctx, &a.expr))
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                let cb = args.into_iter().next().unwrap_or(Expr::Undefined);
                return Ok(Expr::FinalizationRegistryNew(Box::new(cb)));
            }
            // Handle TextEncoder constructor
            if class_name == "TextEncoder" {
                return Ok(Expr::TextEncoderNew);
            }
            // Handle TextDecoder constructor: new TextDecoder(label?, opts?)
            if class_name == "TextDecoder" {
                return lower_text_decoder_new(ctx, new_expr.args.as_deref());
            }

            // Handle Uint8Array constructor
            if class_name == "Uint8Array" {
                // new Uint8Array() or new Uint8Array(length) or new Uint8Array(array)
                let args = new_expr
                    .args
                    .as_ref()
                    .map(|args| {
                        args.iter()
                            .map(|a| lower_expr(ctx, &a.expr))
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                if args.is_empty() {
                    return Ok(Expr::Uint8ArrayNew(None));
                } else if args.len() == 1 {
                    return Ok(Expr::Uint8ArrayNew(Some(Box::new(
                        args.into_iter().next().unwrap(),
                    ))));
                }
                // 2+ args: fall through to Expr::New to handle
                // new Uint8Array(buffer, byteOffset, length) etc.
            }

            // Handle other typed-array constructors (Int8/16/32, Uint16/32, Float32/64,
            // Uint8ClampedArray). Uint8Array stays on the Buffer path above.
            if let Some(kind) = crate::ir::typed_array_kind_for_name(class_name.as_str()) {
                if class_name != "Uint8Array" {
                    let args = new_expr
                        .args
                        .as_ref()
                        .map(|args| {
                            args.iter()
                                .map(|a| lower_expr(ctx, &a.expr))
                                .collect::<Result<Vec<_>>>()
                        })
                        .transpose()?
                        .unwrap_or_default();
                    if args.is_empty() {
                        return Ok(Expr::TypedArrayNew { kind, arg: None });
                    } else if args.len() == 1 {
                        return Ok(Expr::TypedArrayNew {
                            kind,
                            arg: Some(Box::new(args.into_iter().next().unwrap())),
                        });
                    }
                    // Multi-arg form (buffer, byteOffset, length): fall through.
                }
            }

            let mut args = new_expr
                .args
                .as_ref()
                .map(|args| {
                    args.iter()
                        .map(|a| lower_expr(ctx, &a.expr))
                        .collect::<Result<Vec<_>>>()
                })
                .transpose()?
                .unwrap_or_default();
            // Extract explicit type arguments if present (e.g., new Box<number>(42))
            let type_args = new_expr
                .type_args
                .as_ref()
                .map(|ta| {
                    ta.params
                        .iter()
                        .map(|t| extract_ts_type_with_ctx(t, Some(ctx)))
                        .collect()
                })
                .unwrap_or_default();
            if ctx.lookup_class(&class_name).is_none() {
                if let Some(resolved) = ctx.resolve_class_alias(&class_name) {
                    if matches!(resolved.as_str(), "Blob" | "File") {
                        ctx.uses_fetch = true;
                        return Ok(Expr::New {
                            class_name: resolved,
                            args,
                            type_args,
                        });
                    }
                }
            }
            // Issue #838 followup (b): when `<Ident>` is NOT a real
            // class but resolves to a local binding, route through
            // `Expr::NewDynamic { callee: LocalGet(id), … }` so codegen
            // reaches the `js_new_function_construct` helper. dayjs's
            // minified outer `var _ = (function(){function M(){…}; …;
            // return M; })()` flows here: `_`'s init is a `Call` (not a
            // raw `Closure`/`FuncRef`), so the `function_valued_locals`
            // tracking can't prove function-ness at HIR time — but the
            // runtime helper performs its own `CLOSURE_MAGIC` check
            // before dispatching the constructor, so non-callable
            // receivers fall back to a class_id=0 empty-object
            // allocation that matches the pre-fix baseline. Real
            // classes still win — the `lookup_class` check above
            // returns `Expr::New { class_name }` before reaching here.
            if ctx.lookup_class(&class_name).is_none() {
                if let Some(local_id) = ctx.lookup_local(&class_name) {
                    return Ok(Expr::NewDynamic {
                        callee: Box::new(Expr::LocalGet(local_id)),
                        args,
                    });
                }
                // ES5 function constructors: `function Foo(){ this.x = … }`
                // used as `new Foo()`. A top-level `function` declaration is
                // tracked as a func (not a local, not a class), so neither the
                // local branch above nor the `lookup_class` path fires — it
                // would otherwise fall through to `Expr::New { class_name }`,
                // whose codegen finds no class named `Foo` and produces an
                // empty placeholder object that never runs the constructor
                // body (so `this.x = …` writes are lost and `new Foo().x` is
                // `undefined`). Route through `NewDynamic { FuncRef }` instead,
                // which reaches `js_new_function_construct`: it allocates the
                // instance, binds `this` for the duration of the call, runs the
                // body, and returns the populated object — the same helper the
                // local-binding path above relies on.
                if let Some(func_id) = ctx.lookup_func(&class_name) {
                    return Ok(Expr::NewDynamic {
                        callee: Box::new(Expr::FuncRef(func_id)),
                        args,
                    });
                }
            }
            // Issue #212: classes nested in a function may capture
            // enclosing-scope locals. `lower_class_decl` extended the
            // constructor with one synthesized param per captured id;
            // pass each as `LocalGet(id)` here so the outer scope's
            // current value is snapshotted onto the new instance.
            //
            // Issue #740: when `class_name` is the name of a `let/const`
            // alias (`const C = Inner` or `const C = makeChild(...)`
            // where the returned class is statically known via a
            // `ClassRef` chain), resolve through the alias before
            // looking up captures. Plain function-return aliases
            // (`const C = makeChild("foo")`) can't be resolved at HIR
            // time — those flow through the closure mechanism in
            // `compile_function` (the function body inlines `new`
            // with the captures forwarded correctly).
            let lookup_name = ctx
                .resolve_class_alias(&class_name)
                .unwrap_or_else(|| class_name.clone());
            let class_captures: Vec<LocalId> = ctx
                .lookup_class_captures(&lookup_name)
                .map(|c| c.to_vec())
                .unwrap_or_default();
            for cid in class_captures {
                args.push(Expr::LocalGet(cid));
            }
            Ok(Expr::New {
                class_name,
                args,
                type_args,
            })
        }
        // Non-identifier callee (e.g., new (condition ? A : B)() or new someVar())
        _ => {
            // Check for class expressions: new (class extends X { ... })()
            let class_expr_opt = match callee_expr {
                ast::Expr::Class(ce) => Some(ce),
                ast::Expr::Paren(paren) => match paren.expr.as_ref() {
                    ast::Expr::Class(ce) => Some(ce),
                    _ => None,
                },
                _ => None,
            };
            if let Some(class_expr) = class_expr_opt {
                let synthetic_name = format!("__anon_class_{}", ctx.fresh_class());
                let class = lower_class_from_ast(ctx, &class_expr.class, &synthetic_name, false)?;
                ctx.pending_classes.push(class);
                let args = new_expr
                    .args
                    .as_ref()
                    .map(|args| {
                        args.iter()
                            .map(|a| lower_expr(ctx, &a.expr))
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                let type_args = new_expr
                    .type_args
                    .as_ref()
                    .map(|ta| {
                        ta.params
                            .iter()
                            .map(|t| extract_ts_type_with_ctx(t, Some(ctx)))
                            .collect()
                    })
                    .unwrap_or_default();
                return Ok(Expr::New {
                    class_name: synthetic_name,
                    args,
                    type_args,
                });
            }

            let callee = Box::new(lower_expr(ctx, callee_expr)?);
            let args = new_expr
                .args
                .as_ref()
                .map(|args| {
                    args.iter()
                        .map(|a| lower_expr(ctx, &a.expr))
                        .collect::<Result<Vec<_>>>()
                })
                .transpose()?
                .unwrap_or_default();
            if let Expr::PropertyGet { object, property } = callee.as_ref() {
                if matches!(object.as_ref(), Expr::GlobalGet(_))
                    && matches!(property.as_str(), "Symbol" | "BigInt" | "Math")
                {
                    return Ok(nonconstructable_builtin_throw_expr(property, args));
                }
                if matches!(object.as_ref(), Expr::GlobalGet(_))
                    && matches!(property.as_str(), "Blob" | "File")
                {
                    ctx.uses_fetch = true;
                    return Ok(Expr::New {
                        class_name: property.clone(),
                        args,
                        type_args: Vec::new(),
                    });
                }
                if matches!(object.as_ref(), Expr::NativeModuleRef(module)
                    if module == "buffer" || module == "node:buffer")
                    && matches!(property.as_str(), "Blob" | "File")
                {
                    ctx.uses_fetch = true;
                    return Ok(Expr::New {
                        class_name: property.clone(),
                        args,
                        type_args: Vec::new(),
                    });
                }
            }
            Ok(Expr::NewDynamic { callee, args })
        }
    }
}
