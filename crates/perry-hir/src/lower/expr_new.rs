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

use super::expr_new_builtins::{global_member_constructor_name, module_constructor_name};
use super::{lower_expr, LoweringContext};

mod helpers;
mod member;
mod non_ident;

pub(crate) use helpers::{
    callee_is_generic_construct_shape, collect_const_string_parts, is_depd_wrapfunction_shape,
    is_fetch_constructor_name, is_global_object_expr, is_url_encoding_constructor_name,
    is_worker_messaging_constructor_name, is_worker_threads_module_name, lower_new_spread_args,
    lower_optional_args, lower_text_decoder_new, lower_url_encoding_constructor,
    lower_worker_messaging_new, lower_worker_new, nonconstructable_builtin_throw_expr,
    peel_new_callee,
};
pub(crate) use member::lower_new_member_native;
pub(crate) use non_ident::{lower_new_non_ident, register_stream_controller_params};

pub(super) fn lower_new(ctx: &mut LoweringContext, new_expr: &ast::NewExpr) -> Result<Expr> {
    let callee_expr = peel_new_callee(new_expr.callee.as_ref());
    // #5253: source byte offset of this `new` expression, captured once and
    // threaded into every `New`/`NewDynamic`/`NewDynamicSpread` we build below.
    // Under `--debug-symbols`, codegen resolves it to a `file:line` for the
    // runtime "X is not a constructor" TypeError. Mirrors `Call.byte_offset`
    // (#5247). `_byte_offset` because not every arm below builds a New variant.
    let new_byte_offset = new_expr.span.lo.0;

    // `new <callee>(...args)` — spread arguments. Every per-constructor branch
    // below collapses spreads into a plain array argument (they map over
    // `a.expr` and drop `a.spread`), so `new f(...[1,2])` would pass a single
    // array instead of two arguments. When any argument is a spread AND the
    // callee is a generic constructable shape (function/class expression, IIFE,
    // arrow, or a user-class identifier), route through `NewDynamicSpread` so
    // the spread positions survive lowering. Built-in/native special
    // constructors (URL, TypedArray, net.Socket, …) keep their existing
    // behavior — calling those with a spread argument is vanishingly rare and
    // already unsupported.
    if let Some(args_ast) = new_expr.args.as_deref() {
        if args_ast.iter().any(|a| a.spread.is_some())
            && callee_is_generic_construct_shape(ctx, callee_expr)
        {
            let callee = lower_expr(ctx, callee_expr)?;
            let args = lower_new_spread_args(ctx, args_ast)?;
            return Ok(Expr::NewDynamicSpread {
                callee: Box::new(callee),
                args,
                byte_offset: new_byte_offset,
            });
        }
    }

    if let ast::Expr::Ident(callee_ident) = callee_expr {
        let is_module_constructor = ctx
            .lookup_native_module(callee_ident.sym.as_ref())
            .map(|(module_name, method)| {
                module_name == "module"
                    && matches!(method.as_deref(), Some("Module") | Some("default"))
            })
            .unwrap_or(false);
        if is_module_constructor {
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
                method: "Module".to_string(),
                args,
            });
        }
        // #4995: `new EE()` where `EE` is the events module *value* — the
        // default import (`import EE from 'events'`) or a CJS alias
        // (`var EE = require('events')`). Node's `events` module exports the
        // EventEmitter class itself, so construct it exactly like the named
        // import (`Expr::New { class_name: "EventEmitter" }` → codegen's
        // lower_builtin_new → `js_event_emitter_new_with_options`).
        // Previously this fell through to `New { class_name: "EE" }`, which
        // codegen resolved to the empty-object placeholder — instances had no
        // `.on`/`.emit`/`.setMaxListeners`, so signal-exit's module init
        // threw and blocked ink (#348).
        if ctx.lookup_local(callee_ident.sym.as_ref()).is_none() {
            let is_events_module_value = ctx
                .lookup_native_module(callee_ident.sym.as_ref())
                .map(|(module_name, method)| {
                    module_name == "events"
                        && (method.is_none() || method.as_deref() == Some("default"))
                })
                .unwrap_or(false)
                || ctx.lookup_builtin_module_alias(callee_ident.sym.as_ref()) == Some("events");
            if is_events_module_value {
                return Ok(Expr::New {
                    class_name: "EventEmitter".to_string(),
                    args: lower_optional_args(ctx, new_expr.args.as_deref())?,
                    type_args: Vec::new(),
                    byte_offset: new_byte_offset,
                });
            }
        }
    }

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
    if let Some(expr) = lower_new_member_native(ctx, new_expr, callee_expr, new_byte_offset)? {
        return Ok(expr);
    }

    // Issue #237: pre-register the controller param of every
    // `start` / `pull` / `cancel` / `transform` / `flush` callback passed to
    // `new ReadableStream({...})` / `new TransformStream({...})` as a native
    // instance so `controller.enqueue(...)` etc. dispatch through the streams
    // arms in lower_call.rs.
    register_stream_controller_params(ctx, new_expr);

    // Try to extract class name from callee
    match callee_expr {
        ast::Expr::Ident(ident) => {
            // Resolve through any scope-local class rename so `new X` binds to
            // the lexically-correct (possibly disambiguated) class.
            let mut class_name = ctx.resolve_class_name(ident.sym.as_str());
            // Snapshot the callee identifier's local/param binding at the TOP
            // of the ident arm, before any argument lowering or native-module
            // probing below runs. Two distinct hazards make a later lookup
            // unreliable, and both surface as the same bug — `new C(args)`
            // where `C` is a function parameter (the `function _f(Class,
            // params) { return new Class(params); }` factory shape that
            // zod-style libraries use heavily) silently falling through to an
            // empty-object `Expr::New { class_name }` placeholder whose
            // constructor body never runs, so `Object.defineProperty(this, …)`
            // / `this.x = …` writes are lost and the constructed value is
            // missing all its prototype methods:
            //
            //   1. Lowering an argument expression (e.g. a spread object
            //      literal `new C({ ...f(x) })`, which lowers to a synthesized
            //      IIFE closure) opens and closes nested lexical scopes;
            //      `exit_scope` truncates the locals stack and can drop the
            //      enclosing function's OWN parameters from view, so a later
            //      `lookup_local` misses the param.
            //
            //   2. A local/param `C` lexically shadows a same-named outer
            //      `const C = class {}` alias, but `let_class_aliases` is
            //      name-keyed and NOT scope-aware, so `resolve_class_alias`
            //      returns the stale enclosing-scope alias even though the
            //      param shadows it — and the `resolve_class_alias().is_none()`
            //      guard on the reroute block below would then skip it.
            //
            // Capturing the binding here (when no real class of this name is in
            // scope) keeps the reroute stable against both.
            let callee_local_at_entry: Option<LocalId> = if ctx.lookup_class(&class_name).is_none()
            {
                ctx.lookup_local(&class_name)
            } else {
                None
            };
            // #6233: a user-declared binding — `class Symbol extends Base {}`,
            // a local/param, a `function` declaration, or an imported binding —
            // lexically shadows the same-named global for every reference in
            // scope, `new` expressions included. Every built-in constructor arm
            // below (Map/Set/Date/RegExp, the Symbol/BigInt/Math/JSON
            // non-constructible rejection, Proxy, the boxed primitives, the
            // Error family, WeakRef, FinalizationRegistry, AggregateError,
            // typed arrays, …) must back off when the name is shadowed so the
            // construct falls through to the user-class / local-dispatch paths
            // at the bottom. Snapshotted ONCE here, next to
            // `callee_local_at_entry`, for the same reason it is: argument
            // lowering inside an arm can disturb the locals scope stack, so a
            // fresh lookup later is unreliable. `forward_class_names` covers a
            // sibling `class X` declared later in the same function body
            // (pre-registered by the Phase-1.5 scan but not yet lowered).
            let shadowed_by_user_binding = ctx.lookup_class(&class_name).is_some()
                || callee_local_at_entry.is_some()
                || ctx.lookup_func(&class_name).is_some()
                || ctx.lookup_imported_func(&class_name).is_some()
                || ctx.forward_class_names.contains(class_name.as_str());
            if matches!(
                ctx.lookup_native_module(&class_name),
                Some(("url", Some("Url")))
            ) {
                return Ok(Expr::NativeMethodCall {
                    module: "url".to_string(),
                    class_name: None,
                    object: None,
                    method: "Url".to_string(),
                    args: Vec::new(),
                });
            }

            let crypto_constructor_export =
                ctx.lookup_native_module(&class_name)
                    .and_then(|(module_name, export_name)| {
                        if matches!(module_name, "crypto" | "node:crypto")
                            && matches!(export_name, Some("DiffieHellman" | "DiffieHellmanGroup"))
                        {
                            export_name.map(str::to_string)
                        } else {
                            None
                        }
                    });
            if let Some(method_name) = crypto_constructor_export {
                let args = lower_optional_args(ctx, new_expr.args.as_deref())?;
                return Ok(Expr::Call {
                    callee: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::NativeModuleRef("crypto".to_string())),
                        property: method_name,
                    }),
                    args,
                    type_args: Vec::new(),
                    byte_offset: 0,
                });
            }

            // #3157: `import { MessageChannel } from "worker_threads"` then
            // `new MessageChannel()` — the bare-ident form must route to the
            // same receiver-less worker_threads NativeMethodCall as the
            // `new worker_threads.MessageChannel()` member form above, so the
            // runtime `js_worker_threads_message_channel_new` allocates the
            // real `{ port1, port2 }` object. Without this it falls through to
            // the user-class `Expr::New` path and gets an empty object.
            if matches!(
                ctx.lookup_native_module(&class_name),
                Some(("worker_threads", Some("MessageChannel")))
                    | Some(("worker_threads", Some("BroadcastChannel")))
            ) {
                return lower_worker_messaging_new(ctx, &class_name, new_expr.args.as_deref());
            }

            // #4873: bare `new MessageChannel()` / `new BroadcastChannel()`
            // with NO worker_threads import is the *global* constructor form
            // (React's scheduler feature-detects exactly this way). Lower as
            // `Expr::New` so codegen's `lower_builtin_new` emits the
            // always-linked `js_message_channel_new` /
            // `js_broadcast_channel_new` (perry-runtime). The previous
            // worker_threads NativeMethodCall routing referenced the
            // stdlib-only `js_worker_threads_*_new` symbols, which fail to
            // link unless something else pulls in `node:worker_threads`. The
            // runtime globals delegate to the registered worker_threads
            // factories when the stdlib is present, so ports stay fully
            // functional in graphs that have it.
            if is_worker_messaging_constructor_name(&class_name) && !shadowed_by_user_binding {
                return Ok(Expr::New {
                    class_name: class_name.to_string(),
                    args: lower_optional_args(ctx, new_expr.args.as_deref())?,
                    type_args: Vec::new(),
                    byte_offset: new_byte_offset,
                });
            }

            let inspector_session_module = ctx.lookup_native_module(&class_name).and_then(
                |(module_name, export_name)| match (module_name, export_name) {
                    ("inspector" | "inspector/promises", Some("Session")) => {
                        Some(module_name.to_string())
                    }
                    _ => None,
                },
            );
            if let Some(module_name) = inspector_session_module {
                let args = lower_optional_args(ctx, new_expr.args.as_deref())?;
                return Ok(Expr::NativeMethodCall {
                    module: module_name,
                    class_name: None,
                    object: None,
                    method: "Session".to_string(),
                    args,
                });
            }

            let repl_constructor = ctx.lookup_native_module(&class_name).and_then(
                |(module_name, export_name)| match (module_name, export_name) {
                    ("repl", Some("Recoverable" | "REPLServer")) => {
                        export_name.map(|name| (module_name.to_string(), name.to_string()))
                    }
                    _ => None,
                },
            );
            if let Some((module_name, method_name)) = repl_constructor {
                let args = lower_optional_args(ctx, new_expr.args.as_deref())?;
                return Ok(Expr::NewDynamic {
                    callee: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::NativeModuleRef(module_name)),
                        property: method_name,
                    }),
                    args,
                    byte_offset: new_byte_offset,
                });
            }

            // #4904: bare-ident construction of the http classes —
            // `const { ClientRequest } = require('http'); new
            // ClientRequest(...)` (also IncomingMessage / ServerResponse,
            // joining the existing OutgoingMessage route).
            let http_class_export =
                ctx.lookup_native_module(&class_name)
                    .and_then(|(module, export)| match (module, export) {
                        (
                            "http",
                            Some(
                                x @ ("OutgoingMessage" | "ClientRequest" | "IncomingMessage"
                                | "ServerResponse"),
                            ),
                        ) => Some(x.to_string()),
                        _ => None,
                    });
            if let Some(export) = http_class_export {
                let args = lower_optional_args(ctx, new_expr.args.as_deref())?;
                return Ok(Expr::NewDynamic {
                    callee: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::NativeModuleRef("http".to_string())),
                        property: export,
                    }),
                    args,
                    byte_offset: new_byte_offset,
                });
            }

            // #4904: bare-ident `new Agent(opts)` where Agent came from
            // `require('http')` / `require('https')` (named import,
            // destructure, or member alias). Route to the same
            // receiver-less NativeMethodCall as the `new http.Agent()`
            // member form so the dispatch row runs `js_*_agent_new` and the
            // let-stmt machinery tags the local for Agent method dispatch.
            let http_agent_module =
                ctx.lookup_native_module(&class_name)
                    .and_then(|(module, export)| match (module, export) {
                        (m @ ("http" | "https"), Some("Agent")) => Some(m.to_string()),
                        _ => None,
                    });
            if let Some(agent_module) = http_agent_module {
                let args = lower_optional_args(ctx, new_expr.args.as_deref())?;
                return Ok(Expr::NativeMethodCall {
                    module: agent_module,
                    class_name: None,
                    object: None,
                    method: "Agent".to_string(),
                    args,
                });
            }

            if matches!(
                ctx.lookup_native_module(&class_name),
                Some(("v8", Some("GCProfiler")))
            ) {
                let args = lower_optional_args(ctx, new_expr.args.as_deref())?;
                return Ok(Expr::NewDynamic {
                    callee: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::NativeModuleRef("v8".to_string())),
                        property: "GCProfiler".to_string(),
                    }),
                    args,
                    byte_offset: new_byte_offset,
                });
            }

            if matches!(class_name.as_str(), "MIMEType" | "MIMEParams") {
                if let Some((module_name, Some(method_name))) =
                    ctx.lookup_native_module(&class_name)
                {
                    if matches!(module_name, "util" | "sys")
                        && matches!(method_name, "MIMEType" | "MIMEParams")
                    {
                        let module_name = module_name.to_string();
                        let method_name = method_name.to_string();
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
                            method: method_name,
                            args,
                        });
                    }
                }
            }

            if let Some((module_name, method_name)) = ctx.lookup_native_module(&class_name) {
                if matches!((module_name, method_name), ("module", Some("Module"))) {
                    let args = lower_optional_args(ctx, new_expr.args.as_deref())?;
                    return Ok(Expr::NativeMethodCall {
                        module: "module".to_string(),
                        class_name: None,
                        object: None,
                        method: "Module".to_string(),
                        args,
                    });
                }
                if let Some(class_name) = module_constructor_name(module_name, method_name) {
                    if let Some(expr) =
                        lower_url_encoding_constructor(ctx, class_name, new_expr.args.as_deref())?
                    {
                        return Ok(expr);
                    }
                }
            }

            // Issue #5912 (CodeRabbit follow-up): an alias like
            // `const MyURL = URL; new MyURL()` must not bind to the native
            // constructor either when the ALIASED name is itself shadowed
            // (`function URL(url) {...}` in scope) — `resolve_class_alias`
            // is name-keyed and not scope-aware, so it happily maps
            // `MyURL` -> `"URL"` without knowing `URL` was ever shadowed.
            if let Some(resolved) = ctx.resolve_class_alias(&class_name) {
                if is_url_encoding_constructor_name(&resolved)
                    && !ctx.shadows_unqualified_global(&resolved)
                {
                    if let Some(expr) =
                        lower_url_encoding_constructor(ctx, &resolved, new_expr.args.as_deref())?
                    {
                        return Ok(expr);
                    }
                }
            }

            if class_name == "Worker"
                && ctx
                    .lookup_native_module("Worker")
                    .map(|(module_name, export_name)| {
                        is_worker_threads_module_name(module_name) && export_name == Some("Worker")
                    })
                    .unwrap_or(false)
            {
                return lower_worker_new(ctx, new_expr);
            }

            // #1677 `new Function(...)` handling, when `Function` is not
            // shadowed. Phase 1 (#1679) first: when every argument is a
            // compile-time-constant string, fold the call into a real
            // native function. Otherwise Phase 0 (#1678): refuse the
            // runtime-unknown bucket with a precise diagnostic; log the
            // const/known-codegen buckets and fall through to the existing
            // placeholder lowering.
            if class_name == "Function" && !shadowed_by_user_binding {
                let args_slice = new_expr.args.as_deref().unwrap_or(&[]);
                if let Some(folded) = super::const_fold_fn::try_const_fold_function_construct(
                    ctx,
                    args_slice,
                    crate::eval_classifier::EvalSurface::NewFunction,
                    new_expr.span,
                )? {
                    return Ok(folded);
                }
                // depd `wrapfunction` builds its deprecation wrapper with a
                // runtime-constructed body (`'…return function ('+a+') {…'`), so
                // it isn't const-foldable and the classifier would defer it to a
                // throw-on-call value — which Next.js' `send` invokes eagerly at
                // startup (`new Function(…)(fn,…)`), crashing before `✓ Ready`.
                // The runtime `js_function_ctor_from_strings` recognizes this
                // exact template and returns the wrapped fn (the deprecation log
                // is a non-essential warning), so PROCEED to the codegen
                // `Expr::New { "Function" }` path for it instead of deferring.
                // Any other runtime-unknown body still defers. NO general eval.
                if is_depd_wrapfunction_shape(args_slice) {
                    // fall through to `Expr::New { class_name: "Function" }`.
                } else {
                    // Not fully const-foldable — body is the last argument
                    // (`new Function(p1, p2, body)`); earlier args are param names.
                    let body_arg = args_slice.last().map(|a| a.expr.as_ref());
                    match crate::eval_classifier::check_site(
                        crate::eval_classifier::EvalSurface::NewFunction,
                        body_arg,
                        &ctx.source_file_path,
                        new_expr.span,
                    )? {
                        crate::eval_classifier::EvalDecision::Proceed => {}
                        // #5206: default (defer) mode — compile to a function value
                        // that throws a descriptive Error only when invoked.
                        crate::eval_classifier::EvalDecision::DeferToRuntimeError(message) => {
                            return super::const_fold_fn::synth_deferred_eval_value(
                                ctx,
                                crate::eval_classifier::EvalSurface::NewFunction,
                                &message,
                                new_expr.span,
                            );
                        }
                    }
                }
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
                    | "FormData"
                    | "Blob"
                    | "File"
                    | "ReadableStream"
                    | "ReadableStreamBYOBReader"
                    | "WritableStream"
                    | "TransformStream"
            ) {
                ctx.uses_fetch = true;
            }

            // Handle built-in types
            if class_name == "Object" && !shadowed_by_user_binding {
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
                let arg = args.drain(..).next().unwrap_or(Expr::Undefined);
                return Ok(Expr::ObjectCoerce(Box::new(arg)));
            }
            if class_name == "Map" && !shadowed_by_user_binding {
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
            if class_name == "Set" && !shadowed_by_user_binding {
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
            if class_name == "Date" && !shadowed_by_user_binding {
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
            if class_name == "RegExp" && !shadowed_by_user_binding {
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
                // Only take the constant-folded literal path when the flags
                // argument is absent or itself a string literal. If a flags
                // argument is present but NOT a string literal (e.g. an object
                // `{ toString() {…} }`, a variable, or a number), it must be
                // `ToString`-coerced at runtime — and a throwing `toString`
                // must propagate — so fall through to `RegExpDynamic`. Folding
                // those to `Expr::RegExp` here silently dropped the flags.
                let flags_arg_is_string_literal_or_absent = match args_ast {
                    Some(args) => match args.get(1) {
                        None => true,
                        Some(a) => matches!(a.expr.as_ref(), ast::Expr::Lit(ast::Lit::Str(_))),
                    },
                    None => true,
                };
                if let Some(pattern) = pattern_lit {
                    if flags_arg_is_string_literal_or_absent {
                        return Ok(Expr::RegExp {
                            pattern,
                            flags: flags_lit,
                        });
                    }
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
                            // `new RegExp(x)` always constructs a fresh object —
                            // never the identity shortcut (#5586).
                            is_call: false,
                        });
                    }
                }
            }
            if matches!(class_name.as_str(), "Symbol" | "BigInt" | "Math" | "JSON")
                && !shadowed_by_user_binding
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
                return Ok(nonconstructable_builtin_throw_expr(&class_name, args));
            }
            if class_name == "Proxy" && !shadowed_by_user_binding {
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
            if matches!(class_name.as_str(), "Number" | "String" | "Boolean")
                && !shadowed_by_user_binding
            {
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
                // A *present* argument is coerced per spec: `new Number(x)` →
                // ToNumber(x), `new String(x)` → ToString(x). This matters for
                // an explicit `undefined`: `new Number(undefined)` is NaN and
                // `new String(undefined)` is "undefined" — distinct from the
                // *no-arg* forms `new Number()`/`new String()` which box +0/"".
                // `Number`/`Boolean` disambiguate for free: `NumberCoerce`/
                // `BooleanCoerce` always produce a non-`undefined` primitive
                // (NaN / `false`) for a present `undefined` argument. `String`
                // cannot use the same trick — `Expr::StringCoerce`
                // (`js_string_coerce`) is the *lenient* ToString used by the
                // `String(x)` call form, which renders a Symbol as its
                // descriptive string ("Symbol(desc)") instead of throwing
                // (ECMA-262 §22.1.1 step 2b requires the `new String(sym)`
                // TypeError, test262 `symbol-wrapping.js`) — so the argument is
                // passed raw and `js_boxed_string_new` applies `ToString` +
                // the Symbol rejection itself, gated on the explicit
                // `arg_present` flag below rather than an `undefined` sentinel
                // (a present-but-`undefined`-valued argument must still box to
                // `"undefined"`, not the no-arg `""` default).
                let arg_present = !args.is_empty();
                let arg = match args.drain(..).next() {
                    Some(inner) => match kind {
                        crate::BoxedPrimitiveKind::Number => Expr::NumberCoerce(Box::new(inner)),
                        crate::BoxedPrimitiveKind::String => inner,
                        crate::BoxedPrimitiveKind::Boolean => Expr::BooleanCoerce(Box::new(inner)),
                    },
                    None => Expr::Undefined,
                };
                return Ok(Expr::BoxedPrimitiveNew {
                    kind,
                    arg: Box::new(arg),
                    arg_present,
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
            if class_name == "AggregateError" && !shadowed_by_user_binding {
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
            if (class_name == "Error"
                || class_name == "TypeError"
                || class_name == "RangeError"
                || class_name == "ReferenceError"
                || class_name == "SyntaxError"
                || class_name == "BugIndicatingError")
                && !shadowed_by_user_binding
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
                        "TypeError" => Ok(Expr::TypeErrorNew(Box::new(Expr::Undefined))),
                        "RangeError" => Ok(Expr::RangeErrorNew(Box::new(Expr::Undefined))),
                        "ReferenceError" => Ok(Expr::ReferenceErrorNew(Box::new(Expr::Undefined))),
                        "SyntaxError" => Ok(Expr::SyntaxErrorNew(Box::new(Expr::Undefined))),
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

            // Handle URL class. #5912: gated so a local function/const/
            // imported-binding shadowing the global name (e.g. a vendored
            // `function URL(url?) {...}` polyfill, or `import { URL } from
            // "./my-url-polyfill"`) routes through the generic local-dispatch
            // fallback below instead of always binding to perry's native
            // WHATWG URL constructor. Uses the `shadowed_by_user_binding`
            // snapshot (NOT fresh `shadows_unqualified_global` lookups):
            // earlier arms lower `new_expr.args`, which can disturb the
            // locals scope stack before we get here (see the comment above
            // `callee_local_at_entry`).
            if class_name == "URL" && !shadowed_by_user_binding {
                return Ok(
                    lower_url_encoding_constructor(ctx, "URL", new_expr.args.as_deref())?.unwrap(),
                );
            }

            // Handle URLSearchParams / URLPattern classes
            if matches!(class_name.as_str(), "URLSearchParams" | "URLPattern")
                && !shadowed_by_user_binding
            {
                return Ok(lower_url_encoding_constructor(
                    ctx,
                    &class_name,
                    new_expr.args.as_deref(),
                )?
                .unwrap());
            }

            // Handle WeakRef class — wraps a value (object) in a weak reference object.
            // Pragmatic implementation: stores a strong reference and `deref()` always returns it.
            if class_name == "WeakRef" && !shadowed_by_user_binding {
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
            if class_name == "FinalizationRegistry" && !shadowed_by_user_binding {
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
            if class_name == "TextEncoder" && !shadowed_by_user_binding {
                return Ok(lower_url_encoding_constructor(
                    ctx,
                    "TextEncoder",
                    new_expr.args.as_deref(),
                )?
                .unwrap());
            }
            // Handle TextDecoder constructor: new TextDecoder(label?, opts?)
            if class_name == "TextDecoder" && !shadowed_by_user_binding {
                return Ok(lower_url_encoding_constructor(
                    ctx,
                    "TextDecoder",
                    new_expr.args.as_deref(),
                )?
                .unwrap());
            }

            // Handle Uint8Array constructor
            if class_name == "Uint8Array" && !shadowed_by_user_binding {
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
                if class_name != "Uint8Array" && !shadowed_by_user_binding {
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
                    if matches!(
                        resolved.as_str(),
                        "Blob"
                            | "File"
                            | "FormData"
                            | "Headers"
                            | "Request"
                            | "Response"
                            | "WebSocket"
                    ) {
                        if is_fetch_constructor_name(&resolved) {
                            ctx.uses_fetch = true;
                        }
                        return Ok(Expr::New {
                            class_name: resolved,
                            args,
                            type_args,
                            byte_offset: new_byte_offset,
                        });
                    }
                }
            }
            // A local/param binding lexically shadows any same-named outer
            // `let`/`const` class alias. When `callee_local_at_entry` is set
            // (a non-class local was in scope at the top of this arm, before
            // arg lowering could disturb the scope), route the construct
            // through that VALUE — even if `resolve_class_alias` would
            // otherwise resolve `class_name` to a stale enclosing-scope alias
            // (its map is name-keyed, not scope-aware). Without this, the
            // `resolve_class_alias().is_none()` guard on the local-reroute
            // block below is false and the construct falls through to an
            // empty-object `Expr::New { class_name }` placeholder whose
            // constructor body never runs.
            if ctx.lookup_class(&class_name).is_none() {
                if let Some(local_id) = callee_local_at_entry {
                    return Ok(Expr::NewDynamic {
                        callee: Box::new(Expr::LocalGet(local_id)),
                        args,
                        byte_offset: new_byte_offset,
                    });
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
            if ctx.lookup_class(&class_name).is_none()
                && ctx.resolve_class_alias(&class_name).is_none()
            {
                if let Some(local_id) =
                    callee_local_at_entry.or_else(|| ctx.lookup_local(&class_name))
                {
                    return Ok(Expr::NewDynamic {
                        callee: Box::new(Expr::LocalGet(local_id)),
                        args,
                        byte_offset: new_byte_offset,
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
                        byte_offset: new_byte_offset,
                    });
                }
                // #4698: `new <imported-binding>()` where the binding is a
                // function (or a `const`/`let` holding a closure) imported from
                // another module is intentionally NOT rerouted here. At lowering
                // time (single collect_modules pass) an imported class and an
                // imported function are indistinguishable — both are unknown to
                // `lookup_class`/`lookup_func` and both appear in the imported
                // bindings — and the cross-module class-inline machinery in
                // `collect_modules` relies on `new <ImportedClass>()` staying as
                // `Expr::New { class_name }`. Rerouting to `NewDynamic` here
                // broke that (the `dependency_is_transformed_before_importer…`
                // test). Instead, the codegen `lower_new` fallback detects an
                // imported *function/closure* value (a name that is NOT a
                // registered class but IS an imported binding) and constructs it
                // via `js_new_function_construct` — see
                // `perry-codegen/src/lower_call/new.rs`.
            }
            // #wall: an ALIASED named import of a native built-in class
            // (`import { BlockList as Wj4 } from "net"; new Wj4()`) must
            // construct exactly like the un-aliased form. The bare-ident
            // construction below falls through to `Expr::New { class_name }`,
            // and codegen's builtin-`New` dispatch recognizes the class by its
            // LITERAL name ("BlockList", "SocketAddress", "Socket", "Server",
            // "URL", …). Under an alias the local name ("Wj4") misses every
            // arm, so codegen builds an empty placeholder object with no native
            // methods (`q.addSubnet` → "addSubnet is not a function").
            //
            // `lookup_native_module` is already alias-aware: the named-import
            // lowering registers `local → (module, Some(<imported>))`, so a
            // native-class import resolves the alias to its imported export
            // name. Rewrite `class_name` to that export so the alias path is
            // byte-for-byte identical to the un-aliased path. This is a no-op
            // for the un-aliased case (export == local) and only fires for
            // native-module class imports (a user `import { foo as bar }` from
            // a TS module registers as an imported func, NOT a native module,
            // so `lookup_native_module` returns None — no over-trigger). A
            // user class or local of the alias name shadows it (handled by the
            // `lookup_class`/`lookup_local` returns above that precede this).
            if class_name
                .chars()
                .next()
                .map(|c| c.is_uppercase())
                .unwrap_or(false)
            {
                if let Some((_module, Some(export))) = ctx.lookup_native_module(&class_name) {
                    if export != class_name
                        && ctx.lookup_class(&class_name).is_none()
                        && ctx.lookup_local(&class_name).is_none()
                    {
                        class_name = export.to_string();
                    }
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
                byte_offset: new_byte_offset,
            })
        }
        // Non-identifier callee (e.g., new (condition ? A : B)() or new someVar()).
        _ => lower_new_non_ident(ctx, new_expr, callee_expr, new_byte_offset),
    }
}
