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

/// Collect the compile-time-constant string fragments of a `+`-concatenation
/// (or template) expression, skipping any dynamic operands. Used to recognize a
/// runtime-constructed `new Function` body by its constant skeleton.
fn collect_const_string_parts(e: &ast::Expr, out: &mut String) {
    match e {
        ast::Expr::Lit(ast::Lit::Str(s)) => out.push_str(s.value.as_str().unwrap_or("")),
        ast::Expr::Bin(b) if b.op == ast::BinaryOp::Add => {
            collect_const_string_parts(&b.left, out);
            collect_const_string_parts(&b.right, out);
        }
        ast::Expr::Paren(p) => collect_const_string_parts(&p.expr, out),
        ast::Expr::Tpl(t) => {
            for q in &t.quasis {
                out.push_str(q.raw.as_str());
            }
        }
        // Dynamic operand (an identifier, call, etc.) — skip it.
        _ => {}
    }
}

/// Recognize depd's `wrapfunction` deprecation-wrapper shape:
/// `new Function("fn","log","deprecate","message","site",
///   '"use strict"\n'+"return function ("+a+") {"+
///   "log.call(deprecate, message, site)\n"+"return fn.apply(this, arguments)\n"+"}")`.
/// The five param-name args are constant string literals; only the body
/// (last arg) is runtime-constructed. The runtime `js_function_ctor_from_strings`
/// re-verifies the full template and returns the wrapped fn, so matching here
/// lets the site proceed to that recognizer instead of being deferred to a
/// throw-on-call value (which `send` invokes eagerly at Next.js startup).
fn is_depd_wrapfunction_shape(args: &[ast::ExprOrSpread]) -> bool {
    if args.len() != 6 {
        return false;
    }
    const PARAM_NAMES: [&str; 5] = ["fn", "log", "deprecate", "message", "site"];
    for (i, name) in PARAM_NAMES.iter().enumerate() {
        if args[i].spread.is_some() {
            return false;
        }
        match crate::eval_classifier::const_string_of(&args[i].expr) {
            Some(s) if s == *name => {}
            _ => return false,
        }
    }
    if args[5].spread.is_some() {
        return false;
    }
    let mut body = String::new();
    collect_const_string_parts(&args[5].expr, &mut body);
    body.contains("return function (")
        && body.contains("log.call(deprecate, message, site)")
        && body.contains("return fn.apply(this, arguments)")
}

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
        byte_offset: 0,
    };

    if args.is_empty() {
        throw_expr
    } else {
        args.push(throw_expr);
        Expr::Sequence(args)
    }
}

fn lower_optional_args(
    ctx: &mut LoweringContext,
    args: Option<&[ast::ExprOrSpread]>,
) -> Result<Vec<Expr>> {
    args.map(|args| {
        args.iter()
            .map(|a| lower_expr(ctx, &a.expr))
            .collect::<Result<Vec<_>>>()
    })
    .transpose()
    .map(|args| args.unwrap_or_default())
}

/// Lower a `new` argument list preserving spread positions as
/// `CallArg::Spread`, for the `NewDynamicSpread` path.
fn lower_new_spread_args(
    ctx: &mut LoweringContext,
    args: &[ast::ExprOrSpread],
) -> Result<Vec<crate::ir::CallArg>> {
    use crate::ir::CallArg;
    args.iter()
        .map(|a| {
            let e = lower_expr(ctx, &a.expr)?;
            Ok(if a.spread.is_some() {
                CallArg::Spread(e)
            } else {
                CallArg::Expr(e)
            })
        })
        .collect()
}

/// Whether a `new` callee is a generic constructable shape that the
/// `NewDynamicSpread` path can handle: a function/class expression, an IIFE
/// (`new (function(){…})()`), or an arrow (constructing one is a `TypeError` —
/// the runtime reports it). Bare-identifier callees (user classes, native
/// module constructors, built-ins) are intentionally excluded — they keep their
/// dedicated per-constructor lowering, whose argument marshalling (rest
/// parameters, default values, …) the generic construct helper does not
/// replicate. `callee` must already be peeled (see `peel_new_callee`).
fn callee_is_generic_construct_shape(ctx: &LoweringContext, callee: &ast::Expr) -> bool {
    // A bare-identifier callee that resolves to a *local* binding (a parameter
    // or `let`/`const` holding a runtime constructor value, e.g. test262's
    // `checkSubclassingIgnored`'s `new construct(...constructArgs)`) has no
    // dedicated per-constructor lowering — it falls through to the generic
    // construct path, which otherwise collapses a spread into one array arg.
    // Route it through `NewDynamicSpread`. Top-level class/function names keep
    // their dedicated lowering (they aren't local bindings).
    if let ast::Expr::Ident(ident) = callee {
        if ctx.lookup_local(ident.sym.as_ref()).is_some() {
            return true;
        }
    }
    matches!(
        callee,
        ast::Expr::Fn(_)
            | ast::Expr::Class(_)
            | ast::Expr::Arrow(_)
            | ast::Expr::Call(_)
            // Member-expression callees (`new Temporal.Duration(...args)`,
            // `new ns.Ctor(...args)`) also route through the generic
            // construct path, whose argument lowering otherwise collapses a
            // spread into a single array argument. The handful of specially
            // lowered member constructors (URL, TextEncoder, …) are never
            // invoked with a spread in practice.
            | ast::Expr::Member(_)
    )
}

