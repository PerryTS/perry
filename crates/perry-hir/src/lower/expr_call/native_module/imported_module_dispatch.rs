//! Generic dispatch for a receiver bound to an imported native module.
//!
//! Extracted from `expr_call/native_module.rs` as a mechanical move. This is
//! the last check in `try_native_module_methods`, so falling out of it with
//! `Ok(Err(args))` is exactly what the inline block did when it reached its
//! closing brace.

use super::*;

use crate::types::Type;
use anyhow::Result;
use swc_ecma_ast as ast;

pub(super) fn try_imported_module_dispatch(
    ctx: &mut LoweringContext,
    member: &ast::MemberExpr,
    obj_name: &str,
    args: Vec<Expr>,
) -> Result<Result<Expr, Vec<Expr>>> {
    if let Some((module_name, imported_method)) = ctx.lookup_native_module(obj_name) {
        if module_name == "url" && imported_method == Some("URL") {
            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                let method_name = method_ident.sym.as_ref();
                if method_name == "canParse" && !args.is_empty() {
                    let mut iter = args.into_iter();
                    let input = iter.next().unwrap();
                    if let Some(base) = iter.next() {
                        return Ok(Ok(Expr::UrlCanParseWithBase {
                            input: Box::new(input),
                            base: Box::new(base),
                        }));
                    }
                    return Ok(Ok(Expr::UrlCanParse(Box::new(input))));
                }
                if method_name == "parse" && !args.is_empty() {
                    let mut iter = args.into_iter();
                    let input = iter.next().unwrap();
                    if let Some(base) = iter.next() {
                        return Ok(Ok(Expr::UrlParseWithBase {
                            input: Box::new(input),
                            base: Box::new(base),
                        }));
                    }
                    return Ok(Ok(Expr::UrlParse(Box::new(input))));
                }
            }
        }

        if let Some(submodule) = path_submodule_name(module_name) {
            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                let method_name = method_ident.sym.to_string();
                return Ok(
                    match super::super::nested_namespace::dispatch_path_subnamespace(
                        submodule,
                        &method_name,
                        args,
                    ) {
                        Ok(expr) => Ok(expr),
                        Err(args) => Err(args),
                    },
                );
            }
        }

        // Skip modules handled specifically below (path, fs, child_process, etc.)
        // `net` used to be in this list back when its method calls
        // were short-circuited into `Expr::NetCreateConnection` etc.
        // After A1.5 `net` goes through the generic NativeMethodCall
        // path so the LLVM backend's NATIVE_MODULE_TABLE dispatches
        // to `js_net_socket_*` in perry-stdlib.
        let is_handled_module = module_name == "path"
            || module_name == "node:path"
            || module_name == "fs"
            || module_name == "node:fs"
            || module_name == "child_process"
            || module_name == "node:child_process"
            || module_name == "crypto"
            || module_name == "node:crypto"
            || module_name == "os"
            || module_name == "node:os";
        if !is_handled_module {
            // This is a call on a native module (e.g., mysql.createConnection)
            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                let method_name = method_ident.sym.to_string();
                // A destructured/named DATA member of a native module
                // (`const { METHODS } = require('node:http')` →
                // `imported_method = Some("METHODS")`) holds a real
                // Array/Object value. An Array/Object prototype method on it
                // (`METHODS.map(...)`, `STATUS_CODES.hasOwnProperty(...)`) is
                // a call on that VALUE — NOT a `module.method` native call.
                // Bail to generic dynamic dispatch so it runs on the member's
                // resolved value (express's `router` does
                // `METHODS.map((m) => m.toLowerCase())`). Without this the
                // call lowered to `NativeMethodCall { module: "http", method:
                // "map" }`, which returned undefined / a deferred throw.
                if imported_method.is_some()
                    && (super::super::super::array_fold::is_known_array_prototype_method(
                        &method_name,
                    ) || super::super::super::array_fold::is_known_object_prototype_method(
                        &method_name,
                    ))
                {
                    return Ok(Err(args));
                }
                if module_name == "worker_threads" && method_name == "workerData" {
                    return Ok(Err(args));
                }
                if module_name.strip_prefix("node:").unwrap_or(module_name) == "vm"
                    && imported_method.is_none()
                    && method_name == "Module"
                {
                    let mut exprs = args;
                    exprs.push(Expr::Call {
                        callee: Box::new(Expr::ExternFuncRef {
                            name: "js_vm_module_call".to_string(),
                            param_types: Vec::new(),
                            return_type: Type::Any,
                        }),
                        args: Vec::new(),
                        type_args: Vec::new(),
                        byte_offset: 0,
                    });
                    return Ok(Ok(Expr::Sequence(exprs)));
                }
                let normalized_module = module_name.strip_prefix("node:").unwrap_or(module_name);
                if normalized_module == "cluster"
                    && matches!(imported_method, Some("default"))
                    && is_cluster_default_event_emitter_method(&method_name)
                {
                    return Ok(Ok(Expr::NativeMethodCall {
                        module: module_name.to_string(),
                        class_name: None,
                        object: None,
                        method: method_name,
                        args,
                    }));
                }
                if method_name == "call" {
                    if normalized_module == "stream"
                        && matches!(imported_method, None | Some("Stream"))
                    {
                        return Ok(Ok(event_emitter_constructor_call(args)));
                    }
                    if normalized_module == "events"
                        && matches!(imported_method, Some("EventEmitter"))
                    {
                        return Ok(Ok(event_emitter_constructor_call(args)));
                    }
                    // #4973: named-import form of the inherits
                    // pattern — `const { Server } = require('http');
                    // Server.call(this, handler)`. Same extern as
                    // the dotted `http.Server.call(...)` form in
                    // module_class_static.rs.
                    if matches!(normalized_module, "http" | "https")
                        && matches!(imported_method, Some("Server"))
                        && !args.is_empty()
                    {
                        let mut it = args.into_iter();
                        let this_arg = it.next().unwrap();
                        let mut rest: Vec<Expr> = it.collect();
                        rest.resize(2, Expr::Undefined);
                        let mut call_args = vec![this_arg];
                        call_args.extend(rest);
                        let extern_name = if normalized_module == "https" {
                            "js_https_server_construct_with_this"
                        } else {
                            "js_http_server_construct_with_this"
                        };
                        return Ok(Ok(Expr::Call {
                            callee: Box::new(Expr::ExternFuncRef {
                                name: extern_name.to_string(),
                                param_types: Vec::new(),
                                return_type: Type::Any,
                            }),
                            args: call_args,
                            type_args: Vec::new(),
                            byte_offset: 0,
                        }));
                    }
                }
                // Unimplemented-API gate (#463 / #525) for the 2-deep
                // `mod.method()` call form. Without this, perry/* and
                // other native-module call sites short-circuited past
                // the `lower_member` gate that fires for the property-
                // read form, then bailed in codegen with a per-module
                // message (`'X' is not a known function`) — different
                // wording, different escape hatch, harder for users to
                // recognize as the same class of mistake. Mirrors the
                // 3-deep gate above for `mod.X.Y()`.
                let manifest_entry =
                    perry_api_manifest::module_has_symbol(module_name, &method_name);
                if perry_api_manifest::module_has_any_entries(module_name)
                    && manifest_entry.is_none()
                    // #wall4: an unmistakable `String.prototype` method
                    // (`endsWith`, `slice`, …) called on an identifier that
                    // shares a node-core module name (`url`, `path`) means
                    // the receiver is a runtime string, NOT the module —
                    // don't gate it as an unimplemented module API; fall
                    // through to dynamic dispatch on the real receiver.
                    // Next.js app-page-turbo calls `url.endsWith(...)` on a
                    // URL string bound to a local named `url`.
                    && !super::super::super::array_fold::is_known_string_prototype_method(
                        &method_name,
                    )
                    // A destructured/named DATA member of a native module
                    // (`const { METHODS } = require('node:http')`,
                    // registered with `imported_method = Some("METHODS")`)
                    // holds a real Array/Object value, not a callable
                    // namespace. An Array/Object prototype method on it
                    // (`METHODS.map(...)`, `STATUS_CODES.hasOwnProperty(...)`)
                    // is a call on that VALUE — gating it as
                    // `http.map`/`http.hasOwnProperty` (#463) compiled it to
                    // a deferred "not implemented" throw, breaking express's
                    // `router` (`METHODS.map((m) => m.toLowerCase())`). Fall
                    // through to dynamic dispatch on the real member value.
                    && !(imported_method.is_some()
                        && (super::super::super::array_fold::is_known_array_prototype_method(
                            &method_name,
                        ) || super::super::super::array_fold::is_known_object_prototype_method(
                            &method_name,
                        )))
                {
                    // #925: this is the gate that fires
                    // for `crypto.hmacSha256(data, key)`.
                    let hint = super::super::super::unimpl_hints::module_member_hint(
                        module_name,
                        &method_name,
                    )
                    .map(|h| format!(" {h}"))
                    .unwrap_or_default();
                    let msg = format!(
                        "`{}.{}` is not implemented in Perry — see `perry --print-api-manifest` for the supported surface, \
                         or set `PERRY_ALLOW_UNIMPLEMENTED=1` to ignore. (#463){}",
                        module_name, method_name, hint,
                    );
                    // #5245: default → throw-on-reach + notice; strict →
                    // hard #463 refusal. #2309 tree-shake handled inside.
                    let api = format!("{module_name}.{method_name}");
                    let location = crate::eval_classifier::location_string(
                        &ctx.source_file_path,
                        member.span.lo.0,
                    );
                    match crate::check_unimplemented_api(&msg, &api, &location, member.span.lo.0) {
                        crate::UnimplementedDecision::Refuse => {
                            crate::lower_bail!(member.span, "{}", msg);
                        }
                        crate::UnimplementedDecision::DeferToRuntimeError(runtime_msg) => {
                            return Ok(Ok(
                                super::super::super::const_fold_fn::synth_deferred_throw_value(
                                    ctx,
                                    &runtime_msg,
                                    member.span,
                                )?,
                            ));
                        }
                    }
                }
                if let Some(entry) = manifest_entry {
                    if !matches!(
                        entry.kind,
                        perry_api_manifest::ApiKind::Method {
                            has_receiver: false,
                            class_filter: None
                        }
                    ) {
                        return Ok(Err(args));
                    }
                }
                let class_name = if module_name == "stream"
                    && imported_method.is_some_and(is_node_stream_class_name)
                {
                    imported_method.map(str::to_string)
                } else {
                    None
                };
                return Ok(Ok(Expr::NativeMethodCall {
                    module: module_name.to_string(),
                    class_name,
                    object: None, // Static call on module itself
                    method: method_name,
                    args,
                }));
            }
        }
    }

    Ok(Err(args))
}
