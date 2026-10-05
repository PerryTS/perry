//! module.Class.staticMethod() and process.std{in,out} dispatch.
//!
//! Extracted from `expr_call/mod.rs` as a mechanical move.

use crate::types::Type;
use anyhow::Result;
use swc_ecma_ast as ast;

use crate::ir::*;

use super::super::LoweringContext;

pub(super) fn try_module_class_static(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    expr: &ast::Expr,
    args: Vec<Expr>,
) -> Result<Result<Expr, Vec<Expr>>> {
    // A native class call is valid only when the dispatcher has a matching
    // static entry. Otherwise ns.Class.method must read the actual exported
    // value, just like a user-module class or a method read used as a value.
    // Do not invent a receiver-less NativeMethodCall from syntax alone
    // (#11896): it bypasses the object's statics and returns undefined.
    if let ast::Expr::Member(outer_member) = expr {
        if let ast::Expr::Member(inner_member) = outer_member.obj.as_ref() {
            if let ast::Expr::Ident(mod_ident) = inner_member.obj.as_ref() {
                if let Some((module_name, None)) = ctx.lookup_native_module(mod_ident.sym.as_ref())
                {
                    if let (
                        ast::MemberProp::Ident(class_ident),
                        ast::MemberProp::Ident(method_ident),
                    ) = (&inner_member.prop, &outer_member.prop)
                    {
                        let class_name = class_ident.sym.as_ref();
                        let method_name = method_ident.sym.as_ref();
                        let normalized = module_name.strip_prefix("node:").unwrap_or(module_name);
                        // `Buffer.prototype.m(...)` calls a method of the
                        // prototype OBJECT; `prototype` is never a module
                        // class, so this is an ordinary method call.
                        if class_name == "prototype" {
                            return Ok(Err(args));
                        }
                        // Preserve the value/subnamespace routes already used on main,
                        // including Buffer's dedicated lowering (#11941). Manifest
                        // rows for these values need not be class-call table entries.
                        if matches!(
                            (normalized, class_name),
                            ("fs", "promises" | "constants")
                                | ("path", "posix" | "win32")
                                | ("perf_hooks", "supportedEntryTypes")
                                | ("module" | "repl", "builtinModules")
                                | (
                                    "process" | "process.namespace" | "process.default",
                                    "stdin" | "stdout" | "stderr" | "version"
                                )
                                | ("os", "EOL" | "devNull")
                                | (
                                    "bun",
                                    "YAML" | "TOML" | "semver" | "JSONL" | "hash" | "plugin"
                                )
                                | ("buffer", "Buffer")
                        ) {
                            return Ok(Err(args));
                        }
                        // util.inherits-era Server.call initializes and aliases
                        // the supplied instance to the native server (#4973).
                        if matches!(normalized, "http" | "https")
                            && class_name == "Server"
                            && method_name == "call"
                            && !args.is_empty()
                        {
                            let mut it = args.into_iter();
                            let this_arg = it.next().unwrap();
                            let mut rest: Vec<Expr> = it.collect();
                            rest.resize(2, Expr::Undefined);
                            let mut call_args = vec![this_arg];
                            call_args.extend(rest);
                            let extern_name = if normalized == "https" {
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
                        // Unimplemented-API gate (#463) for the chained
                        // `mod.X.Y()` case. The lower_member gate fires
                        // for `mod.X` standalone but not when this arm
                        // short-circuits the chain into a single
                        // `NativeMethodCall` without recursing through
                        // lower_member. Without this, `crypto.subtle.encrypt(...)`
                        // built cleanly and silently returned undefined.
                        if perry_api_manifest::module_has_any_entries(module_name)
                            && perry_api_manifest::module_has_symbol(module_name, class_name)
                                .is_none()
                        {
                            // #925: append a replacement hint if
                            // we have one for this exact shape.
                            let hint = super::super::unimpl_hints::module_member_hint(
                                module_name,
                                class_name,
                            )
                            .map(|h| format!(" {h}"))
                            .unwrap_or_default();
                            let msg = format!(
                                "`{}.{}` is not implemented in Perry — see `perry --print-api-manifest` for the supported surface, \
                                 or set `PERRY_ALLOW_UNIMPLEMENTED=1` to ignore. (#463){}",
                                module_name, class_name, hint,
                            );
                            // #5245: default → throw-on-reach + notice; strict →
                            // hard #463 refusal. #2309 tree-shake handled inside.
                            let api = format!("{module_name}.{class_name}");
                            let location = crate::eval_classifier::location_string(
                                &ctx.source_file_path,
                                outer_member.span.lo.0,
                            );
                            match crate::check_unimplemented_api(
                                &msg,
                                &api,
                                &location,
                                outer_member.span.lo.0,
                            ) {
                                crate::UnimplementedDecision::Refuse => {
                                    crate::lower_bail!(outer_member.span, "{}", msg);
                                }
                                crate::UnimplementedDecision::DeferToRuntimeError(runtime_msg) => {
                                    return Ok(Ok(
                                        super::super::const_fold_fn::synth_deferred_throw_value(
                                            ctx,
                                            &runtime_msg,
                                            outer_member.span,
                                        )?,
                                    ));
                                }
                            }
                        }
                        // Preserve genuine native fast paths, including module-wide
                        // entries: codegen accepts class_filter: None for any class.
                        // This keeps inherited events/cluster statics working.
                        // Everything else falls through to the same property
                        // read as ns.Class.method used as a value. lower_member
                        // also retains the unimplemented-export gate.
                        if !super::call_has_spread_arg(call)
                            && perry_api_manifest::entries_for_module(module_name).any(|entry| {
                                entry.name == method_name
                                    && matches!(
                                        entry.kind,
                                        perry_api_manifest::ApiKind::Method {
                                            has_receiver: false,
                                            class_filter,
                                        } if class_filter.is_none_or(|class| class == class_name)
                                    )
                            })
                        {
                            return Ok(Ok(Expr::NativeMethodCall {
                                module: module_name.to_string(),
                                class_name: Some(class_name.to_string()),
                                object: None,
                                method: method_name.to_string(),
                                args,
                            }));
                        }
                    }
                }
            }
        }
    }

    // process.stdin.setRawMode/.on and lifecycle methods, plus process.stdout.on — methods
    // we recognize on the stdin/stdout stream objects. (#347
    // Phases 2 & 3.) These are stream values rather than class statics. Falls through to the generic dispatch
    // (which lowers it as a closure call on the stub object) for
    // any other method name — `process.stdout.write` keeps
    // working through that path.
    if let ast::Expr::Member(outer_member) = expr {
        if let ast::Expr::Member(inner_member) = outer_member.obj.as_ref() {
            if let ast::Expr::Ident(root_ident) = inner_member.obj.as_ref() {
                let root = root_ident.sym.as_ref();
                let native_process_import = matches!(
                    ctx.lookup_native_module(root),
                    Some(("process" | "process.namespace" | "process.default", None))
                );
                if (root == "process" && !ctx.shadows_unqualified_global("process"))
                    || native_process_import
                {
                    if let ast::MemberProp::Ident(stream_ident) = &inner_member.prop {
                        let stream = stream_ident.sym.as_ref();
                        if let ast::MemberProp::Ident(method_ident) = &outer_member.prop {
                            let method_name = method_ident.sym.as_ref();
                            match (stream, method_name) {
                                ("stdin", "setRawMode") if !args.is_empty() => {
                                    let arg = args.into_iter().next().unwrap();
                                    return Ok(Ok(Expr::ProcessStdinSetRawMode(Box::new(arg))));
                                }
                                // `once` lowers here too. Without it, only
                                // `on`/`addListener` reached readline's stdin
                                // listener registry and `process.stdin.once(…)`
                                // fell through to the generic member-call path,
                                // which never registers with the fd-0 reader —
                                // so the callback simply never fired.
                                //
                                // That is not a corner case: Claude Code's `-p`
                                // stdin reader awaits
                                // `race(stdin.once("end"), timeout(3000))`, so
                                // with `once` dropped the `end` half could never
                                // win. The race fell through to the timeout, and
                                // because that timer is unref'd nothing kept the
                                // loop alive — `echo hi | claude -p …` exited 0
                                // having printed NOTHING (node prints the result).
                                //
                                // One-shot semantics are handled downstream by
                                // the pump, which takes the `end` listener list
                                // when it fires; `data`/`readable` listeners
                                // registered via `once` are a documented
                                // residual (they behave like `on`) — the streams
                                // that matter here are EOF-driven.
                                ("stdin", "on") | ("stdin", "addListener") | ("stdin", "once")
                                    if args.len() >= 2 =>
                                {
                                    let mut iter = args.into_iter();
                                    let event = iter.next().unwrap();
                                    let handler = iter.next().unwrap();
                                    return Ok(Ok(Expr::ProcessStdinOn {
                                        event: Box::new(event),
                                        handler: Box::new(handler),
                                    }));
                                }
                                ("stdin", "removeListener") | ("stdin", "off")
                                    if args.len() >= 2 =>
                                {
                                    let mut iter = args.into_iter();
                                    let event = iter.next().unwrap();
                                    let handler = iter.next().unwrap();
                                    return Ok(Ok(Expr::ProcessStdinRemoveListener {
                                        event: Box::new(event),
                                        handler: Box::new(handler),
                                    }));
                                }
                                ("stdin", "pause") => {
                                    return Ok(Ok(Expr::ProcessStdinLifecycle(
                                        ProcessStdinLifecycleMethod::Pause,
                                    )));
                                }
                                ("stdin", "resume") => {
                                    return Ok(Ok(Expr::ProcessStdinLifecycle(
                                        ProcessStdinLifecycleMethod::Resume,
                                    )));
                                }
                                ("stdin", "unref") => {
                                    return Ok(Ok(Expr::ProcessStdinLifecycle(
                                        ProcessStdinLifecycleMethod::Unref,
                                    )));
                                }
                                ("stdin", "ref") => {
                                    return Ok(Ok(Expr::ProcessStdinLifecycle(
                                        ProcessStdinLifecycleMethod::Ref,
                                    )));
                                }
                                ("stdin", "destroy") => {
                                    return Ok(Ok(Expr::ProcessStdinLifecycle(
                                        ProcessStdinLifecycleMethod::Destroy,
                                    )));
                                }
                                ("stdout", "on") if args.len() >= 2 => {
                                    let mut iter = args.into_iter();
                                    let event = iter.next().unwrap();
                                    let handler = iter.next().unwrap();
                                    return Ok(Ok(Expr::ProcessStdoutOn {
                                        event: Box::new(event),
                                        handler: Box::new(handler),
                                    }));
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(Err(args))
}
