//! Member-callee native-module dispatch for `new ns.Ctor(...)`, extracted from
//! `expr_new.rs`. Pure code move — no behavior change. Returns `Some(expr)`
//! when this block produced an early-return result, `None` to fall through to
//! the rest of `lower_new`.

use super::*;

use crate::types::Type;
use anyhow::Result;
use swc_ecma_ast as ast;

use crate::ir::Expr;

use super::super::expr_new_builtins::global_member_constructor_name;
use super::super::{lower_expr, LoweringContext};

/// Issue #422: `new net.Socket()` over a `net` module alias and the many other
/// `new <module>.<Ctor>(...)` native-module dispatch forms. Reroutes to a
/// receiver-less `NativeMethodCall` (or specialized variant) so subsequent
/// method calls dispatch correctly. Returns `None` to fall through.
pub(crate) fn lower_new_member_native(
    ctx: &mut LoweringContext,
    new_expr: &ast::NewExpr,
    callee_expr: &ast::Expr,
    new_byte_offset: u32,
) -> Result<Option<Expr>> {
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
                    return Ok(Some(Expr::New {
                        class_name: class_name.to_string(),
                        args: lower_optional_args(ctx, new_expr.args.as_deref())?,
                        type_args: Vec::new(),
                        byte_offset: new_byte_offset,
                        cap_args_appended: 0,
                    }));
                }
                if let Some(expr) =
                    lower_url_encoding_constructor(ctx, class_name, new_expr.args.as_deref())?
                {
                    return Ok(Some(expr));
                }
            }
            if obj_name == "globalThis"
                && ctx.lookup_local("globalThis").is_none()
                && is_fetch_constructor_name(prop_ident.sym.as_ref())
            {
                ctx.uses_fetch = true;
                return Ok(Some(Expr::New {
                    class_name: prop_ident.sym.to_string(),
                    args: lower_optional_args(ctx, new_expr.args.as_deref())?,
                    type_args: Vec::new(),
                    byte_offset: new_byte_offset,
                    cap_args_appended: 0,
                }));
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
                return Ok(Some(Expr::NativeMethodCall {
                    module: "net".to_string(),
                    class_name: None,
                    object: None,
                    method: method.to_string(),
                    args,
                }));
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
                return Ok(Some(Expr::NativeMethodCall {
                    module: if is_https_module {
                        "https".to_string()
                    } else {
                        "http".to_string()
                    },
                    class_name: None,
                    object: None,
                    method: "Agent".to_string(),
                    args,
                }));
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
                return Ok(Some(Expr::NewDynamic {
                    callee: Box::new(Expr::PropertyGet {
                        byte_offset: 0,
                        object: Box::new(Expr::NativeModuleRef("http".to_string())),
                        property: prop_ident.sym.to_string(),
                    }),
                    args,
                    byte_offset: new_byte_offset,
                }));
            }
            let is_url_module =
                obj_name == "url" || ctx.lookup_builtin_module_alias(obj_name) == Some("url");
            if is_url_module && prop_ident.sym.as_ref() == "Url" {
                return Ok(Some(Expr::NativeMethodCall {
                    module: "url".to_string(),
                    class_name: None,
                    object: None,
                    method: "Url".to_string(),
                    args: Vec::new(),
                }));
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
                    return Ok(Some(Expr::NativeMethodCall {
                        module: module_name,
                        class_name: None,
                        object: None,
                        method: "Resolver".to_string(),
                        args,
                    }));
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
                return Ok(Some(Expr::NativeMethodCall {
                    module: "module".to_string(),
                    class_name: None,
                    object: None,
                    method: prop_ident.sym.to_string(),
                    args,
                }));
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
                return Ok(Some(Expr::NativeMethodCall {
                    module: "vm".to_string(),
                    class_name: None,
                    object: None,
                    method: prop_ident.sym.to_string(),
                    args,
                }));
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
                return Ok(Some(Expr::Sequence(exprs)));
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
                    return Ok(Some(Expr::WebAssemblyModuleNew(Box::new(bytes))));
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
                return Ok(Some(Expr::NativeMethodCall {
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
                }));
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
                )
                .map(Some);
            }
            if is_worker_threads_module && prop_ident.sym.as_ref() == "Worker" {
                return lower_worker_new(ctx, new_expr).map(Some);
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
                return Ok(Some(Expr::NativeMethodCall {
                    module: module_name,
                    class_name: None,
                    object: None,
                    method: "Session".to_string(),
                    args,
                }));
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
                return Ok(Some(Expr::New {
                    class_name: prop_ident.sym.to_string(),
                    args: lower_optional_args(ctx, new_expr.args.as_deref())?,
                    type_args: Vec::new(),
                    byte_offset: new_byte_offset,
                    cap_args_appended: 0,
                }));
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
                    return Ok(Some(Expr::New {
                        class_name: class_name.to_string(),
                        args,
                        type_args: Vec::new(),
                        byte_offset: new_byte_offset,
                        cap_args_appended: 0,
                    }));
                }
            }
        }
    }
    Ok(None)
}