fn lower_url_encoding_constructor(
    ctx: &mut LoweringContext,
    class_name: &str,
    args: Option<&[ast::ExprOrSpread]>,
) -> Result<Option<Expr>> {
    match class_name {
        "URL" => {
            let args = lower_optional_args(ctx, args)?;
            let mut args_iter = args.into_iter();
            let url_arg = args_iter
                .next()
                .ok_or_else(|| anyhow!("URL constructor requires at least 1 argument"))?;
            let base_arg = args_iter.next();
            Ok(Some(Expr::UrlNew {
                url: Box::new(url_arg),
                base: base_arg.map(Box::new),
            }))
        }
        "URLSearchParams" => {
            let args = lower_optional_args(ctx, args)?;
            let init_arg = args.into_iter().next();
            Ok(Some(Expr::UrlSearchParamsNew(init_arg.map(Box::new))))
        }
        "URLPattern" => {
            let args = lower_optional_args(ctx, args)?;
            let mut args_iter = args.into_iter();
            let input = args_iter.next().unwrap_or(Expr::Undefined);
            let base = args_iter.next();
            Ok(Some(Expr::UrlPatternNew {
                input: Box::new(input),
                base: base.map(Box::new),
            }))
        }
        "TextEncoder" => Ok(Some(Expr::TextEncoderNew)),
        "TextDecoder" => Ok(Some(lower_text_decoder_new(ctx, args)?)),
        _ => Ok(None),
    }
}

fn is_url_encoding_constructor_name(name: &str) -> bool {
    matches!(
        name,
        "URL" | "URLSearchParams" | "URLPattern" | "TextEncoder" | "TextDecoder"
    )
}

fn is_worker_messaging_constructor_name(name: &str) -> bool {
    matches!(name, "MessageChannel" | "BroadcastChannel")
}

fn lower_worker_messaging_new(
    ctx: &mut LoweringContext,
    class_name: &str,
    args: Option<&[ast::ExprOrSpread]>,
) -> Result<Expr> {
    Ok(Expr::NativeMethodCall {
        module: "worker_threads".to_string(),
        class_name: None,
        object: None,
        method: class_name.to_string(),
        args: lower_optional_args(ctx, args)?,
    })
}

fn lower_worker_new(ctx: &mut LoweringContext, new_expr: &ast::NewExpr) -> Result<Expr> {
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
    let mut args = args.into_iter();
    let filename = args.next().unwrap_or(Expr::Undefined);
    let options = args.next().map(Box::new);
    Ok(Expr::WorkerNew {
        paths: Vec::new(),
        filename: Box::new(filename),
        options,
    })
}

fn is_worker_threads_module_name(module_name: &str) -> bool {
    module_name == "worker_threads" || module_name == "node:worker_threads"
}

fn is_fetch_constructor_name(name: &str) -> bool {
    matches!(
        name,
        "Blob" | "File" | "FormData" | "Headers" | "Request" | "Response"
    )
}

