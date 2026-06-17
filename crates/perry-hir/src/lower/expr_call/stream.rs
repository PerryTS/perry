//! Stream-API helpers used by `expr_call/mod.rs`.
//!
//! Extracted from `expr_call/mod.rs` in #1104 as a pure mechanical move;
//! the only consumers are inside `lower_call_inner` in this module.

use swc_ecma_ast as ast;

use super::super::LoweringContext;

/// Issue #562 — does `method` name a stream-API method on the given
/// stream module? Used to gate the native-instance method rerouting so
/// user-declared subclass methods fall through to regular class method
/// dispatch. Mirrors the methods table in
/// `crates/perry-codegen/src/lower_call.rs`'s `module == "<stream>"`
/// arms — see also the parallel `is_stream_api_member` in
/// `expr_member.rs`.
pub(super) fn is_stream_api_method(module: &str, method: &str) -> bool {
    match module {
        "readable_stream" => matches!(
            method,
            "getReader"
                | "cancel"
                | "tee"
                | "pipeTo"
                | "pipeThrough"
                | "locked"
                | "enqueue"
                | "close"
                | "error"
                | "desiredSize"
                | "byobRequest"
        ),
        "readable_stream_reader" => {
            matches!(method, "read" | "releaseLock" | "cancel" | "closed")
        }
        "writable_stream" => matches!(method, "getWriter" | "abort" | "close" | "locked"),
        "writable_stream_writer" => matches!(
            method,
            "write" | "close" | "abort" | "releaseLock" | "closed" | "ready" | "desiredSize"
        ),
        "transform_stream" => matches!(method, "readable" | "writable"),
        _ => false,
    }
}

/// Issue #562 — `class X extends ReadableStream/TransformStream`
/// constructor: register the controller param of each callback in
/// `super({...})` as a `readable_stream` native instance so
/// `controller.enqueue(...)` inside those bodies dispatches through the
/// streams arms in `lower_call.rs`.
///
/// Mirrors the field-spec table in `expr_new.rs::lower_new` for the
/// `new ReadableStream/TransformStream` form. WritableStream's
/// `write`/`close`/`abort` callbacks don't take a controller —
/// no pre-registration is required for that parent.
pub(super) fn register_super_stream_controller_params(
    ctx: &mut LoweringContext,
    parent_ident: &str,
    call: &ast::CallExpr,
) {
    let field_specs: &[(&'static str, usize, &'static str, &'static str)] = match parent_ident {
        "ReadableStream" => &[
            ("start", 0, "readable_stream", "ReadableStream"),
            ("pull", 0, "readable_stream", "ReadableStream"),
        ],
        "TransformStream" => &[
            ("transform", 1, "readable_stream", "ReadableStream"),
            ("flush", 0, "readable_stream", "ReadableStream"),
        ],
        _ => return,
    };

    let Some(first) = call.args.first() else {
        return;
    };
    let ast::Expr::Object(obj_lit) = first.expr.as_ref() else {
        return;
    };
    for prop in &obj_lit.props {
        let ast::PropOrSpread::Prop(boxed_prop) = prop else {
            continue;
        };
        match boxed_prop.as_ref() {
            ast::Prop::KeyValue(kv) => {
                let name = match &kv.key {
                    ast::PropName::Ident(i) => Some(i.sym.as_ref()),
                    ast::PropName::Str(s) => s.value.as_str(),
                    _ => None,
                };
                let Some(name) = name else { continue };
                let Some((_, idx, mod_name, class_name)) =
                    field_specs.iter().find(|(f, _, _, _)| *f == name)
                else {
                    continue;
                };
                let pat: Option<&ast::Pat> = match kv.value.as_ref() {
                    ast::Expr::Arrow(arrow) => arrow.params.get(*idx),
                    ast::Expr::Fn(fn_expr) => fn_expr.function.params.get(*idx).map(|p| &p.pat),
                    _ => None,
                };
                if let Some(ast::Pat::Ident(pid)) = pat {
                    ctx.register_native_instance(
                        pid.id.sym.to_string(),
                        mod_name.to_string(),
                        class_name.to_string(),
                    );
                }
            }
            ast::Prop::Method(m) => {
                let name = match &m.key {
                    ast::PropName::Ident(i) => Some(i.sym.as_ref()),
                    ast::PropName::Str(s) => s.value.as_str(),
                    _ => None,
                };
                let Some(name) = name else { continue };
                let Some((_, idx, mod_name, class_name)) =
                    field_specs.iter().find(|(f, _, _, _)| *f == name)
                else {
                    continue;
                };
                if let Some(param) = m.function.params.get(*idx) {
                    if let ast::Pat::Ident(pid) = &param.pat {
                        ctx.register_native_instance(
                            pid.id.sym.to_string(),
                            mod_name.to_string(),
                            class_name.to_string(),
                        );
                    }
                }
            }
            _ => {}
        }
    }
}
