//! Static method calls (Class.method) and native instance method calls (emitter.on, ws.send).
//!
//! Extracted from `expr_call/mod.rs` as a mechanical move.

use anyhow::{anyhow, Result};
use swc_common::Spanned;
use swc_ecma_ast as ast;

use super::stream::is_stream_api_method;
use crate::ir::*;
use crate::lower_patterns::detect_native_instance_expr;

use super::super::{is_widget_modifier_name, lower_expr, LoweringContext};

fn unwrap_ts_wrappers(e: &ast::Expr) -> &ast::Expr {
    let mut cur = e;
    loop {
        cur = match cur {
            ast::Expr::TsAs(x) => x.expr.as_ref(),
            ast::Expr::TsNonNull(x) => x.expr.as_ref(),
            ast::Expr::TsSatisfies(x) => x.expr.as_ref(),
            ast::Expr::TsTypeAssertion(x) => x.expr.as_ref(),
            ast::Expr::TsConstAssertion(x) => x.expr.as_ref(),
            ast::Expr::Paren(x) => x.expr.as_ref(),
            _ => return cur,
        };
    }
}

fn is_node_stream_iterator_helper_instance_method(
    module_name: &str,
    class_name: &str,
    method_name: &str,
) -> bool {
    matches!(module_name, "stream" | "node:stream")
        && matches!(
            class_name,
            "Readable" | "Duplex" | "Transform" | "PassThrough"
        )
        && matches!(
            method_name,
            "toArray"
                | "map"
                | "filter"
                | "reduce"
                | "forEach"
                | "find"
                | "some"
                | "every"
                | "flatMap"
                | "take"
                | "drop"
                | "iterator"
        )
}