fn is_global_object_expr(ctx: &LoweringContext, expr: &Expr) -> bool {
    match expr {
        Expr::GlobalGet(_) => true,
        Expr::LocalGet(id) => ctx.global_this_aliases.contains(id),
        Expr::PropertyGet { object, property } => {
            property == "globalThis" && matches!(object.as_ref(), Expr::GlobalGet(_))
        }
        _ => false,
    }
}

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
    if let ast::Expr::Member(member) = callee_expr {
        if let (ast::Expr::Ident(obj_ident), ast::MemberProp::Ident(prop_ident)) =
            (peel_new_callee(member.obj.as_ref()), &member.prop)
        {
            let obj_name = obj_ident.sym.as_ref();
            if let Some(class_name) =
                global_member_constructor_name(ctx, obj_name, prop_ident.sym.as_ref())
            {
                // #4873: the *global* `new globalThis.MessageChannel()` /
                // `BroadcastChannel` forms must lower as `Expr::New` so codegen
                // emits the always-linked runtime constructors
                // (`js_message_channel_new` / `js_broadcast_channel_new`,
                // perry-runtime). Routing them to the worker_threads
                // NativeMethodCall left an undefined
                // `js_worker_threads_message_channel_new` symbol in binaries
                // that never import `node:worker_threads`. The runtime global
                // delegates to the full worker_threads factory whenever the
                // stdlib has registered it, so no behavior is lost.
                if is_worker_messaging_constructor_name(class_name) {
                    return Ok(Expr::New {
                        class_name: class_name.to_string(),
                        args: lower_optional_args(ctx, new_expr.args.as_deref())?,
                        type_args: Vec::new(),
                        byte_offset: new_byte_offset,
                    });
                }
                if let Some(expr) =
                    lower_url_encoding_constructor(ctx, class_name, new_expr.args.as_deref())?
                {
                    return Ok(expr);
                }
            }
            if obj_name == "globalThis"
                && ctx.lookup_local("globalThis").is_none()
                && is_fetch_constructor_name(prop_ident.sym.as_ref())
            {
                ctx.uses_fetch = true;
                return Ok(Expr::New {
                    class_name: prop_ident.sym.to_string(),
                    args: lower_optional_args(ctx, new_expr.args.as_deref())?,
                    type_args: Vec::new(),
                    byte_offset: new_byte_offset,
                });
            }

            let is_net_module =
                obj_name == "net" || ctx.lookup_builtin_module_alias(obj_name) == Some("net");
            if is_net_module
                && matches!(
                    prop_ident.sym.as_ref(),
                    "Socket" | "Stream" | "Server" | "BlockList" | "SocketAddress"
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
                let method = if prop_ident.sym.as_ref() == "Stream" {
                    "Socket"
                } else {
                    prop_ident.sym.as_ref()
                };
                return Ok(Expr::NativeMethodCall {
                    module: "net".to_string(),
                    class_name: None,
                    object: None,
                    method: method.to_string(),
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
            // #4904: `new http.ClientRequest(opts)` / `new
            // http.IncomingMessage(socket)` / `new http.ServerResponse(req)`
            // join the OutgoingMessage route: NewDynamic over the module
            // export value, which `js_new_function_construct` forwards to the
            // stdlib http dispatcher. Instances stay dynamically dispatched
            // (HANDLE_*_DISPATCH), matching OutgoingMessage.
            if is_http_module
                && matches!(
                    prop_ident.sym.as_ref(),
                    "OutgoingMessage" | "ClientRequest" | "IncomingMessage" | "ServerResponse"
                )
            {
                let args = lower_optional_args(ctx, new_expr.args.as_deref())?;
                return Ok(Expr::NewDynamic {
                    callee: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::NativeModuleRef("http".to_string())),
                        property: prop_ident.sym.to_string(),
                    }),
                    args,
                    byte_offset: new_byte_offset,
                });
            }
            let is_url_module =
                obj_name == "url" || ctx.lookup_builtin_module_alias(obj_name) == Some("url");
            if is_url_module && prop_ident.sym.as_ref() == "Url" {
                return Ok(Expr::NativeMethodCall {
                    module: "url".to_string(),
                    class_name: None,
                    object: None,
                    method: "Url".to_string(),
                    args: Vec::new(),
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
                            if matches!(module_name, "dns" | "dns/promises")
                                && (method.is_none() || method.as_deref() == Some("default"))
                            {
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
            if is_module_module && matches!(prop_ident.sym.as_ref(), "Module" | "SourceMap") {
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
                    method: prop_ident.sym.to_string(),
                    args,
                });
            }
            let is_vm_module = obj_name == "vm"
                || ctx.lookup_builtin_module_alias(obj_name) == Some("vm")
                || ctx
                    .lookup_native_module(obj_name)
                    .map(|(module_name, method)| {
                        module_name == "vm"
                            && (method.is_none() || method.as_deref() == Some("default"))
                    })
                    .unwrap_or(false);
            if is_vm_module
                && matches!(
                    prop_ident.sym.as_ref(),
                    "SourceTextModule" | "SyntheticModule"
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
                    module: "vm".to_string(),
                    class_name: None,
                    object: None,
                    method: prop_ident.sym.to_string(),
                    args,
                });
            }
            if is_vm_module && prop_ident.sym.as_ref() == "Module" {
                let mut exprs = new_expr
                    .args
                    .as_ref()
                    .map(|args| {
                        args.iter()
                            .map(|a| lower_expr(ctx, &a.expr))
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                exprs.push(Expr::Call {
                    callee: Box::new(Expr::ExternFuncRef {
                        name: "js_vm_module_constructor_error".to_string(),
                        param_types: Vec::new(),
                        return_type: Type::Any,
                    }),
                    args: Vec::new(),
                    type_args: Vec::new(),
                    byte_offset: 0,
                });
                return Ok(Expr::Sequence(exprs));
            }
            if obj_name == "WebAssembly" && prop_ident.sym.as_ref() == "Module" {
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
                if let Some(bytes) = args.into_iter().next() {
                    ctx.uses_webassembly = true;
                    return Ok(Expr::WebAssemblyModuleNew(Box::new(bytes)));
                }
            }
            let is_util_module = obj_name == "util"
                || obj_name == "sys"
                || ctx.lookup_builtin_module_alias(obj_name) == Some("util")
                || ctx.lookup_builtin_module_alias(obj_name) == Some("sys")
                || ctx
                    .lookup_native_module(obj_name)
                    .map(|(module_name, method)| {
                        method.is_none() && matches!(module_name, "util" | "sys")
                    })
                    .unwrap_or(false);
            if is_util_module && matches!(prop_ident.sym.as_ref(), "MIMEType" | "MIMEParams") {
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
                    module: if obj_name == "sys"
                        || ctx.lookup_builtin_module_alias(obj_name) == Some("sys")
                    {
                        "sys".to_string()
                    } else {
                        "util".to_string()
                    },
                    class_name: None,
                    object: None,
                    method: prop_ident.sym.to_string(),
                    args,
                });
            }
            let module_alias = obj_ident.sym.as_ref();
            let is_worker_threads_module = module_alias == "worker_threads"
                || ctx.lookup_builtin_module_alias(module_alias) == Some("worker_threads")
                || match ctx.lookup_native_module(module_alias) {
                    Some((module_name, _)) => is_worker_threads_module_name(module_name),
                    None => false,
                };
            if is_worker_threads_module && is_worker_messaging_constructor_name(&prop_ident.sym) {
                return lower_worker_messaging_new(
                    ctx,
                    prop_ident.sym.as_ref(),
                    new_expr.args.as_deref(),
                );
            }
            if is_worker_threads_module && prop_ident.sym.as_ref() == "Worker" {
                return lower_worker_new(ctx, new_expr);
            }
            let inspector_session_module =
                ctx.lookup_native_module(module_alias)
                    .and_then(
                        |(module_name, _)| match (module_name, prop_ident.sym.as_ref()) {
                            ("inspector" | "inspector/promises", "Session") => {
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
            // #4995: `new ev.EventEmitter()` over an events module alias
            // (`import * as ev from 'events'` / `import EE from 'events'` /
            // `const ev = require('events')`) joins the same `Expr::New`
            // route as the named import. Aliases registered only as
            // builtin-module aliases (not native-module bindings) are
            // covered by the `lookup_builtin_module_alias` arm.
            if ctx.lookup_builtin_module_alias(module_alias) == Some("events")
                && matches!(
                    prop_ident.sym.as_ref(),
                    "EventEmitter" | "EventEmitterAsyncResource"
                )
            {
                return Ok(Expr::New {
                    class_name: prop_ident.sym.to_string(),
                    args: lower_optional_args(ctx, new_expr.args.as_deref())?,
                    type_args: Vec::new(),
                    byte_offset: new_byte_offset,
                });
            }
            if let Some((module_name, _)) = ctx.lookup_native_module(module_alias) {
                let class_name = prop_ident.sym.as_ref();
                if matches!(
                    (module_name, class_name),
                    ("events", "EventEmitter")
                        | ("events", "EventEmitterAsyncResource")
                        | ("async_hooks", "AsyncLocalStorage" | "AsyncResource")
                        | ("sqlite", "DatabaseSync" | "Session" | "StatementSync")
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
                        byte_offset: new_byte_offset,
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
            // Resolve through any scope-local class rename so `new X` binds to
            // the lexically-correct (possibly disambiguated) class.
            let mut class_name = ctx.resolve_class_name(ident.sym.as_str());
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
            if is_worker_messaging_constructor_name(&class_name)
                && ctx.lookup_local(&class_name).is_none()
            {
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

            if let Some(resolved) = ctx.resolve_class_alias(&class_name) {
                if is_url_encoding_constructor_name(&resolved) {
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
            if class_name == "Object"
                && ctx.lookup_local("Object").is_none()
                && ctx.lookup_func("Object").is_none()
                && ctx.lookup_class("Object").is_none()
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
                let arg = args.drain(..).next().unwrap_or(Expr::Undefined);
                return Ok(Expr::ObjectCoerce(Box::new(arg)));
            }
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
                // A *present* argument is coerced per spec: `new Number(x)` →
                // ToNumber(x), `new String(x)` → ToString(x). This matters for
                // an explicit `undefined`: `new Number(undefined)` is NaN and
                // `new String(undefined)` is "undefined" — distinct from the
                // *no-arg* forms `new Number()`/`new String()` which box +0/""
                // (handled by the undefined sentinel in `js_boxed_*_new`).
                // Without this, both collapse to `Expr::Undefined` and the
                // runtime can't tell them apart.
                let arg = match args.drain(..).next() {
                    Some(inner) => match kind {
                        crate::BoxedPrimitiveKind::Number => Expr::NumberCoerce(Box::new(inner)),
                        crate::BoxedPrimitiveKind::String => Expr::StringCoerce(Box::new(inner)),
                        crate::BoxedPrimitiveKind::Boolean => Expr::BooleanCoerce(Box::new(inner)),
                    },
                    None => Expr::Undefined,
                };
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

            // Handle URL class
            if class_name == "URL" {
                return Ok(
                    lower_url_encoding_constructor(ctx, "URL", new_expr.args.as_deref())?.unwrap(),
                );
            }

            // Handle URLSearchParams / URLPattern classes
            if matches!(class_name.as_str(), "URLSearchParams" | "URLPattern") {
                return Ok(lower_url_encoding_constructor(
                    ctx,
                    &class_name,
                    new_expr.args.as_deref(),
                )?
                .unwrap());
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
                return Ok(lower_url_encoding_constructor(
                    ctx,
                    "TextEncoder",
                    new_expr.args.as_deref(),
                )?
                .unwrap());
            }
            // Handle TextDecoder constructor: new TextDecoder(label?, opts?)
            if class_name == "TextDecoder" {
                return Ok(lower_url_encoding_constructor(
                    ctx,
                    "TextDecoder",
                    new_expr.args.as_deref(),
                )?
                .unwrap());
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
                if let Some(local_id) = ctx.lookup_local(&class_name) {
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
                let mut args: Vec<Expr> = new_expr
                    .args
                    .as_ref()
                    .map(|args| {
                        args.iter()
                            .map(|a| lower_expr(ctx, &a.expr))
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                // Issue #212 (anon-class-expression parity): a class expression
                // nested in a function may capture enclosing-scope locals.
                // `lower_class_from_ast` → `synthesize_class_captures` extended
                // the synthesized constructor with one param per captured id and
                // rewrote the METHOD bodies to read `this.__perry_cap_<id>`. The
                // named-class `new C()` path above forwards those captures as
                // `LocalGet(id)`; the directly-constructed anonymous form
                // (`new class { m() { return outer } }()`) must do the same, or
                // the cap params receive `undefined` and every method that reads
                // a captured local sees `undefined`. Refs Next.js bundled tracer
                // (`getActiveScopeSpan` → `trace.getSpan` on undefined `trace`).
                let class_captures: Vec<LocalId> = ctx
                    .lookup_class_captures(&synthetic_name)
                    .map(|c| c.to_vec())
                    .unwrap_or_default();
                for cid in class_captures {
                    args.push(Expr::LocalGet(cid));
                }
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
                    byte_offset: new_byte_offset,
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
                if is_global_object_expr(ctx, object.as_ref())
                    && matches!(property.as_str(), "Symbol" | "BigInt" | "Math")
                {
                    return Ok(nonconstructable_builtin_throw_expr(property, args));
                }
                if is_global_object_expr(ctx, object.as_ref())
                    && matches!(
                        property.as_str(),
                        "Blob"
                            | "File"
                            | "FormData"
                            | "Headers"
                            | "Request"
                            | "Response"
                            | "WebSocket"
                    )
                {
                    if is_fetch_constructor_name(property) {
                        ctx.uses_fetch = true;
                    }
                    return Ok(Expr::New {
                        class_name: property.clone(),
                        args,
                        type_args: Vec::new(),
                        byte_offset: new_byte_offset,
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
                        byte_offset: new_byte_offset,
                    });
                }
            }
            Ok(Expr::NewDynamic {
                callee,
                args,
                byte_offset: new_byte_offset,
            })
        }
    }
}
