//! module.Class.staticMethod() and process.std{in,out} dispatch.
//!
//! Extracted from `expr_call/mod.rs` as a mechanical move.

use anyhow::{anyhow, Result};
use perry_types::{LocalId, Type};
use swc_ecma_ast as ast;

use super::super::unimpl_hints;
use super::stream::is_stream_api_method;
use crate::ir::*;
use crate::lower_types::extract_ts_type_with_ctx;

use super::super::{
    extract_typed_parse_source_order, is_generator_call_expr, is_widget_modifier_name, lower_expr,
    resolve_typed_parse_ty, LoweringContext,
};

pub(super) fn try_module_class_static(
    ctx: &mut LoweringContext,
    // #854: kept for the uniform `try_*` dispatch-helper signature; this arm
    // works off `expr`, not the raw `CallExpr`.
    _call: &ast::CallExpr,
    expr: &ast::Expr,
    args: Vec<Expr>,
) -> Result<Result<Expr, Vec<Expr>>> {
    // Check for module.Class.staticMethod() pattern (e.g.,
    // ethers.Wallet.createRandom()). Modelled after the
    // process.hrtime.bigint() handler above.
    //
    // Some "module.foo.method()" shapes are NOT class statics —
    // they're sub-namespaces with dedicated codegen arms in
    // `crates/perry-codegen/src/expr.rs` (e.g. fs.promises.X
    // routes to the sync counterpart + js_promise_resolved).
    // Skip them here so the existing codegen path keeps working.
    // v0.5.385 (#299) introduced this arm; v0.5.386 (this fix)
    // adds the exclusion list after fs.promises.readFile silently
    // started returning `undefined` because the new HIR shape
    // bypassed the old codegen arm and fell into the
    // "unhandled fs.<method>()" warn-and-undef path.
    if let ast::Expr::Member(outer_member) = expr {
        if let ast::Expr::Member(inner_member) = outer_member.obj.as_ref() {
            if let ast::Expr::Ident(mod_ident) = inner_member.obj.as_ref() {
                let mod_name = mod_ident.sym.to_string();
                if let Some((module_name, _)) = ctx.lookup_native_module(&mod_name) {
                    if let ast::MemberProp::Ident(class_ident) = &inner_member.prop {
                        let class_name = class_ident.sym.to_string();
                        let is_sub_namespace = matches!(
                            (module_name, class_name.as_str()),
                            ("fs", "promises")
                                | ("fs", "constants")
                                | ("path", "posix")
                                | ("path", "win32")
                                // #1320: `PerformanceObserver.supportedEntryTypes`
                                // is a static *array value*, not a class — so
                                // `…supportedEntryTypes.includes(x)` is a value
                                // method, not a class static. Fall through to
                                // value-method dispatch instead of building a
                                // bogus NativeMethodCall(class="supportedEntryTypes").
                                | ("perf_hooks", "supportedEntryTypes")
                                // `module.builtinModules` is an Array value,
                                // so `module.builtinModules.slice()` must
                                // dispatch as an array method, not a
                                // `module.builtinModules.slice` class static.
                                | ("module", "builtinModules")
                                | ("node:module", "builtinModules")
                                | ("repl", "builtinModules")
                                | ("node:repl", "builtinModules")
                                // `process.version` is a string value. Let
                                // String.prototype methods dispatch through
                                // the normal value-method path instead of
                                // building NativeMethodCall(class="version").
                                | ("process", "version")
                                | ("process.namespace", "version")
                                | ("process.default", "version")
                                | ("node:process", "version")
                                | ("node:process.namespace", "version")
                                | ("node:process.default", "version")
                                // `os.EOL` / `os.devNull` are string-valued
                                // module properties, so `os.devNull.includes(x)`
                                // is a String method on the property value.
                                | ("os", "EOL")
                                | ("os", "devNull")
                        );
                        // Unimplemented-API gate (#463) for the chained
                        // `mod.X.Y()` case. The lower_member gate fires
                        // for `mod.X` standalone but not when this arm
                        // short-circuits the chain into a single
                        // `NativeMethodCall` without recursing through
                        // lower_member. Without this, `crypto.subtle.encrypt(...)`
                        // built cleanly and silently returned undefined.
                        let allow_unimplemented =
                            std::env::var_os("PERRY_ALLOW_UNIMPLEMENTED").is_some();
                        if !is_sub_namespace
                            && !allow_unimplemented
                            && perry_api_manifest::module_has_any_entries(module_name)
                            && perry_api_manifest::module_has_symbol(module_name, &class_name)
                                .is_none()
                        {
                            // #925: append a replacement hint if
                            // we have one for this exact shape.
                            let hint = super::super::unimpl_hints::module_member_hint(
                                module_name,
                                &class_name,
                            )
                            .map(|h| format!(" {h}"))
                            .unwrap_or_default();
                            let msg = format!(
                                "`{}.{}` is not implemented in Perry — see `perry --print-api-manifest` for the supported surface, \
                                 or set `PERRY_ALLOW_UNIMPLEMENTED=1` to ignore. (#463){}",
                                module_name, class_name, hint,
                            );
                            // #2309: defer under tree-shaking; re-raised only
                            // if the module survives pruning.
                            if !crate::try_defer_refusal(msg.clone(), outer_member.span.lo.0) {
                                crate::lower_bail!(outer_member.span, "{}", msg);
                            }
                        }
                        if !is_sub_namespace {
                            if let ast::MemberProp::Ident(method_ident) = &outer_member.prop {
                                let method_name = method_ident.sym.to_string();
                                return Ok(Ok(Expr::NativeMethodCall {
                                    module: module_name.to_string(),
                                    class_name: Some(class_name),
                                    object: None,
                                    method: method_name,
                                    args,
                                }));
                            }
                        }
                    }
                }
            }
        }
    }

    // process.stdin.setRawMode/.on and lifecycle methods, plus process.stdout.on — methods
    // we recognize on the stdin/stdout stream objects. (#347
    // Phases 2 & 3.) Recognized BEFORE the generic
    // module.Class.staticMethod() arm because process.std{in,out}
    // are not classes. Falls through to the generic dispatch
    // (which lowers it as a closure call on the stub object) for
    // any other method name — `process.stdout.write` keeps
    // working through that path.
    if let ast::Expr::Member(outer_member) = expr {
        if let ast::Expr::Member(inner_member) = outer_member.obj.as_ref() {
            if let ast::Expr::Ident(root_ident) = inner_member.obj.as_ref() {
                if root_ident.sym.as_ref() == "process" {
                    if let ast::MemberProp::Ident(stream_ident) = &inner_member.prop {
                        let stream = stream_ident.sym.as_ref();
                        if let ast::MemberProp::Ident(method_ident) = &outer_member.prop {
                            let method_name = method_ident.sym.as_ref();
                            match (stream, method_name) {
                                ("stdin", "setRawMode") => {
                                    if !args.is_empty() {
                                        let arg = args.into_iter().next().unwrap();
                                        return Ok(Ok(Expr::ProcessStdinSetRawMode(Box::new(arg))));
                                    }
                                }
                                ("stdin", "on") | ("stdin", "addListener") => {
                                    if args.len() >= 2 {
                                        let mut iter = args.into_iter();
                                        let event = iter.next().unwrap();
                                        let handler = iter.next().unwrap();
                                        return Ok(Ok(Expr::ProcessStdinOn {
                                            event: Box::new(event),
                                            handler: Box::new(handler),
                                        }));
                                    }
                                }
                                ("stdin", "removeListener") | ("stdin", "off") => {
                                    if args.len() >= 2 {
                                        let mut iter = args.into_iter();
                                        let event = iter.next().unwrap();
                                        let handler = iter.next().unwrap();
                                        return Ok(Ok(Expr::ProcessStdinRemoveListener {
                                            event: Box::new(event),
                                            handler: Box::new(handler),
                                        }));
                                    }
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
                                ("stdout", "on") => {
                                    if args.len() >= 2 {
                                        let mut iter = args.into_iter();
                                        let event = iter.next().unwrap();
                                        let handler = iter.next().unwrap();
                                        return Ok(Ok(Expr::ProcessStdoutOn {
                                            event: Box::new(event),
                                            handler: Box::new(handler),
                                        }));
                                    }
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