pub(super) fn try_static_method_and_instance(
    ctx: &mut LoweringContext,
    // #854: kept for the uniform `try_*` dispatch-helper signature; this arm
    // works off `expr`, not the raw `CallExpr` — except for the spread check.
    call: &ast::CallExpr,
    expr: &ast::Expr,
    args: Vec<Expr>,
) -> Result<Result<Expr, Vec<Expr>>> {
    // `StaticMethodCall` carries plain positional `args: Vec<Expr>` with no way
    // to express argument spreads, so routing a spread call (`C.method(...xs)`)
    // here would silently drop the spread markers — the spread sources land as
    // single positional args and `arguments.length` undercounts. When the call
    // spreads, skip the static-method production below and let the generic
    // `CallSpread` path (which expands spreads into the runtime args array)
    // handle it. Refs test262 language/arguments-object
    // cls-*-static-*-spread-operator.
    let static_call_has_spread = call.args.iter().any(|a| a.spread.is_some());
    // Check for static method calls (e.g., Counter.increment())
    if let ast::Expr::Member(member) = expr {
        if let ast::Expr::Ident(obj_ident) = unwrap_ts_wrappers(member.obj.as_ref()) {
            let obj_name = obj_ident.sym.to_string();
            // A bound LOCAL variable / parameter shadows any class, imported
            // class, or native-module binding of the SAME name (JS lexical
            // scoping). Minified bundles frequently reuse single-letter names:
            // Next.js's `app-route-turbo` defines `class n { static has(e,t){…} }`
            // at module scope AND a `let n = new Set` inside its implicit-tags
            // builder. Without this guard, `n.has(key)` (Set.has) was lowered to
            // `StaticMethodCall { class_name: "n", method: "has" }`, calling the
            // class's static `has(e,t)` with `(key, undefined)` →
            // `Reflect.has("…", undefined)` → "Reflect.has called on non-object"
            // (Next.js dynamic/API routes 500'd). The local must win.
            //
            // Scope this guard to the STATIC-class / native-module-static arms
            // below — NOT to the native-INSTANCE arm (line ~191), which is
            // itself keyed on a local/param binding via `lookup_native_instance`
            // (e.g. an upgrade handler's `wsId.send(...)` parameter) and must
            // still dispatch.
            let local_shadows_class = ctx.lookup_local(&obj_name).is_some();
            if local_shadows_class {
                // fall through past the static arms to native-instance / generic
                // dispatch below.
            } else if let Some((module_name, Some(class_name))) =
                ctx.lookup_native_module(&obj_name)
            {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.to_string();
                    let normalized_module =
                        module_name.strip_prefix("node:").unwrap_or(module_name);
                    let is_supported_native_class_static =
                        perry_api_manifest::iter_entries().any(|entry| {
                            entry.module == normalized_module
                                && entry.name == method_name
                                && matches!(
                                    entry.kind,
                                    perry_api_manifest::ApiKind::Method {
                                        has_receiver: false,
                                        class_filter: Some(filter),
                                    } if filter == class_name
                                )
                        });
                    if is_supported_native_class_static {
                        return Ok(Ok(Expr::NativeMethodCall {
                            module: module_name.to_string(),
                            class_name: Some(class_name.to_string()),
                            object: None,
                            method: method_name,
                            args,
                        }));
                    }
                }
            }
            // Treat uppercase imported identifiers as candidate classes —
            // we don't have cross-module class metadata at HIR-lower
            // time, so without this `import { MongoClient } from
            // 'pkg'; MongoClient.connect(...)` falls through to the
            // dynamic-dispatch path and reads garbage from the static
            // ClosureHeader.  See compile.rs::imported_classes for the
            // backing dispatch table that resolves these calls at
            // codegen time.
            // A wildcard namespace import (`import * as Effect from "mod"`) is
            // a module namespace, not a class — even when uppercase. Its
            // members must be resolved and CALLED (`Effect.succeed(42)`), not
            // lowered to a StaticMethodCall (which, post-V8-removal, returns
            // the member function uncalled — the Effect #321 blocker).
            // Excluding namespaces here lets `Effect.succeed(42)` fall through
            // to the namespace-member-call path. Named/default imports of real
            // classes (`import { MongoClient }`) are not namespace locals and
            // keep this static-method path.
            let is_imported_upper = ctx.lookup_imported_func(&obj_name).is_some()
                && !ctx.namespace_import_locals.contains(&obj_name)
                && obj_name
                    .chars()
                    .next()
                    .map(|c| c.is_uppercase())
                    .unwrap_or(false);
            // #5938 follow-up: a body-local `class X` colliding with an
            // outer/prior `class X` is registered under a scope-local rename
            // (`class_renames`), and a later `X.method()` in the same body
            // must dispatch to the RENAMED class — the raw name binds the
            // FIRST same-named registrant (factory B's `e.who()` read factory
            // A's decl-site snapshot). Imports keep the raw name: a renamed
            // class is by construction body-local, never imported.
            let resolved_class = ctx.resolve_class_name(&obj_name);
            if !local_shadows_class
                && (ctx.lookup_class(&resolved_class).is_some() || is_imported_upper)
            {
                match &member.prop {
                    ast::MemberProp::Ident(method_ident) => {
                        let method_name = method_ident.sym.to_string();
                        let is_readable_stream_from = method_name == "from"
                            && (matches!(
                                ctx.lookup_native_module(&obj_name),
                                Some(("stream/web", Some("ReadableStream")))
                                    | Some(("node:stream/web", Some("ReadableStream")))
                            ) || (obj_name == "ReadableStream" && is_imported_upper));
                        if is_readable_stream_from {
                            return Ok(Ok(Expr::NativeMethodCall {
                                module: "readable_stream".to_string(),
                                class_name: Some("ReadableStream".to_string()),
                                object: None,
                                method: "from".to_string(),
                                args,
                            }));
                        }
                        if (ctx.has_static_method(&resolved_class, &method_name)
                            || is_imported_upper)
                            && !static_call_has_spread
                        {
                            return Ok(Ok(Expr::StaticMethodCall {
                                class_name: resolved_class,
                                method_name,
                                args,
                            }));
                        }
                    }
                    // Private static method: WithPrivateStatic.#helper()
                    ast::MemberProp::PrivateName(priv_ident) => {
                        let method_name = format!("#{}", priv_ident.name);
                        if ctx.has_static_method(&resolved_class, &method_name)
                            && !static_call_has_spread
                        {
                            return Ok(Ok(Expr::StaticMethodCall {
                                class_name: resolved_class,
                                method_name,
                                args,
                            }));
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // Check for native instance method calls (e.g., emitter.on(), ws.send())
    if let ast::Expr::Member(member) = expr {
        if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
            let obj_name = obj_ident.sym.to_string();
            // Clone module_name and class_name to avoid borrow issues
            let native_instance = ctx
                .lookup_native_instance(&obj_name)
                .map(|(m, c)| (m.to_string(), c.to_string()));

            if let Some((module_name, class_name)) = native_instance {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.to_string();
                    // Issue #562: stream subclass instances carry the
                    // bare-stream module/class tag for inherited-method
                    // dispatch (`w.pipeTo(...)`, `w.getWriter()`).
                    // Routing user-declared methods through the
                    // NativeMethodCall arm misses every dispatcher and
                    // falls through to the receiver-less zero-sentinel,
                    // so the method call returns undefined. Only route
                    // known stream-API methods through NativeMethodCall;
                    // anything else falls through to the user-class
                    // method dispatch path further down. Mirrors the
                    // PropertyGet gate in expr_member.rs.
                    let is_stream_module = matches!(
                        module_name.as_str(),
                        "readable_stream"
                            | "writable_stream"
                            | "transform_stream"
                            | "readable_stream_reader"
                            | "writable_stream_writer"
                    );
                    let is_util_mime_instance = matches!(module_name.as_str(), "util" | "sys")
                        && matches!(class_name.as_str(), "MIMEType" | "MIMEParams");
                    let is_worker_messaging_instance = module_name == "worker_threads"
                        && matches!(class_name.as_str(), "BroadcastChannel" | "MessagePort")
                        && matches!(
                            method_name.as_str(),
                            "postMessage"
                                | "close"
                                | "ref"
                                | "unref"
                                | "hasRef"
                                | "addEventListener"
                                | "removeEventListener"
                        );
                    let is_node_stream_iterator_helper =
                        is_node_stream_iterator_helper_instance_method(
                            &module_name,
                            &class_name,
                            &method_name,
                        );
                    if is_util_mime_instance
                        || is_worker_messaging_instance
                        || is_node_stream_iterator_helper
                    {
                        // MIMEType/MIMEParams methods are ordinary object
                        // prototype methods registered in the runtime class
                        // vtable; node:stream iterator helpers are also only
                        // installed on the runtime object method table. Let the
                        // generic property-call path bind `this` dynamically.
                    } else if is_stream_module && !is_stream_api_method(&module_name, &method_name)
                    {
                        // Fall through — let the regular method-call
                        // dispatch further down handle the user-class
                        // method.
                    } else {
                        // Get the object expression (the instance variable)
                        let object_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::NativeMethodCall {
                            module: module_name,
                            class_name: Some(class_name), // Use the registered class name
                            object: Some(Box::new(object_expr)),
                            method: method_name,
                            args,
                        }));
                    }
                }
            }
        }

        // Chained native-factory receiver: `createServer(handler).listen(...)`
        // (and the https/http2 variants). The receiver is the inline factory
        // CALL result, not an Ident the native-instance table can resolve, so
        // the `if let ast::Expr::Ident` arm above misses it. Without this branch
        // the chained `.listen(...)` falls through to the generic dynamic
        // dispatch, the NativeModSig with `class_filter = Some("HttpServer")` is
        // never matched, and the native listen fn is never called — so the
        // server never binds and the process exits immediately. The variable
        // form (`const s = createServer(...); s.listen(...)`) already works
        // because the let-stmt arm tags `s`; only the chained form was broken.
        // Issue #2041.
        if let ast::Expr::Call(inner_call) = member.obj.as_ref() {
            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                if let Some((module_name, class_name)) =
                    native_class_from_factory_call(ctx, inner_call)
                {
                    let method_name = method_ident.sym.to_string();
                    let object_expr = lower_expr(ctx, &member.obj)?;
                    return Ok(Ok(Expr::NativeMethodCall {
                        module: module_name.to_string(),
                        class_name: Some(class_name.to_string()),
                        object: Some(Box::new(object_expr)),
                        method: method_name,
                        args,
                    }));
                }
            }
        }

        // issue #195: WidgetCtor(...).modifierName(...) is silently dropped.
        // Reject at compile time so users discover the options-object form.
        if let ast::Expr::Call(inner_call) = member.obj.as_ref() {
            if let ast::Callee::Expr(inner_callee) = &inner_call.callee {
                if let ast::Expr::Ident(widget_ident) = inner_callee.as_ref() {
                    let widget_name = widget_ident.sym.as_ref();
                    if matches!(
                        widget_name,
                        "Text"
                            | "VStack"
                            | "HStack"
                            | "ZStack"
                            | "Image"
                            | "Spacer"
                            | "Divider"
                            | "ForEach"
                            | "Label"
                            | "Gauge"
                    ) && matches!(ctx.lookup_native_module(widget_name), Some(("perry/ui", _)))
                    {
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let modifier_name = method_ident.sym.as_ref();
                            if is_widget_modifier_name(modifier_name) {
                                return Err(anyhow!(
                                                "modifier '{}' must be passed as an option-object on the widget constructor; use: {}(\"...\", {{ {}: ... }})",
                                                modifier_name, widget_name, modifier_name
                                            ));
                            }
                        }
                    }
                }
            }
        }

        // Check for method calls on new Big/Decimal/BigNumber() expressions
        // e.g., new Big("100").div(2)
        if let Some(module_name) = detect_native_instance_expr(ctx, &member.obj) {
            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                let method_name = method_ident.sym.to_string();
                let object_expr = lower_expr(ctx, &member.obj)?;
                return Ok(Ok(Expr::NativeMethodCall {
                    module: module_name.to_string(),
                    class_name: None, // Will be set by js_transform if needed
                    object: Some(Box::new(object_expr)),
                    method: method_name,
                    args,
                }));
            }
        }

        // Check for chained method calls on registered native instances
        // e.g., r1.times(...).times(...) where r1 is a Big
        // The inner call might lower to a NativeMethodCall, and we need to chain properly
        if let ast::MemberProp::Ident(method_ident) = &member.prop {
            let method_name = method_ident.sym.to_string();
            if !may_lower_to_native_method_call(ctx, &member.obj) {
                return Ok(Err(args));
            }
            // Lower the object expression once.
            //
            // Perf (O(n²)/exponential → O(n) on long native-fluent chains): this
            // is a FULL recursive lowering of the entire receiver prefix, done
            // speculatively to inspect whether the inner call lowered to a
            // `NativeMethodCall` of a recognized fluent module. On a chain like
            // `K.name(..).description(..).option(..)…` (commander / minified CLI
            // builders) where `may_lower_to_native_method_call` over-approximates
            // to `true` but the inner call actually lowers to a *generic* `Call`,
            // every arm below misses, `object_expr` is discarded, and we return
            // `Err(args)` — whereupon the `lower_call_inner` fall-through tail
            // re-lowers the same member callee (and thus this whole prefix)
            // again. Repeated per chain level that is exponential blowup.
            //
            // To make the tail reuse this lowering instead of redoing it, stash
            // it keyed by the receiver's source span. `lower_member_inner` (the
            // tail's receiver-lowering site) consumes it for the matching span.
            // Reuse is sound: lowering a receiver is idempotent in the value it
            // produces, and the fluent-success arms below already reuse this very
            // `object_expr`.
            let obj_span = member.obj.as_ref().span();
            let object_expr = lower_expr(ctx, &member.obj)?;
            // Check if it's a NativeMethodCall for a fluent-API native module
            if let Expr::NativeMethodCall {
                module,
                class_name,
                method: prior_method,
                ..
            } = &object_expr
            {
                // Methods that return the same type (builder pattern)
                let is_math_lib =
                    matches!(module.as_str(), "big.js" | "decimal.js" | "bignumber.js");
                let is_math_method = matches!(
                    method_name.as_str(),
                    // arithmetic + chainable rounding/formatting
                    "plus" | "minus" | "times" | "div" | "mod" |
                            "pow" | "sqrt" | "abs" | "neg" | "round" | "floor" | "ceil" | "toFixed" |
                            // decimal.js: terminal-shape methods that still need
                            // NativeMethodCall dispatch (so a.plus(b).eq(c) etc.
                            // doesn't fall back to the generic Call+PropertyGet path).
                            "toString" | "toNumber" | "valueOf" |
                            "eq" | "lt" | "lte" | "gt" | "gte" | "cmp" |
                            "isZero" | "isPositive" | "isNegative"
                );
                // commander Command — every fluent method either
                // returns the same handle (name/version/description/
                // option/requiredOption/action) or a sub-Command with
                // the same module + class (.command(name)). Either way
                // the next chained call must dispatch through the
                // commander NativeModSig table, not the generic
                // dynamic-property fallback. Without this branch
                // `program.name(...).version(...)` only the first
                // call landed as a NativeMethodCall and the rest
                // silently no-op'd at codegen — issue #187.
                let is_commander = module.as_str() == "commander";
                let is_commander_method = matches!(
                    method_name.as_str(),
                    "name"
                        | "version"
                        | "description"
                        | "option"
                        | "requiredOption"
                        | "action"
                        | "command"
                        | "parse"
                        | "opts"
                        | "argument"
                );
                // #1048 — fastify Reply chainable methods. `reply.code(201)
                // .type("application/json").send(payload)` ships every method
                // returning the same reply handle for chaining; without this
                // branch only the inner `.code(...)` resolved as a
                // NativeMethodCall and the rest of the chain fell through to
                // the generic Call+PropertyGet path. That path routes through
                // `js_native_call_method` → HANDLE_METHOD_DISPATCH, which has no
                // method-dispatch arm for fastify (the in-stdlib adapter was
                // removed; fastify is served by perry-ext-fastify) — so without
                // this branch `.type(...)` returned an
                // untagged NaN that the next chain step read as a number
                // ("(number).send is not a function"). Static NATIVE_MODULE_TABLE
                // dispatch covers it correctly.
                let is_fastify_reply =
                    module.as_str() == "fastify" && class_name.as_deref() == Some("Reply");
                let is_fastify_reply_chain_method = matches!(
                    method_name.as_str(),
                    "code" | "status" | "header" | "type" | "send"
                );
                // #2208 — http(s) `ClientRequest` fluent methods. Node's
                // `EventEmitter.prototype.on`/`once`/`off`/etc. return the
                // emitter itself, and `setHeader`/`setTimeout` likewise
                // return the request, so `http.request(...).on(...).on(...)`
                // (or any `.setHeader(...).end()` shape) must keep the
                // ClientRequest class tag flowing through each chain step.
                // Without this branch the second `.on(...)` fell through to
                // the generic Call+PropertyGet path, the receiver came back
                // as an untagged number, and the next step crashed with
                // "(number).end is not a function". Same shape as the
                // fastify Reply chain above.
                let is_http_client_request =
                    module.as_str() == "http" && class_name.as_deref() == Some("ClientRequest");
                let is_client_request_chain_method = matches!(
                    method_name.as_str(),
                    "on" | "once"
                        | "off"
                        | "addListener"
                        | "removeListener"
                        | "removeAllListeners"
                        | "setHeader"
                        | "setTimeout"
                        | "write"
                        | "destroy"
                        | "end"
                );
                if (is_math_lib && is_math_method)
                    || (is_commander && is_commander_method)
                    || (is_fastify_reply && is_fastify_reply_chain_method)
                    || (is_http_client_request && is_client_request_chain_method)
                {
                    return Ok(Ok(Expr::NativeMethodCall {
                        module: module.clone(),
                        class_name: class_name.clone(),
                        object: Some(Box::new(object_expr)),
                        method: method_name,
                        args,
                    }));
                }
                // Database-driver chaining: methods like
                // `db.prepare(sql).run()` / `db.prepare(sql).get()` /
                // `db.prepare(sql).all()` where the inner call returns
                // a *new* native class (Statement) — not the same
                // handle as the receiver. Look up `(module,
                // prior_method)` in the chaining table and dispatch
                // the outer call against the resulting class. Without
                // this, the outer `.run()`/`.get()`/`.all()` fell
                // through to the generic js_native_call_method
                // dispatcher: SQL never executed, returned objects
                // had no keys_array, `Object.keys(row)` was `[]` and
                // `row.id` was undefined.
                let chained_class: Option<&'static str> =
                    match (module.as_str(), prior_method.as_str()) {
                        ("better-sqlite3", "prepare") => Some("Statement"),
                        ("sqlite", "prepare") => Some("StatementSync"),
                        ("sqlite", "createTagStore") => Some("SQLTagStore"),
                        ("sqlite", "createSession") => Some("Session"),
                        ("mongodb", "db") => Some("Database"),
                        ("mongodb", "collection") => Some("Collection"),
                        ("mysql2", "getConnection") | ("mysql2/promise", "getConnection") => {
                            Some("PoolConnection")
                        }
                        ("pg", "connect") => Some("PoolClient"),
                        ("ioredis", "duplicate") => Some("Redis"),
                        _ => None,
                    };
                if let Some(result_class) = chained_class {
                    return Ok(Ok(Expr::NativeMethodCall {
                        module: module.clone(),
                        class_name: Some(result_class.to_string()),
                        object: Some(Box::new(object_expr)),
                        method: method_name,
                        args,
                    }));
                }
            }
            // No fluent arm matched: we are about to return `Err(args)` and the
            // `lower_call_inner` fall-through tail will re-lower this same member
            // callee. Hand it the receiver we just lowered so it doesn't repeat
            // the (potentially whole-prefix) work. Keyed by span so the tail only
            // reuses it for the exact receiver subtree.
            ctx.prelowered_member_receiver = Some(((obj_span.lo.0, obj_span.hi.0), object_expr));
        }
    }

    Ok(Err(args))
}

