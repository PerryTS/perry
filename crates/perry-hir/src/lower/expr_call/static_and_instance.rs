//! Static method calls (Class.method) and native instance method calls (emitter.on, ws.send).
//!
//! Extracted from `expr_call/mod.rs` as a mechanical move.

use anyhow::{anyhow, Result};
use perry_types::{LocalId, Type};
use swc_ecma_ast as ast;

use super::stream::is_stream_api_method;
use crate::ir::*;
use crate::lower_patterns::detect_native_instance_expr;
use crate::lower_types::extract_ts_type_with_ctx;

use super::super::{
    extract_typed_parse_source_order, is_generator_call_expr, is_widget_modifier_name, lower_expr,
    resolve_typed_parse_ty, LoweringContext,
};

pub(super) fn try_static_method_and_instance(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    expr: &ast::Expr,
    args: Vec<Expr>,
) -> Result<Result<Expr, Vec<Expr>>> {
    // Check for static method calls (e.g., Counter.increment())
    if let ast::Expr::Member(member) = expr {
        if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
            let obj_name = obj_ident.sym.to_string();
            // Treat uppercase imported identifiers as candidate classes —
            // we don't have cross-module class metadata at HIR-lower
            // time, so without this `import { MongoClient } from
            // 'pkg'; MongoClient.connect(...)` falls through to the
            // dynamic-dispatch path and reads garbage from the static
            // ClosureHeader.  See compile.rs::imported_classes for the
            // backing dispatch table that resolves these calls at
            // codegen time.
            let is_imported_upper = ctx.lookup_imported_func(&obj_name).is_some()
                && obj_name
                    .chars()
                    .next()
                    .map(|c| c.is_uppercase())
                    .unwrap_or(false);
            if ctx.lookup_class(&obj_name).is_some() || is_imported_upper {
                match &member.prop {
                    ast::MemberProp::Ident(method_ident) => {
                        let method_name = method_ident.sym.to_string();
                        if ctx.has_static_method(&obj_name, &method_name) || is_imported_upper {
                            return Ok(Ok(Expr::StaticMethodCall {
                                class_name: obj_name,
                                method_name,
                                args,
                            }));
                        }
                    }
                    // Private static method: WithPrivateStatic.#helper()
                    ast::MemberProp::PrivateName(priv_ident) => {
                        let method_name = format!("#{}", priv_ident.name);
                        if ctx.has_static_method(&obj_name, &method_name) {
                            return Ok(Ok(Expr::StaticMethodCall {
                                class_name: obj_name,
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
                    if is_stream_module && !is_stream_api_method(&module_name, &method_name) {
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
            // Lower the object expression first
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
                );
                // #1048 — fastify Reply chainable methods. `reply.code(201)
                // .type("application/json").send(payload)` ships every method
                // returning the same reply handle for chaining; without this
                // branch only the inner `.code(...)` resolved as a
                // NativeMethodCall and the rest of the chain fell through to
                // the generic Call+PropertyGet path. That path routes through
                // `js_native_call_method` → HANDLE_METHOD_DISPATCH, which has
                // no fastify arm when the well-known flip strips
                // `bundled-fastify` from stdlib — so `.type(...)` returned an
                // untagged NaN that the next chain step read as a number
                // ("(number).send is not a function"). Static NATIVE_MODULE_TABLE
                // dispatch covers it correctly.
                let is_fastify_reply =
                    module.as_str() == "fastify" && class_name.as_deref() == Some("Reply");
                let is_fastify_reply_chain_method = matches!(
                    method_name.as_str(),
                    "code" | "status" | "header" | "type" | "send"
                );
                if (is_math_lib && is_math_method)
                    || (is_commander && is_commander_method)
                    || (is_fastify_reply && is_fastify_reply_chain_method)
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
        }
    }

    Ok(Err(args))
}
