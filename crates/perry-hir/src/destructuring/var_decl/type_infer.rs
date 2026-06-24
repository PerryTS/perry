//! Declared/inferred type computation and plain-object tagging for a simple
//! `let/const/var` identifier binding (extracted from `var_decl.rs`'s
//! `Pat::Ident` arm).

use super::*;

use anyhow::{anyhow, Result};
use perry_types::{LocalId, Type};
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::lower::{lower_expr, LoweringContext};
use crate::lower_patterns::*;
use crate::lower_types::*;

use crate::destructuring::var_decl_sources::*;

/// Computes the declared/inferred `Type` for the binding and records the
/// `plain_object_locals` tag where applicable. Mirrors the original inline
/// block verbatim.
pub(crate) fn infer_decl_type(
    ctx: &mut LoweringContext,
    decl: &ast::VarDeclarator,
    ident: &ast::BindingIdent,
    name: &str,
) -> Type {
    // #809: tag locals provably bound to a plain object (an object
    // literal or `Object.create(...)`). `static_receiver_class`
    // consults this so `x.toJSON()` / `.toString()` / `.valueOf()`
    // etc. on such a local fall through to generic dynamic dispatch
    // instead of the Date intrinsics (which would interpret the
    // object pointer's bits as a timestamp).
    if let Some(init_expr) = decl.init.as_deref() {
        let is_plain_object = match init_expr {
            ast::Expr::Object(_) => true,
            ast::Expr::Call(call) => {
                if let ast::Callee::Expr(callee) = &call.callee {
                    if let ast::Expr::Member(m) = callee.as_ref() {
                        let obj_is = |name: &str| matches!(m.obj.as_ref(), ast::Expr::Ident(o) if o.sym.as_ref() == name);
                        let prop_is = |name: &str| matches!(&m.prop, ast::MemberProp::Ident(p) if p.sym.as_ref() == name);
                        // Object.create(...) — #809.
                        (obj_is("Object") && prop_is("create"))
                            // #1387: `performance.mark(...)` /
                            // `performance.measure(...)` return a
                            // PerformanceEntry — a plain shaped object,
                            // never a Date — so `entry.toJSON()` (and
                            // `.toString()`/`.valueOf()`) must skip the
                            // ambiguous-Date arms and fall through to
                            // generic dispatch (which finds the
                            // synthesized PerformanceEntry#toJSON).
                            || (obj_is("performance")
                                && (prop_is("mark") || prop_is("measure")))
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            _ => false,
        };
        if is_plain_object {
            ctx.plain_object_locals.insert(name.to_string());
        }
    }
    let mut ty = ident
        .type_ann
        .as_ref()
        .map(|ann| extract_ts_type(&ann.type_ann))
        .unwrap_or_else(|| {
            // No type annotation: try local inference from initializer
            if let Some(init_expr) = &decl.init {
                let inferred = infer_type_from_expr(init_expr, ctx);
                if !matches!(inferred, Type::Any) {
                    return inferred;
                }
                // Fall back to tsgo resolved types if available
                if let Some(resolved) = ctx.resolved_types.as_ref() {
                    if let Some(resolved_ty) = resolved.get(&(ident.id.span.lo.0)) {
                        return resolved_ty.clone();
                    }
                }
            }
            Type::Any
        });

    // If no type annotation, infer from new Set<T>() or new Map<K, V>() or new URLSearchParams() expressions
    if matches!(ty, Type::Any) {
        if let Some(init_expr) = &decl.init {
            if let ast::Expr::New(new_expr) = init_expr.as_ref() {
                if let ast::Expr::Ident(class_ident) = new_expr.callee.as_ref() {
                    let class_name = class_ident.sym.as_ref();
                    if class_name == "Set" || class_name == "Map" {
                        // Extract type arguments from new Set<T>() or new Map<K, V>()
                        let type_args: Vec<Type> = new_expr
                            .type_args
                            .as_ref()
                            .map(|ta| ta.params.iter().map(|t| extract_ts_type(t)).collect())
                            .unwrap_or_default();
                        ty = Type::Generic {
                            base: class_name.to_string(),
                            type_args,
                        };
                    } else if class_name == "URLSearchParams" {
                        ty = Type::Named("URLSearchParams".to_string());
                    } else if class_name == "TextEncoder" {
                        ty = Type::Named("TextEncoder".to_string());
                    } else if class_name == "TextDecoder" {
                        ty = Type::Named("TextDecoder".to_string());
                    } else if matches!(
                        class_name,
                        "EventTarget" | "Event" | "CustomEvent" | "DOMException"
                    ) {
                        ty = Type::Named(class_name.to_string());
                    } else if matches!(
                        class_name,
                        "Readable" | "Writable" | "Duplex" | "Transform" | "PassThrough"
                    ) {
                        ty = Type::Named(class_name.to_string());
                    } else if class_name == "Uint8Array" || class_name == "Buffer" {
                        ty = Type::Named("Uint8Array".to_string());
                    } else if matches!(
                        class_name,
                        "Int8Array"
                            | "Int16Array"
                            | "Uint16Array"
                            | "Int32Array"
                            | "Uint32Array"
                            | "Float16Array"
                            | "Float32Array"
                            | "Float64Array"
                    ) {
                        ty = Type::Named(class_name.to_string());
                    } else if ctx.classes_index.contains_key(class_name) {
                        // User-defined class: infer type from new ClassName(...)
                        let type_args: Vec<Type> = new_expr
                            .type_args
                            .as_ref()
                            .map(|ta| ta.params.iter().map(|t| extract_ts_type(t)).collect())
                            .unwrap_or_default();
                        if type_args.is_empty() {
                            ty = Type::Named(class_name.to_string());
                        } else {
                            ty = Type::Generic {
                                base: class_name.to_string(),
                                type_args,
                            };
                        }
                    }
                }
            }
        }
    }

    // #1642/#1643: a `const x = <stream>.getReader(...)` / `.getWriter(...)`
    // / `ReadableStream.from(...)` binding is typed Any by inference, but
    // the result is a Web Streams native instance. Type it as the stream
    // class so codegen `receiver_class_name` resolves value-read method
    // binds (`typeof reader.read === "function"`) for the Any-typed
    // local. Safe: the call path (lower/expr_call/static_and_instance.rs)
    // dispatches via the native-instance registry, not this declared type.
    if matches!(ty, Type::Any) {
        if let Some(init_expr) = &decl.init {
            if let ast::Expr::Call(call) = init_expr.as_ref() {
                if let ast::Callee::Expr(callee) = &call.callee {
                    if let ast::Expr::Member(m) = callee.as_ref() {
                        if let ast::MemberProp::Ident(prop) = &m.prop {
                            // Peel `as T` / `!` / `as const` / parens on
                            // the receiver (`(rs as any).getReader(...)`).
                            let mut obj_inner: &ast::Expr = m.obj.as_ref();
                            loop {
                                obj_inner = match obj_inner {
                                    ast::Expr::TsAs(x) => &x.expr,
                                    ast::Expr::TsNonNull(x) => &x.expr,
                                    ast::Expr::TsSatisfies(x) => &x.expr,
                                    ast::Expr::TsTypeAssertion(x) => &x.expr,
                                    ast::Expr::TsConstAssertion(x) => &x.expr,
                                    ast::Expr::Paren(x) => &x.expr,
                                    _ => break,
                                };
                            }
                            if let ast::Expr::Ident(obj_id) = obj_inner {
                                let method = prop.sym.as_ref();
                                let recv_class = ctx
                                    .lookup_native_instance(obj_id.sym.as_ref())
                                    .map(|(_, c)| c.to_string());
                                if method == "getReader"
                                    && recv_class.as_deref() == Some("ReadableStream")
                                {
                                    ty = Type::Named("ReadableStreamDefaultReader".to_string());
                                } else if method == "getWriter"
                                    && recv_class.as_deref() == Some("WritableStream")
                                {
                                    ty = Type::Named("WritableStreamDefaultWriter".to_string());
                                } else if method == "from"
                                    && obj_id.sym.as_ref() == "ReadableStream"
                                {
                                    ty = Type::Named("ReadableStream".to_string());
                                } else if method == "from" && obj_id.sym.as_ref() == "Readable" {
                                    ty = Type::Named("Readable".to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    ty
}