fn may_lower_to_native_method_call(ctx: &LoweringContext, expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::Ident(ident) => ident_may_start_native_method_call(ctx, ident.sym.as_ref()),
        ast::Expr::New(_) => detect_native_instance_expr(ctx, expr).is_some(),
        ast::Expr::Paren(paren) => may_lower_to_native_method_call(ctx, &paren.expr),
        ast::Expr::TsAs(ts_as) => may_lower_to_native_method_call(ctx, &ts_as.expr),
        ast::Expr::TsNonNull(ts_nn) => may_lower_to_native_method_call(ctx, &ts_nn.expr),
        ast::Expr::TsSatisfies(ts_sat) => may_lower_to_native_method_call(ctx, &ts_sat.expr),
        ast::Expr::TsTypeAssertion(ts_ta) => may_lower_to_native_method_call(ctx, &ts_ta.expr),
        ast::Expr::TsConstAssertion(ts_const) => {
            may_lower_to_native_method_call(ctx, &ts_const.expr)
        }
        ast::Expr::Call(call) => {
            if native_class_from_factory_call(ctx, call).is_some() {
                return true;
            }

            let ast::Callee::Expr(callee_expr) = &call.callee else {
                return false;
            };
            let ast::Expr::Member(member) = callee_expr.as_ref() else {
                return false;
            };

            match member.obj.as_ref() {
                ast::Expr::Ident(ident) => {
                    ident_may_start_native_method_call(ctx, ident.sym.as_ref())
                }
                object => {
                    detect_native_instance_expr(ctx, object).is_some()
                        || may_lower_to_native_method_call(ctx, object)
                }
            }
        }
        _ => false,
    }
}

