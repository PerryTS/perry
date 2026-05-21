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
    call: &ast::CallExpr,
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
                                | ("util", "types")
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
                            crate::lower_bail!(
                                outer_member.span,
                                "`{}.{}` is not implemented in Perry — see `perry --print-api-manifest` for the supported surface, \
                                 or set `PERRY_ALLOW_UNIMPLEMENTED=1` to ignore. (#463){}",
                                module_name,
                                class_name,
                                hint,
                            );
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

    // process.stdin.setRawMode/.on and process.stdout.on — methods
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
                                ("stdin", "on") => {
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