fn ident_may_start_native_method_call(ctx: &LoweringContext, name: &str) -> bool {
    ctx.lookup_native_instance(name).is_some() || ctx.lookup_native_module(name).is_some()
}

/// Resolve the native `(module, class)` produced by an inline factory call
/// like `createServer(...)` / `http.createServer(...)` /
/// `http2.createSecureServer(...)`, so a method chained directly on the result
/// (`createServer(...).listen(...)`) can dispatch against the right
/// NativeModSig `class_filter`. Mirrors the variable-binding factory maps in
/// `module_decl.rs` / `destructuring/var_decl.rs` / `lower/stmt.rs`. Returns
/// `None` for any other call so non-factory chains fall through unchanged.
/// Issue #2041.
fn native_class_from_factory_call(
    ctx: &LoweringContext,
    call: &ast::CallExpr,
) -> Option<(&'static str, &'static str)> {
    let callee_expr = match &call.callee {
        ast::Callee::Expr(e) => e.as_ref(),
        _ => return None,
    };
    // Resolve to `(module, method)`, handling both the named-import form
    // (`createServer(...)`) and the namespace form (`http.createServer(...)`).
    let (module, method): (String, String) = match callee_expr {
        ast::Expr::Member(member) => {
            let obj_ident = match member.obj.as_ref() {
                ast::Expr::Ident(i) => i,
                _ => return None,
            };
            let (module, _) = ctx.lookup_native_module(obj_ident.sym.as_ref())?;
            let method = match &member.prop {
                ast::MemberProp::Ident(i) => i.sym.to_string(),
                _ => return None,
            };
            (module.to_string(), method)
        }
        ast::Expr::Ident(ident) => {
            let (module, method_opt) = ctx.lookup_native_module(ident.sym.as_ref())?;
            (module.to_string(), method_opt?.to_string())
        }
        _ => return None,
    };
    match (module.as_str(), method.as_str()) {
        ("http", "createServer") => Some(("http", "HttpServer")),
        ("https", "createServer") => Some(("https", "HttpsServer")),
        ("http2", "createSecureServer") => Some(("http2", "Http2SecureServer")),
        ("tls", "createServer") | ("tls", "Server") => Some(("tls", "Server")),
        // Issue #2208: `http.request(...).on(...)` / `https.get(...).on(...)`
        // chains — the inline factory call returns a `ClientRequest` whose
        // instance methods (`on`/`end`/`write`/`setHeader`/`setTimeout`) are
        // registered under module `"http"` for both schemes (see
        // `lower_call/native_table/http.rs`). Without these arms the chained
        // `.on(...)` fell through to the generic typed-feedback dispatch,
        // which has no `ClientRequest` arm and returned NaN; each subsequent
        // chain step then dereffed NaN as a number ("(number).on is not a
        // function"). Mirrors the createServer entry above.
        ("http", "request") | ("http", "get") => Some(("http", "ClientRequest")),
        ("https", "request") | ("https", "get") => Some(("http", "ClientRequest")),
        _ => None,
    }
}
